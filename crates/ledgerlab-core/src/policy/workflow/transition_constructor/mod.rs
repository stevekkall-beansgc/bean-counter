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
    AuthorityContextData, CapacityHead, DecisionHead, DraftContextData, DraftEnvelope,
    DraftStateData, DraftTransition, EconomicRecordView, EconomicStateData, ExactRef, FamilyHead,
    OperationKey, ProposedHeads, RecordInput, SavedFinancialInput,
    SupplierStateData,AppStateData,InvocationHead,CostHead,CostOwnerHead,
    PartyAuthorizationHead, EconomicObservationInput,
};
mod authority;
mod body;
mod construct;
mod correction;
mod close;
mod cost;
mod app_cost;
mod cost_postings;
mod source_authority;
mod source_projection;
pub use source_projection::{NominatedSourceFrame,NominatedSourceGrant,validate_app_nominated_sources};
mod financial_observation;
pub use financial_observation::audit_stream::{EconomicAuditStream,OriginWitness,RecomputedAuditDigest};
mod funding;
mod family_context;
mod intent;
pub use intent::normalize_saved_intent;
mod emitted_record;
pub(in crate::policy::workflow) mod original_financial;
mod outcome;
mod outcome_proofs;
pub(in crate::policy::workflow) mod owned_facts;
mod pricing;
mod record_body;
mod working_value;
mod record_index;
mod records;
mod root_correction;
pub(in crate::policy::workflow) mod shape;
mod targets;
pub use construct::{draft_transition, replay_draft};
#[cfg(test)]
mod first_work_tests;

#[cfg(test)]
mod repair_tests;
mod representation;
mod retention;
pub use retention::{retain_attestation,RetainedAssertionData,RetainedAssertionEnvelope};

#[cfg(test)]
mod calibration_tests;
mod inventory;
pub use inventory::slots::validate_app_slot_projection;

// Per-kind immutable DATA; never a host completed-origin capability.
pub(in crate::policy::workflow) mod intrinsic_record;

mod candidate;
mod capacity;
pub use candidate::{
    CandidateEnvelope, ClaimEffect, CustomerLocalFundingEffect, DependencyEffect, DependencyKind,
    EconomicCandidate, FinancialHeadKey, HeadEffect, LinkEffect, PermanentClaimKey,
    CandidateResultKind, CandidateResultMetadata, ObservationAssociation,
    CostLineKey,ProviderAccountIdentity,AccountEnrollmentEffect,
};

mod economic;
pub use economic::{construct_economic_candidate,reconstruct_original_candidate};
