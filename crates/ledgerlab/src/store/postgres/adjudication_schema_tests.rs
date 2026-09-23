//! Actual PostgreSQL schema probes, not an R3 commit-capability or physical proof.
use super::*;
use crate::{store::sqlite::tests::installation, PostgresConfig, PostgresTrust};
use tokio_postgres::types::ToSql;
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
async fn refused(tx: &tokio_postgres::Transaction<'_>, sql: &str, args: &[&(dyn ToSql + Sync)]) {
    tx.batch_execute("SAVEPOINT negative").await.unwrap();
    let result = tx.execute(sql, args).await;
    assert!(result.is_err(), "invalid schema operation was accepted");
    tx.batch_execute("ROLLBACK TO SAVEPOINT negative; RELEASE SAVEPOINT negative")
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL17/18 TLS test database"]
async fn postgres_r3_schema_maximum_keys_counters_and_immutable_projections() {
    let database = format!("ledgerlab_r3_schema_{}", std::process::id());
    let admin = config("ledgerlab", "postgres").connect().await.unwrap();
    admin
        .client
        .batch_execute(&format!("CREATE DATABASE {database}"))
        .await
        .unwrap();
    admin.discard().await;
    let mut owner = config(&database, "postgres").connect().await.unwrap();
    let major: i32 = std::env::var("LEDGERLAB_PG_TEST_MAJOR")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        super::super::verify_server(&owner.client).await.unwrap() / 10000,
        major
    );
    create(&mut owner.client, installation(), ROLE)
        .await
        .unwrap();
    verify(&owner.client).await.unwrap();
    let tx = owner.client.transaction().await.unwrap();
    let j = vec![1u8; 1115];
    let key = vec![2u8; 1115];
    let origin = vec![3u8; 2048];
    let object_key = vec![4u8; 4096];
    let delivery = vec![5u8; 4096];
    let page = vec![6u8; 4096];
    let body = vec![7u8; 8 * 1024 * 1024];
    let zero = 0u128.to_be_bytes().to_vec();
    let one = 1u128.to_be_bytes().to_vec();
    let two = 2u128.to_be_bytes().to_vec();
    let hash = "a".repeat(64);
    tx.execute(
        "INSERT INTO ledgerlab.r3_journals VALUES($1,$2,$3,$4,$4)",
        &[&j, &b"{}".as_slice(), &zero, &hash],
    )
    .await
    .unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_segments VALUES($1,$2,$3,$3,8388608,2048)",
        &[&j, &one, &hash],
    )
    .await
    .unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_segment_pages VALUES($1,$2,2047,$3)",
        &[&j, &one, &page],
    )
    .await
    .unwrap();
    tx.execute("INSERT INTO ledgerlab.r3_objects(journal,ordinal,kind,origin,full_key,body_hash,byte_length,metadata) VALUES($1,$2,'ORIGINAL_BASE',$3,$4,$5,262144,$6)", &[&j,&one,&origin,&object_key,&hash,&vec![8u8;8192]]).await.unwrap();
    tx.execute("INSERT INTO ledgerlab.r3_object_pages(journal,origin,kind,full_key,body_hash,page,bytes) VALUES($1,$2,'ORIGINAL_BASE',$3,$4,63,$5)", &[&j,&origin,&object_key,&hash,&page]).await.unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_heads VALUES($1,17,$2,$3,$4)",
        &[&j, &key, &zero, &body],
    )
    .await
    .unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_head_versions VALUES($1,17,$2,$3,$3,$4)",
        &[&j, &key, &one, &body],
    )
    .await
    .unwrap();
    tx.execute("INSERT INTO ledgerlab.r3_commands(journal,delivery,ordinal,command,result,receipt) VALUES($1,$2,$3,$4,$5,$6)", &[&j,&delivery,&one,&vec![9u8;262144],&body,&vec![10u8;8192]]).await.unwrap();
    let saved: Vec<u8> = tx.query_one("SELECT result FROM ledgerlab.r3_commands WHERE journal=$1 AND delivery_hash=sha256($2) AND delivery=$2", &[&j,&delivery]).await.unwrap().get(0);
    assert_eq!(saved, body);
    let stored:Vec<u8>=tx.query_one("SELECT bytes FROM ledgerlab.r3_object_pages WHERE journal=$1 AND origin_hash=sha256($2) AND origin=$2 AND key_hash=sha256($3) AND full_key=$3 AND kind='ORIGINAL_BASE' AND body_hash=$4 AND page=63", &[&j,&origin,&object_key,&hash]).await.unwrap().get(0);
    assert_eq!(stored, page);
    refused(
        &tx,
        "UPDATE ledgerlab.r3_heads SET revision=$1 WHERE journal=$2",
        &[&two, &j],
    )
    .await;
    tx.execute(
        "UPDATE ledgerlab.r3_heads SET revision=$1 WHERE journal=$2",
        &[&one, &j],
    )
    .await
    .unwrap();
    refused(
        &tx,
        "UPDATE ledgerlab.r3_journals SET ordinal=$1 WHERE journal=$2",
        &[&two, &j],
    )
    .await;
    tx.execute(
        "UPDATE ledgerlab.r3_journals SET ordinal=$1 WHERE journal=$2",
        &[&one, &j],
    )
    .await
    .unwrap();
    let max = ledgerlab_core::adjudication::types::Count::MAX;
    let predecessor = (max - 1).to_be_bytes().to_vec();
    let maximum = max.to_be_bytes().to_vec();
    let successor: Vec<u8> = tx
        .query_one("SELECT ledgerlab.r3_successor($1)", &[&predecessor])
        .await
        .unwrap()
        .get(0);
    assert_eq!(successor, maximum);
    refused(&tx, "SELECT ledgerlab.r3_successor($1)", &[&maximum]).await;
    refused(
        &tx,
        "INSERT INTO ledgerlab.r3_heads VALUES($1,16,$2,$3,$4)",
        &[
            &j,
            &key,
            &(max + 1).to_be_bytes().to_vec(),
            &b"{}".as_slice(),
        ],
    )
    .await;
    for table in [
        "r3_segments",
        "r3_segment_pages",
        "r3_objects",
        "r3_object_pages",
        "r3_head_versions",
        "r3_commands",
        "r3_namespaces",
        "r3_deliveries",
        "r3_index_pages",
        "r3_index_roots",
        "r3_held_intentions",
    ] {
        refused(&tx, &format!("DELETE FROM ledgerlab.{table}"), &[]).await;
        refused(&tx, &format!("TRUNCATE ledgerlab.{table} CASCADE"), &[]).await;
    }
    let tag = "b".repeat(32);
    tx.execute(
        "INSERT INTO ledgerlab.r3_namespaces VALUES('synthetic','sandbox',$1,'g1',$2)",
        &[&tag, &j],
    )
    .await
    .unwrap();
    let external = format!("gw1.{tag}.occupied");
    refused(&tx,"INSERT INTO ledgerlab.delivery_keys(tenant,environment,source,external_id,ingress_hash,canonical_event_id,kind,observed_us,canonical_bytes,content_hash,schema_version) VALUES('synthetic','sandbox','source',$1,$2,$3,'alias',0,$4,$2,1)",&[&external,&format!("sha256:{hash}"),&format!("ev_{hash}"),&b"{}".as_slice()]).await;
    tx.execute(
        "INSERT INTO ledgerlab.r3_deliveries VALUES('synthetic','sandbox','source',$1,$2,$3)",
        &[&external, &j, &b"{}".as_slice()],
    )
    .await
    .unwrap();
    refused(&tx,"INSERT INTO ledgerlab.acceptance_delivery_namespace VALUES('synthetic','sandbox','source',$1,'v1')",&[&external]).await;
    tx.rollback().await.unwrap();
    let idle = owner.client.query_one("SELECT generation,state,backend_pid IS NULL AND backend_start IS NULL AND journal IS NULL AND delivery IS NULL AND command_hash IS NULL FROM ledgerlab.r3_unresolved_work WHERE singleton=1", &[]).await.unwrap();
    assert_eq!(idle.get::<_, Vec<u8>>(0), zero);
    assert_eq!(idle.get::<_, String>(1), "IDLE");
    assert!(idle.get::<_, bool>(2));
    owner.discard().await;
    let store = super::super::PostgresStore::open(config(&database, ROLE))
        .await
        .unwrap();
    store.close().await;
    let mut runtime = config(&database, ROLE).connect().await.unwrap();
    let tx = runtime.client.transaction().await.unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_scope_locks VALUES($1,9,$2)",
        &[&j, &key],
    )
    .await
    .unwrap();
    tx.query_one("SELECT full_key FROM ledgerlab.r3_scope_locks WHERE journal=$1 AND class=9 AND full_key=$2 FOR UPDATE", &[&j,&key]).await.unwrap();
    refused(
        &tx,
        "UPDATE ledgerlab.r3_scope_locks SET full_key=full_key",
        &[],
    )
    .await;
    // CHECK must be FALSE, never UNKNOWN, for each omitted resolving identity.
    const RESOLVING: &str = "UPDATE ledgerlab.r3_unresolved_work SET state='RESOLVING',backend_pid=pg_backend_pid(),backend_start=(SELECT backend_start FROM pg_stat_activity WHERE pid=pg_backend_pid()),journal=$1,delivery=$2,command_hash=$3 WHERE singleton=1";
    for missing in 0..3 {
        let journal = (missing != 0).then_some(j.as_slice());
        let delivery_key = (missing != 1).then_some(delivery.as_slice());
        let command_hash = (missing != 2).then_some(hash.as_str());
        tx.batch_execute("SAVEPOINT missing_identity")
            .await
            .unwrap();
        let error = tx
            .execute(RESOLVING, &[&journal, &delivery_key, &command_hash])
            .await
            .unwrap_err();
        assert_eq!(
            error.as_db_error().unwrap().code(),
            &tokio_postgres::error::SqlState::CHECK_VIOLATION,
            "missing identity field {missing}"
        );
        tx.batch_execute(
            "ROLLBACK TO SAVEPOINT missing_identity; RELEASE SAVEPOINT missing_identity",
        )
        .await
        .unwrap();
    }
    assert_eq!(
        tx.execute(RESOLVING, &[&j, &delivery, &hash])
            .await
            .unwrap(),
        1
    );
    let retained: (String, Vec<u8>, Vec<u8>, String) = {
        let row = tx.query_one("SELECT state,journal,delivery,command_hash FROM ledgerlab.r3_unresolved_work WHERE singleton=1", &[]).await.unwrap();
        (row.get(0), row.get(1), row.get(2), row.get(3))
    };
    assert_eq!(retained, ("RESOLVING".into(), j, delivery, hash));
    tx.rollback().await.unwrap();
    runtime.discard().await;
    eprintln!("actual PostgreSQL{major}: schema5/create+runtime reopen, maximum compact/full keys, 8MiB readback, M boundary/exact successor, immutable projections and shared namespace guards PASS; no R3 capability/physical proof");
}

