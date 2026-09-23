//! Real populated legacy files, built from original migration bytes and frozen
//! independent rows. No current-schema store is downgraded to simulate age.
use super::*;
use crate::{
    maintenance::*,
    store::{
        records::WriteOp,
        sqlite::{self, tests},
    },
};
use sqlx::{AssertSqlSafe, Connection};

async fn dump(conn: &mut SqliteConnection) -> Vec<(String, Vec<String>)> {
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' AND name<>'_sqlx_migrations' ORDER BY name").fetch_all(&mut *conn).await.unwrap();
    let mut rows = vec![];
    for table in tables {
        let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE NOT (?='dispatcher_head' AND name='revision') ORDER BY cid")
            .bind(&table).bind(&table).fetch_all(&mut *conn).await.unwrap();
        let expr = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join("||'|'||");
        let values = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT {expr} FROM {table} ORDER BY 1"
        )))
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        rows.push((table, values));
    }
    rows
}
async fn legacy(v: usize) -> (tempfile::TempDir, SqliteConnection) {
    let dir = tempfile::tempdir().unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.path().join("local.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
    let migrations = migrator().iter().take(v).cloned().collect::<Vec<_>>();
    Migrator::with_migrations(migrations)
        .run(&mut conn)
        .await
        .unwrap();
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut installation = tests::installation();
    installation.admission = "frozen".into();
    for op in std::iter::once(WriteOp::SeedInstallation(installation))
        .chain(tests::seed())
        .chain(tests::schedule())
    {
        sqlite::write::operation(&mut tx, &op).await.unwrap();
    }
    // Retain a terminal outcome from the old implementation across the upgrade.
    sqlx::query("UPDATE delivery_state SET state='rejected', attempts=20")
        .execute(&mut *tx)
        .await
        .unwrap();
    if v >= 2 {
        sqlx::query("UPDATE dispatcher_head SET revision=7")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    (dir, conn)
}

#[tokio::test]
async fn sqlite_schema_upgrade_populated_1_through_4_rollback_reopen_retry() {
    for (v, lose_ack) in [
        (1, false),
        (1, true),
        (2, false),
        (2, true),
        (3, false),
        (3, true),
        (4, false),
        (4, true),
    ] {
        let (dir, mut conn) = legacy(v).await;
        let before = dump(&mut conn).await;
        assert!(sqlite::SqliteStore::open(dir.path()).await.is_err());
        assert_eq!(dump(&mut conn).await, before, "open must not migrate");

        let owner_guard = sqlite::owner::Owner::acquire(dir.path()).unwrap();
        assert_eq!(
            upgrade_sqlite(dir.path(), "store-demo-slice").await,
            Err(UpgradeError::Refused)
        );
        drop(owner_guard);
        for (set, restore) in [
            (
                "UPDATE installation SET dispatch_hold=0",
                "UPDATE installation SET dispatch_hold=1",
            ),
            (
                "UPDATE installation SET dispatch_enabled=1",
                "UPDATE installation SET dispatch_enabled=0",
            ),
            (
                "UPDATE dispatcher_head SET owner='old-worker', lease_until_us=1",
                "UPDATE dispatcher_head SET owner=NULL, lease_until_us=NULL",
            ),
        ] {
            sqlx::query(AssertSqlSafe(set))
                .execute(&mut conn)
                .await
                .unwrap();
            assert_eq!(
                upgrade_sqlite(dir.path(), "store-demo-slice").await,
                Err(UpgradeError::Refused)
            );
            sqlx::query(AssertSqlSafe(restore))
                .execute(&mut conn)
                .await
                .unwrap();
        }
        // Fail migration 3 after migration 2, or migration 4 after its first
        // two tables. DDL, user_version and history must roll back together.
        let collision_table = if v == 4 {
            "r3_segments"
        } else if v == 3 {
            "outcome_deliveries"
        } else {
            "delivery_quarantines"
        };
        sqlx::query(AssertSqlSafe(format!(
            "CREATE TABLE {collision_table} (collision INTEGER) STRICT"
        )))
        .execute(&mut conn)
        .await
        .unwrap();
        let collision = dump(&mut conn).await;
        assert_eq!(
            upgrade_sqlite(dir.path(), "store-demo-slice").await,
            Err(UpgradeError::Refused)
        );
        assert_eq!(version(&mut conn).await.unwrap(), v as i64);
        assert_eq!(dump(&mut conn).await, collision);
        sqlx::query(AssertSqlSafe(format!("DROP TABLE {collision_table}")))
            .execute(&mut conn)
            .await
            .unwrap();

        assert_eq!(
            upgrade_sqlite(dir.path(), "wrong-store").await,
            Err(UpgradeError::Refused)
        );
        sqlx::query("UPDATE installation SET admission='open'")
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(
            upgrade_sqlite(dir.path(), "store-demo-slice").await,
            Err(UpgradeError::Refused)
        );
        sqlx::query("UPDATE installation SET admission='frozen'")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=1")
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(
            upgrade_sqlite(dir.path(), "store-demo-slice").await,
            Err(UpgradeError::Refused)
        );
        sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=1")
            .bind(migrator().migrations[0].checksum.as_ref())
            .execute(&mut conn)
            .await
            .unwrap();
        assert_eq!(dump(&mut conn).await, before);
        if lose_ack {
            assert_eq!(
                upgrade_connection(&mut conn, "store-demo-slice", true).await,
                Err(UpgradeError::OutcomeUnknown)
            );
        }
        conn.close().await.unwrap();
        if !lose_ack {
            assert_eq!(
                upgrade_sqlite(dir.path(), "store-demo-slice").await,
                Ok(UpgradeResult::Upgraded)
            );
        }
        // Repeating after an absent caller acknowledgement is safe and resolves
        // the history instead of replaying any economic write or DDL.
        assert_eq!(
            upgrade_sqlite(dir.path(), "store-demo-slice").await,
            Ok(UpgradeResult::AlreadyCurrent)
        );
        let mut conn = SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("local.db")),
        )
        .await
        .unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM dispatcher_head")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(revision, if v >= 2 { 7 } else { 0 });
        let after = dump(&mut conn).await;
        for row in before {
            assert!(after.contains(&row), "changed retained table {}", row.0);
        }
        assert!(sqlx::query("UPDATE delivery_state SET state='pending'")
            .execute(&mut conn)
            .await
            .is_err());
        assert!(sqlx::query("UPDATE delivery_state SET attempts=0")
            .execute(&mut conn)
            .await
            .is_err());
        // Owner explicitly reopens admission only; dispatch remains held.
        sqlx::query("UPDATE installation SET admission='open'")
            .execute(&mut conn)
            .await
            .unwrap();
        let stable = dump(&mut conn).await;
        let ledger = crate::Ledger::open_sqlite(dir.path()).await.unwrap();
        assert_original_retry(&ledger).await;
        assert_eq!(dump(&mut conn).await, stable, "no duplicate economics");
        ledger.close().await;
        conn.close().await.unwrap();
    }
}

