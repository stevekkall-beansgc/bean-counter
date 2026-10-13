//! Mechanical generic publication persistence. The coordinator validates bytes,
//! authority and manifest closure. This adapter does not compile or price.
use super::SqliteTx;
use crate::store::errors::StoreError;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::time::timeout_at;
pub(crate) mod capacity;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordRef {
    pub kind: String,
    pub id: String,
    pub digest: String,
}
pub(crate) struct Document {
    pub reference: RecordRef,
    pub schema: String,
    pub bytes: Vec<u8>,
}
pub(crate) struct RegistryEntry {
    pub kind: String,
    pub semantic_id: String,
    pub version: String,
    pub document_id: String,
}
#[derive(Clone)]
pub(crate) struct StoredOperation {
    pub identity: Vec<u8>,
    pub operation_ref: RecordRef,
    pub operation_bytes: Vec<u8>,
    pub request_hash: String,
    pub manifest_ref: RecordRef,
    pub manifest_bytes: Vec<u8>,
    pub receipt_ref: RecordRef,
    pub receipt_bytes: Vec<u8>,
    pub publication_id: String,
    pub plan_id: String,
    pub sequence: i64,
    pub accepted_at_us: i64,
    pub members: Vec<RecordRef>,
}
/// Crate-private coordinator output, never an externally supplied append API.
pub(crate) struct PublicationAppend {
    pub store_id: String,
    pub tenant: String,
    pub environment: String,
    pub source: String,
    pub operation_id: String,
    pub documents: Vec<Document>,
    pub registry: Vec<RegistryEntry>,
    pub publication_key: Vec<u8>,
    pub root_policy_id: String,
    pub operation: StoredOperation,
}

