//! Actual-reader bridge; expectations live in the independent Python observer.
use super::*;

pub(crate) async fn report<S: ComparisonReadStore, A: ComparisonReadAuthority>(
    reader: &S,
    authority: &A,
    who: &AuthenticatedReadContext,
    selection: RetainedSelection,
) -> FoundationReport {
    let operation = loop {
        match ComparisonOperation::begin(Cancellation::default()) {
            Ok(op) => break op,
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    };
    let workspace = load_workspace(reader, authority, who, selection.clone(), &operation)
        .await
        .unwrap();
    let source = activity(&workspace).unwrap();
    let mut candidates = vec![];
    for (key, atoms) in [
        ("retail-alternative-a", 1600),
        ("retail-alternative-b", 800),
    ] {
        let mut amounts = source.original_amounts();
        for row in &mut amounts {
            if source
                .bindings()
                .iter()
                .any(|b| b.agreement == row.key.agreement && b.book == Book::Retail)
            {
                row.amount = o::Amount::Fixed(
                    Money::new(
                        source.retail_basis().currency(),
                        source.retail_basis().scale(),
                        atoms,
                    )
                    .unwrap(),
                );
            }
        }
        candidates.push(c::AmountCandidate {
            key: key.into(),
            amounts,
        });
    }
    let result = evaluate_workspace(&workspace, &candidates, &operation)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(result.bytes()).unwrap();
    assert_eq!(parsed["milestone"], "PHASE-4 FOUNDATION ONLY");
    assert_eq!(parsed["committed"], false);
    // The original control's independently verified receipt values remain exact.
    assert!(parsed["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["result"]["steps"] == parsed["original"]["result"]["steps"]));
    let mut reversed = candidates.clone();
    reversed.reverse();
    let other = evaluate_workspace(&workspace, &reversed, &operation)
        .await
        .unwrap();
    let other: Value = serde_json::from_slice(other.bytes()).unwrap();
    assert_eq!(parsed["candidates"][0], other["candidates"][1]);
    assert_eq!(parsed["candidates"][1], other["candidates"][0]);
    let mut forbidden = candidates.clone();
    let supplier = forbidden[0]
        .amounts
        .iter_mut()
        .find(|a| {
            source
                .bindings()
                .iter()
                .any(|b| b.agreement == a.key.agreement && b.book == Book::Supplier)
        })
        .unwrap();
    supplier.amount = o::Amount::Fixed(
        Money::new(
            source.retail_basis().currency(),
            source.retail_basis().scale(),
            1,
        )
        .unwrap(),
    );
    let denied = evaluate_workspace(&workspace, &forbidden, &operation)
        .await
        .unwrap();
    let denied: Value = serde_json::from_slice(denied.bytes()).unwrap();
    assert_eq!(
        denied["candidates"][0]["result"]["reason"],
        "COMPARISON_SUPPLIER_TERMS"
    );
    assert!(denied["candidates"][0]["result"].get("latest").is_none());
    assert_eq!(denied["candidates"][1], parsed["candidates"][1]);
    assert!(matches!(
        ComparisonOperation::begin(Cancellation::default()),
        Err(ComparisonError::Busy)
    ));
    drop(operation);
    // Cancellation on the same runtime is observable between driver advances.
    let cancellation = Cancellation::default();
    let operation = loop {
        match ComparisonOperation::begin(cancellation.clone()) {
            Ok(op) => break op,
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    };
    let (cancelled, ()) = tokio::join!(
        evaluate_workspace(&workspace, &candidates, &operation),
        async {
            tokio::task::yield_now().await;
            cancellation.cancel();
        }
    );
    assert!(matches!(cancelled, Err(ComparisonError::Cancelled)));
    drop(operation);
    let direct = loop {
        match compare_retained(
            reader,
            authority,
            who,
            selection.clone(),
            &candidates,
            Cancellation::default(),
        )
        .await
        {
            Ok(report) => break report,
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    };
    assert_eq!(result.bytes(), direct.bytes());
    assert!(canonical::outcome::decode(result.bytes()).is_err());
    // No original source evidence bodies or current authority documents appear.
    let text = std::str::from_utf8(result.bytes()).unwrap();
    for forbidden in [
        "evidence_ref",
        "authentication",
        "grant_revision",
        "economic_ingress_hash",
        "connection_string",
        "password",
        "canonical_bytes",
    ] {
        assert!(!text.contains(forbidden));
    }
    result
}

pub(crate) fn preserve_report(report: &FoundationReport, backend: &str) {
    if let Ok(dir) = std::env::var("LEDGERLAB_FOUNDATION_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/{backend}-report.json"), report.bytes()).unwrap();
    }
}

pub(crate) fn observe(
    report: &FoundationReport,
    backend: &str,
    before: Value,
    after: Value,
    reopened: Value,
    metadata: Value,
) {
    use std::process::{Command, Stdio};
    let envelope = json!({"report":serde_json::from_slice::<Value>(report.bytes()).unwrap(),"backend":backend,"B0":before,"B1":after,"B2":reopened,"attempted_writes":0,"external_calls":0,"metadata":metadata});
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ledgerlab-testkit/oracle/phase4/foundation.py"),
        )
        .arg("--evidence")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "foundation independent observer {backend}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    if let Ok(dir) = std::env::var("LEDGERLAB_FOUNDATION_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let steps = envelope["report"]["original"]["result"]["steps"]
            .as_array()
            .unwrap()
            .len();
        std::fs::write(
            format!("{dir}/{backend}-prefix-{steps}-evidence.json"),
            bytes,
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/{backend}-prefix-{steps}-observer.json"),
            output.stdout,
        )
        .unwrap();
    }
}
