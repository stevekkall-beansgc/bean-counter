//! Primary, SELECT-only paged snapshots. The worker owns the actual transaction,
//! including while its client is idle, and releases it at the bounded deadline.
use super::*;
use crate::{
    service::accept::adjudication::{PrefixSelection, TrustedPrefix},
    store::adjudication::*,
};
use r3::{reads::PageFragment, runtime::points::State, Validate};
use sqlx::{pool::PoolConnection, Sqlite};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, OwnedRwLockWriteGuard, OwnedSemaphorePermit},
    time::{timeout_at, Instant},
};
type Result<T> = std::result::Result<T, StoreError>;
const BYTES: u128 = physical::READER_WORKSPACE_BYTES;
const PAGES: u128 = 4096;
enum Request {
    Segment(IndexedPageRequest, oneshot::Sender<Result<PageFragment>>),
    Object(ObjectPageRequest, oneshot::Sender<Result<Vec<u8>>>),
    Certificate(
        wire::Family,
        wire::ExpectedPrefix,
        oneshot::Sender<Result<Option<wire::Certificate>>>,
    ),
    Finish(oneshot::Sender<Result<()>>),
}
pub(crate) struct SqliteAdjudicationRead {
    prefix: TrustedPrefix,
    lease: ReadLease,
    send: mpsc::Sender<Request>,
    deadline: Instant,
}
struct Session {
    connection: Option<PoolConnection<Sqlite>>,
    _slot: OwnedSemaphorePermit,
    store: super::super::SqliteStore,
    journal: JournalIdentity,
    at: Count,
    budget: wire::ReadBudget,
    seen: BTreeSet<String>,
    deadline: Instant,
}
impl Session {
    fn tx(&mut self) -> &mut SqliteConnection {
        self.connection.as_mut().expect("live snapshot")
    }
    fn charge(&mut self, bytes: u128, pages: u128) -> Result<()> {
        self.budget.bytes = self
            .budget
            .bytes
            .checked_sub(Count::new(bytes).map_err(core)?)
            .map_err(|_| StoreError::Overloaded)?;
        self.budget.pages = self
            .budget
            .pages
            .checked_sub(Count::new(pages).map_err(core)?)
            .map_err(|_| StoreError::Overloaded)?;
        Ok(())
    }
    async fn state(&mut self, kind: HeadKind, key: &[u8], at: Count) -> Result<Option<State>> {
        let journal = journal_key(&self.journal)?;
        let meta:Option<(Vec<u8>,i64)>=sqlx::query_as("SELECT ordinal,length(value) FROM r3_head_versions WHERE journal=? AND kind=? AND full_key=? AND ordinal<=? ORDER BY ordinal DESC LIMIT 1")
            .bind(&journal).bind(head_tag(kind)).bind(key).bind(at.value().to_be_bytes().as_slice()).fetch_optional(self.tx()).await?;
        let Some((ordinal, length)) = meta else {
            return Ok(None);
        };
        if !(2..=r3::COMMAND_BYTES as i64).contains(&length) {
            return Err(invalid());
        }
        self.charge(length as u128, (length as u128).div_ceil(4096))?;
        let value:Vec<u8>=sqlx::query_scalar("SELECT value FROM r3_head_versions WHERE journal=? AND kind=? AND full_key=? AND ordinal=?")
            .bind(journal).bind(head_tag(kind)).bind(key).bind(ordinal).fetch_one(self.tx()).await?;
        let parsed =
            ledgerlab_core::canonical::parse_bounded(&value, r3::COMMAND_BYTES).map_err(core)?;
        if r3::canonical_bytes(&parsed, r3::COMMAND_BYTES).map_err(core)? != value {
            return Err(invalid());
        }
        Ok(Some(serde_json::from_value(parsed).map_err(|_| invalid())?))
    }
    async fn segment(&mut self, q: &IndexedPageRequest) -> Result<PageFragment> {
        let journal = journal_key(&self.journal)?;
        let n: Vec<u8> =
            sqlx::query_scalar("SELECT ordinal FROM r3_segments WHERE journal=? AND segment=?")
                .bind(journal)
                .bind(q.address.segment.as_str())
                .fetch_one(self.tx())
                .await?;
        if ordinal(n)? > self.at {
            return Err(invalid());
        }
        if !self.seen.contains(q.address.segment.as_str()) {
            self.budget.segments = self
                .budget
                .segments
                .checked_sub(Count::new(1).map_err(core)?)
                .map_err(|_| StoreError::Overloaded)?;
            self.seen.insert(q.address.segment.as_str().into());
        }
        self.charge(q.max_bytes.into(), 1)?;
        let j = self.journal.clone();
        segment_page(
            self.tx(),
            &j,
            &q.address.segment,
            q.address.page,
            q.offset,
            q.max_bytes,
        )
        .await
    }
    async fn object(&mut self, q: &ObjectPageRequest) -> Result<Vec<u8>> {
        let journal = journal_key(&self.journal)?;
        let kind = serde_json::to_value(&q.kind).map_err(|_| invalid())?;
        let origin = r3::canonical_bytes(&q.origin, 2048).map_err(core)?;
        let n:Vec<u8>=sqlx::query_scalar("SELECT ordinal FROM r3_objects WHERE journal=? AND origin=? AND kind=? AND full_key=? AND body_hash=?")
            .bind(journal).bind(origin).bind(kind.as_str().ok_or_else(invalid)?).bind(&q.key).bind(q.hash.as_str()).fetch_one(self.tx()).await?;
        if ordinal(n)? > self.at {
            return Err(invalid());
        }
        self.charge(q.max_bytes.into(), 1)?;
        let j = self.journal.clone();
        object_page(self.tx(), &j, q).await
    }
    async fn certificate(&mut self, family: &wire::Family) -> Result<Option<wire::Certificate>> {
        let key = r3::runtime::points::Point::family(family)
            .map_err(core)?
            .key;
        let Some(State::Family(state)) = self.state(HeadKind::Family, &key, self.at).await? else {
            return Err(invalid());
        };
        let Some(expected) = state.first_closure else {
            return Ok(None);
        };
        let journal = journal_key(&self.journal)?;
        // The partial index names the first retained unavailable family version.
        // This does not scan historical rounds or rewrite any case.
        let n:Vec<u8>=sqlx::query_scalar("SELECT ordinal FROM r3_head_versions WHERE journal=? AND kind=9 AND full_key=? AND ordinal<=? AND json_extract(CAST(value AS TEXT),'$.body.first_closure') IS NOT NULL ORDER BY ordinal LIMIT 1")
            .bind(&journal).bind(key).bind(self.at.value().to_be_bytes().as_slice()).fetch_one(self.tx()).await?;
        // The ordinal index selects one bounded CLOSE command, then its retained
        // certificate head. Never materialize its potentially 8 MiB segment.
        let length: i64 = sqlx::query_scalar(
            "SELECT length(command) FROM r3_commands WHERE journal=? AND ordinal=?",
        )
        .bind(&journal)
        .bind(&n)
        .fetch_one(self.tx())
        .await?;
        if !(2..=r3::COMMAND_BYTES as i64).contains(&length) {
            return Err(invalid());
        }
        self.charge(length as u128, (length as u128).div_ceil(4096))?;
        let bytes: Vec<u8> =
            sqlx::query_scalar("SELECT command FROM r3_commands WHERE journal=? AND ordinal=?")
                .bind(journal)
                .bind(&n)
                .fetch_one(self.tx())
                .await?;
        let command: wire::Command = r3::parse_exact(&bytes, r3::COMMAND_BYTES).map_err(core)?;
        let wire::Command::Close { payload, .. } = command else {
            return Err(invalid());
        };
        let key = index_key(
            *b"CERTIFIC",
            &[payload.round.value().to_string().as_bytes()],
        )
        .map_err(core)?;
        let Some(State::Certificate(certificate)) =
            self.state(HeadKind::Round, &key, ordinal(n)?).await?
        else {
            return Err(invalid());
        };
        if r3::runtime::hash("closure", &certificate).map_err(core)? != expected
            || !certificate.unavailable.contains(family)
        {
            return Err(invalid());
        }
        Ok(Some(*certificate))
    }
    async fn cleanup(&mut self) -> Result<()> {
        // This owned worker retains the physical lane and connection permit until
        // SQLite acknowledges rollback and shutdown, including initialization errors.
        let mut connection = self.connection.take().ok_or_else(invalid)?;
        let cleanup_deadline = Instant::now() + Duration::from_secs(2);
        let handler = match connection.lock_handle().await {
            Ok(mut handle) => {
                handle.set_progress_handler(1000, move || Instant::now() < cleanup_deadline);
                Ok(())
            }
            Err(error) => Err(StoreError::from(error)),
        };
        let result = match handler {
            Ok(()) => match timeout_at(
                cleanup_deadline,
                sqlx::query("ROLLBACK").execute(&mut *connection),
            )
            .await
            {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(sqlx::Error::Database(e)))
                    if e.message() == "cannot rollback - no transaction is active" =>
                {
                    Ok(())
                }
                Ok(Err(e)) => Err(StoreError::from(e)),
                Err(_) => Err(StoreError::Deadline),
            },
            Err(e) => Err(e),
        };
        if result.is_err() {
            self.store
                .inner
                .disabled
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let closed = connection.close().await.map_err(StoreError::from);
        if closed.is_err() {
            self.store
                .inner
                .disabled
                .store(true, std::sync::atomic::Ordering::Release);
        }
        result.and(closed)
    }
}
impl AdjudicationReadStore for super::super::SqliteStore {
    type Read = SqliteAdjudicationRead;
    async fn begin_adjudication_read(
        &self,
        selection: &SnapshotSelection,
        budget: &wire::ReadBudget,
        deadline: Instant,
    ) -> Result<Self::Read> {
        budget.validate().map_err(core)?;
        if budget.bytes.value() > BYTES
            || budget.pages.value() > PAGES
            || budget.segments.value() > PAGES
        {
            return Err(StoreError::Overloaded);
        }
        let slot = Arc::clone(&self.inner.queue)
            .try_acquire_owned()
            .map_err(|_| StoreError::Overloaded)?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        let (send, receive) = oneshot::channel();
        let store = self.clone();
        let selection = selection.clone();
        let budget = budget.clone();
        tokio::spawn(async move {
            let result = start(store, selection, budget, deadline, slot).await;
            let _ = send.send(result);
        });
        reply(deadline, receive).await
    }
}
async fn start(
    store: super::super::SqliteStore,
    selection: SnapshotSelection,
    budget: wire::ReadBudget,
    deadline: Instant,
    slot: OwnedSemaphorePermit,
) -> Result<SqliteAdjudicationRead> {
    // One bounded reader workspace; excludes other snapshots and WAL writers.
    let lane = timeout_at(
        deadline,
        Arc::clone(&store.inner.adjudication_gate).write_owned(),
    )
    .await
    .map_err(|_| StoreError::Deadline)?;
    if store
        .inner
        .disabled
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return Err(StoreError::WritesDisabled);
    }
    store.require_published()?;
    let connection = timeout_at(deadline, store.inner.readers.acquire())
        .await
        .map_err(|_| StoreError::Deadline)??;
    let mut session = Session {
        connection: Some(connection),
        _slot: slot,
        store,
        journal: selection.journal.clone(),
        at: Count::ZERO,
        budget: budget.clone(),
        seen: BTreeSet::new(),
        deadline,
    };
    let initialized = timeout_at(deadline, initialize(&mut session, &selection))
        .await
        .map_err(|_| StoreError::Deadline)
        .and_then(|r| r);
    match initialized {
        Ok(prefix) => {
            let lease = ReadLease {
                id: prefix.observation().clone(),
                retained_bytes: Count::ZERO,
                workspace_bytes: Count::new(BYTES).expect("fixed workspace below Count maximum"),
                pages: budget.pages,
            };
            Ok(spawn(session, lane, prefix, lease))
        }
        Err(error) => {
            let _ = session.cleanup().await;
            Err(error)
        }
    }
}
async fn initialize(session: &mut Session, selection: &SnapshotSelection) -> Result<TrustedPrefix> {
    let deadline = session.deadline;
    session
        .tx()
        .lock_handle()
        .await?
        .set_progress_handler(1000, move || Instant::now() < deadline);
    sqlx::query("BEGIN").execute(session.tx()).await?;
    let (bytes, pages): (Vec<u8>, i64) =
        sqlx::query_as("SELECT profile,maximum_pages FROM r3_storage_profile WHERE singleton=1")
            .fetch_one(session.tx())
            .await?;
    let profile = ledgerlab_core::canonical::parse_bounded(&bytes, 8192).map_err(core)?;
    let logical: wire::Resource =
        serde_json::from_value(profile["logical"].clone()).map_err(|_| invalid())?;
    let backing: Count =
        serde_json::from_value(profile["backing_bytes"].clone()).map_err(|_| invalid())?;
    if physical::backing_needed(
        u32::try_from(pages).map_err(|_| invalid())?,
        logical.workspace_bytes,
    )? > backing.value()
    {
        return Err(StoreError::Overloaded);
    }
    let install = super::super::read::installation(session.tx()).await?;
    let j = &selection.journal;
    if install.logical_store_id != j.store.as_str()
        || install.scope.tenant != j.scope.0.as_str()
        || install.scope.environment != j.scope.1.as_str()
    {
        return Err(invalid());
    }
    let current = head(session.tx(), j).await?;
    let selected = if let Some(expected) = &selection.historical {
        if expected.ordinal > current.ordinal() || expected.ordinal == Count::ZERO {
            return Err(invalid());
        }
        let (segment, root): (String, String) = sqlx::query_as(
            "SELECT segment,replay_root FROM r3_segments WHERE journal=? AND ordinal=?",
        )
        .bind(journal_key(j)?)
        .bind(expected.ordinal.value().to_be_bytes().as_slice())
        .fetch_one(session.tx())
        .await?;
        let observation = r3::raw_sha256(
            &r3::canonical_bytes(
                &json!([
                    "sqlite-primary-history/1",
                    current.observation(),
                    expected.ordinal,
                    segment,
                    root
                ]),
                8192,
            )
            .map_err(core)?,
        );
        TrustedJournalHead::from_backend(
            j.clone(),
            expected.ordinal,
            Digest::parse(&segment).map_err(core)?,
            Digest::parse(&root).map_err(core)?,
            observation,
        )
        .map_err(core)?
    } else {
        current
    };
    session.at = selected.ordinal();
    let key = index_key(*b"ENROLL__", &[j.registration.as_str().as_bytes()]).map_err(core)?;
    let Some(State::Enrollment(enrollment)) = session
        .state(HeadKind::Enrollment, &key, selected.ordinal())
        .await?
    else {
        return Err(invalid());
    };
    let expected = wire::ExpectedPrefix {
        store: j.store.clone(),
        scope: j.scope.clone(),
        target: enrollment.terms.target,
        enrollment: enrollment.enrollment,
        profile: wire::ExpectedPrefixProfile::CentralAdjudicationR31,
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: selected.ordinal(),
        segment: selected.segment().clone(),
        root: selected.root().clone(),
    };
    if let Some(requested) = &selection.historical {
        requested.matches(&expected).map_err(core)?;
    }
    TrustedPrefix::from_backend(
        &selected,
        expected,
        if selection.historical.is_some() {
            PrefixSelection::Historical
        } else {
            PrefixSelection::CurrentAtRead
        },
    )
    .map_err(core)
}

