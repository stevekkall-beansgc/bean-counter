//! Actual local-owner prerequisite path. Test proof material is explicitly supplied;
//! no synthetic dictionary is used by a production authority/evidence factory.
use super::*;
use crate::{
    billing::BillingLedger,
    service::workflow::LocalWorkflowLedger,
    store::ports::{AcceptanceStore, AcceptanceTx},
};
async fn installation() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!("prerequisite-path-test");
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    (dir, path)
}
fn envelope(id: &str, kind: &str, payload: Value) -> Value {
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":kind,"key":{"logical_store_id":"prerequisite-path-test","scope":["example-company","local"],"source":"urn:example:work","operation_id":id},"payload":payload})
}
fn proof_payload() -> Value {
    let content=b"Explicit test proof material supplied through the local operator route; no commercial acceptance.";
    json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"test-customer-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-acceptor-A"}})
}
#[tokio::test]
async fn agreement_actual_owner_evidence_registration_original_alias_conflict_reopen() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let proof = envelope("explicit-proof", "evidence.retain", proof_payload());
    let raw = records::bytes(&proof).unwrap();
    let committed = ledger.prerequisite(&raw).await.unwrap();
    assert_eq!(committed["receipt"]["commit_sequence"], "1");
    assert_eq!(committed["receipt"]["result_kind"], "administrative");
    let proof_ref: RecordRef =
        serde_json::from_value(committed["receipt"]["result_refs"][0].clone()).unwrap();
    let key = super::super::request::OperationKey::parse(&records::bytes(&proof["key"]).unwrap())
        .unwrap();
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let retained = loaded(&mut tx, &key, &proof_ref).await.unwrap();
    let body = canonical_body(&retained).unwrap();
    assert_eq!(body["retained_by"], "example-operator");
    assert_eq!(body["installation_mode"], "real_local");
    assert_eq!(body["content_base64"], proof["payload"]["content_base64"]);
    tx.rollback().await.unwrap();
    let pretty = serde_json::to_vec_pretty(&proof).unwrap();
    let retry = ledger.prerequisite(&pretty).await.unwrap();
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], committed["receipt"]);
    assert_eq!(retry["request_evidence"], committed["request_evidence"]);
    let mut alias = proof.clone();
    alias["key"]["operation_id"] = json!("proof-new-key");
    let aliased = ledger
        .prerequisite(&records::bytes(&alias).unwrap())
        .await
        .unwrap();
    assert_eq!(aliased["duplicate_kind"], "semantic");
    assert_eq!(
        aliased["receipt"]["result_refs"],
        committed["receipt"]["result_refs"]
    );
    let registration = envelope(
        "explicit-register",
        "authority.change",
        json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":"test-customer-A","acceptor":"test-acceptor-A","registration_evidence_ref":proof_ref}),
    );
    let registered = ledger
        .prerequisite(&records::bytes(&registration).unwrap())
        .await
        .unwrap();
    assert_eq!(registered["receipt"]["commit_sequence"], "3");
    let mut alias = registration.clone();
    alias["key"]["operation_id"] = json!("register-new-key");
    let aliased = ledger
        .prerequisite(&records::bytes(&alias).unwrap())
        .await
        .unwrap();
    assert_eq!(aliased["duplicate_kind"], "semantic");
    assert_eq!(
        aliased["receipt"]["result_refs"],
        registered["receipt"]["result_refs"]
    );
    let mut different = registration.clone();
    different["payload"]["acceptor"] = json!("other-acceptor");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&different).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    different["key"]["operation_id"] = json!("refuse-changed-registration");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&different).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="SEMANTIC_CONFLICT")
    );
    let mut crosskind = proof.clone();
    crosskind["kind"] = json!("authority.change");
    crosskind["payload"] = registration["payload"].clone();
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&crosskind).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    // A globally retained key also conflicts through the older publication
    // facade before compiler/build preparation or non-publication pair decoding.
    // Reuse the immutable publication oracle; no fresh publication is required.
    let publication: Value = serde_json::from_str(include_str!(
        "../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json"
    ))
    .unwrap();
    for retained_key in [&proof["key"], &registration["key"]] {
        let mut attempt = publication["normalized_request"].clone();
        attempt["key"] = retained_key.clone();
        let attempted_raw = records::bytes(&attempt).unwrap();
        super::super::request::PublicationRequest::parse(&attempted_raw).unwrap();
        assert!(
            matches!(ledger.publish(&attempted_raw).await, Err(ServiceError::Rejection(code)) if code == "IDENTITY_CONFLICT")
        );
    }
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 4);
    tx.rollback().await.unwrap();
    let status = ledger.status(&key.bytes().unwrap()).await.unwrap();
    assert_eq!(status["receipt"], committed["receipt"]);
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap(),
        status
    );
    assert_eq!(
        reopened.prerequisite(&pretty).await.unwrap()["receipt"],
        committed["receipt"]
    );
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(
        tx.workflow_test_meter().await.unwrap().payload_bytes,
        meter.payload_bytes
    );
    tx.rollback().await.unwrap();
    reopened.close().await;
}
#[tokio::test]
async fn agreement_explicit_attestation_requires_prior_exact_subject_terms_and_actor() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let digest = format!("sha256:{}", "7".repeat(64));
    let content = b"Explicit test assent material only, never commercial activation.";
    let assent = envelope(
        "assent-material",
        "evidence.retain",
        json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"assent","subject":{"customer":"test-customer-A","agreement_id":"test-agreement-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"terms_digest":digest,"acceptor":"test-acceptor-A","accepted_at":"2026-10-13T00:00:00.000000Z"}}),
    );
    let saved = ledger
        .prerequisite(&records::bytes(&assent).unwrap())
        .await
        .unwrap();
    let r = &saved["receipt"]["result_refs"][0];
    let attestation = envelope(
        "explicit-attestation",
        "evidence.retain",
        json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"operator_attestation","subject":{"customer":"test-customer-A","agreement_id":"test-agreement-A"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(b"Explicit test operator statement."),"content_sha256":records::raw_hash(b"Explicit test operator statement."),"metadata":{"terms_digest":digest,"acceptor":"test-acceptor-A","assent_evidence_ref":r,"statement":"operator_verified_exact_terms_acceptance"}}),
    );
    ledger
        .prerequisite(&records::bytes(&attestation).unwrap())
        .await
        .unwrap();
    let mut bad = attestation.clone();
    bad["key"]["operation_id"] = json!("wrong-terms");
    bad["payload"]["metadata"]["terms_digest"] = json!(format!("sha256:{}", "9".repeat(64)));
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&bad).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="ASSENT_TERMS_MISMATCH")
    );
    let mut invented = attestation;
    invented["key"]["operation_id"] = json!("missing-explicit-statement");
    invented["payload"]["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("statement");
    assert!(
        ledger
            .prerequisite(&records::bytes(&invented).unwrap())
            .await
            .is_err()
    );
    ledger.close().await;
}

