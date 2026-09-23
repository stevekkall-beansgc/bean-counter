//! Bounded R3 persistence primitives. Economic/authority validation stays in the coordinator.
use crate::{
    service::accept::adjudication::TrustedJournalHead,
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
use serde_json::json;
use sqlx::{Row, SqliteConnection};

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
    let row = sqlx::query("SELECT ordinal,segment,replay_root FROM r3_journals WHERE journal=?")
        .bind(&key)
        .fetch_optional(c)
        .await?;
    let (n, segment, root) = match row {
        Some(row) => (
            ordinal(row.try_get(0)?)?,
            Digest::parse(&row.try_get::<String, _>(1)?).map_err(core)?,
            Digest::parse(&row.try_get::<String, _>(2)?).map_err(core)?,
        ),
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
pub(super) async fn point(
    c: &mut SqliteConnection,
    key: &HeadKey,
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
            &json!(["sqlite-saved-journal/1", journal, n, segment, root]),
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
