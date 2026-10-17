//! Actual locked source projection, with byte probes before TEXT fetch. This
//! returns owning locators only; the caller/core authenticate immutable state
//! and original/current lineage under the same writer.
use super::*;
const PROBE: &str = "SELECT CASE WHEN typeof(h.revision)='integer' THEN h.revision ELSE NULL END,octet_length(h.state_id),octet_length(h.grant_id),octet_length(h.status),octet_length(d.kind),octet_length(d.digest) FROM workflow_source_heads h LEFT JOIN workflow_documents d ON d.id=h.state_id WHERE h.logical_store_id=? AND h.tenant=? AND h.environment=? AND h.enrollment_id=?";
const VALUE: &str = "SELECT h.revision,h.state_id,h.grant_id,h.status,d.kind,d.digest FROM workflow_source_heads h LEFT JOIN workflow_documents d ON d.id=h.state_id WHERE h.logical_store_id=? AND h.tenant=? AND h.environment=? AND h.enrollment_id=?";
pub(crate) struct SourceHeadLocator {
    pub revision: i64,
    pub state_id: AuditedBytes,
    pub grant_id: Option<AuditedBytes>,
    pub status: AuditedBytes,
    pub kind: AuditedBytes,
    pub digest: AuditedBytes,
}
fn query<'a>(
    sql: &'static str,
    args: &[&'a str; 4],
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    sqlx::query(sql)
        .bind(args[0])
        .bind(args[1])
        .bind(args[2])
        .bind(args[3])
}
impl SqliteTx {
    pub(crate) async fn workflow_financial_source_head(
        &mut self,
        store: &str,
        scope: &[String; 2],
        enrollment: &str,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<SourceHeadLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned source head observation").into());
        }
        self.failed = true;
        let result = timeout_at(
            self.deadline,
            self.financial_source_head_inner(store, scope, enrollment, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("unfinished source head observation").into());
            }
            self.failed = false;
        }
        result
    }
    async fn financial_source_head_inner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        enrollment: &str,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<SourceHeadLocator>, FinancialReadError> {
        for input in [store, &scope[0], &scope[1], enrollment] {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            if !resource_compare(session, input.len() as i64, 0)?.is_gt()
                || resource_compare(session, input.len() as i64, 128)?.is_gt()
            {
                return Err(invalid());
            }
            session.semantic_work().scan_bytes(input.len())?;
            for c in input.chars() {
                session.semantic_work().visit(ValidationVisit::Constraint)?;
                if c.is_control() {
                    return Err(invalid());
                }
            }
        }
        session.check_cancelled()?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(session, version, 13)?.is_eq()
            && !resource_compare(session, version, 14)?.is_eq()
            && !resource_compare(session, version, 15)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let args = [store, scope[0].as_str(), scope[1].as_str(), enrollment];
        let bytes = args.map(str::as_bytes);
        let payload = io::binding_payload(&bytes, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let probe = query(PROBE, &args).fetch_optional(self.conn()).await?;
        let Some(probe) = probe else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        let revision: i64 = probe.try_get(0)?;
        if !resource_compare(session, revision, 0)?.is_gt() {
            return Err(invalid());
        }
        for (position, maximum, optional) in [
            (1usize, 104i64, false),
            (2, 104, true),
            (3, 7, false),
            (4, 48, false),
            (5, 71, false),
        ] {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let size: Option<i64> = probe.try_get(position)?;
            let Some(size) = size else {
                session.semantic_work().visit(ValidationVisit::Constraint)?;
                if optional {
                    continue;
                } else {
                    return Err(invalid());
                }
            };
            session.semantic_work().arithmetic(
                ArithmeticOperation::Multiply,
                &[&maximum.into(), &2i64.into()],
            )?;
            if !resource_compare(session, size, 0)?.is_gt()
                || resource_compare(session, size, maximum * 2)?.is_gt()
            {
                return Err(invalid());
            }
        }
        drop(probe);
        self.financial_finish_io(session).await?;
        // SAME pinned 8M per-TEXT UTF16 conversion/statement/result overlap as
        // the existing head reader. Per-cell headers/terminators are covered by
        // its fixed I/O metadata lease; no full-process RSS claim is made.
        const RESULT: usize = 8 * (104 + 104 + 7 + 48 + 71);
        session.semantic_work().scan_bytes(RESULT)?;
        session.semantic_work().write_bytes(RESULT)?;
        let payload = io::binding_payload(&bytes, &[], 0, RESULT, session)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let row = query(VALUE, &args)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        let found: i64 = row.try_get(0)?;
        if !resource_compare(session, found, revision)?.is_eq() {
            return Err(invalid());
        }
        for (position, maximum, optional) in [
            (1usize, 104i64, false),
            (2, 104, true),
            (3, 7, false),
            (4, 48, false),
            (5, 71, false),
        ] {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let value: Option<&str> = row.try_get(position)?;
            let Some(value) = value else {
                session.semantic_work().visit(ValidationVisit::Constraint)?;
                if optional {
                    continue;
                } else {
                    return Err(invalid());
                }
            };
            if !resource_compare(session, value.len() as i64, 0)?.is_gt()
                || resource_compare(session, value.len() as i64, maximum)?.is_gt()
            {
                return Err(invalid());
            }
        }
        let state_id = session.copy_representation(
            row.try_get::<&str, _>(1)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let grant: Option<&str> = row.try_get(2)?;
        let grant_id = grant
            .map(|x| {
                session.copy_representation(
                    x.as_bytes(),
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
            })
            .transpose()?;
        let status = session.copy_representation(
            row.try_get::<&str, _>(3)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let kind = session.copy_representation(
            row.try_get::<&str, _>(4)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let digest = session.copy_representation(
            row.try_get::<&str, _>(5)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(SourceHeadLocator {
            revision,
            state_id,
            grant_id,
            status,
            kind,
            digest,
        }))
    }
}