// Deliberate offline-history tampering bypasses immutable triggers to model a
// damaged retained database, not an authorized production mutation API.
async fn corrupt_origin(path: &std::path::Path, original: &JournalOperation, mode: u8) {
    use sqlx::Connection;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path.join(".ledger/local.db"));
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    let mut transaction = connection.begin().await.unwrap();
    if mode == 3 {
        sqlx::query("DROP TRIGGER workflow_agreement_semantics_update_immutable")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE workflow_agreement_semantics SET original_identity_key=(SELECT identity_key FROM workflow_operations WHERE commit_sequence=2)").execute(&mut *transaction).await.unwrap();
        sqlx::query("CREATE TRIGGER workflow_agreement_semantics_update_immutable BEFORE UPDATE ON workflow_agreement_semantics BEGIN SELECT RAISE(ABORT,'immutable workflow semantic identity'); END").execute(&mut *transaction).await.unwrap();
    } else if mode == 2 {
        sqlx::query("DROP TRIGGER workflow_manifest_members_delete_immutable")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("DELETE FROM workflow_manifest_members WHERE identity_key=? AND kind='workflow-evidence'").bind(&original.identity).execute(&mut *transaction).await.unwrap();
        sqlx::query("CREATE TRIGGER workflow_manifest_members_delete_immutable BEFORE DELETE ON workflow_manifest_members BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END").execute(&mut *transaction).await.unwrap();
    } else {
        let mut changed = original.clone();
        let mut receipt: Value = serde_json::from_slice(&changed.receipt_bytes).unwrap();
        if mode == 0 {
            // Canonical receipt bytes change, while its retained digest stays old.
            receipt["original_request_bytes_sha256"] = json!("0".repeat(64));
            changed.receipt_bytes = records::bytes(&receipt).unwrap();
        } else {
            let mut operation: Value = serde_json::from_slice(&changed.operation_bytes).unwrap();
            let evidence = &mut operation["request_evidence"];
            evidence["normalized_request"]["payload"]["metadata"]["acceptor"] =
                json!("test-acceptor-B");
            evidence["request_hash"] = json!(
                records::hash(
                    "operation-request",
                    2,
                    &json!([
                        evidence["descriptor_resolution"]["closure_digest"],
                        evidence["normalized_request"]
                    ])
                )
                .unwrap()
            );
            changed.request_hash = text(evidence, "request_hash").unwrap().into();
            // Rehash all enclosing operation/manifest/receipt copies, but preserve
            // the original raw request and the genuine retained result document.
            let key = super::super::request::OperationKey::parse(&changed.identity).unwrap();
            let previous_ref = changed.operation_ref.clone();
            changed.operation_ref =
                records::record_reference("workflow-operation", &key.scope, &operation).unwrap();
            changed.operation_bytes = records::bytes(&operation).unwrap();
            let mut manifest: Value = serde_json::from_slice(&changed.manifest_bytes).unwrap();
            let members: &mut Vec<Value> = manifest["members"].as_array_mut().unwrap();
            for reference in members.iter_mut() {
                if *reference == json!(previous_ref) {
                    *reference = json!(changed.operation_ref);
                }
            }
            canonical::sort_set(members).unwrap();
            changed.manifest_ref =
                records::record_reference("workflow-manifest", &key.scope, &manifest).unwrap();
            changed.manifest_bytes = records::bytes(&manifest).unwrap();
            receipt["request_hash"] = json!(changed.request_hash);
            receipt["manifest_hash"] = json!(changed.manifest_ref.digest);
            receipt.as_object_mut().unwrap().remove("receipt_hash");
            receipt["receipt_hash"] =
                json!(records::hash("operation-receipt", 2, &receipt).unwrap());
            changed.receipt_ref =
                records::record_reference("workflow-receipt", &key.scope, &receipt).unwrap();
            changed.receipt_bytes = records::bytes(&receipt).unwrap();
            sqlx::query("DROP TRIGGER workflow_manifest_members_update_immutable")
                .execute(&mut *transaction)
                .await
                .unwrap();
            sqlx::query("UPDATE workflow_manifest_members SET digest=? WHERE identity_key=? AND kind='workflow-operation'").bind(&changed.operation_ref.digest).bind(&changed.identity).execute(&mut *transaction).await.unwrap();
            sqlx::query("CREATE TRIGGER workflow_manifest_members_update_immutable BEFORE UPDATE ON workflow_manifest_members BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END").execute(&mut *transaction).await.unwrap();
            assert_eq!(
                records::record_reference("workflow-operation", &key.scope, &operation).unwrap(),
                changed.operation_ref
            );
            assert_eq!(
                records::record_reference("workflow-receipt", &key.scope, &receipt).unwrap(),
                changed.receipt_ref
            );
        }
        assert_eq!(
            original.operation_bytes.len(),
            changed.operation_bytes.len()
        );
        assert_eq!(original.manifest_bytes.len(), changed.manifest_bytes.len());
        assert_eq!(original.receipt_bytes.len(), changed.receipt_bytes.len());
        sqlx::query("DROP TRIGGER workflow_operations_update_immutable")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE workflow_operations SET operation_bytes=?,operation_digest=?,normalized_request_hash=?,manifest_bytes=?,manifest_id=?,manifest_digest=?,receipt_bytes=?,receipt_id=?,receipt_digest=? WHERE identity_key=?")
            .bind(&changed.operation_bytes).bind(&changed.operation_ref.digest).bind(&changed.request_hash).bind(&changed.manifest_bytes).bind(&changed.manifest_ref.id).bind(&changed.manifest_ref.digest).bind(&changed.receipt_bytes).bind(&changed.receipt_ref.id).bind(&changed.receipt_ref.digest).bind(&changed.identity).execute(&mut *transaction).await.unwrap();
        sqlx::query("CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END").execute(&mut *transaction).await.unwrap();
    }
    transaction.commit().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn agreement_alias_and_fresh_root_require_full_origin_after_reopen() {
    for mode in [0, 1, 2, 3] {
        let (_dir, path) = installation().await;
        let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
        ledger.enable_profile_b().await.unwrap();
        let proof = envelope("origin-proof", "evidence.retain", proof_payload());
        let raw = records::bytes(&proof).unwrap();
        ledger.prerequisite(&raw).await.unwrap();
        let mut alias = proof.clone();
        alias["key"]["operation_id"] = json!("origin-alias");
        let alias_raw = records::bytes(&alias).unwrap();
        let saved_alias = ledger.prerequisite(&alias_raw).await.unwrap();
        let original_key =
            super::super::request::OperationKey::parse(&records::bytes(&proof["key"]).unwrap())
                .unwrap();
        let alias_key =
            super::super::request::OperationKey::parse(&records::bytes(&alias["key"]).unwrap())
                .unwrap();
        let mut tx = ledger
            .store
            .begin(crate::service::workflow::local::deadline())
            .await
            .unwrap();
        let original = tx
            .workflow_agreement_lookup(&original_key.bytes().unwrap())
            .await
            .unwrap()
            .unwrap();
        tx.rollback().await.unwrap();
        // Genuine alias recovery succeeds before adversarial origin damage.
        assert_eq!(
            ledger.status(&alias_key.bytes().unwrap()).await.unwrap()["receipt"],
            saved_alias["receipt"]
        );
        ledger.close().await;
        corrupt_origin(&path, &original, mode).await;
        // Meter and owner setup remain intact: refusal is retained certificate
        // verification, not a count/byte/open-time failure.
        let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
        assert_eq!(
            reopened.status(&alias_key.bytes().unwrap()).await,
            Err(ServiceError::IntegrityFailure),
            "mode {mode}"
        );
        assert_eq!(
            reopened.prerequisite(&alias_raw).await,
            Err(ServiceError::IntegrityFailure),
            "mode {mode}"
        );
        let mut fresh = proof.clone();
        fresh["key"]["operation_id"] = json!("fresh-after-origin-damage");
        fresh["payload"]["subject"]["customer"] = json!("test-customer-B");
        assert_eq!(
            reopened
                .prerequisite(&records::bytes(&fresh).unwrap())
                .await,
            Err(ServiceError::IntegrityFailure),
            "mode {mode}"
        );
        let mut tx = reopened
            .store
            .begin(crate::service::workflow::local::deadline())
            .await
            .unwrap();
        assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 2);
        tx.rollback().await.unwrap();
        reopened.close().await;
    }
}

