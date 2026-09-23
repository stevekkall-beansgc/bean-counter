//! Disposable real PG tests. Owner/writer access occurs ONLY in fixture setup,
//! adversarial mutations and the independent correction; never in the reader.
use super::*;
use crate::{
    service::{
        accept::outcome::{self as accept, fixture},
        comparison::*,
    },
    store::{
        postgres::{self, PostgresStore},
        records::Scope,
    },
    PostgresTrust,
};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
const WRITER: &str = "ledgerlab_phase1_runtime";
const READER: &str = "ledgerlab_p4_reader";
fn config(database: &str, user: &str) -> PostgresConfig {
    PostgresConfig {
        host: "localhost".into(),
        port: std::env::var("LEDGERLAB_PG_TEST_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        user: user.into(),
        database: database.into(),
        password: std::env::var("LEDGERLAB_PG_TEST_PASSWORD")
            .unwrap()
            .into_bytes(),
        trust: PostgresTrust::PemOnly(
            std::fs::read(std::env::var("LEDGERLAB_PG_TEST_CA").unwrap()).unwrap(),
        ),
    }
}
struct Fixture {
    name: String,
    owner: Session,
    writer: PostgresStore,
    reader: PostgresComparisonStore,
    data: fixture::Fixture,
    who: AuthenticatedReadContext,
    selection: RetainedSelection,
}
impl Fixture {
    async fn new(steps: usize) -> Self {
        let name = format!(
            "ledgerlab_p4_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let admin = config("ledgerlab", "postgres").connect().await.unwrap();
        // Role provisioning is explicit disposable test setup, not a reader fallback.
        if admin
            .client
            .query_opt("SELECT 1 FROM pg_roles WHERE rolname=$1", &[&READER])
            .await
            .unwrap()
            .is_none()
        {
            let password = String::from_utf8(config("ledgerlab", "postgres").password).unwrap();
            assert!(password
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'));
            admin
                .client
                .batch_execute(&format!("CREATE ROLE {READER} LOGIN PASSWORD '{password}'"))
                .await
                .unwrap();
        }
        admin
            .client
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        admin.discard().await;
        let mut owner = config(&name, "postgres").connect().await.unwrap();
        let mut installation = crate::store::sqlite::tests::installation();
        installation.scope = Scope {
            tenant: "synthetic".into(),
            environment: "sandbox".into(),
        };
        postgres::migrate::create(&mut owner.client, installation, WRITER)
            .await
            .unwrap();
        owner.client.batch_execute(&format!("GRANT USAGE ON SCHEMA ledgerlab TO {READER}; GRANT SELECT ON ALL TABLES IN SCHEMA ledgerlab TO {READER}; REVOKE TEMP ON DATABASE {name} FROM PUBLIC")).await.unwrap();
        let writer = PostgresStore::open(config(&name, WRITER)).await.unwrap();
        assert_eq!(
            writer.version / 10000,
            std::env::var("LEDGERLAB_PG_TEST_MAJOR")
                .unwrap()
                .parse::<i32>()
                .unwrap()
        );
        let data = fixture::lifecycle();
        let q = data.plans[0].resolution();
        let tx = owner.client.transaction().await.unwrap();
        let mut held = postgres::outcomes::Locked::default();
        postgres::outcomes::lock(&tx, &mut held, &q.locks)
            .await
            .unwrap();
        for raw in &data.provisioned_records {
            let v = parsed(raw).unwrap();
            let id = bytes(&v["id"]).unwrap();
            tx.execute(
                "INSERT INTO ledgerlab.outcome_records VALUES($1,$2,$3,$4,$5,$6)",
                &[
                    &q.delivery.scope[0],
                    &q.delivery.scope[1],
                    &text(&v["kind"]).unwrap(),
                    &id,
                    &text(&v["content_hash"]).unwrap(),
                    raw,
                ],
            )
            .await
            .unwrap();
            tx.execute(
                "INSERT INTO ledgerlab.outcome_members VALUES($1,$2,$3,$4,$5,$6)",
                &[
                    &q.delivery.scope[0],
                    &q.delivery.scope[1],
                    &q.target,
                    &q.invocation_id,
                    &text(&v["kind"]).unwrap(),
                    &id,
                ],
            )
            .await
            .unwrap();
        }
        for h in &data.provisioned_heads {
            if let (Some(rev), Some(value)) = (&h.revision, &h.value) {
                tx.execute(
                    "INSERT INTO ledgerlab.outcome_heads VALUES($1,$2,$3,$4,$5,$6)",
                    &[
                        &q.delivery.scope[0],
                        &q.delivery.scope[1],
                        &(h.lock.class as i16),
                        &h.lock.key,
                        &rev.parse::<i64>().unwrap(),
                        value,
                    ],
                )
                .await
                .unwrap();
            }
        }
        tx.commit().await.unwrap();
        for command in data.commands.iter().take(steps) {
            assert!(matches!(
                accept::run(&writer, command, &WriteAuthority(data.proofs.clone()))
                    .await
                    .unwrap(),
                accept::OutcomeResult::Accepted(_)
            ));
        }
        let who = AuthenticatedReadContext {
            scope: q.delivery.scope.clone(),
            principal_id: "synthetic-authorized-principal".into(),
            authority_head: "operator".into(),
        };
        let selection = RetainedSelection {
            scope: who.scope.clone(),
            target: q.target.clone(),
            invocation_id: q.invocation_id.clone(),
            expected_snapshot: None,
        };
        let reader =
            PostgresComparisonStore::new(config(&name, READER), "synthetic-store".into()).unwrap();
        Self {
            name,
            owner,
            writer,
            reader,
            data,
            who,
            selection,
        }
    }
    async fn inventory(&self) -> Value {
        inventory(&self.owner).await
    }
    async fn unchanged(&self, before: &Value, label: &str) {
        self.reader.audit.lock().unwrap().zero();
        let after = self.inventory().await;
        assert_eq!(&after, before, "B1 {label}");
        let reopened = config(&self.name, "postgres").connect().await.unwrap();
        let reopen = inventory(&reopened).await;
        reopened.discard().await;
        assert_eq!(&reopen, before, "B2 {label}");
        if let Ok(dir) = std::env::var("LEDGERLAB_P4_EVIDENCE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                format!("{dir}/{}-{label}.json", self.name),
                serde_json::to_vec(&json!({"B0":before,"B1":after,"B2":reopen,"comparison_pids":self.reader.audit.lock().unwrap().pids,"attempted_writes":0,"external_calls":0})).unwrap(),
            )
            .unwrap();
        }
    }
    async fn workspace(
        &self,
        auth: &ReadAuthority,
        selection: RetainedSelection,
    ) -> Result<ComparisonWorkspace, ComparisonError> {
        let operation = ComparisonOperation::begin(Cancellation::default()).unwrap();
        load_workspace(&self.reader, auth, &self.who, selection, &operation).await
    }
    async fn finish(self) {
        self.writer.close().await;
        self.owner.discard().await;
        let admin = config("ledgerlab", "postgres").connect().await.unwrap();
        admin
            .client
            .batch_execute(&format!("DROP DATABASE {}", self.name))
            .await
            .unwrap();
        admin.discard().await;
    }
}
async fn inventory(session: &Session) -> Value {
    let rows = session
        .client
        .query(
            "SELECT tablename FROM pg_tables WHERE schemaname='ledgerlab' ORDER BY tablename",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 54); // Original37 plus17 native R3 tables, including publication witness.
    let mut tables = serde_json::Map::new();
    for row in rows {
        let name: String = row.get(0);
        assert!(name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
        let columns = session.client.query("SELECT column_name,data_type,is_nullable,column_default FROM information_schema.columns WHERE table_schema='ledgerlab' AND table_name=$1 ORDER BY ordinal_position", &[&name]).await.unwrap().iter().map(|r| json!([r.get::<_,String>(0),r.get::<_,String>(1),r.get::<_,String>(2),r.get::<_,Option<String>>(3)])).collect::<Vec<_>>();
        let data = session.client.query(&format!("SELECT row_to_json(t)::text FROM ledgerlab.\"{name}\" t ORDER BY row_to_json(t)::text"), &[]).await.unwrap().iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>();
        tables.insert(name, json!({"columns":columns,"rows":data}));
    }
    Value::Object(tables)
}
struct WriteAuthority(Vec<accept::AuthorityProof>);
impl accept::OutcomeAuthority for WriteAuthority {
    fn verify(
        &self,
        c: &accept::OutcomeCommand,
        _: &OutcomeSnapshot,
        write: bool,
    ) -> Result<accept::AuthorityProof, crate::ServiceError> {
        assert_eq!(c.principal.principal_id, "synthetic-authorized-principal");
        assert!(c.principal.can_read && (!write || c.principal.can_submit));
        Ok(self
            .0
            .iter()
            .find(|p| {
                p.source == c.principal.source
                    && p.target == c.target
                    && p.invocation_id == c.invocation_id
            })
            .unwrap()
            .clone())
    }
}
// A deliberately local fixture verifier with exact current principal/grant and
// full retained binding selection; absent/inactive/mismatched observations deny.
struct ReadAuthority {
    deny_scope: bool,
    deny_disclosure: bool,
}
impl ReadAuthority {
    fn allowed() -> Self {
        Self {
            deny_scope: false,
            deny_disclosure: false,
        }
    }
}
impl ComparisonReadAuthority for ReadAuthority {
    fn verify_scope(
        &self,
        who: &AuthenticatedReadContext,
        selection: &RetainedSelection,
        observed: &ReadAuthorityObservation,
    ) -> Result<(), ComparisonError> {
        if self.deny_scope
            || who.principal_id != "synthetic-authorized-principal"
            || who.scope != observed.scope
            || who.scope != selection.scope
        {
            return Err(ComparisonError::Denied);
        }
        let h = observed
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Authority)
            .ok_or(ComparisonError::Denied)?;
        let value = parsed(h.value.as_deref().ok_or(ComparisonError::Denied)?)
            .map_err(|_| ComparisonError::Denied)?;
        if h.revision.as_deref() != Some("1")
            || value["active"] != true
            || !observed.records.iter().any(|raw| {
                parsed(raw).is_ok_and(|v| {
                    v["id"] == value["grant"]["id"]
                        && v["content_hash"] == value["grant"]["content_hash"]
                })
            })
        {
            return Err(ComparisonError::Denied);
        }
        Ok(())
    }
    fn verify_disclosure(
        &self,
        _: &AuthenticatedReadContext,
        _: &RetainedSelection,
        _: &ReadAuthorityObservation,
        raw: &RawRetainedSnapshot,
    ) -> Result<(), ComparisonError> {
        if self.deny_disclosure || raw.records.is_empty() {
            Err(ComparisonError::Denied)
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_success_denial_reopen_and_select_role() {
    let f = Fixture::new(4).await;
    let mut alias = f.data.commands[1].clone();
    alias.principal.can_submit = false;
    if let accept::OutcomeOperation::Economic { ingress, .. } = &mut alias.operation {
        let mut event = parsed(ingress).unwrap();
        event["data"]["external_id"] = json!("comparison-existing-alias");
        *ingress = bytes(&event).unwrap();
    }
    assert!(matches!(
        accept::run(&f.writer, &alias, &WriteAuthority(f.data.proofs.clone()))
            .await
            .unwrap(),
        accept::OutcomeResult::Duplicate(_)
    ));
    let before = f.inventory().await;
    let w = f
        .workspace(&ReadAuthority::allowed(), f.selection.clone())
        .await
        .unwrap();
    assert_eq!(w.historical_receipts().len(), 4);
    let mut pinned = f.selection.clone();
    pinned.expected_snapshot = Some(w.fingerprint().clone());
    assert_eq!(
        f.workspace(&ReadAuthority::allowed(), pinned.clone())
            .await
            .unwrap()
            .fingerprint(),
        w.fingerprint()
    );
    f.unchanged(&before, "success").await;
    pinned.expected_snapshot = Some(SnapshotFingerprint("0".repeat(64)));
    assert!(matches!(
        f.workspace(&ReadAuthority::allowed(), pinned).await,
        Err(ComparisonError::SnapshotChanged)
    ));
    for disclosure in [false, true] {
        let auth = ReadAuthority {
            deny_scope: !disclosure,
            deny_disclosure: disclosure,
        };
        let mut selection = f.selection.clone();
        if !disclosure {
            selection.target = "nonexistent".into();
        }
        assert!(matches!(
            f.workspace(&auth, selection).await,
            Err(ComparisonError::Denied)
        ));
        f.unchanged(
            &before,
            if disclosure {
                "disclosure-denial"
            } else {
                "scope-denial-before-notfound"
            },
        )
        .await;
    }
    // Each probe runs on the SAME reader session after actual successful reads.
    // SELECT-only privilege and ordinary runtime READ ONLY are checked separately.
    for user in [READER, WRITER] {
        let reader =
            PostgresComparisonStore::new(config(&f.name, user), "synthetic-store".into()).unwrap();
        let mut tx = reader
            .begin_read(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        tx.load_authority(&f.who).await.unwrap();
        tx.load_retained(&f.selection).await.unwrap();
        let c = &tx.state.as_ref().unwrap().session.client;
        let r = c.query_one("SELECT current_setting('transaction_isolation'), current_setting('transaction_read_only'), has_column_privilege(current_user,'ledgerlab.outcome_heads','value','UPDATE')", &[]).await.unwrap();
        assert_eq!(r.get::<_, String>(0), "repeatable read");
        assert_eq!(r.get::<_, String>(1), "on");
        assert_eq!(r.get::<_, bool>(2), user == WRITER);
        for sql in [
            "INSERT INTO ledgerlab.outcome_heads SELECT * FROM ledgerlab.outcome_heads WHERE false",
            "UPDATE ledgerlab.outcome_heads SET value=value WHERE false",
            "DELETE FROM ledgerlab.outcome_heads WHERE false",
            "CREATE TABLE ledgerlab.forbidden_reader_probe(x integer)",
            "SELECT * FROM ledgerlab.outcome_heads FOR UPDATE",
            "SELECT * FROM ledgerlab.outcome_heads FOR SHARE",
        ] {
            c.batch_execute("SAVEPOINT probe").await.unwrap();
            let e = c.batch_execute(sql).await.unwrap_err();
            assert!(
                matches!(e.code().map(|c| c.code()), Some("25006" | "42501")),
                "{e}"
            );
            c.batch_execute("ROLLBACK TO SAVEPOINT probe; RELEASE SAVEPOINT probe")
                .await
                .unwrap();
        }
        tx.finish().await.unwrap();
        if let Ok(dir) = std::env::var("LEDGERLAB_P4_EVIDENCE_DIR") {
            let pids = reader.audit.lock().unwrap().pids.clone();
            std::fs::write(format!("{dir}/{}-positive-probes-{user}.json", f.name), serde_json::to_vec(&json!({"probe_pids":pids,"rejected_operations":["INSERT","UPDATE","DELETE","DDL","FOR UPDATE","FOR SHARE"]})).unwrap()).unwrap();
        }
    }
    f.unchanged(&before, "sql-denial-probes").await;
    f.finish().await;
}

// Independent attempted-SQL and outbound-connection traps. Test builds run the
// actual reader through both. A source allowlist below checks that new call sites
// cannot silently bypass the two instrumented paths. Neither probe executes a
// forbidden statement or opens an external connection.
pub(super) struct Audit {
    host: String,
    port: u16,
    writes: usize,
    external: usize,
    queries: Vec<String>,
    pub(super) pids: Vec<i32>,
    connections: usize,
}
impl Audit {
    pub(super) fn new(c: &PostgresConfig) -> Self {
        Self {
            host: c.host.clone(),
            port: c.port,
            writes: 0,
            external: 0,
            queries: vec![],
            pids: vec![],
            connections: 0,
        }
    }
    pub(super) fn sql(&mut self, sql: &str) -> Result<(), ReadError> {
        let upper = sql.to_ascii_uppercase();
        if !sql.starts_with("SELECT ")
            || [
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "CREATE ",
                "ALTER ",
                "DROP ",
                " FOR SHARE",
                " FOR UPDATE",
                "PG_ADVISORY",
                "NEXTVAL(",
                "SETVAL(",
            ]
            .iter()
            .any(|s| upper.contains(s))
        {
            self.writes += 1;
            return Err(ReadError::Integrity);
        }
        self.queries.push(sql.into());
        Ok(())
    }
    pub(super) fn connection(&mut self, c: &PostgresConfig) -> Result<(), ReadError> {
        if c.host != self.host || c.port != self.port {
            self.external += 1;
            return Err(ReadError::Integrity);
        }
        self.connections += 1;
        Ok(())
    }
    fn zero(&self) {
        assert_eq!(self.writes, 0, "attempted comparison write");
        assert_eq!(self.external, 0, "attempted external call");
        assert!(self.connections > 0);
    }
}
#[test]
fn postgres_comparison_separate_traps_and_source_boundary() {
    let c = PostgresConfig {
        host: "localhost".into(),
        port: 12345,
        user: "test".into(),
        password: vec![],
        database: "test".into(),
        trust: PostgresTrust::Public,
    };
    let mut trap = Audit::new(&c);
    assert!(trap
        .sql("UPDATE ledgerlab.outcome_heads SET revision=0")
        .is_err());
    assert_eq!((trap.writes, trap.external), (1, 0));
    let mut external = c.clone();
    external.host = "example.invalid".into();
    assert!(trap.connection(&external).is_err());
    assert_eq!((trap.writes, trap.external), (1, 1));
    let source = include_str!("mod.rs");
    let queries = include_str!("queries.rs");
    assert_eq!(source.matches(".connect()").count(), 1);
    assert!(!queries.contains(".client"));
    for forbidden in [
        "AcceptanceTx",
        "OutcomeTx",
        "PostgresStore",
        "append",
        "TcpStream",
        "reqwest",
        "std::net",
        "outbox::",
        "maintenance::",
        "FOR UPDATE",
        "FOR SHARE",
        "pg_advisory",
    ] {
        assert!(
            !source.contains(forbidden) && !queries.contains(forbidden),
            "reader acquired {forbidden}"
        );
    }
}

async fn replace_envelope(f: &Fixture, body: &[u8]) {
    // Owner-only corruption fixture. Application triggers are re-enabled before B0.
    f.owner
        .client
        .batch_execute(
            "ALTER TABLE ledgerlab.outcome_records DISABLE TRIGGER outcome_records_immutable",
        )
        .await
        .unwrap();
    f.owner
        .client
        .execute(
            "UPDATE ledgerlab.outcome_records SET envelope=$1 WHERE kind='base-evaluation'",
            &[&body],
        )
        .await
        .unwrap();
    f.owner
        .client
        .batch_execute(
            "ALTER TABLE ledgerlab.outcome_records ENABLE TRIGGER outcome_records_immutable",
        )
        .await
        .unwrap();
}
async fn raw_error(f: &Fixture, expected: ReadError) {
    let mut tx = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.load_authority(&f.who).await.unwrap();
    assert!(matches!(tx.load_retained(&f.selection).await, Err(e) if e == expected));
    tx.finish().await.unwrap();
}
#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_malformed_and_preallocation_limits() {
    let f = Fixture::new(1).await;
    replace_envelope(&f, b"[]").await;
    let before = f.inventory().await;
    assert!(matches!(
        f.workspace(&ReadAuthority::allowed(), f.selection.clone())
            .await,
        Err(ComparisonError::Integrity)
    ));
    f.unchanged(&before, "malformed").await;
    replace_envelope(&f, &vec![b'x'; MAX_ENVELOPE_BYTES + 1]).await;
    let before = f.inventory().await;
    let start = f.reader.audit.lock().unwrap().queries.len();
    raw_error(&f, ReadError::Limit).await;
    assert!(!f.reader.audit.lock().unwrap().queries[start..]
        .iter()
        .any(|q| q.starts_with("SELECT r.envelope")));
    f.unchanged(&before, "envelope-limit-before-fetch").await;
    f.finish().await;

    let f = Fixture::new(1).await;
    f.owner.client.execute("INSERT INTO ledgerlab.outcome_records SELECT 'synthetic','sandbox','evidence',convert_to('extra-'||n,'UTF8'),'sha256:'||repeat('0',64),convert_to('{}','UTF8') FROM generate_series(1,4097) n", &[]).await.unwrap();
    f.owner.client.execute("INSERT INTO ledgerlab.outcome_members SELECT tenant,environment,$1,$2,kind,id FROM ledgerlab.outcome_records WHERE convert_from(id,'UTF8') LIKE 'extra-%'", &[&f.selection.target,&f.selection.invocation_id]).await.unwrap();
    let before = f.inventory().await;
    raw_error(&f, ReadError::Limit).await;
    f.unchanged(&before, "membership-count-limit").await;
    f.finish().await;

    let f = Fixture::new(1).await;
    // A valid-size authority head containing too many independently bounded refs.
    let refs = (0..1025).map(|i| json!({"kind":"evidence","id":format!("ref-{i}"),"content_hash":format!("sha256:{}","0".repeat(64))})).collect::<Vec<_>>();
    let head = bytes(&json!({"refs":refs})).unwrap();
    f.owner
        .client
        .execute(
            "UPDATE ledgerlab.outcome_heads SET revision=revision+1,value=$1 WHERE class=1",
            &[&head],
        )
        .await
        .unwrap();
    let before = f.inventory().await;
    let mut tx = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(
        tx.load_authority(&f.who).await,
        Err(ReadError::Limit)
    ));
    tx.finish().await.unwrap();
    f.unchanged(&before, "authority-reference-limit").await;
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_shared_authority_retained_budget() {
    let f = Fixture::new(1).await;
    let mut refs = vec![];
    // 9 MiB authority and 8 MiB retained are each below 16 MiB. Together they
    // must fail before retained body fetch, not after allocation or decoding.
    for i in 0..36 {
        let id = json!(format!("authority-{i}"));
        let hash = format!("sha256:{}", "0".repeat(64));
        let mut v = json!({"kind":"evidence","id":id,"scope":f.who.scope,"content_hash":hash,"body":{"padding":""}});
        let len = bytes(&v).unwrap().len();
        v["body"]["padding"] = json!("a".repeat(256 * 1024 - len));
        let raw = bytes(&v).unwrap();
        f.owner.client.execute("INSERT INTO ledgerlab.outcome_records VALUES('synthetic','sandbox','evidence',$1,$2,$3)", &[&bytes(&id).unwrap(), &hash, &raw]).await.unwrap();
        refs.push(json!({"kind":"evidence","id":id,"content_hash":hash}));
    }
    f.owner
        .client
        .execute(
            "UPDATE ledgerlab.outcome_heads SET revision=revision+1,value=$1 WHERE class=1",
            &[&bytes(&json!({"refs":refs})).unwrap()],
        )
        .await
        .unwrap();
    f.owner.client.execute("INSERT INTO ledgerlab.outcome_records SELECT 'synthetic','sandbox','evidence',convert_to('budget-'||n,'UTF8'),'sha256:'||repeat('0',64),convert_to(repeat('x',262144),'UTF8') FROM generate_series(1,32) n", &[]).await.unwrap();
    f.owner.client.execute("INSERT INTO ledgerlab.outcome_members SELECT tenant,environment,$1,$2,kind,id FROM ledgerlab.outcome_records WHERE convert_from(id,'UTF8') LIKE 'budget-%'", &[&f.selection.target,&f.selection.invocation_id]).await.unwrap();
    let before = f.inventory().await;
    raw_error(&f, ReadError::Limit).await;
    assert!(!f
        .reader
        .audit
        .lock()
        .unwrap()
        .queries
        .iter()
        .any(|q| q.starts_with("SELECT r.envelope")));
    f.unchanged(&before, "shared-16mib-budget").await;
    f.finish().await;
}
async fn gone(f: &Fixture, pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let count: i64 = f
            .owner
            .client
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE pid=$1",
                &[&pid],
            )
            .await
            .unwrap()
            .get(0);
        if count == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "comparison backend leaked");
        tokio::task::yield_now().await;
    }
}
async fn pid(tx: &PostgresComparisonTx) -> i32 {
    tx.state
        .as_ref()
        .unwrap()
        .session
        .client
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0)
}
#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_cancellation_deadline_and_discard() {
    let f = Fixture::new(1).await;
    let before = f.inventory().await;
    let tx = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let backend = pid(&tx).await;
    drop(tx);
    gone(&f, backend).await;
    f.unchanged(&before, "dropped-transaction").await;
    let mut tx = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.load_authority(&f.who).await.unwrap();
    let backend = pid(&tx).await;
    // Real server wait on metadata access, then drop the in-flight load future.
    let blocker = config(&f.name, "postgres").connect().await.unwrap();
    blocker
        .client
        .batch_execute("BEGIN; LOCK TABLE ledgerlab.outcome_members IN ACCESS EXCLUSIVE MODE")
        .await
        .unwrap();
    let mut load = Box::pin(tx.load_retained(&f.selection));
    tokio::select! {
        result = &mut load => panic!("reader escaped actual blocker: {result:?}"),
        _ = async {
            let deadline = Instant::now()+Duration::from_secs(1);
            loop {
                let waiting: bool = f.owner.client.query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&backend]).await.unwrap().get(0);
                if waiting { break; }
                assert!(Instant::now()<deadline); tokio::task::yield_now().await;
            }
        } => {}
    }
    drop(load);
    gone(&f, backend).await;
    assert!(matches!(tx.finish().await, Err(ReadError::Unavailable)));
    blocker.client.batch_execute("ROLLBACK").await.unwrap();
    blocker.discard().await;
    f.unchanged(&before, "cancelled-pending-select").await;
    let mut tx = f
        .reader
        .begin_read(Instant::now() + Duration::from_millis(150))
        .await
        .unwrap();
    let backend = pid(&tx).await;
    tokio::time::sleep(Duration::from_millis(160)).await;
    assert!(matches!(
        tx.load_authority(&f.who).await,
        Err(ReadError::Deadline)
    ));
    tx.finish().await.unwrap();
    gone(&f, backend).await;
    f.unchanged(&before, "expired-deadline").await;
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_concurrent_authorized_correction_whole_snapshot() {
    let f = Fixture::new(2).await;
    let mut pinned = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let authority_old = pinned.load_authority(&f.who).await.unwrap();
    let old = pinned.load_retained(&f.selection).await.unwrap();
    pinned.finish().await.unwrap();
    let mut pinned = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    pinned.load_authority(&f.who).await.unwrap();
    // Commit a genuine authorized Phase 3 correction after snapshot pinning and
    // before all retained preflight/fetch queries. No synthetic head-only race.
    assert!(matches!(
        accept::run(
            &f.writer,
            &f.data.commands[2],
            &WriteAuthority(f.data.proofs.clone())
        )
        .await
        .unwrap(),
        accept::OutcomeResult::Accepted(_)
    ));
    let after_writer = f.inventory().await;
    let observed = pinned.load_retained(&f.selection).await.unwrap();
    pinned.finish().await.unwrap();
    assert_eq!(snapshot(&observed), snapshot(&old));
    let mut fresh = f
        .reader
        .begin_read(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        fresh.load_authority(&f.who).await.unwrap().records,
        authority_old.records
    );
    let new = fresh.load_retained(&f.selection).await.unwrap();
    fresh.finish().await.unwrap();
    assert_ne!(snapshot(&new), snapshot(&old));
    assert_eq!(old.original_deliveries.len(), 2);
    assert_eq!(new.original_deliveries.len(), 3);
    assert_eq!(
        f.workspace(&ReadAuthority::allowed(), f.selection.clone())
            .await
            .unwrap()
            .historical_receipts()
            .len(),
        3
    );
    f.unchanged(&after_writer, "concurrent-correction").await;
    f.finish().await;
}
fn snapshot(r: &RawRetainedSnapshot) -> String {
    format!("{r:?}")
}

