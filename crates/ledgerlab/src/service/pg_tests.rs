//! Explicit opt-in, real PostgreSQL acceptance adapter. Only frozen initializer
//! rows are seeded; all acceptance decisions run through the shared coordinator.
use super::{accept, hooks::Hooks, tests};
use crate::{
    store::{
        postgres::{self, PostgresStore},
        records::Scope,
    },
    *,
};
use ledgerlab_testkit::{
    failpoints::*, history::Snapshot, stores as tk, FixtureOracle, HarnessError,
};
use serde_json::{json, Value};
use std::{
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{runtime::Runtime, time::Instant};

const ROLE: &str = "ledgerlab_phase1_runtime";
static DATABASE: AtomicU64 = AtomicU64::new(0);
struct Factory;
struct Backend {
    runtime: Runtime,
    config: PostgresConfig,
    store: Option<PostgresStore>,
    oracle: FixtureOracle,
    pending: Option<Hooks>,
    affected: i32,
    uncertain: Option<(i32, tk::Outcome)>,
}
fn err(e: impl std::fmt::Display) -> HarnessError {
    HarnessError(e.to_string())
}
fn scope() -> Scope {
    Scope {
        tenant: "demo".into(),
        environment: "sandbox".into(),
    }
}
fn config(database: String, admin: bool) -> PostgresConfig {
    PostgresConfig {
        host: "localhost".into(),
        port: std::env::var("LEDGERLAB_PG_TEST_PORT")
            .expect("explicit local PG port")
            .parse()
            .unwrap(),
        user: if admin { "postgres" } else { ROLE }.into(),
        password: std::env::var("LEDGERLAB_PG_TEST_PASSWORD")
            .expect("explicit synthetic test password")
            .into_bytes(),
        database,
        trust: PostgresTrust::PemOnly(
            std::fs::read(std::env::var("LEDGERLAB_PG_TEST_CA").expect("explicit test CA path"))
                .unwrap(),
        ),
    }
}
// Inspect the idle target from a different connection. Querying its own state
// would merely observe the probe query as active and could hide an open BEGIN.
async fn external_transaction_probe(
    settings: &PostgresConfig,
    pid: i32,
) -> ledgerlab_testkit::Result<bool> {
    let observer = config(settings.database.clone(), true)
        .connect()
        .await
        .map_err(err)?;
    let active: bool = observer
        .client
        .query_one(
            "SELECT xact_start IS NOT NULL FROM pg_catalog.pg_stat_activity WHERE pid=$1",
            &[&pid],
        )
        .await
        .map_err(err)?
        .try_get(0)
        .map_err(err)?;
    observer.discard().await;
    Ok(active)
}
impl tk::BackendFactory for Factory {
    type Backend = Backend;
    fn seeded(&self, o: &FixtureOracle) -> ledgerlab_testkit::Result<Backend> {
        Backend::new(o, false)
    }
    fn real_without_terms(&self, o: &FixtureOracle) -> ledgerlab_testkit::Result<Backend> {
        Backend::new(o, true)
    }
}
impl Backend {
    fn new(oracle: &FixtureOracle, real: bool) -> ledgerlab_testkit::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(err)?;
        let database = format!(
            "ledgerlab_test_{}_{}",
            std::process::id(),
            DATABASE.fetch_add(1, Ordering::Relaxed)
        );
        let config = config(database.clone(), false);
        let store = runtime.block_on(async {
            let admin = self::config("ledgerlab".into(), true)
                .connect()
                .await
                .map_err(err)?;
            // Generated identifier contains only the fixed prefix and decimal counters.
            admin
                .client
                .batch_execute(&format!("CREATE DATABASE {database}"))
                .await
                .map_err(err)?;
            admin.discard().await;
            let mut owner = self::config(database, true).connect().await.map_err(err)?;
            let mut installation = crate::store::sqlite::tests::installation();
            if real {
                installation.mode = "real".into();
            }
            postgres::migrate::create(&mut owner.client, installation, ROLE)
                .await
                .map_err(err)?;
            let tx = owner.client.transaction().await.map_err(err)?;
            for op in crate::store::sqlite::tests::seed() {
                postgres::write::operation(&tx, &op).await.map_err(err)?;
            }
            tx.commit().await.map_err(err)?;
            owner.discard().await;
            PostgresStore::open(config.clone()).await.map_err(err)
        })?;
        Ok(Self {
            runtime,
            config,
            store: Some(store),
            oracle: oracle.clone(),
            pending: None,
            affected: 0,
            uncertain: None,
        })
    }
    fn pid_gone(&self, pid: i32) -> ledgerlab_testkit::Result<bool> {
        self.runtime.block_on(async {
            let admin = config("ledgerlab".into(), true)
                .connect()
                .await
                .map_err(err)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            let gone = loop {
                let active: bool = admin
                    .client
                    .query_one(
                        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid=$1)",
                        &[&pid],
                    )
                    .await
                    .map_err(err)?
                    .try_get(0)
                    .map_err(err)?;
                if !active {
                    break true;
                }
                if Instant::now() >= deadline {
                    break false;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            };
            admin.discard().await;
            Ok(gone)
        })
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(h) = self.pending.take() {
            self.runtime.block_on(h.drain());
        }
        if let Some(store) = self.store.take() {
            self.runtime.block_on(store.close());
        }
        // Keep a failed case's isolated database for diagnosis. Successful cases
        // remove only the unique database allocated by this test instance.
        if !std::thread::panicking() {
            self.runtime.block_on(async {
                if let Ok(admin) = config("ledgerlab".into(), true).connect().await {
                    let _ = admin
                        .client
                        .batch_execute(&format!(
                            "DROP DATABASE {} WITH (FORCE)",
                            self.config.database
                        ))
                        .await;
                    admin.discard().await;
                }
            });
        }
    }
}
impl tk::AcceptanceBackend for Backend {
    fn evidence(&self) -> tk::BackendEvidence {
        let v = self.store.as_ref().unwrap().version;
        tk::BackendEvidence {kind:if v>=180000 {tk::BackendKind::Postgres18}else{tk::BackendKind::Postgres17},location:format!("local test database {}",self.config.database),engine_version:v.to_string(),version_number:v as u32,durability:"verified TLS, primary, permanent tables, fsync/full_page_writes/synchronous_commit=on; SERIALIZABLE".into()}
    }
    fn observe(&mut self) -> ledgerlab_testkit::Result<Snapshot> {
        let data=self.runtime.block_on(async {
            let mut session=self.config.connect().await.map_err(err)?;
            let tx=session.client.build_transaction().isolation_level(tokio_postgres::IsolationLevel::RepeatableRead).read_only(true).start().await.map_err(err)?;
            let tables=tx.query("SELECT c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND c.relkind='r' ORDER BY c.relname",&[]).await.map_err(err)?;
            let mut data=serde_json::Map::new();
            for row in tables {
                let table:String=row.try_get(0).map_err(err)?;
                assert!(table.bytes().all(|b|b.is_ascii_lowercase()||b==b'_'));
                let ident=format!("ledgerlab.\"{table}\"");
                let columns=tx.query("SELECT a.attname FROM pg_catalog.pg_attribute a WHERE a.attrelid=$1::text::regclass AND a.atttypid='bytea'::regtype AND NOT a.attisdropped",&[&ident]).await.map_err(err)?;
                let bytes:Vec<String>=columns.iter().map(|r|r.get(0)).collect();
                let keys=tx.query("SELECT a.attname FROM pg_catalog.pg_index i CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,ord) JOIN pg_catalog.pg_attribute a ON a.attrelid=i.indrelid AND a.attnum=k.attnum WHERE i.indrelid=$1::text::regclass AND i.indisprimary ORDER BY k.ord",&[&ident]).await.map_err(err)?;
                let keys:Vec<String>=keys.iter().map(|r|r.get(0)).collect();
                let rows=tx.query(&format!("SELECT row_to_json(t)::text FROM {ident} t"),&[]).await.map_err(err)?;
                let rows:Vec<Value>=rows.iter().map(|r|serde_json::from_str(r.get::<_,&str>(0)).map_err(err)).collect::<Result<_,_>>()?;
                data.insert(table,json!({"pk":keys,"byte_columns":bytes,"rows":rows}));
            }
            tx.commit().await.map_err(err)?;
            session.discard().await;
            Ok::<_,HarnessError>(data)
        })?;
        let mut child = std::process::Command::new("python3")
            .args([
                "-B",
                concat!(env!("CARGO_MANIFEST_DIR"), "/src/service/observe_sqlite.py"),
                "--postgres",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(err)?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&data).map_err(err)?)
            .map_err(err)?;
        let result = child.wait_with_output().map_err(err)?;
        if !result.status.success() {
            return Err(err(String::from_utf8_lossy(&result.stderr)));
        }
        tests::snapshot(serde_json::from_slice(&result.stdout).map_err(err)?)
    }
    fn reopen(&mut self) -> ledgerlab_testkit::Result<()> {
        if let Some(h) = self.pending.take() {
            self.runtime.block_on(h.drain());
        }
        self.runtime.block_on(self.store.take().unwrap().close());
        if let Some((pid, _)) = &self.uncertain {
            if !self.pid_gone(*pid)? {
                return Err(err("uncertain PostgreSQL backend still active after drain"));
            }
        }
        self.store = Some(
            self.runtime
                .block_on(PostgresStore::open(self.config.clone()))
                .map_err(err)?,
        );
        Ok(())
    }
    fn accept(
        &mut self,
        c: &tk::Command,
        injection: Option<&Injection>,
    ) -> ledgerlab_testkit::Result<tk::Attempt> {
        let h = Hooks::new(injection.cloned(), None);
        let result = self.runtime.block_on(accept::run(
            self.store.as_ref().unwrap(),
            &tests::Backend::command(c),
            &h,
        ));
        self.affected = self.store.as_ref().unwrap().test_pid();
        let outcome = tests::outcome(result)?;
        if h.unknown().is_some() {
            self.pending = Some(h.clone());
            self.uncertain = Some((self.affected, outcome.clone()));
        }
        Ok(tk::Attempt {
            outcome,
            hit: h.hit(),
        })
    }
    fn resolve_and_retry(&mut self, c: &tk::Command) -> ledgerlab_testkit::Result<tk::Attempt> {
        if self.pending.is_some() {
            self.reopen()?;
        }
        self.accept(c, None)
    }
    fn probe_active_commit(
        &mut self,
        _c: &tk::Command,
    ) -> ledgerlab_testkit::Result<tk::ActiveCommitProbe> {
        let (pid, outcome) = self.uncertain.as_ref().unwrap();
        let (absent,active)=self.runtime.block_on(async {
            let absent=self.store.as_ref().unwrap().lookup_identity(&scope(),"urn:demo:app","generation-1").await.map_err(err)?.is_none();
            let admin=config(self.config.database.clone(),true).connect().await.map_err(err)?;
            let active:bool=admin.client.query_one("SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_stat_activity a WHERE a.pid=$1 AND a.xact_start IS NOT NULL AND EXISTS(SELECT 1 FROM pg_catalog.pg_locks l WHERE l.pid=a.pid AND l.granted AND l.mode='RowExclusiveLock'))",&[pid]).await.map_err(err)?.try_get(0).map_err(err)?;
            admin.discard().await;
            Ok::<_,HarnessError>((absent,active))
        })?;
        Ok(tk::ActiveCommitProbe {
            primary_rows_absent: absent,
            original_transaction_active: active,
            outcome: outcome.clone(),
        })
    }
    fn pool_probe(&mut self) -> ledgerlab_testkit::Result<tk::PoolProbe> {
        if self.pending.is_some() {
            self.reopen()?;
        }
        let original = self.uncertain.take().map(|x| x.0).unwrap_or(self.affected);
        let discarded = self.pid_gone(original)?;
        let (next, active) = self.runtime.block_on(async {
            let session = self.config.connect().await.map_err(err)?;
            let pid: i32 = session
                .client
                .query_one("SELECT pg_backend_pid()", &[])
                .await
                .map_err(err)?
                .try_get(0)
                .map_err(err)?;
            let active = external_transaction_probe(&self.config, pid).await?;
            session.discard().await;
            Ok::<_, HarnessError>((pid, active))
        })?;
        Ok(tk::PoolProbe {
            affected_connection: original.to_string(),
            next_connection: next.to_string(),
            affected_discarded: discarded,
            next_has_open_transaction: active,
        })
    }
    fn cancellation_points(&mut self) -> ledgerlab_testkit::Result<Vec<CancellationPoint>> {
        tests::cancellation_points(&self.oracle)
    }
    fn cancel(
        &mut self,
        c: &tk::Command,
        p: &CancellationPoint,
    ) -> ledgerlab_testkit::Result<tk::Attempt> {
        let h = Hooks::new(p.trigger.clone(), Some(p.clone()));
        let store = self.store.as_ref().unwrap().clone();
        let command = tests::Backend::command(c);
        let task_h = h.clone();
        self.runtime.block_on(async {
            let task = tokio::spawn(async move { accept::run(&store, &command, &task_h).await });
            tokio::time::timeout(Duration::from_secs(5), h.state.reached.notified())
                .await
                .expect("PG cancellation hook reached");
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        });
        self.affected = self.store.as_ref().unwrap().test_pid();
        let outcome = match p.phase {
            CommitPhase::Before => tk::Outcome::Cancelled,
            CommitPhase::Acknowledged => tk::Outcome::ResponseLost,
            CommitPhase::InFlight => tk::Outcome::OutcomeUnknown {
                scope: ["demo".into(), "sandbox".into()],
                source: "urn:demo:app".into(),
                external_id: "generation-1".into(),
            },
        };
        if p.phase == CommitPhase::InFlight {
            self.pending = Some(h.clone());
            self.uncertain = Some((self.affected, outcome.clone()));
        }
        Ok(tk::Attempt {
            outcome,
            hit: h.hit(),
        })
    }
    fn zero_evidence(&mut self) -> ledgerlab_testkit::Result<tk::ZeroEvidence> {
        tests::zero_evidence(self.observe()?)
    }
}
#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_acceptance_83_cases() {
    let oracle = FixtureOracle::workspace().unwrap();
    let cases = ledgerlab_testkit::cases::acceptance_cases(&oracle);
    assert_eq!(cases.len(), 83);
    for case in cases {
        println!("running PostgreSQL {case:?}");
        ledgerlab_testkit::cases::run_case(&Factory, &oracle, &case)
            .unwrap_or_else(|e| panic!("{case:?}: {e}"));
    }
}
#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_cancellation_all_awaits() {
    let oracle = FixtureOracle::workspace().unwrap();
    let reports = ledgerlab_testkit::cases::run_cancellation(&Factory, &oracle).unwrap();
    println!(
        "{} real PostgreSQL cancellation cases passed",
        reports.len()
    );
}

