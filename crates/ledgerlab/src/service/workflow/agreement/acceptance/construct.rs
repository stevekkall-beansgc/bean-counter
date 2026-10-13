//! Deterministic record construction from already resolved data. This does not
//! authenticate admission or perform a head CAS; the held coordinator does so.
use super::super::*;
use super::{AcceptInput, PublicationInputs};
use crate::service::workflow::request::OperationKey;
use crate::store::sqlite::workflow::agreement::acceptance::{AcceptanceProjection, AgreementHead};

pub(in crate::service::workflow::agreement) struct CommitMetadata<'a> {
    pub(in crate::service::workflow::agreement) key: &'a OperationKey,
    pub(in crate::service::workflow::agreement) actor: &'a str,
    pub(in crate::service::workflow::agreement) at: &'a Timestamp,
    pub(in crate::service::workflow::agreement) sequence: i64,
    pub(in crate::service::workflow::agreement) prior: Option<RecordRef>,
}
pub(in crate::service::workflow::agreement) struct PreparedRecords {
    pub(in crate::service::workflow::agreement) documents: Vec<Document>,
    pub(in crate::service::workflow::agreement) results: Vec<RecordRef>,
    pub(in crate::service::workflow::agreement) projection: Projection,
}
pub(in crate::service::workflow::agreement) fn build(
    input: &AcceptInput,
    definitions: &PublicationInputs,
    publication: &Document,
    plan: &Document,
    authority: &Document,
    metadata: CommitMetadata<'_>,
) -> Result<PreparedRecords> {
    let payload = input.value()?;
    let key = metadata.key;
    if payload["publication_ref"] != json!(publication.reference)
        || publication.reference.kind != "workflow-publication"
        || plan.reference.kind != "workflow-compiled-plan"
        || authority.reference.kind != "workflow-authority"
        || metadata.sequence <= 0
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let old = input
        .expected_revision()
        .parse::<i64>()
        .map_err(|_| reject("INVALID_SCALAR"))?;
    let new = old.checked_add(1).ok_or_else(|| reject("INVALID_SCALAR"))?;
    if (old == 0) != metadata.prior.is_none() {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut binding = payload["binding_inputs"].clone();
    let version = text(&binding, "version")?
        .parse::<u64>()
        .map_err(|_| reject("INVALID_SCALAR"))?;
    binding["schema"] = json!("ledger-workflow-binding/1");
    binding["policy_ref"] = json!(definitions.policy_ref);
    binding["product_ref"] = json!(definitions.product_ref);
    binding["fact_schema_refs"] = json!(definitions.fact_schema_refs);
    binding["compiled_plan_ref"] = json!(plan.reference);
    binding["evaluator"] = json!("ledger-workflow-evaluator/1");
    binding["accepted_by"] = json!(metadata.actor);
    binding["accepted_at"] = json!(metadata.at);
    let binding = records::source_document("workflow-binding", binding)?;
    let intent = records::source_document(
        "workflow-agreement-intent",
        json!({"schema":"ledger-workflow-agreement-intent/1","kind":"agreement.accept","payload":payload}),
    )?;
    let accepted = record::semantic_document(
        "workflow-accepted-agreement",
        key,
        input.customer(),
        input.agreement_id(),
        version,
        json!({"schema":"ledger-workflow-accepted-agreement/1","operation_key":key,"scope":key.scope,
            "customer":input.customer(),"agreement_id":input.agreement_id(),"version":version.to_string(),
            "publication_ref":publication.reference,"binding_ref":binding.reference,
            "terms_ref":input.terms().reference(),"terms_digest":input.terms().terms_digest(),
            "authority_observation_ref":authority.reference,"accepted_at":metadata.at,
            "commit_sequence":metadata.sequence.to_string()}),
    )?;
    let mut head = json!({"schema":"ledger-workflow-head-transition/1","head_kind":"agreement_acceptance",
        "scope":key.scope,"customer":input.customer(),"agreement_id":input.agreement_id(),"operation_key":key,
        "actor":metadata.actor,"old_revision":old.to_string(),"new_revision":new.to_string(),
        "accepted_binding_ref":binding.reference,"accepted_at":metadata.at,"commit_sequence":metadata.sequence.to_string()});
    if let Some(prior) = metadata.prior {
        require(
            prior.kind == "workflow-head-transition",
            "INVALID_REFERENCE",
        )?;
        head["prior_head_ref"] = json!(prior);
    }
    let head = record::semantic_document(
        "workflow-head-transition",
        key,
        input.customer(),
        input.agreement_id(),
        new as u64,
        head,
    )?;
    let mut results = vec![
        accepted.reference.clone(),
        head.reference.clone(),
        intent.reference.clone(),
    ];
    records::sort_refs(&mut results)?;
    let projection = Projection::Acceptance(AcceptanceProjection {
        customer: input.customer().into(),
        agreement: input.agreement_id().into(),
        expected_revision: old,
        head: AgreementHead {
            revision: new,
            transition: head.reference.clone(),
        },
        accepted: accepted.reference.clone(),
        binding: binding.reference.clone(),
        intent: intent.reference.clone(),
    });
    Ok(PreparedRecords {
        documents: vec![binding, intent, accepted, head],
        results,
        projection,
    })
}

#[cfg(test)]
mod tests;