#[tokio::test]
#[ignore = "requires disposable local PostgreSQL 17/18 TLS service"]
async fn postgres_comparison_missing_components_and_head_bound() {
    for (table, trigger, label) in [
        (
            "outcome_anchors",
            "outcome_anchors_immutable",
            "missing-anchor",
        ),
        (
            "outcome_members",
            "outcome_members_immutable",
            "missing-member",
        ),
        (
            "outcome_deliveries",
            "outcome_deliveries_immutable",
            "missing-original-pair",
        ),
    ] {
        let f = Fixture::new(1).await;
        f.owner
            .client
            .batch_execute(&format!(
                "ALTER TABLE ledgerlab.{table} DISABLE TRIGGER {trigger}"
            ))
            .await
            .unwrap();
        f.owner.client.batch_execute(&format!("DELETE FROM ledgerlab.{table} WHERE ctid=(SELECT ctid FROM ledgerlab.{table} LIMIT 1)")).await.unwrap();
        f.owner
            .client
            .batch_execute(&format!(
                "ALTER TABLE ledgerlab.{table} ENABLE TRIGGER {trigger}"
            ))
            .await
            .unwrap();
        let before = f.inventory().await;
        assert!(
            matches!(
                f.workspace(&ReadAuthority::allowed(), f.selection.clone())
                    .await,
                Err(ComparisonError::Integrity)
            ),
            "{label}"
        );
        f.unchanged(&before, label).await;
        f.finish().await;
    }
    let f = Fixture::new(1).await;
    // The selection's record count/body bytes fit; deriving >128 distinct head
    // references must stop before querying or growing an unbounded head vector.
    for i in 0..64 {
        let id = bytes(&json!(format!("extra-binding-{i}"))).unwrap();
        let raw = bytes(
            &json!({"kind":"binding-snapshot","body":{"binding_id":format!("extra-binding-{i}")}}),
        )
        .unwrap();
        f.owner.client.execute("INSERT INTO ledgerlab.outcome_records VALUES('synthetic','sandbox','binding-snapshot',$1,$2,$3)", &[&id,&format!("sha256:{}","0".repeat(64)),&raw]).await.unwrap();
        f.owner.client.execute("INSERT INTO ledgerlab.outcome_members VALUES('synthetic','sandbox',$1,$2,'binding-snapshot',$3)", &[&f.selection.target,&f.selection.invocation_id,&id]).await.unwrap();
    }
    let before = f.inventory().await;
    raw_error(&f, ReadError::Limit).await;
    f.unchanged(&before, "head-count-limit").await;
    f.finish().await;
}

