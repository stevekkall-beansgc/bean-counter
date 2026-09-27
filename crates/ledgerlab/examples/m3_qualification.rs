//! M3 public-API qualification harness for the ordinary local SQLite profile.
//!
//! This harness submits a single-host continuous-history workload that mixes
//! billable work with outcomes, corrections and delivery retries, then reports
//! what actually happened. It is author evidence, not product conformance and
//! not a capacity guarantee.
//!
//! Scope and honesty limits, all deliberate:
//!
//! - It exercises only the public `ledgerlab::billing::BillingLedger` facade of
//!   one local installation on one host. It adds no storage behaviour, no limit
//!   change and no new dependency.
//! - It does not model, predict or depend on any admission ceiling, and it does
//!   not carry any expected refusal. It submits the work it was asked to submit
//!   and reports what the product did: what it accepted, what it refused and
//!   with which code, and how far it got. A refusal is a reported result, never
//!   an expected one, and a refused request is never reported as complete.
//! - A retryable or unknown result is never treated as an answer: the run aborts
//!   and names it instead of continuing with an unverified count.
//! - A measured elapsed time and a measured count describe this host and this
//!   run. They are not a throughput promise, a resource reservation, a
//!   saturation result or a completion guarantee for accepted offline work, and
//!   no number printed here is extrapolated to any other workload size.
//! - Snapshots are taken only at the small fixed `CHECKPOINTS` set, plus one
//!   final complete snapshot. One statement and one CSV are read at a time and
//!   released again.
//! - Each checkpoint is reconciled across three different products of the run,
//!   never against itself: the ordered receipt identities and the running total
//!   that the accepted public operations returned at submission time, the
//!   capture's own statement, and a streaming pass over the final ordered
//!   statement. A checkpoint keeps one SHA-256 digest and one integer, never the
//!   identities themselves, so a run of 100,000 base submissions reconciles in
//!   bounded memory. The report states which checkpoints were reached and which
//!   were not.
//! - Postponed capabilities (PostgreSQL, multi-host, resource proofs) are not
//!   exercised and are not certified by any number printed here.
//!
//! Usage:
//!
//! ```text
//! cargo run -p ledgerlab --release --example m3_qualification -- --mode smoke
//! cargo run -p ledgerlab --release --example m3_qualification -- --mode large
//! ```
//!
//! `smoke` is the practical default: a small deterministic workload that
//! exercises every phase and reconciliation check. Its default size reaches the
//! first checkpoint only; pass `--decisions 150` to reach the second. `large`
//! requests exactly 100,000 base submissions, which reaches every checkpoint.
//! The requested count is base submissions and nothing else: the outcomes,
//! corrections and delivery retries this schedule also submits are separate
//! counters, and the decisions the store retains are a third. Neither mode is
//! fast: every submission re-verifies the whole retained history, so the
//! measured cost of a run grows with the size of the history it builds. A mode
//! reports whatever actually happened, including a refusal part-way through.

use ledgerlab::{billing::BillingLedger, local::LocalError, ServiceError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command as Process,
    time::{Duration, Instant},
};

const CUSTOMER: &str = "customer-1";
const SOURCE: &str = "urn:example:work";
/// The work occurrence must be at or before the frozen ordinary window start,
/// which the shared example setup fixes at the same instant.
const WORK_OCCURRED_AT: &str = "2026-09-01T00:00:00.000000Z";
const ADJUSTMENT_OCCURRED_AT: &str = "2026-09-22T00:00:00.000000Z";
const SETUP: &[u8] = include_bytes!("../../../examples/billing/setup.json");
const MAPPING: &[u8] = include_bytes!("../../../examples/finance/mapping.json");

/// Independent arithmetic model of the agreed price, used to reconcile the
/// authoritative statement instead of copying the statement's own total.
const PRICE_ATOMS: i128 = 250;
const REBATE_ATOMS: i128 = -50;
/// The fixed set of checkpoints a run captures a complete statement and a finance
/// CSV projection at, counted in **retained economic entries** — the unit the
/// statement itself counts, and therefore the real work accounting. Under this
/// schedule one base submission is retained along with an outcome every third
/// decision and a correction every fifteenth, so roughly 71,000 accepted base
/// submissions produce 100,000 retained entries.
///
/// The set is small and fixed on purpose. An earlier version captured every 20
/// retained entries, which at the large mode's size is thousands of complete
/// statements and thousands of full CSVs, and held a copy of every prefix in
/// memory at once. Here each capture keeps only bounded metadata, the run keeps
/// only the final statement's own reconciliation data, and the exact number of
/// checkpoints a run reached is reported.
const CHECKPOINTS: &[usize] = &[10, 100, 1_000, 10_000, 50_000, 100_000];
/// How many further submissions to try after the first refusal, to show whether
/// the refusal is stable or a one-off. Reported, never assumed.
const REFUSAL_PROBE: usize = 5;
/// A changed-worktree listing is reported as a count plus this many paths.
const MAX_LISTED_PATHS: usize = 20;

const USAGE: &str = "\
m3_qualification - M3 continuous-history workload and report

  --mode smoke|large   smoke (default, small deterministic run) or large
                       (an explicit request for 100,000 base submissions)
  --decisions N        override the base-submission count for the chosen mode
  --keep DIR           keep the installation and its snapshots in DIR
  --help               print this text

The report states the source commit and whether the worktree was dirty, the
host and Rust version, the exact work submitted, what the product accepted or
refused, elapsed time, the ledger-artifact bytes left on disk and a sampled
RSS. It captures a statement and a finance CSV only at the fixed checkpoint
set, and reconciles each one against the ordered receipt identities its own
accepted operations returned and against the final history. Refusals and
unfinished workloads are reported as they happened; no ceiling is assumed and no
capacity is claimed.

The requested count is base submissions. Outcomes, corrections and delivery
retries are additional submissions the schedule also makes, they are counted
separately, and the decisions the store retains are counted separately again.";

fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("m3_qualification: {message}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if options.help {
        println!("{USAGE}");
        return;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("m3_qualification: cannot start the async runtime: {error}");
            std::process::exit(2);
        }
    };
    match runtime.block_on(run(options)) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("report JSON")
        ),
        Err(error) => {
            eprintln!("m3_qualification: {error}");
            std::process::exit(1);
        }
    }
}

/// The requested workload. `decisions` is a count of base submissions and
/// nothing else: the outcomes, corrections and delivery retries the schedule
/// also submits are counted separately, and the decisions the store retains are
/// a third number again.
struct Options {
    mode: &'static str,
    decisions: usize,
    keep: Option<PathBuf>,
    help: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut mode: Option<&'static str> = None;
        let mut decisions: Option<usize> = None;
        let mut keep: Option<PathBuf> = None;
        let mut help = false;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => help = true,
                "--mode" => {
                    mode = Some(match args.next().as_deref() {
                        Some("smoke") => "smoke",
                        Some("large") => "large",
                        other => {
                            return Err(format!(
                                "--mode needs smoke or large, got {:?}",
                                other.unwrap_or("<nothing>")
                            ))
                        }
                    });
                }
                "--decisions" => {
                    let raw = args
                        .next()
                        .ok_or("--decisions needs a base-submission count")?;
                    let value: usize = raw.parse().map_err(|_| {
                        format!("--decisions needs a base-submission count, got {raw:?}")
                    })?;
                    if value == 0 {
                        return Err("--decisions must be at least 1 base submission".into());
                    }
                    decisions = Some(value);
                }
                "--keep" => {
                    let raw = args.next().ok_or("--keep needs a directory")?;
                    keep = Some(PathBuf::from(raw));
                }
                other => return Err(format!("unrecognized argument {other:?}")),
            }
        }
        let mode = mode.unwrap_or("smoke");
        Ok(Self {
            mode,
            decisions: decisions.unwrap_or(if mode == "large" { 100_000 } else { 60 }),
            keep,
            help,
        })
    }
}

