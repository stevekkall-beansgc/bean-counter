//! Test-only durable observations. No expected bytes come from an accepted plan.
use super::outcomes::{OutcomeSnapshot, StoredCompositeDelivery};
use serde_json::{json, Value};

pub(crate) fn observe(
    snapshot: OutcomeSnapshot,
    deliveries: Vec<StoredCompositeDelivery>,
    physical: Vec<(String, Vec<String>)>,
) -> Value {
    let utf8 = |b: Vec<u8>| String::from_utf8(b).unwrap();
    let mut records: Vec<_> = snapshot.records.into_iter().map(utf8).collect();
    records.sort();
    let mut heads: Vec<_> = snapshot
        .heads
        .into_iter()
        .map(|h| {
            json!({
                "class":format!("{:?}", h.lock.class), "key_utf8":utf8(h.lock.key),
                "revision":h.revision, "value_utf8":h.value.map(utf8)
            })
        })
        .collect();
    heads.sort_by_key(Value::to_string);
    let mut anchors: Vec<_> = snapshot
        .anchors
        .into_iter()
        .map(|a| {
            json!({
                "scope":a.scope,"kind":a.kind,"id_utf8":utf8(a.id),"content_hash":a.content_hash
            })
        })
        .collect();
    anchors.sort_by_key(Value::to_string);
    let deliveries: Vec<_> = deliveries.into_iter().map(|d| json!({
        "scope":d.key.scope,"source":d.key.source,"external_id":d.key.external_id,
        "canonical_source":d.canonical_key.source,"canonical_external_id":d.canonical_key.external_id,
        "command_utf8":utf8(d.command),"ingress_utf8":utf8(d.ingress),"ingress_hash":d.ingress_hash,
        "economic_utf8":d.economic_receipt.map(utf8),"settlement_utf8":utf8(d.settlement_receipt)
    })).collect();
    json!({"records_utf8":records,"heads":heads,"anchors":anchors,"deliveries":deliveries,"physical":physical})
}

pub(crate) fn check(backend: &str, prefixes: Vec<Value>) {
    let evidence = json!({"backend":backend,"prefixes":prefixes});
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("observed.json");
    std::fs::write(&path, serde_json::to_vec(&evidence).unwrap()).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ledgerlab-testkit/oracle/phase3/durable.py");
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(script)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "independent durable audit: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    if let Some(dir) = std::env::var_os("LEDGERLAB_P3_EVIDENCE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(&path, dir.join(format!("{backend}.json"))).unwrap();
    }
}
