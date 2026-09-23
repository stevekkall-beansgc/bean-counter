//! Projection of a coordinator-owned plan into the caller's existing transaction.
use super::*;
use crate::service::accept::adjudication::ValidatedAdjudicationPlan;
use r3::{proofs::VerifiedObjectBytes, runtime, Validate};

pub(super) async fn reassert(
    c: &mut SqliteConnection,
    expected: &[ObservedHead],
) -> Result<(), StoreError> {
    for old in expected {
        let now = point(c, &old.key).await?;
        if now.revision != old.revision || now.value != old.value {
            return Err(StoreError::ExpectedCurrent);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        adjudication::ObjectPageRequest,
        sqlite::{tests::installation, SqliteStore},
    };
    use r3::types::Id;

    #[tokio::test]
    async fn r3_copied_facts_keep_distinct_source_origins_and_bounded_pages() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::create(dir.path(), installation())
            .await
            .unwrap();
        let j = JournalIdentity {
            store: Id::parse("store-demo-slice").unwrap(),
            scope: wire::Scope(Id::parse("tenant").unwrap(), Id::parse("env").unwrap()),
            registration: Id::parse("reg").unwrap(),
            host: Id::parse("center").unwrap(),
        };
        let key = journal_key(&j).unwrap();
        let n = 1u128.to_be_bytes();
        let mut c = store.inner.writer.acquire().await.unwrap();
        sqlx::query("INSERT INTO r3_journals VALUES (?,?,?,?,?)")
            .bind(&key)
            .bind(b"{}".as_slice())
            .bind(n.as_slice())
            .bind("a".repeat(64))
            .bind("b".repeat(64))
            .execute(&mut *c)
            .await
            .unwrap();
        sqlx::query("INSERT INTO r3_segments VALUES (?,?,?,?,?,?)")
            .bind(&key)
            .bind(n.as_slice())
            .bind("a".repeat(64))
            .bind("b".repeat(64))
            .bind(2i64)
            .bind(1i64)
            .execute(&mut *c)
            .await
            .unwrap();
        let first = wire::RetainedObject {
            origin: wire::ObjectOrigin {
                store: j.store.clone(),
                scope: j.scope.clone(),
                registration: j.registration.clone(),
                host: Id::parse("gateway-one").unwrap(),
                ordinal: Count::new(7).unwrap(),
            },
            kind: wire::FactKind::Receipt,
            full_key: wire::RetainedObjectFullKey::V2(Id::parse("same-id").unwrap()),
            body: "e30=".into(),
            body_hash: r3::raw_sha256(b"{}"),
            bytes: Count::new(2).unwrap(),
        };
        let mut second = first.clone();
        second.origin.host = Id::parse("gateway-two").unwrap();
        object(&mut c, &key, Count::new(1).unwrap(), &first)
            .await
            .unwrap();
        object(&mut c, &key, Count::new(1).unwrap(), &second)
            .await
            .unwrap();
        assert!(object(&mut c, &key, Count::new(1).unwrap(), &first)
            .await
            .is_err());
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM r3_objects")
            .fetch_one(&mut *c)
            .await
            .unwrap();
        assert_eq!(rows, 2);
        let q = ObjectPageRequest {
            origin: second.origin.clone(),
            kind: second.kind.clone(),
            key: r3::canonical_bytes(&second.full_key, 4096).unwrap(),
            hash: second.body_hash.clone(),
            offset: Count::new(1).unwrap(),
            max_bytes: 4096,
        };
        assert_eq!(object_page(&mut c, &j, &q).await.unwrap(), b"}");
        let mut bad = q.clone();
        bad.origin.ordinal = Count::new(1).unwrap();
        assert!(object_page(&mut c, &j, &bad).await.is_err());
        drop(c);
        store.close().await;
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        let mut c = reopened.inner.readers.acquire().await.unwrap();
        assert_eq!(object_page(&mut c, &j, &q).await.unwrap(), b"}");
        drop(c);
        reopened.close().await;
    }
}

pub(super) async fn object(
    c: &mut SqliteConnection,
    journal: &[u8],
    introduced: Count,
    object: &wire::RetainedObject,
) -> Result<(), StoreError> {
    let checked = VerifiedObjectBytes::check(object.clone()).map_err(core)?;
    let origin = r3::canonical_bytes(&object.origin, 2048).map_err(core)?;
    let kind = serde_json::to_value(&object.kind).map_err(|_| invalid())?;
    let kind = kind.as_str().ok_or_else(invalid)?;
    let key = r3::canonical_bytes(&object.full_key, 4096).map_err(core)?;
    let metadata = r3::canonical_bytes(&json!({"origin":object.origin,"kind":object.kind,"full_key":object.full_key,"body_hash":object.body_hash,"bytes":object.bytes}),8192).map_err(core)?;
    sqlx::query("INSERT INTO r3_objects (journal,ordinal,kind,origin,full_key,body_hash,byte_length,metadata) VALUES (?,?,?,?,?,?,?,?)")
        .bind(journal).bind(introduced.value().to_be_bytes().as_slice()).bind(kind).bind(&origin).bind(&key).bind(object.body_hash.as_str()).bind(checked.bytes().len() as i64).bind(metadata).execute(&mut *c).await?;
    for (page, bytes) in checked.bytes().chunks(r3::PAGE_BYTES).enumerate() {
        sqlx::query("INSERT INTO r3_object_pages VALUES (?,?,?,?,?,?,?)")
            .bind(journal)
            .bind(&origin)
            .bind(kind)
            .bind(&key)
            .bind(object.body_hash.as_str())
            .bind(page as i64)
            .bind(bytes)
            .execute(&mut *c)
            .await?;
    }
    Ok(())
}

