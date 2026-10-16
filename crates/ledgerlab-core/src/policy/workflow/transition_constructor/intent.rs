//! Retry intent normalization under a completed ORIGINAL program. No current
//! policy, clock, grants, pricing, saved money or new economic child is consulted.
//! The host must first authenticate/recompute the original candidate and then
//! compare the returned normalized bytes before attempting fresh selection.
use super::super::{FactSchema, selected_program::SelectedProgram};
use super::{CoreMeter, codec, owned_facts, shape};
use crate::{
    Error, Result,
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{self as w, ArithmeticOperation, OriginalAuditSession, RepresentationKind},
};
use serde_json::Value;
use std::mem;
fn conflict() -> Error {
    Error::new(
        "IDENTITY_CONFLICT",
        "original request schema/subject changed",
    )
}
fn schema<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    reference: &Value,
) -> Result<&'a FactSchema> {
    for (r, schema) in p.fact_schemas() {
        m.visit("intent.original_schema")?;
        // DefinitionRef is immutable program data. Its three string fields are
        // compared directly, without a new serialized/ref JSON allocation.
        let id = shape::text(m, reference, "id")?;
        let kind = shape::text(m, reference, "kind")?;
        let digest = shape::text(m, reference, "digest")?;
        if shape::same_text(m, r.id(), id)?
            && shape::same_text(m, r.kind(), kind)?
            && shape::same_text(m, r.digest(), digest)?
        {
            return Ok(schema);
        }
    }
    Err(shape::fail("retry original schema unavailable"))
}
fn facts(
    program: &SelectedProgram,
    original: &Value,
    incoming: &mut AuditedValue,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let old = shape::field(session.meter_mut(), original, "fact_schema_ref")?;
    let new = shape::field(session.meter_mut(), incoming.value(), "fact_schema_ref")?;
    if !shape::same(session.meter_mut(), old, new)? {
        return Err(conflict());
    }
    let definition = schema(session.meter_mut(), program, old)?;
    let input = shape::field(session.meter_mut(), incoming.value(), "facts")?;
    let arena = session.arena().shared_handle();
    let normalized = owned_facts::normalize(&arena, session.meter_mut(), definition, input)?;
    let body = arena.clone_value(normalized.facts().canonical_value(), session.meter_mut())?;
    incoming.replace_existing_owned("facts", body, session.meter_mut())?;
    sort_set(incoming, "fact_proofs", true, session)
}
fn multiply(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    crate::validation_work::ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&a.into(), &b.into()],
    )?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("retry metadata bound"))
}
/// SAME insertion ordering as the constructor's named-proof and nomination
/// normalization, with owned key bytes and moves instead of Value clones.
fn sort_set(
    payload: &mut AuditedValue,
    key: &str,
    field_key: bool,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let items = shape::array(session.meter_mut(), payload.value(), key)?;
    let n = items.len();
    let vector = multiply(
        session.meter_mut(),
        n,
        mem::size_of::<(usize, AuditedBytes)>(),
    )?;
    let permutation = multiply(session.meter_mut(), n, mem::size_of::<usize>())?;
    let bytes = w::usize_add(session.meter_mut(), vector, permutation)?;
    let headers = multiply(session.meter_mut(), 2, mem::size_of::<Vec<usize>>())?;
    let bytes = w::usize_add(session.meter_mut(), bytes, headers)?;
    // Declare the lease before owned vectors; reverse local drop order destroys
    // all metadata/key owners first on every error and successful return.
    let _metadata = session.reserve_external_payload(bytes)?;
    let mut ordered: Vec<(usize, AuditedBytes)> = Vec::new();
    ordered
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("retry key allocation"))?;
    for (index, item) in items.iter().enumerate() {
        session.meter_mut().visit("intent.normalized_set_member")?;
        let value = if field_key {
            shape::field(session.meter_mut(), item, "field")?
        } else {
            item
        };
        let bytes = session.canonical_representation(value, RepresentationKind::OriginalRequest)?;
        let mut at = ordered.len();
        for (i, (_, prior)) in ordered.iter().enumerate() {
            session.meter_mut().visit("intent.normalized_set_compare")?;
            let order = shape::byte_order(session.meter_mut(), bytes.bytes(), prior.bytes())?;
            if field_key && order.is_eq() {
                return Err(shape::fail("duplicate proof field"));
            }
            if !order.is_gt() {
                at = i;
                break;
            }
        }
        for _ in at..ordered.len() {
            session.meter_mut().visit("intent.normalized_set_move")?;
            let bytes = mem::size_of::<(usize, AuditedBytes)>();
            session.meter_mut().scan_bytes(bytes)?;
            session.meter_mut().write_bytes(bytes)?;
        }
        ordered.insert(at, (index, bytes));
    }
    let mut destination = Vec::new();
    destination
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("retry permutation allocation"))?;
    for _ in 0..n {
        session.meter_mut().visit("intent.permutation_element")?;
        destination.push(0usize);
    }
    for (new, (old, _)) in ordered.iter().enumerate() {
        session.meter_mut().visit("intent.permutation_position")?;
        session.meter_mut().write_bytes(mem::size_of::<usize>())?;
        destination[*old] = new;
    }
    payload.reorder_existing_array(key, &mut destination, session.meter_mut())
}
fn normalized(
    program: &SelectedProgram,
    original: &Value,
    incoming: &mut AuditedValue,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let old_schema = shape::text(session.meter_mut(), original, "schema")?;
    let new_schema = shape::text(session.meter_mut(), incoming.value(), "schema")?;
    if !shape::same_text(session.meter_mut(), old_schema, new_schema)? {
        return Err(conflict());
    }
    if shape::same_text(session.meter_mut(), old_schema, "ledger-workflow-work/1")?
        || shape::same_text(session.meter_mut(), old_schema, "ledger-workflow-outcome/1")?
    {
        facts(program, original, incoming, session)?;
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-outcome-correction/1",
    )? {
        let original_replacement = shape::field(session.meter_mut(), original, "replacement")?;
        let replacement = shape::field(session.meter_mut(), incoming.value(), "replacement")?;
        if replacement.get("kind").is_none() && original_replacement.get("kind").is_none() {
            let arena = session.arena().shared_handle();
            let mut owned = arena.clone_value(replacement, session.meter_mut())?;
            facts(program, original_replacement, &mut owned, session)?;
            incoming.replace_existing_owned("replacement", owned, session.meter_mut())?;
        }
        // Explicit-none has no facts. A changed replacement variant stays
        // changed in the returned intent and cannot alias the original.
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-quantity-correction/2",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-work-reverse/2",
    )? {
        // The registered root interpreter preserves scalar spelling, and sorts
        // these complete object sets. No pricing or current authority occurs.
        sort_set(incoming, "dependent_reversals", false, session)?;
        sort_set(incoming, "family_closures", false, session)?;
    } else if !shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-outcome-close/1",
    )? {
        return Err(Error::new(
            "UNSUPPORTED_SCHEMA_PROFILE",
            "original intent recipe unavailable",
        ));
    }
    Ok(())
}
/// `original_normalized_payload` must come from the independently completed
/// original candidate, never a wrapper-consistent saved assertion alone. This
/// resource/data API does not itself authenticate that host provenance.
pub fn normalize_saved_intent(
    program: &SelectedProgram,
    original_normalized_payload: &[u8],
    incoming_raw: &[u8],
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        let original = session.parse_representation(
            original_normalized_payload,
            RepresentationKind::OriginalRequest,
        )?;
        codec::validate_selected_value(session.meter_mut(), original.value(), 262144)?;
        let mut incoming =
            session.parse_representation(incoming_raw, RepresentationKind::OriginalRequest)?;
        codec::validate_selected_value(session.meter_mut(), incoming.value(), 262144)?;
        normalized(program, original.value(), &mut incoming, session)?;
        session.canonical_representation(incoming.value(), RepresentationKind::OriginalRequest)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::selected_program::{
            SelectedCompilerSupport, SelectedSourceInput, compile_selected_program,
        },
    };
    use serde_json::json;
    #[test]
    fn retry_uses_original_schema_and_same_finished_child_owner_without_rerating() {
        let source:Value=canonical::parse(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let input = SelectedSourceInput {
            documents: source["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        };
        let mut compile = CoreMeter::for_compile(1_048_576).unwrap();
        let p = compile_selected_program(
            input,
            &SelectedCompilerSupport::builtin().unwrap(),
            &mut compile,
        )
        .unwrap();
        let instruction = p.compiled_plan().canonical_value()["typed_instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["rule"]["formula"]["kind"] == "unit")
            .unwrap();
        let field = instruction["rule"]["formula"]["quantity_field"]
            .as_str()
            .unwrap();
        let activity = &instruction["rule"]["activity"];
        let product = p
            .source_documents()
            .iter()
            .find(|d| d.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap()
            .canonical_value();
        let reference = &product["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| &a["name"] == activity)
            .unwrap()["fact_schema"];
        let original = json!({"schema":"ledger-workflow-work/1","activity":activity,"fact_schema_ref":reference,"facts":{field:"3"},"fact_proofs":[]});
        let mut incoming = original.clone();
        incoming["facts"][field] = json!("0003");
        let original = canonical::CanonicalBytes::from_value(&original).unwrap();
        let raw = canonical::CanonicalBytes::from_value(&incoming).unwrap();
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child = session.meter_mut().original_economic_meter().unwrap();
        child.visit("test.already_completed_original").unwrap();
        session.meter_mut().finish_original_economic(child).unwrap();
        let output =
            normalize_saved_intent(&p, original.as_slice(), raw.as_slice(), &mut session).unwrap();
        assert_eq!(output.bytes(), original.as_slice());
        assert!(session.meter_mut().original_economic_meter().is_err());
        drop(output);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        // This is normalization-only input DATA, not a forged original receipt:
        // no authority/acceptance or completed-candidate claim is made by test.
        let mut changed = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        incoming["fact_schema_ref"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
        let changed_raw = canonical::CanonicalBytes::from_value(&incoming).unwrap();
        assert_eq!(
            normalize_saved_intent(
                &p,
                original.as_slice(),
                changed_raw.as_slice(),
                &mut changed
            )
            .err()
            .unwrap()
            .code,
            "IDENTITY_CONFLICT"
        );
        assert!(changed.work().scan_bytes(1).is_err());
        let mut invalid = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        incoming["fact_schema_ref"] = reference.clone();
        incoming["facts"][field] = json!("-0");
        let invalid_raw = canonical::CanonicalBytes::from_value(&incoming).unwrap();
        assert!(
            normalize_saved_intent(
                &p,
                original.as_slice(),
                invalid_raw.as_slice(),
                &mut invalid
            )
            .is_err()
        );
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn owned_named_sets_preserve_canonical_permutation_and_refuse_duplicate_fields() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut value=session.parse_representation(br#"{"fact_proofs":[{"field":"z","value":1},{"field":"a","value":2},{"field":"m","value":3}]}"#,RepresentationKind::OriginalRequest).unwrap();
        sort_set(&mut value, "fact_proofs", true, &mut session).unwrap();
        assert_eq!(value.value()["fact_proofs"][0]["field"], "a");
        assert_eq!(value.value()["fact_proofs"][1]["field"], "m");
        assert_eq!(value.value()["fact_proofs"][2]["field"], "z");
        drop(value);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let mut duplicate = session
            .parse_representation(
                br#"{"fact_proofs":[{"field":"a","value":1},{"field":"a","value":2}]}"#,
                RepresentationKind::OriginalRequest,
            )
            .unwrap();
        assert!(sort_set(&mut duplicate, "fact_proofs", true, &mut session).is_err());
        drop(duplicate);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
