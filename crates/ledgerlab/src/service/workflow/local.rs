//! Trusted local-process entry. Remote/source actors cannot mint this capability.
use super::agreement::proof_session;
use super::*;
use crate::{
    local as local_io,
    service::billing,
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::{SqliteStore, SqliteTx},
    },
};
use serde_json::json;
use std::{path::Path, time::Duration};
use tokio::time::Instant;

type PublicationPreparation = (Option<Value>, Option<Vec<Vec<u8>>>);

/// Private fields and no Deserialize, request/config flag or snapshot constructor.
pub(super) struct LocalOwnerAdmin {
    instance: SqliteStore,
    logical_store_id: String,
    scope: [String; 2],
    operator: String,
}
#[cfg(test)]
#[derive(Clone)]
struct TestPreparationGate {
    barrier: std::sync::Arc<tokio::sync::Barrier>,
    wait_for_commit: Option<(String, std::sync::Arc<tokio::sync::Notify>)>,
}
/// Local-only installation access. No remote source token is accepted.
pub struct LocalWorkflowLedger {
    pub(super) store: SqliteStore,
    pub(super) admin: LocalOwnerAdmin,
    #[cfg(test)]
    lose_reply: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    preparation_gate: std::sync::Mutex<Option<TestPreparationGate>>,
    #[cfg(test)]
    pub(super) preferred_root: std::sync::Mutex<Option<crate::store::sqlite::workflow::RecordRef>>,
}
impl LocalWorkflowLedger {
    /// Publish through the journal using only the implemented builtin compiler
    /// and code-owned captured build inputs. Publishing obtains no assent.
    pub async fn publish(&self, raw: &[u8]) -> Result<Value> {
        if request::PublicationRequest::parse(raw)?.payload_schema()=="ledger-workflow-policy-publication/6" {
            return self.publish_local_owned(raw).await.map(agreement::WorkflowReply::into_legacy_value);
        }
        self.publish_with(raw, || {
            ledgerlab_core::policy::workflow::VerifiedCompilerSupport::builtin()
                .map_err(|_| reject("UNSUPPORTED_SCHEMA_PROFILE"))
        })
        .await
    }
    pub async fn open_local(path: &Path) -> Result<Self> {
        let path = local_io::normalize_path(path).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        local_io::private_existing(&path, true).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        let raw = local_io::read_file(&path.join("billing.json"), local_io::CONFIG_LIMIT)
            .map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        let config = ledgerlab_core::canonical::parse_bounded(&raw, 65536)
            .map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        require(
            config.as_object().is_some_and(|o| {
                o.len() == 3
                    && o.contains_key("schema")
                    && o.contains_key("store_id")
                    && o.contains_key("scope")
            }) && config["schema"] == "ledger-billing-installation/1",
            "AUTHORITY_REQUIRED",
        )?;
        let directory = path.join(".ledger");
        local_io::private_existing(&directory, true).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        local_io::private_existing(&directory.join("local.db"), false)
            .map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        let store = SqliteStore::open(&directory)
            .await
            .map_err(super::super::store_error)?;
        let result = async {
            let mut tx = store
                .begin(deadline())
                .await
                .map_err(super::super::store_error)?;
            let setup = setup_identity(&mut tx).await?;
            require(
                config["store_id"] == setup.store_id && config["scope"] == json!(setup.scope),
                "AUTHORITY_REQUIRED",
            )?;
            tx.rollback().await.map_err(super::super::store_error)?;
            Ok(LocalOwnerAdmin {
                instance: store.clone(),
                logical_store_id: setup.store_id,
                scope: [
                    setup.scope.tenant().into(),
                    setup.scope.environment().into(),
                ],
                operator: setup.operator,
            })
        }
        .await;
        match result {
            Ok(admin) => Ok(Self {
                store,
                admin,
                #[cfg(test)]
                lose_reply: std::sync::atomic::AtomicBool::new(false),
                #[cfg(test)]
                preparation_gate: std::sync::Mutex::new(None),
                #[cfg(test)]
                preferred_root: std::sync::Mutex::new(None),
            }),
            Err(e) => {
                store.close().await;
                Err(e)
            }
        }
    }
    /// Read an exact retained publication member in this verified owner scope.
    pub async fn read(&self, raw: &[u8]) -> Result<Value> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Read {
            schema: String,
            kind: String,
            payload: Payload,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Payload {
            r#ref: crate::store::sqlite::workflow::RecordRef,
        }
        let v = ledgerlab_core::canonical::parse_bounded(raw, 262_144)
            .map_err(|_| reject("INVALID_SHAPE"))?;
        if v["kind"] == "operation.descriptors"
            || v.get("payload")
                .and_then(|p| p.get("representation"))
                .is_some()
        {
            return agreement::transport::read(self, raw).await;
        }
        let read: Read = serde_json::from_value(v).map_err(|_| reject("INVALID_SHAPE"))?;
        require(
            read.schema == "ledger-read/1" && read.kind == "policy.document",
            "INVALID_SHAPE",
        )?;
        let r = read.payload.r#ref;
        require(
            matches!(
                r.kind.as_str(),
                "workflow-product"
                    | "workflow-fact-schema"
                    | "workflow-slot-template"
                    | "workflow-policy"
                    | "workflow-compiler-descriptor"
                    | "workflow-compiled-plan"
                    | "workflow-publication"
                    | "workflow-authority"
                    | "workflow-build-manifest"
            ),
            "INVALID_SHAPE",
        )?;
        let key = request::OperationKey {
            logical_store_id: self.admin.logical_store_id.clone(),
            scope: self.admin.scope.clone(),
            source: "urn:internal:workflow-read".into(),
            operation_id: "workflow-read".into(),
        };
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(super::super::store_error)?;
        let result = proof_session::with_proofs(&mut tx, &key, |tx| Box::pin(async {
            self.admin.read(&self.store, tx, &key).await?;
            let d = tx.workflow_document(&r,&key.logical_store_id,&key.scope[0],&key.scope[1],None).await.map_err(super::super::store_error)?.ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
            let body = ledgerlab_core::canonical::parse_bounded(&d.bytes, 4*1024*1024).map_err(|_| ServiceError::IntegrityFailure)?;
            if records::bytes(&body)? != d.bytes || body["schema"] != d.schema { return Err(ServiceError::IntegrityFailure); }
            let derived = match r.kind.as_str() {
                "workflow-publication" => records::publication_ref(&key.logical_store_id,&key.scope,&body)?,
                "workflow-compiled-plan" => { let digest=records::hash("workflow-plan",1,&body)?; crate::store::sqlite::workflow::RecordRef {kind:r.kind.clone(),id:format!("wplan_{}",&digest[7..]),digest} },
                _ => records::source_document(&r.kind,body.clone())?.reference,
            };
            if derived != r { return Err(ServiceError::IntegrityFailure); }
            Ok(json!({"schema":"ledger-document-read/1","record":{"kind":r.kind,"logical_store_id":key.logical_store_id,"scope":key.scope,"id":r.id,"digest":r.digest,"body":body},"canonical_bytes_base64":publication::base64(&d.bytes),"canonical_digest":r.digest}))
        })).await;
        tx.rollback().await.map_err(super::super::store_error)?;
        result
    }
    pub async fn status(&self, raw_key: &[u8]) -> Result<Value> {
        let key = request::OperationKey::parse(raw_key)?;
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(super::super::store_error)?;
        let result = proof_session::with_proofs(&mut tx, &key, |tx| Box::pin(async {
            self.admin.read(&self.store, tx, &key).await?;
            match tx
                .workflow_operation_kind(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                Some(kind) if kind == "policy.publish" => {
                    if let Some(original)=agreement::financial::local_management_status(tx,&key,raw_key).await? {
                        return Ok(original);
                    }
                    let original = tx
                        .workflow_lookup(&key.bytes()?)
                        .await
                        .map_err(super::super::store_error)?
                        .ok_or(ServiceError::IntegrityFailure)?;
                    records::verify_retained(tx, &key, &original).await
                }
                Some(kind) if agreement::financial::is_kind(&kind) => {
                    agreement::financial::original_status(self, tx, &key, raw_key).await
                }
                Some(_) => {
                    if let Some(original)=agreement::financial::local_management_status(tx,&key,raw_key).await? {
                        Ok(original)
                    } else { agreement::status(tx,&key).await }
                },
                None => Ok(
                    json!({"schema":"ledger-operation-status/1","status":"not_observed","key":key}),
                ),
            }
        }))
        .await;
        tx.rollback().await.map_err(super::super::store_error)?;
        result
    }
    /// Explicit trusted-local opt-in. Whole legacy and workflow histories are
    /// validated under the same ordered transaction before their schema steps.
    pub async fn enable_profile_b(&self) -> Result<()> {
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(super::super::store_error)?;
        let key = request::OperationKey {
            logical_store_id: self.admin.logical_store_id.clone(),
            scope: self.admin.scope.clone(),
            source: "urn:internal:workflow-enable".into(),
            operation_id: "workflow-enable".into(),
        };
        let prepared = async {
            self.admin.read(&self.store, &mut tx, &key).await?;
            let snapshot = tx
                .billing_snapshot()
                .await
                .map_err(super::super::store_error)?;
            billing::validate_snapshot(&snapshot)?;
            tx.workflow_enable_profile_b()
                .await
                .map_err(super::super::store_error)?;
            agreement::financial::enable_app_schemas(self, &mut tx, &key).await
        }
        .await;
        if let Err(e) = prepared {
            tx.rollback().await.map_err(super::super::store_error)?;
            return Err(e);
        }
        tx.commit().await.map_err(|e| match e {
            crate::store::errors::CommitError::RolledBack(e) => super::super::store_error(e),
            crate::store::errors::CommitError::OutcomeUnknown => ServiceError::OutcomeUnknown {
                scope: key.scope,
                source: key.source,
                external_id: key.operation_id,
            },
        })
    }
    pub async fn close(self) {
        let Self { store, admin, .. } = self;
        drop(admin);
        store.close().await;
    }

