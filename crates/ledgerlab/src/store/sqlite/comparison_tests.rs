//! Real SQLite evidence. Writers below are fixture setup/concurrent accepted
//! corrections only; the comparison module has no access to these capabilities.
use super::*;
use crate::service::{
    accept::outcome::{self, fixture, AuthorityProof, OutcomeAuthority, OutcomeCommand},
    comparison::{
        self as facade, Cancellation, ComparisonError, ComparisonOperation, ComparisonReadAuthority,
    },
};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn input(f: &fixture::Fixture) -> (AuthenticatedReadContext, RetainedSelection) {
    let c = &f.commands[0];
    let scope = ["synthetic".into(), "sandbox".into()];
    (
        AuthenticatedReadContext {
            scope: scope.clone(),
            principal_id: c.principal.principal_id.clone(),
            authority_head: c.principal.authority_head.clone(),
        },
        RetainedSelection {
            scope,
            target: c.target.clone(),
            invocation_id: c.invocation_id.clone(),
            expected_snapshot: None,
        },
    )
}
struct WriteAuthority(AuthorityProof);
impl OutcomeAuthority for WriteAuthority {
    fn verify(
        &self,
        _: &OutcomeCommand,
        _: &OutcomeSnapshot,
        _: bool,
    ) -> std::result::Result<AuthorityProof, crate::ServiceError> {
        Ok(self.0.clone())
    }
}
async fn accepted(store: &SqliteStore, f: &fixture::Fixture, i: usize) {
    assert!(matches!(
        outcome::run(store, &f.commands[i], &WriteAuthority(f.proofs[i].clone()))
            .await
            .unwrap(),
        outcome::OutcomeResult::Accepted(_)
    ));
}
async fn prepared(f: &fixture::Fixture, n: usize) -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let mut installation = super::super::tests::installation();
    installation.scope.tenant = "synthetic".into();
    installation.scope.environment = "sandbox".into();
    let store = SqliteStore::create(dir.path(), installation).await.unwrap();
    let mut tx = store.inner.writer.begin().await.unwrap();
    // Nonempty legacy/control/outbox data gives invariance checks something to
    // detect outside the selected Phase 3 journal (including another scope).
    for op in super::super::tests::seed()
        .into_iter()
        .chain(super::super::tests::schedule())
    {
        super::super::write::operation(&mut tx, &op).await.unwrap();
    }
    for raw in &f.provisioned_records {
        let v = parse(raw).unwrap();
        sqlx::query("INSERT INTO outcome_records VALUES (?,?,?,?,?,?)")
            .bind(v["scope"][0].as_str().unwrap())
            .bind(v["scope"][1].as_str().unwrap())
            .bind(v["kind"].as_str().unwrap())
            .bind(encode(&v["id"]).unwrap())
            .bind(v["content_hash"].as_str().unwrap())
            .bind(raw)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for h in &f.provisioned_heads {
        if let (Some(r), Some(v)) = (&h.revision, &h.value) {
            sqlx::query("INSERT INTO outcome_heads VALUES (?,?,?,?)")
                .bind(class(h.lock.class))
                .bind(&h.lock.key)
                .bind(r)
                .bind(v)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    for i in 0..n {
        accepted(&store, f, i).await;
    }
    (dir, store)
}
// Full application table/column inventories and all physical values, never a
// hard-coded subset. Includes DDL, indexes and trigger definitions on reopen.
async fn inventory(store: &SqliteStore) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut tx = store.inner.readers.begin().await.unwrap();
    let tables:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        tables
            .iter()
            .filter(|t| t.as_str() != "_sqlx_migrations")
            .count(),
        33
    );
    let mut out = vec![];
    for t in tables {
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&t)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let expr = cols
            .iter()
            .map(|c| format!("quote({})", quote(c)))
            .collect::<Vec<_>>()
            .join("||'|'||");
        let rows = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT {expr} FROM {} ORDER BY 1",
            quote(&t)
        )))
        .fetch_all(&mut *tx)
        .await
        .unwrap();
        out.push((t, cols, rows));
    }
    let ddl:Vec<String>=sqlx::query_scalar("SELECT quote(type)||'|'||quote(name)||'|'||quote(sql) FROM sqlite_schema ORDER BY type,name").fetch_all(&mut *tx).await.unwrap();
    out.push(("sqlite_schema".into(), vec![], ddl));
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    out.push((
        "user_version".into(),
        vec!["value".into()],
        vec![version.to_string()],
    ));
    tx.rollback().await.unwrap();
    out
}
async fn reopen(
    dir: &tempfile::TempDir,
    store: SqliteStore,
    before: &[(String, Vec<String>, Vec<String>)],
) -> SqliteStore {
    assert_eq!(inventory(&store).await, before);
    store.close().await;
    let store = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(inventory(&store).await, before);
    store
}
struct LocalAuthority {
    principal: String,
    grant: Vec<u8>,
    deny_scope: bool,
    deny_disclosure: bool,
}
impl LocalAuthority {
    fn new(f: &fixture::Fixture) -> Self {
        let grant = f
            .provisioned_records
            .iter()
            .find(|r| parse(r).unwrap()["body"]["purpose"] == "grant")
            .unwrap()
            .clone();
        Self {
            principal: f.commands[0].principal.principal_id.clone(),
            grant,
            deny_scope: false,
            deny_disclosure: false,
        }
    }
}
impl ComparisonReadAuthority for LocalAuthority {
    fn verify_scope(
        &self,
        w: &AuthenticatedReadContext,
        s: &RetainedSelection,
        a: &ReadAuthorityObservation,
    ) -> std::result::Result<(), ComparisonError> {
        let grant = parse(&self.grant).unwrap();
        let h = a
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Authority)
            .ok_or(ComparisonError::Denied)?;
        let value = h
            .value
            .as_ref()
            .and_then(|v| parse(v).ok())
            .ok_or(ComparisonError::Denied)?;
        if self.deny_scope
            || w.principal_id != self.principal
            || w.scope != s.scope
            || a.scope != s.scope
            || h.lock.key != encode(&json!([w.scope, w.authority_head])).unwrap()
            || h.revision.as_deref() != Some("1")
            || value["active"] != true
            || value["grant"]["content_hash"] != grant["content_hash"]
            || a.records != [self.grant.clone()]
        {
            return Err(ComparisonError::Denied);
        }
        Ok(())
    }
    fn verify_disclosure(
        &self,
        w: &AuthenticatedReadContext,
        s: &RetainedSelection,
        a: &ReadAuthorityObservation,
        r: &RawRetainedSnapshot,
    ) -> std::result::Result<(), ComparisonError> {
        self.verify_scope(w, s, a)?;
        if self.deny_disclosure
            || r.records
                .iter()
                .any(|r| parse(r).unwrap()["scope"] != json!(s.scope))
        {
            Err(ComparisonError::Denied)
        } else {
            Ok(())
        }
    }
}
async fn operation(c: Cancellation) -> ComparisonOperation {
    loop {
        match ComparisonOperation::begin(c.clone()) {
            Ok(op) => return op,
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    }
}
struct LabelledReader<'a>(&'a SqliteStore, &'static str);
impl ComparisonReadStore for LabelledReader<'_> {
    type Read = SqliteComparisonRead;
    async fn begin_read(&self, deadline: Instant) -> Result<Self::Read> {
        let mut tx = self.0.begin_read(deadline).await?;
        tx.statements.label = self.1;
        Ok(tx)
    }
}
async fn load(
    store: &SqliteStore,
    f: &fixture::Fixture,
    a: &LocalAuthority,
) -> std::result::Result<facade::ComparisonWorkspace, ComparisonError> {
    let (w, s) = input(f);
    let op = operation(Cancellation::default()).await;
    let label = if a.deny_scope {
        "scope-denied"
    } else if a.deny_disclosure {
        "disclosure-denied"
    } else {
        "workspace"
    };
    facade::load_workspace(&LabelledReader(store, label), a, &w, s, &op).await
}
#[tokio::test]
async fn comparison_sqlite_success_denials_enforcement_and_reopen() {
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f, f.plans.len()).await;
    let before = inventory(&store).await;
    for table in [
        "installation",
        "dispatcher_head",
        "intentions",
        "delivery_state",
        "authority_heads",
        "outcome_heads",
        "outcome_records",
        "outcome_members",
        "outcome_anchors",
        "outcome_deliveries",
    ] {
        assert!(
            !before.iter().find(|r| r.0 == table).unwrap().2.is_empty(),
            "nonempty {table}"
        );
    }
    let a = LocalAuthority::new(&f);
    let ws = load(&store, &f, &a).await.unwrap();
    assert_eq!(ws.historical_receipts().len(), f.plans.len());
    let fingerprint = ws.fingerprint().clone();
    store = reopen(&dir, store, &before).await;
    assert_eq!(
        *load(&store, &f, &a).await.unwrap().fingerprint(),
        fingerprint
    );
    for (scope, disclosure) in [(true, false), (false, true)] {
        let mut a = LocalAuthority::new(&f);
        a.deny_scope = scope;
        a.deny_disclosure = disclosure;
        assert!(matches!(
            load(&store, &f, &a).await,
            Err(ComparisonError::Denied)
        ));
        store = reopen(&dir, store, &before).await;
    }
    // Actual engine enforcement, including a no-op write which cannot be
    // detected by after-state comparison alone. TEMP is query_only-protected.
    let mut tx = store.begin_read(deadline()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA query_only")
            .fetch_one(tx.tx())
            .await
            .unwrap(),
        1
    );
    for sql in [
        "UPDATE installation SET generation=generation WHERE 0",
        "CREATE TEMP TABLE forbidden (id INTEGER)",
        "DELETE FROM outcome_heads WHERE 0",
        "INSERT INTO outcome_heads SELECT * FROM outcome_heads WHERE 0",
    ] {
        let error = sqlx::query(AssertSqlSafe(sql))
            .execute(tx.tx())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("8")
        );
    }
    tx.finish().await.unwrap();
    store = reopen(&dir, store, &before).await;
    // Read-only open flag remains effective even if query_only is disabled in
    // this hostile test. Production reader never changes either setting.
    let mut tx = store.begin_read(deadline()).await.unwrap();
    sqlx::query("PRAGMA query_only=OFF")
        .execute(tx.tx())
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE installation SET generation=generation+1")
            .execute(tx.tx())
            .await
            .is_err()
    );
    tx.finish().await.unwrap();
    store = reopen(&dir, store, &before).await;
    store.close().await;
}
#[tokio::test]
async fn comparison_sqlite_whole_snapshot_across_authorized_correction() {
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f, 1).await;
    let (who, selection) = input(&f);
    for i in 1..f.plans.len() {
        let before = inventory(&store).await;
        let mut baseline = store.begin_read(deadline()).await.unwrap();
        baseline.load_authority(&who).await.unwrap();
        let expected_old = baseline.load_retained(&selection).await.unwrap();
        baseline.finish().await.unwrap();
        let mut read = store.begin_read(deadline()).await.unwrap();
        let authority = read.load_authority(&who).await.unwrap();
        // This is the real authorized coordinator on another connection, with
        // the reader held across its commit. No lock-row or writer exclusion.
        accepted(&store, &f, i).await;
        let new = inventory(&store).await;
        assert_ne!(new, before);
        let old = read.load_retained(&selection).await.unwrap();
        // Compare every field, including complete records/members/anchors, with
        // the independently loaded committed prefix before the writer ran.
        assert_eq!(format!("{old:?}"), format!("{expected_old:?}"));
        assert_eq!(old.original_deliveries.len(), i);
        assert_eq!(authority.records, [LocalAuthority::new(&f).grant]);
        read.finish().await.unwrap();
        let ws = load(&store, &f, &LocalAuthority::new(&f)).await.unwrap();
        assert_eq!(ws.historical_receipts().len(), i + 1);
        let mut fresh = store.begin_read(deadline()).await.unwrap();
        fresh.load_authority(&who).await.unwrap();
        let current = fresh.load_retained(&selection).await.unwrap();
        fresh.finish().await.unwrap();
        for p in &f.plans[..i] {
            assert!(old.original_deliveries.contains(p.delivery()));
        }
        assert!(!old.original_deliveries.contains(f.plans[i].delivery()));
        assert!(current.original_deliveries.contains(f.plans[i].delivery()));
        for h in &old.heads {
            if let Some(w) = f.plans[..i]
                .iter()
                .rev()
                .flat_map(|p| p.head_writes())
                .find(|w| w.lock.class == h.lock.class && w.lock.key == h.lock.key)
            {
                assert_eq!(h.revision.as_ref(), Some(&w.revision));
                assert_eq!(h.value.as_ref(), Some(&w.value));
            } else {
                let provisioned = f
                    .provisioned_heads
                    .iter()
                    .find(|w| w.lock.class == h.lock.class && w.lock.key == h.lock.key)
                    .unwrap();
                assert_eq!(h.revision, provisioned.revision);
                assert_eq!(h.value, provisioned.value);
            }
        }
        store = reopen(&dir, store, &new).await;
    }
    store.close().await;
}
#[tokio::test]
async fn comparison_sqlite_cancelled_methods_and_deadline_discard() {
    let f = fixture::lifecycle();
    let (dir, mut store) = prepared(&f, 1).await;
    let before = inventory(&store).await;
    let (w, s) = input(&f);
    for phase in 0..4 {
        if phase == 0 {
            let mut future = Box::pin(store.begin_read(deadline()));
            assert!(matches!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            ));
            drop(future);
        } else {
            let mut tx = store.begin_read(deadline()).await.unwrap();
            tx.statements.label = match phase {
                1 => "cancel-authority",
                2 => "cancel-retained",
                _ => "cancel-finish",
            };
            if phase == 1 {
                let mut future = Box::pin(tx.load_authority(&w));
                assert!(matches!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Pending
                ));
                drop(future);
                assert!(!tx.ready);
                drop(tx);
            } else if phase == 2 {
                tx.load_authority(&w).await.unwrap();
                let mut future = Box::pin(tx.load_retained(&s));
                assert!(matches!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Pending
                ));
                drop(future);
                assert!(!tx.ready);
                drop(tx);
            } else {
                let mut future = Box::pin(tx.finish());
                assert!(matches!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Pending
                ));
                drop(future);
            }
        }
        store = reopen(&dir, store, &before).await;
    }
    let mut tx = store.begin_read(deadline()).await.unwrap();
    tx.deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        tx.load_authority(&w).await.unwrap_err(),
        ReadError::Deadline
    );
    drop(tx);
    store = reopen(&dir, store, &before).await;
    let c = Cancellation::default();
    let op = operation(c.clone()).await;
    c.cancel();
    assert!(matches!(
        facade::load_workspace(&store, &LocalAuthority::new(&f), &w, s, &op).await,
        Err(ComparisonError::Cancelled)
    ));
    drop(op);
    store = reopen(&dir, store, &before).await;
    store.close().await;
}

