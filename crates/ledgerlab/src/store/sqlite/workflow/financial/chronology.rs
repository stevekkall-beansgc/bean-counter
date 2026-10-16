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
