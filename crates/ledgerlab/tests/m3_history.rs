//! M3 public-API evidence slice: continuous history and concurrency.
//!
//! These tests use only the public `ledgerlab::billing::BillingLedger` facade
//! and the public local error surface of one ordinary local SQLite
//! installation. They change no production limit, add no dependency, and treat
//! the product's own refusals as the expected result rather than as defects.
//!
//! Evidence scope, stated up front:
//!
//! - What is proved here is the behavior of one host, one installation, one
//!   process, one store instance and one serialized writer, at the sizes these
//!   tests actually run. It is not a capacity guarantee, a saturation result, a
//!   resource reservation, or a completion guarantee for accepted offline work.
//! - A `Retryable` result is a busy writer, not an answer. A caller retries it
//!   under the same identity, counts it, and never reads it as a refusal or a
//!   success. A definite refusal is one `ServiceError::Rejection` carrying the
//!   product's own code, and nothing else is: an unavailable store, an unknown
//!   commit outcome, an exhausted read budget, a failed integrity check or a
//!   local fault is neither a refusal nor an answer, so every other result fails
//!   its test by name instead of being counted as a decision the product made.
//! - Concurrency is proved with a `tokio::sync::Barrier` that releases every
//!   racer before any racer can enter the store, and with a channel that
//!   announces each snapshot call to a snapshot task parked before a live write
//!   stream starts, one announcement per snapshot call. `tokio::join!` alone is not
//!   treated as evidence of a race: it does not prevent the joined futures from
//!   running one after one on a single-threaded runtime, which is exactly what
//!   happened before this slice was reviewed.
//! - The public `statement`/`export_csv` path and the public write path share
//!   the store's single physical writer connection, so a complete snapshot is
//!   never produced from *inside* an open write transaction, and this slice
//!   does not claim otherwise. What the snapshot test proves is the weaker
//!   exact thing, and it proves it per capture rather than as a total: for
//!   every captured statement call and every captured CSV projection call, one
//!   named public `accept` **attempt** was inside the store at the instant that
//!   call was invoked — named by the decision it was submitting and by which
//!   attempt of that decision it was. The witness is always a single attempt,
//!   never a retry loop: the span between a busy result and the next attempt is a
//!   backoff gap and not an attempt, so no gap can ever be credited as an
//!   in-flight write. Because the one writer connection admits one attempt at a
//!   time, that attempt is also the only attempt of the live stream in flight at
//!   that instant, and the test asserts exactly that rather than assuming it.
//!   These are overlapping *public calls*: a snapshot is never read from inside
//!   an open write transaction, and no containment of one call by another is
//!   claimed. The measured geometry of each pair is printed instead.
//! - The long-running M3 acceptance gate is `#[ignore]`d and documented with
//!   its exact command below. It is run separately from the ordinary integration
//!   suite so its repeated 1,026-decision workload remains explicit.
//! - Postponed capabilities (PostgreSQL, multi-host, resource proofs, hosted
//!   operation, payments) are untouched by this file.

use ledgerlab::{billing::BillingLedger, local::LocalError, ServiceError};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, Barrier},
    time::Instant,
};

const CUSTOMER: &str = "customer-1";
const SOURCE: &str = "urn:example:work";
/// The shared example setup fixes the ordinary outcome window start at exactly
/// this instant, so work may not be reported as occurring after it.
const WORK_OCCURRED_AT: &str = "2026-09-01T00:00:00.000000Z";
const ADJUSTMENT_OCCURRED_AT: &str = "2026-09-22T00:00:00.000000Z";
const SETUP: &[u8] = include_bytes!("../../../examples/billing/setup.json");
const MAPPING: &[u8] = include_bytes!("../../../examples/finance/mapping.json");
/// The former M2 retained-decision ceiling. M3 acceptance proves that the
/// measured source-built profile retains decisions across this boundary.
const PRE_M3_DECISION_CEILING: usize = 1000;
/// The first decision past the pre-M3 ceiling. M3 acceptance requires this
/// decision, and decisions after it, to be accepted and retained.
const ACCEPTANCE_CROSSING: usize = 1001;
/// Decisions submitted after the crossing, so acceptance covers repeated
/// crossings of the old boundary rather than a single decision at it.
const ACCEPTANCE_TAIL: usize = 25;
/// How many independent fresh installations the M3 acceptance run must clear. One
/// installation would show a first crossing; M3 requires the crossing to
/// repeat, so the same run is repeated on unrelated installations of their own.
const ACCEPTANCE_INSTALLATIONS: usize = 3;
/// The agreed fixed price in integer atoms, used to reconcile totals from an
/// independent model instead of from the statement under test.
const PRICE_ATOMS: i128 = 250;
/// Callers released from one barrier for a genuine race.
const RACERS: usize = 4;
/// Snapshot calls taken during that live stream. Each round is one public
/// statement call and one public CSV projection call, and each of them is
/// witnessed separately.
const SNAPSHOT_ROUNDS: usize = 3;
/// One announcement per public snapshot call. Announcements can queue while an
/// earlier snapshot waits for storage, so six writes alone do not guarantee
/// that the later calls overlap a live attempt.
const SNAPSHOT_CALLS: usize = 2 * SNAPSHOT_ROUNDS;
/// The stream keeps submitting unique decisions until every capture finishes.
/// This ceiling bounds a starved/failed harness; hitting it fails the test.
const MAX_LIVE_WRITES: usize = 64;

/// One public call's own budget. A busy writer is retried under the same
/// identity within this budget; a call that spends it all still returns its busy
/// result, which is an uncertain outcome and never an answer.
const CALL_DEADLINE: Duration = Duration::from_secs(5);

/// Every published snapshot file gets its own serial, so two captures that
/// legitimately report the same cutoff still write to different paths. The
/// product publishes with `create_new`, so a repeated path would fail instead
/// of overwriting an earlier projection.
static CAPTURE_SERIAL: AtomicUsize = AtomicUsize::new(0);

fn next_capture_serial() -> usize {
    CAPTURE_SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// The repository keeps every other test's scratch space under `work/`, which is
/// git-ignored. The store must be private, so the parent is canonicalized before
/// the product's own `0700` check runs. The temporary directory is held for the
/// lifetime of the test so the installation is removed afterwards.
struct Scratch {
    _root: tempfile::TempDir,
    path: PathBuf,
}

fn scratch(name: &str) -> Scratch {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/m3-history");
    fs::create_dir_all(&parent).unwrap();
    let root = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    Scratch {
        // The product creates the installation directory itself with `0700`;
        // pre-creating it here would fail its own privacy check.
        path: root.path().join(name),
        _root: root,
    }
}

async fn install(path: &Path) -> BillingLedger {
    BillingLedger::init(path, SETUP).await.unwrap();
    BillingLedger::open(path).await.unwrap()
}

fn work_event(index: usize) -> Vec<u8> {
    work_event_with(&format!("m3-work-{index:06}"), &format!("m3-op-{index:06}"))
}

fn work_event_with(delivery: &str, operation: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-event/1",
        "id": delivery,
        "operation_id": operation,
        "type": "content.generated",
        "customer": CUSTOMER,
        "occurred_at": WORK_OCCURRED_AT
    }))
    .expect("work event JSON")
}

fn outcome_event(id: &str, target: &str, code: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-billing-outcome/2",
        "customer": CUSTOMER,
        "source": SOURCE,
        "id": id,
        "target": target,
        "family": "quality",
        "occurred_at": ADJUSTMENT_OCCURRED_AT,
        "evidence": "Synthetic M3 outcome recorded by this test",
        "code": code
    }))
    .expect("outcome JSON")
}

/// The product's own refusal code, or a panic. A test never guesses a code, and
/// it never calls anything else a refusal: only a `Rejection` is a refusal, and
/// every other result — a busy writer, an unavailable store, an unknown commit
/// outcome, an exhausted read budget, an integrity failure, a configuration,
/// io or local fault — panics here as the unclassified failure it is.
fn refusal(error: &LocalError) -> String {
    match error {
        LocalError::Service(ServiceError::Rejection(code)) => code.clone(),
        other => panic!("expected a product refusal (ServiceError::Rejection), got {other:?}"),
    }
}

/// A busy writer is an uncertain result, not an answer.
fn is_busy(error: &LocalError) -> bool {
    matches!(error, LocalError::Service(ServiceError::Retryable))
}

fn target_of(receipt: &Value) -> String {
    receipt["body"]["target"]
        .as_str()
        .expect("accepted receipt target")
        .to_owned()
}

fn integer(value: &Value, key: &str) -> i128 {
    value[key]
        .as_str()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| panic!("{key} is not an integer string in {value}"))
}