    /// Internal trusted-support seam. The factory is invoked only for an absent
    /// original key; no public caller supplies a compiled plan or support token.
    pub(super) async fn publish_with<F>(&self, raw: &[u8], fresh_support: F) -> Result<Value>
    where
        F: FnOnce() -> Result<ledgerlab_core::policy::workflow::VerifiedCompilerSupport>,
    {
        use ledgerlab_core::policy::workflow as core;
        let req = request::PublicationRequest::parse(raw)?;
        let key = req.key();
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(super::super::store_error)?;
        let preparation: Result<PublicationPreparation> = proof_session::with_proofs(&mut tx, key, |tx| Box::pin(async {
            self.admin.read(&self.store, tx, key).await?;
            if let Some(kind) = tx
                .workflow_operation_kind(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                require(kind == "policy.publish", "IDENTITY_CONFLICT")?;
            }
            if let Some(original) = tx
                .workflow_lookup(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                return Ok((Some(retry(tx, &req, &original).await?), None));
            }
            self.admin.fresh(&self.store, tx, key).await?;
            Ok((None, Some(resolve_documents(tx, &req).await?)))
        }))
        .await;
        tx.rollback().await.map_err(super::super::store_error)?;
        let (original, raw_documents) = preparation?;
        if let Some(result) = original {
            return Ok(operation_result(result, Some("identity")));
        }
        let raw_documents = raw_documents.ok_or(ServiceError::IntegrityFailure)?;
        #[cfg(test)]
        {
            let gate = { self.preparation_gate.lock().unwrap().clone() };
            if let Some(gate) = gate {
                eprintln!(
                    "workflow race: {} original absent; initial rollback returned; async gate entered",
                    key.operation_id
                );
                tokio::time::timeout(Duration::from_secs(10), gate.barrier.wait())
                    .await
                    .map_err(|_| ServiceError::Injected)?;
                if let Some((digest, committed)) = gate.wait_for_commit {
                    if req.expected_digest() == digest {
                        tokio::time::timeout(Duration::from_secs(10), committed.notified())
                            .await
                            .map_err(|_| ServiceError::Injected)?;
                    }
                }
                eprintln!(
                    "workflow race: {} async preparation gate released",
                    key.operation_id
                );
            }
        }

        let fresh_prepared: Result<(
            core::PreparedPublication,
            Value,
            publication::BuildProvenance,
        )> = (|| {
            let sources = publication::normalized(&req, &raw_documents)?;
            let evidence = publication::evidence(&req, &sources)?;
            let supported = fresh_support()?;
            let root =
                core::DefinitionRef::new(&req.root().kind, &req.root().id, &req.root().digest)
                    .map_err(core_error)?;
            let prepared =
                core::compile_publication(&raw_documents, &root, req.expected_digest(), &supported)
                    .map_err(core_error)?;
            let provenance =
                publication::BuildProvenance::capture(supported.descriptor().reference())?;
            Ok((prepared, evidence, provenance))
        })();
        let mut tx = self
            .store
            .begin(deadline())
            .await
            .map_err(super::super::store_error)?;
        let written: Result<(Value, bool)> = proof_session::with_proofs(&mut tx, key, |tx| Box::pin(async {
            self.admin.read(&self.store, tx, key).await?;
            if let Some(kind) = tx
                .workflow_operation_kind(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                require(kind == "policy.publish", "IDENTITY_CONFLICT")?;
            }
            // A racing commit wins before mutable fresh configuration/support.
            if let Some(original) = tx
                .workflow_lookup(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                return Ok((retry(tx, &req, &original).await?, false));
            }
            let (prepared, evidence, provenance) = fresh_prepared?;
            let setup = self.admin.fresh(&self.store, tx, key).await?;
            if req.retained_refs().is_some() {
                let locked = resolve_documents(tx, &req).await?;
                require(locked == raw_documents, "SEMANTIC_CONFLICT")?;
            }
            // A supplied predecessor is only a normalization input. Its exact
            // validated bytes must already belong to a publication in this scope.
            for template in prepared
                .source_documents()
                .iter()
                .filter(|d| d.kind() == "workflow-slot-template")
            {
                if let Some(previous) = template.canonical_value().get("previous") {
                    let r: crate::store::sqlite::workflow::RecordRef =
                        serde_json::from_value(previous.clone())
                            .map_err(|_| reject("INVALID_SHAPE"))?;
                    let retained = tx
                        .workflow_document(
                            &r,
                            &key.logical_store_id,
                            &key.scope[0],
                            &key.scope[1],
                            None,
                        )
                        .await
                        .map_err(super::super::store_error)?
                        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
                    let supplied = prepared
                        .source_documents()
                        .iter()
                        .find(|d| d.id() == r.id)
                        .ok_or(ServiceError::IntegrityFailure)?;
                    if retained.bytes != supplied.canonical_bytes()
                        || json!(supplied.reference()) != json!(r)
                    {
                        return Err(ServiceError::IntegrityFailure);
                    }
                }
            }
            let sequence = tx
                .workflow_sequence()
                .await
                .map_err(super::super::store_error)?;
            let pub_hash = records::hash(
                "workflow-publication",
                1,
                &json!([key.logical_store_id, key.scope, req.root()]),
            )?;
            let pub_id = format!("wpub_{}", &pub_hash[7..]);
            let existing = if tx
                .workflow_publication(pub_id.as_bytes())
                .await
                .map_err(super::super::store_error)?
                .is_some()
            {
                let publication = tx
                    .workflow_document_by_id(
                        &pub_id,
                        &key.logical_store_id,
                        &key.scope[0],
                        &key.scope[1],
                    )
                    .await
                    .map_err(super::super::store_error)?
                    .ok_or(ServiceError::IntegrityFailure)?;
                let build_ref = tx
                    .workflow_publication_build_ref(
                        &pub_id,
                        &key.logical_store_id,
                        &key.scope[0],
                        &key.scope[1],
                    )
                    .await
                    .map_err(super::super::store_error)?;
                let build = if let Some(r) = build_ref {
                    Some(
                        tx.workflow_document(
                            &r,
                            &key.logical_store_id,
                            &key.scope[0],
                            &key.scope[1],
                            None,
                        )
                        .await
                        .map_err(super::super::store_error)?
                        .ok_or(ServiceError::IntegrityFailure)?,
                    )
                } else {
                    None
                };
                Some((publication, build))
            } else {
                None
            };
            let at = local_io::now().map_err(|_| ServiceError::Unavailable)?;
            let append = publication::assemble(
                &req,
                &prepared,
                evidence,
                &setup,
                &at,
                sequence,
                &provenance,
                existing,
            )?;
            tx.workflow_append(&append).await.map_err(|e| {
                if matches!(e, crate::store::errors::StoreError::DeliveryConflict) {
                    reject("SEMANTIC_CONFLICT")
                } else {
                    super::super::store_error(e)
                }
            })?;
            proof_session::after_append(tx, key)?;
            let result = records::verify_retained(tx, key, &append.operation).await?;
            Ok((result, true))
        }))
        .await;
        match written {
            Err(e) => {
                tx.rollback().await.map_err(super::super::store_error)?;
                Err(e)
            }
            Ok((result, false)) => {
                tx.rollback().await.map_err(super::super::store_error)?;
                Ok(operation_result(result, Some("identity")))
            }
            Ok((result, true)) => {
                match tx.commit().await {
                    Ok(()) => {}
                    Err(crate::store::errors::CommitError::RolledBack(e)) => {
                        return Err(super::super::store_error(e));
                    }
                    Err(crate::store::errors::CommitError::OutcomeUnknown) => {
                        return Err(ServiceError::OutcomeUnknown {
                            scope: key.scope.clone(),
                            source: key.source.clone(),
                            external_id: key.operation_id.clone(),
                        });
                    }
                }
                #[cfg(test)]
                if self
                    .lose_reply
                    .swap(false, std::sync::atomic::Ordering::AcqRel)
                {
                    return Err(ServiceError::ResponseLost);
                }
                let alias = result["receipt"]["result_kind"] == "semantic_alias";
                Ok(operation_result(
                    result,
                    if alias { Some("semantic") } else { None },
                ))
            }
        }
    }
}

