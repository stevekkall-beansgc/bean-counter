//! Shared activation data normalization and reconstruction. These helpers never
//! authenticate an actor, select a schedule, or confer append permission.
use super::super::request::OperationKey;
use super::*;
use crate::service::workflow::core_error;
mod authority;
mod certificate;
pub(super) mod schedule;
pub(super) mod shape;
use crate::store::sqlite::workflow::agreement::acceptance::{
    ActivationAction, ActivationProjection, AgreementHead,
};
pub(super) use authority::{source_observation, stop_evidence_dependencies, verify_capture};
pub(super) use certificate::retained_local;
use ledgerlab_core::policy::workflow::AgreementRef;

pub(super) struct TransitionMetadata<'a> {
    pub(super) key: &'a OperationKey,
    pub(super) actor: &'a str,
    pub(super) at: &'a Timestamp,
    pub(super) sequence: i64,
    pub(super) binding: &'a Document,
    pub(super) prior: Option<RecordRef>,
    pub(super) target: Option<RecordRef>,
}
pub(super) struct PreparedTransitionRecords {
    pub(super) documents: Vec<Document>,
    pub(super) results: Vec<RecordRef>,
    pub(super) projection: Projection,
}
fn checked_ref(v: &Value, kind: &str) -> Result<RecordRef> {
    let r = AgreementRef::parse(&records::bytes(v)?).map_err(typed_error)?;
    require(r.kind() == kind, "INVALID_REFERENCE")?;
    Ok(RecordRef {
        kind: r.kind().into(),
        id: r.id().into(),
        digest: r.digest().into(),
    })
}
fn counter(v: &Value) -> Result<String> {
    normalize_counter(
        v.as_str().ok_or_else(|| reject("INVALID_SHAPE"))?,
        &mut ledgerlab_core::validation_work::NoopValidationWork,
    )
}
/// SAME scalar language for the legacy and owning activation normalizer.
/// Caller retains the finite20-byte decimal result and parse scratch lease.
pub(super) fn normalize_counter(
    raw: &str,
    work: &mut dyn ledgerlab_core::validation_work::ValidationWork,
) -> Result<String> {
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op, ValidationVisit as Visit};
    work.visit(Visit::Constraint).map_err(core_error)?;
    work.arithmetic(Op::Compare, &[&raw.len().into(), &0usize.into()])
        .map_err(core_error)?;
    require(!raw.is_empty(), "INVALID_SHAPE")?;
    work.arithmetic(Op::Compare, &[&raw.len().into(), &64usize.into()])
        .map_err(core_error)?;
    require(raw.len() <= 64, "INVALID_SHAPE")?;
    work.scan_bytes(raw.len()).map_err(core_error)?;
    for b in raw.bytes() {
        work.visit(Visit::Constraint).map_err(core_error)?;
        require(b.is_ascii_digit(), "INVALID_SHAPE")?;
    }
    let mut n = 0i64;
    for b in raw.bytes() {
        work.visit(Visit::ScalarDigit).map_err(core_error)?;
        work.scan_bytes(1).map_err(core_error)?;
        require(b.is_ascii_digit(), "INVALID_SHAPE")?;
        work.arithmetic(Op::Subtract, &[&b.into(), &b'0'.into()])
            .map_err(core_error)?;
        let digit = i64::from(b - b'0');
        work.arithmetic(Op::Multiply, &[&n.into(), &10i64.into()])
            .map_err(core_error)?;
        n = n.checked_mul(10).ok_or_else(|| reject("INVALID_SCALAR"))?;
        work.arithmetic(Op::Add, &[&n.into(), &digit.into()])
            .map_err(core_error)?;
        n = n
            .checked_add(digit)
            .ok_or_else(|| reject("INVALID_SCALAR"))?;
    }
    work.scan_bytes(20).map_err(core_error)?;
    work.write_bytes(20).map_err(core_error)?;
    Ok(n.to_string())
}

