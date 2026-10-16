//! Borrowed mechanical write operands from the private coordinator. Owning
//! artifacts and their resource session remain alive in that coordinator.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{EconomicCandidate, ExactRef};
use serde_json::Value;

pub(crate) struct FinancialDocument<'a> {
    pub reference: &'a ExactRef,
    pub schema: &'a str,
    pub canonical_bytes: &'a [u8],
}
pub(crate) struct FinancialJournal<'a> {
    pub identity: &'a [u8],
    pub kind: &'a str,
    pub operation_ref: &'a ExactRef,
    pub operation_bytes: &'a [u8],
    pub request_hash: &'a str,
    pub manifest_ref: &'a ExactRef,
    pub manifest_bytes: &'a [u8],
    pub receipt_ref: &'a ExactRef,
    pub receipt_bytes: &'a [u8],
    /// Borrowed from the owning, closed canonical manifest; no cloned Ref set.
    pub members: &'a [Value],
    pub sequence: i64,
    pub accepted_at_us: i64,
}
/// Neither a financial authority nor a public append surface. The service's
/// private prepared commit must own the authenticated checked snapshot, the
/// complete core candidate and all of these original/canonical byte artifacts.
pub(crate) struct FinancialAppend<'a> {
    pub candidate: &'a EconomicCandidate,
    pub documents: &'a [FinancialDocument<'a>],
    pub journal: FinancialJournal<'a>,
}
fn member(value: &Value) -> Result<(&str, &str, &str), StoreError> {
    let object = value
        .as_object()
        .ok_or(StoreError::Integrity("financial member object"))?;
    if object.len() != 3 {
        return Err(StoreError::Integrity("financial member fields"));
    }
    let text = |key| {
        object
            .get(key)
            .and_then(Value::as_str)
            .ok_or(StoreError::Integrity("financial member reference"))
    };
    Ok((text("kind")?, text("id")?, text("digest")?))
}
fn extra(total: i64, amount: usize) -> Result<i64, StoreError> {
    total
        .checked_add(i64::try_from(amount).map_err(|_| StoreError::BillingHistoryLimit)?)
        .ok_or(StoreError::BillingHistoryLimit)
}
impl SqliteTx {
    /// Same writer/poison/unknown-commit boundary as the administrative journal.
    /// No canonical payload is copied into an ordinary host Vec by this method.
    pub(crate) async fn workflow_financial_append(
        &mut self,
        prepared: &FinancialAppend<'_>,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::Integrity("poisoned financial transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.financial_append_inner(prepared))
            .await
            .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    async fn financial_append_inner(&mut self, p: &FinancialAppend<'_>) -> Result<(), StoreError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if version != 13 {
            return Err(StoreError::BillingUpgradeRequired);
        }
        let key = p.candidate.key();
        let o = &p.journal;
        if !matches!(
            o.kind,
            "work.accept"
                | "work.reverse"
                | "quantity.correct"
                | "outcome.submit"
                | "outcome.correct"
                | "outcome.close"
                | "link.assert"
        ) || o.operation_ref.kind != "workflow-operation"
            || o.manifest_ref.kind != "workflow-manifest"
            || o.receipt_ref.kind != "workflow-receipt"
            || o.identity.is_empty()
            || o.identity.len() > 262144
            || o.operation_bytes.is_empty()
            || o.operation_bytes.len() > 8388608
            || o.manifest_bytes.is_empty()
            || o.manifest_bytes.len() > 4194304
            || o.receipt_bytes.is_empty()
            || o.receipt_bytes.len() > 1048576
            || p.documents.len() > 4095
            || o.members.is_empty()
            || o.members.len() > 4096
        {
            return Err(StoreError::Integrity("financial prepared framing"));
        }
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=?)",
        )
        .bind(o.identity)
        .fetch_one(self.conn())
        .await?;
        if occupied {
            return Err(StoreError::ExpectedCurrent);
        }
        let mut additional = 0;
        for bytes in [
            o.identity,
            o.operation_bytes,
            o.manifest_bytes,
            o.receipt_bytes,
        ] {
            additional = extra(additional, bytes.len())?;
        }
        for d in p.documents {
            if d.reference.kind == "workflow-operation"
                || d.canonical_bytes.is_empty()
                || d.canonical_bytes.len() > 4194304
            {
                return Err(StoreError::Integrity("financial prepared document bounds"));
            }
            // SQLite performs the exact comparison; no unleased multi-MiB
            // retained BLOB is fetched into Rust merely to compare it.
            let retained:Option<bool>=sqlx::query_scalar("SELECT kind=? AND digest=? AND schema_name=? AND canonical_bytes=? FROM workflow_documents WHERE id=?")
                .bind(&d.reference.kind).bind(&d.reference.digest).bind(d.schema).bind(d.canonical_bytes).bind(&d.reference.id)
                .fetch_optional(self.conn()).await?;
            match retained {
                Some(false) => {
                    return Err(StoreError::Integrity(
                        "immutable financial document conflict",
                    ));
                }
                Some(true) => {}
                None => {
                    additional = extra(additional, d.canonical_bytes.len())?;
                }
            }
        }
        let before = super::super::capacity::check(self.conn(), 1, additional).await?;
        if before.operations.checked_add(1) != Some(o.sequence) {
            return Err(StoreError::Integrity("financial journal sequence gap"));
        }
        for d in p.documents {
            sqlx::query("INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) SELECT ?,?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM workflow_documents WHERE id=?)")
                .bind(&d.reference.kind).bind(&d.reference.id).bind(&d.reference.digest).bind(d.schema).bind(d.canonical_bytes).bind(&d.reference.id)
                .execute(self.conn()).await?;
        }
        sqlx::query("INSERT INTO workflow_operations(identity_key,logical_store_id,tenant,environment,source,operation_id,operation_record_id,operation_digest,operation_bytes,normalized_request_hash,manifest_id,manifest_digest,manifest_bytes,receipt_id,receipt_digest,receipt_bytes,publication_id,plan_id,commit_sequence,accepted_at_us,operation_kind) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,NULL,NULL,?,?,?)")
            .bind(o.identity).bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
            .bind(&key.source).bind(&key.operation_id).bind(&o.operation_ref.id).bind(&o.operation_ref.digest).bind(o.operation_bytes)
            .bind(o.request_hash).bind(&o.manifest_ref.id).bind(&o.manifest_ref.digest).bind(o.manifest_bytes)
            .bind(&o.receipt_ref.id).bind(&o.receipt_ref.digest).bind(o.receipt_bytes)
            .bind(o.sequence).bind(o.accepted_at_us).bind(o.kind).execute(self.conn()).await?;
        let mut own_operation_members = 0;
        for value in o.members {
            let (kind, id, digest) = member(value)?;
            if kind == "workflow-operation" {
                if id == o.operation_ref.id && digest == o.operation_ref.digest {
                    own_operation_members += 1;
                } else {
                    let original:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND operation_record_id=? AND operation_digest=? AND commit_sequence<?)")
                        .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                        .bind(id).bind(digest).bind(o.sequence).fetch_one(self.conn()).await?;
                    if !original {
                        return Err(StoreError::Integrity(
                            "financial operation member not an earlier scoped original",
                        ));
                    }
                }
            } else {
                let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents WHERE kind=? AND id=? AND digest=?)")
                    .bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
                if !present {
                    return Err(StoreError::Integrity("financial manifest missing document"));
                }
            }
            // The primary key rejects repeated full Refs, inside this same
            // poisoned transaction. No unleased host BTree/Ref clone is made.
            sqlx::query("INSERT INTO workflow_manifest_members VALUES(?,?,?,?)")
                .bind(o.identity)
                .bind(kind)
                .bind(id)
                .bind(digest)
                .execute(self.conn())
                .await?;
        }
        if own_operation_members != 1 {
            return Err(StoreError::Integrity(
                "financial operation manifest membership",
            ));
        }
        for r in p.candidate.consumed_refs() {
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members WHERE identity_key=? AND kind=? AND id=? AND digest=?)")
                .bind(o.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).fetch_one(self.conn()).await?;
            if !present {
                return Err(StoreError::Integrity(
                    "financial consumed reference absent from manifest",
                ));
            }
        }
        for d in p.documents {
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members WHERE identity_key=? AND kind=? AND id=? AND digest=?)")
                .bind(o.identity).bind(&d.reference.kind).bind(&d.reference.id).bind(&d.reference.digest).fetch_one(self.conn()).await?;
            if !present {
                return Err(StoreError::Integrity(
                    "financial prepared document outside manifest",
                ));
            }
        }
        for d in p.candidate.introduced() {
            let r = d.reference();
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents d JOIN workflow_manifest_members m ON m.kind=d.kind AND m.id=d.id AND m.digest=d.digest WHERE m.identity_key=? AND d.kind=? AND d.id=? AND d.digest=? AND d.schema_name=? AND d.canonical_bytes=?)")
                .bind(o.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).bind(d.schema()).bind(d.canonical_body()).fetch_one(self.conn()).await?;
            if !present {
                return Err(StoreError::Integrity(
                    "financial introduced result absent or changed",
                ));
            }
        }
        self.apply_financial_effects(p.candidate, o.identity, o.sequence)
            .await?;
        sqlx::query("INSERT INTO workflow_feed VALUES(?,?)")
            .bind(o.sequence)
            .bind(o.identity)
            .execute(self.conn())
            .await?;
        let changed=sqlx::query("UPDATE workflow_state SET operation_count=?,payload_bytes=? WHERE singleton=1 AND operation_count=? AND payload_bytes=?")
            .bind(o.sequence).bind(before.payload_bytes.checked_add(additional).ok_or(StoreError::BillingHistoryLimit)?)
            .bind(before.operations).bind(before.payload_bytes).execute(self.conn()).await?.rows_affected();
        if changed != 1 {
            return Err(StoreError::ExpectedCurrent);
        }
        // Exact counted final state catches duplicate preflight documents and
        // any mismatch before this append may clear its poison bit.
        super::super::capacity::check(self.conn(), 0, 0).await?;
        Ok(())
    }
}
