//! Mechanical administrative journal persistence; no assent or grant decisions.
use super::{Document, RecordRef, capacity};
use crate::store::{errors::StoreError, sqlite::SqliteTx};
use ledgerlab_core::canonical::CanonicalBytes;
use serde_json::json;
use sqlx::Row;
use tokio::time::timeout_at;

#[derive(Clone, Debug)]
pub(crate) struct JournalOperation {
    pub identity: Vec<u8>,
    pub kind: String,
    pub operation_ref: RecordRef,
    pub operation_bytes: Vec<u8>,
    pub request_hash: String,
    pub manifest_ref: RecordRef,
    pub manifest_bytes: Vec<u8>,
    pub receipt_ref: RecordRef,
    pub receipt_bytes: Vec<u8>,
    pub sequence: i64,
    pub accepted_at_us: i64,
    pub members: Vec<RecordRef>,
}
pub(crate) struct JournalAppend {
    pub store: String,
    pub tenant: String,
    pub environment: String,
    pub source: String,
    pub operation_id: String,
    pub documents: Vec<Document>,
    pub operation: JournalOperation,
    pub semantic_key: Vec<u8>,
    pub alias_of: Option<Vec<u8>>,
    pub projection: Projection,
}
pub(crate) enum Projection {
    None,
    Customer {
        customer: String,
        document: String,
    },
    Enrollment {
        enrollment: String,
        document: String,
        source: String,
        namespace: String,
        head: SourceHead,
    },
    State {
        enrollment: String,
        expected_revision: i64,
        head: SourceHead,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceHead {
    pub revision: i64,
    pub state: String,
    pub grant: Option<String>,
    pub status: String,
}

impl SqliteTx {
    pub(crate) async fn workflow_agreement_root_candidate(
        &mut self,
        store: &str,
        scope: &[String; 2],
    ) -> Result<Option<Document>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            self.workflow_schema().await?;
            let row=sqlx::query("SELECT d.kind,d.id,d.digest,d.schema_name,d.canonical_bytes FROM workflow_documents d JOIN workflow_manifest_members m ON m.kind=d.kind AND m.id=d.id AND m.digest=d.digest JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE d.kind='workflow-authority' AND o.logical_store_id=? AND o.tenant=? AND o.environment=? ORDER BY o.commit_sequence DESC LIMIT 1").bind(store).bind(&scope[0]).bind(&scope[1]).fetch_optional(self.conn()).await?;
            row.map(|r|Ok(Document{reference:RecordRef{kind:r.try_get("kind")?,id:r.try_get("id")?,digest:r.try_get("digest")?},schema:r.try_get("schema_name")?,bytes:r.try_get("canonical_bytes")?})).transpose().map_err(StoreError::from)
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_lookup(
        &mut self,
        identity: &[u8],
    ) -> Result<Option<JournalOperation>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            let row = sqlx::query("SELECT * FROM workflow_operations WHERE identity_key=?")
                .bind(identity)
                .fetch_optional(self.conn())
                .await?;
            let Some(r) = row else { return Ok(None) };
            let kind: String = r.try_get("operation_kind")?;
            if !matches!(
                kind.as_str(),
                "evidence.retain"
                    | "authority.change"
                    | "agreement.accept"
                    | "agreement.activate"
                    | "agreement.revoke"
            ) {
                return Err(StoreError::Integrity("wrong agreement journal kind"));
            }
            if r.try_get::<Option<String>, _>("publication_id")?.is_some()
                || r.try_get::<Option<String>, _>("plan_id")?.is_some()
            {
                return Err(StoreError::Integrity(
                    "nonpublication acquired publication pair",
                ));
            }
            let members = sqlx::query(
                "SELECT kind,id,digest FROM workflow_manifest_members WHERE identity_key=?",
            )
            .bind(identity)
            .fetch_all(self.conn())
            .await?
            .into_iter()
            .map(|m| {
                Ok(RecordRef {
                    kind: m.try_get("kind")?,
                    id: m.try_get("id")?,
                    digest: m.try_get("digest")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
            Ok(Some(JournalOperation {
                identity: identity.to_vec(),
                kind,
                operation_ref: RecordRef {
                    kind: "workflow-operation".into(),
                    id: r.try_get("operation_record_id")?,
                    digest: r.try_get("operation_digest")?,
                },
                operation_bytes: r.try_get("operation_bytes")?,
                request_hash: r.try_get("normalized_request_hash")?,
                manifest_ref: RecordRef {
                    kind: "workflow-manifest".into(),
                    id: r.try_get("manifest_id")?,
                    digest: r.try_get("manifest_digest")?,
                },
                manifest_bytes: r.try_get("manifest_bytes")?,
                receipt_ref: RecordRef {
                    kind: "workflow-receipt".into(),
                    id: r.try_get("receipt_id")?,
                    digest: r.try_get("receipt_digest")?,
                },
                receipt_bytes: r.try_get("receipt_bytes")?,
                sequence: r.try_get("commit_sequence")?,
                accepted_at_us: r.try_get("accepted_at_us")?,
                members,
            }))
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_semantic(
        &mut self,
        store: &str,
        scope: &[String; 2],
        kind: &str,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT original_identity_key FROM workflow_agreement_semantics WHERE logical_store_id=? AND tenant=? AND environment=? AND operation_kind=? AND semantic_key=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(kind).bind(key).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_customer(
        &mut self,
        store: &str,
        scope: &[String; 2],
        customer: &str,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT document_id FROM workflow_customer_registrations WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(customer).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_enrollment(
        &mut self,
        store: &str,
        scope: &[String; 2],
        enrollment: &str,
    ) -> Result<Option<String>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;sqlx::query_scalar("SELECT document_id FROM workflow_source_enrollments WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(enrollment).fetch_optional(self.conn()).await.map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    pub(crate) async fn workflow_agreement_source_head(
        &mut self,
        store: &str,
        scope: &[String; 2],
        enrollment: &str,
    ) -> Result<Option<SourceHead>, StoreError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {self.workflow_schema().await?;let row=sqlx::query("SELECT revision,state_id,grant_id,status FROM workflow_source_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=?").bind(store).bind(&scope[0]).bind(&scope[1]).bind(enrollment).fetch_optional(self.conn()).await?;row.map(|r|Ok(SourceHead{revision:r.try_get("revision")?,state:r.try_get("state_id")?,grant:r.try_get("grant_id")?,status:r.try_get("status")?})).transpose().map_err(StoreError::from)}).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    /// Only the trusted coordinator can construct this crate-private append.
    /// Cancellation or any failed insert poisons the entire transaction.
    pub(crate) async fn workflow_agreement_append(
        &mut self,
        p: &JournalAppend,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("poisoned agreement transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.workflow_agreement_append_inner(p))
            .await
            .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    async fn workflow_agreement_append_inner(
        &mut self,
        p: &JournalAppend,
    ) -> Result<(), StoreError> {
        self.workflow_schema().await?;
        let o = &p.operation;
        if p.documents.len() > 4095 || o.members.is_empty() || o.members.len() > 4096 {
            return Err(StoreError::BillingHistoryLimit);
        }
        if !matches!(
            o.kind.as_str(),
            "evidence.retain"
                | "authority.change"
                | "agreement.accept"
                | "agreement.activate"
                | "agreement.revoke"
        ) {
            return Err(StoreError::Integrity("unsupported agreement journal kind"));
        }
        let body = ledgerlab_core::canonical::parse_bounded(&o.operation_bytes, 8 * 1024 * 1024)
            .map_err(|_| StoreError::Integrity("invalid prepared journal body"))?;
        let key = json!({"logical_store_id":p.store,"scope":[p.tenant,p.environment],"source":p.source,"operation_id":p.operation_id});
        if body["kind"] != o.kind
            || body["key"] != key
            || CanonicalBytes::from_value(&key)
                .map_err(|_| StoreError::Integrity("invalid prepared identity"))?
                .as_slice()
                != o.identity
            || CanonicalBytes::from_value(&body)
                .map_err(|_| StoreError::Integrity("invalid prepared journal"))?
                .as_slice()
                != o.operation_bytes
        {
            return Err(StoreError::Integrity("journal column/body mismatch"));
        }
        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=?)",
        )
        .bind(&o.identity)
        .fetch_one(self.conn())
        .await?
        {
            return Err(StoreError::ExpectedCurrent);
        }
        let existing:Option<Vec<u8>>=sqlx::query_scalar("SELECT original_identity_key FROM workflow_agreement_semantics WHERE logical_store_id=? AND tenant=? AND environment=? AND operation_kind=? AND semantic_key=?").bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(&o.kind).bind(&p.semantic_key).fetch_optional(self.conn()).await?;
        if existing != p.alias_of
            || (p.alias_of.is_some() && !matches!(p.projection, Projection::None))
        {
            return Err(StoreError::DeliveryConflict);
        }
        let mut unique = std::collections::BTreeSet::new();
        let mut new = std::collections::BTreeSet::new();
        let mut additional = [
            o.identity.len(),
            o.operation_bytes.len(),
            o.manifest_bytes.len(),
            o.receipt_bytes.len(),
        ]
        .into_iter()
        .try_fold(0_i64, |n, l| {
            i64::try_from(l).ok().and_then(|l| n.checked_add(l))
        })
        .ok_or(StoreError::BillingHistoryLimit)?;
        for d in &p.documents {
            if !unique.insert(&d.reference.id) {
                return Err(StoreError::Integrity(
                    "duplicate prepared agreement document",
                ));
            }
            let prior: Option<(String, String, String, Vec<u8>)> = sqlx::query_as(
                "SELECT kind,digest,schema_name,canonical_bytes FROM workflow_documents WHERE id=?",
            )
            .bind(&d.reference.id)
            .fetch_optional(self.conn())
            .await?;
            match prior {
                Some((kind, digest, schema, bytes))
                    if kind != d.reference.kind
                        || digest != d.reference.digest
                        || schema != d.schema
                        || bytes != d.bytes =>
                {
                    return Err(StoreError::Integrity(
                        "immutable agreement document conflict",
                    ));
                }
                Some(_) => {}
                None => {
                    new.insert(&d.reference.id);
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
            return Err(StoreError::Integrity("agreement journal sequence gap"));
        }
        for d in &p.documents {
            if new.contains(&d.reference.id) {
                sqlx::query("INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) VALUES(?,?,?,?,?)").bind(&d.reference.kind).bind(&d.reference.id).bind(&d.reference.digest).bind(&d.schema).bind(&d.bytes).execute(self.conn()).await?;
            }
        }
        match &p.projection {
            Projection::None => {}
            Projection::Customer { customer, document } => {
                sqlx::query("INSERT INTO workflow_customer_registrations VALUES(?,?,?,?,?)")
                    .bind(&p.store)
                    .bind(&p.tenant)
                    .bind(&p.environment)
                    .bind(customer)
                    .bind(document)
                    .execute(self.conn())
                    .await?;
            }
            Projection::Enrollment {
                enrollment,
                document,
                source,
                namespace,
                head,
            } => {
                let old:Option<String>=sqlx::query_scalar("SELECT canonical_economic_namespace FROM workflow_source_namespaces WHERE logical_store_id=? AND tenant=? AND environment=? AND source=?").bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(source).fetch_optional(self.conn()).await?;
                if old.as_ref().is_some_and(|old| old != namespace) {
                    return Err(StoreError::DeliveryConflict);
                }
                sqlx::query("INSERT INTO workflow_source_namespaces VALUES(?,?,?,?,?) ON CONFLICT DO NOTHING").bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(source).bind(namespace).execute(self.conn()).await?;
                sqlx::query("INSERT INTO workflow_source_enrollments VALUES(?,?,?,?,?,?,?)")
                    .bind(&p.store)
                    .bind(&p.tenant)
                    .bind(&p.environment)
                    .bind(enrollment)
                    .bind(document)
                    .bind(source)
                    .bind(namespace)
                    .execute(self.conn())
                    .await?;
                if head.revision != 1 || head.status != "active" || head.grant.is_none() {
                    return Err(StoreError::Integrity(
                        "invalid initial source state projection",
                    ));
                }
                sqlx::query("INSERT INTO workflow_source_heads VALUES(?,?,?,?,?,?,?,?)")
                    .bind(&p.store)
                    .bind(&p.tenant)
                    .bind(&p.environment)
                    .bind(enrollment)
                    .bind(head.revision)
                    .bind(&head.state)
                    .bind(&head.grant)
                    .bind(&head.status)
                    .execute(self.conn())
                    .await?;
            }
            Projection::State {
                enrollment,
                expected_revision,
                head,
            } => {
                if expected_revision.checked_add(1) != Some(head.revision) {
                    return Err(StoreError::Integrity("invalid source state increment"));
                }
                let changed=sqlx::query("UPDATE workflow_source_heads SET revision=?,state_id=?,grant_id=?,status=? WHERE logical_store_id=? AND tenant=? AND environment=? AND enrollment_id=? AND revision=? AND status='active'").bind(head.revision).bind(&head.state).bind(&head.grant).bind(&head.status).bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(enrollment).bind(expected_revision).execute(self.conn()).await?;
                if changed.rows_affected() != 1 {
                    return Err(StoreError::ExpectedCurrent);
                }
            }
        }
        sqlx::query("INSERT INTO workflow_operations(identity_key,logical_store_id,tenant,environment,source,operation_id,operation_record_id,operation_digest,operation_bytes,normalized_request_hash,manifest_id,manifest_digest,manifest_bytes,receipt_id,receipt_digest,receipt_bytes,publication_id,plan_id,commit_sequence,accepted_at_us,operation_kind) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,NULL,NULL,?,?,?)")
            .bind(&o.identity).bind(&p.store).bind(&p.tenant).bind(&p.environment).bind(&p.source).bind(&p.operation_id).bind(&o.operation_ref.id).bind(&o.operation_ref.digest).bind(&o.operation_bytes).bind(&o.request_hash).bind(&o.manifest_ref.id).bind(&o.manifest_ref.digest).bind(&o.manifest_bytes).bind(&o.receipt_ref.id).bind(&o.receipt_ref.digest).bind(&o.receipt_bytes).bind(o.sequence).bind(o.accepted_at_us).bind(&o.kind).execute(self.conn()).await?;
        if p.alias_of.is_none() {
            sqlx::query("INSERT INTO workflow_agreement_semantics VALUES(?,?,?,?,?,?)")
                .bind(&p.store)
                .bind(&p.tenant)
                .bind(&p.environment)
                .bind(&o.kind)
                .bind(&p.semantic_key)
                .bind(&o.identity)
                .execute(self.conn())
                .await?;
        }
        let mut members = std::collections::BTreeSet::new();
        for r in &o.members {
            if !members.insert((&r.kind, &r.id, &r.digest)) {
                return Err(StoreError::Integrity("duplicate prepared agreement member"));
            }
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