fn operation_result(mut result: Value, duplicate: Option<&str>) -> Value {
    result["schema"] = json!("ledger-operation-result/1");
    if let Some(kind) = duplicate {
        result["duplicate_kind"] = json!(kind);
    }
    result
}

async fn resolve_documents(
    tx: &mut SqliteTx,
    req: &request::PublicationRequest,
) -> Result<Vec<Vec<u8>>> {
    if let Some(documents) = req.inline_documents() {
        return documents.iter().map(records::bytes).collect();
    }
    let key = req.key();
    let mut seen = std::collections::BTreeSet::new();
    let mut docs = vec![];
    for r in req.retained_refs().ok_or(ServiceError::IntegrityFailure)? {
        require(seen.insert(&r.id), "INVALID_SHAPE")?;
        let d = tx
            .workflow_document(r, &key.logical_store_id, &key.scope[0], &key.scope[1], None)
            .await
            .map_err(super::super::store_error)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        // Scoped bytes alone do not prove a source's retained origin. Resolve
        // the complete saved certificate before fresh compilation/publication.
        agreement::transport::verify_source_origin(tx, key, r).await?;
        docs.push(d.bytes);
    }
    Ok(docs)
}
async fn retry(
    tx: &mut SqliteTx,
    req: &request::PublicationRequest,
    original: &crate::store::sqlite::workflow::StoredOperation,
) -> Result<Value> {
    let result = records::verify_retained(tx, req.key(), original).await?;
    let evidence = &result["request_evidence"];
    if records::raw_hash(req.raw()) == evidence["original_request_bytes_sha256"] {
        return Ok(result);
    }
    let raw_documents = resolve_documents(tx, req).await?;
    let sources = publication::normalized(req, &raw_documents)?;
    let attempted = publication::evidence(req, &sources)?;
    require(
        attempted["request_hash"] == evidence["request_hash"]
            && attempted["descriptor_resolution"] == evidence["descriptor_resolution"],
        "IDENTITY_CONFLICT",
    )?;
    Ok(result)
}
pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
async fn setup_identity(tx: &mut SqliteTx) -> Result<billing::Setup> {
    let snapshot = tx.billing_meta().await.map_err(super::super::store_error)?;
    let setup = billing::permissions::effective(&snapshot)?;
    let installation = tx
        .load_installation()
        .await
        .map_err(super::super::store_error)?;
    require(
        installation.logical_store_id == setup.store_id
            && installation.scope.tenant == setup.scope.tenant()
            && installation.scope.environment == setup.scope.environment()
            && installation.mode == "real",
        "AUTHORITY_REQUIRED",
    )?;
    Ok(setup)
}
/// Private actual held-snapshot DATA. Neither this owning observation nor its
/// normalized fields are a prepared append capability.
pub(super) struct OwnedLocalManagement {
    key: request::OperationKey,
    owner: crate::store::sqlite::WorkflowProofFailureHandle,
    changes: i64,
    setup: crate::service::billing::setup_owned::OwnedSetup,
    installation: crate::store::sqlite::workflow::financial::ManagementInstallation,
    _key_storage: ledgerlab_core::canonical::audit_arena::ExternalBufferLease,
    _control: ledgerlab_core::canonical::audit_arena::ExternalBufferLease,
}
impl OwnedLocalManagement {
    pub(super) fn setup(&self)->&billing::Setup { self.setup.setup() }
    pub(super) fn installation(&self)->&crate::store::records::Installation { self.installation.installation() }
}
impl LocalOwnerAdmin {
    /// Same local ownership and current setup predicates, with actual bounded
    /// metadata/typed outputs retained through their consuming preparation.
    /// The enclosing path/fence/commit envelope remains separately required.
    pub(super) async fn read_management(
        &self, store:&SqliteStore, tx:&mut SqliteTx, key:&request::OperationKey,
        s:&mut ledgerlab_core::validation_work::OriginalAuditSession,
    )->Result<OwnedLocalManagement> {
        use ledgerlab_core::validation_work::ValidationVisit as Visit;
        fn checked(s:&mut ledgerlab_core::validation_work::OriginalAuditSession,condition:impl FnOnce()->bool)->Result<()> {
            s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
            require(condition(),"AUTHORITY_REQUIRED")
        }
        fn same(a:&str,b:&str,s:&mut ledgerlab_core::validation_work::OriginalAuditSession)->Result<bool> {
            s.semantic_work().visit(Visit::MemberCompare).map_err(core_error)?;
            s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
            s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
            Ok(a==b)
        }
        let control=s.reserve_external_payload(std::mem::size_of::<OwnedLocalManagement>()).map_err(core_error)?;
        s.semantic_work().write_bytes(std::mem::size_of::<OwnedLocalManagement>()).map_err(core_error)?;
        checked(s,||self.instance.workflow_same_instance(store))?;
        self.management_paths(store,s)?;
        let setup=agreement::financial::effective_setup(tx,s).await?;
        let body=setup.setup();
        let installation=tx.workflow_management_installation(&body.store_id,body.scope.tenant(),body.scope.environment(),s).await
            .map_err(agreement::financial::management_read_error)?.ok_or_else(||reject("AUTHORITY_REQUIRED"))?;
        let mode=same(&installation.installation().mode,"real",s)?;checked(s,||mode)?;
        for (a,b) in [(&self.logical_store_id,body.store_id.as_str()),(&self.scope[0],body.scope.tenant()),(&self.scope[1],body.scope.environment()),(&self.operator,body.operator.as_str()),(&key.logical_store_id,self.logical_store_id.as_str()),(&key.scope[0],self.scope[0].as_str()),(&key.scope[1],self.scope[1].as_str())] {
            let equal=same(a,b,s)?;checked(s,||equal)?;
        }
        let mut key_bytes=0usize;
        for text in [&key.logical_store_id,&key.scope[0],&key.scope[1],&key.source,&key.operation_id] {
            s.semantic_work().arithmetic(ledgerlab_core::validation_work::ArithmeticOperation::Add,&[&key_bytes.into(),&text.len().into()]).map_err(core_error)?;
            key_bytes=key_bytes.checked_add(text.len()).ok_or(ServiceError::Unavailable)?;
            s.semantic_work().scan_bytes(text.len()).map_err(core_error)?;
        }
        let key_storage=s.reserve_external_payload(key_bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(key_bytes).map_err(core_error)?;
        let saved_key=key.clone();
        let changes=tx.workflow_management_changes(s).await.map_err(agreement::financial::management_read_error)?;
        Ok(OwnedLocalManagement{key:saved_key,owner:tx.workflow_proof_failure_handle(),changes,setup,installation,_key_storage:key_storage,_control:control})
    }
    pub(super) async fn fresh_management(
        &self, store:&SqliteStore, tx:&mut SqliteTx, key:&request::OperationKey,
        s:&mut ledgerlab_core::validation_work::OriginalAuditSession,
    )->Result<OwnedLocalManagement> {
        let held=self.read_management(store,tx,key,s).await?;
        self.management_available(&held,key,s)?;
        Ok(held)
    }
    fn management_available(&self,held:&OwnedLocalManagement,key:&request::OperationKey,s:&mut ledgerlab_core::validation_work::OriginalAuditSession)->Result<()> {
        use ledgerlab_core::validation_work::ValidationVisit as Visit;
        let setup=held.setup();
        s.semantic_work().visit(Visit::MemberCompare).map_err(core_error)?;
        s.semantic_work().scan_bytes(key.source.len()).map_err(core_error)?;
        s.semantic_work().scan_bytes(setup.source.len()).map_err(core_error)?;
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        require(key.source==setup.source,"AUTHORITY_REQUIRED")?;
        let state=held.installation();
        s.semantic_work().visit(Visit::MemberCompare).map_err(core_error)?;
        s.semantic_work().scan_bytes(state.admission.len()).map_err(core_error)?;
        s.semantic_work().scan_bytes(4).map_err(core_error)?;
        let open=state.admission=="open";
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        if !open {return Err(ServiceError::Unavailable);}
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        if !state.dispatch_hold {return Err(ServiceError::Unavailable);}
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        if state.dispatch_enabled {return Err(ServiceError::Unavailable);}
        Ok(())
    }
    fn management_paths(&self,store:&SqliteStore,s:&mut ledgerlab_core::validation_work::OriginalAuditSession)->Result<()> {
        #[cfg(target_os="macos")]
        {
            let paths=store.workflow_management_paths(s).map_err(agreement::financial::management_read_error)?;
            for (path,directory) in [(paths.directory(),true),(paths.database(),false)] {
                local_io::private_existing_with_work(path,directory,s.semantic_work()).map_err(|error|match error {
                    local_io::LocalError::Service(error)=>error,
                    _=>reject("AUTHORITY_REQUIRED"),
                })?;
            }
        }
        // Other native targets retain the existing behavior; this fixed-host
        // ownership delta makes no resource qualification for those builds.
        #[cfg(not(target_os="macos"))]
        {
            let (directory,database)=store.workflow_owner_paths().map_err(super::super::store_error)?;
            local_io::private_existing(&directory,true).map_err(|_|reject("AUTHORITY_REQUIRED"))?;
            local_io::private_existing(&database,false).map_err(|_|reject("AUTHORITY_REQUIRED"))?;
        }
        Ok(())
    }
    /// Only immutable normalized DATA is reused. Actual native paths and the
    /// connection mutation baseline are checked again at every consuming stage.
    pub(super) async fn recheck_management_fresh(&self,held:&OwnedLocalManagement,store:&SqliteStore,tx:&mut SqliteTx,key:&request::OperationKey,s:&mut ledgerlab_core::validation_work::OriginalAuditSession)->Result<()> {
        use ledgerlab_core::validation_work::{ValidationVisit as Visit,ArithmeticOperation as Op};
        s.check_cancelled().map_err(core_error)?;
        s.require_external_owner(&held._control).map_err(core_error)?;
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        require(proof_session::present(tx,key)?,"INTEGRITY_FAILURE")?;
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        let owner=tx.workflow_proof_failure_handle();
        if !held.owner.same_instance(&owner) {
            held.owner.store(true,std::sync::atomic::Ordering::Release);
            owner.store(true,std::sync::atomic::Ordering::Release);
            return Err(ServiceError::IntegrityFailure);
        }
        s.semantic_work().visit(Visit::Constraint).map_err(core_error)?;
        require(self.instance.workflow_same_instance(store),"AUTHORITY_REQUIRED")?;
        for (a,b) in [(&held.key.logical_store_id,&key.logical_store_id),(&held.key.scope[0],&key.scope[0]),(&held.key.scope[1],&key.scope[1]),(&held.key.source,&key.source),(&held.key.operation_id,&key.operation_id)] {
            s.semantic_work().visit(Visit::MemberCompare).map_err(core_error)?;
            s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
            s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
            require(a==b,"INTEGRITY_FAILURE")?;
        }
        let actual=tx.workflow_management_changes(s).await.map_err(agreement::financial::management_read_error)?;
        s.semantic_work().arithmetic(Op::Compare,&[&actual.into(),&held.changes.into()]).map_err(core_error)?;
        require(actual==held.changes,"STALE_REVISION")?;
        self.management_paths(store,s)?;
        self.management_available(held,key,s)
    }
    pub(super) async fn read(
        &self,
        store: &SqliteStore,
        tx: &mut SqliteTx,
        key: &request::OperationKey,
    ) -> Result<billing::Setup> {
        require(
            self.instance.workflow_same_instance(store),
            "AUTHORITY_REQUIRED",
        )?;
        let (directory, database) = store
            .workflow_owner_paths()
            .map_err(super::super::store_error)?;
        local_io::private_existing(&directory, true).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        local_io::private_existing(&database, false).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        let setup = setup_identity(tx).await?;
        require(
            self.logical_store_id == setup.store_id
                && self.scope == [setup.scope.tenant(), setup.scope.environment()]
                && self.operator == setup.operator
                && key.logical_store_id == self.logical_store_id
                && key.scope == self.scope,
            "AUTHORITY_REQUIRED",
        )?;
        // Source rights/current mapping do not revoke this distinct local admin.
        Ok(setup)
    }
    pub(super) async fn fresh(
        &self,
        store: &SqliteStore,
        tx: &mut SqliteTx,
        key: &request::OperationKey,
    ) -> Result<billing::Setup> {
        let setup = self.read(store, tx, key).await?;
        require(key.source == setup.source, "AUTHORITY_REQUIRED")?;
        let state = tx
            .load_installation()
            .await
            .map_err(super::super::store_error)?;
        // Mutable installation availability is a service boundary, not an
        // invented publication/economic refusal in the closed registry.
        if state.admission != "open" || !state.dispatch_hold || state.dispatch_enabled {
            return Err(ServiceError::Unavailable);
        }
        Ok(setup)
    }
}

#[cfg(test)]
mod tests;
