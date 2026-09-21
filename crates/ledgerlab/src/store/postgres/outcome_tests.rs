//! Opt-in real PostgreSQL tests. Composite plans come exclusively from the
//! coordinator's validating fixture; no test forges an accepted plan.
use super::*;
use crate::{
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        postgres::{self, PostgresStore},
        records::Scope,
    },
    PostgresConfig, PostgresTrust,
};
use serde_json::json;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::time::Instant;

static NEXT: AtomicU64 = AtomicU64::new(0);
const ROLE: &str = "ledgerlab_phase1_runtime";
fn config(database: &str, admin: bool) -> PostgresConfig {
    PostgresConfig {
        host: "localhost".into(),
        port: std::env::var("LEDGERLAB_PG_TEST_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        user: if admin { "postgres" } else { ROLE }.into(),
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
    owner: postgres::connect::Session,
    store: PostgresStore,
}
impl Fixture {
    async fn new() -> Self {
        let name = format!(
            "ledgerlab_p3_{}_{}",
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
        installation.scope = Scope {
            tenant: "synthetic".into(),
            environment: "sandbox".into(),
        };
        postgres::migrate::create(&mut owner.client, installation, ROLE)
            .await
            .unwrap();
        let store = PostgresStore::open(config(&name, false)).await.unwrap();
        let expected: i32 = std::env::var("LEDGERLAB_PG_TEST_MAJOR")
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(store.version / 10000, expected);
        println!(
            "P3 queried PostgreSQL {}",
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
        self.store.clone().close().await;
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
fn lock_key(class: OutcomeLockClass, id: &str) -> OutcomeLock {
    OutcomeLock {
        class,
        key: bytes(&json!([["synthetic", "sandbox"], id])).unwrap(),
        mode: OutcomeLockMode::Write,
    }
}
fn request(locks: Vec<OutcomeLock>) -> OutcomeResolve {
    OutcomeResolve {
        delivery: ScopedDelivery {
            scope: ["synthetic".into(), "sandbox".into()],
            source: "urn:synthetic:store".into(),
            external_id: "primitive".into(),
        },
        target: "target".into(),
        invocation_id: "invocation".into(),
        family_key: None,
        required: vec![],
        locks,
    }
}
fn frozen_records() -> Vec<Vec<u8>> {
    let v: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/candidates/reservation-settlement-v1/vectors.json"
    ))
    .unwrap();
    v[0]["steps"][0]["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| bytes(r).unwrap())
        .collect()
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_primitive_bytes_partition_guards_and_reopen() {
    let mut f = Fixture::new().await;
    let l = lock_key(OutcomeLockClass::Reservation, "invocation");
    let q = request(vec![l.clone()]);
    let records = frozen_records();
    let tx = f
        .owner
        .client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::Serializable)
        .start()
        .await
        .unwrap();
    let mut held = Locked::default();
    lock(&tx, &mut held, &q.locks).await.unwrap();
    let mut steps = Steps::default();
    for b in &records {
        insert_record(&tx, &q, b, &mut steps).await.unwrap();
    }
    let old = ObservedOutcomeHead {
        lock: l.clone(),
        revision: None,
        value: None,
    };
    let w = OutcomeHeadWrite {
        lock: l.clone(),
        revision: "0".into(),
        value: bytes(&json!({"revision":"0","synthetic":"initial"})).unwrap(),
    };
    write_head(&tx, &old, &w, &mut steps).await.unwrap();
    tx.commit().await.unwrap();
    f.store.clone().close().await;
    let store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    let mut t = store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    t.lock_scopes(&q.locks).await.unwrap();
    let OutcomeResolution::Complete(snapshot) = t.resolve_outcome(&q).await.unwrap() else {
        panic!()
    };
    let mut expected = records.clone();
    expected.sort();
    let mut actual = snapshot.records;
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(snapshot.heads[0].revision, Some("0".into()));
    assert_eq!(snapshot.heads[0].value, Some(w.value.clone()));
    let mut wrong_partition = q.clone();
    wrong_partition.target = "another-target".into();
    let OutcomeResolution::Complete(empty) = t.resolve_outcome(&wrong_partition).await.unwrap()
    else {
        panic!()
    };
    assert!(empty.records.is_empty());
    let mut missing = q.clone();
    missing.required = vec![ScopedRecordRef {
        scope: q.delivery.scope.clone(),
        kind: "evidence".into(),
        id: b"\"missing\"".to_vec(),
        content_hash: format!("sha256:{}", "0".repeat(64)),
    }];
    assert!(matches!(
        t.resolve_outcome(&missing).await.unwrap(),
        OutcomeResolution::Missing(_)
    ));
    let mut expanded = q.clone();
    expanded
        .locks
        .push(lock_key(OutcomeLockClass::Target, "target"));
    assert!(matches!(
        t.resolve_outcome(&expanded).await.unwrap(),
        OutcomeResolution::MoreLocks(_)
    ));
    t.rollback().await.unwrap();
    store.close().await;
    // Revision compare-and-swap compares complete values as well as counters.
    let tx = f.owner.client.transaction().await.unwrap();
    let stale = ObservedOutcomeHead {
        lock: l.clone(),
        revision: Some("0".into()),
        value: Some(b"{}".to_vec()),
    };
    let next = OutcomeHeadWrite {
        lock: l,
        revision: "1".into(),
        value: b"{}".to_vec(),
    };
    assert!(matches!(
        write_head(&tx, &stale, &next, &mut Steps::default()).await,
        Err(StoreError::ExpectedCurrent)
    ));
    tx.rollback().await.unwrap();
    // Immutable collisions may reuse exact bytes, never replace them.
    let tx = f.owner.client.transaction().await.unwrap();
    insert_record(&tx, &q, &records[0], &mut Steps::default())
        .await
        .unwrap();
    let mut changed = parsed(&records[0]).unwrap();
    changed["body"]["received_at"] = json!("2026-09-21T13:00:01.000000Z");
    assert!(
        insert_record(&tx, &q, &bytes(&changed).unwrap(), &mut Steps::default())
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_each_record_statement_boundary_rolls_back() {
    let mut f = Fixture::new().await;
    let q = request(vec![]);
    let mut records = frozen_records();
    // Include a literal frozen intention to cover its held-state statement too.
    // This is a byte-storage catalog, not a fabricated composite acceptance.
    let catalog: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/candidates/v2/goldens/correction-replacement.json"
    ))
    .unwrap();
    let intention = catalog["decisions"][1]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "intention")
        .unwrap();
    records.push(bytes(intention).unwrap());
    // Every envelope, membership and held-state statement has both boundaries.
    let count = records.len() * 4 + 2;
    for at in 0..count {
        let tx = f.owner.client.transaction().await.unwrap();
        let mut steps = Steps {
            fail_at: Some(at),
            count: 0,
        };
        let mut failed = false;
        for b in &records {
            if insert_record(&tx, &q, b, &mut steps).await.is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "boundary {at}");
        tx.rollback().await.unwrap();
        let n:i64=f.owner.client.query_one("SELECT (SELECT count(*) FROM ledgerlab.outcome_records)+(SELECT count(*) FROM ledgerlab.outcome_members)+(SELECT count(*) FROM ledgerlab.outcome_held_intentions)",&[]).await.unwrap().get(0);
        assert_eq!(n, 0, "boundary {at}");
    }
    // Deferred reference failure must also roll back the shared namespace.
    let tx = f.owner.client.transaction().await.unwrap();
    for b in &records {
        insert_record(&tx, &q, b, &mut Steps::default())
            .await
            .unwrap();
    }
    tx.execute("INSERT INTO ledgerlab.outcome_deliveries(tenant,environment,source,external_id,canonical_source,canonical_external_id,command,ingress,ingress_hash,settlement_id) VALUES($1,$2,$3,$4,$3,$4,$5,$5,$6,$7)", &[&q.delivery.scope[0],&q.delivery.scope[1],&q.delivery.source,&q.delivery.external_id,&b"{}".as_slice(),&format!("sha256:{}","0".repeat(64)),&b"\"missing\"".as_slice()]).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(error.code().unwrap().code(), "23503");
    let n:i64=f.owner.client.query_one("SELECT (SELECT count(*) FROM ledgerlab.outcome_records)+(SELECT count(*) FROM ledgerlab.outcome_members)+(SELECT count(*) FROM ledgerlab.outcome_held_intentions)+(SELECT count(*) FROM ledgerlab.acceptance_delivery_namespace)",&[]).await.unwrap().get(0);
    assert_eq!(n, 0);
    println!(
        "P3 primitive record before/after boundaries: {count}; deferred companion failure rollback"
    );
    f.finish().await;
}

async fn wait_blocked(c: &tokio_postgres::Client, pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let n: i32 = c
            .query_one("SELECT cardinality(pg_blocking_pids($1))", &[&pid])
            .await
            .unwrap()
            .get(0);
        if n > 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "must observe actual lock contention"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_ordered_real_locks_cancel_and_serializable_restart() {
    let f = Fixture::new().await;
    for class in [
        OutcomeLockClass::Admission,
        OutcomeLockClass::Authority,
        OutcomeLockClass::Binding,
        OutcomeLockClass::Reservation,
        OutcomeLockClass::Target,
        OutcomeLockClass::Claim,
        OutcomeLockClass::BindingAggregate,
        OutcomeLockClass::InvocationConsumption,
        OutcomeLockClass::BaseReversal,
    ] {
        let l = lock_key(class, "shared-across-chains");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut first = f.store.begin_outcome(deadline).await.unwrap();
        first.lock_scopes(std::slice::from_ref(&l)).await.unwrap();
        let mut second = f.store.begin_outcome(deadline).await.unwrap();
        let pid = second.pid;
        let waiter = tokio::spawn(async move {
            let result = second.lock_scopes(&[l]).await;
            second.rollback().await.unwrap();
            result
        });
        wait_blocked(&f.owner.client, pid).await;
        first.commit().await.unwrap();
        let error = waiter
            .await
            .unwrap()
            .expect_err("stale snapshot on newly committed scope row");
        assert!(error.retryable_after_rollback());
    }
    let l = lock_key(OutcomeLockClass::Reservation, "cancel");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut first = f.store.begin_outcome(deadline).await.unwrap();
    first.lock_scopes(std::slice::from_ref(&l)).await.unwrap();
    let mut second = f.store.begin_outcome(deadline).await.unwrap();
    let pid = second.pid;
    let waiter = tokio::spawn(async move { second.lock_scopes(&[l]).await });
    wait_blocked(&f.owner.client, pid).await;
    waiter.abort();
    let _ = waiter.await;
    first.rollback().await.unwrap();
    f.store.clone().close().await;
    let exists: bool = f
        .owner
        .client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1)",
            &[&pid],
        )
        .await
        .unwrap()
        .get(0);
    assert!(!exists);
    let store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    let mut t = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let a = lock_key(OutcomeLockClass::Claim, "a");
    let b = lock_key(OutcomeLockClass::Reservation, "b");
    assert!(t.lock_scopes(&[a, b]).await.is_err());
    assert!(t.commit().await.is_err());
    store.close().await;
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_shared_delivery_namespace_and_original_alias_receipts() {
    let mut f = Fixture::new().await;
    let q = request(vec![]);
    let records = frozen_records();
    let receipt = records
        .iter()
        .find(|b| envelope(b).unwrap().kind == "reservation-receipt")
        .unwrap()
        .clone();
    let mut delivery = StoredCompositeDelivery {
        key: q.delivery.clone(),
        canonical_key: q.delivery.clone(),
        command: b"{}".to_vec(),
        ingress: b"{}".to_vec(),
        ingress_hash: format!("sha256:{}", "0".repeat(64)),
        economic_receipt: None,
        settlement_receipt: receipt,
    };
    let tx = f.owner.client.transaction().await.unwrap();
    for b in &records {
        insert_record(&tx, &q, b, &mut Steps::default())
            .await
            .unwrap();
    }
    insert_delivery(&tx, &delivery, &mut Steps::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // Exact original bytes are returned; lookup creates no accepted rows.
    let mut t = f
        .store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        t.lookup_outcome_delivery(&delivery.key).await.unwrap(),
        Some(delivery.clone())
    );
    t.rollback().await.unwrap();
    let scope = Scope {
        tenant: delivery.key.scope[0].clone(),
        environment: delivery.key.scope[1].clone(),
    };
    assert!(matches!(
        postgres::read::identity(
            &f.owner.client,
            &scope,
            &delivery.key.source,
            &delivery.key.external_id
        )
        .await,
        Err(StoreError::DeliveryConflict)
    ));
    // An alias retains the original settlement receipt and canonical key.
    delivery.key.external_id = "alias".into();
    let tx = f.owner.client.transaction().await.unwrap();
    insert_delivery(&tx, &delivery, &mut Steps::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        lookup(&f.owner.client, &delivery.key).await.unwrap(),
        Some(delivery.clone())
    );
    // Old v1 insertion cannot claim an already occupied outcome/control key.
    let tx = f.owner.client.transaction().await.unwrap();
    let error=tx.execute("INSERT INTO ledgerlab.delivery_keys(tenant,environment,source,external_id,ingress_hash,canonical_event_id,kind,observed_us,canonical_bytes,content_hash,schema_version) VALUES($1,$2,$3,$4,$5,$6,'alias',0,$7,$5,1)", &[&delivery.key.scope[0],&delivery.key.scope[1],&delivery.key.source,&delivery.key.external_id,&delivery.ingress_hash,&format!("ev_{}","0".repeat(64)),&b"{}".as_slice()]).await.unwrap_err();
    assert_eq!(error.code().unwrap().code(), "23505");
    tx.rollback().await.unwrap();
    // Opposite direction uses the same arbiter; deferred v1 refs never commit.
    delivery.key.external_id = "v1-first".into();
    let tx = f.owner.client.transaction().await.unwrap();
    tx.execute("INSERT INTO ledgerlab.delivery_keys(tenant,environment,source,external_id,ingress_hash,canonical_event_id,kind,observed_us,canonical_bytes,content_hash,schema_version) VALUES($1,$2,$3,$4,$5,$6,'alias',0,$7,$5,1)", &[&delivery.key.scope[0],&delivery.key.scope[1],&delivery.key.source,&delivery.key.external_id,&delivery.ingress_hash,&format!("ev_{}","0".repeat(64)),&b"{}".as_slice()]).await.unwrap();
    let error = insert_delivery(&tx, &delivery, &mut Steps::default())
        .await
        .unwrap_err();
    assert!(
        matches!(error,StoreError::Postgres(ref e) if e.code().is_some_and(|c|c.code()=="23505"))
    );
    tx.rollback().await.unwrap();
    // A durably occupied legacy identity is a conflict, never a missing
    // composite receipt or a retryable expected-current mismatch.
    let tx = f.owner.client.transaction().await.unwrap();
    for op in crate::store::sqlite::tests::seed()
        .into_iter()
        .chain(crate::store::sqlite::tests::schedule())
    {
        postgres::write::operation(&tx, &op).await.unwrap();
    }
    tx.commit().await.unwrap();
    let legacy = ScopedDelivery {
        scope: ["demo".into(), "sandbox".into()],
        source: "urn:demo:app".into(),
        external_id: "generation-1".into(),
    };
    assert!(matches!(
        lookup(&f.owner.client, &legacy).await,
        Err(StoreError::DeliveryConflict)
    ));
    // Retained records and namespace cannot be overwritten/deleted/truncated.
    let runtime = config(&f.name, false).connect().await.unwrap();
    for table in [
        "outcome_records",
        "outcome_members",
        "outcome_anchors",
        "outcome_deliveries",
        "acceptance_delivery_namespace",
    ] {
        for mutation in [
            format!("DELETE FROM ledgerlab.{table}"),
            format!("TRUNCATE ledgerlab.{table} CASCADE"),
        ] {
            assert!(runtime.client.batch_execute(&mutation).await.is_err());
        }
    }
    runtime.discard().await;
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_shared_read_locks_and_unlocked_discovery() {
    let f = Fixture::new().await;
    let mut l = lock_key(OutcomeLockClass::Authority, "readers");
    let deadline = Instant::now() + Duration::from_secs(5);
    // Precreate an operational row; it reserves no delivery, claim or economics.
    let mut pre = f.store.begin_outcome(deadline).await.unwrap();
    pre.lock_scopes(&[l.clone()]).await.unwrap();
    pre.commit().await.unwrap();
    l.mode = OutcomeLockMode::Read;
    let mut a = f.store.begin_outcome(deadline).await.unwrap();
    a.lock_scopes(&[l.clone()]).await.unwrap();
    let mut b = f.store.begin_outcome(deadline).await.unwrap();
    b.lock_scopes(&[l.clone()]).await.unwrap();
    let q = request(vec![l.clone()]);
    let OutcomeResolution::Complete(s) = b.resolve_outcome(&q).await.unwrap() else {
        panic!()
    };
    assert_eq!(s.heads[0].revision, None);
    assert_eq!(s.heads[0].value, None);
    let mut stronger = q.clone();
    stronger.locks[0].mode = OutcomeLockMode::Write;
    assert!(matches!(
        b.resolve_outcome(&stronger).await.unwrap(),
        OutcomeResolution::MoreLocks(_)
    ));
    assert!(
        b.lock_scopes(&stronger.locks).await.is_err(),
        "must restart rather than upgrade out of order"
    );
    b.rollback().await.unwrap();
    let mut writer = f.store.begin_outcome(deadline).await.unwrap();
    let pid = writer.pid;
    let w = stronger.locks;
    let task = tokio::spawn(async move {
        writer.lock_scopes(&w).await.unwrap();
        writer.rollback().await.unwrap();
    });
    wait_blocked(&f.owner.client, pid).await;
    a.rollback().await.unwrap();
    task.await.unwrap();
    let n:i64=f.owner.client.query_one("SELECT (SELECT count(*) FROM ledgerlab.outcome_heads)+(SELECT count(*) FROM ledgerlab.outcome_records)+(SELECT count(*) FROM ledgerlab.outcome_deliveries)",&[]).await.unwrap().get(0);
    assert_eq!(n, 0);
    f.finish().await;
}

use crate::service::accept::outcome::{self as coordinator, fixture as validated};
// Reuse the exact opaque relay without widening the shared service test API.
#[allow(clippy::duplicate_mod)]
#[path = "../../service/pg_transport_tests.rs"]
mod relay;

async fn provision(f: &mut Fixture, v: &validated::Fixture) {
    let q = v.plans[0].resolution();
    let tx = f.owner.client.transaction().await.unwrap();
    let mut held = Locked::default();
    lock(&tx, &mut held, &q.locks).await.unwrap();
    for raw in &v.provisioned_records {
        insert_record(&tx, q, raw, &mut Steps::default())
            .await
            .unwrap();
    }
    for h in &v.provisioned_heads {
        if let (Some(revision), Some(value)) = (&h.revision, &h.value) {
            let absent = ObservedOutcomeHead {
                lock: h.lock.clone(),
                revision: None,
                value: None,
            };
            let w = OutcomeHeadWrite {
                lock: h.lock.clone(),
                revision: revision.clone(),
                value: value.clone(),
            };
            write_head(&tx, &absent, &w, &mut Steps::default())
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
}
async fn ready(store: &PostgresStore, plan: &ValidatedOutcomePlan) -> postgres::tx::PostgresTx {
    let mut t = store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    t.lock_scopes(&plan.resolution().locks).await.unwrap();
    assert!(matches!(
        t.resolve_outcome(plan.resolution()).await.unwrap(),
        OutcomeResolution::Complete(_)
    ));
    t
}
async fn physical(f: &Fixture) -> Vec<(String, Vec<String>)> {
    let tables = f
        .owner
        .client
        .query(
            "SELECT tablename FROM pg_tables WHERE schemaname='ledgerlab' ORDER BY tablename",
            &[],
        )
        .await
        .unwrap();
    let mut result = vec![];
    for r in tables {
        let name: String = r.get(0);
        let rows = f.owner.client.query(&format!("SELECT row_to_json(t)::text FROM ledgerlab.\"{name}\" t ORDER BY row_to_json(t)::text"), &[]).await.unwrap();
        result.push((name, rows.iter().map(|r| r.get(0)).collect()));
    }
    result
}
async fn assert_snapshot(store: &PostgresStore, q: &OutcomeResolve, expected: &OutcomeSnapshot) {
    let mut t = store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    t.lock_scopes(&q.locks).await.unwrap();
    let OutcomeResolution::Complete(mut actual) = t.resolve_outcome(q).await.unwrap() else {
        panic!()
    };
    let mut records = expected.records.clone();
    records.sort();
    actual.records.sort();
    assert_eq!(actual.records, records);
    let mut anchors = expected.anchors.clone();
    anchors.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    assert_eq!(actual.anchors, anchors);
    assert_eq!(actual.heads, expected.heads);
    t.rollback().await.unwrap();
}
// Explicitly synthetic test verifier, matched to the coordinator-owned fixture.
// It does not exist in a production build or certify any external authority.
struct SyntheticAuthority(Vec<coordinator::AuthorityProof>);
impl coordinator::OutcomeAuthority for SyntheticAuthority {
    fn verify(
        &self,
        c: &coordinator::OutcomeCommand,
        _: &OutcomeSnapshot,
        write: bool,
    ) -> Result<coordinator::AuthorityProof, crate::ServiceError> {
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

struct ReadOnlyAuthority(SyntheticAuthority);
impl coordinator::OutcomeAuthority for ReadOnlyAuthority {
    fn verify(
        &self,
        c: &coordinator::OutcomeCommand,
        snapshot: &OutcomeSnapshot,
        write: bool,
    ) -> Result<coordinator::AuthorityProof, crate::ServiceError> {
        if write {
            return Err(crate::ServiceError::Unavailable);
        }
        self.0.verify(c, snapshot, false)
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_host_write_denial_preserves_every_physical_cell() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let allowed = SyntheticAuthority(v.proofs.clone());
    let denied = ReadOnlyAuthority(SyntheticAuthority(v.proofs.clone()));
    for (i, command) in v.commands.iter().enumerate() {
        let before = physical(&f).await;
        assert_eq!(
            coordinator::run(&f.store, command, &denied).await,
            Err(crate::ServiceError::Unavailable),
            "fresh stage {i} requires host write authorization"
        );
        assert_eq!(physical(&f).await, before, "denied stage {i}");
        f.store.clone().close().await;
        f.owner.discard().await;
        f.owner = config(&f.name, true).connect().await.unwrap();
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        assert_eq!(physical(&f).await, before, "reopened denied stage {i}");
        assert!(matches!(
            coordinator::run(&f.store, command, &allowed).await.unwrap(),
            coordinator::OutcomeResult::Accepted(_)
        ));
        let accepted = physical(&f).await;
        assert_eq!(
            coordinator::run(&f.store, command, &denied).await.unwrap(),
            coordinator::OutcomeResult::Duplicate(v.plans[i].delivery().clone())
        );
        assert_eq!(physical(&f).await, accepted, "read-only retry stage {i}");
    }
    let before = physical(&f).await;
    let mut alias = v.commands[1].clone();
    alias.principal.can_submit = false;
    if let coordinator::OutcomeOperation::Economic { ingress, .. } = &mut alias.operation {
        let mut event: Value = serde_json::from_slice(ingress).unwrap();
        event["data"]["external_id"] = json!("host-write-denied-ordinary-alias");
        *ingress = bytes(&event).unwrap();
    }
    let coordinator::OutcomeResult::Duplicate(delivery) =
        coordinator::run(&f.store, &alias, &denied).await.unwrap()
    else {
        panic!("read-only ordinary alias must preserve original receipts")
    };
    assert_eq!(delivery.canonical_key, v.plans[1].delivery().key);
    assert_eq!(
        delivery.economic_receipt,
        v.plans[1].delivery().economic_receipt
    );
    assert_eq!(
        delivery.settlement_receipt,
        v.plans[1].delivery().settlement_receipt
    );
    let after = physical(&f).await;
    for ((name, old), (next_name, new)) in before.iter().zip(&after) {
        assert_eq!(name, next_name);
        if matches!(
            name.as_str(),
            "outcome_deliveries" | "acceptance_delivery_namespace"
        ) {
            assert_eq!(new.len(), old.len() + 1);
        } else {
            assert_eq!(old, new, "read-only alias changed {name}");
        }
    }
    f.store.clone().close().await;
    f.owner.discard().await;
    f.owner = config(&f.name, true).connect().await.unwrap();
    f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    assert_eq!(physical(&f).await, after);
    assert_eq!(
        coordinator::run(&f.store, &alias, &denied).await.unwrap(),
        coordinator::OutcomeResult::Duplicate(delivery)
    );
    assert_eq!(physical(&f).await, after);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn outcome_durable_independent_oracle() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let auth = SyntheticAuthority(v.proofs.clone());
    let mut prefixes = Vec::new();
    for i in 0..v.commands.len() {
        assert!(matches!(
            coordinator::run(&f.store, &v.commands[i], &auth)
                .await
                .unwrap(),
            coordinator::OutcomeResult::Accepted(_)
        ));
        let before = physical(&f).await;
        f.store.clone().close().await;
        f.owner.discard().await;
        f.owner = config(&f.name, true).connect().await.unwrap();
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        let after = physical(&f).await;
        assert_eq!(before, after);
        let mut tx = f
            .store
            .begin_outcome(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let q = v.plans[i].resolution();
        tx.lock_scopes(&q.locks).await.unwrap();
        let OutcomeResolution::Complete(mut snapshot) = tx.resolve_outcome(q).await.unwrap() else {
            panic!("complete durable prefix")
        };
        let mut records: Vec<Vec<u8>> = f
            .owner
            .client
            .query("SELECT envelope FROM ledgerlab.outcome_records", &[])
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.get(0))
            .collect();
        records.sort();
        snapshot.records.sort();
        assert_eq!(
            records, snapshot.records,
            "observer must include every retained envelope"
        );
        let mut deliveries = Vec::new();
        for prior in &v.plans[..=i] {
            deliveries.push(
                tx.lookup_outcome_delivery(&prior.delivery().key)
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        tx.rollback().await.unwrap();
        prefixes.push(crate::store::outcome_evidence::observe(
            snapshot, deliveries, after,
        ));
    }
    let backend = format!("postgres{}", f.store.version / 10000);
    f.finish().await;
    crate::store::outcome_evidence::check(&backend, prefixes);
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_validated_lifecycle_every_write_boundary_and_reopen() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let mut expected = OutcomeSnapshot {
        anchors: vec![],
        records: v.provisioned_records.clone(),
        heads: v.provisioned_heads.clone(),
    };
    let mut total = 0;
    for (index, plan) in v.plans.iter().enumerate() {
        // Count the actual statements of the genuine validated plan in a rollback.
        let tx = f.owner.client.transaction().await.unwrap();
        let mut held = Locked::default();
        lock(&tx, &mut held, &plan.resolution().locks)
            .await
            .unwrap();
        resolve(&tx, &mut held, plan.resolution()).await.unwrap();
        let mut steps = Steps::default();
        append(&tx, &held, plan, &mut steps).await.unwrap();
        tx.rollback().await.unwrap();
        let before = physical(&f).await;
        for at in 0..steps.count {
            let mut t = ready(&f.store, plan).await;
            t.fail_outcome_at(at).await.unwrap();
            let error = t.append_outcome(plan).await.unwrap_err();
            assert!(
                matches!(
                    error,
                    StoreError::Integrity("injected outcome write boundary")
                ),
                "step {index} boundary {at}: {error:?}"
            );
            assert!(matches!(
                t.commit().await,
                Err(crate::store::errors::CommitError::RolledBack(_))
            ));
            assert_eq!(physical(&f).await, before, "step {index} boundary {at}");
        }
        total += steps.count;
        let mut t = ready(&f.store, plan).await;
        t.append_outcome(plan).await.unwrap();
        t.commit().await.unwrap();
        validated::apply(&mut expected, plan);
        f.store.clone().close().await;
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        assert_snapshot(&f.store, plan.resolution(), &expected).await;
        let before = physical(&f).await;
        let auth = SyntheticAuthority(v.proofs.clone());
        assert_eq!(
            coordinator::run(&f.store, &v.commands[index], &auth)
                .await
                .unwrap(),
            coordinator::OutcomeResult::Duplicate(plan.delivery().clone())
        );
        assert_eq!(physical(&f).await, before, "retry must not write");
        println!("P3 validated lifecycle step {index}: {} statement boundaries, exact bytes/heads/anchors and retry after reopen", steps.count);
    }
    println!("P3 validated lifecycle total before/after statement boundaries: {total}");
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_coordinator_same_identity_races() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let authority = SyntheticAuthority(v.proofs.clone());
    let mut expected = OutcomeSnapshot {
        anchors: vec![],
        records: v.provisioned_records.clone(),
        heads: v.provisioned_heads.clone(),
    };
    for (index, command) in v.commands.iter().enumerate() {
        let started = Instant::now();
        let (a, b) = tokio::join!(
            coordinator::run(&f.store, command, &authority),
            coordinator::run(&f.store, command, &authority)
        );
        println!(
            "P3 concurrent lifecycle step {index}: {}ms",
            started.elapsed().as_millis()
        );
        assert!(
            a.is_ok() && b.is_ok(),
            "lifecycle race step {index}: first={:?}, second={:?}",
            a.as_ref().err(),
            b.as_ref().err()
        );
        let mut accepted = 0;
        let mut duplicate = 0;
        for result in [a.unwrap(), b.unwrap()] {
            match result {
                coordinator::OutcomeResult::Accepted(d) => {
                    accepted += 1;
                    assert_eq!(&d, v.plans[index].delivery());
                }
                coordinator::OutcomeResult::Duplicate(d) => {
                    duplicate += 1;
                    assert_eq!(&d, v.plans[index].delivery());
                }
                other => panic!("unexpected race result {other:?}"),
            }
        }
        assert_eq!((accepted, duplicate), (1, 1));
        validated::apply(&mut expected, &v.plans[index]);
        assert_snapshot(&f.store, v.plans[index].resolution(), &expected).await;
    }
    f.finish().await;
    // Distinct correction identities compete for the same current claim; only
    // one may create its inverse/replacement pair. The loser must re-resolve.
    let mut f = Fixture::new().await;
    provision(&mut f, &v).await;
    for command in &v.commands[..2] {
        coordinator::run(&f.store, command, &authority)
            .await
            .unwrap();
    }
    let mut other = v.commands[2].clone();
    if let coordinator::OutcomeOperation::Economic { ingress, .. } = &mut other.operation {
        let mut event: Value = serde_json::from_slice(ingress).unwrap();
        event["data"]["external_id"] = json!("competing-correction");
        *ingress = bytes(&event).unwrap();
    }
    let (a, b) = tokio::join!(
        coordinator::run(&f.store, &v.commands[2], &authority),
        coordinator::run(&f.store, &other, &authority)
    );
    let mut accepted = 0;
    let mut stale = 0;
    for result in [a, b] {
        match result {
            Ok(coordinator::OutcomeResult::Accepted(d)) => {
                accepted += 1;
                let state: Value = serde_json::from_slice(&d.settlement_receipt).unwrap();
                let prior: Value =
                    serde_json::from_slice(&v.plans[1].delivery().settlement_receipt).unwrap();
                assert_eq!(state["body"]["result"], prior["body"]["result"]);
            }
            Err(crate::ServiceError::Rejection(reason)) if reason == "STALE_CORRECTION" => {
                stale += 1;
            }
            result => panic!("unexpected distinct correction race result {result:?}"),
        }
    }
    assert_eq!((accepted, stale), (1, 1));
    let count: i64 = f
        .owner
        .client
        .query_one("SELECT count(*) FROM ledgerlab.outcome_deliveries", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 3);
    println!(
        "P3 distinct correction identities: one successor, one stale rejection, reservation exact"
    );
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_real_commit_cuts_are_unknown_and_retry_recovers() {
    use crate::store::errors::CommitError;
    for (stage, durable) in (0..4).flat_map(|stage| [false, true].map(|durable| (stage, durable))) {
        let mut f = Fixture::new().await;
        let v = validated::lifecycle();
        provision(&mut f, &v).await;
        for prior in &v.plans[..stage] {
            let mut t = ready(&f.store, prior).await;
            t.append_outcome(prior).await.unwrap();
            t.commit().await.unwrap();
        }
        let proxy = relay::Relay::start(config(&f.name, false).port).await;
        let mut settings = config(&f.name, false);
        settings.port = proxy.port;
        let store = PostgresStore::open(settings).await.unwrap();
        let plan = &v.plans[stage];
        let before = physical(&f).await;
        let mut t = ready(&store, plan).await;
        let pid = t.pid;
        t.append_outcome(plan).await.unwrap();
        proxy
            .mode
            .store(if durable { 2 } else { 1 }, Ordering::SeqCst);
        let committing = tokio::spawn(t.commit());
        tokio::time::timeout(Duration::from_secs(2), proxy.intercepted.notified())
            .await
            .unwrap();
        if durable {
            let limit = Instant::now() + Duration::from_secs(2);
            loop {
                let count: i64 = f
                    .owner
                    .client
                    .query_one("SELECT count(*) FROM ledgerlab.outcome_deliveries", &[])
                    .await
                    .unwrap()
                    .get(0);
                if count == stage as i64 + 1 {
                    break;
                }
                assert!(Instant::now() < limit);
                tokio::task::yield_now().await;
            }
        }
        proxy.cut().await;
        assert!(matches!(
            committing.await.unwrap(),
            Err(CommitError::OutcomeUnknown)
        ));
        store.close().await;
        let remaining: i64 = f
            .owner
            .client
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE pid=$1",
                &[&pid],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(remaining, 0);
        if !durable {
            assert_eq!(physical(&f).await, before);
        }
        let result = coordinator::run(
            &f.store,
            &v.commands[stage],
            &SyntheticAuthority(v.proofs.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            if durable {
                coordinator::OutcomeResult::Duplicate(plan.delivery().clone())
            } else {
                coordinator::OutcomeResult::Accepted(plan.delivery().clone())
            }
        );
        let mut expected = OutcomeSnapshot {
            anchors: vec![],
            records: v.provisioned_records.clone(),
            heads: v.provisioned_heads.clone(),
        };
        for accepted in &v.plans[..=stage] {
            validated::apply(&mut expected, accepted);
        }
        assert_snapshot(&f.store, plan.resolution(), &expected).await;
        println!("P3 actual COMMIT cut stage={stage} durable={durable}: OutcomeUnknown, discarded backend, exact complete retry state");
        f.finish().await;
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_alias_conflict_stale_plan_and_closed_noop() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let auth = SyntheticAuthority(v.proofs.clone());
    for plan in &v.plans {
        let mut t = ready(&f.store, plan).await;
        t.append_outcome(plan).await.unwrap();
        t.commit().await.unwrap();
    }
    let before = physical(&f).await;
    let mut stale = ready(&f.store, &v.plans[1]).await;
    assert!(matches!(
        stale.append_outcome(&v.plans[1]).await,
        Err(StoreError::ExpectedCurrent)
    ));
    stale.rollback().await.unwrap();
    assert_eq!(physical(&f).await, before);
    let mut conflict = v.commands[3].clone();
    if let coordinator::OutcomeOperation::Close {
        expected_revision, ..
    } = &mut conflict.operation
    {
        *expected_revision = "2".into();
    }
    assert_eq!(
        coordinator::run(&f.store, &conflict, &auth).await.unwrap(),
        coordinator::OutcomeResult::IdentityConflict
    );
    assert_eq!(physical(&f).await, before);
    let mut renamed_correction = v.commands[2].clone();
    if let coordinator::OutcomeOperation::Economic { ingress, .. } =
        &mut renamed_correction.operation
    {
        let mut event: Value = serde_json::from_slice(ingress).unwrap();
        event["data"]["external_id"] = json!("renamed-correction");
        *ingress = bytes(&event).unwrap();
    }
    assert!(
        matches!(coordinator::run(&f.store, &renamed_correction, &auth).await, Err(crate::ServiceError::Rejection(reason)) if reason == "STALE_CORRECTION")
    );
    assert_eq!(physical(&f).await, before);
    // Permanent ordinary-claim alias keeps the original receipts after correction/closure.
    let mut alias = v.commands[1].clone();
    alias.principal.can_submit = false;
    if let coordinator::OutcomeOperation::Economic { ingress, .. } = &mut alias.operation {
        let mut event: Value = serde_json::from_slice(ingress).unwrap();
        event["data"]["external_id"] = json!("ordinary-alias-after-close");
        *ingress = bytes(&event).unwrap();
    }
    let coordinator::OutcomeResult::Duplicate(delivery) =
        coordinator::run(&f.store, &alias, &auth).await.unwrap()
    else {
        panic!("alias must preserve original acceptance")
    };
    assert_eq!(delivery.canonical_key, v.plans[1].delivery().key);
    assert_eq!(
        delivery.economic_receipt,
        v.plans[1].delivery().economic_receipt
    );
    assert_eq!(
        delivery.settlement_receipt,
        v.plans[1].delivery().settlement_receipt
    );
    let after = physical(&f).await;
    for ((name, old), (next_name, new)) in before.iter().zip(&after) {
        assert_eq!(name, next_name);
        if matches!(
            name.as_str(),
            "outcome_deliveries" | "acceptance_delivery_namespace"
        ) {
            assert_eq!(new.len(), old.len() + 1);
        } else {
            assert_eq!(old, new, "alias changed {name}");
        }
    }
    assert_eq!(
        coordinator::run(&f.store, &alias, &auth).await.unwrap(),
        coordinator::OutcomeResult::Duplicate(delivery)
    );
    assert_eq!(physical(&f).await, after);
    let mut noop = conflict;
    if let coordinator::OutcomeOperation::Close { external_id, .. } = &mut noop.operation {
        *external_id = "already-closed-noop".into();
    }
    let coordinator::OutcomeResult::Accepted(delivery) =
        coordinator::run(&f.store, &noop, &auth).await.unwrap()
    else {
        panic!()
    };
    assert!(delivery.economic_receipt.is_none());
    let after_noop = physical(&f).await;
    for name in ["outcome_anchors", "outcome_held_intentions"] {
        assert_eq!(
            after.iter().find(|(n, _)| n == name),
            after_noop.iter().find(|(n, _)| n == name)
        );
    }
    let control_heads = |rows: &Vec<(String, Vec<String>)>| {
        rows.iter()
            .find(|(name, _)| name == "outcome_heads")
            .unwrap()
            .1
            .iter()
            .filter(|row| {
                serde_json::from_str::<Value>(row).unwrap()["class"]
                    != json!(OutcomeLockClass::Target as i16)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(control_heads(&after), control_heads(&after_noop));
    let transitions: i64 = f
        .owner
        .client
        .query_one(
            "SELECT count(*) FROM ledgerlab.outcome_records WHERE kind='reservation-transition'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(transitions, 2, "ordinary consumption and first close only");
    f.store.clone().close().await;
    f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    assert_eq!(
        coordinator::run(&f.store, &noop, &auth).await.unwrap(),
        coordinator::OutcomeResult::Duplicate(delivery)
    );
    assert_eq!(physical(&f).await, after_noop);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_zero_negative_and_close_race() {
    for code in ["none", "rebate"] {
        let mut f = Fixture::new().await;
        let v = validated::lifecycle();
        provision(&mut f, &v).await;
        let auth = SyntheticAuthority(v.proofs.clone());
        assert!(matches!(
            coordinator::run(&f.store, &v.commands[0], &auth)
                .await
                .unwrap(),
            coordinator::OutcomeResult::Accepted(_)
        ));
        let mut command = v.commands[1].clone();
        if let coordinator::OutcomeOperation::Economic { ingress, .. } = &mut command.operation {
            let mut event: Value = serde_json::from_slice(ingress).unwrap();
            event["data"]["code"] = json!(code);
            *ingress = bytes(&event).unwrap();
        }
        let coordinator::OutcomeResult::Accepted(delivery) =
            coordinator::run(&f.store, &command, &auth).await.unwrap()
        else {
            panic!()
        };
        let receipt: Value = serde_json::from_slice(&delivery.settlement_receipt).unwrap();
        let result = &receipt["body"]["result"];
        assert_eq!(result["held"], "12000");
        let base: Value =
            serde_json::from_slice(&v.plans[0].delivery().settlement_receipt).unwrap();
        assert_eq!(result["consumed"], base["body"]["result"]["consumed"]);
        assert_eq!(result["released"], "0");
        assert_eq!(result["revision"], "1");
        assert!(result["families"]
            .as_array()
            .unwrap()
            .iter()
            .any(|family| family["status"] != "open"));
        let before = physical(&f).await;
        f.store.clone().close().await;
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        assert_eq!(
            coordinator::run(&f.store, &command, &auth).await.unwrap(),
            coordinator::OutcomeResult::Duplicate(delivery)
        );
        assert_eq!(physical(&f).await, before);
        println!("P3 ordinary code {code}: no consumption/release; slot closed and revision advanced once");
        f.finish().await;
    }
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let auth = SyntheticAuthority(v.proofs.clone());
    coordinator::run(&f.store, &v.commands[0], &auth)
        .await
        .unwrap();
    let mut close = v.commands[3].clone();
    if let coordinator::OutcomeOperation::Close {
        expected_revision, ..
    } = &mut close.operation
    {
        *expected_revision = "0".into();
    }
    let (ordinary, closure) = tokio::join!(
        coordinator::run(&f.store, &v.commands[1], &auth),
        coordinator::run(&f.store, &close, &auth)
    );
    let mut successes = 0;
    for result in [ordinary, closure] {
        match result {
            Ok(coordinator::OutcomeResult::Accepted(d)) => {
                successes += 1;
                let r: Value = serde_json::from_slice(&d.settlement_receipt).unwrap();
                assert_eq!(r["body"]["result"]["revision"], "1");
            }
            Err(crate::ServiceError::Rejection(reason)) => {
                assert!(
                    matches!(reason.as_str(), "EXPECTED_REVISION" | "ORDINARY_CLOSED"),
                    "unexpected guard: {reason}"
                );
            }
            other => panic!("unexpected competing close/ordinary result {other:?}"),
        }
    }
    assert_eq!(successes, 1);
    let counts=f.owner.client.query_one("SELECT (SELECT count(*) FROM ledgerlab.outcome_deliveries),(SELECT count(*) FROM ledgerlab.outcome_records WHERE kind='reservation-transition')",&[]).await.unwrap();
    assert_eq!(counts.get::<_, i64>(0), 2);
    assert_eq!(counts.get::<_, i64>(1), 1);
    println!("P3 ordinary versus closure: exactly one accepted transition, losing transaction rejected after re-resolution");
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_cancel_pending_delivery_rolls_back_all_companions() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    // Owner-only synthetic barrier after all companion writes and the namespace
    // trigger. The production supervisor still executes the actual INSERT.
    f.owner.client.batch_execute("CREATE FUNCTION ledgerlab.test_outcome_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(90621); RETURN NEW; END $$; CREATE TRIGGER z_test_outcome_barrier BEFORE INSERT ON ledgerlab.outcome_deliveries FOR EACH ROW EXECUTE FUNCTION ledgerlab.test_outcome_barrier();").await.unwrap();
    let blocker = config(&f.name, true).connect().await.unwrap();
    blocker
        .client
        .query_one("SELECT pg_advisory_lock(90621)", &[])
        .await
        .unwrap();
    let before = physical(&f).await;
    let mut t = ready(&f.store, &v.plans[0]).await;
    let pid = t.pid;
    let blocked = async {
        let limit = Instant::now() + Duration::from_secs(1);
        loop {
            let waiting: bool = f
                .owner
                .client
                .query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get(0);
            if waiting {
                break;
            }
            assert!(
                Instant::now() < limit,
                "actual delivery insert did not reach barrier"
            );
            tokio::task::yield_now().await;
        }
    };
    let mut append = Box::pin(t.append_outcome(&v.plans[0]));
    tokio::select! { result=&mut append => panic!("append escaped barrier: {result:?}"), _=blocked => {} }
    drop(append);
    blocker.discard().await;
    // A cancelled reply may disappear before rollback can be acknowledged;
    // conservative OutcomeUnknown is valid, but success is forbidden.
    assert!(t.commit().await.is_err());
    f.store.clone().close().await;
    let remaining: i64 = f
        .owner
        .client
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE pid=$1",
            &[&pid],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(remaining, 0);
    assert_eq!(physical(&f).await, before);
    f.owner.client.batch_execute("DROP TRIGGER z_test_outcome_barrier ON ledgerlab.outcome_deliveries; DROP FUNCTION ledgerlab.test_outcome_barrier();").await.unwrap();
    f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    assert!(matches!(
        coordinator::run(
            &f.store,
            &v.commands[0],
            &SyntheticAuthority(v.proofs.clone())
        )
        .await
        .unwrap(),
        coordinator::OutcomeResult::Accepted(_)
    ));
    println!("P3 cancelled actual pending delivery INSERT: every companion/namespace/head rolled back and backend discarded");
    f.finish().await;
}

// The diagnostic observer runs on a separate OS thread/runtime so synchronous
// coordinator work cannot prevent it from observing server lock/idle state.
struct RaceObserver {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl RaceObserver {
    fn start(config: PostgresConfig) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let (ready, receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
                let session = config.connect().await.unwrap();
                ready.send(()).unwrap();
                while !flag.load(Ordering::Acquire) {
                    let rows = session.client.query("SELECT json_build_object('pid',pid,'state',state,'wait_type',wait_event_type,'wait_event',wait_event,'blockers',pg_blocking_pids(pid),'xact_ms',extract(epoch from clock_timestamp()-xact_start)*1000,'query_ms',extract(epoch from clock_timestamp()-query_start)*1000,'query',query)::text FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND state<>'idle' ORDER BY pid",&[]).await.unwrap();
                    for row in rows { postgres::trace::log(format_args!("server {}",row.get::<_,String>(0))); }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                session.discard().await;
            });
        });
        receive.recv().unwrap();
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for RaceObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service; correlated R2 diagnosis"]
async fn postgres_outcome_correlated_race_and_exact_failed_pair_state() {
    let mut f = Fixture::new().await;
    let mut control = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    provision(&mut control, &v).await;
    assert_eq!(physical(&f).await, physical(&control).await);
    let authority = SyntheticAuthority(v.proofs.clone());
    let mut expected = OutcomeSnapshot {
        anchors: vec![],
        records: v.provisioned_records.clone(),
        heads: v.provisioned_heads.clone(),
    };
    for (index, command) in v.commands.iter().enumerate() {
        let before = physical(&f).await;
        let observer = RaceObserver::start(config(&f.name, true));
        let started = Instant::now();
        let (a, b) = tokio::join!(
            postgres::trace::scope(
                format!("stage{index}-left"),
                coordinator::run(&f.store, command, &authority)
            ),
            postgres::trace::scope(
                format!("stage{index}-right"),
                coordinator::run(&f.store, command, &authority)
            )
        );
        let result = |r: &Result<coordinator::OutcomeResult, crate::ServiceError>| match r {
            Ok(coordinator::OutcomeResult::Accepted(_)) => "Accepted".to_string(),
            Ok(coordinator::OutcomeResult::Duplicate(_)) => "Duplicate".to_string(),
            other => format!("{other:?}"),
        };
        println!(
            "R2 pair database={} stage={index} elapsed_ms={} left={} right={}",
            f.name,
            started.elapsed().as_millis(),
            result(&a),
            result(&b)
        );
        drop(observer);
        // Build an exact physical reference by persisting the same validating
        // fixture plan once in a separate provisioned database, with no race.
        let mut tx = ready(&control.store, &v.plans[index]).await;
        tx.append_outcome(&v.plans[index]).await.unwrap();
        tx.commit().await.unwrap();
        let complete = physical(&control).await;
        let actual = physical(&f).await;
        let committed = actual == complete;
        assert!(
            committed || actual == before,
            "partial or unexpected physical race state at {index}"
        );
        if committed {
            validated::apply(&mut expected, &v.plans[index]);
        }
        f.store.clone().close().await;
        f.owner.discard().await;
        f.owner = config(&f.name, true).connect().await.unwrap();
        f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        assert_eq!(physical(&f).await, actual, "reopen changed stage {index}");
        assert_snapshot(&f.store, v.plans[index].resolution(), &expected).await;
        if let Ok(path) = std::env::var("LEDGERLAB_R2_EVIDENCE_DIR") {
            std::fs::create_dir_all(&path).unwrap();
            let major = std::env::var("LEDGERLAB_PG_TEST_MAJOR").unwrap();
            let evidence = json!({"stage":index,"left":result(&a),"right":result(&b),"complete":committed,"before":before,"actual":actual,"sequential_control":complete});
            std::fs::write(
                format!("{path}/pg{major}-stage{index}.json"),
                serde_json::to_vec_pretty(&evidence).unwrap(),
            )
            .unwrap();
        }
        println!("R2 state stage={index}: exact {} physical state, all tables and reopened snapshot checked before result assertion",if committed {"complete"} else {"prior"});
        if a.is_err() || b.is_err() {
            f.finish().await;
            control.finish().await;
            panic!(
                "R2 failed pair after state inspection: left={} right={}",
                result(&a),
                result(&b)
            );
        }
        assert_eq!(
            [&a, &b]
                .iter()
                .filter(|r| matches!(r, Ok(coordinator::OutcomeResult::Accepted(_))))
                .count(),
            1
        );
        for r in [a, b] {
            let d = match r.unwrap() {
                coordinator::OutcomeResult::Accepted(d)
                | coordinator::OutcomeResult::Duplicate(d) => d,
                other => panic!("{other:?}"),
            };
            assert_eq!(&d, v.plans[index].delivery());
        }
    }
    f.finish().await;
    control.finish().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL 17/18 TLS service"]
async fn postgres_outcome_stale_snapshot_restarts_at_lock_before_planning() {
    let mut f = Fixture::new().await;
    let v = validated::lifecycle();
    provision(&mut f, &v).await;
    let mut register = ready(&f.store, &v.plans[0]).await;
    register.append_outcome(&v.plans[0]).await.unwrap();
    register.commit().await.unwrap();
    let mut stale = f
        .store
        .begin_outcome(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    // Establish the SERIALIZABLE snapshot before a different transaction updates
    // the reservation/target heads. The immutable scope guard remains unchanged.
    stale.load_installation().await.unwrap();
    let mut winner = ready(&f.store, &v.plans[1]).await;
    winner.append_outcome(&v.plans[1]).await.unwrap();
    winner.commit().await.unwrap();
    let complete = physical(&f).await;
    let result = stale.lock_scopes(&v.plans[1].resolution().locks).await;
    let rejected_at_lock = matches!(&result, Err(StoreError::Postgres(e)) if e.code().is_some_and(|c| c.code()=="40001"));
    println!("R2 deterministic stale-snapshot lock result: {result:?}");
    stale.rollback().await.unwrap();
    assert_eq!(physical(&f).await, complete);
    f.store.clone().close().await;
    f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    assert_eq!(physical(&f).await, complete);
    let original = coordinator::run(
        &f.store,
        &v.commands[1],
        &ReadOnlyAuthority(SyntheticAuthority(v.proofs.clone())),
    )
    .await
    .unwrap();
    assert_eq!(
        original,
        coordinator::OutcomeResult::Duplicate(v.plans[1].delivery().clone())
    );
    assert_eq!(physical(&f).await, complete);
    f.finish().await;
    assert!(rejected_at_lock,"stale SERIALIZABLE snapshot reached planning instead of restarting during lock acquisition");
}