impl tk::RaceBackend for Backend {
    fn concurrent_identical(
        &mut self,
        c: &tk::Command,
        participants: usize,
    ) -> ledgerlab_testkit::Result<tk::RaceEvidence> {
        let stores = (0..participants)
            .map(|_| (self.store.as_ref().unwrap().clone(), None))
            .collect();
        self.runtime.block_on(super::race_tests::race(stores, c))
    }
}
#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_barrier_race() {
    let oracle = FixtureOracle::workspace().unwrap();
    ledgerlab_testkit::cases::run_race(&Factory, &oracle, 4).unwrap();
}

#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_storage_guards_and_failed_transaction() {
    use crate::store::{
        errors::{CommitError, StoreError},
        ports::{AcceptanceStore, AcceptanceTx},
    };
    use tk::AcceptanceBackend;
    let oracle = FixtureOracle::workspace().unwrap();
    let mut backend = Backend::new(&oracle, false).unwrap();
    let before = backend.observe().unwrap();
    backend.runtime.block_on(async {
        let mut target = backend.config.connect().await.unwrap();
        let pid: i32 = target
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .unwrap()
            .get(0);
        assert!(!external_transaction_probe(&backend.config, pid)
            .await
            .unwrap());
        let open = target.client.transaction().await.unwrap();
        assert!(
            external_transaction_probe(&backend.config, pid)
                .await
                .unwrap(),
            "negative control must detect the target's open transaction"
        );
        open.rollback().await.unwrap();
        assert!(!external_transaction_probe(&backend.config, pid)
            .await
            .unwrap());
        target.discard().await;
        let store = backend.store.as_ref().unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        tx.write(&crate::store::sqlite::tests::schedule()[0])
            .await
            .unwrap();
        let e = tx
            .write(&crate::store::sqlite::tests::seed()[0])
            .await
            .unwrap_err();
        assert!(matches!(e,StoreError::Postgres(e) if e.code().is_some_and(|c|c.code()=="23505")));
        assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
        assert!(matches!(
            PostgresStore::open(config(backend.config.database.clone(), true)).await,
            Err(StoreError::InvalidStore(_))
        ));
    });
    before
        .assert_exact(
            &backend.observe().unwrap(),
            "failed PostgreSQL statement cannot commit earlier writes",
        )
        .unwrap();
    let command = tk::Command::fixture(&oracle, "input").unwrap();
    assert_eq!(
        backend.accept(&command, None).unwrap().outcome,
        tk::Outcome::Accepted(oracle.receipt.clone())
    );
    let after = backend.observe().unwrap();
    backend.runtime.block_on(async {
        let mut owner=config(backend.config.database.clone(),true).connect().await.unwrap();
        let tables=owner.client.query("SELECT DISTINCT c.relname FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='ledgerlab' AND t.tgname LIKE '%_immutable' ORDER BY c.relname",&[]).await.unwrap();
        assert_eq!(tables.len(),18);
        for table in tables {
            let name:String=table.get(0);
            assert!(name.bytes().all(|b|b.is_ascii_lowercase()||b==b'_'));
            for sql in [format!("UPDATE ledgerlab.{name} SET content_hash=content_hash"),format!("DELETE FROM ledgerlab.{name}"),format!("TRUNCATE ledgerlab.{name} CASCADE")] {
                let tx=owner.client.transaction().await.unwrap();
                let e=tx.batch_execute(&sql).await.unwrap_err();
                assert_eq!(e.code().map(|c|c.code()),Some("23000"),"{name}: {e}");
                tx.rollback().await.unwrap();
            }
        }
        owner.discard().await;
    });
    after
        .assert_exact(&backend.observe().unwrap(), "PG immutable guards")
        .unwrap();
    backend.reopen().unwrap();
    after
        .assert_exact(&backend.observe().unwrap(), "PG guard reopen")
        .unwrap();
}

