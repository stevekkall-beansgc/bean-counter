use super::compile::require;
use super::evaluate::{deltas, is_reversed, validate_authority, validate_history};
use super::model::*;
use crate::canonical::{self, Domain};
use crate::domain::Event;
use crate::money::Money;
use crate::wire::EventKind;
use crate::{Error, Result};
use serde_json::json;
use std::collections::BTreeSet;

/// Exact full-decision reversal. Uses original booked actions/terms only, never
/// today's prices. The caller supplies complete locked chain history, verifies
/// retained correction evidence, and enforces the unique `reverses` guard.
/// No reservation is replenished and no closed stage is reopened.
pub fn reverse(
    event: &Event,
    history: &[Evaluation],
    authority: &SourceAuthority,
) -> Result<Evaluation> {
    require(
        event.dto().kind == EventKind::Reversal,
        "SCHEMA",
        "reversal event",
    )?;
    validate_authority(event, authority)?;
    validate_history(event, history)?;
    let targets = event.dto().targets.as_ref().expect("normalized reversal");
    let selected: Vec<_> = targets
        .iter()
        .map(|id| {
            history
                .iter()
                .find(|d| d.event.id() == id)
                .ok_or_else(|| Error::new("WAITING_DEPENDENCIES", "reversal target decision"))
        })
        .collect::<Result<_>>()?;
    let first = selected.first().expect("normalized target bound");
    for d in history {
        require(
            d.context == first.context && d.bundle == first.bundle,
            "PINNED_CONTEXT",
            "complete reversal history must share the pinned chain context",
        )?;
    }
    let mut ids = BTreeSet::new();
    for d in &selected {
        require(
            d.event.dto().kind != EventKind::Reversal && !d.actions.is_empty(),
            "NOTHING_TO_REVERSE",
            "original complete economics required",
        )?;
        require(
            d.context == first.context && d.bundle == first.bundle,
            "PINNED_CONTEXT",
            "reversal chain context",
        )?;
        for a in &d.actions {
            require(
                !is_reversed(&a.id, history),
                "ALREADY_REVERSED",
                "original action",
            )?;
            require(
                a.binding
                    .correction_sources
                    .iter()
                    .any(|s| s == event.source()),
                "CORRECTION_UNAUTHORIZED",
                "original agreement correction source",
            )?;
            ids.insert(a.id.as_str());
        }
    }
    require(ids.len() <= 128, "LIMIT", "full reversal actions")?;
    // Iteration is unnecessary: if every direct dependent decision is selected,
    // the same check covers each successive level of the closure.
    for d in history {
        if targets.iter().any(|t| t == d.event.id()) || d.event.dto().kind == EventKind::Reversal {
            continue;
        }
        let has_live_economics = d.actions.iter().any(|a| !is_reversed(&a.id, history));
        let depends = d
            .actions
            .iter()
            .flat_map(|a| &a.inputs)
            .chain(d.explanations.iter().flat_map(|s| &s.inputs))
            .any(|id| ids.contains(id.as_str()));
        if has_live_economics && depends {
            return Err(Error::new("REVERSAL_DEPENDENTS_REQUIRED", d.event.id()));
        }
    }
    let mut originals: Vec<_> = selected
        .iter()
        .flat_map(|d| d.actions.iter().map(move |a| (d.event.id(), a)))
        .collect();
    originals.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    let mut result = Evaluation {
        event: event.clone(),
        bundle: first.bundle.clone(),
        context: first.context.clone(),
        claim_id: None,
        actions: vec![],
        explanations: vec![],
        deltas: vec![],
        consumptions: vec![],
        invocations: vec![],
        closed_stage: None,
        received_at: None,
        source_authority: authority.clone(),
        costs: vec![],
    };
    for (source, original) in originals {
        let effect_id =
            canonical::identity(Domain::ReversalEffect, &json!([event.scope(), original.id]))?;
        let id = canonical::identity(Domain::Action, &json!([effect_id]))?;
        let mut action = original.clone();
        action.id = id.clone();
        action.effect_id = effect_id;
        action.kind = ActionKind::Reversal;
        action.amount = Money::new(
            original.amount.currency(),
            original.amount.scale(),
            -original.amount.atoms(),
        )?;
        action.sources = vec![event.id().into(), source.into()];
        action.sources.sort();
        action.inputs = vec![original.id.clone()];
        action.reverses = Some(original.id.clone());
        action.discount_target = None;
        // Stored allocation parent and entries are preserved and negated, never
        // recomputed using current percentages or remainder ordering.
        result.explanations.push(Explanation {
            binding_id: original.binding.id.clone(),
            rule_id: None,
            code: "EXACT_REVERSAL",
            basis: None,
            unrounded: Some(crate::money::ExactRatio::integer(action.amount.atoms())),
            rounded: Some(action.amount.atoms()),
            inputs: vec![original.id.clone()],
            action_ids: vec![id],
        });
        result.actions.push(action);
    }
    result.deltas = deltas(&result.actions)?;
    Ok(result)
}