async fn malformed_record(store: &SqliteStore, s: &RetainedSelection, id: &str, raw: &[u8]) {
    let mut tx = store.inner.writer.begin().await.unwrap();
    let id = encode(&json!(id)).unwrap();
    let hash = format!("sha256:{}", "0".repeat(64));
    sqlx::query("INSERT INTO outcome_records VALUES (?,?,'evidence',?,?,?)")
        .bind(&s.scope[0])
        .bind(&s.scope[1])
        .bind(&id)
        .bind(&hash)
        .bind(raw)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO outcome_members VALUES (?,?,?,?,'evidence',?,?)")
        .bind(&s.scope[0])
        .bind(&s.scope[1])
        .bind(&s.target)
        .bind(&s.invocation_id)
        .bind(&id)
        .bind(&hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn comparison_sqlite_malformed_and_preallocation_limits_reopen() {
    let f = fixture::lifecycle();
    for case in [
        "malformed",
        "envelope",
        "head",
        "members",
        "reference_bytes",
        "authority_refs",
        "shared_budget",
    ] {
        let (dir, store) = prepared(&f, 1).await;
        let (w, s) = input(&f);
        match case {
            "malformed" => {
                malformed_record(
                    &store,
                    &s,
                    "bad",
                    &encode(&json!({"scope":s.scope})).unwrap(),
                )
                .await
            }
            "envelope" => {
                malformed_record(&store, &s, "large", &vec![b' '; MAX_ENVELOPE_BYTES + 1]).await
            }
            "head" => {
                sqlx::query("UPDATE outcome_heads SET value=zeroblob(262145) WHERE class=4")
                    .execute(&store.inner.writer)
                    .await
                    .unwrap();
            }
            "members" => {
                let mut tx = store.inner.writer.begin().await.unwrap();
                sqlx::query("WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<4096) INSERT INTO outcome_records SELECT 'synthetic','sandbox','evidence',CAST('large-'||i AS BLOB),?,x'7b7d' FROM n").bind(format!("sha256:{}","0".repeat(64))).execute(&mut *tx).await.unwrap();
                sqlx::query("INSERT INTO outcome_members SELECT tenant,environment,?,? ,kind,id,content_hash FROM outcome_records WHERE CAST(id AS TEXT) LIKE 'large-%'").bind(&s.target).bind(&s.invocation_id).execute(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
            }
            "reference_bytes" => {
                let value = json!({"grant":{"kind":"evidence","id":"é".repeat(2050),"content_hash":format!("sha256:{}","0".repeat(64))}});
                sqlx::query("UPDATE outcome_heads SET value=? WHERE class=1")
                    .bind(encode(&value).unwrap())
                    .execute(&store.inner.writer)
                    .await
                    .unwrap();
            }
            "authority_refs" => {
                let refs=(0..1025).map(|n|json!({"kind":"evidence","id":format!("missing-{n}"),"content_hash":format!("sha256:{}","0".repeat(64))})).collect::<Vec<_>>();
                sqlx::query("UPDATE outcome_heads SET value=? WHERE class=1")
                    .bind(encode(&json!({"refs":refs})).unwrap())
                    .execute(&store.inner.writer)
                    .await
                    .unwrap();
            }
            "shared_budget" => {}
            _ => unreachable!(),
        }
        let before = inventory(&store).await;
        if case == "malformed" {
            assert!(matches!(
                load(&store, &f, &LocalAuthority::new(&f)).await,
                Err(ComparisonError::Integrity)
            ));
        } else {
            let mut tx = store.begin_read(deadline()).await.unwrap();
            tx.statements.label = case;
            let auth = tx.load_authority(&w).await;
            if case == "authority_refs" || case == "reference_bytes" {
                assert_eq!(auth.unwrap_err(), ReadError::Limit);
            } else {
                auth.unwrap();
                if case == "shared_budget" {
                    // The transaction's actual authority charge is already
                    // present: adding the full shared budget must fail.
                    assert_eq!(tx.budget.charge(MAX_RETAINED_BYTES), Err(ReadError::Limit));
                    for power in (0..24).rev() {
                        let _ = tx.budget.charge(1 << power);
                    }
                }
                assert_eq!(
                    tx.load_retained(&s).await.unwrap_err(),
                    ReadError::Limit,
                    "{case}"
                );
            }
            tx.finish().await.unwrap();
        }
        reopen(&dir, store, &before).await.close().await;
    }
}

fn attempted_write_trap(source: &str) -> bool {
    [
        ".execute(",
        ".execute_many(",
        ".commit(",
        ".writer",
        "begin_outcome",
        "AcceptanceTx",
        "OutcomeTx",
        "BEGIN IMMEDIATE",
        "INSERT INTO",
        "UPDATE ",
        "DELETE FROM",
        "CREATE TABLE",
        "CREATE TEMP",
        "PRAGMA ",
        "super::outcomes",
    ]
    .iter()
    .any(|token| source.contains(token))
}
fn external_call_trap(source: &str) -> bool {
    [
        "std::net",
        "tokio::net",
        "reqwest",
        "std::process",
        "std::fs",
        "Command::",
        "outbox::",
        "Destination",
        "reqwest::",
        "connect_with(",
        "SqliteStore::open",
        "SqliteStore::create",
        "super::read",
        "super::write",
    ]
    .iter()
    .any(|token| source.contains(token))
}
#[test]
fn comparison_sqlite_independent_attempted_write_and_external_capability_traps() {
    let source = include_str!("comparison.rs");
    // Independent fail-closed source capability tripwires, not an after-state
    // proxy and not a claim of sqlite3_authorizer runtime interception.
    assert!(!attempted_write_trap(source));
    assert!(!external_call_trap(source));
    for mutation in [
        ".execute(",
        ".writer",
        "BEGIN IMMEDIATE",
        "INSERT INTO outcome_heads",
    ] {
        assert!(attempted_write_trap(mutation));
    }
    for call in [
        "tokio::net::TcpStream",
        "std::process::Command::",
        "outbox::Destination",
    ] {
        assert!(external_call_trap(call));
    }
}

#[tokio::test]
async fn comparison_sqlite_original_receipts_members_and_anchors_are_independent() {
    let f = fixture::lifecycle();
    for case in [
        "alias",
        "missing_original",
        "missing_member",
        "wrong_anchor",
        "duplicate_original",
    ] {
        let (dir, store) = prepared(&f, 1).await;
        let (w, s) = input(&f);
        let mut tx = store.inner.writer.begin().await.unwrap();
        match case {
            "alias" | "duplicate_original" => {
                let suffix = case == "duplicate_original";
                sqlx::query("INSERT INTO outcome_deliveries SELECT tenant,environment,source,external_id||'-other',canonical_source,CASE WHEN ? THEN external_id||'-other' ELSE canonical_external_id END,command,ingress,ingress_hash,economic_kind,economic_id,economic_hash,settlement_kind,settlement_id,settlement_hash FROM outcome_deliveries").bind(suffix).execute(&mut *tx).await.unwrap();
            }
            "missing_original" | "missing_member" | "wrong_anchor" => {
                let (trigger, sql) = match case {
                    "missing_original" => (
                        "outcome_deliveries_immutable_delete",
                        "DELETE FROM outcome_deliveries",
                    ),
                    "missing_member" => (
                        "outcome_members_immutable_delete",
                        "DELETE FROM outcome_members WHERE kind='base-acceptance'",
                    ),
                    _ => (
                        "outcome_anchors_immutable_delete",
                        "DELETE FROM outcome_anchors WHERE kind='base-acceptance'",
                    ),
                };
                let ddl: String = sqlx::query_scalar(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?",
                )
                .bind(trigger)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
                sqlx::query(AssertSqlSafe(format!("DROP TRIGGER {trigger}")))
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                sqlx::query(AssertSqlSafe(sql))
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                sqlx::query(AssertSqlSafe(ddl))
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                if case == "wrong_anchor" {
                    sqlx::query("INSERT INTO outcome_anchors SELECT tenant,environment,target,invocation_id,kind,id,content_hash FROM outcome_members WHERE kind='evidence' LIMIT 1").execute(&mut *tx).await.unwrap();
                }
            }
            _ => unreachable!(),
        }
        tx.commit().await.unwrap();
        let before = inventory(&store).await;
        if case == "alias" {
            let mut read = store.begin_read(deadline()).await.unwrap();
            read.load_authority(&w).await.unwrap();
            let raw = read.load_retained(&s).await.unwrap();
            read.finish().await.unwrap();
            assert_eq!(raw.original_deliveries.len(), 1);
            assert_eq!(raw.original_deliveries[0], *f.plans[0].delivery());
            load(&store, &f, &LocalAuthority::new(&f)).await.unwrap();
        } else {
            assert!(
                matches!(
                    load(&store, &f, &LocalAuthority::new(&f)).await,
                    Err(ComparisonError::Integrity)
                ),
                "{case}"
            );
        }
        reopen(&dir, store, &before).await.close().await;
    }
}

#[tokio::test]
async fn comparison_sqlite_exact_head_and_authority_reference_admission() {
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f, 1).await;
    let (w, s) = input(&f);
    let mut read = store.begin_read(deadline()).await.unwrap();
    read.load_authority(&w).await.unwrap();
    let count = read.load_retained(&s).await.unwrap().heads.len();
    read.finish().await.unwrap();
    // Deliberately structural-only records isolate the reader count gate from
    // the stricter downstream frozen-policy verification.
    for n in 0..=(MAX_HEADS - count) {
        let mut tx = store.inner.writer.begin().await.unwrap();
        let id = encode(&json!(format!("synthetic-policy-{n}"))).unwrap();
        let hash = format!("sha256:{}", "0".repeat(64));
        let raw=encode(&json!({"kind":"policy-snapshot","body":{"agreement_id":"extra-agreement","family_id":format!("family-{n}")}})).unwrap();
        sqlx::query("INSERT INTO outcome_records VALUES (?,?,'policy-snapshot',?,?,?)")
            .bind(&s.scope[0])
            .bind(&s.scope[1])
            .bind(&id)
            .bind(&hash)
            .bind(raw)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO outcome_members VALUES (?,?,?,?,'policy-snapshot',?,?)")
            .bind(&s.scope[0])
            .bind(&s.scope[1])
            .bind(&s.target)
            .bind(&s.invocation_id)
            .bind(&id)
            .bind(&hash)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        if n + count >= MAX_HEADS - 1 {
            let before = inventory(&store).await;
            let mut read = store.begin_read(deadline()).await.unwrap();
            read.load_authority(&w).await.unwrap();
            let r = read.load_retained(&s).await;
            if n + count == MAX_HEADS - 1 {
                assert_eq!(r.unwrap().heads.len(), MAX_HEADS);
            } else {
                assert_eq!(r.unwrap_err(), ReadError::Limit);
            }
            read.finish().await.unwrap();
            assert_eq!(inventory(&store).await, before);
        }
    }
    let before = inventory(&store).await;
    let store = reopen(&dir, store, &before).await;
    let mut read = store.begin_read(deadline()).await.unwrap();
    let mut refs = vec![];
    let mut seen = BTreeSet::new();
    for n in 0..=MAX_REFERENCES {
        let v = json!({"kind":"evidence","id":format!("ref-{n}"),"content_hash":format!("sha256:{}","0".repeat(64))});
        let result = read.references(&v, &w.scope, &mut refs, &mut seen);
        if n < MAX_REFERENCES {
            result.unwrap();
        } else {
            assert_eq!(result, Err(ReadError::Limit));
        }
    }
    assert_eq!(refs.len(), MAX_REFERENCES);
    read.finish().await.unwrap();
    reopen(&dir, store, &before).await.close().await;
}

fn trace_has_mutation(sql: &str) -> bool {
    sql.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| {
            matches!(
                word.to_ascii_uppercase().as_str(),
                "INSERT"
                    | "UPDATE"
                    | "DELETE"
                    | "REPLACE"
                    | "CREATE"
                    | "DROP"
                    | "ALTER"
                    | "ATTACH"
                    | "DETACH"
                    | "PRAGMA"
                    | "VACUUM"
                    | "REINDEX"
                    | "ANALYZE"
            )
        })
}
// Every constructed concrete reader (including facade denials, faults and
// cancelled futures) checks its actual submitted statements on destruction.
// Engine-only enforcement probes intentionally use the test connection directly;
// source coverage below proves the production mapper cannot take that bypass.
impl Drop for ReadStatements {
    fn drop(&mut self) {
        if !self.audit {
            return;
        }
        let trace = self.trace.lock().unwrap();
        assert!(!trace.is_empty());
        if std::env::var("LEDGERLAB_SQLITE_COMPARISON_TRACE").as_deref() == Ok("1") {
            eprintln!(
                "COMPARISON_SQL_TRACE {}",
                json!({"test":std::thread::current().name(),"phase":self.label,"statements":*trace,"application_write_attempts":trace.iter().filter(|s|trace_has_mutation(s)).count()})
            );
        }
        assert!(
            trace.iter().all(|sql| !trace_has_mutation(sql)),
            "attempted comparison mutation: {trace:?}"
        );
    }
}
#[test]
fn comparison_sqlite_runtime_statement_trace_positive_controls_and_no_bypass() {
    let mut statements = ReadStatements::default();
    for sql in [
        "UPDATE installation SET generation=generation WHERE 0",
        "CREATE TEMP TABLE trap (id INTEGER)",
        "INSERT INTO outcome_deliveries SELECT * FROM outcome_deliveries WHERE 0",
        "INSERT INTO outcome_heads SELECT * FROM outcome_heads WHERE 0",
        "SELECT 1; DELETE FROM outcome_members",
    ] {
        assert!(statements.select(sql).is_err());
        let trace = statements.trace.lock().unwrap();
        assert_eq!(trace.last().unwrap(), sql);
        assert!(trace_has_mutation(trace.last().unwrap()));
        if std::env::var("LEDGERLAB_SQLITE_COMPARISON_TRACE").as_deref() == Ok("1") {
            eprintln!(
                "COMPARISON_SQL_POSITIVE_CONTROL {}",
                json!({"attempt":sql,"recorded_before_rejection":true})
            );
        }
    }
    let source = include_str!("comparison.rs");
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    let calls = compact.split("sqlx::query").skip(1).collect::<Vec<_>>();
    assert!(!calls.is_empty());
    for call in calls {
        assert!(
            call.starts_with("(self.statements.select(")
                || call.starts_with("_as(self.statements.select(")
                || call.starts_with("_as::<_,(i64,i64,i64)>(self.statements.select(")
                || call.starts_with("_scalar(statements.select("),
            "untraced query constructor: {call}"
        );
    }
    assert_eq!(
        source.matches("AssertSqlSafe(sql)").count(),
        1,
        "only the traced gate may bless SQL"
    );
    for bypass in [
        ".execute(",
        ".execute_many(",
        "raw_sql(",
        "query!(",
        "query_as!(",
        "query_scalar!(",
        "query_with(",
        "query_as_with(",
        "query_scalar_with(",
        ".prepare(",
        ".fetch(",
        ".fetch_many(",
        ".fetch_all(\"",
        "Executor::",
        "super::outcomes",
        "load_extension",
    ] {
        assert!(!source.contains(bypass), "untraced bypass {bypass}");
    }
}

#[tokio::test]
async fn comparison_sqlite_actual_authority_and_history_share_preallocation_budget() {
    use ledgerlab_core::canonical::outcome as codec;
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f, 1).await;
    let (w, s) = input(&f);
    let template = parse(&LocalAuthority::new(&f).grant).unwrap();
    let mut tx = store.inner.writer.begin().await.unwrap();
    let mut refs = vec![];
    for n in 0..88 {
        let mut body = template["body"].clone();
        body["utf8"] = json!(format!("{{\"padding\":\"{}-{n}\"}}", "x".repeat(220000)));
        // A structurally valid retained envelope isolates preallocation from
        // later semantic document/economic checks. It confers no permission.
        let v = codec::envelope(codec::ECONOMIC, "evidence", &json!(s.scope), body).unwrap();
        let raw = encode(&v).unwrap();
        codec::decode(&raw).unwrap();
        sqlx::query("INSERT INTO outcome_records VALUES (?,?,'evidence',?,?,?)")
            .bind(&s.scope[0])
            .bind(&s.scope[1])
            .bind(encode(&v["id"]).unwrap())
            .bind(v["content_hash"].as_str().unwrap())
            .bind(raw)
            .execute(&mut *tx)
            .await
            .unwrap();
        if n < 48 {
            refs.push(codec::reference(&v));
        } else {
            sqlx::query("INSERT INTO outcome_members VALUES (?,?,?,?,'evidence',?,?)")
                .bind(&s.scope[0])
                .bind(&s.scope[1])
                .bind(&s.target)
                .bind(&s.invocation_id)
                .bind(encode(&v["id"]).unwrap())
                .bind(v["content_hash"].as_str().unwrap())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE outcome_heads SET value=? WHERE class=1")
        .bind(
            encode(&json!({"active":true,"grant":codec::reference(&template),"extra":refs}))
                .unwrap(),
        )
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = inventory(&store).await;
    let mut read = store.begin_read(deadline()).await.unwrap();
    read.statements.label = "actual-shared-budget";
    assert_eq!(read.load_authority(&w).await.unwrap().records.len(), 49);
    assert_eq!(read.load_retained(&s).await.unwrap_err(), ReadError::Limit);
    assert!(
        !read
            .statements
            .trace
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.starts_with("SELECT r.canonical_bytes FROM")),
        "aggregate rejection must precede history body allocation"
    );
    read.finish().await.unwrap();
    reopen(&dir, store, &before).await.close().await;
}

#[tokio::test]
async fn comparison_sqlite_foundation_report_and_oracle() {
    let f = fixture::lifecycle();
    let (dir, store) = prepared(&f, f.plans.len()).await;
    let before = inventory(&store).await;
    let (who, selection) = input(&f);
    let report = facade::foundation::tests::report(
        &LabelledReader(&store, "foundation-report"),
        &LocalAuthority::new(&f),
        &who,
        selection.clone(),
    )
    .await;
    let after = inventory(&store).await;
    assert_eq!(before, after);
    let store = reopen(&dir, store, &before).await;
    let reopened = inventory(&store).await;
    let repeated = facade::foundation::tests::report(
        &LabelledReader(&store, "foundation-reopen"),
        &LocalAuthority::new(&f),
        &who,
        selection,
    )
    .await;
    assert_eq!(report.bytes(), repeated.bytes());
    facade::foundation::tests::preserve_report(&report, "sqlite");
    facade::foundation::tests::observe(
        &report,
        "sqlite",
        json!(before),
        json!(after),
        json!(reopened),
        json!({"B0":foundation_metadata(&before),"B1":foundation_metadata(&after),"B2":foundation_metadata(&reopened)}),
    );
    if let Ok(dir) = std::env::var("LEDGERLAB_FOUNDATION_EVIDENCE_DIR") {
        std::fs::write(
            format!("{dir}/sqlite-inventories.json"),
            serde_json::to_vec(&json!({"backend":"sqlite","B0":before,"B1":after,"B2":reopened,"metadata":{"B0":foundation_metadata(&before),"B1":foundation_metadata(&after),"B2":foundation_metadata(&reopened)}}))
                .unwrap(),
        )
        .unwrap();
    }
    store.close().await;
}

fn foundation_metadata(rows: &[(String, Vec<String>, Vec<String>)]) -> Value {
    json!({"schema":rows.iter().find(|r|r.0=="sqlite_schema").unwrap().2,"user_version":rows.iter().find(|r|r.0=="user_version").unwrap().2})
}
