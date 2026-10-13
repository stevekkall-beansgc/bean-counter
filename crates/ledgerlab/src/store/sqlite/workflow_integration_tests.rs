//! Shared schema and commit hooks, using actual legacy billing initialization.
use super::*;
use crate::{
    billing::BillingLedger,
    store::{
        errors::CommitError,
        ports::{AcceptanceStore, AcceptanceTx},
    },
};
const SETUP: &[u8] = include_bytes!("../../../../../examples/billing/setup.json");
const EVENT: &[u8] = include_bytes!("../../../../../examples/billing/event.json");
async fn create() -> (tempfile::TempDir, std::path::PathBuf, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    BillingLedger::init(&path, SETUP).await.unwrap();
    let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
    (dir, path, store)
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
async fn enable(store: &SqliteStore) {
    let mut tx = store.begin(deadline()).await.unwrap();
    let snapshot = tx.billing_snapshot().await.unwrap();
    crate::service::billing::validate_snapshot(&snapshot).unwrap();
    tx.workflow_enable_profile_b().await.unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn explicit_upgrade_preserves_legacy_bytes_and_reopens_with_legacy_writer() {
    let (_dir, path, store) = create().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&store.inner.writer)
            .await
            .unwrap(),
        11
    );
    let before: Vec<u8> = sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup")
        .fetch_one(&store.inner.writer)
        .await
        .unwrap();
    enable(&store).await;
    enable(&store).await;
    let after: Vec<u8> = sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup")
        .fetch_one(&store.inner.writer)
        .await
        .unwrap();
    assert_eq!(before, after);
    store.close().await;
    let ledger = BillingLedger::open(&path).await.unwrap();
    let first = ledger
        .accept("customer-1", "urn:example:work", EVENT)
        .await
        .unwrap();
    let repeat = ledger
        .accept("customer-1", "urn:example:work", EVENT)
        .await
        .unwrap();
    assert!(first["receipt"].is_object());
    assert_eq!(first["receipt"], repeat["receipt"]);
    ledger.close().await;
    let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
    let mut tx = store.begin(deadline()).await.unwrap();
    assert_eq!(tx.billing_schema_version().await.unwrap(), 12);
    let snapshot = tx.billing_snapshot().await.unwrap();
    crate::service::billing::validate_snapshot(&snapshot).unwrap();
    assert_eq!(snapshot.entry_count, 1);
    assert_eq!(
        workflow::capacity::check(tx.conn(), 0, 0)
            .await
            .unwrap()
            .operations,
        0
    );
    tx.rollback().await.unwrap();
    store.close().await;
}
#[tokio::test]
async fn upgrade_failure_rolls_back_schema_and_all_additive_tables() {
    let (_dir, path, store) = create().await;
    sqlx::raw_sql("CREATE TRIGGER fail_workflow_migration BEFORE INSERT ON _sqlx_migrations WHEN NEW.version=12 BEGIN SELECT RAISE(ABORT,'injected migration failure'); END;").execute(&store.inner.writer).await.unwrap();
    let mut tx = store.begin(deadline()).await.unwrap();
    let snapshot = tx.billing_snapshot().await.unwrap();
    crate::service::billing::validate_snapshot(&snapshot).unwrap();
    assert!(tx.workflow_enable_profile_b().await.is_err());
    assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&store.inner.writer)
            .await
            .unwrap(),
        11
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'workflow_%'"
        )
        .fetch_one(&store.inner.writer)
        .await
        .unwrap(),
        0
    );
    sqlx::raw_sql("DROP TRIGGER fail_workflow_migration")
        .execute(&store.inner.writer)
        .await
        .unwrap();
    store.close().await;
    let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
    enable(&store).await;
    store.close().await;
}
#[tokio::test]
async fn common_commit_hook_rechecks_both_legacy_meters_after_mutation() {
    let (_dir, _path, store) = create().await;
    enable(&store).await;
    for mutation in [
        "UPDATE billing_m3_bounds SET guard=guard+1,entry_count=entry_count+1,entry_bytes=entry_bytes+1",
        "UPDATE billing_m5_state SET next_command_sequence=next_command_sequence+1",
    ] {
        let mut tx = store.begin(deadline()).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(mutation))
            .execute(tx.conn())
            .await
            .unwrap();
        assert!(matches!(
            tx.commit().await,
            Err(CommitError::RolledBack(StoreError::Integrity(_)))
        ));
        let mut tx = store.begin(deadline()).await.unwrap();
        workflow::capacity::check(tx.conn(), 0, 0).await.unwrap();
        tx.rollback().await.unwrap();
    }
    store.close().await;
}

#[tokio::test]
async fn common_commit_capacity_guard_refuses_legacy_pool_growth_after_workflow() {
    let (_dir, _path, store) = create().await;
    enable(&store).await;
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.workflow_append(&workflow::tests::fixture())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // Mechanical rows isolate admission arithmetic from price/authority. The
    // independent expected capacity is E100000 + W1 > 100000; legacy's own
    // E100000 ceiling is respected. These are not accepted economic histories.
    let mut tx = store.begin(deadline()).await.unwrap();
    sqlx::raw_sql("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO billing_m3_entries SELECT x,'customer-1','urn:example:work',CAST(x AS TEXT),CAST(x AS BLOB),x'00',x'00',x'00',0,'agreement-1',1 FROM n;")
        .execute(tx.conn()).await.unwrap();
    sqlx::raw_sql("INSERT INTO billing_m3_index SELECT ordinal,customer,source,external_id,semantic_key,external_id,'base',accepted_at_us FROM billing_m3_entries;")
        .execute(tx.conn()).await.unwrap();
    assert!(matches!(
        tx.commit().await,
        Err(CommitError::RolledBack(StoreError::BillingHistoryLimit))
    ));
    let mut tx = store.begin(deadline()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM billing_m3_entries")
            .fetch_one(tx.conn())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        workflow::capacity::check(tx.conn(), 0, 0)
            .await
            .unwrap()
            .operations,
        1
    );
    tx.rollback().await.unwrap();
    store.close().await;
}