/// What the harness actually submitted, counted by kind. A refused submission is
/// not counted here: refusals are counted separately, because a refusal is not
/// work the product accepted.
#[derive(Default)]
struct Counters {
    base: usize,
    outcomes: usize,
    corrections: usize,
    duplicate_identity: usize,
    duplicate_semantic: usize,
    /// The rebate outcomes the product accepted, and the accepted corrections
    /// that reversed such an outcome back to the zero-amount code. These are
    /// counts, not lists of decision indices: the independent total is computed
    /// from them and from the base count, and a run of 100,000 base submissions
    /// must not keep a collection per submission to reconcile its own arithmetic.
    rebate_outcomes: usize,
    corrected_rebates: usize,
    refusals: BTreeMap<String, usize>,
    /// The first refusal, with the decision it stopped the workload at.
    stop: Option<Stop>,
}

struct Stop {
    index: usize,
    kind: &'static str,
    code: String,
    /// What the product did with a few further submissions after the stop:
    /// measured, never assumed.
    probe: Vec<(usize, String)>,
}

impl Counters {
    /// Every submission, accepted or refused, is total submitted work.
    fn submitted(&self) -> usize {
        self.base
            + self.outcomes
            + self.corrections
            + self.duplicate_identity
            + self.duplicate_semantic
            + self.refusals.values().sum::<usize>()
    }
    fn retained(&self) -> usize {
        self.base + self.outcomes + self.corrections
    }
    fn retries(&self) -> usize {
        self.duplicate_identity + self.duplicate_semantic
    }
    fn refused(&self) -> usize {
        self.refusals.values().sum()
    }
    fn refuse(&mut self, index: usize, kind: &'static str, code: &str) {
        *self.refusals.entry(code.to_owned()).or_default() += 1;
        if self.stop.is_none() {
            self.stop = Some(Stop {
                index,
                kind,
                code: code.to_owned(),
                probe: Vec::new(),
            });
        }
    }
    /// A retry must resolve to the original result and the original receipt.
    /// Anything else - including a fresh acceptance - is a hard error, never a
    /// count, because a retry that books a second effect is a billing defect.
    fn duplicate(
        &mut self,
        result: Result<Value, LocalError>,
        expected: &Value,
    ) -> Result<(), String> {
        let value = result.map_err(describe)?;
        if value["status"] != "duplicate" {
            return Err(format!(
                "a retry was not resolved as a duplicate: status {:?}",
                value["status"]
            ));
        }
        if value["receipt"] != *expected {
            return Err("a retry did not return the original receipt".into());
        }
        match value["kind"].as_str() {
            Some("identity") => self.duplicate_identity += 1,
            Some("semantic") => self.duplicate_semantic += 1,
            other => return Err(format!("unexpected duplicate kind {other:?}")),
        }
        Ok(())
    }
    /// The oldest retained delivery identity after a reopen. This is the
    /// caller's only route after an unknown commit result, so it must resolve
    /// by identity, return the retained receipt, and change nothing.
    fn reopened_oldest(
        &mut self,
        result: Result<Value, LocalError>,
        expected: &Value,
        before: &Value,
        after: &Value,
    ) -> Result<(), String> {
        self.duplicate(result, expected)?;
        if after["cutoff"] != before["cutoff"]
            || after["net_atoms"] != before["net_atoms"]
            || after["snapshot_hash"] != before["snapshot_hash"]
        {
            return Err("a retry after reopen changed the retained history".into());
        }
        Ok(())
    }
}

fn code(error: &LocalError) -> Option<&str> {
    match error {
        LocalError::Service(ServiceError::Rejection(code)) => Some(code),
        _ => None,
    }
}

fn describe(error: LocalError) -> String {
    match error {
        LocalError::Service(ServiceError::Retryable) => {
            "writer busy: an uncertain, retryable result, not an answer".into()
        }
        LocalError::Service(ServiceError::Unavailable) => "storage unavailable".into(),
        LocalError::Service(ServiceError::IntegrityFailure) => {
            "retained history failed verification".into()
        }
        LocalError::Service(ServiceError::OutcomeUnknown { .. }) => {
            "commit outcome unknown: reopen and resolve the original identity".into()
        }
        LocalError::Service(ServiceError::Rejection(code)) => format!("refused: {code}"),
        LocalError::Service(ServiceError::ReadBudgetExhausted) => {
            "read budget exhausted; not an answer".into()
        }
        LocalError::Config(message) => format!("configuration: {message}"),
        LocalError::Diagnostic(message) => format!("local: {message}"),
        LocalError::Io(error) => format!("io: {error}"),
    }
}

fn work_event(index: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-event/1",
        "id": format!("m3-work-{index:06}"),
        "operation_id": format!("m3-op-{index:06}"),
        "type": "content.generated",
        "customer": CUSTOMER,
        "occurred_at": WORK_OCCURRED_AT
    }))
    .expect("work event JSON")
}

/// A different delivery identity for an already accepted operation, used to
/// exercise the permanent semantic alias path.
fn alias_event(index: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-event/1",
        "id": format!("m3-alias-{index:06}"),
        "operation_id": format!("m3-op-{index:06}"),
        "type": "content.generated",
        "customer": CUSTOMER,
        "occurred_at": WORK_OCCURRED_AT
    }))
    .expect("alias event JSON")
}

fn outcome_event(index: usize, target: &str, code: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-billing-outcome/2",
        "customer": CUSTOMER,
        "source": SOURCE,
        "id": format!("m3-outcome-{index:06}"),
        "target": target,
        "family": "quality",
        "occurred_at": ADJUSTMENT_OCCURRED_AT,
        "evidence": "Synthetic M3 qualification outcome recorded by the harness",
        "code": code
    }))
    .expect("outcome JSON")
}

fn correction_event(index: usize, target: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "ledger-billing-correction/2",
        "customer": CUSTOMER,
        "source": SOURCE,
        "id": format!("m3-correction-{index:06}"),
        "target": target,
        "family": "quality",
        "occurred_at": ADJUSTMENT_OCCURRED_AT,
        "evidence": "Synthetic M3 qualification correction recorded by the harness",
        "expected_revision": "1",
        "replacement": {"kind": "code", "code": "none"}
    }))
    .expect("correction JSON")
}

/// The workload schedule, fixed by decision index so two runs of the same mode
/// submit the same bytes in the same order.
fn wants_outcome(index: usize) -> bool {
    index.is_multiple_of(3)
}

fn wants_correction(index: usize) -> bool {
    index.is_multiple_of(15)
}

fn wants_retry(index: usize) -> bool {
    index.is_multiple_of(10)
}

fn wants_alias(index: usize) -> bool {
    index.is_multiple_of(25)
}

fn outcome_code(index: usize) -> &'static str {
    if index.is_multiple_of(6) {
        "rebate"
    } else {
        "none"
    }
}

/// The harness's own arithmetic model of the agreed total, closed over the
/// counters of what the product accepted: the agreed price for every accepted
/// base submission, plus the rebate for every accepted rebate outcome that was
/// not later corrected back to the zero-amount code. It is computed from the
/// harness's own record of what it submitted and what the product accepted, never
/// from the statement under test, so a disagreement is a real disagreement.
fn expected_net(
    bases: usize,
    rebate_outcomes: usize,
    corrected_rebates: usize,
) -> Result<i128, String> {
    let uncorrected = rebate_outcomes
        .checked_sub(corrected_rebates)
        .ok_or("more accepted rebate corrections than accepted rebate outcomes")?;
    Ok(PRICE_ATOMS * bases as i128 + REBATE_ATOMS * uncorrected as i128)
}

/// One step of the harness's ordered-identity digest. Both sides of every
/// comparison in this report use this function, so a difference is always a
/// difference in the receipt identities themselves and never in how they were
/// digested.
fn chain_step(chain: &mut Sha256, id: &str) {
    chain.update((id.len() as u64).to_be_bytes());
    chain.update(id.as_bytes());
}

/// What the harness's own record of the accepted history held at one instant: the
/// retained-entry count it had reached, the digest of the ordered receipt
/// identities its accepted public operations had returned by then, and the
/// running total from its own price model. Nothing here grows with the size of
/// the history.
struct Mark {
    count: usize,
    chain: String,
    total: i128,
}

