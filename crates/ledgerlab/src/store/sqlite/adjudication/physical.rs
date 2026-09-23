//! Conservative native-SQL projection, separate from the canonical radix model.
//! Exact source/balancing proof and observed growth are acceptance obligations.
use super::*;
use r3::runtime::accounting::{Template, Worksheet};

pub(super) const TRANSIENT_PAGES: u128 = 24576;
pub(super) const FIXED_PAGES: u128 = 256;

pub(in crate::store::sqlite) async fn physical_usage(
    c: &mut SqliteConnection,
) -> Result<u32, StoreError> {
    let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut *c)
        .await?;
    let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(c)
        .await?;
    if free < 0 || pages < free {
        return Err(invalid());
    }
    u32::try_from(pages - free).map_err(|_| invalid())
}
/// Optional legacy work owns a separate finite allowance. Reuse of a free page
/// is charged too; raw file growth alone would let it consume R3's reserved room.
pub(in crate::store::sqlite) async fn charge_legacy(
    c: &mut SqliteConnection,
    before: u32,
) -> Result<(), StoreError> {
    let after = physical_usage(c).await?;
    let delta = after.saturating_sub(before);
    if delta == 0 {
        return Ok(());
    }
    let (limit, used): (i64, Vec<u8>) = sqlx::query_as(
        "SELECT legacy_allowance,legacy_used FROM r3_storage_profile WHERE singleton=1",
    )
    .fetch_one(&mut *c)
    .await?;
    let used = ordinal(used)?;
    let next = used
        .checked_add(Count::new(delta.into()).map_err(core)?)
        .map_err(core)?;
    if next.value() > limit as u128 {
        return Err(StoreError::Overloaded);
    }
    let result = sqlx::query(
        "UPDATE r3_storage_profile SET legacy_used=? WHERE singleton=1 AND legacy_used=?",
    )
    .bind(next.value().to_be_bytes().as_slice())
    .bind(used.value().to_be_bytes().as_slice())
    .execute(c)
    .await?;
    if result.rows_affected() != 1 {
        return Err(invalid());
    }
    Ok(())
}

fn add(a: u128, b: u128) -> Result<u128, StoreError> {
    a.checked_add(b).ok_or(StoreError::Overloaded)
}
fn mul(a: u128, b: u128) -> Result<u128, StoreError> {
    a.checked_mul(b).ok_or(StoreError::Overloaded)
}
pub(super) fn live_pages(kind: &str, t: &Template) -> Result<u128, StoreError> {
    let s = u128::from(t.segment_bytes);
    let n = s.div_ceil(4096);
    let objects = u128::from(t.records.saturating_sub(1).min(216));
    let heads = u128::from(
        *t.counter_increments
            .get("index_cardinality")
            .ok_or_else(invalid)?,
    )
    .min(256);
    let namespaces = if kind == "ENROLL" { 4 } else { 0 };
    let actions = if matches!(kind, "PREPARE_ENROLL" | "ENROLL") {
        0
    } else {
        128
    };
    let rows = 3 + 2 * n + 2 * objects + 3 * heads + namespaces + actions;
    let bytes = 3 * s
        + 10240 * n
        + 24576 * objects
        + 552960 * heads
        + 24576
        + 2048 * namespaces
        + 10240 * actions;
    Ok(9 * rows + (bytes + 16384 * rows).div_ceil(4092))
}

/// Each dimension independently bounds sum(template physical credits). Taking
/// the minimum of those upper bounds never treats workspace as retained bytes.
pub(super) fn retained_pages(resources: &wire::Resource) -> Result<u128, StoreError> {
    let worksheet = Worksheet::frozen().map_err(core)?;
    let mut ratios = [(0u128, 1u128); 5];
    for (kind, t) in &worksheet.transitions {
        let pages = live_pages(kind, t)?;
        for (i, dimension) in t
            .resources()
            .map_err(core)?
            .dimensions()
            .into_iter()
            .take(5)
            .enumerate()
        {
            let d = dimension.value();
            if d == 0 {
                return Err(invalid());
            }
            if pages * ratios[i].1 > ratios[i].0 * d {
                ratios[i] = (pages, d);
            }
        }
    }
    resources
        .dimensions()
        .into_iter()
        .take(5)
        .zip(ratios)
        .map(|(d, (n, den))| Ok(mul(d.value(), n)?.div_ceil(den)))
        .collect::<Result<Vec<_>, StoreError>>()?
        .into_iter()
        .min()
        .ok_or_else(invalid)
}

pub(super) fn total_pages(
    resources: &wire::Resource,
    baseline: u128,
    legacy: u128,
) -> Result<u32, StoreError> {
    let pages = add(
        add(add(retained_pages(resources)?, baseline)?, legacy)?,
        TRANSIENT_PAGES + FIXED_PAGES,
    )?;
    u32::try_from(pages)
        .ok()
        .filter(|v| *v > 0 && *v < u32::MAX)
        .ok_or(StoreError::Overloaded)
}

pub(super) fn check_plan(
    p: &crate::service::accept::adjudication::ValidatedAdjudicationPlan,
) -> Result<(), StoreError> {
    let v = r3::runtime::command_value(p.command().command()).map_err(core)?;
    let worksheet = Worksheet::frozen().map_err(core)?;
    let kind = v["kind"].as_str().ok_or_else(invalid)?;
    let t = worksheet.template(kind).map_err(core)?;
    let heads = *t
        .counter_increments
        .get("index_cardinality")
        .ok_or_else(invalid)?;
    if !p.indices().is_empty()
        || p.segment_bytes().len() as u64 > t.segment_bytes
        || p.segment().objects.len() as u64 > t.records.saturating_sub(1).min(216)
        || p.head_writes().len() as u64 > heads.min(256)
        || p.head_writes()
            .iter()
            .any(|w| w.value.len() > r3::COMMAND_BYTES)
        || p.held_intentions().len() > 128
    {
        return Err(StoreError::Overloaded);
    }
    Ok(())
}
