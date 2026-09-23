//! Native persistence primitives only. No economic acceptance or physically
//! backed CommitCapability is fabricated by these tests.
use super::*;
use crate::store::{
    outcomes::{OutcomeLock, OutcomeLockClass, OutcomeLockMode},
    ports::{AcceptanceStore, AcceptanceTx},
    postgres::{PostgresConfig, PostgresStore, PostgresTrust},
};
use r3::types::Id;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::time::Instant;
pub(in crate::store::postgres) struct Primitive {
    pub journal: JournalIdentity,
    pub segment: wire::Segment,
    pub writes: Vec<HeadWrite>,
    pub fail_after_segment: bool,
}
pub(super) async fn primitive<C: GenericClient + Sync>(
    c: &C,
    p: &Primitive,
) -> Result<(), StoreError> {
    let prior = head(c, &p.journal).await?;
    let bytes = r3::canonical_bytes(&p.segment, r3::SEGMENT_BYTES).map_err(core)?;
    persist::segment(c, &p.journal, &prior, &p.segment, &bytes).await?;
    if p.fail_after_segment {
        return Err(StoreError::Integrity("test native cut after segment"));
    }
    let jk = journal_key(&p.journal)?;
    for o in &p.segment.objects {
        persist::object(c, &jk, p.segment.ordinal, o).await?;
    }
    for w in &p.writes {
        persist::write_head(c, p.segment.ordinal, w).await?;
    }
    let command = r3::canonical_bytes(&p.segment.command, r3::COMMAND_BYTES).map_err(core)?;
    persist::command(c, &p.journal, &p.segment, &command, None).await
}
fn config(database: &str, admin: bool) -> PostgresConfig {
    PostgresConfig {
        host: "localhost".into(),
        port: std::env::var("LEDGERLAB_PG_TEST_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        user: if admin {
            "postgres"
        } else {
            "ledgerlab_phase1_runtime"
        }
        .into(),
        password: std::env::var("LEDGERLAB_PG_TEST_PASSWORD")
            .unwrap()
            .into_bytes(),
        database: database.into(),
        trust: PostgresTrust::PemOnly(
            std::fs::read(std::env::var("LEDGERLAB_PG_TEST_CA").unwrap()).unwrap(),
        ),
    }
}
struct Fixture {
    name: String,
    owner: super::super::connect::Session,
    store: PostgresStore,
}
impl Fixture {
    async fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "ledgerlab_r3_native_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let admin = config("ledgerlab", true).connect().await.unwrap();
        admin
            .client
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
        admin.discard().await;
        let mut owner = config(&name, true).connect().await.unwrap();
        let mut installation = crate::store::sqlite::tests::installation();
        installation.logical_store_id = "center".into();
        installation.scope.tenant = "demo".into();
        installation.scope.environment = "sandbox".into();
        super::super::migrate::create(&mut owner.client, installation, "ledgerlab_phase1_runtime")
            .await
            .unwrap();
        let store = PostgresStore::open(config(&name, false)).await.unwrap();
        let expected: i32 = std::env::var("LEDGERLAB_PG_TEST_MAJOR")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(store.version / 10000, expected);
        println!(
            "Native R3 storage primitive PostgreSQL {}",
            owner
                .client
                .query_one("SELECT version()", &[])
                .await
                .unwrap()
                .get::<_, String>(0)
        );
        Self { name, owner, store }
    }
    async fn finish(self) {
        self.store.close().await;
        self.owner.discard().await;
        let admin = config("ledgerlab", true).connect().await.unwrap();
        admin
            .client
            .batch_execute(&format!("DROP DATABASE {}", self.name))
            .await
            .unwrap();
        admin.discard().await;
    }
}
fn sample() -> (JournalIdentity, wire::Segment) {
    let s:wire::Segment=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/minimal-segment.json")).unwrap();
    let j = JournalIdentity {
        store: Id::parse("center").unwrap(),
        scope: wire::Scope(Id::parse("demo").unwrap(), Id::parse("sandbox").unwrap()),
        registration: Id::parse("registration").unwrap(),
        host: s.host.clone(),
    };
    (j, s)
}
fn guards(j: &JournalIdentity) -> Vec<Guard> {
    vec![
        Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Admission,
            key: r3::canonical_bytes(&json!([j.scope]), 4096).unwrap(),
            mode: OutcomeLockMode::Write,
        }),
        Guard::R3 {
            class: GuardClass::EnrollmentNamespace,
            host: j.host.clone(),
            key: b"native-schema-test".to_vec(),
        },
        Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Authority,
            key: r3::canonical_bytes(&json!([j.scope, "native"]), 4096).unwrap(),
            mode: OutcomeLockMode::Write,
        }),
    ]
}
async fn begin(f: &Fixture, j: &JournalIdentity) -> super::super::PostgresTx {
    let mut tx = f
        .store
        .begin(Instant::now() + Duration::from_secs(30))
        .await
        .unwrap();
    tx.native_adjudication(Operation::Locks(j.clone(), guards(j)))
        .await
        .unwrap();
    tx
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_r3_exact_storage_lookup_resolve_source_and_reopen() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let key = HeadKey {
        journal: j.clone(),
        kind: HeadKind::Counter,
        full_key: vec![b'k'; r3::MAX_KEY_BYTES],
    };
    let value = r3::canonical_bytes(&json!({"test":"v".repeat(9000)}), r3::SEGMENT_BYTES).unwrap();
    let w = HeadWrite {
        key: key.clone(),
        expected: None,
        revision: Count::new(1).unwrap(),
        value: value.clone(),
    };
    let mut tx = begin(&f, &j).await;
    tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
        journal: j.clone(),
        segment: s.clone(),
        writes: vec![w],
        fail_after_segment: false,
    })))
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = begin(&f, &j).await;
    let Value::Head(h) = tx
        .native_adjudication(Operation::Head(j.clone()))
        .await
        .unwrap()
    else {
        panic!("head")
    };
    assert_eq!(h.ordinal(), s.ordinal);
    assert_eq!(h.segment(), &runtime::hash("segment", &s).unwrap());
    assert_eq!(h.root(), &s.result.root);
    let v = runtime::command_value(&s.command).unwrap();
    let d: wire::Delivery = serde_json::from_value(v["key"].clone()).unwrap();
    let Value::Saved(saved) = tx
        .native_adjudication(Operation::Lookup(j.clone(), d.clone()))
        .await
        .unwrap()
    else {
        panic!("saved")
    };
    let saved = saved.expect("saved outcome");
    assert_eq!(
        saved.command,
        r3::canonical_bytes(&s.command, r3::COMMAND_BYTES).unwrap()
    );
    assert_eq!(saved.result, s.result);
    let fact = s
        .objects
        .iter()
        .find(|o| o.kind == wire::FactKind::Enrollment)
        .unwrap();
    let fk: wire::ProofFullKey = serde_json::from_value(json!(fact.full_key)).unwrap();
    let Value::Source(source) = tx
        .native_adjudication(Operation::Source(
            j.clone(),
            s.ordinal,
            wire::FactKind::Enrollment,
            fk,
        ))
        .await
        .unwrap()
    else {
        panic!("source")
    };
    assert_eq!(source.object(), fact);
    let Value::Resolved(i) = tx
        .native_adjudication(Operation::Resolve(ResolveRequest {
            journal: j.clone(),
            key: d,
            guards: guards(&j),
            objects: vec![source.proof().clone()],
            heads: vec![key.clone()],
        }))
        .await
        .unwrap()
    else {
        panic!("resolved")
    };
    assert_eq!(i.heads[0].value.as_ref().unwrap(), &value);
    assert_eq!(i.retained, vec![fact.clone()]);
    assert!(i.sources.is_empty());
    tx.rollback().await.unwrap();
    f.store.clone().close().await;
    let reopened = PostgresStore::open(config(&f.name, false)).await.unwrap();
    let mut tx = reopened
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.native_adjudication(Operation::Locks(j.clone(), guards(&j)))
        .await
        .unwrap();
    let Value::Head(h2) = tx.native_adjudication(Operation::Head(j)).await.unwrap() else {
        panic!("head")
    };
    assert_eq!(h2.root(), h.root());
    tx.rollback().await.unwrap();
    reopened.close().await;
    f.finish().await;
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_r3_poison_rollback_and_ordered_guard_refusal() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let mut tx = begin(&f, &j).await;
    assert!(tx
        .native_adjudication(Operation::Primitive(Box::new(Primitive {
            journal: j.clone(),
            segment: s,
            writes: vec![],
            fail_after_segment: true
        })))
        .await
        .is_err());
    assert!(tx
        .native_adjudication(Operation::Head(j.clone()))
        .await
        .is_err());
    assert!(matches!(
        tx.commit().await,
        Err(crate::store::errors::CommitError::RolledBack(_))
    ));
    let mut tx = begin(&f, &j).await;
    let Value::Head(h) = tx
        .native_adjudication(Operation::Head(j.clone()))
        .await
        .unwrap()
    else {
        panic!("head")
    };
    assert_eq!(h.ordinal(), Count::ZERO);
    tx.rollback().await.unwrap();
    let mut tx = f
        .store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let mut wrong = guards(&j);
    wrong.swap(1, 2);
    assert!(tx
        .native_adjudication(Operation::Locks(j, wrong))
        .await
        .is_err());
    assert!(tx.commit().await.is_err());
    assert_eq!(
        f.owner
            .client
            .query_one("SELECT count(*) FROM ledgerlab.r3_segments", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        0
    );
    f.finish().await;
}

