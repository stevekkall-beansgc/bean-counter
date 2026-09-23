//! Exact unchanged original-profile proposal plus separately provisioned documents.
use super::*;
use crate::service::{accept::outcome, retained::base as b};
use crate::store::outcomes::*;
use crate::{PrincipalContext, ServiceError};
use ledgerlab_core::{
    adjudication as r3,
    domain::{Scope, Timestamp},
};
use serde_json::{json, Value};

pub(super) struct BaseFixture {
    pub command: outcome::OutcomeCommand,
    pub resolve: OutcomeResolve,
    pub heads: Vec<ObservedOutcomeHead>,
    pub evidence: Vec<Vec<u8>>,
    pub objects: Vec<wire::RetainedObject>,
    proof: outcome::AuthorityProof,
}
impl BaseFixture {
    pub fn new(input: &Value) -> Self {
        let objects: Vec<wire::RetainedObject> =
            serde_json::from_value(input["initial"]["original_objects"].clone()).unwrap();
        let raw: Vec<Vec<u8>> = objects
            .iter()
            .map(|o| r3::proofs::decode_base64(&o.body, r3::COMMAND_BYTES).unwrap())
            .collect();
        let rows: Vec<Value> = raw
            .iter()
            .map(|r| serde_json::from_slice(r).unwrap())
            .collect();
        let records = b::Records::new(&raw, json!(["synthetic", "sandbox"])).unwrap();
        let base = b::decode_base(
            &records,
            &b::reference(records.one("base-acceptance").unwrap()),
        )
        .unwrap();
        let docs: Vec<_> = rows.iter().filter(|r| r["kind"] == "evidence").collect();
        let proof_ref = |purpose: &str| {
            b::reference(
                docs.iter()
                    .find(|r| r["body"]["purpose"] == purpose)
                    .unwrap(),
            )
        };
        let event = rows.iter().find(|r| r["kind"] == "event").unwrap();
        let command = outcome::OutcomeCommand {
            principal: PrincipalContext {
                scope: Scope::new("synthetic", "sandbox").unwrap(),
                principal_id: "synthetic-authorized-principal".into(),
                source: base.evaluation.event().source().into(),
                authority_head: "operator".into(),
                can_read: true,
                can_submit: true,
            },
            target: base.snapshot["body"]["target"].as_str().unwrap().into(),
            invocation_id: "invocation-supplier".into(),
            operation: outcome::OutcomeOperation::FinalBase { seed: raw.clone() },
            received_at: Timestamp::parse("2026-09-21T12:00:00.000000Z").unwrap(),
            accepted_at: Timestamp::parse("2026-09-21T12:00:00.000000Z").unwrap(),
            required: docs
                .iter()
                .map(|r| ScopedRecordRef {
                    scope: ["synthetic".into(), "sandbox".into()],
                    kind: "evidence".into(),
                    id: b::bytes(&r["id"]).unwrap(),
                    content_hash: r["content_hash"].as_str().unwrap().into(),
                })
                .collect(),
        };
        let locks = outcome::fresh_base_locks(&command).unwrap();
        let heads = locks
            .iter()
            .filter_map(|lock| {
                let value = match lock.class {
                    OutcomeLockClass::Authority => {
                        json!({"active":true,"grant":proof_ref("grant")})
                    }
                    OutcomeLockClass::Binding => {
                        let k: Value = serde_json::from_slice(&lock.key).unwrap();
                        let row = rows
                            .iter()
                            .find(|r| {
                                r["kind"] == "binding-snapshot" && r["body"]["binding_id"] == k[1]
                            })
                            .unwrap();
                        json!({"active":true,"binding":b::reference(row)})
                    }
                    _ => return None,
                };
                Some(ObservedOutcomeHead {
                    lock: lock.clone(),
                    revision: Some("1".into()),
                    value: Some(b::bytes(&value).unwrap()),
                })
            })
            .collect();
        let proof = outcome::AuthorityProof {
            scope: ["synthetic".into(), "sandbox".into()],
            target: command.target.clone(),
            invocation_id: command.invocation_id.clone(),
            source: command.principal.source.clone(),
            authority: json!({"principal":command.principal.principal_id,"grant":proof_ref("grant"),"grant_revision":"1","active":true,"permissions":["close","correct","read","submit"],"evidence":b::ordered(docs.iter().map(|r|b::reference(r)).collect()).unwrap()}),
            authentication: proof_ref("authentication"),
            verified_terms: docs
                .iter()
                .map(|r| r["body"]["document_id"].as_str().unwrap().into())
                .collect(),
            finality: true,
            authorized_early_close: true,
        };
        let resolve = OutcomeResolve {
            delivery: ScopedDelivery {
                scope: ["synthetic".into(), "sandbox".into()],
                source: command.principal.source.clone(),
                external_id: event["body"]["data"]["external_id"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            target: command.target.clone(),
            invocation_id: command.invocation_id.clone(),
            family_key: None,
            required: command.required.clone(),
            locks,
        };
        let evidence = docs.iter().map(|r| b::bytes(r).unwrap()).collect();
        Self {
            command,
            resolve,
            heads,
            evidence,
            objects,
            proof,
        }
    }
    pub async fn prepare(
        &self,
        tx: &mut crate::store::sqlite::SqliteTx,
        terms: &wire::Enroll,
        inputs: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        let snapshot = match tx
            .resolve_outcome(&self.resolve)
            .await
            .map_err(crate::service::store_error)?
        {
            OutcomeResolution::Complete(s) => s,
            other => {
                eprintln!("original resolution {other:?}");
                return Err(ServiceError::IntegrityFailure);
            }
        };
        let existing = tx
            .lookup_outcome_delivery(&self.resolve.delivery)
            .await
            .map_err(crate::service::store_error)?;
        let plan =
            outcome::prepare_original_base(&self.command, self, &snapshot, existing.as_ref())?;
        let mut objects = self.objects.clone();
        for object in &mut objects {
            object.origin.ordinal = inputs
                .prefix
                .ordinal()
                .checked_add(Count::new(1).unwrap())
                .unwrap();
        }
        FreshBaseAcceptance::from_fresh_v2(plan, terms, objects)
            .map_err(|e| ServiceError::Rejection(e.code.into()))
    }
}
impl outcome::OutcomeAuthority for BaseFixture {
    fn verify(
        &self,
        command: &outcome::OutcomeCommand,
        snapshot: &OutcomeSnapshot,
        write: bool,
    ) -> Result<outcome::AuthorityProof, ServiceError> {
        // Configured synthetic trust: full exact evidence and current authority/
        // binding observations must be returned by the live locked transaction.
        if !write
            || command.target != self.command.target
            || command.principal.principal_id != self.command.principal.principal_id
            || self.evidence.iter().any(|r| !snapshot.records.contains(r))
            || self.heads.iter().any(|h| !snapshot.heads.contains(h))
        {
            return Err(ServiceError::Rejection("HOST_AUTHORITY".into()));
        }
        Ok(self.proof.clone())
    }
}

impl BaseFixture {
    pub async fn assert_atomic_state(
        &self,
        store: &crate::store::sqlite::SqliteStore,
        accepted: bool,
    ) {
        use crate::store::ports::AcceptanceTx;
        let mut tx = store
            .begin_outcome(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
            .unwrap();
        tx.lock_scopes(&self.resolve.locks).await.unwrap();
        let OutcomeResolution::Complete(snapshot) =
            tx.resolve_outcome(&self.resolve).await.unwrap()
        else {
            panic!("original snapshot")
        };
        if accepted {
            assert!(matches!(
                tx.lookup_outcome_delivery(&self.resolve.delivery).await,
                Err(crate::store::errors::StoreError::DeliveryConflict)
            ));
            assert_eq!(snapshot.anchors.len(), 1);
            assert_eq!(snapshot.records.len(), self.objects.len());
            for object in &self.objects {
                assert!(snapshot.records.contains(
                    &r3::proofs::decode_base64(&object.body, r3::COMMAND_BYTES).unwrap()
                ));
            }
            // Even when the caller supplies no previous reply, the locked target
            // and complete original membership cannot be enrolled a second time.
            assert!(matches!(
                outcome::prepare_original_base(&self.command, self, &snapshot, None),
                Err(ServiceError::IntegrityFailure)
            ));
        } else {
            assert!(snapshot.anchors.is_empty());
            assert_eq!(snapshot.records.len(), self.evidence.len());
            for head in &snapshot.heads {
                if !matches!(
                    head.lock.class,
                    OutcomeLockClass::Authority
                        | OutcomeLockClass::Binding
                        | OutcomeLockClass::Admission
                ) {
                    assert!(head.revision.is_none() && head.value.is_none());
                }
            }
            assert!(tx
                .lookup_outcome_delivery(&self.resolve.delivery)
                .await
                .unwrap()
                .is_none());
        }
        tx.rollback().await.unwrap();
    }
}
