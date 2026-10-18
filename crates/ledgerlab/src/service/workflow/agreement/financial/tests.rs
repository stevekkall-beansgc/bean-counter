//! Disabled-shell checks. No financial acceptance, fake receipt or raw append.
use super::*;
use crate::{billing::BillingLedger, store::errors::CommitError};

async fn installation(store: &str) -> (tempfile::TempDir, LocalWorkflowLedger, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!(store);
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    (dir, ledger, path)
}
fn key(store: &str, operation: &str) -> OperationKey {
    OperationKey {
        logical_store_id: store.into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:work".into(),
        operation_id: operation.into(),
    }
}
fn envelope(key: &OperationKey) -> Value {
    // Deliberately only lexical-envelope data. No complete normalized economic
    // payload or snapshot is claimed by the disabled host path.
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"work.accept","key":key,"payload":{"schema":"ledger-workflow-work/1"}})
}

#[test]
fn financial_shell_preserves_closed_lexical_envelope_only() {
    let value = envelope(&key("disabled-store", "never-written"));
    let raw = records::bytes(&value).unwrap();
    assert_eq!(request::Request::parse_financial(&raw).unwrap().raw(), raw);
    let mut value = value.clone();
    value["prepared"] = json!(true);
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
    let duplicate = String::from_utf8(raw).unwrap().replacen(
        "\"kind\":\"work.accept\"",
        "\"kind\":\"work.accept\",\"\\u006bind\":\"work.accept\"",
        1,
    );
    assert!(request::Request::parse_financial(duplicate.as_bytes()).is_err());
    value.as_object_mut().unwrap().remove("prepared");
    value["kind"] = json!("agreement.accept");
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
}

#[tokio::test]
async fn financial_shell_denies_fresh_with_zero_delta_and_reopens_not_observed() {
    let (_dir, ledger, path) = installation("disabled-financial").await;
    let key = key("disabled-financial", "never-written");
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        ledger
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    let mut wrong = key.clone();
    wrong.scope[0] = "another-tenant".into();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&wrong)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    assert_eq!(
        reopened
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    reopened.close().await;
}

