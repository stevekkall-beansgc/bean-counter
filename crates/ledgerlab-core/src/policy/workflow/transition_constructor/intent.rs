//! Retry intent normalization under a completed ORIGINAL program. No current
//! policy, clock, grants, pricing, saved money or new economic child is consulted.
//! The host must first authenticate/recompute the original candidate and then
//! compare the returned normalized bytes before attempting fresh selection.
use super::super::{selected_program::SelectedProgram, FactSchema};
use super::{codec, owned_facts, shape, CoreMeter};
use crate::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{self as w, ArithmeticOperation, OriginalAuditSession, RepresentationKind},
    Error, Result,
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
    let arena = session.arena().shared_handle();
    sort_owned_set(payload, key, field_key, session.meter_mut(), &arena)
}
pub(super) fn sort_owned_set(
    payload: &mut AuditedValue,
    key: &str,
    field_key: bool,
    m: &mut CoreMeter,
    arena: &crate::canonical::audit_arena::AuditArena,
) -> Result<()> {
    let items = shape::array(m, payload.value(), key)?;
    let n = items.len();
    let vector = multiply(m, n, mem::size_of::<(usize, AuditedBytes)>())?;
    let permutation = multiply(m, n, mem::size_of::<usize>())?;
    let bytes = w::usize_add(m, vector, permutation)?;
    let headers = multiply(m, 2, mem::size_of::<Vec<usize>>())?;
    let bytes = w::usize_add(m, bytes, headers)?;
    // Declare the lease before owned vectors; reverse local drop order destroys
    // all metadata/key owners first on every error and successful return.
    let _metadata = arena.reserve_external_payload(bytes, m)?;
    // Vector controls and first insert/zero initialization must remain paid
    // even when an allocation or a later member refuses. Moves still pay
    // their separate actual scans/writes below.
    m.write_bytes(bytes)?;
    let mut ordered: Vec<(usize, AuditedBytes)> = Vec::new();
    ordered
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("retry key allocation"))?;
    for (index, item) in items.iter().enumerate() {
        m.visit("intent.normalized_set_member")?;
        let value = if field_key {
            shape::field(m, item, "field")?
        } else {
            item
        };
        let bytes =
            m.canonical_owned_representation(arena, value, RepresentationKind::OriginalRequest)?;
        let mut at = ordered.len();
        for (i, (_, prior)) in ordered.iter().enumerate() {
            m.visit("intent.normalized_set_compare")?;
            let order = shape::byte_order(m, bytes.bytes(), prior.bytes())?;
            if order.is_eq() {
                return Err(shape::fail("duplicate normalized set member"));
            }
            if !order.is_gt() {
                at = i;
                break;
            }
        }
        for _ in at..ordered.len() {
            m.visit("intent.normalized_set_move")?;
            let bytes = mem::size_of::<(usize, AuditedBytes)>();
            m.scan_bytes(bytes)?;
            m.write_bytes(bytes)?;
        }
        ordered.insert(at, (index, bytes));
    }
    let mut destination = Vec::new();
    destination
        .try_reserve_exact(n)
        .map_err(|_| shape::fail("retry permutation allocation"))?;
    for _ in 0..n {
        m.visit("intent.permutation_element")?;
        destination.push(0usize);
    }
    for (new, (old, _)) in ordered.iter().enumerate() {
        m.visit("intent.permutation_position")?;
        m.write_bytes(mem::size_of::<usize>())?;
        destination[*old] = new;
    }
    payload.reorder_existing_array(key, &mut destination, m)
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
    if matches!(
        old_schema,
        "ledger-workflow-invocation-authorize/1"
            | "ledger-workflow-invocation-close/1"
            | "ledger-workflow-stage-authorize/1"
            | "ledger-workflow-stage-close/1"
    ) {
        if !program.uses_stage_lifecycle_recipe() {
            return Err(conflict());
        }
        let mut value =
            super::working_value::WorkingValue::copy(session.meter_mut(), incoming.value())?;
        match old_schema {
            "ledger-workflow-invocation-authorize/1" => {
                super::invocation::normalize_authorization(session.meter_mut(), &mut value)?
            }
            "ledger-workflow-invocation-close/1" => {
                super::invocation::normalize_close(session.meter_mut(), &mut value)?
            }
            "ledger-workflow-stage-authorize/1" => {
                super::stage::normalize_authorization(session.meter_mut(), &mut value)?
            }
            "ledger-workflow-stage-close/1" => {
                let old = shape::field(session.meter_mut(), original, "fact_schema_ref")?;
                let new = shape::field(session.meter_mut(), &value, "fact_schema_ref")?;
                if !shape::same(session.meter_mut(), old, new)? {
                    return Err(conflict());
                }
                super::stage::normalize_close(program, session.meter_mut(), &mut value)?;
            }
            _ => unreachable!(),
        }
        *incoming = value.into_owned()?;
    } else if shape::same_text(session.meter_mut(), old_schema, "ledger-workflow-work/1")?
        || shape::same_text(session.meter_mut(), old_schema, "ledger-workflow-outcome/1")?
        || program.uses_declared_work_link_recipe()
            && shape::same_text(session.meter_mut(), old_schema, "ledger-workflow-outcome/2")?
    {
        facts(program, original, incoming, session)?;
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-outcome-correction/1",
    )? || program.uses_declared_work_link_recipe()
        && shape::same_text(
            session.meter_mut(),
            old_schema,
            "ledger-workflow-outcome-correction/2",
        )?
    {
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
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-cost-finalize/2",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-cost-correct/2",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-app-observe/1",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-app-cost-attest/1",
    )? {
        super::app_cost::normalize_intent(program, incoming, session.meter_mut())?;
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-cost-finalize/1",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-cost-correct/1",
    )? {
        if !program.uses_owner_local_cost_recipe() {
            return Err(conflict());
        }
        let mut value =
            super::working_value::WorkingValue::copy(session.meter_mut(), incoming.value())?;
        super::cost::normalize_request(session.meter_mut(), &mut value)?;
        *incoming = value.into_owned()?;
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-outcome-close/2",
    )? {
        if !program.uses_family_close_recipe() {
            return Err(conflict());
        }
        let revision = shape::text(session.meter_mut(), incoming.value(), "expected_revision")?;
        let revision = super::super::facts::counter_with_work(revision, session.meter_mut())?;
        if !w::compare(session.meter_mut(), revision, 0u64)?.is_gt() {
            return Err(shape::fail("positive close intent revision"));
        }
        let arena = session.arena().shared_handle();
        let value = super::close::counter_value(session.meter_mut(), &arena, revision)?;
        incoming.replace_existing_owned("expected_revision", value, session.meter_mut())?;
        sort_set(incoming, "evidence_refs", false, session)?;
    } else if shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-fact-attest/1",
    )? || shape::same_text(
        session.meter_mut(),
        old_schema,
        "ledger-workflow-family-close-attest/1",
    )? {
        let close = shape::same_text(
            session.meter_mut(),
            old_schema,
            "ledger-workflow-family-close-attest/1",
        )?;
        if close && !program.uses_family_close_recipe() {
            return Err(conflict());
        }
        let att = shape::field(session.meter_mut(), incoming.value(), "attestation")?;
        let arena = session.arena().shared_handle();
        let normalized = super::retention::normalize_assertion(
            session.meter_mut(),
            &arena,
            program,
            att,
            close,
        )?;
        incoming.replace_existing_owned("attestation", normalized, session.meter_mut())?;
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

/// Local original-key intent normalization only. Before calling, the host
/// verifies the actual saved local10 normalized original, descriptor/registry,
/// committed operation/manifest/receipt/results and targeted original program
/// and terms. This DATA helper does not rerate or grant current mutation rights.
pub fn normalize_local_saved_intent(
    program: &SelectedProgram,
    original_normalized_payload: &[u8],
    incoming_raw: &[u8],
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        session.check_cancelled()?;
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_local_execution_recipe() {
            return Err(Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "local original intent requires exact original local program",
            ));
        }
        normalize_saved_intent(program, original_normalized_payload, incoming_raw, session)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

/// Comparison DATA for a fresh-key Work alias under its authenticated ORIGINAL
/// program. Current write/read and the new alias journal remain host/core gates.
/// Only the transport event label is replaced; every economic field/proof stays.
pub fn normalize_local_work_alias_intent(
    program: &SelectedProgram,
    original_normalized_payload: &[u8],
    incoming_raw: &[u8],
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        session.check_cancelled()?;
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_shared_namespace_recipe() {
            return Err(Error::new(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "exact shared Work alias recipe",
            ));
        }
        let normalized = normalize_local_saved_intent(
            program,
            original_normalized_payload,
            incoming_raw,
            session,
        )?;
        let original = session.parse_representation(
            original_normalized_payload,
            RepresentationKind::OriginalRequest,
        )?;
        let mut incoming = session
            .parse_representation(normalized.bytes(), RepresentationKind::OriginalRequest)?;
        for value in [original.value(), incoming.value()] {
            let schema = shape::text(session.meter_mut(), value, "schema")?;
            if !shape::same_text(session.meter_mut(), schema, "ledger-workflow-work/1")? {
                return Err(conflict());
            }
            let event = shape::text(session.meter_mut(), value, "external_event_id")?;
            if !w::compare(session.meter_mut(), event.len(), 0usize)?.is_gt()
                || w::compare(session.meter_mut(), event.len(), 128usize)?.is_gt()
            {
                return Err(shape::fail("alias transport event bound"));
            }
            session.meter_mut().scan_bytes(event.len())?;
            for ch in event.chars() {
                session.meter_mut().visit("alias.transport_character")?;
                if ch.is_control() {
                    return Err(shape::fail("alias transport control character"));
                }
            }
        }
        let arena = session.arena().shared_handle();
        let event = shape::field(session.meter_mut(), original.value(), "external_event_id")?;
        let event = arena.clone_value(event, session.meter_mut())?;
        incoming.replace_existing_owned("external_event_id", event, session.meter_mut())?;
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
            compile_selected_program, SelectedCompilerSupport, SelectedSourceInput,
        },
    };
    use serde_json::json;
    #[test]
    fn stage_and_invocation_intents_share_forward_normalization_under_original_program() {
        use super::super::working_value::WorkingValue;
        use crate::policy::workflow::selected_program::compile_selected_publication;
        let arena = AuditArena::new().unwrap();
        let mut compile = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support =
            SelectedCompilerSupport::local_stage_lifecycle_v2_execution_with_session(&mut compile)
                .unwrap();
        let source: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-stage/source-platform-funding.json"
        ))
        .unwrap();
        let mut documents = source["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect::<Vec<_>>();
        documents.push(support.canonical_bytes().to_vec());
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents,
                root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
            },
            &support,
            &mut compile,
        )
        .unwrap();
        let reference = |name: &str| json!({"id":name,"kind":"workflow-source-grant","digest":format!("sha256:{}", "a".repeat(64))});
        let auth = json!({"schema":"ledger-workflow-invocation-authorize/1","agreement_id":"stage-support","binding_ref":reference("binding"),"business_operation_id":"stage-member","chain_id":"stage-chain","activity":"support:prepare","supplier_slot":"support:supplier","source_slot":"support:worker","unit":"support:item","maximum_quantity":"1","maximum_exposure":{"atoms":"1000000000000000000","currency":"USD","scale":18},"start_before":"2099-01-01T00:00:00.000000Z","evidence_refs":[reference("a"),reference("z")]});
        let declaration = json!({"schema":"ledger-workflow-stage-authorize/1","expected_chain_revision":"0","stage_instance":{"agreement_id":"stage-support","binding_ref":reference("binding"),"chain":"stage-chain","customer":"customer-stage","stage_name":"support:stage","authority_refs":[reference("a"),reference("z")],"expected":[{"template_input_key":"a","activity":"support:prepare","business_operation_id":"member-a","source":"worker","component_slots":["a","z"]},{"template_input_key":"z","activity":"support:prepare","business_operation_id":"member-z","source":"worker","component_slots":["a","z"]}]},"invocation_inputs":[auth.clone()]});
        let close = json!({"schema":"ledger-workflow-invocation-close/1","invocation_ref":reference("invocation"),"expected_revision":"2","reason":"completed","evidence_refs":[reference("a"),reference("z")]});
        let product = program
            .source_documents()
            .iter()
            .find(|v| v.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap()
            .canonical_value();
        let close_schema = &product["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "support:close")
            .unwrap()["fact_schema"];
        let stage_close = json!({"schema":"ledger-workflow-stage-close/1","external_event_id":"close","business_operation_id":"stage-close","customer":"customer-stage","chain":"stage-chain","activity":"support:close","product_ref":reference("product"),"policy_ref":reference("policy"),"fact_schema_ref":close_schema,"binding_ref":reference("binding"),"facts":{"support:occurred":"2026-10-27T00:00:00.000000Z","support:success":true},"fact_proofs":[{"field":"a"},{"field":"z"}],"links":[reference("a"),reference("z")],"evidence_refs":[reference("a"),reference("z")],"occurred_at":"2026-10-27T00:00:00.000000Z","stage":"support:stage","stage_instance_ref":reference("stage")});
        for original in [&auth, &close, &declaration, &stage_close] {
            let mut forward = WorkingValue::copy(compile.meter_mut(), original).unwrap();
            match original["schema"].as_str().unwrap() {
                "ledger-workflow-invocation-authorize/1" => {
                    super::super::invocation::normalize_authorization(
                        compile.meter_mut(),
                        &mut forward,
                    )
                    .unwrap()
                }
                "ledger-workflow-invocation-close/1" => {
                    super::super::invocation::normalize_close(compile.meter_mut(), &mut forward)
                        .unwrap()
                }
                "ledger-workflow-stage-authorize/1" => {
                    super::super::stage::normalize_authorization(compile.meter_mut(), &mut forward)
                        .unwrap()
                }
                _ => super::super::stage::normalize_close(
                    &program,
                    compile.meter_mut(),
                    &mut forward,
                )
                .unwrap(),
            }
            let forward = forward.bytes(compile.meter_mut()).unwrap();
            let mut incoming = original.clone();
            if let Some(evidence) = incoming.get_mut("evidence_refs") {
                evidence.as_array_mut().unwrap().reverse();
            }
            match incoming["schema"].as_str().unwrap() {
                "ledger-workflow-invocation-authorize/1" => {
                    incoming["maximum_quantity"] = json!("0001.000")
                }
                "ledger-workflow-stage-authorize/1" => {
                    incoming["stage_instance"]["authority_refs"]
                        .as_array_mut()
                        .unwrap()
                        .reverse();
                    incoming["stage_instance"]["expected"]
                        .as_array_mut()
                        .unwrap()
                        .reverse();
                    incoming["stage_instance"]["expected"][0]["component_slots"]
                        .as_array_mut()
                        .unwrap()
                        .reverse();
                    incoming["invocation_inputs"][0]["maximum_quantity"] = json!("0001.000");
                    incoming["invocation_inputs"][0]["evidence_refs"]
                        .as_array_mut()
                        .unwrap()
                        .reverse();
                }
                "ledger-workflow-stage-close/1" => {
                    incoming["fact_proofs"].as_array_mut().unwrap().reverse();
                    incoming["links"].as_array_mut().unwrap().reverse();
                }
                _ => {}
            }
            let raw = canonical::CanonicalBytes::from_value(&incoming).unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let normalized =
                normalize_local_saved_intent(&program, &forward[..], raw.as_slice(), &mut session)
                    .unwrap();
            assert_eq!(normalized.bytes(), &forward[..]);
            // Substantive changes stay different; normalizing is not authority.
            match incoming["schema"].as_str().unwrap() {
                "ledger-workflow-invocation-authorize/1" => {
                    incoming["maximum_quantity"] = json!("2")
                }
                "ledger-workflow-invocation-close/1" => incoming["expected_revision"] = json!("3"),
                "ledger-workflow-stage-authorize/1" => {
                    incoming["stage_instance"]["customer"] = json!("different-customer")
                }
                _ => incoming["facts"]["support:success"] = json!(false),
            }
            let raw = canonical::CanonicalBytes::from_value(&incoming).unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert_ne!(
                normalize_local_saved_intent(&program, &forward[..], raw.as_slice(), &mut session)
                    .unwrap()
                    .bytes(),
                &forward[..]
            );
        }
        let mut duplicate = auth.clone();
        duplicate["evidence_refs"] = json!([reference("a"), reference("a")]);
        let raw = canonical::CanonicalBytes::from_value(&duplicate).unwrap();
        let original = canonical::CanonicalBytes::from_value(&auth).unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(normalize_local_saved_intent(
            &program,
            original.as_slice(),
            raw.as_slice(),
            &mut session
        )
        .is_err());
        // A real non-Stage program retains its rejection of these four literals.
        let legacy: Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        let mut meter = CoreMeter::for_compile(1_048_576).unwrap();
        let legacy = compile_selected_program(
            SelectedSourceInput {
                documents: legacy["documents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                    .collect(),
                root_policy_ref: serde_json::from_value(legacy["root_policy_ref"].clone()).unwrap(),
            },
            &SelectedCompilerSupport::builtin().unwrap(),
            &mut meter,
        )
        .unwrap();
        for original in [&auth, &close, &declaration, &stage_close] {
            let raw = canonical::CanonicalBytes::from_value(original).unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert_eq!(
                normalize_saved_intent(&legacy, raw.as_slice(), raw.as_slice(), &mut session)
                    .err()
                    .unwrap()
                    .code,
                "IDENTITY_CONFLICT"
            );
        }
    }
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
        assert!(normalize_saved_intent(
            &p,
            original.as_slice(),
            invalid_raw.as_slice(),
            &mut invalid
        )
        .is_err());
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
    #[test]
    fn shared_work_alias_preserves_all_economic_intent_and_validates_transport() {
        use crate::policy::workflow::selected_program::compile_selected_publication;
        let arena = AuditArena::with_resource_limits(crate::validation_work::ResourceLimits::unlimited()).unwrap();
        let mut compiling = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support = SelectedCompilerSupport::local_shared_namespace_execution_with_session(&mut compiling).unwrap();
        let mut source: Value = serde_json::from_slice(include_bytes!("../../../../../../examples/workflow-work-lifecycle/source.json")).unwrap();
        let documents = source["documents"].as_array_mut().unwrap();
        let policy = documents.iter_mut().find(|v| v["schema"] == "ledger-workflow-policy/4").unwrap();
        policy["compiler_ref"] = json!(support.reference());
        let digest = crate::policy::workflow::document_digest("ledger-workflow-policy/4", policy).unwrap();
        let root = json!({"kind":"workflow-policy","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
        let schema = documents.iter().find(|v| v["schema"] == "ledger-workflow-fact-schema/1" && v["fields"].as_array().unwrap().iter().any(|f| f["name"] == "support:quantity")).unwrap();
        let digest = crate::policy::workflow::document_digest("ledger-workflow-fact-schema/1", schema).unwrap();
        let schema = json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
        let mut bytes: Vec<_> = documents.iter().map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect();
        bytes.push(support.canonical_bytes().to_vec());
        let p = compile_selected_publication(SelectedSourceInput { documents:bytes, root_policy_ref:serde_json::from_value(root).unwrap() }, &support, &mut compiling).unwrap();
        let original=json!({"schema":"ledger-workflow-work/1","external_event_id":"original-delivery","business_operation_id":"one-completion","status":"succeeded","fact_schema_ref":schema,"facts":{"support:quantity":"0.1"},"fact_proofs":[],"links":[],"invocation_refs":[],"evidence_refs":[]});
        let canonical_original=canonical::CanonicalBytes::from_value(&original).unwrap();
        let mut incoming=original.clone(); incoming["external_event_id"]=json!("different-delivery"); incoming["facts"]["support:quantity"]=json!("0.100");
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let raw=canonical::CanonicalBytes::from_value(&incoming).unwrap();
        let normalized=normalize_local_work_alias_intent(&p,canonical_original.as_slice(),raw.as_slice(),&mut s).unwrap();
        assert_eq!(normalized.bytes(),canonical_original.as_slice());
        incoming["status"]=json!("failed");
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let raw=canonical::CanonicalBytes::from_value(&incoming).unwrap();
        assert_ne!(normalize_local_work_alias_intent(&p,canonical_original.as_slice(),raw.as_slice(),&mut s).unwrap().bytes(),canonical_original.as_slice());
        incoming["external_event_id"]=json!("bad\ntransport");
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let raw=canonical::CanonicalBytes::from_value(&incoming).unwrap();
        assert!(normalize_local_work_alias_intent(&p,canonical_original.as_slice(),raw.as_slice(),&mut s).is_err());
    }

}
