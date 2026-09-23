//! Private snapshot mapper. Only the existing reader pool is reachable here.
//! SQL preflights and bodies run in one deferred transaction. No pricing or
//! authorization decision is made by this module.
#![allow(dead_code)]
use super::SqliteStore;
use crate::store::{comparison::*, outcomes::*};
use ledgerlab_core::canonical;
use serde_json::{json, Value};
use sqlx::{sqlite::SqliteRow, AssertSqlSafe, Row, Sqlite, Transaction};
use std::collections::BTreeSet;
use tokio::time::{timeout_at, Duration, Instant};

type Result<T> = std::result::Result<T, ReadError>;
fn db(_: sqlx::Error) -> ReadError {
    ReadError::Unavailable
}
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ReadError::Integrity)
    }
}
fn bound(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ReadError::Limit)
    }
}
fn encode(v: &Value) -> Result<Vec<u8>> {
    canonical::CanonicalBytes::from_value(v)
        .map(|b| b.as_slice().to_vec())
        .map_err(|_| ReadError::Integrity)
}
// Measure a reference's encoded identity before the canonical encoder allocates.
fn bounded_identity(v: &Value) -> Result<Vec<u8>> {
    struct Measure(usize);
    impl std::io::Write for Measure {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= 4096)
                .ok_or_else(|| std::io::Error::other("reference limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Measure(0), v).map_err(|_| ReadError::Limit)?;
    let id = encode(v)?;
    bound(id.len() <= 4096)?;
    Ok(id)
}
fn parse(b: &[u8]) -> Result<Value> {
    canonical::parse_bounded(b, MAX_ENVELOPE_BYTES).map_err(|_| ReadError::Integrity)
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    let s = v[k].as_str().ok_or(ReadError::Integrity)?;
    bound(!s.is_empty() && s.len() <= 128)?;
    Ok(s)
}
fn class(c: OutcomeLockClass) -> i64 {
    match c {
        OutcomeLockClass::Admission => 0,
        OutcomeLockClass::Authority => 1,
        OutcomeLockClass::Binding => 2,
        OutcomeLockClass::Reservation => 3,
        OutcomeLockClass::Target => 4,
        OutcomeLockClass::Claim => 5,
        OutcomeLockClass::BindingAggregate => 6,
        OutcomeLockClass::InvocationConsumption => 7,
        OutcomeLockClass::BaseReversal => 8,
    }
}
/// Every application statement is recorded before submission in tests. The
/// production gate accepts only this module's single SELECT statements. BEGIN
/// and ROLLBACK remain SQLx's tracked transaction lifecycle, never caller SQL.
#[derive(Default, Clone)]
struct ReadStatements {
    #[cfg(test)]
    trace: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    #[cfg(test)]
    label: &'static str,
    #[cfg(test)]
    audit: bool,
}
impl ReadStatements {
    fn select(&mut self, sql: impl Into<String>) -> Result<AssertSqlSafe<String>> {
        let sql = sql.into();
        #[cfg(test)]
        self.trace.lock().unwrap().push(sql.clone());
        require(sql.starts_with("SELECT ") && !sql.contains(';'))?;
        Ok(AssertSqlSafe(sql))
    }
}
pub(crate) struct SqliteComparisonRead {
    transaction: Option<Transaction<'static, Sqlite>>,
    deadline: Instant,
    budget: ReadBudget,
    statements: ReadStatements,
    // A cancelled method cannot be resumed with a partly loaded budget/snapshot.
    ready: bool,
    authority_scope: Option<[String; 2]>,
    bounded: Option<lease::Bounded>,
}
impl ComparisonReadStore for SqliteStore {
    type Read = SqliteComparisonRead;
    async fn begin_read(&self, deadline: Instant) -> Result<Self::Read> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        timeout_at(deadline, async {
            let lane = if self
                .inner
                .adjudication_enabled
                .load(std::sync::atomic::Ordering::Acquire)
            {
                Some(
                    std::sync::Arc::clone(&self.inner.adjudication_gate)
                        .read_owned()
                        .await,
                )
            } else {
                None
            };
            self.require_published()
                .map_err(|_| ReadError::Unavailable)?;
            let mut connection = self.inner.readers.acquire().await.map_err(db)?;
            // Even an interrupted BEGIN/ROLLBACK discards this session; it can
            // never return to the pool with an uncertain transaction state.
            connection.close_on_drop();
            connection
                .lock_handle()
                .await
                .map_err(db)?
                .set_progress_handler(1000, move || Instant::now() < deadline);
            let mut transaction = Transaction::begin(connection, None).await.map_err(db)?;
            // BEGIN is deferred. This first application SELECT pins the snapshot,
            // including on an empty installation. It does not mutate pragmas.
            let mut statements = ReadStatements::default();
            #[cfg(test)]
            {
                statements.audit = true;
                statements.label = "begin";
            }
            let _: i64 =
                sqlx::query_scalar(statements.select("SELECT count(*) FROM installation")?)
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(db)?;
            let read = SqliteComparisonRead {
                transaction: Some(transaction),
                deadline,
                budget: ReadBudget::default(),
                statements,
                ready: true,
                authority_scope: None,
                bounded: None,
            };
            Ok(match lane {
                Some(lane) => lease::spawn(read, lane),
                None => read,
            })
        })
        .await
        .map_err(|_| ReadError::Deadline)?
    }
}
impl SqliteComparisonRead {
    fn tx(&mut self) -> &mut sqlx::SqliteConnection {
        self.transaction.as_mut().expect("live read transaction")
    }
    /// All fragments are module constants, never caller SQL. The exact same
    /// predicate and bound parameters are used for size admission and body read.
    /// CAST AS BLOB measures UTF-8 bytes, not SQLite text character counts.
    async fn rows(
        &mut self,
        from: &str,
        columns: &[(&str, usize)],
        params: &[&str],
        cap: usize,
    ) -> Result<Vec<SqliteRow>> {
        let sizes = columns
            .iter()
            .map(|(c, _)| format!("coalesce(length(CAST({c} AS BLOB)),0)"))
            .collect::<Vec<_>>();
        let total = sizes.join("+");
        let invalid = sizes
            .iter()
            .zip(columns)
            .map(|(s, (_, n))| format!("{s}>{n}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!("SELECT count(*),coalesce(sum({total}),0),coalesce(max(CASE WHEN {invalid} THEN 1 ELSE 0 END),0) FROM {from}");
        let mut q = sqlx::query_as::<_, (i64, i64, i64)>(self.statements.select(sql)?);
        for p in params {
            q = q.bind(*p);
        }
        let (count, bytes, invalid) = q.fetch_one(self.tx()).await.map_err(db)?;
        bound(count >= 0 && count as usize <= cap && bytes >= 0 && invalid == 0)?;
        self.budget
            .charge(usize::try_from(bytes).map_err(|_| ReadError::Limit)?)?;
        let sql = format!(
            "SELECT {} FROM {from}",
            columns
                .iter()
                .map(|(c, _)| *c)
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut q = sqlx::query(self.statements.select(sql)?);
        for p in params {
            q = q.bind(*p);
        }
        q.fetch_all(self.tx()).await.map_err(db)
    }
    async fn head(
        &mut self,
        scope: &[String; 2],
        c: OutcomeLockClass,
        parts: Vec<Value>,
    ) -> Result<ObservedOutcomeHead> {
        let mut keys = vec![json!(scope)];
        keys.extend(parts);
        let key = encode(&json!(keys))?;
        bound(key.len() <= 16 * 1024)?;
        self.budget.charge(key.len())?;
        let sizes: Option<(i64,i64)> = sqlx::query_as(self.statements.select("SELECT length(CAST(revision AS BLOB)),length(value) FROM outcome_heads WHERE class=? AND key=?")?)
            .bind(class(c)).bind(&key).fetch_optional(self.tx()).await.map_err(db)?;
        let (revision, value) = if let Some((r, v)) = sizes {
            bound((0..=19).contains(&r) && (0..=262144).contains(&v))?;
            self.budget.charge((r + v) as usize)?;
            let (r, v): (String, Vec<u8>) = sqlx::query_as(
                self.statements
                    .select("SELECT revision,value FROM outcome_heads WHERE class=? AND key=?")?,
            )
            .bind(class(c))
            .bind(&key)
            .fetch_one(self.tx())
            .await
            .map_err(db)?;
            (Some(r), Some(v))
        } else {
            (None, None)
        };
        Ok(ObservedOutcomeHead {
            lock: OutcomeLock {
                class: c,
                key,
                mode: OutcomeLockMode::Read,
            },
            revision,
            value,
        })
    }
    async fn document(&mut self, r: &ScopedRecordRef) -> Result<Vec<u8>> {
        let sizes: Option<(i64,i64)> = sqlx::query_as(self.statements.select("SELECT length(canonical_bytes),length(CAST(content_hash AS BLOB)) FROM outcome_records WHERE tenant=? AND environment=? AND kind=? AND id=?")?)
            .bind(&r.scope[0]).bind(&r.scope[1]).bind(&r.kind).bind(&r.id).fetch_optional(self.tx()).await.map_err(db)?;
        let (n, h) = sizes.ok_or(ReadError::Integrity)?;
        ReadBudget::preflight_records(1, n as u64, n as u64)?;
        bound((0..=128).contains(&h))?;
        self.budget.charge((n + h) as usize)?;
        let (hash,raw): (String,Vec<u8>) = sqlx::query_as(self.statements.select("SELECT content_hash,canonical_bytes FROM outcome_records WHERE tenant=? AND environment=? AND kind=? AND id=?")?)
            .bind(&r.scope[0]).bind(&r.scope[1]).bind(&r.kind).bind(&r.id).fetch_one(self.tx()).await.map_err(db)?;
        require(hash == r.content_hash)?;
        let v = canonical::outcome::decode(&raw).map_err(|_| ReadError::Integrity)?;
        require(
            v["scope"] == json!(r.scope)
                && v["kind"] == r.kind
                && encode(&v["id"])? == r.id
                && v["content_hash"] == hash,
        )?;
        Ok(raw)
    }
    // Discover explicit immutable reference objects only. No authority is
    // inferred from their content; the mandatory trusted host verifies it.
    fn references(
        &mut self,
        v: &Value,
        scope: &[String; 2],
        refs: &mut Vec<ScopedRecordRef>,
        seen: &mut BTreeSet<(String, Vec<u8>, String)>,
    ) -> Result<()> {
        if v.get("kind").is_some() && v.get("id").is_some() && v.get("content_hash").is_some() {
            if let Some(s) = v.get("scope") {
                require(*s == json!(scope))?;
            }
            let kind = field(v, "kind")?;
            let hash = field(v, "content_hash")?;
            let id = bounded_identity(&v["id"])?;
            if !seen.contains(&(kind.to_owned(), id.clone(), hash.to_owned())) {
                bound(refs.len() < MAX_REFERENCES)?;
                self.budget.charge(
                    kind.len()
                        + hash.len()
                        + id.len()
                        + scope.iter().map(String::len).sum::<usize>(),
                )?;
                seen.insert((kind.into(), id.clone(), hash.into()));
                refs.push(ScopedRecordRef {
                    scope: scope.clone(),
                    kind: kind.into(),
                    id,
                    content_hash: hash.into(),
                });
            }
            return Ok(());
        }
        match v {
            Value::Array(a) => {
                for x in a {
                    self.references(x, scope, refs, seen)?;
                }
            }
            Value::Object(o) => {
                for x in o.values() {
                    self.references(x, scope, refs, seen)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    async fn authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation> {
        bound(
            who.scope.iter().all(|s| !s.is_empty() && s.len() <= 128)
                && !who.authority_head.is_empty()
                && who.authority_head.len() <= 128
                && !who.principal_id.is_empty()
                && who.principal_id.len() <= 128,
        )?;
        let heads = vec![
            self.head(&who.scope, OutcomeLockClass::Admission, vec![])
                .await?,
            self.head(
                &who.scope,
                OutcomeLockClass::Authority,
                vec![json!(who.authority_head)],
            )
            .await?,
        ];
        let mut refs = vec![];
        let mut seen = BTreeSet::new();
        for h in &heads {
            if let Some(v) = &h.value {
                self.references(&parse(v)?, &who.scope, &mut refs, &mut seen)?;
            }
        }
        let mut records = vec![];
        let mut i = 0;
        while i < refs.len() {
            let raw = self.document(&refs[i]).await?;
            // References in an envelope body, not the envelope's self identity.
            self.references(&parse(&raw)?["body"], &who.scope, &mut refs, &mut seen)?;
            records.push(raw);
            i += 1;
        }
        self.authority_scope = Some(who.scope.clone());
        Ok(ReadAuthorityObservation {
            scope: who.scope.clone(),
            heads,
            records,
        })
    }
    fn want(
        &mut self,
        wanted: &mut BTreeSet<(OutcomeLockClass, Vec<String>)>,
        class: OutcomeLockClass,
        parts: Vec<String>,
    ) -> Result<()> {
        if !wanted.contains(&(class, parts.clone())) {
            bound(wanted.len() < MAX_HEADS)?;
            self.budget.charge(parts.iter().map(String::len).sum())?;
            wanted.insert((class, parts));
        }
        Ok(())
    }
    async fn retained(&mut self, s: &RetainedSelection) -> Result<RawRetainedSnapshot> {
        require(self.authority_scope.as_ref() == Some(&s.scope))?;
        bound(
            s.scope.iter().all(|x| x.len() <= 128)
                && !s.target.is_empty()
                && s.target.len() <= 128
                && !s.invocation_id.is_empty()
                && s.invocation_id.len() <= 128,
        )?;
        let params = [&*s.scope[0], &*s.scope[1], &*s.target, &*s.invocation_id];
        let identity = self
            .rows(
                "installation WHERE singleton=1 AND tenant=? AND environment=?",
                &[("logical_store_id", 128)],
                &params[..2],
                1,
            )
            .await?;
        let store_identity: String = identity
            .first()
            .ok_or(ReadError::NotFound)?
            .try_get(0)
            .map_err(db)?;
        require(!store_identity.is_empty())?;
        let columns = [
            ("kind", 128),
            ("id", 4096),
            ("content_hash", 128),
            ("tenant", 128),
            ("environment", 128),
        ];
        let anchor_rows = self
            .rows(
                "outcome_anchors WHERE tenant=? AND environment=? AND target=? AND invocation_id=?",
                &columns,
                &params,
                2,
            )
            .await?;
        let member_rows = self
            .rows(
                "outcome_members WHERE tenant=? AND environment=? AND target=? AND invocation_id=?",
                &columns,
                &params,
                MAX_RECORDS,
            )
            .await?;
        if member_rows.is_empty() {
            return Err(ReadError::NotFound);
        }
        let refs = |rows: Vec<SqliteRow>| -> Result<Vec<ScopedRecordRef>> {
            rows.into_iter()
                .map(|r| {
                    Ok(ScopedRecordRef {
                        scope: [r.try_get(3).map_err(db)?, r.try_get(4).map_err(db)?],
                        kind: r.try_get(0).map_err(db)?,
                        id: r.try_get(1).map_err(db)?,
                        content_hash: r.try_get(2).map_err(db)?,
                    })
                })
                .collect()
        };
        let anchors = refs(anchor_rows)?;
        let members = refs(member_rows)?;
        // LEFT JOIN retains missing/mismatched members for explicit rejection.
        const HISTORY: &str="outcome_members m LEFT JOIN outcome_records r ON r.tenant=m.tenant AND r.environment=m.environment AND r.kind=m.kind AND r.id=m.id AND r.content_hash=m.content_hash WHERE m.tenant=? AND m.environment=? AND m.target=? AND m.invocation_id=?";
        let sql=format!("SELECT count(*),coalesce(sum(length(r.canonical_bytes)),0),coalesce(max(length(r.canonical_bytes)),0),count(r.canonical_bytes) FROM {HISTORY}");
        let (n, b, m, present): (i64, i64, i64, i64) = sqlx::query_as(self.statements.select(sql)?)
            .bind(params[0])
            .bind(params[1])
            .bind(params[2])
            .bind(params[3])
            .fetch_one(self.tx())
            .await
            .map_err(db)?;
        ReadBudget::preflight_records(n as u64, b as u64, m as u64)?;
        require(n == present && n as usize == members.len())?;
        let rows = self
            .rows(
                HISTORY,
                &[("r.canonical_bytes", MAX_ENVELOPE_BYTES)],
                &params,
                MAX_RECORDS,
            )
            .await?;
        let mut records = Vec::new();
        let mut wanted = BTreeSet::new();
        use OutcomeLockClass as L;
        self.want(&mut wanted, L::Target, vec![s.target.clone()])?;
        self.want(&mut wanted, L::BaseReversal, vec![s.target.clone()])?;
        self.want(&mut wanted, L::Reservation, vec![s.invocation_id.clone()])?;
        self.want(
            &mut wanted,
            L::InvocationConsumption,
            vec![s.invocation_id.clone()],
        )?;
        for row in rows {
            let raw: Vec<u8> = row.try_get(0).map_err(db)?;
            let v = parse(&raw)?;
            match v["kind"].as_str() {
                Some("binding-snapshot") => {
                    let id = field(&v["body"], "binding_id")?.to_owned();
                    self.want(&mut wanted, L::Binding, vec![id.clone()])?;
                    self.want(&mut wanted, L::BindingAggregate, vec![s.target.clone(), id])?;
                }
                Some("policy-snapshot") => {
                    self.want(
                        &mut wanted,
                        L::Claim,
                        vec![
                            field(&v["body"], "agreement_id")?.into(),
                            field(&v["body"], "family_id")?.into(),
                            s.target.clone(),
                        ],
                    )?;
                }
                _ => {}
            }
            bound(wanted.len() <= MAX_HEADS)?;
            records.push(raw);
        }
        let mut heads = vec![];
        for (c, parts) in wanted {
            heads.push(
                self.head(&s.scope, c, parts.into_iter().map(Value::String).collect())
                    .await?,
            );
        }
        // Membership selects every reservation receipt independently. LEFT JOIN
        // prevents missing original mappings/companions from disappearing.
        const DELIVERIES:&str="outcome_members m LEFT JOIN outcome_deliveries d ON d.tenant=m.tenant AND d.environment=m.environment AND d.settlement_kind=m.kind AND d.settlement_id=m.id AND d.settlement_hash=m.content_hash AND d.source=d.canonical_source AND d.external_id=d.canonical_external_id LEFT JOIN outcome_records e ON e.tenant=d.tenant AND e.environment=d.environment AND e.kind=d.economic_kind AND e.id=d.economic_id AND e.content_hash=d.economic_hash LEFT JOIN outcome_records r ON r.tenant=d.tenant AND r.environment=d.environment AND r.kind=d.settlement_kind AND r.id=d.settlement_id AND r.content_hash=d.settlement_hash WHERE m.tenant=? AND m.environment=? AND m.target=? AND m.invocation_id=? AND m.kind='reservation-receipt'";
        let rows = self
            .rows(
                DELIVERIES,
                &[
                    ("d.source", 256),
                    ("d.external_id", 256),
                    ("d.canonical_source", 256),
                    ("d.canonical_external_id", 256),
                    ("d.command", MAX_ENVELOPE_BYTES),
                    ("d.ingress", MAX_ENVELOPE_BYTES),
                    ("d.ingress_hash", 256),
                    ("e.canonical_bytes", MAX_ENVELOPE_BYTES),
                    ("r.canonical_bytes", MAX_ENVELOPE_BYTES),
                    ("d.economic_kind", 128),
                    ("d.tenant", 128),
                    ("d.environment", 128),
                    ("d.tenant", 128),
                    ("d.environment", 128),
                ],
                &params,
                MAX_STEPS + 1,
            )
            .await?;
        let mut original_deliveries = vec![];
        for r in rows {
            let source: Option<String> = r.try_get(0).map_err(db)?;
            let economic_receipt: Option<Vec<u8>> = r.try_get(7).map_err(db)?;
            let economic_kind: Option<String> = r.try_get(9).map_err(db)?;
            require(economic_kind.is_some() == economic_receipt.is_some())?;
            original_deliveries.push(StoredCompositeDelivery {
                key: ScopedDelivery {
                    scope: s.scope.clone(),
                    source: source.ok_or(ReadError::Integrity)?,
                    external_id: r.try_get(1).map_err(db)?,
                },
                canonical_key: ScopedDelivery {
                    scope: s.scope.clone(),
                    source: r.try_get(2).map_err(db)?,
                    external_id: r.try_get(3).map_err(db)?,
                },
                command: r.try_get(4).map_err(db)?,
                ingress: r.try_get(5).map_err(db)?,
                ingress_hash: r.try_get(6).map_err(db)?,
                economic_receipt,
                settlement_receipt: r
                    .try_get::<Option<Vec<u8>>, _>(8)
                    .map_err(db)?
                    .ok_or(ReadError::Integrity)?,
            });
        }
        Ok(RawRetainedSnapshot {
            store_identity,
            anchors,
            members,
            records,
            heads,
            original_deliveries,
        })
    }
}
impl ComparisonReadTx for SqliteComparisonRead {
    async fn load_authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation> {
        if let Some(bounded) = &self.bounded {
            require(self.ready)?;
            self.ready = false;
            let result = bounded.authority(who.clone()).await;
            self.ready = result.is_ok();
            return result;
        }
        require(self.ready && self.authority_scope.is_none())?;
        self.ready = false;
        let r = timeout_at(self.deadline, self.authority(who))
            .await
            .map_err(|_| ReadError::Deadline)?;
        self.ready = r.is_ok();
        r
    }
    async fn load_retained(&mut self, s: &RetainedSelection) -> Result<RawRetainedSnapshot> {
        if let Some(bounded) = &self.bounded {
            require(self.ready)?;
            self.ready = false;
            return bounded.retained(s.clone()).await;
        }
        require(self.ready)?;
        self.ready = false;
        timeout_at(self.deadline, self.retained(s))
            .await
            .map_err(|_| ReadError::Deadline)?
    }
    async fn finish(mut self) -> Result<()> {
        if let Some(bounded) = self.bounded.take() {
            return bounded.finish().await;
        }
        let tx = self.transaction.take().ok_or(ReadError::Unavailable)?;
        timeout_at(self.deadline, tx.rollback())
            .await
            .map_err(|_| ReadError::Deadline)?
            .map_err(db)
    }
}
mod lease;
#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;
