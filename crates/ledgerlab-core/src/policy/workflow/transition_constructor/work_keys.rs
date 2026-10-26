//! Same immutable identity recipes used by work construction and parent DATA
//! lookup. No amounts, candidate, authority, absence or completed-origin proof.
use super::{body, data::OperationKey, shape, CoreMeter};
use crate::{canonical::audit_arena::AuditArena, Result};
use serde_json::Value;
fn arena(m: &mut CoreMeter) -> Result<AuditArena> {
    m.representation_arena()
        .ok_or_else(|| shape::fail("owning work identity"))
}
pub(super) fn scope(
    m: &mut CoreMeter,
    key: &OperationKey,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let a = arena(m)?;
    let l = a.string_value(&key.scope[0], m)?;
    let r = a.string_value(&key.scope[1], m)?;
    a.array_from_values(&[l.value(), r.value()], m)
}
pub(super) fn claim(
    m: &mut CoreMeter,
    key: &OperationKey,
    namespace: &str,
    p: &Value,
) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = scope(m, key)?;
    let ns = a.string_value(namespace, m)?;
    let completion = a.string_value("completion", m)?;
    let business = shape::field(m, p, "business_operation_id")?;
    let tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            ns.value(),
            business,
            completion.value(),
        ],
        m,
    )?;
    body::semantic_id(m, "wclaim_", "workflow-work-claim", tuple.value())
}
pub(super) fn event(m: &mut CoreMeter, key: &OperationKey, p: &Value) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = scope(m, key)?;
    let source = a.string_value(&key.source, m)?;
    let external = shape::field(m, p, "external_event_id")?;
    let tuple =
        a.array_from_values(&[store.value(), scope.value(), source.value(), external], m)?;
    body::semantic_id(m, "wevent_", "workflow-event", tuple.value())
}
pub(super) fn decision(m: &mut CoreMeter, claim: &str) -> Result<String> {
    let a = arena(m)?;
    let claim = a.string_value(claim, m)?;
    let discriminator = a.string_value("work", m)?;
    let tuple = a.array_from_values(&[claim.value(), discriminator.value()], m)?;
    body::semantic_id(m, "wdecision_", "workflow-decision", tuple.value())
}
pub(super) fn target(
    m: &mut CoreMeter,
    key: &OperationKey,
    agreement: &Value,
    claim: &str,
    slot: &str,
) -> Result<String> {
    let a = arena(m)?;
    let scope = scope(m, key)?;
    let claim = a.string_value(claim, m)?;
    let slot = a.string_value(slot, m)?;
    let tuple = a.array_from_values(&[scope.value(), agreement, claim.value(), slot.value()], m)?;
    body::semantic_id(m, "wtarget_", "workflow-target", tuple.value())
}
pub(super) fn app_decision(
    m: &mut CoreMeter,
    scope: &Value,
    work_id: &str,
    slot: &str,
) -> Result<String> {
    let a = arena(m)?;
    let work = a.string_value(work_id, m)?;
    let slot = a.string_value(slot, m)?;
    let discriminator = a.string_value("app_cost", m)?;
    let tuple = a.array_from_values(
        &[scope, work.value(), slot.value(), discriminator.value()],
        m,
    )?;
    body::semantic_id(m, "wdecision_", "workflow-decision", tuple.value())
}

/// The stage amendment extends only a present staged Work's original semantic
/// tuple. Every unstaged original retains its exact twelve/thirteen fields.
pub(super) fn claim_facts_digest(m: &mut CoreMeter, p: &Value, tuple: &Value) -> Result<String> {
    let Some(stage) = p.get("stage_instance_ref") else {
        return super::records::hash(m, "workflow-claim-facts", tuple);
    };
    let fields = tuple
        .as_array()
        .ok_or_else(|| shape::fail("Work claim tuple"))?;
    if crate::validation_work::compare(m, fields.len(), 13usize)?.is_gt() {
        return Err(shape::fail("Work claim fixed tuple dimension"));
    }
    let a = arena(m)?;
    let absent = Value::Null;
    let mut selected = [&absent; 14];
    for (i, value) in fields.iter().enumerate() {
        m.visit("work.staged_claim_tuple")?;
        m.write_bytes(std::mem::size_of::<&Value>())?;
        selected[i] = value;
    }
    selected[fields.len()] = stage;
    let count = crate::validation_work::usize_add(m, fields.len(), 1)?;
    let value = a.array_from_values(&selected[..count], m)?;
    super::records::hash(m, "workflow-claim-facts", value.value())
}

#[cfg(test)]
mod staged_claim_tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    use serde_json::json;
    #[test]
    fn staged_claim_extends_only_present_stage_and_original_consumer_repeats_it() {
        let a = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let m = s.meter_mut();
        let mut p = json!({"customer":"customer","chain":"chain","status":"succeeded","activity":"test:work","product_ref":{},"policy_ref":{},"fact_schema_ref":{},"facts":{},"links":[],"binding_ref":{},"invocation_refs":[],"evidence_refs":[],"business_operation_id":"business"});
        let tuple = json!([
            p["customer"],
            p["chain"],
            p["status"],
            p["activity"],
            p["product_ref"],
            p["policy_ref"],
            p["fact_schema_ref"],
            p["facts"],
            p["links"],
            p["binding_ref"],
            p["invocation_refs"],
            p["evidence_refs"]
        ]);
        let unstaged = super::super::records::hash(m, "workflow-claim-facts", &tuple).unwrap();
        assert_eq!(claim_facts_digest(m, &p, &tuple).unwrap(), unstaged);
        p["stage_instance_ref"] = json!({"kind":"workflow-stage-instance","id":format!("wstage_{}","1".repeat(64)),"digest":format!("sha256:{}","2".repeat(64))});
        let staged = claim_facts_digest(m, &p, &tuple).unwrap();
        assert_ne!(staged, unstaged);
        let claim = json!({"business_operation_id":"business","facts_digest":staged});
        let work = json!({"payload":p});
        super::super::outcome::original_claim_association(m, &claim, &work).unwrap();
        let wrong = json!({"business_operation_id":"business","facts_digest":unstaged});
        assert!(super::super::outcome::original_claim_association(m, &wrong, &work).is_err());
    }
}
