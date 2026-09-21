//! Independent expected histories shared by both physical-store adapters.
use crate::store::{
    errors::{CommitError, StoreError},
    ports::{AcceptanceStore, AcceptanceTx},
    records::*,
};
use ledgerlab_testkit::{history::Snapshot, stores as tk, FixtureOracle};
use serde_json::{json, Value};
use tokio::time::{Duration, Instant};

pub(super) fn expected(zero: bool, count: usize) -> Value {
    let output = std::process::Command::new("python3")
        .args([
            "-B",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ledgerlab-testkit/oracle/review_histories.py"
            ),
            if zero { "zero" } else { "history" },
            &count.to_string(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
pub(super) fn commands(expected: &Value) -> Vec<tk::Command> {
    let oracle = FixtureOracle::workspace().unwrap();
    expected["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let mut c = tk::Command::fixture(&oracle, "input").unwrap();
            c.bytes = serde_json::to_vec(v).unwrap();
            c
        })
        .collect()
}
pub(super) fn seed(expected: &Value) -> Vec<WriteOp> {
    let mut ops: Vec<_> = expected["seed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| WriteOp::Journal(Box::new(crate::store::sqlite::tests::row(v))))
        .collect();
    for mut op in crate::store::sqlite::tests::seed() {
        if let WriteOp::SeedBinding(b) = &mut op {
            b.selector_doc = expected["state"]["binding_head"]["selector_doc"]
                .as_str()
                .unwrap()
                .into();
        }
        if !matches!(op, WriteOp::Journal(_)) {
            ops.push(op);
        }
    }
    ops
}
pub(super) fn assert_history(snapshot: &Snapshot, expected: &Value) {
    let bytes = |key: &str| -> Vec<Vec<u8>> {
        expected[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| serde_json::to_vec(v).unwrap())
            .collect()
    };
    assert_eq!(
        snapshot.journal,
        bytes("journal"),
        "independent full canonical journal"
    );
    assert_eq!(
        snapshot.indexes,
        bytes("indexes"),
        "independent physical indexes"
    );
    let count = expected["commands"].as_array().unwrap().len();
    let state: Value = serde_json::from_slice(&snapshot.state).unwrap();
    assert_eq!(state["chain"]["revision"], count.to_string());
    assert_eq!(state["chain"]["event_count"], count.to_string());
    assert_eq!(
        snapshot.row_count("documents").unwrap(),
        7,
        "one shared snapshot"
    );
    assert_eq!(snapshot.row_count("snapshots").unwrap(), 7 * count);
    assert_eq!(snapshot.row_count("actions").unwrap(), 2 * count);
    assert_eq!(snapshot.row_count("explanations").unwrap(), 2 * count);
    let intentions: usize = expected["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["intention_ids"].as_array().unwrap().len())
        .sum();
    assert_eq!(snapshot.row_count("intentions").unwrap(), intentions);
    assert_eq!(snapshot.row_count("delivery_state").unwrap(), intentions);
}
pub(super) fn history<B: tk::AcceptanceBackend>(backend: &mut B, expected: &Value) {
    let initial = backend.observe().unwrap();
    for (i, c) in commands(expected).iter().enumerate() {
        let receipt = serde_json::to_vec(&expected["receipts"][i]).unwrap();
        assert_eq!(
            backend.accept(c, None).unwrap().outcome,
            tk::Outcome::Accepted(receipt)
        );
        backend.reopen().unwrap();
        for (j, retry) in commands(expected).iter().take(i + 1).enumerate() {
            assert_eq!(
                backend.accept(retry, None).unwrap().outcome,
                tk::Outcome::Duplicate {
                    kind: tk::DuplicateKind::Identity,
                    receipt: serde_json::to_vec(&expected["receipts"][j]).unwrap()
                }
            );
        }
    }
    let before = backend.observe().unwrap();
    assert_history(&before, expected);
    let oracle = FixtureOracle::workspace().unwrap();
    let count = expected["commands"].as_array().unwrap().len();
    let mut deltas: std::collections::BTreeMap<_, _> = oracle
        .counts
        .iter()
        .map(|(table, (_, delta))| (table.clone(), delta * count))
        .collect();
    deltas.insert("documents".into(), 1);
    let intentions = expected["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["intention_ids"].as_array().unwrap().len())
        .sum();
    deltas.insert("intentions".into(), intentions);
    deltas.insert("delivery_state".into(), intentions);
    initial.assert_delta(&before, &deltas, &["chains"]).unwrap();
    backend.reopen().unwrap();
    before
        .assert_exact(&backend.observe().unwrap(), "history reopen")
        .unwrap();
}
pub(super) fn binding_retries<B: tk::AcceptanceBackend>(
    backend: &mut B,
    expected: &Value,
    suffix: &str,
) {
    for (i, c) in commands(expected).iter().enumerate() {
        let receipt = serde_json::to_vec(&expected["receipts"][i]).unwrap();
        assert_eq!(
            backend.accept(c, None).unwrap().outcome,
            tk::Outcome::Duplicate {
                kind: tk::DuplicateKind::Identity,
                receipt: receipt.clone()
            }
        );
        let mut alias = c.clone();
        let mut body: Value = serde_json::from_slice(&c.bytes).unwrap();
        body["id"] = json!(format!("alias-{suffix}-{i}"));
        alias.bytes = serde_json::to_vec(&body).unwrap();
        assert_eq!(
            backend.accept(&alias, None).unwrap().outcome,
            tk::Outcome::Duplicate {
                kind: tk::DuplicateKind::Semantic,
                receipt
            }
        );
    }
    assert_history(&backend.observe().unwrap(), expected);
}
pub(super) async fn collisions<S: AcceptanceStore>(store: &S, field: usize) {
    let original = crate::store::sqlite::tests::schedule().remove(0);
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.write(&original).await.unwrap();
    tx.commit().await.unwrap();
    {
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        tx.write(&original).await.unwrap();
        // Prove a later collision also prevents an earlier unrelated write.
        let mut prior = original.clone();
        if let WriteOp::Journal(r) = &mut prior {
            if let JournalRow::Document { id, .. } = &mut r.row {
                *id = format!("doc_{}", "a".repeat(64));
            }
        }
        tx.write(&prior).await.unwrap();
        let mut bad = original.clone();
        if let WriteOp::Journal(r) = &mut bad {
            match field {
                0 => {
                    if let JournalRow::Document { kind, .. } = &mut r.row {
                        *kind = "policy".into();
                    }
                }
                1 => r.canonical.canonical_bytes.push(b' '),
                _ => r.canonical.content_hash = format!("sha256:{}", "b".repeat(64)),
            }
        }
        assert!(matches!(
            tx.write(&bad).await,
            Err(StoreError::Integrity("immutable document collision"))
        ));
        assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
    }
}
pub(super) async fn distinct_race<S>(stores: Vec<(S, Option<String>)>, expected: &Value)
where
    S: AcceptanceStore + 'static,
    S::Tx: super::race_tests::ConnectionId + 'static,
{
    let commands = commands(expected);
    let evidence = super::race_tests::race_commands(stores, &commands)
        .await
        .unwrap();
    assert!(evidence.trace.iter().any(|e| matches!(
        e.kind,
        tk::TraceKind::BeginBlocked | tk::TraceKind::LockBlocked
    )));
    for (i, a) in evidence.attempts.iter().enumerate() {
        assert_eq!(
            a.outcome,
            tk::Outcome::Accepted(serde_json::to_vec(&expected["receipts"][i]).unwrap())
        );
    }
}