#[tokio::test]
async fn agreement_committed_reply_loss_original_survives_permission_revision_and_freeze() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let proof = envelope(
        "committed-prerequisite-lost-reply",
        "evidence.retain",
        proof_payload(),
    );
    let raw = records::bytes(&proof).unwrap();
    let key = super::super::request::OperationKey::parse(&records::bytes(&proof["key"]).unwrap())
        .unwrap();
    service::inject_committed_reply_loss(key.bytes().unwrap());
    assert_eq!(
        ledger.prerequisite(&raw).await,
        Err(ServiceError::ResponseLost)
    );
    let status = ledger.status(&key.bytes().unwrap()).await.unwrap();
    assert_eq!(status["status"], "committed");
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let original_meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(original_meter.operations, 1);
    tx.rollback().await.unwrap();
    ledger.close().await;
    let billing = BillingLedger::open(&path).await.unwrap();
    billing.permissions("customer-1", "urn:example:work", br#"{"schema":"ledger-billing-permissions/2","customer":"customer-1","source":"urn:example:work","change_id":"prerequisite-remove-source-permissions","expected_revision":"1","permissions":[],"reason":"explicit test source permission revocation"}"#).await.unwrap();
    billing.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    let pretty = serde_json::to_vec_pretty(&proof).unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap(),
        status
    );
    assert_eq!(
        reopened.prerequisite(&pretty).await.unwrap()["receipt"],
        status["receipt"]
    );
    let mut fresh = proof.clone();
    fresh["key"]["operation_id"] = json!("fresh-prerequisite-after-freeze");
    assert_eq!(
        reopened
            .prerequisite(&records::bytes(&fresh).unwrap())
            .await,
        Err(ServiceError::Unavailable)
    );
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), original_meter);
    tx.rollback().await.unwrap();
    reopened.close().await;
    let final_open = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        final_open.prerequisite(&raw).await.unwrap()["request_evidence"],
        status["request_evidence"]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Actual local read authority requires the held directory to remain
        // private. Historical existence cannot bypass this current check.
        let directory = path.join(".ledger");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(final_open.status(&key.bytes().unwrap()).await, Err(ServiceError::Rejection(code)) if code == "AUTHORITY_REQUIRED")
        );
        assert!(
            matches!(final_open.prerequisite(&raw).await, Err(ServiceError::Rejection(code)) if code == "AUTHORITY_REQUIRED")
        );
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            final_open.status(&key.bytes().unwrap()).await.unwrap(),
            status
        );
    }
    final_open.close().await;
}

