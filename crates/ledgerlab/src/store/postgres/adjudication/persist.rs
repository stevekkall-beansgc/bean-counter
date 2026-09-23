//! Coordinator-plan projection only. Physical admission remains unavailable;
//! this layer never issues a commit capability or evaluates business policy.
use super::*;
use crate::service::accept::adjudication::OriginalBaseWrites;
use std::collections::BTreeSet;
pub(super) async fn reassert<C: GenericClient + Sync>(
    c: &C,
    expected: &[ObservedHead],
) -> Result<(), StoreError> {
    for old in expected {
        let now = read::point(c, &old.key, r3::SEGMENT_BYTES).await?;
        if old.revision != now.revision || old.value != now.value {
            return Err(StoreError::ExpectedCurrent);
        }
    }
    Ok(())
}
pub(super) async fn object<C: GenericClient + Sync>(
    c: &C,
    journal: &[u8],
    introduced: Count,
    o: &wire::RetainedObject,
) -> Result<(), StoreError> {
    let checked = r3::proofs::VerifiedObjectBytes::check(o.clone()).map_err(core)?;
    let origin = r3::canonical_bytes(&o.origin, 2048).map_err(core)?;
    let key = r3::canonical_bytes(&o.full_key, 4096).map_err(core)?;
    let k = json!(o.kind);
    let k = k.as_str().ok_or_else(invalid)?;
    let old=c.query_opt("SELECT origin,full_key FROM ledgerlab.r3_objects WHERE journal=$1 AND origin_hash=sha256($2) AND kind=$3 AND key_hash=sha256($4) LIMIT 1",&[&journal,&origin,&k,&key]).await?;
    if old.is_some() {
        return Err(invalid());
    } // Full identity is immutable, even if a new body hash is supplied.
    let metadata=r3::canonical_bytes(&json!({"origin":o.origin,"kind":o.kind,"full_key":o.full_key,"body_hash":o.body_hash,"bytes":o.bytes}),8192).map_err(core)?;
    c.execute("INSERT INTO ledgerlab.r3_objects(journal,ordinal,kind,origin,full_key,body_hash,byte_length,metadata) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",&[&journal,&number(introduced),&k,&origin,&key,&o.body_hash.as_str(),&(checked.bytes().len() as i32),&metadata]).await?;
    for (page, bytes) in checked.bytes().chunks(r3::PAGE_BYTES).enumerate() {
        c.execute("INSERT INTO ledgerlab.r3_object_pages(journal,origin,kind,full_key,body_hash,page,bytes) VALUES($1,$2,$3,$4,$5,$6,$7)",&[&journal,&origin,&k,&key,&o.body_hash.as_str(),&(page as i32),&bytes]).await?;
    }
    Ok(())
}
pub(super) async fn segment<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    prior: &TrustedJournalHead,
    s: &wire::Segment,
    bytes: &[u8],
) -> Result<Digest, StoreError> {
    s.validate().map_err(core)?;
    if s.host != j.host
        || prior.journal() != j
        || s.ordinal
            != prior
                .ordinal()
                .checked_add(Count::new(1).map_err(core)?)
                .map_err(core)?
        || s.previous != *prior.segment()
        || s.previous_root != *prior.root()
        || r3::canonical_bytes(s, r3::SEGMENT_BYTES).map_err(core)? != bytes
    {
        return Err(invalid());
    }
    let now = head(c, j).await?;
    if now.ordinal() != prior.ordinal()
        || now.segment() != prior.segment()
        || now.root() != prior.root()
    {
        return Err(StoreError::ExpectedCurrent);
    }
    let jk = journal_key(j)?;
    let digest = runtime::hash("segment", s).map_err(core)?;
    let n = number(s.ordinal);
    let zero = "0".repeat(64);
    let changed = if prior.ordinal() == Count::ZERO {
        c.execute("INSERT INTO ledgerlab.r3_journals(journal,identity,ordinal,segment,replay_root) VALUES($1,$2,$3,$4,$5) ON CONFLICT(journal) DO UPDATE SET ordinal=excluded.ordinal,segment=excluded.segment,replay_root=excluded.replay_root WHERE r3_journals.identity=excluded.identity AND r3_journals.ordinal=$6 AND r3_journals.segment=$7 AND r3_journals.replay_root=$7",&[&jk,&identity(j)?,&n,&digest.as_str(),&s.result.root.as_str(),&number(Count::ZERO),&zero]).await?
    } else {
        c.execute("UPDATE ledgerlab.r3_journals SET ordinal=$1,segment=$2,replay_root=$3 WHERE journal=$4 AND ordinal=$5 AND segment=$6 AND replay_root=$7",&[&n,&digest.as_str(),&s.result.root.as_str(),&jk,&number(prior.ordinal()),&prior.segment().as_str(),&prior.root().as_str()]).await?
    };
    if changed != 1 {
        return Err(StoreError::ExpectedCurrent);
    }
    c.execute("INSERT INTO ledgerlab.r3_segments(journal,ordinal,segment,replay_root,byte_length,page_count) VALUES($1,$2,$3,$4,$5,$6)",&[&jk,&n,&digest.as_str(),&s.result.root.as_str(),&(bytes.len() as i32),&(bytes.len().div_ceil(r3::PAGE_BYTES) as i32)]).await?;
    for (page, b) in bytes.chunks(r3::PAGE_BYTES).enumerate() {
        c.execute("INSERT INTO ledgerlab.r3_segment_pages(journal,ordinal,page,bytes) VALUES($1,$2,$3,$4)",&[&jk,&n,&(page as i32),&b]).await?;
    }
    Ok(digest)
}
pub(super) async fn write_head<C: GenericClient + Sync>(
    c: &C,
    n: Count,
    w: &HeadWrite,
) -> Result<(), StoreError> {
    if w.key.full_key.is_empty()
        || w.key.full_key.len() > r3::MAX_KEY_BYTES
        || w.value.len() > r3::SEGMENT_BYTES
        || w.revision
            != w.expected
                .unwrap_or(Count::ZERO)
                .checked_add(Count::new(1).map_err(core)?)
                .map_err(core)?
    {
        return Err(invalid());
    }
    let jk = journal_key(&w.key.journal)?;
    let t = tag(w.key.kind);
    let changed=match w.expected {
        None=>c.execute("INSERT INTO ledgerlab.r3_heads(journal,kind,full_key,revision,value) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",&[&jk,&t,&w.key.full_key,&number(w.revision),&w.value]).await?,
        Some(old)=>c.execute("UPDATE ledgerlab.r3_heads SET revision=$1,value=$2 WHERE journal=$3 AND kind=$4 AND full_key=$5 AND revision=$6",&[&number(w.revision),&w.value,&jk,&t,&w.key.full_key,&number(old)]).await?
    };
    if changed != 1 {
        return Err(StoreError::ExpectedCurrent);
    }
    c.execute("INSERT INTO ledgerlab.r3_head_versions(journal,kind,full_key,ordinal,revision,value) VALUES($1,$2,$3,$4,$5,$6)",&[&jk,&t,&w.key.full_key,&number(n),&number(w.revision),&w.value]).await?;
    Ok(())
}
pub(super) async fn command<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    s: &wire::Segment,
    exact: &[u8],
    receipt: Option<&[u8]>,
) -> Result<(), StoreError> {
    let parsed = r3::ParsedCommand::parse(exact).map_err(core)?;
    if parsed.command() != &s.command {
        return Err(invalid());
    }
    let v = runtime::command_value(&s.command).map_err(core)?;
    let key = r3::canonical_bytes(&v["key"], 4096).map_err(core)?;
    c.execute("INSERT INTO ledgerlab.r3_commands(journal,delivery,ordinal,command,result,receipt) VALUES($1,$2,$3,$4,$5,$6)",&[&journal_key(j)?,&key,&number(s.ordinal),&exact,&r3::canonical_bytes(&s.result,r3::SEGMENT_BYTES).map_err(core)?,&receipt]).await?;
    Ok(())
}
pub(super) async fn plan<C: GenericClient + Sync>(
    c: &C,
    held: &Locked,
    legacy: &super::super::outcomes::Locked,
    steps: &mut super::super::outcomes::Steps,
    p: &ValidatedAdjudicationPlan,
) -> Result<(), StoreError> {
    if held.appended
        || held.guards != p.guards()
        || held.journal.as_ref() != Some(p.journal())
        || p.segment().command != *p.command().command()
        || !p.indices().is_empty()
    {
        return Err(invalid());
    }
    // Native SQL uses bounded composite indexes; logical radix page updates are
    // intentionally unsupported rather than silently ignored.
    reassert(c, p.observed()).await?;
    if let Some(base) = p.base() {
        reassert(c, base.absence()).await?;
        match base.writes() {
            OriginalBaseWrites::OriginalV2(b) => {
                super::super::outcomes::append_original(
                    c,
                    legacy,
                    b,
                    &journal_key(p.journal())?,
                    steps,
                )
                .await?
            }
            OriginalBaseWrites::V2(b) => {
                super::super::outcomes::append(c, legacy, b, steps).await?
            }
            OriginalBaseWrites::V1 { writes, .. } => {
                for op in writes {
                    super::super::write::operation(c, op).await?;
                }
            }
        }
    }
    let s = p.segment();
    segment(c, p.journal(), p.prior(), s, p.segment_bytes()).await?;
    let jk = journal_key(p.journal())?;
    for o in &s.objects {
        object(c, &jk, s.ordinal, o).await?;
    }
    let cv = runtime::command_value(p.command().command()).map_err(core)?;
    let delivery: wire::Delivery =
        serde_json::from_value(cv["key"].clone()).map_err(|_| invalid())?;
    let mut receipt = None;
    let mut keys = BTreeSet::new();
    for w in p.head_writes() {
        if w.key.journal != *p.journal()
            || !keys.insert((tag(w.key.kind), w.key.full_key.clone()))
            || !p
                .observed()
                .iter()
                .any(|o| o.key == w.key && o.revision == w.expected)
        {
            return Err(invalid());
        }
        write_head(c, s.ordinal, w).await?;
        if w.key.kind == HeadKind::Delivery {
            let value = ledgerlab_core::canonical::parse_bounded(&w.value, r3::COMMAND_BYTES)
                .map_err(core)?;
            if value["kind"] != "Delivery" {
                return Err(invalid());
            }
            let d: runtime::DeliveryState =
                serde_json::from_value(value["body"].clone()).map_err(|_| invalid())?;
            if runtime::delivery_key(&d.delivery).map_err(core)? != w.key.full_key {
                return Err(invalid());
            }
            d.receipt.validate().map_err(core)?;
            c.execute("INSERT INTO ledgerlab.r3_deliveries(tenant,environment,source,external_id,journal,value) VALUES($1,$2,$3,$4,$5,$6)",&[&d.delivery.0.0.as_str(),&d.delivery.0.1.as_str(),&d.delivery.1.as_str(),&d.delivery.2.as_str(),&jk,&r3::canonical_bytes(&d,16384).map_err(core)?]).await?;
            if d.delivery == delivery {
                receipt = Some(r3::canonical_bytes(&d.receipt, 8192).map_err(core)?);
            }
        }
    }
    if let wire::Command::Enroll { payload, .. } = &s.command {
        for ns in &payload.gateways {
            c.execute("INSERT INTO ledgerlab.r3_namespaces(tenant,environment,tag,gateway,journal) VALUES($1,$2,$3,$4,$5)",&[&ns.scope.0.as_str(),&ns.scope.1.as_str(),&ns.tag,&ns.gateway.as_str(),&jk]).await?;
        }
    }
    for (i, a) in p.held_intentions().iter().enumerate() {
        c.execute("INSERT INTO ledgerlab.r3_held_intentions(journal,ordinal,position,action) VALUES($1,$2,$3,$4)",&[&jk,&number(s.ordinal),&(i as i32),&r3::canonical_bytes(a,8192).map_err(core)?]).await?;
    }
    command(c, p.journal(), s, p.command().bytes(), receipt.as_deref()).await
}
