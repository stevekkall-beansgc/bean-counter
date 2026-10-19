//! Closed selected grammar. Names are ordinary data, never engine branches.
use super::super::DefinitionRef;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "R: Deserialize<'de>"))]
pub(super) struct Policy<R = DefinitionRef> {
    pub schema: String,
    pub policy_id: String,
    pub version: String,
    pub product_ref: R,
    pub context_schema_ref: R,
    pub slot_template_ref: R,
    pub compiler_ref: R,
    pub currency: String,
    pub scale: u8,
    pub rounding: String,
    pub mode: String,
    pub binding_slots: Vec<BindingSlot>,
    pub rules: Vec<Rule>,
    pub families: Vec<Family>,
    pub stages: Vec<serde_json::Value>,
    pub delayed_costs: Vec<DelayedCost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_delayed_costs: Option<Vec<AppCostDeclaration>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_policy_ref: Option<R>,
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
    FinalCost {
        evidence_slot: String,
        supplier_slot: String,
        multiplier: CostMultiplier,
        markup: String,
        ceiling: Amount,
    },
    SupplierFinalCost {
        evidence_slot: String,
        ceiling: Amount,
    },
    AppFinalCost {
        evidence_slot: String,
        multiplier: CostMultiplier,
        markup: String,
        ceiling: Amount,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CostMultiplier {
    pub numerator: String,
    pub denominator: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DelayedCost {
    pub component_key: String,
    pub evidence_source_slot: String,
    pub finalize_within_us: String,
    pub on_missing: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppCostDeclaration {
    pub component_key: String,
    pub activity: String,
    pub resource_funding_leg: String,
    pub usage_field: String,
    pub usage_unit: String,
    pub work_source_slot: String,
    pub cost_source_slot: String,
    pub correction_source_slot: String,
    pub finalize_within_us: String,
    pub correction_within_us: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causal_reversal: Option<AppCausalClause>,
    pub maximum_final_amount: Amount,
    pub on_missing: String,
    pub cost_basis: String,
    pub comparable_quantity_rule: String,
    pub allocation: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AppCausalClause {
    pub root_relation: String,
    pub mode: String,
    pub standalone_deadline_effect: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_success_close: Option<NoSuccessClose>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NoSuccessClose {
    pub close_source_slot: String,
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
