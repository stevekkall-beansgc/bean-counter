//! PostgreSQL-only bounded observation queries. LEFT JOIN preserves missing rows.
use super::*;

const MEMBERS: &str = " FROM ledgerlab.outcome_members m LEFT JOIN ledgerlab.outcome_records r USING(tenant,environment,kind,id) WHERE m.tenant=$1 AND m.environment=$2 AND m.target=$3 AND m.invocation_id=$4";
const ANCHORS: &str = " FROM ledgerlab.outcome_anchors WHERE tenant=$1 AND environment=$2 AND target=$3 AND invocation_id=$4";
const DELIVERIES: &str = " FROM ledgerlab.outcome_members m LEFT JOIN ledgerlab.outcome_deliveries d ON (d.tenant,d.environment,d.settlement_kind,d.settlement_id)=(m.tenant,m.environment,m.kind,m.id) AND d.source=d.canonical_source AND d.external_id=d.canonical_external_id LEFT JOIN ledgerlab.outcome_records s ON (s.tenant,s.environment,s.kind,s.id)=(m.tenant,m.environment,m.kind,m.id) LEFT JOIN ledgerlab.outcome_records e ON (e.tenant,e.environment,e.kind,e.id)=(d.tenant,d.environment,d.economic_kind,d.economic_id) WHERE m.tenant=$1 AND m.environment=$2 AND m.target=$3 AND m.invocation_id=$4 AND m.kind='reservation-receipt'";

