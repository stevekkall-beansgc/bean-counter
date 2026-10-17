//! Pure selected draft/replay foundation. No output is a financial commit,
//! authenticated host capability or future-correction capacity certificate.
pub(in crate::policy::workflow) mod arithmetic;
pub(in crate::policy::workflow) mod codec;
pub(in crate::policy::workflow) mod meter;
pub use meter::{Charge, Meter as CoreMeter, Phase, Usage as MeterUsage};
mod data;
#[cfg(test)]
mod tests;
pub use data::{
    AppStateData, AuthorityContextData, CapacityHead, CostHead, CostOwnerHead, DecisionHead,
    DraftContextData, DraftEnvelope, DraftStateData, DraftTransition, EconomicObservationInput,
    EconomicRecordView, EconomicStateData, ExactRef, FamilyHead, InvocationHead, OperationKey,
    PartyAuthorizationHead, ProposedHeads, RecordInput, SavedFinancialInput, SupplierStateData,
};
mod authority;
mod body;
mod construct;
mod correction;
mod financial_observation;
mod funding;
pub(in crate::policy::workflow) mod original_financial;
mod outcome;
mod outcome_proofs;
mod pricing;
mod record_index;
mod records;
mod root_correction;
mod shape;
mod targets;
pub use construct::{draft_transition, replay_draft};
#[cfg(test)]
mod first_work_tests;

mod intent;
#[cfg(test)]
mod repair_tests;
mod representation;
pub use intent::normalize_saved_intent;
pub(in crate::policy::workflow) mod owned_facts;

#[cfg(test)]
mod calibration_tests;
mod inventory;
pub use inventory::slots::validate_app_slot_projection;

// Per-kind immutable DATA; never a host completed-origin capability.
pub(in crate::policy::workflow) mod intrinsic_record;

mod candidate;
pub use candidate::{
    AccountEnrollmentEffect, CandidateEnvelope, CandidateResultKind, CandidateResultMetadata,
    ClaimEffect, CostLineKey, CustomerLocalFundingEffect, DependencyEffect, DependencyKind,
    EconomicCandidate, FinancialHeadKey, HeadEffect, LinkEffect, ObservationAssociation,
    PermanentClaimKey, ProviderAccountIdentity,
};
