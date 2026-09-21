//! Bounded fake-only delivery and recovery. No economic evaluation occurs here.
//! The embedding host supplies trusted UTC microseconds and owns scheduling.
//! Reads use keyset pages capped at 64 intentions / 8 MiB and an independent in-memory fake;
//! it is not a backup tool or a process-durable destination implementation.
pub mod fake;
#[cfg(test)]
pub(crate) mod tests;
mod workflow;

use crate::store::records::Installation;
use ledgerlab_core::canonical::{self, CanonicalBytes, Domain};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Held,
    Owned,
    Fenced,
    StaleReport,
    NeedsReview,
    InvalidInput,
    ScanLimit,
    Retryable,
    OutcomeUnknown,
    Unavailable,
    Integrity,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Held,
    Pending,
    Leased,
    Delivered,
    Retry,
    Unknown,
    Rejected,
}
impl State {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Delivered => "delivered",
            Self::Retry => "retry",
            Self::Unknown => "unknown",
            Self::Rejected => "rejected",
        }
    }
    pub(crate) fn parse(s: &str) -> std::result::Result<Self, crate::store::errors::StoreError> {
        match s {
            "held" => Ok(Self::Held),
            "pending" => Ok(Self::Pending),
            "leased" => Ok(Self::Leased),
            "delivered" => Ok(Self::Delivered),
            "retry" => Ok(Self::Retry),
            "unknown" => Ok(Self::Unknown),
            "rejected" => Ok(Self::Rejected),
            _ => Err(crate::store::errors::StoreError::Integrity(
                "delivery state",
            )),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Delivery {
    pub intention_id: String,
    pub state: State,
    pub attempts: i64,
    pub next_attempt_us: i64,
    pub(crate) owner: Option<String>,
    pub(crate) generation: i64,
    pub(crate) until: Option<i64>,
    pub last_observation: Option<String>,
    /// Permanent operator isolation; never satisfies a delivery dependency.
    pub quarantine: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Lease {
    pub(crate) store_id: String,
    pub(crate) owner: String,
    pub(crate) generation: i64,
    pub(crate) restore_generation: i64,
}
#[derive(Clone, Debug)]
pub struct Attempt {
    pub(crate) lease: Lease,
    pub(crate) number: i64,
    pub(crate) until: i64,
    pub(crate) request: Request,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub store_id: String,
    pub key: String,
    pub request_hash: String,
    pub payload: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub digest: String,
    pub unresolved: Vec<String>,
    pub orphan_keys: Vec<String>,
    /// Lists above are bounded samples; these counts cover the entire inventory.
    pub unresolved_count: i64,
    pub orphan_count: i64,
    pub intention_count: i64,
}
#[derive(Clone, Debug)]
pub(crate) struct Intention {
    pub id: String,
    pub destination: String,
    pub key: String,
    pub bytes: Vec<u8>,
    pub hash: String,
}
#[derive(Clone, Debug)]
pub(crate) struct Head {
    pub owner: Option<String>,
    pub generation: i64,
    pub until: Option<i64>,
    pub enabled: bool,
    pub revision: i64,
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub installation: Installation,
    pub head: Head,
    pub items: Vec<(Intention, Delivery)>,
    pub report: Option<(String, Vec<u8>)>,
}
pub(crate) const PAGE_SIZE: usize = 64;
#[derive(Clone, Debug)]
pub(crate) enum Query {
    Control,
    Page { after: String, due: Option<i64> },
    Key(String),
}
impl Query {
    pub(crate) fn page(after: &str) -> Self {
        Self::Page {
            after: after.into(),
            due: None,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Sweep {
    Leases,
    Expired(i64),
    Restore,
}
#[derive(Clone, Debug)]
pub(crate) struct Mutation {
    pub snapshot: Snapshot,
    pub sweep: Option<Sweep>,
    pub quarantine: Option<(String, String, Vec<u8>)>,
    pub attempt: Option<(String, i64, i64, String, Vec<u8>)>,
    pub observation: Vec<u8>,
    pub report: Option<(String, Vec<u8>)>,
}
fn bytes(v: &Value) -> Result<Vec<u8>> {
    CanonicalBytes::from_value(v)
        .map(|b| b.into_vec())
        .map_err(|_| Error::Integrity)
}
fn digest(v: &Value) -> Result<String> {
    // Tagged operational record; never a new economic canonical-record variant.
    canonical::digest(Domain::RecordContent, &json!(["outbox-operational", 1, v]))
        .map_err(|_| Error::Integrity)
}
impl Intention {
    fn request(&self, snapshot: &Snapshot) -> Result<(Request, Vec<String>)> {
        let v = canonical::parse_bounded(&self.bytes, canonical::BUNDLE_LIMIT)
            .map_err(|_| Error::Integrity)?;
        let scope = &snapshot.installation.scope;
        if bytes(&v)? != self.bytes
            || canonical::digest(Domain::RecordContent, &json!(["intention", 1, v]))
                .map_err(|_| Error::Integrity)?
                != self.hash
            || v["id"] != self.id
            || v["idempotency_key"] != self.key
            || self.key != self.id
            || self.destination != "fake"
            || v["destination_id"] != self.destination
            || v["scope"] != json!([scope.tenant, scope.environment])
            || v["schema"] != "ledger-intention/1"
        {
            return Err(Error::Integrity);
        }
        let dependencies: Vec<String> =
            serde_json::from_value(v["depends_on"].clone()).map_err(|_| Error::Integrity)?;
        let payload = v.get("payload").ok_or(Error::Integrity)?;
        Ok((
            Request {
                store_id: snapshot.installation.logical_store_id.clone(),
                key: self.key.clone(),
                request_hash: canonical::digest(Domain::IntentionPayload, payload)
                    .map_err(|_| Error::Integrity)?,
                payload: bytes(payload)?,
            },
            dependencies,
        ))
    }
}
fn plus(v: i64, n: i64) -> Result<i64> {
    v.checked_add(n).ok_or(Error::InvalidInput)
}
fn retry_delay(attempt: i64) -> i64 {
    (1_000_000_i64 << (attempt - 1).clamp(0, 9)).min(300_000_000)
}
fn fingerprint_start(s: &Snapshot) -> Result<String> {
    digest(&json!([
        "reconciliation-v2",
        s.installation.logical_store_id,
        s.installation.generation.to_string(),
        s.head.generation.to_string()
    ]))
}
fn fingerprint_item(previous: &str, i: &Intention, d: &Delivery) -> Result<String> {
    digest(&json!([
        previous,
        i.id,
        i.hash,
        d.state.name(),
        d.attempts.to_string(),
        d.next_attempt_us.to_string(),
        d.last_observation,
        d.quarantine
    ]))
}
/// Rejection is a permanent fact, independent of subsequent inventory availability.
/// Delivered mappings are rechecked after restore; quarantine is a separate permanent veto.
fn reconciled_state(d: &Delivery, request: &Request, outcome: &fake::Outcome) -> State {
    if d.state == State::Rejected {
        return State::Rejected;
    }
    match outcome {
        fake::Outcome::Delivered(r) if &r.request == request => State::Delivered,
        fake::Outcome::Delivered(_) | fake::Outcome::Rejected => State::Rejected,
        fake::Outcome::Absent if d.attempts >= 20 => State::Rejected,
        fake::Outcome::Absent => State::Pending,
        fake::Outcome::Unknown | fake::Outcome::Fenced => State::Unknown,
    }
}
pub use workflow::Outbox;

impl Request {
    pub(crate) fn value(&self) -> Value {
        json!({"store_id":self.store_id,"key":self.key,"request_hash":self.request_hash,"payload_hex":canonical::hex(&self.payload)})
    }
}
