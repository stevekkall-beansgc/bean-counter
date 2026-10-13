use super::*;
fn at(second: u8) -> Timestamp {
    Timestamp::parse(&format!("2026-10-13T00:00:{second:02}Z")).unwrap()
}
// Closed record operands test schedule semantics only; no operator or grant is
// authenticated and no transaction/facade success is implied by these tests.
fn make(
    old: i64,
    accepted: u8,
    effective: u8,
    sequence: i64,
    end: Option<u8>,
    prior: Option<RecordRef>,
    target: Option<RecordRef>,
) -> Transition {
    let fixtures: Value =
        serde_json::from_slice(include_bytes!("../../testdata/descriptor-independent.json"))
            .unwrap();
    let mut envelope = fixtures["operations"][2]["request_evidence"]["normalized_request"].clone();
    let p = &mut envelope["payload"];
    let mut body = json!({"schema":"ledger-workflow-binding/1","scope":["fixture","sandbox"],"customer":"customer-A","agreement_id":"agreement-A",
        "policy_ref":p["policy_ref"],"product_ref":p["product_ref"],"fact_schema_refs":p["fact_schema_refs"],"compiled_plan_ref":p["compiled_plan_ref"],"starts_at":at(if end.is_some(){10}else{0})});
    if let Some(end) = end {
        body["ends_at"] = json!(at(end));
    }
    let binding = records::source_document("workflow-binding", body).unwrap();
    p["accepted_binding_ref"] = json!(binding.reference);
    p["expected_revision"] = json!(old.to_string());
    p["effective"] = json!({"kind":"not_before","at":at(effective)});
    if let Some(target) = &target {
        p["schema"] = json!("ledger-workflow-activation-replace/1");
        p["replaces_transition_ref"] = json!(target);
    }
    envelope["key"]["operation_id"] = json!(format!("schedule-{sequence}"));
    let req = request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
    let payload = normalize(&req, &binding).unwrap();
    let accepted = at(accepted);
    let prepared = build_records(
        &req,
        &payload,
        &TransitionMetadata {
            key: req.key(),
            actor: "fixture-admin",
            at: &accepted,
            sequence,
            binding: &binding,
            prior,
            target,
        },
    )
    .unwrap();
    let mut docs = prepared.documents.into_iter();
    docs.next();
    transition(
        req.key(),
        "customer-A",
        "agreement-A",
        docs.next().unwrap(),
        docs.next().unwrap(),
        binding,
    )
    .unwrap()
}
#[test]
fn certified_boundary_selection_expires_without_falling_back_and_replace_stays_pending() {
    let v1 = make(0, 0, 0, 1, None, None, None);
    check_next(&[], &v1).unwrap();
    let v2 = make(1, 5, 10, 2, Some(20), Some(v1.head.reference.clone()), None);
    check_next(std::slice::from_ref(&v1), &v2).unwrap();
    let first = v1.binding.reference.clone();
    let second = v2.binding.reference.clone();
    let history = vec![v1, v2];
    assert_eq!(
        selected_binding(&history, &at(9)).unwrap(),
        Some(first.clone())
    );
    for t in [10, 19] {
        assert_eq!(
            selected_binding(&history, &at(t)).unwrap(),
            Some(second.clone())
        );
    }
    for t in [20, 21] {
        assert_eq!(selected_binding(&history, &at(t)).unwrap(), None);
    }
    let replace = make(
        2,
        6,
        15,
        3,
        Some(20),
        Some(history[1].head.reference.clone()),
        Some(history[1].activation.reference.clone()),
    );
    check_next(&history, &replace).unwrap();
    let third = replace.binding.reference.clone();
    let mut history = history;
    history.push(replace);
    assert_eq!(
        selected_binding(&history, &at(5)).unwrap(),
        Some(first.clone())
    );
    assert_eq!(selected_binding(&history, &at(14)).unwrap(), Some(first));
    assert_eq!(selected_binding(&history, &at(15)).unwrap(), Some(third));
    assert_eq!(selected_binding(&history, &at(20)).unwrap(), None);
}
#[test]
fn competing_pending_wrong_target_and_replacement_at_boundary_refuse() {
    let v1 = make(0, 0, 0, 1, None, None, None);
    let v2 = make(1, 5, 10, 2, Some(20), Some(v1.head.reference.clone()), None);
    let history = vec![v1, v2];
    let competing = make(
        2,
        6,
        15,
        3,
        Some(20),
        Some(history[1].head.reference.clone()),
        None,
    );
    assert!(check_next(&history, &competing).is_err());
    let wrong = make(
        2,
        6,
        15,
        3,
        Some(20),
        Some(history[1].head.reference.clone()),
        Some(history[0].activation.reference.clone()),
    );
    assert!(check_next(&history, &wrong).is_err());
    let late = make(
        2,
        10,
        15,
        3,
        Some(20),
        Some(history[1].head.reference.clone()),
        Some(history[1].activation.reference.clone()),
    );
    assert!(check_next(&history, &late).is_err());
}
fn stop(history: &[Transition], accepted: u8, target: &RecordRef) -> Transition {
    let previous = history.last().unwrap();
    let binding = Document {
        reference: previous.binding.reference.clone(),
        schema: previous.binding.schema.clone(),
        bytes: previous.binding.bytes.clone(),
    };
    let envelope = json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"agreement.revoke",
        "key":{"logical_store_id":"fixture-store","scope":["fixture","sandbox"],"source":"urn:fixture:publisher","operation_id":"schedule-stop"},
        "payload":{"schema":"ledger-workflow-agreement-revoke/1","agreement_id":"agreement-A","accepted_binding_ref":binding.reference,
            "expected_revision":previous.revision.to_string(),"transition_ref":target,"reason":"explicit local stop",
            "evidence_refs":[{"kind":"workflow-evidence","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))}]}});
    let req = request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
    let p = normalize(&req, &binding).unwrap();
    let accepted = at(accepted);
    let prepared = build_records(
        &req,
        &p,
        &TransitionMetadata {
            key: req.key(),
            actor: "fixture-admin",
            at: &accepted,
            sequence: previous.sequence + 1,
            binding: &binding,
            prior: Some(previous.head.reference.clone()),
            target: Some(target.clone()),
        },
    )
    .unwrap();
    let mut docs = prepared.documents.into_iter();
    docs.next();
    transition(
        req.key(),
        "customer-A",
        "agreement-A",
        docs.next().unwrap(),
        docs.next().unwrap(),
        binding,
    )
    .unwrap()
}
#[test]
fn pending_stop_requires_explicit_pending_edge_and_expired_binding_can_stop_without_fallback() {
    let v1 = make(0, 0, 0, 1, None, None, None);
    let v2 = make(1, 5, 10, 2, Some(20), Some(v1.head.reference.clone()), None);
    let mut history = vec![v1, v2];
    let wrong = stop(&history, 6, &history[0].activation.reference);
    assert!(check_next(&history, &wrong).is_err());
    let good = stop(&history, 6, &history[1].activation.reference);
    check_next(&history, &good).unwrap();
    history.push(good);
    assert_eq!(
        selected_binding(&history, &at(4)).unwrap(),
        Some(history[0].binding.reference.clone())
    );
    for t in [6, 10, 20] {
        assert_eq!(selected_binding(&history, &at(t)).unwrap(), None);
    }
    let v1 = make(0, 0, 0, 1, None, None, None);
    let v2 = make(1, 5, 10, 2, Some(20), Some(v1.head.reference.clone()), None);
    let history = vec![v1, v2];
    let expired_stop = stop(&history, 21, &history[1].activation.reference);
    check_next(&history, &expired_stop).unwrap();
}
