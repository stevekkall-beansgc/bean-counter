//! Consistent, bounded retained-record reads. No policy evaluation.
use super::{read, SqliteStore};
use crate::{
    local::ExplainTarget,
    store::{errors::StoreError, records::*},
};

pub(crate) struct StoredEvent {
    pub id: String,
    pub records: Vec<(String, String, CanonicalRecord)>,
}
impl SqliteStore {
    pub(crate) async fn local_installation(&self) -> Result<Installation, StoreError> {
        read::installation(&mut *self.inner.readers.acquire().await?).await
    }
    pub(crate) async fn inspect(
        &self,
        s: &Scope,
        source: &str,
        target: &ExplainTarget,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        let mut tx = self.inner.readers.begin().await?;
        let (event, chain) = match target {
            ExplainTarget::Event(id) => (id.as_str(), ""),
            ExplainTarget::Chain(id) => ("", id.as_str()),
        };
        let ids: Vec<String> = sqlx::query_scalar("SELECT e.id FROM events e JOIN chain_revisions r ON r.tenant=e.tenant AND r.environment=e.environment AND r.event_id=e.id WHERE e.tenant=? AND e.environment=? AND e.source=? AND ((?<>'' AND e.chain_id=?) OR (?<>'' AND (e.id=? OR e.id IN (SELECT canonical_event_id FROM delivery_keys WHERE tenant=e.tenant AND environment=e.environment AND source=? AND external_id=?)))) ORDER BY r.revision,e.id LIMIT 1001")
            .bind(&s.tenant).bind(&s.environment).bind(source).bind(chain).bind(chain).bind(event).bind(event).bind(source).bind(event).fetch_all(&mut *tx).await?;
        if ids.len() > 1000 {
            return Err(StoreError::Integrity("chain exceeds supported read bound"));
        }
        let mut events = Vec::new();
        for id in ids {
            // Only static table names and bound input. A transaction pins one snapshot.
            let rows: Vec<(String,String,Vec<u8>,String)> = sqlx::query_as(
                "SELECT kind,id,canonical_bytes,content_hash FROM (
                SELECT 'event' AS kind,id,canonical_bytes,content_hash FROM events WHERE tenant=?1 AND environment=?2 AND id=?3
                UNION ALL SELECT 'decision-manifest',id,canonical_bytes,content_hash FROM decision_manifests WHERE tenant=?1 AND environment=?2 AND event_id=?3
                UNION ALL SELECT 'receipt',id,canonical_bytes,content_hash FROM accepted_receipts WHERE tenant=?1 AND environment=?2 AND event_id=?3
                UNION ALL SELECT 'action',id,canonical_bytes,content_hash FROM actions WHERE tenant=?1 AND environment=?2 AND event_id=?3
                UNION ALL SELECT 'explanation',id,canonical_bytes,content_hash FROM explanations WHERE tenant=?1 AND environment=?2 AND event_id=?3
                UNION ALL SELECT 'intention',id,canonical_bytes,content_hash FROM intentions WHERE tenant=?1 AND environment=?2 AND event_id=?3
                UNION ALL SELECT 'document',id,canonical_bytes,content_hash FROM documents WHERE tenant=?1 AND environment=?2 AND id IN (SELECT document_id FROM snapshots WHERE tenant=?1 AND environment=?2 AND event_id=?3)
                ) ORDER BY kind,id")
                .bind(&s.tenant).bind(&s.environment).bind(&id).fetch_all(&mut *tx).await?;
            events.push(StoredEvent {
                id,
                records: rows
                    .into_iter()
                    .map(|(kind, id, bytes, hash)| {
                        (
                            kind,
                            id,
                            CanonicalRecord {
                                canonical_bytes: bytes,
                                content_hash: hash,
                            },
                        )
                    })
                    .collect(),
            });
        }
        tx.rollback().await?;
        Ok(events)
    }
}
