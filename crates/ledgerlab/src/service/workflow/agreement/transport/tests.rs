//! Public trusted local transport tests. Fixture source bodies are immutable
//! data inputs, never a fake SQL seed or a production authority snapshot.
use super::*;
use crate::{
    billing::BillingLedger,
    service::workflow::LocalWorkflowLedger,
    store::ports::{AcceptanceStore, AcceptanceTx},
};
async fn installation() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!("real-source-transport-test");
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    (directory, path)
}
fn requests() -> Vec<Value> {
    serde_json::from_slice::<Value>(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/publication-transport-02-fixture-input.json")).unwrap()["requests"].as_array().unwrap()[..4].to_vec()
}
fn actual(mut request: Value) -> Value {
    request["key"] = json!({"logical_store_id":"real-source-transport-test","scope":["example-company","local"],"source":"urn:example:work","operation_id":request["key"]["operation_id"]});
    request
}
fn reference(result: &Value) -> RecordRef {
    serde_json::from_value(result["receipt"]["result_refs"][0].clone()).unwrap()
}
fn key(request: &Value) -> OperationKey {
    OperationKey::parse(&records::bytes(&request["key"]).unwrap()).unwrap()
}
fn canonical_read(r: &RecordRef) -> Vec<u8> {
    records::bytes(&json!({"schema":"ledger-read/1","kind":"policy.document","payload":{"ref":r,"representation":"canonical_bytes"}})).unwrap()
}
#[tokio::test]
async fn transport_public_direct_fragments_finalize_reply_loss_alias_read_reopen() {
    let (_directory, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let inputs = requests();
    let direct = actual(inputs[0].clone());
    let direct_raw = records::bytes(&direct).unwrap();
    service::lose_reply(key(&direct).bytes().unwrap());
    assert_eq!(
        ledger.prerequisite(&direct_raw).await,
        Err(ServiceError::ResponseLost)
    );
    let original = ledger.status(&key(&direct).bytes().unwrap()).await.unwrap();
    let direct_ref = reference(&original);
    assert_eq!(
        ledger
            .prerequisite(&serde_json::to_vec_pretty(&direct).unwrap())
            .await
            .unwrap()["receipt"],
        original["receipt"]
    );
    let mut alias = direct.clone();
    alias["key"]["operation_id"] = json!("direct-new-key");
    let aliased = ledger
        .prerequisite(&records::bytes(&alias).unwrap())
        .await
        .unwrap();
    assert_eq!(aliased["duplicate_kind"], "semantic");
    assert_eq!(reference(&aliased), direct_ref);
    let fragments = [actual(inputs[1].clone()), actual(inputs[2].clone())];
    let first = ledger
        .prerequisite(&records::bytes(&fragments[0]).unwrap())
        .await
        .unwrap();
    let mut finalize = actual(inputs[3].clone());
    finalize["payload"]["fragment_refs"] = json!([reference(&first)]);
    assert!(ledger
        .prerequisite(&records::bytes(&finalize).unwrap())
        .await
        .is_err());
    let second = ledger
        .prerequisite(&records::bytes(&fragments[1]).unwrap())
        .await
        .unwrap();
    let mut refs = vec![reference(&first), reference(&second)];
    records::sort_refs(&mut refs).unwrap();
    finalize["payload"]["fragment_refs"] = json!(refs);
    let saved = ledger
        .prerequisite(&records::bytes(&finalize).unwrap())
        .await
        .unwrap();
    let template_ref = reference(&saved);
    let read = ledger.read(&canonical_read(&template_ref)).await.unwrap();
    assert_eq!(read["schema"], "ledger-workflow-document-read/2");
    assert_eq!(read.as_object().unwrap().len(), 5);
    assert!(read.get("body").is_none());
    let bytes = ledgerlab_core::adjudication::proofs::decode_base64(
        text(&read, "canonical_bytes_base64").unwrap(),
        4194304,
    )
    .unwrap();
    let parts: Vec<_> = inputs[1..3]
        .iter()
        .map(|r| {
            core::SourceRetentionInput::parse(&records::bytes(&r["payload"]).unwrap()).unwrap()
        })
        .collect();
    let expected: Vec<_> = parts
        .iter()
        .flat_map(|p| p.fragment().unwrap().decoded_bytes().iter().copied())
        .collect();
    assert_eq!(bytes, expected);
    assert_eq!(read["canonical_bytes_sha256"], records::raw_hash(&bytes));
    assert_eq!(read["body_bytes"], bytes.len().to_string());
    assert!(serde_json::to_vec(&read).unwrap().len() < 8388608);
    let mut fragment_alias = fragments[0].clone();
    fragment_alias["key"]["operation_id"] = json!("fragment-new-key");
    assert_eq!(
        ledger
            .prerequisite(&records::bytes(&fragment_alias).unwrap())
            .await
            .unwrap()["duplicate_kind"],
        "semantic"
    );
    let mut direct_template = direct.clone();
    direct_template["key"]["operation_id"] = json!("template-direct-after-finalize");
    direct_template["payload"]["document"] = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        reference(
            &ledger
                .prerequisite(&records::bytes(&direct_template).unwrap())
                .await
                .unwrap()
        ),
        template_ref
    );
    let mut changed = direct.clone();
    changed["payload"]["document"]["fields"][0]["type"]["max"] = json!("101");
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&changed).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    let mut conflict = fragments[0].clone();
    conflict["key"]["operation_id"] = json!("fragment-metadata-conflict");
    conflict["payload"]["canonical_document_sha256"] = json!("0".repeat(64));
    assert!(
        matches!(ledger.prerequisite(&records::bytes(&conflict).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="SOURCE_FRAGMENT_MISMATCH")
    );
    let mut spoof = direct.clone();
    spoof["key"]["operation_id"] = json!("private-role-source-spoof");
    spoof["payload"]["document"] = json!({"schema":"ledger-workflow-roles/1"});
    assert!(ledger
        .prerequisite(&records::bytes(&spoof).unwrap())
        .await
        .is_err());
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 7);
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        ledger.prerequisite(&direct_raw).await.unwrap()["receipt"],
        original["receipt"]
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened
            .status(&key(&finalize).bytes().unwrap())
            .await
            .unwrap()["receipt"],
        saved["receipt"]
    );
    assert_eq!(
        reopened.read(&canonical_read(&template_ref)).await.unwrap(),
        read
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
#[test]
fn transport_fragment_descriptor_evidence_matches_independent_bytes() {
    let expected:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/transport-02-independent/expected.json")).unwrap();
    for name in ["fragment-0", "fragment-1"] {
        let op = expected["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == name)
            .unwrap();
        let raw = records::bytes(&op["normalized_request"]).unwrap();
        let request = request::Request::parse(&raw).unwrap();
        let input =
            core::SourceRetentionInput::parse(&records::bytes(request.payload()).unwrap()).unwrap();
        let payload = input.fragment().unwrap().canonical_value().unwrap();
        let resolution = resolve::Resolution {
            input,
            payload,
            definition: None,
            fragments: vec![],
            dependencies: vec![],
        };
        assert_eq!(
            evidence::request_evidence(&request, &resolution).unwrap(),
            op["request_evidence"]
        );
    }
}

#[tokio::test]
async fn transport_real_typed_canonical_closure_budget_accepts_long_counter_equivalent() {
    let (_directory, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let fixture:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    let fact: Value =
        serde_json::from_str(vectors[0]["canonical_body_utf8"].as_str().unwrap()).unwrap();
    let mut product: Value =
        serde_json::from_str(vectors[4]["canonical_body_utf8"].as_str().unwrap()).unwrap();
    let values: Vec<_> = (0..32)
        .map(|i| format!("{}:v{i:02}{}", "n".repeat(60), "x".repeat(59)))
        .collect();
    let mut bodies:Vec<Value>=(0..4).map(|schema|json!({"schema":"ledger-workflow-fact-schema/1","schema_id":format!("test.extra:s{schema}"),"version":"1","normalization":"ledger-facts-normalization/1","fields":(0..63).map(|i|json!({"name":format!("test.enum:f{i:02}"),"required":true,"type":{"kind":"enum","values":values},"provenance":{"kind":"caller"}})).collect::<Vec<_>>()})).collect();
    let typed_fact = core::normalize_retained_definition(&records::bytes(&fact).unwrap(), &[])
        .unwrap()
        .document()
        .clone();
    // Fixed-width digest refs make the Product byte length independent of the
    // controlled enum-name changes; each name remains a valid <=128-byte name.
    let mut typed = vec![typed_fact.clone()];
    for body in &bodies {
        typed.push(
            core::normalize_retained_definition(&records::bytes(body).unwrap(), &[])
                .unwrap()
                .document()
                .clone(),
        );
    }
    product["fact_schemas"] = json!(typed.iter().map(|d| d.reference()).collect::<Vec<_>>());
    let root =
        core::normalize_retained_definition(&records::bytes(&product).unwrap(), &typed).unwrap();
    let desired = 1048576usize - 32;
    let original_total = typed
        .iter()
        .map(|d| d.canonical_bytes().len())
        .sum::<usize>()
        + root.document().canonical_bytes().len();
    assert!(original_total < desired);
    let mut additional = desired - original_total;
    for body in &mut bodies {
        let current = records::bytes(body).unwrap().len();
        let mut room = 262144usize - current;
        for field in body["fields"].as_array_mut().unwrap() {
            for value in field["type"]["values"].as_array_mut().unwrap() {
                let name = value.as_str().unwrap();
                let delta = additional
                    .min(room)
                    .min(64 - name.split_once(':').unwrap().1.len());
                *value = json!(format!("{name}{}", "x".repeat(delta)));
                additional -= delta;
                room -= delta;
            }
        }
    }
    assert_eq!(additional, 0);
    typed = vec![typed_fact];
    for body in &bodies {
        typed.push(
            core::normalize_retained_definition(&records::bytes(body).unwrap(), &[])
                .unwrap()
                .document()
                .clone(),
        );
    }
    product["fact_schemas"] = json!(typed.iter().map(|d| d.reference()).collect::<Vec<_>>());
    let canonical =
        core::normalize_retained_definition(&records::bytes(&product).unwrap(), &typed).unwrap();
    let dependency_bytes = typed
        .iter()
        .map(|d| d.canonical_bytes().len())
        .sum::<usize>();
    assert_eq!(
        dependency_bytes + canonical.document().canonical_bytes().len(),
        desired
    );
    let mut input = actual(requests()[0].clone());
    let mut actual_refs = vec![];
    for (i, body) in std::iter::once(&fact).chain(bodies.iter()).enumerate() {
        input["key"]["operation_id"] = json!(format!("canonical-boundary-dependency-{i}"));
        input["payload"]["document"] = body.clone();
        let result =
            retain_document(&ledger, &format!("canonical-boundary-dependency-{i}"), body).await;
        actual_refs.push(reference(&result));
    }
    product["fact_schemas"] = json!(actual_refs);
    product["version"] = json!(format!("{}1", "0".repeat(63)));
    assert!(dependency_bytes + records::bytes(&product).unwrap().len() > 1048576);
    input["key"]["operation_id"] = json!("canonical-boundary-product");
    input["payload"]["document"] = product.clone();
    let result = ledger
        .prerequisite(&records::bytes(&input).unwrap())
        .await
        .unwrap();
    assert_eq!(
        reference(&result),
        record_ref(canonical.document().reference()).unwrap()
    );
    let read = ledger
        .read(&canonical_read(&reference(&result)))
        .await
        .unwrap();
    assert_eq!(
        ledgerlab_core::adjudication::proofs::decode_base64(
            text(&read, "canonical_bytes_base64").unwrap(),
            4194304
        )
        .unwrap(),
        canonical.document().canonical_bytes()
    );
    // The unpadded declaration is a normalized original retry, preserving the
    // actual original long-counter request evidence and receipt.
    input["payload"]["document"]["version"] = json!("1");
    let retry = ledger
        .prerequisite(&records::bytes(&input).unwrap())
        .await
        .unwrap();
    assert_eq!(retry["receipt"], result["receipt"]);
    assert_eq!(retry["request_evidence"], result["request_evidence"]);
    ledger.close().await;
}

async fn retain_document(ledger: &LocalWorkflowLedger, id: &str, body: &Value) -> Value {
    let mut request = actual(requests()[0].clone());
    request["key"]["operation_id"] = json!(id);
    request["payload"] = json!({"schema":"ledger-workflow-definition-retain/1","document":body});
    let wire = records::bytes(&request).unwrap();
    if wire.len() <= 262144 {
        return ledger.prerequisite(&wire).await.unwrap();
    }
    let bytes = records::bytes(body).unwrap();
    assert!(bytes.len() <= 262144);
    let upload = format!("{id}-upload");
    let mut refs = vec![];
    for (part, bytes_part) in bytes.chunks(131072).enumerate() {
        request["key"]["operation_id"] = json!(format!("{id}-part-{part}"));
        request["payload"] = json!({"schema":"ledger-workflow-definition-fragment/1","upload_id":upload,"document_schema":body["schema"],"canonical_document_sha256":records::raw_hash(&bytes),"document_bytes":bytes.len().to_string(),"part":part.to_string(),"bytes_base64":super::super::super::publication::base64(bytes_part)});
        refs.push(reference(
            &ledger
                .prerequisite(&records::bytes(&request).unwrap())
                .await
                .unwrap(),
        ));
    }
    records::sort_refs(&mut refs).unwrap();
    request["key"]["operation_id"] = json!(id);
    request["payload"] = json!({"schema":"ledger-workflow-definition-finalize/1","upload_id":upload,"fragment_refs":refs});
    ledger
        .prerequisite(&records::bytes(&request).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn transport_public_publication_prepare_matches_frozen_recipe_without_writes() {
    let (_directory, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let fixture: Value = serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    let documents: Vec<_> = vectors[..6].iter().map(|v| v["body"].clone()).collect();
    let prepare = |candidate: Value| {
        records::bytes(&json!({"schema":"ledger-read/1","kind":"operation.descriptors","payload":{"operation_kind":"policy.publish","candidate":candidate}})).unwrap()
    };
    let mut candidate = fixture["normalized_request"]["payload"].clone();
    candidate["mode"] = json!("inline");
    candidate["documents"] = json!(documents);
    assert_eq!(
        ledger.read(&prepare(candidate.clone())).await.unwrap(),
        fixture["descriptor_resolution"]
    );
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 0);
    tx.rollback().await.unwrap();
    let mut refs = vec![];
    for (index, body) in documents.iter().enumerate() {
        refs.push(reference(
            &retain_document(&ledger, &format!("prepare-source-{index}"), body).await,
        ));
    }
    records::sort_refs(&mut refs).unwrap();
    candidate.as_object_mut().unwrap().remove("documents");
    candidate["mode"] = json!("retained");
    candidate["document_refs"] = json!(refs);
    let retained = ledger.read(&prepare(candidate.clone())).await.unwrap();
    assert_eq!(
        retained["anchors"],
        fixture["descriptor_resolution"]["anchors"]
    );
    assert_eq!(
        retained["descriptors"],
        fixture["descriptor_resolution"]["descriptors"]
    );
    assert_eq!(
        retained["registry_digest"],
        fixture["descriptor_resolution"]["registry_digest"]
    );
    assert_ne!(
        retained["locator_digest"],
        fixture["descriptor_resolution"]["locator_digest"]
    );
    candidate["unexpected"] = json!(true);
    assert!(ledger.read(&prepare(candidate)).await.is_err());
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap().operations, 6);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
#[ignore = "requires actual controlled captured build; public retention never seeds SQL"]
async fn captured_transport_large_public_retention_publication_canonical_read_reopen() {
    assert!(crate::build_identity::embedded_inputs().is_ok());
    let (_directory, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let fixture:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/publication-transport-02-fixture-input.json")).unwrap();
    let inputs = fixture["requests"].as_array().unwrap();
    let mut parts = vec![];
    let mut retained = std::collections::BTreeMap::new();
    let mut first_receipt = None;
    for (index, input) in inputs[..26].iter().enumerate() {
        let mut request = actual(input.clone());
        if index == 3 {
            records::sort_refs(&mut parts).unwrap();
            request["payload"]["fragment_refs"] = json!(parts);
        }
        let prepared=ledger.read(&records::bytes(&json!({"schema":"ledger-read/1","kind":"operation.descriptors","payload":{"operation_kind":"evidence.retain","candidate":request["payload"]}})).unwrap()).await.unwrap();
        let result = ledger
            .prerequisite(&records::bytes(&request).unwrap())
            .await
            .unwrap();
        assert_eq!(
            prepared,
            result["request_evidence"]["descriptor_resolution"]
        );
        let r = reference(&result);
        if index == 1 || index == 2 {
            parts.push(r.clone());
        }
        if source_kind(&r.kind) {
            retained.insert(r.id.clone(), r);
        }
        if index == 0 {
            first_receipt = Some((request, result));
        }
        eprintln!(
            "transport public retained {index} {}",
            input["key"]["operation_id"]
        );
    }
    let sources:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/plan-size-counterexample/input.json")).unwrap();
    let mut refs = vec![];
    for body in sources["documents"].as_array().unwrap() {
        let kind = match body["schema"].as_str().unwrap() {
            "ledger-workflow-fact-schema/1" => "workflow-fact-schema",
            "ledger-workflow-product/1" => "workflow-product",
            "ledger-workflow-policy/1" => "workflow-policy",
            "ledger-workflow-slot-template/1" => "workflow-slot-template",
            "ledger-workflow-compiler-descriptor/1" => "workflow-compiler-descriptor",
            _ => panic!("unsupported frozen source"),
        };
        let derived = records::source_document(kind, body.clone()).unwrap();
        refs.push(retained.get(&derived.reference.id).unwrap().clone());
    }
    records::sort_refs(&mut refs).unwrap();
    assert_eq!(refs.len(), 21);
    let expected:Value=serde_json::from_slice(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/transport-02-independent/expected.json")).unwrap();
    let plan = expected["envelopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "workflow-compiled-plan")
        .unwrap();
    let mut publish = actual(inputs[0].clone());
    publish["key"]["operation_id"] = json!("actual-large-publication");
    publish["kind"] = json!("policy.publish");
    publish["payload"] = json!({"schema":"ledger-workflow-policy-publication/1","mode":"retained","document_refs":refs,"root_policy_ref":sources["root_policy_ref"],"expected_plan_digest":plan["digest"]});
    let prepare=ledger.read(&records::bytes(&json!({"schema":"ledger-read/1","kind":"operation.descriptors","payload":{"operation_kind":"policy.publish","candidate":publish["payload"]}})).unwrap()).await.unwrap();
    assert_eq!(
        prepare,
        expected["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "publish-large")
            .unwrap()["descriptor_resolution"]
    );
    let result = ledger
        .publish(&records::bytes(&publish).unwrap())
        .await
        .unwrap();
    assert_eq!(prepare, result["request_evidence"]["descriptor_resolution"]);
    let plan_ref: RecordRef = serde_json::from_value(
        result["receipt"]["result_refs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "workflow-compiled-plan")
            .unwrap()
            .clone(),
    )
    .unwrap();
    let read = ledger.read(&canonical_read(&plan_ref)).await.unwrap();
    let decoded = ledgerlab_core::adjudication::proofs::decode_base64(
        text(&read, "canonical_bytes_base64").unwrap(),
        4194304,
    )
    .unwrap();
    assert_eq!(decoded.len(), 1089536);
    assert_eq!(decoded, records::bytes(&plan["body"]).unwrap());
    assert_eq!(read.as_object().unwrap().len(), 5);
    assert!(read.get("body").is_none());
    assert!(serde_json::to_vec(&read).unwrap().len() < 8388608);
    assert_eq!(
        ledger
            .status(&key(&publish).bytes().unwrap())
            .await
            .unwrap()["receipt"],
        result["receipt"]
    );
    let (first, first_result) = first_receipt.unwrap();
    assert_eq!(
        ledger
            .prerequisite(&serde_json::to_vec_pretty(&first).unwrap())
            .await
            .unwrap()["receipt"],
        first_result["receipt"]
    );
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    assert_eq!(meter.operations, 27);
    tx.rollback().await.unwrap();
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.read(&canonical_read(&plan_ref)).await.unwrap(),
        read
    );
    assert_eq!(
        reopened
            .publish(&records::bytes(&publish).unwrap())
            .await
            .unwrap()["receipt"],
        result["receipt"]
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

async fn transport_test_sql(path: &std::path::Path, statement: &str) {
    use sqlx::Connection;
    let mut c = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path.join(".ledger/local.db")),
    )
    .await
    .unwrap();
    // Only closed literal test triggers enter this seam, never ingress data.
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&mut c)
        .await
        .unwrap();
    c.close().await.unwrap();
}
async fn transport_counts(path: &std::path::Path) -> Vec<i64> {
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
        "workflow_definition_registry",
        "workflow_definition_uploads",
        "workflow_definition_upload_parts",
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
async fn transport_upload_and_finalize_projection_cuts_leave_no_partial_history() {
    let (_directory, path) = installation().await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    let first = actual(requests()[1].clone());
    let raw = records::bytes(&first).unwrap();
    let baseline = transport_counts(&path).await;
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let meter = tx.workflow_test_meter().await.unwrap();
    tx.rollback().await.unwrap();
    for (index, cut) in [
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_documents WHEN NEW.kind='workflow-definition-fragment' BEGIN SELECT RAISE(ABORT,'fragment document cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_definition_uploads BEGIN SELECT RAISE(ABORT,'upload metadata cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_definition_upload_parts BEGIN SELECT RAISE(ABORT,'upload part cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_operations BEGIN SELECT RAISE(ABORT,'operation cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_agreement_semantics BEGIN SELECT RAISE(ABORT,'semantic index cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_feed BEGIN SELECT RAISE(ABORT,'feed cut'); END",
        "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_manifest_members WHEN NEW.kind='workflow-definition-fragment' BEGIN SELECT RAISE(ABORT,'fragment closure cut'); END",
        "CREATE TRIGGER transport_cut BEFORE UPDATE ON workflow_state BEGIN SELECT RAISE(ABORT,'meter cut'); END",
    ].iter().enumerate() {
        transport_test_sql(&path, cut).await;
        assert!(ledger.prerequisite(&raw).await.is_err(), "cut {index}");
        transport_test_sql(&path, "DROP TRIGGER transport_cut").await;
        assert_eq!(transport_counts(&path).await, baseline, "cut {index}");
        assert_eq!(ledger.status(&key(&first).bytes().unwrap()).await.unwrap()["status"], "not_observed");
        let mut tx = ledger.store.begin(crate::service::workflow::local::deadline()).await.unwrap();
        assert_eq!(tx.workflow_test_meter().await.unwrap(), meter, "cut {index}");
        tx.rollback().await.unwrap();
    }
    let saved_first = ledger.prerequisite(&raw).await.unwrap();
    let saved_second = ledger
        .prerequisite(&records::bytes(&actual(requests()[2].clone())).unwrap())
        .await
        .unwrap();
    let mut finalize = actual(requests()[3].clone());
    let mut parts = vec![reference(&saved_first), reference(&saved_second)];
    records::sort_refs(&mut parts).unwrap();
    finalize["payload"]["fragment_refs"] = json!(parts);
    let baseline = transport_counts(&path).await;
    transport_test_sql(&path, "CREATE TRIGGER transport_cut BEFORE INSERT ON workflow_definition_registry BEGIN SELECT RAISE(ABORT,'finalized definition registry cut'); END").await;
    assert!(ledger
        .prerequisite(&records::bytes(&finalize).unwrap())
        .await
        .is_err());
    transport_test_sql(&path, "DROP TRIGGER transport_cut").await;
    assert_eq!(transport_counts(&path).await, baseline);
    let saved = ledger
        .prerequisite(&records::bytes(&finalize).unwrap())
        .await
        .unwrap();
    let finalized = reference(&saved);
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened
            .status(&key(&finalize).bytes().unwrap())
            .await
            .unwrap()["receipt"],
        saved["receipt"]
    );
    assert!(reopened.read(&canonical_read(&finalized)).await.is_ok());
    reopened.close().await;
    // Keep the valid fragment body, upload projection and finalized body, but
    // remove its originating committed membership. Those indices cannot serve
    // as proof of a retained closure after restart.
    use sqlx::Connection;
    let mut c = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(path.join(".ledger/local.db")),
    )
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER workflow_manifest_members_delete_immutable")
        .execute(&mut c)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_manifest_members WHERE identity_key=? AND kind='workflow-definition-fragment'").bind(key(&first).bytes().unwrap()).execute(&mut c).await.unwrap();
    sqlx::query("CREATE TRIGGER workflow_manifest_members_delete_immutable BEFORE DELETE ON workflow_manifest_members BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END").execute(&mut c).await.unwrap();
    c.close().await.unwrap();
    let damaged = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        damaged.read(&canonical_read(&finalized)).await,
        Err(ServiceError::IntegrityFailure)
    );
    assert_eq!(
        damaged.status(&key(&finalize).bytes().unwrap()).await,
        Err(ServiceError::IntegrityFailure)
    );
    assert_eq!(
        damaged
            .prerequisite(&records::bytes(&finalize).unwrap())
            .await,
        Err(ServiceError::IntegrityFailure)
    );
    damaged.close().await;
}