impl State {
    pub(super) async fn retained(
        &mut self,
        q: &RetainedSelection,
    ) -> Result<RawRetainedSnapshot, ReadError> {
        check(self.authority_scope.as_ref() == Some(&q.scope) && !self.retained_loaded)?;
        self.retained_loaded = true;
        scope_bound(&q.scope)?;
        bound(
            !q.target.is_empty()
                && q.target.len() <= 256
                && !q.invocation_id.is_empty()
                && q.invocation_id.len() <= 256,
        )?;
        let params: &[&(dyn ToSql + Sync)] =
            &[&q.scope[0], &q.scope[1], &q.target, &q.invocation_id];
        // Metadata and body lengths are fetched before either collection grows.
        let stats = self.one(&format!("SELECT count(*)::bigint,COALESCE(sum(octet_length(r.envelope)),0)::bigint,COALESCE(max(octet_length(r.envelope)),0)::bigint,COALESCE(sum(octet_length(m.kind)+octet_length(m.id)+octet_length(r.content_hash)+octet_length(m.tenant)+octet_length(m.environment)),0)::bigint,COALESCE(max(octet_length(m.id)),0)::bigint,COALESCE(max(GREATEST(octet_length(m.kind),octet_length(r.content_hash),octet_length(m.tenant),octet_length(m.environment))),0)::bigint,count(r.id)::bigint{MEMBERS}"), params).await?;
        let count = num(&stats, 0)?;
        if count == 0 {
            return Err(ReadError::NotFound);
        }
        ReadBudget::preflight_records(count, num(&stats, 1)?, num(&stats, 2)?)?;
        bound(num(&stats, 4)? <= 4096 && num(&stats, 5)? <= 128)?;
        check(num(&stats, 6)? == count)?;
        self.budget.charge(size(&stats, 1)?)?;
        self.budget.charge(size(&stats, 3)?)?;
        let rows = self
            .query(
                &format!("SELECT m.kind,m.id,r.content_hash{MEMBERS} ORDER BY m.kind,m.id"),
                params,
            )
            .await?;
        let members = rows
            .into_iter()
            .map(|r| reference_row(r, &q.scope))
            .collect::<Result<Vec<_>, _>>()?;
        let records = self
            .query(
                &format!("SELECT r.envelope{MEMBERS} ORDER BY m.kind,m.id"),
                params,
            )
            .await?
            .into_iter()
            .map(|r| r.try_get(0).map_err(db))
            .collect::<Result<Vec<Vec<u8>>, _>>()?;
        let stats = self.one(&format!("SELECT count(*)::bigint,COALESCE(sum(octet_length(kind)+octet_length(id)+octet_length(content_hash)+octet_length(tenant)+octet_length(environment)),0)::bigint,COALESCE(max(octet_length(id)),0)::bigint,COALESCE(max(GREATEST(octet_length(kind),octet_length(content_hash),octet_length(tenant),octet_length(environment))),0)::bigint{ANCHORS}"), params).await?;
        bound(num(&stats, 0)? <= 2 && num(&stats, 2)? <= 4096 && num(&stats, 3)? <= 128)?;
        check(num(&stats, 0)? == 2)?;
        self.budget.charge(size(&stats, 1)?)?;
        let anchors = self
            .query(
                &format!("SELECT kind,id,content_hash{ANCHORS} ORDER BY kind,id"),
                params,
            )
            .await?
            .into_iter()
            .map(|r| reference_row(r, &q.scope))
            .collect::<Result<Vec<_>, _>>()?;
        // Identify heads from frozen record fields only. The facade later checks
        // the full records and completeness against the original base policy.
        let mut keys = BTreeSet::new();
        for (class, part) in [
            (OutcomeLockClass::Target, &q.target),
            (OutcomeLockClass::Reservation, &q.invocation_id),
            (OutcomeLockClass::InvocationConsumption, &q.invocation_id),
            (OutcomeLockClass::BaseReversal, &q.target),
        ] {
            add_key(
                &mut self.budget,
                &mut keys,
                key(&q.scope, class, vec![json!(part)])?,
            )?;
        }
        for raw in &records {
            self.checkpoint().await?;
            let v = parsed(raw)?;
            let b = &v["body"];
            match text(&v["kind"])? {
                "binding-snapshot" => {
                    let id = text(&b["binding_id"])?;
                    bound(id.len() <= 128)?;
                    add_key(
                        &mut self.budget,
                        &mut keys,
                        key(&q.scope, OutcomeLockClass::Binding, vec![json!(id)])?,
                    )?;
                    add_key(
                        &mut self.budget,
                        &mut keys,
                        key(
                            &q.scope,
                            OutcomeLockClass::BindingAggregate,
                            vec![json!(q.target), json!(id)],
                        )?,
                    )?;
                }
                "policy-snapshot" => {
                    let agreement = text(&b["agreement_id"])?;
                    let family = text(&b["family_id"])?;
                    bound(agreement.len() <= 128 && family.len() <= 128)?;
                    add_key(
                        &mut self.budget,
                        &mut keys,
                        key(
                            &q.scope,
                            OutcomeLockClass::Claim,
                            vec![json!(agreement), json!(family), json!(q.target)],
                        )?,
                    )?;
                }
                _ => {}
            }
        }
        bound(keys.len() + self.heads <= MAX_HEADS)?;
        let mut heads = Vec::new();
        for lock in keys {
            heads.push(self.head(&q.scope, lock).await?);
        }
        let stats = self.one(&format!("SELECT count(*)::bigint,COALESCE(sum(octet_length(d.command)+octet_length(d.ingress)+octet_length(s.envelope)+COALESCE(octet_length(e.envelope),0)+2*octet_length(m.tenant)+2*octet_length(m.environment)+octet_length(d.source)+octet_length(d.external_id)+octet_length(d.canonical_source)+octet_length(d.canonical_external_id)+octet_length(d.ingress_hash)),0)::bigint,COALESCE(max(GREATEST(octet_length(d.command),octet_length(d.ingress),octet_length(s.envelope),COALESCE(octet_length(e.envelope),0))),0)::bigint,COALESCE(max(GREATEST(octet_length(d.source),octet_length(d.external_id),octet_length(d.canonical_source),octet_length(d.canonical_external_id),octet_length(d.ingress_hash))),0)::bigint,count(d.source)::bigint,count(s.id)::bigint,count(d.economic_id)::bigint,count(e.id)::bigint{DELIVERIES}"), params).await?;
        let count = num(&stats, 0)?;
        bound(
            count <= (MAX_STEPS + 1) as u64
                && num(&stats, 2)? <= MAX_ENVELOPE_BYTES as u64
                && num(&stats, 3)? <= 256,
        )?;
        check(
            count == num(&stats, 4)?
                && count == num(&stats, 5)?
                && num(&stats, 6)? == num(&stats, 7)?,
        )?;
        self.budget.charge(size(&stats, 1)?)?;
        let rows = self.query(&format!("SELECT d.source,d.external_id,d.canonical_source,d.canonical_external_id,d.command,d.ingress,d.ingress_hash,e.envelope,s.envelope{DELIVERIES} ORDER BY m.id,d.source,d.external_id"), params).await?;
        let original_deliveries = rows
            .into_iter()
            .map(|r| {
                Ok(StoredCompositeDelivery {
                    key: ScopedDelivery {
                        scope: q.scope.clone(),
                        source: r.try_get(0).map_err(db)?,
                        external_id: r.try_get(1).map_err(db)?,
                    },
                    canonical_key: ScopedDelivery {
                        scope: q.scope.clone(),
                        source: r.try_get(2).map_err(db)?,
                        external_id: r.try_get(3).map_err(db)?,
                    },
                    command: r.try_get(4).map_err(db)?,
                    ingress: r.try_get(5).map_err(db)?,
                    ingress_hash: r.try_get(6).map_err(db)?,
                    economic_receipt: r.try_get(7).map_err(db)?,
                    settlement_receipt: r.try_get(8).map_err(db)?,
                })
            })
            .collect::<Result<Vec<_>, ReadError>>()?;
        Ok(RawRetainedSnapshot {
            store_identity: self.identity.clone(),
            anchors,
            members,
            records,
            heads,
            original_deliveries,
        })
    }
}
fn num(r: &Row, i: usize) -> Result<u64, ReadError> {
    u64::try_from(r.try_get::<_, i64>(i).map_err(db)?).map_err(|_| ReadError::Integrity)
}
fn reference_row(r: Row, scope: &[String; 2]) -> Result<ScopedRecordRef, ReadError> {
    Ok(ScopedRecordRef {
        scope: scope.clone(),
        kind: r.try_get(0).map_err(db)?,
        id: r.try_get(1).map_err(db)?,
        content_hash: r.try_get(2).map_err(db)?,
    })
}
fn add_key(
    budget: &mut ReadBudget,
    keys: &mut BTreeSet<OutcomeLock>,
    k: OutcomeLock,
) -> Result<(), ReadError> {
    bound(keys.len() < MAX_HEADS)?;
    budget.charge(k.key.len())?;
    keys.insert(k);
    Ok(())
}

fn size(r: &Row, i: usize) -> Result<usize, ReadError> {
    usize::try_from(num(r, i)?).map_err(|_| ReadError::Limit)
}
