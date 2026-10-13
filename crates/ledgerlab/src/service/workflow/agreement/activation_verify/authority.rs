//! Immutable activation eligibility. Arguments are authenticated observations,
//! not caller capabilities; current owner/time/head checks remain the host's.
use super::*;
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
fn contains(v: &Value, name: &str) -> bool {
    v.as_array()
        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)))
}
fn selected(v: &Value, name: &str) -> bool {
    v.as_array().is_some_and(|a| {
        a.iter().any(|s| {
            contains(&s["names"], name)
                || s["namespace"]
                    .as_str()
                    .is_some_and(|prefix| name.starts_with(prefix))
        })
    })
}
/// Complete bound projection, not every unrelated capability of an old grant.
fn coverage(
    binding: &Value,
    product: &Value,
    docs: &[Document],
    ceiling: &Value,
    source: &str,
    customer: &str,
) -> Result<()> {
    require(
        contains(&ceiling["customer_scope"], customer)
            && ["read", "submit", "correct"]
                .iter()
                .all(|p| contains(&ceiling["permissions"], p))
            && ["work.accept", "quantity.correct"]
                .iter()
                .all(|p| contains(&ceiling["operations"], p))
            && selected(&ceiling["product_selectors"], text(product, "product_id")?),
        "AUTHORITY_REQUIRED",
    )?;
    for slot in binding["source_slots"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        require(
            contains(&ceiling["source_slots"], text(slot, "name")?)
                && slot["sources"]
                    .as_array()
                    .is_some_and(|a| a.len() == 1 && a[0] == source),
            "AUTHORITY_REQUIRED",
        )?;
    }
    for entry in binding["bindings"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let r = reference(&entry["binding_ref"])?;
        let terms = docs
            .iter()
            .find(|d| d.reference == r)
            .ok_or(ServiceError::IntegrityFailure)?;
        let terms = canonical_body(terms)?;
        require(
            contains(
                &ceiling["source_slots"],
                text(&terms, "correction_source_slot")?,
            ),
            "AUTHORITY_REQUIRED",
        )?;
        for activity in terms["allowed_activities"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            require(
                selected(
                    &ceiling["activity_selectors"],
                    activity.as_str().ok_or(ServiceError::IntegrityFailure)?,
                ),
                "AUTHORITY_REQUIRED",
            )?;
        }
    }
    Ok(())
}
pub(in crate::service::workflow::agreement) fn source_observation(
    source: &context::ObservedSource,
    at: &Timestamp,
) -> Result<Value> {
    let mut v = json!({"enrollment_ref":source.enrollment().reference(),"state_ref":source.state().reference(),"revision":source.state().revision().to_string(),"status":source.state().status()});
    match (source.state().status(), source.grant()) {
        ("revoked", None) => {}
        ("active", Some(grant)) => {
            v["grant_ref"] = json!(
                source
                    .state()
                    .current_grant_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
            );
            let c = grant.ceiling().canonical_value().map_err(typed_error)?;
            let starts = Timestamp::parse(text(&c, "starts_at")?)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let status = if at.micros() < starts.micros() {
                "not_started"
            } else if grant.ceiling().contains_time(at) {
                "valid"
            } else {
                "expired"
            };
            v["grant_time_status"] = json!(status);
        }
        _ => return Err(ServiceError::IntegrityFailure),
    }
    Ok(v)
}
// Explicit reviewed captured authority inputs; none is request-deserializable.
#[allow(clippy::too_many_arguments)]
pub(in crate::service::workflow::agreement) async fn verify_capture(
    tx: &mut SqliteTx,
    key: &OperationKey,
    accepted: &context::ObservedAcceptedBinding,
    source: &context::ObservedSource,
    actor: &str,
    at: &Timestamp,
    payload: &Value,
    purpose: &str,
) -> Result<Vec<Document>> {
    let b = canonical_body(&accepted.binding)?;
    require(
        payload["accepted_binding_ref"] == json!(accepted.binding.reference)
            && payload["agreement_id"] == b["agreement_id"],
        "INVALID_REFERENCE",
    )?;
    let original_refs = b["authority_refs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require(original_refs.len() == 1, "UNSUPPORTED_COMPOSITION")?;
    let original_ref = reference(&original_refs[0])?;
    let original = accepted
        .documents
        .iter()
        .find(|d| d.reference == original_ref)
        .ok_or(ServiceError::IntegrityFailure)?;
    let original = ledgerlab_core::policy::workflow::SourceGrant::parse(&original.bytes)
        .map_err(typed_error)?;
    require(
        json!(original.original_ceiling_ref()) == json!(source.enrollment().reference())
            && original.ceiling().principal_id() == source.enrollment().ceiling().principal_id()
            && original.ceiling().source() == source.enrollment().ceiling().source()
            && b["source_namespace"]
                == source.enrollment().ceiling().canonical_economic_namespace(),
        "AUTHORITY_REQUIRED",
    )?;
    let mut docs: Vec<Document> = source
        .documents()
        .iter()
        .map(|d| Document {
            reference: d.reference.clone(),
            schema: d.schema.clone(),
            bytes: d.bytes.clone(),
        })
        .collect();
    match purpose {
        "admit" => {
            require(
                matches!(
                    payload["schema"].as_str(),
                    Some("ledger-workflow-activation/1" | "ledger-workflow-activation-replace/1")
                ),
                "INVALID_SHAPE",
            )?;
            let grant = source.grant().ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
            require(
                source.state().status() == "active"
                    && grant.ceiling().contains_time(at)
                    && grant.ceiling().principal_id() == actor
                    && grant.ceiling().source() == key.source
                    && grant.ceiling().canonical_economic_namespace() == b["source_namespace"],
                "AUTHORITY_REQUIRED",
            )?;
            let ceiling = grant.ceiling().canonical_value().map_err(typed_error)?;
            coverage(
                &b,
                accepted.acceptance.definitions.product.canonical_value(),
                &accepted.acceptance.private_documents,
                &ceiling,
                key.source.as_str(),
                text(&b, "customer")?,
            )?;
        }
        "stop" => {
            require(
                payload["schema"] == "ledger-workflow-agreement-revoke/1",
                "INVALID_SHAPE",
            )?;
            for r in payload["evidence_refs"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                let r = reference(r)?;
                verify::document_origin(tx, key, &r).await?;
                let d = loaded(tx, key, &r).await?;
                let body = canonical_body(&d)?;
                let input = evidence_input(&body)?;
                require(
                    input.purpose() == "admission_stop"
                        && input.customer() == text(&b, "customer")?
                        && input.agreement_id() == Some(text(&b, "agreement_id")?)
                        && body["retained_by"] == actor
                        && body["installation_mode"] == "real_local"
                        && input.metadata()["binding_ref"] == json!(accepted.binding.reference)
                        && input.metadata()["target_transition_ref"] == payload["transition_ref"]
                        && input.metadata()["reason"] == payload["reason"],
                    "AUTHORITY_REQUIRED",
                )?;
                docs.push(d);
            }
            require(
                payload["evidence_refs"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty()),
                "AUTHORITY_REQUIRED",
            )?;
        }
        _ => return Err(ServiceError::IntegrityFailure),
    }
    Ok(docs)
}
/// Retaining an explicit stop statement authenticates its original binding and
/// target, but performs no stop/CAS/source-time admission itself.
pub(in crate::service::workflow::agreement) async fn stop_evidence_dependencies(
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
) -> Result<Vec<Document>> {
    require(input.purpose() == "admission_stop", "INVALID_SHAPE")?;
    let binding_ref = reference(&input.metadata()["binding_ref"])?;
    let target_ref = reference(&input.metadata()["target_transition_ref"])?;
    verify::document_origin(tx, key, &target_ref).await?;
    let target = loaded(tx, key, &target_ref).await?;
    let body = canonical::parse_bounded(&target.bytes, 262144)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    require(
        target_ref.kind == "workflow-activation" && body["binding_ref"] == json!(binding_ref),
        "INVALID_REFERENCE",
    )?;
    let accepted = context::observe_accepted_binding(tx, key, &binding_ref, i64::MAX).await?;
    let b = canonical_body(&accepted.binding)?;
    require(
        input.customer() == text(&b, "customer")?
            && input.agreement_id() == Some(text(&b, "agreement_id")?),
        "INVALID_REFERENCE",
    )?;
    let mut docs: Vec<_> = accepted
        .documents
        .into_iter()
        .filter(|d| {
            !matches!(
                d.reference.kind.as_str(),
                "workflow-head-transition" | "workflow-agreement-intent"
            )
        })
        .collect();
    let head: RecordRef = reference(&body["head_transition_ref"])?;
    docs.extend(Box::pin(replay::predecessor_closure(tx, key, &head)).await?);
    docs.push(target);
    Ok(docs)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_bound_projection_allows_unrelated_narrowing_but_keeps_unused_bound_activity_and_correction()
     {
        let terms=records::source_document("workflow-slot-terms",json!({"schema":"ledger-workflow-slot-binding-terms/1","allowed_activities":["used","bound-unused"],"correction_source_slot":"correction"})).unwrap();
        let binding = json!({"source_slots":[{"name":"work","sources":["urn:source"]}],"bindings":[{"binding_ref":terms.reference}]});
        let product = json!({"product_id":"product"});
        let ceiling = json!({"customer_scope":["customer"],"permissions":["read","submit","correct"],"operations":["work.accept","quantity.correct"],"product_selectors":[{"names":["product"]}],"activity_selectors":[{"names":["used","bound-unused"]}],"source_slots":["work","correction"]});
        let docs = vec![terms];
        coverage(
            &binding,
            &product,
            &docs,
            &ceiling,
            "urn:source",
            "customer",
        )
        .unwrap();
        // No requirement for unrelated old extras: exact remaining nomination
        // covers both used and unused bound activity and its correction source.
        for mode in 0..4 {
            let mut bad = ceiling.clone();
            match mode {
                0 => bad["activity_selectors"] = json!([{"names":["used"]}]),
                1 => bad["source_slots"] = json!(["work"]),
                2 => bad["operations"] = json!(["work.accept"]),
                _ => bad["permissions"] = json!(["read", "submit"]),
            };
            assert!(coverage(&binding, &product, &docs, &bad, "urn:source", "customer").is_err());
        }
    }
}
