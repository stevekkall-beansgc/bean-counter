//! Private local comparison acquisition. Historical verification grants no
//! present read rights; current read permission grants no acceptance capability.
#![allow(dead_code)]
use super::retained::{self, base as b};
use crate::store::{
    comparison::*,
    outcomes::{OutcomeLockClass, OutcomeSnapshot},
};
use ledgerlab_core::{canonical, policy::chaining::outcomes as o};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    sync::Semaphore,
    time::{Duration, Instant},
};

static ADMISSION: Semaphore = Semaphore::const_new(1);
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const WORK_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const SEMANTICS: &str = "ledgerlab-private-comparison/1";
pub(crate) const ASSUMPTIONS: &str = "Fixed booked final base, original parties, currency/scale, membership, evidence, windows, limits, capacity, authority assumptions and complete activity. Amounts substituted for comparison; no assent, eligibility change or posting authorized. No behavioral forecast.";

#[derive(Clone, Default)]
pub(crate) struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    fn check(&self, deadline: Instant) -> Result<(), ComparisonError> {
        if self.0.load(Ordering::Relaxed) {
            Err(ComparisonError::Cancelled)
        } else if Instant::now() >= deadline {
            Err(ComparisonError::Deadline)
        } else {
            Ok(())
        }
    }
}
/// Outer-operation guard: retain it through all candidate work and report assembly.
/// The global permit cannot be duplicated by cloning readers or workspaces.
pub(crate) struct ComparisonOperation {
    _permit: tokio::sync::SemaphorePermit<'static>,
    cancellation: Cancellation,
    deadline: Instant,
}
impl ComparisonOperation {
    pub fn begin(cancellation: Cancellation) -> Result<Self, ComparisonError> {
        let permit = ADMISSION.try_acquire().map_err(|_| ComparisonError::Busy)?;
        let deadline = Instant::now() + WORK_TIMEOUT;
        cancellation.check(deadline)?;
        Ok(Self {
            _permit: permit,
            cancellation,
            deadline,
        })
    }
    /// The outer async candidate driver calls this between advances, allowing
    /// timer and cancellation tasks on the same runtime to make progress.
    pub async fn yield_and_checkpoint(&self) -> Result<(), ComparisonError> {
        self.checkpoint()?;
        tokio::task::yield_now().await;
        self.checkpoint()
    }
    pub fn checkpoint(&self) -> Result<(), ComparisonError> {
        self.cancellation.check(self.deadline)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComparisonError {
    Denied,
    Busy,
    Deadline,
    Cancelled,
    Limit,
    Integrity,
    Unavailable,
    NotFound,
    SnapshotChanged,
}
impl From<ReadError> for ComparisonError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Unavailable => Self::Unavailable,
            ReadError::Deadline => Self::Deadline,
            ReadError::Cancelled => Self::Cancelled,
            ReadError::Limit => Self::Limit,
            ReadError::Integrity => Self::Integrity,
            ReadError::NotFound => Self::NotFound,
        }
    }
}
/// Mandatory trusted host seam. No permissive implementation or write flag.
/// Scope verification happens before a target lookup/error. Disclosure verifies
/// every linked/supplier record and binding, not merely the request's source.
/// Permission is explicitly as of this read snapshot; no persistent grant cache.
pub(crate) trait ComparisonReadAuthority: Send + Sync {
    fn verify_scope(
        &self,
        who: &AuthenticatedReadContext,
        selection: &RetainedSelection,
        observation: &ReadAuthorityObservation,
    ) -> Result<(), ComparisonError>;
    fn verify_disclosure(
        &self,
        who: &AuthenticatedReadContext,
        selection: &RetainedSelection,
        observation: &ReadAuthorityObservation,
        snapshot: &RawRetainedSnapshot,
    ) -> Result<(), ComparisonError>;
}
/// Owned, immutable and detached. No connection, clock, callback, authority proof
/// or posting plan survives acquisition. Only verified construction is possible.
pub(crate) struct ComparisonWorkspace {
    history: retained::Registered,
    fingerprint: SnapshotFingerprint,
    activity_fingerprint: String,
    selection: RetainedSelection,
    original_receipts: Vec<(String, Option<String>)>,
}
impl ComparisonWorkspace {
    pub fn target(&self) -> &o::Target {
        &self.history.base.target
    }
    pub fn decisions(&self) -> &[o::Decision] {
        &self.history.decisions
    }
    pub fn economic_rows(&self) -> &[Value] {
        &self.history.records.rows
    }
    pub fn settlement_rows(&self) -> &[Value] {
        &self.history.settlement
    }
    pub fn fingerprint(&self) -> &SnapshotFingerprint {
        &self.fingerprint
    }
    pub fn activity_fingerprint(&self) -> &str {
        &self.activity_fingerprint
    }
    pub fn selection(&self) -> &RetainedSelection {
        &self.selection
    }
    pub fn historical_receipts(&self) -> &[(String, Option<String>)] {
        &self.original_receipts
    }
}