fn entry_count(statement: &Value) -> usize {
    statement["entries"]
        .as_array()
        .expect("statement entries")
        .len()
}

fn receipt_ids(statement: &Value) -> Vec<String> {
    statement["entries"]
        .as_array()
        .expect("statement entries")
        .iter()
        .map(|entry| {
            entry["receipt"]["id"]
                .as_str()
                .expect("entry receipt id")
                .to_owned()
        })
        .collect()
}

fn expect_complete(statement: &Value) {
    assert_eq!(statement["schema"], "ledger-billing-statement/2");
    assert_eq!(statement["complete"], true, "{statement}");
    assert_eq!(statement["currency"], "USD");
    assert_eq!(statement["scale"], 2);
    assert_eq!(statement["payment_collected"], false);
}

/// A complete statement's own total must equal the sum of its own entries. Every
/// snapshot in this file is checked against the cutoff it declares.
fn expect_self_consistent(statement: &Value) -> usize {
    expect_complete(statement);
    let cutoff = integer(statement, "cutoff") as usize;
    assert_eq!(
        cutoff,
        entry_count(statement),
        "cutoff is not the entry count"
    );
    let summed: i128 = statement["entries"]
        .as_array()
        .expect("statement entries")
        .iter()
        .map(|entry| integer(entry, "net_atoms"))
        .sum();
    assert_eq!(
        summed,
        integer(statement, "net_atoms"),
        "cutoff {cutoff} entries do not sum to its own total"
    );
    assert_eq!(receipt_ids(statement).len(), cutoff);
    cutoff
}

/// Which public call to make against the one serialized store.
#[derive(Debug, Clone, Copy)]
enum Call<'a> {
    /// A new base submission.
    Write(&'a [u8]),
    /// A complete statement read.
    Read,
    /// A finance CSV projection of an already captured pin.
    Export { pin: &'a str, output: &'a Path },
}

/// One *attempt* of one public call: the instant the caller entered the API for
/// that attempt and the instant that attempt's own answer came back.
///
/// Attempts are recorded individually rather than collapsed into one interval
/// per call, because only an attempt is ever inside the store. The span between
/// a busy result and the next attempt is a backoff gap and not an attempt, so a
/// gap can never be credited as an in-flight write. Every overlap witness in
/// this file is read from these intervals and from nothing else.
#[derive(Debug, Clone, Copy)]
struct Attempt {
    opened: Instant,
    left: Instant,
}

impl Attempt {
    /// Whether this attempt was still inside the store at `at`.
    fn in_flight_at(&self, at: Instant) -> bool {
        self.opened <= at && at < self.left
    }
}

/// One public call: what it answered, how many busy results it had to retry, and
/// every attempt it made in order. A busy result is an uncertain outcome, so it
/// is retried under the same identity inside `CALL_DEADLINE` and counted; the
/// last attempt is the one that answered and the ones before it each returned a
/// busy result. The whole call's own start-to-completion interval is derived
/// from these attempts and is reported as measured, never used as a witness.
#[derive(Debug)]
struct Answered {
    result: Result<Value, LocalError>,
    busy_retries: usize,
    attempts: Vec<Attempt>,
}

impl Answered {
    /// The instant the caller first entered the API for this call.
    fn opened(&self) -> Instant {
        self.attempts.first().expect("one attempt at least").opened
    }
    /// The instant the call's answer came back.
    fn left(&self) -> Instant {
        self.attempts.last().expect("one attempt at least").left
    }
    /// The single attempt that answered. Never a retry loop, never a backoff gap.
    fn answering(&self) -> Attempt {
        *self.attempts.last().expect("one attempt at least")
    }
}

/// One public call's own start-to-completion interval: the instant the caller
/// first entered the API and the instant the answer came back. It spans any busy
/// retries, because the call really was in flight for that whole time, and it is
/// therefore reported and never used as an overlap witness.
#[derive(Debug, Clone, Copy)]
struct Window {
    opened: Instant,
    left: Instant,
}

impl Window {
    fn of(answered: &Answered) -> Self {
        Self {
            opened: answered.opened(),
            left: answered.left(),
        }
    }
}

/// One captured public call's own instants: the interval the whole call occupied
/// and the single attempt of it that answered. Both are measured; only the
/// invocation instant of the whole call is used to place a witness, because that
/// is the instant the claim is about.
#[derive(Debug, Clone, Copy)]
struct Span {
    call: Window,
    answering: Attempt,
}

impl Span {
    fn of(answered: &Answered) -> Self {
        Self {
            call: Window::of(answered),
            answering: answered.answering(),
        }
    }
}

/// Whether a busy result is followed by an explicit yield before the next
/// attempt, or whether the next attempt starts without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backoff {
    /// Yield first. A caller that has nothing else to do lets the runtime run
    /// other work before it tries again.
    Yield,
    /// Start the next attempt at once, with no scheduling point in between.
    /// Used only by the live stream, where a scheduling point between two
    /// attempts would create an instant at which the writer is inside no attempt
    /// at all and no witness could exist.
    Immediate,
}

/// Make one public call, retrying a busy writer under the same identity.
///
/// A busy store is an uncertain result, never an answer, so it is retried and
/// counted until the call's own deadline is spent; a call that runs out of
/// budget returns its busy result, which every caller in this file treats as
/// unresolved rather than as a refusal or a success. Every other error is
/// returned as it arrived: a definite refusal is a `Rejection` with a code, and
/// anything else — unavailable, unknown commit outcome, exhausted read budget,
/// integrity failure, configuration, io or local fault — is a failure of this
/// test, not a decision the product made, so it is never read as a refusal and
/// never counted as one. Where a test expects a refusal it asserts the code by
/// name through `refusal`, which panics on anything else.
async fn call(ledger: &BillingLedger, request: Call<'_>) -> Answered {
    call_with(ledger, request, Backoff::Yield).await
}

/// One public call whose busy retries are retried with no yield between
/// attempts.
///
/// This is the same call as `call`, with one difference: a busy result is
/// followed immediately by the next attempt. That makes every scheduling point
/// of the calling task fall inside a recorded attempt, so a task waiting to be
/// released by a channel can never observe this caller between two attempts. The
/// live stream uses it so that a snapshot call's invocation instant is always
/// inside a real attempt, and the snapshot test then *verifies* that for every
/// capture instead of assuming it.
async fn call_in_line(ledger: &BillingLedger, request: Call<'_>) -> Answered {
    call_with(ledger, request, Backoff::Immediate).await
}

async fn call_with(ledger: &BillingLedger, request: Call<'_>, backoff: Backoff) -> Answered {
    let started = Instant::now();
    let mut busy_retries = 0usize;
    let mut attempts: Vec<Attempt> = Vec::new();
    let result = loop {
        let opened = Instant::now();
        let attempt = match request {
            Call::Write(raw) => ledger.accept(CUSTOMER, SOURCE, raw).await,
            Call::Read => ledger.statement(CUSTOMER, None).await,
            Call::Export { pin, output } => ledger.export_csv(CUSTOMER, pin, MAPPING, output).await,
        };
        attempts.push(Attempt {
            opened,
            left: Instant::now(),
        });
        match attempt {
            Err(error) if is_busy(&error) && started.elapsed() <= CALL_DEADLINE => {
                busy_retries += 1;
                if backoff == Backoff::Yield {
                    tokio::task::yield_now().await;
                }
            }
            other => break other,
        }
    };
    let elapsed = started.elapsed();
    assert!(
        elapsed <= CALL_DEADLINE,
        "a public store call exceeded its own deadline: {elapsed:?} after \
         {busy_retries} busy retries"
    );
    Answered {
        result,
        busy_retries,
        attempts,
    }
}

/// One accepted submission, or a panic. A busy result is retried inside `call`,
/// so what reaches here is either the product's own refusal code or a failure
/// that is neither an acceptance nor a refusal; both fail the test, and only a
/// `Rejection` is ever called a refusal.
async fn accept(ledger: &BillingLedger, raw: &[u8]) -> Value {
    call(ledger, Call::Write(raw))
        .await
        .result
        .unwrap_or_else(|error| panic!("submission was not accepted: {error:?}"))
}

/// One racer's whole submission attempt, with the window its answering attempt
/// occupied inside the store and the busy retries it needed to get a definite
/// answer.
#[derive(Debug)]
struct Racer {
    result: Result<Value, LocalError>,
    busy_retries: usize,
    entered: Instant,
    left: Instant,
}

