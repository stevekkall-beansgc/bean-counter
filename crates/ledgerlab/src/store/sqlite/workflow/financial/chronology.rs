//! Chronological journal locators for the held-root auditor. A locator is not
//! an authenticated origin and does not establish an introduction or a head.
//! Rows are deliberately not filtered by their asserted store/scope columns:
//! the recipe reader must validate those copies rather than hide a bad row.
use super::*;

pub(crate) struct OriginalLocator {
    pub identity: AuditedBytes,
    pub sequence: i64,
}

impl SqliteTx {
    /// Return at most one physical journal identity, using the existing unique
    /// sequence index. No complete history, frame, member set or SQL sort is
    /// materialized. Legacy interleaving may leave gaps between workflow rows.
    pub(crate) async fn workflow_original_next(
        &mut self,
        after: i64,
        through: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<OriginalLocator>, FinancialReadError> {
        let prior = self.failed;
        self.failed = true;
        for (left, right, permitted) in [(after, 0, false), (through, 100000, true)] {
            let comparison = resource_compare(session, left, right)?;
            if if permitted {
                comparison.is_gt()
            } else {
                comparison.is_lt()
            } {
                return Err(invalid());
            }
        }
        if resource_compare(session, after, through)?.is_gt() {
            return Err(invalid());
        }
        // SQLite may retain the source key, a transient result and the SQLx
        // result copy. The parsed/canonical key is a separate owning artifact.
        // Only fixed integer arguments enter this probe; the same transaction
        // retains this lease until statement finalization has acknowledged.
        self.financial_reserve_io(session, 3 * 262144)?;
        let result = timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let next: Option<i64> = sqlx::query_scalar(
                "SELECT min(commit_sequence) FROM workflow_operations WHERE commit_sequence>? AND commit_sequence<=?",
            )
            .bind(after)
            .bind(through)
            .fetch_one(self.conn())
            .await?;
            let Some(sequence) = next else { return Ok(None) };
            if !resource_compare(session, sequence, after)?.is_gt()
                || resource_compare(session, sequence, through)?.is_gt()
            {
                return Err(invalid());
            }
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let length: i64 = sqlx::query_scalar(
                "SELECT length(identity_key) FROM workflow_operations WHERE commit_sequence=?",
            )
            .bind(sequence)
            .fetch_one(self.conn())
            .await?;
            if !resource_compare(session, length, 0)?.is_gt()
                || resource_compare(session, length, 262144)?.is_gt()
            {
                return Err(invalid());
            }
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let row = sqlx::query(
                "SELECT identity_key FROM workflow_operations WHERE commit_sequence=?",
            )
            .bind(sequence)
            .fetch_one(self.conn())
            .await?;
            let raw = row.try_get::<&[u8], _>("identity_key")?;
            if !resource_compare(session, raw.len() as i64, length)?.is_eq() {
                return Err(invalid());
            }
            let identity = {
                let (arena, work) = session.parts();
                arena.copy_bytes(raw, work)?
            };
            drop(row);
            // Feed membership is only a mechanical cross-check; neither index
            // is provenance. Full origin semantics still run on the exact key.
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let matches: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM workflow_feed f JOIN workflow_operations o ON o.commit_sequence=f.commit_sequence AND o.identity_key=f.identity_key WHERE o.commit_sequence=?",
            )
            .bind(sequence)
            .fetch_one(self.conn())
            .await?;
            if !resource_compare(session, matches, 1)?.is_eq() {
                return Err(invalid());
            }
            Ok(Some(OriginalLocator { identity, sequence }))
        })
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        self.financial_finish_io(session).await?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
}

impl SqliteTx {
    /// Earliest indexed membership is an untrusted locator only. The caller
    /// still validates the exact original frame and chronological introduction.
    pub(crate) async fn workflow_financial_origin_sequence(
        &mut self,
        reference: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        store: &str,
        scope: &[String; 2],
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<i64>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(invalid());
        }
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            session.check_cancelled()?;
            for (v,max) in [(reference.kind.as_str(),48usize),(reference.id.as_str(),104),(reference.digest.as_str(),71),(store,128),(&scope[0],128),(&scope[1],128)] {
                session.semantic_work().visit(ValidationVisit::Constraint)?;
                if !resource_compare(session,v.len() as i64,0)?.is_gt() || resource_compare(session,v.len() as i64,max as i64)?.is_gt(){return Err(invalid());}
            }
            if !resource_compare(session,before,0)?.is_gt() || resource_compare(session,before,100001)?.is_gt(){return Err(invalid());}
            let bindings=[reference.kind.as_bytes(),reference.id.as_bytes(),reference.digest.as_bytes(),store.as_bytes(),scope[0].as_bytes(),scope[1].as_bytes()];
            let payload=io::binding_payload(&bindings,&[],0,0,session)?;
            self.financial_reserve_io(session,payload)?;
            self.workflow_schema().await?;
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let sequence:Option<i64>=sqlx::query_scalar("SELECT min(o.commit_sequence) FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=? AND m.id=? AND m.digest=? AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?")
                .bind(&reference.kind).bind(&reference.id).bind(&reference.digest).bind(store).bind(&scope[0]).bind(&scope[1]).bind(before).fetch_one(self.conn()).await?;
            if let Some(sequence)=sequence {
                if !resource_compare(session,sequence,0)?.is_gt() || !resource_compare(session,sequence,before)?.is_lt(){return Err(invalid());}
            }
            self.financial_finish_io(session).await?;
            session.check_cancelled()?;
            Ok(sequence)
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(invalid());
            }
            self.failed = false;
        }
        result
    }
}
