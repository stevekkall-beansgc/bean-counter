use ledgerlab_testkit::cases::{acceptance_cases, verify_overlap, Case};
use ledgerlab_testkit::failpoints::{Boundary, Edge, Fault, Hit, Injection};
use ledgerlab_testkit::history::Snapshot;
use ledgerlab_testkit::stores::{BackendKind, RaceEvidence, TraceEvent, TraceKind};
use ledgerlab_testkit::FixtureOracle;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

fn oracle() -> &'static FixtureOracle {
    static ORACLE: OnceLock<FixtureOracle> = OnceLock::new();
    ORACLE.get_or_init(|| FixtureOracle::workspace().expect("frozen expectations disagree; stop"))
}

/// Assertion self-test data ONLY. This is never passed to a backend runner and
/// supplies no evidence that a database or production evaluator was exercised.
fn assertion_examples() -> (Snapshot, Snapshot) {
    let oracle = oracle();
    let mut before = Snapshot {
        journal: oracle.seed_journal.clone(),
        indexes: oracle.seed_indexes.clone(),
        state: oracle.pre_state.clone(),
        ..Snapshot::default()
    };
    for (table, (count, _)) in &oracle.counts {
        before.rows.insert(
            table.clone(),
            (0..*count)
                .map(|i| (i.to_be_bytes().to_vec(), b"existing".to_vec()))
                .collect(),
        );
    }
    before
        .rows
        .insert("adapter_extra_table".into(), BTreeMap::new());
    let mut after = before.clone();
    after.journal = oracle.post_journal.clone();
    after.indexes = oracle.post_indexes.clone();
    after.state = oracle.post_state.clone();
    after.operational = Some(oracle.operational.clone());
    for (table, (count, delta)) in &oracle.counts {
        for i in *count..count + delta {
            after
                .rows
                .get_mut(table)
                .unwrap()
                .insert(i.to_be_bytes().to_vec(), b"new".to_vec());
        }
    }
    *after
        .rows
        .get_mut("chains")
        .unwrap()
        .values_mut()
        .next()
        .unwrap() = b"revision 1".to_vec();
    (before, after)
}

#[test]
fn frozen_integrity_ids_arithmetic_and_parser_profile() {
    let oracle = oracle();
    assert_eq!(oracle.post_journal.len() - oracle.seed_journal.len(), 25);
    assert_ne!(
        oracle.input("input").unwrap(),
        oracle.input("equivalent").unwrap()
    );
    assert_eq!(oracle.counts["actions"], (0, 2));
}

#[test]
fn every_frozen_write_item_has_before_and_after() {
    let oracle = oracle();
    assert_eq!(oracle.write_boundaries.len(), 54);
    for pair in oracle.write_boundaries.as_chunks::<2>().0 {
        match (&pair[0], &pair[1]) {
            (
                Boundary::Write {
                    name: a,
                    item: i,
                    edge: Edge::Before,
                },
                Boundary::Write {
                    name: b,
                    item: j,
                    edge: Edge::After,
                },
            ) => {
                assert_eq!((a, i), (b, j));
            }
            _ => panic!("incomplete item schedule"),
        }
    }
    let cases = acceptance_cases(oracle);
    assert_eq!(cases.len(), 83);
    assert_eq!(
        cases
            .iter()
            .filter(|c| matches!(c, Case::WriteRollback(_)))
            .count(),
        54
    );
    assert_eq!(
        cases
            .iter()
            .filter(|c| matches!(c, Case::Invalid(_)))
            .count(),
        11
    );
    assert!(cases.contains(&Case::ZeroAction));
    assert!(cases.contains(&Case::UnknownCommit { durable: false }));
    assert!(cases.contains(&Case::UnknownCommit { durable: true }));
}

