//! Sole construction boundary for future R3 validated plans. Wire DTOs and store
//! observations cannot construct these capabilities or bypass current host checks.
use crate::store::{adjudication::*, outcomes::ValidatedOutcomePlan, records::WriteOp};
use ledgerlab_core::adjudication::{
    commands as wire,
    types::{Count, Digest, Id},
    ParsedCommand,
};

/// Host-selected primary/historical observation. Not deserializable from a proof.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct TrustedPrefix {
    expected: wire::ExpectedPrefix,
    observation: Digest,
    selection: PrefixSelection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum PrefixSelection {
    CurrentAtRead,
    Historical,
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
impl TrustedPrefix {
    pub(crate) fn expected(&self) -> &wire::ExpectedPrefix {
        &self.expected
    }
    pub(crate) fn observation(&self) -> &Digest {
        &self.observation
    }
    pub(crate) fn selection(&self) -> PrefixSelection {
        self.selection
    }
}
/// Complete source membership and semantic reconstruction through a host-observed
/// historical prefix; a submitted hash/root never creates this witness.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct VerifiedSource {
    prefix: TrustedJournalHead,
    proof: wire::Proof,
    exact_object: wire::RetainedObject,
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
impl VerifiedSource {
    pub(crate) fn prefix(&self) -> &TrustedJournalHead {
        &self.prefix
    }
    pub(crate) fn proof(&self) -> &wire::Proof {
        &self.proof
    }
    pub(crate) fn object(&self) -> &wire::RetainedObject {
        &self.exact_object
    }
}
/// A non-Clone, non-wire lease minted by the trusted backend boundary while its
/// actual exclusion and protected work allocation are held. Fields are private.
/// Possessing a signing key, a path or an epoch DTO cannot manufacture this lease.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct CommitCapability {
    journal: JournalIdentity,
    transaction: Digest,
    storage_incarnation: Digest,
    writer_epoch: Count,
    recovered_through: TrustedJournalHead,
    allocation_owner: Id,
    physical: PhysicalEnvelope,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct PhysicalEnvelope {
    maximum_retained_bytes: Count,
    maximum_index_pages: Count,
    maximum_wal_bytes: Count,
    maximum_staging_bytes: Count,
    maximum_reader_retention_bytes: Count,
    protected_workspace_bytes: Count,
    enforcement_observation: Digest,
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
impl CommitCapability {
    pub(crate) fn journal(&self) -> &JournalIdentity {
        &self.journal
    }
    pub(crate) fn epoch(&self) -> Count {
        self.writer_epoch
    }
    pub(crate) fn transaction(&self) -> &Digest {
        &self.transaction
    }
    pub(crate) fn recovered_through(&self) -> &TrustedJournalHead {
        &self.recovered_through
    }
    pub(crate) fn storage_incarnation(&self) -> &Digest {
        &self.storage_incarnation
    }
    pub(crate) fn allocation_owner(&self) -> &Id {
        &self.allocation_owner
    }
    pub(crate) fn physical(&self) -> &PhysicalEnvelope {
        &self.physical
    }
}
/// The actual pre-fence final head must equal the replacement's recovered head,
/// including exported/unactivated grants. No independent writable copy is valid.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ReplacementEvidence {
    excluded_writer_epoch: Count,
    actual_commit_boundary_fence: Digest,
    final_authoritative_prefix: TrustedPrefix,
    recovered_prefix: TrustedPrefix,
    replacement_capability: CommitCapability,
}
/// Original profile evaluator output and its exact concrete write projection.
/// Only the acceptance coordinator builds this after original-profile validation.
/// An already accepted receipt or a bare base digest cannot fill this slot.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum OriginalBaseWrites {
    V1 {
        decision: Box<ledgerlab_core::domain::DecisionPlan>,
        writes: Vec<WriteOp>,
    },
    V2(Box<ValidatedOutcomePlan>),
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct FreshBaseAcceptance {
    writes: OriginalBaseWrites,
    absence: Vec<ObservedHead>,
    manifest: Digest,
    receipt: Digest,
    exact_members: Vec<wire::RetainedObject>,
}
/// Names an immutable allocation and exact one-time transition; unknown outcome
/// holds staging and credits until same-identity authoritative resolution.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ReservationConversion {
    pub owner: Id,
    pub slot: Id,
    pub discharged: wire::Resource,
    pub actual: wire::Resource,
    pub counter_reserved: wire::Counters,
    pub counter_actual: wire::Counters,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct IndexChange {
    pub full_key: Vec<u8>,
    pub old_root: Digest,
    pub new_root: Digest,
    pub retained_pages: Vec<Vec<u8>>,
}
/// All fields private. There is deliberately no public constructor, Deserialize,
/// unchecked build function or raw-action append. This module owns future build.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ValidatedAdjudicationPlan {
    journal: JournalIdentity,
    command: ParsedCommand,
    prior: TrustedJournalHead,
    segment: wire::Segment,
    exact_segment: Vec<u8>,
    sources: Vec<VerifiedSource>,
    guards: Vec<Guard>,
    observed: Vec<ObservedHead>,
    writes: Vec<HeadWrite>,
    resources: Vec<ReservationConversion>,
    indices: Vec<IndexChange>,
    base: Option<Box<FreshBaseAcceptance>>,
    /// At most one bounded certificate; no per-pending-case transfer writes.
    closure: Option<wire::Certificate>,
    held_intentions: Vec<wire::Action>,
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
impl ValidatedAdjudicationPlan {
    pub(crate) fn journal(&self) -> &JournalIdentity {
        &self.journal
    }
    pub(crate) fn command(&self) -> &ParsedCommand {
        &self.command
    }
    pub(crate) fn prior(&self) -> &TrustedJournalHead {
        &self.prior
    }
    pub(crate) fn segment(&self) -> &wire::Segment {
        &self.segment
    }
    pub(crate) fn segment_bytes(&self) -> &[u8] {
        &self.exact_segment
    }
    pub(crate) fn sources(&self) -> &[VerifiedSource] {
        &self.sources
    }
    pub(crate) fn guards(&self) -> &[Guard] {
        &self.guards
    }
    pub(crate) fn observed(&self) -> &[ObservedHead] {
        &self.observed
    }
    pub(crate) fn head_writes(&self) -> &[HeadWrite] {
        &self.writes
    }
    pub(crate) fn conversions(&self) -> &[ReservationConversion] {
        &self.resources
    }
    pub(crate) fn indices(&self) -> &[IndexChange] {
        &self.indices
    }
    pub(crate) fn base(&self) -> Option<&FreshBaseAcceptance> {
        self.base.as_deref()
    }
    pub(crate) fn closure(&self) -> Option<&wire::Certificate> {
        self.closure.as_ref()
    }
    pub(crate) fn held_intentions(&self) -> &[wire::Action] {
        &self.held_intentions
    }
}
/// Semantic replay state is owned by this coordinator session, not reconstructed
/// from a caller cursor's claimed verified_root. One live successor per session.
/// Incomplete comparison has no comparable total; abort releases the read lease.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct VerifiedReadContinuation {
    selected: TrustedPrefix,
    cursor: wire::ReadCursor,
    source_cursors: Vec<TrustedPrefix>,
    lease: ReadLease,
    verified_state_digest: Digest,
    remaining_budget: wire::ReadBudget,
}

