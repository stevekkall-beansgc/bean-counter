//! Storage qualification uses original v1-v8 migration bytes, never a current
//! database with its version number rewritten to pretend it is old.
use super::*;
use crate::maintenance::{UpgradeError, UpgradeResult};
use ledgerlab_core::{canonical::CanonicalBytes, domain::Timestamp};
use sqlx::{Connection, SqliteConnection};

async fn legacy() -> (tempfile::TempDir, SqliteConnection, BillingUpgradeSeed) {
    let dir = tempfile::tempdir().unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.path().join("local.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    Migrator::with_migrations(migrator().iter().take(8).cloned().collect())
        .run(&mut conn)
        .await
        .unwrap();
    let value = ledgerlab_core::canonical::parse(include_bytes!(
        "../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    let setup_bytes = CanonicalBytes::from_value(&value).unwrap().into_vec();
    let seed = BillingUpgradeSeed {
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
    (dir, conn, seed)
}

async fn old_rows(conn: &mut SqliteConnection) -> Vec<Vec<String>> {
    let mut rows = vec![];
    for sql in [
        "SELECT quote(canonical_bytes) FROM billing_setup ORDER BY singleton",
        "SELECT quote(ordinal)||quote(source)||quote(external_id)||quote(semantic_key)||quote(ingress)||quote(facts)||quote(bundle) FROM billing_entries ORDER BY ordinal",
        "SELECT quote(source)||quote(external_id)||quote(ingress)||quote(ordinal) FROM billing_aliases ORDER BY source,external_id",
        "SELECT quote(revision)||quote(canonical_bytes) FROM billing_permissions ORDER BY revision",
    ] {
        rows.push(sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).fetch_all(&mut *conn).await.unwrap());
    }
    rows
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
        assert_eq!(version(&mut conn).await.unwrap(), 9);
        assert_eq!(before, old_rows(&mut conn).await);
        // A caller may sample a new upgrade time after losing the original ack;
        // that cannot overwrite the retained first transition time.
        let mut retry = seed.clone();
        retry.recorded_at_us += 1;
        assert_eq!(
            upgrade_billing(dir.path(), &retry).await,
            Ok(UpgradeResult::AlreadyCurrent)
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
