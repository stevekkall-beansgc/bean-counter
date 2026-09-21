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
    query: Query,
    seconds: u64,
    f: impl AsyncFnOnce(&mut S::Tx, Snapshot) -> Result<(T, Mutation)>,
) -> Result<T> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(seconds))
        .await
        .map_err(store_error)?;
    let result = async {
        let s = tx.load_outbox(query).await.map_err(store_error)?;
        let (result, mutation) = f(&mut tx, s).await?;
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
        transact!($this, Query::Control, 5, $f)
    };
    ($this:ident, $query:expr, $seconds:expr, $f:expr) => {
        match &$this.ledger.store {
            Backend::Sqlite(s) => transaction(s, $query, $seconds, $f).await,
            Backend::Postgres(s) => transaction(s, $query, $seconds, $f).await,
        }
    };
}
async fn read_snapshot<S: AcceptanceStore>(store: &S, query: Query) -> Result<Snapshot> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .map_err(store_error)?;
    let result = tx.load_outbox(query).await.map_err(store_error);
    tx.rollback().await.map_err(store_error)?;
    result
}
async fn read_deliveries<S: AcceptanceStore>(store: &S) -> Result<Vec<Delivery>> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .map_err(store_error)?;
    let result = async {
        let mut rows = Vec::new();
        let mut after = String::new();
        loop {
            let page = tx
                .load_outbox(Query::page(&after))
                .await
                .map_err(store_error)?;
            if page.items.is_empty() {
                break;
            }
            if rows.len() + page.items.len() > 1000 {
                return Err(Error::ScanLimit);
            }
            after = page.items.last().unwrap().0.id.clone();
            rows.extend(page.items.into_iter().map(|(_, d)| d));
        }
        Ok(rows)
    }
    .await;
    tx.rollback().await.map_err(store_error)?;
    result
}
macro_rules! snapshot {
    ($this:ident, $query:expr) => {
        match &$this.ledger.store {
            Backend::Sqlite(s) => read_snapshot(s, $query).await,
            Backend::Postgres(s) => read_snapshot(s, $query).await,
        }
    };
}
fn mutation(mut s: Snapshot, event: Value) -> Result<Mutation> {
    s.head.revision = plus(s.head.revision, 1)?;
    Ok(Mutation {
        snapshot: s,
        sweep: None,
        quarantine: None,
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
        transact!(self, async |_, mut s: Snapshot| {
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
            let mut m = mutation(s, json!({"kind":"acquired","at":now.to_string()}))?;
            m.sweep = Some(Sweep::Leases);
            Ok((lease, m))
        })
    }
    pub async fn renew(&self, lease: &Lease, now: i64) -> Result<()> {
        transact!(self, async |_, mut s: Snapshot| {
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
        transact!(self, async |tx, mut s: Snapshot| {
            if !valid(&s, lease, now) {
                return Err(Error::Fenced);
            }
            let mut after = String::new();
            let mut picked = None;
            loop {
                s = tx
                    .load_outbox(Query::Page {
                        after: after.clone(),
                        due: Some(now),
                    })
                    .await
                    .map_err(store_error)?;
                if s.items.is_empty() {
                    break;
                }
                for (index, (i, _)) in s.items.iter().enumerate() {
                    let (request, deps) = i.request(&s)?;
                    let mut ready = true;
                    for id in deps {
                        let dependency =
                            tx.load_outbox(Query::Key(id)).await.map_err(store_error)?;
                        let (_, d) = dependency.items.first().ok_or(Error::Integrity)?;
                        ready &= d.state == State::Delivered && d.quarantine.is_none();
                    }
                    if ready {
                        picked = Some((index, request));
                        break;
                    }
                }
                if picked.is_some() {
                    break;
                }
                after = s.items.last().unwrap().0.id.clone();
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
            m.sweep = Some(Sweep::Expired(now));
            Ok((attempt, m))
        })
    }
    /// The fake receives outside any ledger transaction. Fencing and stable-key
    /// deduplication are atomic at that independent destination.
    pub async fn send(&self, attempt: &Attempt, now: i64, mode: Mode) -> Result<Outcome> {
        let s = snapshot!(self, Query::Key(attempt.request.key.clone()))?;
        if !valid(&s, &attempt.lease, now) || !current_attempt(&s, attempt, now) {
            return Err(Error::Fenced);
        }
        Ok(self.destination.send(attempt, now, mode))
    }
    /// Late observations are retained, but cannot change delivery state.
    pub async fn observe(&self, a: &Attempt, now: i64, result: Outcome) -> Result<State> {
        transact!(
            self,
            Query::Key(a.request.key.clone()),
            5,
            async |_, mut s: Snapshot| {
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
            }
        )?
    }
    pub async fn dispatch_one(&self, lease: &Lease, now: i64, mode: Mode) -> Result<Option<State>> {
        let Some(a) = self.claim(lease, now).await? else {
            return Ok(None);
        };
        let outcome = self.send(&a, now, mode).await?;
        self.observe(&a, now, outcome).await.map(Some)
    }
    /// Hold dispatch and revoke capabilities. Restore also advances the installation
    /// generation and invalidates receipt mappings, preserving rejection and quarantine.
    pub async fn hold(&self, restore: bool, now: i64) -> Result<()> {
        transact!(self, async |_, mut s: Snapshot| {
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
            self.destination.fence(
                &s.installation.logical_store_id,
                s.head.generation,
                i64::MIN,
            )?;
            let mut m = mutation(
                s,
                json!({"kind":if restore {"restore_hold"} else {"pause"},"at":now.to_string()}),
            )?;
            m.sweep = Some(if restore {
                Sweep::Restore
            } else {
                Sweep::Leases
            });
            Ok(((), m))
        })
    }
    /// Reconcile all intentions with bounded keyset reads, evidence pages and samples.
    /// The transaction retains the hold on cancellation, conflict or incomplete inventory.
    pub async fn reconcile(&self, now: i64, mode: Mode) -> Result<Report> {
        transact!(self, Query::Control, 60, async |tx, mut s: Snapshot| {
            if !s.installation.dispatch_hold || s.head.owner.is_some() {
                return Err(Error::Held);
            }
            let store = s.installation.logical_store_id.clone();
            let inventory = self.destination.inventory_digest(&store, mode)?;
            let mut fingerprint = fingerprint_start(&s)?;
            let mut report = Report {
                digest: String::new(),
                unresolved: Vec::new(),
                orphan_keys: Vec::new(),
                unresolved_count: 0,
                orphan_count: 0,
                intention_count: 0,
            };
            let mut observations = Vec::new();
            let mut after = String::new();
            loop {
                s = tx
                    .load_outbox(Query::page(&after))
                    .await
                    .map_err(store_error)?;
                if s.items.is_empty() {
                    break;
                }
                let mut page = Vec::new();
                for index in 0..s.items.len() {
                    let request = s.items[index].0.request(&s)?.0;
                    let outcome = self.destination.lookup(&store, &request.key, mode);
                    let (i, d) = &mut s.items[index];
                    let state = reconciled_state(d, &request, &outcome);
                    if matches!(state, State::Unknown | State::Rejected) && d.quarantine.is_none() {
                        report.unresolved_count = plus(report.unresolved_count, 1)?;
                        if report.unresolved.len() < PAGE_SIZE {
                            report.unresolved.push(i.id.clone());
                        }
                    }
                    let remote = match &outcome {
                        Outcome::Delivered(r) => Some(r),
                        _ => None,
                    };
                    let evidence = json!({"key":request.key,"state":state.name(),"quarantine":d.quarantine,
                        "expected_request_hash":request.request_hash,"remote_request_hash":remote.map(|r|&r.request.request_hash),
                        "remote_id":remote.map(|r|&r.remote_id),"observed_us":now.to_string()});
                    clear(d, state);
                    d.last_observation = Some(digest(&evidence)?);
                    fingerprint = fingerprint_item(&fingerprint, i, d)?;
                    if observations.len() < PAGE_SIZE {
                        observations.push(evidence.clone());
                    }
                    page.push(evidence);
                    report.intention_count = plus(report.intention_count, 1)?;
                }
                after = s.items.last().unwrap().0.id.clone();
                let m = mutation(
                    s,
                    json!({"kind":"reconciliation_page","at":now.to_string(),"observations":page}),
                )?;
                tx.write(&WriteOp::Outbox(Box::new(m)))
                    .await
                    .map_err(store_error)?;
            }
            // Stream only remote keys; no complete receipt or request collection is loaded.
            after.clear();
            loop {
                let keys = self.destination.keys_page(&store, &after);
                if keys.is_empty() {
                    break;
                }
                for key in &keys {
                    let local = tx
                        .load_outbox(Query::Key(key.clone()))
                        .await
                        .map_err(store_error)?;
                    if local.items.is_empty() {
                        report.orphan_count = plus(report.orphan_count, 1)?;
                        if report.orphan_keys.len() < PAGE_SIZE {
                            report.orphan_keys.push(key.clone());
                        }
                    }
                }
                after = keys.last().unwrap().clone();
            }
            if self.destination.inventory_digest(&store, mode)? != inventory {
                return Err(Error::StaleReport);
            }
            let body = json!({"kind":"reconciliation","version":2,"sequence":plus(s.head.revision,1)?.to_string(),
                "store":store,"generation":s.installation.generation.to_string(),"fingerprint":fingerprint,
                "inventory":inventory,"complete":inventory.is_some() && report.unresolved_count==0 && report.orphan_count==0,
                "unresolved":report.unresolved,"orphans":report.orphan_keys,"observations":observations,
                "intention_count":report.intention_count.to_string(),"unresolved_count":report.unresolved_count.to_string(),"orphan_count":report.orphan_count.to_string()});
            let hash = digest(&body)?;
            report.digest = hash.clone();
            let mut m = mutation(
                s,
                json!({"kind":"reconciled","digest":hash,"at":now.to_string()}),
            )?;
            m.report = Some((hash, bytes(&body)?));
            Ok((report, m))
        })
    }
    /// Permanent isolation of one unresolved intention. The trusted host authenticates
    /// the operator; this cannot retry, erase a rejection or attest economic settlement.
    /// expected_observation prevents resolving an intention whose evidence changed.
    pub async fn quarantine(
        &self,
        intention_id: &str,
        expected_observation: &str,
        operator: &str,
        reason: &str,
        now: i64,
    ) -> Result<String> {
        if operator.trim().is_empty()
            || operator.len() > 128
            || operator.chars().any(char::is_control)
            || reason.trim().is_empty()
            || reason.len() > 2048
            || reason.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput);
        }
        transact!(
            self,
            Query::Key(intention_id.into()),
            5,
            async |_, s: Snapshot| {
                if !s.installation.dispatch_hold || s.head.owner.is_some() {
                    return Err(Error::Held);
                }
                let (i, d) = s.items.first().ok_or(Error::InvalidInput)?;
                if d.quarantine.is_some()
                    || d.last_observation.as_deref() != Some(expected_observation)
                {
                    return Err(Error::StaleReport);
                }
                if !matches!(d.state, State::Rejected | State::Unknown) {
                    return Err(Error::InvalidInput);
                }
                let event = json!({"kind":"quarantine","store":s.installation.logical_store_id,
                "generation":s.installation.generation.to_string(),"key":i.id,"intention_hash":i.hash,
                "previous_state":d.state.name(),"attempts":d.attempts.to_string(),"observation":expected_observation,
                "operator":operator,"reason":reason,"at":now.to_string()});
                let hash = digest(&event)?;
                let mut m = mutation(s, event.clone())?;
                m.quarantine = Some((intention_id.into(), hash.clone(), bytes(&event)?));
                Ok((hash, m))
            }
        )
    }
    pub async fn resume(&self, report_digest: &str, now: i64) -> Result<()> {
        transact!(self, Query::Control, 60, async |tx, mut s: Snapshot| {
            if !s.installation.dispatch_hold || s.installation.admission != "open" {
                return Err(Error::Held);
            }
            let (hash, body) = s.report.as_ref().ok_or(Error::StaleReport)?;
            let v: Value = serde_json::from_slice(body).map_err(|_| Error::Integrity)?;
            if hash != report_digest || digest(&v)? != *hash || v["version"] != 2 {
                return Err(Error::StaleReport);
            }
            let mut fingerprint = fingerprint_start(&s)?;
            let mut after = String::new();
            loop {
                let page = tx
                    .load_outbox(Query::page(&after))
                    .await
                    .map_err(store_error)?;
                if page.items.is_empty() {
                    break;
                }
                for (i, d) in &page.items {
                    fingerprint = fingerprint_item(&fingerprint, i, d)?;
                }
                after = page.items.last().unwrap().0.id.clone();
            }
            if v["fingerprint"] != fingerprint
                || v["inventory"]
                    != json!(self
                        .destination
                        .inventory_digest(&s.installation.logical_store_id, Mode::Normal)?)
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
    /// Complete small-installation query, retaining the original 1,000-row bound.
    /// Large callers must use deliveries_after; control operations have no row cap.
    pub async fn deliveries(&self) -> Result<Vec<Delivery>> {
        match &self.ledger.store {
            Backend::Sqlite(s) => read_deliveries(s).await,
            Backend::Postgres(s) => read_deliveries(s).await,
        }
    }
    /// At most 64 rows / 8 MiB of intention input. Continue after the last returned ID
    /// until empty; use a reconciliation report for an atomic complete-set assessment.
    pub async fn deliveries_after(&self, after: &str) -> Result<Vec<Delivery>> {
        Ok(snapshot!(self, Query::page(after))?
            .items
            .into_iter()
            .map(|(_, d)| d)
            .collect())
    }
}

fn current_attempt(s: &Snapshot, a: &Attempt, now: i64) -> bool {
    now < a.until
        && s.items.iter().any(|(i, d)| {
            i.id == a.request.key
                && d.state == State::Leased
                && d.quarantine.is_none()
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
