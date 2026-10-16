//! Mechanical supplier projections from the complete candidate. Account and
//! provider-line identities are installation-wide; financial bodies, rights,
//! conservation and high-water derivation remain core/coordinator obligations.
use super::effects::{head_text, next_revision, revision, same_ref, same_text};
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    AccountEnrollmentEffect, CostLineKey, EconomicCandidate, ExactRef, HeadEffect,
};

fn line_parts(line: &CostLineKey) -> [&str; 6] {
    let a = line.provider_account_identity();
    [
        line.cost_authority_id(),
        a.provider_namespace(),
        a.provider_account_id(),
        a.environment(),
        line.original_document_id(),
        line.original_line_id(),
    ]
}
fn bind<'a>(
    sql: &'static str,
    values: &[&'a str],
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for value in values {
        query = query.bind(*value);
    }
    query
}
impl SqliteTx {
    pub(super) async fn insert_supplier_line_claim(
        &mut self,
        _identity: &[u8],
        line: &CostLineKey,
        r: &ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if !same_text(&r.kind, "workflow-cost", session)? {
            return Err(StoreError::Integrity("supplier claim kind").into());
        }
        let p = line_parts(line);
        let values = [
            p[0], p[1], p[2], p[3], p[4], p[5], &r.kind, &r.id, &r.digest,
        ];
        let bytes = values.map(str::as_bytes);
        let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes, &bytes], 3, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        bind(
            "INSERT INTO workflow_supplier_cost_line_claims VALUES(?,?,?,?,?,?,?,?,?)",
            &values,
        )
        .execute(self.conn())
        .await?;
        self.financial_finish_io(session).await
    }
    pub(super) async fn insert_supplier_component_claim(
        &mut self,
        candidate: &EconomicCandidate,
        scope: &[String; 2],
        work: &str,
        slot: &str,
        r: &ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let key = candidate.key();
        if !same_text(&scope[0], &key.scope[0], session)?
            || !same_text(&scope[1], &key.scope[1], session)?
            || !same_text(&r.kind, "workflow-cost", session)?
        {
            return Err(StoreError::Integrity("supplier component scope/kind").into());
        }
        let values = [
            key.logical_store_id.as_str(),
            &scope[0],
            &scope[1],
            work,
            slot,
            &r.kind,
            &r.id,
            &r.digest,
        ];
        let bytes = values.map(str::as_bytes);
        let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes, &bytes], 3, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        bind(
            "INSERT INTO workflow_supplier_cost_component_claims VALUES(?,?,?,?,?,?,?,?)",
            &values,
        )
        .execute(self.conn())
        .await?;
        self.financial_finish_io(session).await
    }
    pub(super) async fn insert_supplier_account_enrollment(
        &mut self,
        identity: &[u8],
        effect: &AccountEnrollmentEffect,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let a = effect.account();
        let authority = effect.cost_authority_ref();
        let activation = effect.owner_activation_ref();
        for r in [authority, activation] {
            if !same_text(&r.kind, "workflow-authority", session)? {
                return Err(StoreError::Integrity("supplier account authority kind").into());
            }
            self.financial_member(identity, r, session).await?;
        }
        let values = [
            a.provider_namespace(),
            a.provider_account_id(),
            a.environment(),
            &authority.kind,
            &authority.id,
            &authority.digest,
            &activation.kind,
            &activation.id,
            &activation.digest,
        ];
        let bytes = values.map(str::as_bytes);
        let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes], 2, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        bind(
            "INSERT INTO workflow_supplier_account_enrollments VALUES(?,?,?,?,?,?,?,?,?)",
            &values,
        )
        .execute(self.conn())
        .await?;
        self.financial_finish_io(session).await
    }
    pub(super) async fn advance_supplier_owner(
        &mut self,
        candidate: &EconomicCandidate,
        effect: &HeadEffect,
        line: &CostLineKey,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let key = candidate.key();
        let r = effect.proposed();
        let next = revision(effect.revision(), session)?;
        let position = effect.introduced_position();
        session.semantic_work().arithmetic(
            ArithmeticOperation::Compare,
            &[&position.into(), &candidate.introduced().len().into()],
        )?;
        let document = candidate
            .introduced()
            .get(position)
            .ok_or(StoreError::Integrity("supplier owner introduced position"))?;
        if !same_text(&r.kind, "workflow-cost", session)?
            || !same_ref(document.reference(), r, session)?
        {
            return Err(StoreError::Integrity("supplier owner not introduced").into());
        }
        let old = self
            .financial_head_during_append(
                &key.logical_store_id,
                &key.scope,
                HeadKey::CostOwner(line),
                session,
            )
            .await?;
        let p = line_parts(line);
        session
            .semantic_work()
            .arithmetic(ArithmeticOperation::Compare, &[&next.into(), &1u64.into()])?;
        match (old, effect.expected()) {
            (None, None) if next == 1 => {
                let values = [
                    p[0],
                    p[1],
                    p[2],
                    p[3],
                    p[4],
                    p[5],
                    effect.revision(),
                    &r.kind,
                    &r.id,
                    &r.digest,
                ];
                let bytes = values.map(str::as_bytes);
                let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes], 2, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                bind(
                    "INSERT INTO workflow_supplier_cost_owner_heads VALUES(?,?,?,?,?,?,?,?,?,?)",
                    &values,
                )
                .execute(self.conn())
                .await?;
                self.financial_finish_io(session).await?;
            }
            (Some(old), Some(expected)) => {
                let old_revision = head_text(&old.revision, session)?;
                if !same_text(head_text(&old.kind, session)?, &expected.kind, session)?
                    || !same_text(head_text(&old.id, session)?, &expected.id, session)?
                    || !same_text(head_text(&old.digest, session)?, &expected.digest, session)?
                    || !next_revision(old_revision, next, session)?
                {
                    return Err(StoreError::ExpectedCurrent.into());
                }
                let values = [
                    effect.revision(),
                    &r.kind,
                    &r.id,
                    &r.digest,
                    p[0],
                    p[1],
                    p[2],
                    p[3],
                    p[4],
                    p[5],
                    old_revision,
                    &expected.kind,
                    &expected.id,
                    &expected.digest,
                ];
                let bytes = values.map(str::as_bytes);
                let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes], 2, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                let changed=bind("UPDATE workflow_supplier_cost_owner_heads SET revision=?,record_kind=?,record_id=?,record_digest=? WHERE cost_authority_id=? AND provider_namespace=? AND provider_account_id=? AND provider_environment=? AND original_document_id=? AND original_line_id=? AND revision=? AND record_kind=? AND record_id=? AND record_digest=?",&values).execute(self.conn()).await?.rows_affected();
                self.financial_finish_io(session).await?;
                session.semantic_work().arithmetic(
                    ArithmeticOperation::Compare,
                    &[&changed.into(), &1u64.into()],
                )?;
                if changed != 1 {
                    return Err(StoreError::ExpectedCurrent.into());
                }
            }
            _ => return Err(StoreError::ExpectedCurrent.into()),
        }
        Ok(())
    }
}
