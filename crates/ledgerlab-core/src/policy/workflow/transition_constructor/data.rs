//! Explicit pure input snapshots. These values are not authenticated capabilities.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactRef {
    pub kind: String,
    pub id: String,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationKey {
    pub logical_store_id: String,
    pub scope: [String; 2],
    pub source: String,
    pub operation_id: String,
}
#[derive(Clone, Debug)]
pub struct RecordInput {
    pub reference: ExactRef,
    pub canonical_body: Vec<u8>,
}
/// Borrowed exact retained or pending record DATA. The host retains owning
/// artifacts and actual provenance; this view copies neither bodies nor trust.
#[derive(Clone, Copy)]
pub struct EconomicRecordView<'a> {
    pub reference: &'a ExactRef,
    pub canonical_body: &'a [u8],
}
/// Literal original operands and comparison bodies, not authenticated prestate.
/// Earliest validated introductions distinguish preexisting inputs from new
/// results; saved result amounts never become economic construction inputs.
pub struct SavedFinancialInput<'a> {
    pub operation: EconomicRecordView<'a>,
    pub original_request_bytes: &'a [u8],
    pub activation_ref: &'a ExactRef,
    pub original_state: &'a EconomicStateData<'a>,
    pub original_inputs: &'a [EconomicRecordView<'a>],
}
#[derive(Clone, Debug)]
pub struct DraftContextData {
    pub key: OperationKey,
    pub actor: String,
    pub canonical_economic_namespace: String,
    pub installation_mode: String,
    pub received_at: String,
    pub accepted_at: String,
    pub authorities: Vec<AuthorityContextData>,
    pub source_observation_inputs: Vec<RecordInput>,
}
#[derive(Clone, Debug)]
pub struct AuthorityContextData {
    pub source_slot: String,
    pub enrollment_ref: ExactRef,
    pub current_state_ref: ExactRef,
    pub current_grant_ref: ExactRef,
    pub original_grant_refs: Vec<ExactRef>,
    pub binding_ref: ExactRef,
    pub accepted_agreement_ref: ExactRef,
    pub activation_ref: ExactRef,
    pub authority_observation_ref: ExactRef,
}
#[derive(Clone, Debug)]
pub struct DecisionHead {
    pub decision_id: String,
    pub revision: String,
    pub reference: ExactRef,
}
#[derive(Clone, Debug)]
pub struct FamilyHead {
    pub target_ref: ExactRef,
    pub family_slot: String,
    pub revision: String,
    pub reference: ExactRef,
}
#[derive(Clone, Debug)]
pub struct CapacityHead {
    pub root_decision_id: String,
    pub revision: String,
    pub reference: ExactRef,
}
#[derive(Clone,Debug)]
pub struct InvocationHead {
    pub invocation_id:String,
    pub revision:String,
    pub reference:ExactRef,
}
#[derive(Clone,Debug)]
pub struct CostHead {
    pub cost_claim_ref:ExactRef,
    pub revision:String,
    pub reference:ExactRef,
}
#[derive(Clone,Debug)]
pub struct CostOwnerHead {
    pub revision:String,
    pub reference:ExactRef,
}
/// Held-snapshot DATA, not index completeness, owner custody or a caller proof.
/// Each consumer derives the exact key from actual original records and checks
/// the selected head's full body/identity/lineage before constructing effects.
pub struct SupplierStateData<'a> {
    pub invocation_heads:&'a [InvocationHead],
    pub cost_heads:&'a [CostHead],
    pub cost_consumption_heads:&'a [CostHead],
    pub cost_owner_heads:&'a [CostOwnerHead],
}
/// Actual point-lookup hits selected under the held host snapshot. Lists carry
/// retained rows, not a completeness/authority bit; each pure consumer checks
/// exact immutable keys, bodies and current revision. Empty hits still require
/// the host's complete locked absence observation before preparation.
pub struct AppStateData<'a> {
    pub cost_heads: &'a [CostHead],
    pub observation_refs: &'a [ExactRef],
    pub cost_claim_refs: &'a [ExactRef],
    pub evidence_consumption_refs: &'a [ExactRef],
}
/// The complete candidate's extra projection DATA. The host must select these
/// exact capacity heads under the same snapshot/locks as financial heads;
/// neither a payload nor this view asserts completeness or currentness.
pub struct EconomicStateData<'a> {
    pub financial: &'a DraftStateData,
    pub capacity_heads: &'a [CapacityHead],
    pub supplier:Option<&'a SupplierStateData<'a>>,
    pub app:Option<&'a AppStateData<'a>>,
}
#[derive(Clone, Debug, Default)]
pub struct DraftStateData {
    pub chain_head: Option<ExactRef>,
    pub decision_heads: Vec<DecisionHead>,
    pub family_heads: Vec<FamilyHead>,
    pub work_claims: Vec<ExactRef>,
    pub outcome_claims: Vec<ExactRef>,
    pub links: Vec<ExactRef>,
    pub evidence_verifications: Vec<ExactRef>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DraftEnvelope {
    pub reference: ExactRef,
    pub canonical_body: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ProposedHeads {
    pub decision: Vec<(String, String, ExactRef)>,
    pub family: Vec<(ExactRef, String, String, ExactRef)>,
    pub chain: Option<ExactRef>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DraftTransition {
    pub normalized_payload: Vec<u8>,
    pub new_envelopes: Vec<DraftEnvelope>,
    pub evaluation_steps: Vec<serde_json::Value>,
    pub proposed_heads: ProposedHeads,
    pub claim_introductions: Vec<ExactRef>,
    pub link_introductions: Vec<ExactRef>,
    pub prospective_obligations: Vec<serde_json::Value>,
    pub consumed_refs: Vec<ExactRef>,
    pub history_edges: Vec<(ExactRef, ExactRef)>,
    pub usage: super::MeterUsage,
    pub unresolved_admission_obligations: &'static [&'static str],
}
/// Internal output of the one economic interpreter, before either a legacy
/// DATA projection or complete capacity/resource/funding derivation. It is not
/// a DraftTransition and exposes no host capability or public conversion.
pub(super) struct Construction {
    pub normalized_payload: Vec<u8>,
    pub new_envelopes: Vec<DraftEnvelope>,
    pub evaluation_steps: Vec<serde_json::Value>,
    pub proposed_heads: ProposedHeads,
    pub claim_introductions: Vec<ExactRef>,
    pub link_introductions: Vec<ExactRef>,
    pub prospective_obligations: Vec<serde_json::Value>,
    pub consumed_refs: Vec<ExactRef>,
    pub history_edges: Vec<(ExactRef, ExactRef)>,
}
impl Construction {
    pub(super) fn draft_projection(
        self,
        m: &mut super::CoreMeter,
    ) -> crate::Result<DraftTransition> {
        Ok(DraftTransition {
            normalized_payload: self.normalized_payload,
            new_envelopes: self.new_envelopes,
            evaluation_steps: self.evaluation_steps,
            proposed_heads: self.proposed_heads,
            claim_introductions: self.claim_introductions,
            link_introductions: self.link_introductions,
            prospective_obligations: self.prospective_obligations,
            consumed_refs: self.consumed_refs,
            history_edges: self.history_edges,
            usage: m.snapshot_usage()?,
            unresolved_admission_obligations: UNRESOLVED_ADMISSION_OBLIGATIONS,
        })
    }
}
pub const UNRESOLVED_ADMISSION_OBLIGATIONS: &[&str] = &[
    "host-root-source-and-origin-authentication",
    "complete-index-absence",
    "global-funding-and-balance-reservations",
    "all-ancestor-future-correction-capacity",
    "financial-journal-atomicity",
    "installation-profile-b-admission",
];
