use super::compile::require;
use super::model::*;
use crate::canonical::{self, Domain};
use crate::domain::{prefixed, slug, text, validate_source, Event};
use crate::money::{add_atoms, ExactRatio, Money};
use crate::wire::{Completion, EventKind, Relation};
use crate::{Error, Result};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

impl Bundle {
    pub fn evaluate(&self, input: Input<'_>) -> Result<Evaluation> {
        let event = input.event;
        require(
            event.dto().kind.is_work() || event.dto().kind == EventKind::Acquired,
            "UNSUPPORTED_SLICE",
            "use reverse for reversal; late assertions require later records",
        )?;
        validate_authority(event, input.source_authority)?;
        validate_history(event, input.history)?;
        validate_context(self, &input)?;
        validate_links(event, input.history)?;
        if let Some(id) = &event.dto().binding_id {
            require(
                self.policies.iter().any(|p| p.binding.id == *id),
                "TERMS_NOT_ACCEPTED",
                "explicit binding outside pinned set",
            )?;
        }
        if let Some(id) = &event.dto().corrects {
            let original = input
                .history
                .iter()
                .find(|d| d.event.id() == id)
                .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "correction target"))?;
            require(
                !original.actions.is_empty()
                    && original
                        .actions
                        .iter()
                        .all(|a| is_reversed(&a.id, input.history)),
                "CORRECTION_REQUIRED",
                "replacement requires full original reversal",
            )?;
        }
        require(
            self.policies[0]
                .binding
                .event_types
                .contains(&event.dto().kind),
            "TERMS_NOT_ACCEPTED",
            "retail event scope",
        )?;
        validate_binding(&self.policies[0].binding, &input, None)?;
        let claim_id = claim_id(event)?;
        require(
            !input
                .history
                .iter()
                .any(|d| d.claim_id.as_deref() == Some(&claim_id)),
            "CLAIM_CONFLICT",
            "coordinator must resolve existing semantic claim before evaluating",
        )?;
        require(
            input.invocations.len() <= 16 && input.costs.len() <= 16,
            "LIMIT",
            "resolved authorities",
        )?;
        let mut invocation_ids = BTreeSet::new();
        for i in input.invocations {
            require(
                invocation_ids.insert(&i.id),
                "INVOCATION_CONFLICT",
                "duplicate invocation",
            )?;
        }
        let mut costs = BTreeSet::new();
        for c in input.costs {
            require(
                costs.insert((&c.binding_id, &c.event_id)),
                "COST_EVIDENCE",
                "ambiguous cost",
            )?;
        }
        let mut result = Evaluation {
            event: event.clone(),
            bundle: self.clone(),
            context: input.context.clone(),
            claim_id: Some(claim_id),
            actions: vec![],
            explanations: vec![],
            deltas: vec![],
            consumptions: vec![],
            invocations: input.invocations.to_vec(),
            closed_stage: None,
        };
        if event.dto().status == Some(Completion::Failed) {
            // Even failed work must be inside its accepted source/unit/quantity
            // scope. Supplier authority is still needed to release a reservation.
            for policy in &self.policies {
                if policy.rules.iter().any(|r| r.on == event.dto().kind) {
                    validate_binding(&policy.binding, &input, None)?;
                }
            }
            result.explanations.push(Explanation {
                binding_id: self.policies[0].binding.id.clone(),
                rule_id: None,
                code: "FAILED_WORK",
                basis: None,
                unrounded: None,
                rounded: None,
                inputs: vec![],
                action_ids: vec![],
            });
            result.consumptions = consumptions(self, &input, &result)?;
            return Ok(result);
        }
        // Booked values retain zero/skipped slots so predicates cannot create a
        // missing-basis panic. Actual action provenance is collected separately.
        let mut booked: BTreeMap<(usize, String), i128> = BTreeMap::new();
        let mut adjustments: BTreeMap<(usize, String), i128> = BTreeMap::new();
        let mut net_inputs: BTreeMap<(usize, String), Vec<String>> = BTreeMap::new();
        for &(pi, ri) in &self.order {
            let policy = &self.policies[pi];
            let b = &policy.binding;
            let rule = &policy.rules[ri];
            if rule.on != event.dto().kind {
                continue;
            }
            let key = (pi, rule.component.clone());
            booked.insert(key.clone(), 0);
            let mut step = Explanation {
                binding_id: b.id.clone(),
                rule_id: Some(rule.id.clone()),
                code: "PREDICATE_FALSE",
                basis: None,
                unrounded: None,
                rounded: None,
                inputs: vec![],
                action_ids: vec![],
            };
            if !rule.when.iter().all(|p| predicate(p, &input)) {
                result.explanations.push(step);
                continue;
            }
            let matched = match_rule(event, rule.matcher, input.history)?;
            if rule.matcher.is_some() && matched.is_none() {
                step.code = "NO_MATCH";
                result.explanations.push(step);
                continue;
            }
            validate_binding(b, &input, matched.as_ref().map(|m| m.target))?;
            let mut sources = vec![event.id().to_owned()];
            let mut links = Vec::new();
            if let Some(m) = &matched {
                sources.extend(m.sources.clone());
                links = m.links.clone();
            }
            let mut basis_target = None;
            let mut discount_target = None;
            let mut share_basis = None;
            let (kind, unrounded, override_atoms) = match &rule.operation {
                Operation::Base(price) | Operation::Premium(price) => {
                    let premium = matches!(rule.operation, Operation::Premium(_));
                    step.code = if premium {
                        "PREMIUM_APPLIED"
                    } else {
                        "BASE_APPLIED"
                    };
                    let amount = price_value(
                        price,
                        self.scale,
                        event,
                        pi,
                        &booked,
                        &result.actions,
                        &mut step,
                    )?;
                    (
                        if premium {
                            ActionKind::Premium
                        } else if b.book == Book::Retail {
                            ActionKind::Charge
                        } else {
                            ActionKind::Cost
                        },
                        amount,
                        None,
                    )
                }
                Operation::Discount {
                    amount,
                    component,
                    mode,
                } => {
                    let target = (pi, component.clone());
                    let base = *booked
                        .get(&target)
                        .ok_or_else(|| Error::new("POLICY_MISSING_BASIS", component))?;
                    let net = add_atoms(base, *adjustments.get(&target).unwrap_or(&0))?;
                    let basis = if *mode == DiscountMode::Sequential {
                        net
                    } else {
                        base
                    };
                    let original = result
                        .actions
                        .iter()
                        .find(|a| a.binding.id == b.id && a.component == *component);
                    if let Some(a) = original {
                        step.inputs.push(a.id.clone());
                        discount_target = Some(a.id.clone());
                    }
                    if *mode == DiscountMode::Sequential {
                        step.inputs
                            .extend(net_inputs.get(&target).cloned().unwrap_or_default());
                    }
                    step.basis = Some(ExactRatio::integer(basis));
                    step.code = "DISCOUNT_APPLIED";
                    basis_target = Some((target, net));
                    (
                        ActionKind::Discount,
                        discount_value(amount, self.scale, basis)?,
                        None,
                    )
                }
                Operation::LinkedDiscount { amount, component } => {
                    let target = matched.as_ref().expect("compiled matcher").target;
                    let original = target
                        .actions
                        .iter()
                        .find(|a| {
                            a.binding.agreement == b.agreement
                                && a.book == Book::Retail
                                && a.component == *component
                                && matches!(a.kind, ActionKind::Charge | ActionKind::Premium)
                        })
                        .ok_or_else(|| {
                            Error::new("POLICY_MISSING_BASIS", "matched booked component")
                        })?;
                    require(
                        !is_reversed(&original.id, input.history),
                        "ALREADY_REVERSED",
                        "discount basis",
                    )?;
                    let mut net = original.amount.atoms();
                    step.inputs.push(original.id.clone());
                    for d in input.history {
                        for a in &d.actions {
                            if a.discount_target.as_ref() == Some(&original.id)
                                && !is_reversed(&a.id, input.history)
                            {
                                net = add_atoms(net, a.amount.atoms())?;
                                step.inputs.push(a.id.clone());
                            }
                        }
                    }
                    for a in &result.actions {
                        if a.discount_target.as_ref() == Some(&original.id) {
                            net = add_atoms(net, a.amount.atoms())?;
                            step.inputs.push(a.id.clone());
                        }
                    }
                    require(net >= 0, "NEGATIVE_BASIS", "linked component net")?;
                    let ratio = discount_value(amount, self.scale, original.amount.atoms())?;
                    require(
                        add_atoms(net, ratio.round_atoms()?)? >= 0,
                        "DISCOUNT_EXCEEDS_BASIS",
                        "linked component remaining balance",
                    )?;
                    step.basis = Some(ExactRatio::integer(original.amount.atoms()));
                    step.code = "DISCOUNT_APPLIED";
                    discount_target = Some(original.id.clone());
                    (ActionKind::Discount, ratio, None)
                }
                Operation::Cap {
                    ceiling,
                    stage,
                    component,
                } => {
                    let (prior, refs) = stage_prior(self, &input, stage)?;
                    let target = (pi, component.clone());
                    let current = *booked
                        .get(&target)
                        .ok_or_else(|| Error::new("POLICY_MISSING_BASIS", component))?;
                    // The cap's C is the named closure component's booked net.
                    let current = add_atoms(current, *adjustments.get(&target).unwrap_or(&0))?;
                    let cap = ceiling.atoms_exact(self.scale)?;
                    require(
                        cap >= prior && current >= 0,
                        "CAP_BELOW_BOOKED",
                        "cap cannot cancel prior obligations",
                    )?;
                    let total = add_atoms(prior, current)?;
                    let credit = if total > cap { -(total - cap) } else { 0 };
                    require(
                        -credit <= current,
                        "CAP_BELOW_BOOKED",
                        "credit exceeds current component",
                    )?;
                    step.inputs = refs;
                    step.inputs
                        .extend(component_inputs(&result.actions, &b.id, component));
                    step.inputs
                        .extend(net_inputs.get(&target).cloned().unwrap_or_default());
                    step.basis = Some(ExactRatio::integer(current));
                    step.code = if credit == 0 {
                        "CAP_NOT_BINDING"
                    } else {
                        "CAP_APPLIED"
                    };
                    basis_target = Some((target, current));
                    result.closed_stage = Some(stage.clone());
                    (ActionKind::Credit, ExactRatio::integer(credit), None)
                }
                Operation::Share {
                    percent,
                    ceiling,
                    retail_component,
                } => {
                    let target = (0, retail_component.clone());
                    let basis = add_atoms(
                        *booked
                            .get(&target)
                            .ok_or_else(|| Error::new("POLICY_MISSING_BASIS", retail_component))?,
                        *adjustments.get(&target).unwrap_or(&0),
                    )?;
                    require(basis >= 0, "NEGATIVE_BASIS", "share")?;
                    let ratio = ExactRatio::integer(basis).mul(&percent.percent()?)?;
                    let raw = ratio.round_atoms()?;
                    let limited = raw.min(ceiling.atoms_exact(self.scale)?);
                    step.code = if limited < raw {
                        "SHARE_CEILING"
                    } else {
                        "SHARE_APPLIED"
                    };
                    step.basis = Some(ExactRatio::integer(basis));
                    step.inputs = component_inputs(
                        &result.actions,
                        &self.policies[0].binding.id,
                        retail_component,
                    );
                    step.inputs
                        .extend(net_inputs.get(&target).cloned().unwrap_or_default());
                    share_basis = Some(basis);
                    (ActionKind::Share, ratio, Some(limited))
                }
                Operation::ObserveCost => {
                    if input.context.funding == Funding::Byok {
                        step.code = "BYOK_NO_HOST_COST";
                        result.explanations.push(step);
                        continue;
                    }
                    let Some(cost) = input
                        .costs
                        .iter()
                        .find(|c| c.binding_id == b.id && c.event_id == event.id())
                    else {
                        step.code = "COST_UNKNOWN";
                        result.explanations.push(step);
                        continue;
                    };
                    prefixed(&cost.document, "doc_")?;
                    require(
                        cost.amount.currency() == self.currency
                            && cost.amount.scale() == self.scale
                            && cost.amount.atoms() >= 0,
                        "COST_EVIDENCE",
                        "retained known cost currency/amount",
                    )?;
                    step.inputs.push(cost.document.clone());
                    step.code = "COST_OBSERVED";
                    (
                        ActionKind::Cost,
                        ExactRatio::integer(cost.amount.atoms()),
                        None,
                    )
                }
            };
            let atoms = override_atoms.unwrap_or(unrounded.round_atoms()?);
            if let Some((target, net)) = basis_target {
                require(
                    add_atoms(net, atoms)? >= 0,
                    "DISCOUNT_EXCEEDS_BASIS",
                    "combined component reductions",
                )?;
                let value = add_atoms(*adjustments.get(&target).unwrap_or(&0), atoms)?;
                adjustments.insert(target.clone(), value);
                // Fill the action ref after constructing it below.
                if atoms != 0 {
                    net_inputs.entry(target).or_default().push(
                        action_id(
                            event,
                            result.claim_id.as_deref().expect("original claim"),
                            b,
                            &rule.component,
                            &links,
                        )?
                        .1,
                    );
                }
            }
            booked.insert(key, atoms);
            step.inputs.sort();
            step.inputs.dedup();
            for id in &step.inputs {
                if let Some(a) = input
                    .history
                    .iter()
                    .flat_map(|d| &d.actions)
                    .find(|a| &a.id == id)
                {
                    sources.extend(a.sources.clone());
                }
            }
            step.unrounded = Some(unrounded);
            step.rounded = Some(atoms);
            if atoms != 0 {
                let dependencies = step
                    .inputs
                    .iter()
                    .filter(|id| id.starts_with("ac_"))
                    .cloned()
                    .collect();
                let action = make_action(
                    self,
                    event,
                    result.claim_id.as_deref().expect("original claim"),
                    b,
                    kind,
                    b.book,
                    &rule.component,
                    atoms,
                    sources,
                    links,
                    dependencies,
                    discount_target,
                )?;
                step.action_ids.push(action.id.clone());
                result.actions.push(action);
                if let Some(basis) = share_basis.filter(|_| b.allocation_view) {
                    let share = result.actions.last().expect("share").clone();
                    for (suffix, amount) in [("supplier", atoms), ("host", basis - atoms)] {
                        if amount == 0 {
                            continue;
                        }
                        let mut a = make_action(
                            self,
                            event,
                            result.claim_id.as_deref().expect("original claim"),
                            b,
                            ActionKind::Allocation,
                            Book::Allocation,
                            &format!("{}.{suffix}", rule.component),
                            amount,
                            share.sources.clone(),
                            share.links.clone(),
                            share.inputs.clone(),
                            None,
                        )?;
                        a.allocation_recipient = Some(
                            if suffix == "supplier" {
                                b.roles.recipient()
                            } else {
                                self.policies[0].binding.roles.recipient()
                            }
                            .into(),
                        );
                        a.allocation_parent = Some(share.id.clone());
                        a.inputs.push(share.id.clone());
                        a.inputs.sort();
                        a.inputs.dedup();
                        step.action_ids.push(a.id.clone());
                        result.actions.push(a);
                    }
                }
            } else if step.code != "CAP_NOT_BINDING" {
                step.code = "ZERO_ROUNDED";
            }
            result.explanations.push(step);
        }
        require(
            result.actions.len() <= 128 && result.explanations.len() <= 256,
            "LIMIT",
            "decision size",
        )?;
        for policy in &self.policies {
            if let Some(maximum) = &policy.binding.maximum_exposure {
                let mut total = 0;
                for a in result
                    .actions
                    .iter()
                    .filter(|a| a.binding.id == policy.binding.id && a.book != Book::Allocation)
                {
                    total = add_atoms(total, a.amount.atoms())?;
                }
                require(
                    total <= maximum.atoms(),
                    "EXPOSURE_EXCEEDED",
                    "binding decision limit",
                )?;
            }
        }
        result.deltas = deltas(&result.actions)?;
        result.consumptions = consumptions(self, &input, &result)?;
        Ok(result)
    }
}
fn predicate(p: &Predicate, input: &Input<'_>) -> bool {
    match p {
        Predicate::Tier(t) => input.context.tier.as_ref() == Some(t),
        Predicate::Funding(f) => input.context.funding == *f,
        Predicate::Priority(v) => input.context.priority == Some(*v),
        Predicate::Source(s) => input.event.source() == s,
    }
}
pub(super) fn claim_id(event: &Event) -> Result<String> {
    if event.dto().kind.is_work() {
        return event.completion_claim_id();
    }
    let claim = event
        .dto()
        .claim_id
        .as_deref()
        .ok_or_else(|| Error::new("SCHEMA", "outcome claim"))?;
    canonical::identity(
        Domain::Claim,
        &json!([event.scope(), event.source(), claim, "acquisition", claim]),
    )
}
pub(super) fn validate_authority(event: &Event, authority: &SourceAuthority) -> Result<()> {
    prefixed(&authority.grant, "doc_")?;
    validate_source(&authority.source)?;
    require(
        authority.active
            && authority.source == event.source()
            && authority.event_types.contains(&event.dto().kind),
        "SOURCE_UNAUTHORIZED",
        "current authenticated grant",
    )?;
    require(
        authority.event_types.len() <= 7 && authority.relations.len() <= 5,
        "LIMIT",
        "source grant",
    )?;
    for link in event.dto().links.as_deref().unwrap_or(&[]) {
        require(
            authority.relations.contains(&link.relation),
            "LINK_UNAUTHORIZED",
            "current relation grant",
        )?;
    }
    Ok(())
}
pub(super) fn validate_history(event: &Event, history: &[Evaluation]) -> Result<()> {
    require(history.len() < 1000, "LIMIT", "chain events")?;
    let mut ids = BTreeSet::new();
    let mut claims = BTreeSet::new();
    let mut actions = BTreeSet::new();
    let mut bytes = event.bytes().as_slice().len();
    for d in history {
        require(
            d.event.scope() == event.scope()
                && d.event.chain() == event.chain()
                && d.event.dto().customer == event.dto().customer,
            "CHAIN_MISMATCH",
            "scope/chain/customer",
        )?;
        require(
            d.event.id() != event.id()
                && ids.insert(d.event.id())
                && d.claim_id.as_ref().is_none_or(|claim| claims.insert(claim)),
            "CLAIM_CONFLICT",
            "duplicate history/current identity",
        )?;
        bytes += d.event.bytes().as_slice().len();
        for a in &d.actions {
            require(
                actions.insert(&a.id),
                "EFFECT_CONFLICT",
                "duplicate history effect",
            )?;
            // Conservative retained-input estimate; canonical byte accounting is
            // still required when encoding the final decision in the coordinator.
            bytes += a.id.len()
                + a.effect_id.len()
                + a.obligation_id.len()
                + a.component.len()
                + (a.inputs.len() + a.sources.len() + a.links.len()) * 128
                + 1024;
        }
    }
    require(bytes <= 8 * 1024 * 1024, "LIMIT", "resolved input")
}
fn validate_context(bundle: &Bundle, input: &Input<'_>) -> Result<()> {
    let c = input.context;
    prefixed(&c.document, "doc_")?;
    text(&c.customer, 128)?;
    if let Some(t) = &c.tier {
        slug(t)?;
    }
    require(
        c.customer == input.event.dto().customer,
        "CHAIN_MISMATCH",
        "pinned customer",
    )?;
    for d in input.history {
        require(
            d.context == *c && d.bundle == *bundle,
            "PINNED_CONTEXT",
            "chain terms/context cannot change",
        )?;
    }
    if let Some(stage) = &c.stage {
        slug(&stage.id)?;
        slug(&stage.closure_claim_namespace)?;
        require(
            !stage.expected.is_empty() && stage.expected.len() <= 32,
            "LIMIT",
            "stage expected set",
        )?;
        let mut ops = BTreeSet::new();
        for e in &stage.expected {
            validate_source(&e.source)?;
            text(&e.operation_id, 128)?;
            require(
                e.kind.is_work()
                    && ops.insert((&e.source, &e.operation_id))
                    && !e.retail_components.is_empty()
                    && e.retail_components.len() <= 64,
                "STAGE_DEFINITION",
                "unique complete work set",
            )?;
            let mut components = BTreeSet::new();
            for component in &e.retail_components {
                slug(component)?;
                require(
                    components.insert(component)
                        && bundle.policies[0]
                            .rules
                            .iter()
                            .any(|r| r.on == e.kind && r.component == *component),
                    "STAGE_DEFINITION",
                    "declared retail component",
                )?;
            }
        }
        require(
            !input
                .history
                .iter()
                .any(|d| d.closed_stage.as_ref() == Some(&stage.id)),
            "STAGE_CLOSED",
            "new originals cannot enter a closed stage, even after reversal",
        )?;
    }
    for p in &bundle.policies {
        for r in &p.rules {
            if let Operation::Cap { stage, .. } = &r.operation {
                require(
                    c.stage.as_ref().is_some_and(|s| s.id == *stage),
                    "STAGE_DEFINITION",
                    "cap stage pinned before work",
                )?;
            }
        }
    }
    Ok(())
}
struct Match<'a> {
    target: &'a Evaluation,
    sources: Vec<String>,
    links: Vec<String>,
}
fn parent<'a>(
    child: &Event,
    relation: Relation,
    history: &'a [Evaluation],
) -> Result<Option<&'a Evaluation>> {
    let Some(link) = child
        .dto()
        .links
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|l| l.relation == relation)
    else {
        return Ok(None);
    };
    history
        .iter()
        .find(|d| d.event.source() == link.from.source && d.event.dto().id == link.from.id)
        .map(Some)
        .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "explicit predecessor missing"))
}
fn link_id(child: &Event, relation: Relation, predecessor: &Event) -> Result<String> {
    canonical::identity(
        Domain::Link,
        &json!([child.scope(), relation, child.id(), predecessor.id()]),
    )
}
fn match_rule<'a>(
    event: &Event,
    matcher: Option<Matcher>,
    history: &'a [Evaluation],
) -> Result<Option<Match<'a>>> {
    let Some(matcher) = matcher else {
        return Ok(None);
    };
    let (relation, target_kind) = match matcher {
        Matcher::Direct { relation, target } => (relation, target),
        Matcher::AcquisitionOptimization => (Relation::AttributedTo, EventKind::Published),
    };
    require(
        event
            .dto()
            .links
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|l| l.relation == relation)
            .count()
            <= 1,
        "POLICY_AMBIGUOUS_MATCH",
        "matcher requires one explicit endpoint",
    )?;
    let Some(first) = parent(event, relation, history)? else {
        return Ok(None);
    };
    if first.event.dto().kind != target_kind {
        return Ok(None);
    }
    let mut m = Match {
        target: first,
        sources: vec![first.event.id().into()],
        links: vec![link_id(event, relation, &first.event)?],
    };
    if matcher == Matcher::AcquisitionOptimization {
        let Some(second) = parent(&first.event, Relation::PublishedAs, history)? else {
            return Ok(None);
        };
        if second.event.dto().kind != EventKind::Optimized {
            return Ok(None);
        }
        m.links
            .push(link_id(&first.event, Relation::PublishedAs, &second.event)?);
        m.sources.push(second.event.id().into());
        m.target = second;
    }
    Ok(Some(m))
}
fn validate_links(event: &Event, history: &[Evaluation]) -> Result<()> {
    let mut seen = BTreeMap::new();
    for link in event.dto().links.as_deref().unwrap_or(&[]) {
        let p = history
            .iter()
            .find(|d| d.event.source() == link.from.source && d.event.dto().id == link.from.id)
            .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "explicit predecessor missing"))?;
        let kind = p.event.dto().kind;
        let allowed = match link.relation {
            Relation::GeneratedFrom | Relation::OptimizedFrom => kind == EventKind::Generated,
            Relation::PublishedAs => matches!(kind, EventKind::Generated | EventKind::Optimized),
            Relation::AttributedTo => kind == EventKind::Published,
            Relation::ConsumesService => kind == EventKind::ToolCompleted,
        };
        require(allowed, "LINK_TYPE", "typed predecessor")?;
        let inbound = history
            .iter()
            .flat_map(|d| d.event.dto().links.as_deref().unwrap_or(&[]))
            .filter(|l| l.from == link.from)
            .count();
        require(inbound < 128, "LIMIT", "inbound children")?;
        depth(&p.event, history, &mut BTreeSet::new(), &mut seen, 1)?;
    }
    Ok(())
}
fn depth(
    event: &Event,
    history: &[Evaluation],
    path: &mut BTreeSet<String>,
    seen: &mut BTreeMap<String, u8>,
    hops: u8,
) -> Result<()> {
    require(hops <= 16, "LIMIT", "chain traversal")?;
    require(
        !path.contains(event.id()),
        "LINK_CYCLE",
        "predecessor cycle",
    )?;
    if seen
        .get(event.id())
        .is_some_and(|previous| *previous >= hops)
    {
        return Ok(());
    }
    seen.insert(event.id().into(), hops);
    path.insert(event.id().into());
    for link in event.dto().links.as_deref().unwrap_or(&[]) {
        let p = history
            .iter()
            .find(|d| d.event.source() == link.from.source && d.event.dto().id == link.from.id)
            .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "history predecessor"))?;
        depth(&p.event, history, path, seen, hops + 1)?;
    }
    path.remove(event.id());
    Ok(())
}
fn validate_binding(b: &Binding, input: &Input<'_>, matched: Option<&Evaluation>) -> Result<()> {
    let event = input.event;
    require(
        b.sources.iter().any(|s| s == event.source()),
        "SOURCE_UNAUTHORIZED",
        "binding source",
    )?;
    if event.dto().kind.is_work() {
        require(
            event.dto().unit.as_deref() == Some(&b.unit),
            "UNIT_MISMATCH",
            "binding unit",
        )?;
        require(
            leq(
                event.dto().quantity.as_ref().expect("normalized"),
                &b.maximum_quantity,
            )?,
            "QUANTITY",
            "binding ceiling",
        )?;
    } else {
        let o = b
            .outcome
            .as_ref()
            .ok_or_else(|| Error::new("OUTCOME_AUTHORITY", "accepted outcome terms"))?;
        require(
            o.source == event.source(),
            "OUTCOME_AUTHORITY",
            "designated outcome source",
        )?;
        let publication = parent(event, Relation::AttributedTo, input.history)?
            .ok_or_else(|| Error::new("OUTCOME_AUTHORITY", "publication link"))?;
        require(
            publication.event.dto().status == Some(Completion::Succeeded),
            "OUTCOME_AUTHORITY",
            "successful publication",
        )?;
        let published = publication
            .event
            .dto()
            .occurred_at
            .as_ref()
            .ok_or_else(|| Error::new("OUTCOME_WINDOW", "publication timestamp"))?
            .micros();
        let deadline = published
            .checked_add(o.window_us as i64)
            .ok_or_else(|| Error::new("OUTCOME_WINDOW", "timestamp overflow"))?;
        let occurred = event
            .dto()
            .occurred_at
            .as_ref()
            .expect("normalized outcome")
            .micros();
        require(
            occurred >= published && occurred < deadline,
            "OUTCOME_WINDOW",
            "half-open occurrence interval",
        )?;
        require(
            input.received_at.micros()
                <= deadline
                    .checked_add(o.report_grace_us as i64)
                    .ok_or_else(|| Error::new("OUTCOME_WINDOW", "grace overflow"))?,
            "AUTHORITY_REVIEW_REQUIRED",
            "report deadline",
        )?;
    }
    if b.book == Book::Supplier {
        invocation(b, input, matched)?;
    }
    Ok(())
}
fn leq(a: &crate::money::Decimal, b: &crate::money::Decimal) -> Result<bool> {
    Ok(!a.ratio().add(&b.ratio().negated())?.is_positive())
}
fn invocation<'a>(
    b: &Binding,
    input: &'a Input<'_>,
    matched: Option<&Evaluation>,
) -> Result<&'a Invocation> {
    let event = input.event;
    let candidates: Vec<_> = input
        .invocations
        .iter()
        .filter(|i| {
            i.binding_id == b.id
                && if event.dto().kind.is_work() {
                    i.operation_id == event.candidate().operation_id() && i.source == event.source()
                } else {
                    matched.is_some_and(|d| i.completion_event.as_deref() == Some(d.event.id()))
                }
        })
        .collect();
    require(
        candidates.len() == 1,
        "INVOCATION_REQUIRED",
        "one nominated supplier authorization",
    )?;
    let i = candidates[0];
    let mut used = 0;
    for d in input.history {
        if let Some(previous) = d.invocations.iter().find(|old| old.id == i.id) {
            let mut comparable = i.clone();
            comparable.held = previous.held.clone();
            comparable.completion_event = previous.completion_event.clone();
            require(
                comparable == *previous,
                "INVOCATION_CONFLICT",
                "historical authorization cannot be enlarged or replaced",
            )?;
        }
        for c in d.consumptions.iter().filter(|c| c.invocation_id == i.id) {
            used = add_atoms(used, add_atoms(c.consume.atoms(), c.release.atoms())?)?;
        }
    }
    require(
        add_atoms(used, i.held.atoms())? <= i.maximum_exposure.atoms(),
        "EXPOSURE_EXCEEDED",
        "held capacity cannot be replenished",
    )?;
    text(&i.id, 128)?;
    text(&i.operation_id, 128)?;
    validate_source(&i.source)?;
    require(
        i.chain == event.chain() && i.customer == event.dto().customer && i.unit == b.unit,
        "INVOCATION_SCOPE",
        "chain/customer/unit",
    )?;
    require(
        i.maximum_exposure.currency() == b.maximum_exposure.as_ref().expect("compiled").currency()
            && i.maximum_exposure.scale() == b.maximum_exposure.as_ref().expect("compiled").scale()
            && i.maximum_exposure.atoms() >= 0
            && i.maximum_exposure.atoms() <= b.maximum_exposure.as_ref().expect("compiled").atoms()
            && i.held.currency() == i.maximum_exposure.currency()
            && i.held.scale() == i.maximum_exposure.scale()
            && i.held.atoms() >= 0
            && i.held.atoms() <= i.maximum_exposure.atoms(),
        "EXPOSURE_EXCEEDED",
        "invocation/held limits",
    )?;
    require(
        i.authorized_at.micros() <= i.attested_start.micros()
            && i.attested_start.micros() < i.start_before.micros(),
        "INVOCATION_EXPIRED",
        "attested work start, not report arrival",
    )?;
    require(
        leq(&i.maximum_quantity, &b.maximum_quantity)?,
        "QUANTITY",
        "invocation ceiling",
    )?;
    if event.dto().kind.is_work() {
        require(
            i.completion_event.is_none()
                && event.dto().invocation_id.as_deref() == Some(&i.id)
                && event.dto().binding_id.as_deref() == Some(&b.id),
            "INVOCATION_CONFLICT",
            "explicit unconsumed work authorization",
        )?;
        require(
            leq(
                event.dto().quantity.as_ref().expect("normalized"),
                &i.maximum_quantity,
            )?,
            "QUANTITY",
            "invocation quantity",
        )?;
    } else {
        let predecessor = matched.expect("candidate matched");
        require(
            predecessor.event.source() == i.source
                && predecessor.event.candidate().operation_id() == i.operation_id
                && predecessor.event.dto().status == Some(Completion::Succeeded)
                && predecessor.event.dto().invocation_id.as_deref() == Some(&i.id),
            "INVOCATION_SCOPE",
            "historical supplier completion",
        )?;
        let deadline = i
            .outcome_deadline
            .as_ref()
            .ok_or_else(|| Error::new("OUTCOME_AUTHORITY", "contingent authorization"))?;
        require(
            event.dto().occurred_at.as_ref().expect("outcome").micros() < deadline.micros(),
            "OUTCOME_WINDOW",
            "invocation outcome deadline",
        )?;
    }
    Ok(i)
}
fn price_value(
    price: &Price,
    scale: u8,
    event: &Event,
    pi: usize,
    booked: &BTreeMap<(usize, String), i128>,
    actions: &[Action],
    step: &mut Explanation,
) -> Result<ExactRatio> {
    match price {
        Price::Fixed(d) => d.atoms_ratio(scale),
        Price::Unit { rate, unit } => {
            require(
                event.dto().unit.as_ref() == Some(unit),
                "UNIT_MISMATCH",
                "unit rate",
            )?;
            rate.atoms_ratio(scale)?.mul(
                &event
                    .dto()
                    .quantity
                    .as_ref()
                    .expect("compiled work")
                    .ratio(),
            )
        }
        Price::Percent { percent, component } => {
            let basis = *booked
                .get(&(pi, component.clone()))
                .ok_or_else(|| Error::new("POLICY_MISSING_BASIS", component))?;
            require(basis >= 0, "NEGATIVE_BASIS", "percentage")?;
            step.basis = Some(ExactRatio::integer(basis));
            step.inputs = component_inputs(actions, &step.binding_id, component);
            ExactRatio::integer(basis).mul(&percent.percent()?)
        }
    }
}
fn discount_value(amount: &DiscountAmount, scale: u8, basis: i128) -> Result<ExactRatio> {
    require(basis >= 0, "NEGATIVE_BASIS", "discount")?;
    Ok(match amount {
        DiscountAmount::Fixed(d) => d.atoms_ratio(scale)?,
        DiscountAmount::Percent(d) => ExactRatio::integer(basis).mul(&d.percent()?)?,
    }
    .negated())
}
fn component_inputs(actions: &[Action], binding: &str, component: &str) -> Vec<String> {
    actions
        .iter()
        .filter(|a| a.binding.id == binding && a.component == component)
        .map(|a| a.id.clone())
        .collect()
}
fn stage_prior(bundle: &Bundle, input: &Input<'_>, stage_id: &str) -> Result<(i128, Vec<String>)> {
    let stage = input
        .context
        .stage
        .as_ref()
        .filter(|s| s.id == stage_id)
        .ok_or_else(|| Error::new("STAGE_DEFINITION", "missing stage"))?;
    require(
        bundle.policies[0]
            .binding
            .outcome
            .as_ref()
            .is_some_and(|o| o.claim_namespace == stage.closure_claim_namespace),
        "OUTCOME_AUTHORITY",
        "stage claim namespace",
    )?;
    let mut total = 0;
    let mut refs = Vec::new();
    for expected in &stage.expected {
        let d = input
            .history
            .iter()
            .find(|d| {
                d.event.source() == expected.source
                    && d.event.candidate().operation_id() == expected.operation_id
            })
            .ok_or_else(|| {
                Error::new(
                    "WAITING_DEPENDENCIES",
                    format!("stage operation {}", expected.operation_id),
                )
            })?;
        require(
            d.event.dto().kind == expected.kind,
            "STAGE_DEFINITION",
            "expected completion kind",
        )?;
        for a in &d.actions {
            if a.book != Book::Retail {
                continue;
            }
            require(
                a.binding == bundle.policies[0].binding
                    && expected.retail_components.contains(&a.component),
                "STAGE_DEFINITION",
                "same obligation and complete declared components",
            )?;
            require(
                !is_reversed(&a.id, input.history),
                "STAGE_DEFINITION",
                "reversed stage input needs newly authorized context",
            )?;
            total = add_atoms(total, a.amount.atoms())?;
            refs.push(a.id.clone());
        }
    }
    require(total >= 0, "NEGATIVE_BASIS", "stage prior")?;
    Ok((total, refs))
}
pub(super) fn is_reversed(id: &str, history: &[Evaluation]) -> bool {
    history
        .iter()
        .flat_map(|d| &d.actions)
        .any(|a| a.reverses.as_deref() == Some(id))
}
fn action_id(
    event: &Event,
    claim: &str,
    b: &Binding,
    component: &str,
    links: &[String],
) -> Result<(String, String)> {
    let mut links = links.to_vec();
    links.sort();
    links.dedup();
    let match_key = if links.is_empty() {
        json!("self")
    } else {
        json!(links)
    };
    let effect = canonical::identity(
        Domain::Effect,
        &json!([
            event.scope(),
            b.agreement,
            component,
            claim,
            match_key,
            "original"
        ]),
    )?;
    let action = canonical::identity(Domain::Action, &json!([effect]))?;
    Ok((effect, action))
}
#[allow(clippy::too_many_arguments)]
fn make_action(
    bundle: &Bundle,
    event: &Event,
    claim: &str,
    b: &Binding,
    kind: ActionKind,
    book: Book,
    component: &str,
    atoms: i128,
    mut sources: Vec<String>,
    mut links: Vec<String>,
    mut inputs: Vec<String>,
    discount_target: Option<String>,
) -> Result<Action> {
    sources.sort();
    sources.dedup();
    links.sort();
    links.dedup();
    inputs.sort();
    inputs.dedup();
    let (effect_id, id) = action_id(event, claim, b, component, &links)?;
    let obligation_id = canonical::identity(
        Domain::Obligation,
        &json!([
            event.scope(),
            b.agreement,
            book.name(),
            bundle.currency,
            bundle.scale,
            b.roles
        ]),
    )?;
    Ok(Action {
        id,
        effect_id,
        obligation_id,
        binding: b.clone(),
        kind,
        book,
        component: component.into(),
        amount: Money::new(&bundle.currency, bundle.scale, atoms)?,
        sources,
        links,
        inputs,
        reverses: None,
        allocation_parent: None,
        allocation_recipient: None,
        discount_target,
    })
}
pub(super) fn deltas(actions: &[Action]) -> Result<Vec<ObligationDelta>> {
    let mut groups: BTreeMap<String, ObligationDelta> = BTreeMap::new();
    for a in actions {
        if !matches!(a.book, Book::Retail | Book::Supplier) {
            continue;
        }
        if let Some(g) = groups.get_mut(&a.obligation_id) {
            g.amount = g.amount.checked_add(&a.amount)?;
            g.actions.push(a.id.clone());
        } else {
            groups.insert(
                a.obligation_id.clone(),
                ObligationDelta {
                    obligation_id: a.obligation_id.clone(),
                    book: a.book,
                    roles: a.binding.roles.clone(),
                    amount: a.amount.clone(),
                    actions: vec![a.id.clone()],
                },
            );
        }
    }
    require(groups.len() <= 32, "LIMIT", "obligation deltas")?;
    Ok(groups
        .into_values()
        .filter(|g| g.amount.atoms() != 0)
        .map(|mut g| {
            g.actions.sort();
            g
        })
        .collect())
}
fn consumptions(
    bundle: &Bundle,
    input: &Input<'_>,
    result: &Evaluation,
) -> Result<Vec<Consumption>> {
    let mut output = Vec::new();
    for p in &bundle.policies {
        if p.binding.book != Book::Supplier {
            continue;
        }
        let relevant: Vec<_> = p
            .rules
            .iter()
            .filter(|r| {
                r.on == input.event.dto().kind
                    && (input.event.dto().status == Some(Completion::Failed)
                        || predicate_all(r, input))
            })
            .collect();
        if relevant.is_empty() {
            continue;
        }
        let mut chosen = None;
        for r in relevant {
            let matched = match_rule(input.event, r.matcher, input.history)?;
            if r.matcher.is_some()
                && matched.is_none()
                && input.event.dto().status != Some(Completion::Failed)
            {
                continue;
            }
            let i = invocation(&p.binding, input, matched.as_ref().map(|m| m.target))?;
            if let Some(previous) = chosen {
                require(
                    previous == i.id,
                    "INVOCATION_CONFLICT",
                    "one invocation per supplier decision",
                )?;
            }
            chosen = Some(i.id.as_str());
        }
        let Some(id) = chosen else {
            continue;
        };
        let i = input
            .invocations
            .iter()
            .find(|i| i.id == id)
            .expect("chosen");
        let mut consume = 0;
        for a in result
            .actions
            .iter()
            .filter(|a| a.binding.id == p.binding.id && a.book == Book::Supplier)
        {
            consume = add_atoms(consume, a.amount.atoms())?;
        }
        require(
            consume >= 0 && consume <= i.held.atoms(),
            "EXPOSURE_EXCEEDED",
            "supplier decision exceeds held authorization",
        )?;
        let contingent = p.rules.iter().any(|r| r.on == EventKind::Acquired);
        let close = input.event.dto().kind == EventKind::Acquired
            || input.event.dto().status == Some(Completion::Failed)
            || !contingent;
        output.push(Consumption {
            invocation_id: i.id.clone(),
            consume: Money::new(&bundle.currency, bundle.scale, consume)?,
            release: Money::new(
                &bundle.currency,
                bundle.scale,
                if close { i.held.atoms() - consume } else { 0 },
            )?,
        });
    }
    output.sort_by(|a, b| a.invocation_id.cmp(&b.invocation_id));
    Ok(output)
}
fn predicate_all(rule: &Rule, input: &Input<'_>) -> bool {
    rule.when.iter().all(|p| predicate(p, input))
}
