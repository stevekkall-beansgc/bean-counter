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
    /// Global identity existence before any fresh source/program selection.
    /// A scalar probe returns no asserted kind/body and copies only the already
    /// owning exact key through the SAME worker argument/ACK boundary.
    pub(crate) async fn workflow_original_identity_exists(
        &mut self,
        identity: &[u8],
        session: &mut OriginalAuditSession,
    ) -> Result<bool, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned original identity lookup").into());
        }
        self.failed = true;
        let payload = io::binding_payload(&[identity], &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
            self.workflow_schema().await?;
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=?)",
            )
            .bind(identity)
            .fetch_one(self.conn())
            .await
            .map_err(FinancialReadError::from)
        })
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        self.financial_finish_io(session).await?;
        if result.is_ok() {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("original identity lookup retained failed work").into(),
                );
            }
            self.failed = false;
        }
        result
    }

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
        let through_max=if session.resource_limits().is_some(){i64::MAX}else{100000};
        for (left, right, permitted) in [(after, 0, false), (through, through_max, true)] {
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
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
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
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
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
        }).await.map_err(|_|FinancialReadError::Store(self.workflow_timeout_error()))?;
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

impl SqliteTx {
    /// Two fresh, immediately unlinked files within the actual private owner
    /// directory. No saved path/cursor/file content is accepted as audit state.
    /// The held writer serializes this one attempt; an existing name refuses
    /// rather than opening or trusting a leftover scratch file.
    pub(crate) fn workflow_original_scratch(
        &mut self,
        session: &mut OriginalAuditSession,
    ) -> Result<[std::fs::File; 2], FinancialReadError> {
        use std::fs::OpenOptions;
        self.store._owner.verify_path()?;
        fn open(
            tx: &mut SqliteTx,
            name: &'static str,
            s: &mut OriginalAuditSession,
        ) -> Result<std::fs::File, FinancialReadError> {
            s.check_cancelled()?;
            let directory = &tx.store._owner.directory;
            let bytes = resource_add(
                s,
                directory.as_os_str().as_encoded_bytes().len() as i64,
                name.len() as i64,
            )?;
            let bytes = resource_add(s, bytes, 1)?;
            s.semantic_work().arithmetic(
                ArithmeticOperation::Multiply,
                &[&bytes.into(), &3i64.into()],
            )?;
            let bytes = bytes.checked_mul(3).ok_or_else(invalid)?;
            let _path =
                s.reserve_external_payload(usize::try_from(bytes).map_err(|_| invalid())?)?;
            s.semantic_work()
                .scan_bytes(directory.as_os_str().as_encoded_bytes().len())?;
            s.semantic_work().scan_bytes(name.len())?;
            s.semantic_work()
                .write_bytes(usize::try_from(bytes).map_err(|_| invalid())?)?;
            let path = directory.join(name);
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(&path).map_err(StoreError::Io)?;
            // All subsequent reads use only these private descriptors. A crash
            // after unlink cannot leave a restartable/trusted index behind.
            std::fs::remove_file(&path).map_err(StoreError::Io)?;
            s.check_cancelled()?;
            Ok(file)
        }
        let index = open(self, ".workflow-audit-index", session)?;
        let bodies = open(self, ".workflow-audit-bodies", session)?;
        Ok([index, bodies])
    }
}
