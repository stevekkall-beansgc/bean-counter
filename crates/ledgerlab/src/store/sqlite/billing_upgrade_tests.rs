//! Storage qualification uses original v1-v8 migration bytes, never a current
//! database with its version number rewritten to pretend it is old.
use super::*;
use crate::maintenance::{UpgradeError, UpgradeResult};
use ledgerlab_core::{canonical::CanonicalBytes, domain::Timestamp};
use sqlx::{Connection, SqliteConnection};
const SETUP: &[u8] = include_bytes!("../../../../../examples/billing/setup.json");
const EVENT: &[u8] = include_bytes!("../../../../../examples/billing/event.json");
/// The first retained decision's own ledger time. A later decision, an upgrade
/// or a retry is proved to advance past it rather than to reuse it.
const FIRST_ACCEPTED_AT: &str = "2026-09-02T00:00:00.000000Z";

/// A real installation directory: the private `.ledger` data directory, the
/// canonical `billing.json`, the frozen migrations, the original installation
/// row and the exact setup bytes the billing writer stores. No public upgrade
/// path is qualified against a database the product could not have produced.
struct Installation {
    path: std::path::PathBuf,
    data: std::path::PathBuf,
    options: sqlx::sqlite::SqliteConnectOptions,
    setup: serde_json::Value,
}

async fn legacy() -> (tempfile::TempDir, SqliteConnection, BillingUpgradeSeed) {
    legacy_at(8).await
}