#[tokio::test]
async fn financial_shell_global_original_lookup_precedes_frozen_fresh_gate() {
    let (_dir, ledger, _path) = installation("disabled-original-order").await;
    let key = key("disabled-original-order", "existing-administrative-key");
    let content = b"Explicit operator test material; no commercial financial authority.";
    let mut proof = envelope(&key);
    proof["kind"] = json!("evidence.retain");
    proof["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"test-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-acceptor"}});
    let original = ledger
        .prerequisite(&records::bytes(&proof).unwrap())
        .await
        .unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&key)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["receipt"],
        original["receipt"]
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn financial_shell_management_frame_wrong_transaction_poisons_both_handles() {
    let (_a, first, path_a) = installation("frame-store").await;
    first.close().await;
    // Produce a genuine scoped permissions control through the existing
    // facade, so the new metadata caller exercises its retained association.
    let billing = BillingLedger::open(&path_a).await.unwrap();
    let base: Value = serde_json::from_slice(include_bytes!("../../../../../../../examples/billing/setup.json")).unwrap();
    let customer = base["customer"].as_str().unwrap();
    let source = base["source"].as_str().unwrap();
    let change = json!({"schema":"ledger-billing-permissions/2","customer":customer,"source":source,"change_id":"management-held-snapshot","expected_revision":"1","permissions":["read"],"reason":"Exercise the actual retained local administration control"});
    billing.permissions(customer,source,&records::bytes(&change).unwrap()).await.unwrap();
    // A later legitimate change may restore rights from the ORIGINAL ceiling,
    // even though the immediately preceding current set contains only read.
    let restored=json!({"schema":"ledger-billing-permissions/2","customer":customer,"source":source,"change_id":"management-original-ceiling-restore","expected_revision":"2","permissions":["read","submit","correct"],"reason":"Preserve the independent original ceiling while moving current state"});
    billing.permissions(customer,source,&records::bytes(&restored).unwrap()).await.unwrap();
    billing.close().await;
    let first = LocalWorkflowLedger::open_local(&path_a).await.unwrap();
    let (_b, second, _path_b) = installation("frame-store").await;
    let key = key("frame-store", "frame-only");
    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
    let mut session = ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let key_body = serde_json::to_value(&key).unwrap();
    let mut a = first.store.begin(deadline()).await.unwrap();
    let frame = proof_session::with_proofs(&mut a, &key, |tx| {
        Box::pin(async {
            let frame=ManagementObservation::capture(&first,tx,&key,&key_body,&mut session).await?;
            frame.recheck(&first,tx,&key,&mut session).await?;
            // SQLite's held writer does not freeze native filesystem privacy.
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                let database=path_a.join(".ledger/local.db");
                let prior=std::fs::metadata(&database).unwrap().permissions();
                std::fs::set_permissions(&database,std::fs::Permissions::from_mode(0o644)).unwrap();
                assert_eq!(frame.recheck(&first,tx,&key,&mut session).await.unwrap_err(),reject("AUTHORITY_REQUIRED"));
                std::fs::set_permissions(&database,prior).unwrap();
                frame.recheck(&first,tx,&key,&mut session).await?;
            }
            Ok(frame)
        })
    })
    .await
    .unwrap();
    let setup = first.admin.fresh(&first.store, &mut a, &key).await.unwrap();
    let state = a.load_installation().await.unwrap();
    // Exercise the actual bounded same-snapshot metadata pipeline, not only
    // its leaf decoder. Installation mismatch must not materialize identity.
    let selected = management::effective_setup(&mut a, &mut session).await.unwrap();
    assert_eq!(serde_json::to_value(selected.setup()).unwrap(), serde_json::to_value(&setup).unwrap());
    let selected_installation = a.workflow_management_installation(&setup.store_id, setup.scope.tenant(), setup.scope.environment(), &mut session).await.unwrap().unwrap();
    assert_eq!(selected_installation.installation(), &state);
    assert!(a.workflow_management_installation("different-store", setup.scope.tenant(), setup.scope.environment(), &mut session).await.unwrap().is_none());
    drop(selected_installation);
    drop(selected);
    let retained_setup = a.billing_meta().await.unwrap().setup;
    let legacy_setup = crate::service::billing::Setup::parse(&retained_setup).unwrap();
    let owned_setup = crate::service::billing::setup_owned::parse(&retained_setup, &mut session).unwrap();
    assert_eq!(serde_json::to_value(owned_setup.setup()).unwrap(), serde_json::to_value(legacy_setup).unwrap());
    drop(owned_setup);
    // Compare malformed/unsupported prefixes against the preserved legacy
    // decoder, using the actual installation's setup as the source operand.
    let initial: Value = serde_json::from_slice(&retained_setup).unwrap();
    let mut cases = Vec::new();
    for (name, value) in [("unknown", json!(true)), ("scope", json!(["x"])), ("accepted_at", json!("invalid")), ("permissions", json!(["read","read"])), ("price", json!("0")), ("source", json!("bad\nsource")), ("schema", json!("ledger-local-billing/3"))] {
        let mut input = initial.clone(); input[name] = value; cases.push(input);
    }
    let mut input = initial.clone(); input["outcome_policy"]["families"][0]["ordinary"]["accepted_by"] = json!("invalid"); cases.push(input);
    let mut input = initial.clone(); input["outcome_policy"]["limits"][0]["premium"]["atoms"] = json!("-0"); cases.push(input);
    let mut input = initial.clone(); input["outcome_policy"]["families"][0]["replacement_codes"] = json!(["absent"]); cases.push(input);
    let mut unit_setup = initial.clone();
    unit_setup["schema"] = json!("ledger-local-billing/2");
    unit_setup["unit"] = json!("call"); unit_setup["maximum_quantity"] = json!("20.000"); unit_setup["price"] = json!("1.2300");
    unit_setup["outcome_policy"]["limits"][0]["premium"]["scale"] = json!(18);
    for code in unit_setup["outcome_policy"]["families"][0]["codes"].as_array_mut().unwrap() { code["amount"]["money"]["scale"] = json!(18); }
    let valid_unit_raw = records::bytes(&unit_setup).unwrap();
    let legacy_unit = crate::service::billing::Setup::parse(&valid_unit_raw).unwrap();
    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
    let mut unit_attempt = ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let owned_unit = crate::service::billing::setup_owned::parse(&valid_unit_raw, &mut unit_attempt).unwrap();
    assert_eq!(serde_json::to_value(owned_unit.setup()).unwrap(), serde_json::to_value(legacy_unit).unwrap());
    for (field, value) in [("maximum_quantity", "0"), ("maximum_quantity", "1.5"), ("maximum_quantity", "0.10"), ("maximum_quantity", "1000000000000000000000000000000"), ("price", "1.0000000000000000000"), ("price", "100000000000000000000000000000")] {
        let mut input = unit_setup.clone(); input[field] = json!(value); cases.push(input);
    }
    for input in cases {
        let raw = records::bytes(&input).unwrap();
        let legacy = crate::service::billing::Setup::parse(&raw).unwrap_err();
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut attempt = ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let owned = crate::service::billing::setup_owned::parse(&raw, &mut attempt).err().unwrap();
        assert_eq!(owned, legacy);
    }
    let legacy = records::bytes(&json!([
        key,
        setup.grant_revision.to_string(),
        setup,
        state.logical_store_id,
        state.scope.tenant,
        state.scope.environment,
        state.mode,
        state.admission,
        state.dispatch_hold,
        state.dispatch_enabled,
        state.generation.to_string()
    ])).unwrap();
    assert_eq!(frame.fingerprint_bytes(), legacy);
    let mut b = second.store.begin(deadline()).await.unwrap();
    assert_eq!(
        frame.recheck(&second, &mut b, &key, &mut session).await.unwrap_err(),
        ServiceError::IntegrityFailure
    );
    assert!(matches!(a.commit().await, Err(CommitError::RolledBack(_))));
    assert!(matches!(b.commit().await, Err(CommitError::RolledBack(_))));
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn chronological_owning_frames_follow_actual_public_prerequisite_history() {
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_directory, ledger, _path) = installation("actual-prefix").await;
    let mut receipts = Vec::new();
    for (operation, content) in [
        ("first", b"Original customer evidence".as_slice()),
        ("second", b"Second original customer evidence".as_slice()),
    ] {
        let mut input = envelope(&key("actual-prefix", operation));
        input["kind"] = json!("evidence.retain");
        input["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"prefix-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"prefix-acceptor"}});
        receipts.push(
            ledger
                .prerequisite(&records::bytes(&input).unwrap())
                .await
                .unwrap(),
        );
    }
    let through = receipts[1]["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let root = key("actual-prefix", "prefix-reader");
    let arena = AuditArena::new().unwrap();
    let baseline = arena.usage().unwrap().live_bytes;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &root, |tx| {
        Box::pin(async {
            let mut after = 0;
            for (operation, receipt) in [("first", &receipts[0]), ("second", &receipts[1])] {
                let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let next = prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .unwrap();
                assert_eq!(next.key(), &key("actual-prefix", operation));
                after = next.frame.sequence;
                let resolved = next.checked_wrappers(tx, &mut session).await?;
                assert_eq!(
                    resolved.wrappers().class(),
                    wrappers::WireClass::Prerequisite
                );
                assert_eq!(resolved.wrappers().receipt(), &receipt["receipt"]);
                // Exact saved request is read from the original row, not recreated
                // from the receipt or supplied as an expected construction operand.
                assert_eq!(
                    resolved.wrappers().original_request().value()["key"]["operation_id"],
                    operation
                );
                // The connected current/prefix document reader is exercised
                // only with a genuine public prerequisite's retained member.
                // No synthetic financial history or provenance token is made.
                let reference: ledgerlab_core::policy::workflow::transition_constructor::ExactRef =
                    serde_json::from_value(
                        receipt["receipt"]["result_refs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["kind"] == "workflow-evidence")
                            .unwrap()
                            .clone(),
                    )
                    .unwrap();
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let foreign = ["foreign-tenant".into(), root.scope[1].clone()];
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &foreign,
                        through + 1,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let live_before = arena.usage().unwrap().live_bytes;
                let retained = tx
                    .workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after + 1,
                        &mut session,
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .unwrap();
                assert_eq!(
                    retained.schema.bytes(),
                    b"ledger-workflow-retained-evidence/1"
                );
                let body = canonical::parse_bounded(retained.body.bytes(), 4194304).unwrap();
                assert_eq!(
                    body["content_sha256"],
                    resolved.wrappers().original_request().value()["payload"]["content_sha256"]
                );
                assert!(arena.usage().unwrap().live_bytes > live_before);
                drop(retained);
                assert_eq!(arena.usage().unwrap().live_bytes, live_before);
                drop(resolved);
            }
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(
                prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    // This test qualifies mechanical chronological acquisition only. It creates
    // neither an AUTH02 CompletedOrigin nor a financial receipt/candidate.
    ledger.close().await;
}

#[tokio::test]
async fn actual_local_party_retention_commits_retries_and_reopens() {
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_directory, ledger, path) = installation("actual-party-input").await;
    let operation = key("actual-party-input", "party-authorization-proof");
    let material = b"Explicit local operator representation material";
    let spec = json!({"authorization_id":"platform-owner-mapping","scope":operation.scope,
        "customer":"customer-1","agreement_id":"agreement-1","party":"platform-1",
        "acceptor":"platform-representative","roles":["bearer","payer"],
        "starts_at":"2026-01-01T00:00:00.000000Z","ends_at":"2099-01-01T00:00:00.000000Z"});
    let mut input = envelope(&operation);
    input["kind"] = json!("evidence.retain");
    input["payload"] = json!({"schema":"ledger-workflow-party-evidence-retain/1",
        "subject":{"customer":"customer-1","agreement_id":"agreement-1","party":"platform-1","acceptor":"platform-representative"},
        "media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(material),
        "content_sha256":records::raw_hash(material),"metadata":{"purpose":"party_authorization","authorization":spec}});
    let raw = records::bytes(&input).unwrap();
    let arena = AuditArena::new().unwrap();
    let baseline = arena.usage().unwrap().live_bytes;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &operation, |tx| {
        Box::pin(async {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let request = ingress::IncomingRequest::parse(&raw, &mut session)?;
            let held=ledger.admin.read_management(&ledger.store,tx,request.key(),&mut session).await?;
            let key_body=request_wrapper::field(request.value(),"key",&mut session)?;
            let management=ManagementObservation::capture_owned(&ledger,tx,request.key(),key_body,held,&mut session).await?;
            management.recheck(&ledger,tx,request.key(),&mut session).await?;
            let pending=current::party::produce::prepare_observed(&ledger,tx,&request,management.held(),&mut session).await?;
            for record in [&pending.root, &pending.authority, &pending.result] {
                let body: Value = serde_json::from_slice(record.bytes()).unwrap();
                let expected = records::source_document(&record.reference().kind, body).unwrap();
                assert_eq!(record.bytes(), expected.bytes);
                assert_eq!(record.reference().kind, expected.reference.kind);
                assert_eq!(record.reference().id, expected.reference.id);
                assert_eq!(record.reference().digest, expected.reference.digest);
            }
            let body: Value = serde_json::from_slice(pending.result.bytes()).unwrap();
            assert_eq!(body["retained_by"], "example-operator");
            assert_eq!(body["installation_mode"], "real_local");
            assert_eq!(body["scope"], json!(operation.scope));
            assert_eq!(body["metadata"], input["payload"]["metadata"]);
            assert_eq!(body["retained_at"], json!(pending.at));
            assert_eq!(pending.dependency_refs().count(), 0);
            assert_eq!(
                pending.normalized_payload(),
                records::bytes(&input["payload"]).unwrap()
            );
            drop(pending);
            drop(management);
            drop(request);
            drop(session);
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    let committed=ledger.financial(&raw).await.unwrap();
    let retried=ledger.financial(&raw).await.unwrap();
    assert_eq!(retried["receipt"],committed["receipt"]);
    assert_eq!(retried["duplicate_kind"],"identity");
    let status=ledger.status(&operation.bytes().unwrap()).await.unwrap();
    assert_eq!(status["schema"],"ledger-operation-status/1");
    assert_eq!(status["receipt"],committed["receipt"]);
    let mut tx=ledger.store.begin(deadline()).await.unwrap();
    let committed_meter=tx.workflow_test_meter().await.unwrap();
    assert_ne!(committed_meter,before);
    tx.rollback().await.unwrap();
    // The empty-prefix case covers only a recipe with no retained operands.
    // A genuine immediate target cannot disappear into that representation.
    let mut missing = input.clone();
    missing["key"]["operation_id"]=json!("missing-party-target");
    missing["payload"]["metadata"] = json!({
        "purpose":"party_operator_attestation",
        "statement":"operator_verified_party_acceptor_authority",
        "target_evidence_ref":{"kind":"workflow-evidence","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))}
    });
    assert_eq!(
        ledger
            .financial(&records::bytes(&missing).unwrap())
            .await
            .unwrap_err(),
        reject("AUTHORITY_REQUIRED")
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), committed_meter);
    tx.rollback().await.unwrap();
    input["payload"]["content_sha256"] = json!("0".repeat(64));
    assert!(
        ledger
            .financial(&records::bytes(&input).unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        ledger.status(&operation.bytes().unwrap()).await.unwrap()["status"],
        "committed"
    );
    ledger.close().await;
    let reopened=LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(reopened.status(&operation.bytes().unwrap()).await.unwrap()["receipt"],committed["receipt"]);
    assert_eq!(reopened.financial(&raw).await.unwrap()["receipt"],committed["receipt"]);
    reopened.close().await;
}

#[tokio::test]
async fn chronological_fold_replays_real_administrative_source_chain_under_writer() {
    let (_dir, ledger, _path) = installation("chronological-real-source").await;
    let envelope = |id: &str, kind: &str, payload: Value| json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":kind,"key":key("chronological-real-source",id),"payload":payload});
    let retain = |id: &str, purpose: &str, metadata: Value| {
        envelope(
            id,
            "evidence.retain",
            json!({"schema":"ledger-workflow-evidence-retain/1","purpose":purpose,"subject":{"customer":"fold-customer"},"media_type":"text/plain;charset=utf-8","content_base64":"eA==","content_sha256":records::raw_hash(b"x"),"metadata":metadata}),
        )
    };
    let customer_proof = ledger
        .prerequisite(
            &records::bytes(&retain(
                "customer-proof",
                "customer_registration",
                json!({"acceptor":"fold-acceptor"}),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let registered=ledger.prerequisite(&records::bytes(&envelope("customer","authority.change",json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":"fold-customer","acceptor":"fold-acceptor","registration_evidence_ref":customer_proof["receipt"]["result_refs"][0]}))).unwrap()).await.unwrap();
    let source_proof = ledger
        .prerequisite(
            &records::bytes(&retain(
                "source-proof",
                "source_enrollment",
                json!({"source":"urn:example:work","principal_id":"example-operator"}),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let ceiling = json!({"principal_id":"example-operator","source":"urn:example:work","canonical_economic_namespace":"urn:example:work","customer_scope":["fold-customer"],"operations":["work.accept","quantity.correct"],"product_selectors":[{"names":["fold:product"]}],"activity_selectors":[{"names":["fold:work"]}],"link_selectors":[],"source_slots":["fold:worker"],"permissions":["read","submit","correct"],"starts_at":"2026-01-01T00:00:00Z","ends_at":"2030-01-01T00:00:00Z"});
    let enrolled=ledger.prerequisite(&records::bytes(&envelope("enroll","authority.change",json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"fold-source","customer_refs":[registered["receipt"]["result_refs"][0]],"ceiling":ceiling,"evidence_ref":source_proof["receipt"]["result_refs"][0]}))).unwrap()).await.unwrap();
    assert_eq!(enrolled["receipt"]["commit_sequence"], "4");
    let selected = key("chronological-real-source", "enroll");
    let raw_key = records::bytes(&serde_json::to_value(&selected).unwrap()).unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let failure = tx.workflow_proof_failure_handle();
    proof_session::with_proofs(&mut tx, &selected, |tx| {
        Box::pin(async {
            let arena =
                ledgerlab_core::canonical::audit_arena::AuditArena::new().map_err(core_error)?;
            let mut session =
                ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(
                    &arena,
                )
                .map_err(core_error)?;
            let identity = session
                .copy_representation(
                    &raw_key,
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let selected_origin = prefix::target(tx, &selected, identity, &mut session)
                .await?
                .checked_wrappers(tx, &mut session)
                .await?;
            let mut folded = prefix::fold_original_prefix(
                &ledger,
                tx,
                &selected,
                &selected_origin,
                &mut session,
            )
            .await?;
            // This lookup can return only the live key associated AFTER the
            // actual enrollment origin and its required-edge/pointer finish.
            let enrollment_ref = &enrolled["receipt"]["result_refs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["kind"] == "workflow-source-enrollment")
                .unwrap();
            {
                let correspondence = std::ptr::from_ref(
                    folded.initial_source_correspondence(enrollment_ref, &mut session)?,
                );
                assert_eq!(
                    correspondence,
                    std::ptr::from_ref(
                        folded.initial_source_correspondence(enrollment_ref, &mut session)?
                    )
                );
            }
            // SAME selected-body owner used by the app caller, over actual
            // completed prerequisite records. This emits no financial header,
            // original, economic result or capability.
            {
                use ledgerlab_core::policy::workflow::transition_constructor::OperationKey as CoreKey;
                let core_key=CoreKey{logical_store_id:selected.logical_store_id.clone(),scope:selected.scope.clone(),source:selected.source.clone(),operation_id:selected.operation_id.clone()};
                let seeds=&enrolled["receipt"]["result_refs"];
                let inputs=prefix::ResolvedEconomicInputs::select(&mut folded,&core_key,seeds,&json!([]),&mut session)?;
                let bodies=folded.retain_economic_inputs(inputs,&mut session)?;
                let views=bodies.views(&mut session)?;
                assert_eq!(views.rows.len(),bodies.selected().members.len());
                assert!(views.rows.iter().any(|row|row.reference.kind=="workflow-source-enrollment"));
                assert!(views.rows.iter().any(|row|row.reference.kind=="workflow-source-state"));
                for row in &views.rows {
                    assert!(!row.canonical_body.is_empty());
                    let body=session.parse_representation(row.canonical_body,ledgerlab_core::validation_work::RepresentationKind::Record).map_err(core_error)?;
                    folded._inventory.compare_record(row.reference,row.canonical_body,&mut session)?;
                    assert!(body.value().is_object());
                }
            }
            // These pointers come from the actual four public committed
            // prerequisite recipes, after their original semantic/custody fold.
            // They are not a financial witness membership or audit digest.
            for (sequence, id) in [
                (1, "customer-proof"),
                (2, "customer"),
                (3, "source-proof"),
                (4, "enroll"),
            ] {
                let raw = folded._inventory.origin_pointer(sequence, &mut session)?;
                let pointer = session
                    .parse_representation(
                        raw.bytes(),
                        ledgerlab_core::validation_work::RepresentationKind::Record,
                    )
                    .map_err(core_error)?;
                assert_eq!(pointer.value()["commit_sequence"], sequence.to_string());
                assert_eq!(pointer.value()["operation_key"]["operation_id"], id);
                assert_eq!(
                    pointer.value()["operation_key"]["logical_store_id"],
                    "chronological-real-source"
                );
                assert_eq!(
                    pointer.value()["operation_ref"]["kind"],
                    "workflow-operation"
                );
                assert_eq!(pointer.value()["manifest_ref"]["kind"], "workflow-manifest");
                assert_eq!(pointer.value()["receipt_ref"]["kind"], "workflow-receipt");
            }
            // Adjacencies come from the Ref/body associations checked by
            // those same recipes, not from an asserted pointer list. Every
            // edge goes strictly backward; own introductions add no edge.
            for (sequence, required) in
                [(1, &[][..]), (2, &[1][..]), (3, &[][..]), (4, &[2, 3][..])]
            {
                let links = folded._inventory.origin_links(sequence, &mut session)?;
                let n = u32::from_le_bytes(links.bytes()[..4].try_into().unwrap()) as usize;
                let middle = 8 + 8 * n;
                let mut values: Vec<u32> = links.bytes()[8..middle]
                    .chunks_exact(8)
                    .flat_map(|b| {
                        let word = u32::from_le_bytes(b[..4].try_into().unwrap());
                        let mask = u32::from_le_bytes(b[4..8].try_into().unwrap());
                        (0..32)
                            .filter(move |bit| mask & (1 << bit) != 0)
                            .map(move |bit| word * 32 + bit + 1)
                    })
                    .collect();
                values.extend(
                    links.bytes()[middle..]
                        .chunks_exact(256)
                        .map(|b| u32::from_le_bytes(b[228..232].try_into().unwrap())),
                );
                assert!(
                    values
                        .iter()
                        .all(|prior| *prior > 0 && *prior < sequence as u32)
                );
                for prior in required {
                    assert!(values.contains(prior));
                }
                // Later operations can reuse an earlier exact immutable
                // authority body; its earliest origin remains an extra edge.
                if sequence == 1 {
                    assert!(values.is_empty());
                }
            }
            let first = prefix::next(tx, &selected, 0, 1, &mut session)
                .await?
                .ok_or(ServiceError::IntegrityFailure)?
                .checked_wrappers(tx, &mut session)
                .await?;
            let root = &first.wrappers().operation()["authority_snapshot"]["authority_refs"][0];
            // Independent retained SQL meter + exact manifest sets check the
            // append-derived accounting, including reused documents once.
            let expected_meter = tx
                .workflow_test_meter()
                .await
                .map_err(crate::service::store_error)?;
            let mut expected_documents = std::collections::BTreeSet::new();
            for sequence in 1..=4 {
                let saved = prefix::next(tx, &selected, sequence - 1, sequence, &mut session)
                    .await?
                    .ok_or(ServiceError::IntegrityFailure)?
                    .checked_wrappers(tx, &mut session)
                    .await?;
                for member in saved.wrappers().manifest()["members"].as_array().unwrap() {
                    if member["kind"] != "workflow-operation" {
                        expected_documents.insert(records::bytes(member).unwrap());
                    }
                }
            }
            let retained_root = current::source::retained_record(
                tx,
                &selected,
                root,
                "workflow-authority",
                "ledger-workflow-local-authority-snapshot/1",
                5,
                &mut session,
            )
            .await?;
            let root_body = session
                .copy_representation(
                    retained_root.canonical_body(),
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let accounting_root = wire::source_record(
                "workflow-authority",
                "ledger-workflow-local-authority-snapshot/1",
                root_body,
                &mut session,
            )?;
            let cut = prefix::inventory::ConfiguredRoot::from_value(root, &mut session)?;
            assert_eq!(
                folded._inventory.seed_origin(root, &cut, &mut session)?,
                None
            );
            assert_eq!(
                folded._inventory.seed_origin(
                    &registered["receipt"]["result_refs"][0],
                    &cut,
                    &mut session
                )?,
                Some(2)
            );
            let _reader = session
                .reserve_external_payload(prefix::inventory::REQUIRED_EDGE_READER_PAYLOAD)
                .map_err(core_error)?;
            // These genuine committed prefixes use dense sequences1..4;
            // the stored masks themselves are ordinal keyed, not sequence keyed.
            for (ordinal, expected) in [(1, vec![1]), (2, vec![])] {
                let mut direct = Vec::new();
                folded._inventory.required_word_masks_at(
                    ordinal,
                    &cut,
                    &mut |word, mask, _| {
                        for bit in 0..32 {
                            if mask & (1 << bit) != 0 {
                                direct.push((word * 32 + bit + 1) as i64);
                            }
                        }
                        Ok(())
                    },
                    &mut session,
                )?;
                assert_eq!(direct, expected);
            }
            folded.assert_packed_reachable(
                root,
                &[&registered["receipt"]["result_refs"][0]],
                &[0b11],
                &mut session,
            )?;
            folded.assert_packed_reachable(
                root,
                &[&enrolled["receipt"]["result_refs"][0]],
                &[0b1111],
                &mut session,
            )?;
            folded.assert_packed_accounting(
                &accounting_root,
                &[0b1111],
                (
                    expected_documents.len() as u64,
                    expected_meter.payload_bytes as u64,
                ),
                &mut session,
            )?;
            // A cut root outside an empty selected set is included exactly
            // once; neither unrelated journal bytes nor its origin are added.
            folded.assert_packed_accounting(
                &accounting_root,
                &[0],
                (1, retained_root.canonical_body().len() as u64),
                &mut session,
            )?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(!failure.load(Ordering::Acquire));
    tx.rollback().await.unwrap();
    // This changed-route check creates no economic/prepared/financial result.
    assert_eq!(
        ledger.status(&raw_key).await.unwrap()["receipt"],
        enrolled["receipt"]
    );
}

#[tokio::test]
async fn fresh_work_projection_reads_real_empty_installation_without_writes() {
    use crate::store::sqlite::workflow::financial::HeadKey;
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena,
        policy::workflow::transition_constructor::PermanentClaimKey,
        validation_work::OriginalAuditSession,
    };
    let (_dir, ledger, _path) = installation("work-projection-empty").await;
    let key = key("work-projection-empty", "read-only-work-locators");
    let arena = AuditArena::new().unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let held_ledger = &ledger;
    proof_session::with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            // Empty real prerequisite prefix, actual local admin, existing
            // mechanical migration only; no financial origins are seeded.
            held_ledger
                .admin
                .fresh(&held_ledger.store, tx, &key)
                .await?;
            assert_eq!(
                tx.workflow_sequence()
                    .await
                    .map_err(crate::service::store_error)?,
                1
            );
            tx.workflow_enable_financial_schema()
                .await
                .map_err(crate::service::store_error)?;
            let before = tx
                .workflow_test_meter()
                .await
                .map_err(crate::service::store_error)?;
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let claim = PermanentClaimKey::Work {
                logical_store_id: key.logical_store_id.clone(),
                scope: key.scope.clone(),
                source_namespace: key.source.clone(),
                business_operation_id: "new-event".into(),
                completion: "completed".into(),
            };
            assert!(
                tx.workflow_work_claim(&claim, &mut s)
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
            );
            assert!(
                tx.workflow_family_head_by_target_identity(
                    &key.logical_store_id,
                    &key.scope,
                    "new-target",
                    "assessment",
                    &mut s
                )
                .await
                .map_err(prefix::read_error)?
                .is_none()
            );
            for locator in [
                HeadKey::Decision("new-decision"),
                HeadKey::Capacity("new-decision"),
                HeadKey::Chain {
                    customer: "customer",
                    chain: "service",
                },
            ] {
                assert!(
                    tx.workflow_financial_head(&key.logical_store_id, &key.scope, locator, &mut s)
                        .await
                        .map_err(prefix::read_error)?
                        .is_none()
                );
            }
            assert_eq!(
                tx.workflow_test_meter()
                    .await
                    .map_err(crate::service::store_error)?,
                before
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn held_management_reuse_refuses_actual_same_transaction_mutation() {
    use ledgerlab_core::{canonical::audit_arena::AuditArena,validation_work::OriginalAuditSession};
    let (_directory,ledger,_path)=installation("held-management-change").await;
    let key=key("held-management-change","held-management-change");
    let arena=AuditArena::new().unwrap();
    let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
    let mut tx=ledger.store.begin(deadline()).await.unwrap();
    let error=proof_session::with_proofs(&mut tx,&key,|tx|Box::pin(async {
        let held=ledger.admin.read_management(&ledger.store,tx,&key,&mut session).await?;
        ledger.admin.recheck_management_fresh(&held,&ledger.store,tx,&key,&mut session).await?;
        // An actual existing validated control producer changes this connection.
        // Even equal resulting rights must invalidate the captured baseline.
        let snapshot=tx.billing_meta().await.map_err(crate::service::store_error)?;
        let setup=held.setup();
        let raw=records::bytes(&json!({"schema":"ledger-billing-permissions/2","customer":setup.customer,"source":setup.source,"change_id":"after-management-read","expected_revision":setup.grant_revision.to_string(),"permissions":setup.permissions,"reason":"Validate held snapshot mutation refusal"}))?;
        let (_,plan)=crate::service::billing::permissions::prepare_scoped(&snapshot,&setup.customer,&setup.source,&raw,&crate::local::now().unwrap())?;
        tx.billing_m2_permissions(&plan.ok_or(ServiceError::IntegrityFailure)?).await.map_err(crate::service::store_error)?;
        ledger.admin.recheck_management_fresh(&held,&ledger.store,tx,&key,&mut session).await
    })).await.unwrap_err();
    assert_eq!(error,reject("STALE_REVISION"));
    assert!(matches!(tx.commit().await,Err(CommitError::RolledBack(_))));
    ledger.close().await;
}
