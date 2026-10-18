//! Conservative native-SQL projection, separate from the canonical radix model.
//! Source worksheet: docs/phase4/SQLITE-PHYSICAL-PAGES.md. Host backing and
//! allocator bounds remain separate independent acceptance obligations.
use super::*;
use r3::runtime::accounting::{Template, Worksheet};

pub(super) const TRANSIENT_PAGES: u128 = 24576;
pub(super) const FIXED_PAGES: u128 = 256;
pub(super) const READER_WORKSPACE_BYTES: u128 = 16 * 1024 * 1024;
// Pinned-toolchain allocation envelope; see docs/phase4/COMPARISON-WORKSPACE.md.
// This optional session reservation is separate from mandatory and SQL reader work.
const fn dom(bytes: u128) -> u128 {
    256 * bytes + 65_536
}
const OBJECT: u128 = 262_144;
const SEGMENT: u128 = 3_411_620;
const BASE: u128 = 1_048_576;
const AUTHORITY_ENCODED: u128 = 83 * 21_979;
const COMMON: u128 = 5 * SEGMENT
    + 8 * OBJECT
    + 4 * dom(OBJECT)
    + 3 * dom(OBJECT)
    + 2 * dom(AUTHORITY_ENCODED)
    + dom(524_288);
const REPLAY: u128 = 192 * OBJECT;
const COMPARISON_PEAK: u128 = COMMON
    + dom(SEGMENT)
    + 2 * BASE
    + 4 * dom(BASE)
    + 12 * dom(OBJECT)
    + 2 * dom(REPLAY)
    + 4 * REPLAY;
pub(super) const COMPARISON_WORKSPACE_BYTES: u128 = COMPARISON_PEAK.div_ceil(1_048_576) * 1_048_576;
const _: () = assert!(COMPARISON_WORKSPACE_BYTES == 30_284_972_032);
const _: () = assert!(std::mem::size_of::<serde_json::Value>() <= 32);
const _: () = assert!(std::mem::size_of::<String>() <= 24);

pub(super) fn backing_needed(pages: u32, workspace: Count) -> Result<u128, StoreError> {
    let frames = u128::from(pages) + 2 + 65536u128.div_ceil(4120);
    let wal = 32 + frames * 4120;
    // The first 32KiB WAL-index region has 4062 entries; later regions 4096.
    let shm = 32768 * (frames + 34).div_ceil(4096);
    add(
        add(
            u128::from(pages) * 4096 + wal + shm,
            mul(2, workspace.value())?,
        )?,
        16384 + READER_WORKSPACE_BYTES + COMPARISON_WORKSPACE_BYTES,
    )
}

pub(in crate::store::sqlite) async fn physical_usage(
    c: &mut SqliteConnection,
) -> Result<u32, StoreError> {
    physical_usage_with_work(c,&mut ledgerlab_core::validation_work::NoopValidationWork).await.map_err(|error|match error {
        crate::store::sqlite::workflow::financial::FinancialReadError::Store(error)=>error,
        crate::store::sqlite::workflow::financial::FinancialReadError::Resource(_)=>invalid(),
    })
}
pub(in crate::store::sqlite) async fn physical_usage_with_work<W:ledgerlab_core::validation_work::ValidationWork+Send>(c:&mut SqliteConnection,work:&mut W)->Result<u32,crate::store::sqlite::workflow::financial::FinancialReadError> {
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op,ValidationVisit as Visit};
    work.write_bytes(2*std::mem::size_of::<i64>())?;
    work.visit(Visit::RefLookup)?;
    let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut *c)
        .await?;
    work.visit(Visit::RefLookup)?;
    let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(c)
        .await?;
    work.arithmetic(Op::Compare,&[&free.into(),&0i64.into()])?;
    let invalid_free=free<0;
    let invalid_pages=if invalid_free {false} else {
        work.arithmetic(Op::Compare,&[&pages.into(),&free.into()])?;
        pages<free
    };
    if invalid_free || invalid_pages {return Err(invalid().into());}
    work.arithmetic(Op::Subtract,&[&pages.into(),&free.into()])?;
    let used=pages-free;
    work.visit(Visit::Constraint)?;
    u32::try_from(used).map_err(|_| invalid().into())
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

