//! Bounded R3 persistence primitives. Economic/authority validation stays in the coordinator.
mod persist;
mod physical;
mod provision;
mod reader;
mod resolve;
pub(super) mod tx;
use crate::{
    service::accept::adjudication::{TrustedJournalHead, VerifiedSource},
    store::{
        adjudication::{HeadKey, HeadKind, JournalIdentity, ObservedHead, SavedOutcome},
        errors::StoreError,
    },
};
use ledgerlab_core::adjudication::{
    self as r3, commands as wire,
    runtime::index_key,
    types::{Count, Digest},
};
pub(super) use physical::{charge_legacy, physical_usage};
pub(super) use provision::verify_stored_profile;
use serde_json::json;
use sqlx::{Row, SqliteConnection};

impl super::SqliteStore {
    pub(crate) async fn adjudication_enrollment_source(
        &self,
        j: &JournalIdentity,
    ) -> Result<VerifiedSource, StoreError> {
        if j.host != j.store {
            return Err(invalid());
        }
        let key = wire::ProofFullKey::V2(j.registration.clone());
        let origin=tokio::time::timeout(std::time::Duration::from_secs(2),async {
            let _lane=if self.inner.adjudication_enabled.load(std::sync::atomic::Ordering::Acquire) {Some(self.inner.adjudication_gate.read().await)}else{None};
            self.require_published()?;
            let mut c=self.inner.readers.acquire().await?;
            let rows:Vec<Vec<u8>>=sqlx::query_scalar("SELECT metadata FROM r3_objects WHERE journal=? AND kind='ENROLLMENT' AND full_key=? AND length(metadata) BETWEEN 2 AND 8192 LIMIT 2")
                .bind(journal_key(j)?).bind(r3::canonical_bytes(&key,4096).map_err(core)?).fetch_all(&mut *c).await?;
            if rows.len()!=1 {return Err(invalid());}
            let metadata=ledgerlab_core::canonical::parse_bounded(&rows[0],8192).map_err(core)?;
            let origin:wire::ObjectOrigin=serde_json::from_value(metadata["origin"].clone()).map_err(|_|invalid())?;
            if origin.store!=j.store || origin.scope!=j.scope || origin.registration!=j.registration || origin.host!=j.host {return Err(invalid());}
            Ok::<_,StoreError>(origin)
        }).await.map_err(|_|StoreError::Deadline)??;
        self.adjudication_source(j, origin.ordinal, &wire::FactKind::Enrollment, &key)
            .await
    }
    pub(crate) async fn adjudication_exact_source(
        &self,
        proof: &wire::Proof,
    ) -> Result<VerifiedSource, StoreError> {
        let j = JournalIdentity {
            store: proof.store.clone(),
            scope: proof.scope.clone(),
            registration: proof.registration.clone(),
            host: proof.host.clone(),
        };
        let actual = self
            .adjudication_source(&j, proof.ordinal, &proof.fact_kind, &proof.full_key)
            .await?;
        if actual.proof() != proof {
            return Err(invalid());
        }
        Ok(actual)
    }
    /// Trusted host integration selects this store; no remote-supplied database
    /// name, hash list or segment body can construct a primary source witness.
    pub(crate) async fn adjudication_source(
        &self,
        journal: &JournalIdentity,
        at: Count,
        kind: &wire::FactKind,
        key: &wire::ProofFullKey,
    ) -> Result<VerifiedSource, StoreError> {
        use std::{sync::atomic::Ordering, time::Duration};
        tokio::time::timeout(Duration::from_secs(5), async {
            let _lane = if self.inner.adjudication_enabled.load(Ordering::Acquire) {
                Some(self.inner.adjudication_gate.read().await)
            } else {
                None
            };
            self.require_published()?;
            let mut tx = self.inner.readers.begin().await?;
            let installation = super::read::installation(&mut tx).await?;
            if installation.logical_store_id != journal.store.as_str() {
                return Err(invalid());
            }
            let proof = primary_proof(&mut tx, journal, at, kind, key).await?;
            let verified = source(&mut tx, &proof).await?;
            tx.rollback().await?;
            Ok(verified)
        })
        .await
        .map_err(|_| StoreError::Deadline)?
    }
}