#[test]
fn exact_comparison_rejects_missing_extra_rewritten_and_corrupt_rows() {
    let (before, after) = assertion_examples();
    oracle().assert_accepted(&before, &after).unwrap();
    for table in ["actions", "delivery_state", "snapshots", "chain_revisions"] {
        let mut bad = after.clone();
        bad.rows.get_mut(table).unwrap().pop_first();
        assert!(oracle().assert_accepted(&before, &bad).is_err(), "{table}");
    }
    let mut bad = after.clone();
    bad.rows
        .get_mut("adapter_extra_table")
        .unwrap()
        .insert(vec![1], vec![2]);
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    *bad.rows
        .get_mut("documents")
        .unwrap()
        .values_mut()
        .next()
        .unwrap() = b"changed seed".to_vec();
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    bad.journal[0].push(b' ');
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    bad.state = before.state.clone();
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    bad.rows.remove("inbox");
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    bad.operational = None;
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    let mut bad = after.clone();
    bad.indexes[0].push(b' ');
    assert!(oracle().assert_accepted(&before, &bad).is_err());
    assert!(after.assert_exact(&before, "reopen").is_err());
}

#[test]
fn ignored_or_multiply_fired_hooks_fail_closed() {
    let injection = Injection {
        boundary: Boundary::BeforeCommitSend,
        fault: Fault::Rollback,
    };
    for hits in [0, 2] {
        assert!(Hit {
            injection: injection.clone(),
            hits
        }
        .verify(&injection)
        .is_err());
    }
    let wrong = Injection {
        boundary: Boundary::AfterCommitAcknowledged,
        fault: Fault::LoseReply,
    };
    assert!(Hit {
        injection: wrong,
        hits: 1
    }
    .verify(&injection)
    .is_err());
    Hit {
        injection: injection.clone(),
        hits: 1,
    }
    .verify(&injection)
    .unwrap();
}

fn event(sequence: u64, request: usize, kind: TraceKind) -> TraceEvent {
    TraceEvent {
        sequence,
        request,
        connection: format!("connection-{request}"),
        kind,
    }
}

#[test]
fn race_evidence_requires_actual_connection_contention() {
    use ledgerlab_testkit::stores::{Attempt, Outcome};
    let attempts = vec![
        Attempt {
            outcome: Outcome::Accepted(vec![]),
            hit: None
        };
        2
    ];
    let mut race = RaceEvidence {
        attempts,
        trace: vec![
            event(1, 0, TraceKind::RequestStarted),
            event(2, 1, TraceKind::RequestStarted),
            event(3, 0, TraceKind::TransactionOpened),
            event(4, 1, TraceKind::TransactionOpened),
            event(5, 0, TraceKind::LockHeld),
            event(6, 1, TraceKind::LockBlocked),
            event(7, 0, TraceKind::CommitAcknowledged),
            event(8, 0, TraceKind::RequestFinished),
            event(9, 1, TraceKind::CommitAcknowledged),
            event(10, 1, TraceKind::RequestFinished),
        ],
    };
    verify_overlap(&race, BackendKind::Postgres18, 2).unwrap();
    assert!(verify_overlap(&race, BackendKind::FileSqlite, 2).is_err());
    race.trace[5].kind = TraceKind::BeginBlocked;
    verify_overlap(&race, BackendKind::FileSqlite, 2).unwrap();
    race.trace[5].connection = "connection-0".into();
    assert!(verify_overlap(&race, BackendKind::FileSqlite, 2).is_err());
    race.trace.retain(|e| e.kind != TraceKind::BeginBlocked);
    assert!(verify_overlap(&race, BackendKind::Postgres18, 2).is_err());
}

#[test]
fn paid_journal_cannot_pass_zero_action_oracle() {
    assert!(oracle()
        .verify_zero_journal(&oracle().post_journal, &oracle().post_indexes)
        .is_err());
}

#[test]
fn independent_python_negative_and_property_tests() {
    let output = Command::new("python3")
        .arg("-B")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle/test_oracle.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
