//! Closed example-local artifact inputs; these are not public wire/authority DTOs.
use super::{CLASSIFICATION, Result, require};
use ledgerlab_core::{
    canonical::{self, CanonicalBytes, Domain},
    domain::{Revision, Roles, Scope, Timestamp},
    money::{Decimal, ExactRatio, Money},
    policy::chaining::{self as c, outcomes as o},
    wire::{EventKind, Relation},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub classification: String,
    pub scope: Scope,
    pub customer: String,
    pub acceptor: String,
    pub currency: String,
    pub scale: u8,
    pub policies: Vec<PolicyInput>,
    pub context: ContextInput,
    pub acknowledgement_at: Timestamp,
    pub work: Value,
    pub received_at: Timestamp,
    pub accepted_at: Timestamp,
    pub source: SourceInput,
    pub cost: CostInput,
    pub outcome: OutcomeInput,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyInput {
    pub id: String,
    pub agreement: String,
    pub book: c::Book,
    pub roles: [String; 6],
    pub sources: Vec<String>,
    pub event_types: Vec<EventKind>,
    pub unit: String,
    pub maximum_quantity: Decimal,
    pub correction_sources: Vec<String>,
    pub rules: Vec<c::Rule>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextInput {
    pub funding: c::Funding,
    pub tier: Option<String>,
    pub priority: Option<bool>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub principal: String,
    pub source: String,
    pub revision: Revision,
    pub event_types: Vec<EventKind>,
    pub relations: Vec<Relation>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostInput {
    pub schema: String,
    pub classification: String,
    pub scope: Scope,
    pub customer: String,
    pub provider: String,
    pub binding_id: String,
    pub event_external_id: String,
    pub model_component: String,
    pub quantity: Decimal,
    pub unit: String,
    pub original_unit_rate: Decimal,
    pub observed_amount: Decimal,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowInput {
    pub starts_at: Timestamp,
    pub occurs_before: Timestamp,
    pub received_by: Timestamp,
    pub accepted_by: Timestamp,
}
impl WindowInput {
    pub fn value(&self) -> o::Window {
        o::Window {
            starts_at: self.starts_at.clone(),
            occurs_before: self.occurs_before.clone(),
            received_by: self.received_by.clone(),
            accepted_by: self.accepted_by.clone(),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeInput {
    pub code: String,
    pub percent: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeInput {
    pub version: String,
    pub family: String,
    pub binding_id: String,
    pub source: String,
    pub correction_source: String,
    pub ordinary: WindowInput,
    pub corrections: WindowInput,
    pub codes: Vec<CodeInput>,
    pub premium_ceiling: Decimal,
    pub evidence_required: bool,
    pub replacement_codes: Vec<String>,
    pub allow_reversal: bool,
    pub principal: String,
    pub grant_revision: Revision,
    pub branches: Vec<BranchInput>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchInput {
    pub name: String,
    pub id: String,
    pub code: String,
    pub occurred_at: Timestamp,
    pub received_at: Timestamp,
    pub accepted_at: Timestamp,
    pub evidence: AssessmentEvidence,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentEvidence {
    pub schema: String,
    pub classification: String,
    pub scope: Scope,
    pub customer: String,
    pub source: String,
    pub family: String,
    pub target_external_id: String,
    pub assessment: String,
    pub code: String,
    pub occurred_at: Timestamp,
}
pub fn bytes<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    Ok(CanonicalBytes::from_value(v)?.into_vec())
}
pub fn doc<T: Serialize>(v: &T) -> Result<String> {
    Ok(canonical::identity(Domain::Document, v)?)
}
pub fn parse(raw: &[u8]) -> Result<Input> {
    let value = canonical::parse_bounded(raw, 1_048_576)?;
    let input: Input = serde_json::from_value(value)?;
    require(
        input.schema == "bean-counter-billing-journey-input/1"
            && input.classification == CLASSIFICATION,
        "unsupported synthetic input classification",
    )?;
    require(
        !input.customer.is_empty()
            && !input.acceptor.is_empty()
            && input.customer.len() <= 128
            && input.acceptor.len() <= 128,
        "customer/acceptor identity",
    )?;
    require(
        input.policies.len() == 2
            && input.outcome.branches.len() == 2
            && input.outcome.codes.len() == 2,
        "only this two-binding/two-alternative demonstration is supported",
    )?;
    require(
        input.context.funding == c::Funding::Platform,
        "provider-rate demonstration requires platform funding",
    )?;
    require(
        input.acknowledgement_at.micros() <= input.accepted_at.micros(),
        "acknowledgement after base acceptance",
    )?;
    Ok(input)
}
pub struct Prepared {
    pub terms: Value,
    pub terms_ref: String,
    pub acknowledgement: Value,
    pub acknowledgement_ref: String,
    pub evidence: Value,
    pub bundle: c::Bundle,
    pub context: c::Context,
    pub source_authority: c::SourceAuthority,
    pub outcome_policy: o::Policy,
    pub outcome_grant_ref: String,
}
pub fn prepare(input: &Input) -> Result<Prepared> {
    let terms = json!({"schema":"bean-counter-synthetic-original-terms/1", "classification":CLASSIFICATION,
        "scope":input.scope,"customer":input.customer,"acceptor":input.acceptor,
        "currency":input.currency,"scale":input.scale,"policies":input.policies,"outcome":input.outcome});
    let terms_ref = doc(&terms)?;
    let acknowledgement = json!({"schema":"bean-counter-synthetic-policy-acknowledgement/1",
        "classification":CLASSIFICATION,"scope":input.scope,"customer":input.customer,
        "acceptor":input.acceptor,"policy_terms_ref":terms_ref,"acknowledged_at":input.acknowledgement_at});
    let acknowledgement_ref = doc(&acknowledgement)?;
    let mut policies = Vec::new();
    for p in &input.policies {
        let parties = p.roles.each_ref().map(String::as_str);
        let roles = Roles::new(parties, None)?;
        require(
            p.roles[2] == input.customer && p.roles[3] == input.customer,
            "synthetic terms bearer/payer must match acknowledged customer",
        )?;
        policies.push(c::Policy {
            binding: c::Binding {
                id: p.id.clone(),
                agreement: p.agreement.clone(),
                book: p.book,
                roles,
                assent: acknowledgement_ref.clone(),
                offer: None,
                sources: p.sources.clone(),
                event_types: p.event_types.clone(),
                unit: p.unit.clone(),
                maximum_quantity: p.maximum_quantity.clone(),
                maximum_exposure: None,
                outcome: None,
                correction_sources: p.correction_sources.clone(),
                allowed_modifiers: vec![],
                allocation_view: false,
            },
            rules: p.rules.clone(),
        });
    }
    let bundle = c::Bundle::compile(&input.currency, input.scale, policies)?;
    let retail = bundle
        .policies()
        .iter()
        .find(|p| p.binding.book == c::Book::Retail)
        .ok_or("retail binding missing")?;
    let observation = bundle
        .policies()
        .iter()
        .find(|p| p.binding.book == c::Book::CostObservation)
        .ok_or("cost observation binding missing")?;
    require(
        retail.binding.id == input.outcome.binding_id
            && retail.binding.roles.payer() == input.customer,
        "acknowledged outcome binding/customer mismatch",
    )?;
    require(
        retail.rules.len() == 2 && observation.rules.len() == 1,
        "unsupported policy composition",
    )?;
    require(
        observation.binding.id == input.cost.binding_id,
        "cost binding association",
    )?;
    let model = retail
        .rules
        .iter()
        .find(|r| r.component == input.cost.model_component)
        .ok_or("model component missing")?;
    let c::Operation::Base(c::Price::Unit { rate, unit }) = &model.operation else {
        return Err("model cost must use saved original unit price".into());
    };
    require(
        rate == &input.cost.original_unit_rate && unit == &input.cost.unit,
        "provider evidence unit rate differs from acknowledged terms",
    )?;
    require(
        retail
            .rules
            .iter()
            .filter(|r| matches!(r.operation, c::Operation::Base(c::Price::Fixed(_))))
            .count()
            == 1
            && matches!(observation.rules[0].operation, c::Operation::ObserveCost),
        "unsupported base/cost composition",
    )?;
    require(
        input.cost.schema == "bean-counter-synthetic-provider-cost/1"
            && input.cost.classification == CLASSIFICATION
            && input.cost.scope == input.scope
            && input.cost.customer == input.customer
            && input.cost.provider
                == input
                    .policies
                    .iter()
                    .find(|p| p.id == input.cost.binding_id)
                    .ok_or("cost policy missing")?
                    .roles[0],
        "synthetic provider-cost identity mismatch",
    )?;
    let context_body = json!({"schema":"bean-counter-synthetic-context/1","classification":CLASSIFICATION,
        "scope":input.scope,"customer":input.customer,"context":input.context});
    let context = c::Context {
        document: doc(&context_body)?,
        customer: input.customer.clone(),
        funding: input.context.funding,
        tier: input.context.tier.clone(),
        priority: input.context.priority,
        stage: None,
    };
    let source_body = json!({"schema":"bean-counter-synthetic-work-grant/1","classification":CLASSIFICATION,
        "scope":input.scope,"customer":input.customer,"grant":input.source});
    let source_authority = c::SourceAuthority {
        source: input.source.source.clone(),
        grant: doc(&source_body)?,
        revision: input.source.revision,
        active: true,
        event_types: input.source.event_types.clone(),
        relations: input.source.relations.clone(),
    };
    let outcome_grant = json!({"schema":"bean-counter-synthetic-outcome-grant/1","classification":CLASSIFICATION,
        "scope":input.scope,"customer":input.customer,"principal":input.outcome.principal,
        "source":input.outcome.source,"correction_source":input.outcome.correction_source,
        "revision":input.outcome.grant_revision,"family":input.outcome.family,
        "policy_terms_ref":terms_ref,"may_read":true,"may_submit":true,"may_correct":false});
    let outcome_grant_ref = doc(&outcome_grant)?;
    let codes = input
        .outcome
        .codes
        .iter()
        .map(|c| {
            Ok(o::Code {
                code: c.code.clone(),
                amount: o::Amount::Percent(ExactRatio::from_canonical(&c.percent, "1")?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let outcome_policy = o::Policy {
        version: input.outcome.version.clone(),
        document: terms_ref.clone(),
        families: vec![o::Family {
            family: input.outcome.family.clone(),
            binding_id: input.outcome.binding_id.clone(),
            source: input.outcome.source.clone(),
            correction_source: input.outcome.correction_source.clone(),
            evidence_required: input.outcome.evidence_required,
            ordinary: input.outcome.ordinary.value(),
            corrections: input.outcome.corrections.value(),
            codes,
            replacement_codes: input.outcome.replacement_codes.clone(),
            allow_reversal: input.outcome.allow_reversal,
        }],
        limits: vec![o::Limit {
            binding_id: input.outcome.binding_id.clone(),
            premium: Money::new(
                &input.currency,
                input.scale,
                input.outcome.premium_ceiling.atoms_exact(input.scale)?,
            )?,
        }],
    };
    let evidence = json!({"work_grant":{"ref":source_authority.grant,"body":source_body},
        "outcome_grant":{"ref":outcome_grant_ref,"body":outcome_grant},
        "context":{"ref":context.document,"body":context_body},
        "provider_cost":{"ref":doc(&input.cost)?,"body":input.cost},
        "assessments":input.outcome.branches.iter().map(|b| Ok(json!({"ref":doc(&b.evidence)?,"body":b.evidence})))
            .collect::<Result<Vec<Value>>>()?});
    Ok(Prepared {
        terms,
        terms_ref,
        acknowledgement,
        acknowledgement_ref,
        evidence,
        bundle,
        context,
        source_authority,
        outcome_policy,
        outcome_grant_ref,
    })
}
