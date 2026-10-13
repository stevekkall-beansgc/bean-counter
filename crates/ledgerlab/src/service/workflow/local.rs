//! Trusted local-process entry. Remote/source actors cannot mint this capability.
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
    store: SqliteStore,
    admin: LocalOwnerAdmin,
    #[cfg(test)]
    lose_reply: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    preparation_gate: std::sync::Mutex<Option<TestPreparationGate>>,
}
impl LocalWorkflowLedger {
    /// Publish through the journal using only the implemented builtin compiler
    /// and code-owned captured build inputs. Publishing obtains no assent.
    pub async fn publish(&self, raw: &[u8]) -> Result<Value> {
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
        let result = async {
            self.admin.read(&self.store, &mut tx, &key).await?;
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
        }.await;
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
        let result = async {
            self.admin.read(&self.store, &mut tx, &key).await?;
            match tx
                .workflow_lookup(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                Some(o) => records::verify_retained(&mut tx, &key, &o).await,
                None => Ok(
                    json!({"schema":"ledger-operation-status/1","status":"not_observed","key":key}),
                ),
            }
        }
        .await;
        tx.rollback().await.map_err(super::super::store_error)?;
        result
    }
    /// Explicit trusted-local opt-in. Whole legacy history is validated under
    /// the same ordered transaction before the mechanical additive migration.
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
                .map_err(super::super::store_error)
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
        let preparation: Result<PublicationPreparation> = async {
            self.admin.read(&self.store, &mut tx, key).await?;
            if let Some(original) = tx
                .workflow_lookup(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                return Ok((Some(retry(&mut tx, &req, &original).await?), None));
            }
            self.admin.fresh(&self.store, &mut tx, key).await?;
            Ok((None, Some(resolve_documents(&mut tx, &req).await?)))
        }
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
        let written: Result<(Value, bool)> = async {
            self.admin.read(&self.store, &mut tx, key).await?;
            // A racing commit wins before mutable fresh configuration/support.
            if let Some(original) = tx
                .workflow_lookup(&key.bytes()?)
                .await
                .map_err(super::super::store_error)?
            {
                return Ok((retry(&mut tx, &req, &original).await?, false));
            }
            let (prepared, evidence, provenance) = fresh_prepared?;
            let setup = self.admin.fresh(&self.store, &mut tx, key).await?;
            if req.retained_refs().is_some() {
                let locked = resolve_documents(&mut tx, &req).await?;
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
            let result = records::verify_retained(&mut tx, key, &append.operation).await?;
            Ok((result, true))
        }
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
impl LocalOwnerAdmin {
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
