use super::{
    fake::{MemoryDestination, Mode, Outcome},
    *,
};
use crate::{
    store::{
        errors::{CommitError, StoreError},
        ports::{AcceptanceStore, AcceptanceTx},
        records::WriteOp,
    },
    Backend, Ledger,
};
use std::time::Duration;
use tokio::time::Instant;

pub struct Outbox<'a> {
    ledger: &'a Ledger,
    destination: &'a MemoryDestination,
}
impl Ledger {
    /// Open an externally restored, offline SQLite directory with dispatch held
    /// before returning a usable handle. The operator must stop the old owner and
    /// verify the restored journal separately; this does not copy or verify backups.
    pub async fn open_restored_sqlite(
        path: &std::path::Path,
        destination: &MemoryDestination,
        now: i64,
    ) -> Result<Self> {
        let ledger = Self::open_sqlite(path)
            .await
            .map_err(|_| Error::Unavailable)?;
        if let Err(error) = ledger.outbox(destination).hold(true, now).await {
            ledger.close().await;
            return Err(error);
        }
        Ok(ledger)
    }
    /// PostgreSQL equivalent; use only after the offline restore and journal
    /// verification, with the old installation stopped/fenced by the operator.
    pub async fn open_restored_postgres(
        config: crate::PostgresConfig,
        destination: &MemoryDestination,
        now: i64,
    ) -> Result<Self> {
        let ledger = Self::open_postgres(config)
            .await
            .map_err(|_| Error::Unavailable)?;
        if let Err(error) = ledger.outbox(destination).hold(true, now).await {
            ledger.close().await;
            return Err(error);
        }
        Ok(ledger)
    }
    /// Bounded local fake-destination workflow. The host must retain the destination
    /// independently across ledger reopen/recovery and supply trusted UTC times.
    pub fn outbox<'a>(&'a self, destination: &'a MemoryDestination) -> Outbox<'a> {
        Outbox {
            ledger: self,
            destination,
        }
    }
}
fn store_error(e: StoreError) -> Error {
    if matches!(e, StoreError::InvalidStore("outbox scan limit")) {
        Error::ScanLimit
    } else if e.retryable_after_rollback() {
        Error::Retryable
    } else if matches!(e, StoreError::Integrity(_) | StoreError::InvalidStore(_)) {
        Error::Integrity
    } else {
        Error::Unavailable
    }
}
async fn transaction<S: AcceptanceStore, T>(
    store: &S,
    f: impl FnOnce(Snapshot) -> Result<(T, Mutation)>,
) -> Result<T> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .map_err(store_error)?;
    let result = async {
        let s = tx.load_outbox().await.map_err(store_error)?;
        if s.items.len() > 1000 {
            return Err(Error::ScanLimit);
        }
        let (result, mutation) = f(s)?;
        tx.write(&WriteOp::Outbox(Box::new(mutation)))
            .await
            .map_err(store_error)?;
        Ok(result)
    }
    .await;
    match result {
        Ok(value) => {
            tx.commit().await.map_err(|e| match e {
                CommitError::OutcomeUnknown => Error::OutcomeUnknown,
                CommitError::RolledBack(e) => store_error(e),
            })?;
            Ok(value)
        }
        Err(e) => {
            tx.rollback().await.map_err(store_error)?;
            Err(e)
        }
    }
}
macro_rules! transact {
    ($this:ident, $f:expr) => {
        match &$this.ledger.store {
            Backend::Sqlite(s) => transaction(s, $f).await,
            Backend::Postgres(s) => transaction(s, $f).await,
        }
    };
}
async fn read_snapshot<S: AcceptanceStore>(store: &S) -> Result<Snapshot> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .map_err(store_error)?;
    let result = tx.load_outbox().await.map_err(store_error);
    tx.rollback().await.map_err(store_error)?;
    result
}
macro_rules! snapshot {
    ($this:ident) => {
        match &$this.ledger.store {
            Backend::Sqlite(s) => read_snapshot(s).await,
            Backend::Postgres(s) => read_snapshot(s).await,
        }
    };
}
fn mutation(mut s: Snapshot, event: Value) -> Result<Mutation> {
    s.head.revision = plus(s.head.revision, 1)?;
    Ok(Mutation {
        snapshot: s,
        attempt: None,
        observation: bytes(&event)?,
        report: None,
    })
}
fn clear(d: &mut Delivery, state: State) {
    d.state = state;
    d.owner = None;
    d.until = None;
}
fn valid(s: &Snapshot, l: &Lease, now: i64) -> bool {
    s.installation.logical_store_id == l.store_id
        && s.installation.generation == l.restore_generation
        && s.installation.admission == "open"
        && !s.installation.dispatch_hold
        && s.installation.dispatch_enabled
        && s.head.enabled
        && s.head.owner.as_ref() == Some(&l.owner)
        && s.head.generation == l.generation
        && s.head.until.is_some_and(|u| now < u)
}
impl Outbox<'_> {
    /// Explicit operator acquisition; a still-live dispatcher refuses replacement.
    pub async fn acquire(&self, owner: &str, now: i64) -> Result<Lease> {
        if owner.is_empty() || owner.len() > 128 || owner.chars().any(char::is_control) {
            return Err(Error::InvalidInput);
        }
        transact!(self, |mut s: Snapshot| {
            if s.installation.dispatch_hold
                || !s.installation.dispatch_enabled
                || s.installation.admission != "open"
            {
                return Err(Error::Held);
            }
            if s.head.owner.is_some() && s.head.until.is_some_and(|u| now < u) {
                return Err(Error::Owned);
            }
            s.head.generation = plus(
                s.head.generation.max(
                    self.destination
                        .generation(&s.installation.logical_store_id),
                ),
                1,
            )?;
            s.head.owner = Some(owner.into());
            s.head.until = Some(plus(now, 15_000_000)?);
            s.head.enabled = true;
            for (_, d) in &mut s.items {
                if d.state == State::Leased {
                    clear(d, State::Unknown);
                }
            }
            // Advance the independent fake fence before commit. Unknown/rolled-back
            // commits may stop an old worker early, but cannot let it act late.
            self.destination.fence(
                &s.installation.logical_store_id,
                s.head.generation,
                s.head.until.unwrap(),
            )?;
            let lease = Lease {
                store_id: s.installation.logical_store_id.clone(),
                owner: owner.into(),
                generation: s.head.generation,
                restore_generation: s.installation.generation,
            };
            Ok((
                lease,
                mutation(s, json!({"kind":"acquired","at":now.to_string()}))?,
            ))
        })
    }
    pub async fn renew(&self, lease: &Lease, now: i64) -> Result<()> {
        transact!(self, |mut s: Snapshot| {
            if !valid(&s, lease, now) {
                return Err(Error::Fenced);
            }
            s.head.until = Some(plus(now, 15_000_000)?);
            self.destination
                .fence(&lease.store_id, lease.generation, s.head.until.unwrap())?;
            Ok((
                (),
                mutation(s, json!({"kind":"renewed","at":now.to_string()}))?,
            ))
        })
    }
    /// A committed attempt is the only way to obtain a send capability.
    pub async fn claim(&self, lease: &Lease, now: i64) -> Result<Option<Attempt>> {
        transact!(self, |mut s: Snapshot| {
            if !valid(&s, lease, now) {
                return Err(Error::Fenced);
            }
            for (_, d) in &mut s.items {
                if d.state == State::Leased && d.until.is_some_and(|u| now >= u) {
                    clear(d, State::Unknown);
                }
            }
            let mut picked = None;
            for (index, (i, d)) in s.items.iter().enumerate() {
                if !matches!(d.state, State::Pending | State::Retry | State::Held)
                    || d.next_attempt_us > now
                {
                    continue;
                }
                let (request, deps) = i.request(&s)?;
                if deps
                    .iter()
                    .any(|id| !s.items.iter().any(|(i, _)| &i.id == id))
                {
                    return Err(Error::Integrity);
                }
                if deps.iter().any(|id| {
                    !s.items
                        .iter()
                        .any(|(i, d)| &i.id == id && d.state == State::Delivered)
                }) {
                    continue;
                }
                if d.attempts >= 20 {
                    continue;
                }
                picked = Some((index, request));
                break;
            }
            let mut attempt = None;
            let mut evidence = None;
            if let Some((index, request)) = picked {
                let d = &mut s.items[index].1;
                d.attempts = plus(d.attempts, 1)?;
                d.state = State::Leased;
                d.owner = Some(lease.owner.clone());
                d.generation = lease.generation;
                d.until = Some(plus(now, 30_000_000)?);
                attempt = Some(Attempt {
                    lease: lease.clone(),
                    number: d.attempts,
                    until: d.until.unwrap(),
                    request: request.clone(),
                });
                evidence = Some((
                    request.key.clone(),
                    d.attempts,
                    now,
                    request.request_hash.clone(),
                    bytes(
                        &json!({"request":request.value(),"attempt":d.attempts.to_string(),"generation":lease.generation.to_string()}),
                    )?,
                ));
            }
            let mut m = mutation(s, json!({"kind":"claim","at":now.to_string()}))?;
            m.attempt = evidence;
            Ok((attempt, m))
        })
    }
    /// The fake receives outside any ledger transaction. Fencing and stable-key
    /// deduplication are atomic at that independent destination.
    pub async fn send(&self, attempt: &Attempt, now: i64, mode: Mode) -> Result<Outcome> {
        let s = snapshot!(self)?;
        if !valid(&s, &attempt.lease, now) || !current_attempt(&s, attempt, now) {
            return Err(Error::Fenced);
        }
        Ok(self.destination.send(attempt, now, mode))
    }
    /// Late observations are retained, but cannot change delivery state.
    pub async fn observe(&self, a: &Attempt, now: i64, result: Outcome) -> Result<State> {
        transact!(self, |mut s: Snapshot| {
            let current = valid(&s, &a.lease, now) && current_attempt(&s, a, now);
            let observation = json!({"kind":if current {"response"} else {"late_response"},"key":a.request.key,"attempt":a.number.to_string(),"generation":a.lease.generation.to_string(),"at":now.to_string(),"outcome":outcome_value(&result)});
            let mut state = State::Unknown;
            if current {
                let d = &mut s
                    .items
                    .iter_mut()
                    .find(|(i, _)| i.id == a.request.key)
                    .ok_or(Error::Integrity)?
                    .1;
                state = match &result {
                    Outcome::Delivered(r) if r.request == a.request => State::Delivered,
                    Outcome::Delivered(_) | Outcome::Rejected => State::Rejected,
                    Outcome::Absent if d.attempts < 20 => {
                        d.next_attempt_us = plus(now, retry_delay(d.attempts))?;
                        State::Retry
                    }
                    Outcome::Absent => State::Rejected,
                    Outcome::Unknown | Outcome::Fenced => State::Unknown,
                };
                clear(d, state);
                d.last_observation = Some(digest(&observation)?);
            }
            Ok((
                if current {
                    Ok(state)
                } else {
                    Err(Error::Fenced)
                },
                mutation(s, observation)?,
            ))
        })?
    }
    pub async fn dispatch_one(&self, lease: &Lease, now: i64, mode: Mode) -> Result<Option<State>> {
        let Some(a) = self.claim(lease, now).await? else {
            return Ok(None);
        };
        let outcome = self.send(&a, now, mode).await?;
        self.observe(&a, now, outcome).await.map(Some)
    }
    /// Hold dispatch and revoke capabilities. Restore also advances the installation
    /// generation and invalidates *all* mappings, including previously delivered.
    pub async fn hold(&self, restore: bool, now: i64) -> Result<()> {
        transact!(self, |mut s: Snapshot| {
            if restore {
                s.installation.generation = plus(s.installation.generation, 1)?;
            }
            s.installation.dispatch_hold = true;
            s.installation.dispatch_enabled = false;
            s.head.generation = plus(
                s.head.generation.max(
                    self.destination
                        .generation(&s.installation.logical_store_id),
                ),
                1,
            )?;
            s.head.owner = None;
            s.head.until = None;
            s.head.enabled = false;
            for (_, d) in &mut s.items {
                if restore || d.state == State::Leased {
                    clear(d, State::Unknown);
                }
            }
            self.destination.fence(
                &s.installation.logical_store_id,
                s.head.generation,
                i64::MIN,
            )?;
            Ok((
                (),
                mutation(
                    s,
                    json!({"kind":if restore {"restore_hold"} else {"pause"},"at":now.to_string()}),
                )?,
            ))
        })
    }
    /// Reconcile the entire bounded intention set and the destination inventory.
    /// An unknown inventory can never produce a resume-capable report.
    pub async fn reconcile(&self, now: i64, mode: Mode) -> Result<Report> {
        // Inventory access is an in-memory independent read. No sending occurs.
        transact!(self, |mut s: Snapshot| {
            if !s.installation.dispatch_hold || s.head.owner.is_some() {
                return Err(Error::Held);
            }
            let inventory = self
                .destination
                .inventory(&s.installation.logical_store_id, mode);
            let mut unresolved = Vec::new();
            let mut orphan_keys = Vec::new();
            let mut observations = Vec::new();
            let requests = s
                .items
                .iter()
                .map(|(i, _)| i.request(&s).map(|(r, _)| r))
                .collect::<Result<Vec<_>>>()?;
            for ((_, d), request) in s.items.iter_mut().zip(&requests) {
                let state = match &inventory {
                    None => State::Unknown,
                    Some(receipts) => {
                        match receipts.iter().find(|r| r.request.key == request.key) {
                            Some(r) if &r.request == request => State::Delivered,
                            Some(_) => State::Rejected,
                            None if d.state == State::Rejected || d.attempts >= 20 => {
                                State::Rejected
                            }
                            None => State::Pending,
                        }
                    }
                };
                if matches!(state, State::Unknown | State::Rejected) {
                    unresolved.push(d.intention_id.clone());
                }
                clear(d, state);
                let remote = inventory
                    .as_ref()
                    .and_then(|receipts| receipts.iter().find(|r| r.request.key == request.key));
                let evidence = json!({"key":request.key,"state":state.name(),"expected_request_hash":request.request_hash,"remote_request_hash":remote.map(|r|&r.request.request_hash),"remote_id":remote.map(|r|&r.remote_id),"observed_us":now.to_string()});
                d.last_observation = Some(digest(&evidence)?);
                observations.push(evidence);
            }
            if let Some(receipts) = &inventory {
                for r in receipts {
                    if !requests.iter().any(|i| i.key == r.request.key) {
                        orphan_keys.push(r.request.key.clone());
                    }
                }
            }
            let body = json!({"kind":"reconciliation","sequence":plus(s.head.revision,1)?.to_string(),"store":s.installation.logical_store_id,"generation":s.installation.generation.to_string(),"fingerprint":fingerprint(&s)?,"inventory":digest(&fake::inventory_value(&inventory))?,"complete":inventory.is_some() && unresolved.is_empty() && orphan_keys.is_empty(),"unresolved":unresolved,"orphans":orphan_keys,"observations":observations});
            let hash = digest(&body)?;
            let report = Report {
                digest: hash.clone(),
                unresolved,
                orphan_keys,
            };
            let mut m = mutation(
                s,
                json!({"kind":"reconciled","digest":hash,"at":now.to_string()}),
            )?;
            m.report = Some((hash, bytes(&body)?));
            Ok((report, m))
        })
    }
    pub async fn resume(&self, report_digest: &str, now: i64) -> Result<()> {
        transact!(self, |mut s: Snapshot| {
            if !s.installation.dispatch_hold || s.installation.admission != "open" {
                return Err(Error::Held);
            }
            let (hash, body) = s.report.as_ref().ok_or(Error::StaleReport)?;
            let v: Value = serde_json::from_slice(body).map_err(|_| Error::Integrity)?;
            if hash != report_digest
                || digest(&v)? != *hash
                || v["fingerprint"] != fingerprint(&s)?
                || v["inventory"]
                    != digest(&fake::inventory_value(
                        &self
                            .destination
                            .inventory(&s.installation.logical_store_id, Mode::Normal),
                    ))?
            {
                return Err(Error::StaleReport);
            }
            if v["complete"] != true {
                return Err(Error::NeedsReview);
            }
            s.installation.dispatch_hold = false;
            s.installation.dispatch_enabled = true;
            s.head.enabled = true;
            Ok((
                (),
                mutation(
                    s,
                    json!({"kind":"resumed","digest":report_digest,"at":now.to_string()}),
                )?,
            ))
        })
    }
    pub async fn deliveries(&self) -> Result<Vec<Delivery>> {
        Ok(snapshot!(self)?.items.into_iter().map(|(_, d)| d).collect())
    }
}
fn current_attempt(s: &Snapshot, a: &Attempt, now: i64) -> bool {
    now < a.until
        && s.items.iter().any(|(i, d)| {
            i.id == a.request.key
                && d.state == State::Leased
                && d.attempts == a.number
                && d.generation == a.lease.generation
                && d.owner.as_ref() == Some(&a.lease.owner)
                && d.until == Some(a.until)
        })
}
fn outcome_value(o: &Outcome) -> Value {
    match o {
        Outcome::Delivered(r) => json!({"delivered":r.request.value(),"remote_id":r.remote_id}),
        Outcome::Absent => json!("confirmed_absent"),
        Outcome::Unknown => json!("unknown"),
        Outcome::Rejected => json!("rejected"),
        Outcome::Fenced => json!("fenced"),
    }
}