pub(crate) async fn load_workspace<S: ComparisonReadStore, A: ComparisonReadAuthority>(
    reader: &S,
    authority: &A,
    who: &AuthenticatedReadContext,
    selection: RetainedSelection,
    operation: &ComparisonOperation,
) -> Result<ComparisonWorkspace, ComparisonError> {
    operation.checkpoint()?;
    let cancellation = &operation.cancellation;
    validate_selection(who, &selection)?;
    let deadline = (Instant::now() + READ_TIMEOUT).min(operation.deadline);
    cancellation.check(deadline)?;
    let mut tx = tokio::time::timeout_at(deadline, reader.begin_read(deadline))
        .await
        .map_err(|_| ComparisonError::Deadline)??;
    let result = tokio::time::timeout_at(deadline, async {
        let observed = tx.load_authority(who).await?;
        cancellation.check(deadline)?;
        let mut budget = validate_authority_bounds(&observed)?;
        if observed.scope != selection.scope {
            return Err(ComparisonError::Denied);
        }
        authority
            .verify_scope(who, &selection, &observed)
            .map_err(|_| ComparisonError::Denied)?;
        let raw = tx.load_retained(&selection).await?;
        cancellation.check(deadline)?;
        validate_raw_bounds_into(&raw, &mut budget)?;
        authority
            .verify_disclosure(who, &selection, &observed, &raw)
            .map_err(|_| ComparisonError::Denied)?;
        Ok(raw)
    })
    .await
    .map_err(|_| ComparisonError::Deadline)
    .and_then(|r| r);
    // Even denial/errors explicitly finish. If the deadline already elapsed,
    // timeout drops finish: the adapter must discard through its Drop guard.
    tokio::time::timeout_at(deadline, tx.finish())
        .await
        .map_err(|_| ComparisonError::Deadline)??;
    let raw = result?;
    cancellation.check(deadline)?;
    verify_workspace(raw, selection, cancellation, operation.deadline)
}
fn validate_selection(
    who: &AuthenticatedReadContext,
    s: &RetainedSelection,
) -> Result<(), ComparisonError> {
    if who.scope != s.scope {
        return Err(ComparisonError::Denied);
    }
    for v in s.scope.iter().chain([
        &s.target,
        &s.invocation_id,
        &who.principal_id,
        &who.authority_head,
    ]) {
        if v.is_empty() || v.len() > 128 || v.chars().any(char::is_control) {
            return Err(ComparisonError::Denied);
        }
    }
    Ok(())
}
fn check(v: bool) -> Result<(), ComparisonError> {
    if v {
        Ok(())
    } else {
        Err(ComparisonError::Integrity)
    }
}
fn limit(v: bool) -> Result<(), ComparisonError> {
    if v {
        Ok(())
    } else {
        Err(ComparisonError::Limit)
    }
}
fn integrity<T>(v: Result<T, crate::ServiceError>) -> Result<T, ComparisonError> {
    v.map_err(|_| ComparisonError::Integrity)
}
fn validate_authority_bounds(a: &ReadAuthorityObservation) -> Result<ReadBudget, ComparisonError> {
    limit(a.heads.len() <= MAX_HEADS && a.records.len() <= MAX_REFERENCES)?;
    let mut budget = ReadBudget::default();
    for raw in &a.records {
        limit(raw.len() <= MAX_ENVELOPE_BYTES)?;
        budget.charge(raw.len())?;
    }
    for h in &a.heads {
        limit(h.lock.key.len() <= 16 * 1024 && h.revision.as_ref().is_none_or(|r| r.len() <= 19))?;
        budget.charge(h.lock.key.len())?;
        if let Some(v) = &h.value {
            limit(v.len() <= 256 * 1024)?;
            budget.charge(v.len())?;
        }
    }
    Ok(budget)
}
fn validate_raw_bounds(r: &RawRetainedSnapshot) -> Result<(), ComparisonError> {
    validate_raw_bounds_into(r, &mut ReadBudget::default())
}
fn validate_raw_bounds_into(
    r: &RawRetainedSnapshot,
    budget: &mut ReadBudget,
) -> Result<(), ComparisonError> {
    limit(
        r.records.len() <= MAX_RECORDS
            && r.members.len() <= MAX_RECORDS
            && r.anchors.len() <= 2
            && r.heads.len() <= MAX_HEADS
            && r.original_deliveries.len() <= MAX_STEPS + 1,
    )?;
    limit(!r.store_identity.is_empty() && r.store_identity.len() <= 128)?;
    for raw in &r.records {
        limit(raw.len() <= MAX_ENVELOPE_BYTES)?;
        budget.charge(raw.len())?;
    }
    for rf in r.anchors.iter().chain(&r.members) {
        limit(
            rf.id.len() <= 4096
                && rf.kind.len() <= 128
                && rf.content_hash.len() <= 128
                && rf.scope.iter().all(|s| s.len() <= 128),
        )?;
        budget.charge(
            rf.id.len()
                + rf.kind.len()
                + rf.content_hash.len()
                + rf.scope.iter().map(String::len).sum::<usize>(),
        )?;
    }
    for h in &r.heads {
        limit(h.lock.key.len() <= 16 * 1024 && h.revision.as_ref().is_none_or(|r| r.len() <= 19))?;
        budget.charge(h.lock.key.len())?;
        if let Some(v) = &h.value {
            limit(v.len() <= 256 * 1024)?;
            budget.charge(v.len())?;
        }
    }
    for d in &r.original_deliveries {
        for raw in [&d.command, &d.ingress, &d.settlement_receipt]
            .into_iter()
            .chain(d.economic_receipt.iter())
        {
            limit(raw.len() <= MAX_ENVELOPE_BYTES)?;
            budget.charge(raw.len())?;
        }
        for s in d.key.scope.iter().chain(&d.canonical_key.scope).chain([
            &d.key.source,
            &d.key.external_id,
            &d.canonical_key.source,
            &d.canonical_key.external_id,
            &d.ingress_hash,
        ]) {
            limit(s.len() <= 256)?;
            budget.charge(s.len())?;
        }
    }
    Ok(())
}
fn verify_workspace(
    raw: RawRetainedSnapshot,
    selection: RetainedSelection,
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<ComparisonWorkspace, ComparisonError> {
    validate_raw_bounds(&raw)?;
    let mut all = integrity(b::Records::new(&[], json!(selection.scope)))?;
    for envelope in &raw.records {
        cancellation.check(deadline)?;
        let row = canonical::outcome::decode(envelope).map_err(|_| ComparisonError::Integrity)?;
        integrity(all.insert(row))?;
    }
    integrity(all.documents())?;
    let mut members = BTreeSet::new();
    for member in &raw.members {
        check(member.scope == selection.scope)?;
        let rf = integrity(retained::ref_value(member))?;
        integrity(all.deref(&rf))?;
        check(members.insert(integrity(b::bytes(&rf))?))?;
    }
    check(members.len() == all.rows.len())?;
    let mut seen = BTreeSet::new();
    for h in &raw.heads {
        cancellation.check(deadline)?;
        let key = canonical::parse(&h.lock.key).map_err(|_| ComparisonError::Integrity)?;
        check(key[0] == json!(selection.scope) && integrity(b::bytes(&key))? == h.lock.key)?;
        check(seen.insert((h.lock.class, h.lock.key.clone())))?;
        integrity(retained::head_value(h))?;
    }
    let snapshot = OutcomeSnapshot {
        anchors: raw.anchors,
        records: Vec::new(),
        heads: raw.heads,
    };
    let hsel = retained::HistorySelection {
        scope: selection.scope.clone(),
        target: &selection.target,
        invocation_id: &selection.invocation_id,
        required: &[],
    };
    let records = integrity(retained::historical_records(&snapshot, &hsel, &all))?;
    let econ = integrity(retained::economic_only(&records))?;
    let anchor = integrity(econ.one("base-acceptance"))?;
    let base = integrity(b::decode_base(&econ, &b::reference(anchor)))?;
    let observations = all
        .rows
        .iter()
        .filter(|r| r["kind"] == "reservation-observation")
        .count();
    limit(observations <= MAX_STEPS + 1)?;
    // Exactly the expected head set; reject surplus or foreign target observations.
    let mut expected = BTreeSet::new();
    let mut expect = |class, parts: Vec<Value>| -> Result<(), ComparisonError> {
        let mut key = vec![json!(selection.scope)];
        key.extend(parts);
        expected.insert((class, integrity(b::bytes(&json!(key)))?));
        Ok(())
    };
    use OutcomeLockClass as L;
    expect(L::Target, vec![json!(selection.target)])?;
    expect(L::Reservation, vec![json!(selection.invocation_id)])?;
    expect(
        L::InvocationConsumption,
        vec![json!(selection.invocation_id)],
    )?;
    expect(L::BaseReversal, vec![json!(selection.target)])?;
    for binding in econ.rows.iter().filter(|r| r["kind"] == "binding-snapshot") {
        expect(L::Binding, vec![binding["body"]["binding_id"].clone()])?;
        expect(
            L::BindingAggregate,
            vec![
                json!(selection.target),
                binding["body"]["binding_id"].clone(),
            ],
        )?;
    }
    for f in &base.target.policy().families {
        let binding = base
            .evaluation
            .bundle()
            .policies()
            .iter()
            .find(|p| p.binding.id == f.binding_id)
            .ok_or(ComparisonError::Integrity)?;
        expect(
            L::Claim,
            vec![
                json!(binding.binding.agreement),
                json!(f.family),
                json!(selection.target),
            ],
        )?;
    }
    check(seen == expected)?;
    // Immutable-prefix heads have exact revision meanings. Current binding
    // selector revisions remain observations and cannot reprice this history.
    for h in &snapshot.heads {
        let key = canonical::parse(&h.lock.key).map_err(|_| ComparisonError::Integrity)?;
        let expected_revision = match h.lock.class {
            L::Claim => {
                if let Some(value) = integrity(retained::head_value(h))? {
                    let row = integrity(econ.deref(&value["revision"]))?;
                    Some(integrity(b::text(&row["body"]["number"]))?.to_owned())
                } else {
                    None
                }
            }
            L::BindingAggregate => Some(
                econ.rows
                    .iter()
                    .filter(|r| r["kind"] == "claim-revision" && r["body"]["binding_id"] == key[2])
                    .count()
                    .to_string(),
            ),
            L::InvocationConsumption | L::BaseReversal => Some("0".into()),
            _ => continue,
        };
        check(h.revision == expected_revision)?;
    }
    let mut interrupted = None;
    let history =
        retained::validate_registered_checked(&snapshot, &hsel, records, econ, base, &mut || {
            cancellation.check(deadline).map_err(|e| {
                interrupted = Some(e);
                crate::ServiceError::Unavailable
            })
        })
        .map_err(|_| interrupted.unwrap_or(ComparisonError::Integrity))?;
    let mut receipts = BTreeSet::new();
    let mut original_receipts = vec![];
    for d in &raw.original_deliveries {
        cancellation.check(deadline)?;
        check(d.key == d.canonical_key && d.key.scope == selection.scope)?;
        integrity(retained::validate_delivery(d, &all))?;
        let settle = canonical::outcome::decode(&d.settlement_receipt)
            .map_err(|_| ComparisonError::Integrity)?;
        let observation = integrity(all.deref(&settle["body"]["observation"]))?;
        check(observation["body"]["command"]["invocation_id"] == selection.invocation_id)?;
        let id = integrity(b::text(&settle["id"]))?.to_owned();
        check(receipts.insert(id.clone()))?;
        let economic = d
            .economic_receipt
            .as_ref()
            .map(|raw| {
                let e = canonical::outcome::decode(raw).map_err(|_| ComparisonError::Integrity)?;
                Ok::<_, ComparisonError>(integrity(b::text(&e["id"]))?.to_owned())
            })
            .transpose()?;
        original_receipts.push((id, economic));
    }
    let required_receipts = history
        .settlement
        .iter()
        .filter(|r| r["kind"] == "reservation-receipt")
        .map(|r| integrity(b::text(&r["id"])).map(str::to_owned))
        .collect::<Result<BTreeSet<_>, _>>()?;
    check(receipts == required_receipts)?;
    original_receipts.sort();
    let mut heads = snapshot
        .heads
        .iter()
        .map(|h| -> Result<Value, ComparisonError> {
            Ok(json!([
                format!("{:?}", h.lock.class),
                canonical::parse(&h.lock.key).map_err(|_| ComparisonError::Integrity)?,
                h.revision,
                integrity(retained::head_value(h))?
            ]))
        })
        .collect::<Result<Vec<_>, _>>()?;
    heads.sort_by_key(|v| b::bytes(v).expect("already validated bounded descriptor"));
    let anchors = snapshot
        .anchors
        .iter()
        .map(retained::ref_value)
        .collect::<b::Result<Vec<_>>>();
    let descriptor = json!({"semantics":SEMANTICS,"store":raw.store_identity,"scope":selection.scope,"target":selection.target,"invocation":selection.invocation_id,"anchors":integrity(b::ordered(integrity(anchors)?))?,"members":members.into_iter().map(|v| canonical::parse(&v)).collect::<ledgerlab_core::Result<Vec<_>>>().map_err(|_|ComparisonError::Integrity)?,"heads":heads,"historical_receipts":original_receipts});
    let activity_fingerprint = provenance_digest(
        "activity",
        &json!({
            "semantics":SEMANTICS,"scope":selection.scope,"target":selection.target,
            "invocation":selection.invocation_id,"base":b::reference(&history.base.acceptance),
            "economic_members":integrity(b::members(&history.records.rows))?,
            "settlement_chronology":history.settlement.iter().map(b::reference).collect::<Vec<_>>()
        }),
    )?;
    let fingerprint = SnapshotFingerprint(provenance_digest("snapshot", &descriptor)?);
    if selection
        .expected_snapshot
        .as_ref()
        .is_some_and(|expected| *expected != fingerprint)
    {
        return Err(ComparisonError::SnapshotChanged);
    }
    cancellation.check(deadline)?;
    Ok(ComparisonWorkspace {
        history,
        fingerprint,
        activity_fingerprint,
        selection,
        original_receipts,
    })
}

/// Candidate lengths must be measured with a bounded writer before cloning or
/// parsing a caller's amount table. No aggregate candidate buffer is needed.
pub(crate) fn admit_candidates(lengths: &[usize]) -> Result<(), ComparisonError> {
    limit((2..=8).contains(&lengths.len()))?;
    let mut total = 0usize;
    for n in lengths {
        limit(*n <= MAX_CANDIDATE_BYTES)?;
        total = total.checked_add(*n).ok_or(ComparisonError::Limit)?;
        limit(total <= MAX_CANDIDATES_BYTES)?;
    }
    Ok(())
}
/// A streaming size gate for report assembly; never partially return a report.
pub(crate) struct BoundedReport {
    bytes: Vec<u8>,
    failed: bool,
}
impl BoundedReport {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            failed: false,
        }
    }
    pub fn finish(self) -> Result<Vec<u8>, ComparisonError> {
        if self.failed {
            Err(ComparisonError::Limit)
        } else {
            Ok(self.bytes)
        }
    }
}
impl std::io::Write for BoundedReport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.failed
            || self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > MAX_REPORT_BYTES)
        {
            self.failed = true;
            return Err(std::io::Error::other("comparison report limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;

fn provenance_digest(domain: &str, descriptor: &Value) -> Result<String, ComparisonError> {
    // Bound escaped serialized length before the canonical encoder allocates.
    let mut bounded = BoundedReport::new();
    serde_json::to_writer(&mut bounded, descriptor).map_err(|_| ComparisonError::Limit)?;
    drop(bounded);
    let bytes =
        canonical::CanonicalBytes::from_value(descriptor).map_err(|_| ComparisonError::Limit)?;
    ledgerlab_core::policy::chaining::comparison_provenance::provenance_digest(
        domain,
        bytes.as_slice(),
    )
    .map_err(|_| ComparisonError::Integrity)
}
/// Typed provenance only. Candidate numeric report assembly is integration-owned.
/// No serialized ledger envelope or receipt is generated here.
pub(crate) struct ReportProvenance {
    snapshot: SnapshotFingerprint,
    activity: String,
    candidates: Vec<String>,
    source_build: String,
    digest: String,
}
impl ReportProvenance {
    pub fn new(
        workspace: &ComparisonWorkspace,
        candidates: Vec<String>,
        source_build: String,
    ) -> Result<Self, ComparisonError> {
        limit(
            (2..=8).contains(&candidates.len())
                && !source_build.is_empty()
                && source_build.len() <= 128,
        )?;
        let activity = workspace.activity_fingerprint.clone();
        let is_digest = |hex: &str| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        check(is_digest(&activity) && candidates.iter().all(|s| is_digest(s)))?;
        let digest = provenance_digest(
            "report",
            &json!({"semantics":SEMANTICS,"snapshot":workspace.fingerprint.0,"activity":activity,"draft_candidates":candidates,"source_build":source_build,"assumptions":ASSUMPTIONS,"committed":false}),
        )?;
        Ok(Self {
            snapshot: workspace.fingerprint.clone(),
            activity,
            candidates,
            source_build,
            digest,
        })
    }
    pub fn committed(&self) -> bool {
        false
    }
    pub fn descriptor(&self) -> Value {
        json!({"semantics":SEMANTICS,"snapshot":self.snapshot.0,"activity":self.activity,"draft_candidates":self.candidates,"source_build":self.source_build,"assumptions":ASSUMPTIONS,"committed":false,"report_provenance":self.digest})
    }
}

#[path = "comparison_run.rs"]
pub(crate) mod foundation;