impl SqliteTx {
    async fn workflow_schema(&mut self) -> Result<(), StoreError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if version != 12 {
            return Err(StoreError::BillingUpgradeRequired);
        }
        Ok(())
    }

    pub(crate) async fn workflow_lookup(
        &mut self,
        identity: &[u8],
    ) -> Result<Option<StoredOperation>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            let row = sqlx::query("SELECT * FROM workflow_operations WHERE identity_key=?")
                .bind(identity).fetch_optional(self.conn()).await?;
            let Some(r) = row else { return Ok(None); };
            let members = sqlx::query_as::<_, (String,String,String)>("SELECT kind,id,digest FROM workflow_manifest_members WHERE identity_key=? ORDER BY kind,id,digest")
                .bind(identity).fetch_all(self.conn()).await?.into_iter()
                .map(|(kind,id,digest)| RecordRef{kind,id,digest}).collect();
            Ok(Some(StoredOperation {
                identity: r.try_get("identity_key")?,
                operation_ref: RecordRef { kind:"workflow-operation".into(),id:r.try_get("operation_record_id")?,digest:r.try_get("operation_digest")? },
                operation_bytes:r.try_get("operation_bytes")?, request_hash:r.try_get("normalized_request_hash")?,
                manifest_ref:RecordRef {kind:"workflow-manifest".into(),id:r.try_get("manifest_id")?,digest:r.try_get("manifest_digest")?},
                manifest_bytes:r.try_get("manifest_bytes")?,
                receipt_ref:RecordRef {kind:"workflow-receipt".into(),id:r.try_get("receipt_id")?,digest:r.try_get("receipt_digest")?},
                receipt_bytes:r.try_get("receipt_bytes")?,publication_id:r.try_get("publication_id")?,plan_id:r.try_get("plan_id")?,
                sequence:r.try_get("commit_sequence")?,accepted_at_us:r.try_get("accepted_at_us")?,members,
            }))
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }

    /// Disclosure must be reachable from an original manifest in the authorized
    /// scope. A known content hash alone never grants visibility. `source=None`
    /// is only for the coordinator's verified installation administrator.
    pub(crate) async fn workflow_document(
        &mut self,
        reference: &RecordRef,
        store: &str,
        tenant: &str,
        environment: &str,
        source: Option<&str>,
    ) -> Result<Option<Document>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            let row:Option<(String,Vec<u8>)> = sqlx::query_as("SELECT d.schema_name,d.canonical_bytes FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.digest=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND (? IS NULL OR o.source=?))")
                .bind(&reference.kind).bind(&reference.id).bind(&reference.digest).bind(store).bind(tenant).bind(environment).bind(source).bind(source).fetch_optional(self.conn()).await?;
            Ok(row.map(|(schema,bytes)|Document{reference:reference.clone(),schema,bytes}))
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }

    pub(crate) async fn workflow_publication(
        &mut self,
        key: &[u8],
    ) -> Result<Option<(String, String, String)>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            self.workflow_schema().await?;
            sqlx::query_as("SELECT publication_id,plan_id,root_policy_id FROM workflow_publications WHERE publication_key=?")
                .bind(key).fetch_optional(self.conn()).await.map_err(StoreError::from)
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }

    pub(crate) async fn workflow_sequence(&mut self) -> Result<i64, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            let sequence: i64 = sqlx::query_scalar(
                "SELECT COALESCE(max(commit_sequence),0)+1 FROM workflow_operations",
            )
            .fetch_one(self.conn())
            .await?;
            if sequence > 100000 {
                return Err(StoreError::BillingHistoryLimit);
            }
            Ok(sequence)
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }

    pub(crate) async fn workflow_append(
        &mut self,
        p: &PublicationAppend,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("poisoned workflow transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.workflow_append_inner(p))
            .await
            .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    async fn workflow_append_inner(&mut self, p: &PublicationAppend) -> Result<(), StoreError> {
        self.workflow_schema().await?;
        let o = &p.operation;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=?)",
        )
        .bind(&o.identity)
        .fetch_one(self.conn())
        .await?;
        if exists {
            return Err(StoreError::ExpectedCurrent);
        }
        if p.documents.len() > 147 || o.members.len() > 147 {
            return Err(StoreError::BillingHistoryLimit);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut new_ids = std::collections::BTreeSet::new();
        let mut additional = [
            o.identity.len(),
            o.operation_bytes.len(),
            o.manifest_bytes.len(),
            o.receipt_bytes.len(),
        ]
        .into_iter()
        .try_fold(0_i64, |n, len| {
            i64::try_from(len).ok().and_then(|len| n.checked_add(len))
        })
        .ok_or(StoreError::BillingHistoryLimit)?;
        for d in &p.documents {
            if !seen.insert(&d.reference.id) {
                return Err(StoreError::Integrity("duplicate prepared document"));
            }
            let existing: Option<(String, String, String, Vec<u8>)> = sqlx::query_as(
                "SELECT kind,digest,schema_name,canonical_bytes FROM workflow_documents WHERE id=?",
            )
            .bind(&d.reference.id)
            .fetch_optional(self.conn())
            .await?;
            match existing {
                Some((kind, digest, schema, bytes))
                    if (
                        kind.as_str(),
                        digest.as_str(),
                        schema.as_str(),
                        bytes.as_slice(),
                    ) != (
                        d.reference.kind.as_str(),
                        d.reference.digest.as_str(),
                        d.schema.as_str(),
                        d.bytes.as_slice(),
                    ) =>
                {
                    return Err(StoreError::Integrity(
                        "immutable workflow document conflict",
                    ));
                }
                Some(_) => {}
                None => {
                    new_ids.insert(&d.reference.id);
                    additional = additional
                        .checked_add(
                            i64::try_from(d.bytes.len())
                                .map_err(|_| StoreError::BillingHistoryLimit)?,
                        )
                        .ok_or(StoreError::BillingHistoryLimit)?;
                }
            }
        }
        let meter = capacity::check(self.conn(), 1, additional).await?;
        if o.sequence != meter.operations + 1 {
            return Err(StoreError::Integrity("workflow sequence gap"));
        }
        // Detect all immutable registry conflicts before the first insertion.
        for r in &p.registry {
            let existing:Option<String>=sqlx::query_scalar("SELECT document_id FROM workflow_definition_registry WHERE logical_store_id=? AND tenant=? AND environment=? AND kind=? AND semantic_id=? AND version=?")
                .bind(&p.store_id).bind(&p.tenant).bind(&p.environment).bind(&r.kind).bind(&r.semantic_id).bind(&r.version).fetch_optional(self.conn()).await?;
            if existing.as_ref().is_some_and(|id| id != &r.document_id) {
                return Err(StoreError::DeliveryConflict);
            }
        }
        for d in &p.documents {
            if new_ids.contains(&d.reference.id) {
                sqlx::query("INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) VALUES(?,?,?,?,?)")
                    .bind(&d.reference.kind).bind(&d.reference.id).bind(&d.reference.digest).bind(&d.schema).bind(&d.bytes).execute(self.conn()).await?;
            }
        }
        for r in &p.registry {
            sqlx::query("INSERT INTO workflow_definition_registry VALUES(?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING")
                .bind(&p.store_id).bind(&p.tenant).bind(&p.environment).bind(&p.source).bind(&r.kind).bind(&r.semantic_id).bind(&r.version).bind(&r.document_id).execute(self.conn()).await?;
        }
        let publication:Option<(String,String,String)>=sqlx::query_as("SELECT publication_id,plan_id,root_policy_id FROM workflow_publications WHERE publication_key=?")
            .bind(&p.publication_key).fetch_optional(self.conn()).await?;
        if let Some(existing) = publication {
            if existing
                != (
                    o.publication_id.clone(),
                    o.plan_id.clone(),
                    p.root_policy_id.clone(),
                )
            {
                return Err(StoreError::Integrity("workflow publication conflict"));
            }
        } else {
            sqlx::query("INSERT INTO workflow_publications VALUES(?,?,?,?)")
                .bind(&p.publication_key)
                .bind(&o.publication_id)
                .bind(&o.plan_id)
                .bind(&p.root_policy_id)
                .execute(self.conn())
                .await?;
        }
        sqlx::query(
            "INSERT INTO workflow_operations VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&o.identity)
        .bind(&p.store_id)
        .bind(&p.tenant)
        .bind(&p.environment)
        .bind(&p.source)
        .bind(&p.operation_id)
        .bind(&o.operation_ref.id)
        .bind(&o.operation_ref.digest)
        .bind(&o.operation_bytes)
        .bind(&o.request_hash)
        .bind(&o.manifest_ref.id)
        .bind(&o.manifest_ref.digest)
        .bind(&o.manifest_bytes)
        .bind(&o.receipt_ref.id)
        .bind(&o.receipt_ref.digest)
        .bind(&o.receipt_bytes)
        .bind(&o.publication_id)
        .bind(&o.plan_id)
        .bind(o.sequence)
        .bind(o.accepted_at_us)
        .execute(self.conn())
        .await?;
        for r in &o.members {
            sqlx::query("INSERT INTO workflow_manifest_members VALUES(?,?,?,?)")
                .bind(&o.identity)
                .bind(&r.kind)
                .bind(&r.id)
                .bind(&r.digest)
                .execute(self.conn())
                .await?;
        }
        sqlx::query("INSERT INTO workflow_feed VALUES(?,?)")
            .bind(o.sequence)
            .bind(&o.identity)
            .execute(self.conn())
            .await?;
        sqlx::query("UPDATE workflow_state SET operation_count=operation_count+1,payload_bytes=payload_bytes+? WHERE singleton=1").bind(additional).execute(self.conn()).await?;
        capacity::check(self.conn(), 0, 0).await?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
