//! Independent oracle literals applied to the actual retained SQLite chronology.
use super::*;
use crate::store::adjudication::{AdjudicationReadStore, AdjudicationReadTx, SnapshotSelection};
pub(super) async fn check(store: &SqliteStore, witness: &[Value]) {
    let audit = if std::env::var("LEDGERLAB_R3_SQL_OBSERVE").as_deref() == Ok("1") {
        Some(comparison_observer::Audit::install())
    } else {
        None
    };
    let before = store.test_full_inventory().await;
    if let Some(audit) = &audit {
        audit.begin();
        store.test_comparison_driver_positive_controls().await;
        let statements = audit.end();
        assert!(statements
            .iter()
            .any(|s| s.contains("UPDATE r3_heads SET revision=revision WHERE 0")));
        assert!(statements
            .iter()
            .any(|s| s.contains("CREATE TEMP TABLE comparison_observer_probe")));
        assert!(
            statements
                .iter()
                .filter(|s| comparison_observer::business_write(s))
                .count()
                >= 2
        );
        assert_eq!(store.test_full_inventory().await, before);
        eprintln!("R3_SQL_POSITIVE_CONTROLS actual driver observed zero-row UPDATE and rolled-back TEMP DDL while full inventory unchanged");
    }

    let budget = wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).unwrap(),
        pages: Count::new(4096).unwrap(),
        segments: Count::new(4096).unwrap(),
    };
    let read = store
        .begin_adjudication_read(
            &SnapshotSelection {
                journal: journal("center"),
                historical: None,
            },
            &budget,
            deadline(),
        )
        .await
        .unwrap();
    let expected = read.expected_prefix().expected().clone();
    read.finish().await.unwrap();
    let close = witness
        .iter()
        .find(|s| s["command"]["kind"] == "CLOSE")
        .unwrap();
    let coverage: Vec<wire::Coverage> =
        serde_json::from_value(close["result"]["effects"][0]["body"]["cutoffs"].clone()).unwrap();
    assert_eq!(coverage.len(), 4);
    const THROUGH: [usize; 15] = [5, 11, 12, 18, 19, 25, 26, 32, 38, 44, 91, 92, 93, 94, 95];
    const ORIGINAL: [i128; 15] = [
        10000, 10000, 11200, 11200, 11200, 11200, 11700, 11700, 11700, 11700, 11700, 11800, 11650,
        11650, 11450,
    ];
    const ALTERNATIVE: [i128; 15] = [
        10000, 10000, 11500, 11500, 11500, 11500, 12000, 12000, 12000, 12000, 12000, 12100, 11950,
        11950, 11750,
    ];
    for amount in [1200, 1500, 6000] {
        let mut request = wire::ComparisonRequest {
            expected: expected.clone(),
            policy: wire::ComparisonPolicy {
                resolution_atoms: Count::new(amount).unwrap(),
            },
            budget: budget.clone(),
            coverage: coverage.clone(),
            cursor: None,
        };
        if let Some(audit) = &audit {
            audit.begin();
        }
        let mut comparison = SqliteComparison::from_store(store.clone(), &request)
            .await
            .unwrap();
        let mut checkpoints = 0;
        loop {
            let response = comparison.advance(&request).await.unwrap_or_else(|e| {
                panic!("comparison amount{amount} cursor{:?}: {e}", request.cursor)
            });
            let (ordinal, actual, alternative, supplier) = comparison.test_progress();
            for (i, through) in THROUGH.iter().enumerate() {
                let n = witness[..*through]
                    .iter()
                    .filter(|s| s["host"] == "center")
                    .count() as u128;
                if amount != 6000
                    && n == ordinal
                    && !matches!(response, wire::ComparisonResponse::PolicyFailure { .. })
                {
                    assert_eq!(actual, ORIGINAL[i], "actual S{i:02}");
                    assert_eq!(supplier, 3000);
                    assert_eq!(
                        alternative,
                        if amount == 1200 {
                            ORIGINAL[i]
                        } else {
                            ALTERNATIVE[i]
                        },
                        "candidate S{i:02}"
                    );
                    checkpoints += 1;
                }
            }
            match response {
                wire::ComparisonResponse::Incomplete { cursor, .. } => {
                    let mut changed = request.clone();
                    changed.cursor = Some(*cursor.clone());
                    changed.policy.resolution_atoms = Count::new(7).unwrap();
                    assert!(comparison.advance(&changed).await.is_err());
                    request.cursor = Some(*cursor);
                }
                wire::ComparisonResponse::Comparable {
                    actual,
                    alternative,
                    difference,
                    supplier_booked,
                    measured,
                    ..
                } => {
                    assert_ne!(amount, 6000);
                    assert_eq!(actual.value(), 11450);
                    assert_eq!(
                        alternative.value(),
                        if amount == 1200 { 11450 } else { 11750 }
                    );
                    assert_eq!(difference.value(), if amount == 1200 { 0 } else { 300 });
                    assert_eq!(supplier_booked.value(), 3000);
                    assert_eq!(checkpoints, 15);
                    assert_eq!(measured.segments.value(), 55);
                    eprintln!("actual stored comparison policy{amount}: original{} alternative{} difference{} supplier{};15oracle prefixes; charged{}bytes/{}pages/{}segments",actual.value(),alternative.value(),difference.value(),supplier_booked.value(),measured.bytes.value(),measured.pages.value(),measured.segments.value());
                    break;
                }
                wire::ComparisonResponse::PolicyFailure {
                    at_case, reason, ..
                } => {
                    assert_eq!(amount, 6000);
                    assert!(reason.starts_with("PREMIUM_CAP at central ordinal "));
                    assert_eq!(at_case.0 .2.as_str(), "resolution");
                    assert_eq!(ordinal, 55);
                    assert_eq!(reason, "PREMIUM_CAP at central ordinal 5");
                    eprintln!("actual stored comparison6000: firstfailureS02 {reason}; no comparable partial total");
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        drop(comparison);
        if let Some(audit) = &audit {
            let statements = audit.end();
            assert!(statements.len() > 100);
            let writes = statements
                .iter()
                .filter(|s| comparison_observer::business_write(s))
                .count();
            assert_eq!(
                writes, 0,
                "actual SQLx driver observed mutation: {statements:?}"
            );
            eprintln!("R3_SQL_OBSERVATION policy{amount} actual_driver_statements={} business_write_attempts={writes}",statements.len());
            if let Ok(path) = std::env::var("LEDGERLAB_R3_SQL_TRACE_DIR") {
                std::fs::write(
                    std::path::Path::new(&path).join(format!("policy-{amount}.json")),
                    serde_json::to_vec_pretty(&statements).unwrap(),
                )
                .unwrap();
            }
        }
        assert_eq!(store.test_full_inventory().await, before);
    }
}

#[path = "comparison_observer.rs"]
mod comparison_observer;
