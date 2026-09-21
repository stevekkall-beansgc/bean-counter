//! Test-only observation at the real private port. Contention events are emitted
//! only after the database returns its actual busy/lock-timeout error.
use super::{accept, hooks::Hooks, tests};
use crate::store::{
    errors::{CommitError, StoreError},
    ports::{AcceptanceStore, AcceptanceTx},
    postgres::PostgresTx,
    records::*,
    sqlite::SqliteTx,
};
use ledgerlab_testkit::{stores as tk, HarnessError};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::{
    sync::Notify,
    time::{Duration, Instant},
};

pub(super) trait ConnectionId {
    fn connection_id(&self, fallback: &str) -> String;
}
impl ConnectionId for SqliteTx {
    fn connection_id(&self, fallback: &str) -> String {
        fallback.into()
    }
}
impl ConnectionId for PostgresTx {
    fn connection_id(&self, _fallback: &str) -> String {
        self.pid.to_string()
    }
}
#[derive(Default)]
struct Trace {
    events: Mutex<Vec<tk::TraceEvent>>,
    starts: AtomicUsize,
    blocked: AtomicBool,
    held: AtomicBool,
    changed: Notify,
    release: Notify,
}
impl Trace {
    fn push(&self, request: usize, id: &str, kind: tk::TraceKind) {
        let mut events = self.events.lock().unwrap();
        let sequence = events.len() as u64;
        events.push(tk::TraceEvent {
            sequence,
            request,
            connection: id.into(),
            kind,
        });
    }
}
struct Request {
    index: usize,
    started: AtomicBool,
    id: Mutex<String>,
    trace: Arc<Trace>,
}
impl Request {
    fn start(&self, id: String) {
        *self.id.lock().unwrap() = id.clone();
        if !self.started.swap(true, Ordering::SeqCst) {
            self.trace
                .push(self.index, &id, tk::TraceKind::RequestStarted);
            self.trace.starts.fetch_add(1, Ordering::SeqCst);
            self.trace.changed.notify_one();
        }
    }
    fn push(&self, kind: tk::TraceKind) {
        self.trace.push(self.index, &self.id.lock().unwrap(), kind);
    }
    fn blocked(&self, kind: tk::TraceKind) {
        self.push(kind);
        self.trace.blocked.store(true, Ordering::SeqCst);
        self.trace.changed.notify_one();
    }
}
struct Store<S> {
    inner: S,
    request: Arc<Request>,
    sqlite_id: Option<String>,
}
struct Tx<T> {
    inner: T,
    request: Arc<Request>,
}
impl<S: AcceptanceStore> AcceptanceStore for Store<S>
where
    S::Tx: ConnectionId,
{
    type Tx = Tx<S::Tx>;
    async fn begin(&self, deadline: Instant) -> Result<Self::Tx, StoreError> {
        if let Some(id) = &self.sqlite_id {
            self.request.start(id.clone());
        }
        match self.inner.begin(deadline).await {
            Ok(tx) => {
                let id = tx.connection_id(self.sqlite_id.as_deref().unwrap_or(""));
                self.request.start(id);
                self.request.push(tk::TraceKind::TransactionOpened);
                Ok(Tx {
                    inner: tx,
                    request: self.request.clone(),
                })
            }
            Err(e) => {
                if matches!(&e,StoreError::Database(sqlx::Error::Database(d)) if d.code().and_then(|c|c.parse::<u32>().ok()).is_some_and(|c|c&255==5))
                {
                    self.request.blocked(tk::TraceKind::BeginBlocked);
                }
                Err(e)
            }
        }
    }
}
impl<T: AcceptanceTx> AcceptanceTx for Tx<T> {
    async fn load_installation(&mut self) -> Result<Installation, StoreError> {
        self.inner.load_installation().await
    }
    async fn load_chain(&mut self, s: &Scope, id: &str) -> Result<Option<Chain>, StoreError> {
        let result = self.inner.load_chain(s, id).await;
        if matches!(&result,Err(StoreError::Postgres(e)) if e.code().is_some_and(|c|c.code()=="55P03"))
        {
            self.request.blocked(tk::TraceKind::LockBlocked);
        }
        if self.request.index == 0
            && result.as_ref().is_ok_and(Option::is_some)
            && !self.request.trace.held.swap(true, Ordering::SeqCst)
        {
            self.request.push(tk::TraceKind::LockHeld);
            self.request.trace.changed.notify_one();
            tokio::time::timeout(
                Duration::from_secs(3),
                self.request.trace.release.notified(),
            )
            .await
            .expect("race contention release");
        }
        result
    }
    async fn load_authority(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<AuthorityHead>, StoreError> {
        self.inner.load_authority(s, id).await
    }
    async fn load_binding(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<BindingHead>, StoreError> {
        self.inner.load_binding(s, id).await
    }
    async fn load_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
        self.inner.load_document(s, id).await
    }
    async fn load_grant_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<String>, StoreError> {
        self.inner.load_grant_document(s, id).await
    }
    async fn load_identity(
        &mut self,
        s: &Scope,
        source: &str,
        id: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        self.inner.load_identity(s, source, id).await
    }
    async fn load_claim(
        &mut self,
        s: &Scope,
        source: &str,
        operation: &str,
        kind: &str,
        token: &str,
    ) -> Result<Option<StoredClaim>, StoreError> {
        self.inner
            .load_claim(s, source, operation, kind, token)
            .await
    }
    async fn write(&mut self, op: &WriteOp) -> Result<(), StoreError> {
        self.inner.write(op).await
    }
    async fn rollback(self) -> Result<(), StoreError> {
        self.inner.rollback().await
    }
    async fn commit(self) -> Result<(), CommitError> {
        let result = self.inner.commit().await;
        if result.is_ok() {
            self.request.push(tk::TraceKind::CommitAcknowledged);
        }
        result
    }
}
pub(super) async fn race<S>(
    stores: Vec<(S, Option<String>)>,
    command: &tk::Command,
) -> ledgerlab_testkit::Result<tk::RaceEvidence>
where
    S: AcceptanceStore + 'static,
    S::Tx: ConnectionId + 'static,
{
    let commands = vec![command.clone(); stores.len()];
    race_commands(stores, &commands).await
}
pub(super) async fn race_commands<S>(
    stores: Vec<(S, Option<String>)>,
    commands: &[tk::Command],
) -> ledgerlab_testkit::Result<tk::RaceEvidence>
where
    S: AcceptanceStore + 'static,
    S::Tx: ConnectionId + 'static,
{
    assert_eq!(stores.len(), commands.len());
    let trace = Arc::new(Trace::default());
    let count = stores.len();
    let mut tasks = Vec::new();
    for (index, (inner, sqlite_id)) in stores.into_iter().enumerate() {
        if index > 0 {
            tokio::time::timeout(Duration::from_secs(3), async {
                while !trace.held.load(Ordering::SeqCst) {
                    trace.changed.notified().await;
                }
            })
            .await
            .map_err(|_| HarnessError("race leader never held chain lock".into()))?;
        }
        let request = Arc::new(Request {
            index,
            started: AtomicBool::new(false),
            id: Mutex::new(String::new()),
            trace: trace.clone(),
        });
        let command = tests::Backend::command(&commands[index]);
        tasks.push(tokio::spawn(async move {
            let store = Store {
                inner,
                request: request.clone(),
                sqlite_id,
            };
            let result = accept::run(&store, &command, &Hooks::default()).await;
            request.push(tk::TraceKind::RequestFinished);
            tests::outcome(result).map(|outcome| tk::Attempt { outcome, hit: None })
        }));
    }
    let wait = tokio::time::timeout(Duration::from_secs(3), async {
        while trace.starts.load(Ordering::SeqCst) != count || !trace.blocked.load(Ordering::SeqCst)
        {
            trace.changed.notified().await;
        }
    })
    .await;
    trace.release.notify_one();
    let mut attempts = Vec::new();
    for task in tasks {
        attempts.push(task.await.map_err(|e| HarnessError(e.to_string()))??);
    }
    wait.map_err(|_| {
        HarnessError("no observed database contention before bounded release".into())
    })?;
    let events = trace.events.lock().unwrap().clone();
    Ok(tk::RaceEvidence {
        attempts,
        trace: events,
    })
}