/// The harness's own record of the accepted history, built only from what the
/// public accept operations returned.
///
/// The product returns an accepted receipt identity for every operation it
/// retains, so the harness digests those identities in the order it received
/// them and adds up its own price model beside them. That is the harness's side
/// of every checkpoint comparison in this report. The other side is computed
/// from the product's final ordered statement, so the two sides never come from
/// the same product output: a statement that dropped, reordered or restated an
/// entry would disagree with what the product returned at submission time instead
/// of agreeing with itself. Keeping a digest rather than the identities is what
/// bounds this: a run of 100,000 base submissions reconciles in constant memory.
///
/// A duplicate result is never recorded, because it retained no new entry.
struct Accepted {
    chain: Sha256,
    count: usize,
    total: i128,
}

impl Accepted {
    fn new() -> Self {
        Self {
            chain: Sha256::new(),
            count: 0,
            total: 0,
        }
    }

    /// Record one accepted public operation: the receipt identity it returned,
    /// and the atoms the harness's own price model gives it.
    fn record(&mut self, accepted: &Value, atoms: i128) -> Result<(), String> {
        let id = accepted["receipt"]["id"]
            .as_str()
            .ok_or_else(|| format!("an accepted result carried no receipt identity: {accepted}"))?;
        chain_step(&mut self.chain, id);
        self.count += 1;
        self.total = self
            .total
            .checked_add(atoms)
            .ok_or("the running total of accepted operations overflowed")?;
        Ok(())
    }

    /// What this record holds right now, for a capture taken at this instant.
    fn mark(&self) -> Mark {
        Mark {
            count: self.count,
            chain: ledgerlab_core::canonical::hex(&self.chain.clone().finalize()),
            total: self.total,
        }
    }
}

/// One complete statement plus its pinned finance CSV projection, reduced to
/// bounded metadata: what a later comparison still needs and nothing that grows
/// with the size of the history.
///
/// Two digests are kept per capture, and they come from different product
/// output. `own` is the harness's own record of what the accepted public
/// operations returned, and `statement_chain` is the digest of the ordered
/// receipt identities inside this one statement call. The final statement is
/// then walked once, streaming, to supply the third side. The chains are compared
/// between computations over different outputs; they are not a security digest
/// and they are not the product's own `snapshot_hash`.
struct Snapshot {
    serial: usize,
    cutoff: usize,
    net_atoms: i128,
    snapshot_hash: String,
    own: Mark,
    statement_chain: String,
    csv_export_id: String,
    csv_postings: usize,
    csv_rows: usize,
    csv_total_atoms: i128,
}

/// Every projection gets its own file, even when two captures legitimately
/// report the same cutoff: the product publishes with `create_new`, so a
/// repeated path would fail instead of writing a second projection.
fn projection_name(serial: usize, cutoff: usize) -> String {
    format!("statement-{serial:04}-cutoff-{cutoff:06}.csv")
}

impl Snapshot {
    fn json(&self) -> Value {
        json!({
            "capture_serial": self.serial,
            "output_file": projection_name(self.serial, self.cutoff),
            "cutoff": self.cutoff.to_string(),
            "net_atoms": self.net_atoms.to_string(),
            "snapshot_hash": self.snapshot_hash,
            "ordered_receipt_chain_from_returned_operations": self.own.chain,
            "prefix_total_atoms_from_returned_operations": self.own.total.to_string(),
            "ordered_receipt_chain_from_this_statement": self.statement_chain,
            "returned_operation_count_at_this_cutoff": self.own.count,
            "csv": {
                "export_id": self.csv_export_id,
                "posting_count": self.csv_postings.to_string(),
                "total_rows_including_header_and_trailer": self.csv_rows,
                "summed_amount_atoms": self.csv_total_atoms.to_string(),
                "agrees_with_statement_total": self.csv_total_atoms == self.net_atoms
            }
        })
    }
}

/// The statement's own entries, borrowed. The final statement of a large run is
/// never copied: only what a later comparison needs is taken from it.
fn entry_list(value: &Value) -> Result<&Vec<Value>, String> {
    value["entries"]
        .as_array()
        .ok_or_else(|| "statement has no entries array".to_owned())
}

fn integer(value: &Value, key: &str) -> Result<i128, String> {
    value[key]
        .as_str()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("{key} is not an integer string"))
}

fn target_of(receipt: &Value) -> Result<String, String> {
    receipt["body"]["target"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "accepted receipt has no target".to_owned())
}

/// Walk the product's final ordered statement in order and record, at each cutoff
/// a capture declared, the ordered receipt-identity chain and the integer total
/// of the prefix ending there. One pass, O(1) extra memory, and this is the
/// product's own side of every checkpoint comparison: it is never built from the
/// same output as the harness's own side, which comes from the accepted receipt
/// identities the public operations returned. Each entry's own ordinal must also
/// be its position in that order, so the statement cannot quietly reorder the
/// history and still agree with the harness's running count.
fn prefixes_at(
    statement: &Value,
    cutoffs: &BTreeSet<usize>,
) -> Result<BTreeMap<usize, (String, i128)>, String> {
    let mut observed: BTreeMap<usize, (String, i128)> = BTreeMap::new();
    let mut chain = Sha256::new();
    let mut total = 0i128;
    for (offset, entry) in entry_list(statement)?.iter().enumerate() {
        let position = offset + 1;
        let ordinal = position.to_string();
        if entry["ordinal"].as_str() != Some(ordinal.as_str()) {
            return Err(format!(
                "statement entry {position} reports ordinal {:?}",
                entry["ordinal"]
            ));
        }
        let id = entry["receipt"]["id"]
            .as_str()
            .ok_or_else(|| format!("statement entry {position} has no receipt id"))?;
        chain_step(&mut chain, id);
        total = total
            .checked_add(integer(entry, "net_atoms")?)
            .ok_or("the statement's running total overflowed")?;
        if cutoffs.contains(&position) {
            observed.insert(
                position,
                (
                    ledgerlab_core::canonical::hex(&chain.clone().finalize()),
                    total,
                ),
            );
        }
    }
    Ok(observed)
}

