use super::*;
use crate::store::adjudication::{LockedInputs, ResolveRequest};

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        out.push(A[(c[0] >> 2) as usize] as char);
        out.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        out.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

async fn retained(
    c: &mut SqliteConnection,
    j: &JournalIdentity,
    p: &wire::Proof,
) -> Result<Option<wire::RetainedObject>, StoreError> {
    let origin = wire::ObjectOrigin {
        store: p.store.clone(),
        scope: p.scope.clone(),
        registration: p.registration.clone(),
        host: p.host.clone(),
        ordinal: p.ordinal,
    };
    let q = crate::store::adjudication::ObjectPageRequest {
        origin,
        kind: p.fact_kind.clone(),
        key: r3::canonical_bytes(&p.full_key, 4096).map_err(core)?,
        hash: p.body_hash.clone(),
        offset: Count::ZERO,
        max_bytes: r3::PAGE_BYTES as u16,
    };
    let journal = journal_key(j)?;
    let origin = r3::canonical_bytes(&q.origin, 2048).map_err(core)?;
    let kind = serde_json::to_value(&q.kind).map_err(|_| invalid())?;
    let row=sqlx::query("SELECT byte_length,length(metadata) FROM r3_objects WHERE journal=? AND origin=? AND kind=? AND full_key=? AND body_hash=?")
        .bind(&journal).bind(&origin).bind(kind.as_str().ok_or_else(invalid)?).bind(&q.key).bind(q.hash.as_str()).fetch_optional(&mut *c).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let length: i64 = row.try_get(0)?;
    let metadata: i64 = row.try_get(1)?;
    if !(2..=262144).contains(&length)
        || !(2..=8192).contains(&metadata)
        || p.bytes.value() != length as u128
    {
        return Err(invalid());
    }
    let mut body = Vec::with_capacity(length as usize);
    for n in 0..(length as usize).div_ceil(r3::PAGE_BYTES) {
        let mut page = q.clone();
        page.offset = Count::new((n * r3::PAGE_BYTES) as u128).map_err(core)?;
        body.extend(object_page(c, j, &page).await?);
    }
    if body.len() != length as usize {
        return Err(invalid());
    }
    let object = wire::RetainedObject {
        origin: q.origin,
        kind: q.kind,
        full_key: serde_json::from_value(serde_json::to_value(&p.full_key).map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        body: base64(&body),
        body_hash: q.hash,
        bytes: p.bytes,
    };
    r3::proofs::VerifiedObjectBytes::check(object.clone()).map_err(core)?;
    Ok(Some(object))
}

pub(super) async fn locked(
    c: &mut SqliteConnection,
    q: &ResolveRequest,
) -> Result<LockedInputs, StoreError> {
    if q.heads.len() > 256
        || q.objects.len() > 6
        || q.key.0 != q.journal.scope
        || q.heads.iter().any(|h| h.journal != q.journal)
    {
        return Err(invalid());
    }
    let prefix = head(c, &q.journal).await?;
    let mut heads = Vec::with_capacity(q.heads.len());
    let mut materialized = 0usize;
    for key in &q.heads {
        if heads.iter().any(|h: &ObservedHead| h.key == *key) {
            return Err(invalid());
        }
        let observed = point_limited(c, key, 64 * r3::COMMAND_BYTES - materialized).await?;
        materialized = materialized
            .checked_add(observed.value.as_ref().map_or(0, Vec::len))
            .ok_or_else(invalid)?;
        if materialized > 64 * r3::COMMAND_BYTES {
            return Err(StoreError::Overloaded);
        }
        heads.push(observed);
    }
    let mut objects = Vec::with_capacity(q.objects.len());
    for proof in &q.objects {
        if let Some(object) = retained(c, &q.journal, proof).await? {
            objects.push(object);
        }
    }
    Ok(LockedInputs {
        journal: q.journal.clone(),
        prefix,
        heads,
        retained: objects,
        sources: vec![],
    })
}