/// Submit every payload through the one store, released together from one
/// barrier. Because the store admits a caller into its transaction queue before
/// the barrier is released, every racer's answering attempt is already in flight
/// before the first racer can finish; `assert_genuinely_raced` checks that
/// instead of assuming it.
async fn race(ledger: &Arc<BillingLedger>, payloads: Vec<Vec<u8>>) -> Vec<Racer> {
    let start = Arc::new(Barrier::new(payloads.len()));
    let mut tasks = Vec::with_capacity(payloads.len());
    for raw in payloads {
        let ledger = Arc::clone(ledger);
        let start = Arc::clone(&start);
        tasks.push(tokio::spawn(async move {
            start.wait().await;
            let answered = call(&ledger, Call::Write(&raw)).await;
            let answering = answered.answering();
            Racer {
                result: answered.result,
                busy_retries: answered.busy_retries,
                entered: answering.opened,
                left: answering.left,
            }
        }));
    }
    let mut racers = Vec::with_capacity(tasks.len());
    for racer in tasks {
        racers.push(racer.await.expect("a racer task panicked"));
    }
    racers
}

/// The observable race witness: the last racer to enter the store was still
/// inside its answering attempt when the first racer returned. If this fails,
/// the racers were serialized and the test proved nothing about a race. The
/// instants compared are the answering attempts' own, never a wrapper that spans
/// a busy backoff.
fn assert_genuinely_raced(racers: &[Racer], label: &str) {
    let entered_last = racers.iter().map(|racer| racer.entered).max().unwrap();
    let left_first = racers.iter().map(|racer| racer.left).min().unwrap();
    assert!(
        entered_last < left_first,
        "{label}: the {} racers were serialized, so no race was proved \
         (last entry {entered_last:?} is not before first return {left_first:?})",
        racers.len()
    );
    eprintln!(
        "m3_history: {label} race of {} racers; every answering attempt was in flight from \
         {entered_last:?} to {left_first:?}, {} busy retries",
        racers.len(),
        racers.iter().map(|racer| racer.busy_retries).sum::<usize>()
    );
}

/// One complete statement the product returned to a public snapshot call,
/// reduced to what a later comparison can still check, plus that one call's own
/// instants.
#[derive(Debug)]
struct Shot {
    cutoff: usize,
    net_atoms: i128,
    snapshot_hash: String,
    receipt_ids: Vec<String>,
    busy_retries: usize,
    span: Span,
}

/// The finance CSV the product published for a pin, reduced to the counts a
/// later comparison can still check.
#[derive(Debug)]
struct Published {
    export_id: String,
    postings: usize,
    rows: usize,
    atoms: i128,
    busy_retries: usize,
}

/// What the product did with the pin a snapshot call had just returned.
enum Projection {
    /// The pin still described the retained history and was published.
    Published(Box<Published>),
    /// A write committed between the statement and the projection. The product
    /// refused the pin, published nothing, and the refusal is verified here.
    Moved,
}

/// One projection: what the product did with the pin, and the own instants of the
/// single public export call that returned that answer. A refused pin has instants
/// too, so a capture whose projection moved is still witnessed.
struct Projected {
    outcome: Projection,
    span: Span,
    busy_retries: usize,
}

/// One coordinated snapshot: the public statement call's own instants and the
/// result it returned, plus what the product did with that statement's pin and
/// the own instants of the public call that asked for it.
struct Capture {
    round: usize,
    shot: Shot,
    projection: Projection,
    projection_span: Span,
    projection_busy_retries: usize,
}

/// One unique write from the live stream, with every attempt its public `accept`
/// call made. There is no wrapper interval here: a span that covered the retries
/// too could be open while the writer was inside no attempt at all, so the
/// recorded attempts are the only instants a witness may be read from.
#[derive(Debug)]
struct Write {
    index: usize,
    status: String,
    busy_retries: usize,
    attempts: Vec<Attempt>,
}

/// The one named accept attempt that was inside the store when a captured
/// snapshot call was invoked, with its geometry as measured. Nothing here is
/// inferred from a retry loop: a backoff gap between two attempts is not an
/// attempt and cannot produce a `Witness`.
#[derive(Debug, Clone, Copy)]
struct Witness {
    /// The decision that attempt was submitting.
    decision: usize,
    /// Which attempt of that decision it was, counting from one.
    attempt: usize,
    /// How many attempts that decision's call made in total.
    attempts: usize,
    /// How long after that attempt was invoked the snapshot call was invoked.
    offset: Duration,
    /// How much of that attempt was still to run at that instant.
    remaining: Duration,
    /// Whether the attempt that was in flight is the one that answered.
    answered: bool,
    /// Whether the same attempt was still in flight when the snapshot call's own
    /// answering attempt started, or whether that call had to retry behind a
    /// different one. Reported, never assumed.
    held_to_the_answer: bool,
    /// Whether the witnessing attempt belongs to the write that announced this
    /// call's slot. Reported, never assumed.
    announced_here: bool,
}

/// Every recorded accept attempt of the live stream, as `(decision, attempt,
/// opened, left)`, for a failure message that has to name the whole record.
fn attempt_table(writes: &[Write]) -> Vec<(usize, usize, Instant, Instant)> {
    writes
        .iter()
        .flat_map(|write| {
            write
                .attempts
                .iter()
                .enumerate()
                .map(move |(index, attempt)| (write.index, index + 1, attempt.opened, attempt.left))
        })
        .collect()
}

/// The witness for one captured snapshot call: the one accept attempt of the live
/// stream that was inside the store at the instant that call was invoked.
///
/// This is checked, not assumed, and it is checked three ways:
///
/// - At least one attempt must have been in flight at that instant. A missing
///   witness is a failure naming every recorded attempt, never a total that
///   happens to be greater than zero somewhere in the run.
/// - At most one attempt of the whole stream may be in flight at that instant,
///   which is this test's own one-writer invariant. If two were, the writer's
///   recorded attempts no longer describe a single serialized writer and every
///   witness derived from them would be meaningless.
/// - The witnessing attempt is one attempt of a named decision, and the geometry
///   is measured: how far into that attempt the snapshot call was invoked, and
///   how much of the attempt was still to run.
fn witness(writes: &[Write], span: Span, what: &str, round: usize, expected: usize) -> Witness {
    let at = span.call.opened;
    let mut in_flight: Vec<(usize, usize)> = Vec::new();
    for (position, write) in writes.iter().enumerate() {
        for (index, attempt) in write.attempts.iter().enumerate() {
            if attempt.in_flight_at(at) {
                in_flight.push((position, index));
            }
        }
    }
    assert!(
        in_flight.len() <= 1,
        "capture {round}'s {what} call was invoked at {at:?} with {} accept attempts of the live \
         stream in flight at once, so the one-writer invariant this witness rests on is broken. \
         Attempts: {:?}",
        in_flight.len(),
        attempt_table(writes)
    );
    let (position, index) = in_flight.first().copied().unwrap_or_else(|| {
        panic!(
            "capture {round} has no overlap witness for its {what} call: at {at:?}, the instant it \
             entered the API, no attempt of the live stream was inside the store. A backoff gap \
             would look exactly like this, which is why only attempts are ever witnesses. \
             Attempts: {:?}",
            attempt_table(writes)
        )
    });
    let write = &writes[position];
    let attempt = write.attempts[index];
    Witness {
        decision: write.index,
        attempt: index + 1,
        attempts: write.attempts.len(),
        offset: at - attempt.opened,
        remaining: attempt.left - at,
        answered: index + 1 == write.attempts.len(),
        held_to_the_answer: attempt.in_flight_at(span.answering.opened),
        announced_here: position == expected,
    }
}

/// One captured call and the attempt that witnessed it, as measured: how the
/// call was invoked relative to that attempt, whether the attempt was the one
/// that answered, and how long the call itself took once the attempt is taken
/// out of the picture.
fn describe(witness: &Witness, span: Span, retries: usize) -> String {
    format!(
        "invoked {offset:?} into decision {decision}'s attempt {attempt} of {attempts} \
         ({remaining:?} of that attempt still to run, and it {answered}); the call itself took \
         {total:?} over {count} attempt(s){held}",
        offset = witness.offset,
        decision = witness.decision,
        attempt = witness.attempt,
        attempts = witness.attempts,
        remaining = witness.remaining,
        answered = if witness.answered {
            "was the one that answered the submission"
        } else {
            "returned a busy result and was retried"
        },
        total = span.call.left - span.call.opened,
        count = retries + 1,
        held = if retries == 0 {
            String::new()
        } else if witness.held_to_the_answer {
            String::from(
                ", and the same attempt was still in flight when its own answering attempt started",
            )
        } else {
            String::from(
                ", but that attempt had already returned by the time its own answering attempt \
                 started",
            )
        },
    )
}

