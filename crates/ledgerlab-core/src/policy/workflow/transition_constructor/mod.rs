//! Pure selected draft/replay foundation. No output is a financial commit,
//! authenticated host capability or future-correction capacity certificate.
mod fact_reference_groups;
pub use fact_reference_groups::{
    select_work_fact_reference_inputs, OperationFactReferenceGroup, OperationFactReferenceInputs,
};
mod fact_relations;
#[cfg(test)]
mod lifecycle_consumer_tests;
pub use fact_relations::validate_lifecycle_context_references;
mod fact_references;
pub use fact_references::{select_fact_reference_inputs, FactReferenceInput, FactReferenceInputs};
mod lifecycle_edges;
pub use lifecycle_edges::{
    select_lifecycle_input_edges, LifecycleInputEdges, LifecycleRecordIdentity,
};
mod input_edges;
pub use input_edges::{
    select_component_local_input_edges, select_local_input_edges, select_resolved_inputs9_edges,
    select_stage_local_input_edges, validate_local_record_reference, LocalInputEdges,
    ResolvedInputEdges9,
};
mod work_keys;
pub use input_selection::{select_work_head_locators, WorkFamilyHeadLocator, WorkHeadLocators};
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
    PartyAuthorizationHead, ProposedHeads, RecordInput, SavedFinancialInput, StageHead,
    SupplierStateData,
};
mod alias_authority;
mod app_cost;
mod authority;
pub use alias_authority::{construct_local_work_alias_authority, LocalWorkAliasAuthority};
pub use candidate::{
    construct_local_cost_authority_enrollment, construct_local_work_alias_candidate,
    normalize_local_cost_authority_enrollment_intent, CostAuthorityEnrollmentState,
    LocalCostAuthorityEnrollment,
};
mod invocation;
pub use invocation::{select_invocation_operation_locators, InvocationOperationLocators};
mod stage;
mod stage_locators;
mod supplier_consent;
pub use stage::{
    select_stage_work_head_locator, select_work_invocation_head_locators, StageWorkHeadLocator,
    WorkInvocationHeadLocator, WorkInvocationHeadLocators,
};
pub use stage_locators::{
    select_stage_operation_locators, StageExpectedHeadLocator, StageOperationLocators,
};
pub use supplier_consent::{
    derive_local_supplier_commitment, derive_local_supplier_consent_verifications,
    normalize_local_supplier_assent_with_session, LocalSupplierAssent, LocalSupplierCommitment,
    LocalSupplierConsentVerifications, SupplierConsentInputs,
};
mod body;
mod close;
mod component_observation;
mod construct;
mod correction;
mod cost;
mod cost_postings;
mod declared_links;
mod link_assert;
mod input_selection;
mod source_authority;
mod source_projection;
pub use input_selection::{
    select_work_input_seeds, select_work_source_frame, EconomicCapture, SourceAuthorityCapture,
    WorkInputSeeds, WorkSourceLocator,
};
mod source_records;
pub use source_projection::{
    validate_app_local_source_shape, validate_app_nominated_sources, NominatedSourceFrame,
    NominatedSourceGrant,
};
pub use source_records::{
    produce_initial_registered_source_records, produce_initial_shared_namespace_source_records,
    produce_initial_source_records, InitialSourceCorrespondence, InitialSourceRecord,
    InitialSourceRecords,
};
mod financial_observation;
pub use financial_observation::audit_stream::{
    EconomicAuditStream, FinancialPrefixInput, OriginPointerBatch, OriginPointerBlock,
    OriginPointerEntry, OriginWitness, PreparedOriginBatch, ProducedOriginPrefix,
    ProducedOriginPrefixes, RecomputedAuditDigest, SourcePrefixInput, ValidatedOriginPointers,
    ORIGIN_POINTER_BATCH, ORIGIN_POINTER_BATCH_WORDS, ORIGIN_POINTER_BLOCK,
    ORIGIN_POINTER_MAX_BYTES,
};
mod family_context;
mod funding;
mod intent;
pub use intent::{
    normalize_local_saved_intent, normalize_local_work_alias_intent, normalize_saved_intent,
};
mod emitted_record;
#[cfg(test)]
mod legacy_original_compatibility;
mod outcome;
mod outcome_proofs;
pub(in crate::policy::workflow) mod owned_facts;
mod pricing;
mod record_body;
mod record_index;
mod records;
mod root_correction;
pub(in crate::policy::workflow) mod shape;
mod targets;
mod work_proofs;
mod working_value;
pub use construct::{draft_transition, replay_draft};
#[cfg(test)]
mod first_work_tests;

#[cfg(test)]
mod repair_tests;
mod representation;
mod retention;
pub use retention::{retain_attestation, RetainedAssertionData, RetainedAssertionEnvelope};

#[cfg(test)]
mod calibration_tests;
mod inventory;
pub use inventory::slots::validate_app_slot_projection;

// Per-kind immutable DATA; never a host completed-origin capability.
pub(in crate::policy::workflow) mod intrinsic_record;

mod audit_metadata;
pub use audit_metadata::{produce_app_audit_metadata, AppAuditMetadata, AuditVerifierDocument};
mod candidate;
mod capacity;
pub use candidate::{
    AccountEnrollmentEffect, CandidateEnvelope, CandidateResultKind, CandidateResultMetadata,
    ClaimEffect, CostLineKey, CustomerLocalFundingEffect, DependencyEffect, DependencyKind,
    EconomicCandidate, FinancialHeadKey, HeadEffect, LinkEffect, ObservationAssociation,
    PermanentClaimKey, ProviderAccountIdentity,
};

mod economic;
pub use economic::{
    construct_economic_candidate, construct_local_financial_candidate,
    reconstruct_original_candidate,
};

mod local_financial_evidence;
pub use local_financial_evidence::{
    produce_local_financial_request_evidence, LocalFinancialRequestEvidence,
};

mod local_locators;
pub use local_locators::{
    locate_local_subject_binding, select_local_operation_locators, LocalOperationLocators,
    LocalRecordIdentity, LocalSubjectBinding,
};