#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_alias_write_cancellation() {
    let oracle = FixtureOracle::workspace().unwrap();
    let mut backend = Backend::new(&oracle, false).unwrap();
    tests::check_alias_cancellation(&mut backend, &oracle);
}

#[test]
#[ignore = "requires explicit isolated local PostgreSQL with verified test CA and restricted runtime role"]
fn postgres_driver_commit_drop_recovers_none_or_complete() {
    use crate::store::ports::{AcceptanceStore, AcceptanceTx};
    use std::{
        future::Future,
        task::{Context, Waker},
    };
    use tk::AcceptanceBackend;
    let oracle = FixtureOracle::workspace().unwrap();
    for iteration in 0..12 {
        let mut backend = Backend::new(&oracle, false).unwrap();
        let before = backend.observe().unwrap();
        let committed = backend.runtime.block_on(async {
            let mut session = backend.config.connect().await.unwrap();
            let tx = session
                .client
                .build_transaction()
                .isolation_level(tokio_postgres::IsolationLevel::Serializable)
                .start()
                .await
                .unwrap();
            for op in crate::store::sqlite::tests::schedule() {
                postgres::write::operation(&tx, &op).await.unwrap();
            }
            let mut committing = Box::pin(tx.commit());
            let _ = committing
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()));
            drop(committing);
            if iteration % 2 == 1 {
                tokio::task::yield_now().await;
            }
            // Discard the actual driver/socket after abandoning COMMIT's reply.
            // No absent lookup is interpreted until a replacement takes its lock.
            session.discard().await;
            let deadline = Instant::now() + Duration::from_secs(5);
            for _ in 0..5 {
                let mut replacement = backend
                    .store
                    .as_ref()
                    .unwrap()
                    .begin(deadline)
                    .await
                    .unwrap();
                let result = async {
                    replacement.load_installation().await?;
                    replacement
                        .load_authority(&scope(), "demo-source-grant-v1")
                        .await?;
                    replacement
                        .load_binding(&scope(), "demo-retail-selector")
                        .await?;
                    replacement.load_chain(&scope(), "demo-slice").await?;
                    replacement
                        .load_identity(&scope(), "urn:demo:app", "generation-1")
                        .await
                }
                .await;
                replacement.rollback().await.unwrap();
                match result {
                    Ok(identity) => return identity.is_some(),
                    Err(e) if e.retryable_after_rollback() && Instant::now() < deadline => continue,
                    Err(e) => panic!("replacement lock/resolution failed: {e}"),
                }
            }
            panic!("replacement serialization retry budget exceeded")
        });
        println!(
            "transport cut {iteration}: {}",
            if committed { "complete" } else { "none" }
        );
        let observed = backend.observe().unwrap();
        if committed {
            oracle.assert_accepted(&before, &observed).unwrap();
        } else {
            before
                .assert_exact(&observed, "abandoned real PostgreSQL COMMIT")
                .unwrap();
        }
        let result = backend
            .accept(&tk::Command::fixture(&oracle, "input").unwrap(), None)
            .unwrap();
        let expected = if committed {
            tk::Outcome::Duplicate {
                kind: tk::DuplicateKind::Identity,
                receipt: oracle.receipt.clone(),
            }
        } else {
            tk::Outcome::Accepted(oracle.receipt.clone())
        };
        assert_eq!(result.outcome, expected, "transport cut {iteration}");
        oracle
            .assert_accepted(&before, &backend.observe().unwrap())
            .unwrap();
        backend.reopen().unwrap();
        oracle
            .assert_accepted(&before, &backend.observe().unwrap())
            .unwrap();
    }
}
