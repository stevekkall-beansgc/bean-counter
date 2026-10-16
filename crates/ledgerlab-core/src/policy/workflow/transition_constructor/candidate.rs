//! Store-facing effects of a COMPLETE economic derivation. These types have no
//! public constructors, Serde implementation, or conversion from DraftTransition.
//! They convey pure facts only; host custody/currentness and atomic preparation
//! are separate obligations. No candidate factory exists until derivation closes.
use super::data::{ExactRef, OperationKey};
use crate::canonical::audit_arena::AuditedBytes;

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
}
pub struct HeadEffect {
    key: FinancialHeadKey,
    expected: Option<ExactRef>,
    proposed: ExactRef,
    revision: String,
}
impl HeadEffect {
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
}
pub struct ClaimEffect {
    key: PermanentClaimKey,
    record_ref: ExactRef,
}
impl ClaimEffect {
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
}
impl CustomerLocalFundingEffect {
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
    body: AuditedBytes,
}
impl CandidateEnvelope {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn reference(&self) -> &ExactRef {
        &self.reference
    }
    pub fn canonical_body(&self) -> &[u8] {
        self.body.bytes()
    }
}
pub struct EconomicCandidate {
    key: OperationKey,
    normalized_payload: AuditedBytes,
    introduced: Vec<CandidateEnvelope>,
    consumed_refs: Vec<ExactRef>,
    heads: Vec<HeadEffect>,
    claims: Vec<ClaimEffect>,
    links: Vec<LinkEffect>,
    dependencies: Vec<DependencyEffect>,
    funding: Vec<CustomerLocalFundingEffect>,
}
impl EconomicCandidate {
    pub fn key(&self) -> &OperationKey {
        &self.key
    }
    pub fn normalized_payload(&self) -> &[u8] {
        self.normalized_payload.bytes()
    }
    pub fn introduced(&self) -> &[CandidateEnvelope] {
        &self.introduced
    }
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
