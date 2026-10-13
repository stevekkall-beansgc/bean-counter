//! Mechanical immutable source transport projections. No parser establishes
//! authority, compiler support, source normalization or a customer agreement.
use super::{
    super::{RecordRef, RegistryEntry},
    SqliteTx, StoreError,
};
use sqlx::Row;
use tokio::time::timeout_at;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Upload {
    pub upload_id: String,
    pub document_schema: String,
    pub digest: String,
    pub bytes: i64,
}
pub(crate) struct Part {
    pub upload: Upload,
    pub part: i64,
    pub document: String,
}
impl SqliteTx {
    /// Candidate identity only. The coordinator still verifies its full saved
    /// certificate; raw JSON is never used as a persistence key/value shortcut.
    pub(crate) async fn workflow_transport_result_origin(
        &mut self,
        store: &str,
        scope: &[String; 2],
        reference: &RecordRef,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            self.workflow_schema().await?;
            let rows:Vec<(Vec<u8>,Vec<u8>)>=sqlx::query_as("SELECT o.identity_key,o.operation_bytes FROM workflow_operations o JOIN workflow_manifest_members m ON m.identity_key=o.identity_key WHERE o.operation_kind='evidence.retain' AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND m.kind=? AND m.id=? AND m.digest=? ORDER BY o.commit_sequence").bind(store).bind(&scope[0]).bind(&scope[1]).bind(&reference.kind).bind(&reference.id).bind(&reference.digest).fetch_all(self.conn()).await?;
            for (identity,bytes) in rows {
                let body=ledgerlab_core::canonical::parse_bounded(&bytes,8*1024*1024).map_err(|_|StoreError::Integrity("invalid transport origin candidate"))?;
                if ledgerlab_core::canonical::CanonicalBytes::from_value(&body).map_err(|_|StoreError::Integrity("invalid transport origin candidate"))?.as_slice()!=bytes {return Err(StoreError::Integrity("noncanonical transport origin candidate"));}
                let refs:Vec<RecordRef>=serde_json::from_value(body["result_refs"].clone()).map_err(|_|StoreError::Integrity("invalid transport origin result copies"))?;
                if refs==vec![reference.clone()] {return Ok(Some(identity));}
            }Ok(None)
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_transport_upload(
        &mut self,
        store: &str,
        scope: &[String; 2],
        source: &str,
        id: &str,
    ) -> Result<Option<Upload>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;let row=sqlx::query("SELECT document_schema,canonical_document_sha256,document_bytes FROM workflow_definition_uploads WHERE logical_store_id=? AND tenant=? AND environment=? AND source=? AND upload_id=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(source).bind(id).fetch_optional(self.conn()).await?;
        row.map(|r|Ok::<_,sqlx::Error>(Upload{upload_id:id.into(),document_schema:r.try_get("document_schema")?,digest:r.try_get("canonical_document_sha256")?,bytes:r.try_get("document_bytes")?})).transpose().map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_transport_part(
        &mut self,
        store: &str,
        scope: &[String; 2],
        source: &str,
        id: &str,
        part: i64,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT document_id FROM workflow_definition_upload_parts WHERE logical_store_id=? AND tenant=? AND environment=? AND source=? AND upload_id=? AND part=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(source).bind(id).bind(part).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_transport_definition(
        &mut self,
        store: &str,
        scope: &[String; 2],
        kind: &str,
        name: &str,
        version: &str,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT document_id FROM workflow_definition_registry WHERE logical_store_id=? AND tenant=? AND environment=? AND kind=? AND semantic_id=? AND version=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(kind).bind(name).bind(version).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(super) async fn workflow_transport_insert_definition(
        &mut self,
        store: &str,
        scope: &[String; 2],
        source: &str,
        entry: &RegistryEntry,
    ) -> Result<(), StoreError> {
        let old = self
            .workflow_transport_definition(
                store,
                scope,
                &entry.kind,
                &entry.semantic_id,
                &entry.version,
            )
            .await?;
        if old.as_ref().is_some_and(|id| id != &entry.document_id) {
            return Err(StoreError::DeliveryConflict);
        }
        sqlx::query("INSERT INTO workflow_definition_registry(logical_store_id,tenant,environment,source,kind,semantic_id,version,document_id) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING").bind(store).bind(&scope[0]).bind(&scope[1]).bind(source).bind(&entry.kind).bind(&entry.semantic_id).bind(&entry.version).bind(&entry.document_id).execute(self.conn()).await?;
        Ok(())
    }
    pub(super) async fn workflow_transport_insert_part(
        &mut self,
        store: &str,
        scope: &[String; 2],
        source: &str,
        part: &Part,
    ) -> Result<(), StoreError> {
        if self
            .workflow_transport_upload(store, scope, source, &part.upload.upload_id)
            .await?
            .as_ref()
            .is_some_and(|old| old != &part.upload)
        {
            return Err(StoreError::DeliveryConflict);
        }
        if self
            .workflow_transport_part(store, scope, source, &part.upload.upload_id, part.part)
            .await?
            .is_some()
        {
            return Err(StoreError::DeliveryConflict);
        }
        sqlx::query("INSERT INTO workflow_definition_uploads VALUES(?,?,?,?,?,?,?,?) ON CONFLICT DO NOTHING").bind(store).bind(&scope[0]).bind(&scope[1]).bind(source).bind(&part.upload.upload_id).bind(&part.upload.document_schema).bind(&part.upload.digest).bind(part.upload.bytes).execute(self.conn()).await?;
        sqlx::query("INSERT INTO workflow_definition_upload_parts VALUES(?,?,?,?,?,?,?)")
            .bind(store)
            .bind(&scope[0])
            .bind(&scope[1])
            .bind(source)
            .bind(&part.upload.upload_id)
            .bind(part.part)
            .bind(&part.document)
            .execute(self.conn())
            .await?;
        Ok(())
    }
}
