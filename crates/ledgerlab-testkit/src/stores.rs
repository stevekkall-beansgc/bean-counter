//! Integration seam owned by the testkit; not a proposed production store port.
use crate::failpoints::{CancellationPoint, Hit, Injection};
use crate::history::Snapshot;
use crate::{FixtureOracle, Result};

pub const RECEIVED_AT: &str = "2026-09-20T14:00:00.000000Z";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    FileSqlite,
    Postgres18,
    Postgres17,
}

#[derive(Clone, Debug)]
pub struct BackendEvidence {
    pub kind: BackendKind,
    /// Local file path or redacted database identifier; never a credentialed URL.
    pub location: String,
    pub engine_version: String,
    /// SQLite sqlite3_libversion_number(), or PostgreSQL server_version_num.
    pub version_number: u32,
    pub durability: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    DemoApp,
    NoSubmitPermission,
    NoReadPermission,
}

#[derive(Clone, Debug)]
pub struct Command {
    pub bytes: Vec<u8>,
    pub principal: Principal,
    pub received_at: &'static str,
}
impl Command {
    pub fn fixture(oracle: &FixtureOracle, name: &str) -> Result<Self> {
        Ok(Self {
            bytes: oracle.input(name)?.to_vec(),
            principal: Principal::DemoApp,
            received_at: RECEIVED_AT,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateKind {
    Identity,
    Semantic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    Identity,
    Semantic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    InvalidInput,
    Unauthorized,
    TermsNotAccepted,
    EvaluationInvalid,
    ArithmeticOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted(Vec<u8>),
    Duplicate {
        kind: DuplicateKind,
        receipt: Vec<u8>,
    },
    Conflict(ConflictKind),
    Rejected(Rejection),
    RolledBack,
    ResponseLost,
    /// Original scoped identity; absence during active commit is NOT rollback.
    OutcomeUnknown {
        scope: [String; 2],
        source: String,
        external_id: String,
    },
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct Attempt {
    pub outcome: Outcome,
    pub hit: Option<Hit>,
}

#[derive(Clone, Debug)]
pub struct PoolProbe {
    pub affected_connection: String,
    pub next_connection: String,
    pub affected_discarded: bool,
    /// Obtained from a driver query/transaction probe, never hardcoded.
    pub next_has_open_transaction: bool,
}

#[derive(Clone, Debug)]
pub struct ActiveCommitProbe {
    pub primary_rows_absent: bool,
    pub original_transaction_active: bool,
    pub outcome: Outcome,
}

#[derive(Clone, Debug)]
pub struct ZeroEvidence {
    pub event_id: String,
    pub receipt: Vec<u8>,
    pub explanation_codes: Vec<String>,
    pub action_ids: Vec<String>,
    pub intention_ids: Vec<String>,
    pub revision: String,
    pub event_count: String,
}

/// Implement in the integration lane. Each call creates a fresh isolated REAL
/// backend, seeds only the frozen initializer, and precreates all scope locks.
/// The adapter may own a runtime; the production facade must not nest one.
pub trait BackendFactory {
    type Backend: AcceptanceBackend;
    fn seeded(&self, oracle: &FixtureOracle) -> Result<Self::Backend>;
    /// Real-mode setup with no accepted real binding; demo assent must not suffice.
    fn real_without_terms(&self, oracle: &FixtureOracle) -> Result<Self::Backend>;
}

pub trait AcceptanceBackend {
    fn evidence(&self) -> BackendEvidence;
    fn observe(&mut self) -> Result<Snapshot>;
    /// Close ALL connections/owners and open the same durable store again.
    fn reopen(&mut self) -> Result<()>;
    fn accept(&mut self, command: &Command, injection: Option<&Injection>) -> Result<Attempt>;
    /// Resolve on the authoritative primary under original identity/locks, then
    /// retry the SAME bytes. Ends only after complete commit or an explicit error.
    fn resolve_and_retry(&mut self, command: &Command) -> Result<Attempt>;
    /// Test-controlled live transaction plus primary lookup, before releasing the
    /// barrier. resolve_and_retry releases/drains it under a bounded deadline.
    fn probe_active_commit(&mut self, command: &Command) -> Result<ActiveCommitProbe>;
    fn pool_probe(&mut self) -> Result<PoolProbe>;
    /// Complete instrumented await catalogue, including error/rollback paths.
    fn cancellation_points(&mut self) -> Result<Vec<CancellationPoint>>;
    fn cancel(&mut self, command: &Command, point: &CancellationPoint) -> Result<Attempt>;
    /// Reads stored zero-result projections, never evaluates or supplies expectations.
    fn zero_evidence(&mut self) -> Result<ZeroEvidence>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceKind {
    RequestStarted,
    TransactionOpened,
    LockHeld,
    LockBlocked,
    BeginBlocked,
    Retried,
    CommitAcknowledged,
    RequestFinished,
}
#[derive(Clone, Debug)]
pub struct TraceEvent {
    pub sequence: u64,
    pub request: usize,
    pub connection: String,
    pub kind: TraceKind,
}
#[derive(Clone, Debug)]
pub struct RaceEvidence {
    pub attempts: Vec<Attempt>,
    pub trace: Vec<TraceEvent>,
}

pub trait RaceBackend: AcceptanceBackend {
    /// Barrier-controlled overlap; bounded deadline, no sleep-only evidence.
    fn concurrent_identical(
        &mut self,
        command: &Command,
        participants: usize,
    ) -> Result<RaceEvidence>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    Held,
    Unknown,
    Delivered,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteReceipt {
    pub destination: String,
    pub key: String,
    pub request_hash: String,
    pub payload: Vec<u8>,
    pub amount_atoms: String,
    pub receipt: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct DeliveryEvidence {
    pub state: DeliveryState,
    pub intention_id: String,
    pub remote: Vec<RemoteReceipt>,
    /// Evidence from a durable simulator namespace, outside local transaction rollback.
    pub destination_location: String,
    pub lost_response_observed: bool,
}

pub trait DeliveryBackend: AcceptanceBackend {
    fn enable_fake_and_lose_response(&mut self) -> Result<DeliveryEvidence>;
    fn reopen_destination(&mut self) -> Result<()>;
    fn reconcile_fake(&mut self) -> Result<DeliveryEvidence>;
    fn read_fake(&mut self) -> Result<DeliveryEvidence>;
}
