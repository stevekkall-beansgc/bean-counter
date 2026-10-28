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
    pub stages: Vec<StageDef>,
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
pub(super) struct StageDef {
    pub name: String,
    pub authorization_source_slot: String,
    pub closure_activity: String,
    pub closure_component_key: String,
    pub closure_source_slot: String,
    pub closure_evidence_kind: String,
    pub window: Window,
    pub expected_slots: Vec<StageExpectedSlot>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StageExpectedSlot {
    pub key: String,
    pub source_slot: String,
    pub activity: String,
    pub component_keys: Vec<String>,
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
    False,
    Eq {
        field: Operand,
        value: serde_json::Value,
    },
    Lt {
        field: Operand,
        value: serde_json::Value,
    },
    Le {
        field: Operand,
        value: serde_json::Value,
    },
    Gt {
        field: Operand,
        value: serde_json::Value,
    },
    Ge {
        field: Operand,
        value: serde_json::Value,
    },
    In {
        field: Operand,
        values: Vec<serde_json::Value>,
    },
    All {
        args: Vec<Predicate>,
    },
    Any {
        args: Vec<Predicate>,
    },
    Not {
        arg: Box<Predicate>,
    },
}
impl Predicate {
    pub(super) fn visit_operands(
        &self,
        m: &mut super::super::transition_constructor::CoreMeter,
        visitor: &mut dyn FnMut(
            &Operand,
            &mut super::super::transition_constructor::CoreMeter,
        ) -> crate::Result<()>,
    ) -> crate::Result<()> {
        m.visit("plan.predicate_operand_node")?;
        match self {
            Self::True | Self::False => Ok(()),
            Self::Eq { field, .. }
            | Self::Lt { field, .. }
            | Self::Le { field, .. }
            | Self::Gt { field, .. }
            | Self::Ge { field, .. }
            | Self::In { field, .. } => visitor(field, m),
            Self::All { args } | Self::Any { args } => {
                for child in args {
                    child.visit_operands(m, visitor)?;
                }
                Ok(())
            }
            Self::Not { arg } => arg.visit_operands(m, visitor),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum Operand {
    BindingFact { binding_fact: String },
    Fact { fact: String },
    Context { context: String },
}
impl Operand {
    pub(super) fn binding_name(&self) -> Option<&str> {
        match self {
            Self::BindingFact { binding_fact } => Some(binding_fact),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Formula {
    ClosureCap {
        stage: String,
        basis_component_key: String,
        ceiling: Amount,
    },
    ClosureShare {
        basis_component_key: String,
        percent: String,
        ceiling: Amount,
        allocation_view: bool,
    },
    Fixed {
        amount: String,
    },
    Unit {
        quantity_field: String,
        unit: String,
        rate: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        normalize_to: Option<String>,
    },
    Percent {
        basis: Basis,
        percent: String,
    },
    Discount {
        basis: Basis,
        mode: String,
        amount: Percent,
    },
    ObserveCost {
        evidence_slot: String,
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Percent {
    Percent { percent: String },
    Fixed { amount: String },
}
impl Percent {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Percent { .. } => "percent",
            Self::Fixed { .. } => "fixed",
        }
    }
    pub(super) fn value(&self) -> &str {
        match self {
            Self::Percent { percent } => percent,
            Self::Fixed { amount } => amount,
        }
    }
    pub(super) fn value_mut(&mut self) -> &mut String {
        match self {
            Self::Percent { percent } => percent,
            Self::Fixed { amount } => amount,
        }
    }
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

// Existing rule ordering is unchanged; closure share runs after its cap basis.
pub(super) fn instruction_phase(rule: &Rule, component: bool) -> u8 {
    if component {
        return match rule.phase.as_str() {
            "base" | "cost" => 0,
            "premium" => 1,
            "discount" => 2,
            "cap" => 3,
            "share" => 4,
            _ => 5,
        };
    }
    match &rule.formula {
        Formula::ClosureCap { .. } => 1,
        Formula::ClosureShare { .. } => 2,
        _ if rule.phase == "base" => 0,
        _ => 1,
    }
}

/// Component ordering follows declared book before phase. Legacy plans never
/// enter this selected helper, so their instruction ordinals remain unchanged.
pub(super) fn component_book(
    m: &mut super::super::transition_constructor::CoreMeter,
    slots: &[BindingSlot],
    rule: &Rule,
) -> crate::Result<u8> {
    use super::super::transition_constructor::shape;
    for slot in slots {
        m.visit("plan.component_book")?;
        if shape::same_text(m, &slot.name, &rule.binding_slot)? {
            return match slot.book.as_str() {
                "retail" => Ok(0),
                "supplier" => Ok(1),
                "cost_observation" => Ok(2),
                _ => Err(shape::fail("component original book order")),
            };
        }
    }
    Err(shape::fail("component original binding slot"))
}