/// Current trusted host resolution supplies exact authority source bodies as well
/// as the current head; neither sender flags nor membership in a digest set suffice.
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct AuthorityObservation {
    principal: Id,
    command: Digest,
    document: Digest,
    revision: Count,
    observed_at: ledgerlab_core::adjudication::types::Time,
    permission: wire::AuthorityPermission,
    exact_sources: Vec<wire::AuthoritySource>,
    current_heads: Vec<ObservedHead>,
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) trait AdjudicationAuthority: Send + Sync {
    fn current(
        &self,
        command: &ParsedCommand,
        inputs: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, crate::ServiceError>;
}
#[derive(Clone, Copy, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum AuthorityAccess {
    ReadSavedResult,
    NewTransition,
}

#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
impl FreshBaseAcceptance {
    pub(crate) fn writes(&self) -> &OriginalBaseWrites {
        &self.writes
    }
    pub(crate) fn absence(&self) -> &[ObservedHead] {
        &self.absence
    }
    pub(crate) fn manifest(&self) -> &Digest {
        &self.manifest
    }
    pub(crate) fn receipt(&self) -> &Digest {
        &self.receipt
    }
    pub(crate) fn exact_members(&self) -> &[wire::RetainedObject] {
        &self.exact_members
    }
}

mod backend;
pub(crate) use backend::TrustedJournalHead;
