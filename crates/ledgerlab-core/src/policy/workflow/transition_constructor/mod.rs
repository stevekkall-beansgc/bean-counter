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
    AuthorityContextData, DecisionHead, DraftContextData, DraftEnvelope, DraftStateData,
    DraftTransition, EconomicRecordView, ExactRef, FamilyHead, OperationKey, ProposedHeads,
    RecordInput, SavedFinancialInput,
};
mod authority;
mod body;
mod construct;
mod correction;
mod financial_observation;
pub(in crate::policy::workflow) mod original_financial;
mod outcome;
mod outcome_proofs;
mod pricing;
mod record_index;
mod records;
mod shape;
mod targets;
pub use construct::{draft_transition, replay_draft};
#[cfg(test)]
mod first_work_tests;

#[cfg(test)]
mod repair_tests;
mod representation;

#[cfg(test)]
mod calibration_tests;
mod inventory;

// Per-kind immutable DATA; never a host completed-origin capability.
pub(in crate::policy::workflow) mod intrinsic_record;

mod candidate;
pub use candidate::{
    CandidateEnvelope, ClaimEffect, CustomerLocalFundingEffect, DependencyEffect, DependencyKind,
    EconomicCandidate, FinancialHeadKey, HeadEffect, LinkEffect, PermanentClaimKey,
};
