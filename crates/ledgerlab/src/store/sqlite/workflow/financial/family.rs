//! Held locator by stable Target ID before its new body/digest exists. Any
//! returned full Target/head Ref is actual projection DATA, never a candidate
//! target or authority. Multiple matches are integrity failure, never latest.
use super::*;
use heads::{HeadKey, HeadLocator, locator_text};
pub(crate) struct FamilyIdentityHead {
    pub target_kind: AuditedBytes,
    pub target_id: AuditedBytes,
    pub target_digest: AuditedBytes,
    pub head: HeadLocator,
}
const COUNT: &str = "SELECT count(*) FROM (SELECT 1 FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind='family' AND key_a='workflow-target' AND key_b=? AND key_d=? LIMIT 2)";
const LENGTH: &str = "SELECT octet_length(key_c) FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind='family' AND key_a='workflow-target' AND key_b=? AND key_d=?";
const VALUE: &str = "SELECT key_c FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind='family' AND key_a='workflow-target' AND key_b=? AND key_d=?";
fn bind<'q>(
    sql: &'static str,
    values: &[&'q str],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for v in values {
        query = query.bind(*v);
    }
    query
}
impl SqliteTx {
    pub(crate) async fn workflow_family_head_by_target_identity(
        &mut self,
        store: &str,
        scope: &[String; 2],
        target_id: &str,
        slot: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<FamilyIdentityHead>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned family identity read").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.family_identity_inner(store, scope, target_id, slot, s),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("family identity read retained worker").into());
            }
            self.failed = false;
        }
        result
    }
    async fn family_identity_inner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        target_id: &str,
        slot: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<FamilyIdentityHead>, FinancialReadError> {
        for v in [store, &scope[0], &scope[1], target_id, slot] {
            locator_text(v, 128, s)?;
        }
        s.check_cancelled()?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(s, version, 13)?.is_eq()
            && !resource_compare(s, version, 14)?.is_eq()
            && !resource_compare(s, version, 15)?.is_eq()
            && !resource_compare(s, version, 16)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let bindings = [store, &scope[0], &scope[1], target_id, slot];
        let bytes = bindings.map(str::as_bytes);
        s.semantic_work().scan_bytes(8)?;
        s.semantic_work().write_bytes(8)?;
        let payload = io::binding_payload(&bytes, &[], 0, 8, s)?;
        self.financial_reserve_io(s, payload)?;
        let row = bind(COUNT, &bindings).fetch_one(self.conn()).await?;
        let count: i64 = row.try_get(0)?;
        drop(row);
        self.financial_finish_io(s).await?;
        if resource_compare(s, count, 0)?.is_eq() {
            return Ok(None);
        }
        if !resource_compare(s, count, 1)?.is_eq() {
            return Err(StoreError::Integrity("ambiguous family target identity").into());
        }
        s.semantic_work().scan_bytes(8)?;
        s.semantic_work().write_bytes(8)?;
        let payload = io::binding_payload(&bytes, &[], 0, 8, s)?;
        self.financial_reserve_io(s, payload)?;
        let row = bind(LENGTH, &bindings).fetch_one(self.conn()).await?;
        let length: i64 = row.try_get(0)?;
        if !resource_compare(s, length, 0)?.is_gt() || resource_compare(s, length, 142)?.is_gt() {
            return Err(StoreError::Integrity("family target digest byte preflight").into());
        }
        drop(row);
        self.financial_finish_io(s).await?;
        // SAME direct-column byte probe and UTF16 conversion/result overlap
        // reservation as existing Ref/head readers; original UTF8 cap remains71.
        s.semantic_work().scan_bytes(8 * 71)?;
        s.semantic_work().write_bytes(8 * 71)?;
        let payload = io::binding_payload(&bytes, &[], 0, 8 * 71, s)?;
        self.financial_reserve_io(s, payload)?;
        let row = bind(VALUE, &bindings).fetch_one(self.conn()).await?;
        let text: &str = row.try_get(0)?;
        if !resource_compare(s, text.len() as i64, 0)?.is_gt()
            || resource_compare(s, text.len() as i64, 71)?.is_gt()
        {
            return Err(StoreError::Integrity("family target digest UTF8 cap").into());
        }
        let digest = s.copy_representation(
            text.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        drop(row);
        self.financial_finish_io(s).await?;
        s.semantic_work().scan_bytes(digest.bytes().len())?;
        let text = std::str::from_utf8(digest.bytes())
            .map_err(|_| StoreError::Integrity("family target digest encoding"))?;
        let head = self
            .financial_head_inner(
                store,
                scope,
                HeadKey::Family {
                    target_kind: "workflow-target",
                    target_id,
                    target_digest: text,
                    slot,
                },
                s,
            )
            .await?
            .ok_or_else(|| StoreError::Integrity("family target vanished in held snapshot"))?;
        let target_kind = s.copy_representation(
            b"workflow-target",
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let target_id = s.copy_representation(
            target_id.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        Ok(Some(FamilyIdentityHead {
            target_kind,
            target_id,
            target_digest: digest,
            head,
        }))
    }
}
