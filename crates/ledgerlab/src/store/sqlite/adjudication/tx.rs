//! Live R3 transaction binding. The OS owner and shared physical lane remain in
//! SqliteTx until acknowledged rollback or the registered commit drain finishes.
use super::*;
use crate::{
    service::accept::adjudication::{
        CommitCapability, OriginalBaseWrites, PhysicalEnvelope, ValidatedAdjudicationPlan,
    },
    store::{adjudication::*, outcomes::OutcomeTx},
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::time::{timeout_at, Instant};

pub(in crate::store::sqlite) struct Context {
    pub work: WorkRequest,
    pub prior: TrustedJournalHead,
    pub transaction: Digest,
    pub incarnation: Digest,
    pub epoch: Count,
    pub physical: PhysicalEnvelope,
    pub resource_ceiling: wire::Resource,
    pub writer_fence: Option<Digest>,
    pub guards: Vec<Guard>,
    pub appended: bool,
}

/// Constructed only by explicit host provisioning. The gate's finite database,
/// WAL and staging limits are verified against this live writer connection.
pub(crate) struct SqliteAdjudicationStore {
    pub(super) store: super::super::SqliteStore,
    pub(super) journal: JournalIdentity,
    pub(super) logical: wire::Resource,
    pub(super) pages: u32,
    pub(super) workspace: Count,
    pub(super) incarnation: Digest,
}
impl AdjudicationStore for SqliteAdjudicationStore {
    type Tx = super::super::SqliteTx;
    async fn begin_adjudication(
        &self,
        work: &WorkRequest,
        deadline: Instant,
    ) -> Result<Self::Tx, StoreError> {
        provision::verify_incarnation(&self.store, &self.journal, &self.incarnation)?;
        if work.journal != self.journal
            || !work.maximum.fits(&self.logical)
            || work.maximum.workspace_bytes > self.workspace
        {
            return Err(StoreError::Overloaded);
        }
        let mut tx = self.store.begin_lane(deadline, work.mandatory).await?;
        tx.failed = true;
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(tx.conn())
            .await?;
        let maximum: i64 = sqlx::query_scalar("PRAGMA max_page_count")
            .fetch_one(tx.conn())
            .await?;
        if page_size != 4096 || maximum != i64::from(self.pages) || tx.physical_lane.is_none() {
            return Err(invalid());
        }
        let prior = head(tx.conn(), &self.journal).await?;
        let resource_key = HeadKey {
            journal: self.journal.clone(),
            kind: HeadKind::Resource,
            full_key: index_key(*b"RESOURCE", &[self.journal.host.as_str().as_bytes()])
                .map_err(core)?,
        };
        let resource = point(tx.conn(), &resource_key).await?;
        let state: r3::runtime::points::State =
            serde_json::from_slice(resource.value.as_deref().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        let r3::runtime::points::State::Resource(resource) = state else {
            return Err(invalid());
        };
        resource.validate().map_err(core)?;
        if !resource.provisioned.fits(&self.logical) {
            return Err(invalid());
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let serial = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| invalid())?;
        let transaction = r3::raw_sha256(
            &r3::canonical_bytes(
                &json!([
                    "sqlite-r3-tx/1",
                    self.incarnation,
                    std::process::id(),
                    serial
                ]),
                4096,
            )
            .map_err(core)?,
        );
        // Pinned SQLite FULL-sync WAL frame bound after the exclusive successful
        // TRUNCATE barrier: finite pages + last-page duplicate + sector padding.
        let wal = 32 + (u128::from(self.pages) + 2 + 65536u128.div_ceil(4120)) * 4120;
        let retained = Count::new(u128::from(self.pages) * 4096).map_err(core)?;
        let physical = PhysicalEnvelope::from_backend(
            retained,
            Count::new(self.pages.into()).map_err(core)?,
            Count::new(wal).map_err(core)?,
            Count::new(self.workspace.value() + 16384).map_err(core)?,
            Count::ZERO,
            self.workspace,
            transaction.clone(),
        )
        .map_err(core)?;
        let writer_fence = writer_fence(
            &self.store,
            &prior,
            resource.q.writer_epoch,
            &self.incarnation,
        )?;
        tx.adjudication = Some(Context {
            work: work.clone(),
            prior,
            transaction,
            incarnation: self.incarnation.clone(),
            epoch: resource.q.writer_epoch,
            physical,
            resource_ceiling: self.logical.clone(),
            writer_fence,
            guards: vec![],
            appended: false,
        });
        tx.failed = false;
        Ok(tx)
    }
}

impl AdjudicationTx for super::super::SqliteTx {
    async fn lock_adjudication(&mut self, guards: &[Guard]) -> Result<(), StoreError> {
        if self.failed {
            return Err(invalid());
        }
        self.failed = true;
        validate_guards(guards)?;
        let ctx = self.adjudication.as_ref().ok_or_else(invalid)?;
        if (!ctx.guards.is_empty() && ctx.guards != guards)
            || guards
                .iter()
                .any(|g| matches!(g,Guard::R3{host,..} if *host!=ctx.work.journal.host))
        {
            return Err(invalid());
        }
        let legacy: Vec<_> = guards
            .iter()
            .filter_map(|g| match g {
                Guard::Legacy(l) => Some(l.clone()),
                _ => None,
            })
            .collect();
        self.failed = false;
        self.lock_scopes(&legacy).await?;
        self.adjudication.as_mut().ok_or_else(invalid)?.guards = guards.to_vec();
        Ok(())
    }
    async fn lookup_adjudication(
        &mut self,
        j: &JournalIdentity,
        key: &wire::Delivery,
    ) -> Result<Option<SavedOutcome>, StoreError> {
        if self.failed
            || self
                .adjudication
                .as_ref()
                .is_none_or(|c| c.work.journal != *j || c.guards.is_empty())
        {
            return Err(invalid());
        }
        self.failed = true;
        let result = timeout_at(
            self.deadline.min(Instant::now() + Duration::from_secs(2)),
            saved(self.conn(), j, key),
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match &result {
            Ok(_) => self.failed = false,
            Err(e) if e.disables_writes() => self.store.disabled.store(true, Ordering::Release),
            _ => {}
        }
        result
    }
    async fn resolve_adjudication(&mut self, q: &ResolveRequest) -> Result<Resolution, StoreError> {
        if self.failed {
            return Err(invalid());
        }
        self.failed = true;
        let ctx = self.adjudication.as_ref().ok_or_else(invalid)?;
        if ctx.work.journal != q.journal {
            return Err(invalid());
        }
        if ctx.guards != q.guards {
            self.failed = false;
            return Ok(Resolution::MoreLocks(q.guards.clone()));
        }
        let result = timeout_at(
            self.deadline.min(Instant::now() + Duration::from_secs(2)),
            resolve::locked(self.conn(), q),
        )
        .await
        .map_err(|_| StoreError::Deadline)?;
        match result {
            Ok(inputs) => {
                self.failed = false;
                Ok(Resolution::Complete(Box::new(inputs)))
            }
            Err(e) => {
                if e.disables_writes() {
                    self.store.disabled.store(true, Ordering::Release);
                }
                Err(e)
            }
        }
    }
    async fn commit_capability(&mut self) -> Result<CommitCapability, StoreError> {
        if self.failed || self.physical_lane.is_none() {
            return Err(invalid());
        }
        let c = self.adjudication.as_ref().ok_or_else(invalid)?;
        if c.guards.is_empty() || c.appended {
            return Err(invalid());
        }
        CommitCapability::from_backend(
            c.work.journal.clone(),
            c.transaction.clone(),
            c.incarnation.clone(),
            c.epoch,
            c.prior.clone(),
            c.work.owner.clone(),
            c.physical.clone(),
            c.resource_ceiling.clone(),
            c.writer_fence.clone(),
        )
        .map_err(core)
    }
    async fn append_adjudication(
        &mut self,
        p: &ValidatedAdjudicationPlan,
        cap: &CommitCapability,
    ) -> Result<(), StoreError> {
        if self.failed || self.physical_lane.is_none() {
            return Err(invalid());
        }
        self.failed = true;
        let c = self.adjudication.as_ref().ok_or_else(invalid)?;
        if c.appended
            || c.work.journal != *p.journal()
            || c.work.journal != *cap.journal()
            || c.transaction != *cap.transaction()
            || c.incarnation != *cap.storage_incarnation()
            || c.epoch != cap.epoch()
            || c.work.owner != *cap.allocation_owner()
            || c.resource_ceiling != *cap.resource_ceiling()
            || c.writer_fence.as_ref() != cap.writer_fence()
            || c.guards != p.guards()
            || c.prior.ordinal() != p.prior().ordinal()
            || c.prior.root() != p.prior().root()
        {
            return Err(invalid());
        }
        physical::check_plan(p)?;
        timeout_at(self.deadline, persist::reassert(self.conn(), p.observed()))
            .await
            .map_err(|_| StoreError::Deadline)??;
        if let Some(base) = p.base() {
            let before = physical::physical_usage(self.conn()).await?;
            timeout_at(
                self.deadline,
                persist::reassert(self.conn(), base.absence()),
            )
            .await
            .map_err(|_| StoreError::Deadline)??;
            self.failed = false;
            match base.writes() {
                OriginalBaseWrites::V2(plan) => self.append_outcome(plan).await?,
                OriginalBaseWrites::OriginalV2(plan) => {
                    let journal = journal_key(p.journal())?;
                    self.append_original_base(plan, &journal).await?;
                }
                OriginalBaseWrites::V1 { writes, .. } => {
                    use crate::store::ports::AcceptanceTx;
                    for op in writes {
                        self.write(op).await?;
                    }
                }
            }
            self.failed = true;
            #[cfg(test)]
            if self
                .store
                .fail_after_original_base
                .swap(false, Ordering::AcqRel)
            {
                return Err(StoreError::Deadline);
            }
            timeout_at(self.deadline, physical::charge_legacy(self.conn(), before))
                .await
                .map_err(|_| StoreError::Deadline)??;
        }
        let result = timeout_at(self.deadline, persist::plan(self.conn(), p))
            .await
            .map_err(|_| StoreError::Deadline)?;
        match result {
            Ok(()) => {
                self.adjudication.as_mut().ok_or_else(invalid)?.appended = true;
                self.failed = false;
                Ok(())
            }
            Err(e) => {
                if e.disables_writes() {
                    self.store.disabled.store(true, Ordering::Release);
                }
                Err(e)
            }
        }
    }
}

fn writer_fence(
    store: &super::super::SqliteStore,
    prior: &TrustedJournalHead,
    epoch: Count,
    incarnation: &Digest,
) -> Result<Option<Digest>, StoreError> {
    let Some(fence) = &store.inner._owner.fence else {
        return Ok(None);
    };
    let j = prior.journal();
    let binding = r3::canonical_bytes(
        &json!([
            j.store,
            j.scope,
            j.registration,
            j.host,
            prior.ordinal(),
            prior.segment(),
            prior.root(),
            epoch,
            incarnation
        ]),
        8192,
    )
    .map_err(core)?;
    fence.observation(&binding).map(Some)
}
impl SqliteAdjudicationStore {
    /// Host-only payload discovery; no epoch increment, append or publication.
    pub(crate) async fn writer_fence(&self, deadline: Instant) -> Result<Digest, StoreError> {
        use crate::store::ports::AcceptanceTx;
        provision::verify_incarnation(&self.store, &self.journal, &self.incarnation)?;
        let mut tx = self.store.begin_lane(deadline, false).await?;
        let prior = head(tx.conn(), &self.journal).await?;
        let key = HeadKey {
            journal: self.journal.clone(),
            kind: HeadKind::Resource,
            full_key: index_key(*b"RESOURCE", &[self.journal.host.as_str().as_bytes()])
                .map_err(core)?,
        };
        let observed = point(tx.conn(), &key).await?;
        let state: r3::runtime::points::State =
            serde_json::from_slice(observed.value.as_deref().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        let r3::runtime::points::State::Resource(resource) = state else {
            return Err(invalid());
        };
        resource.validate().map_err(core)?;
        let result = writer_fence(
            &self.store,
            &prior,
            resource.q.writer_epoch,
            &self.incarnation,
        )?
        .ok_or_else(invalid)?;
        tx.rollback().await?;
        Ok(result)
    }
}
