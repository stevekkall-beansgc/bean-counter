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
    assert!(
        ledger
            .prerequisite(&records::bytes(&finalize).unwrap())
            .await
            .is_err()
    );
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
    assert!(
        ledger
            .prerequisite(&records::bytes(&spoof).unwrap())
            .await
            .is_err()
    );
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
