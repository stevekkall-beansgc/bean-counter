//! Local agreement entry: original recovery precedes fresh eligibility; every
//! append must pass saved-recipe verification inside the same writer lock.
use super::super::*;
use super::prepare;
use crate::{
    service::workflow::local::{deadline, LocalWorkflowLedger},
    store::ports::{AcceptanceStore, AcceptanceTx},
};

pub(super) async fn execute(ledger: &LocalWorkflowLedger, raw: &[u8]) -> Result<Value> {
    let req = request::Request::parse_agreement(raw)?;
    require(
        req.kind() == "agreement.accept",
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let key = req.key();
    let mut tx = ledger
        .store
        .begin(deadline())
        .await
        .map_err(crate::service::store_error)?;
    let attempted: Result<(Value, bool)> = async {
        ledger.admin.read(&ledger.store, &mut tx, key).await?;
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
            let original = verify::retained(&mut tx, key, &old).await?;
            let saved = &original["request_evidence"];
            if saved["original_request_bytes_sha256"] != records::raw_hash(req.raw()) {
                require(
                    req.payload()["publication_ref"]
                        == saved["normalized_request"]["payload"]["publication_ref"],
                    "IDENTITY_CONFLICT",
                )?;
                let data =
                    replay::acceptance_data(&mut tx, key, &req)
                        .await
                        .map_err(|e| match e {
                            ServiceError::Rejection(_) => reject("IDENTITY_CONFLICT"),
                            e => e,
                        })?;
                let evidence = descriptor::evidence(&req, &data.input.value()?, &data.descriptors)?;
                require(
                    evidence["request_hash"] == saved["request_hash"]
                        && evidence["descriptor_resolution"] == saved["descriptor_resolution"],
                    "IDENTITY_CONFLICT",
                )?;
            }
            return Ok((original, false));
        }
        let append = prepare::fresh(ledger, &mut tx, &req).await?;
        tx.workflow_agreement_append(&append).await.map_err(|e| {
            if matches!(e, crate::store::errors::StoreError::DeliveryConflict) {
                reject("SEMANTIC_CONFLICT")
            } else {
                crate::service::store_error(e)
            }
        })?;
        Ok((
            verify::retained(&mut tx, key, &append.operation).await?,
            true,
        ))
    }
    .await;
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
            result["schema"] = json!("ledger-operation-result/1");
            Ok(result)
        }
    }
}