async fn run(options: Options) -> Result<Value, String> {
    let started = Instant::now();
    // Read the source identity before anything else, so the report names the
    // tree this run was built from.
    let source = source_report();
    // The temporary directory guard is held here, for the whole run, so the
    // installation and its snapshots stay on disk until the run has finished
    // reading them. Dropping it early removed the installation out from under
    // the open store.
    let scratch = match &options.keep {
        Some(_) => None,
        None => Some(
            tempfile::tempdir()
                .map_err(|error| format!("cannot create a private scratch directory: {error}"))?,
        ),
    };
    let root = match (&options.keep, &scratch) {
        (Some(dir), _) => dir.canonicalize_or_create()?.join("m3-qualification"),
        (None, Some(scratch)) => scratch.path().to_path_buf(),
        (None, None) => return Err("no scratch directory was created".into()),
    };
    let root = root.canonicalize_or_create()?;
    let installation = root.join("store");
    let snapshot_dir = root.join("snapshots");
    fs::create_dir_all(&snapshot_dir)
        .map_err(|error| format!("cannot create the snapshot directory: {error}"))?;
    BillingLedger::init(&installation, SETUP)
        .await
        .map_err(|error| format!("cannot create the installation: {}", describe(error)))?;
    let ledger = BillingLedger::open(&installation)
        .await
        .map_err(|error| format!("cannot open the installation: {}", describe(error)))?;

    let mut counters = Counters::default();
    // The harness's own record of the accepted history, built only from what the
    // public accept operations return. Nothing else in this run feeds it.
    let mut accepted_history = Accepted::new();
    let mut captured: Vec<Snapshot> = Vec::new();
    let mut serial = 0usize;
    let mut next_checkpoint = 0usize;
    let mut last_submitted = 0usize;
    let workload_started = Instant::now();

    'workload: for index in 1..=options.decisions {
        let raw = work_event(index);
        let receipt = match ledger.accept(CUSTOMER, SOURCE, &raw).await {
            Ok(value) => value,
            Err(error) => {
                if let Some(refusal) = code(&error) {
                    counters.refuse(index, "work", refusal);
                    break 'workload;
                }
                return Err(format!("decision {index}: {}", describe(error)));
            }
        };
        if receipt["status"] != "accepted" {
            return Err(format!("decision {index} was {}", receipt["status"]));
        }
        let original = receipt["receipt"].clone();
        counters.base += 1;
        accepted_history.record(&receipt, PRICE_ATOMS)?;
        last_submitted = index;
        let target = target_of(&original)?;

        if wants_outcome(index) {
            let raw = outcome_event(index, &target, outcome_code(index));
            match ledger.outcome(CUSTOMER, SOURCE, &raw).await {
                Ok(value) if value["status"] == "accepted" => {
                    counters.outcomes += 1;
                    let rebate = outcome_code(index) == "rebate";
                    if rebate {
                        counters.rebate_outcomes += 1;
                    }
                    accepted_history.record(&value, if rebate { REBATE_ATOMS } else { 0 })?;
                }
                Ok(value) => return Err(format!("outcome {index} was {}", value["status"])),
                Err(error) => {
                    if let Some(refusal) = code(&error) {
                        counters.refuse(index, "outcome", refusal);
                        break 'workload;
                    }
                    return Err(format!("outcome {index}: {}", describe(error)));
                }
            }
        }
        if wants_correction(index) {
            let raw = correction_event(index, &target);
            match ledger.correct(CUSTOMER, SOURCE, &raw).await {
                Ok(value) if value["status"] == "accepted" => {
                    counters.corrections += 1;
                    // A correction reverses the outcome at the same decision, so
                    // it carries the opposite sign of whatever that outcome was
                    // worth and nothing at all for a zero-amount outcome.
                    let reversed_rebate = outcome_code(index) == "rebate";
                    if reversed_rebate {
                        counters.corrected_rebates += 1;
                    }
                    accepted_history
                        .record(&value, if reversed_rebate { -REBATE_ATOMS } else { 0 })?;
                }
                Ok(value) => return Err(format!("correction {index} was {}", value["status"])),
                Err(error) => {
                    if let Some(refusal) = code(&error) {
                        counters.refuse(index, "correction", refusal);
                        break 'workload;
                    }
                    return Err(format!("correction {index}: {}", describe(error)));
                }
            }
        }
        if wants_retry(index) {
            counters.duplicate(ledger.accept(CUSTOMER, SOURCE, &raw).await, &original)?;
        }
        if wants_alias(index) {
            counters.duplicate(
                ledger.accept(CUSTOMER, SOURCE, &alias_event(index)).await,
                &original,
            )?;
        }
        // One capture per checkpoint the run has reached, and no more: the
        // checkpoint set is fixed and small, so a large run holds bounded
        // metadata instead of a copy of the history per capture. The harness's
        // own mark of the accepted history is taken at the same instant, so the
        // capture can be reconciled against what the product returned at
        // submission time and against the final statement's own prefix.
        while next_checkpoint < CHECKPOINTS.len()
            && counters.retained() >= CHECKPOINTS[next_checkpoint]
        {
            serial += 1;
            // The harness's own mark of the accepted history is taken at the same
            // instant as the capture and handed to it, so the capture is
            // reconciled against what the product returned at submission time and
            // against the final statement's own prefix, never against itself.
            let shot = snapshot(&ledger, &snapshot_dir, serial, accepted_history.mark()).await?;
            captured.push(shot);
            next_checkpoint += 1;
        }
    }

    // A refusal stops the run. Its stability is then measured rather than
    // assumed: a few further submissions record what the product actually did,
    // including any it accepts. A probe that the product accepts is counted as
    // accepted work, not hidden.
    if counters.stop.is_some() {
        for offset in 1..=REFUSAL_PROBE {
            let index = last_submitted + offset;
            let outcome = match ledger.accept(CUSTOMER, SOURCE, &work_event(index)).await {
                Ok(value) => {
                    if value["status"] == "accepted" {
                        counters.base += 1;
                        accepted_history.record(&value, PRICE_ATOMS)?;
                    }
                    format!(
                        "{} {}",
                        value["status"],
                        value["receipt"]["id"].as_str().unwrap_or("")
                    )
                }
                Err(error) => match code(&error) {
                    Some(refusal) => {
                        counters.refuse(index, "work-after-refusal", refusal);
                        format!("refused {refusal}")
                    }
                    None => return Err(format!("probe {index}: {}", describe(error))),
                },
            };
            if let Some(stop) = counters.stop.as_mut() {
                stop.probe.push((index, outcome));
            }
        }
    }
    let workload_elapsed = workload_started.elapsed();
    let accepted = counters.base;

    // An unknown commit result is resolved by reopening and retrying the
    // original identity, never by assuming rollback. Reopen once and retry the
    // oldest decision in the retained history.
    ledger.close().await;
    let reopen_started = Instant::now();
    let ledger = BillingLedger::open(&installation)
        .await
        .map_err(|error| format!("cannot reopen the installation: {}", describe(error)))?;
    let reopen_elapsed = reopen_started.elapsed();
    let statement = ledger
        .statement(CUSTOMER, None)
        .await
        .map_err(|error| format!("final statement: {}", describe(error)))?;
    if statement["complete"] != true || statement["schema"] != "ledger-billing-statement/2" {
        return Err("final statement is not a complete schema-2 statement".into());
    }
    let final_cutoff = integer(&statement, "cutoff")? as usize;
    let final_net = integer(&statement, "net_atoms")?;
    let written = counters.retained();
    if final_cutoff != written {
        return Err(format!(
            "final cutoff {final_cutoff} does not match the {written} written entries"
        ));
    }
    // The harness's own count of accepted public operations, the product's own
    // entry count, and the entries those operations reported must all be the same
    // number. None of them is derived from the other.
    if accepted_history.count != final_cutoff {
        return Err(format!(
            "the product returned {} accepted operations but its final statement reports {final_cutoff} \
             entries",
            accepted_history.count
        ));
    }
    // Two computations of the harness's own arithmetic over the same accepted
    // work: the running total accumulated one accepted operation at a time, and
    // the closed form over the accepted-kind counters. They must agree before
    // either is compared with the product.
    let model = expected_net(
        counters.base,
        counters.rebate_outcomes,
        counters.corrected_rebates,
    )?;
    if accepted_history.total != model {
        return Err(format!(
            "the running total of accepted operations {} does not match the independent model \
             {model}",
            accepted_history.total
        ));
    }
    if final_net != model {
        return Err(format!(
            "final net {final_net} does not match the independent model {model}"
        ));
    }
    let mut oldest_retry_matched = false;
    if accepted > 0 {
        let original = entry_list(&statement)?[0]["receipt"].clone();
        let retry = ledger.accept(CUSTOMER, SOURCE, &work_event(1)).await;
        let after = ledger
            .statement(CUSTOMER, None)
            .await
            .map_err(|error| format!("statement after retry: {}", describe(error)))?;
        counters.reopened_oldest(retry, &original, &statement, &after)?;
        oldest_retry_matched = after["entries"][0]["receipt"] == original;
    }
    // One streaming pass over the final ordered statement: the ordered receipt
    // chain and the integer total at each captured cutoff. Nothing per capture is
    // held, so a run of 100,000 base submissions reconciles without a copy of the
    // history per checkpoint.
    let captured_cutoffs: BTreeSet<usize> = captured.iter().map(|shot| shot.cutoff).collect();
    let observed = prefixes_at(&statement, &captured_cutoffs)?;
    let mut unreconciled: Vec<String> = Vec::new();
    for shot in &captured {
        let Some((final_chain, final_prefix_total)) = observed.get(&shot.cutoff) else {
            unreconciled.push(format!(
                "cutoff {} is beyond the final history",
                shot.cutoff
            ));
            continue;
        };
        // Four comparisons, none of which compares a statement with itself.
        //
        // 1. What the product returned at submission time, digested in the order
        //    the harness received it, against the final statement's own prefix:
        //    the independence check. A statement that dropped, reordered or
        //    restated an entry fails here even if it is perfectly self
        //    consistent.
        if shot.own.chain != *final_chain {
            unreconciled.push(format!(
                "cutoff {}: the receipt identities the product returned when it accepted the work \
                 do not match the identities in the final statement's own prefix",
                shot.cutoff
            ));
        // 2. The same two, for the integer total: the harness's own running
        //    arithmetic against the final statement's own prefix total.
        } else if shot.own.total != *final_prefix_total {
            unreconciled.push(format!(
                "cutoff {}: the harness's own total of accepted operations is {} against the \
                 final statement's own prefix total {final_prefix_total}",
                shot.cutoff, shot.own.total
            ));
        // 3. The capture's own statement against the harness's own record, so
        //    the capture is known to have been self consistent and about the
        //    same history the harness recorded at that instant.
        } else if shot.statement_chain != shot.own.chain || shot.net_atoms != shot.own.total {
            unreconciled.push(format!(
                "cutoff {}: the capture's own statement does not match the accepted operations the \
                 product had returned by then",
                shot.cutoff
            ));
        // 4. The retained prefix against the final statement, so nothing behind
        //    the capture moved after it was taken.
        } else if shot.statement_chain != *final_chain || shot.net_atoms != *final_prefix_total {
            unreconciled.push(format!(
                "cutoff {} receipt prefix changed under the final history",
                shot.cutoff
            ));
        } else if shot.csv_postings == 0
            || shot.csv_rows != shot.csv_postings + 2
            || shot.csv_total_atoms != shot.net_atoms
            || shot.csv_export_id.is_empty()
        {
            unreconciled.push(format!("cutoff {} CSV did not reconcile", shot.cutoff));
        }
    }
    if !unreconciled.is_empty() {
        return Err(format!(
            "{} captured snapshots do not reconcile: {}",
            unreconciled.len(),
            unreconciled.join("; ")
        ));
    }
    serial += 1;
    let final_shot = snapshot(&ledger, &snapshot_dir, serial, accepted_history.mark()).await?;
    ledger.close().await;
    let database = storage_report(&installation);
    let memory = memory_report();
    let total_elapsed = started.elapsed();

    Ok(json!({
        "schema": "bean-counter-m3-qualification/2",
        "status": if counters.stop.is_some() { "incomplete_refused_by_product" } else { "requested_workload_submitted" },
        "mode": options.mode,
        "scope": {
            "requested_base_submissions": options.decisions,
            "profile": "ordinary local SQLite, one installation, one host, public billing facade only",
            "statement": "Synthetic single-host measurement. Not product conformance, not a capacity or saturation guarantee, not a resource reservation, and no evidence for PostgreSQL, multi-host or resource-proof capabilities.",
            "ceilings": "This harness does not model, predict or depend on any admission ceiling. It submits the work it was asked to submit and reports what the product did with it.",
            "counting": "The requested count is base submissions: --decisions and the mode default count the billable work submissions only. Every third base submission is followed by an outcome submission, every fifteenth by a correction, every tenth by a delivery retry and every twenty-fifth by a semantic alias; those are counted separately, and a delivery retry or alias retains no new decision. The decisions the store retains are a third number again, given as final_retained_economic_decisions and as the retained_entry_breakdown, and the checkpoint schedule is counted in those retained entries rather than in submissions."
        },
        "source": source,
        "host": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
            "pointer_width": usize::BITS
        },
        "toolchain": toolchain_report(),
        "workload": {
            "requested_base_submissions": options.decisions,
            "requested_base_submissions_note": "Base submissions only. Outcomes, corrections, delivery retries and semantic aliases are additional submissions this schedule also makes and are counted separately below, and the retained decisions are counted again as final_retained_economic_decisions.",
            "base_submissions_accepted": counters.base,
            "outcome_submissions_accepted": counters.outcomes,
            "correction_submissions_accepted": counters.corrections,
            "retries_submitted": counters.retries(),
            "retries_resolved_by_same_identity": counters.duplicate_identity,
            "retries_resolved_by_semantic_alias": counters.duplicate_semantic,
            "total_submitted_work": counters.submitted(),
            "final_retained_economic_decisions": final_cutoff,
            "retained_entry_breakdown": {
                "from_base_submissions": counters.base,
                "from_outcome_submissions": counters.outcomes,
                "from_correction_submissions": counters.corrections
            },
            "refusals": counters.refusals,
            "refused_at": counters.stop.as_ref().map(|stop| json!({
                "decision": stop.index,
                "submission_kind": stop.kind,
                "refusal_code": stop.code,
                "further_attempts": stop.probe.iter().map(|(index, outcome)| json!({
                    "decision": index,
                    "outcome": outcome
                })).collect::<Vec<_>>()
            }))
        },
        "reconciliation": {
            "statement_complete": statement["complete"],
            "statement_cutoff": final_cutoff.to_string(),
            "statement_net_atoms": final_net.to_string(),
            "accepted_operations_the_product_returned": accepted_history.count,
            "harness_own_running_total_atoms": accepted_history.total.to_string(),
            "independent_model_net_atoms": model.to_string(),
            "model_agrees": final_net == model,
            "running_total_agrees_with_model": accepted_history.total == model,
            "returned_operation_count_is_the_statement_cutoff": accepted_history.count == final_cutoff,
            "oldest_identity_retry_after_reopen": oldest_retry_matched,
            "checkpoints": {
                "plan_retained_entries": CHECKPOINTS,
                "unit": "retained economic entries, the unit the statement counts; roughly 71,000 accepted base submissions per 100,000 retained entries on this schedule",
                "reached": captured.len(),
                "not_reached": CHECKPOINTS[next_checkpoint..].to_vec(),
                "reached_cutoffs": captured.iter().map(|shot| shot.cutoff).collect::<Vec<_>>(),
                "reconciling_to_their_cutoff": captured.len(),
                "unreconciled": unreconciled.len(),
                "compared_by": "at each captured cutoff, a running SHA-256 chain over the ordered receipt identities the accepted public operations returned, and the harness's own running total, against a streaming chain and prefix total computed from the final ordered statement. The two sides are computed from different product outputs: the first from what the product returned when it accepted the work, the second from what it reports afterwards.",
                "also_compared": "the capture's own statement chain and total, against both, so the capture is known to have been self consistent and the retained prefix is known not to have moved",
                "independence_note": "No comparison here is between a statement and itself, and no receipt identities are retained: a checkpoint keeps one digest and one integer, so a run of 100,000 base submissions reconciles in bounded memory.",
                "note": "One statement and one CSV are read at a time and released immediately. The run keeps bounded metadata per checkpoint and the final statement's own reconciliation data, not a copy of the history per checkpoint.",
                "captured": captured.iter().map(|shot| shot.json()).collect::<Vec<_>>()
            },
            "final_snapshot": final_shot.json()
        },
        "elapsed": {
            "total_seconds": seconds(total_elapsed),
            "workload_seconds": seconds(workload_elapsed),
            "reopen_and_final_statement_seconds": seconds(reopen_elapsed),
            "submitted_work_per_second_this_run": rate(counters.submitted(), workload_elapsed),
            "note": "One run on one host. No figure here is extrapolated to another workload size, and none of them is a capacity, throughput or completion guarantee."
        },
        "storage": database,
        "memory": memory,
        "installation": {
            "root": root.display().to_string(),
            "temporary_root_removed_at_exit": scratch.is_some(),
            "snapshot_directory": snapshot_dir.display().to_string()
        },
        "notes": notes(
            options.mode,
            counters.stop.as_ref(),
            counters.submitted(),
            counters.refused(),
            counters.base
        )
    }))
}

