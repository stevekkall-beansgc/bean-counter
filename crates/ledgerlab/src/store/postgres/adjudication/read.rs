use super::*;
pub(super) async fn head<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
) -> Result<TrustedJournalHead, StoreError> {
    let row=c.query_opt("SELECT identity,ordinal,segment,replay_root FROM ledgerlab.r3_journals WHERE journal=$1",&[&journal_key(j)?]).await?;
    match row {
        None => prefix(
            j,
            Count::ZERO,
            Digest::parse(&"0".repeat(64)).map_err(core)?,
            Digest::parse(&"0".repeat(64)).map_err(core)?,
        ),
        Some(r) => {
            if r.try_get::<_, Vec<u8>>(0)? != identity(j)? {
                return Err(invalid());
            }
            prefix(
                j,
                ordinal(r.try_get(1)?)?,
                Digest::parse(r.try_get::<_, &str>(2)?).map_err(core)?,
                Digest::parse(r.try_get::<_, &str>(3)?).map_err(core)?,
            )
        }
    }
}
pub(super) async fn point<C: GenericClient + Sync>(
    c: &C,
    k: &HeadKey,
    maximum: usize,
) -> Result<ObservedHead, StoreError> {
    if k.full_key.is_empty() || k.full_key.len() > r3::MAX_KEY_BYTES {
        return Err(invalid());
    }
    let j = journal_key(&k.journal)?;
    let t = tag(k.kind);
    let meta=c.query_opt("SELECT revision,octet_length(value) FROM ledgerlab.r3_heads WHERE journal=$1 AND kind=$2 AND full_key=$3",&[&j,&t,&k.full_key]).await?;
    let Some(m) = meta else {
        return Ok(ObservedHead {
            key: k.clone(),
            revision: None,
            value: None,
        });
    };
    let n: i32 = m.try_get(1)?;
    if !(2..=r3::SEGMENT_BYTES as i32).contains(&n) {
        return Err(invalid());
    }
    if n as usize > maximum {
        return Err(StoreError::Overloaded);
    }
    let value: Vec<u8> = c
        .query_one(
            "SELECT value FROM ledgerlab.r3_heads WHERE journal=$1 AND kind=$2 AND full_key=$3",
            &[&j, &t, &k.full_key],
        )
        .await?
        .try_get(0)?;
    if value.len() != n as usize {
        return Err(invalid());
    }
    Ok(ObservedHead {
        key: k.clone(),
        revision: Some(ordinal(m.try_get(0)?)?),
        value: Some(value),
    })
}
pub(super) async fn saved<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    key: &wire::Delivery,
) -> Result<Option<SavedOutcome>, StoreError> {
    key.validate().map_err(core)?;
    if key.0 != j.scope {
        return Err(invalid());
    }
    let jk = journal_key(j)?;
    let delivery = r3::canonical_bytes(key, 4096).map_err(core)?;
    let m=c.query_opt("SELECT delivery,octet_length(command),octet_length(result),octet_length(receipt) FROM ledgerlab.r3_commands WHERE journal=$1 AND delivery_hash=sha256($2)",&[&jk,&delivery]).await?;
    let Some(m) = m else { return Ok(None) };
    // Compact hash selects one indexed row. Full exact identity is authoritative.
    if m.try_get::<_, Vec<u8>>(0)? != delivery {
        return Err(invalid());
    }
    let a: i32 = m.try_get(1)?;
    let b: i32 = m.try_get(2)?;
    let d: Option<i32> = m.try_get(3)?;
    if !(2..=r3::COMMAND_BYTES as i32).contains(&a)
        || !(2..=r3::SEGMENT_BYTES as i32).contains(&b)
        || d.is_some_and(|d| !(2..=8192).contains(&d))
    {
        return Err(invalid());
    }
    let r=c.query_one("SELECT c.command,c.result,c.receipt,c.ordinal,s.segment,s.replay_root FROM ledgerlab.r3_commands c JOIN ledgerlab.r3_segments s ON s.journal=c.journal AND s.ordinal=c.ordinal WHERE c.journal=$1 AND c.delivery_hash=sha256($2)",&[&jk,&delivery]).await?;
    let command: Vec<u8> = r.try_get(0)?;
    let result: Vec<u8> = r.try_get(1)?;
    let receipt: Option<Vec<u8>> = r.try_get(2)?;
    if command.len() != a as usize
        || result.len() != b as usize
        || receipt.as_ref().map(Vec::len) != d.map(|n| n as usize)
    {
        return Err(invalid());
    }
    let parsed = r3::ParsedCommand::parse(&command).map_err(core)?;
    let cv = runtime::command_value(parsed.command()).map_err(core)?;
    if cv["key"] != json!(key) {
        return Err(invalid());
    }
    let result: wire::CommandResult = r3::parse_exact(&result, r3::SEGMENT_BYTES).map_err(core)?;
    let root = Digest::parse(r.try_get::<_, &str>(5)?).map_err(core)?;
    if result.root != root {
        return Err(invalid());
    }
    Ok(Some(SavedOutcome {
        command,
        result,
        receipt: receipt
            .map(|b| r3::parse_exact(&b, 8192).map_err(core))
            .transpose()?,
        prefix: prefix(
            j,
            ordinal(r.try_get(3)?)?,
            Digest::parse(r.try_get::<_, &str>(4)?).map_err(core)?,
            root,
        )?,
    }))
}
pub(super) async fn segment_bytes<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    n: Count,
) -> Result<(Vec<u8>, Digest, Digest), StoreError> {
    let jk = journal_key(j)?;
    let requested = n;
    let n = number(n);
    let m=c.query_one("SELECT segment,replay_root,byte_length,page_count FROM ledgerlab.r3_segments WHERE journal=$1 AND ordinal=$2",&[&jk,&n]).await?;
    let length: i32 = m.try_get(2)?;
    let pages: i32 = m.try_get(3)?;
    if !(2..=r3::SEGMENT_BYTES as i32).contains(&length)
        || pages != (length as usize).div_ceil(r3::PAGE_BYTES) as i32
    {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(length as usize);
    for page in 0..pages {
        let b:Vec<u8>=c.query_one("SELECT bytes FROM ledgerlab.r3_segment_pages WHERE journal=$1 AND ordinal=$2 AND page=$3",&[&jk,&n,&page]).await?.try_get(0)?;
        let expected = (length as usize - bytes.len()).min(r3::PAGE_BYTES);
        if b.len() != expected {
            return Err(invalid());
        }
        bytes.extend(b);
    }
    let s: wire::Segment = r3::parse_exact(&bytes, r3::SEGMENT_BYTES).map_err(core)?;
    let digest = Digest::parse(m.try_get::<_, &str>(0)?).map_err(core)?;
    let root = Digest::parse(m.try_get::<_, &str>(1)?).map_err(core)?;
    if runtime::hash("segment", &s).map_err(core)? != digest
        || s.result.root != root
        || s.host != j.host
        || s.ordinal != requested
    {
        return Err(invalid());
    }
    Ok((bytes, digest, root))
}
pub(super) async fn source<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    n: Count,
    kind: &wire::FactKind,
    key: &wire::ProofFullKey,
) -> Result<VerifiedSource, StoreError> {
    let current = head(c, j).await?;
    if n == Count::ZERO || n > current.ordinal() {
        return Err(invalid());
    }
    let origin = wire::ObjectOrigin {
        store: j.store.clone(),
        scope: j.scope.clone(),
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: n,
    };
    let ob = r3::canonical_bytes(&origin, 2048).map_err(core)?;
    let kb = r3::canonical_bytes(key, 4096).map_err(core)?;
    let kval = json!(kind);
    let k = kval.as_str().ok_or_else(invalid)?;
    let jk = journal_key(j)?;
    let rows=c.query("SELECT origin,full_key,body_hash,byte_length FROM ledgerlab.r3_objects WHERE journal=$1 AND origin_hash=sha256($2) AND kind=$3 AND key_hash=sha256($4) LIMIT 2",&[&jk,&ob,&k,&kb]).await?;
    if rows.len() != 1
        || rows[0].try_get::<_, Vec<u8>>(0)? != ob
        || rows[0].try_get::<_, Vec<u8>>(1)? != kb
    {
        return Err(invalid());
    }
    let body_hash = Digest::parse(rows[0].try_get::<_, &str>(2)?).map_err(core)?;
    let length: i32 = rows[0].try_get(3)?;
    if !(2..=262144).contains(&length) {
        return Err(invalid());
    }
    let (bytes, segment, root) = segment_bytes(c, j, n).await?;
    let pin = prefix(j, n, segment.clone(), root.clone())?;
    if *kind == wire::FactKind::EnrollPreparation
        && (current.ordinal() != n || current.root() != &root || current.segment() != &segment)
    {
        return Err(invalid());
    }
    let p = wire::Proof {
        store: j.store.clone(),
        scope: j.scope.clone(),
        registration: j.registration.clone(),
        host: j.host.clone(),
        ordinal: n,
        segment,
        root,
        fact_kind: kind.clone(),
        full_key: key.clone(),
        body_hash,
        bytes: Count::new(length as u128).map_err(core)?,
        trusted_observation_ref: pin.observation().clone(),
    };
    VerifiedSource::from_backend(pin, p, &bytes).map_err(core)
}
pub(super) async fn retained<C: GenericClient + Sync>(
    c: &C,
    j: &JournalIdentity,
    p: &wire::Proof,
) -> Result<Option<wire::RetainedObject>, StoreError> {
    p.validate().map_err(core)?;
    let origin = wire::ObjectOrigin {
        store: p.store.clone(),
        scope: p.scope.clone(),
        registration: p.registration.clone(),
        host: p.host.clone(),
        ordinal: p.ordinal,
    };
    let ob = r3::canonical_bytes(&origin, 2048).map_err(core)?;
    let kb = r3::canonical_bytes(&p.full_key, 4096).map_err(core)?;
    let kval = json!(p.fact_kind);
    let k = kval.as_str().ok_or_else(invalid)?;
    let jk = journal_key(j)?;
    let row=c.query_opt("SELECT origin,full_key,byte_length,metadata FROM ledgerlab.r3_objects WHERE journal=$1 AND origin_hash=sha256($2) AND kind=$3 AND key_hash=sha256($4) AND body_hash=$5",&[&jk,&ob,&k,&kb,&p.body_hash.as_str()]).await?;
    let Some(m) = row else { return Ok(None) };
    let n: i32 = m.try_get(2)?;
    if m.try_get::<_, Vec<u8>>(0)? != ob
        || m.try_get::<_, Vec<u8>>(1)? != kb
        || !(2..=262144).contains(&n)
        || p.bytes.value() != n as u128
    {
        return Err(invalid());
    }
    let metadata: Vec<u8> = m.try_get(3)?;
    let expected=r3::canonical_bytes(&json!({"origin":origin,"kind":p.fact_kind,"full_key":p.full_key,"body_hash":p.body_hash,"bytes":p.bytes}),8192).map_err(core)?;
    if metadata != expected {
        return Err(invalid());
    }
    let mut body = Vec::with_capacity(n as usize);
    for page in 0..(n as usize).div_ceil(r3::PAGE_BYTES) {
        let r=c.query_one("SELECT origin,full_key,bytes FROM ledgerlab.r3_object_pages WHERE journal=$1 AND origin_hash=sha256($2) AND kind=$3 AND key_hash=sha256($4) AND body_hash=$5 AND page=$6",&[&jk,&ob,&k,&kb,&p.body_hash.as_str(),&(page as i32)]).await?;
        let b: Vec<u8> = r.try_get(2)?;
        if r.try_get::<_, Vec<u8>>(0)? != ob
            || r.try_get::<_, Vec<u8>>(1)? != kb
            || b.len() != (n as usize - body.len()).min(r3::PAGE_BYTES)
        {
            return Err(invalid());
        }
        body.extend(b);
    }
    let object = wire::RetainedObject {
        origin,
        kind: p.fact_kind.clone(),
        full_key: serde_json::from_value(json!(p.full_key)).map_err(|_| invalid())?,
        body: crate::service::accept::adjudication::encode_source(&body),
        body_hash: p.body_hash.clone(),
        bytes: p.bytes,
    };
    r3::proofs::VerifiedObjectBytes::check(object.clone()).map_err(core)?;
    Ok(Some(object))
}
