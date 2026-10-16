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
    pub original_state: &'a DraftStateData,
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
pub const UNRESOLVED_ADMISSION_OBLIGATIONS: &[&str] = &[
    "host-root-source-and-origin-authentication",
    "complete-index-absence",
    "global-funding-and-balance-reservations",
    "all-ancestor-future-correction-capacity",
    "financial-journal-atomicity",
    "installation-profile-b-admission",
];
