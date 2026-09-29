use super::*;
use crate::store::{
    ports::{AcceptanceStore, AcceptanceTx},
    records::{Installation, Scope, WriteOp},
};
use sqlx::{Connection, SqliteConnection};
use std::time::Duration;

async fn schema10() -> (tempfile::TempDir, String, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("local.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db)
        .create_if_missing(true)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    migrator().run(&mut conn).await.unwrap();
    let raw = include_bytes!("../../../../../examples/billing/setup.json");
    let setup = crate::service::billing::Setup::parse(raw).unwrap();
    let bytes = ledgerlab_core::canonical::outcome::bytes(&serde_json::json!(setup)).unwrap();
    super::super::write::operation(
        &mut conn,
        &WriteOp::SeedInstallation(Installation {
            scope: Scope {
                tenant: setup.scope.tenant().to_owned(),
                environment: setup.scope.environment().to_owned(),
            },
            logical_store_id: setup.store_id.clone(),
            mode: "real".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 0,
        }),
    )
    .await
    .unwrap();
    conn.close().await.unwrap();
    let store = super::super::SqliteStore::open(dir.path()).await.unwrap();
    let mut tx = store
        .begin(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    tx.billing_setup(&bytes).await.unwrap();
    tx.billing_m2_initialize(super::super::BillingM2Initialization {
        customer: &setup.customer,
        tenant: setup.scope.tenant(),
        environment: setup.scope.environment(),
        source: &setup.source,
        agreement_id: &setup.agreement,
        accepted_at_us: setup.accepted_at.micros(),
        setup: &bytes,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    store.close().await;
    (dir, setup.store_id, bytes)
}

async fn connect(dir: &tempfile::TempDir) -> SqliteConnection {
    SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.path().join("local.db"))
            .create_if_missing(false)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn fresh_installation_has_exact_schema11_sidecar_and_initial_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let store = super::super::SqliteStore::create(
        dir.path(),
        Installation {
            scope: Scope {
                tenant: "tenant".into(),
                environment: "dev".into(),
            },
            logical_store_id: "store".into(),
            mode: "real".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 0,
        },
    )
    .await
    .unwrap();
    store.close().await;
    let mut conn = connect(&dir).await;
    assert_eq!(version(&mut conn).await.unwrap(), 11);
    super::super::m5::verify(&mut conn).await.unwrap();
    let initial: (i64, i64, i64) = sqlx::query_as(
        "SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(initial, (1, 0, 0));
}

#[tokio::test]
async fn schema10_upgrade_preserves_old_bytes_and_reconciles_retry() {
    let (dir, id, setup) = schema10().await;
    let seed = preflight_m5(dir.path(), &id).await.unwrap();
    assert_eq!(
        upgrade_m5(dir.path(), &seed).await.unwrap(),
        crate::maintenance::UpgradeResult::Upgraded
    );
    let mut conn = connect(&dir).await;
    assert_eq!(version(&mut conn).await.unwrap(), 11);
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT canonical_bytes FROM billing_setup WHERE singleton=1"
        )
        .fetch_one(&mut conn)
        .await
        .unwrap(),
        setup
    );
    super::super::m5::verify(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    assert_eq!(
        upgrade_m5(dir.path(), &seed).await.unwrap(),
        crate::maintenance::UpgradeResult::AlreadyCurrent
    );
}

#[tokio::test]
async fn malformed_m3_index_refuses_before_ddl() {
    let (dir, id, _) = schema10().await;
    let mut conn = connect(&dir).await;
    sqlx::query("INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us) VALUES(1,'customer-1','urn:example:work','forged',x'01','urn:example:work','base',1)")
        .execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    assert!(preflight_m5(dir.path(), &id).await.is_err());
    let mut conn = connect(&dir).await;
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    let m5: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'billing_m5_%'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(m5, 0);
}

#[tokio::test]
async fn post_ddl_failure_rolls_back_every_sidecar_object() {
    let (dir, _, _) = schema10().await;
    let mut conn = connect(&dir).await;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await.unwrap();
    apply_m5(&mut tx).await.unwrap();
    assert!(sqlx::query("INSERT INTO billing_m5_state(singleton,migration_id,record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence,next_record_sequence) VALUES(1,'conflict',0,0,0,0,1,1)")
        .execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    let m5: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'billing_m5_%'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(m5, 0);
}