/// Wait for the writer's announcement of one snapshot-call slot, and check that
/// it is the slot this capture is waiting for. An out-of-order or missing
/// announcement would mean the stream did not run straight through the captures,
/// so it is reported instead of waited on.
async fn announced(
    armed: &mut mpsc::UnboundedReceiver<usize>,
    slot: usize,
    what: &str,
    round: usize,
) -> Result<(), String> {
    let got = armed.recv().await.ok_or_else(|| {
        format!("the writer task stopped before it announced round {round}'s {what} call")
    })?;
    if got != slot {
        return Err(format!(
            "the writer task announced slot {got} where slot {slot} was due for round {round}'s \
             {what} call, so its stream did not run straight through the captures"
        ));
    }
    Ok(())
}

/// Project an already captured complete statement as a finance CSV, and verify
/// that the published projection covers the same cutoff and the same integer
/// total as the statement it was pinned to.
async fn pin_and_project(
    ledger: &BillingLedger,
    exports: &Path,
    serial: usize,
    statement: &Value,
) -> Result<Projected, String> {
    let cutoff = expect_self_consistent(statement);
    let net_atoms = integer(statement, "net_atoms");
    let hash = statement["snapshot_hash"]
        .as_str()
        .expect("statement snapshot_hash")
        .to_owned();
    let output = exports.join(format!("during-writes-{serial:04}-cutoff-{cutoff:06}.csv"));
    let answered = call(
        ledger,
        Call::Export {
            pin: &hash,
            output: &output,
        },
    )
    .await;
    let span = Span::of(&answered);
    let summary = match answered.result {
        Ok(summary) => summary,
        Err(error) => {
            assert_eq!(
                refusal(&error),
                "BILLING_EXPORT_SNAPSHOT",
                "an unexpected export refusal"
            );
            // A pin that moved must publish nothing at all.
            assert!(!output.exists(), "a refused export published a file");
            return Ok(Projected {
                outcome: Projection::Moved,
                span,
                busy_retries: answered.busy_retries,
            });
        }
    };
    assert_eq!(summary["complete"], true);
    assert_eq!(summary["snapshot_hash"], hash.as_str());
    assert_eq!(summary["cutoff"], cutoff.to_string());
    assert_eq!(summary["net_atoms"], net_atoms.to_string());
    assert_eq!(summary["delivered"], false);
    assert_eq!(summary["payment_collected"], false);
    let published = fs::read_to_string(&output).expect("published export");
    let (postings, rows, atoms) = csv_totals(&published);
    // The projection is complete and pinned to the same cutoff and total.
    assert_eq!(postings, integer(&summary, "posting_count") as usize);
    assert_eq!(atoms, net_atoms, "CSV total differs at cutoff {cutoff}");
    assert_eq!(rows, postings + 2, "CSV has a header and a trailer");
    assert_eq!(postings, cutoff, "one posting per retained decision");
    Ok(Projected {
        outcome: Projection::Published(Box::new(Published {
            export_id: summary["export_id"].as_str().expect("export id").to_owned(),
            postings,
            rows,
            atoms,
            busy_retries: answered.busy_retries,
        })),
        span,
        busy_retries: answered.busy_retries,
    })
}

/// Close the shared owner. Every task has been joined by then, so the reference
/// count is one and the installation is closed for real.
async fn close_shared(ledger: Arc<BillingLedger>) {
    match Arc::try_unwrap(ledger) {
        Ok(ledger) => ledger.close().await,
        Err(_) => panic!("every task has finished, so the owner must be unique"),
    }
}

/// The published CSV's own row count, posting count and integer total.
fn csv_totals(published: &str) -> (usize, usize, i128) {
    let mut lines = published.lines();
    let header = lines.next().expect("CSV header");
    assert!(header.starts_with("\"row_type\""), "{header}");
    let mut rows = 1usize;
    let mut postings = 0usize;
    let mut atoms = 0i128;
    for line in lines {
        rows += 1;
        let cells: Vec<&str> = line.split("\",\"").collect();
        if cells.first().copied().unwrap_or_default().trim_matches('"') != "posting" {
            continue;
        }
        postings += 1;
        atoms += cells[29]
            .trim_matches('"')
            .parse::<i128>()
            .expect("CSV amount_atoms");
    }
    (postings, rows, atoms)
}