/// No commit and no semantic acceptance happen here. The owning adapter must
/// retain its live work/exclusion capability and poison the Tx around this await.
pub(super) async fn plan(
    c: &mut SqliteConnection,
    p: &ValidatedAdjudicationPlan,
) -> Result<(), StoreError> {
    let j = p.journal();
    let journal = journal_key(j)?;
    let prior = head(c, j).await?;
    let s = p.segment();
    s.validate().map_err(core)?;
    if prior.ordinal() != p.prior().ordinal()
        || prior.segment() != p.prior().segment()
        || prior.root() != p.prior().root()
        || s.ordinal
            != prior
                .ordinal()
                .checked_add(Count::new(1).map_err(core)?)
                .map_err(core)?
        || s.previous != *prior.segment()
        || s.previous_root != *prior.root()
        || s.host != j.host
        || s.command != *p.command().command()
        || r3::canonical_bytes(s, r3::SEGMENT_BYTES).map_err(core)? != p.segment_bytes()
    {
        return Err(invalid());
    }
    reassert(c, p.observed()).await?;
    if let Some(base) = p.base() {
        reassert(c, base.absence()).await?;
    }
    let segment = runtime::hash("segment", s).map_err(core)?;
    let n = s.ordinal.value().to_be_bytes();
    if prior.ordinal() == Count::ZERO {
        let identity =
            r3::canonical_bytes(&json!([j.store, j.scope, j.registration, j.host]), 4096)
                .map_err(core)?;
        sqlx::query("INSERT INTO r3_journals VALUES (?,?,?,?,?)")
            .bind(&journal)
            .bind(identity)
            .bind(n.as_slice())
            .bind(segment.as_str())
            .bind(s.result.root.as_str())
            .execute(&mut *c)
            .await?;
    } else {
        let updated=sqlx::query("UPDATE r3_journals SET ordinal=?,segment=?,replay_root=? WHERE journal=? AND ordinal=? AND segment=? AND replay_root=?")
            .bind(n.as_slice()).bind(segment.as_str()).bind(s.result.root.as_str()).bind(&journal).bind(prior.ordinal().value().to_be_bytes().as_slice()).bind(prior.segment().as_str()).bind(prior.root().as_str()).execute(&mut *c).await?;
        if updated.rows_affected() != 1 {
            return Err(invalid());
        }
    }
    sqlx::query("INSERT INTO r3_segments VALUES (?,?,?,?,?,?)")
        .bind(&journal)
        .bind(n.as_slice())
        .bind(segment.as_str())
        .bind(s.result.root.as_str())
        .bind(p.segment_bytes().len() as i64)
        .bind(p.segment_bytes().len().div_ceil(r3::PAGE_BYTES) as i64)
        .execute(&mut *c)
        .await?;
    for (page, bytes) in p.segment_bytes().chunks(r3::PAGE_BYTES).enumerate() {
        sqlx::query("INSERT INTO r3_segment_pages VALUES (?,?,?,?)")
            .bind(&journal)
            .bind(n.as_slice())
            .bind(page as i64)
            .bind(bytes)
            .execute(&mut *c)
            .await?;
    }
    for o in &s.objects {
        object(c, &journal, s.ordinal, o).await?;
    }
    let cv = runtime::command_value(p.command().command()).map_err(core)?;
    let delivery: wire::Delivery =
        serde_json::from_value(cv["key"].clone()).map_err(|_| invalid())?;
    delivery.validate().map_err(core)?;
    let mut receipt = None;
    for w in p.head_writes() {
        if w.key.journal != *j
            || w.revision
                != w.expected
                    .unwrap_or(Count::ZERO)
                    .checked_add(Count::new(1).map_err(core)?)
                    .map_err(core)?
        {
            return Err(invalid());
        }
        if !p
            .observed()
            .iter()
            .any(|o| o.key == w.key && o.revision == w.expected)
        {
            return Err(invalid());
        }
        match w.expected {
            None => {
                sqlx::query("INSERT INTO r3_heads VALUES (?,?,?,?,?)")
                    .bind(&journal)
                    .bind(head_tag(w.key.kind))
                    .bind(&w.key.full_key)
                    .bind(w.revision.value().to_be_bytes().as_slice())
                    .bind(&w.value)
                    .execute(&mut *c)
                    .await?;
            }
            Some(old) => {
                let updated=sqlx::query("UPDATE r3_heads SET revision=?,value=? WHERE journal=? AND kind=? AND full_key=? AND revision=?").bind(w.revision.value().to_be_bytes().as_slice()).bind(&w.value).bind(&journal).bind(head_tag(w.key.kind)).bind(&w.key.full_key).bind(old.value().to_be_bytes().as_slice()).execute(&mut *c).await?;
                if updated.rows_affected() != 1 {
                    return Err(invalid());
                }
            }
        }
        sqlx::query("INSERT INTO r3_head_versions VALUES (?,?,?,?,?,?)")
            .bind(&journal)
            .bind(head_tag(w.key.kind))
            .bind(&w.key.full_key)
            .bind(n.as_slice())
            .bind(w.revision.value().to_be_bytes().as_slice())
            .bind(&w.value)
            .execute(&mut *c)
            .await?;
        if w.key.kind == HeadKind::Delivery {
            let value = ledgerlab_core::canonical::parse_bounded(&w.value, r3::COMMAND_BYTES)
                .map_err(core)?;
            if value["kind"] != "Delivery" {
                return Err(invalid());
            }
            let state: runtime::DeliveryState =
                serde_json::from_value(value["body"].clone()).map_err(|_| invalid())?;
            if runtime::delivery_key(&state.delivery).map_err(core)? != w.key.full_key {
                return Err(invalid());
            }
            state.receipt.validate().map_err(core)?;
            let exact = r3::canonical_bytes(&state, 16384).map_err(core)?;
            sqlx::query("INSERT INTO r3_deliveries VALUES (?,?,?,?,?,?)")
                .bind(state.delivery.0 .0.as_str())
                .bind(state.delivery.0 .1.as_str())
                .bind(state.delivery.1.as_str())
                .bind(state.delivery.2.as_str())
                .bind(&journal)
                .bind(exact)
                .execute(&mut *c)
                .await?;
            if state.delivery == delivery {
                receipt = Some(r3::canonical_bytes(&state.receipt, 8192).map_err(core)?);
            }
        }
    }
    if let wire::Command::Enroll { payload, .. } = &s.command {
        for ns in &payload.gateways {
            sqlx::query("INSERT INTO r3_namespaces VALUES (?,?,?,?,?)")
                .bind(ns.scope.0.as_str())
                .bind(ns.scope.1.as_str())
                .bind(&ns.tag)
                .bind(ns.gateway.as_str())
                .bind(&journal)
                .execute(&mut *c)
                .await?;
        }
    }
    for index in p.indices() {
        let old:Option<String>=sqlx::query_scalar("SELECT root FROM r3_index_roots WHERE journal=? AND full_key=? ORDER BY ordinal DESC LIMIT 1").bind(&journal).bind(&index.full_key).fetch_optional(&mut *c).await?;
        if old.as_deref().unwrap_or(&"0".repeat(64)) != index.old_root.as_str() {
            return Err(invalid());
        }
        for bytes in &index.retained_pages {
            let hash = r3::raw_sha256(bytes);
            let old: Option<Vec<u8>> =
                sqlx::query_scalar("SELECT bytes FROM r3_index_pages WHERE journal=? AND hash=?")
                    .bind(&journal)
                    .bind(hash.as_str())
                    .fetch_optional(&mut *c)
                    .await?;
            match old {
                Some(old) if old != *bytes => return Err(invalid()),
                Some(_) => {}
                None => {
                    sqlx::query("INSERT INTO r3_index_pages VALUES (?,?,?)")
                        .bind(&journal)
                        .bind(hash.as_str())
                        .bind(bytes)
                        .execute(&mut *c)
                        .await?;
                }
            }
        }
        sqlx::query("INSERT INTO r3_index_roots VALUES (?,?,?,?)")
            .bind(&journal)
            .bind(&index.full_key)
            .bind(n.as_slice())
            .bind(index.new_root.as_str())
            .execute(&mut *c)
            .await?;
    }
    for (position, action) in p.held_intentions().iter().enumerate() {
        sqlx::query(
            "INSERT INTO r3_held_intentions (journal,ordinal,position,action) VALUES (?,?,?,?)",
        )
        .bind(&journal)
        .bind(n.as_slice())
        .bind(position as i64)
        .bind(r3::canonical_bytes(action, 8192).map_err(core)?)
        .execute(&mut *c)
        .await?;
    }
    sqlx::query("INSERT INTO r3_commands VALUES (?,?,?,?,?,?)")
        .bind(&journal)
        .bind(r3::canonical_bytes(&delivery, 4096).map_err(core)?)
        .bind(n.as_slice())
        .bind(p.command().bytes())
        .bind(r3::canonical_bytes(&s.result, r3::SEGMENT_BYTES).map_err(core)?)
        .bind(receipt)
        .execute(c)
        .await?;
    Ok(())
}
