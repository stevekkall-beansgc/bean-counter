//! Private SELECT-only comparison port. No transaction supertrait or write handle.
//! Implementations pin one committed snapshot, preflight lengths/counts before
//! fetching bodies, and rollback/discard on finish, cancellation, error or Drop.
#![allow(dead_code)]
use super::outcomes::{ObservedOutcomeHeads, ScopedRecordRef, StoredCompositeDelivery};
use std::future::Future;
use tokio::time::Instant;

pub(crate) const MAX_RECORDS: usize = 4096;
pub(crate) const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_ENVELOPE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_REFERENCES: usize = 1024;
pub(crate) const MAX_HEADS: usize = 128;
pub(crate) const MAX_STEPS: usize = 999;
pub(crate) const MAX_CANDIDATE_BYTES: usize = 256 * 1024;
pub(crate) const MAX_CANDIDATES_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnapshotFingerprint(pub String);
#[derive(Clone, Debug)]
pub(crate) struct RetainedSelection {
    pub scope: [String; 2],
    pub target: String,
    pub invocation_id: String,
    pub expected_snapshot: Option<SnapshotFingerprint>,
}
/// Supplied by the trusted local embedding host, never parsed from a candidate.
#[derive(Clone, Debug)]
pub(crate) struct AuthenticatedReadContext {
    pub scope: [String; 2],
    pub principal_id: String,
    pub authority_head: String,
}
/// Current scoped observation from the SAME read snapshot as the retained rows.
/// No bool here claims that a permission has been verified.
#[derive(Clone, Debug)]
pub(crate) struct ReadAuthorityObservation {
    pub scope: [String; 2],
    pub heads: ObservedOutcomeHeads,
    pub records: Vec<Vec<u8>>,
}
/// Store identity is trusted host configuration, stable across reconnects; it is
/// not a path, connection string or credential. No candidate may choose it.
#[derive(Clone, Debug)]
pub(crate) struct RawRetainedSnapshot {
    pub store_identity: String,
    pub anchors: Vec<ScopedRecordRef>,
    /// Independently selected membership index, including hashes; no inner-join
    /// disappearance. Exactly one member for every retained envelope.
    pub members: Vec<ScopedRecordRef>,
    pub records: Vec<Vec<u8>>,
    /// Complete target/reservation/consumption/reversal, binding/aggregate and
    /// claim observations, including absent unclaimed families. Lock metadata
    /// identifies existing rows only; this port cannot acquire outcome locks.
    pub heads: ObservedOutcomeHeads,
    /// Exactly the original mapping for every reservation receipt. Aliases are
    /// not extra activity, and this loader never creates or loads alias rows.
    pub original_deliveries: Vec<StoredCompositeDelivery>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadError {
    Unavailable,
    Deadline,
    Cancelled,
    Limit,
    Integrity,
    NotFound,
}
/// Before allocation, apply to SQL COUNT/SUM/MAX results in the pinned snapshot.
/// Reader implementations must additionally bound references, heads and receipt
/// mappings and charge their bytes against the same aggregate budget.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReadBudget {
    bytes: usize,
}
impl ReadBudget {
    pub fn preflight_records(count: u64, bytes: u64, largest: u64) -> Result<(), ReadError> {
        if count > MAX_RECORDS as u64
            || bytes > MAX_RETAINED_BYTES as u64
            || largest > MAX_ENVELOPE_BYTES as u64
        {
            return Err(ReadError::Limit);
        }
        Ok(())
    }
    pub fn charge(&mut self, bytes: usize) -> Result<(), ReadError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= MAX_RETAINED_BYTES)
            .ok_or(ReadError::Limit)?;
        Ok(())
    }
}
pub(crate) trait ComparisonReadStore: Send + Sync {
    type Read: ComparisonReadTx;
    fn begin_read(
        &self,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Read, ReadError>> + Send;
}
pub(crate) trait ComparisonReadTx: Send {
    fn load_authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> impl Future<Output = Result<ReadAuthorityObservation, ReadError>> + Send;
    fn load_retained(
        &mut self,
        selection: &RetainedSelection,
    ) -> impl Future<Output = Result<RawRetainedSnapshot, ReadError>> + Send;
    /// Always ends the read snapshot. On uncertain cleanup discard the session.
    /// An implementation's Drop must also rollback/discard, including cancelled
    /// begin/finish futures. There is intentionally no commit method.
    fn finish(self) -> impl Future<Output = Result<(), ReadError>> + Send;
}
