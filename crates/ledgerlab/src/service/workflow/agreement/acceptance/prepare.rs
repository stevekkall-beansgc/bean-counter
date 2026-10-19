//! Fresh acceptance preparation inside the caller's held writer transaction.
//! No public append or successful facade is exposed by this preparer. Commit
//! still requires the original-recipe verifier after the atomic append.
use super::super::*;
use super::{authority, construct, AcceptInput, PublicationInputs};
use crate::service::workflow::local::LocalWorkflowLedger;
use crate::store::sqlite::workflow::agreement::acceptance::HeadKind;

pub(super) async fn normalized(
    tx: &mut SqliteTx,
    req: &request::Request,
    publication: &context::ObservedPublication,
) -> Result<(AcceptInput, PublicationInputs, Vec<Document>)> {
    let definitions = PublicationInputs::original(publication)?;
    let mut input = AcceptInput::parse(
        &records::bytes(req.payload())?,
        &req.key().scope,
        &definitions.context,
    )?;
    let mut retained = vec![];
    for r in input.private_refs()? {
        if let Some(d) = tx
            .workflow_document(
                &r,
                &req.key().logical_store_id,
                &req.key().scope[0],
                &req.key().scope[1],
                None,
            )
            .await
            .map_err(crate::service::store_error)?
        {
            verify::document_origin(tx, req.key(), &r).await?;
            retained.push(d);
        }
    }
    if input.supplier_recipe() {
        require(
            publication.original_registry().is_some(),
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        // Terms4 may itself have been retained. Resolve its exact Offer2 edge
        // after acquiring that body; never substitute a current offer/version.
        for r in input.resolved_private_refs(&retained)? {
            if retained.iter().any(|d| d.reference == r) {
                continue;
            }
            if let Some(d) = tx
                .workflow_document(
                    &r,
                    &req.key().logical_store_id,
                    &req.key().scope[0],
                    &req.key().scope[1],
                    None,
                )
                .await
                .map_err(crate::service::store_error)?
            {
                verify::document_origin(tx, req.key(), &r).await?;
                retained.push(d);
            }
        }
    }
    let private = input.normalize_private_documents(
        &definitions.template,
        &definitions.product,
        &retained,
    )?;
    Ok((input, definitions, private))
}

pub(super) async fn fresh(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &request::Request,
) -> Result<JournalAppend> {
    #[cfg(test)]
    let begun = std::time::Instant::now();
    require(
        req.kind() == "agreement.accept",
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let key = req.key();
    ledger.admin.read(&ledger.store, tx, key).await?;
    // The coordinator must handle original identity before entering this fresh
    // path, including wrong-kind identities in the shared operation namespace.
    require(
        tx.workflow_operation_kind(&key.bytes()?)
            .await
            .map_err(crate::service::store_error)?
            .is_none(),
        "IDENTITY_CONFLICT",
    )?;
    let publication_ref: RecordRef =
        serde_json::from_value(req.payload()["publication_ref"].clone())
            .map_err(|_| reject("INVALID_REFERENCE"))?;
    #[cfg(test)]
    phase(key, "before original publication", begun);
    let publication = context::observe_publication(tx, key, &publication_ref).await?;
    #[cfg(test)]
    phase(key, "before private normalization", begun);
    let (input, definitions, private) = normalized(tx, req, &publication).await?;
    #[cfg(test)]
    phase(key, "private normalization complete", begun);
    let payload = input.value()?;
    let semantic_key = records::bytes(&json!([
        input.customer(),
        input.agreement_id(),
        payload["binding_inputs"]["version"]
    ]))?;
    if let Some(identity) = tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, req.kind(), &semantic_key)
        .await
        .map_err(crate::service::store_error)?
    {
        let old = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let old_key = super::super::super::request::OperationKey::parse(&old.identity)?;
        let original = verify::retained(tx, &old_key, &old).await?;
        let mut intent = original["request_evidence"]["normalized_request"]["payload"].clone();
        intent
            .as_object_mut()
            .ok_or(ServiceError::IntegrityFailure)?
            .remove("documents");
        require(intent == input.semantic_intent()?, "SEMANTIC_CONFLICT")?;
        // This branch must use the reviewed authority/3 alias recipe, not
        // fresh grant/time eligibility or old authority/2 wrappers.
        return alias04::prepare(ledger, tx, req, &old, &payload).await;
    }
    if let Some(owner) = tx
        .workflow_agreement_owner(&key.logical_store_id, &key.scope, input.agreement_id())
        .await
        .map_err(crate::service::store_error)?
    {
        require(owner == input.customer(), "AGREEMENT_OWNERSHIP_CONFLICT")?;
    }
    let head = tx
        .workflow_agreement_head(
            &key.logical_store_id,
            &key.scope,
            input.customer(),
            input.agreement_id(),
            HeadKind::Acceptance,
        )
        .await
        .map_err(crate::service::store_error)?;
    let revision = head.as_ref().map(|h| h.revision).unwrap_or(0);
    require(
        revision.to_string() == input.expected_revision(),
        "STALE_REVISION",
    )?;
    let mut predecessors = vec![];
    #[cfg(test)]
    phase(key, "before prior head proof", begun);
    if let Some(head) = &head {
        verify::document_origin(tx, key, &head.transition).await?;
        let prior = loaded(tx, key, &head.transition).await?;
        let body = canonical::parse_bounded(&prior.bytes, 262144)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let revision_text = revision.to_string();
        require(
            body["head_kind"] == "agreement_acceptance"
                && body["scope"] == json!(key.scope)
                && body["customer"] == input.customer()
                && body["agreement_id"] == input.agreement_id()
                && body["new_revision"].as_str() == Some(revision_text.as_str()),
            "INTEGRITY_FAILURE",
        )?;
        predecessors =
            verify::authenticated_closure(tx, key, std::slice::from_ref(&head.transition)).await?;
    }
    #[cfg(test)]
    phase(key, "before current admission proof", begun);
    let admitted = authority::check(ledger, tx, key, &input, &definitions, &private).await?;
    #[cfg(test)]
    phase(key, "current admission proof complete", begun);
    let root = journal::fresh_root(&admitted.setup, &admitted.at)?;
    let source = admitted
        .source
        .grant()
        .ok_or(ServiceError::IntegrityFailure)?;
    let authority = records::source_document(
        "workflow-authority",
        json!({
            "schema":"ledger-workflow-agreement-authority/2","purpose":"admit","operation":req.kind(),
            "actor":admitted.setup.operator,"source":admitted.setup.source,"scope":key.scope,
            "customer":input.customer(),"agreement_id":input.agreement_id(),"local_admin_ref":root.reference,
            "canonical_economic_namespace":source.ceiling().canonical_economic_namespace(),
            "source_grant_refs":payload["authority_refs"],"assent_refs":payload["assent_refs"],
            "source_revision":admitted.source.state().revision().to_string(),
            "source_state_refs":[admitted.source.state().reference()],"observed_at":admitted.at
        }),
    )?;
    let sequence = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let built = construct::build(
        &input,
        &definitions,
        publication.publication(),
        publication.plan(),
        &authority,
        construct::CommitMetadata {
            key,
            actor: &admitted.setup.operator,
            at: &admitted.at,
            sequence,
            prior: head.map(|h| h.transition),
        },
    )?;
    let descriptors: Vec<_> = publication
        .sources()
        .filter(|d| {
            matches!(
                d.reference.kind.as_str(),
                "workflow-fact-schema" | "workflow-compiler-descriptor"
            )
        })
        .map(|d| d.reference.clone())
        .collect();
    let evidence = descriptor::acceptance_evidence(req, &payload, &descriptors, &publication)?;
    let mut documents = admitted.documents;
    documents.extend(predecessors);
    documents.extend(publication.documents().iter().map(|d| Document {
        reference: d.reference.clone(),
        schema: d.schema.clone(),
        bytes: d.bytes.clone(),
    }));
    documents.extend(private);
    documents.extend(built.documents);
    journal::assemble_agreement(
        req,
        evidence,
        journal::AgreementCommit {
            setup: &admitted.setup,
            at: &admitted.at,
            sequence,
            root,
            authority,
            documents,
            results: built.results,
            semantic_key,
            alias_of: None,
            projection: built.projection,
        },
    )
}

#[cfg(test)]
fn phase(key: &super::super::super::request::OperationKey, label: &str, begun: std::time::Instant) {
    eprintln!(
        "ACCEPT-DIAG {} prepare {label} elapsed_ms={}",
        key.operation_id,
        begun.elapsed().as_millis()
    );
    verify::diagnostic_checkpoint(&format!("{} prepare {label}", key.operation_id));
}
