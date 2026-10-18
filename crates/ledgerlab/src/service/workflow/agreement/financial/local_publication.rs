//! New local publication preparation uses the real selected compiler and the
//! released atomic publication journal. No descriptor DATA supplies support.
use super::*;
use crate::service::workflow::{publication, records, request::PublicationRequest};
use ledgerlab_core::{
    policy::workflow::selected_program::SelectedCompilerSupport,
    validation_work::{OriginalAuditSession, RepresentationKind},
};

pub(super) async fn prepare_append<F>(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    raw: &[u8],
    fresh_support: F,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply>
where
    F: FnOnce(&mut OriginalAuditSession) -> ledgerlab_core::Result<SelectedCompilerSupport>,
{
    let prepared = publication_prepare::prepare_new(ledger, tx, raw, fresh_support, s).await?;
    request_wrapper::same_text(
        prepared
            .compiled
            .program
            .execution_registry()
            .ok_or(ServiceError::Unavailable)?,
        wrappers::LOCAL_REGISTRY,
        s,
    )?;
    let incoming = PublicationRequest::parse(raw)?;
    let key = incoming.key();
    let evidence = publication::evidence(&incoming, &prepared.compiled.program)?;
    let provenance =
        publication::BuildProvenance::capture(prepared.compiled.program.compiler_descriptor_ref())?;
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let pub_hash = records::hash(
        "workflow-publication",
        1,
        &serde_json::json!([key.logical_store_id, key.scope, incoming.root()]),
    )?;
    let pub_id = format!("wpub_{}", &pub_hash[7..]);
    let existing = if tx
        .workflow_publication(pub_id.as_bytes())
        .await
        .map_err(crate::service::store_error)?
        .is_some()
    {
        let publication = tx
            .workflow_document_by_id(&pub_id, &key.logical_store_id, &key.scope[0], &key.scope[1])
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let build_ref = tx
            .workflow_publication_build_ref(
                &pub_id,
                &key.logical_store_id,
                &key.scope[0],
                &key.scope[1],
            )
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let build = tx
            .workflow_document(
                &build_ref,
                &key.logical_store_id,
                &key.scope[0],
                &key.scope[1],
                None,
            )
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        Some((publication, Some(build)))
    } else {
        None
    };
    // Retained inputs were loaded under this same held writer. Their exact
    // committed association is checked before the compiler output is staged.
    prepared
        .compiled
        .verify_local_retained(tx, key, prepared.before, s)
        .await?;
    for template in prepared
        .compiled
        .program
        .source_documents()
        .iter()
        .filter(|d| d.reference().kind() == "workflow-slot-template")
    {
        if let Some(previous) = template.canonical_value().get("previous") {
            let previous = current::source::from_value(previous, "workflow-slot-template", s)?;
            prefix::verify_selected_association(tx, key, &previous.0, prepared.before, s).await?;
        }
    }
    let append = publication::assemble(
        &incoming,
        &prepared.compiled.program,
        evidence,
        &setup,
        &prepared.at,
        prepared.before,
        &provenance,
        existing,
    )?;
    let operation = s
        .parse_representation(
            &append.operation.operation_bytes,
            RepresentationKind::OperationFrame,
        )
        .map_err(core_error)?;
    let receipt = s
        .parse_representation(
            &append.operation.receipt_bytes,
            RepresentationKind::ReceiptFrame,
        )
        .map_err(core_error)?;
    let evidence = request_wrapper::field(operation.value(), "request_evidence", s)?;
    let reply = WorkflowReply::publication(receipt.value(), evidence, s)?;
    prepared.recheck(ledger, tx, s).await?;
    tx.workflow_append(&append).await.map_err(|e| {
        if matches!(e, crate::store::errors::StoreError::DeliveryConflict) {
            reject("SEMANTIC_CONFLICT")
        } else {
            crate::service::store_error(e)
        }
    })?;
    proof_session::after_append(tx, key)?;
    Ok(reply)
}
