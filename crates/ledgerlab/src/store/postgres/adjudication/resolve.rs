use super::*;
pub(super) async fn locked<C: GenericClient + Sync>(
    c: &C,
    q: &ResolveRequest,
) -> Result<LockedInputs, StoreError> {
    if q.heads.len() > 256
        || q.objects.len() > 6
        || q.key.0 != q.journal.scope
        || q.heads.iter().any(|h| h.journal != q.journal)
    {
        return Err(invalid());
    }
    let mut heads = Vec::with_capacity(q.heads.len());
    let mut remaining = 64 * r3::COMMAND_BYTES;
    for key in &q.heads {
        if heads.iter().any(|h: &ObservedHead| h.key == *key) {
            return Err(invalid());
        }
        let h = read::point(c, key, remaining).await?;
        remaining = remaining
            .checked_sub(h.value.as_ref().map_or(0, Vec::len))
            .ok_or_else(invalid)?;
        heads.push(h);
    }
    let mut retained = Vec::new();
    for p in &q.objects {
        if let Some(o) = read::retained(c, &q.journal, p).await? {
            retained.push(o);
        }
    }
    Ok(LockedInputs {
        journal: q.journal.clone(),
        prefix: head(c, &q.journal).await?,
        heads,
        retained,
        sources: vec![],
    })
}
