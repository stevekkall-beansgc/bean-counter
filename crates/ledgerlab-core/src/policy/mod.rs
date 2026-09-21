//! Typed compiler/evaluator boundary. Phase 1 supports fixed retail bases and
//! additive percentage discounts; later operators fail explicitly.
use crate::canonical;
use crate::domain::{slug, text, Context, Event};
use crate::money::{add_atoms, validate_currency, Decimal, ExactRatio};
use crate::wire::{Completion, EventKind};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Typed Phase 2 evaluation. It deliberately does not extend the frozen wire
/// parser or manufacture Phase 1 journal records for later record families.
pub mod chaining;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDto {
    schema: String,
    id: String,
    currency: String,
    scale: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rounding: Option<String>,
    rules: Vec<RuleDto>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDto {
    id: String,
    on: EventKind,
    op: String,
    component: String,
    book: String,
    amount: AmountDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when: Option<Vec<PredicateDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discount_mode: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AmountDto {
    Fixed { fixed: Decimal },
    Percent { percent: Decimal, basis: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PredicateDto {
    field: String,
    eq: String,
}
#[derive(Clone, Debug)]
pub struct CompiledPolicy {
    pub(crate) body: serde_json::Value,
    pub(crate) currency: String,
    pub(crate) scale: u8,
    pub(crate) rules: Vec<Rule>,
}
#[derive(Clone, Debug)]
pub(crate) struct Rule {
    pub id: String,
    pub component: String,
    pub operation: Operation,
    pub predicates: Vec<Predicate>,
}
#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Base {
        fixed: Decimal,
    },
    Discount {
        percent: Decimal,
        basis: usize,
        basis_name: String,
    },
}
#[derive(Clone, Debug)]
pub(crate) enum Predicate {
    Tier(String),
    Funding(String),
    Priority(bool),
    Source(String),
    Status(Completion),
}
#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub rule: Option<usize>,
    pub code: &'static str,
    pub unrounded: Option<ExactRatio>,
    pub rounded: Option<i128>,
    pub basis: Option<(usize, ExactRatio)>,
    pub predicate_inputs: Vec<(String, PredicateValue)>,
}
#[derive(Clone, Debug)]
pub(crate) enum PredicateValue {
    Text(String),
    Boolean(bool),
    Source(String),
}
impl CompiledPolicy {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_value(canonical::parse(bytes)?)
    }
    pub fn canonical_value(&self) -> &serde_json::Value {
        &self.body
    }
    pub(crate) fn from_value(value: serde_json::Value) -> Result<Self> {
        // Untagged DTO alternatives alone ignore unknown variant fields. Check
        // amount keys before deserializing, so no extra expression is discarded.
        if let Some(rules) = value.get("rules").and_then(serde_json::Value::as_array) {
            for rule in rules {
                if let Some(amount) = rule.get("amount").and_then(serde_json::Value::as_object) {
                    let legal = amount.len() == 1 && amount.contains_key("fixed")
                        || amount.len() == 2
                            && amount.contains_key("percent")
                            && amount.contains_key("basis");
                    if !legal {
                        return Err(Error::new("POLICY_UNSUPPORTED_OP", "amount form"));
                    }
                }
            }
        }
        let dto: PolicyDto = canonical::dto(value)?;
        if dto.schema != "ledger-policy/1"
            || dto
                .rounding
                .as_deref()
                .is_some_and(|s| s != "nearest_ties_away")
        {
            return Err(Error::new("SCHEMA", "policy version/rounding"));
        }
        slug(&dto.id)?;
        validate_currency(&dto.currency, dto.scale)?;
        if dto.rules.is_empty() || dto.rules.len() > 64 {
            return Err(Error::new("POLICY_LIMIT", "rules"));
        }
        let mut rules: Vec<Rule> = Vec::new();
        let mut ids = BTreeSet::new();
        let mut components = BTreeSet::new();
        let mut discount_phase = false;
        let mut nodes = 1;
        for raw in &dto.rules {
            slug(&raw.id)?;
            slug(&raw.component)?;
            if !ids.insert(&raw.id) || !components.insert(&raw.component) {
                return Err(Error::new(
                    "POLICY_AMBIGUOUS_MATCH",
                    "duplicate rule/component",
                ));
            }
            if raw.on != EventKind::Generated || raw.book != "retail" {
                return Err(Error::new(
                    "UNSUPPORTED_SLICE",
                    "Phase 1 generated retail policy",
                ));
            }
            let predicates = raw.when.as_deref().unwrap_or(&[]);
            if predicates.len() > 8 {
                return Err(Error::new("POLICY_LIMIT", "predicates"));
            }
            nodes += 2 + predicates.len();
            let predicates = predicates
                .iter()
                .map(|p| {
                    text(&p.eq, 128)?;
                    Ok(match p.field.as_str() {
                        "binding.tier" => {
                            slug(&p.eq)?;
                            Predicate::Tier(p.eq.clone())
                        }
                        "binding.funding" => {
                            if !["byok", "platform"].contains(&p.eq.as_str()) {
                                return Err(Error::new(
                                    "POLICY_UNKNOWN_FIELD",
                                    "funding predicate",
                                ));
                            }
                            Predicate::Funding(p.eq.clone())
                        }
                        "binding.priority" => Predicate::Priority(match p.eq.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => {
                                return Err(Error::new("POLICY_UNKNOWN_FIELD", "boolean predicate"))
                            }
                        }),
                        "source" => {
                            crate::domain::validate_source(&p.eq)?;
                            Predicate::Source(p.eq.clone())
                        }
                        "status" => Predicate::Status(match p.eq.as_str() {
                            "succeeded" => Completion::Succeeded,
                            _ => {
                                return Err(Error::new(
                                    "POLICY_UNSUPPORTED_OP",
                                    "failed-work pricing",
                                ))
                            }
                        }),
                        _ => return Err(Error::new("POLICY_UNKNOWN_FIELD", "predicate field")),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let operation = match (raw.op.as_str(), &raw.amount) {
                ("base", AmountDto::Fixed { fixed }) => {
                    if raw.discount_mode.is_some() || discount_phase {
                        return Err(Error::new("POLICY_CYCLE", "base must precede discounts"));
                    }
                    fixed.atoms_exact(dto.scale)?;
                    Operation::Base {
                        fixed: fixed.clone(),
                    }
                }
                ("discount", AmountDto::Percent { percent, basis }) => {
                    discount_phase = true;
                    percent.percent()?;
                    if raw.discount_mode.as_deref() != Some("additive") {
                        return Err(Error::new(
                            "UNSUPPORTED_SLICE",
                            "only additive discount in Phase 1",
                        ));
                    }
                    text(basis, 128)?;
                    let component = basis
                        .strip_prefix("self.")
                        .filter(|c| c.ends_with(".base"))
                        .ok_or_else(|| {
                            Error::new("POLICY_MISSING_BASIS", "typed self base required")
                        })?;
                    let index = rules
                        .iter()
                        .position(|r| {
                            r.component == component
                                && matches!(r.operation, Operation::Base { .. })
                        })
                        .ok_or_else(|| {
                            Error::new("POLICY_MISSING_BASIS", "earlier base component required")
                        })?;
                    Operation::Discount {
                        percent: percent.clone(),
                        basis: index,
                        basis_name: basis.clone(),
                    }
                }
                _ => {
                    return Err(Error::new(
                        "POLICY_UNSUPPORTED_OP",
                        "Phase 1 operator/amount",
                    ))
                }
            };
            rules.push(Rule {
                id: raw.id.clone(),
                component: raw.component.clone(),
                operation,
                predicates,
            });
        }
        if nodes > 256 {
            return Err(Error::new("POLICY_LIMIT", "AST nodes"));
        }
        Ok(Self {
            body: serde_json::to_value(&dto).map_err(|e| Error::new("SCHEMA", e.to_string()))?,
            currency: dto.currency,
            scale: dto.scale,
            rules,
        })
    }
    pub(crate) fn evaluate_steps(
        &self,
        event: &Event,
        context: &Context,
        failure: Option<&'static str>,
    ) -> Result<Vec<Step>> {
        if event.dto().status == Some(Completion::Failed) {
            return Ok(vec![Step {
                rule: None,
                code: "FAILED_WORK",
                unrounded: None,
                rounded: None,
                basis: None,
                predicate_inputs: vec![],
            }]);
        }
        let mut steps = Vec::new();
        let mut booked = vec![0i128; self.rules.len()];
        let mut discounted = vec![0i128; self.rules.len()];
        for (index, rule) in self.rules.iter().enumerate() {
            let mut predicate_inputs = Vec::new();
            let mut matches = true;
            for predicate in &rule.predicates {
                let (name, value, yes) = match predicate {
                    Predicate::Tier(want) => (
                        "binding.tier",
                        context
                            .tier
                            .as_ref()
                            .map(|s| PredicateValue::Text(s.clone())),
                        context.tier.as_ref() == Some(want),
                    ),
                    Predicate::Funding(want) => (
                        "binding.funding",
                        Some(PredicateValue::Text(context.funding.clone())),
                        context.funding == *want,
                    ),
                    Predicate::Priority(want) => (
                        "binding.priority",
                        context.priority.map(PredicateValue::Boolean),
                        context.priority == Some(*want),
                    ),
                    Predicate::Source(want) => (
                        "source",
                        Some(PredicateValue::Source(event.source().into())),
                        event.source() == want,
                    ),
                    Predicate::Status(want) => (
                        "status",
                        Some(PredicateValue::Text("succeeded".into())),
                        event.dto().status == Some(*want),
                    ),
                };
                matches &= yes;
                if let Some(value) = value {
                    predicate_inputs.push((name.into(), value));
                }
            }
            if !matches {
                steps.push(Step {
                    rule: Some(index),
                    code: "PREDICATE_FALSE",
                    unrounded: None,
                    rounded: None,
                    basis: None,
                    predicate_inputs,
                });
                continue;
            }
            let (code, unrounded, basis) = match &rule.operation {
                Operation::Base { fixed } => ("BASE_APPLIED", fixed.atoms_ratio(self.scale)?, None),
                Operation::Discount { percent, basis, .. } => {
                    let amount = booked[*basis];
                    if amount < 0 {
                        return Err(Error::new("NEGATIVE_BASIS", "discount"));
                    }
                    let ratio = ExactRatio::integer(amount);
                    (
                        "DISCOUNT_APPLIED",
                        ratio.mul(&percent.percent()?)?.negated(),
                        Some((*basis, ratio)),
                    )
                }
            };
            let rounded = unrounded.round_atoms()?;
            if let Some((basis, _)) = &basis {
                discounted[*basis] = add_atoms(discounted[*basis], rounded)?;
                if add_atoms(booked[*basis], discounted[*basis])? < 0 {
                    return Err(Error::new(
                        "DISCOUNT_EXCEEDS_BASIS",
                        "combined additive discounts",
                    ));
                }
            }
            booked[index] = rounded;
            if code == "BASE_APPLIED" {
                if let Some(code) = failure {
                    return Err(Error::new(code, "injected after completed base evaluation"));
                }
            }
            steps.push(Step {
                rule: Some(index),
                code: if rounded == 0 { "ZERO_ROUNDED" } else { code },
                unrounded: Some(unrounded),
                rounded: Some(rounded),
                basis,
                predicate_inputs,
            });
        }
        Ok(steps)
    }
}
/// Evaluate one fully resolved, validated input with no environmental access.
pub fn evaluate(input: &crate::domain::ResolvedInput) -> Result<crate::domain::DecisionPlan> {
    crate::domain::assemble(input)
}

/// Deterministic failure data for conformance tests; never reads external state.
#[cfg(feature = "test-failpoints")]
#[derive(Clone, Copy)]
pub enum EvaluationFault {
    InvalidAfterBase,
    OverflowAfterBase,
}
#[cfg(feature = "test-failpoints")]
pub fn evaluate_with_fault(
    input: &crate::domain::ResolvedInput,
    fault: EvaluationFault,
) -> Result<crate::domain::DecisionPlan> {
    crate::domain::assemble_inner(
        input,
        Some(match fault {
            EvaluationFault::InvalidAfterBase => "EVALUATION_INVALID",
            EvaluationFault::OverflowAfterBase => "ARITHMETIC_OVERFLOW",
        }),
    )
}