async fn primary_proof(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
    at: Count,
    kind: &wire::FactKind,
    key: &wire::ProofFullKey,
) -> Result<wire::Proof, StoreError> {
    let journal = journal_key(j)?;
    let origin = wire::ObjectOrigin {
        store: j.store.clone(),
        scope: j.scope.clone(),
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: at,
    };
    let origin = r3::canonical_bytes(&origin, 2048).map_err(core)?;
    let kv = serde_json::to_value(kind).map_err(|_| invalid())?;
    let key_bytes = r3::canonical_bytes(key, 4096).map_err(core)?;
    let rows=sqlx::query("SELECT o.body_hash,o.byte_length,s.segment,s.replay_root FROM r3_objects o JOIN r3_segments s ON s.journal=o.journal AND s.ordinal=o.ordinal WHERE o.journal=? AND o.origin=? AND o.kind=? AND o.full_key=? AND o.ordinal=? LIMIT 2")
        .bind(&journal).bind(origin).bind(kv.as_str().ok_or_else(invalid)?).bind(key_bytes).bind(at.value().to_be_bytes().as_slice()).fetch_all(c).await?;
    if rows.len() != 1 {
        return Err(invalid());
    }
    let row = &rows[0];
    let segment = Digest::parse(&row.try_get::<String, _>(2)?).map_err(core)?;
    let root = Digest::parse(&row.try_get::<String, _>(3)?).map_err(core)?;
    let observation = r3::raw_sha256(
        &r3::canonical_bytes(
            &json!(["sqlite-primary-journal/1", journal, at, segment, root]),
            r3::COMMAND_BYTES,
        )
        .map_err(core)?,
    );
    Ok(wire::Proof {
        store: j.store.clone(),
        scope: j.scope.clone(),
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: at,
        segment,
        root,
        fact_kind: kind.clone(),
        full_key: key.clone(),
        body_hash: Digest::parse(&row.try_get::<String, _>(0)?).map_err(core)?,
        bytes: Count::new(row.try_get::<i64, _>(1)? as u128).map_err(core)?,
        trusted_observation_ref: observation,
    })
}

fn invalid() -> StoreError {
    StoreError::Integrity("R3 retained storage binding")
}
fn core(_: ledgerlab_core::Error) -> StoreError {
    invalid()
}

pub(super) fn journal_key(j: &JournalIdentity) -> Result<Vec<u8>, StoreError> {
    index_key(
        *b"r3host01",
        &[
            j.store.as_str().as_bytes(),
            j.scope.0.as_str().as_bytes(),
            j.scope.1.as_str().as_bytes(),
            j.registration.as_str().as_bytes(),
            j.host.as_str().as_bytes(),
        ],
    )
    .map_err(core)
}
pub(super) fn ordinal(raw: Vec<u8>) -> Result<Count, StoreError> {
    let bytes: [u8; 16] = raw.try_into().map_err(|_| invalid())?;
    Count::new(u128::from_be_bytes(bytes)).map_err(core)
}
pub(super) fn head_tag(kind: HeadKind) -> i64 {
    match kind {
        HeadKind::Enrollment => 0,
        HeadKind::Authority => 1,
        HeadKind::Grant => 2,
        HeadKind::GrantRegistry => 3,
        HeadKind::Token => 4,
        HeadKind::Allocation => 5,
        HeadKind::Receipt => 6,
        HeadKind::Round => 7,
        HeadKind::Gateway => 8,
        HeadKind::Family => 9,
        HeadKind::Case => 10,
        HeadKind::Entitlement => 11,
        HeadKind::Supplier => 12,
        HeadKind::Adjustment => 13,
        HeadKind::Resource => 14,
        HeadKind::Counter => 15,
        HeadKind::VerifiedCursor => 16,
        HeadKind::Delivery => 17,
    }
}
/// The caller owns a real primary read or write transaction. Absence is genuine
/// genesis; it never manufactures an enrolled ExpectedPrefix.
pub(super) async fn head(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
) -> Result<TrustedJournalHead, StoreError> {
    let key = journal_key(j)?;
    let row =
        sqlx::query("SELECT ordinal,segment,replay_root,identity FROM r3_journals WHERE journal=?")
            .bind(&key)
            .fetch_optional(c)
            .await?;
    let (n, segment, root) = match row {
        Some(row) => {
            let identity: Vec<u8> = row.try_get(3)?;
            if identity
                != r3::canonical_bytes(&json!([j.store, j.scope, j.registration, j.host]), 4096)
                    .map_err(core)?
            {
                return Err(invalid());
            }
            (
                ordinal(row.try_get(0)?)?,
                Digest::parse(&row.try_get::<String, _>(1)?).map_err(core)?,
                Digest::parse(&row.try_get::<String, _>(2)?).map_err(core)?,
            )
        }
        None => (
            Count::ZERO,
            Digest::parse(&"0".repeat(64)).map_err(core)?,
            Digest::parse(&"0".repeat(64)).map_err(core)?,
        ),
    };
    let observation = r3::raw_sha256(
        &r3::canonical_bytes(
            &json!(["sqlite-primary-journal/1", key, n, segment, root]),
            r3::COMMAND_BYTES,
        )
        .map_err(core)?,
    );
    TrustedJournalHead::from_backend(j.clone(), n, segment, root, observation).map_err(core)
}

