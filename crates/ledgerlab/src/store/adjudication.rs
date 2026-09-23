//! R3 persistence boundary. These ports persist coordinator-validated plans;
//! adapters never decide economics, assent, proof validity, or resource promises.
//! No implementation is advertised by the interface-only review target.
use super::{
    errors::StoreError,
    outcomes::{OutcomeLock, OutcomeLockClass},
    ports::AcceptanceTx,
};
use crate::service::accept::adjudication::{
    CommitCapability, TrustedJournalHead, TrustedPrefix, ValidatedAdjudicationPlan, VerifiedSource,
};
use ledgerlab_core::adjudication::{
    commands as wire,
    reads::{PageAddress, PageFragment},
    types::{Count, Digest, Id},
};
use std::future::Future;
use tokio::time::Instant;

/// Explicit independent tag and acquisition rank: old persisted tags stay 0..8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum GuardClass {
    EnrollmentNamespace,
    CapacityAllocation,
    GatewayRound,
    FamilyPrerequisite,
    Case,
    Entitlement,
    SupplierPool,
    AdjustmentPool,
}
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Typed seam; production caller follows review")
)]
impl GuardClass {
    pub(crate) fn storage_tag(self) -> u16 {
        match self {
            Self::EnrollmentNamespace => 9,
            Self::CapacityAllocation => 10,
            Self::GatewayRound => 11,
            Self::FamilyPrerequisite => 12,
            Self::Case => 13,
            Self::Entitlement => 14,
            Self::SupplierPool => 15,
            Self::AdjustmentPool => 16,
        }
    }
    pub(crate) fn rank(self) -> u16 {
        match self {
            Self::EnrollmentNamespace => 1,
            Self::CapacityAllocation => 10,
            Self::GatewayRound => 11,
            Self::FamilyPrerequisite => 12,
            Self::Case => 13,
            Self::Entitlement => 14,
            Self::SupplierPool => 15,
            Self::AdjustmentPool => 16,
        }
    }
}
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Typed seam; production caller follows review")
)]
pub(crate) fn legacy_tag(class: OutcomeLockClass) -> u16 {
    match class {
        OutcomeLockClass::Admission => 0,
        OutcomeLockClass::Authority => 1,
        OutcomeLockClass::Binding => 2,
        OutcomeLockClass::Reservation => 3,
        OutcomeLockClass::Target => 4,
        OutcomeLockClass::Claim => 5,
        OutcomeLockClass::BindingAggregate => 6,
        OutcomeLockClass::InvocationConsumption => 7,
        OutcomeLockClass::BaseReversal => 8,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Typed seam; production caller follows review")
)]
pub(crate) enum Guard {
    /// Admission WRITE is required and participates in all legacy writer paths.
    Legacy(OutcomeLock),
    R3 {
        class: GuardClass,
        host: Id,
        key: Vec<u8>,
    },
}
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Typed seam; production caller follows review")
)]
impl Guard {
    pub(crate) fn order_key(&self) -> (u16, Vec<u8>) {
        match self {
            Self::Legacy(lock) => (
                if lock.class == OutcomeLockClass::Admission {
                    0
                } else {
                    legacy_tag(lock.class) + 1
                },
                lock.key.clone(),
            ),
            Self::R3 { class, host, key } => {
                let mut full = (host.as_str().len() as u16).to_be_bytes().to_vec();
                full.extend(host.as_str().as_bytes());
                full.extend(key);
                (class.rank(), full)
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JournalIdentity {
    pub store: Id,
    pub scope: wire::Scope,
    pub registration: Id,
    pub host: Id,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ResolveRequest {
    pub journal: JournalIdentity,
    pub key: wire::Delivery,
    pub guards: Vec<Guard>,
    /// Exact bounded point lookups, not a full history snapshot.
    pub objects: Vec<wire::Proof>,
    pub heads: Vec<HeadKey>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct HeadKey {
    pub journal: JournalIdentity,
    pub kind: HeadKind,
    pub full_key: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum HeadKind {
    Enrollment,
    Authority,
    Grant,
    GrantRegistry,
    Token,
    Allocation,
    Receipt,
    Round,
    Gateway,
    Family,
    Case,
    Entitlement,
    Supplier,
    Adjustment,
    Resource,
    Counter,
    VerifiedCursor,
    Delivery,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ObservedHead {
    pub key: HeadKey,
    pub revision: Option<Count>,
    pub value: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct HeadWrite {
    pub key: HeadKey,
    pub expected: Option<Count>,
    pub revision: Count,
    pub value: Vec<u8>,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct SavedOutcome {
    pub command: Vec<u8>,
    pub result: wire::CommandResult,
    pub prefix: wire::ExpectedPrefix,
    pub receipt: Option<wire::Receipt>,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct LockedInputs {
    pub journal: JournalIdentity,
    pub prefix: TrustedJournalHead,
    pub heads: Vec<ObservedHead>,
    pub retained: Vec<wire::RetainedObject>,
    pub sources: Vec<VerifiedSource>,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) enum Resolution {
    MoreLocks(Vec<Guard>),
    Missing(Vec<wire::Proof>),
    Complete(Box<LockedInputs>),
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct WorkRequest {
    pub journal: JournalIdentity,
    pub owner: Id,
    pub transition: Digest,
    pub mandatory: bool,
    pub maximum: wire::Resource,
}

#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) trait AdjudicationStore: Send + Sync {
    type Tx: AdjudicationTx;
    /// Claim the physical bounded work lane before materializing a command plan.
    /// Capability checks include all competing processes and pinned readers.
    fn begin_adjudication(
        &self,
        work: &WorkRequest,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Tx, StoreError>> + Send;
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) trait AdjudicationTx: AcceptanceTx {
    /// Acquire complete sorted guards once; discovery restarts the transaction.
    /// ENROLL scans occupancy AND claims future prefix ownership under Admission.
    fn lock_adjudication(
        &mut self,
        guards: &[Guard],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    /// Same authoritative host only. Occupied identity precedes semantic alias.
    fn lookup_adjudication(
        &mut self,
        journal: &JournalIdentity,
        key: &wire::Delivery,
    ) -> impl Future<Output = Result<Option<SavedOutcome>, StoreError>> + Send;
    fn resolve_adjudication(
        &mut self,
        request: &ResolveRequest,
    ) -> impl Future<Output = Result<Resolution, StoreError>> + Send;
    /// Nonserializable lease bound to this transaction, actual journal and epoch.
    fn commit_capability(
        &mut self,
    ) -> impl Future<Output = Result<CommitCapability, StoreError>> + Send;
    /// Reassert every observed head, capability and resource account before writes.
    /// Segment, exact bodies, original base, identities, indices, credits and
    /// intentions share this transaction; commit uses existing CommitError.
    fn append_adjudication(
        &mut self,
        plan: &ValidatedAdjudicationPlan,
        capability: &CommitCapability,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct SnapshotSelection {
    pub journal: JournalIdentity,
    pub historical: Option<wire::ExpectedPrefix>,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct IndexedPageRequest {
    pub address: PageAddress,
    pub offset: u16,
    pub max_bytes: u16,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ObjectPageRequest {
    pub origin: wire::ObjectOrigin,
    pub kind: wire::FactKind,
    pub key: Vec<u8>,
    pub hash: Digest,
    pub offset: Count,
    pub max_bytes: u16,
}
#[derive(Clone, Debug)]
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) struct ReadLease {
    pub id: Digest,
    pub retained_bytes: Count,
    pub workspace_bytes: Count,
    pub pages: Count,
}
/// No AcceptanceTx supertrait, business locks, write handle or commit method.
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) trait AdjudicationReadStore: Send + Sync {
    type Read: AdjudicationReadTx;
    fn begin_adjudication_read(
        &self,
        selection: &SnapshotSelection,
        budget: &wire::ReadBudget,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Read, StoreError>> + Send;
}
#[expect(
    dead_code,
    reason = "Typed seam review; production adapters/executor are the next assigned gate"
)]
pub(crate) trait AdjudicationReadTx: Send {
    /// Obtained independently from committed membership under the primary snapshot.
    fn expected_prefix(&self) -> &TrustedPrefix;
    fn lease(&self) -> &ReadLease;
    /// Indexed address, metadata preflight, at most 4096 bytes; old dependencies
    /// and semantic verification work charge the same request/session budget.
    fn segment_page(
        &mut self,
        request: &IndexedPageRequest,
    ) -> impl Future<Output = Result<PageFragment, StoreError>> + Send;
    fn object_page(
        &mut self,
        request: &ObjectPageRequest,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send;
    /// A bounded topology lookup for first unavailability, not N-case mutation.
    fn family_certificate(
        &mut self,
        family: &wire::Family,
        at: &wire::ExpectedPrefix,
    ) -> impl Future<Output = Result<Option<wire::Certificate>, StoreError>> + Send;
    /// Always releases snapshot/version/WAL retention; Drop/cancel do likewise.
    fn finish(self) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// Reject malformed lock discovery before any lock is acquired. R3 writers always
/// enter the SAME legacy Admission write gate; later groups use separate tags.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Typed seam; production caller follows review")
)]
pub(crate) fn validate_guards(guards: &[Guard]) -> Result<(), StoreError> {
    use super::outcomes::OutcomeLockMode;
    if guards.len() > 512
        || !matches!(
            guards.first(),
            Some(Guard::Legacy(OutcomeLock {
                class: OutcomeLockClass::Admission,
                mode: OutcomeLockMode::Write,
                ..
            }))
        )
        || guards
            .windows(2)
            .any(|p| p[0].order_key() >= p[1].order_key())
    {
        return Err(StoreError::Integrity(
            "R3 shared admission and ordered guards",
        ));
    }
    for guard in guards {
        match guard {
            Guard::Legacy(l) if l.key.is_empty() || l.key.len() > 4096 => {
                return Err(StoreError::Integrity("legacy lock key bound"))
            }
            Guard::R3 { key, .. }
                if key.is_empty() || key.len() > ledgerlab_core::adjudication::MAX_KEY_BYTES =>
            {
                return Err(StoreError::Integrity("R3 full lock key bound"))
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::outcomes::OutcomeLockMode;
    #[test]
    fn legacy_storage_tags_and_relative_order_are_unchanged() {
        let classes = [
            OutcomeLockClass::Admission,
            OutcomeLockClass::Authority,
            OutcomeLockClass::Binding,
            OutcomeLockClass::Reservation,
            OutcomeLockClass::Target,
            OutcomeLockClass::Claim,
            OutcomeLockClass::BindingAggregate,
            OutcomeLockClass::InvocationConsumption,
            OutcomeLockClass::BaseReversal,
        ];
        for (tag, class) in classes.into_iter().enumerate() {
            assert_eq!(legacy_tag(class), tag as u16);
            assert_eq!(class as u16, tag as u16);
        }
        assert_eq!(GuardClass::EnrollmentNamespace.storage_tag(), 9);
        assert_eq!(GuardClass::AdjustmentPool.storage_tag(), 16);
        let admission = Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Admission,
            key: b"[[\"t\",\"e\"]]".to_vec(),
            mode: OutcomeLockMode::Write,
        });
        let namespace = Guard::R3 {
            class: GuardClass::EnrollmentNamespace,
            host: Id::parse("center").unwrap(),
            key: b"full-key".to_vec(),
        };
        let authority = Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Authority,
            key: b"[[\"t\",\"e\"],\"authority\"]".to_vec(),
            mode: OutcomeLockMode::Read,
        });
        assert!(
            validate_guards(&[admission.clone(), namespace.clone(), authority.clone()]).is_ok()
        );
        assert!(validate_guards(&[namespace.clone(), authority.clone()]).is_err());
        assert!(validate_guards(&[admission.clone(), authority, namespace]).is_err());
        assert!(validate_guards(&[admission.clone(), admission]).is_err());
    }
}
