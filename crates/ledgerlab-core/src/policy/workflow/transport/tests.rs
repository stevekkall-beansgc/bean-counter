use super::*;
fn expected() -> Value {
    serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/transport-02-independent/expected.json")).unwrap()
}
fn request(name: &str) -> Value {
    expected()["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == name)
        .unwrap()["normalized_request"]
        .clone()
}
fn parse(value: &Value) -> SourceRetentionInput {
    SourceRetentionInput::parse(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn fragments() -> Vec<DefinitionFragment> {
    expected()["envelopes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "workflow-definition-fragment")
        .map(|e| DefinitionFragment::parse(&serde_json::to_vec(&e["body"]).unwrap()).unwrap())
        .collect()
}
fn encoded(raw: &[u8]) -> String {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | (chunk.get(2).copied().unwrap_or(0) as u32);
        out.push(table[(n >> 18) as usize] as char);
        out.push(table[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            table[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            table[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
#[test]
fn exact_independent_fragment_records_and_complete_template_reassembly() {
    let x = expected();
    for name in ["fragment-0", "fragment-1"] {
        let req = request(name);
        let input = parse(&req["payload"]);
        let f = input.fragment().unwrap();
        let value = f
            .record_value(
                req["key"]["logical_store_id"].as_str().unwrap(),
                &["fixture".into(), "sandbox".into()],
                req["key"]["source"].as_str().unwrap(),
            )
            .unwrap();
        let actual = DefinitionFragment::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        let e = x["envelopes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| {
                e["kind"] == "workflow-definition-fragment" && e["body"]["part"] == value["part"]
            })
            .unwrap();
        assert_eq!(
            actual.canonical_bytes(),
            CanonicalBytes::from_value(&e["body"]).unwrap().as_slice()
        );
        assert_eq!(actual.reference().id(), e["id"]);
        assert_eq!(actual.reference().digest(), e["digest"]);
    }
    let req = request("finalize-template");
    let input = parse(&req["payload"]);
    let bytes = input
        .finalization()
        .unwrap()
        .reassemble(
            &fragments(),
            "fixture-transport-store",
            &["fixture".into(), "sandbox".into()],
            "urn:fixture:publisher",
        )
        .unwrap();
    let template = x["envelopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "workflow-slot-template")
        .unwrap();
    assert_eq!(
        bytes,
        CanonicalBytes::from_value(&template["body"])
            .unwrap()
            .as_slice()
    );
}
#[test]
fn exact_chunk_and_document_boundaries_are_checked_before_reassembly() {
    for total in [1usize, 131071, 131072, 131073, 262144] {
        for part in 0..if total > CHUNK { 2 } else { 1 } {
            let len = if part == 0 {
                total.min(CHUNK)
            } else {
                total - CHUNK
            };
            let mut body = request("fragment-0")["payload"].clone();
            body["document_bytes"] = json!(total.to_string());
            body["part"] = json!(part.to_string());
            body["bytes_base64"] = json!(encoded(&vec![0; len]));
            assert_eq!(parse(&body).fragment().unwrap().decoded_bytes().len(), len);
            body["bytes_base64"] = json!(encoded(&vec![0; len - 1]));
            assert!(SourceRetentionInput::parse(&serde_json::to_vec(&body).unwrap()).is_err());
        }
    }
    let mut body = request("fragment-0")["payload"].clone();
    for value in ["0", "262145", "01"] {
        body["document_bytes"] = json!(value);
        assert!(SourceRetentionInput::parse(&serde_json::to_vec(&body).unwrap()).is_err());
    }
    body["document_bytes"] = json!("131072");
    body["part"] = json!("1");
    assert!(SourceRetentionInput::parse(&serde_json::to_vec(&body).unwrap()).is_err());
}
#[test]
fn raw_transport_is_closed_and_base64_has_only_one_spelling() {
    let body = request("fragment-0")["payload"].clone();
    for (key, value) in [
        ("extra", json!(true)),
        ("part", Value::Null),
        ("document_schema", json!("ledger-workflow-binding/1")),
        ("canonical_document_sha256", json!("A".repeat(64))),
    ] {
        let mut bad = body.clone();
        bad[key] = value;
        assert!(SourceRetentionInput::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let raw = serde_json::to_string(&body)
        .unwrap()
        .replacen("{", "{\"p\\u0061rt\":\"1\",", 1);
    assert!(SourceRetentionInput::parse(raw.as_bytes()).is_err());
    let mut small = body;
    small["document_bytes"] = json!("1");
    small["bytes_base64"] = json!("AB==");
    assert!(SourceRetentionInput::parse(&serde_json::to_vec(&small).unwrap()).is_err());
    small["bytes_base64"] = json!("AA==");
    parse(&small);
    let mut raw = serde_json::to_vec(&small).unwrap();
    raw.resize(MAX_DOCUMENT, b' ');
    parse(&canonical::parse(&raw).unwrap());
    assert!(SourceRetentionInput::parse(&raw).is_ok());
    raw.push(b' ');
    assert!(SourceRetentionInput::parse(&raw).is_err());
}
#[test]
fn finalize_refuses_duplicate_missing_mixed_upload_and_wrong_context() {
    let req = request("finalize-template");
    let input = parse(&req["payload"]);
    let f = input.finalization().unwrap();
    let parts = fragments();
    let scope = ["fixture".into(), "sandbox".into()];
    for (store, scope, source) in [
        ("other-store", scope.clone(), "urn:fixture:publisher"),
        (
            "fixture-transport-store",
            ["other".into(), "sandbox".into()],
            "urn:fixture:publisher",
        ),
        (
            "fixture-transport-store",
            scope.clone(),
            "urn:other:publisher",
        ),
    ] {
        assert!(f.reassemble(&parts, store, &scope, source).is_err());
    }
    assert!(f
        .reassemble(
            &parts[..1],
            "fixture-transport-store",
            &scope,
            "urn:fixture:publisher"
        )
        .is_err());
    let mut body = req["payload"].clone();
    body["fragment_refs"] = json!([body["fragment_refs"][0], body["fragment_refs"][0]]);
    assert!(SourceRetentionInput::parse(&serde_json::to_vec(&body).unwrap()).is_err());
    body = req["payload"].clone();
    body["upload_id"] = json!("another-upload");
    assert!(parse(&body)
        .finalization()
        .unwrap()
        .reassemble(
            &parts,
            "fixture-transport-store",
            &scope,
            "urn:fixture:publisher"
        )
        .is_err());
}
#[test]
fn reassembly_requires_exact_hash_and_canonical_json_without_asserting_type_support() {
    let canonical = b"{\"schema\":\"ledger-workflow-fact-schema/1\"}";
    for bytes in [
        canonical.as_slice(),
        b" {\"schema\":\"ledger-workflow-fact-schema/1\"}".as_slice(),
    ] {
        let mut payload = request("fragment-0")["payload"].clone();
        payload["document_schema"] = json!("ledger-workflow-fact-schema/1");
        payload["document_bytes"] = json!(bytes.len().to_string());
        payload["bytes_base64"] = json!(encoded(bytes));
        payload["canonical_document_sha256"] = json!(canonical::hex(&Sha256::digest(bytes)));
        let input = parse(&payload);
        let record = input
            .fragment()
            .unwrap()
            .record_value("store", &["fixture".into(), "sandbox".into()], "source")
            .unwrap();
        let part = DefinitionFragment::parse(&serde_json::to_vec(&record).unwrap()).unwrap();
        let finalize = parse(
            &json!({"schema":"ledger-workflow-definition-finalize/1","upload_id":input.fragment().unwrap().upload_id(),"fragment_refs":[part.reference()]}),
        );
        let result = finalize.finalization().unwrap().reassemble(
            &[part],
            "store",
            &["fixture".into(), "sandbox".into()],
            "source",
        );
        assert_eq!(result.is_ok(), bytes == canonical);
        // The minimal schema-only body is not a typed valid FactSchema; this
        // transport result must still pass the separate definition normalizer.
        if let Ok(bytes) = result {
            assert!(super::super::FactSchema::parse(&bytes).is_err());
        }
    }
}