/// Materialize one bounded immutable source segment under an actual primary
/// snapshot. The host supplies this store handle; a submitted path is never used.
pub(super) async fn source(
    c: &mut SqliteConnection,
    proof: &wire::Proof,
) -> Result<VerifiedSource, StoreError> {
    let j = JournalIdentity {
        store: proof.store.clone(),
        scope: proof.scope.clone(),
        registration: proof.registration.clone(),
        host: proof.host.clone(),
    };
    let current = head(c, &j).await?;
    if current.ordinal() < proof.ordinal {
        return Err(invalid());
    }
    if proof.fact_kind == wire::FactKind::EnrollPreparation
        && (current.ordinal() != proof.ordinal
            || *current.segment() != proof.segment
            || *current.root() != proof.root)
    {
        return Err(invalid());
    }
    let journal = journal_key(&j)?;
    let row=sqlx::query("SELECT segment,replay_root,byte_length,page_count FROM r3_segments WHERE journal=? AND ordinal=?")
        .bind(&journal).bind(proof.ordinal.value().to_be_bytes().as_slice()).fetch_one(&mut *c).await?;
    let segment = Digest::parse(&row.try_get::<String, _>(0)?).map_err(core)?;
    let root = Digest::parse(&row.try_get::<String, _>(1)?).map_err(core)?;
    let length: i64 = row.try_get(2)?;
    let pages: i64 = row.try_get(3)?;
    if !(2..=r3::SEGMENT_BYTES as i64).contains(&length)
        || pages != (length as usize).div_ceil(r3::PAGE_BYTES) as i64
    {
        return Err(invalid());
    }
    let observation = r3::raw_sha256(
        &r3::canonical_bytes(
            &json!([
                "sqlite-primary-journal/1",
                journal,
                proof.ordinal,
                segment,
                root
            ]),
            r3::COMMAND_BYTES,
        )
        .map_err(core)?,
    );
    let prefix = TrustedJournalHead::from_backend(
        j.clone(),
        proof.ordinal,
        segment.clone(),
        root,
        observation,
    )
    .map_err(core)?;
    let mut bytes = Vec::with_capacity(length as usize);
    for page in 0..pages {
        let fragment = segment_page(
            c,
            &j,
            &segment,
            Count::new(page as u128).map_err(core)?,
            0,
            r3::PAGE_BYTES as u16,
        )
        .await?;
        bytes.extend(fragment.bytes);
    }
    if bytes.len() != length as usize {
        return Err(invalid());
    }
    VerifiedSource::from_backend(prefix, proof.clone(), &bytes).map_err(core)
}
pub(super) async fn point(
    c: &mut SqliteConnection,
    key: &HeadKey,
) -> Result<ObservedHead, StoreError> {
    point_limited(c, key, r3::SEGMENT_BYTES).await
}
async fn point_limited(
    c: &mut SqliteConnection,
    key: &HeadKey,
    maximum: usize,
) -> Result<ObservedHead, StoreError> {
    if key.full_key.is_empty() || key.full_key.len() > r3::MAX_KEY_BYTES {
        return Err(invalid());
    }
    // Size is checked before body materialization; one bounded point row only.
    let journal = journal_key(&key.journal)?;
    let row = sqlx::query(
        "SELECT revision,length(value) FROM r3_heads WHERE journal=? AND kind=? AND full_key=?",
    )
    .bind(&journal)
    .bind(head_tag(key.kind))
    .bind(&key.full_key)
    .fetch_optional(&mut *c)
    .await?;
    match row {
        None => Ok(ObservedHead {
            key: key.clone(),
            revision: None,
            value: None,
        }),
        Some(row) => {
            let revision = ordinal(row.try_get(0)?)?;
            let length: i64 = row.try_get(1)?;
            if !(2..=r3::SEGMENT_BYTES as i64).contains(&length) {
                return Err(invalid());
            }
            if length as usize > maximum {
                return Err(StoreError::Overloaded);
            }
            let value: Vec<u8> = sqlx::query_scalar(
                "SELECT value FROM r3_heads WHERE journal=? AND kind=? AND full_key=?",
            )
            .bind(&journal)
            .bind(head_tag(key.kind))
            .bind(&key.full_key)
            .fetch_one(c)
            .await?;
            if value.len() != length as usize {
                return Err(invalid());
            }
            Ok(ObservedHead {
                key: key.clone(),
                revision: Some(revision),
                value: Some(value),
            })
        }
    }
}
pub(super) async fn saved(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
    key: &wire::Delivery,
) -> Result<Option<SavedOutcome>, StoreError> {
    let journal = journal_key(j)?;
    let delivery = r3::canonical_bytes(key, r3::COMMAND_BYTES).map_err(core)?;
    let metadata=sqlx::query("SELECT length(command),length(result),length(receipt) FROM r3_commands WHERE journal=? AND delivery=?").bind(&journal).bind(&delivery).fetch_optional(&mut *c).await?;
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let a: i64 = metadata.try_get(0)?;
    let b: i64 = metadata.try_get(1)?;
    let d: Option<i64> = metadata.try_get(2)?;
    if !(2..=r3::COMMAND_BYTES as i64).contains(&a)
        || !(2..=r3::SEGMENT_BYTES as i64).contains(&b)
        || d.is_some_and(|n| !(2..=8192).contains(&n))
    {
        return Err(invalid());
    }
    let row=sqlx::query("SELECT c.command,c.result,c.receipt,c.ordinal,s.segment,s.replay_root FROM r3_commands c JOIN r3_segments s ON s.journal=c.journal AND s.ordinal=c.ordinal WHERE c.journal=? AND c.delivery=?").bind(&journal).bind(&delivery).fetch_one(c).await?;
    let command: Vec<u8> = row.try_get(0)?;
    let result: Vec<u8> = row.try_get(1)?;
    let receipt: Option<Vec<u8>> = row.try_get(2)?;
    let n = ordinal(row.try_get(3)?)?;
    let segment = Digest::parse(&row.try_get::<String, _>(4)?).map_err(core)?;
    let root = Digest::parse(&row.try_get::<String, _>(5)?).map_err(core)?;
    let observation = r3::raw_sha256(
        &r3::canonical_bytes(
            &json!(["sqlite-primary-journal/1", journal, n, segment, root]),
            r3::COMMAND_BYTES,
        )
        .map_err(core)?,
    );
    Ok(Some(SavedOutcome {
        command,
        result: r3::parse_exact(&result, r3::SEGMENT_BYTES).map_err(core)?,
        prefix: TrustedJournalHead::from_backend(j.clone(), n, segment, root, observation)
            .map_err(core)?,
        receipt: receipt
            .map(|v| r3::parse_exact(&v, 8192).map_err(core))
            .transpose()?,
    }))
}
/// Page selection is indexed by exact immutable address; no complete segment is
/// loaded or concatenated before a caller applies its per-advance byte budget.
pub(super) async fn segment_page(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
    segment: &Digest,
    page: Count,
    offset: u16,
    max_bytes: u16,
) -> Result<r3::reads::PageFragment, StoreError> {
    if max_bytes == 0
        || usize::from(max_bytes) > r3::PAGE_BYTES
        || usize::from(offset) + usize::from(max_bytes) > r3::PAGE_BYTES
        || page.value() >= 2048
    {
        return Err(invalid());
    }
    let journal = journal_key(j)?;
    let metadata = sqlx::query(
        "SELECT ordinal,byte_length,page_count FROM r3_segments WHERE journal=? AND segment=?",
    )
    .bind(&journal)
    .bind(segment.as_str())
    .fetch_one(&mut *c)
    .await?;
    let n: Vec<u8> = metadata.try_get(0)?;
    let total: i64 = metadata.try_get(1)?;
    let pages: i64 = metadata.try_get(2)?;
    if !(2..=r3::SEGMENT_BYTES as i64).contains(&total) || page.value() >= pages as u128 {
        return Err(invalid());
    }
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT substr(bytes,?,?) FROM r3_segment_pages WHERE journal=? AND ordinal=? AND page=?",
    )
    .bind(i64::from(offset) + 1)
    .bind(i64::from(max_bytes))
    .bind(&journal)
    .bind(&n)
    .bind(page.value() as i64)
    .fetch_one(c)
    .await?;
    let result = r3::reads::PageFragment {
        address: r3::reads::PageAddress {
            segment: segment.clone(),
            page,
        },
        offset,
        bytes,
        total_bytes: Count::new(total as u128).map_err(core)?,
    };
    result.validate().map_err(core)?;
    Ok(result)
}