// Inventory is test observation only; runtime projections never scan history.
async fn inventory(f: &Fixture) -> Vec<Vec<Vec<u8>>> {
    let mut tables = Vec::new();
    for table in [
        "r3_journals",
        "r3_segments",
        "r3_segment_pages",
        "r3_objects",
        "r3_object_pages",
        "r3_heads",
        "r3_head_versions",
        "r3_commands",
        "r3_deliveries",
        "r3_namespaces",
        "r3_held_intentions",
        "r3_scope_locks",
        "outcome_scope_locks",
    ] {
        let rows=f.owner.client.query(&format!("SELECT sha256(convert_to(row_to_json(t)::text,'UTF8')) FROM ledgerlab.{table} t ORDER BY 1"),&[]).await.unwrap();
        tables.push(rows.into_iter().map(|r| r.get(0)).collect());
    }
    tables
}

#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_r3_stale_head_and_wrong_source_leave_exact_inventory() {
    let f = Fixture::new().await;
    let (j, s) = sample();
    let key = HeadKey {
        journal: j.clone(),
        kind: HeadKind::Counter,
        full_key: b"head-cas".to_vec(),
    };
    let write = HeadWrite {
        key: key.clone(),
        expected: None,
        revision: Count::new(1).unwrap(),
        value: br#"{"test":1}"#.to_vec(),
    };
    let mut tx = begin(&f, &j).await;
    tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
        journal: j.clone(),
        segment: s.clone(),
        writes: vec![write.clone()],
        fail_after_segment: false,
    })))
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let before = inventory(&f).await;
    // The second storage-only segment is rolled back after its head CAS fails.
    // This deliberately does not pretend the synthetic segment is economic replay.
    let mut next = s.clone();
    next.ordinal = Count::new(2).unwrap();
    next.previous = runtime::hash("segment", &s).unwrap();
    next.previous_root = s.result.root.clone();
    next.objects.clear();
    let mut tx = begin(&f, &j).await;
    assert!(matches!(
        tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
            journal: j.clone(),
            segment: next,
            writes: vec![write],
            fail_after_segment: false
        })))
        .await,
        Err(StoreError::ExpectedCurrent)
    ));
    assert!(tx.commit().await.is_err());
    assert_eq!(inventory(&f).await, before);
    let fact = s
        .objects
        .iter()
        .find(|o| o.kind == wire::FactKind::Enrollment)
        .unwrap();
    let good: wire::ProofFullKey = serde_json::from_value(json!(fact.full_key)).unwrap();
    for (ordinal, key) in [
        (
            s.ordinal,
            wire::ProofFullKey::V2(Id::parse("wrong-full-key").unwrap()),
        ),
        (Count::new(2).unwrap(), good.clone()),
    ] {
        let mut tx = begin(&f, &j).await;
        assert!(tx
            .native_adjudication(Operation::Source(
                j.clone(),
                ordinal,
                wire::FactKind::Enrollment,
                key
            ))
            .await
            .is_err());
        assert!(tx.commit().await.is_err());
        assert_eq!(inventory(&f).await, before);
    }
    let mut foreign = j.clone();
    foreign.host = Id::parse("wrong-host").unwrap();
    let mut tx = begin(&f, &j).await;
    assert!(tx
        .native_adjudication(Operation::Source(
            foreign,
            s.ordinal,
            wire::FactKind::Enrollment,
            good
        ))
        .await
        .is_err());
    assert!(tx.commit().await.is_err());
    assert_eq!(inventory(&f).await, before);
    let mut tx = begin(&f, &j).await;
    let Value::Head(h) = tx.native_adjudication(Operation::Head(j)).await.unwrap() else {
        panic!("head")
    };
    assert_eq!(h.ordinal(), s.ordinal);
    assert_eq!(h.root(), &s.result.root);
    tx.rollback().await.unwrap();
    f.finish().await;
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18"]
async fn native_r3_cancel_contended_lock_joins_then_observes_backend_exit() {
    let f = Fixture::new().await;
    let (j, _) = sample();
    let first = begin(&f, &j).await;
    let mut second = f
        .store
        .begin(Instant::now() + Duration::from_secs(10))
        .await
        .unwrap();
    let pid = second.pid;
    let waiting = tokio::spawn(async move {
        second
            .native_adjudication(Operation::Locks(j.clone(), guards(&j)))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let blockers: i32 = f
                .owner
                .client
                .query_one("SELECT cardinality(pg_blocking_pids($1))", &[&pid])
                .await
                .unwrap()
                .get(0);
            if blockers > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("actual PostgreSQL lock contention");
    waiting.abort();
    assert!(matches!(waiting.await, Err(e) if e.is_cancelled()));
    first.rollback().await.unwrap();
    f.store.clone().close().await;
    // Joining the driver and observing server cleanup are distinct boundaries.
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let row=f.owner.client.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1), EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1)",&[&pid]).await.unwrap();
            if !row.get::<_,bool>(0) && !row.get::<_,bool>(1) {break;}
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("server backend and locks exit within bounded observation");
    assert!(inventory(&f).await.iter().all(Vec::is_empty));
    f.finish().await;
}