#[tokio::test]
async fn sqlite_r3_reader_bound_upgrade_validates_old_keys_and_guards_new_keys() {
    for malformed in [false, true] {
        let (dir, mut conn) = legacy(5).await;
        let kind = if malformed {
            "unbounded-kind".repeat(100)
        } else {
            "AUTHORITY".into()
        };
        let zero = 0u128.to_be_bytes();
        let hash = "a".repeat(64);
        sqlx::query("INSERT INTO r3_journals VALUES(x'01',x'7b7d',?,?,?)")
            .bind(zero.as_slice())
            .bind(&hash)
            .bind(&hash)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO r3_segments VALUES(x'01',?,?,?,2,1)")
            .bind(zero.as_slice())
            .bind(&hash)
            .bind(&hash)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO r3_objects VALUES(x'01',?,?,x'7b7d',x'01',?,2,x'7b7d')")
            .bind(zero.as_slice())
            .bind(&kind)
            .bind(&hash)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO r3_object_pages VALUES(x'01',x'7b7d',?,x'01',?,0,x'7b7d')")
            .bind(&kind)
            .bind(&hash)
            .execute(&mut conn)
            .await
            .unwrap();
        sqlite::connect::integrity(&mut conn).await.unwrap();
        let before = dump(&mut conn).await;
        let result = upgrade_sqlite(dir.path(), "store-demo-slice").await;
        assert_eq!(
            result,
            if malformed {
                Err(UpgradeError::Refused)
            } else {
                Ok(UpgradeResult::Upgraded)
            }
        );
        assert_eq!(
            version(&mut conn).await.unwrap(),
            if malformed { 5 } else { 8 }
        );
        let after = dump(&mut conn).await;
        for row in &before {
            assert!(
                after.contains(row),
                "additive migration changed retained table {}",
                row.0
            );
        }
        let added: Vec<_> = after
            .iter()
            .filter(|row| !before.iter().any(|old| old.0 == row.0))
            .collect();
        if malformed {
            assert!(added.is_empty());
        } else {
            assert_eq!(
                added.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
                [
                    "billing_aliases",
                    "billing_entries",
                    "billing_permissions",
                    "billing_setup"
                ]
            );
            assert!(
                added.iter().all(|r| r.1.is_empty()),
                "migration cannot seed billing history"
            );
        }
        if !malformed {
            for sql in [
                "INSERT INTO r3_objects SELECT journal,ordinal,'unknown-kind',origin,full_key,body_hash,byte_length,metadata FROM r3_objects",
                "INSERT INTO r3_object_pages SELECT journal,origin,'unknown-kind',full_key,body_hash,page,bytes FROM r3_object_pages",
            ] {
                let error = sqlx::query(AssertSqlSafe(sql)).execute(&mut conn).await.unwrap_err();
                assert!(error.to_string().contains("unbounded R3 fact kind"), "{error}");
            }
            assert_eq!(dump(&mut conn).await, after);
            assert_eq!(
                upgrade_sqlite(dir.path(), "store-demo-slice").await,
                Ok(UpgradeResult::AlreadyCurrent)
            );
        }
        conn.close().await.unwrap();
    }
}