pub(super) async fn object_page(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
    q: &crate::store::adjudication::ObjectPageRequest,
) -> Result<Vec<u8>, StoreError> {
    if q.max_bytes == 0 || usize::from(q.max_bytes) > r3::PAGE_BYTES || q.key.len() > 4096 {
        return Err(invalid());
    }
    let journal = journal_key(j)?;
    let origin = r3::canonical_bytes(&q.origin, 2048).map_err(core)?;
    let kind = serde_json::to_value(&q.kind).map_err(|_| invalid())?;
    let kind = kind.as_str().ok_or_else(invalid)?;
    let length: i64 = sqlx::query_scalar("SELECT byte_length FROM r3_objects WHERE journal=? AND origin=? AND kind=? AND full_key=? AND body_hash=?")
        .bind(&journal).bind(&origin).bind(kind).bind(&q.key).bind(q.hash.as_str()).fetch_one(&mut *c).await?;
    if !(2..=262144).contains(&length) || q.offset.value() >= length as u128 {
        return Err(invalid());
    }
    let offset = q.offset.value() as usize;
    let page = offset / r3::PAGE_BYTES;
    let within = offset % r3::PAGE_BYTES;
    let take = usize::from(q.max_bytes)
        .min(r3::PAGE_BYTES - within)
        .min(length as usize - offset);
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT substr(bytes,?,?) FROM r3_object_pages WHERE journal=? AND origin=? AND kind=? AND full_key=? AND body_hash=? AND page=?")
        .bind(within as i64+1).bind(take as i64).bind(&journal).bind(&origin).bind(kind).bind(&q.key).bind(q.hash.as_str()).bind(page as i64).fetch_one(c).await?;
    if bytes.len() != take {
        return Err(invalid());
    }
    Ok(bytes)
}

