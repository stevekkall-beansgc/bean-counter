//! Owning M5 presentation artifacts. All Value copies are admitted before copy
//! and retain their leases through append; the output is still only DATA.
use super::*;
use crate::store::sqlite::{m5, m5_workflow};
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use sha2::{Digest, Sha256};
fn core(e: ledgerlab_core::Error) -> ServiceError {
    crate::service::workflow::core_error(e)
}
pub(super) struct Object {
    pub value: Value,
    owners: Vec<ExternalBufferLease>,
    _controls: ExternalBufferLease,
}
impl Object {
    fn new(template: &[u8], s: &mut OriginalAuditSession) -> Result<Self, ServiceError> {
        let source = s
            .parse_representation(template, RepresentationKind::Record)
            .map_err(core)?;
        let count = source
            .value()
            .as_object()
            .ok_or(ServiceError::IntegrityFailure)?
            .len()
            .checked_mul(2)
            .and_then(|v| v.checked_add(4))
            .ok_or(ServiceError::Unavailable)?;
        let controls = s
            .reserve_external_payload(
                count
                    .checked_mul(std::mem::size_of::<ExternalBufferLease>())
                    .ok_or(ServiceError::Unavailable)?,
            )
            .map_err(core)?;
        let mut owners = Vec::new();
        owners
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        service::require(owners.capacity() <= count, "INTEGRITY_FAILURE")?;
        let bound = {
            let (a, w) = s.parts();
            a.value_copy_payload_bound(source.value(), w)
                .map_err(core)?
        };
        owners.push(s.reserve_external_payload(bound).map_err(core)?);
        Ok(Self {
            value: source.value().clone(),
            owners,
            _controls: controls,
        })
    }
    fn replace(
        &mut self,
        name: &str,
        value: Value,
        owner: ExternalBufferLease,
        s: &mut OriginalAuditSession,
    ) -> Result<(), ServiceError> {
        s.work().visit(Visit::Constraint).map_err(core)?;
        service::require(
            self.owners.len() < self.owners.capacity(),
            "INTEGRITY_FAILURE",
        )?;
        for (k, slot) in self
            .value
            .as_object_mut()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.work().visit(Visit::Field).map_err(core)?;
            s.work().scan_bytes(k.len() + name.len()).map_err(core)?;
            if k == name {
                *slot = value;
                self.owners.push(owner);
                return Ok(());
            }
        }
        Err(ServiceError::IntegrityFailure)
    }
    fn set(
        &mut self,
        name: &str,
        value: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<(), ServiceError> {
        let bound = {
            let (a, w) = s.parts();
            a.value_copy_payload_bound(value, w).map_err(core)?
        };
        let owner = s.reserve_external_payload(bound).map_err(core)?;
        self.replace(name, value.clone(), owner, s)
    }
    fn text(
        &mut self,
        name: &str,
        text: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<(), ServiceError> {
        let owner = s
            .reserve_external_payload(text.len() + std::mem::size_of::<Value>())
            .map_err(core)?;
        s.work().scan_bytes(text.len()).map_err(core)?;
        s.work().write_bytes(text.len()).map_err(core)?;
        self.replace(name, Value::String(text.to_owned()), owner, s)
    }
    fn counter(
        &mut self,
        name: &str,
        n: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<(), ServiceError> {
        let _temporary = s.reserve_external_payload(20).map_err(core)?;
        s.work().write_bytes(20).map_err(core)?;
        self.text(name, &n.to_string(), s)
    }
    fn encoded(&self, s: &mut OriginalAuditSession) -> Result<AuditedBytes, ServiceError> {
        let b = s
            .canonical_representation(&self.value, RepresentationKind::Record)
            .map_err(core)?;
        service::require(b.bytes().len() <= 262144, "BILLING_M5_BOUNDS")?;
        Ok(b)
    }
}
pub(super) struct Text {
    pub(super) value: String,
    _owner: ExternalBufferLease,
}
fn hash(prefix: &str, parts: &[&[u8]], s: &mut OriginalAuditSession) -> Result<Text, ServiceError> {
    let owner = s
        .reserve_external_payload(prefix.len() + 64 + std::mem::size_of::<String>())
        .map_err(core)?;
    let mut h = Sha256::new();
    for p in parts {
        s.work().scan_bytes(p.len()).map_err(core)?;
        s.work().hash_bytes(p.len()).map_err(core)?;
        h.update(p);
    }
    s.work().write_bytes(prefix.len() + 64).map_err(core)?;
    let mut value = String::with_capacity(prefix.len() + 64);
    value.push_str(prefix);
    const HEX: &[u8] = b"0123456789abcdef";
    for b in h.finalize() {
        value.push(HEX[(b >> 4) as usize] as char);
        value.push(HEX[(b & 15) as usize] as char);
    }
    Ok(Text {
        value,
        _owner: owner,
    })
}
pub(super) struct Child {
    pub key: AuditedBytes,
    pub payload: AuditedBytes,
    pub family: &'static str,
    pub source: Option<Text>,
}
pub(super) struct Artifacts {
    pub statement: Object,
    pub response: AuditedBytes,
    pub children: Vec<Child>,
    _controls: ExternalBufferLease,
}
fn record(
    identity: &[u8],
    key: &[u8],
    accepted: &str,
    command: i64,
    sequence: i64,
    digest: Option<&str>,
    s: &mut OriginalAuditSession,
) -> Result<Object, ServiceError> {
    let id = hash(
        "m5r_",
        &[b"bean-counter/m5/record-id/1\0", identity, b"\0", key],
        s,
    )?;
    let mut v = Object::new(
        if digest.is_some() {
            br#"{"accepted_at":null,"command_sequence":null,"payload_hash":null,"record_id":null,"sequence":null}"#
        } else {
            br#"{"accepted_at":null,"command_sequence":null,"record_id":null,"sequence":null}"#
        },
        s,
    )?;
    v.text("accepted_at", accepted, s)?;
    v.counter("command_sequence", command, s)?;
    v.text("record_id", &id.value, s)?;
    v.counter("sequence", sequence, s)?;
    if let Some(d) = digest {
        v.text("payload_hash", d, s)?
    }
    Ok(v)
}
#[allow(clippy::too_many_arguments)]
fn seal(
    mut value: Object,
    key: AuditedBytes,
    family: &'static str,
    identity: &[u8],
    accepted: &str,
    command: i64,
    sequence: i64,
    s: &mut OriginalAuditSession,
) -> Result<Child, ServiceError> {
    let envelope = record(identity, key.bytes(), accepted, command, sequence, None, s)?;
    value.set("record", &envelope.value, s)?;
    let unsigned = value.encoded(s)?;
    let digest = hash(
        "",
        &[
            b"bean-counter/m5/record/1\0",
            family.as_bytes(),
            b"\0",
            unsigned.bytes(),
        ],
        s,
    )?;
    let envelope = record(
        identity,
        key.bytes(),
        accepted,
        command,
        sequence,
        Some(&digest.value),
        s,
    )?;
    value.set("record", &envelope.value, s)?;
    let source = if family == "ledger-billing-presentation-claim/1" {
        let text = value.value["source"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let owner = s.reserve_external_payload(text.len()).map_err(core)?;
        s.work().scan_bytes(text.len()).map_err(core)?;
        s.work().write_bytes(text.len()).map_err(core)?;
        Some(Text {
            value: text.to_owned(),
            _owner: owner,
        })
    } else {
        None
    };
    Ok(Child {
        key,
        payload: value.encoded(s)?,
        family,
        source,
    })
}
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    identity: &[u8],
    customer: &str,
    period: &Value,
    scope: &Value,
    term: &Value,
    state: &m5::PeriodCloseState,
    start: i64,
    end: i64,
    accepted: &ledgerlab_core::domain::Timestamp,
    lines: &Value,
    totals: &Value,
    through: i64,
    included_records: &Value,
    s: &mut OriginalAuditSession,
) -> Result<Artifacts, ServiceError> {
    let resolution = state
        .resolution
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut boundary = Object::new(br#"{"commit_sequence":null,"scope":null,"store":null}"#, s)?;
    boundary.counter("commit_sequence", through, s)?;
    boundary.set("scope", &scope["scope"], s)?;
    boundary.set("store", &scope["store"], s)?;
    let mut statement=Object::new(br#"{"boundary_resolution_id":null,"boundary_rule_version":null,"close_acceptance_time":null,"complete":true,"currency":"USD","customer":null,"end_utc":null,"lines":null,"m3_high_water":null,"m5_high_water":null,"period_id":null,"scale":18,"schema":"ledger-billing-statement/5","snapshot_boundary_id":null,"start_utc":null,"statement_hash":null,"status":"closed","timezone":null,"timezone_rules_version":null,"totals":null,"workflow_boundary":null}"#,s)?;
    statement.text("boundary_resolution_id", &resolution.resolution_id, s)?;
    statement.set("boundary_rule_version", &term["boundary_rule_version"], s)?;
    statement.text("close_acceptance_time", accepted.as_str(), s)?;
    statement.text("customer", customer, s)?;
    let _times = s.reserve_external_payload(54).map_err(core)?;
    s.work().write_bytes(54).map_err(core)?;
    statement.text(
        "start_utc",
        ledgerlab_core::domain::Timestamp::from_micros(start)
            .map_err(core)?
            .as_str(),
        s,
    )?;
    statement.text(
        "end_utc",
        ledgerlab_core::domain::Timestamp::from_micros(end)
            .map_err(core)?
            .as_str(),
        s,
    )?;
    statement.set("lines", lines, s)?;
    statement.counter("m3_high_water", state.m3_high_water, s)?;
    statement.counter("m5_high_water", state.m5_high_water, s)?;
    statement.set("period_id", period, s)?;
    statement.counter("snapshot_boundary_id", state.snapshot_boundary_id, s)?;
    statement.set("timezone", &term["timezone"], s)?;
    statement.set("timezone_rules_version", &term["timezone_rules_version"], s)?;
    statement.set("totals", totals, s)?;
    statement.set("workflow_boundary", &boundary.value, s)?;
    finish(
        identity,
        customer,
        period,
        scope,
        None,
        accepted,
        lines,
        statement,
        included_records,
        &state.adjustments,
        state.command_sequence,
        state.first_record_sequence,
        s,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_ad_hoc(
    identity: &[u8],
    customer: &str,
    command_id: &str,
    scope: &Value,
    adjustments: &[m5::PresentableAdjustment],
    requested_adjustments: &Value,
    workflow_actions: &Value,
    position: (i64, i64, i64, i64, i64),
    accepted: &ledgerlab_core::domain::Timestamp,
    lines: &Value,
    totals: &Value,
    through: i64,
    s: &mut OriginalAuditSession,
) -> Result<Artifacts, ServiceError> {
    let mut boundary = Object::new(br#"{"commit_sequence":null,"scope":null,"store":null}"#, s)?;
    boundary.counter("commit_sequence", through, s)?;
    boundary.set("scope", &scope["scope"], s)?;
    boundary.set("store", &scope["store"], s)?;
    let mut statement=Object::new(br#"{"adjustments":null,"complete":true,"currency":"USD","customer":null,"lines":null,"m3_high_water":null,"m5_high_water":null,"scale":18,"schema":"ledger-billing-ad-hoc-statement-result/2","snapshot_boundary_id":null,"statement_hash":null,"statement_id":null,"status":"issued","totals":null,"workflow_actions":null,"workflow_boundary":null}"#,s)?;
    statement.text("customer", customer, s)?;
    statement.text("statement_id", command_id, s)?;
    statement.set("adjustments", requested_adjustments, s)?;
    statement.set("workflow_actions", workflow_actions, s)?;
    statement.set("lines", lines, s)?;
    statement.set("totals", totals, s)?;
    statement.set("workflow_boundary", &boundary.value, s)?;
    statement.counter("m3_high_water", position.2, s)?;
    statement.counter("m5_high_water", position.3, s)?;
    statement.counter("snapshot_boundary_id", position.4, s)?;
    finish(
        identity,
        customer,
        &Value::Null,
        scope,
        Some(command_id),
        accepted,
        lines,
        statement,
        &Value::Null,
        adjustments,
        position.0,
        position.1,
        s,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish(
    identity: &[u8],
    customer: &str,
    period: &Value,
    scope: &Value,
    command_id: Option<&str>,
    accepted: &ledgerlab_core::domain::Timestamp,
    lines: &Value,
    mut statement: Object,
    included_records: &Value,
    adjustments: &[m5::PresentableAdjustment],
    command_sequence: i64,
    first_record_sequence: i64,
    s: &mut OriginalAuditSession,
) -> Result<Artifacts, ServiceError> {
    // Hash the exact unsigned shape with no statement_hash member.
    let copy_bound = {
        let (a, w) = s.parts();
        a.value_copy_payload_bound(&statement.value, w)
            .map_err(core)?
    };
    let _unsigned_owner = s.reserve_external_payload(copy_bound).map_err(core)?;
    let mut unsigned = statement.value.clone();
    s.work().scan_bytes(21 * 32).map_err(core)?;
    unsigned
        .as_object_mut()
        .ok_or(ServiceError::IntegrityFailure)?
        .remove("statement_hash");
    let raw = s
        .canonical_representation(&unsigned, RepresentationKind::Record)
        .map_err(core)?;
    let domain: &[u8] = if command_id.is_some() {
        b"bean-counter/m5/ad-hoc-statement/2\0"
    } else {
        b"bean-counter/m5/statement/5\0"
    };
    let digest = hash("", &[domain, raw.bytes()], s)?;
    statement.text("statement_hash", &digest.value, s)?;
    let response = statement.encoded(s)?;
    let mut workflow_count = 0usize;
    for line in lines.as_array().ok_or(ServiceError::IntegrityFailure)? {
        s.work().visit(Visit::Element).map_err(core)?;
        if line.get("source_record_ref").is_some() {
            s.work()
                .arithmetic(Op::Add, &[&workflow_count.into(), &1usize.into()])
                .map_err(core)?;
            workflow_count = workflow_count
                .checked_add(1)
                .ok_or(ServiceError::Unavailable)?;
        }
    }
    s.work()
        .arithmetic(
            Op::Add,
            &[&workflow_count.into(), &adjustments.len().into()],
        )
        .map_err(core)?;
    s.work()
        .arithmetic(
            Op::Add,
            &[
                &workflow_count.into(),
                &adjustments.len().into(),
                &1usize.into(),
            ],
        )
        .map_err(core)?;
    let count = workflow_count
        .checked_add(adjustments.len())
        .and_then(|n| n.checked_add(1))
        .ok_or(ServiceError::Unavailable)?;
    let presentation = if command_id.is_some() {
        "ad-hoc"
    } else {
        "standard-period"
    };
    let statement_id = command_id.unwrap_or(&digest.value);
    s.work()
        .arithmetic(
            Op::Multiply,
            &[&count.into(), &std::mem::size_of::<Child>().into()],
        )
        .map_err(core)?;
    let controls = s
        .reserve_external_payload(
            count
                .checked_mul(std::mem::size_of::<Child>())
                .ok_or(ServiceError::Unavailable)?,
        )
        .map_err(core)?;
    let mut children = Vec::new();
    children
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    service::require(children.capacity() <= count, "INTEGRITY_FAILURE")?;
    // Child order is lexicographic canonical key: period-close before workflow
    // claims (same customer, key differs); construct then paid-sort and assign
    // record sequence only after complete ordering is known.
    let pending_controls = s
        .reserve_external_payload(
            count
                .checked_mul(std::mem::size_of::<(AuditedBytes, Object, &'static str)>())
                .ok_or(ServiceError::Unavailable)?,
        )
        .map_err(core)?;
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    service::require(pending.capacity() <= count, "INTEGRITY_FAILURE")?;
    let included_controls = s
        .reserve_external_payload(
            workflow_count
                .checked_mul(
                    std::mem::size_of::<Value>() + std::mem::size_of::<ExternalBufferLease>(),
                )
                .ok_or(ServiceError::Unavailable)?,
        )
        .map_err(core)?;
    let mut included_owners = Vec::new();
    let mut included = Vec::new();
    included
        .try_reserve_exact(workflow_count)
        .map_err(|_| ServiceError::Unavailable)?;
    included_owners
        .try_reserve_exact(workflow_count)
        .map_err(|_| ServiceError::Unavailable)?;
    s.work()
        .arithmetic(
            Op::Compare,
            &[
                &included.capacity().into(),
                &included_owners.capacity().into(),
                &workflow_count.into(),
            ],
        )
        .map_err(core)?;
    service::require(
        included.capacity() <= workflow_count && included_owners.capacity() <= workflow_count,
        "INTEGRITY_FAILURE",
    )?;
    for line in lines.as_array().ok_or(ServiceError::IntegrityFailure)? {
        s.work().visit(Visit::Element).map_err(core)?;
        let Some(reference) = line.get("source_record_ref") else {
            continue;
        };
        let bound = {
            let (a, w) = s.parts();
            a.value_copy_payload_bound(reference, w).map_err(core)?
        };
        included_owners.push(s.reserve_external_payload(bound).map_err(core)?);
        included.push(reference.clone());
        let mut key_inner = Object::new(br#"{"source_record_ref":null,"workflow_scope":null}"#, s)?;
        key_inner.set("source_record_ref", reference, s)?;
        key_inner.set("workflow_scope", scope, s)?;
        let mut key=Object::new(br#"{"customer":null,"key":null,"kind":"ledger-billing-workflow-presentation-claim/1","role":"workflow-presentation-claim"}"#,s)?;
        key.text("customer", customer, s)?;
        key.set("key", &key_inner.value, s)?;
        let mut claim=Object::new(br#"{"customer":null,"presentation_kind":"standard-period","record":null,"schema":"ledger-billing-workflow-presentation-claim/1","source_record_ref":null,"statement_hash":null,"statement_id":null,"workflow_scope":null}"#,s)?;
        claim.text("customer", customer, s)?;
        claim.set("source_record_ref", reference, s)?;
        claim.text("statement_hash", &digest.value, s)?;
        claim.text("statement_id", statement_id, s)?;
        claim.text("presentation_kind", presentation, s)?;
        claim.set("workflow_scope", scope, s)?;
        pending.push((key.encoded(s)?, claim, m5_workflow::CLAIM));
    }
    let included = Value::Array(included);
    for adjustment in adjustments {
        s.work().visit(Visit::Element).map_err(core)?;
        let mut inner = Object::new(br#"{"adjustment_id":null,"presentation_kind":null}"#, s)?;
        inner.text("adjustment_id", &adjustment.adjustment_id, s)?;
        inner.text("presentation_kind", presentation, s)?;
        let mut key=Object::new(br#"{"customer":null,"key":null,"kind":"ledger-billing-presentation-claim/1","role":"presentation-claim","source":null}"#,s)?;
        key.text("customer", customer, s)?;
        key.text("source", &adjustment.source, s)?;
        key.set("key", &inner.value, s)?;
        let mut claim=Object::new(br#"{"adjustment_id":null,"customer":null,"presentation_kind":null,"record":null,"schema":"ledger-billing-presentation-claim/1","source":null,"statement_id":null}"#,s)?;
        claim.text("adjustment_id", &adjustment.adjustment_id, s)?;
        claim.text("customer", customer, s)?;
        claim.text("presentation_kind", presentation, s)?;
        claim.text("source", &adjustment.source, s)?;
        claim.text("statement_id", statement_id, s)?;
        pending.push((
            key.encoded(s)?,
            claim,
            "ledger-billing-presentation-claim/1",
        ));
    }
    let (key, mut closed, family) = if let Some(command_id) = command_id {
        let mut inner = Object::new(br#"{"statement_id":null}"#, s)?;
        inner.text("statement_id", command_id, s)?;
        let mut key=Object::new(br#"{"customer":null,"key":null,"kind":"ledger-billing-ad-hoc-statement-record/2","role":"ad-hoc-statement"}"#,s)?;
        key.text("customer", customer, s)?;
        key.set("key", &inner.value, s)?;
        let record=Object::new(br#"{"adjustments":null,"customer":null,"lines":null,"m3_high_water":null,"m5_high_water":null,"record":null,"schema":"ledger-billing-ad-hoc-statement-record/2","snapshot_boundary_id":null,"statement_hash":null,"statement_id":null,"totals":null,"workflow_actions":null,"workflow_boundary":null}"#,s)?;
        (key, record, "ledger-billing-ad-hoc-statement-record/2")
    } else {
        let mut inner = Object::new(br#"{"period_id":null}"#, s)?;
        inner.set("period_id", period, s)?;
        let mut key=Object::new(br#"{"customer":null,"key":null,"kind":"ledger-billing-period-close-record/2","role":"period-close"}"#,s)?;
        key.text("customer", customer, s)?;
        key.set("key", &inner.value, s)?;
        let record=Object::new(br#"{"boundary_resolution_id":null,"customer":null,"end_utc":null,"included_records":null,"included_workflow_actions":null,"lines":null,"m3_high_water":null,"m5_high_water":null,"period_id":null,"record":null,"schema":"ledger-billing-period-close-record/2","snapshot_boundary_id":null,"start_utc":null,"statement_hash":null,"totals":null,"workflow_boundary":null}"#,s)?;
        (key, record, m5_workflow::RECORD)
    };
    for k in [
        "customer",
        "lines",
        "m3_high_water",
        "m5_high_water",
        "snapshot_boundary_id",
        "statement_hash",
        "totals",
        "workflow_boundary",
    ] {
        closed.set(k, &statement.value[k], s)?;
    }
    if command_id.is_some() {
        for k in ["statement_id", "workflow_actions"] {
            closed.set(k, &statement.value[k], s)?;
        }
        // Preserve the legacy record's complete customer/source/id references.
        let per =
            std::mem::size_of::<Value>() + std::mem::size_of::<(Object, ExternalBufferLease)>();
        s.work()
            .arithmetic(Op::Multiply, &[&adjustments.len().into(), &per.into()])
            .map_err(core)?;
        let bound = adjustments
            .len()
            .checked_mul(
                std::mem::size_of::<Value>() + std::mem::size_of::<(Object, ExternalBufferLease)>(),
            )
            .ok_or(ServiceError::Unavailable)?;
        let _refs_controls = s.reserve_external_payload(bound).map_err(core)?;
        s.work().write_bytes(bound).map_err(core)?;
        let mut owners = Vec::new();
        let mut refs = Vec::new();
        refs.try_reserve_exact(adjustments.len())
            .map_err(|_| ServiceError::Unavailable)?;
        owners
            .try_reserve_exact(adjustments.len())
            .map_err(|_| ServiceError::Unavailable)?;
        s.work()
            .arithmetic(
                Op::Compare,
                &[
                    &refs.capacity().into(),
                    &owners.capacity().into(),
                    &adjustments.len().into(),
                ],
            )
            .map_err(core)?;
        service::require(
            refs.capacity() <= adjustments.len() && owners.capacity() <= adjustments.len(),
            "INTEGRITY_FAILURE",
        )?;
        for adjustment in adjustments {
            s.work().visit(Visit::Element).map_err(core)?;
            let mut r = Object::new(
                br#"{"adjustment_id":null,"customer":null,"source":null}"#,
                s,
            )?;
            r.text("adjustment_id", &adjustment.adjustment_id, s)?;
            r.text("customer", customer, s)?;
            r.text("source", &adjustment.source, s)?;
            // Object retains its admitted copy while the borrowed array is copied.
            let b = {
                let (a, w) = s.parts();
                a.value_copy_payload_bound(&r.value, w).map_err(core)?
            };
            let o = s.reserve_external_payload(b).map_err(core)?;
            refs.push(r.value.clone());
            owners.push((r, o));
        }
        let refs = Value::Array(refs);
        closed.set("adjustments", &refs, s)?;
        drop(refs);
        drop(owners);
    } else {
        for k in [
            "boundary_resolution_id",
            "end_utc",
            "period_id",
            "start_utc",
        ] {
            closed.set(k, &statement.value[k], s)?;
        }
        closed.set("included_records", included_records, s)?;
        closed.set("included_workflow_actions", &included, s)?;
    }
    pending.push((key.encoded(s)?, closed, family));
    // Bounded insertion sort with every actual comparison and move paid.
    for i in 1..pending.len() {
        let mut j = i;
        while j > 0 {
            s.work().visit(Visit::MemberCompare).map_err(core)?;
            s.work()
                .scan_bytes(pending[j - 1].0.bytes().len() + pending[j].0.bytes().len())
                .map_err(core)?;
            if pending[j - 1].0.bytes() < pending[j].0.bytes() {
                break;
            }
            service::require(
                pending[j - 1].0.bytes() != pending[j].0.bytes(),
                "INTEGRITY_FAILURE",
            )?;
            s.work().visit(Visit::SortMove).map_err(core)?;
            s.work()
                .write_bytes(2 * std::mem::size_of::<(AuditedBytes, Object, &str)>())
                .map_err(core)?;
            pending.swap(j - 1, j);
            j -= 1;
        }
    }
    for (i, (key, value, family)) in pending.into_iter().enumerate() {
        let sequence = first_record_sequence
            .checked_add(i as i64)
            .ok_or(ServiceError::Unavailable)?;
        children.push(seal(
            value,
            key,
            family,
            identity,
            accepted.as_str(),
            command_sequence,
            sequence,
            s,
        )?);
    }
    drop(included);
    drop(included_owners);
    drop(included_controls);
    drop(pending_controls);
    Ok(Artifacts {
        statement,
        response,
        children,
        _controls: controls,
    })
}
