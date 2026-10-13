//! Actual privately held local-owner transport. Every write and projection is
//! committed in the existing poisoned Profile B transaction; no staging pool.
use super::*;
use crate::{
    service::workflow::local::{LocalWorkflowLedger, deadline},
    store::ports::{AcceptanceStore, AcceptanceTx},
};
#[cfg(test)]
static LOST_REPLY_KEY: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(super) fn lose_reply(key: Vec<u8>) {
    let mut selected = LOST_REPLY_KEY.lock().unwrap();
    assert!(selected.is_none());
    *selected = Some(key);
}
pub(in super::super) async fn execute(
    ledger: &LocalWorkflowLedger,
    req: request::Request,
) -> Result<Value> {
    let key = req.key();
    let mut tx = ledger
        .store
        .begin(deadline())
        .await
        .map_err(crate::service::store_error)?;
    let result: Result<(Value, bool)> = proof_session::with_proofs(&mut tx, key, |tx| Box::pin(async {
        ledger.admin.read(&ledger.store, tx, key).await?;
        if let Some(kind) = tx
            .workflow_operation_kind(&key.bytes()?)
            .await
            .map_err(crate::service::store_error)?
        {
            require(kind == req.kind(), "IDENTITY_CONFLICT")?;
            let old = tx
                .workflow_agreement_lookup(&key.bytes()?)
                .await
                .map_err(crate::service::store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let saved = verify::retained(tx, key, &old).await?;
            let previous = &saved["request_evidence"];
            require(
                previous["normalized_request"]["payload"]["schema"] == req.payload()["schema"],
                "IDENTITY_CONFLICT",
            )?;
            if previous["original_request_bytes_sha256"] != records::raw_hash(req.raw()) {
                let normalized = resolve::resolve(tx, key, req.payload(), false).await?;
                let attempted = evidence::request_evidence(&req, &normalized)?;
                require(
                    attempted["request_hash"] == previous["request_hash"]
                        && attempted["descriptor_resolution"] == previous["descriptor_resolution"],
                    "IDENTITY_CONFLICT",
                )?;
            }
            return Ok((saved, false));
        }
        let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
        let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
        let resolution = resolve::resolve(tx, key, req.payload(), true).await?;
        let semantic = semantic(key, &resolution)?;
        let alias_of = tx
            .workflow_agreement_semantic(&key.logical_store_id, &key.scope, req.kind(), &semantic)
            .await
            .map_err(crate::service::store_error)?;
        let root = match tx
            .workflow_agreement_root_candidate(&key.logical_store_id, &key.scope)
            .await
            .map_err(crate::service::store_error)?
        {
            Some(root) => {
                Box::pin(verify::document_origin(tx, key, &root.reference)).await?;
                let body = canonical_body(&root)?;
                verify::root(&body, key, None)?;
                if body["basis"] == journal::basis(&setup) {
                    root
                } else {
                    journal::fresh_root(&setup, &at)?
                }
            }
            None => journal::fresh_root(&setup, &at)?,
        };
        let (result, projection, semantic_alias) =
            if let Some(fragment) = resolution.input.fragment() {
                let upload = Upload {
                    upload_id: fragment.upload_id().into(),
                    document_schema: fragment.document_schema().into(),
                    digest: fragment.document_sha256().into(),
                    bytes: fragment.document_bytes() as i64,
                };
                if tx
                    .workflow_transport_upload(
                        &key.logical_store_id,
                        &key.scope,
                        &key.source,
                        fragment.upload_id(),
                    )
                    .await
                    .map_err(crate::service::store_error)?
                    .as_ref()
                    .is_some_and(|old| old != &upload)
                {
                    return Err(reject("SOURCE_FRAGMENT_MISMATCH"));
                }
                let result = records::source_document(
                    "workflow-definition-fragment",
                    fragment
                        .record_value(&key.logical_store_id, &key.scope, &key.source)
                        .map_err(transport_error)?,
                )?;
                let previous = tx
                    .workflow_transport_part(
                        &key.logical_store_id,
                        &key.scope,
                        &key.source,
                        fragment.upload_id(),
                        fragment.part() as i64,
                    )
                    .await
                    .map_err(crate::service::store_error)?;
                if let Some(id) = previous {
                    require(id == result.reference.id, "SOURCE_FRAGMENT_MISMATCH")?;
                    Box::pin(verify::document_origin(tx, key, &result.reference)).await?;
                    let old = loaded(tx, key, &result.reference).await?;
                    if old.bytes != result.bytes {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    (result, Projection::None, true)
                } else {
                    let part = Part {
                        upload,
                        part: fragment.part() as i64,
                        document: result.reference.id.clone(),
                    };
                    (result, Projection::Fragment(part), false)
                }
            } else {
                let definition = resolution
                    .definition
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .document();
                let result = document(definition)?;
                let entry = registry(definition)?;
                let previous = if let Some(entry) = &entry {
                    tx.workflow_transport_definition(
                        &key.logical_store_id,
                        &key.scope,
                        &entry.kind,
                        &entry.semantic_id,
                        &entry.version,
                    )
                    .await
                    .map_err(crate::service::store_error)?
                } else {
                    tx.workflow_document(
                        &result.reference,
                        &key.logical_store_id,
                        &key.scope[0],
                        &key.scope[1],
                        None,
                    )
                    .await
                    .map_err(crate::service::store_error)?
                    .map(|d| d.reference.id)
                };
                if let Some(id) = previous {
                    require(id == result.reference.id, "SEMANTIC_CONFLICT")?;
                    Box::pin(verify::document_origin(tx, key, &result.reference)).await?;
                    if loaded(tx, key, &result.reference).await?.bytes != result.bytes {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    (result, Projection::None, true)
                } else {
                    (
                        result,
                        entry.map_or(Projection::None, Projection::Definition),
                        false,
                    )
                }
            };
        if let Some(identity) = &alias_of {
            let original = tx
                .workflow_agreement_lookup(identity)
                .await
                .map_err(crate::service::store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let original_key =
                super::super::super::request::OperationKey::parse(&original.identity)?;
            let original_result = verify::retained(tx, &original_key, &original).await?;
            if original_result["receipt"]["result_refs"] != json!([result.reference])
                || !semantic_alias
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        let request_evidence = evidence::request_evidence(&req, &resolution)?;
        let mut documents = vec![result];
        documents.extend(resolution.dependencies);
        documents.extend(resolution.fragments);
        let result_refs = vec![documents[0].reference.clone()];
        let sequence = tx
            .workflow_sequence()
            .await
            .map_err(crate::service::store_error)?;
        let append = journal::assemble_definition(
            &req,
            request_evidence,
            &setup,
            &at,
            sequence,
            root,
            documents,
            result_refs,
            semantic,
            alias_of,
            projection,
            semantic_alias,
        )?;
        tx.workflow_agreement_append(&append).await.map_err(|e| {
            if matches!(e, crate::store::errors::StoreError::DeliveryConflict) {
                reject("SEMANTIC_CONFLICT")
            } else {
                crate::service::store_error(e)
            }
        })?;
        proof_session::after_append(tx, key)?;
        Ok((
            verify::retained(tx, key, &append.operation).await?,
            true,
        ))
    }))
    .await;
    match result {
        Err(e) => {
            tx.rollback().await.map_err(crate::service::store_error)?;
            Err(e)
        }
        Ok((mut result, false)) => {
            tx.rollback().await.map_err(crate::service::store_error)?;
            result["schema"] = json!("ledger-operation-result/1");
            result["duplicate_kind"] = json!("identity");
            Ok(result)
        }
        Ok((mut result, true)) => {
            tx.commit().await.map_err(|e| match e {
                crate::store::errors::CommitError::RolledBack(e) => crate::service::store_error(e),
                crate::store::errors::CommitError::OutcomeUnknown => ServiceError::OutcomeUnknown {
                    scope: key.scope.clone(),
                    source: key.source.clone(),
                    external_id: key.operation_id.clone(),
                },
            })?;
            #[cfg(test)]
            {
                let mut selected = LOST_REPLY_KEY.lock().unwrap();
                if selected.as_ref() == Some(&key.bytes()?) {
                    *selected = None;
                    return Err(ServiceError::ResponseLost);
                }
            }
            result["schema"] = json!("ledger-operation-result/1");
            if result["receipt"]["result_kind"] == "semantic_alias" {
                result["duplicate_kind"] = json!("semantic");
            }
            Ok(result)
        }
    }
}