/// SAME storage-allowance transition, owned and paid by the actual host commit.
/// This is physical accounting, never a monetary or financial support proof.
pub(in crate::store::sqlite) const LEGACY_COMMIT_IO_BYTES:usize=262144+2*(16+32)+2*(2*std::mem::size_of::<Vec<u8>>()+2*std::mem::size_of::<usize>())+4*std::mem::size_of::<sqlx::sqlite::SqliteArgumentValue>()+std::mem::size_of::<sqlx::sqlite::SqliteQueryResult>();
pub(in crate::store::sqlite) async fn charge_legacy_with_session(
    c:&mut SqliteConnection,before:u32,s:&mut ledgerlab_core::validation_work::OriginalAuditSession,
)->Result<(),crate::store::sqlite::workflow::financial::FinancialReadError> {
    use crate::store::sqlite::workflow::{capacity::SessionWork,financial::FinancialReadError};
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op,ValidationVisit as Visit};
    fn count_error(error:ledgerlab_core::Error)->FinancialReadError {
        if error.code=="COUNTER_EXHAUSTED" {FinancialReadError::Store(core(error))} else {FinancialReadError::Resource(error)}
    }
    let after=physical_usage_with_work(c,&mut SessionWork(s)).await?;
    s.semantic_work().arithmetic(Op::Subtract,&[&after.into(),&before.into()])?;
    let delta=after.saturating_sub(before);
    s.semantic_work().arithmetic(Op::Compare,&[&delta.into(),&0u32.into()])?;
    if delta==0 {return Ok(());}
    s.semantic_work().visit(Visit::RefLookup)?;
    s.semantic_work().write_bytes(2*std::mem::size_of::<i64>())?;
    let (limit,length):(i64,i64)=sqlx::query_as("SELECT legacy_allowance,length(legacy_used) FROM r3_storage_profile WHERE singleton=1").fetch_one(&mut *c).await?;
    s.semantic_work().arithmetic(Op::Compare,&[&length.into(),&16i64.into()])?;
    if length!=16 {return Err(invalid().into());}
    s.semantic_work().visit(Visit::RefLookup)?;
    s.semantic_work().scan_bytes(16)?;
    s.semantic_work().write_bytes(16+std::mem::size_of::<Vec<u8>>())?;
    let used:Vec<u8>=sqlx::query_scalar("SELECT legacy_used FROM r3_storage_profile WHERE singleton=1").fetch_one(&mut *c).await?;
    s.semantic_work().visit(Visit::Constraint)?;
    let raw:[u8;16]=used.try_into().map_err(|_|invalid())?;
    let used=Count::new_with_work(u128::from_be_bytes(raw),s.semantic_work()).map_err(count_error)?;
    let delta=Count::new_with_work(delta.into(),s.semantic_work()).map_err(count_error)?;
    let next=used.checked_add_with_work(delta,s.semantic_work()).map_err(count_error)?;
    let allowed=limit as u128;
    s.semantic_work().arithmetic(Op::Compare,&[&next.value().into(),&allowed.into()])?;
    if next.value()>allowed {return Err(StoreError::Overloaded.into());}
    // Two fixed-width stack encodings, SQLx-owned argument copies and
    // SQLite TRANSIENT copies; all controls/row results are prepaid too.
    s.semantic_work().scan_bytes(3*2*16)?;
    s.semantic_work().write_bytes(3*2*16+2*(2*std::mem::size_of::<Vec<u8>>()+2*std::mem::size_of::<usize>())+4*std::mem::size_of::<sqlx::sqlite::SqliteArgumentValue>()+std::mem::size_of::<sqlx::sqlite::SqliteQueryResult>())?;
    s.semantic_work().visit(Visit::RefLookup)?;
    let result=sqlx::query("UPDATE r3_storage_profile SET legacy_used=? WHERE singleton=1 AND legacy_used=?")
        .bind(next.value().to_be_bytes().as_slice()).bind(used.value().to_be_bytes().as_slice()).execute(c).await?;
    s.semantic_work().arithmetic(Op::Compare,&[&result.rows_affected().into(),&1u64.into()])?;
    if result.rows_affected()!=1 {return Err(invalid().into());}
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
    // Charge the additional retained-authority lookup index for every object.
    let rows = 3 + 2 * n + 3 * objects + 3 * heads + namespaces + actions;
    let bytes = 3 * s
        + 10240 * n
        + 24576 * objects
        + 552960 * heads
        + 24576
        + 2048 * namespaces
        + 10240 * actions;
    // Cumulative occupied-cell and payload inequalities include every native
    // index and object page; 9 is not a per-insert split-depth assumption.
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_credits_dominate_all_frozen_native_projections() {
        let worksheet = Worksheet::frozen().unwrap();
        for (kind, t) in &worksheet.transitions {
            let s = u128::from(t.segment_bytes);
            let n = s.div_ceil(4096);
            let o = u128::from(t.records.saturating_sub(1).min(216));
            let h = u128::from(t.counter_increments["index_cardinality"]).min(256);
            let g = if kind == "ENROLL" { 4 } else { 0 };
            let a = if matches!(kind.as_str(), "PREPARE_ENROLL" | "ENROLL") {
                0
            } else {
                128
            };
            let payload = 3 * s + 17508 * n + 44850 * o + 555805 * h + 30880 + 2083 * g + 10738 * a;
            let native = 19 + 10 * n + 14 * o + 19 * h + 5 * g + 5 * a + payload.div_ceil(4092);
            assert!(live_pages(kind, t).unwrap() >= native, "{kind}");
        }
        let transient = 4 * (2 * (9u128 * 1024 * 1024).div_ceil(4092) + 8 * 21 + 2);
        assert_eq!(transient, 19136);
        assert!(TRANSIENT_PAGES >= transient);
    }
}