fn seconds(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64())
}

fn rate(count: usize, duration: Duration) -> String {
    if duration.is_zero() {
        return "unmeasured".into();
    }
    format!("{:.2}", count as f64 / duration.as_secs_f64())
}

/// Take a complete statement and its pinned finance CSV projection from the same
/// open installation, then check that both cover the same cutoff and the same
/// integer total. `own` is the harness's own record of the accepted operations at
/// this instant; it is required rather than derived here, so a capture can never
/// be reconciled against itself.
async fn snapshot(
    ledger: &BillingLedger,
    directory: &Path,
    serial: usize,
    own: Mark,
) -> Result<Snapshot, String> {
    let statement = ledger
        .statement(CUSTOMER, None)
        .await
        .map_err(|error| format!("statement: {}", describe(error)))?;
    if statement["complete"] != true || statement["schema"] != "ledger-billing-statement/2" {
        return Err("statement is not a complete schema-2 statement".into());
    }
    let cutoff = integer(&statement, "cutoff")? as usize;
    if cutoff != own.count {
        return Err(format!(
            "statement reports cutoff {cutoff} against the {} accepted operations the product \
             had returned when this capture was taken",
            own.count
        ));
    }
    let net_atoms = integer(&statement, "net_atoms")?;
    let hash = statement["snapshot_hash"]
        .as_str()
        .ok_or("statement has no snapshot_hash")?
        .to_owned();
    // The statement's own entries must be exactly its own cutoff, and must sum
    // to its own total. They are read in place and released with the statement,
    // so nothing per entry is retained past this call.
    let mut summed = 0i128;
    let mut chain = Sha256::new();
    for (offset, entry) in entry_list(&statement)?.iter().enumerate() {
        let id = entry["receipt"]["id"]
            .as_str()
            .ok_or_else(|| format!("statement entry {} has no receipt id", offset + 1))?;
        chain_step(&mut chain, id);
        summed = summed
            .checked_add(integer(entry, "net_atoms")?)
            .ok_or("the statement's own total overflowed")?;
    }
    let entries_seen = entry_list(&statement)?.len();
    if cutoff != entries_seen || summed != net_atoms {
        return Err(format!(
            "statement cutoff {cutoff} reports {entries_seen} entries summing to {summed} \
             against its own total {net_atoms}"
        ));
    }
    let statement_chain = ledgerlab_core::canonical::hex(&chain.finalize());
    let output = directory.join(projection_name(serial, cutoff));
    let summary = ledger
        .export_csv(CUSTOMER, &hash, MAPPING, &output)
        .await
        .map_err(|error| format!("finance CSV export: {}", describe(error)))?;
    let published = fs::read_to_string(&output)
        .map_err(|error| format!("cannot read the published export: {error}"))?;
    if summary["complete"] != true || summary["snapshot_hash"] != hash {
        return Err("export summary is not the pinned complete snapshot".into());
    }
    let export_id = summary["export_id"]
        .as_str()
        .ok_or("export summary has no export_id")?
        .to_owned();
    let postings = integer(&summary, "posting_count")? as usize;
    let mut lines = published.lines();
    let header = lines.next().unwrap_or_default();
    let mut total_rows = 1usize;
    let mut posting_rows = 0usize;
    let mut total_atoms = 0i128;
    for line in lines {
        total_rows += 1;
        let cells: Vec<&str> = line.split("\",\"").collect();
        if cells.first().copied().unwrap_or_default().trim_matches('"') != "posting" {
            continue;
        }
        posting_rows += 1;
        let cell = |index: usize| cells.get(index).map(|cell| cell.trim_matches('"'));
        total_atoms += cell(29)
            .and_then(|atoms| atoms.parse::<i128>().ok())
            .ok_or("CSV posting row has no integer amount_atoms")?;
    }
    if !header.starts_with("\"row_type\"") {
        return Err("published CSV has no header row".into());
    }
    if posting_rows != postings || total_atoms != net_atoms || total_rows != postings + 2 {
        return Err(format!(
            "CSV at cutoff {cutoff} has {posting_rows} posting rows, {total_rows} total rows and {total_atoms} atoms against {postings} and {net_atoms}"
        ));
    }
    Ok(Snapshot {
        serial,
        cutoff,
        net_atoms,
        snapshot_hash: hash,
        own,
        statement_chain,
        csv_export_id: export_id,
        csv_postings: postings,
        csv_rows: total_rows,
        csv_total_atoms: total_atoms,
    })
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

trait CanonicalizeOrCreate {
    fn canonicalize_or_create(&self) -> Result<PathBuf, String>;
}

impl CanonicalizeOrCreate for PathBuf {
    fn canonicalize_or_create(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(self)
            .map_err(|error| format!("cannot create {}: {error}", self.display()))?;
        self.canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", self.display()))
    }
}

/// Ask git directly, in this worktree. An unavailable answer is reported as
/// unavailable rather than reconstructed: this harness reads the repository the
/// way the project's own packaging scripts do.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Process::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned(),
    )
}

