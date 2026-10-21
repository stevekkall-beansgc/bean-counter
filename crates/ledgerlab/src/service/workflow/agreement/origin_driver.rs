//! Untrusted, compact origin demands. Only the held-transaction driver can
//! turn these locators into completed proofs; preparation grants no authority.
use super::super::request::OperationKey;
use super::*;
use std::collections::BTreeMap;

pub(super) enum Stage {
    Members,
    HistoricalPrefixes,
    Semantics,
}
pub(super) struct LocalContinuation {
    pub(super) identity: Vec<u8>,
    pub(super) binding: String,
    pub(super) stage: Stage,
}
pub(super) struct OriginDemand {
    pub(super) references: Vec<RecordRef>,
    pub(super) identity: Vec<u8>,
    pub(super) sequence: i64,
    pub(super) parent_sequence: i64,
    pub(super) kind: String,
    pub(super) original: bool,
}
pub(super) enum LocalStep {
    NeedOrigins {
        demands: Vec<OriginDemand>,
        resume: LocalContinuation,
    },
    Complete(verify::LocalCertificate),
}

fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
async fn add_reference(
    tx: &mut SqliteTx,
    key: &OperationKey,
    parent: &JournalOperation,
    r: &RecordRef,
    demands: &mut BTreeMap<Vec<u8>, OriginDemand>,
) -> Result<()> {
    let (identity, kind, sequence) = tx
        .workflow_agreement_document_origin(&key.logical_store_id, &key.scope, r)
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if identity == parent.identity {
        // Candidate own members are NOT completed proofs. The local checker
        // still validates their exact bodies and complete manifest/receipt.
        if sequence != parent.sequence || !parent.members.contains(r) {
            return Err(ServiceError::IntegrityFailure);
        }
        return Ok(());
    }
    if sequence >= parent.sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    // A released prerequisite can retain a root born under the local recipe.
    // Prove that exact scoped member with its own wrapper validator rather
    // than scheduling its newer request through the released R1 parser.
    if financial::local_selected_origin(tx, key, r, sequence).await? {
        return Ok(());
    }
    let demand = demands.entry(identity.clone()).or_insert(OriginDemand {
        references: Vec::new(),
        identity,
        sequence,
        parent_sequence: parent.sequence,
        kind: kind.clone(),
        original: false,
    });
    if demand.sequence != sequence || demand.kind != kind {
        return Err(ServiceError::IntegrityFailure);
    }
    if !demand.references.contains(r) {
        demand.references.push(r.clone());
    }

    Ok(())
}
fn add_operation(
    parent: &JournalOperation,
    o: JournalOperation,
    original: bool,
    demands: &mut BTreeMap<Vec<u8>, OriginDemand>,
) -> Result<()> {
    if o.sequence >= parent.sequence {
        return Err(ServiceError::IntegrityFailure);
    }
    let d = demands.entry(o.identity.clone()).or_insert(OriginDemand {
        references: Vec::new(),
        identity: o.identity,
        sequence: o.sequence,
        parent_sequence: parent.sequence,
        kind: o.kind,
        original,
    });
    d.original |= original;
    Ok(())
}
/// Validate birth sequence/body copies BEFORE selecting the immutable prefix.
/// This is the same untrusted locator validation as source/schedule replay;
/// full certificates are then scheduled, never treated as verified here.
fn prefix_operation(key: &OperationKey, o: &JournalOperation, source: bool) -> Result<()> {
    let saved = OperationKey::parse(&o.identity).map_err(|_| ServiceError::IntegrityFailure)?;
    let body = verify::checked(&o.operation_bytes, 8388608)?;
    if saved.logical_store_id != key.logical_store_id
        || saved.scope != key.scope
        || body["key"] != json!(saved)
        || body["kind"] != o.kind
        || body["commit_sequence"] != o.sequence.to_string().as_str()
        || o.sequence <= 0
        || (source && o.kind != "authority.change")
        || (!source && !matches!(o.kind.as_str(), "agreement.activate" | "agreement.revoke"))
        || records::record_reference("workflow-operation", &key.scope, &body)? != o.operation_ref
        || (!source
            && Timestamp::parse(text(&body, "accepted_at")?)
                .map_err(|_| ServiceError::IntegrityFailure)?
                .micros()
                != o.accepted_at_us)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}