/// A frozen store built from the original migrations up to `version`. A schema-9
/// store is the M2 lane's own state: the schema-8 tables are already frozen by
/// their guards, and the scoped sidecars are writable.
async fn legacy_at(version: usize) -> (tempfile::TempDir, SqliteConnection, BillingUpgradeSeed) {
    let dir = tempfile::tempdir().unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.path().join("local.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    // Always start from schema 8: the frozen tables accept their rows before
    // their own migration makes them immutable.
    Migrator::with_migrations(migrator().iter().take(8).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    let value = ledgerlab_core::canonical::parse(include_bytes!(
        "../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    let setup_bytes = CanonicalBytes::from_value(&value).unwrap().into_vec();
    let mut seed = BillingUpgradeSeed {
        store_id: value["store_id"].as_str().unwrap().into(),
        tenant: value["scope"][0].as_str().unwrap().into(),
        environment: value["scope"][1].as_str().unwrap().into(),
        customer: value["customer"].as_str().unwrap().into(),
        source: value["source"].as_str().unwrap().into(),
        agreement_id: value["agreement"].as_str().unwrap().into(),
        effective_at_us: Timestamp::parse(value["accepted_at"].as_str().unwrap())
            .unwrap()
            .micros(),
        recorded_at_us: Timestamp::parse("2026-09-26T00:00:00.000000Z")
            .unwrap()
            .micros(),
        setup_bytes,
        snapshot_digest: [0; 32],
        source_version: version as i64,
        index: vec![BillingUpgradeIndex {
            ordinal: 1,
            customer: value["customer"].as_str().unwrap().into(),
            source: value["source"].as_str().unwrap().into(),
            external_id: "delivery".into(),
            // The retained schema-8 semantic key is x'0102'; the store re-derives
            // it from the frozen row and refuses any other value.
            semantic_key: vec![0x01, 0x02],
            target: "urn:example:legacy-target".into(),
            kind: "base".into(),
            accepted_at_us: Timestamp::parse(value["accepted_at"].as_str().unwrap())
                .unwrap()
                .micros(),
        }],
    };
    super::super::write::operation(
        &mut conn,
        &crate::store::records::WriteOp::SeedInstallation(crate::store::records::Installation {
            scope: crate::store::records::Scope {
                tenant: seed.tenant.clone(),
                environment: seed.environment.clone(),
            },
            logical_store_id: seed.store_id.clone(),
            mode: "real".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 0,
        }),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO billing_setup VALUES(1,?)")
        .bind(&seed.setup_bytes)
        .execute(&mut conn)
        .await
        .unwrap();
    // Distinct opaque storage bytes make accidental rewriting observable.
    sqlx::query(
        "INSERT INTO billing_entries VALUES(1,?,'delivery',x'0102',x'0304',x'0506',x'0708')",
    )
    .bind(&seed.source)
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO billing_aliases VALUES(?,'alias',x'090a',1)")
        .bind(&seed.source)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO billing_permissions VALUES(2,x'0b0c')")
        .execute(&mut conn)
        .await
        .unwrap();
    if version > 8 {
        // The schema-8 tables freeze at their own migration, so the frozen rows
        // are retained first and the scoped identity is seeded exactly as the
        // billing upgrade seeds it.
        Migrator::with_migrations(migrator().iter().take(version).cloned().collect())
            .run(&mut conn)
            .await
            .unwrap();
        seed_m2_customer(
            &mut conn,
            &seed.customer,
            &seed.tenant,
            &seed.environment,
            &seed.source,
            &seed.agreement_id,
            seed.effective_at_us,
            &seed.setup_bytes,
        )
        .await;
    }
    seed.snapshot_digest = preflight_billing(dir.path()).await.unwrap().digest;
    (dir, conn, seed)
}

/// The M2 lane's own schema-9 seeding: the installation's own customer and its
/// first agreement, written exactly as `billing_m2_initialize` writes them, so a
/// schema-9 source is the state the product actually left behind rather than a
/// schema-8 table with a bumped version number.
#[allow(clippy::too_many_arguments)]
async fn seed_m2_customer(
    conn: &mut SqliteConnection,
    customer: &str,
    tenant: &str,
    environment: &str,
    source: &str,
    agreement_id: &str,
    effective_at_us: i64,
    setup_bytes: &[u8],
) {
    sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
        .bind(customer)
        .bind(tenant)
        .bind(environment)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,1,?,1,'start',?,?,?)")
        .bind(customer)
        .bind(source)
        .bind(agreement_id)
        .bind(effective_at_us)
        .bind(effective_at_us)
        .bind(setup_bytes)
        .execute(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn billing_preflight_returns_original_history_and_requires_exact_migrated_snapshot() {
    let (dir, mut conn, seed) = legacy().await;
    let preflight = preflight_billing(dir.path()).await.unwrap();
    assert_eq!(preflight.digest, seed.snapshot_digest);
    assert_eq!(preflight.version, 8);
    assert_eq!(preflight.snapshot.setup, seed.setup_bytes);
    assert_eq!(preflight.snapshot.entry_count, 1);
    assert_eq!(preflight.snapshot.alias_count, 1);
    assert!(!preflight.snapshot.indexed);
    assert_eq!(preflight.snapshot.entries.len(), 1);
    assert_eq!(preflight.snapshot.entries[0].ordinal, 1);
    assert_eq!(
        preflight.snapshot.entries[0].ingress.as_slice(),
        b"\x03\x04"
    );
    assert_eq!(preflight.snapshot.aliases.len(), 1);
    assert_eq!(
        preflight.snapshot.aliases[0].ingress.as_slice(),
        b"\x09\x0a"
    );
    assert_eq!(preflight.snapshot.permissions, vec![b"\x0b\x0c".to_vec()]);
    sqlx::query("INSERT INTO billing_entries VALUES(2,?,'later',x'11',x'12',x'13',x'14')")
        .bind(&seed.source)
        .execute(&mut conn)
        .await
        .unwrap();
    assert_ne!(
        preflight_billing(dir.path()).await.unwrap().digest,
        seed.snapshot_digest
    );
    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    assert_eq!(version(&mut conn).await.unwrap(), 8);
    let preserved: Vec<u8> =
        sqlx::query_scalar("SELECT ingress FROM billing_entries WHERE ordinal=2")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(preserved.as_slice(), b"\x12");
}

/// A schema-8 preflight that goes stale because the installation advances to
/// schema 9 before the upgrade transaction is refused, even though the frozen
/// schema-8 rows the coordinator validated are still there byte for byte. The
/// retained history is not the identity of a seed: the schema it was read and
/// validated under is. A stale schema-8 seed must not be allowed to spend a
/// schema-9 installation the coordinator never read, and must leave that
/// installation exactly as the M2 lane wrote it.
#[tokio::test]
async fn billing_upgrade_refuses_a_stale_schema8_seed_after_the_installation_advances() {
    let (dir, mut conn, seed) = legacy_at(8).await;
    assert_eq!(version(&mut conn).await.unwrap(), 8);
    assert_eq!(seed.source_version, 8);
    let stale_digest = seed.snapshot_digest;

    // The installation advances to a valid schema 9 the M2 way and retains no
    // further decision, so every frozen schema-8 row survives untouched: the
    // one thing a stale schema-8 seed still matches on is the history the
    // coordinator already validated.
    Migrator::with_migrations(migrator().iter().take(9).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    seed_m2_customer(
        &mut conn,
        &seed.customer,
        &seed.tenant,
        &seed.environment,
        &seed.source,
        &seed.agreement_id,
        seed.effective_at_us,
        &seed.setup_bytes,
    )
    .await;
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    let advanced = preflight_billing(dir.path()).await.unwrap();
    assert_eq!(advanced.version, 9);
    assert_eq!(advanced.store_version, 9);
    assert_eq!(advanced.snapshot.entry_count, 1);
    // The same retained rows read as schema 9 are a different history with a
    // different digest, so the stale seed names a snapshot this installation no
    // longer reports rather than one it merely re-frames.
    assert_ne!(advanced.digest, stale_digest);
    let retained_rows = old_rows(&mut conn).await;
    let retained_scoped = scoped_rows(&mut conn).await;

    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    // Refused atomically: no migration, no source marker, no backfilled index,
    // and not one retained byte changed.
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    assert_eq!(old_rows(&mut conn).await, retained_rows);
    assert_eq!(scoped_rows(&mut conn).await, retained_scoped);
    let m3: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'billing_m3_%'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(m3, 0);
    super::super::connect::integrity(&mut conn).await.unwrap();

    // The same installation upgrades on the seed taken from the schema it is
    // actually at, which is the only reading that can answer for this history.
    let mut current = seed.clone();
    current.source_version = advanced.version;
    current.snapshot_digest = advanced.digest;
    assert_eq!(
        upgrade_billing(dir.path(), &current).await,
        Ok(UpgradeResult::Upgraded)
    );
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    let source_version: i64 =
        sqlx::query_scalar("SELECT source_version FROM billing_m3_upgrade WHERE singleton=1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    // The marker records the schema this store actually came from, not the one
    // the refused seed claimed.
    assert_eq!(source_version, 9);
    assert_eq!(old_rows(&mut conn).await, retained_rows);
    assert_eq!(scoped_rows(&mut conn).await, retained_scoped);
    super::super::connect::integrity(&mut conn).await.unwrap();
}

#[tokio::test]
async fn billing_upgrade_refuses_a_schema9_index_customer_mismatch_atomically() {
    let (dir, mut conn, mut seed) = legacy_at(9).await;
    let before = old_rows(&mut conn).await;
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    seed.index[0].customer = "other-customer".into();

    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    assert_eq!(old_rows(&mut conn).await, before);
    let m3_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name GLOB 'billing_m3_*'",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(m3_tables, 0);
    conn.close().await.unwrap();
}

async fn old_rows(conn: &mut SqliteConnection) -> Vec<Vec<String>> {
    let mut rows = vec![];
    for sql in [
        "SELECT quote(canonical_bytes) FROM billing_setup ORDER BY singleton",
        "SELECT quote(ordinal)||quote(source)||quote(external_id)||quote(semantic_key)||quote(ingress)||quote(facts)||quote(bundle) FROM billing_entries ORDER BY ordinal",
        "SELECT quote(source)||quote(external_id)||quote(ingress)||quote(ordinal) FROM billing_aliases ORDER BY source,external_id",
        "SELECT quote(revision)||quote(canonical_bytes) FROM billing_permissions ORDER BY revision",
    ] {
        rows.push(
            sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *conn)
                .await
                .unwrap(),
        );
    }
    rows
}

/// The scoped schema-9 sidecars, for a store that already has them. Their bytes
/// are as frozen after the schema-10 upgrade as they were before it.
async fn scoped_rows(conn: &mut SqliteConnection) -> Vec<Vec<String>> {
    let mut rows = vec![];
    for sql in [
        "SELECT quote(ordinal)||quote(customer)||quote(source)||quote(external_id)||quote(semantic_key)||quote(ingress)||quote(facts)||quote(bundle)||quote(accepted_at_us)||quote(agreement_id)||quote(agreement_version) FROM billing_m2_entries ORDER BY ordinal",
        "SELECT quote(customer)||quote(source)||quote(external_id)||quote(ingress)||quote(ordinal) FROM billing_m2_aliases ORDER BY customer,source,external_id",
        "SELECT quote(customer)||quote(source)||quote(revision)||quote(canonical_bytes)||quote(recorded_at_us) FROM billing_m2_permissions ORDER BY customer,source,revision",
        "SELECT quote(customer)||quote(source)||quote(revision)||quote(transition)||quote(agreement_id)||quote(agreement_version)||quote(effective_at_us)||quote(recorded_at_us)||quote(setup_bytes) FROM billing_agreements ORDER BY customer,source,revision",
        "SELECT quote(customer)||quote(source)||quote(change_id)||quote(operation)||quote(request)||quote(response)||quote(recorded_at_us) FROM billing_m2_changes ORDER BY customer,source,change_id",
        "SELECT quote(customer)||quote(tenant)||quote(environment) FROM billing_customers ORDER BY customer",
    ] {
        rows.push(
            sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *conn)
                .await
                .unwrap(),
        );
    }
    rows
}

/// Create a real installation directory frozen at schema 8, with the original
/// installation row and the exact canonical setup bytes the billing writer
/// stores. Both public upgrade paths start from this store.
async fn installation(dir: &tempfile::TempDir) -> (Installation, SqliteConnection) {
    let path = dir.path().canonicalize().unwrap().join("billing");
    let data = path.join(".ledger");
    crate::local::private_dir(&path).unwrap();
    crate::local::private_dir(&data).unwrap();

    let setup = ledgerlab_core::canonical::parse(SETUP).unwrap();
    let setup_bytes = CanonicalBytes::from_value(&setup).unwrap().into_vec();
    let config = serde_json::to_vec(&serde_json::json!({
        "schema": "ledger-billing-installation/1",
        "scope": setup["scope"],
        "store_id": setup["store_id"],
    }))
    .unwrap();
    crate::local::write_new(&path.join("billing.json"), &config).unwrap();

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(data.join("local.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    Migrator::with_migrations(migrator().iter().take(8).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            data.join("local.db"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    super::super::write::operation(
        &mut conn,
        &crate::store::records::WriteOp::SeedInstallation(crate::store::records::Installation {
            scope: crate::store::records::Scope {
                tenant: setup["scope"][0].as_str().unwrap().into(),
                environment: setup["scope"][1].as_str().unwrap().into(),
            },
            logical_store_id: setup["store_id"].as_str().unwrap().into(),
            mode: "real".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 0,
        }),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO billing_setup VALUES(1,?)")
        .bind(&setup_bytes)
        .execute(&mut conn)
        .await
        .unwrap();
    (
        Installation {
            path,
            data,
            options,
            setup,
        },
        conn,
    )
}

#[tokio::test]
async fn schema9_billing_refuses_open_until_explicit_upgrade_without_mutation() {
    use crate::billing::BillingLedger;

    let dir = tempfile::tempdir().unwrap();
    let (installation, mut conn) = installation(&dir).await;
    let (path, options, setup) = (
        installation.path.clone(),
        installation.options.clone(),
        installation.setup.clone(),
    );
    Migrator::with_migrations(migrator().iter().take(9).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    let setup_bytes = CanonicalBytes::from_value(&setup).unwrap().into_vec();
    seed_m2_customer(
        &mut conn,
        setup["customer"].as_str().unwrap(),
        setup["scope"][0].as_str().unwrap(),
        setup["scope"][1].as_str().unwrap(),
        setup["source"].as_str().unwrap(),
        setup["agreement"].as_str().unwrap(),
        Timestamp::parse(setup["accepted_at"].as_str().unwrap())
            .unwrap()
            .micros(),
        &setup_bytes,
    )
    .await;
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    let retained_before = (old_rows(&mut conn).await, scoped_rows(&mut conn).await);
    conn.close().await.unwrap();

    let open_error = BillingLedger::open(&path).await;
    assert!(matches!(
        open_error,
        Err(crate::local::LocalError::Service(
            crate::ServiceError::Rejection(code)
        )) if code == "BILLING_UPGRADE_REQUIRED"
    ));

    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    assert_eq!(
        (old_rows(&mut conn).await, scoped_rows(&mut conn).await),
        retained_before,
        "a billing open refusal must not mutate schema-9 history"
    );
    let m3_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name GLOB 'billing_m3_*'",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(m3_tables, 0);
    conn.close().await.unwrap();

    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"upgraded","from_schema":9,"to_schema":10})
    );
    let ledger = BillingLedger::open(&path).await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn invalid_schema10_meter_does_not_report_already_current() {
    use crate::billing::BillingLedger;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("billing");
    BillingLedger::init(&path, SETUP).await.unwrap();
    let db_path = path.join(".ledger/local.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_path)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql("DROP TRIGGER billing_m3_bounds_guard")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("UPDATE billing_m3_bounds SET entry_count=1 WHERE singleton=1")
        .execute(&mut conn)
        .await
        .unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    conn.close().await.unwrap();

    assert!(
        BillingLedger::upgrade(&path).await.is_err(),
        "a failed full schema-10 open must not be reconciled as already current"
    );
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    let count: i64 =
        sqlx::query_scalar("SELECT entry_count FROM billing_m3_bounds WHERE singleton=1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(count, 1, "failed reconciliation leaves the store unchanged");
    conn.close().await.unwrap();
}

#[tokio::test]
async fn stale_schema9_retry_revalidates_concurrent_schema10_upgrade() {
    use crate::maintenance::UpgradeResult;
    use crate::store::sqlite::migrate::{BillingUpgradeIndex, BillingUpgradeSeed};
    use crate::{billing::BillingLedger, service::billing as service};

    let temp = tempfile::tempdir().unwrap();
    let (installation, mut conn) = installation(&temp).await;
    let (path, data, options, setup) = (
        installation.path.clone(),
        installation.data.clone(),
        installation.options.clone(),
        installation.setup.clone(),
    );

    Migrator::with_migrations(migrator().iter().take(9).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    let setup_bytes = CanonicalBytes::from_value(&setup).unwrap().into_vec();
    let accepted_at_us = Timestamp::parse(setup["accepted_at"].as_str().unwrap())
        .unwrap()
        .micros();
    seed_m2_customer(
        &mut conn,
        setup["customer"].as_str().unwrap(),
        setup["scope"][0].as_str().unwrap(),
        setup["scope"][1].as_str().unwrap(),
        setup["source"].as_str().unwrap(),
        setup["agreement"].as_str().unwrap(),
        accepted_at_us,
        &setup_bytes,
    )
    .await;
    conn.close().await.unwrap();

    // This is the original caller's schema-9 preflight and seed. A separate
    // caller will advance the store before this seed reaches its write transaction.
    let preflight = preflight_billing(&data).await.unwrap();
    assert_eq!(preflight.store_version, 9);
    assert_eq!(preflight.version, 9);
    let migration_at = Timestamp::parse("2026-09-26T00:00:00.000000Z").unwrap();
    let validated = service::validate_m2_upgrade(&preflight.snapshot, &migration_at).unwrap();
    let index = service::upgrade_index(&preflight.snapshot)
        .unwrap()
        .into_iter()
        .map(|row| BillingUpgradeIndex {
            ordinal: row.ordinal,
            customer: row.customer,
            source: row.source,
            external_id: row.external_id,
            semantic_key: row.semantic_key,
            target: row.target,
            kind: row.kind,
            accepted_at_us: row.accepted_at_us,
        })
        .collect();
    let seed = BillingUpgradeSeed {
        store_id: validated.store_id.clone(),
        tenant: validated.scope.tenant().to_owned(),
        environment: validated.scope.environment().to_owned(),
        customer: validated.customer.clone(),
        source: validated.source.clone(),
        agreement_id: validated.agreement.clone(),
        effective_at_us: validated.accepted_at.micros(),
        recorded_at_us: migration_at.micros(),
        setup_bytes: preflight.snapshot.setup.clone(),
        snapshot_digest: preflight.digest,
        source_version: preflight.version,
        index,
    };

    // A second upgrader commits schema 10 after the first caller's preflight.
    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"upgraded","from_schema":9,"to_schema":10})
    );
    assert_eq!(
        upgrade_billing(&data, &seed).await,
        Ok(UpgradeResult::AlreadyCurrent)
    );
    assert_eq!(
        BillingLedger::confirm_already_current(&path).await.unwrap(),
        serde_json::json!({"status":"already_current","from_schema":10,"to_schema":10})
    );

    // Migration-layer reconciliation can still return AlreadyCurrent for a
    // damaged M3 meter. The public result path must fully open and validate it.
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql("DROP TRIGGER billing_m3_bounds_guard")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("UPDATE billing_m3_bounds SET entry_count=1 WHERE singleton=1")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();

    assert_eq!(
        upgrade_billing(&data, &seed).await,
        Ok(UpgradeResult::AlreadyCurrent)
    );
    assert!(BillingLedger::confirm_already_current(&path).await.is_err());
}

#[tokio::test]
async fn billing_coordinator_upgrades_valid_m1_history_and_retries_exact_originals() {
    use crate::{billing::BillingLedger, service::billing as service};

    let dir = tempfile::tempdir().unwrap();
    let (installation, mut conn) = installation(&dir).await;
    let (path, data, options, setup) = (
        installation.path.clone(),
        installation.data.clone(),
        installation.options.clone(),
        installation.setup.clone(),
    );

    // Construct the M1 entry through the billing coordinator, then store its
    // validated fields in the original schema-8 table.
    let accepted_at = Timestamp::parse(FIRST_ACCEPTED_AT).unwrap();
    let mut initial = preflight_billing(&data).await.unwrap().snapshot;
    service::validate_legacy_upgrade(&mut initial, &accepted_at).unwrap();
    let customer = setup["customer"].as_str().unwrap();
    let source = setup["source"].as_str().unwrap();
    let submission = service::begin(&initial, customer, source, EVENT, &accepted_at).unwrap();
    let (accepted, plan) =
        service::finish(&initial, &submission, &service::Dedup::empty()).unwrap();
    let plan = plan.unwrap();
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(plan.expected_count(), 0);
    let plan_external = plan.external_id().to_owned();
    sqlx::query("INSERT INTO billing_entries(ordinal,source,external_id,semantic_key,ingress,facts,bundle) VALUES(1,?,?,?,?,?,?)")
        .bind(plan.source())
        .bind(plan.external_id())
        .bind(plan.semantic_key())
        .bind(plan.ingress())
        .bind(plan.facts())
        .bind(plan.bundle())
        .execute(&mut conn)
        .await
        .unwrap();

    let permission = ledgerlab_core::canonical::parse(
        br#"{"schema":"ledger-billing-permissions/1","expected_revision":"1","permissions":["read"],"reason":"retain read and revoke submit"}"#,
    )
    .unwrap();
    let permission_bytes = CanonicalBytes::from_value(&permission).unwrap().into_vec();
    sqlx::query("INSERT INTO billing_permissions(revision,canonical_bytes) VALUES(2,?)")
        .bind(&permission_bytes)
        .execute(&mut conn)
        .await
        .unwrap();

    let preflight = preflight_billing(&data).await.unwrap();
    let original_digest = preflight.digest;
    let mut before = preflight.snapshot;
    service::validate_legacy_upgrade(
        &mut before,
        &Timestamp::parse("2026-09-26T00:00:00.000000Z").unwrap(),
    )
    .unwrap();
    let original_statement = service::statement(&before, customer, None).unwrap();
    let original_receipt = accepted["receipt"].clone();
    assert_eq!(
        original_statement["entries"][0]["receipt"],
        original_receipt
    );
    assert_eq!(version(&mut conn).await.unwrap(), 8);
    let original_rows = old_rows(&mut conn).await;
    conn.close().await.unwrap();

    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"upgraded","from_schema":8,"to_schema":10})
    );
    // A schema-10 store re-derives the frozen digest of the schema it was
    // migrated from, so a lost acknowledgement reconciles against the identical
    // history instead of a differently framed one.
    let migrated = preflight_billing(&data).await.unwrap();
    assert_eq!(migrated.store_version, 10);
    assert_eq!(migrated.version, 8);
    assert_eq!(migrated.digest, original_digest);
    // Repeating the commercial seed reconciles the committed state and runs no
    // further migration.
    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"already_current","from_schema":10,"to_schema":10})
    );
    let ledger = BillingLedger::open(&path).await.unwrap();
    assert_eq!(
        ledger.statement(customer, None).await.unwrap(),
        original_statement
    );
    let retry = ledger.accept(customer, source, EVENT).await.unwrap();
    assert_eq!(retry["status"], "duplicate");
    assert_eq!(retry["receipt"], original_receipt);
    let permission_retry = ledger
        .permissions(customer, source, &permission_bytes)
        .await
        .unwrap();
    assert_eq!(permission_retry["status"], "permissions_updated");
    assert_eq!(permission_retry["revision"], "2");
    assert_eq!(
        ledger.statement(customer, None).await.unwrap(),
        original_statement
    );
    ledger.close().await;

    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    assert_eq!(old_rows(&mut conn).await, original_rows);
    let counts: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM billing_entries),(SELECT count(*) FROM billing_m2_entries),(SELECT count(*) FROM billing_m2_aliases),(SELECT count(*) FROM billing_permissions),(SELECT count(*) FROM billing_m2_permissions),(SELECT count(*) FROM billing_m2_changes),(SELECT count(*) FROM billing_m3_index)",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    // The backfilled index covers the frozen schema-8 decision, and no M3 row
    // was invented for it.
    assert_eq!(counts, (1, 0, 0, 1, 0, 0, 1));
    let indexed: (i64, String, String, String) =
        sqlx::query_as("SELECT ordinal,customer,source,external_id FROM billing_m3_index")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    // The backfilled identity is the frozen decision's own external id, never a
    // literal copied from another fixture.
    assert_eq!(
        indexed,
        (1, customer.into(), source.into(), plan_external.to_string())
    );
}

