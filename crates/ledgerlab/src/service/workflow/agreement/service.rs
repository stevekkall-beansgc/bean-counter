//! Trusted local prerequisite entry. No request can supply an authority snapshot.
use super::*;
use crate::{
    service::workflow::local::{LocalWorkflowLedger, deadline},
    store::ports::{AcceptanceStore, AcceptanceTx},
};
use serde::{Deserialize, Serialize};

#[cfg(test)]
static LOST_REPLY_KEY: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(super) fn inject_committed_reply_loss(key: Vec<u8>) {
    let mut selected = LOST_REPLY_KEY.lock().unwrap();
    assert!(selected.is_none());
    *selected = Some(key);
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    schema: String,
    action: String,
    expected_revision: String,
    customer: String,
    acceptor: String,
    registration_evidence_ref: RecordRef,
}
fn normalized(req: &request::Request) -> Result<Value> {
    match req.kind() {
        "evidence.retain" => {
            ledgerlab_core::policy::workflow::EvidenceInput::parse(&records::bytes(req.payload())?)
                .map_err(typed_error)?
                .canonical_value()
                .map_err(typed_error)
        }
        "authority.change" if req.payload()["schema"] == "ledger-workflow-customer-register/1" => {
            let r: Registration = serde_json::from_value(req.payload().clone())
                .map_err(|_| reject("INVALID_SHAPE"))?;
            require(
                r.action == "customer.register"
                    && r.expected_revision == "0"
                    && valid_id(&r.customer)
                    && valid_id(&r.acceptor)
                    && valid_evidence_ref(&r.registration_evidence_ref),
                "INVALID_SHAPE",
            )?;
            serde_json::to_value(r).map_err(|_| ServiceError::IntegrityFailure)
        }
        "authority.change" => source::normalize(req.payload()),
        _ => Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
    }
}
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
fn valid_evidence_ref(r: &RecordRef) -> bool {
    r.kind == "workflow-evidence"
        && super::super::request::valid_digest(&r.digest)
        && r.id == format!("wdoc_{}", &r.digest[7..])
}