fn spawn(
    mut session: Session,
    lane: OwnedRwLockWriteGuard<()>,
    prefix: TrustedPrefix,
    lease: ReadLease,
) -> SqliteAdjudicationRead {
    let (send, mut receive) = mpsc::channel(1);
    let deadline = session.deadline;
    let expected = prefix.expected().clone();
    tokio::spawn(async move {
        let _lane = lane;
        loop {
            let request = tokio::select! {biased;_=tokio::time::sleep_until(deadline)=>None,r=receive.recv()=>r};
            let Some(request) = request else { break };
            let stop = match request {
                Request::Segment(q, mut reply) => {
                    let result = tokio::select! {biased;_=reply.closed()=>Err(StoreError::Deadline),r=timeout_at(deadline,session.segment(&q))=>r.map_err(|_|StoreError::Deadline).and_then(|r|r)};
                    let stop = result.is_err();
                    reply.send(result).is_err() || stop
                }
                Request::Object(q, mut reply) => {
                    let result = tokio::select! {biased;_=reply.closed()=>Err(StoreError::Deadline),r=timeout_at(deadline,session.object(&q))=>r.map_err(|_|StoreError::Deadline).and_then(|r|r)};
                    let stop = result.is_err();
                    reply.send(result).is_err() || stop
                }
                Request::Certificate(f, at, mut reply) => {
                    let result = if at != expected {
                        Err(invalid())
                    } else {
                        tokio::select! {biased;_=reply.closed()=>Err(StoreError::Deadline),r=timeout_at(deadline,session.certificate(&f))=>r.map_err(|_|StoreError::Deadline).and_then(|r|r)}
                    };
                    let stop = result.is_err();
                    reply.send(result).is_err() || stop
                }
                Request::Finish(reply) => {
                    let _ = reply.send(session.cleanup().await);
                    return;
                }
            };
            if stop {
                break;
            }
        }
        let _ = session.cleanup().await;
    });
    SqliteAdjudicationRead {
        prefix,
        lease,
        send,
        deadline,
    }
}
async fn reply<T>(deadline: Instant, receive: oneshot::Receiver<Result<T>>) -> Result<T> {
    timeout_at(deadline, receive)
        .await
        .map_err(|_| StoreError::Deadline)?
        .map_err(|_| StoreError::WritesDisabled)?
}
impl AdjudicationReadTx for SqliteAdjudicationRead {
    fn expected_prefix(&self) -> &TrustedPrefix {
        &self.prefix
    }
    fn lease(&self) -> &ReadLease {
        &self.lease
    }
    async fn segment_page(&mut self, q: &IndexedPageRequest) -> Result<PageFragment> {
        let (send, receive) = oneshot::channel();
        self.send
            .try_send(Request::Segment(q.clone(), send))
            .map_err(|_| StoreError::Overloaded)?;
        reply(self.deadline, receive).await
    }
    async fn object_page(&mut self, q: &ObjectPageRequest) -> Result<Vec<u8>> {
        if q.key.len() > 4096
            || q.key.is_empty()
            || q.max_bytes == 0
            || usize::from(q.max_bytes) > r3::PAGE_BYTES
        {
            return Err(invalid());
        }
        let (send, receive) = oneshot::channel();
        self.send
            .try_send(Request::Object(q.clone(), send))
            .map_err(|_| StoreError::Overloaded)?;
        reply(self.deadline, receive).await
    }
    async fn family_certificate(
        &mut self,
        f: &wire::Family,
        at: &wire::ExpectedPrefix,
    ) -> Result<Option<wire::Certificate>> {
        let (send, receive) = oneshot::channel();
        self.send
            .try_send(Request::Certificate(f.clone(), at.clone(), send))
            .map_err(|_| StoreError::Overloaded)?;
        reply(self.deadline, receive).await
    }
    async fn finish(self) -> Result<()> {
        let (send, receive) = oneshot::channel();
        self.send
            .try_send(Request::Finish(send))
            .map_err(|_| StoreError::Overloaded)?;
        reply(self.deadline + Duration::from_secs(2), receive).await
    }
}

#[cfg(test)]
impl super::super::SqliteStore {
    pub(crate) async fn test_reader_indices(&self) {
        let mut c = self.inner.readers.acquire().await.unwrap();
        for (sql,index) in [
            ("EXPLAIN QUERY PLAN SELECT command FROM r3_commands WHERE journal=? AND ordinal=?", "r3_command_ordinal"),
            ("EXPLAIN QUERY PLAN SELECT ordinal FROM r3_head_versions WHERE journal=? AND kind=9 AND full_key=? AND ordinal<=? AND json_extract(CAST(value AS TEXT),'$.body.first_closure') IS NOT NULL ORDER BY ordinal LIMIT 1", "r3_family_first_closure"),
        ] {
            let mut q=sqlx::query(sql).bind(Vec::<u8>::new()).bind(Vec::<u8>::new());
            if index=="r3_family_first_closure" {q=q.bind(0u128.to_be_bytes().as_slice());}
            let rows=q.fetch_all(&mut *c).await.unwrap();
            assert!(rows.iter().any(|row|row.get::<String,_>("detail").contains(index)), "indexed bounded lookup {index}: {rows:?}");
        }
    }
}
