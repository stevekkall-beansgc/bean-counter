//! Actual scoped party mapping projections. A hit/CAS is mechanical DATA;
//! original operator custody and exact assent remain coordinator obligations.
use super::effects::{head_text, same_ref, same_text};
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;
const LENGTHS: &str = "SELECT octet_length(revision),octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_party_authorization_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=? AND agreement_id=? AND authorization_id=?";
const VALUES: &str = "SELECT revision,record_kind,record_id,record_digest FROM workflow_party_authorization_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=? AND agreement_id=? AND authorization_id=?";
pub(crate) struct PartyHeadKey<'a> {
    pub store: &'a str,
    pub scope: &'a [String; 2],
    pub customer: &'a str,
    pub agreement: &'a str,
    pub authorization_id: &'a str,
}
impl<'a> PartyHeadKey<'a> {
    fn parts(&self) -> [&'a str; 6] {
        [
            self.store,
            &self.scope[0],
            &self.scope[1],
            self.customer,
            self.agreement,
            self.authorization_id,
        ]
    }
}
/// Borrowed output from actual host construction, not a public prepared commit.
/// The fixed lifecycle has only initial revision1 or exact terminal revision2.
pub(crate) struct PartyHeadEffect<'a> {
    pub key: PartyHeadKey<'a>,
    pub expected: Option<&'a ExactRef>,
    pub proposed: &'a ExactRef,
}
/// Mechanical immutable journal plus optional exact party-mapping CAS. The
/// private coordinator owns original custody/currentness/resource eligibility.
pub(crate) struct PartyAppend<'a> {
    pub logical_store_id: &'a str,
    pub scope: &'a [String; 2],
    pub source: &'a str,
    pub operation_id: &'a str,
    pub documents: &'a [super::append::FinancialDocument<'a>],
    pub journal: super::append::FinancialJournal<'a>,
    pub head: Option<PartyHeadEffect<'a>>,
}
impl SqliteTx {
    /// SAME held poisoned writer owns journal, projection and Profile-B delta.
    /// No DATA caller may use this instead of complete private preparation.
    pub(crate) async fn workflow_party_append(
        &mut self,
        p: &PartyAppend<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned party append").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.party_append_inner(p, session),
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
                return Err(StoreError::Integrity("party append retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    async fn party_append_inner(
        &mut self,
        p: &PartyAppend<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if !matches!(p.journal.kind, "authority.change" | "evidence.retain") {
            return Err(StoreError::Integrity("party append journal kind").into());
        }
        if p.head.is_none() && !same_text(p.journal.kind, "evidence.retain", session)? {
            return Err(StoreError::Integrity("party authority append lacks mapping CAS").into());
        }
        let key = super::append::JournalKey {
            logical_store_id: p.logical_store_id,
            scope: p.scope,
            source: p.source,
            operation_id: p.operation_id,
        };
        let delta = self
            .append_journal_rows(key, p.documents, &p.journal, session)
            .await?;
        if let Some(head) = &p.head {
            if !same_text(p.journal.kind, "authority.change", session)? {
                return Err(StoreError::Integrity("party evidence cannot advance mapping").into());
            }
            let mut matches = 0i64;
            for document in p.documents {
                session.semantic_work().visit(ValidationVisit::Element)?;
                if same_ref(document.reference, head.proposed, session)? {
                    if !same_text(
                        document.schema,
                        "ledger-workflow-party-authorization/1",
                        session,
                    )? {
                        return Err(StoreError::Integrity("party mapping schema").into());
                    }
                    matches = resource_add(session, matches, 1)?;
                }
            }
            if !resource_compare(session, matches, 1)?.is_eq() {
                return Err(
                    StoreError::Integrity("party mapping outside exact new documents").into(),
                );
            }
            self.party_advance_inner(p.journal.identity, head, session)
                .await?;
        }
        self.finish_journal_rows(&p.journal, delta, session).await
    }
    pub(crate) async fn workflow_party_authorization_head(
        &mut self,
        key: &PartyHeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned party head observation").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.party_head_inner(key, session),
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
                return Err(StoreError::Integrity("party head retained failed worker").into());
            }
            self.failed = false;
        }
        result
    }
    async fn party_head_inner(
        &mut self,
        key: &PartyHeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        session.check_cancelled()?;
        let io = io::binding_payload(&[], &[], 0, 0, session)?;
        self.financial_reserve_io(session, io)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<i64>())?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        self.financial_finish_io(session).await?;
        if !resource_compare(session, version, 15)?.is_eq()
            && !resource_compare(session, version, 16)?.is_eq()
            && !resource_compare(session, version, 17)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        for value in key.parts() {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            if !resource_compare(session, value.len() as i64, 0)?.is_gt()
                || resource_compare(session, value.len() as i64, 128)?.is_gt()
            {
                return Err(StoreError::Integrity("party head identity length").into());
            }
            session.semantic_work().scan_bytes(value.len())?;
            for c in value.chars() {
                session.semantic_work().visit(ValidationVisit::Constraint)?;
                if c.is_control() {
                    return Err(StoreError::Integrity("party head identity control").into());
                }
            }
        }
        self.read_financial_head_projection(LENGTHS, VALUES, key.parts(), session)
            .await
    }
    /// Called only after the new operation and exact proposed member have been
    /// appended in this SAME held writer. Failure poisons the whole transaction.
    pub(crate) async fn workflow_party_advance(
        &mut self,
        identity: &[u8],
        effect: &PartyHeadEffect<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned party transition").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.party_advance_inner(identity, effect, session),
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
                return Err(
                    StoreError::Integrity("party transition retained failed worker").into(),
                );
            }
            self.failed = false;
        }
        result
    }
    async fn party_advance_inner(
        &mut self,
        identity: &[u8],
        effect: &PartyHeadEffect<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if !same_text(&effect.proposed.kind, "workflow-authority", session)? {
            return Err(StoreError::Integrity("party authority kind").into());
        }
        self.financial_member(identity, effect.proposed, session)
            .await?;
        let prior = self.party_head_inner(&effect.key, session).await?;
        let parts = effect.key.parts();
        let changed = match (prior, effect.expected) {
            (None, None) => {
                let values = [
                    parts[0],
                    parts[1],
                    parts[2],
                    parts[3],
                    parts[4],
                    parts[5],
                    "1",
                    &effect.proposed.kind,
                    &effect.proposed.id,
                    &effect.proposed.digest,
                ];
                let bytes = values.map(str::as_bytes);
                let payload = io::binding_payload_sets(&bytes, &[&bytes, &bytes], 2, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                let mut q = sqlx::query(
                    "INSERT INTO workflow_party_authorization_heads VALUES(?,?,?,?,?,?,?,?,?,?)",
                );
                for v in values {
                    session.semantic_work().visit(ValidationVisit::Element)?;
                    q = q.bind(v);
                }
                let result = q.execute(self.conn()).await?;
                let changed = result.rows_affected();
                self.financial_finish_io(session).await?;
                changed
            }
            (Some(prior), Some(expected)) => {
                if !same_text(head_text(&prior.revision, session)?, "1", session)?
                    || !same_text(head_text(&prior.kind, session)?, &expected.kind, session)?
                    || !same_text(head_text(&prior.id, session)?, &expected.id, session)?
                    || !same_text(
                        head_text(&prior.digest, session)?,
                        &expected.digest,
                        session,
                    )?
                    || same_ref(expected, effect.proposed, session)?
                {
                    return Err(StoreError::ExpectedCurrent.into());
                }
                let values = [
                    &effect.proposed.kind,
                    &effect.proposed.id,
                    &effect.proposed.digest,
                    parts[0],
                    parts[1],
                    parts[2],
                    parts[3],
                    parts[4],
                    parts[5],
                    &expected.kind,
                    &expected.id,
                    &expected.digest,
                ];
                let bytes = values.map(str::as_bytes);
                let payload = io::binding_payload_sets(&bytes, &[&bytes], 1, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                let mut q = sqlx::query(
                    "UPDATE workflow_party_authorization_heads SET revision='2',record_kind=?,record_id=?,record_digest=? WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=? AND agreement_id=? AND authorization_id=? AND revision='1' AND record_kind=? AND record_id=? AND record_digest=?",
                );
                for v in values {
                    session.semantic_work().visit(ValidationVisit::Element)?;
                    q = q.bind(v);
                }
                let result = q.execute(self.conn()).await?;
                let changed = result.rows_affected();
                self.financial_finish_io(session).await?;
                changed
            }
            _ => return Err(StoreError::ExpectedCurrent.into()),
        };
        if !resource_compare(session, changed as i64, 1)?.is_eq() {
            return Err(StoreError::ExpectedCurrent.into());
        }
        Ok(())
    }
}