impl LocalWorkflowLedger {
    /// Retain explicit proof material or register its declared local customer.
    /// The private owner capability supplies administration, never assent.
    pub async fn prerequisite(&self, raw: &[u8]) -> Result<Value> {
        let req = request::Request::parse(raw)?;
        if transport::selected(req.payload()) {
            return transport::service::execute(self, req).await;
        }
        let key = req.key();
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(crate::service::store_error)?;
        let written:Result<(Value,bool)>=async {
            self.admin.read(&self.store,&mut tx,key).await?;
            if let Some(kind)=tx.workflow_operation_kind(&key.bytes()?).await.map_err(crate::service::store_error)? {
                require(kind==req.kind(),"IDENTITY_CONFLICT")?;
                let old=tx.workflow_agreement_lookup(&key.bytes()?).await.map_err(crate::service::store_error)?.ok_or(ServiceError::IntegrityFailure)?;
                return Ok((retry(&mut tx,&req,&old).await?,false));
            }
            let payload=normalized(&req)?;
            let setup=self.admin.fresh(&self.store,&mut tx,key).await?;
            let installation=tx.load_installation().await.map_err(crate::service::store_error)?;
            require(installation.mode=="real","INSTALLATION_MODE_MISMATCH")?;
            let mode="real_local";
            let at=crate::local::now().map_err(|_|ServiceError::Unavailable)?;
            let root=match tx.workflow_agreement_root_candidate(&key.logical_store_id,&key.scope).await.map_err(crate::service::store_error)? {
                Some(d)=>{verify::document_origin(&mut tx,key,&d.reference).await?;let body=canonical_body(&d)?;verify::root(&body,key,None)?;if body["basis"]==journal::basis(&setup){d}else{journal::fresh_root(&setup,&at)?}},
                None=>journal::fresh_root(&setup,&at)?,
            };
            let sequence=tx.workflow_sequence().await.map_err(crate::service::store_error)?;
            let semantic_key=if req.kind()=="evidence.retain" {evidence_identity(key,&ledgerlab_core::policy::workflow::EvidenceInput::parse(&records::bytes(&payload)?).map_err(typed_error)?)?} else if payload["action"]=="customer.register" {records::bytes(&json!(["customer.register",payload["customer"]]))?} else if payload["action"]=="source.enroll" {source::semantic(&payload,text(&payload,"enrollment_id")?)?} else {let r:RecordRef=serde_json::from_value(payload["enrollment_ref"].clone()).map_err(|_|ServiceError::IntegrityFailure)?;let e=source::enrollment(&mut tx,key,&r).await?;source::semantic(&payload,e.enrollment_id())?};
            let alias=tx.workflow_agreement_semantic(&key.logical_store_id,&key.scope,req.kind(),&semantic_key).await.map_err(crate::service::store_error)?;
            let (documents,results,projection)=if let Some(identity)=&alias {
                let original=tx.workflow_agreement_lookup(identity).await.map_err(crate::service::store_error)?.ok_or(ServiceError::IntegrityFailure)?;
                let original_key=super::super::request::OperationKey::parse(&original.identity)?;
                require(original_key.logical_store_id==key.logical_store_id && original_key.scope==key.scope,"AUTHORITY_REQUIRED")?;
                let original_result=verify::retained(&mut tx,&original_key,&original).await?;
                require(original_result["request_evidence"]["normalized_request"]["payload"]==payload,"SEMANTIC_CONFLICT")?;
                let results:Vec<RecordRef>=serde_json::from_value(original_result["receipt"]["result_refs"].clone()).map_err(|_|ServiceError::IntegrityFailure)?;
                let documents=verify::authenticated_closure(&mut tx,key,&results).await?;
                (documents,results,Projection::None)
            }else if req.kind()=="evidence.retain" {
                let input=ledgerlab_core::policy::workflow::EvidenceInput::parse(&records::bytes(&payload)?).map_err(typed_error)?;
                validate_evidence_dependencies(&mut tx,key,&input,&setup.operator,mode).await?;
                let result=evidence_record(&input,&setup.operator,&at,mode,&root.reference)?;let refs=vec![result.reference.clone()];let mut docs=vec![result];docs.extend(verify::locator_dependencies(&mut tx,key,&req,&payload).await?);(docs,refs,Projection::None)
            }else if payload["action"]=="customer.register" {
                let registration:Registration=serde_json::from_value(payload.clone()).map_err(|_|ServiceError::IntegrityFailure)?;
                require(tx.workflow_agreement_customer(&key.logical_store_id,&key.scope,&registration.customer).await.map_err(crate::service::store_error)?.is_none(),"SEMANTIC_CONFLICT")?;
                verify::document_origin(&mut tx,key,&registration.registration_evidence_ref).await?;let proof=loaded(&mut tx,key,&registration.registration_evidence_ref).await?;let body=canonical_body(&proof)?;let input=evidence_input(&body)?;
                require(input.purpose()=="customer_registration" && input.customer()==registration.customer && input.metadata()["acceptor"]==registration.acceptor,"AUTHORITY_REQUIRED")?;require(body["installation_mode"]==mode,"INSTALLATION_MODE_MISMATCH")?;
                let doc=records::source_document("workflow-customer",json!({"schema":"ledger-workflow-customer-registration/1","scope":key.scope,"customer":registration.customer,"acceptor":registration.acceptor,"evidence_ref":registration.registration_evidence_ref,"registered_by":setup.operator,"registered_at":at,"authority_ref":root.reference,"installation_mode":mode}))?;
                let projection=Projection::Customer{customer:registration.customer,document:doc.reference.id.clone()};let refs=vec![doc.reference.clone()];let mut docs=vec![doc];docs.extend(verify::authenticated_closure(&mut tx,key,&[proof.reference]).await?);(docs,refs,projection)
            }else {let prepared=source::prepare(&mut tx,key,&payload,&setup,&at,mode,&root.reference).await?;(prepared.documents,prepared.results,prepared.projection)};
            let evidence=journal::evidence(&req,&payload)?;
            let append=journal::assemble(&req,evidence,&setup,&at,sequence,root,documents,results,semantic_key,alias,projection)?;
            tx.workflow_agreement_append(&append).await.map_err(|e|if matches!(e,crate::store::errors::StoreError::DeliveryConflict){reject("SEMANTIC_CONFLICT")}else{crate::service::store_error(e)})?;
            Ok((verify::retained(&mut tx,key,&append.operation).await?,true))
        }.await;
        match written {
            Err(e) => {
                tx.rollback().await.map_err(crate::service::store_error)?;
                Err(e)
            }
            Ok((result, false)) => {
                tx.rollback().await.map_err(crate::service::store_error)?;
                Ok(result_envelope(result, Some("identity")))
            }
            Ok((result, true)) => {
                tx.commit().await.map_err(|e| match e {
                    crate::store::errors::CommitError::RolledBack(e) => {
                        crate::service::store_error(e)
                    }
                    crate::store::errors::CommitError::OutcomeUnknown => {
                        ServiceError::OutcomeUnknown {
                            scope: key.scope.clone(),
                            source: key.source.clone(),
                            external_id: key.operation_id.clone(),
                        }
                    }
                })?;
                #[cfg(test)]
                {
                    let mut selected = LOST_REPLY_KEY.lock().unwrap();
                    if selected.as_ref() == Some(&key.bytes()?) {
                        *selected = None;
                        return Err(ServiceError::ResponseLost);
                    }
                }
                let alias = result["receipt"]["result_kind"] == "semantic_alias";
                Ok(result_envelope(
                    result,
                    if alias { Some("semantic") } else { None },
                ))
            }
        }
    }
}
fn result_envelope(mut v: Value, duplicate: Option<&str>) -> Value {
    v["schema"] = json!("ledger-operation-result/1");
    if let Some(d) = duplicate {
        v["duplicate_kind"] = json!(d);
    }
    v
}
async fn retry(tx: &mut SqliteTx, req: &request::Request, old: &JournalOperation) -> Result<Value> {
    let result = verify::retained(tx, req.key(), old).await?;
    let saved = &result["request_evidence"];
    if saved["original_request_bytes_sha256"] != records::raw_hash(req.raw()) {
        let attempted = journal::evidence(req, &normalized(req)?)?;
        require(
            attempted["request_hash"] == saved["request_hash"]
                && attempted["descriptor_resolution"] == saved["descriptor_resolution"],
            "IDENTITY_CONFLICT",
        )?;
    }
    Ok(result)
}
async fn validate_evidence_dependencies(
    tx: &mut SqliteTx,
    key: &super::super::request::OperationKey,
    input: &ledgerlab_core::policy::workflow::EvidenceInput,
    actor: &str,
    mode: &str,
) -> Result<()> {
    if input.purpose() == "operator_attestation" {
        let r: RecordRef = serde_json::from_value(input.metadata()["assent_evidence_ref"].clone())
            .map_err(|_| reject("INVALID_REFERENCE"))?;
        verify::document_origin(tx, key, &r).await?;
        let prior = loaded(tx, key, &r).await?;
        let body = canonical_body(&prior)?;
        let assent = evidence_input(&body)?;
        require(
            assent.purpose() == "assent"
                && assent.customer() == input.customer()
                && assent.agreement_id() == input.agreement_id()
                && assent.metadata()["terms_digest"] == input.metadata()["terms_digest"]
                && assent.metadata()["acceptor"] == input.metadata()["acceptor"]
                && body["retained_by"] == actor,
            "ASSENT_TERMS_MISMATCH",
        )?;
        require(
            body["installation_mode"] == mode,
            "INSTALLATION_MODE_MISMATCH",
        )?;
    } else if input.purpose() == "admission_stop" {
        return Err(reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    Ok(())
}
pub(super) fn normalize_payload(req: &request::Request) -> Result<Value> {
    normalized(req)
}
