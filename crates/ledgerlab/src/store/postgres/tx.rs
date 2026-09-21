//! A registered bounded task owns the driver's borrowed Transaction. Its public
//! private-port handle is owned, Send, poison-on-cancel, and never self-referential.
use super::{outcomes, read, write, Inner};
use crate::store::outcomes::*;
use crate::store::{
    errors::{CommitError, StoreError},
    ports::AcceptanceTx,
    records::*,
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, OwnedSemaphorePermit},
    time::{timeout_at, Instant},
};
use tokio_postgres::{IsolationLevel, Transaction};

enum Read {
    Outbox(crate::outbox::Query),
    Installation,
    Chain(Scope, String),
    Authority(Scope, String),
    Binding(Scope, String),
    Document(Scope, String),
    Grant(Scope, String),
    Identity(Scope, String, String),
    Claim(Scope, String, String, String, String),
}
enum Value {
    Outbox(crate::outbox::Snapshot),
    Installation(Installation),
    Chain(Option<Chain>),
    Authority(Option<AuthorityHead>),
    Binding(Option<BindingHead>),
    Document(Option<(String, CanonicalRecord)>),
    Grant(Option<String>),
    Identity(Option<StoredIdentity>),
    Claim(Option<StoredClaim>),
}
enum OutcomeOp {
    #[cfg(test)]
    FailAt(usize),
    Locks(Vec<OutcomeLock>),
    Lookup(ScopedDelivery),
    Resolve(OutcomeResolve),
    Append(Box<ValidatedOutcomePlan>),
}
enum OutcomeValue {
    Unit,
    Delivery(Box<Option<StoredCompositeDelivery>>),
    Resolution(OutcomeResolution),
}
enum Command {
    Outcome(OutcomeOp, oneshot::Sender<Result<OutcomeValue, StoreError>>),
    Read(Read, oneshot::Sender<Result<Value, StoreError>>),
    Write(WriteOp, oneshot::Sender<Result<(), StoreError>>),
    Commit(oneshot::Sender<Result<(), CommitError>>),
    Rollback(oneshot::Sender<Result<(), StoreError>>),
}
pub(crate) struct PostgresTx {
    sender: mpsc::UnboundedSender<Command>,
    failed: bool,
    // Holding the owner keeps the task registry and bounded connection lease alive.
    _owner: Arc<Inner>,
    #[cfg(test)]
    pub(crate) pid: i32,
}
pub(super) async fn start(
    owner: Arc<Inner>,
    permit: OwnedSemaphorePermit,
    deadline: Instant,
) -> Result<PostgresTx, StoreError> {
    let (sender, receiver) = mpsc::unbounded_channel();
    let (ready, started) = oneshot::channel();
    // Register before even opening a socket. Close and registration share this
    // critical section, so an in-flight begin cannot escape the shutdown drain.
    {
        let mut tasks = owner.tasks.lock().expect("PG task registry");
        if owner.closed.load(Ordering::Acquire) {
            return Err(StoreError::WritesDisabled);
        }
        let config = owner.config.clone();
        let handle = tokio::spawn(async move {
            let _permit = permit;
            let mut session = match timeout_at(deadline, config.connect()).await {
                Ok(Ok(session)) => session,
                Ok(Err(e)) => {
                    let _ = ready.send(Err(e));
                    return;
                }
                Err(_) => {
                    let _ = ready.send(Err(StoreError::Deadline));
                    return;
                }
            };
            let result = async {
                super::verify_server(&session.client).await?;
                let pid: i32 = session
                    .client
                    .query_one("SELECT pg_backend_pid()", &[])
                    .await?
                    .try_get(0)?;
                let tx = session
                    .client
                    .build_transaction()
                    .isolation_level(IsolationLevel::Serializable)
                    .start()
                    .await?;
                Ok::<_, StoreError>((tx, pid))
            };
            match timeout_at(deadline, result).await {
                Ok(Ok((tx, pid))) => {
                    if ready.send(Ok(pid)).is_ok() {
                        drive(tx, receiver, deadline).await;
                    } else {
                        let _ = timeout_at(Instant::now() + Duration::from_secs(5), tx.rollback())
                            .await;
                    }
                }
                Ok(Err(e)) => {
                    let _ = ready.send(Err(e));
                }
                Err(_) => {
                    let _ = ready.send(Err(StoreError::Deadline));
                }
            }
            session.discard().await;
        });
        tasks.retain(|t| !t.is_finished());
        tasks.push(handle);
    }
    let pid = started.await.map_err(|_| StoreError::WritesDisabled)??;
    #[cfg(test)]
    owner.last_pid.store(pid, Ordering::Release);
    if owner.closed.load(Ordering::Acquire) {
        drop(sender);
        return Err(StoreError::WritesDisabled);
    }
    #[cfg(not(test))]
    let _ = pid;
    Ok(PostgresTx {
        sender,
        failed: false,
        _owner: owner,
        #[cfg(test)]
        pid,
    })
}
async fn read_one(tx: &Transaction<'_>, request: Read) -> Result<Value, StoreError> {
    Ok(match request {
        Read::Outbox(query) => Value::Outbox(super::outbox::read(tx, query).await?),
        Read::Installation => Value::Installation(read::installation(tx).await?),
        Read::Chain(s, id) => Value::Chain(read::chain(tx, &s, &id).await?),
        Read::Authority(s, id) => Value::Authority(read::authority(tx, &s, &id).await?),
        Read::Binding(s, id) => Value::Binding(read::binding(tx, &s, &id).await?),
        Read::Document(s, id) => Value::Document(read::document(tx, &s, &id).await?),
        Read::Grant(s, id) => Value::Grant(read::grant_document(tx, &s, &id).await?),
        Read::Identity(s, source, id) => {
            Value::Identity(read::identity(tx, &s, &source, &id).await?)
        }
        Read::Claim(s, source, operation, kind, token) => {
            Value::Claim(read::claim(tx, &s, &source, &operation, &kind, &token).await?)
        }
    })
}
async fn clamp(tx: &Transaction<'_>, deadline: Instant) -> Result<(), StoreError> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    if remaining == 0 {
        return Err(StoreError::Deadline);
    }
    tx.query_one("SELECT set_config('statement_timeout',$1,true),set_config('lock_timeout',$2,true),set_config('idle_in_transaction_session_timeout',$3,true)",&[&remaining.min(2000).to_string(),&remaining.min(500).to_string(),&remaining.min(5000).to_string()]).await?;
    Ok(())
}
async fn drive(
    tx: Transaction<'_>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    deadline: Instant,
) {
    let mut failed = false;
    let mut held = outcomes::Locked::default();
    let mut steps = outcomes::Steps::default();
    loop {
        let command = match timeout_at(deadline, commands.recv()).await {
            Ok(Some(c)) => c,
            _ => break,
        };
        match command {
            Command::Outcome(op, reply) => {
                if failed {
                    let _ = reply.send(Err(StoreError::Integrity("failed PG transaction")));
                    continue;
                }
                let result = timeout_at(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                    async {
                        clamp(&tx, deadline).await?;
                        Ok(match op {
                            #[cfg(test)]
                            OutcomeOp::FailAt(at) => {
                                steps.fail_at = Some(at);
                                OutcomeValue::Unit
                            }
                            OutcomeOp::Locks(scopes) => {
                                outcomes::lock(&tx, &mut held, &scopes).await?;
                                OutcomeValue::Unit
                            }
                            OutcomeOp::Lookup(key) => {
                                OutcomeValue::Delivery(Box::new(outcomes::lookup(&tx, &key).await?))
                            }
                            OutcomeOp::Resolve(q) => OutcomeValue::Resolution(
                                outcomes::resolve(&tx, &mut held, &q).await?,
                            ),
                            OutcomeOp::Append(plan) => {
                                outcomes::append(&tx, &held, &plan, &mut steps).await?;
                                OutcomeValue::Unit
                            }
                        })
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                failed = result.is_err();
                if reply.send(result).is_err() {
                    break;
                }
            }
            Command::Read(request, reply) => {
                if failed {
                    let _ = reply.send(Err(StoreError::Integrity("failed PG transaction")));
                    continue;
                }
                let result = timeout_at(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                    async {
                        clamp(&tx, deadline).await?;
                        read_one(&tx, request).await
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                failed = result.is_err();
                if reply.send(result).is_err() {
                    break;
                }
            }
            Command::Write(op, reply) => {
                if failed {
                    let _ = reply.send(Err(StoreError::Integrity("failed PG transaction")));
                    continue;
                }
                let result = timeout_at(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                    async {
                        clamp(&tx, deadline).await?;
                        write::operation(&tx, &op).await
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                failed = result.is_err();
                if reply.send(result).is_err() {
                    break;
                }
            }
            Command::Rollback(reply) => {
                let result = timeout_at(Instant::now() + Duration::from_secs(5), tx.rollback())
                    .await
                    .map_err(|_| StoreError::WritesDisabled)
                    .and_then(|r| r.map_err(StoreError::from));
                let _ = reply.send(result);
                return;
            }
            Command::Commit(reply) => {
                let drain = Instant::now() + Duration::from_secs(5);
                let result = if failed || Instant::now() >= deadline {
                    match timeout_at(drain, tx.rollback()).await {
                        Ok(Ok(())) => Err(CommitError::RolledBack(StoreError::Integrity(
                            "failed or expired PG transaction",
                        ))),
                        _ => Err(CommitError::OutcomeUnknown),
                    }
                } else {
                    match timeout_at(drain, tx.commit()).await {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(e))
                            if matches!(e.code().map(|c| c.code()), Some("40001" | "40P01")) =>
                        {
                            Err(CommitError::RolledBack(e.into()))
                        }
                        _ => Err(CommitError::OutcomeUnknown),
                    }
                };
                let _ = reply.send(result);
                return;
            }
        }
    }
    // Closed handle or expired admission: no COMMIT has been issued. Explicit
    // rollback is bounded; the enclosing session is discarded in every case.
    let _ = timeout_at(Instant::now() + Duration::from_secs(5), tx.rollback()).await;
}
impl PostgresTx {
    async fn read(&mut self, request: Read) -> Result<Value, StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("failed or cancelled PG handle"));
        }
        self.failed = true;
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Read(request, reply))
            .map_err(|_| StoreError::WritesDisabled)?;
        let value = receive.await.map_err(|_| StoreError::WritesDisabled)??;
        self.failed = false;
        Ok(value)
    }
}
impl AcceptanceTx for PostgresTx {
    async fn load_outbox(
        &mut self,
        query: crate::outbox::Query,
    ) -> Result<crate::outbox::Snapshot, StoreError> {
        match self.read(Read::Outbox(query)).await? {
            Value::Outbox(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_installation(&mut self) -> Result<Installation, StoreError> {
        match self.read(Read::Installation).await? {
            Value::Installation(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_chain(&mut self, s: &Scope, id: &str) -> Result<Option<Chain>, StoreError> {
        match self.read(Read::Chain(s.clone(), id.into())).await? {
            Value::Chain(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_authority(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<AuthorityHead>, StoreError> {
        match self.read(Read::Authority(s.clone(), id.into())).await? {
            Value::Authority(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_binding(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<BindingHead>, StoreError> {
        match self.read(Read::Binding(s.clone(), id.into())).await? {
            Value::Binding(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
        match self.read(Read::Document(s.clone(), id.into())).await? {
            Value::Document(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_grant_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<String>, StoreError> {
        match self.read(Read::Grant(s.clone(), id.into())).await? {
            Value::Grant(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_identity(
        &mut self,
        s: &Scope,
        source: &str,
        id: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        match self
            .read(Read::Identity(s.clone(), source.into(), id.into()))
            .await?
        {
            Value::Identity(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn load_claim(
        &mut self,
        s: &Scope,
        source: &str,
        operation: &str,
        kind: &str,
        token: &str,
    ) -> Result<Option<StoredClaim>, StoreError> {
        match self
            .read(Read::Claim(
                s.clone(),
                source.into(),
                operation.into(),
                kind.into(),
                token.into(),
            ))
            .await?
        {
            Value::Claim(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn write(&mut self, op: &WriteOp) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("failed or cancelled PG handle"));
        }
        self.failed = true;
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Write(op.clone(), reply))
            .map_err(|_| StoreError::WritesDisabled)?;
        receive.await.map_err(|_| StoreError::WritesDisabled)??;
        self.failed = false;
        Ok(())
    }
    async fn rollback(self) -> Result<(), StoreError> {
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Rollback(reply))
            .map_err(|_| StoreError::WritesDisabled)?;
        receive.await.map_err(|_| StoreError::WritesDisabled)?
    }
    async fn commit(self) -> Result<(), CommitError> {
        if self.failed {
            return self.rollback().await.map_or_else(
                |_| Err(CommitError::OutcomeUnknown),
                |()| {
                    Err(CommitError::RolledBack(StoreError::Integrity(
                        "cancelled PG handle",
                    )))
                },
            );
        }
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Commit(reply))
            .map_err(|_| CommitError::OutcomeUnknown)?;
        receive.await.unwrap_or(Err(CommitError::OutcomeUnknown))
    }
}

impl PostgresTx {
    #[cfg(test)]
    pub(super) async fn fail_outcome_at(&mut self, at: usize) -> Result<(), StoreError> {
        self.outcome(OutcomeOp::FailAt(at)).await?;
        Ok(())
    }
    async fn outcome(&mut self, op: OutcomeOp) -> Result<OutcomeValue, StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("failed or cancelled PG handle"));
        }
        self.failed = true;
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Outcome(op, reply))
            .map_err(|_| StoreError::WritesDisabled)?;
        let value = receive.await.map_err(|_| StoreError::WritesDisabled)??;
        self.failed = false;
        Ok(value)
    }
}
impl OutcomeTx for PostgresTx {
    async fn lock_scopes(&mut self, scopes: &[OutcomeLock]) -> Result<(), StoreError> {
        self.outcome(OutcomeOp::Locks(scopes.to_vec())).await?;
        Ok(())
    }
    async fn lookup_outcome_delivery(
        &mut self,
        key: &ScopedDelivery,
    ) -> Result<Option<StoredCompositeDelivery>, StoreError> {
        match self.outcome(OutcomeOp::Lookup(key.clone())).await? {
            OutcomeValue::Delivery(v) => Ok(*v),
            _ => unreachable!(),
        }
    }
    async fn resolve_outcome(
        &mut self,
        request: &OutcomeResolve,
    ) -> Result<OutcomeResolution, StoreError> {
        match self.outcome(OutcomeOp::Resolve(request.clone())).await? {
            OutcomeValue::Resolution(v) => Ok(v),
            _ => unreachable!(),
        }
    }
    async fn append_outcome(&mut self, plan: &ValidatedOutcomePlan) -> Result<(), StoreError> {
        self.outcome(OutcomeOp::Append(Box::new(plan.clone())))
            .await?;
        Ok(())
    }
}