async fn inventory(client: &Client) -> Vec<(String, Vec<String>)> {
    let mut out = vec![];
    for row in client.query("SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname='ledgerlab' ORDER BY tablename", &[]).await.unwrap() {
        let table:String=row.get(0);
        assert!(table.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'_'));
        let rows=client.query(&format!("SELECT row_to_json(t)::text FROM ledgerlab.{table} t ORDER BY 1"),&[]).await.unwrap().into_iter().map(|r|r.get(0)).collect();
        out.push((table,rows));
    }
    out
}
#[tokio::test]
#[ignore = "requires explicit isolated PostgreSQL17/18 TLS test database"]
async fn postgres_r3_upgrade_from_populated_v4_rolls_back_and_preserves_old_records() {
    use crate::maintenance::UpgradeResult;
    let database = format!("ledgerlab_r3_upgrade_{}", std::process::id());
    let admin = config("ledgerlab", "postgres").connect().await.unwrap();
    admin
        .client
        .batch_execute(&format!("CREATE DATABASE {database}"))
        .await
        .unwrap();
    admin.discard().await;
    let owner_config = config(&database, "postgres");
    let mut owner = owner_config.connect().await.unwrap();
    let tx = owner.client.transaction().await.unwrap();
    for (n, sql, hash) in [
        (1i64, SQL, checksum()),
        (2, OUTBOX, outbox_checksum()),
        (3, SAFETY, safety_checksum()),
        (4, OUTCOMES, outcomes_checksum()),
    ] {
        tx.batch_execute(sql).await.unwrap();
        tx.execute(
            "INSERT INTO ledgerlab.migration_history VALUES($1,$2)",
            &[&n, &hash],
        )
        .await
        .unwrap();
    }
    let mut install = installation();
    install.admission = "frozen".into();
    for op in std::iter::once(crate::store::records::WriteOp::SeedInstallation(install))
        .chain(crate::store::sqlite::tests::seed())
        .chain(crate::store::sqlite::tests::schedule())
    {
        super::super::write::operation(&tx, &op).await.unwrap();
    }
    tx.batch_execute("REVOKE CREATE ON SCHEMA public FROM PUBLIC; ALTER TABLE ledgerlab.migration_history ADD CONSTRAINT test_stop_next_version CHECK(version<5)").await.unwrap();
    grant_base_runtime(&tx, ROLE).await.unwrap();
    grant_outcomes(&tx, ROLE).await.unwrap();
    tx.commit().await.unwrap();
    let before = inventory(&owner.client).await;
    assert!(upgrade(owner_config.clone(), "store-demo-slice", ROLE)
        .await
        .is_err());
    assert_eq!(version(&owner.client).await.unwrap(), 4);
    assert_eq!(
        inventory(&owner.client).await,
        before,
        "failed upgrade rolls back all new DDL/data/history"
    );
    owner
        .client
        .batch_execute(
            "ALTER TABLE ledgerlab.migration_history DROP CONSTRAINT test_stop_next_version",
        )
        .await
        .unwrap();
    assert_eq!(
        upgrade(owner_config.clone(), "store-demo-slice", ROLE)
            .await
            .unwrap(),
        UpgradeResult::Upgraded
    );
    assert_eq!(
        upgrade(owner_config, "store-demo-slice", ROLE)
            .await
            .unwrap(),
        UpgradeResult::AlreadyCurrent
    );
    let after = inventory(&owner.client).await;
    for (table, rows) in before {
        if table != "migration_history" {
            assert!(after.contains(&(table, rows)), "retained table changed");
        }
    }
    assert_eq!(version(&owner.client).await.unwrap(), 5);
    owner.discard().await;
    let store = super::super::PostgresStore::open(config(&database, ROLE))
        .await
        .unwrap();
    store.close().await;
    eprintln!("actual populated v4→v5: forced upgrade rollback, successful upgrade, identical old records, idempotent retry and runtime reopen PASS");
}