/// The exact source this run was built from, read from git itself. The
/// hand-rolled `.git` file parsing this replaces reported a full 40-character id
/// as a "short" revision, silently missed a dirty worktree, and could report an
/// unresolved ref on a packed-refs checkout.
fn source_report() -> Value {
    let root = repo_root();
    let porcelain = git(&root, &["status", "--porcelain"]);
    let (dirty, changed) = match &porcelain {
        Some(text) => {
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let mut value = json!({
                "count": lines.len(),
                "paths": lines.iter().take(MAX_LISTED_PATHS).map(|line| line.trim().to_owned()).collect::<Vec<_>>()
            });
            if lines.len() > MAX_LISTED_PATHS {
                value["truncated"] = json!(true);
            }
            (json!(!lines.is_empty()), value)
        }
        None => (
            Value::Null,
            json!("unavailable: git status --porcelain did not run here"),
        ),
    };
    json!({
        "short_commit": git(&root, &["rev-parse", "--short", "HEAD"]),
        "commit": git(&root, &["rev-parse", "HEAD"]),
        "branch": git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "worktree_dirty": dirty,
        "worktree_changed_entries": changed,
        "measured_by": ["git rev-parse --short HEAD", "git rev-parse HEAD", "git status --porcelain"],
        "measured_when": "at the start of this run, before the harness wrote anything",
        "dirty_note": "A dirty worktree means these numbers describe a tree that is not the commit above. Nothing is inferred from the crate version.",
        "crate": "ledgerlab",
        "crate_version": env!("CARGO_PKG_VERSION"),
        "billing_contract_version": ledgerlab::billing::CONTRACT_VERSION,
        "repository_root": root.display().to_string()
    })
}

fn toolchain_report() -> Value {
    let compiler = std::env::var_os("RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rustc"));
    let verbose = Process::new(&compiler)
        .arg("-vV")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let field = |key: &str| {
        verbose.as_ref().and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix(&format!("{key}: ")))
                .map(str::to_owned)
        })
    };
    // The first line of `rustc -vV` is the version itself, with no field name.
    let version = verbose
        .as_ref()
        .and_then(|text| text.lines().next())
        .map(str::to_owned);
    let pin_path = repo_root().join("rust-toolchain.toml");
    let channel = fs::read_to_string(&pin_path).ok().and_then(|text| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix("channel"))
            .and_then(|rest| rest.split('=').nth(1))
            .map(|value| value.trim().trim_matches('"').to_owned())
    });
    json!({
        "rustc": version,
        "host_triple": field("host"),
        "rustc_source": if std::env::var_os("RUSTC").is_some() { "RUSTC" } else { "PATH" },
        "pinned_channel": channel,
        "pinned_channel_source": "rust-toolchain.toml",
        "note": "The pin is a development toolchain choice read from the repository. It is not an MSRV claim and not a supported-platform claim."
    })
}