/// A populated schema-9 installation is the M2 lane's own state, and it must
/// take the same explicit public path a schema-8 source takes. `upgrade`
/// validates the customer registry, agreement and control history, scoped
/// permissions and both retained tiers where they stand, migrates 9 to 10,
/// leaves every frozen and M2 byte untouched, backfills one index row per
/// retained decision, and then answers the original identity and semantic
/// retries with the original receipt after a reopen.
#[tokio::test]
async fn billing_coordinator_upgrades_populated_schema9_history_and_retries_exact_originals() {
    use crate::{billing::BillingLedger, service::billing as service};

    let dir = tempfile::tempdir().unwrap();
    let (installation, mut conn) = installation(&dir).await;
    let (path, data, options, setup) = (
        installation.path.clone(),
        installation.data.clone(),
        installation.options.clone(),
        installation.setup.clone(),
    );
    let customer = setup["customer"].as_str().unwrap().to_owned();
    let source = setup["source"].as_str().unwrap().to_owned();
    let accepted_at = Timestamp::parse(FIRST_ACCEPTED_AT).unwrap();

    // The frozen schema-8 tier an M2 store carries forward, accepted through the
    // coordinator before the schema-9 migration froze that table.
    let mut initial = preflight_billing(&data).await.unwrap().snapshot;
    service::validate_legacy_upgrade(&mut initial, &accepted_at).unwrap();
    let submission = service::begin(&initial, &customer, &source, EVENT, &accepted_at).unwrap();
    let (legacy_accepted, legacy_plan) =
        service::finish(&initial, &submission, &service::Dedup::empty()).unwrap();
    let legacy_plan = legacy_plan.unwrap();
    assert_eq!(legacy_accepted["status"], "accepted");
    sqlx::query("INSERT INTO billing_entries(ordinal,source,external_id,semantic_key,ingress,facts,bundle) VALUES(1,?,?,?,?,?,?)")
        .bind(legacy_plan.source())
        .bind(legacy_plan.external_id())
        .bind(legacy_plan.semantic_key())
        .bind(legacy_plan.ingress())
        .bind(legacy_plan.facts())
        .bind(legacy_plan.bundle())
        .execute(&mut conn)
        .await
        .unwrap();

    // The M2 migration, then exactly the customer and first agreement the M2
    // lane wrote. Nothing about this state is created by the upgrade: the first
    // transition takes the setup's own acceptance time, not a work decision's.
    Migrator::with_migrations(migrator().iter().take(9).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    let setup_bytes = CanonicalBytes::from_value(&setup).unwrap().into_vec();
    let setup_accepted_us = Timestamp::parse(setup["accepted_at"].as_str().unwrap())
        .unwrap()
        .micros();
    seed_m2_customer(
        &mut conn,
        &customer,
        setup["scope"][0].as_str().unwrap(),
        setup["scope"][1].as_str().unwrap(),
        &source,
        setup["agreement"].as_str().unwrap(),
        setup_accepted_us,
        &setup_bytes,
    )
    .await;

    // A second decision on a different target, retained in the tier M2 added.
    // Its work time stays inside the frozen policy window; only its own delivery
    // and operation differ from the frozen decision.
    let second_event = serde_json::to_vec(&serde_json::json!({
        "schema":"ledger-event/1",
        "id":"work-2",
        "operation_id":"operation-2",
        "type":"content.generated",
        "customer":customer,
        "occurred_at":"2026-09-01T00:00:00.000000Z"
    }))
    .unwrap();
    let second_at = Timestamp::parse("2026-09-04T00:00:00.000000Z").unwrap();
    let schema9 = preflight_billing(&data).await.unwrap();
    assert_eq!(schema9.version, 9);
    assert_eq!(schema9.store_version, 9);
    let submission = service::begin(
        &schema9.snapshot,
        &customer,
        &source,
        &second_event,
        &second_at,
    )
    .unwrap();
    let (accepted, plan) =
        service::finish(&schema9.snapshot, &submission, &service::Dedup::empty()).unwrap();
    let plan = plan.unwrap();
    assert_eq!(accepted["status"], "accepted");
    sqlx::query("INSERT INTO billing_m2_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version) VALUES(2,?,?,?,?,?,?,?,?,?,?)")
        .bind(&customer)
        .bind(plan.source())
        .bind(plan.external_id())
        .bind(plan.semantic_key())
        .bind(plan.ingress())
        .bind(plan.facts())
        .bind(plan.bundle())
        .bind(second_at.micros())
        .bind(plan.agreement_id().unwrap())
        .bind(plan.agreement_version().unwrap())
        .execute(&mut conn)
        .await
        .unwrap();

    // The complete pre-upgrade state a schema-9 source must preserve.
    let preflight = preflight_billing(&data).await.unwrap();
    let original_digest = preflight.digest;
    service::validate_m2_upgrade(&preflight.snapshot, &accepted_at).unwrap_err();
    let original_statement = service::statement(&preflight.snapshot, &customer, None).unwrap();
    let original_receipts = vec![
        legacy_accepted["receipt"].clone(),
        accepted["receipt"].clone(),
    ];
    for (entry, receipt) in original_statement["entries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&original_receipts)
    {
        assert_eq!(entry["receipt"], *receipt);
    }
    let original_rights =
        service::permissions::effective_for(&preflight.snapshot, &customer, &source).unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    let original_rows = old_rows(&mut conn).await;
    let original_scoped = scoped_rows(&mut conn).await;
    conn.close().await.unwrap();

    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"upgraded","from_schema":9,"to_schema":10})
    );
    // A schema-10 store re-derives the frozen digest of the schema it was
    // migrated from, so a lost acknowledgement reconciles against the identical
    // history instead of a differently framed one.
    let migrated = preflight_billing(&data).await.unwrap();
    assert_eq!(migrated.store_version, 10);
    assert_eq!(migrated.version, 9);
    assert_eq!(migrated.digest, original_digest);
    // Repeating the same request reconciles the committed state and runs no
    // further migration.
    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"already_current","from_schema":10,"to_schema":10})
    );

    let ledger = BillingLedger::open(&path).await.unwrap();
    assert_eq!(
        ledger.statement(&customer, None).await.unwrap(),
        original_statement
    );
    // The M2 authority and grant chain survive the migration unchanged, so an
    // M2-scoped permission change still answers from its own retained state.
    assert_eq!(
        ledger.permission_status(&customer, &source).await.unwrap()["permissions"],
        serde_json::json!(original_rights.permissions)
    );
    // The frozen tier's own identity and the M2 tier's own identity both return
    // their original receipts, not a re-priced or re-targeted answer.
    for (event, receipt) in [
        (EVENT, &original_receipts[0]),
        (&second_event, &original_receipts[1]),
    ] {
        let retry = ledger.accept(&customer, &source, event).await.unwrap();
        assert_eq!(retry["status"], "duplicate");
        assert_eq!(retry["receipt"], *receipt);
    }
    // A new delivery id over the same operation is a semantic retry: the original
    // receipt again, and no second charge.
    let semantic_retry = serde_json::to_vec(&serde_json::json!({
        "schema":"ledger-event/1",
        "id":"work-1-again",
        "operation_id":"operation-1",
        "type":"content.generated",
        "customer":customer,
        "occurred_at":"2026-09-01T00:00:00.000000Z"
    }))
    .unwrap();
    let semantic = ledger
        .accept(&customer, &source, &semantic_retry)
        .await
        .unwrap();
    assert_eq!(semantic["status"], "duplicate");
    assert_eq!(semantic["kind"], "semantic");
    assert_eq!(semantic["receipt"], original_receipts[0]);
    assert_eq!(
        ledger.statement(&customer, None).await.unwrap(),
        original_statement
    );
    ledger.close().await;

    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    // Neither the frozen schema-8 tier nor the M2 tier changed by a single byte,
    // including the customer's own agreement and control history.
    assert_eq!(old_rows(&mut conn).await, original_rows);
    assert_eq!(scoped_rows(&mut conn).await, original_scoped);
    // One index row per retained decision from both tiers, and the source schema
    // this store was actually migrated from is recorded once.
    let indexed: Vec<(i64, String, String, String, String, String, i64)> = sqlx::query_as(
        "SELECT ordinal,customer,source,external_id,kind,target,accepted_at_us FROM billing_m3_index ORDER BY ordinal",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(
        indexed,
        vec![
            (
                1,
                customer.clone(),
                source.clone(),
                legacy_plan.external_id().to_owned(),
                "base".to_string(),
                legacy_accepted["receipt"]["body"]["target"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                accepted_at.micros(),
            ),
            (
                2,
                customer.clone(),
                source.clone(),
                plan.external_id().to_owned(),
                "base".to_string(),
                accepted["receipt"]["body"]["target"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                second_at.micros(),
            ),
        ]
    );
    let source_version: i64 =
        sqlx::query_scalar("SELECT source_version FROM billing_m3_upgrade WHERE singleton=1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(source_version, 9);
    // The frozen decision stays in its own tier: the upgrade invents no M3 row.
    let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM billing_entries),(SELECT count(*) FROM billing_m2_entries),(SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_m3_aliases),(SELECT count(*) FROM billing_m3_index)",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 0, 1, 2));
    super::super::connect::integrity(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    // The reconciled store reopens on the schema-10 writer path with no upgrade
    // left to run.
    assert_eq!(
        BillingLedger::upgrade(&path).await.unwrap(),
        serde_json::json!({"status":"already_current","from_schema":10,"to_schema":10})
    );
}

#[tokio::test]
async fn billing_upgrade_preserves_original_bytes_and_reconciles_unknown_commit() {
    for cut in [0, 2] {
        let (dir, mut conn, seed) = legacy().await;
        let before = old_rows(&mut conn).await;
        assert!(super::super::SqliteStore::open(dir.path()).await.is_err());
        assert_eq!(version(&mut conn).await.unwrap(), 8);
        if cut == 2 {
            assert_eq!(
                upgrade_billing_connection(&mut conn, &seed, cut).await,
                Err(UpgradeError::OutcomeUnknown)
            );
        } else {
            assert_eq!(
                upgrade_billing(dir.path(), &seed).await,
                Ok(UpgradeResult::Upgraded)
            );
        }
        assert_eq!(version(&mut conn).await.unwrap(), 10);
        assert_eq!(before, old_rows(&mut conn).await);
        // A caller may sample a new upgrade time after losing the original ack;
        // that cannot overwrite the retained first transition time.
        let mut retry = seed.clone();
        retry.recorded_at_us += 1;
        assert_eq!(
            upgrade_billing(dir.path(), &retry).await,
            Ok(UpgradeResult::AlreadyCurrent)
        );
        // An already-current retry must still refuse a foreign index rather than
        // accepting the committed state on trust.
        let mut foreign = seed.clone();
        foreign.index[0].target = "urn:example:other-target".into();
        assert_eq!(
            upgrade_billing(dir.path(), &foreign).await,
            Err(UpgradeError::Refused)
        );
        let retained: i64 = sqlx::query_scalar("SELECT recorded_at_us FROM billing_agreements")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(retained, seed.recorded_at_us);
        let mut wrong = seed.clone();
        wrong.customer = "other-customer".into();
        assert_eq!(
            upgrade_billing(dir.path(), &wrong).await,
            Err(UpgradeError::Refused)
        );
        assert_eq!(before, old_rows(&mut conn).await);
        conn.close().await.unwrap();
        let store = super::super::SqliteStore::open(dir.path()).await.unwrap();
        store.close().await;
    }
}

#[tokio::test]
async fn billing_upgrade_failures_owner_and_partial_states_refuse_atomically() {
    let (dir, mut conn, seed) = legacy().await;
    let before = old_rows(&mut conn).await;
    let owner = super::super::owner::Owner::acquire(dir.path()).unwrap();
    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    drop(owner);
    assert_eq!(
        upgrade_billing_connection(&mut conn, &seed, 1).await,
        Err(UpgradeError::Refused)
    );
    // This read drains the queued rollback left by the injected precommit cut.
    assert_eq!(version(&mut conn).await.unwrap(), 8);
    assert_eq!(before, old_rows(&mut conn).await);
    let added: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'billing_m2_%' OR name='billing_customers' OR name='billing_agreements'").fetch_one(&mut conn).await.unwrap();
    assert_eq!(added, 0);
    for (change, restore) in [
        (
            "UPDATE installation SET admission='frozen'",
            "UPDATE installation SET admission='open'",
        ),
        (
            "UPDATE installation SET dispatch_hold=0",
            "UPDATE installation SET dispatch_hold=1",
        ),
        (
            "UPDATE installation SET dispatch_enabled=1",
            "UPDATE installation SET dispatch_enabled=0",
        ),
        (
            "UPDATE dispatcher_head SET owner='worker',lease_until_us=1",
            "UPDATE dispatcher_head SET owner=NULL,lease_until_us=NULL",
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(change))
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(
            upgrade_billing(dir.path(), &seed).await,
            Err(UpgradeError::Refused)
        );
        sqlx::query(sqlx::AssertSqlSafe(restore))
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(version(&mut conn).await.unwrap(), 8);
        assert_eq!(before, old_rows(&mut conn).await);
    }
    sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=8")
        .execute(&mut conn)
        .await
        .unwrap();
    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=8")
        .bind(migrator().migrations[7].checksum.as_ref())
        .execute(&mut conn)
        .await
        .unwrap();
    // Simulate a schema marker with absent seeding: it must not be accepted as
    // an already-complete upgrade, nor silently repaired.
    let migration = migrator().migrations[8].clone();
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_ref()))
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(9,?,TRUE,?,0)").bind(migration.description.as_ref()).bind(migration.checksum.as_ref()).execute(&mut conn).await.unwrap();
    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Err(UpgradeError::Refused)
    );
    assert_eq!(before, old_rows(&mut conn).await);
    let customers: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_customers")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(customers, 0);
}

