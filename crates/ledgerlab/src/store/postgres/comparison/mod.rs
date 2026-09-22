//! Private comparison reader: a fresh verified TLS session, never a writer lease.
//! Every query belongs to one repeatable-read read-only snapshot. Taking State
//! out of the handle across awaits makes dropped load futures discard the session.
#![allow(dead_code)] // Private host wiring is owned by the integration lane.
use super::{connect::Session, PostgresConfig};
use crate::store::{comparison::*, outcomes::*};
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::{timeout_at, Instant};
use tokio_postgres::{types::ToSql, Row};
mod queries;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct PostgresComparisonStore {
    config: PostgresConfig,
    identity: String,
    #[cfg(test)]
    audit: std::sync::Arc<std::sync::Mutex<tests::Audit>>,
}
impl PostgresComparisonStore {
    pub(crate) fn new(config: PostgresConfig, identity: String) -> Result<Self, ReadError> {
        bound(!identity.is_empty() && identity.len() <= 128)?;
        Ok(Self {
            #[cfg(test)]
            audit: std::sync::Arc::new(std::sync::Mutex::new(tests::Audit::new(&config))),
            config,
            identity,
        })
    }
}
pub(crate) struct PostgresComparisonTx {
    state: Option<State>,
    discarded: bool,
}
struct State {
    session: Session,
    deadline: Instant,
    identity: String,
    budget: ReadBudget,
    authority_scope: Option<[String; 2]>,
    retained_loaded: bool,
    heads: usize,
    #[cfg(test)]
    audit: std::sync::Arc<std::sync::Mutex<tests::Audit>>,
}
impl ComparisonReadStore for PostgresComparisonStore {
    type Read = PostgresComparisonTx;
    async fn begin_read(&self, deadline: Instant) -> Result<Self::Read, ReadError> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        timeout_at(deadline, async {
            #[cfg(test)]
            self.audit.lock().unwrap().connection(&self.config)?;
            let session = self
                .config
                .connect()
                .await
                .map_err(|_| ReadError::Unavailable)?;
            super::verify_server(&session.client)
                .await
                .map_err(|_| ReadError::Unavailable)?;
            super::verify_role(&session.client)
                .await
                .map_err(|_| ReadError::Unavailable)?;
            session
                .client
                .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
                .await
                .map_err(db)?;
            // Pin before returning: authority, preflight and retained rows share it.
            let pinned = session
                .client
                .query_one("SELECT pg_current_snapshot()::text,pg_backend_pid()", &[])
                .await
                .map_err(db)?;
            #[cfg(test)]
            self.audit
                .lock()
                .unwrap()
                .pids
                .push(pinned.get::<_, i32>(1));
            #[cfg(not(test))]
            let _ = pinned;
            Ok(PostgresComparisonTx {
                discarded: false,
                state: Some(State {
                    session,
                    deadline,
                    identity: self.identity.clone(),
                    budget: ReadBudget::default(),
                    authority_scope: None,
                    retained_loaded: false,
                    heads: 0,
                    #[cfg(test)]
                    audit: self.audit.clone(),
                }),
            })
        })
        .await
        .map_err(|_| ReadError::Deadline)?
    }
}
impl ComparisonReadTx for PostgresComparisonTx {
    async fn load_authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation, ReadError> {
        let mut state = self.state.take().ok_or(ReadError::Unavailable)?;
        let result = timeout_at(state.deadline, state.authority(who))
            .await
            .map_err(|_| ReadError::Deadline)
            .and_then(|r| r);
        match result {
            Ok(value) => {
                self.state = Some(state);
                Ok(value)
            }
            Err(e) => {
                state.session.discard().await;
                self.discarded = true;
                Err(e)
            }
        }
    }
    async fn load_retained(
        &mut self,
        selection: &RetainedSelection,
    ) -> Result<RawRetainedSnapshot, ReadError> {
        let mut state = self.state.take().ok_or(ReadError::Unavailable)?;
        let result = timeout_at(state.deadline, state.retained(selection))
            .await
            .map_err(|_| ReadError::Deadline)
            .and_then(|r| r);
        match result {
            Ok(value) => {
                self.state = Some(state);
                Ok(value)
            }
            Err(e) => {
                state.session.discard().await;
                self.discarded = true;
                Err(e)
            }
        }
    }
    async fn finish(mut self) -> Result<(), ReadError> {
        if self.discarded {
            return Ok(());
        }
        let state = self.state.take().ok_or(ReadError::Unavailable)?;
        let result = timeout_at(
            state.deadline,
            state.session.client.batch_execute("ROLLBACK"),
        )
        .await
        .map_err(|_| ReadError::Deadline)
        .and_then(|r| r.map_err(db));
        state.session.discard().await;
        result
    }
}
fn db(e: tokio_postgres::Error) -> ReadError {
    if e.code().is_some_and(|c| c.code() == "57014") {
        ReadError::Deadline
    } else {
        ReadError::Unavailable
    }
}
fn bound(ok: bool) -> Result<(), ReadError> {
    if ok {
        Ok(())
    } else {
        Err(ReadError::Limit)
    }
}
fn check(ok: bool) -> Result<(), ReadError> {
    if ok {
        Ok(())
    } else {
        Err(ReadError::Integrity)
    }
}
fn text(v: &Value) -> Result<&str, ReadError> {
    v.as_str().ok_or(ReadError::Integrity)
}
fn bytes(v: &Value) -> Result<Vec<u8>, ReadError> {
    CanonicalBytes::from_value(v)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| ReadError::Integrity)
}
fn parsed(b: &[u8]) -> Result<Value, ReadError> {
    let v = canonical::parse_bounded(b, MAX_ENVELOPE_BYTES).map_err(|_| ReadError::Integrity)?;
    check(bytes(&v)? == b)?;
    Ok(v)
}
fn scope_bound(scope: &[String; 2]) -> Result<(), ReadError> {
    bound(scope.iter().all(|s| !s.is_empty() && s.len() <= 128))
}
fn key(
    scope: &[String; 2],
    class: OutcomeLockClass,
    parts: Vec<Value>,
) -> Result<OutcomeLock, ReadError> {
    let mut k = vec![json!(scope)];
    k.extend(parts);
    let key = bytes(&json!(k))?;
    bound(key.len() <= 16 * 1024)?;
    Ok(OutcomeLock {
        class,
        key,
        mode: OutcomeLockMode::Read,
    })
}
impl State {
    async fn checkpoint(&self) -> Result<(), ReadError> {
        tokio::task::yield_now().await;
        if Instant::now() >= self.deadline {
            Err(ReadError::Deadline)
        } else {
            Ok(())
        }
    }
    async fn query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, ReadError> {
        self.checkpoint().await?;
        // Central choke point also makes attempted SQL independently auditable.
        #[cfg(test)]
        self.audit.lock().unwrap().sql(sql)?;
        check(sql.starts_with("SELECT "))?;
        timeout_at(self.deadline, self.session.client.query(sql, params))
            .await
            .map_err(|_| ReadError::Deadline)?
            .map_err(db)
    }
    async fn one(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Row, ReadError> {
        let mut rows = self.query(sql, params).await?;
        check(rows.len() == 1)?;
        Ok(rows.remove(0))
    }
    async fn head(
        &mut self,
        scope: &[String; 2],
        lock: OutcomeLock,
    ) -> Result<ObservedOutcomeHead, ReadError> {
        bound(self.heads < MAX_HEADS)?;
        self.heads += 1;
        self.budget.charge(lock.key.len() + 19)?;
        let class = lock.class as i16;
        let params: &[&(dyn ToSql + Sync)] = &[&scope[0], &scope[1], &class, &lock.key];
        let size = self.query("SELECT octet_length(value)::bigint FROM ledgerlab.outcome_heads WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4", params).await?;
        if size.is_empty() {
            return Ok(ObservedOutcomeHead {
                lock,
                revision: None,
                value: None,
            });
        }
        let n: i64 = size[0].try_get(0).map_err(db)?;
        bound((0..=256 * 1024).contains(&n))?;
        self.budget.charge(n as usize)?;
        let r = self.one("SELECT revision,value FROM ledgerlab.outcome_heads WHERE tenant=$1 AND environment=$2 AND class=$3 AND key=$4", params).await?;
        Ok(ObservedOutcomeHead {
            lock,
            revision: Some(r.try_get::<_, i64>(0).map_err(db)?.to_string()),
            value: Some(r.try_get(1).map_err(db)?),
        })
    }
    async fn authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation, ReadError> {
        check(self.authority_scope.is_none())?;
        scope_bound(&who.scope)?;
        bound(
            !who.authority_head.is_empty()
                && who.authority_head.len() <= 128
                && !who.principal_id.is_empty()
                && who.principal_id.len() <= 128,
        )?;
        let mut heads = Vec::new();
        for l in [
            key(&who.scope, OutcomeLockClass::Admission, vec![])?,
            key(
                &who.scope,
                OutcomeLockClass::Authority,
                vec![json!(who.authority_head)],
            )?,
        ] {
            heads.push(self.head(&who.scope, l).await?);
        }
        let mut refs = Vec::new();
        for h in &heads {
            if let Some(v) = &h.value {
                collect_refs(&parsed(v)?, &mut refs, &mut self.budget)?;
            }
        }
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        let mut at = 0;
        while at < refs.len() {
            self.checkpoint().await?;
            let rf = refs[at].clone();
            at += 1;
            let id = bytes(&rf["id"])?;
            let kind = text(&rf["kind"])?;
            let hash = text(&rf["content_hash"])?;
            bound(id.len() <= 4096 && kind.len() <= 128 && hash.len() <= 128)?;
            if !seen.insert((kind.to_owned(), id.clone(), hash.to_owned())) {
                continue;
            }
            self.budget.charge(
                id.len()
                    + kind.len()
                    + hash.len()
                    + who.scope.iter().map(String::len).sum::<usize>(),
            )?;
            let params: &[&(dyn ToSql + Sync)] = &[&who.scope[0], &who.scope[1], &kind, &id, &hash];
            let r = self.one("SELECT count(*)::bigint,COALESCE(sum(octet_length(envelope)),0)::bigint,COALESCE(max(octet_length(envelope)),0)::bigint FROM ledgerlab.outcome_records WHERE tenant=$1 AND environment=$2 AND kind=$3 AND id=$4 AND content_hash=$5", params).await?;
            let count: i64 = r.try_get(0).map_err(db)?;
            check(count == 1)?;
            let n: i64 = r.try_get(1).map_err(db)?;
            ReadBudget::preflight_records(1, n as u64, n as u64)?;
            self.budget.charge(n as usize)?;
            let row = self.one("SELECT envelope FROM ledgerlab.outcome_records WHERE tenant=$1 AND environment=$2 AND kind=$3 AND id=$4 AND content_hash=$5", params).await?;
            let raw: Vec<u8> = row.try_get(0).map_err(db)?;
            let v = parsed(&raw)?;
            check(
                v["kind"] == rf["kind"]
                    && v["id"] == rf["id"]
                    && v["content_hash"] == rf["content_hash"]
                    && v["scope"] == json!(who.scope),
            )?;
            collect_refs(&v["body"], &mut refs, &mut self.budget)?;
            records.push(raw);
        }
        self.authority_scope = Some(who.scope.clone());
        Ok(ReadAuthorityObservation {
            scope: who.scope.clone(),
            heads,
            records,
        })
    }
}
fn collect_refs(
    v: &Value,
    refs: &mut Vec<Value>,
    budget: &mut ReadBudget,
) -> Result<(), ReadError> {
    match v {
        Value::Object(m)
            if m.contains_key("kind") && m.contains_key("id") && m.contains_key("content_hash") =>
        {
            bound(refs.len() < MAX_REFERENCES)?;
            let kind = text(&v["kind"])?;
            let hash = text(&v["content_hash"])?;
            let id = bytes(&v["id"])?;
            bound(kind.len() <= 128 && hash.len() <= 128 && id.len() <= 4096)?;
            budget.charge(kind.len() + hash.len() + id.len())?;
            refs.push(json!({"kind":kind,"id":v["id"],"content_hash":hash}));
        }
        Value::Object(m) => {
            for v in m.values() {
                collect_refs(v, refs, budget)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                collect_refs(v, refs, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}
