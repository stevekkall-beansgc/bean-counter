//! Private Phase 3 transaction seam. Values are observations, never authorization.
//!
//! The adapter must hold the supplied total-order locks, return complete retained
//! envelopes and assert every observed head at append, including unchanged heads.
#![allow(dead_code)] // Concrete adapter implementations are owned by parallel lanes.
use super::{errors::StoreError, ports::AcceptanceTx};
pub(crate) use crate::service::accept::outcome::ValidatedOutcomePlan;
use std::future::Future;
use tokio::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScopedDelivery {
    pub scope: [String; 2],
    pub source: String,
    pub external_id: String,
}

/// Declaration order is the global lock order. Key bytes are canonical JSON of
/// [scope, ...structured key components], compared as UTF-8 bytes within a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OutcomeLockClass {
    Admission,
    Authority,
    Binding,
    Reservation,
    Target,
    Claim,
    BindingAggregate,
    InvocationConsumption,
    BaseReversal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OutcomeLockMode {
    Read,
    Write,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OutcomeLock {
    pub class: OutcomeLockClass,
    pub key: Vec<u8>,
    pub mode: OutcomeLockMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopedRecordRef {
    pub scope: [String; 2],
    pub kind: String,
    /// Canonical JSON: immutable IDs include structured array keys.
    pub id: Vec<u8>,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredCompositeDelivery {
    pub key: ScopedDelivery,
    pub canonical_key: ScopedDelivery,
    pub command: Vec<u8>,
    pub ingress: Vec<u8>,
    pub ingress_hash: String,
    /// Original full canonical envelope (base-acceptance or receipt).
    pub economic_receipt: Option<Vec<u8>>,
    /// Original full canonical reservation-receipt envelope; mandatory.
    pub settlement_receipt: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutcomeResolve {
    pub delivery: ScopedDelivery,
    pub target: String,
    pub invocation_id: String,
    /// Permanent [scope, agreement, family, target], absent for base/closure.
    pub family_key: Option<Vec<u8>>,
    pub required: Vec<ScopedRecordRef>,
    pub locks: Vec<OutcomeLock>,
}
#[derive(Clone, Debug)]
pub(crate) enum OutcomeResolution {
    MoreLocks(Vec<OutcomeLock>),
    Missing(Vec<ScopedRecordRef>),
    Complete(OutcomeSnapshot),
}

/// None means locked absence. Revision is a canonical unsigned decimal string.
/// Value is the complete canonical operational head, not a selected projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedOutcomeHead {
    pub lock: OutcomeLock,
    pub revision: Option<String>,
    pub value: Option<Vec<u8>>,
}
pub(crate) type ObservedOutcomeHeads = Vec<ObservedOutcomeHead>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OutcomeHeadWrite {
    pub lock: OutcomeLock,
    pub revision: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct OutcomeSnapshot {
    /// Externally retained roots read under target/reservation locks. The
    /// coordinator checks these against head/index values and complete members.
    pub anchors: Vec<ScopedRecordRef>,
    /// Complete v2 closure, settlement prefix and current authority documents.
    /// Every item is one exact canonical envelope; no truncation or filtering.
    pub records: Vec<Vec<u8>>,
    pub heads: ObservedOutcomeHeads,
}

/// Additive capability: existing first-slice stores need not advertise outcome
/// support until the concrete adapter implements this complete protocol.
pub(crate) trait OutcomeStore: Send + Sync {
    type Tx: OutcomeTx;
    fn begin_outcome(
        &self,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Tx, StoreError>> + Send;
}
pub(crate) trait OutcomeTx: AcceptanceTx {
    fn lock_scopes(
        &mut self,
        scopes: &[OutcomeLock],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
    fn lookup_outcome_delivery(
        &mut self,
        key: &ScopedDelivery,
    ) -> impl Future<Output = Result<Option<StoredCompositeDelivery>, StoreError>> + Send;
    fn resolve_outcome(
        &mut self,
        request: &OutcomeResolve,
    ) -> impl Future<Output = Result<OutcomeResolution, StoreError>> + Send;
    fn append_outcome(
        &mut self,
        plan: &ValidatedOutcomePlan,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
