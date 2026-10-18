//! Existing activation transition bodies and semantic IDs over owning input.
//! Current projection reads remain DATA until chronological schedule and rights
//! validation; producing these exact rows never grants permission to append.
use super::*;
use ledgerlab_core::validation_work::ArithmeticOperation as Op;

pub(in super::super) struct ActivationRecords {
    pub(in super::super) intent: wire::SourceRecord,
    pub(in super::super) head: wire::SourceRecord,
    pub(in super::super) activation: wire::SourceRecord,
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn build_records(
    kind: &str,
    payload: &Value,
    key: &Value,
    binding_ref: &ExactRef,
    binding: &Value,
    actor: &str,
    at: &Timestamp,
    sequence: i64,
    prior: Option<&Value>,
    s: &mut OriginalAuditSession,
) -> Result<ActivationRecords> {
    let result = build_inner(
        kind,
        payload,
        key,
        binding_ref,
        binding,
        actor,
        at,
        sequence,
        prior,
        s,
    );
    if result.is_err() {
        s.invalidate();
    }
    result
}
#[allow(clippy::too_many_arguments)]
fn build_inner(
    kind: &str,
    payload: &Value,
    key: &Value,
    binding_ref: &ExactRef,
    binding: &Value,
    actor: &str,
    at: &Timestamp,
    sequence: i64,
    prior: Option<&Value>,
    s: &mut OriginalAuditSession,
) -> Result<ActivationRecords> {
    let normalized = normalize(kind, payload, binding_ref, binding, s)?;
    let actual = encode(payload, s)?;
    same(normalized.bytes(), actual.bytes(), s)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &0i64.into()])
        .map_err(core_error)?;
    require(sequence > 0, "INTEGRITY_FAILURE")?;
    s.semantic_work()
        .scan_bytes(actor.len())
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(!actor.is_empty(), "INTEGRITY_FAILURE")?;
    let shape = activation_verify::shape::select(
        kind,
        request_wrapper::text(payload, "schema", s)?,
        |a, b| acceptance::same_recipe_text(a, b, s),
    )?;
    let old = acceptance::counter(request_wrapper::text(payload, "expected_revision", s)?, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&old.into(), &i64::MAX.into()])
        .map_err(core_error)?;
    require(old < i64::MAX as u64, "INVALID_SCALAR")?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&old.into(), &1u64.into()])
        .map_err(core_error)?;
    let next = old.checked_add(1).ok_or_else(|| reject("INVALID_SCALAR"))?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&old.into(), &0u64.into()])
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require((old == 0) == prior.is_none(), "INTEGRITY_FAILURE")?;
    let prior = prior
        .map(|v| {
            acceptance::validate_reference(v, "workflow-head-transition", s)?;
            encode(v, s)
        })
        .transpose()?;
    let revoking = acceptance::same_recipe_text(shape.action, "revoke", s)?;
    let target_name = if revoking {
        Some("transition_ref")
    } else if acceptance::same_recipe_text(shape.action, "replace", s)? {
        Some("replaces_transition_ref")
    } else {
        None
    };
    let target = target_name
        .map(|name| encode(request_wrapper::field(payload, name, s)?, s))
        .transpose()?;
    let at_commit = if revoking {
        true
    } else {
        acceptance::same_recipe_text(
            request_wrapper::text(request_wrapper::field(payload, "effective", s)?, "kind", s)?,
            "at_commit",
            s,
        )?
    };
    let _time = s
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    let effective = if at_commit {
        at.as_str()
    } else {
        request_wrapper::text(request_wrapper::field(payload, "effective", s)?, "at", s)?
    };
    let parsed = Timestamp::parse_with_work(effective, s.semantic_work()).map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&parsed.micros().into(), &at.micros().into()])
        .map_err(core_error)?;
    require(parsed.micros() >= at.micros(), "INVALID_SHAPE")?;
    let customer = request_wrapper::text(binding, "customer", s)?;
    let agreement = request_wrapper::text(payload, "agreement_id", s)?;
    let old_text = wire::counter_string(old, s)?;
    let new_text = wire::counter_string(next, s)?;
    let _decimal = s
        .reserve_external_payload(20 + 4 * 64)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(20).map_err(core_error)?;
    s.semantic_work().write_bytes(20).map_err(core_error)?;
    let revision = next.to_string();
    let at = wire::string(at.as_str(), s)?;
    let effective = wire::string(effective, s)?;
    let actor = wire::string(actor, s)?;
    let customer_bytes = wire::string(customer, s)?;
    let agreement_bytes = wire::string(agreement, s)?;
    let kind = wire::string(kind, s)?;
    let action = wire::string(shape.action, s)?;
    let sequence = wire::counter_string(sequence as u64, s)?;
    let binding_ref = wire::reference(binding_ref, s)?;
    let scope = encode(request_wrapper::field(key, "scope", s)?, s)?;
    let key_bytes = encode(key, s)?;
    let _intent_fields = s
        .reserve_external_payload(std::mem::size_of::<[(&str, &[u8]); 3]>() * 2)
        .map_err(core_error)?;
    let intent = wire::object(
        &[
            ("kind", kind.bytes()),
            ("payload", normalized.bytes()),
            ("schema", b"\"ledger-workflow-agreement-intent/1\""),
        ],
        262144,
        s,
    )?;
    let intent = wire::source_record(
        "workflow-agreement-intent",
        "ledger-workflow-agreement-intent/1",
        intent,
        s,
    )?;
    // Fixed dictionaries retain all stack views for the emitter's lifetime.
    let _fields = s
        .reserve_external_payload(
            std::mem::size_of::<[(&str, &[u8]); 14]>() * 2
                + std::mem::size_of::<[(&str, &[u8]); 8]>() * 2,
        )
        .map_err(core_error)?;
    let mut head = [
        ("accepted_at", at.bytes()),
        ("accepted_binding_ref", binding_ref.bytes()),
        ("actor", actor.bytes()),
        ("agreement_id", agreement_bytes.bytes()),
        ("commit_sequence", sequence.bytes()),
        ("customer", customer_bytes.bytes()),
        ("head_kind", b"\"agreement_activation\"".as_slice()),
        ("new_revision", new_text.bytes()),
        ("old_revision", old_text.bytes()),
        ("operation_key", key_bytes.bytes()),
        ("prior_head_ref", b"".as_slice()),
        ("resolved_effective_time", effective.bytes()),
        (
            "schema",
            b"\"ledger-workflow-head-transition/1\"".as_slice(),
        ),
        ("scope", scope.bytes()),
    ];
    s.semantic_work()
        .write_bytes(std::mem::size_of_val(&head))
        .map_err(core_error)?;
    let body = if let Some(prior) = &prior {
        head[10].1 = prior.bytes();
        s.semantic_work()
            .write_bytes(std::mem::size_of::<&[u8]>())
            .map_err(core_error)?;
        wire::object(&head, 262144, s)?
    } else {
        // The omitted optional field is removed by moving the fixed tail.
        s.semantic_work()
            .scan_bytes(3 * std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(3 * std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        head.copy_within(11..14, 10);
        wire::object(&head[..13], 262144, s)?
    };
    let head = wire::agreement_record(
        wire::AgreementRecord::ActivationHead,
        body,
        key,
        customer,
        agreement,
        &revision,
        s,
    )?;
    let head_ref = wire::reference(head.reference(), s)?;
    let mut fields = [
        ("accepted_at", at.bytes()),
        ("action", action.bytes()),
        ("binding_ref", binding_ref.bytes()),
        ("commit_sequence", sequence.bytes()),
        ("effective_at", effective.bytes()),
        ("head_transition_ref", head_ref.bytes()),
        (
            "schema",
            b"\"ledger-workflow-activation-transition/1\"".as_slice(),
        ),
        ("target_transition_ref", b"".as_slice()),
    ];
    s.semantic_work()
        .write_bytes(std::mem::size_of_val(&fields))
        .map_err(core_error)?;
    let body = if let Some(target) = &target {
        fields[7].1 = target.bytes();
        s.semantic_work()
            .write_bytes(std::mem::size_of::<&[u8]>())
            .map_err(core_error)?;
        wire::object(&fields, 262144, s)?
    } else {
        wire::object(&fields[..7], 262144, s)?
    };
    let activation = wire::agreement_record(
        wire::AgreementRecord::Activation,
        body,
        key,
        customer,
        agreement,
        &revision,
        s,
    )?;
    Ok(ActivationRecords {
        intent,
        head,
        activation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owning_activation_records_match_existing_activate_replace_and_revoke_recipes() {
        let descriptors: Value =
            serde_json::from_slice(include_bytes!("../../testdata/descriptor-independent.json"))
                .unwrap();
        let semantics: Value =
            serde_json::from_slice(include_bytes!("../../testdata/semantic-independent.json"))
                .unwrap();
        let activation = &descriptors["operations"][2];
        let original = &activation["request_evidence"]["normalized_request"];
        let p = &original["payload"];
        let prior_value = &semantics["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "A.activation-head-v1")
            .unwrap()["ref"];
        let target_value = &semantics["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "A.activation-v1")
            .unwrap()["ref"];
        let binding_ref: RecordRef =
            serde_json::from_value(p["accepted_binding_ref"].clone()).unwrap();
        let binding_body = json!({"schema":"ledger-workflow-binding/1","customer":"customer-A","agreement_id":p["agreement_id"],"policy_ref":p["policy_ref"],"product_ref":p["product_ref"],"fact_schema_refs":p["fact_schema_refs"],"compiled_plan_ref":p["compiled_plan_ref"]});
        let binding = Document {
            reference: binding_ref.clone(),
            schema: "ledger-workflow-binding/1".into(),
            bytes: records::bytes(&binding_body).unwrap(),
        };
        for mode in 0..3 {
            let mut envelope = original.clone();
            if mode == 1 {
                envelope["payload"]["schema"] = json!("ledger-workflow-activation-replace/1");
                envelope["payload"]["expected_revision"] = json!("1");
                envelope["payload"]["replaces_transition_ref"] = target_value.clone();
                envelope["payload"]["effective"] =
                    json!({"kind":"not_before","at":"2026-10-13T00:00:30Z"});
            } else if mode == 2 {
                envelope =
                    descriptors["operations"][4]["request_evidence"]["normalized_request"].clone();
            }
            let req =
                request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
            let payload = activation_verify::normalize(&req, &binding).unwrap();
            let at = Timestamp::parse("2026-10-13T00:00:20Z").unwrap();
            let prior: Option<RecordRef> = if mode == 0 {
                None
            } else {
                Some(serde_json::from_value(prior_value.clone()).unwrap())
            };
            let target: Option<RecordRef> = if mode == 0 {
                None
            } else {
                Some(serde_json::from_value(target_value.clone()).unwrap())
            };
            let expected = activation_verify::build_records(
                &req,
                &payload,
                &activation_verify::TransitionMetadata {
                    key: req.key(),
                    actor: "fixture-admin",
                    at: &at,
                    sequence: 20,
                    binding: &binding,
                    prior,
                    target,
                },
            )
            .unwrap();
            let mut app_binding = binding_body.clone();
            app_binding["schema"] = json!("ledger-workflow-binding/2");
            let exact: ExactRef = serde_json::from_value(json!(binding_ref)).unwrap();
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let baseline = arena.usage().unwrap().live_bytes;
            let actual = build_records(
                req.kind(),
                &payload,
                &envelope["key"],
                &exact,
                &app_binding,
                "fixture-admin",
                &at,
                20,
                if mode == 0 { None } else { Some(prior_value) },
                &mut s,
            )
            .unwrap();
            for row in [&actual.intent, &actual.head, &actual.activation] {
                let wanted = expected
                    .documents
                    .iter()
                    .find(|v| v.reference.kind == row.reference().kind)
                    .unwrap();
                assert_eq!(json!(row.reference()), json!(wanted.reference));
                assert_eq!(row.bytes(), wanted.bytes);
            }
            drop(actual);
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        }
    }
}
