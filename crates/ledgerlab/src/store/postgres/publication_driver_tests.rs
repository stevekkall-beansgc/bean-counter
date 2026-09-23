//! Real driver/owner integration; this does not grant physical admission.
use super::*;
use crate::store::{
    outcomes::{OutcomeLock, OutcomeLockClass, OutcomeLockMode, OutcomeTx},
    ports::AcceptanceTx,
};
use ledgerlab_core::adjudication as r3;
use serde_json::json;
fn config(database: &str, owner: bool) -> PostgresConfig {
    PostgresConfig {
        host: "localhost".into(),
        port: std::env::var("LEDGERLAB_PG_TEST_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        user: if owner {
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
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn state(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path.join("state.json")).unwrap()
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL17/18 TLS and trusted test directory"]
async fn postgres_publication_driver_bootstrap_legacy_reads_and_guard_mutations() {
    let name = format!("ledgerlab_publication_{}", std::process::id());
    let admin = config("ledgerlab", true).connect().await.unwrap();
    admin
        .client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .unwrap();
    admin.discard().await;
    let mut sql_owner = config(&name, true).connect().await.unwrap();
    let mut install = crate::store::sqlite::tests::installation();
    install.admission = "frozen".into();
    migrate::create(
        &mut sql_owner.client,
        install.clone(),
        "ledgerlab_phase1_runtime",
    )
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap().keep();
    eprintln!(
        "retained publication fixture database={name} anchor={}",
        directory.display()
    );
    let unfenced = PostgresStore::open(config(&name, false)).await.unwrap();
    let mut stale = unfenced.begin(deadline()).await.unwrap();
    let owner = bootstrap::bind(config(&name, true), &directory, &install.logical_store_id)
        .await
        .unwrap();
    let before = state(&directory);
    let document = crate::store::sqlite::tests::seed().remove(0);
    assert!(
        stale.write(&document).await.is_err(),
        "old unbound snapshot must not write after binding"
    );
    stale.rollback().await.unwrap();
    assert!(unfenced.begin(deadline()).await.is_err());
    unfenced.close().await;
    assert!(PostgresStore::open(config(&name, false)).await.is_err());
    assert_eq!(state(&directory), before);
    assert!(
        migrate::upgrade(
            config(&name, true),
            &install.logical_store_id,
            "ledgerlab_phase1_runtime"
        )
        .await
        .is_err(),
        "unfenced maintenance cannot bypass a bound owner"
    );
    let store = PostgresStore::open_with_owner(config(&name, false), Some(owner))
        .await
        .unwrap();
    // Two actual read snapshots can coexist and commit without witness churn.
    let (a, b) = tokio::join!(store.begin(deadline()), store.begin(deadline()));
    let mut a = a.unwrap();
    let mut b = b.unwrap();
    assert_eq!(a.load_installation().await.unwrap(), install);
    assert_eq!(b.load_installation().await.unwrap(), install);
    a.commit().await.unwrap();
    b.commit().await.unwrap();
    assert_eq!(state(&directory), before);
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.write(&document).await.unwrap();
    tx.commit().await.unwrap();
    let after_document = state(&directory);
    assert_ne!(before, after_document);
    let count: i64 = sql_owner
        .client
        .query_one("SELECT count(*) FROM ledgerlab.documents", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
    let guard = OutcomeLock {
        class: OutcomeLockClass::Admission,
        key: r3::canonical_bytes(
            &json!([[install.scope.tenant, install.scope.environment]]),
            4096,
        )
        .unwrap(),
        mode: OutcomeLockMode::Read,
    };
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.lock_scopes(std::slice::from_ref(&guard)).await.unwrap();
    tx.commit().await.unwrap();
    let after_guard = state(&directory);
    assert_ne!(
        after_document, after_guard,
        "new read guard is a real retained insertion"
    );
    let (a, b) = tokio::join!(store.begin(deadline()), store.begin(deadline()));
    let mut a = a.unwrap();
    let mut b = b.unwrap();
    let (ra, rb) = tokio::join!(
        a.lock_scopes(std::slice::from_ref(&guard)),
        b.lock_scopes(std::slice::from_ref(&guard))
    );
    ra.unwrap();
    rb.unwrap();
    a.commit().await.unwrap();
    b.commit().await.unwrap();
    assert_eq!(
        state(&directory),
        after_guard,
        "existing read guards must not mutate or publish"
    );
    // Rollback discards even real inserts without advancing the publication.
    let mut other = guard.clone();
    other.class = OutcomeLockClass::Target;
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.lock_scopes(&[other]).await.unwrap();
    tx.rollback().await.unwrap();
    store.close().await;
    let reopened = PostgresStore::open_fenced(config(&name, false), &directory)
        .await
        .unwrap();
    assert_eq!(state(&directory), after_guard);
    let mut tx = reopened.begin(deadline()).await.unwrap();
    assert_eq!(tx.load_installation().await.unwrap(), install);
    tx.rollback().await.unwrap();
    reopened.close().await;
    // Actual same-database witness rollback simulates the identifying projection
    // of a stale restore. It is only a witness refusal control, not a full backup.
    let old: serde_json::Value = serde_json::from_slice(&before).unwrap();
    sql_owner
        .client
        .execute(
            "UPDATE ledgerlab.r3_commit_witness SET witness=$1 WHERE singleton=1",
            &[&old["publication"]["witness"].as_str().unwrap()],
        )
        .await
        .unwrap();
    assert!(PostgresStore::open_fenced(config(&name, false), &directory)
        .await
        .is_err());
    assert_eq!(state(&directory), after_guard);
    let latest: serde_json::Value = serde_json::from_slice(&after_guard).unwrap();
    sql_owner
        .client
        .execute(
            "UPDATE ledgerlab.r3_commit_witness SET witness=$1 WHERE singleton=1",
            &[&latest["publication"]["witness"].as_str().unwrap()],
        )
        .await
        .unwrap();
    PostgresStore::open_fenced(config(&name, false), &directory)
        .await
        .unwrap()
        .close()
        .await;
    sql_owner.discard().await;
    eprintln!("actual publication driver: bound bootstrap, stale unbound handles, legacy write, two concurrent pure readers, read guard insert/no-op/rollback, reopen and stale witness refusal PASS; no physical or full backup proof");
}
