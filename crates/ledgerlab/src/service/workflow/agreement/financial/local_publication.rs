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
    F: FnOnce(&Value, &mut OriginalAuditSession) -> Result<SelectedCompilerSupport>,
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

pub(super) async fn verify_original(
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<Option<prefix::RecompiledPublication>> {
    if original.wrappers().class() != wrappers::WireClass::Publication {
        return Ok(None);
    }
    let evidence = request_wrapper::field(original.wrappers().operation(), "request_evidence", s)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    if request_wrapper::text(resolution, "registry_digest", s)? != wrappers::LOCAL_REGISTRY {
        return Ok(None);
    }
    let publication = original.recompile_publication(tx, s).await?;
    let incoming = PublicationRequest::parse(original.wrappers().original_request().raw())?;
    verify_sources(
        tx,
        &incoming,
        &publication,
        original.wrappers().sequence(),
        s,
    )
    .await?;
    let reproduced = publication::evidence(&incoming, publication.program())?;
    same_value(&reproduced, evidence, s)?;
    Ok(Some(publication))
}
fn same_value(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let a = s
        .canonical_representation(a, RepresentationKind::OperationFrame)
        .map_err(core_error)?;
    let b = s
        .canonical_representation(b, RepresentationKind::OperationFrame)
        .map_err(core_error)?;
    s.semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(a.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(b.bytes().len())
        .map_err(core_error)?;
    require(a.bytes() == b.bytes(), "IDENTITY_CONFLICT")
}
async fn verify_sources(
    tx: &mut SqliteTx,
    req: &PublicationRequest,
    publication: &prefix::RecompiledPublication,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    use ledgerlab_core::policy::workflow::{
        selected_program::{normalize_local_publication_input, SelectedSourceInput},
        DefinitionRef,
    };
    let program = publication.program();
    let root = req.root();
    require(
        root.kind == program.root_policy_ref().kind()
            && root.id == program.root_policy_ref().id()
            && root.digest == program.root_policy_ref().digest(),
        "IDENTITY_CONFLICT",
    )?;
    require(
        req.expected_digest() == program.compiled_plan().reference().digest(),
        "IDENTITY_CONFLICT",
    )?;
    let mut documents = Vec::new();
    if let Some(inline) = req.inline_documents() {
        for value in inline {
            documents.push(records::bytes(value)?);
        }
    } else {
        let refs = req.retained_refs().ok_or(ServiceError::IntegrityFailure)?;
        require(
            refs.len() == program.source_documents().len(),
            "IDENTITY_CONFLICT",
        )?;
        for reference in refs {
            let exact = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
                kind: reference.kind.clone(),
                id: reference.id.clone(),
                digest: reference.digest.clone(),
            };
            prefix::verify_selected_association(tx, req.key(), &exact, before, s).await?;
            let stored = tx
                .workflow_financial_document(
                    &exact,
                    &req.key().logical_store_id,
                    &req.key().scope,
                    before,
                    s,
                )
                .await
                .map_err(prefix::read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            documents.push(stored.body.bytes().to_vec());
        }
    }
    let normalized = normalize_local_publication_input(
        SelectedSourceInput {
            documents,
            root_policy_ref: DefinitionRef::new(&root.kind, &root.id, &root.digest)
                .map_err(core_error)?,
        },
        program,
        s,
    )
    .map_err(|e| {
        if matches!(
            e.code,
            "INVALID_SHAPE" | "INVALID_SCALAR" | "DEFINITION_DIGEST_MISMATCH"
        ) {
            reject("IDENTITY_CONFLICT")
        } else {
            core_error(e)
        }
    })?;
    require(
        normalized.source_documents().len() == program.source_documents().len(),
        "IDENTITY_CONFLICT",
    )?;
    for expected in program.source_documents() {
        let mut matches = 0;
        for actual in normalized.source_documents() {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.canonical_bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.canonical_bytes().len())
                .map_err(core_error)?;
            if actual.reference() == expected.reference()
                && actual.canonical_bytes() == expected.canonical_bytes()
            {
                matches += 1;
            }
        }
        require(matches == 1, "IDENTITY_CONFLICT")?;
    }
    Ok(())
}
pub(super) async fn compare_retry(
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    original: &prefix::LocatedWrappers,
    publication: &prefix::RecompiledPublication,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    request_wrapper::same_text(req.kind(), "policy.publish", s)?;
    let incoming = PublicationRequest::parse(req.raw())?;
    verify_sources(
        tx,
        &incoming,
        publication,
        original.wrappers().sequence(),
        s,
    )
    .await?;
    let normalized = publication::normalize_request(&incoming, publication.program())?;
    let saved = request_wrapper::field(original.wrappers().operation(), "request_evidence", s)?;
    same_value(
        &normalized,
        request_wrapper::field(saved, "normalized_request", s)?,
        s,
    )
}