#[test]
fn agreement_assembler_matches_independent_prerequisite_certificates_and_byte_deltas() {
    // Mechanical assembly only. Literal oracle actor/time/synthetic mode never
    // passes through or authenticates the production local owner capability.
    let expected: Value =
        serde_json::from_str(include_str!("testdata/prerequisite-independent.json")).unwrap();
    let mut setup = crate::service::billing::Setup::parse(include_bytes!(
        "../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup.store_id = "fixture-store".into();
    setup.scope = ledgerlab_core::domain::Scope::new("fixture", "sandbox").unwrap();
    setup.operator = "fixture-admin".into();
    setup.source = "urn:fixture:publisher".into();
    let root_body = expected["root"]["body"].clone();
    let root = records::source_document("workflow-authority", root_body.clone()).unwrap();
    assert_eq!(
        root.reference,
        serde_json::from_value(expected["root"]["ref"].clone()).unwrap()
    );
    let mut prior_docs = std::collections::BTreeMap::new();
    prior_docs.insert(root.reference.id.clone(), root.bytes.clone());
    for operation in expected["operations"].as_array().unwrap() {
        let name = text(operation, "name").unwrap();
        let vector = expected["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .unwrap();
        let operation_vector = expected["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == format!("{name}.operation"))
            .unwrap();
        let raw = ledgerlab_core::adjudication::proofs::decode_base64(
            text(
                &operation["request_evidence"],
                "original_request_bytes_base64",
            )
            .unwrap(),
            262144,
        )
        .unwrap();
        let request = request::Request::parse(&raw).unwrap();
        let payload = service::normalize_payload(&request).unwrap();
        let at = Timestamp::parse(text(&operation["receipt"], "accepted_at").unwrap()).unwrap();
        let result = if request.kind() == "evidence.retain" {
            let input = ledgerlab_core::policy::workflow::EvidenceInput::parse(
                &records::bytes(&payload).unwrap(),
            )
            .unwrap();
            evidence_record(
                &input,
                &setup.operator,
                &at,
                "isolated_synthetic",
                &root.reference,
            )
            .unwrap()
        } else {
            records::source_document("workflow-customer", vector["body"].clone()).unwrap()
        };
        assert_eq!(
            result.bytes,
            text(vector, "canonical_utf8").unwrap().as_bytes()
        );
        let evidence = journal::evidence(&request, &payload).unwrap();
        assert_eq!(evidence, operation["request_evidence"]);
        let results = vec![result.reference.clone()];
        let mut documents = vec![result];
        if request.kind() == "authority.change" {
            let proof = expected["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "A.registration_evidence")
                .unwrap();
            documents.push(
                records::source_document("workflow-evidence", proof["body"].clone()).unwrap(),
            );
        }
        let append = journal::assemble(
            &request,
            evidence,
            &setup,
            &at,
            text(&operation["receipt"], "commit_sequence")
                .unwrap()
                .parse()
                .unwrap(),
            records::source_document("workflow-authority", root_body.clone()).unwrap(),
            documents,
            results,
            vec![],
            None,
            Projection::None,
        )
        .unwrap();
        assert_eq!(
            append.operation.operation_bytes,
            text(operation_vector, "canonical_utf8").unwrap().as_bytes()
        );
        assert_eq!(
            append.operation.manifest_bytes,
            text(operation, "canonical_manifest_utf8")
                .unwrap()
                .as_bytes()
        );
        assert_eq!(
            append.operation.receipt_bytes,
            text(operation, "canonical_receipt_utf8")
                .unwrap()
                .as_bytes()
        );
        assert_eq!(
            json!(append.operation.manifest_ref),
            operation["manifest_ref"]
        );
        assert_eq!(
            json!(append.operation.receipt_ref),
            operation["receipt_ref"]
        );
        let delta = expected["profile_b_deltas"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .unwrap();
        let wrappers = append.operation.identity.len()
            + append.operation.operation_bytes.len()
            + append.operation.manifest_bytes.len()
            + append.operation.receipt_bytes.len();
        let new_bytes: usize = append
            .documents
            .iter()
            .filter(|d| !prior_docs.contains_key(&d.reference.id))
            .map(|d| d.bytes.len())
            .sum();
        assert_eq!(
            wrappers as u64,
            delta["wrapper_key_bytes"].as_u64().unwrap()
        );
        assert_eq!(
            new_bytes as u64,
            delta["new_document_bytes"].as_u64().unwrap()
        );
        assert_eq!(
            (wrappers + new_bytes) as u64,
            delta["payload_delta"].as_u64().unwrap()
        );
        for doc in append.documents {
            prior_docs.insert(doc.reference.id, doc.bytes);
        }
    }
}

fn source_ceiling(customer: &str) -> Value {
    json!({"principal_id":"example-operator","source":"urn:example:work","canonical_economic_namespace":"urn:example:work","customer_scope":[customer],"operations":["quantity.correct","work.accept"],"product_selectors":[{"names":["test:product"]}],"activity_selectors":[{"names":["test:completed"]}],"link_selectors":[],"source_slots":["test:worker"],"permissions":["correct","read","submit"],"starts_at":"2026-10-12T00:00:00.000000Z","ends_at":"2027-01-01T00:00:00.000000Z"})
}
fn result_ref(value: &Value, kind: &str) -> RecordRef {
    serde_json::from_value(
        value["receipt"]["result_refs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == kind)
            .unwrap()
            .clone(),
    )
    .unwrap()
}
async fn source_inputs(ledger: &LocalWorkflowLedger, customer: &str) -> (RecordRef, RecordRef) {
    let mut proof = proof_payload();
    proof["subject"]["customer"] = json!(customer);
    proof["metadata"]["acceptor"] = json!(customer);
    let proof = ledger
        .prerequisite(
            &records::bytes(&envelope(
                &format!("{customer}-registration-proof"),
                "evidence.retain",
                proof,
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let proof_ref = result_ref(&proof, "workflow-evidence");
    let registered=ledger.prerequisite(&records::bytes(&envelope(&format!("{customer}-register"),"authority.change",json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":customer,"acceptor":customer,"registration_evidence_ref":proof_ref}))).unwrap()).await.unwrap();
    let content = format!(
        "Explicit test source enrollment material for {customer}; no commercial activation."
    );
    let evidence=ledger.prerequisite(&records::bytes(&envelope(&format!("{customer}-source-proof"),"evidence.retain",json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"source_enrollment","subject":{"customer":customer},"media_type":"text/plain;charset=utf-8","content_base64":super::super::publication::base64(content.as_bytes()),"content_sha256":records::raw_hash(content.as_bytes()),"metadata":{"source":"urn:example:work","principal_id":"example-operator"}}))).unwrap()).await.unwrap();
    (
        result_ref(&registered, "workflow-customer"),
        result_ref(&evidence, "workflow-evidence"),
    )
}
#[tokio::test]
async fn agreement_actual_source_enrollment_narrow_revoke_aliases_cas_and_reopen() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let (customer, evidence) = source_inputs(&ledger, "test-source-customer-A").await;
    let ceiling = source_ceiling("test-source-customer-A");
    let enrollment = envelope(
        "source-enroll-A",
        "authority.change",
        json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"source-enrollment-A","customer_refs":[customer],"ceiling":ceiling,"evidence_ref":evidence}),
    );
    let saved = ledger
        .prerequisite(&records::bytes(&enrollment).unwrap())
        .await
        .unwrap();
    assert_eq!(saved["receipt"]["result_refs"].as_array().unwrap().len(), 3);
    let e = result_ref(&saved, "workflow-source-enrollment");
    let initial_grant = result_ref(&saved, "workflow-source-grant");
    let initial_state = result_ref(&saved, "workflow-source-state");
    assert!(initial_state.id.starts_with("wsource_"));
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let key =
        super::super::request::OperationKey::parse(&records::bytes(&enrollment["key"]).unwrap())
            .unwrap();
    let body = canonical_body(&loaded(&mut tx, &key, &e).await.unwrap()).unwrap();
    assert_eq!(body["ceiling"], ceiling);
    assert_eq!(body["enrolled_by"], "example-operator");
    assert_eq!(body["installation_mode"], "real_local");
    let head = tx
        .workflow_agreement_source_head(&key.logical_store_id, &key.scope, "source-enrollment-A")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.revision, 1);
    assert_eq!(head.grant, Some(initial_grant.id.clone()));
    assert_eq!(head.state, initial_state.id);
    ledger
        .admin
        .read(&ledger.store, &mut tx, &key)
        .await
        .unwrap();
    let observed = context::observe_source(&mut tx, &key, &e).await.unwrap();
    assert_eq!(observed.state().revision(), 1);
    assert_eq!(observed.grant().unwrap().revision(), 1);
    assert_eq!(observed.enrollment().enrollment_id(), "source-enrollment-A");
    assert!(
        observed
            .documents()
            .iter()
            .any(|d| d.reference == initial_grant)
    );
    tx.rollback().await.unwrap();
    let mut narrowed = ceiling.clone();
    narrowed["operations"] = json!(["quantity.correct"]);
    narrowed["permissions"] = json!(["correct", "read"]);
    narrowed["ends_at"] = json!("2026-12-01T00:00:00.000000Z");
    let restrict = envelope(
        "source-restrict-A",
        "authority.change",
        json!({"schema":"ledger-workflow-source-restrict/1","action":"source.restrict","enrollment_ref":e,"expected_revision":"1","replacement_ceiling":narrowed}),
    );
    let restricted = ledger
        .prerequisite(&records::bytes(&restrict).unwrap())
        .await
        .unwrap();
    assert_eq!(
        restricted["receipt"]["result_refs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut restore = restrict.clone();
    restore["key"]["operation_id"] = json!("refuse-rights-restore");
    restore["payload"]["expected_revision"] = json!("2");
    restore["payload"]["replacement_ceiling"] = ceiling.clone();
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&restore).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="SOURCE_CEILING_EXCEEDED")
    );
    let mut stale = restore.clone();
    stale["key"]["operation_id"] = json!("refuse-stale-source-state");
    stale["payload"]["expected_revision"] = json!("9");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&stale).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="STALE_REVISION")
    );
    let revoke = envelope(
        "source-revoke-A",
        "authority.change",
        json!({"schema":"ledger-workflow-source-revoke/1","action":"source.revoke","enrollment_ref":e,"expected_revision":"2","reason":"Explicit test revoke of future source admissions."}),
    );
    let revoked = ledger
        .prerequisite(&records::bytes(&revoke).unwrap())
        .await
        .unwrap();
    let mut observed_tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    ledger
        .admin
        .read(&ledger.store, &mut observed_tx, &key)
        .await
        .unwrap();
    let observed = context::observe_source(&mut observed_tx, &key, &e)
        .await
        .unwrap();
    assert_eq!(observed.state().revision(), 3);
    assert_eq!(observed.state().status(), "revoked");
    assert!(observed.grant().is_none());
    let original_source = context::observe_source_at(&mut observed_tx, &key, &e, &initial_state)
        .await
        .unwrap();
    assert_eq!(original_source.state().revision(), 1);
    assert_eq!(original_source.state().status(), "active");
    assert_eq!(original_source.grant().unwrap().revision(), 1);
    let initial_sequence = saved["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let restricted_sequence = restricted["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let revoked_sequence = revoked["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let restricted_state: RecordRef = serde_json::from_value(
        restricted["receipt"]["result_refs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "workflow-source-state")
            .unwrap()
            .clone(),
    )
    .unwrap();
    let revoked_state: RecordRef =
        serde_json::from_value(revoked["receipt"]["result_refs"][0].clone()).unwrap();
    let (first_as_of, counts) = verify::audit_origins(context::source_as_of(
        &mut observed_tx,
        &key,
        &e,
        initial_sequence + 1,
    ))
    .await
    .unwrap();
    assert_eq!(first_as_of, initial_state);
    assert_eq!(
        counts.get(&records::bytes(&enrollment["key"]).unwrap()),
        Some(&1)
    );
    assert_eq!(
        context::source_as_of(&mut observed_tx, &key, &e, restricted_sequence + 1)
            .await
            .unwrap(),
        restricted_state
    );
    assert_eq!(
        context::source_as_of(&mut observed_tx, &key, &e, revoked_sequence + 1)
            .await
            .unwrap(),
        revoked_state
    );
    // Prior active grants remain verification history, never a fake current
    // grant when the actual latest source state is revoked.
    assert!(
        observed
            .documents()
            .iter()
            .any(|d| d.reference == initial_grant)
    );
    observed_tx.rollback().await.unwrap();
    let restrict_identity = records::bytes(&restrict["key"]).unwrap();
    for mode in 0..5 {
        let mut tx = ledger
            .store
            .begin(crate::service::workflow::local::deadline())
            .await
            .unwrap();
        tx.workflow_test_source_history_tamper(mode, &restrict_identity)
            .await
            .unwrap();
        if mode == 4 {
            assert_eq!(
                loaded(&mut tx, &key, &restricted_state)
                    .await
                    .unwrap()
                    .reference,
                restricted_state,
                "state2 survives removal of its original birth row"
            );
            assert!(
                tx.workflow_agreement_source_prefix(
                    &key.logical_store_id,
                    &key.scope,
                    &e,
                    "source-enrollment-A"
                )
                .await
                .unwrap()
                .contains(&restrict_identity),
                "independent state locator preserves missing original identity"
            );
        }
        let frontier = if mode == 4 {
            restricted_sequence + 1
        } else {
            revoked_sequence + 1
        };
        let as_of = context::source_as_of(&mut tx, &key, &e, frontier).await;
        if mode == 0 {
            assert_eq!(
                as_of.unwrap(),
                revoked_state,
                "mutable head is not historical admission evidence"
            );
        } else {
            assert!(
                matches!(as_of, Err(ServiceError::IntegrityFailure)),
                "missing or moved immutable birth cannot resurrect state1"
            );
        }
        tx.rollback().await.unwrap();
    }
    assert_eq!(
        revoked["receipt"]["result_refs"].as_array().unwrap().len(),
        1
    );
    let mut after_revoke = restore;
    after_revoke["key"]["operation_id"] = json!("refuse-reenable-source");
    after_revoke["payload"]["expected_revision"] = json!("3");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&after_revoke).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    for (original, value, id) in [
        (&enrollment, &saved, "alias-enroll-after-revoke"),
        (&restrict, &restricted, "alias-restrict-after-revoke"),
        (&revoke, &revoked, "alias-revoke-after-revoke"),
    ] {
        let mut alias = original.clone();
        alias["key"]["operation_id"] = json!(id);
        let result = ledger
            .prerequisite(&records::bytes(&alias).unwrap())
            .await
            .unwrap();
        assert_eq!(result["duplicate_kind"], "semantic");
        assert_eq!(
            result["receipt"]["result_refs"],
            value["receipt"]["result_refs"]
        );
        assert_eq!(
            ledger
                .prerequisite(&serde_json::to_vec_pretty(original).unwrap())
                .await
                .unwrap()["receipt"],
            value["receipt"]
        );
    }
    let mut changed = enrollment.clone();
    changed["key"]["operation_id"] = json!("refuse-enrollment-ceiling-rewrite");
    changed["payload"]["ceiling"]["ends_at"] = json!("2026-12-02T00:00:00.000000Z");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&changed).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="SEMANTIC_CONFLICT")
    );
    // Same canonical source can have an independent enrollment for B; its own
    // customer evidence/ceiling is required and A's state stays revoked.
    let (b_customer, b_evidence) = source_inputs(&ledger, "test-source-customer-B").await;
    let b = envelope(
        "source-enroll-B",
        "authority.change",
        json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"source-enrollment-B","customer_refs":[b_customer],"ceiling":source_ceiling("test-source-customer-B"),"evidence_ref":b_evidence}),
    );
    ledger
        .prerequisite(&records::bytes(&b).unwrap())
        .await
        .unwrap();
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let a_head = tx
        .workflow_agreement_source_head(&key.logical_store_id, &key.scope, "source-enrollment-A")
        .await
        .unwrap()
        .unwrap();
    let b_head = tx
        .workflow_agreement_source_head(&key.logical_store_id, &key.scope, "source-enrollment-B")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (a_head.revision, a_head.status.as_str(), a_head.grant),
        (3, "revoked", None)
    );
    assert_eq!((b_head.revision, b_head.status.as_str()), (1, "active"));
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 13);
    tx.rollback().await.unwrap();
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["receipt"],
        saved["receipt"]
    );
    assert_eq!(
        reopened
            .prerequisite(&records::bytes(&revoke).unwrap())
            .await
            .unwrap()["receipt"],
        revoked["receipt"]
    );
    let mut tx = reopened
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), meter);
    tx.rollback().await.unwrap();
    reopened.close().await;
}

