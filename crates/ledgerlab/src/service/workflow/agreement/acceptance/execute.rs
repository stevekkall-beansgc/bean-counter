//! Local agreement entry: original recovery precedes fresh eligibility; every
//! append must pass saved-recipe verification inside the same writer lock.
use super::super::*;
use super::prepare;
use crate::{
    service::workflow::local::{deadline, LocalWorkflowLedger},
    store::ports::{AcceptanceStore, AcceptanceTx},
};

#[cfg(test)]
static LOST_REPLY_KEY: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(super) fn inject_committed_reply_loss(key: Vec<u8>) {
    let mut selected = LOST_REPLY_KEY.lock().unwrap();
    assert!(selected.is_none());
    *selected = Some(key);
}

pub(super) async fn execute(ledger: &LocalWorkflowLedger, raw: &[u8]) -> Result<Value> {
    let req = request::Request::parse_agreement(raw)?;
    require(
        matches!(
            req.kind(),
            "agreement.accept" | "agreement.activate" | "agreement.revoke"
        ),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let key = req.key();
    #[cfg(test)]
    eprintln!("ACCEPT-DIAG {} before begin writer", key.operation_id);
    let mut tx = ledger
        .store
        .begin(deadline())
        .await
        .map_err(crate::service::store_error)?;
    #[cfg(test)]
    let writer_begun = std::time::Instant::now();
    let attempted: Result<(Value, bool)> = proof_session::with_proofs(&mut tx, key, |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, key).await?;
            #[cfg(test)]
            verify::diagnostic_checkpoint(&format!(
                "{} writer acquired/admin read writer_ms={}",
                key.operation_id,
                writer_begun.elapsed().as_millis()
            ));
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
                let original = verify::retained(tx, key, &old).await?;
                let saved = &original["request_evidence"];
                let alias = canonical::parse_bounded(&old.operation_bytes, 8388608)
                    .map_err(|_| ServiceError::IntegrityFailure)?["schema"]
                    == "ledger-workflow-agreement-alias-operation/1";
                if saved["original_request_bytes_sha256"] != records::raw_hash(req.raw()) {
                    let evidence = if req.kind() == "agreement.accept" {
                        require(
                            req.payload()["publication_ref"]
                                == saved["normalized_request"]["payload"]["publication_ref"],
                            "IDENTITY_CONFLICT",
                        )?;
                        let data =
                            replay::acceptance_data(tx, key, &req)
                                .await
                                .map_err(|e| match e {
                                    ServiceError::Rejection(_) => reject("IDENTITY_CONFLICT"),
                                    e => e,
                                })?;
                        if alias {
                            alias04::evidence(&req, &data.input.value()?, &data.descriptors)?
                        } else {
                            descriptor::evidence(&req, &data.input.value()?, &data.descriptors)?
                        }
                    } else {
                        require(
                            req.payload()["accepted_binding_ref"]
                                == saved["normalized_request"]["payload"]["accepted_binding_ref"],
                            "IDENTITY_CONFLICT",
                        )?;
                        let reference: RecordRef =
                            serde_json::from_value(req.payload()["accepted_binding_ref"].clone())
                                .map_err(|_| reject("IDENTITY_CONFLICT"))?;
                        let accepted =
                            context::observe_accepted_binding(tx, key, &reference, old.sequence)
                                .await?;
                        let payload = activation_verify::normalize(&req, &accepted.binding)
                            .map_err(|e| match e {
                                ServiceError::Rejection(_) => reject("IDENTITY_CONFLICT"),
                                e => e,
                            })?;
                        if alias {
                            alias04::evidence(&req, &payload, &accepted.acceptance.descriptors)?
                        } else {
                            descriptor::evidence(&req, &payload, &accepted.acceptance.descriptors)?
                        }
                    };
                    require(
                        evidence["request_hash"] == saved["request_hash"]
                            && evidence["descriptor_resolution"] == saved["descriptor_resolution"],
                        "IDENTITY_CONFLICT",
                    )?;
                }
                return Ok((original, false));
            }
            #[cfg(test)]
            verify::diagnostic_checkpoint(&format!(
                "{} before fresh preparation",
                key.operation_id
            ));
            let append = if req.kind() == "agreement.accept" {
                prepare::fresh(ledger, tx, &req).await?
            } else {
                super::activation::fresh(ledger, tx, &req).await?
            };
            #[cfg(test)]
            verify::diagnostic_checkpoint(&format!("{} before append", key.operation_id));
            tx.workflow_agreement_append(&append).await.map_err(|e| {
                if matches!(e, crate::store::errors::StoreError::DeliveryConflict) {
                    reject("SEMANTIC_CONFLICT")
                } else {
                    crate::service::store_error(e)
                }
            })?;
            #[cfg(test)]
            verify::diagnostic_checkpoint(&format!(
                "{} before retained after append",
                key.operation_id
            ));
            proof_session::after_append(tx, key)?;
            let verified = verify::retained(tx, key, &append.operation).await?;
            #[cfg(test)]
            verify::diagnostic_checkpoint(&format!(
                "{} retained after append complete",
                key.operation_id
            ));
            Ok((verified, true))
        })
    })
    .await;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} attempted complete writer_ms={} result={:?}",
        key.operation_id,
        writer_begun.elapsed().as_millis(),
        attempted.as_ref().map(|_| "ok")
    ));
    match attempted {
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
            Ok(result)
        }
    }
}