/// Snapshots taken while a live stream of unique writes is running, with a
/// named per-attempt overlap witness for every public snapshot call, and every
/// snapshot reconciling to the cutoff it declares.
///
/// How the overlap is proved, without guessing from `tokio::join!` and without
/// the writer ever stopping:
///
/// - The snapshot task is spawned first and parks on a channel, so it is
///   registered before any write exists.
/// - Before its first six writes the writer announces each snapshot-call slot
///   and enters `accept` without yielding. A notification may queue while an
///   earlier snapshot call awaits storage, so it is readiness, not evidence of
///   overlap. The writer continues submitting unique writes until all captures
///   finish, without parking or waiting for them. A 64-write ceiling fails the
///   harness if they cannot finish. Every overlap is still measured below;
///   no notification or completion flag is credited as an attempt witness.
/// - The writer's busy retries are retried with no yield between attempts
///   (`call_in_line`), so the writer task has no scheduling point at which it is
///   inside no attempt. A span that covered the retries as well would be open
///   during a backoff gap, and a snapshot call could then start inside such a
///   gap with no attempt in flight; recording attempts one by one removes that
///   possibility, and this bullet is the reason the gap cannot be reached rather
///   than a claim that it was not.
/// - The writer never waits for a snapshot and never parks between writes: its
///   stream runs straight through every capture, the announced slot indices must
///   arrive exactly in order, and every one of its attempts is recorded with its
///   own start and completion instant. The test also checks that record against
///   itself: a decision's attempts are ordered and disjoint, there are exactly
///   as many of them as its busy retries plus the one that answered, and the
///   stream's attempts are ordered and non-overlapping across decisions, so no
///   two attempts of the whole stream can be in flight at the same instant.
///   Non-overlapping, not strictly increasing, is what is asserted: one attempt
///   may complete at the very instant the next is invoked, which is what a
///   coarse clock reports for two genuinely sequential calls, and in-flight is
///   half-open (`opened <= at < left`), so only one of such a pair is ever in
///   flight at their shared instant.
/// - The test then *verifies* the overlap per capture and per call from those
///   recorded attempts instead of assuming it: for each call it names the one
///   attempt that was inside the store at that call's invocation instant, checks
///   that no other attempt was in flight there, prints the measured geometry and
///   reports whether that attempt belongs to the write that announced the slot.
///   It also reconciles every snapshot: its own cutoff, entry count and total,
///   its exact receipt-identity prefix of the final history, and, where its pin
///   still described the history, the published CSV's own counts.
/// - What is not claimed: a snapshot is never read from inside an open write
///   transaction, and neither call is claimed to contain the other. The
///   statement, export and write paths share the store's single physical writer
///   connection, so the queue order decides which of two in-flight calls answers
///   first, and the measured geometry of each pair is printed rather than
///   predicted. A pin that a concurrent write has moved is refused with
///   `BILLING_EXPORT_SNAPSHOT` and publishes nothing. The last section races one
///   pin against two committed writers and verifies both outcomes, so that
///   section asserts nothing about timing.
#[tokio::test]
async fn live_write_stream_and_snapshot_calls_overlap_and_reconcile_to_their_cutoff() {
    let scratch = scratch("snapshots");
    let path = &scratch.path;
    let ledger = Arc::new(install(path).await);
    let exports = path.join("exports");
    let base = 3;
    fs::create_dir(&exports).unwrap();

    // A quiescent base, so no capture can observe an empty customer history.
    for index in 1..=base {
        assert_eq!(
            accept(&ledger, &work_event(index)).await["status"],
            "accepted"
        );
    }

    // The snapshot task exists and is parked before the first write does.
    let (armed, mut armed_rx) = mpsc::unbounded_channel::<usize>();
    let first = base + 1;
    let captures_finished = Arc::new(AtomicBool::new(false));
    let snapshots = {
        let captures_finished = Arc::clone(&captures_finished);
        let ledger = Arc::clone(&ledger);
        let exports = exports.clone();
        tokio::spawn(async move {
            let mut captures = Vec::with_capacity(SNAPSHOT_ROUNDS);
            // Take one capture per round: one public statement call, then one
            // public CSV projection call. Each of those calls waits for the
            // writer's announcement of its own slot first. The continuous
            // stream keeps running after queued announcements; the measured
            // attempt table, not this notification, proves each overlap.
            for round in 0..SNAPSHOT_ROUNDS {
                announced(&mut armed_rx, round * 2, "statement", round).await?;
                let answered = call(&ledger, Call::Read).await;
                let shot_span = Span::of(&answered);
                let statement = answered
                    .result
                    .map_err(|error| format!("statement: {error:?}"))?;
                let cutoff = expect_self_consistent(&statement);
                announced(&mut armed_rx, round * 2 + 1, "CSV projection", round).await?;
                let projected =
                    pin_and_project(&ledger, &exports, next_capture_serial(), &statement).await?;
                captures.push(Capture {
                    round,
                    shot: Shot {
                        cutoff,
                        net_atoms: integer(&statement, "net_atoms"),
                        snapshot_hash: statement["snapshot_hash"]
                            .as_str()
                            .expect("statement snapshot_hash")
                            .to_owned(),
                        receipt_ids: receipt_ids(&statement),
                        busy_retries: answered.busy_retries,
                        span: shot_span,
                    },
                    projection: projected.outcome,
                    projection_span: projected.span,
                    projection_busy_retries: projected.busy_retries,
                });
            }
            captures_finished.store(true, Ordering::SeqCst);
            Ok::<_, String>(captures)
        })
    };
    let writer = {
        let ledger = Arc::clone(&ledger);
        let captures_finished = Arc::clone(&captures_finished);
        tokio::spawn(async move {
            let mut writes = Vec::with_capacity(MAX_LIVE_WRITES);
            for offset in 0..MAX_LIVE_WRITES {
                // Never park or wait for the snapshot task. Keep real writes in
                // flight until the captures finish, including after the six
                // announcements have queued. On this current-thread runtime,
                // no await separates this check from entering the next attempt.
                if offset >= SNAPSHOT_CALLS && captures_finished.load(Ordering::SeqCst) {
                    break;
                }
                if offset < SNAPSHOT_CALLS {
                    armed
                        .send(offset)
                        .map_err(|_| String::from("the snapshot task stopped before it read"))?;
                }
                let index = first + offset;
                let answered = call_in_line(&ledger, Call::Write(&work_event(index))).await;
                let status = match answered.result {
                    Ok(value) => value["status"]
                        .as_str()
                        .expect("a submission status")
                        .to_owned(),
                    Err(error) => {
                        return Err(format!(
                        "decision {index} did not resolve to an accepted submission mid-stream: \
                             {error:?}"
                    ))
                    }
                };
                writes.push(Write {
                    index,
                    status,
                    busy_retries: answered.busy_retries,
                    attempts: answered.attempts,
                });
            }
            if !captures_finished.load(Ordering::SeqCst) {
                return Err(format!(
                    "snapshot captures did not finish within {MAX_LIVE_WRITES} live writes"
                ));
            }
            Ok::<_, String>(writes)
        })
    };
    let writes = writer
        .await
        .expect("the writer task panicked")
        .expect("the writer task reported");
    let captures = snapshots
        .await
        .expect("the snapshot task panicked")
        .expect("the snapshot task reported");

    // The stream is a run of unique, consecutive, accepted decisions that ran
    // through every capture without waiting for one.
    let live_writes = writes.len();
    assert!((SNAPSHOT_CALLS..=MAX_LIVE_WRITES).contains(&live_writes));
    // Base, measured live stream, two raced writes, then one trailing write.
    let final_expected = base + live_writes + 2 + 1;
    assert_eq!(captures.len(), SNAPSHOT_ROUNDS);
    for write in &writes {
        assert_eq!(
            write.status, "accepted",
            "decision {} in the live stream",
            write.index
        );
    }
    for pair in writes.windows(2) {
        assert_eq!(
            pair[1].index,
            pair[0].index + 1,
            "the stream must be unique and consecutive"
        );
    }
    let unique: BTreeSet<usize> = writes.iter().map(|write| write.index).collect();
    assert_eq!(
        unique.len(),
        live_writes,
        "every stream write must be its own decision"
    );

    // The recorded attempt record is checked against itself before anything is
    // read out of it. A decision's attempts must be ordered and disjoint, there
    // must be exactly as many of them as its busy retries plus the one that
    // answered, and the stream's attempts must be ordered and non-overlapping
    // across decisions. That last property is what lets the witness below be a
    // single named attempt: no two accept attempts of the whole stream can be in
    // flight at the same instant, and a span that also covered a backoff gap
    // could not make that claim. The bound is `left <= opened`, not a strict
    // increase, so an attempt that completed at the same instant the next was
    // invoked passes; asserting strictness there would fail a run whose attempts
    // are genuinely sequential whenever the clock cannot separate them.
    let mut attempts_recorded = 0usize;
    let mut previous: Option<(usize, Instant)> = None;
    for write in &writes {
        assert_eq!(
            write.attempts.len(),
            write.busy_retries + 1,
            "decision {} recorded {} attempts for {} busy retries",
            write.index,
            write.attempts.len(),
            write.busy_retries
        );
        for pair in write.attempts.windows(2) {
            assert!(
                pair[0].left <= pair[1].opened,
                "decision {} has two attempts in flight at once at {:?}..{:?}",
                write.index,
                pair[0].opened,
                pair[1].opened
            );
        }
        for attempt in &write.attempts {
            if let Some((index, left)) = previous {
                assert!(
                    left <= attempt.opened,
                    "decisions {index} and {} have attempts in flight at once: one left at {left:?} \
                     and the next was invoked at {:?}",
                    write.index,
                    attempt.opened
                );
            }
            previous = Some((write.index, attempt.left));
            attempts_recorded += 1;
        }
    }
    assert!(
        attempts_recorded >= live_writes,
        "every write of the stream recorded at least its answering attempt"
    );

    // The proof this test exists for, read back from the recorded attempts rather
    // than assumed, and checked for every capture and for every public snapshot
    // call separately. A total overlap count greater than zero would only show
    // that one lucky pair of calls collided somewhere in the run, so each
    // capture names its own witness or fails: the one accept attempt that was
    // inside the store when that capture's statement call was invoked, and the
    // one that was inside it when its projection call was invoked. A backoff gap
    // can produce neither, because a gap is not an attempt. Both witnesses are
    // overlapping public calls; neither is a read inside an open write
    // transaction, and neither call is claimed to contain the other.
    let mut witnesses: Vec<String> = Vec::with_capacity(captures.len());
    let mut engineered = 0usize;
    for capture in &captures {
        let round = capture.round;
        let statement_witness = witness(&writes, capture.shot.span, "statement", round, round * 2);
        let projection_witness = witness(
            &writes,
            capture.projection_span,
            "CSV projection",
            round,
            round * 2 + 1,
        );
        witnesses.push(format!(
            "cutoff {}: statement<-decision {} attempt {}/{} projection<-decision {} attempt {}/{}",
            capture.shot.cutoff,
            statement_witness.decision,
            statement_witness.attempt,
            statement_witness.attempts,
            projection_witness.decision,
            projection_witness.attempt,
            projection_witness.attempts,
        ));
        // The engineered witness is an attempt of the write that announced the
        // call's slot; a witness from another write is counted and reported
        // rather than accepted silently. The geometry is printed as measured.
        if statement_witness.announced_here && projection_witness.announced_here {
            engineered += 1;
        }
        eprintln!(
            "m3_history: capture {round} at cutoff {}: statement call {statement}; projection call \
             {projection}",
            capture.shot.cutoff,
            statement = describe(&statement_witness, capture.shot.span, capture.shot.busy_retries),
            projection = describe(
                &projection_witness,
                capture.projection_span,
                capture.projection_busy_retries
            ),
        );
    }

    // A pinned projection raced against two committed writers from one barrier.
    // Either outcome is verified, so the assertion never depends on timing.
    let pinned = ledger.statement(CUSTOMER, None).await.unwrap();
    let pinned_cutoff = expect_self_consistent(&pinned);
    assert_eq!(pinned_cutoff, base + live_writes);
    let pinned_hash = pinned["snapshot_hash"]
        .as_str()
        .expect("pinned snapshot_hash")
        .to_owned();
    let serial = next_capture_serial();
    let raced_output = exports.join(format!(
        "during-writes-{serial:04}-cutoff-{pinned_cutoff:06}.csv"
    ));
    let raced = (
        work_event(base + live_writes + 1),
        work_event(base + live_writes + 2),
    );
    let start = Arc::new(Barrier::new(4));
    let spawn_writer = |raw: Vec<u8>| {
        let ledger = Arc::clone(&ledger);
        let start = Arc::clone(&start);
        tokio::spawn(async move {
            start.wait().await;
            call(&ledger, Call::Write(&raw)).await
        })
    };
    let writer_a = spawn_writer(raced.0.clone());
    let writer_b = spawn_writer(raced.1.clone());
    let exporter = {
        let ledger = Arc::clone(&ledger);
        let start = Arc::clone(&start);
        let exports = exports.clone();
        let pinned = pinned.clone();
        tokio::spawn(async move {
            start.wait().await;
            pin_and_project(&ledger, &exports, serial, &pinned).await
        })
    };
    start.wait().await;
    let a = writer_a.await.expect("a raced writer task panicked");
    let b = writer_b.await.expect("a raced writer task panicked");
    let outcome = exporter
        .await
        .expect("the export task panicked")
        .expect("the export task reported")
        .outcome;
    for raced_writer in [&a, &b] {
        assert_eq!(
            raced_writer
                .result
                .as_ref()
                .expect("a raced writer's answer")["status"],
            "accepted"
        );
    }
    let mut pins: Vec<String> = captures
        .iter()
        .map(|capture| capture.shot.snapshot_hash.clone())
        .collect();
    let mut published_files = 0usize;
    let mut export_busy_retries = 0usize;
    match outcome {
        Projection::Published(published) => {
            assert_eq!(
                published.postings, pinned_cutoff,
                "a projection that was not refused must cover its pin"
            );
            assert_eq!(published.rows, published.postings + 2);
            assert_eq!(published.atoms, integer(&pinned, "net_atoms"));
            assert!(!published.export_id.is_empty());
            published_files += 1;
            export_busy_retries += published.busy_retries;
            pins.push(pinned_hash);
            eprintln!("m3_history: the raced pin still held at cutoff {pinned_cutoff}");
        }
        Projection::Moved => {
            assert!(
                !raced_output.exists(),
                "a refused raced pin published a file"
            );
            eprintln!(
                "m3_history: a raced pin was refused with BILLING_EXPORT_SNAPSHOT after cutoff \
                 {pinned_cutoff} moved under it"
            );
        }
    }

    // One more decision after every capture, so each declared cutoff is strictly
    // earlier than the final history.
    let trailing = accept(&ledger, &work_event(final_expected)).await;
    assert_eq!(trailing["status"], "accepted");

    let final_statement = ledger.statement(CUSTOMER, None).await.unwrap();
    let final_cutoff = expect_self_consistent(&final_statement);
    assert_eq!(final_cutoff, final_expected);
    assert_eq!(
        integer(&final_statement, "net_atoms"),
        PRICE_ATOMS * final_expected as i128
    );
    let final_ids = receipt_ids(&final_statement);
    let final_atoms: Vec<i128> = final_statement["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| integer(entry, "net_atoms"))
        .collect();
    for capture in &captures {
        let shot = &capture.shot;
        // Every capture reconciles to the cutoff it declared...
        assert_eq!(shot.receipt_ids.len(), shot.cutoff);
        assert!(
            shot.cutoff >= base && shot.cutoff <= base + live_writes,
            "cutoff {} is outside the range the stream had committed by the time every capture \
             had finished, {base}..={}",
            shot.cutoff,
            base + live_writes
        );
        // ... and that cutoff is still an exact, unchanged prefix whose total
        // grew by exactly the entries accepted after it.
        assert_eq!(
            final_ids[..shot.cutoff],
            shot.receipt_ids[..],
            "cutoff {} is not an exact prefix of the final history",
            shot.cutoff
        );
        assert_eq!(
            integer(&final_statement, "net_atoms") - shot.net_atoms,
            final_atoms[shot.cutoff..].iter().sum::<i128>(),
            "cutoff {} total did not extend by the later entries",
            shot.cutoff
        );
        match &capture.projection {
            Projection::Published(published) => {
                assert_eq!(
                    published.postings, shot.cutoff,
                    "one posting per retained decision"
                );
                assert_eq!(published.rows, published.postings + 2);
                assert_eq!(published.atoms, shot.net_atoms);
                assert!(!published.export_id.is_empty());
                published_files += 1;
                export_busy_retries += published.busy_retries;
            }
            Projection::Moved => eprintln!(
                "m3_history: the pin for cutoff {} was refused as moved under a live write",
                shot.cutoff
            ),
        }
    }
    for pair in captures.windows(2) {
        assert!(
            pair[1].shot.cutoff >= pair[0].shot.cutoff,
            "the serialized store reported cutoffs out of order: {} then {}",
            pair[0].shot.cutoff,
            pair[1].shot.cutoff
        );
    }

    // A snapshot taken now, on the quiescent final history, is the same shape as
    // the ones taken inside the live stream, and it publishes its own pin.
    let now = match pin_and_project(&ledger, &exports, next_capture_serial(), &final_statement)
        .await
        .expect("the final projection reported")
        .outcome
    {
        Projection::Published(published) => published,
        Projection::Moved => panic!("a quiet history cannot move a fresh pin"),
    };
    published_files += 1;
    export_busy_retries += now.busy_retries;
    assert_eq!(
        now.postings, final_cutoff,
        "one posting per retained decision"
    );
    assert_eq!(now.rows, now.postings + 2);
    assert_eq!(now.atoms, integer(&final_statement, "net_atoms"));
    assert!(!now.export_id.is_empty());

    // Every pin captured above is stale by construction: the stream ran on past
    // the earliest captures, and the raced and trailing writes all committed
    // after the last one. Each is refused, and none of them publishes.
    let mut refused_pins = 0usize;
    for hash in &pins {
        let serial = next_capture_serial();
        let output = exports.join(format!("stale-pin-{serial:04}.csv"));
        let error = call(
            &ledger,
            Call::Export {
                pin: hash,
                output: &output,
            },
        )
        .await
        .result
        .expect_err("a stale pin must be refused");
        assert_eq!(
            refusal(&error),
            "BILLING_EXPORT_SNAPSHOT",
            "a captured pin must be stale by the end of the run"
        );
        assert!(!output.exists(), "a refused stale pin published a file");
        refused_pins += 1;
    }
    assert_eq!(
        fs::read_dir(&exports).unwrap().count(),
        published_files,
        "each published snapshot must have its own file"
    );
    eprintln!(
        "m3_history: {captures} snapshots against a live stream of {writes} unique writes; every \
         capture names the one accept attempt that was in flight at each of its two public calls \
         ({witnesses}); and {engineered} of them had both witnesses on the write that announced \
         the slot; {attempts_recorded} accept attempts were recorded in total; cutoffs {cutoffs:?}; \
         {published_files} CSV projections published, {refused_pins} pins refused as stale, \
         {busy} busy writer results retried under the same identity",
        captures = captures.len(),
        writes = writes.len(),
        witnesses = witnesses.join("; "),
        published_files = published_files,
        refused_pins = refused_pins,
        cutoffs = captures
            .iter()
            .map(|capture| capture.shot.cutoff)
            .collect::<Vec<_>>(),
        busy = writes.iter().map(|write| write.busy_retries).sum::<usize>()
            + export_busy_retries
            + captures
                .iter()
                .map(|capture| capture.shot.busy_retries)
                .sum::<usize>()
    );
    close_shared(ledger).await;
}

/// M3 acceptance: a single host retains decisions past the pre-M3 ceiling, so
/// the history is continuous rather than refused at 1,000. This test provides
/// acceptance evidence for the measured source-built profile.
///
/// It requires repeated crossings, on more than one installation. Each of
/// `ACCEPTANCE_INSTALLATIONS` independent fresh installations must accept and
/// retain every decision from 1 through `ACCEPTANCE_CROSSING + ACCEPTANCE_TAIL`,
/// hold a history that is exactly that sequence in the submitted order, and —
/// after its own reopen — still resolve the oldest identity and the first
/// decision past the old ceiling to their own receipts. One installation would
/// show a first crossing; the repetition across unrelated installations is the
/// acceptance condition.
///
/// It is `#[ignore]`d because it is a measured run rather than a fast unit test.
/// Run it explicitly by its exact name:
///
/// ```text
/// cargo test --release -p ledgerlab --test m3_history -- --ignored --exact \
///   m3_acceptance_retains_decisions_1001_and_later_on_three_fresh_installations --nocapture
/// ```
#[tokio::test]
#[ignore = "M3 measured acceptance gate; invoke explicitly with the documented command"]
async fn m3_acceptance_retains_decisions_1001_and_later_on_three_fresh_installations() {
    let last = ACCEPTANCE_CROSSING + ACCEPTANCE_TAIL;
    assert!(
        last > PRE_M3_DECISION_CEILING,
        "the run must cross the ceiling"
    );
    for installation in 1..=ACCEPTANCE_INSTALLATIONS {
        eprintln!(
            "m3_history: M3 acceptance, fresh installation {installation} of \
             {ACCEPTANCE_INSTALLATIONS}, decisions 1..={last}"
        );
        accept_beyond_the_ceiling(installation, last).await;
    }
}

/// One fresh installation's whole crossing: submit every decision, verify the
/// exact retained history and its order, close, reopen, and verify that the
/// oldest identity and the first decision past the old ceiling still resolve to
/// their own receipts and change nothing.
async fn accept_beyond_the_ceiling(installation: usize, last: usize) {
    let scratch = scratch(&format!("m3-acceptance-{installation}"));
    let path = &scratch.path;
    let ledger = install(path).await;
    let mut accepted_ids = Vec::with_capacity(last);
    for index in 1..=last {
        let value = match call(&ledger, Call::Write(&work_event(index))).await.result {
            Ok(value) => value,
            Err(error) => panic!(
                "M3 acceptance requires decision {index} to be accepted and retained, got \
                 {error:?}; the measured source-built profile did not meet the acceptance gate."
            ),
        };
        assert_eq!(
            value["status"], "accepted",
            "decision {index} was {}",
            value["status"]
        );
        accepted_ids.push(
            value["receipt"]["id"]
                .as_str()
                .expect("accepted receipt id")
                .to_owned(),
        );
    }
    let crossing_receipt = accepted_ids[ACCEPTANCE_CROSSING - 1].clone();
    let oldest_receipt = accepted_ids[0].clone();

    let at_end = ledger.statement(CUSTOMER, None).await.unwrap();
    // Every decision past the old ceiling is retained, in order, and nothing
    // else is: the history is a continuous sequence, not a truncated prefix.
    assert_eq!(expect_self_consistent(&at_end), last);
    assert_eq!(at_end["cutoff"], last.to_string());
    assert_eq!(integer(&at_end, "net_atoms"), PRICE_ATOMS * last as i128);
    assert_eq!(receipt_ids(&at_end), accepted_ids);
    ledger.close().await;

    // Restart-safe: the oldest identity and the first crossing both resolve to
    // their own receipts, a conflicting reuse of the crossing identity refuses,
    // and none of it changes the retained history.
    let ledger = BillingLedger::open(path).await.unwrap();
    let oldest = accept(&ledger, &work_event(1)).await;
    assert_eq!(oldest["status"], "duplicate");
    assert_eq!(oldest["kind"], "identity");
    assert_eq!(oldest["receipt"]["id"].as_str().unwrap(), oldest_receipt);
    let crossing = accept(&ledger, &work_event(ACCEPTANCE_CROSSING)).await;
    assert_eq!(crossing["status"], "duplicate");
    assert_eq!(crossing["kind"], "identity");
    assert_eq!(
        crossing["receipt"]["id"].as_str().unwrap(),
        crossing_receipt,
        "the first decision past the old ceiling must keep its own receipt"
    );
    let last_decision = accept(&ledger, &work_event(last)).await;
    assert_eq!(last_decision["status"], "duplicate");
    assert_eq!(
        last_decision["receipt"]["id"].as_str().unwrap(),
        accepted_ids[last - 1]
    );
    let conflict = call(
        &ledger,
        Call::Write(&work_event_with(
            &format!("m3-work-{ACCEPTANCE_CROSSING:06}"),
            "m3-op-conflicting-reuse",
        )),
    )
    .await
    .result
    .expect_err("a conflicting reuse must refuse");
    assert_eq!(refusal(&conflict), "IDENTITY_CONFLICT");
    assert_eq!(ledger.statement(CUSTOMER, None).await.unwrap(), at_end);
    ledger.close().await;
    eprintln!(
        "m3_history: installation {installation} retained {last} decisions, {ACCEPTANCE_TAIL} \
         past the pre-M3 ceiling, and resolved the oldest and crossing identities after reopen"
    );
}

/// The oldest retained delivery identity resolves to its original receipt after
/// a reopen, with no second economic effect and no change to any statement.
#[tokio::test]
async fn oldest_identity_retry_after_reopen_returns_the_same_receipt() {
    let scratch = scratch("oldest-retry");
    let path = &scratch.path;
    let decisions = 12;
    let ledger = install(path).await;
    let mut receipts = Vec::new();
    for index in 1..=decisions {
        let value = accept(&ledger, &work_event(index)).await;
        assert_eq!(value["status"], "accepted");
        receipts.push(value["receipt"].clone());
    }
    // One accepted adjustment, so the reopen path covers a second record family.
    let outcome = ledger
        .outcome(
            CUSTOMER,
            SOURCE,
            &outcome_event("m3-oldest-outcome-1", &target_of(&receipts[2]), "rebate"),
        )
        .await
        .unwrap();
    assert_eq!(outcome["status"], "accepted");
    let before = ledger.statement(CUSTOMER, None).await.unwrap();
    expect_self_consistent(&before);
    assert_eq!(entry_count(&before), decisions + 1);
    assert_eq!(
        before["net_atoms"],
        (PRICE_ATOMS * decisions as i128 - 50).to_string()
    );
    let settled_receipts = receipt_ids(&before);
    ledger.close().await;

    // Reopen twice. Delivery identity is restart-safe, not process-lifetime.
    for round in 0..2 {
        let ledger = BillingLedger::open(path).await.unwrap();
        for index in 1..=decisions {
            let value = accept(&ledger, &work_event(index)).await;
            assert_eq!(value["status"], "duplicate", "decision {index}");
            assert_eq!(value["kind"], "identity");
            assert_eq!(
                value["receipt"],
                receipts[index - 1],
                "decision {index} returned a different receipt in round {round}"
            );
        }
        // A different delivery identity for the oldest operation resolves
        // semantically to the same receipt. The first round records the alias;
        // the second round replays the very same bytes, which are now a
        // retained delivery identity in their own right.
        let alias = accept(&ledger, &work_event_with("m3-oldest-alias", "m3-op-000001")).await;
        assert_eq!(alias["status"], "duplicate");
        assert_eq!(
            alias["kind"],
            if round == 0 { "semantic" } else { "identity" }
        );
        assert_eq!(alias["receipt"], receipts[0]);
        // The accepted adjustment keeps its own identity.
        let repeated = ledger
            .outcome(
                CUSTOMER,
                SOURCE,
                &outcome_event("m3-oldest-outcome-1", &target_of(&receipts[2]), "rebate"),
            )
            .await
            .unwrap();
        assert_eq!(repeated["status"], "duplicate");
        assert_eq!(repeated["receipt"], outcome["receipt"]);
        // Conflicting reuse of a retained delivery identity refuses.
        let conflict = ledger
            .accept(
                CUSTOMER,
                SOURCE,
                &work_event_with("m3-work-000001", "m3-op-conflict"),
            )
            .await
            .unwrap_err();
        assert_eq!(refusal(&conflict), "IDENTITY_CONFLICT");
        // Nothing moved. The first submission is still the oldest entry with
        // the same ordinal, target, amount, cutoff, total and digest.
        let after = ledger.statement(CUSTOMER, None).await.unwrap();
        assert_eq!(after["cutoff"], before["cutoff"]);
        assert_eq!(after["net_atoms"], before["net_atoms"]);
        assert_eq!(after["snapshot_hash"], before["snapshot_hash"]);
        assert_eq!(after["entries"], before["entries"]);
        assert_eq!(receipt_ids(&after), settled_receipts);
        ledger.close().await;
    }
}

/// Genuinely concurrent submissions through the one owner process: identical
/// bytes raced from one barrier commit exactly one effect and share one
/// receipt, and distinct payloads racing for one delivery identity commit
/// exactly one and every other racer gets one explicit conflict refusal. A
/// refusal here means exactly one thing — a `Rejection` carrying the product's
/// own code — because an unavailable store or an unknown commit outcome is a
/// failure of the run, not the product turning work down.
#[tokio::test]
async fn barrier_raced_submissions_commit_one_effect_and_conflicts_refuse() {
    let scratch = scratch("concurrency");
    let path = &scratch.path;
    let ledger = Arc::new(install(path).await);
    // A small retained history keeps the interleaving window short.
    for index in 1..=2 {
        assert_eq!(
            accept(&ledger, &work_event(index)).await["status"],
            "accepted"
        );
    }
    let settled = ledger.statement(CUSTOMER, None).await.unwrap();
    expect_self_consistent(&settled);

    // Identical bytes, four callers, one serialized writer.
    let shared = work_event_with("m3-race-delivery", "m3-race-operation");
    let racers = race(&ledger, vec![shared.clone(); RACERS]).await;
    assert_genuinely_raced(&racers, "identical");
    let mut winners: Vec<&Value> = Vec::new();
    let mut duplicates: Vec<&Value> = Vec::new();
    for (index, racer) in racers.iter().enumerate() {
        match &racer.result {
            Ok(value) if value["status"] == "accepted" => winners.push(value),
            Ok(value) if value["status"] == "duplicate" => duplicates.push(value),
            Ok(value) => panic!("racer {index} returned an unexpected result {value}"),
            // Identical bytes for one delivery identity cannot be refused by
            // the product: a busy result was retried inside `call`, and
            // anything else here is a failure of the store, not a decision.
            Err(error) => panic!(
                "racer {index} did not resolve its identical submission: {error:?}. A refusal is \
                 one ServiceError::Rejection, and this case admits none."
            ),
        }
    }
    assert_eq!(winners.len(), 1, "exactly one effect: {racers:?}");
    let race_receipt = winners[0]["receipt"].clone();
    assert_eq!(
        duplicates.len(),
        RACERS - 1,
        "every other racer must resolve to the original receipt"
    );
    for duplicate in &duplicates {
        assert_eq!(duplicate["kind"], "identity");
        assert_eq!(duplicate["receipt"], winners[0]["receipt"]);
    }
    let after_race = ledger.statement(CUSTOMER, None).await.unwrap();
    assert_eq!(
        expect_self_consistent(&after_race),
        entry_count(&settled) + 1
    );
    assert_eq!(
        integer(&after_race, "net_atoms"),
        integer(&settled, "net_atoms") + PRICE_ATOMS,
        "one effect, one charge"
    );

    // Distinct payloads racing for one delivery identity. The first racer
    // through commits; every other racer gets one explicit conflict refusal,
    // which `refusal` pins to the product's own `IDENTITY_CONFLICT` code and so
    // cannot be satisfied by a busy, unavailable or unknown-commit result.
    let payloads: Vec<Vec<u8>> = (0..RACERS)
        .map(|index| work_event_with("m3-conflict-delivery", &format!("m3-conflict-{index}")))
        .collect();
    let racers = race(&ledger, payloads).await;
    assert_genuinely_raced(&racers, "conflicting");
    let mut committed = 0usize;
    let mut conflicts = 0usize;
    let mut winner: Option<Value> = None;
    for (index, racer) in racers.iter().enumerate() {
        match &racer.result {
            Ok(value) => {
                assert_eq!(value["status"], "accepted", "racer {index}");
                committed += 1;
                winner = Some(value["receipt"].clone());
            }
            Err(error) => {
                assert_eq!(
                    refusal(error),
                    "IDENTITY_CONFLICT",
                    "racer {index} must get one explicit conflict refusal, not a busy, unavailable \
                     or unknown-commit result"
                );
                conflicts += 1;
            }
        }
    }
    assert_eq!(committed, 1, "one winner for one identity");
    assert_eq!(conflicts, RACERS - 1, "one refusal per losing racer");
    let final_statement = ledger.statement(CUSTOMER, None).await.unwrap();
    assert_eq!(
        expect_self_consistent(&final_statement),
        entry_count(&after_race) + 1,
        "only the committed effect is retained"
    );
    assert_eq!(
        integer(&final_statement, "net_atoms"),
        integer(&after_race, "net_atoms") + PRICE_ATOMS
    );
    // The oldest retained identity is still exact after the contention.
    let oldest = accept(&ledger, &work_event(1)).await;
    assert_eq!(oldest["status"], "duplicate");
    assert_eq!(oldest["receipt"], settled["entries"][0]["receipt"]);
    close_shared(ledger).await;

    // Reopening after contended writes reconciles to one effect per identity.
    let ledger = BillingLedger::open(path).await.unwrap();
    assert_eq!(
        ledger.statement(CUSTOMER, None).await.unwrap(),
        final_statement
    );
    let again = accept(&ledger, &shared).await;
    assert_eq!(again["status"], "duplicate");
    assert_eq!(again["kind"], "identity");
    assert_eq!(again["receipt"], race_receipt);
    // The single conflict-race winner is the one retained effect for that
    // delivery identity, and every other payload for it still refuses.
    let winner = winner.expect("one racer committed the conflicting identity");
    assert!(receipt_ids(&final_statement).contains(&winner["id"].as_str().unwrap().to_owned()));
    let refused = ledger
        .accept(
            CUSTOMER,
            SOURCE,
            &work_event_with("m3-conflict-delivery", "m3-conflict-after-reopen"),
        )
        .await
        .unwrap_err();
    assert_eq!(refusal(&refused), "IDENTITY_CONFLICT");
    ledger.close().await;
}

/// The overlap witness itself, checked against the exact shape a review of this
/// file rejected: a write whose public call had to retry, so that the span
/// between its two attempts is a backoff gap with no attempt in it.
///
/// These are the only tests here that need no installation, and they exist
/// because the defect they cover cannot be produced on demand from the live
/// stream — a busy writer is a rare event there — while the check that catches
/// it must not be the only evidence that it is caught. The live stream
/// verifies the same predicate per capture; these three fix what it must do
/// when a write does retry.
fn retrying_write(index: usize, base: Instant) -> Write {
    Write {
        index,
        status: "accepted".to_owned(),
        busy_retries: 1,
        attempts: vec![
            Attempt {
                opened: base,
                left: base + Duration::from_nanos(10),
            },
            Attempt {
                opened: base + Duration::from_nanos(20),
                left: base + Duration::from_nanos(30),
            },
        ],
    }
}

/// One call that was invoked `at` and answered there, with no retry of its own.
fn call_at(at: Instant) -> Span {
    Span {
        call: Window {
            opened: at,
            left: at + Duration::from_nanos(5),
        },
        answering: Attempt {
            opened: at,
            left: at + Duration::from_nanos(5),
        },
    }
}

#[test]
fn a_retry_backoff_gap_is_neither_an_attempt_nor_a_witness() {
    let base = Instant::now();
    let write = retrying_write(7, base);
    // Fifteen nanoseconds in: after the first attempt answered with a busy
    // result and before the retry was invoked. The writer was inside no attempt
    // at that instant.
    let in_the_gap = base + Duration::from_nanos(15);
    assert!(
        !write
            .attempts
            .iter()
            .any(|attempt| attempt.in_flight_at(in_the_gap)),
        "no attempt of a retrying write is in flight inside its own backoff gap"
    );
    // The interval the first version of this evidence used — the whole call,
    // retries included — is open across the gap, which is exactly why it could
    // credit a snapshot call that started while nothing was in the store.
    let wrapper_opened = write.attempts.first().unwrap().opened;
    let wrapper_left = write.attempts.last().unwrap().left;
    assert!(
        wrapper_opened <= in_the_gap && in_the_gap < wrapper_left,
        "a span covering the retries as well is open across the gap, so it cannot be a witness"
    );
}

#[test]
#[should_panic(expected = "no attempt of the live stream was inside the store")]
fn a_call_invoked_in_a_retry_backoff_gap_has_no_witness() {
    let base = Instant::now();
    let writes = vec![retrying_write(7, base)];
    let in_the_gap = base + Duration::from_nanos(15);
    witness(&writes, call_at(in_the_gap), "statement", 0, 0);
}

#[test]
fn the_witness_is_the_one_attempt_itself_even_when_its_call_retried() {
    let base = Instant::now();
    let writes = vec![retrying_write(7, base)];
    // Inside the first attempt, which then answered with a busy result: the
    // witness is that attempt, and it is reported as not being the one that
    // answered the submission.
    let first = witness(
        &writes,
        call_at(base + Duration::from_nanos(4)),
        "statement",
        0,
        0,
    );
    assert_eq!(first.decision, 7);
    assert_eq!(first.attempt, 1);
    assert_eq!(first.attempts, 2);
    assert!(
        !first.answered,
        "the first attempt answered with a busy result"
    );
    assert_eq!(first.offset, Duration::from_nanos(4));
    assert_eq!(first.remaining, Duration::from_nanos(6));
    // Inside the retry, which is the attempt that answered.
    let second = witness(
        &writes,
        call_at(base + Duration::from_nanos(25)),
        "CSV projection",
        0,
        0,
    );
    assert_eq!(second.attempt, 2);
    assert!(
        second.answered,
        "the second attempt answered the submission"
    );
    assert_eq!(second.offset, Duration::from_nanos(5));
    assert_eq!(second.remaining, Duration::from_nanos(5));
    // A witness from a write that did not announce the call's slot is reported
    // rather than treated as the engineered one.
    assert!(first.announced_here && second.announced_here);
    assert!(
        !witness(
            &writes,
            call_at(base + Duration::from_nanos(25)),
            "CSV projection",
            0,
            1
        )
        .announced_here,
        "the same attempt is not the announced one for a different slot"
    );
}