/// A store already at schema 9 — the M2 lane's own state, with a frozen
/// schema-8 decision and a schema-9 decision in one contiguous history — must
/// upgrade to schema 10 on its own recorded source version, backfill one index
/// row per frozen decision from both tiers, invent no M3 rows, preserve every
/// frozen byte, and then continue writing in the new tier.
#[tokio::test]
async fn billing_schema9_store_with_both_tiers_upgrades_and_continues_in_m3() {
    let (dir, mut conn, mut seed) = legacy_at(9).await;
    assert_eq!(version(&mut conn).await.unwrap(), 9);
    sqlx::query("INSERT INTO billing_m2_entries VALUES(2,?,?,'delivery-two',x'1112',x'1314',x'1516',x'1718',?,?,1)")
        .bind(&seed.customer)
        .bind(&seed.source)
        .bind(seed.effective_at_us)
        .bind(&seed.agreement_id)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO billing_m2_aliases VALUES(?,?,'alias-two',x'191a',2)")
        .bind(&seed.customer)
        .bind(&seed.source)
        .execute(&mut conn)
        .await
        .unwrap();
    // A combined history is one contiguous ordinal sequence, not two tiers, and
    // the reconciliation seed covers both tiers exactly once.
    seed.index.push(BillingUpgradeIndex {
        ordinal: 2,
        customer: seed.customer.clone(),
        source: seed.source.clone(),
        external_id: "delivery-two".into(),
        semantic_key: vec![0x11, 0x12],
        target: "urn:example:scoped-target".into(),
        kind: "base".into(),
        accepted_at_us: seed.effective_at_us,
    });
    let preflight = preflight_billing(dir.path()).await.unwrap();
    assert_eq!(preflight.version, 9);
    assert_eq!(preflight.snapshot.entry_count, 2);
    assert_eq!(preflight.snapshot.alias_count, 2);
    seed.snapshot_digest = preflight.digest;
    let original_rows = old_rows(&mut conn).await;
    let original_scoped = scoped_rows(&mut conn).await;

    assert_eq!(
        upgrade_billing(dir.path(), &seed).await,
        Ok(UpgradeResult::Upgraded)
    );
    assert_eq!(version(&mut conn).await.unwrap(), 10);
    assert_eq!(old_rows(&mut conn).await, original_rows);
    // The recorded source is the store's own version, so a retry re-derives the
    // same frozen digest from schema 9 and not from schema 8.
    let source_version: i64 =
        sqlx::query_scalar("SELECT source_version FROM billing_m3_upgrade WHERE singleton=1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(source_version, 9);
    let indexed: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT ordinal,customer,source,external_id FROM billing_m3_index ORDER BY ordinal",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(
        indexed,
        vec![
            (
                1,
                seed.customer.clone(),
                seed.source.clone(),
                "delivery".to_string()
            ),
            (
                2,
                seed.customer.clone(),
                seed.source.clone(),
                "delivery-two".to_string()
            ),
        ]
    );
    // The frozen tiers keep their own rows: no decision is copied into M3.
    let invented: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_m3_aliases)",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(invented, (0, 0));

    // The next decision continues in the new tier, starting after every frozen
    // ordinal, and moves the installation-wide guard with it.
    sqlx::query("INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us) VALUES(3,?,?,'delivery-three',x'2122','urn:example:target','base',?)")
        .bind(&seed.customer)
        .bind(&seed.source)
        .bind(seed.effective_at_us)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO billing_m3_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version) VALUES(3,?,?,'delivery-three',x'2122',x'2324',x'2526',x'2728',?,?,1)")
        .bind(&seed.customer)
        .bind(&seed.source)
        .bind(seed.effective_at_us)
        .bind(&seed.agreement_id)
        .execute(&mut conn)
        .await
        .unwrap();
    let bounds: (i64, i64, i64) = sqlx::query_as(
        "SELECT guard,entry_count,alias_count FROM billing_m3_bounds WHERE singleton=1",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    // Three entries and two retained aliases: the guard is every retained
    // decision row from every tier, not just the new one.
    assert_eq!(bounds, (5, 3, 2), "guard is every retained decision row");
    // Frozen bytes and frozen tiers are untouched by the new write.
    assert_eq!(old_rows(&mut conn).await, original_rows);
    assert_eq!(scoped_rows(&mut conn).await, original_scoped);
    super::super::connect::integrity(&mut conn).await.unwrap();
}

