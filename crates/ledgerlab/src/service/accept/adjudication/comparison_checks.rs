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
    use crate::outbox::fake::MemoryDestination;
    if audit.is_some() {
        use crate::outbox::{
            fake::{Mode, Outcome},
            Attempt, Lease, Request,
        };
        let destination = MemoryDestination::new();
        let calls = MemoryDestination::test_boundary_calls();
        destination.fence("observer-control", 1, 10).unwrap();
        let attempt = Attempt {
            lease: Lease {
                store_id: "observer-control".into(),
                owner: "control".into(),
                generation: 1,
                restore_generation: 1,
            },
            number: 1,
            until: 10,
            request: Request {
                store_id: "observer-control".into(),
                key: "positive".into(),
                request_hash: "synthetic-control".into(),
                payload: b"control".to_vec(),
            },
        };
        assert!(matches!(
            destination.send(&attempt, 1, Mode::Normal),
            Outcome::Delivered(_)
        ));
        assert!(matches!(
            destination.lookup("observer-control", "positive", Mode::Normal),
            Outcome::Delivered(_)
        ));
        assert_eq!(MemoryDestination::test_boundary_calls() - calls, 3);
        assert_eq!(store.test_full_inventory().await, before);
        eprintln!("R3_DESTINATION_POSITIVE_CONTROLS actual fake-destination fence/send/lookup observed3calls, successful independent receipt, ledger inventory unchanged");
    }

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
        let external_before = MemoryDestination::test_boundary_calls();
        let mut comparison = SqliteComparison::from_store(store, &request, &budget)
            .await
            .unwrap();
        assert!(
            matches!(
                SqliteComparison::from_store(store, &request, &budget).await,
                Err(ServiceError::Retryable)
            ),
            "one optional comparison workspace per live host"
        );
        let mut checkpoints = 0;
        let mut completed_segments = 0;
        if amount == 1200 {
            request.budget = wire::ReadBudget {
                bytes: Count::new(1).unwrap(),
                pages: Count::new(1).unwrap(),
                segments: Count::new(1).unwrap(),
            };
            let wire::ComparisonResponse::Incomplete {
                cursor, measured, ..
            } = comparison.advance(&request).await.unwrap()
            else {
                panic!("tiny budget must yield")
            };
            assert_eq!((cursor.ordinal.value(), cursor.byte_offset.value()), (0, 0));
            assert_eq!(
                (
                    measured.bytes.value(),
                    measured.pages.value(),
                    measured.segments.value()
                ),
                (0, 0, 0)
            );
            request.cursor = Some(*cursor);
            let initial = comparison.preparation_measured().clone();
            request.budget = wire::ReadBudget {
                bytes: Count::new(initial.bytes.value() + 64 + 64 + 1).unwrap(),
                pages: Count::new(initial.pages.value() + 4).unwrap(),
                segments: Count::new(1).unwrap(),
            };
            let wire::ComparisonResponse::Incomplete {
                cursor, measured, ..
            } = comparison.advance(&request).await.unwrap()
            else {
                panic!("one funded byte must yield")
            };
            assert_eq!((cursor.ordinal.value(), cursor.byte_offset.value()), (0, 1));
            assert_eq!(measured.bytes, request.budget.bytes);
            assert_eq!(measured.segments, Count::ZERO);
            let mut forged = request.clone();
            forged.cursor = Some(*cursor.clone());
            forged.cursor.as_mut().unwrap().byte_offset = Count::new(2).unwrap();
            assert_eq!(
                comparison.advance(&forged).await.unwrap_err(),
                ServiceError::IntegrityFailure
            );
            request.cursor = Some(*cursor);
            // Finish the raw first segment while leaving no allowance for its dependencies.
            let raw_size = comparison.test_partial_total();
            let pages = raw_size.div_ceil(4096) as u128;
            request.budget = wire::ReadBudget {
                bytes: Count::new(initial.bytes.value() + raw_size as u128 - 1 + 64 * pages)
                    .unwrap(),
                pages: Count::new(initial.pages.value() + 3 * pages).unwrap(),
                segments: Count::new(1).unwrap(),
            };
            let wire::ComparisonResponse::Incomplete {
                cursor, measured, ..
            } = comparison.advance(&request).await.unwrap()
            else {
                panic!("unfunded dependencies must yield")
            };
            assert_eq!(cursor.byte_offset.value(), raw_size as u128);
            assert_eq!(comparison.test_progress().0, 0);
            assert_eq!(measured.segments.value(), 1);
            completed_segments += 1;
            request.cursor = Some(*cursor);
            // Offering a larger quota never exceeds the backend's bounded work slice.
            request.budget = wire::ReadBudget {
                bytes: Count::new(1 << 40).unwrap(),
                pages: Count::new(1 << 40).unwrap(),
                segments: Count::new(1 << 40).unwrap(),
            };
            {
                use std::{
                    future::Future,
                    task::{Context, Poll, Waker},
                };
                let mut canceled = Box::pin(comparison.advance(&request));
                assert!(matches!(
                    canceled
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Pending
                ));
                drop(canceled);
            }
            assert_eq!(comparison.test_progress().0, 0);
            eprintln!("comparison budget: tiny no-I/O yield, funded one-byte cursor, completed raw segment/dependency yield, forged offset rejection, cancellation and large-budget bounded resume");
        }

        loop {
            let response = comparison.advance(&request).await.unwrap_or_else(|e| {
                panic!("comparison amount{amount} cursor{:?}: {e}", request.cursor)
            });
            completed_segments += match &response {
                wire::ComparisonResponse::Incomplete { measured, .. }
                | wire::ComparisonResponse::Comparable { measured, .. }
                | wire::ComparisonResponse::PolicyFailure { measured, .. }
                | wire::ComparisonResponse::Unsupported { measured, .. } => {
                    measured.segments.value()
                }
            };
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
                    assert_eq!(completed_segments, 55);
                    eprintln!("actual stored comparison policy{amount}: original{} alternative{} difference{} supplier{};15oracle prefixes; final call charged{}bytes/{}pages/{}segments",actual.value(),alternative.value(),difference.value(),supplier_booked.value(),measured.bytes.value(),measured.pages.value(),measured.segments.value());
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
        if audit.is_some() {
            let calls = MemoryDestination::test_boundary_calls() - external_before;
            assert_eq!(
                calls, 0,
                "comparison invoked an external destination boundary"
            );
            eprintln!(
                "R3_DESTINATION_OBSERVATION policy{amount} actual_fake_destination_calls={calls}"
            );
        }

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
    // A failed candidate still owes complete coverage verification. An omitted
    // gateway fails at enrollment; an invented complete cutoff fails only after
    // the full selected chronology, rather than leaking an early policy result.
    for omitted in [true, false] {
        let mut bad = coverage.clone();
        if omitted {
            bad.pop();
        } else if let wire::Coverage::CompleteGatewayCutoff { observation, .. } = &mut bad[0] {
            *observation = Digest::parse(&"f".repeat(64)).unwrap();
        } else {
            panic!("complete cutoff")
        }
        let mut request = wire::ComparisonRequest {
            expected: expected.clone(),
            policy: wire::ComparisonPolicy {
                resolution_atoms: Count::new(6000).unwrap(),
            },
            budget: budget.clone(),
            coverage: bad,
            cursor: None,
        };
        let mut comparison = SqliteComparison::from_store(store, &request, &budget)
            .await
            .unwrap();
        let mut calls = 0;
        loop {
            calls += 1;
            match comparison.advance(&request).await {
                Ok(wire::ComparisonResponse::Incomplete { cursor, .. }) => {
                    request.cursor = Some(*cursor)
                }
                Err(ServiceError::IntegrityFailure) => break,
                other => panic!("unverified coverage returned {other:?}"),
            }
        }
        assert_eq!(calls, if omitted { 1 } else { 55 });
        assert_eq!(store.test_full_inventory().await, before);
    }
    eprintln!("comparison coverage: earlier named retained cutoff remains valid; omitted gateway and forged complete cutoff rejected including policy-failure candidate");
}

#[path = "comparison_observer.rs"]
mod comparison_observer;
