//! Durable work identity and independent session gate. This establishes cleanup
//! and outcome-resolution ordering, not physical backing or a commit capability.
use super::super::{connect::Session, PostgresConfig};
use super::*;
use std::{
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::{timeout_at, Instant};
const GATE: i64 = 0x4c4c5233434c454e;
#[derive(Clone)]
pub(in crate::store::postgres) struct Work {
    pub journal: JournalIdentity,
    pub key: wire::Delivery,
    pub command_hash: Digest,
}
impl Work {
    pub fn new(journal: JournalIdentity, command: &r3::ParsedCommand) -> Result<Self, StoreError> {
        let v = runtime::command_value(command.command()).map_err(core)?;
        let key: wire::Delivery =
            serde_json::from_value(v["key"].clone()).map_err(|_| invalid())?;
        if key.0 != journal.scope {
            return Err(invalid());
        }
        Ok(Self {
            journal,
            key,
            command_hash: r3::raw_sha256(command.bytes()),
        })
    }
    pub fn matches(&self, p: &ValidatedAdjudicationPlan) -> bool {
        self.journal == *p.journal() && self.command_hash == r3::raw_sha256(p.command().bytes())
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(in crate::store::postgres) enum Disposition {
    Saved,
    Absent,
    Conflict,
}
pub(in crate::store::postgres) struct Resolution {
    pub generation: Count,
    pub disposition: Disposition,
    pub prefix: TrustedJournalHead,
}
pub(in crate::store::postgres) struct Gate {
    session: Session,
    publication_identity: Arc<()>,
    pub work: Option<Work>,
    generation: Option<Count>,
    pub prior_resolution: Option<Resolution>,
}
fn decode_journal(bytes: &[u8]) -> Result<JournalIdentity, StoreError> {
    if bytes.len() > r3::MAX_KEY_BYTES || bytes.get(..9) != Some(b"r3host01\x05") {
        return Err(invalid());
    }
    let mut rest = &bytes[9..];
    let mut parts = Vec::with_capacity(5);
    for _ in 0..5 {
        if rest.len() < 2 {
            return Err(invalid());
        }
        let n = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        rest = &rest[2..];
        if rest.len() < n {
            return Err(invalid());
        }
        parts.push(
            r3::types::Id::parse(std::str::from_utf8(&rest[..n]).map_err(|_| invalid())?)
                .map_err(core)?,
        );
        rest = &rest[n..];
    }
    if !rest.is_empty() {
        return Err(invalid());
    }
    let j = JournalIdentity {
        store: parts[0].clone(),
        scope: wire::Scope(parts[1].clone(), parts[2].clone()),
        registration: parts[3].clone(),
        host: parts[4].clone(),
    };
    if journal_key(&j)? != bytes {
        return Err(invalid());
    }
    Ok(j)
}
impl Gate {
    pub async fn acquire(
        config: &PostgresConfig,
        deadline: Instant,
        pid: &Arc<AtomicI32>,
    ) -> Result<Self, StoreError> {
        let mut gate = Self::acquire_raw(config, deadline, pid).await?;
        timeout_at(deadline, async {
            // A handle opened while UNBOUND may survive owner bootstrap. Do
            // not resolve/clear a later bound PENDING result through that stale
            // handle. The independent gate also excludes bootstrap while this
            // fresh control snapshot proves the singleton is still UNBOUND.
            let tx = gate.session.client.build_transaction()
                .isolation_level(tokio_postgres::IsolationLevel::ReadCommitted)
                .start().await?;
            let row = tx.query_one(
                "SELECT anchor,witness FROM ledgerlab.r3_commit_witness WHERE singleton=1 FOR UPDATE", &[]
            ).await?;
            let zero = "0".repeat(64);
            if row.try_get::<_, &str>(0)? != zero || row.try_get::<_, &str>(1)? != zero {
                return Err(StoreError::InvalidStore(
                    "bound PostgreSQL work requires publication recovery",
                ));
            }
            tx.rollback().await?;
            gate.resolve_after_publication().await
        })
        .await
        .map_err(|_| StoreError::Deadline)??;
        Ok(gate)
    }
    /// Acquire exclusion only. Anchored callers MUST recover external
    /// publication before resolve_after_publication, arm, or application reads.
    pub async fn acquire_raw(
        config: &PostgresConfig,
        deadline: Instant,
        pid: &Arc<AtomicI32>,
    ) -> Result<Self, StoreError> {
        timeout_at(deadline, async {
            let session = config.connect().await?;
            super::super::verify_server(&session.client).await?;
            let n: i32 = session
                .client
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .try_get(0)?;
            pid.store(n, Ordering::Release);
            // Session lock is independent of the work transaction/backend.
            session
                .client
                .query_one("SELECT pg_advisory_lock($1)", &[&GATE])
                .await?;
            Ok(Self {
                session,
                publication_identity: Arc::new(()),
                work: None,
                generation: None,
                prior_resolution: None,
            })
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
    pub(super) fn publication_identity(&self) -> Arc<()> {
        self.publication_identity.clone()
    }
    /// Control session is still protected by the independent advisory gate.
    /// Only publication's short witness-lock transaction may borrow it here.
    pub(super) fn publication_client(&mut self) -> &mut tokio_postgres::Client {
        &mut self.session.client
    }
    /// Must follow STABLE publication recovery on the anchored route. No new
    /// work may be armed until this exact saved/nonmembership resolution ends.
    pub async fn resolve_after_publication(&mut self) -> Result<(), StoreError> {
        self.prior_resolution = self.resolve().await?;
        Ok(())
    }
    /// Read-only exit proof. A missing/unknown backend start cannot establish
    /// absence. Does not inspect saved outcomes or consume the staging slot.
    pub(super) async fn require_prior_exit(&self) -> Result<(), StoreError> {
        let row = self.session.client.query_one(
            "SELECT state,backend_pid,backend_start::text FROM ledgerlab.r3_unresolved_work WHERE singleton=1", &[]
        ).await?;
        match row.try_get::<_, &str>(0)? {
            "IDLE" => Ok(()),
            "RESOLVING" => {
                let pid: i32 = row.try_get(1)?;
                let start: String = row.try_get(2)?;
                self.require_exit(pid, &start).await
            }
            _ => Err(invalid()),
        }
    }
    async fn require_exit(&self, pid: i32, start: &str) -> Result<(), StoreError> {
        let live: bool = self.session.client.query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND (backend_start IS NULL OR backend_start=$2::text::timestamptz)) OR (NOT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND backend_start<>$2::text::timestamptz) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1))", &[&pid, &start]
        ).await?.try_get(0)?;
        if live {
            Err(StoreError::WritesDisabled)
        } else {
            Ok(())
        }
    }
    pub async fn arm<C: GenericClient + Sync>(
        &mut self,
        c: &C,
        work: Work,
    ) -> Result<(), StoreError> {
        if self.work.is_some() {
            return Err(invalid());
        }
        // Immutable occupied identity is a read-only retry/conflict route. It
        // cannot create new work, so do not rewrite the recovery slot.
        if read::saved(&self.session.client, &work.journal, &work.key)
            .await?
            .is_some()
        {
            return Ok(());
        }
        let row=c.query_one("SELECT pid,backend_start::text FROM pg_stat_activity WHERE pid=pg_backend_pid() AND datname=current_database() AND usename=current_user",&[]).await?;
        let pid: i32 = row.try_get(0)?;
        let start: String = row.try_get(1)?;
        let row = self
            .session
            .client
            .query_one(
                "SELECT generation,state FROM ledgerlab.r3_unresolved_work WHERE singleton=1",
                &[],
            )
            .await?;
        if row.try_get::<_, &str>(1)? != "IDLE" {
            return Err(StoreError::WritesDisabled);
        }
        let old = ordinal(row.try_get(0)?)?;
        // Fixed staging slot: attempts do not advance a protocol counter.
        // The live session gate plus exact PID/start/command identity excludes
        // old controllers; a disconnected client cannot resume SQL on a new lease.
        let next = old;
        let changed=self.session.client.execute("UPDATE ledgerlab.r3_unresolved_work SET generation=$1,state='RESOLVING',backend_pid=$2,backend_start=$3::text::timestamptz,journal=$4,delivery=$5,command_hash=$6 WHERE singleton=1 AND generation=$7 AND state='IDLE'",&[&number(next),&pid,&start,&journal_key(&work.journal)?,&r3::canonical_bytes(&work.key,4096).map_err(core)?,&work.command_hash.as_str(),&number(old)]).await?;
        if changed != 1 {
            return Err(StoreError::ExpectedCurrent);
        }
        self.work = Some(work);
        self.generation = Some(next);
        Ok(())
    }
    /// A live prior backend remains a blocker even after its controller dies.
    /// No absence observation while it could still commit may clear the slot.
    async fn resolve(&mut self) -> Result<Option<Resolution>, StoreError> {
        let row=self.session.client.query_one("SELECT generation,state,backend_pid,backend_start::text,journal,delivery,command_hash FROM ledgerlab.r3_unresolved_work WHERE singleton=1",&[]).await?;
        let generation = ordinal(row.try_get(0)?)?;
        let state: &str = row.try_get(1)?;
        if state == "IDLE" {
            return Ok(None);
        }
        if state != "RESOLVING" {
            return Err(invalid());
        }
        let pid: i32 = row.try_get(2)?;
        let start: String = row.try_get(3)?;
        self.require_exit(pid, &start).await?;
        let jb: Vec<u8> = row.try_get(4)?;
        let j = decode_journal(&jb)?;
        let key: wire::Delivery =
            r3::parse_exact(&row.try_get::<_, Vec<u8>>(5)?, 4096).map_err(core)?;
        let hash = Digest::parse(row.try_get::<_, &str>(6)?).map_err(core)?;
        let saved = read::saved(&self.session.client, &j, &key).await?;
        let disposition = match saved {
            None => Disposition::Absent,
            Some(s) if r3::raw_sha256(&s.command) == hash => Disposition::Saved,
            Some(_) => Disposition::Conflict,
        };
        let prefix = read::head(&self.session.client, &j).await?;
        let changed=self.session.client.execute("UPDATE ledgerlab.r3_unresolved_work SET state='IDLE' WHERE singleton=1 AND generation=$1 AND state='RESOLVING'",&[&number(generation)]).await?;
        if changed != 1 {
            return Err(StoreError::ExpectedCurrent);
        }
        Ok(Some(Resolution {
            generation,
            disposition,
            prefix,
        }))
    }
    /// Called only after the work connection has been discarded and its driver
    /// joined. The gate remains held while server exit is separately observed.
    pub async fn finish(mut self) -> Result<Option<Resolution>, StoreError> {
        let result = timeout_at(Instant::now() + Duration::from_secs(2), async {
            loop {
                match self.resolve().await {
                    Err(StoreError::WritesDisabled) => {
                        tokio::time::sleep(Duration::from_millis(5)).await
                    }
                    other => return other,
                }
            }
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if let (Some(work), Some(expected)) = (&self.work, self.generation) {
            let r = result
                .as_ref()
                .map_err(|_| invalid())?
                .as_ref()
                .ok_or_else(invalid)?;
            if r.generation != expected || r.prefix.journal() != &work.journal {
                return Err(invalid());
            }
        }
        self.session.discard().await;
        result
    }
    #[cfg(test)]
    pub async fn abandon(self) {
        self.session.discard().await;
    }
}