async fn change_test_trigger(path: &std::path::Path, statement: &str) {
    use sqlx::Connection;
    let mut c = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path.join(".ledger/local.db")),
    )
    .await
    .unwrap();
    // Test-only statements are constructed below from closed literal table/
    // kind sets; no request/customer/source string enters this SQL.
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&mut c)
        .await
        .unwrap();
    c.close().await.unwrap();
}
async fn retained_projection_counts(path: &std::path::Path) -> Vec<i64> {
    use sqlx::Connection;
    let mut c = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path.join(".ledger/local.db")),
    )
    .await
    .unwrap();
    let mut counts = vec![];
    for table in [
        "workflow_documents",
        "workflow_operations",
        "workflow_manifest_members",
        "workflow_agreement_semantics",
        "workflow_customer_registrations",
        "workflow_source_namespaces",
        "workflow_source_enrollments",
        "workflow_source_heads",
        "workflow_feed",
    ] {
        counts.push(
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut c)
                .await
                .unwrap(),
        );
    }
    c.close().await.unwrap();
    counts
}
#[tokio::test]
async fn agreement_source_enrollment_each_insert_boundary_rolls_back_all_projections() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let (customer, evidence) = source_inputs(&ledger, "test-source-cut-customer").await;
    let request = envelope(
        "source-enrollment-insert-cuts",
        "authority.change",
        json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"source-cut-enrollment","customer_refs":[customer],"ceiling":source_ceiling("test-source-cut-customer"),"evidence_ref":evidence}),
    );
    let raw = records::bytes(&request).unwrap();
    let key = super::super::request::OperationKey::parse(&records::bytes(&request["key"]).unwrap())
        .unwrap();
    let baseline = retained_projection_counts(&path).await;
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let baseline_meter = tx.workflow_test_meter().await.unwrap();
    tx.rollback().await.unwrap();
    let mut cuts = vec![];
    for kind in [
        "workflow-source-enrollment",
        "workflow-source-grant",
        "workflow-source-state",
    ] {
        cuts.push(format!("CREATE TRIGGER test_prerequisite_cut BEFORE INSERT ON workflow_documents WHEN NEW.kind='{kind}' BEGIN SELECT RAISE(ABORT,'injected new document cut'); END"));
    }
    for table in [
        "workflow_source_namespaces",
        "workflow_source_enrollments",
        "workflow_source_heads",
        "workflow_operations",
        "workflow_agreement_semantics",
        "workflow_feed",
    ] {
        cuts.push(format!("CREATE TRIGGER test_prerequisite_cut BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'injected projection or journal cut'); END"));
    }
    for kind in [
        "workflow-operation",
        "workflow-authority",
        "workflow-customer",
        "workflow-evidence",
        "workflow-source-enrollment",
        "workflow-source-grant",
        "workflow-source-state",
    ] {
        cuts.push(format!("CREATE TRIGGER test_prerequisite_cut BEFORE INSERT ON workflow_manifest_members WHEN NEW.kind='{kind}' BEGIN SELECT RAISE(ABORT,'injected closure member cut'); END"));
    }
    cuts.push("CREATE TRIGGER test_prerequisite_cut BEFORE UPDATE ON workflow_state BEGIN SELECT RAISE(ABORT,'injected final meter cut'); END".into());
    for (index, cut) in cuts.iter().enumerate() {
        change_test_trigger(&path, cut).await;
        assert!(ledger.prerequisite(&raw).await.is_err(), "cut {index}");
        change_test_trigger(&path, "DROP TRIGGER test_prerequisite_cut").await;
        assert_eq!(
            retained_projection_counts(&path).await,
            baseline,
            "cut {index}"
        );
        assert_eq!(
            ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
            "not_observed",
            "cut {index}"
        );
        let mut tx = ledger
            .store
            .begin(crate::service::workflow::local::deadline())
            .await
            .unwrap();
        assert_eq!(
            tx.workflow_test_meter().await.unwrap(),
            baseline_meter,
            "cut {index}"
        );
        tx.rollback().await.unwrap();
    }
    ledger.prerequisite(&raw).await.unwrap();
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "committed"
    );
    reopened.close().await;
}