pub(super) async fn prepare(
    tx: &mut SqliteTx,
    key: &OperationKey,
    o: &JournalOperation,
    continuation: LocalContinuation,
) -> Result<LocalStep> {
    if continuation.identity != o.identity
        || continuation.binding != proof_session::journal_binding(o)?
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut demands = BTreeMap::new();
    let stage = match continuation.stage {
        Stage::Members => {
            if o.members.len() > 4096 {
                return Err(ServiceError::IntegrityFailure);
            }
            for r in &o.members {
                if *r != o.operation_ref {
                    add_reference(tx, key, o, r, &mut demands).await?;
                }
            }
            // Alias04's old operation is a retained BLOB, not a wdoc. It must
            // be the exact direct original, not a completed alias substitute.
            let body = verify::checked(&o.operation_bytes, 8388608)?;
            if body["schema"] == "ledger-workflow-agreement-alias-operation/1" {
                let r = reference(&body["authority_snapshot"]["original_operation_ref"])?;
                let old = tx
                    .workflow_agreement_operation_ref(&key.logical_store_id, &key.scope, &r)
                    .await
                    .map_err(crate::service::store_error)?
                    .ok_or(ServiceError::IntegrityFailure)?;
                if old.kind != o.kind {
                    return Err(ServiceError::IntegrityFailure);
                }
                add_operation(o, old, true, &mut demands)?;
            }
            Stage::HistoricalPrefixes
        }
        Stage::HistoricalPrefixes => {
            if matches!(
                o.kind.as_str(),
                "agreement.accept" | "agreement.activate" | "agreement.revoke"
            ) {
                let body = verify::checked(&o.operation_bytes, 8388608)?;
                let authority = &body["authority_snapshot"];
                let state = if authority["purpose"] == "admit" {
                    let refs = authority["source_state_refs"]
                        .as_array()
                        .ok_or(ServiceError::IntegrityFailure)?;
                    if refs.len() != 1 {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    reference(&refs[0])?
                } else {
                    reference(&authority["source_observation"]["state_ref"])?
                };
                let state = loaded(tx, key, &state).await?;
                let enrollment_ref =
                    reference(&verify::checked(&state.bytes, 262144)?["enrollment_ref"])?;
                let enrollment = loaded(tx, key, &enrollment_ref).await?;
                let enrollment =
                    ledgerlab_core::policy::workflow::SourceEnrollment::parse(&enrollment.bytes)
                        .map_err(|_| ServiceError::IntegrityFailure)?;
                if enrollment.scope() != &key.scope
                    || json!(enrollment.reference()) != json!(enrollment_ref)
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                let births = tx
                    .workflow_agreement_source_prefix(
                        &key.logical_store_id,
                        &key.scope,
                        &enrollment_ref,
                        enrollment.enrollment_id(),
                    )
                    .await
                    .map_err(crate::service::store_error)?;
                for id in births {
                    let old = tx
                        .workflow_agreement_lookup(&id)
                        .await
                        .map_err(crate::service::store_error)?
                        .ok_or(ServiceError::IntegrityFailure)?;
                    prefix_operation(key, &old, true)?;
                    if old.sequence < o.sequence {
                        add_operation(o, old, false, &mut demands)?;
                    }
                }
                if matches!(o.kind.as_str(), "agreement.activate" | "agreement.revoke") {
                    let births = tx
                        .workflow_agreement_activation_prefix(
                            &key.logical_store_id,
                            &key.scope,
                            text(authority, "customer")?,
                            text(authority, "agreement_id")?,
                        )
                        .await
                        .map_err(crate::service::store_error)?;
                    for id in births {
                        let old = tx
                            .workflow_agreement_lookup(&id)
                            .await
                            .map_err(crate::service::store_error)?
                            .ok_or(ServiceError::IntegrityFailure)?;
                        prefix_operation(key, &old, false)?;
                        if old.sequence < o.sequence {
                            add_operation(o, old, false, &mut demands)?;
                        }
                    }
                }
            }
            Stage::Semantics
        }
        Stage::Semantics => {
            let local = proof_session::begin_local(tx, key, &o.identity)?;
            let certificate = verify::validate_local(tx, key, o).await?;
            local.finish()?;
            return Ok(LocalStep::Complete(certificate));
        }
    };
    Ok(LocalStep::NeedOrigins {
        demands: demands.into_values().collect(),
        resume: LocalContinuation {
            identity: continuation.identity,
            binding: continuation.binding,
            stage,
        },
    })
}