#[tokio::test]
async fn billing_schema9_identity_and_immutability_guards() {
    let (dir, mut conn, seed) = legacy().await;
    upgrade_billing(dir.path(), &seed).await.unwrap();
    for sql in [
        "INSERT INTO billing_entries SELECT * FROM billing_entries",
        "INSERT INTO billing_aliases SELECT * FROM billing_aliases",
        "INSERT INTO billing_permissions VALUES(3,x'01')",
        "INSERT OR REPLACE INTO billing_customers SELECT * FROM billing_customers",
        "INSERT OR REPLACE INTO billing_agreements SELECT * FROM billing_agreements",
        "UPDATE billing_customers SET environment='changed'",
        "DELETE FROM billing_agreements",
    ] {
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&mut conn)
                .await
                .is_err(),
            "{sql}"
        );
    }
    sqlx::query("INSERT INTO billing_customers VALUES('second',?,'second-scope')")
        .bind(&seed.tenant)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO billing_agreements VALUES('second',?,1,'agreement-two',1,'start',0,1,x'7b7d')",
    )
    .bind(&seed.source)
    .execute(&mut conn)
    .await
    .unwrap();
    // Cross-customer reuse of legacy external ID and semantic key is allowed.
    sqlx::query("INSERT INTO billing_m2_entries VALUES(2,'second',?,'delivery',x'0102',x'0304',x'0506',x'0708',2,'agreement-two',1)").bind(&seed.source).execute(&mut conn).await.unwrap();
    // Same source under a different customer is not authority over its target.
    assert!(
        sqlx::query("INSERT INTO billing_m2_aliases VALUES('second',?,'alias-new',x'01',1)")
            .bind(&seed.source)
            .execute(&mut conn)
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO billing_m2_aliases VALUES('second',?,'alias',x'01',2)")
        .bind(&seed.source)
        .execute(&mut conn)
        .await
        .unwrap();
    for table in ["billing_m2_entries", "billing_m2_aliases"] {
        let sql = format!("INSERT OR REPLACE INTO {table} SELECT * FROM {table}");
        assert!(sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut conn)
            .await
            .is_err());
    }
    assert!(sqlx::query("INSERT INTO billing_m2_entries VALUES(3,?,?,'delivery',x'0102',x'0304',x'0506',x'0708',3,?,1)").bind(&seed.customer).bind(&seed.source).bind(&seed.agreement_id).execute(&mut conn).await.is_err());
    super::super::connect::integrity(&mut conn).await.unwrap();
}

