//! Mechanical migration/constraint tests, not authenticated financial histories.
//! Opaque fixture bytes deliberately carry no semantic/origin qualification.
use super::{FINANCIAL_WORKFLOW, WORKFLOW, finish_financial_schema};
use sqlx::{Connection, Executor, SqliteConnection, sqlite::SqliteConnectOptions};

async fn prior() -> SqliteConnection {
    let mut c = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(version, crate::store::sqlite::connect::SQLITE_VERSION);
    c.execute(sqlx::AssertSqlSafe(WORKFLOW)).await.unwrap();
    c.execute("INSERT INTO workflow_documents VALUES('workflow-publication','pub','digest','opaque',X'010203'),('workflow-compiled-plan','plan','digest','opaque',X'040506'),('workflow-policy','policy','digest','opaque',X'070809')").await.unwrap();
    c.execute("INSERT INTO workflow_operations VALUES(X'01','store','tenant','environment','source','operation','op','digest',X'001122','request','manifest','digest',X'334455','receipt','digest',X'667788','pub','plan',1,123,'policy.publish')").await.unwrap();
    c.execute("INSERT INTO workflow_manifest_members VALUES(X'01','workflow-publication','pub','digest'); INSERT INTO workflow_feed VALUES(1,X'01'); INSERT INTO workflow_publications VALUES(X'02','pub','plan','policy'); INSERT INTO workflow_definition_registry VALUES('store','tenant','environment','source','workflow-policy','policy','1','policy')").await.unwrap();
    c
}

#[tokio::test]
async fn financial_upgrade_preserves_opaque_original_rows_and_references() {
    let mut c = prior().await;
    let before: (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT operation_bytes,manifest_bytes,receipt_bytes FROM workflow_operations",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW))
        .await
        .unwrap();
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *tx)
            .await
            .unwrap()
            .is_none()
    );
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let after: (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT operation_bytes,manifest_bytes,receipt_bytes FROM workflow_operations",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(before, after);
    let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_feed f JOIN workflow_operations o ON o.identity_key=f.identity_key JOIN workflow_manifest_members m ON m.identity_key=o.identity_key JOIN workflow_documents d ON d.kind=m.kind AND d.id=m.id AND d.digest=m.digest")
        .fetch_one(&mut c).await.unwrap();
    assert_eq!(linked, 1);
    assert!(c.execute("DELETE FROM workflow_operations").await.is_err());
    assert!(
        c.execute("UPDATE workflow_documents SET canonical_bytes=X'00'")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn financial_upgrade_rollback_restores_old_closed_schema_and_bytes() {
    let mut c = prior().await;
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW))
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let sidecars: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'workflow_financial_%'",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(sidecars, 0);
    let original: Vec<u8> = sqlx::query_scalar("SELECT operation_bytes FROM workflow_operations")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(original, [0x00, 0x11, 0x22]);
    assert!(c.execute("INSERT INTO workflow_documents VALUES('workflow-decision','financial','digest','opaque',X'01')").await.is_err());
}

#[tokio::test]
async fn financial_outcome_indices_enforce_both_permanence_and_cross_agreement_token() {
    let mut c = prior().await;
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW))
        .await
        .unwrap();
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    c.execute("INSERT INTO workflow_documents VALUES('workflow-outcome-claim','claim1','digest','opaque',X'01'),('workflow-outcome-claim','claim2','digest','opaque',X'02'),('workflow-outcome-claim','claim3','digest','opaque',X'03')").await.unwrap();
    c.execute("INSERT INTO workflow_financial_outcome_claims VALUES('store','tenant','environment','agreementA','family','targetA','namespace','token','workflow-outcome-claim','claim1','digest')").await.unwrap();
    // Different agreement/target cannot reuse the namespace-family token.
    assert!(c.execute("INSERT INTO workflow_financial_outcome_claims VALUES('store','tenant','environment','agreementB','family','targetB','namespace','token','workflow-outcome-claim','claim2','digest')").await.is_err());
    // A new token cannot claim an already permanently selected target-family.
    assert!(c.execute("INSERT INTO workflow_financial_outcome_claims VALUES('store','tenant','environment','agreementA','family','targetA','namespace','token2','workflow-outcome-claim','claim3','digest')").await.is_err());
    c.execute("INSERT INTO workflow_financial_outcome_claims VALUES('store','tenant','other','agreementB','family','targetB','namespace','token','workflow-outcome-claim','claim2','digest')").await.unwrap();
    assert!(
        c.execute("DELETE FROM workflow_financial_outcome_claims")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn financial_upgrade_cannot_reset_a_real_missing_foreign_key() {
    let mut c = prior().await;
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW))
        .await
        .unwrap();
    tx.execute("INSERT INTO workflow_publications VALUES(X'03','missing','plan','policy')")
        .await
        .unwrap();
    assert!(finish_financial_schema(&mut tx).await.is_err());
    let still_deferred: i64 = sqlx::query_scalar("PRAGMA defer_foreign_keys")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(still_deferred, 1);
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_publications")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn financial_dependency_and_retirement_kinds_have_closed_physical_storage() {
    let mut c = prior().await;
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW)).await.unwrap();
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    // These opaque physical witnesses test the exact admitted kind/FK only;
    // they are not constructor outputs or authenticated financial history.
    c.execute("INSERT INTO workflow_documents VALUES('workflow-dependency','dep','digest','ledger-workflow-dependency/1',X'01'),('workflow-capacity-retirement','retirement','digest','ledger-workflow-capacity-retirement/1',X'02'),('workflow-target','target','digest','opaque',X'03'),('workflow-slot-terms','terms','digest','opaque',X'04'),('workflow-correction-capacity','capacity','digest','opaque',X'05')").await.unwrap();
    c.execute("INSERT INTO workflow_financial_dependencies VALUES('store','tenant','environment','ancestor','dependent','outcome_family','workflow-target','target','digest','family','workflow-slot-terms','terms','digest','workflow-correction-capacity','capacity','digest','workflow-dependency','dep','digest')").await.unwrap();
    let dependency: String = sqlx::query_scalar("SELECT record_kind FROM workflow_financial_dependencies")
        .fetch_one(&mut c).await.unwrap();
    assert_eq!(dependency,"workflow-dependency");
    assert!(c.execute("INSERT INTO workflow_documents VALUES('workflow-unknown-financial','unknown','digest','opaque',X'01')").await.is_err());
    assert!(c.execute("INSERT INTO workflow_financial_dependencies VALUES('store','tenant','environment','other','dependent','outcome_family','workflow-target','target','digest','family','workflow-slot-terms','terms','digest','workflow-correction-capacity','capacity','digest','workflow-capacity-retirement','retirement','digest')").await.is_err());
}
