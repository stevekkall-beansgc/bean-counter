//! Store-facing effects of a COMPLETE economic derivation. These types have no
//! public constructors, Serde implementation, or conversion from DraftTransition.
//! They convey pure facts only; host custody/currentness and atomic preparation
//! are separate obligations. The connected factory refuses until complete derivation closes.
use super::data::{ExactRef, OperationKey};
use crate::canonical::audit_arena::ExternalBufferLease;
pub(super) mod assembly;
pub(super) mod heads;
pub(super) mod introductions;
pub(super) mod reservations;

pub enum FinancialHeadKey {
    Decision {
        decision_id: String,
    },
    Family {
        target_ref: ExactRef,
        family_slot: String,
    },
    Chain {
        logical_store_id: String,
        scope: [String; 2],
        customer: String,
        chain: String,
    },
    Capacity {
        root_decision_id: String,
    },
    AppCost {
        cost_claim_ref: ExactRef,
    },
    Invocation {
        invocation_id: String,
    },
    Cost {
        cost_claim_ref: ExactRef,
    },
    CostConsumption {
        cost_claim_ref: ExactRef,
    },
    /// Installation-global immutable provider line identity. The host must
    /// not prefix this projection key with the operation's policy scope.
    CostOwner {
        line_key: CostLineKey,
    },
}
pub struct ProviderAccountIdentity {
    provider_namespace: String,
    provider_account_id: String,
    environment: String,
}
impl ProviderAccountIdentity {
    pub fn provider_namespace(&self) -> &str {
        &self.provider_namespace
    }
    pub fn provider_account_id(&self) -> &str {
        &self.provider_account_id
    }
    pub fn environment(&self) -> &str {
        &self.environment
    }
}
pub struct CostLineKey {
    cost_authority_id: String,
    provider_account_identity: ProviderAccountIdentity,
    original_document_id: String,
    original_line_id: String,
}
impl CostLineKey {
    pub fn cost_authority_id(&self) -> &str {
        &self.cost_authority_id
    }
    pub fn provider_account_identity(&self) -> &ProviderAccountIdentity {
        &self.provider_account_identity
    }
    pub fn original_document_id(&self) -> &str {
        &self.original_document_id
    }
    pub fn original_line_id(&self) -> &str {
        &self.original_line_id
    }
}
/// A distinct installation-global enrollment effect, before any provider line
/// exists. It is derived from actual CostAuthority/OwnerActivation output and
/// does not claim external account discovery or authorize owner replacement.
pub struct AccountEnrollmentEffect {
    account: ProviderAccountIdentity,
    cost_authority_ref: ExactRef,
    owner_activation_ref: ExactRef,
}
impl AccountEnrollmentEffect {
    pub fn account(&self) -> &ProviderAccountIdentity {
        &self.account
    }
    pub fn cost_authority_ref(&self) -> &ExactRef {
        &self.cost_authority_ref
    }
    pub fn owner_activation_ref(&self) -> &ExactRef {
        &self.owner_activation_ref
    }
}
pub struct HeadEffect {
    key: FinancialHeadKey,
    expected: Option<ExactRef>,
    proposed: ExactRef,
    revision: String,
    // Exact position derived after the completed factory fixes its immutable
    // introduced order; this is an association, never an origin certificate.
    introduced_position: usize,
}
impl HeadEffect {
    pub fn introduced_position(&self) -> usize {
        self.introduced_position
    }
    pub fn key(&self) -> &FinancialHeadKey {
        &self.key
    }
    pub fn expected(&self) -> Option<&ExactRef> {
        self.expected.as_ref()
    }
    pub fn proposed(&self) -> &ExactRef {
        &self.proposed
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
}
pub enum PermanentClaimKey {
    /// Shared fact-attestation kind, including family no-success assertions.
    /// ECONOMIC-BOUNDARY§6 fixes store/scope/source/id; schema/recipe changes
    /// do not create another assertion namespace within that identity.
    Assertion {
        logical_store_id: String,
        scope: [String; 2],
        verified_source: String,
        attestation_id: String,
    },
    Work {
        logical_store_id: String,
        scope: [String; 2],
        source_namespace: String,
        business_operation_id: String,
        completion: String,
    },
    Outcome {
        scope: [String; 2],
        agreement_id: String,
        family_slot: String,
        target_id: String,
        source_namespace: String,
        business_outcome_token: String,
    },
    /// App keys are installation-wide; scope, chain and policy labels do not
    /// partition actual call attribution or permit evidence consumption twice.
    AppCall {
        logical_store_id: String,
        source_namespace: String,
        call_id: String,
    },
    AppComponentObservation {
        logical_store_id: String,
        original_work_ref: ExactRef,
        component_slot: String,
    },
    AppProviderCall {
        logical_store_id: String,
        provider: String,
        account: String,
        request_id: String,
    },
    AppCostComponent {
        logical_store_id: String,
        original_work_ref: ExactRef,
        component_slot: String,
    },
    AppFinalEvidence {
        logical_store_id: String,
        final_evidence_ref: ExactRef,
    },
    AppObservationRevision {
        logical_store_id: String,
        observation_ref: ExactRef,
        cost_revision: String,
    },
    /// Independent of scope/agreement/invocation/version/credential labels.
    CostLine { line_key: CostLineKey },
    /// Separate once-only delayed component coverage. It does not replace the
    /// installation-global CostLine uniqueness check.
    CostComponent {
        scope: [String; 2],
        original_work_claim_id: String,
        supplier_component_slot: String,
    },
}
pub struct ClaimEffect {
    key: PermanentClaimKey,
    record_ref: ExactRef,
}
impl ClaimEffect {
    pub(super) fn derived(key: PermanentClaimKey, record_ref: ExactRef) -> Self {
        Self { key, record_ref }
    }
    pub fn key(&self) -> &PermanentClaimKey {
        &self.key
    }
    pub fn record_ref(&self) -> &ExactRef {
        &self.record_ref
    }
}
/// The immutable link's exact association, exposed for mechanical scoped
/// (child, slot) uniqueness without host pricing or reparsing an economic body.
pub struct LinkEffect {
    record_ref: ExactRef,
    scope: [String; 2],
    child_ref: ExactRef,
    slot: String,
    predecessor_ref: ExactRef,
    product_ref: ExactRef,
    binding_ref: ExactRef,
}
impl LinkEffect {
    pub fn record_ref(&self) -> &ExactRef {
        &self.record_ref
    }
    pub fn scope(&self) -> &[String; 2] {
        &self.scope
    }
    pub fn child_ref(&self) -> &ExactRef {
        &self.child_ref
    }
    pub fn slot(&self) -> &str {
        &self.slot
    }
    pub fn predecessor_ref(&self) -> &ExactRef {
        &self.predecessor_ref
    }
    pub fn product_ref(&self) -> &ExactRef {
        &self.product_ref
    }
    pub fn binding_ref(&self) -> &ExactRef {
        &self.binding_ref
    }
}
pub enum DependencyKind {
    OutcomeFamily,
    WorkBasis,
    DelayedCost,
}
pub struct DependencyEffect {
    record_ref: ExactRef,
    ancestor_decision_id: String,
    dependent_decision_id: String,
    kind: DependencyKind,
    target_ref: ExactRef,
    family_slot: Option<String>,
    original_terms_ref: ExactRef,
    capacity_ref: ExactRef,
}
impl DependencyEffect {
    pub fn record_ref(&self) -> &ExactRef {
        &self.record_ref
    }
    pub fn ancestor_decision_id(&self) -> &str {
        &self.ancestor_decision_id
    }
    pub fn dependent_decision_id(&self) -> &str {
        &self.dependent_decision_id
    }
    pub fn kind(&self) -> &DependencyKind {
        &self.kind
    }
    pub fn target_ref(&self) -> &ExactRef {
        &self.target_ref
    }
    pub fn family_slot(&self) -> Option<&str> {
        self.family_slot.as_deref()
    }
    pub fn original_terms_ref(&self) -> &ExactRef {
        &self.original_terms_ref
    }
    pub fn capacity_ref(&self) -> &ExactRef {
        &self.capacity_ref
    }
}
/// Existing customer-local terms/prospective exposure, NOT a cash balance or a
/// fabricated canonical FundingHead. Supplier invocation funding is a distinct
/// derivation and cannot be represented by constructing this customer-local view.
pub struct CustomerLocalFundingEffect {
    original_work_claim_ref: ExactRef,
    binding_slot: String,
    binding_ref: ExactRef,
    slot_terms_ref: ExactRef,
    role_ref: ExactRef,
    funding_leg: String,
    evidence_refs: Vec<ExactRef>,
    currency: String,
    scale: u8,
    exposure_ceiling_atoms: String,
    current_exposure_atoms: String,
    prospective_exposure_atoms: String,
    // Every copied field/vector above is destroyed before its owning lease.
    _ownership: ExternalBufferLease,
}
impl CustomerLocalFundingEffect {
    pub fn original_work_claim_ref(&self) -> &ExactRef {
        &self.original_work_claim_ref
    }
    pub fn binding_slot(&self) -> &str {
        &self.binding_slot
    }
    pub(super) fn from_derived(
        original_work_claim_ref: ExactRef,
        binding_slot: String,
        binding_ref: ExactRef,
        slot_terms_ref: ExactRef,
        role_ref: ExactRef,
        funding_leg: String,
        evidence_refs: Vec<ExactRef>,
        currency: String,
        scale: u8,
        exposure_ceiling_atoms: String,
        current_exposure_atoms: String,
        prospective_exposure_atoms: String,
        ownership: ExternalBufferLease,
    ) -> Self {
        Self {
            original_work_claim_ref,
            binding_slot,
            binding_ref,
            slot_terms_ref,
            role_ref,
            funding_leg,
            evidence_refs,
            currency,
            scale,
            exposure_ceiling_atoms,
            current_exposure_atoms,
            prospective_exposure_atoms,
            _ownership: ownership,
        }
    }
    pub fn binding_ref(&self) -> &ExactRef {
        &self.binding_ref
    }
    pub fn slot_terms_ref(&self) -> &ExactRef {
        &self.slot_terms_ref
    }
    pub fn role_ref(&self) -> &ExactRef {
        &self.role_ref
    }
    pub fn funding_leg(&self) -> &str {
        &self.funding_leg
    }
    pub fn evidence_refs(&self) -> &[ExactRef] {
        &self.evidence_refs
    }
    pub fn currency(&self) -> &str {
        &self.currency
    }
    pub fn scale(&self) -> u8 {
        self.scale
    }
    pub fn exposure_ceiling_atoms(&self) -> &str {
        &self.exposure_ceiling_atoms
    }
    pub fn current_exposure_atoms(&self) -> &str {
        &self.current_exposure_atoms
    }
    pub fn prospective_exposure_atoms(&self) -> &str {
        &self.prospective_exposure_atoms
    }
}
pub struct CandidateEnvelope {
    reference: ExactRef,
    schema: String,
    body: super::emitted_record::EncodedBody,
}
impl CandidateEnvelope {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn reference(&self) -> &ExactRef {
        &self.reference
    }
    pub fn canonical_body(&self) -> &[u8] {
        &self.body
    }
}
/// Exact receipt classification selected by the executed recipe, including
/// zero-valued economic operations. It is never inferred from action count.
pub enum CandidateResultKind {
    Economic,
    Administrative,
}
impl CandidateResultKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Economic => "economic",
            Self::Administrative => "administrative",
        }
    }
}
/// An observation and the verifier document actually selected for that
/// observation. Both immutable records must belong to the completed closure;
/// this DATA association is not a host authentication certificate.
pub struct ObservationAssociation {
    observation_ref: ExactRef,
    verifier_ref: ExactRef,
}
impl ObservationAssociation {
    pub fn observation_ref(&self) -> &ExactRef {
        &self.observation_ref
    }
    pub fn verifier_ref(&self) -> &ExactRef {
        &self.verifier_ref
    }
}
/// Wrapper operands fixed by complete construction. Reused members belong to
/// the manifest closure, while result_refs names exactly this recipe's newly
/// produced authoritative results. The combined digest binds the separately
/// checked source and financial witnesses; it cannot replace either audit.
pub struct ExperimentalAuditMetadata {
    source_observations: Vec<ObservationAssociation>,
    financial_observation: ObservationAssociation,
    combined_audit_digest: String,
}
impl ExperimentalAuditMetadata {
    pub fn source_observations(&self) -> &[ObservationAssociation] {
        &self.source_observations
    }
    pub fn source_observation_refs(&self) -> impl ExactSizeIterator<Item = &ExactRef> {
        self.source_observations.iter().map(|x| &x.observation_ref)
    }
    pub fn financial_observation(&self) -> &ObservationAssociation {
        &self.financial_observation
    }
    pub fn financial_observation_ref(&self) -> &ExactRef {
        &self.financial_observation.observation_ref
    }
    pub fn combined_audit_digest(&self) -> &str {
        &self.combined_audit_digest
    }
}
enum CandidateExecutionMetadata {
    ExperimentalAudit9(ExperimentalAuditMetadata),
    LocalSelected1,
}
pub struct CandidateResultMetadata {
    kind: CandidateResultKind,
    result_refs: Vec<ExactRef>,
    authority_ref: ExactRef,
    execution: CandidateExecutionMetadata,
}
impl CandidateResultMetadata {
    pub fn result_kind(&self) -> &'static str {
        self.kind.as_str()
    }
    pub fn result_refs(&self) -> &[ExactRef] {
        &self.result_refs
    }
    pub fn authority_ref(&self) -> &ExactRef {
        &self.authority_ref
    }
    pub fn experimental_audit(&self) -> Option<&ExperimentalAuditMetadata> {
        match &self.execution {
            CandidateExecutionMetadata::ExperimentalAudit9(v) => Some(v),
            CandidateExecutionMetadata::LocalSelected1 => None,
        }
    }
    pub fn is_local_execution(&self) -> bool {
        matches!(self.execution, CandidateExecutionMetadata::LocalSelected1)
    }
}
pub struct EconomicCandidate {
    key: OperationKey,
    normalized_payload: super::emitted_record::EncodedBody,
    introduced: Vec<CandidateEnvelope>,
    consumed_refs: Vec<ExactRef>,
    heads: Vec<HeadEffect>,
    claims: Vec<ClaimEffect>,
    links: Vec<LinkEffect>,
    dependencies: Vec<DependencyEffect>,
    funding: Vec<CustomerLocalFundingEffect>,
    account_enrollments: Vec<AccountEnrollmentEffect>,
    result: CandidateResultMetadata,
    _ownership: assembly::CandidateOwnership,
}
impl EconomicCandidate {
    pub fn account_enrollments(&self) -> &[AccountEnrollmentEffect] {
        &self.account_enrollments
    }
    pub fn result_metadata(&self) -> &CandidateResultMetadata {
        &self.result
    }
    pub fn key(&self) -> &OperationKey {
        &self.key
    }
    pub fn normalized_payload(&self) -> &[u8] {
        &self.normalized_payload
    }
    pub fn introduced(&self) -> &[CandidateEnvelope] {
        &self.introduced
    }
    /// Used preexisting inputs I. Pending source headers belong to introduced
    /// results N and cannot become financial-history seeds through this view.
    pub fn consumed_refs(&self) -> &[ExactRef] {
        &self.consumed_refs
    }
    pub fn heads(&self) -> &[HeadEffect] {
        &self.heads
    }
    pub fn claims(&self) -> &[ClaimEffect] {
        &self.claims
    }
    pub fn links(&self) -> &[LinkEffect] {
        &self.links
    }
    pub fn dependencies(&self) -> &[DependencyEffect] {
        &self.dependencies
    }
    pub fn customer_local_funding(&self) -> &[CustomerLocalFundingEffect] {
        &self.funding
    }
}