#[tokio::test]
async fn billing_schema9_retains_global_entry_alias_and_permission_ceilings() {
    let (dir, mut conn, seed) = legacy().await;
    upgrade_billing(dir.path(), &seed).await.unwrap();
    // Legacy rows consume the same installation-wide budgets as M2 rows.
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(2) UNION ALL SELECT i+1 FROM n WHERE i<1000) INSERT INTO billing_m2_entries SELECT i,?,?,printf('new-%d',i),CAST(printf('semantic-%d',i) AS BLOB),x'01',x'02',x'03',i,?,1 FROM n")
        .bind(&seed.customer).bind(&seed.source).bind(&seed.agreement_id)
        .execute(&mut conn).await.unwrap();
    assert!(sqlx::query("INSERT INTO billing_m2_entries VALUES(1001,?,?,'one-too-many',x'ff',x'01',x'02',x'03',1001,?,1)")
        .bind(&seed.customer).bind(&seed.source).bind(&seed.agreement_id)
        .execute(&mut conn).await.is_err());
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(2) UNION ALL SELECT i+1 FROM n WHERE i<1000) INSERT INTO billing_m2_aliases SELECT ?,?,printf('alias-%d',i),x'01',1 FROM n")
        .bind(&seed.customer).bind(&seed.source).execute(&mut conn).await.unwrap();
    assert!(
        sqlx::query("INSERT INTO billing_m2_aliases VALUES(?,?,'one-too-many',x'01',1)")
            .bind(&seed.customer)
            .bind(&seed.source)
            .execute(&mut conn)
            .await
            .is_err()
    );
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(3) UNION ALL SELECT i+1 FROM n WHERE i<1001) INSERT INTO billing_m2_permissions SELECT ?,?,i,x'01',i FROM n")
        .bind(&seed.customer).bind(&seed.source).execute(&mut conn).await.unwrap();
    // Another customer cannot evade the shared permission budget.
    sqlx::query("INSERT INTO billing_customers VALUES('second',?,'second-scope')")
        .bind(&seed.tenant)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO billing_agreements VALUES('second',?,1,'agreement-two',1,'start',0,1,x'7b7d')",
    )
    .bind(&seed.source)
    .execute(&mut conn)
    .await
    .unwrap();
    assert!(
        sqlx::query("INSERT INTO billing_m2_permissions VALUES('second',?,2,x'01',1002)")
            .bind(&seed.source)
            .execute(&mut conn)
            .await
            .is_err()
    );
}