#[tokio::test]
async fn agreement_source_append_failure_poison_prevents_partial_commit() {
    let (_dir, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let (customer, evidence) = source_inputs(&ledger, "test-source-poison-customer").await;
    let outer = envelope(
        "source-poison",
        "authority.change",
        json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"source-poison-enrollment","customer_refs":[customer],"ceiling":source_ceiling("test-source-poison-customer"),"evidence_ref":evidence}),
    );
    let raw = records::bytes(&outer).unwrap();
    let request = request::Request::parse(&raw).unwrap();
    let key = request.key();
    let baseline = retained_projection_counts(&path).await;
    for cut in [
        "CREATE TRIGGER test_prerequisite_poison BEFORE INSERT ON workflow_source_heads BEGIN SELECT RAISE(ABORT,'late projection poison'); END",
        "CREATE TRIGGER test_prerequisite_poison BEFORE UPDATE ON workflow_state BEGIN SELECT RAISE(ABORT,'final meter poison'); END",
    ] {
        change_test_trigger(&path, cut).await;
        let mut tx = ledger
            .store
            .begin(crate::service::workflow::local::deadline())
            .await
            .unwrap();
        let setup = ledger
            .admin
            .fresh(&ledger.store, &mut tx, key)
            .await
            .unwrap();
        assert_eq!(tx.load_installation().await.unwrap().mode, "real");
        let root = tx
            .workflow_agreement_root_candidate(&key.logical_store_id, &key.scope)
            .await
            .unwrap()
            .unwrap();
        verify::document_origin(&mut tx, key, &root.reference)
            .await
            .unwrap();
        assert_eq!(
            canonical_body(&root).unwrap()["basis"],
            journal::basis(&setup)
        );
        let at = crate::local::now().unwrap();
        let payload = service::normalize_payload(&request).unwrap();
        let prepared = source::prepare(
            &mut tx,
            key,
            &payload,
            &setup,
            &at,
            "real_local",
            &root.reference,
        )
        .await
        .unwrap();
        let sequence = tx.workflow_sequence().await.unwrap();
        let append = journal::assemble(
            &request,
            journal::evidence(&request, &payload).unwrap(),
            &setup,
            &at,
            sequence,
            root,
            prepared.documents,
            prepared.results,
            source::semantic(&payload, "source-poison-enrollment").unwrap(),
            None,
            prepared.projection,
        )
        .unwrap();
        assert!(tx.workflow_agreement_append(&append).await.is_err());
        assert!(matches!(
            tx.commit().await,
            Err(crate::store::errors::CommitError::RolledBack(_))
        ));
        change_test_trigger(&path, "DROP TRIGGER test_prerequisite_poison").await;
        assert_eq!(retained_projection_counts(&path).await, baseline);
        assert_eq!(
            ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
            "not_observed"
        );
    }
    ledger.prerequisite(&raw).await.unwrap();
    ledger.close().await;
}