/// What the run left on disk, measured after the store was closed.
///
/// The product's installation database name is used to separate the main
/// database from the WAL, shared-memory and other sidecars beside it. A read or
/// stat that fails is reported as unavailable with its error, never as zero
/// bytes: a zero here would be indistinguishable from an empty file.
///
/// Every entry is stat'ed with `symlink_metadata`, so a symbolic link is
/// reported as the link it is and is never followed to whatever it points at.
/// An entry's kind and its length are taken from that one stat and never from a
/// second lookup, so no entry can be classified one way and measured another.
/// Following one would let a link standing in for the database be reported as
/// this installation's own database, and would put another file's bytes into
/// this installation's total. A symbolic link is therefore not measurable here:
/// its own length is the length of a path, not of an artifact of this run, so it
/// is reported with no bytes and the aggregate is withdrawn.
///
/// Enumeration alone is not a measurement either. If the directory could be read
/// but the installation's own main database is not there as a regular file, this
/// is not a description of this installation's artifacts at all, so the
/// measurement is reported as partial with the reason, the main database is
/// reported as absent rather than as a null that reads like a measurement, and
/// the aggregate is withdrawn rather than reported as a sum that silently leaves
/// the database out. In the ordinary case, where the database is there and every
/// entry could be measured, the aggregate stays exactly what it was: the sum of
/// every file in that one directory, correctly labelled as not being the database
/// size.
fn storage_report(installation: &Path) -> Value {
    /// The installation database the product itself requires to exist.
    const MAIN_DATABASE: &str = "local.db";
    let data = installation.join(".ledger");
    let entries = match fs::read_dir(&data) {
        Ok(entries) => entries,
        Err(error) => {
            return json!({
                "status": "unavailable",
                "unavailable_because": format!(
                    "the installation's .ledger directory could not be read: {error}"
                ),
                "main_database": Value::Null,
                "main_database_unavailable_because": format!(
                    "nothing was measured, so no main database size is reported: the \
                     installation's .ledger directory could not be read: {error}"
                ),
                "aggregate_ledger_artifact_bytes": Value::Null,
                "files": [],
                "read_or_stat_failures": [],
                "measured_when": "after the store was closed",
                "note": "Nothing was measured, so nothing is reported as a size."
            })
        }
    };
    let mut files = Vec::new();
    let mut failures: Vec<Value> = Vec::new();
    let mut aggregate: Option<u64> = Some(0);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(
                    json!({"error": format!("a directory entry could not be read: {error}")}),
                );
                aggregate = None;
                continue;
            }
        };
        // The link itself, never its target: `metadata` would follow a symbolic
        // link and report whatever it points at as this installation's file. The
        // kind and the length come from this one result, so an entry cannot be
        // classified one way by a second lookup and measured another.
        match fs::symlink_metadata(entry.path()) {
            Ok(meta) => {
                let name: String = entry.file_name().to_string_lossy().into_owned();
                if meta.file_type().is_symlink() {
                    let reason = format!(
                        "{name} is a symbolic link, so it is not an artifact of this installation \
                         and the file it points at is not this run's work; no bytes are reported \
                         for it and the aggregate is withdrawn"
                    );
                    files.push(json!({
                        "name": name,
                        "bytes": Value::Null,
                        "is_file": false,
                        "is_symlink": true,
                        "bytes_unavailable_because": reason
                    }));
                    failures.push(json!({"name": name, "error": reason}));
                    aggregate = None;
                    continue;
                }
                if let Some(total) = aggregate.as_mut() {
                    *total += meta.len();
                }
                files.push(json!({
                    "name": name,
                    "bytes": meta.len(),
                    "is_file": meta.is_file(),
                    "is_symlink": false
                }));
            }
            Err(error) => {
                failures.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "error": format!("its kind and size could not be read: {error}")
                }));
                aggregate = None;
            }
        }
    }
    files.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    // The main database must be present as a regular file that is not a symbolic
    // link. An absent one, a directory wearing its name, or a link pointing
    // somewhere else is not a smaller measurement: it means this is not a
    // measurement of the installation at all.
    let named = files
        .iter()
        .find(|file| file["name"] == MAIN_DATABASE)
        .cloned();
    let main_database_unavailable_because = match &named {
        Some(file) if file["is_file"] == true && file["is_symlink"] == false => Value::Null,
        Some(file) if file["is_symlink"] == true => {
            let reason = format!(
                "the installation's main database {MAIN_DATABASE} is a symbolic link, so the \
                 installation's own database was not measured and the file it points at is not \
                 reported as this installation's database"
            );
            failures.push(json!({"name": MAIN_DATABASE, "error": reason}));
            aggregate = None;
            json!(reason)
        }
        Some(_) => {
            let reason = format!(
                "the installation's main database {MAIN_DATABASE} exists in its .ledger directory \
                 but is not a regular file, so no database size is reported"
            );
            failures.push(json!({"name": MAIN_DATABASE, "error": reason}));
            aggregate = None;
            json!(reason)
        }
        None => {
            let reason = format!(
                "the installation's main database {MAIN_DATABASE} is not present in its .ledger \
                 directory, so no database size is reported and the aggregate is withdrawn rather \
                 than reported as a sum that leaves the database out"
            );
            failures.push(json!({"name": MAIN_DATABASE, "error": reason}));
            aggregate = None;
            json!(reason)
        }
    };
    let main = named
        .filter(|file| file["is_file"] == true && file["is_symlink"] == false)
        .map(|file| json!({"name": MAIN_DATABASE, "bytes": file["bytes"]}))
        .unwrap_or(Value::Null);
    let unavailable = !failures.is_empty();
    json!({
        "status": if unavailable { "partial" } else { "measured" },
        "main_database": main,
        "main_database_note": "The file the product requires as this installation's database, stat'ed without following a symbolic link. Other files in the same directory are its sidecars, not the database.",
        "main_database_unavailable_because": main_database_unavailable_because,
        "aggregate_ledger_artifact_bytes": aggregate,
        "aggregate_note": "The sum of every file in this installation's .ledger directory: the database, its WAL and shared-memory files, and any other sidecar present at the end of the run. It is not the database size, and it is not a growth rate or a storage requirement. A symbolic link contributes nothing to it and withdraws it, because the file it points at is not this installation's artifact.",
        "files": files,
        "read_or_stat_failures": failures,
        "unavailable_because": if unavailable {
            json!(format!(
                "this installation's ledger artifacts are only partly measurable, so the aggregate \
                 is reported as unavailable rather than as a partial or zero total: {}",
                failures
                    .iter()
                    .map(|failure| format!(
                        "{}: {}",
                        failure["name"].as_str().unwrap_or("an entry"),
                        failure["error"].as_str().unwrap_or("an unknown error")
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        } else {
            Value::Null
        },
        "measured_when": "after the store was closed, from the installation's own .ledger directory, with each entry stat'ed but no symbolic link followed",
        "note": "Bytes on disk for this one installation after this one run. Not a storage requirement, a growth rate or a capacity statement."
    })
}

/// A point sample of this process's resident set, taken at the end of the run.
/// When the operating system will not answer, the report says so instead of
/// printing a number it did not measure.
fn memory_report() -> Value {
    let pid = std::process::id().to_string();
    let attempt = Process::new("ps").args(["-o", "rss=", "-p", &pid]).output();
    let (sampled, status) =
        match attempt {
            Ok(output) if output.status.success() => {
                let kib = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok();
                match kib {
                Some(kib) => (json!(kib * 1024), json!("sampled once at the end of the run")),
                None => (
                    Value::Null,
                    json!("unavailable: the ps command succeeded but printed no resident set size"),
                ),
            }
            }
            Ok(output) => (
                Value::Null,
                json!(format!(
                    "unavailable: ps exited with {} ({})",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                )),
            ),
            Err(error) => (
                Value::Null,
                json!(format!(
                    "unavailable: ps could not be started here: {error}"
                )),
            ),
        };
    json!({
        "rss_bytes_at_end_of_run": sampled,
        "rss_measurement_status": status,
        "rss_measured_by": "ps -o rss= -p <this pid>, read once after the store was closed",
        "peak_rss_bytes": Value::Null,
        "peak_unavailable_because": "No dependency-free peak or high-water measurement is available to this harness: a single end-of-run sample cannot see a transient peak before that point, and a real high-water mark would need a libc binding that this harness may not add.",
        "note": "One point sample of one process, or an explicit unavailable. Not a memory requirement, a bound, or a reservation."
    })
}

fn notes(
    mode: &str,
    stop: Option<&Stop>,
    submitted: usize,
    refused: usize,
    base: usize,
) -> Vec<String> {
    let mut notes = vec![
        "Counts, cutoffs and totals describe this run on this host only. They establish no capacity, saturation, reservation or completion guarantee.".to_owned(),
        "No retryable or unknown result was treated as an answer: the harness aborts and names either instead of continuing with an unverified count.".to_owned(),
        "Postponed capabilities are untouched: PostgreSQL product support, multi-host writers, resource proofs, hosted operation and payments.".to_owned(),
        "No admission ceiling is assumed, predicted or worked around. The requested work was submitted and the product's own answer was reported.".to_owned(),
    ];
    match stop {
        Some(stop) => {
            notes.push(format!(
                "INCOMPLETE: the product refused the {} submission for decision {} with {}. \
                 The requested {mode} workload was not completed, and nothing in this report is \
                 an acceptance result for it.",
                stop.kind, stop.index, stop.code
            ));
            notes.push(format!(
                "{refused} of the {submitted} submissions in this run were refused. The {base} \
                 accepted base submissions, the reconciliation above and the elapsed times are \
                 reported as measured, for this run only."
            ));
        }
        None => notes.push(format!(
            "Every one of the {submitted} submissions in the requested {mode} workload was \
             answered by the product. That is a measured run, not a qualified capacity claim."
        )),
    }
    notes
}

/// Tests for this harness's own reporting, not for the product. They exist
/// because a report that cannot describe its own failure is worse than no report:
/// a storage block that called itself `measured` while its main database was
/// absent would be a false measurement, and nothing in a qualification run would
/// notice.
///
/// Run exactly these:
///
/// ```text
/// cargo test -p ledgerlab --example m3_qualification --locked --offline
/// ```
#[cfg(test)]
mod tests {
    use super::*;

    /// An installation directory whose `.ledger` subdirectory the test arranges.
    /// The temporary directory guard is held for as long as the arrangement is
    /// read, so the scratch space goes away with the test.
    struct Installation {
        _root: tempfile::TempDir,
        path: PathBuf,
    }

    fn installation(name: &str) -> Installation {
        let root = tempfile::tempdir().expect("a private scratch directory");
        let path = root.path().join(name);
        fs::create_dir_all(path.join(".ledger")).expect("the .ledger directory");
        Installation { _root: root, path }
    }

    fn write(path: &Path, name: &str, bytes: &[u8]) {
        fs::write(path.join(".ledger").join(name), bytes).expect("a ledger artifact");
    }

    #[test]
    fn a_readable_directory_with_the_main_database_is_measured() {
        let installation = installation("present");
        let path = &installation.path;
        write(&path, "local.db", b"database");
        write(&path, "local.db-wal", b"wal");
        let report = storage_report(&path);
        assert_eq!(report["status"], "measured");
        assert_eq!(report["main_database"]["name"], "local.db");
        assert_eq!(report["main_database"]["bytes"], 8);
        assert_eq!(report["aggregate_ledger_artifact_bytes"], 11);
        assert_eq!(report["read_or_stat_failures"].as_array().unwrap().len(), 0);
        assert_eq!(report["unavailable_because"], Value::Null);
        assert_eq!(report["main_database_unavailable_because"], Value::Null);
    }

    #[test]
    fn a_missing_main_database_is_partial_with_the_reason_not_measured_with_a_null() {
        let installation = installation("absent");
        let path = &installation.path;
        write(&path, "local.db-wal", b"wal");
        let report = storage_report(&path);
        assert_eq!(report["status"], "partial");
        assert_eq!(report["main_database"], Value::Null);
        // The aggregate is withdrawn, not reported as a sum that quietly omits
        // the database it claims to add up.
        assert_eq!(report["aggregate_ledger_artifact_bytes"], Value::Null);
        let reason = report["main_database_unavailable_because"]
            .as_str()
            .expect("a reason for the absent main database");
        assert!(reason.contains("local.db"), "{reason}");
        let failures = report["read_or_stat_failures"].as_array().unwrap();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0]["name"], "local.db");
        assert!(report["unavailable_because"]
            .as_str()
            .expect("a reason for the partial measurement")
            .contains("local.db"));
        // The sidecar that was there is still reported, with its own size.
        assert_eq!(report["files"].as_array().unwrap().len(), 1);
        assert_eq!(report["files"][0]["name"], "local.db-wal");
        assert_eq!(report["files"][0]["bytes"], 3);
    }

    #[test]
    fn a_main_database_that_is_not_a_regular_file_is_partial_with_the_reason() {
        let installation = installation("not-a-file");
        let path = &installation.path;
        fs::create_dir(path.join(".ledger").join("local.db"))
            .expect("a directory named as the database");
        let report = storage_report(&path);
        assert_eq!(report["status"], "partial");
        assert_eq!(report["main_database"], Value::Null);
        assert_eq!(report["aggregate_ledger_artifact_bytes"], Value::Null);
        assert!(report["main_database_unavailable_because"]
            .as_str()
            .expect("a reason for the unusable main database")
            .contains("not a regular file"));
    }

    /// A symbolic link standing where the database should be is followed by
    /// `metadata` and not by `symlink_metadata`, so a report that used it would
    /// have called some other file this installation's database. This is the
    /// arrangement that must be rejected explicitly instead.
    #[cfg(unix)]
    #[test]
    fn a_main_database_that_is_a_symbolic_link_is_rejected_not_followed() {
        use std::os::unix::fs::symlink;

        let installation = installation("linked");
        let path = &installation.path;
        let elsewhere = installation.path.join("elsewhere.db");
        fs::write(&elsewhere, b"some other file entirely").expect("a file to point at");
        symlink(&elsewhere, path.join(".ledger").join("local.db")).expect("a link as the database");
        let report = storage_report(&path);
        assert_eq!(
            report["status"], "partial",
            "a linked database is not this installation's own measurement"
        );
        assert_eq!(
            report["main_database"],
            Value::Null,
            "the target's bytes must not be reported as the installation's database"
        );
        assert_eq!(report["aggregate_ledger_artifact_bytes"], Value::Null);
        let reason = report["main_database_unavailable_because"]
            .as_str()
            .expect("a reason for the linked main database");
        assert!(reason.contains("symbolic link"), "{reason}");
        let link = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["name"] == "local.db")
            .expect("the link is still listed");
        assert_eq!(link["is_symlink"], true);
        assert_eq!(link["is_file"], false);
        assert_eq!(
            link["bytes"],
            Value::Null,
            "a link's own length is not bytes on disk"
        );
    }

    /// The same rule applies to a sidecar: a link beside the database is not an
    /// artifact of this installation, so it contributes no bytes and withdraws
    /// the aggregate rather than adding the file it points at.
    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_beside_the_database_withdraws_the_aggregate() {
        use std::os::unix::fs::symlink;

        let installation = installation("linked-sidecar");
        let path = &installation.path;
        write(&path, "local.db", b"database");
        let elsewhere = installation.path.join("somewhere-else");
        fs::write(&elsewhere, b"not a sidecar of this installation").expect("a file to point at");
        symlink(&elsewhere, path.join(".ledger").join("local.db-wal")).expect("a linked sidecar");
        let report = storage_report(&path);
        assert_eq!(report["status"], "partial");
        assert_eq!(
            report["main_database"]["bytes"], 8,
            "the real database is still measured"
        );
        assert_eq!(
            report["aggregate_ledger_artifact_bytes"],
            Value::Null,
            "another file's bytes must not be added to this installation's total"
        );
        assert!(report["unavailable_because"]
            .as_str()
            .expect("a reason for the withdrawn aggregate")
            .contains("local.db-wal"));
    }

    #[test]
    fn a_directory_that_cannot_be_enumerated_is_unavailable_not_measured() {
        let root = tempfile::tempdir().expect("a private scratch directory");
        let report = storage_report(&root.path().join("no-such-installation"));
        assert_eq!(report["status"], "unavailable");
        assert_eq!(report["main_database"], Value::Null);
        assert_eq!(report["aggregate_ledger_artifact_bytes"], Value::Null);
        assert!(report["main_database_unavailable_because"]
            .as_str()
            .expect("a reason for the unavailable measurement")
            .contains("could not be read"));
    }
}