/// Binding bytes have already been authenticated by the coordinator or saved
/// certificate. Binding has no scope field; its accepted-agreement/operation
/// provenance and the scoped head authenticate scope at the host boundary.
/// Parsing/copy comparison here does not authenticate membership.
pub(super) fn normalize(req: &request::Request, binding: &Document) -> Result<Value> {
    let mut p = req.payload().clone();
    let schema = text(&p, "schema")?;
    let shape = shape::select(req.kind(), schema, |a, b| Ok(a == b))?;
    let action = shape.action;
    exact_keys(&p, shape.fields).map_err(|_| reject("INVALID_SHAPE"))?;
    let b = canonical::parse_bounded(&binding.bytes, 262144)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    require(
        binding.reference.kind == "workflow-binding" && b["schema"] == "ledger-workflow-binding/1",
        "INVALID_REFERENCE",
    )?;
    require(
        p["agreement_id"] == b["agreement_id"] && p["agreement_id"].as_str().is_some(),
        "INVALID_REFERENCE",
    )?;
    require(
        checked_ref(&p["accepted_binding_ref"], "workflow-binding")? == binding.reference,
        "INVALID_REFERENCE",
    )?;
    p["expected_revision"] = json!(counter(&p["expected_revision"])?);
    let mut evidence: Vec<RecordRef> =
        serde_json::from_value(p["evidence_refs"].clone()).map_err(|_| reject("INVALID_SHAPE"))?;
    require(
        ((if action == "revoke" { 1 } else { 0 })..=16).contains(&evidence.len()),
        "INVALID_SHAPE",
    )?;
    for r in &evidence {
        checked_ref(&json!(r), "workflow-evidence")?;
    }
    records::sort_refs(&mut evidence).map_err(|_| reject("INVALID_SHAPE"))?;
    p["evidence_refs"] = json!(evidence);
    if action == "revoke" {
        checked_ref(&p["transition_ref"], "workflow-activation")?;
        let reason = text(&p, "reason")?;
        require(
            !reason.is_empty() && reason.chars().count() <= 1024,
            "INVALID_SHAPE",
        )?;
    } else {
        for (field, kind) in [
            ("policy_ref", "workflow-policy"),
            ("product_ref", "workflow-product"),
            ("compiled_plan_ref", "workflow-compiled-plan"),
        ] {
            checked_ref(&p[field], kind)?;
            require(p[field] == b[field], "INVALID_REFERENCE")?;
        }
        let mut facts: Vec<RecordRef> = serde_json::from_value(p["fact_schema_refs"].clone())
            .map_err(|_| reject("INVALID_SHAPE"))?;
        require((1..=16).contains(&facts.len()), "INVALID_SHAPE")?;
        for r in &facts {
            checked_ref(&json!(r), "workflow-fact-schema")?;
        }
        records::sort_refs(&mut facts).map_err(|_| reject("INVALID_SHAPE"))?;
        p["fact_schema_refs"] = json!(facts);
        require(
            p["fact_schema_refs"] == b["fact_schema_refs"],
            "INVALID_REFERENCE",
        )?;
        match p["effective"]["kind"].as_str() {
            Some("at_commit") => {
                exact_keys(&p["effective"], &["kind"]).map_err(|_| reject("INVALID_SHAPE"))?
            }
            Some("not_before") => {
                exact_keys(&p["effective"], &["kind", "at"])
                    .map_err(|_| reject("INVALID_SHAPE"))?;
                let at = Timestamp::parse(text(&p["effective"], "at")?)
                    .map_err(|_| reject("INVALID_SCALAR"))?;
                p["effective"]["at"] = json!(at);
            }
            _ => return Err(reject("INVALID_SHAPE")),
        }
        if action == "replace" {
            checked_ref(&p["replaces_transition_ref"], "workflow-activation")?;
        }
    }
    Ok(p)
}
/// Reconstructs exact immutable records from normalized input and explicitly
/// captured metadata. Current eligibility, target selection and CAS are separate.
pub(super) fn build_records(
    req: &request::Request,
    payload: &Value,
    m: &TransitionMetadata<'_>,
) -> Result<PreparedTransitionRecords> {
    require(
        req.key() == m.key && normalize(req, m.binding)? == *payload,
        "INVALID_SHAPE",
    )?;
    if m.sequence <= 0 || m.actor.is_empty() {
        return Err(ServiceError::IntegrityFailure);
    }
    let b = canonical::parse_bounded(&m.binding.bytes, 262144)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let customer = text(&b, "customer")?;
    let agreement = text(payload, "agreement_id")?;
    let old = text(payload, "expected_revision")?
        .parse::<i64>()
        .map_err(|_| reject("INVALID_SCALAR"))?;
    let new = old.checked_add(1).ok_or_else(|| reject("INVALID_SCALAR"))?;
    if (old == 0) != m.prior.is_none() {
        return Err(ServiceError::IntegrityFailure);
    }
    if let Some(prior) = &m.prior {
        checked_ref(&json!(prior), "workflow-head-transition")?;
    }
    let (action, target_field) = match payload["schema"].as_str() {
        Some("ledger-workflow-activation/1") => (ActivationAction::Activate, None),
        Some("ledger-workflow-activation-replace/1") => {
            (ActivationAction::Replace, Some("replaces_transition_ref"))
        }
        Some("ledger-workflow-agreement-revoke/1") => {
            (ActivationAction::Revoke, Some("transition_ref"))
        }
        _ => return Err(ServiceError::IntegrityFailure),
    };
    let target = target_field
        .map(|f| checked_ref(&payload[f], "workflow-activation"))
        .transpose()?;
    if target != m.target {
        return Err(ServiceError::IntegrityFailure);
    }
    let effective =
        if action == ActivationAction::Revoke || payload["effective"]["kind"] == "at_commit" {
            m.at.clone()
        } else {
            Timestamp::parse(text(&payload["effective"], "at")?)
                .map_err(|_| reject("INVALID_SCALAR"))?
        };
    require(effective.micros() >= m.at.micros(), "INVALID_SHAPE")?;
    let intent = records::source_document(
        "workflow-agreement-intent",
        json!({"schema":"ledger-workflow-agreement-intent/1","kind":req.kind(),"payload":payload}),
    )?;
    let mut h = json!({"schema":"ledger-workflow-head-transition/1","head_kind":"agreement_activation","scope":m.key.scope,
        "customer":customer,"agreement_id":agreement,"operation_key":m.key,"actor":m.actor,"old_revision":old.to_string(),
        "new_revision":new.to_string(),"accepted_binding_ref":m.binding.reference,"accepted_at":m.at,
        "commit_sequence":m.sequence.to_string(),"resolved_effective_time":effective});
    if let Some(prior) = &m.prior {
        h["prior_head_ref"] = json!(prior);
    }
    let head = record::semantic_document(
        "workflow-head-transition",
        m.key,
        customer,
        agreement,
        new as u64,
        h,
    )?;
    let mut a = json!({"schema":"ledger-workflow-activation-transition/1","action":action.as_str(),"head_transition_ref":head.reference,
        "binding_ref":m.binding.reference,"effective_at":effective,"commit_sequence":m.sequence.to_string(),"accepted_at":m.at});
    if let Some(target) = &target {
        a["target_transition_ref"] = json!(target);
    }
    let activation = record::semantic_document(
        "workflow-activation",
        m.key,
        customer,
        agreement,
        new as u64,
        a,
    )?;
    let mut results = vec![
        intent.reference.clone(),
        head.reference.clone(),
        activation.reference.clone(),
    ];
    records::sort_refs(&mut results)?;
    let projection = Projection::Activation(ActivationProjection {
        customer: customer.into(),
        agreement: agreement.into(),
        expected_revision: old,
        head: AgreementHead {
            revision: new,
            transition: head.reference.clone(),
        },
        activation: activation.reference.clone(),
        binding: m.binding.reference.clone(),
        intent: intent.reference.clone(),
        action,
        effective_at_us: effective.micros(),
        accepted_at_us: m.at.micros(),
        sequence: m.sequence,
        target,
    });
    Ok(PreparedTransitionRecords {
        documents: vec![intent, head, activation],
        results,
        projection,
    })
}

#[cfg(test)]
mod tests;