async fn foundation_metadata(session: &Session) -> Value {
    let indexes=session.client.query("SELECT indexname,indexdef FROM pg_indexes WHERE schemaname='ledgerlab' ORDER BY indexname",&[]).await.unwrap().iter().map(|r|json!([r.get::<_,String>(0),r.get::<_,String>(1)])).collect::<Vec<_>>();
    let constraints=session.client.query("SELECT c.relname,k.conname,pg_get_constraintdef(k.oid) FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' ORDER BY c.relname,k.conname",&[]).await.unwrap().iter().map(|r|json!([r.get::<_,String>(0),r.get::<_,String>(1),r.get::<_,String>(2)])).collect::<Vec<_>>();
    let triggers=session.client.query("SELECT c.relname,t.tgname,pg_get_triggerdef(t.oid) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' ORDER BY c.relname,t.tgname",&[]).await.unwrap().iter().map(|r|json!([r.get::<_,String>(0),r.get::<_,String>(1),r.get::<_,String>(2)])).collect::<Vec<_>>();
    json!({"schema":{"indexes":indexes,"constraints":constraints,"triggers":triggers}})
}
#[tokio::test]
#[ignore = "requires isolated local PostgreSQL and synthetic TLS"]
async fn postgres_comparison_foundation_report_and_oracle() {
    let mut f = Fixture::new(4).await;
    // Synthetic replicas use the same trusted logical identity as SQLite.
    f.reader =
        PostgresComparisonStore::new(config(&f.name, READER), "store-demo-slice".into()).unwrap();
    let before = f.inventory().await;
    let metadata_before = foundation_metadata(&f.owner).await;
    let report = crate::service::comparison::foundation::tests::report(
        &f.reader,
        &ReadAuthority::allowed(),
        &f.who,
        f.selection.clone(),
    )
    .await;
    let after = f.inventory().await;
    let metadata_after = foundation_metadata(&f.owner).await;
    let session = config(&f.name, "postgres").connect().await.unwrap();
    let reopened = inventory(&session).await;
    let metadata_reopened = foundation_metadata(&session).await;
    session.discard().await;
    assert_eq!(before, after);
    assert_eq!(before, reopened);
    assert_eq!(metadata_before, metadata_after);
    assert_eq!(metadata_before, metadata_reopened);
    let repeated = crate::service::comparison::foundation::tests::report(
        &f.reader,
        &ReadAuthority::allowed(),
        &f.who,
        f.selection.clone(),
    )
    .await;
    assert_eq!(report.bytes(), repeated.bytes());
    crate::service::comparison::foundation::tests::preserve_report(&report, "postgres");
    if let Ok(dir) = std::env::var("LEDGERLAB_FOUNDATION_EVIDENCE_DIR") {
        std::fs::write(
            format!("{dir}/postgres-inventories.json"),
            serde_json::to_vec(&json!({"backend":"postgres","B0":before,"B1":after,"B2":reopened,"metadata":{"B0":metadata_before,"B1":metadata_after,"B2":metadata_reopened}}))
                .unwrap(),
        )
        .unwrap();
    }
    f.reader.audit.lock().unwrap().zero();
    let backend = format!(
        "postgres{}",
        std::env::var("LEDGERLAB_PG_TEST_MAJOR").unwrap()
    );
    crate::service::comparison::foundation::tests::observe(
        &report,
        &backend,
        before.clone(),
        after,
        reopened,
        json!({"B0":metadata_before,"B1":metadata_after,"B2":metadata_reopened}),
    );
    f.unchanged(&before, "foundation").await;
    f.finish().await;
}
