//! A registered bounded task owns the driver's borrowed Transaction. Its public
//! private-port handle is owned, Send, poison-on-cancel, and never self-referential.
use super::{adjudication, outcomes, read, write, Inner};
use crate::store::outcomes::*;
use crate::store::{
    errors::{CommitError, StoreError},
    ports::AcceptanceTx,
    records::*,
};
use adjudication::publication::{
    PublicationOutcome, PublicationOwner, SnapshotPin, SqlCommitOutcome, WritePublication,
};
use adjudication::recovery::{Disposition, Gate, Work};
use std::{
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
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
    Native(
        adjudication::Operation,
        oneshot::Sender<Result<adjudication::Value, StoreError>>,
    ),
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
    pub(crate) admission_pid: Arc<AtomicI32>,
    #[cfg(test)]
    pub(crate) pid: i32,
}
struct DriverState {
    config: super::PostgresConfig,
    gate: Option<Gate>,
    admission_pid: Arc<AtomicI32>,
    publication: Option<Arc<PublicationOwner>>,
    pin: Option<SnapshotPin>,
    write: Option<WritePublication>,
    unbound_locked: bool,
}
impl DriverState {
    async fn ensure_gate(&mut self, deadline: Instant) -> Result<(), StoreError> {
        if self.gate.is_none() {
            self.gate = Some(if let Some(owner) = &self.publication {
                let mut gate =
                    Gate::acquire_raw(&self.config, deadline, &self.admission_pid).await?;
                owner.recover_under_gate(&mut gate, deadline).await?;
                gate.resolve_after_publication().await?;
                gate
            } else {
                Gate::acquire(&self.config, deadline, &self.admission_pid).await?
            });
        }
        Ok(())
    }
    async fn mutation(
        &mut self,
        tx: &Transaction<'_>,
        deadline: Instant,
    ) -> Result<(), StoreError> {
        self.ensure_gate(deadline).await?;
        if let Some(owner) = &self.publication {
            if self.write.is_none() {
                self.write = Some(
                    owner
                        .begin_write(
                            self.gate.as_mut().ok_or(StoreError::WritesDisabled)?,
                            tx,
                            self.pin.as_ref().ok_or(StoreError::WritesDisabled)?,
                            deadline,
                        )
                        .await?,
                );
            }
        } else if !self.unbound_locked {
            // Recheck under a transaction lock so old unfenced handles cannot
            // write after a trusted owner binds the installation.
            tx.query_one(
                "SELECT singleton FROM ledgerlab.r3_commit_witness WHERE singleton=1 FOR UPDATE",
                &[],
            )
            .await?;
            super::require_unbound(tx).await?;
            self.unbound_locked = true;
        }
        Ok(())
    }
    fn dirty(&mut self) {
        if let Some(write) = &mut self.write {
            write.mark_mutation();
        }
    }
}
pub(super) async fn start(
    owner: Arc<Inner>,
    permit: OwnedSemaphorePermit,
    deadline: Instant,
) -> Result<PostgresTx, StoreError> {
    start_work(owner, permit, deadline, None).await
}
pub(super) async fn start_work(
    owner: Arc<Inner>,
    permit: OwnedSemaphorePermit,
    deadline: Instant,
    work: Option<Work>,
) -> Result<PostgresTx, StoreError> {
    let admission_pid = Arc::new(AtomicI32::new(0));
    let control_pid = admission_pid.clone();
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
        let publication = owner.publication.clone();
        #[cfg(test)]
        let trace_label = super::trace::label();
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
            let mut state = DriverState {
                config: config.clone(),
                gate: None,
                admission_pid: control_pid.clone(),
                publication,
                pin: None,
                write: None,
                unbound_locked: false,
            };
            let result = async {
                super::verify_server(&session.client).await?;
                let pid: i32 = session
                    .client
                    .query_one("SELECT pg_backend_pid()", &[])
                    .await?
                    .try_get(0)?;
                if let Some(work) = work {
                    state.ensure_gate(deadline).await?;
                    state
                        .gate
                        .as_mut()
                        .ok_or(StoreError::WritesDisabled)?
                        .arm(&session.client, work)
                        .await?;
                }
                let tx = session
                    .client
                    .build_transaction()
                    .isolation_level(IsolationLevel::Serializable)
                    .start()
                    .await?;
                if let Some(owner) = &state.publication {
                    state.pin = Some(owner.pin_snapshot(&tx, deadline).await?);
                } else {
                    super::require_unbound(&tx).await?;
                }
                Ok::<_, StoreError>((tx, pid))
            };
            let mut pending = None;
            match timeout_at(deadline, result).await {
                Ok(Ok((tx, pid))) => {
                    if ready.send(Ok(pid)).is_ok() {
                        #[cfg(test)]
                        {
                            pending = super::trace::scope(
                                trace_label,
                                drive(tx, receiver, deadline, &mut state, pid),
                            )
                            .await;
                        }
                        #[cfg(not(test))]
                        {
                            pending = drive(tx, receiver, deadline, &mut state).await;
                        }
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
            let claimed = state.gate.as_ref().is_some_and(|g| g.work.is_some());
            let sql_outcome = match pending.as_ref().map(|(_, result)| result) {
                Some(Ok(())) => SqlCommitOutcome::Committed,
                Some(Err(CommitError::OutcomeUnknown)) => SqlCommitOutcome::Unknown,
                _ => SqlCommitOutcome::RolledBack,
            };
            let settled = if let Some(write) = state.write.take() {
                match state.gate.as_mut() {
                    Some(gate) => match write
                        .settle_after_exit(
                            gate,
                            sql_outcome,
                            Instant::now() + Duration::from_secs(5),
                        )
                        .await
                    {
                        PublicationOutcome::StableOld | PublicationOutcome::StableNew => Ok(()),
                        PublicationOutcome::Unresolved(error) => Err(error),
                    },
                    None => Err(StoreError::WritesDisabled),
                }
            } else {
                Ok(())
            };
            let resolution = match (settled, state.gate.take()) {
                (Ok(()), Some(g)) => g.finish().await,
                (Ok(()), None) => Ok(None),
                (Err(e), _) => Err(e), // Never clear a still-unpublished native result.
            };
            if let Some((reply, mut outcome)) = pending {
                if outcome.is_ok()
                    && (resolution.is_err()
                        || (claimed
                            && !matches!(&resolution,Ok(Some(r)) if r.disposition==Disposition::Saved)))
                {
                    outcome = Err(CommitError::OutcomeUnknown);
                }
                let _ = reply.send(outcome);
            }
        });
        tasks.retain(|t| !t.is_finished());
        tasks.push(handle);
    }
    let pid = started.await.map_err(|_| StoreError::WritesDisabled)??;
    #[cfg(test)]
    super::trace::log(format_args!(
        "begin pid={pid} remaining_ms={}",
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
    ));
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
        admission_pid,
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
type CommitDelivery = (
    oneshot::Sender<Result<(), CommitError>>,
    Result<(), CommitError>,
);
async fn drive(
    tx: Transaction<'_>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    deadline: Instant,
    state: &mut DriverState,
    #[cfg(test)] pid: i32,
) -> Option<CommitDelivery> {
    let mut failed = false;
    let mut held = outcomes::Locked::default();
    let mut steps = outcomes::Steps::default();
    let mut native = adjudication::Locked::default();
    loop {
        let command = match timeout_at(deadline, commands.recv()).await {
            Ok(Some(c)) => c,
            _ => {
                #[cfg(test)]
                super::trace::log(format_args!(
                    "driver_exit pid={pid} remaining_ms={} channel_closed={}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                    commands.is_closed()
                ));
                break;
            }
        };
        #[cfg(test)]
        let operation = match &command {
            Command::Native(_, _) => "native-r3",
            Command::Outcome(OutcomeOp::Locks(_), _) => "locks",
            Command::Outcome(OutcomeOp::Resolve(_), _) => "resolve",
            Command::Outcome(OutcomeOp::Lookup(_), _) => "lookup",
            Command::Outcome(OutcomeOp::Append(_), _) => "append",
            Command::Outcome(OutcomeOp::FailAt(_), _) => "failpoint",
            Command::Read(_, _) => "read",
            Command::Write(_, _) => "write",
            Command::Commit(_) => "commit",
            Command::Rollback(_) => "rollback",
        };
        #[cfg(test)]
        super::trace::log(format_args!(
            "op_start pid={pid} op={operation} remaining_ms={}",
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
        ));
        match command {
            Command::Native(op, reply) => {
                if failed {
                    let _ = reply.send(Err(StoreError::Integrity("failed PG transaction")));
                    continue;
                }
                let result = timeout_at(
                    deadline.min(Instant::now() + Duration::from_secs(2)),
                    async {
                        clamp(&tx, deadline).await?;
                        if matches!(&op, adjudication::Operation::Locks(..)) {
                            state.mutation(&tx, deadline).await?;
                        }
                        if let adjudication::Operation::Append(p) = &op {
                            if !state
                                .gate
                                .as_ref()
                                .and_then(|g| g.work.as_ref())
                                .is_some_and(|w| w.matches(p))
                            {
                                return Err(StoreError::Integrity(
                                    "native append requires durable work identity",
                                ));
                            }
                        }
                        let value =
                            adjudication::operation(&tx, &mut native, &mut held, &mut steps, op)
                                .await?;
                        if native.inserted_guard || native.appended {
                            state.dirty();
                        }
                        Ok(value)
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                failed = result.is_err();
                if reply.send(result).is_err() {
                    break;
                }
            }
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
                                if scopes.iter().any(|l| l.mode == OutcomeLockMode::Write)
                                    || outcomes::missing_guards(&tx, &scopes).await?
                                {
                                    state.mutation(&tx, deadline).await?;
                                }
                                outcomes::lock(&tx, &mut held, &scopes).await?;
                                if held.inserted_guard {
                                    state.dirty();
                                }
                                OutcomeValue::Unit
                            }
                            OutcomeOp::Lookup(key) => {
                                OutcomeValue::Delivery(Box::new(outcomes::lookup(&tx, &key).await?))
                            }
                            OutcomeOp::Resolve(q) => OutcomeValue::Resolution(
                                outcomes::resolve(&tx, &mut held, &q).await?,
                            ),
                            OutcomeOp::Append(plan) => {
                                state.mutation(&tx, deadline).await?;

                                outcomes::append(&tx, &held, &plan, &mut steps).await?;
                                state.dirty();
                                OutcomeValue::Unit
                            }
                        })
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                #[cfg(test)]
                super::trace::log(format_args!(
                    "op_end pid={pid} op={operation} remaining_ms={} result={:?}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                    result.as_ref().map(|_| ())
                ));
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
                #[cfg(test)]
                super::trace::log(format_args!(
                    "op_end pid={pid} op={operation} remaining_ms={} result={:?}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                    result.as_ref().map(|_| ())
                ));
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
                        state.mutation(&tx, deadline).await?;
                        write::operation(&tx, &op).await?;
                        state.dirty();
                        Ok(())
                    },
                )
                .await
                .unwrap_or(Err(StoreError::Deadline));
                #[cfg(test)]
                super::trace::log(format_args!(
                    "op_end pid={pid} op={operation} remaining_ms={} result={:?}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis(),
                    result.as_ref().map(|_| ())
                ));
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
                #[cfg(test)]
                super::trace::log(format_args!(
                    "op_end pid={pid} op={operation} remaining_ms={} result={result:?}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                ));
                let _ = reply.send(result);
                return None;
            }
            Command::Commit(reply) => {
                let drain = Instant::now() + Duration::from_secs(5);
                if !failed && Instant::now() < deadline {
                    if let Some(write) = &mut state.write {
                        // The nonce is private storage publication metadata, not
                        // an economic generation or a substitute for command identity.
                        let prepared = write
                            .prepare_commit(&tx, b"ledgerlab-postgres-transaction/1", deadline)
                            .await;
                        failed = prepared.is_err();
                    }
                }
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
                #[cfg(test)]
                super::trace::log(format_args!(
                    "op_end pid={pid} op={operation} remaining_ms={} result={result:?}",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                ));
                return Some((reply, result));
            }
        }
    }
    // Closed handle or expired admission: no COMMIT has been issued. Explicit
    // rollback is bounded; the enclosing session is discarded in every case.
    let rollback = timeout_at(Instant::now() + Duration::from_secs(5), tx.rollback()).await;
    #[cfg(test)]
    super::trace::log(format_args!(
        "driver_exit_rollback pid={pid} result={rollback:?}"
    ));
    #[cfg(not(test))]
    let _ = rollback;
    None
}
impl PostgresTx {
    /// Native storage slice, private to the backend. This is not a physical
    /// admission route and cannot construct a CommitCapability.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Native R3 backend awaits enforced physical admission"
        )
    )]
    pub(super) async fn native_adjudication(
        &mut self,
        op: adjudication::Operation,
    ) -> Result<adjudication::Value, StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("failed or cancelled PG handle"));
        }
        self.failed = true;
        let (reply, receive) = oneshot::channel();
        self.sender
            .send(Command::Native(op, reply))
            .map_err(|_| StoreError::WritesDisabled)?;
        let result = receive.await.map_err(|_| StoreError::WritesDisabled)??;
        self.failed = false;
        Ok(result)
    }
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