#[cfg(test)]
impl super::SqliteStore {
    pub(crate) async fn test_hold_optional_slots(&self) -> tokio::sync::OwnedSemaphorePermit {
        std::sync::Arc::clone(&self.inner.queue)
            .acquire_many_owned(65)
            .await
            .unwrap()
    }
    pub(crate) fn test_publication_cut(&self, cut: u8) {
        assert!([0, 11, 12, 13].contains(&cut));
        self.inner
            .fence_cut
            .store(cut, std::sync::atomic::Ordering::Release);
    }
    /// One-shot fault after actual original base SQL, before any R3 projection.
    pub(crate) fn test_fail_after_original_base(&self) {
        self.inner
            .fail_after_original_base
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
impl super::SqliteStore {
    pub(crate) async fn test_adjudication_stats(
        &self,
        j: &JournalIdentity,
    ) -> std::collections::BTreeMap<String, u128> {
        let mut c = self.inner.readers.acquire().await.unwrap();
        let mut out = std::collections::BTreeMap::new();
        for table in [
            "segments",
            "objects",
            "segment_pages",
            "object_pages",
            "heads",
            "head_versions",
        ] {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM r3_{table} WHERE journal=?"
            )))
            .bind(journal_key(j).unwrap())
            .fetch_one(&mut *c)
            .await
            .unwrap();
            out.insert(table.into(), n as u128);
        }
        for table in ["heads", "head_versions"] {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM r3_{table} WHERE journal=? AND kind=?"
            )))
            .bind(journal_key(j).unwrap())
            .bind(head_tag(HeadKind::Case))
            .fetch_one(&mut *c)
            .await
            .unwrap();
            out.insert(format!("case_{table}"), n as u128);
        }
        for pragma in [
            "page_count",
            "freelist_count",
            "page_size",
            "max_page_count",
        ] {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("PRAGMA {pragma}")))
                .fetch_one(&mut *c)
                .await
                .unwrap();
            out.insert(
                if pragma == "max_page_count" {
                    "reader_maximum_pages".into()
                } else {
                    pragma.into()
                },
                n as u128,
            );
        }
        let maximum: i64 =
            sqlx::query_scalar("SELECT maximum_pages FROM r3_storage_profile WHERE singleton=1")
                .fetch_one(&mut *c)
                .await
                .unwrap();
        out.insert("maximum_pages".into(), maximum as u128);
        let mut writer = self.inner.writer.acquire().await.unwrap();
        let enforced: i64 = sqlx::query_scalar("PRAGMA max_page_count")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        assert_eq!(
            enforced, maximum,
            "actual writer enforces provisioned profile"
        );
        out.insert("writer_maximum_pages".into(), enforced as u128);
        let wal = self.inner._owner.database.with_file_name("local.db-wal");
        out.insert(
            "wal_bytes".into(),
            std::fs::metadata(wal)
                .map(|m| u128::from(m.len()))
                .unwrap_or(0),
        );
        out.insert(
            "maximum_wal_bytes".into(),
            32 + (out["maximum_pages"] + 2 + 65536u128.div_ceil(4120)) * 4120,
        );
        out
    }
}
