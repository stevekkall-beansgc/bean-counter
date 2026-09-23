//! Actual linked SQLite storage tests; these do not stand in for R3 execution.
use super::{tests, SqliteStore};
use crate::store::ports::{AcceptanceStore, AcceptanceTx};
use sqlx::Row;
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test]
async fn r3_explicit_provisioning_binds_real_lane_quota_and_storage_incarnation() {
    use crate::store::{
        adjudication::*,
        outcomes::{OutcomeLock, OutcomeLockClass, OutcomeLockMode},
    };
    use ledgerlab_core::adjudication::{
        self as r3, commands as w,
        runtime::accounting::Worksheet,
        types::{Count, Id},
    };
    let dir = tempfile::tempdir().unwrap();
    let anchor = tempfile::tempdir().unwrap();
    let store = SqliteStore::create_fenced(dir.path(), tests::installation(), anchor.path())
        .await
        .unwrap();
    let install = tests::installation();
    let journal = JournalIdentity {
        store: Id::parse(&install.logical_store_id).unwrap(),
        scope: w::Scope(
            Id::parse(&install.scope.tenant).unwrap(),
            Id::parse(&install.scope.environment).unwrap(),
        ),
        registration: Id::parse("registration").unwrap(),
        host: Id::parse("gateway-one").unwrap(),
    };
    let ws = Worksheet::frozen().unwrap();
    let mut logical = w::Resource::zero();
    for _ in 0..32 {
        for kind in ws.bundle("finish_gateway").unwrap() {
            logical = logical
                .checked_add(&ws.template(kind).unwrap().resources().unwrap())
                .unwrap();
        }
    }
    let maximum = ws.template("PREPARE_ENROLL").unwrap().resources().unwrap();
    logical = logical.checked_add(&maximum).unwrap();
    let backing = Count::new(1u128 << 40).unwrap();
    let ceiling = logical.checked_add(&maximum).unwrap();
    assert!(store
        .provision_adjudication_with_ceiling(
            journal.clone(),
            logical.clone(),
            ceiling.clone(),
            65536,
            Count::new(1).unwrap()
        )
        .await
        .is_err());
    let configured = store
        .provision_adjudication_with_ceiling(
            journal.clone(),
            logical.clone(),
            ceiling.clone(),
            65536,
            backing,
        )
        .await
        .unwrap();
    let work = WorkRequest {
        journal: journal.clone(),
        owner: Id::parse("work-owner").unwrap(),
        transition: r3::raw_sha256(b"transition"),
        mandatory: false,
        maximum,
    };
    let fence = configured
        .writer_fence(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let mut tx = configured
        .begin_adjudication(&work, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let guards = vec![Guard::Legacy(OutcomeLock {
        class: OutcomeLockClass::Admission,
        key: r3::canonical_bytes(&serde_json::json!([journal.scope]), 4096).unwrap(),
        mode: OutcomeLockMode::Write,
    })];
    tx.lock_adjudication(&guards).await.unwrap();
    let cap = tx.commit_capability().await.unwrap();
    assert_eq!(cap.recovered_through().ordinal(), Count::ZERO);
    assert_eq!(cap.epoch().value(), 1);
    assert_eq!(cap.resource_ceiling(), &ceiling);
    assert_eq!(cap.writer_fence(), Some(&fence));
    let pages: i64 = sqlx::query_scalar("PRAGMA max_page_count")
        .fetch_one(tx.conn())
        .await
        .unwrap();
    assert_eq!(
        cap.physical().maximum_retained_bytes().value(),
        pages as u128 * 4096
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        configured
            .writer_fence(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap(),
        fence
    );
    let optional_slots = std::sync::Arc::clone(&store.inner.queue)
        .acquire_many_owned(65)
        .await
        .unwrap();
    assert!(matches!(
        configured
            .begin_adjudication(&work, Instant::now() + Duration::from_secs(2))
            .await,
        Err(crate::store::errors::StoreError::Overloaded)
    ));
    let mut mandatory = work.clone();
    mandatory.mandatory = true;
    let tx = configured
        .begin_adjudication(&mandatory, Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    drop(optional_slots);
    drop(configured);
    store.close().await;
    let reopened = SqliteStore::open_fenced(dir.path(), anchor.path())
        .await
        .unwrap();
    let configured = reopened
        .provision_adjudication_with_ceiling(
            journal.clone(),
            logical.clone(),
            ceiling.clone(),
            65536,
            backing,
        )
        .await
        .unwrap();
    drop(configured);
    reopened.close().await;
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(dir.path().join("local.db"), copy.path().join("local.db")).unwrap();
    assert!(SqliteStore::open(copy.path()).await.is_err());
}

#[tokio::test]
async fn r3_writer_lane_is_shared_and_requires_completed_checkpoint() {
    use crate::store::{
        comparison::{ComparisonReadStore, ComparisonReadTx},
        errors::StoreError,
    };
    use std::sync::atomic::Ordering;
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), tests::installation())
        .await
        .unwrap();
    let contender = store.test_contender().await.unwrap();
    store
        .inner
        .adjudication_enabled
        .store(true, Ordering::Release);
    store
        .begin_read(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap()
        .finish()
        .await
        .unwrap();
    // Holding an idle client handle pins writers only until its actual worker
    // deadline. The worker rolls back even though the client remains alive.
    let idle = store
        .begin_read(Instant::now() + Duration::from_millis(150))
        .await
        .unwrap();
    assert!(matches!(
        contender
            .begin(Instant::now() + Duration::from_millis(20))
            .await,
        Err(StoreError::Deadline)
    ));
    let resumed = contender
        .begin(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    resumed.rollback().await.unwrap();
    assert!(idle.finish().await.is_err());
    let tx = store
        .begin(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    assert!(matches!(
        contender
            .begin(Instant::now() + Duration::from_millis(20))
            .await,
        Err(StoreError::Deadline)
    ));
    tx.rollback().await.unwrap();
    // A deliberately unadmitted raw reader is a positive checkpoint-blocking
    // control. Production profile reads all participate in the shared gate.
    let mut pinned = store.inner.readers.begin().await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM installation")
        .fetch_one(&mut *pinned)
        .await
        .unwrap();
    let mut raw = store.inner.writer.acquire().await.unwrap();
    sqlx::query("UPDATE installation SET generation=generation+1")
        .execute(&mut *raw)
        .await
        .unwrap();
    drop(raw);
    assert!(matches!(
        store.begin(Instant::now() + Duration::from_secs(2)).await,
        Err(StoreError::Overloaded)
    ));
    pinned.rollback().await.unwrap();
    let tx = contender
        .begin(Instant::now() + Duration::from_secs(2))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    contender.close().await;
    store.close().await;
}

#[tokio::test]
async fn r3_namespace_is_permanent_and_protects_both_legacy_writers() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), tests::installation())
        .await
        .unwrap();
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    for op in tests::seed().into_iter().chain(tests::schedule()) {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
    let mut conn = store.inner.writer.acquire().await.unwrap();
    let scope = sqlx::query("SELECT tenant,environment,source FROM delivery_keys LIMIT 1")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let tenant: String = scope.get(0);
    let environment: String = scope.get(1);
    let source: String = scope.get(2);
    let journal = b"synthetic-storage-guard".to_vec();
    sqlx::query("INSERT INTO r3_journals VALUES (?,?,?,?,?)")
        .bind(&journal)
        .bind(b"{}".as_slice())
        .bind(0u128.to_be_bytes().as_slice())
        .bind("0".repeat(64))
        .bind("0".repeat(64))
        .execute(&mut *conn)
        .await
        .unwrap();
    let old_tag = "a".repeat(32);
    let owned_tag = "b".repeat(32);
    let legacy_key = format!("gw1.{old_tag}.existing");
    sqlx::query("INSERT INTO delivery_keys SELECT tenant,environment,source,?,ingress_hash,canonical_event_id,'alias',observed_us,canonical_bytes,content_hash,schema_version FROM delivery_keys LIMIT 1")
        .bind(&legacy_key).execute(&mut *conn).await.unwrap();
    let error = sqlx::query("INSERT INTO r3_namespaces VALUES (?,?,?,?,?)")
        .bind(&tenant)
        .bind(&environment)
        .bind(&old_tag)
        .bind("gateway")
        .bind(&journal)
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("R3 namespace already occupied"));
    sqlx::query("INSERT INTO r3_namespaces VALUES (?,?,?,?,?)")
        .bind(&tenant)
        .bind(&environment)
        .bind(&owned_tag)
        .bind("gateway")
        .bind(&journal)
        .execute(&mut *conn)
        .await
        .unwrap();
    let unused = format!("gw1.{owned_tag}.never-imported");
    let error=sqlx::query("INSERT INTO delivery_keys SELECT tenant,environment,source,?,ingress_hash,canonical_event_id,'alias',observed_us,canonical_bytes,content_hash,schema_version FROM delivery_keys LIMIT 1").bind(&unused).execute(&mut *conn).await.unwrap_err();
    assert!(error.to_string().contains("R3 delivery namespace owned"));
    // The BEFORE guard rejects the same unused range through the outcome path,
    // before missing fixture economics could be mistaken for the rejection cause.
    let error = sqlx::query(
        "INSERT INTO outcome_deliveries (tenant,environment,source,external_id) VALUES (?,?,?,?)",
    )
    .bind(&tenant)
    .bind(&environment)
    .bind("another-source")
    .bind(&unused)
    .execute(&mut *conn)
    .await
    .unwrap_err();
    assert!(error.to_string().contains("R3 delivery namespace owned"));
    let error = sqlx::query("INSERT INTO r3_deliveries VALUES (?,?,?,?,?,?)")
        .bind(&tenant)
        .bind(&environment)
        .bind(&source)
        .bind(&legacy_key)
        .bind(&journal)
        .bind(b"{}".as_slice())
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("prior delivery identity occupied"));
    assert!(sqlx::query("DELETE FROM r3_namespaces")
        .execute(&mut *conn)
        .await
        .unwrap_err()
        .to_string()
        .contains("immutable R3 storage"));
    drop(conn);
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    let tags: Vec<String> = sqlx::query_scalar("SELECT tag FROM r3_namespaces")
        .fetch_all(&reopened.inner.readers)
        .await
        .unwrap();
    assert_eq!(tags, vec![owned_tag]);
    reopened.close().await;
}

#[tokio::test]
async fn r3_pages_are_bounded_immutable_and_ordinals_exceed_i64() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), tests::installation())
        .await
        .unwrap();
    let mut conn = store.inner.writer.acquire().await.unwrap();
    use crate::store::adjudication::JournalIdentity;
    use ledgerlab_core::adjudication::{
        commands::Scope,
        types::{Count, Digest, Id},
    };
    let identity = JournalIdentity {
        store: Id::parse("store-demo-slice").unwrap(),
        scope: Scope(
            Id::parse("tenant").unwrap(),
            Id::parse("environment").unwrap(),
        ),
        registration: Id::parse("registration").unwrap(),
        host: Id::parse("store-demo-slice").unwrap(),
    };
    let j = super::adjudication::journal_key(&identity).unwrap();
    sqlx::query("INSERT INTO r3_journals VALUES (?,?,?,?,?)")
        .bind(&j)
        .bind(b"{}".as_slice())
        .bind(0u128.to_be_bytes().as_slice())
        .bind("0".repeat(64))
        .bind("0".repeat(64))
        .execute(&mut *conn)
        .await
        .unwrap();
    let counts = [
        1u128,
        i64::MAX as u128 + 1,
        999999999999999999999999999999u128,
    ];
    for (i, count) in counts.iter().enumerate() {
        let ordinal = count.to_be_bytes().to_vec();
        sqlx::query("INSERT INTO r3_segments VALUES (?,?,?,?,?,?)")
            .bind(&j)
            .bind(&ordinal)
            .bind(format!("{i:064x}"))
            .bind("0".repeat(64))
            .bind(4096i64)
            .bind(1i64)
            .execute(&mut *conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO r3_segment_pages VALUES (?,?,?,?)")
            .bind(&j)
            .bind(&ordinal)
            .bind(0i64)
            .bind(vec![b'x'; 4096])
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let too_large = (counts[2] + 1).to_be_bytes();
    let error = sqlx::query("INSERT INTO r3_segments VALUES (?,?,?,?,?,?)")
        .bind(&j)
        .bind(too_large.as_slice())
        .bind("f".repeat(64))
        .bind("0".repeat(64))
        .bind(2i64)
        .bind(1i64)
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("CHECK constraint failed"));
    let ordered: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT ordinal FROM r3_segments ORDER BY ordinal")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        ordered,
        counts
            .iter()
            .map(|n| n.to_be_bytes().to_vec())
            .collect::<Vec<_>>()
    );
    assert!(sqlx::query("INSERT INTO r3_segment_pages VALUES (?,?,?,?)")
        .bind(&j)
        .bind(counts[0].to_be_bytes().as_slice())
        .bind(1i64)
        .bind(vec![0; 4097])
        .execute(&mut *conn)
        .await
        .is_err());
    assert!(sqlx::query("UPDATE r3_segment_pages SET bytes=x'00'")
        .execute(&mut *conn)
        .await
        .unwrap_err()
        .to_string()
        .contains("immutable R3 storage"));
    let plan:Vec<sqlx::sqlite::SqliteRow>=sqlx::query("EXPLAIN QUERY PLAN SELECT bytes FROM r3_segment_pages WHERE journal=? AND ordinal=? AND page=?").bind(&j).bind(counts[1].to_be_bytes().as_slice()).bind(0i64).fetch_all(&mut *conn).await.unwrap();
    assert!(plan
        .iter()
        .any(|r| r.get::<String, _>(3).contains("SEARCH")));
    drop(conn);
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    let sizes: Vec<i64> =
        sqlx::query_scalar("SELECT length(bytes) FROM r3_segment_pages ORDER BY ordinal")
            .fetch_all(&reopened.inner.readers)
            .await
            .unwrap();
    assert_eq!(sizes, vec![4096; 3]);
    let mut reader = reopened.inner.readers.acquire().await.unwrap();
    let fragment = super::adjudication::segment_page(
        &mut reader,
        &identity,
        &Digest::parse(&format!("{:064x}", 1)).unwrap(),
        Count::ZERO,
        17,
        31,
    )
    .await
    .unwrap();
    assert_eq!(fragment.bytes, vec![b'x'; 31]);
    assert_eq!(fragment.offset, 17);
    assert_eq!(fragment.total_bytes.value(), 4096);
    assert!(super::adjudication::segment_page(
        &mut reader,
        &identity,
        &Digest::parse(&format!("{:064x}", 1)).unwrap(),
        Count::ZERO,
        4090,
        31
    )
    .await
    .is_err());
    drop(reader);
    let mut writer = reopened.inner.writer.acquire().await.unwrap();
    assert!(sqlx::query("DELETE FROM r3_journals")
        .execute(&mut *writer)
        .await
        .unwrap_err()
        .to_string()
        .contains("permanent R3 journal identity"));
    drop(writer);
    reopened.close().await;
}

