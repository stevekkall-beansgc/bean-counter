//! Opt-in real legacy databases; no schema downgrade or fake persistence.
use super::*;
use crate::{
    maintenance::*,
    store::{records::WriteOp, sqlite::tests},
    PostgresConfig, PostgresTrust,
};

const ROLE: &str = "ledgerlab_phase1_runtime";
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
async fn dump(client: &Client) -> Vec<(String, Vec<String>)> {
    let tables = client.query("SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname='ledgerlab' AND tablename<>'migration_history' ORDER BY tablename", &[]).await.unwrap();
    let mut result = vec![];
    for row in tables {
        let table: String = row.get(0);
        assert!(table.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
        let projection = if table == "dispatcher_head" {
            "(to_jsonb(t)-'revision')::text"
        } else {
            "row_to_json(t)::text"
        };
        let rows = client
            .query(
                &format!("SELECT {projection} FROM ledgerlab.{table} t ORDER BY 1"),
                &[],
            )
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.get(0))
            .collect();
        result.push((table, rows));
    }
    result
}

#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL 17/18 TLS test database"]
async fn postgres_schema_upgrade_populated_1_and_2_rollback_reopen_retry() {
    for (v, lose_ack) in [(1_i64, false), (1, true), (2, false), (2, true)] {
        let database = format!(
            "ledgerlab_upgrade_{}_{}_{}",
            std::process::id(),
            v,
            lose_ack
        );
        let admin = config("ledgerlab", "postgres").connect().await.unwrap();
        admin
            .client
            .batch_execute(&format!("CREATE DATABASE {database}"))
            .await
            .unwrap();
        let owner_config = config(&database, "postgres");
        let runtime_config = config(&database, ROLE);
        let mut owner = owner_config.connect().await.unwrap();
        let tx = owner.client.transaction().await.unwrap();
        tx.batch_execute(SQL).await.unwrap();
        tx.execute(
            "INSERT INTO ledgerlab.migration_history VALUES (1,$1)",
            &[&checksum()],
        )
        .await
        .unwrap();
        if v == 2 {
            tx.batch_execute(OUTBOX).await.unwrap();
            tx.execute(
                "INSERT INTO ledgerlab.migration_history VALUES (2,$1)",
                &[&outbox_checksum()],
            )
            .await
            .unwrap();
        }
        let mut installation = tests::installation();
        installation.admission = "frozen".into();
        for op in std::iter::once(WriteOp::SeedInstallation(installation))
            .chain(tests::seed())
            .chain(tests::schedule())
        {
            super::super::write::operation(&tx, &op).await.unwrap();
        }
        tx.batch_execute(&format!("REVOKE CREATE ON SCHEMA public FROM PUBLIC; GRANT USAGE ON SCHEMA ledgerlab TO {ROLE}; GRANT SELECT ON ALL TABLES IN SCHEMA ledgerlab TO {ROLE}; GRANT INSERT ON ledgerlab.delivery_keys TO {ROLE}; UPDATE ledgerlab.delivery_state SET state='rejected', attempts=20;")).await.unwrap();
        grant_base_runtime(&tx, ROLE).await.unwrap();
        if v == 2 {
            tx.batch_execute("UPDATE ledgerlab.dispatcher_head SET revision=7")
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        let before = dump(&owner.client).await;
        assert!(crate::Ledger::open_postgres(runtime_config.clone())
            .await
            .is_err());
        assert_eq!(dump(&owner.client).await, before);
        assert_eq!(
            upgrade_postgres(runtime_config.clone(), "store-demo-slice", ROLE).await,
            Err(UpgradeError::Refused)
        );
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "wrong-store", ROLE).await,
            Err(UpgradeError::Refused)
        );
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", "postgres").await,
            Err(UpgradeError::Refused)
        );

        for (set, restore) in [
            (
                "UPDATE ledgerlab.installation SET dispatch_hold=0",
                "UPDATE ledgerlab.installation SET dispatch_hold=1",
            ),
            (
                "UPDATE ledgerlab.installation SET dispatch_enabled=1",
                "UPDATE ledgerlab.installation SET dispatch_enabled=0",
            ),
            (
                "UPDATE ledgerlab.dispatcher_head SET owner='old-worker', lease_until_us=1",
                "UPDATE ledgerlab.dispatcher_head SET owner=NULL, lease_until_us=NULL",
            ),
        ] {
            owner.client.batch_execute(set).await.unwrap();
            assert_eq!(
                upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
                Err(UpgradeError::Refused)
            );
            owner.client.batch_execute(restore).await.unwrap();
        }
        owner
            .client
            .batch_execute("UPDATE ledgerlab.installation SET admission='open'")
            .await
            .unwrap();
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
            Err(UpgradeError::Refused)
        );
        owner.client.batch_execute("UPDATE ledgerlab.installation SET admission='frozen'; UPDATE ledgerlab.migration_history SET checksum='corrupt' WHERE version=1").await.unwrap();
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
            Err(UpgradeError::Refused)
        );
        owner
            .client
            .execute(
                "UPDATE ledgerlab.migration_history SET checksum=$1 WHERE version=1",
                &[&checksum()],
            )
            .await
            .unwrap();

        owner.client.batch_execute("ALTER TABLE ledgerlab.migration_history ALTER COLUMN checksum TYPE BYTEA USING convert_to(checksum,'UTF8')").await.unwrap();
        assert!(
            verify(&owner.client).await.is_err(),
            "malformed metadata must return an error, not panic"
        );
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
            Err(UpgradeError::Refused)
        );
        owner.client.batch_execute("ALTER TABLE ledgerlab.migration_history ALTER COLUMN checksum TYPE TEXT USING convert_from(checksum,'UTF8')").await.unwrap();

        owner
            .client
            .batch_execute("CREATE TABLE ledgerlab.delivery_quarantines (collision INTEGER)")
            .await
            .unwrap();
        let collision = dump(&owner.client).await;
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
            Err(UpgradeError::Refused)
        );
        assert_eq!(version(&owner.client).await.unwrap(), v);
        assert_eq!(dump(&owner.client).await, collision, "DDL/history rollback");
        owner
            .client
            .batch_execute("DROP TABLE ledgerlab.delivery_quarantines")
            .await
            .unwrap();

        if lose_ack {
            assert_eq!(
                upgrade_client(&mut owner.client, "store-demo-slice", ROLE, true).await,
                Err(UpgradeError::OutcomeUnknown)
            );
        } else {
            assert_eq!(
                upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
                Ok(UpgradeResult::Upgraded)
            );
        }
        owner.discard().await;
        let owner = owner_config.connect().await.unwrap();
        assert_eq!(
            upgrade_postgres(owner_config.clone(), "store-demo-slice", ROLE).await,
            Ok(UpgradeResult::AlreadyCurrent)
        );
        let revision: i64 = owner
            .client
            .query_one("SELECT revision FROM ledgerlab.dispatcher_head", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(revision, if v == 2 { 7 } else { 0 });
        let after = dump(&owner.client).await;
        for row in before {
            assert!(after.contains(&row), "changed retained table {}", row.0);
        }
        let runtime = runtime_config.connect().await.unwrap();
        let grants: (bool, bool, bool, bool) = {
            let r = runtime.client.query_one("SELECT has_table_privilege(current_user,'ledgerlab.delivery_quarantines','SELECT'), has_table_privilege(current_user,'ledgerlab.delivery_quarantines','INSERT'), has_table_privilege(current_user,'ledgerlab.delivery_quarantines','UPDATE'), has_table_privilege(current_user,'ledgerlab.delivery_quarantines','DELETE')", &[]).await.unwrap();
            (r.get(0), r.get(1), r.get(2), r.get(3))
        };
        assert_eq!(grants, (true, true, false, false));
        assert!(runtime
            .client
            .batch_execute("UPDATE ledgerlab.delivery_state SET state='pending'")
            .await
            .is_err());
        assert!(runtime
            .client
            .batch_execute("UPDATE ledgerlab.delivery_state SET attempts=0")
            .await
            .is_err());
        runtime.discard().await;
        owner
            .client
            .batch_execute("UPDATE ledgerlab.installation SET admission='open'")
            .await
            .unwrap();
        let stable = dump(&owner.client).await;
        let ledger = crate::Ledger::open_postgres(runtime_config).await.unwrap();
        assert_original_retry(&ledger).await;
        assert_eq!(dump(&owner.client).await, stable, "no duplicate economics");
        ledger.close().await;
        owner.discard().await;
        admin
            .client
            .batch_execute(&format!("DROP DATABASE {database}"))
            .await
            .unwrap();
        admin.discard().await;
    }
}
