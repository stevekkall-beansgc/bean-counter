//! Data reconstruction of immutable activation boundaries. Origins are checked
//! by history_before; no mutable head or current clock selects historical terms.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::service::workflow::agreement) struct Transition {
    pub(in crate::service::workflow::agreement) head: Document,
    pub(in crate::service::workflow::agreement) activation: Document,
    pub(in crate::service::workflow::agreement) binding: Document,
    pub(in crate::service::workflow::agreement) revision: i64,
    pub(in crate::service::workflow::agreement) sequence: i64,
    pub(in crate::service::workflow::agreement) at: Timestamp,
    pub(in crate::service::workflow::agreement) effective: Timestamp,
    pub(in crate::service::workflow::agreement) action: ActivationAction,
    pub(in crate::service::workflow::agreement) target: Option<RecordRef>,
    prior: Option<RecordRef>,
}
fn checked(d: &Document) -> Result<Value> {
    let v =
        canonical::parse_bounded(&d.bytes, 262144).map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&v)? != d.bytes || v["schema"] != d.schema {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(v)
}
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
pub(in crate::service::workflow::agreement) fn transition(
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    head: Document,
    activation: Document,
    binding: Document,
) -> Result<Transition> {
    let h = checked(&head)?;
    let a = checked(&activation)?;
    let b = canonical_body(&binding)?;
    let saved_key = OperationKey::parse(&records::bytes(&h["operation_key"])?)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let revision = text(&h, "new_revision")?
        .parse::<i64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let sequence = text(&h, "commit_sequence")?
        .parse::<i64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let at =
        Timestamp::parse(text(&h, "accepted_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let effective =
        Timestamp::parse(text(&a, "effective_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    if saved_key.logical_store_id != key.logical_store_id
        || saved_key.scope != key.scope
        || h["scope"] != json!(key.scope)
        || h["head_kind"] != "agreement_activation"
        || h["customer"] != customer
        || h["agreement_id"] != agreement
        || b["customer"] != customer
        || b["agreement_id"] != agreement
        || h["accepted_binding_ref"] != json!(binding.reference)
        || h["resolved_effective_time"] != a["effective_at"]
        || a["binding_ref"] != json!(binding.reference)
        || a["head_transition_ref"] != json!(head.reference)
        || a["accepted_at"] != h["accepted_at"]
        || a["commit_sequence"] != h["commit_sequence"]
        || sequence <= 0
        || revision <= 0
        || record::semantic_document(
            "workflow-head-transition",
            &saved_key,
            customer,
            agreement,
            revision as u64,
            h.clone(),
        )?
        .reference
            != head.reference
        || record::semantic_document(
            "workflow-activation",
            &saved_key,
            customer,
            agreement,
            revision as u64,
            a.clone(),
        )?
        .reference
            != activation.reference
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let action = match text(&a, "action")? {
        "activate" => ActivationAction::Activate,
        "replace" => ActivationAction::Replace,
        "revoke" => ActivationAction::Revoke,
        _ => return Err(ServiceError::IntegrityFailure),
    };
    let target = a.get("target_transition_ref").map(reference).transpose()?;
    let prior = h.get("prior_head_ref").map(reference).transpose()?;
    Ok(Transition {
        head,
        activation,
        binding,
        revision,
        sequence,
        at,
        effective,
        action,
        target,
        prior,
    })
}
fn valid_binding(t: &Transition) -> Result<()> {
    if t.action == ActivationAction::Revoke {
        return Ok(());
    }
    let b = canonical_body(&t.binding)?;
    let starts =
        Timestamp::parse(text(&b, "starts_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let ends = b
        .get("ends_at")
        .map(|v| {
            Timestamp::parse(v.as_str().ok_or(ServiceError::IntegrityFailure)?)
                .map_err(|_| ServiceError::IntegrityFailure)
        })
        .transpose()?;
    if t.effective.micros() < starts.micros()
        || ends.is_some_and(|end| t.effective.micros() >= end.micros())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}
/// Latest boundary includes revoke. Callers must then check the selected
/// binding's interval; never search backwards for an older valid binding.
pub(in crate::service::workflow::agreement) fn boundary_at<'a>(
    history: &'a [Transition],
    at: &Timestamp,
) -> Result<Option<&'a Transition>> {
    let superseded: BTreeSet<_> = history
        .iter()
        .filter(|t| t.at.micros() <= at.micros())
        .filter_map(|t| t.target.as_ref())
        .map(|r| r.id.clone())
        .collect();
    Ok(history
        .iter()
        .filter(|t| {
            t.at.micros() <= at.micros()
                && !superseded.contains(&t.activation.reference.id)
                && t.effective.micros() <= at.micros()
        })
        .max_by_key(|t| (t.effective.micros(), t.sequence)))
}
/// Evaluate only an already certified schedule prefix. Expiry/revoke yields no
/// selection, never a fallback to a previously valid binding.
#[cfg(test)]
pub(in crate::service::workflow::agreement) fn selected_binding(
    history: &[Transition],
    at: &Timestamp,
) -> Result<Option<RecordRef>> {
    let Some(t) = boundary_at(history, at)? else {
        return Ok(None);
    };
    if t.action == ActivationAction::Revoke {
        return Ok(None);
    }
    let b = canonical_body(&t.binding)?;
    let starts =
        Timestamp::parse(text(&b, "starts_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let ends = b
        .get("ends_at")
        .map(|v| {
            Timestamp::parse(v.as_str().ok_or(ServiceError::IntegrityFailure)?)
                .map_err(|_| ServiceError::IntegrityFailure)
        })
        .transpose()?;
    if at.micros() < starts.micros() || ends.is_some_and(|end| at.micros() >= end.micros()) {
        return Ok(None);
    }
    Ok(Some(t.binding.reference.clone()))
}
/// Validate the unambiguous activate/replace portion and the whole contiguous
/// identity/time lineage. Revoke selection is finalized by the approved stop
/// verifier; this routine does not invent a pending-cancellation rule.
pub(in crate::service::workflow::agreement) fn check_next(
    history: &[Transition],
    next: &Transition,
) -> Result<()> {
    let prior = history.last();
    if prior.map_or(0, |p| p.revision).checked_add(1) != Some(next.revision)
        || next.prior.as_ref() != prior.map(|p| &p.head.reference)
        || prior.is_some_and(|p| p.sequence >= next.sequence || p.at.micros() > next.at.micros())
        || next.effective.micros() < next.at.micros()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    valid_binding(next)?;
    let superseded: BTreeSet<_> = history
        .iter()
        .filter_map(|t| t.target.as_ref())
        .map(|r| r.id.clone())
        .collect();
    let pending: Vec<_> = history
        .iter()
        .filter(|t| {
            !superseded.contains(&t.activation.reference.id)
                && t.effective.micros() > next.at.micros()
        })
        .collect();
    if pending.len() > 1 {
        return Err(ServiceError::IntegrityFailure);
    }
    match next.action {
        ActivationAction::Activate => {
            if !pending.is_empty() || next.target.is_some() {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Replace => {
            if pending.first().map(|t| &t.activation.reference) != next.target.as_ref() {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Revoke => {
            if next.effective != next.at {
                return Err(ServiceError::IntegrityFailure);
            }
            let selected = if let Some(pending) = pending.first() {
                Some(*pending)
            } else {
                boundary_at(history, &next.at)?
            };
            if selected.is_none_or(|t| {
                t.action == ActivationAction::Revoke
                    || Some(&t.activation.reference) != next.target.as_ref()
            }) {
                return Err(reject("ACTIVATION_SCHEDULE_CONFLICT"));
            }
        }
    }
    Ok(())
}
/// Every independent birth identity must resolve. Full committed certificates
/// authenticate these bodies; index copies are compared after, not trusted.
pub(in crate::service::workflow::agreement) async fn history_before(
    tx: &mut SqliteTx,
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    before: i64,
) -> Result<Vec<Transition>> {
    let candidates = tx
        .workflow_agreement_activation_prefix(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
        )
        .await
        .map_err(crate::service::store_error)?;
    let mut history = BTreeMap::new();
    let mut intents = BTreeMap::new();
    for identity in candidates {
        let k = OperationKey::parse(&identity).map_err(|_| ServiceError::IntegrityFailure)?;
        if k.logical_store_id != key.logical_store_id || k.scope != key.scope {
            return Err(ServiceError::IntegrityFailure);
        }
        let o = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let body = canonical::parse_bounded(&o.operation_bytes, 8388608)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let at = Timestamp::parse(text(&body, "accepted_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if body["key"] != json!(k)
            || body["kind"] != o.kind
            || !matches!(o.kind.as_str(), "agreement.activate" | "agreement.revoke")
            || o.sequence <= 0
            || body["commit_sequence"] != o.sequence.to_string().as_str()
            || at.micros() != o.accepted_at_us
            || records::bytes(&body)? != o.operation_bytes
            || records::record_reference("workflow-operation", &key.scope, &body)?
                != o.operation_ref
        {
            return Err(ServiceError::IntegrityFailure);
        }
        if o.sequence >= before {
            continue;
        }
        let verified = Box::pin(verify::retained(tx, &k, &o)).await?;
        if verified["receipt"]["result_kind"] == "semantic_alias" {
            continue;
        }
        if verified["receipt"]["result_kind"] != "administrative" {
            return Err(ServiceError::IntegrityFailure);
        }
        let refs: Vec<RecordRef> =
            serde_json::from_value(verified["receipt"]["result_refs"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
        let find = |kind: &str| -> Result<RecordRef> {
            let matches: Vec<_> = refs.iter().filter(|r| r.kind == kind).collect();
            if matches.len() != 1 {
                return Err(ServiceError::IntegrityFailure);
            }
            Ok(matches[0].clone())
        };
        if refs.len() != 3 {
            return Err(ServiceError::IntegrityFailure);
        }
        let intent = find("workflow-agreement-intent")?;
        let head = loaded(tx, key, &find("workflow-head-transition")?).await?;
        let activation = loaded(tx, key, &find("workflow-activation")?).await?;
        let a = checked(&activation)?;
        let binding = loaded(tx, key, &reference(&a["binding_ref"])?).await?;
        let t = transition(key, customer, agreement, head, activation, binding)?;
        if t.sequence != o.sequence
            || t.at != at
            || intents.insert(t.revision, intent).is_some()
            || history.insert(t.revision, t).is_some()
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    let history: Vec<_> = history.into_values().collect();
    for (i, t) in history.iter().enumerate() {
        check_next(&history[..i], t)?;
    }
    let indexed = tx
        .workflow_agreement_activation_history(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
        )
        .await
        .map_err(crate::service::store_error)?;
    let indexed: Vec<_> = indexed.iter().filter(|t| t.sequence < before).collect();
    if indexed.len() != history.len() {
        return Err(ServiceError::IntegrityFailure);
    }
    for (i, t) in history.iter().enumerate() {
        let r = indexed[i];
        if r.head.revision != t.revision
            || r.head.transition != t.head.reference
            || r.activation != t.activation.reference
            || Some(&r.intent) != intents.get(&t.revision)
            || r.binding != t.binding.reference
            || r.action != t.action
            || r.sequence != t.sequence
            || r.accepted_at_us != t.at.micros()
            || r.effective_at_us != t.effective.micros()
            || r.target != t.target
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests;
