//! Production pure engines perform every monetary calculation. Saved results are
//! comparison evidence only; the example never deserializes an outcome Decision.
use super::{
    CLASSIFICATION, Result,
    input::{self, Input, Prepared},
    require,
};
use ledgerlab_core::{
    domain::{self, Event},
    money::{Money, add_atoms},
    policy::chaining::{self as c, outcomes as o},
};
use serde_json::{Value, json};

pub struct Execution {
    pub base_bytes: Vec<u8>,
    pub computed: Value,
}
fn event(input: &Input, raw: &Value) -> Result<Event> {
    let chain = raw["chain"].as_str().ok_or("explicit chain missing")?;
    Ok(domain::normalize(
        &input::bytes(raw)?,
        input.scope.clone(),
        &input.source.source,
    )?
    .resolve(Some(chain))?)
}
fn costs(input: &Input, event: &Event) -> Result<Vec<c::CostEvidence>> {
    require(
        input.cost.event_external_id == event.dto().id
            && input.cost.customer == event.dto().customer
            && Some(&input.cost.quantity) == event.dto().quantity.as_ref()
            && Some(input.cost.unit.as_str()) == event.dto().unit.as_deref(),
        "provider cost/work association",
    )?;
    Ok(vec![c::CostEvidence {
        binding_id: input.cost.binding_id.clone(),
        event_id: event.id().into(),
        document: input::doc(&input.cost)?,
        amount: Money::new(
            &input.currency,
            input.scale,
            input.cost.observed_amount.atoms_exact(input.scale)?,
        )?,
    }])
}
fn evaluate_base(
    input: &Input,
    p: &Prepared,
    event: &Event,
    costs: &[c::CostEvidence],
) -> Result<c::Evaluation> {
    Ok(p.bundle.evaluate(c::Input {
        event,
        context: &p.context,
        history: &[],
        source_authority: &p.source_authority,
        invocations: &[],
        costs,
        received_at: &input.received_at,
    })?)
}
fn request(
    input: &Input,
    p: &Prepared,
    branch: &input::BranchInput,
    base: &c::Evaluation,
) -> Result<(o::Request, o::Verified)> {
    let e = &branch.evidence;
    require(
        e.schema == "bean-counter-synthetic-assessment/1"
            && e.classification == CLASSIFICATION
            && e.scope == input.scope
            && e.customer == input.customer
            && e.source == input.outcome.source
            && e.family == input.outcome.family
            && e.target_external_id == base.event().dto().id
            && e.assessment == branch.name
            && e.code == branch.code
            && e.occurred_at == branch.occurred_at,
        "synthetic assessment/target/source relationship",
    )?;
    require(
        !input.outcome.principal.is_empty()
            && input.outcome.principal.len() <= 128
            && input.outcome.codes.iter().any(|c| c.code == branch.code),
        "synthetic outcome principal/code",
    )?;
    let evidence = vec![input::doc(e)?];
    let binding = base
        .bundle()
        .policies()
        .iter()
        .find(|p| p.binding.id == input.outcome.binding_id)
        .ok_or("target binding missing")?;
    let req = o::Request {
        scope: input.scope.clone(),
        id: branch.id.clone(),
        target: base.event().id().into(),
        agreement: binding.binding.agreement.clone(),
        family: input.outcome.family.clone(),
        source: input.outcome.source.clone(),
        occurred_at: branch.occurred_at.clone(),
        evidence: evidence.clone(),
        change: o::Change::Claim {
            code: branch.code.clone(),
        },
    };
    // These observations follow exact local retained-byte associations above.
    // They establish synthetic DATA consistency, never authentication/assent.
    let verified = o::Verified {
        scope: req.scope.clone(),
        target: req.target.clone(),
        agreement: req.agreement.clone(),
        family: req.family.clone(),
        source: req.source.clone(),
        principal: input.outcome.principal.clone(),
        grant: p.outcome_grant_ref.clone(),
        grant_revision: input.outcome.grant_revision,
        active: true,
        may_read: true,
        may_submit: true,
        may_correct: false,
        verified_evidence: evidence,
        received_at: branch.received_at.clone(),
        accepted_at: branch.accepted_at.clone(),
    };
    Ok((req, verified))
}
fn decision(
    req: &o::Request,
    v: &o::Verified,
    target: &o::Target,
    base: &c::Evaluation,
    history: &[o::Decision],
) -> Result<o::Decision> {
    match o::evaluate(
        req,
        v,
        std::slice::from_ref(target),
        std::slice::from_ref(base),
        history,
    )? {
        o::Submission::Accepted(d) => Ok(*d),
        o::Submission::Duplicate(_) => Err("unexpected duplicate in isolated alternative".into()),
    }
}
fn must_refuse<T>(result: ledgerlab_core::Result<T>, code: &str) -> Result<String> {
    match result {
        Err(e) if e.code == code => Ok(e.code.into()),
        Err(e) => Err(format!("negative expected {code}, got {e}").into()),
        Ok(_) => Err(format!("negative unexpectedly passed: {code}").into()),
    }
}
pub fn execute(input: &Input, p: &Prepared, retained: Option<&[u8]>) -> Result<Execution> {
    let work = event(input, &input.work)?;
    require(
        work.dto().customer == input.customer
            && work.dto().status == Some(ledgerlab_core::wire::Completion::Succeeded),
        "only completed successful billable work can be frozen",
    )?;
    let costs = costs(input, &work)?;
    let computed_base = evaluate_base(input, p, &work, &costs)?;
    let base_bytes = input::bytes(&computed_base)?;
    let base = if let Some(raw) = retained {
        // decode_evaluation itself recompiles and re-evaluates all saved inputs.
        let replay = c::retained::decode_evaluation(raw, &[])?;
        require(
            input::bytes(&replay)? == base_bytes,
            "saved base differs from acknowledged original input",
        )?;
        replay
    } else {
        computed_base
    };
    let target = o::Target::freeze(
        &base,
        p.outcome_policy.clone(),
        o::TargetVerification {
            rated_final: true,
            accepted_at: input.accepted_at.clone(),
            policy_document: p.terms_ref.clone(),
            verified_assents: vec![p.acknowledgement_ref.clone()],
            verified_offers: vec![],
            verified_delegations: vec![],
        },
    )?;
    let mut retail_atoms = 0;
    let mut model_atoms = None;
    let mut observed_atoms = None;
    let mut components = Vec::new();
    for action in base.actions() {
        if action.book() == c::Book::Retail {
            retail_atoms = add_atoms(retail_atoms, action.amount().atoms())?;
            if action.component() == input.cost.model_component {
                model_atoms = Some(action.amount().atoms());
            }
        }
        if action.book() == c::Book::CostObservation {
            observed_atoms = Some(action.amount().atoms());
        }
        let explanation = base
            .explanations()
            .iter()
            .find(|e| e.action_ids.iter().any(|id| id == action.id()))
            .ok_or("action explanation missing")?;
        let policy = base
            .bundle()
            .policies()
            .iter()
            .find(|p| p.binding.id == action.binding().id)
            .ok_or("action policy missing")?;
        let rule = policy
            .rules
            .iter()
            .find(|r| r.component == action.component())
            .ok_or("action rule missing")?;
        components.push(json!({"action_id":action.id(),"effect_id":action.effect_id(),
            "component":action.component(),"book":action.book(),"kind":action.kind(),
            "amount_atoms":action.amount().atoms().to_string(),"policy_terms_ref":p.terms_ref,
            "rule":rule,"quantity":base.event().dto().quantity,"unit":base.event().dto().unit,
            "provider_original_unit_rate":input.cost.original_unit_rate,"explanation":explanation,
            "binding":action.binding(),"evidence_refs":action.inputs(),"sources":action.sources(),"links":action.links()}));
    }
    let model_atoms = model_atoms.ok_or("retail model output missing")?;
    let observed_atoms = observed_atoms.ok_or("provider observation missing")?;
    require(
        model_atoms == observed_atoms,
        "original provider rate does not equal observed cost; no arbitrary-invoice pass-through",
    )?;
    require(
        retail_atoms == target.retail_basis().atoms(),
        "frozen payable basis mismatch",
    )?;
    let mut alternatives = Vec::new();
    let mut decisions = Vec::new();
    let mut reqs = Vec::new();
    for branch in &input.outcome.branches {
        let (req, v) = request(input, p, branch, &base)?;
        let d = decision(&req, &v, &target, &base, &[])?; // isolated histories, one claim each
        let percentage = target.policy().families[0]
            .codes
            .iter()
            .find(|c| c.code == branch.code)
            .ok_or("frozen code missing")?;
        let o::Amount::Percent(percent) = &percentage.amount else {
            return Err("unsupported fixed outcome in this demo".into());
        };
        let explanations=d.explanations().iter().map(|e|json!({"code":e.code,"basis_atoms":e.basis.atoms().to_string(),
            "exact":e.exact,"rounded_atoms":e.rounded.atoms().to_string(),"currency":e.rounded.currency(),"scale":e.rounded.scale()})).collect::<Vec<_>>();
        let postings=d.postings().iter().map(|x|json!({"amount_atoms":x.amount.atoms().to_string(),
            "currency":x.amount.currency(),"scale":x.amount.scale(),"reverses_revision":x.reverses_revision})).collect::<Vec<_>>();
        alternatives.push(json!({"name":branch.name,"code":branch.code,"family":d.key().family,"target":d.key().target,
            "agreement":d.key().agreement,"scope":d.key().scope,"revision":d.revision().to_string(),
            "policy_terms_ref":p.terms_ref,"basis_atoms":target.retail_basis().atoms().to_string(),"percentage":percent,
            "delta_atoms":d.current().atoms().to_string(),"total_atoms":add_atoms(retail_atoms,d.current().atoms())?.to_string(),
            "postings":postings,"explanations":explanations,"request":{"id":req.id,"target":req.target,
                "family":req.family,"agreement":req.agreement,"source":req.source,"code":branch.code,
                "occurred_at":req.occurred_at,"evidence_refs":req.evidence},
            "synthetic_observation":{"principal":v.principal,"grant":v.grant,"grant_revision":v.grant_revision,
                "received_at":v.received_at,"accepted_at":v.accepted_at,"classification":CLASSIFICATION},
            "original_roles":d.binding().roles}));
        reqs.push((req, v));
        decisions.push(d);
    }
    require(
        reqs[0].0.change != reqs[1].0.change && reqs[0].0.id != reqs[1].0.id,
        "alternatives must have distinct codes and request identities",
    )?;
    let mut wrongtarget = reqs[0].0.clone();
    let mut wrongverified = reqs[0].1.clone();
    let mut otherwork = input.work.clone();
    otherwork["id"] = json!("wrong-target-negative");
    wrongtarget.target = event(input, &otherwork)?.id().into();
    wrongverified.target = wrongtarget.target.clone();
    let wrong_target = must_refuse(
        o::evaluate(
            &wrongtarget,
            &wrongverified,
            std::slice::from_ref(&target),
            std::slice::from_ref(&base),
            &[],
        ),
        "WAITING_DEPENDENCIES",
    )?;
    let mut wrongevidence = reqs[0].0.clone();
    wrongevidence.evidence = vec![input::doc(
        &json!({"schema":"synthetic-unverified-evidence/1","classification":CLASSIFICATION}),
    )?];
    let wrong_evidence = must_refuse(
        o::evaluate(
            &wrongevidence,
            &reqs[0].1,
            std::slice::from_ref(&target),
            std::slice::from_ref(&base),
            &[],
        ),
        "OUTCOME_EVIDENCE_REQUIRED",
    )?;
    let mut wrongunit = input.work.clone();
    wrongunit["unit"] = json!("different-unit");
    let wrong_unit = must_refuse(
        p.bundle.evaluate(c::Input {
            event: &event(input, &wrongunit)?,
            context: &p.context,
            history: &[],
            source_authority: &p.source_authority,
            invocations: &[],
            costs: &[],
            received_at: &input.received_at,
        }),
        "UNIT_MISMATCH",
    )?;
    let exclusive = must_refuse(
        o::evaluate(
            &reqs[1].0,
            &reqs[1].1,
            std::slice::from_ref(&target),
            std::slice::from_ref(&base),
            std::slice::from_ref(&decisions[0]),
        ),
        "CLAIM_CONFLICT",
    )?;
    let computed = json!({"classification":CLASSIFICATION,"financial_admission":false,
        "policy_terms_ref":p.terms_ref,"acknowledgement_ref":p.acknowledgement_ref,
        "base_target":base.event().id(),"currency":input.currency,"scale":input.scale,
        "base":{"components":components,"retail_total_atoms":retail_atoms.to_string(),
            "model_component_atoms":model_atoms.to_string(),"cost_observation_atoms":observed_atoms.to_string(),
            "observation_is_payable":false,"canonical_evaluation_sha256":super::archive::hash(&base_bytes)},
        "alternatives":alternatives,"alternatives_are_isolated":true,
        "negative_checks":{"wrong_target":true,"wrong_evidence":true,"wrong_unit":true,"exclusive_second_claim":true},
        "negative_errors":{"wrong_target":wrong_target,"wrong_evidence":wrong_evidence,"wrong_unit":wrong_unit,"exclusive_second_claim":exclusive}});
    Ok(Execution {
        base_bytes,
        computed,
    })
}
