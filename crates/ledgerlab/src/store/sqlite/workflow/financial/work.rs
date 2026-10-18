//! Exact permanent WorkClaim tuple lookup under the current writer. The row is
//! DATA until its immutable original and scoped index association are checked.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::PermanentClaimKey;
const LENGTHS: &str = "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_financial_work_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND business_operation_id=? AND completion=?";
const VALUES: &str = "SELECT record_kind,record_id,record_digest FROM workflow_financial_work_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND business_operation_id=? AND completion=?";
impl SqliteTx {
    pub(crate) async fn workflow_work_claim(
        &mut self,
        key: &PermanentClaimKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned work claim read").into());
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.work_claim_inner(key, s))
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
                return Err(StoreError::Integrity("work claim read retained worker").into());
            }
            self.failed = false;
        }
        result
    }
    async fn work_claim_inner(
        &mut self,
        key: &PermanentClaimKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        let PermanentClaimKey::Work {
            logical_store_id,
            scope,
            source_namespace,
            business_operation_id,
            completion,
        } = key
        else {
            return Err(StoreError::Integrity("non-work permanent key").into());
        };
        for (value, maximum) in [
            (logical_store_id.as_str(), 128usize),
            (scope[0].as_str(), 128),
            (scope[1].as_str(), 128),
            (source_namespace.as_str(), 256),
            (business_operation_id.as_str(), 128),
            (completion.as_str(), 128),
        ] {
            heads::locator_text(value, maximum, s)?;
        }
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(s, version, 13)?.is_eq()
            && !resource_compare(s, version, 14)?.is_eq()
            && !resource_compare(s, version, 15)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let values = [
            logical_store_id.as_str(),
            scope[0].as_str(),
            scope[1].as_str(),
            source_namespace.as_str(),
            business_operation_id.as_str(),
            completion.as_str(),
        ];
        let bytes = values.map(str::as_bytes);
        self.read_financial_claim_projection(LENGTHS, VALUES, &values, &bytes, s)
            .await
    }
}
