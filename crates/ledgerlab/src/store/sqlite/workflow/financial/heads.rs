//! Locked projection locators only. Every returned reference/revision must be
//! authenticated against the chronological prefix and immutable head body.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::CostLineKey;

pub(crate) enum HeadKey<'a> {
    Decision(&'a str),
    Family {
        target_kind: &'a str,
        target_id: &'a str,
        target_digest: &'a str,
        slot: &'a str,
    },
    Chain {
        customer: &'a str,
        chain: &'a str,
    },
    Capacity(&'a str),
    Invocation(&'a str),
    Cost(&'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef),
    CostConsumption(&'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef),
    CostOwner(&'a CostLineKey),
}
impl<'a> HeadKey<'a> {
    fn parts(&self) -> Result<(&'static str, [&'a str; 4]), FinancialReadError> {
        Ok(match self {
            Self::Decision(id) => ("decision", [id, "", "", ""]),
            Self::Family {
                target_kind,
                target_id,
                target_digest,
                slot,
            } => ("family", [target_kind, target_id, target_digest, slot]),
            Self::Chain { customer, chain } => ("chain", [customer, chain, "", ""]),
            Self::Capacity(id) => ("capacity", [id, "", "", ""]),
            Self::Invocation(id) => ("invocation", [id, "", "", ""]),
            Self::Cost(r) => ("cost", [&r.kind, &r.id, &r.digest, ""]),
            Self::CostConsumption(r) => ("cost_consumption", [&r.kind, &r.id, &r.digest, ""]),
            Self::CostOwner(_) => return Err(invalid()),
        })
    }
}
/// No ordinary String/Ref clones, no authority token and no naked row body.
/// These artifacts remain in the attempt arena until the caller drops them.
pub(crate) struct HeadLocator {
    pub revision: AuditedBytes,
    pub kind: AuditedBytes,
    pub id: AuditedBytes,
    pub digest: AuditedBytes,
}
const LENGTHS: &str = "SELECT length(revision),length(record_kind),length(record_id),length(record_digest) FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=?";
const VALUES: &str = "SELECT revision,record_kind,record_id,record_digest FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=?";
fn bind<'q>(
    sql: &'static str,
    bindings: &[&'q str],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for binding in bindings {
        query = query.bind(*binding);
    }
    query
}
fn locator_text(
    value: &str,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<(), FinancialReadError> {
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    if resource_compare(session, value.len() as i64, maximum as i64)?.is_gt() {
        return Err(invalid());
    }
    // This is only the already-declared key representation bound. Actual
    // subject, original identity and currentness are not established here.
    session.semantic_work().scan_bytes(value.len())?;
    for character in value.chars() {
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if character.is_control() {
            return Err(invalid());
        }
    }
    Ok(())
}
impl SqliteTx {
    pub(crate) async fn workflow_financial_head(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        let was_failed = self.failed;
        if was_failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned financial head observation").into());
        }
        self.failed = true;
        let result = timeout_at(
            self.deadline,
            self.financial_head_inner(store, scope, key, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            // An Ok SQL result cannot erase a failed/cancelled child, a caught
            // proof error or a worker payload still awaiting finalization.
            // The executing guard uses this SAME original session lifecycle.
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("financial completion retains poisoned work").into(),
                );
            }
            self.failed = was_failed;
        }
        result
    }
    pub(super) async fn financial_head_during_append(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        if !self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("financial head read outside active append").into());
        }
        self.financial_head_inner(store, scope, key, session).await
    }
    async fn financial_head_inner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        locator_text(store, 128, session)?;
        locator_text(&scope[0], 128, session)?;
        locator_text(&scope[1], 128, session)?;
        session.check_cancelled()?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(session, version, 13)?.is_eq()
            && !resource_compare(session, version, 14)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        if let HeadKey::CostOwner(line) = key {
            if !resource_compare(session, version, 14)?.is_eq() {
                return Err(StoreError::BillingUpgradeRequired.into());
            }
            let account = line.provider_account_identity();
            let bindings = [
                line.cost_authority_id(),
                account.provider_namespace(),
                account.provider_account_id(),
                account.environment(),
                line.original_document_id(),
                line.original_line_id(),
            ];
            for value in bindings {
                locator_text(value, 128, session)?;
            }
            return self.read_financial_head_projection(
                "SELECT length(revision),length(record_kind),length(record_id),length(record_digest) FROM workflow_supplier_cost_owner_heads WHERE cost_authority_id=? AND provider_namespace=? AND provider_account_id=? AND provider_environment=? AND original_document_id=? AND original_line_id=?",
                "SELECT revision,record_kind,record_id,record_digest FROM workflow_supplier_cost_owner_heads WHERE cost_authority_id=? AND provider_namespace=? AND provider_account_id=? AND provider_environment=? AND original_document_id=? AND original_line_id=?",
                bindings,session).await;
        }
        let (kind, parts) = key.parts()?;
        for part in parts {
            locator_text(part, 128, session)?;
        }
        self.read_financial_head_projection(
            LENGTHS,
            VALUES,
            [
                store, &scope[0], &scope[1], kind, parts[0], parts[1], parts[2], parts[3],
            ],
            session,
        )
        .await
    }
    async fn read_financial_head_projection<const N: usize>(
        &mut self,
        length_sql: &'static str,
        value_sql: &'static str,
        bindings: [&str; N],
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        let bytes = bindings.map(str::as_bytes);
        let payload = io::binding_payload(&bytes, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let lengths = bind(length_sql, &bindings)
            .fetch_optional(self.conn())
            .await?;
        let Some(lengths) = lengths else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        for (position, maximum) in [(0usize, 20i64), (1, 48), (2, 104), (3, 71)] {
            session.semantic_work().visit(ValidationVisit::Field)?;
            let n: i64 = lengths.try_get(position)?;
            if !resource_compare(session, n, 0)?.is_gt()
                || resource_compare(session, n, maximum)?.is_gt()
            {
                return Err(invalid());
            }
        }
        drop(lengths);
        self.financial_finish_io(session).await?;
        // Finalize the length probe before the value query, so one statement's
        // known input payload and at most two copies of the four probed text
        // fields are the request-scaled live payload for this lease.
        let payload = io::binding_payload(&bytes, &[], 0, 2 * (20 + 48 + 104 + 71), session)?;
        self.financial_reserve_io(session, payload)?;
        let row = bind(value_sql, &bindings)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        let revision = session.copy_representation(
            row.try_get::<&str, _>(0)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let record_kind = session.copy_representation(
            row.try_get::<&str, _>(1)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let id = session.copy_representation(
            row.try_get::<&str, _>(2)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let digest = session.copy_representation(
            row.try_get::<&str, _>(3)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        // Returned artifacts now have their own reservations. The source row
        // is destroyed before cache finalization releases its worker lease.
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(HeadLocator {
            revision,
            kind: record_kind,
            id,
            digest,
        }))
    }
}
