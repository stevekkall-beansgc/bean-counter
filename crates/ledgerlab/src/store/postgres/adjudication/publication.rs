//! Private SQL/external publication coordination. No physical admission or
//! CommitCapability: root owns bootstrap lineage and transaction supervision.
pub(in crate::store::postgres) use super::fence::{DatabaseWitness, RecoveryDisposition};
use super::{fence::Fence, recovery::Gate};
use crate::store::errors::StoreError;
use ledgerlab_core::adjudication::types::Digest;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    sync::{OwnedRwLockWriteGuard, RwLock},
    time::{timeout_at, Instant},
};
use tokio_postgres::{GenericClient, IsolationLevel};

#[derive(Debug)]
enum Published {
    Unavailable,
    Stable(DatabaseWitness),
}
pub(in crate::store::postgres) struct PublicationOwner {
    fence: Fence,
    visibility: Arc<RwLock<Published>>,
}
/// Keeps the actual OS owner alive for the lifetime of the SQL read snapshot.
/// It labels that snapshot; it does not promise the head remains current.
pub(in crate::store::postgres) struct SnapshotPin {
    owner: Arc<PublicationOwner>,
    observed: DatabaseWitness,
}
pub(in crate::store::postgres) struct WritePublication {
    owner: Arc<PublicationOwner>,
    gate_identity: Arc<()>,
    original: DatabaseWitness,
    pending: Option<DatabaseWitness>,
    visibility: Option<OwnedRwLockWriteGuard<Published>>,
    dirty: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::store::postgres) enum SqlCommitOutcome {
    Committed,
    RolledBack,
    Unknown,
}
#[derive(Debug)]
pub(in crate::store::postgres) enum PublicationOutcome {
    StableOld,
    StableNew,
    Unresolved(StoreError),
}
fn invalid() -> StoreError {
    StoreError::Integrity("PostgreSQL SQL/external publication mismatch")
}
fn bound(w: &DatabaseWitness) -> bool {
    w.anchor().as_str() != "0".repeat(64) && w.witness().as_str() != "0".repeat(64)
}
/// Reads only the actual singleton. UNBOUND is returned for the root's explicit
/// legacy/bootstrap checks; it never grants publication or storage authority.
pub(in crate::store::postgres) async fn read_witness<C: GenericClient + Sync>(
    c: &C,
) -> Result<DatabaseWitness, StoreError> {
    let row = c
        .query_one(
            "SELECT anchor,witness FROM ledgerlab.r3_commit_witness WHERE singleton=1",
            &[],
        )
        .await?;
    DatabaseWitness::from_database(row.try_get(0)?, row.try_get(1)?)
}
async fn locked_witness<C: GenericClient + Sync>(c: &C) -> Result<DatabaseWitness, StoreError> {
    let row = c
        .query_one(
            "SELECT anchor,witness FROM ledgerlab.r3_commit_witness WHERE singleton=1 FOR UPDATE",
            &[],
        )
        .await?;
    DatabaseWitness::from_database(row.try_get(0)?, row.try_get(1)?)
}
async fn exited(gate: &Gate) -> Result<(), StoreError> {
    loop {
        match gate.require_prior_exit().await {
            Err(StoreError::WritesDisabled) => tokio::time::sleep(Duration::from_millis(5)).await,
            other => return other,
        }
    }
}
impl PublicationOwner {
    pub(in crate::store::postgres) fn acquire(path: &Path) -> Result<Arc<Self>, StoreError> {
        Ok(Arc::new(Self {
            fence: Fence::acquire(path)?,
            visibility: Arc::new(RwLock::new(Published::Unavailable)),
        }))
    }
    pub(in crate::store::postgres) fn identity(&self) -> &Digest {
        self.fence.identity()
    }
    pub(in crate::store::postgres) fn needs_initialization(&self) -> Result<bool, StoreError> {
        self.fence.needs_initialization()
    }
    /// Trusted owner bootstrap only, AFTER exact SQL binding is durable and
    /// lineage/exclusion checks succeeded. Ordinary open must use recovery.
    pub(in crate::store::postgres) async fn initialize(
        &self,
        observed: &DatabaseWitness,
    ) -> Result<(), StoreError> {
        let mut visibility = self.visibility.write().await;
        *visibility = Published::Unavailable;
        self.fence.initialize(observed)?;
        *visibility = Published::Stable(observed.clone());
        Ok(())
    }
    pub(in crate::store::postgres) async fn available(&self) -> bool {
        matches!(*self.visibility.read().await, Published::Stable(_))
    }
    /// Caller has a raw gate. This does not resolve/clear native outcomes: root
    /// must call Gate::resolve_after_publication only after this succeeds.
    pub(in crate::store::postgres) async fn recover_under_gate(
        self: &Arc<Self>,
        gate: &mut Gate,
        deadline: Instant,
    ) -> Result<RecoveryDisposition, StoreError> {
        timeout_at(deadline, async {
            let mut visibility = self.visibility.write().await;
            *visibility = Published::Unavailable;
            exited(gate).await?;
            let tx = gate
                .publication_client()
                .build_transaction()
                .isolation_level(IsolationLevel::ReadCommitted)
                .start()
                .await?;
            // Legacy orphan barrier: every anchored mutation holds this same
            // row lock until its actual server transaction exits.
            let observed = locked_witness(&tx).await?;
            let disposition = self.fence.recover(&observed)?;
            tx.rollback().await?;
            *visibility = Published::Stable(observed);
            Ok(disposition)
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
    /// The shared guard is released after the snapshot's witness is checked.
    /// Existing pins therefore never block later publication for their lifetime.
    pub(in crate::store::postgres) async fn pin_snapshot<C: GenericClient + Sync>(
        self: &Arc<Self>,
        c: &C,
        deadline: Instant,
    ) -> Result<SnapshotPin, StoreError> {
        timeout_at(deadline, async {
            let visibility = self.visibility.read().await;
            let Published::Stable(expected) = &*visibility else {
                return Err(StoreError::WritesDisabled);
            };
            let row = c.query_one("SELECT anchor,witness,current_setting('transaction_isolation') FROM ledgerlab.r3_commit_witness WHERE singleton=1", &[]).await?;
            let isolation: &str = row.try_get(2)?;
            if !matches!(isolation, "serializable" | "repeatable read") {
                return Err(invalid());
            }
            let observed = DatabaseWitness::from_database(row.try_get(0)?, row.try_get(1)?)?;
            if !bound(&observed) || &observed != expected {
                return Err(StoreError::ExpectedCurrent);
            }
            Ok(SnapshotPin { owner: self.clone(), observed })
        }).await.map_err(|_| StoreError::Deadline)?
    }
    /// Must precede every possible retained mutation, including new scope-lock
    /// rows. The caller's SAME SQL transaction retains the row lock to exit.
    pub(in crate::store::postgres) async fn begin_write<C: GenericClient + Sync>(
        self: &Arc<Self>,
        gate: &mut Gate,
        c: &C,
        pin: &SnapshotPin,
        deadline: Instant,
    ) -> Result<WritePublication, StoreError> {
        if !Arc::ptr_eq(self, &pin.owner) {
            return Err(invalid());
        }
        timeout_at(deadline, async {
            let observed = locked_witness(c).await?;
            let visibility = self.visibility.read().await;
            if !bound(&observed)
                || observed != pin.observed
                || !matches!(&*visibility, Published::Stable(w) if w == &observed)
            {
                return Err(StoreError::ExpectedCurrent);
            }
            Ok(WritePublication {
                owner: self.clone(),
                gate_identity: gate.publication_identity(),
                original: observed,
                pending: None,
                visibility: None,
                dirty: false,
            })
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
}
impl WritePublication {
    pub(in crate::store::postgres) fn mark_mutation(&mut self) {
        self.dirty = true;
    }
    pub(in crate::store::postgres) async fn prepare_commit<C: GenericClient + Sync>(
        &mut self,
        c: &C,
        binding: &[u8],
        deadline: Instant,
    ) -> Result<(), StoreError> {
        if self.pending.is_some() || self.visibility.is_some() {
            return Err(invalid());
        }
        if !self.dirty {
            return Ok(());
        }
        timeout_at(deadline, async {
            let mut visibility = self.owner.visibility.clone().write_owned().await;
            if !matches!(&*visibility, Published::Stable(w) if w == &self.original) {
                return Err(StoreError::ExpectedCurrent);
            }
            *visibility = Published::Unavailable;
            // Install guard before any fallible SQL/FS step. Cancellation/drop
            // leaves Unavailable, never an invented successful publication.
            self.visibility = Some(visibility);
            if locked_witness(c).await? != self.original { return Err(StoreError::ExpectedCurrent); }
            let proposed = self.owner.fence.prepare(&self.original, binding)?;
            self.pending = Some(proposed.clone());
            let changed = c.execute("UPDATE ledgerlab.r3_commit_witness SET witness=$1 WHERE singleton=1 AND anchor=$2 AND witness=$3", &[&proposed.witness().as_str(), &self.original.anchor().as_str(), &self.original.witness().as_str()]).await?;
            if changed != 1 { return Err(StoreError::ExpectedCurrent); }
            Ok(())
        }).await.map_err(|_| StoreError::Deadline)?
    }
    /// Root MUST discard/join the work session first. Native PID/start exit is
    /// checked again here; the singleton lock proves legacy server completion.
    /// Keep Gate until this returns, then resolve exact saved/nonmembership.
    pub(in crate::store::postgres) async fn settle_after_exit(
        mut self,
        gate: &mut Gate,
        sql_result: SqlCommitOutcome,
        deadline: Instant,
    ) -> PublicationOutcome {
        let result = timeout_at(deadline, async {
            if !Arc::ptr_eq(&self.gate_identity, &gate.publication_identity()) {
                return Err(invalid());
            }
            if self.visibility.is_none() {
                self.visibility = Some(self.owner.visibility.clone().write_owned().await);
            }
            let visibility = self.visibility.as_mut().ok_or_else(invalid)?;
            **visibility = Published::Unavailable;
            exited(gate).await?;
            let tx = gate
                .publication_client()
                .build_transaction()
                .isolation_level(IsolationLevel::ReadCommitted)
                .start()
                .await?;
            let observed = locked_witness(&tx).await?;
            let outcome = classify(
                &self.original,
                self.pending.as_ref(),
                self.dirty,
                sql_result,
                &observed,
            )?;
            // Resolve PENDING only from exact actual old/new SQL observation.
            self.owner.fence.recover(&observed)?;
            tx.rollback().await?;
            **visibility = Published::Stable(observed);
            Ok(outcome)
        })
        .await;
        match result {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(error)) => PublicationOutcome::Unresolved(error),
            Err(_) => PublicationOutcome::Unresolved(StoreError::Deadline),
        }
    }
}
fn classify(
    original: &DatabaseWitness,
    pending: Option<&DatabaseWitness>,
    dirty: bool,
    sql: SqlCommitOutcome,
    observed: &DatabaseWitness,
) -> Result<PublicationOutcome, StoreError> {
    if observed == original {
        // Without a prepared witness, only an explicit no-COMMIT/rollback
        // outcome can prove a dirty transaction did not publish untracked rows.
        if dirty && pending.is_none() && sql != SqlCommitOutcome::RolledBack {
            return Err(invalid());
        }
        if dirty && sql == SqlCommitOutcome::Committed {
            return Err(invalid());
        }
        return Ok(PublicationOutcome::StableOld);
    }
    if pending == Some(observed) && dirty && sql != SqlCommitOutcome::RolledBack {
        return Ok(PublicationOutcome::StableNew);
    }
    Err(invalid())
}
#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
