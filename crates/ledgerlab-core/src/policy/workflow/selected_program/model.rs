//! Closed selected grammar. Names are ordinary data, never engine branches.
use super::super::DefinitionRef;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    pub schema: String,
    pub policy_id: String,
    pub version: String,
    pub product_ref: DefinitionRef,
    pub context_schema_ref: DefinitionRef,
    pub slot_template_ref: DefinitionRef,
    pub compiler_ref: DefinitionRef,
    pub currency: String,
    pub scale: u8,
    pub rounding: String,
    pub mode: String,
    pub binding_slots: Vec<BindingSlot>,
    pub rules: Vec<Rule>,
    pub families: Vec<Family>,
    pub stages: Vec<serde_json::Value>,
    pub delayed_costs: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_policy_ref: Option<DefinitionRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BindingSlot {
    pub name: String,
    pub book: String,
    pub funding_leg: String,
    pub allowed_activities: Vec<String>,
    pub correction_source_slot: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    pub rule_id: String,
    pub activity: String,
    pub component_name: String,
    pub component_key: String,
    pub binding_slot: String,
    pub phase: String,
    pub when: Predicate,
    pub formula: Formula,
    pub dependency: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Predicate {
    True,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Formula {
    Fixed {
        amount: String,
    },
    Unit {
        quantity_field: String,
        unit: String,
        rate: String,
    },
    Discount {
        basis: Basis,
        mode: String,
        amount: Percent,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Basis {
    pub component_key: String,
    pub view: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Percent {
    pub kind: String,
    pub percent: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Family {
    pub family_name: String,
    pub family_key: String,
    pub trigger_activity: String,
    pub target_link_slot: String,
    pub selector: Selector,
    pub source_slot: String,
    pub code_field: String,
    pub codes: Vec<Code>,
    pub window: Window,
    pub maximum_premium: Amount,
    pub maximum_discount: Amount,
    pub correction_source_slot: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selector {
    pub path: Vec<String>,
    pub target_activity: String,
    pub binding_slot: String,
    pub component_key: String,
    pub basis: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Code {
    pub code: String,
    pub effect: Effect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Effect {
    None,
    Premium { amount: Percent },
    Discount { amount: Percent },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Window {
    pub anchor: String,
    pub anchor_path: Vec<String>,
    pub start_offset_us: String,
    pub occurrence_us: String,
    pub receipt_grace_us: String,
    pub acceptance_grace_us: String,
    pub activation_deadline_us: String,
    pub correction_us: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Amount {
    pub currency: String,
    pub scale: u8,
    pub atoms: String,
}
