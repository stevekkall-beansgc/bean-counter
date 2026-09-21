//! Executable scenarios. Generic functions become real-store tests when an
//! integration adapter implements the relevant traits; no implicit skips.
use crate::failpoints::{AwaitClass, Boundary, CommitPhase, Fault, Injection};
use crate::history::Snapshot;
use crate::oracle::{check, HarnessError};
use crate::stores::*;
use crate::{FixtureOracle, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Case {
    Fresh,
    IdentityDuplicate,
    IdentityConflict,
    SemanticDuplicate,
    SemanticConflict,
    QuantityNormalization,
    Invalid(String),
    UnauthorizedSource,
    UnauthorizedPrincipal,
    UnauthorizedReceiptRead,
    MissingRealTerms,
    EvaluationInvalid,
    EvaluationOverflow,
    WriteRollback(Boundary),
    PreCommitRollback,
    LostResponse,
    UnknownCommit { durable: bool },
    ActiveCommitAbsent,
    ZeroAction,
}

pub fn acceptance_cases(oracle: &FixtureOracle) -> Vec<Case> {
    let mut cases = vec![
        Case::Fresh,
        Case::IdentityDuplicate,
        Case::IdentityConflict,
        Case::SemanticDuplicate,
        Case::SemanticConflict,
        Case::QuantityNormalization,
        Case::UnauthorizedSource,
        Case::UnauthorizedPrincipal,
        Case::UnauthorizedReceiptRead,
        Case::MissingRealTerms,
        Case::EvaluationInvalid,
        Case::EvaluationOverflow,
        Case::PreCommitRollback,
        Case::LostResponse,
        Case::UnknownCommit { durable: false },
        Case::UnknownCommit { durable: true },
        Case::ActiveCommitAbsent,
        Case::ZeroAction,
    ];
    cases.extend(["unknown", "null", "duplicate_key"].map(|s| Case::Invalid(s.into())));
    cases.extend(
        oracle
            .inputs
            .keys()
            .filter(|k| k.starts_with("invalid:"))
            .cloned()
            .map(Case::Invalid),
    );
    cases.extend(
        oracle
            .write_boundaries
            .iter()
            .cloned()
            .map(Case::WriteRollback),
    );
    cases
}

#[derive(Clone, Debug)]
pub struct CaseReport {
    pub case: String,
    pub backend: BackendEvidence,
}

fn real_backend<B: AcceptanceBackend>(backend: &B) -> Result<BackendEvidence> {
    let evidence = backend.evidence();
    check(
        !evidence.location.is_empty()
            && !evidence.engine_version.is_empty()
            && !evidence.durability.is_empty(),
        "missing actual backend provenance",
    )?;
    check(
        !evidence.location.contains(":memory:"),
        "real file-backed store required",
    )?;
    match evidence.kind {
        BackendKind::FileSqlite => check(
            std::path::Path::new(&evidence.location).is_file()
                && evidence.version_number >= 3_051_003,
            "real SQLite file and linked SQLite >= 3.51.3 required",
        )?,
        BackendKind::Postgres18 => check(
            (180_000..190_000).contains(&evidence.version_number),
            "expected actual PostgreSQL 18 server",
        )?,
        BackendKind::Postgres17 => check(
            (170_000..180_000).contains(&evidence.version_number),
            "expected actual PostgreSQL 17 server",
        )?,
    }
    Ok(evidence)
}

fn reopen<B: AcceptanceBackend>(backend: &mut B, expected: &Snapshot) -> Result<()> {
    backend.reopen()?;
    expected.assert_exact(&backend.observe()?, "reopen")
}

fn outcome(attempt: Attempt, expected: Outcome) -> Result<()> {
    check(attempt.hit.is_none(), "unexpected active failpoint")?;
    check(
        attempt.outcome == expected,
        format!(
            "outcome mismatch: {:?}, expected {expected:?}",
            attempt.outcome
        ),
    )
}

fn receipt(attempt: Attempt, expected: &[u8]) -> Result<()> {
    match attempt.outcome {
        Outcome::Accepted(bytes) | Outcome::Duplicate { receipt: bytes, .. } => {
            check(bytes == expected, "resolved receipt bytes differ")
        }
        other => Err(HarnessError(format!(
            "expected resolved receipt, got {other:?}"
        ))),
    }
}

fn fire<B: AcceptanceBackend>(
    backend: &mut B,
    command: &Command,
    injection: &Injection,
) -> Result<Outcome> {
    let attempt = backend.accept(command, Some(injection))?;
    attempt
        .hit
        .ok_or_else(|| HarnessError("missing failpoint evidence".into()))?
        .verify(injection)?;
    Ok(attempt.outcome)
}

fn fresh<B: AcceptanceBackend>(
    backend: &mut B,
    oracle: &FixtureOracle,
    before: &Snapshot,
) -> Result<Snapshot> {
    outcome(
        backend.accept(&Command::fixture(oracle, "input")?, None)?,
        Outcome::Accepted(oracle.receipt.clone()),
    )?;
    let after = backend.observe()?;
    oracle.assert_accepted(before, &after)?;
    reopen(backend, &after)?;
    Ok(after)
}

fn unknown_identity(result: &Outcome) -> Result<()> {
    check(
        *result
            == Outcome::OutcomeUnknown {
                scope: ["demo".into(), "sandbox".into()],
                source: "urn:demo:app".into(),
                external_id: "generation-1".into(),
            },
        "ambiguous commit must retain OUTCOME_UNKNOWN and the original scoped identity",
    )
}

fn clean_pool<B: AcceptanceBackend>(backend: &mut B, must_discard: bool) -> Result<()> {
    let probe = backend.pool_probe()?;
    check(
        !probe.affected_connection.is_empty() && !probe.next_connection.is_empty(),
        "missing pool probe connection IDs",
    )?;
    check(
        !probe.next_has_open_transaction,
        "next borrower received a live transaction",
    )?;
    if must_discard {
        check(
            probe.affected_discarded && probe.affected_connection != probe.next_connection,
            "uncertain connection was not discarded",
        )?;
    }
    Ok(())
}

/// Runs one case against a newly initialized real backend. Never use fixture
/// seeding to append expected accepted rows; all decisions must use the facade.
pub fn run_case<F: BackendFactory>(
    factory: &F,
    oracle: &FixtureOracle,
    case: &Case,
) -> Result<CaseReport> {
    let mut backend = if *case == Case::MissingRealTerms {
        factory.real_without_terms(oracle)?
    } else {
        factory.seeded(oracle)?
    };
    let evidence = real_backend(&backend)?;
    let before = backend.observe()?;
    if *case != Case::MissingRealTerms {
        oracle.assert_seed(&before)?;
    }
    let command = Command::fixture(oracle, "input")?;
    match case {
        Case::Fresh => {
            fresh(&mut backend, oracle, &before)?;
        }
        Case::IdentityDuplicate
        | Case::IdentityConflict
        | Case::SemanticDuplicate
        | Case::SemanticConflict
        | Case::QuantityNormalization
        | Case::UnauthorizedReceiptRead => {
            let accepted = fresh(&mut backend, oracle, &before)?;
            let (input, expected) = match case {
                Case::IdentityDuplicate => (
                    "input",
                    Outcome::Duplicate {
                        kind: DuplicateKind::Identity,
                        receipt: oracle.receipt.clone(),
                    },
                ),
                Case::IdentityConflict => (
                    "identity_conflict",
                    Outcome::Conflict(ConflictKind::Identity),
                ),
                Case::SemanticDuplicate => (
                    "semantic_duplicate",
                    Outcome::Duplicate {
                        kind: DuplicateKind::Semantic,
                        receipt: oracle.receipt.clone(),
                    },
                ),
                Case::SemanticConflict => (
                    "semantic_conflict",
                    Outcome::Conflict(ConflictKind::Semantic),
                ),
                Case::QuantityNormalization => (
                    "equivalent",
                    Outcome::Duplicate {
                        kind: DuplicateKind::Identity,
                        receipt: oracle.receipt.clone(),
                    },
                ),
                _ => ("input", Outcome::Rejected(Rejection::Unauthorized)),
            };
            let mut retry = Command::fixture(oracle, input)?;
            if *case == Case::UnauthorizedReceiptRead {
                retry.principal = Principal::NoReadPermission;
            }
            outcome(backend.accept(&retry, None)?, expected)?;
            let after = backend.observe()?;
            if *case == Case::SemanticDuplicate {
                check(
                    after.journal == accepted.journal
                        && after.indexes == accepted.indexes
                        && after.state == accepted.state
                        && after.operational == accepted.operational,
                    "semantic alias mutated accepted history",
                )?;
                check(
                    after.aliases.len() == 1,
                    "semantic retry must retain exactly one alias",
                )?;
                let alias = &after.aliases[0];
                check(
                    alias.scope == ["demo", "sandbox"]
                        && alias.source == "urn:demo:app"
                        && alias.external_id == "generation-alias"
                        && alias.canonical_receipt == oracle.receipt
                        && alias.observed_at == RECEIVED_AT,
                    "alias mapping fields differ",
                )?;
                check(
                    alias.ingress == oracle.input("alias_ingress")?
                        && alias.ingress_hash.as_bytes() == oracle.input("alias_ingress_hash")?,
                    "alias ingress not retained exactly",
                )?;
                accepted.assert_delta(
                    &after,
                    &BTreeMap::from([("delivery_keys".into(), 1)]),
                    &[],
                )?;
                // The alias must itself participate in identity lookup on retry.
                outcome(
                    backend.accept(&retry, None)?,
                    Outcome::Duplicate {
                        kind: DuplicateKind::Identity,
                        receipt: oracle.receipt.clone(),
                    },
                )?;
                after.assert_exact(&backend.observe()?, "repeated alias")?;
            } else {
                accepted.assert_exact(&after, "duplicate/conflict/rejected read")?;
            }
            reopen(&mut backend, &after)?;
        }
        Case::Invalid(_)
        | Case::UnauthorizedSource
        | Case::UnauthorizedPrincipal
        | Case::MissingRealTerms => {
            let mut candidate = match case {
                Case::Invalid(name) => Command::fixture(oracle, name)?,
                Case::UnauthorizedSource => Command::fixture(oracle, "unauthorized_source")?,
                _ => command.clone(),
            };
            if *case == Case::UnauthorizedPrincipal {
                candidate.principal = Principal::NoSubmitPermission;
            }
            let rejection = match case {
                Case::Invalid(_) => Rejection::InvalidInput,
                Case::MissingRealTerms => Rejection::TermsNotAccepted,
                _ => Rejection::Unauthorized,
            };
            outcome(
                backend.accept(&candidate, None)?,
                Outcome::Rejected(rejection),
            )?;
            before.assert_exact(
                &backend.observe()?,
                "rejection reserved identity or mutated state",
            )?;
            reopen(&mut backend, &before)?;
        }
        Case::EvaluationInvalid
        | Case::EvaluationOverflow
        | Case::WriteRollback(_)
        | Case::PreCommitRollback => {
            let (boundary, fault, expected) = match case {
                Case::EvaluationInvalid => (
                    Boundary::EvaluatorAfterBase,
                    Fault::EvaluationInvalid,
                    Outcome::Rejected(Rejection::EvaluationInvalid),
                ),
                Case::EvaluationOverflow => (
                    Boundary::EvaluatorAfterBase,
                    Fault::EvaluationOverflow,
                    Outcome::Rejected(Rejection::ArithmeticOverflow),
                ),
                Case::WriteRollback(boundary) => {
                    (boundary.clone(), Fault::Rollback, Outcome::RolledBack)
                }
                _ => (
                    Boundary::BeforeCommitSend,
                    Fault::Rollback,
                    Outcome::RolledBack,
                ),
            };
            let actual = fire(&mut backend, &command, &Injection { boundary, fault })?;
            if *case == Case::PreCommitRollback && matches!(actual, Outcome::OutcomeUnknown { .. })
            {
                unknown_identity(&actual)?;
                clean_pool(&mut backend, true)?;
            } else {
                check(actual == expected, "fault result classification differs")?;
                clean_pool(&mut backend, false)?;
            }
            before.assert_exact(&backend.observe()?, "fault rollback")?;
            reopen(&mut backend, &before)?;
            // Same ID remains usable after rollback; no leaked reservation/transaction.
            fresh(&mut backend, oracle, &before)?;
        }
        Case::LostResponse => {
            let result = fire(
                &mut backend,
                &command,
                &Injection {
                    boundary: Boundary::AfterCommitAcknowledged,
                    fault: Fault::LoseReply,
                },
            )?;
            check(
                result == Outcome::ResponseLost,
                "lost response was not observed",
            )?;
            let committed = backend.observe()?;
            oracle.assert_accepted(&before, &committed)?;
            reopen(&mut backend, &committed)?;
            outcome(
                backend.accept(&command, None)?,
                Outcome::Duplicate {
                    kind: DuplicateKind::Identity,
                    receipt: oracle.receipt.clone(),
                },
            )?;
            committed.assert_exact(&backend.observe()?, "lost response retry")?;
        }
        Case::UnknownCommit { durable } => {
            let result = fire(
                &mut backend,
                &command,
                &Injection {
                    boundary: Boundary::CommitInFlight,
                    fault: Fault::UnknownCommit { durable: *durable },
                },
            )?;
            unknown_identity(&result)?;
            clean_pool(&mut backend, true)?;
            // Fault controller releases the old connection, suppressing lookup only
            // for the first response. The next primary read must show the selected branch.
            backend.reopen()?;
            let intermediate = backend.observe()?;
            if *durable {
                oracle.assert_accepted(&before, &intermediate)?;
            } else {
                before.assert_exact(
                    &intermediate,
                    "confirmed absent after old connection drained",
                )?;
            }
            receipt(backend.resolve_and_retry(&command)?, &oracle.receipt)?;
            let after = backend.observe()?;
            oracle.assert_accepted(&before, &after)?;
            reopen(&mut backend, &after)?;
            outcome(
                backend.accept(&command, None)?,
                Outcome::Duplicate {
                    kind: DuplicateKind::Identity,
                    receipt: oracle.receipt.clone(),
                },
            )?;
            after.assert_exact(
                &backend.observe()?,
                "unknown outcome retry created another decision",
            )?;
        }
        Case::ActiveCommitAbsent => {
            let result = fire(
                &mut backend,
                &command,
                &Injection {
                    boundary: Boundary::CommitInFlight,
                    fault: Fault::UnknownCommitActive,
                },
            )?;
            unknown_identity(&result)?;
            let probe = backend.probe_active_commit(&command)?;
            check(
                probe.primary_rows_absent && probe.original_transaction_active,
                "absent-primary/live-transaction branch was not exercised",
            )?;
            unknown_identity(&probe.outcome)?;
            receipt(backend.resolve_and_retry(&command)?, &oracle.receipt)?;
            clean_pool(&mut backend, true)?;
            let after = backend.observe()?;
            oracle.assert_accepted(&before, &after)?;
            reopen(&mut backend, &after)?;
        }
        Case::ZeroAction => {
            run_zero(&mut backend, oracle, &before)?;
        }
    }
    Ok(CaseReport {
        case: format!("{case:?}"),
        backend: evidence,
    })
}

fn run_zero<B: AcceptanceBackend>(
    backend: &mut B,
    oracle: &FixtureOracle,
    before: &Snapshot,
) -> Result<()> {
    let command = Command::fixture(oracle, "zero")?;
    let attempt = backend.accept(&command, None)?;
    let after = backend.observe()?;
    let expected_receipt = oracle.verify_zero_journal(&after.journal, &after.indexes)?;
    outcome(attempt, Outcome::Accepted(expected_receipt.clone()))?;
    let evidence = backend.zero_evidence()?;
    check(
        evidence.event_id.as_bytes() == oracle.input("zero_event_id")?
            && evidence.receipt == expected_receipt
            && evidence.explanation_codes == ["FAILED_WORK"]
            && evidence.action_ids.is_empty()
            && evidence.intention_ids.is_empty()
            && evidence.revision == "1"
            && evidence.event_count == "1",
        "zero decision semantics",
    )?;
    check(
        after.state == oracle.post_state
            && after.operational.as_deref() == Some(oracle.input("zero_operational")?)
            && after.aliases.is_empty(),
        "zero control/operational state",
    )?;
    let mut deltas: BTreeMap<_, _> = oracle
        .counts
        .iter()
        .map(|(k, (_, d))| (k.clone(), *d))
        .collect();
    for table in [
        "effects",
        "actions",
        "action_sources",
        "action_dependencies",
        "intentions",
        "delivery_state",
    ] {
        deltas.insert(table.into(), 0);
    }
    deltas.insert("explanations".into(), 1);
    before.assert_delta(&after, &deltas, &["chains"])?;
    reopen(backend, &after)?;
    outcome(
        backend.accept(&command, None)?,
        Outcome::Duplicate {
            kind: DuplicateKind::Identity,
            receipt: expected_receipt,
        },
    )?;
    after.assert_exact(&backend.observe()?, "zero duplicate")
}

pub fn run_cancellation<F: BackendFactory>(
    factory: &F,
    oracle: &FixtureOracle,
) -> Result<Vec<CaseReport>> {
    let points = factory.seeded(oracle)?.cancellation_points()?;
    check(
        !points.is_empty(),
        "adapter has no instrumented await catalogue",
    )?;
    let unique: BTreeSet<_> = points.iter().map(|p| &p.name).collect();
    check(
        unique.len() == points.len(),
        "duplicate cancellation boundary name",
    )?;
    for phase in [
        CommitPhase::Before,
        CommitPhase::InFlight,
        CommitPhase::Acknowledged,
    ] {
        check(
            points.iter().any(|p| p.phase == phase),
            format!("missing cancellation phase {phase:?}"),
        )?;
    }
    check(
        points.iter().any(|p| p.trigger.is_some()),
        "missing error/rollback await path",
    )?;
    for class in [
        AwaitClass::Begin,
        AwaitClass::Lock,
        AwaitClass::Read,
        AwaitClass::Commit,
        AwaitClass::Rollback,
        AwaitClass::Cleanup,
    ] {
        check(
            points.iter().any(|p| p.class == class),
            format!("missing await class {class:?}"),
        )?;
    }
    for boundary in &oracle.write_boundaries {
        if let Boundary::Write { name, item, .. } = boundary {
            check(
                points.iter().any(|p| {
                    p.class
                        == AwaitClass::Write {
                            name: name.clone(),
                            item: *item,
                        }
                }),
                format!("missing cancellation at write {name}[{item}]"),
            )?;
        }
    }
    let mut reports = Vec::new();
    for point in points {
        let mut backend = factory.seeded(oracle)?;
        let evidence = real_backend(&backend)?;
        let before = backend.observe()?;
        oracle.assert_seed(&before)?;
        let command = Command::fixture(oracle, "input")?;
        let attempt = backend.cancel(&command, &point)?;
        let expected = Injection {
            boundary: Boundary::Await {
                name: point.name.clone(),
                phase: point.phase,
            },
            fault: Fault::Cancel,
        };
        attempt
            .hit
            .ok_or_else(|| HarnessError("cancellation hook was not reached".into()))?
            .verify(&expected)?;
        match (&point.phase, &attempt.outcome) {
            (CommitPhase::Before, Outcome::Cancelled | Outcome::RolledBack) => (),
            (CommitPhase::InFlight, Outcome::RolledBack) => (),
            (CommitPhase::InFlight, Outcome::OutcomeUnknown { .. }) => {
                unknown_identity(&attempt.outcome)?
            }
            (CommitPhase::InFlight | CommitPhase::Acknowledged, Outcome::Accepted(bytes)) => check(
                *bytes == oracle.receipt,
                "cancel returned incorrect receipt",
            )?,
            (CommitPhase::InFlight | CommitPhase::Acknowledged, Outcome::ResponseLost) => (),
            _ => {
                return Err(HarnessError(
                    "cancellation result incompatible with commit phase".into(),
                ))
            }
        }
        clean_pool(
            &mut backend,
            matches!(attempt.outcome, Outcome::OutcomeUnknown { .. }),
        )?;
        backend.reopen()?;
        let actual = backend.observe()?;
        if point.phase == CommitPhase::Before || matches!(attempt.outcome, Outcome::RolledBack) {
            before.assert_exact(&actual, "pre-commit cancellation")?;
        } else if point.phase == CommitPhase::Acknowledged
            || matches!(
                attempt.outcome,
                Outcome::Accepted(_) | Outcome::ResponseLost
            )
        {
            oracle.assert_accepted(&before, &actual)?;
        } else {
            check(
                actual == before || oracle.assert_accepted(&before, &actual).is_ok(),
                "partial journal after in-flight cancellation",
            )?;
        }
        receipt(backend.resolve_and_retry(&command)?, &oracle.receipt)?;
        let after = backend.observe()?;
        oracle.assert_accepted(&before, &after)?;
        reopen(&mut backend, &after)?;
        reports.push(CaseReport {
            case: format!("cancel:{}", point.name),
            backend: evidence,
        });
    }
    Ok(reports)
}

pub fn verify_overlap(
    evidence: &RaceEvidence,
    kind: BackendKind,
    participants: usize,
) -> Result<()> {
    check(
        evidence.attempts.len() == participants && participants >= 2,
        "race participant count",
    )?;
    check(
        evidence
            .trace
            .windows(2)
            .all(|w| w[0].sequence < w[1].sequence),
        "unordered race trace",
    )?;
    let mut starts = BTreeMap::new();
    let mut ends = BTreeMap::new();
    for event in &evidence.trace {
        check(
            event.request < participants && !event.connection.is_empty(),
            "invalid request/connection trace",
        )?;
        match event.kind {
            TraceKind::RequestStarted => {
                check(
                    starts.insert(event.request, event.sequence).is_none(),
                    "duplicate start",
                )?;
            }
            TraceKind::RequestFinished => {
                check(
                    ends.insert(event.request, event.sequence).is_none(),
                    "duplicate finish",
                )?;
            }
            _ => (),
        }
    }
    check(
        starts.len() == participants && ends.len() == participants,
        "incomplete race lifecycle",
    )?;
    check(
        starts.iter().all(|(id, start)| *start < ends[id]),
        "race ends before start",
    )?;
    check(
        starts.values().max() < ends.values().min(),
        "requests never actually overlapped",
    )?;
    if kind != BackendKind::FileSqlite {
        for request in 0..participants {
            check(
                evidence.trace.iter().any(|e| {
                    e.request == request
                        && e.kind == TraceKind::TransactionOpened
                        && starts[&request] < e.sequence
                        && e.sequence < ends[&request]
                }),
                "PG race lacks an actual transaction for a participant",
            )?;
        }
    }
    let blocked_kind = if kind == BackendKind::FileSqlite {
        TraceKind::BeginBlocked
    } else {
        TraceKind::LockBlocked
    };
    let mut contention = false;
    for held in evidence
        .trace
        .iter()
        .filter(|e| e.kind == TraceKind::LockHeld)
    {
        for blocked in evidence.trace.iter().filter(|e| e.kind == blocked_kind) {
            let release = evidence
                .trace
                .iter()
                .find(|e| e.request == held.request && e.kind == TraceKind::CommitAcknowledged);
            if let Some(release) = release {
                contention |= held.request != blocked.request
                    && held.connection != blocked.connection
                    && held.sequence < blocked.sequence
                    && blocked.sequence < release.sequence;
            }
        }
    }
    check(contention, "no real competing connection/lock evidence")
}

pub fn run_race<F>(factory: &F, oracle: &FixtureOracle, participants: usize) -> Result<CaseReport>
where
    F: BackendFactory,
    F::Backend: RaceBackend,
{
    let mut backend = factory.seeded(oracle)?;
    let evidence = real_backend(&backend)?;
    let before = backend.observe()?;
    oracle.assert_seed(&before)?;
    let race = backend.concurrent_identical(&Command::fixture(oracle, "input")?, participants)?;
    verify_overlap(&race, evidence.kind, participants)?;
    let mut accepted = 0;
    for attempt in race.attempts {
        match &attempt.outcome {
            Outcome::Accepted(_) => accepted += 1,
            Outcome::Duplicate {
                kind: DuplicateKind::Identity,
                ..
            } => (),
            _ => {
                return Err(HarnessError(
                    "race did not settle as accepted/identity duplicate".into(),
                ))
            }
        }
        receipt(attempt, &oracle.receipt)?;
    }
    check(accepted == 1, "race must have exactly one Accepted")?;
    let after = backend.observe()?;
    oracle.assert_accepted(&before, &after)?;
    reopen(&mut backend, &after)?;
    Ok(CaseReport {
        case: format!("concurrent_identical:{participants}"),
        backend: evidence,
    })
}

fn remote(evidence: &DeliveryEvidence, oracle: &FixtureOracle, state: DeliveryState) -> Result<()> {
    check(
        evidence.state == state && evidence.intention_id == oracle.intention_id,
        "delivery state/key",
    )?;
    check(
        !evidence.destination_location.is_empty() && evidence.remote.len() == 1,
        "expected one independently durable remote receipt",
    )?;
    let row = &evidence.remote[0];
    check(
        row.destination == "fake"
            && row.key == oracle.intention_id
            && row.request_hash == oracle.payload_digest
            && row.payload == oracle.payload
            && row.amount_atoms == "80"
            && !row.receipt.is_empty(),
        "remote payload/receipt differs",
    )
}

pub fn run_delivery<F>(factory: &F, oracle: &FixtureOracle) -> Result<CaseReport>
where
    F: BackendFactory,
    F::Backend: DeliveryBackend,
{
    let mut backend = factory.seeded(oracle)?;
    let evidence = real_backend(&backend)?;
    let before = backend.observe()?;
    let accepted = fresh(&mut backend, oracle, &before)?;
    let lost = backend.enable_fake_and_lose_response()?;
    remote(&lost, oracle, DeliveryState::Unknown)?;
    check(
        lost.lost_response_observed,
        "downstream response was not lost",
    )?;
    backend.reopen()?;
    backend.reopen_destination()?;
    let still_unknown = backend.read_fake()?;
    remote(&still_unknown, oracle, DeliveryState::Unknown)?;
    check(
        still_unknown.remote == lost.remote,
        "remote receipt did not survive reopen",
    )?;
    let delivered = backend.reconcile_fake()?;
    remote(&delivered, oracle, DeliveryState::Delivered)?;
    check(
        delivered.remote == lost.remote,
        "reconciliation created/changed a remote receipt",
    )?;
    let after = backend.observe()?;
    check(
        after.journal == accepted.journal
            && after.indexes == accepted.indexes
            && after.state == oracle.input("delivery_enabled_state")?
            && after.aliases == accepted.aliases,
        "delivery changed economics/control state",
    )?;
    check(
        accepted.rows.keys().eq(after.rows.keys()),
        "delivery changed table inventory",
    )?;
    // Only the listed delivery tables may change during dispatch/reconciliation.
    for (table, rows) in &accepted.rows {
        if ![
            "installation",
            "delivery_state",
            "dispatcher_head",
            "dispatch_attempts",
            "delivery_observations",
            "fake_receipts",
        ]
        .contains(&table.as_str())
        {
            check(
                after.rows.get(table) == Some(rows),
                format!("delivery changed {table}"),
            )?;
        }
    }
    reopen(&mut backend, &after)?;
    backend.reopen_destination()?;
    let repeated = backend.reconcile_fake()?;
    remote(&repeated, oracle, DeliveryState::Delivered)?;
    check(
        repeated.remote == lost.remote,
        "repeated reconciliation duplicated destination receipt",
    )?;
    Ok(CaseReport {
        case: "fake_lost_response_reconciliation".into(),
        backend: evidence,
    })
}