#[tokio::test]
async fn r3_optional_legacy_quota_rejects_growth_and_freelist_reuse_atomically() {
    use crate::store::{
        adjudication::*,
        errors::{CommitError, StoreError},
    };
    use ledgerlab_core::adjudication::{
        commands as w,
        runtime::accounting::Worksheet,
        types::{Count, Id},
    };
    for reuse in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let installation = tests::installation();
        let store = SqliteStore::create(dir.path(), installation.clone())
            .await
            .unwrap();
        if reuse {
            // Test-only setup creates free pages before provisioning; the scratch
            // table is absent from the actual profile and business inventory.
            let mut conn = store.inner.writer.acquire().await.unwrap();
            sqlx::query("CREATE TABLE quota_scratch (body BLOB)")
                .execute(&mut *conn)
                .await
                .unwrap();
            sqlx::query("INSERT INTO quota_scratch VALUES (zeroblob(262144))")
                .execute(&mut *conn)
                .await
                .unwrap();
            sqlx::query("DROP TABLE quota_scratch")
                .execute(&mut *conn)
                .await
                .unwrap();
        }
        let journal = JournalIdentity {
            store: Id::parse(&installation.logical_store_id).unwrap(),
            scope: w::Scope(
                Id::parse(&installation.scope.tenant).unwrap(),
                Id::parse(&installation.scope.environment).unwrap(),
            ),
            registration: Id::parse("quota-test").unwrap(),
            host: Id::parse("gateway-quota").unwrap(),
        };
        let maximum = Worksheet::frozen()
            .unwrap()
            .template("PREPARE_ENROLL")
            .unwrap()
            .resources()
            .unwrap();
        let configured = store
            .provision_adjudication(
                journal.clone(),
                maximum.clone(),
                0,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let before = super::adjudication::physical_usage(tx.conn())
            .await
            .unwrap();
        let file_before: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(tx.conn())
            .await
            .unwrap();
        // Exercise the real legacy commit boundary with a physical append. This
        // deliberately does not claim to be an accepted business command.
        sqlx::query("INSERT INTO outcome_records VALUES ('synthetic','sandbox','evidence',X'01',?,zeroblob(131072))").bind(format!("sha256:{}","0".repeat(64))).execute(tx.conn()).await.unwrap();
        assert!(
            super::adjudication::physical_usage(tx.conn())
                .await
                .unwrap()
                > before
        );
        let file_after: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(tx.conn())
            .await
            .unwrap();
        if reuse {
            assert_eq!(
                file_after, file_before,
                "free pages reused without file growth"
            );
        } else {
            assert!(file_after > file_before);
        }
        assert!(matches!(
            tx.commit().await,
            Err(CommitError::RolledBack(StoreError::Overloaded))
        ));
        let mut read = store.inner.readers.acquire().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outcome_records")
            .fetch_one(&mut *read)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let used: Vec<u8> = sqlx::query_scalar("SELECT legacy_used FROM r3_storage_profile")
            .fetch_one(&mut *read)
            .await
            .unwrap();
        assert_eq!(used, 0u128.to_be_bytes());
        drop(read);
        let work = WorkRequest {
            journal,
            owner: Id::parse("owner").unwrap(),
            transition: ledgerlab_core::adjudication::raw_sha256(b"quota-work"),
            mandatory: false,
            maximum,
        };
        configured
            .begin_adjudication(&work, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap()
            .rollback()
            .await
            .unwrap();
        drop(configured);
        store.close().await;
    }
}
