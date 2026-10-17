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
    tx.execute(sqlx::AssertSqlSafe(FINANCIAL_WORKFLOW))
        .await
        .unwrap();
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    // These opaque physical witnesses test the exact admitted kind/FK only;
    // they are not constructor outputs or authenticated financial history.
    c.execute("INSERT INTO workflow_documents VALUES('workflow-dependency','dep','digest','ledger-workflow-dependency/1',X'01'),('workflow-capacity-retirement','retirement','digest','ledger-workflow-capacity-retirement/1',X'02'),('workflow-target','target','digest','opaque',X'03'),('workflow-slot-terms','terms','digest','opaque',X'04'),('workflow-correction-capacity','capacity','digest','opaque',X'05')").await.unwrap();
    c.execute("INSERT INTO workflow_financial_dependencies VALUES('store','tenant','environment','ancestor','dependent','outcome_family','workflow-target','target','digest','family','workflow-slot-terms','terms','digest','workflow-correction-capacity','capacity','digest','workflow-dependency','dep','digest')").await.unwrap();
    let dependency: String =
        sqlx::query_scalar("SELECT record_kind FROM workflow_financial_dependencies")
            .fetch_one(&mut c)
            .await
            .unwrap();
    assert_eq!(dependency, "workflow-dependency");
    assert!(c.execute("INSERT INTO workflow_documents VALUES('workflow-unknown-financial','unknown','digest','opaque',X'01')").await.is_err());
    assert!(c.execute("INSERT INTO workflow_financial_dependencies VALUES('store','tenant','environment','other','dependent','outcome_family','workflow-target','target','digest','family','workflow-slot-terms','terms','digest','workflow-correction-capacity','capacity','digest','workflow-capacity-retirement','retirement','digest')").await.is_err());
}

#[tokio::test]
async fn supplier_additive_schema_preserves_history_and_distinct_global_claims() {
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
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    // Physical schema witness only: these opaque bytes are not financial
    // originals, authentication, economic values or successful admission.
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(super::SUPPLIER_WORKFLOW))
        .await
        .unwrap();
    tx.execute("INSERT INTO workflow_documents VALUES('workflow-cost','cost1','digest','opaque',X'01'),('workflow-cost','cost2','digest','opaque',X'02'),('workflow-invocation','inv','digest','opaque',X'03'),('workflow-authority','authority1','digest','opaque',X'04'),('workflow-authority','activation','digest','opaque',X'05')").await.unwrap();
    tx.execute("INSERT INTO workflow_supplier_account_enrollments VALUES('provider','account','production','workflow-authority','authority1','digest','workflow-authority','activation','digest')").await.unwrap();
    assert!(tx.execute("INSERT INTO workflow_supplier_account_enrollments VALUES('provider','account','production','workflow-authority','activation','digest','workflow-authority','activation','digest')").await.is_err());
    tx.execute("INSERT INTO workflow_supplier_cost_line_claims VALUES('authority','provider','account','production','document','line','workflow-cost','cost1','digest')").await.unwrap();
    // A different invocation/scope cannot reclaim an installation-owned line;
    // those fields deliberately do not participate in this key.
    assert!(tx.execute("INSERT INTO workflow_supplier_cost_line_claims VALUES('authority','provider','account','production','document','line','workflow-cost','cost2','digest')").await.is_err());
    tx.execute("INSERT INTO workflow_supplier_cost_component_claims VALUES('store','tenant','environment','original-work','supplier-slot','workflow-cost','cost1','digest')").await.unwrap();
    assert!(tx.execute("INSERT INTO workflow_supplier_cost_component_claims VALUES('store','tenant','environment','original-work','supplier-slot','workflow-cost','cost2','digest')").await.is_err());
    tx.execute("INSERT INTO workflow_supplier_cost_component_claims VALUES('store','tenant','other','original-work','supplier-slot','workflow-cost','cost2','digest')").await.unwrap();
    tx.execute("INSERT INTO workflow_financial_heads VALUES('store','tenant','environment','invocation','invocation-id','','','','1','workflow-invocation','inv','digest')").await.unwrap();
    tx.execute("INSERT INTO workflow_financial_heads VALUES('store','tenant','environment','cost','workflow-cost','cost1','digest','','1','workflow-cost','cost1','digest')").await.unwrap();
    assert!(tx.execute("INSERT INTO workflow_financial_heads VALUES('store','tenant','environment','cost_consumption','workflow-cost','cost1','digest','','1','workflow-authority','authority1','digest')").await.is_err());
    assert!(tx.execute("INSERT INTO workflow_documents VALUES('workflow-unknown','bad','digest','opaque',X'01')").await.is_err());
    assert!(
        tx.execute("DELETE FROM workflow_supplier_cost_line_claims")
            .await
            .is_err()
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
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut c)
            .await
            .unwrap()
            .is_none()
    );
}

// Opaque physical records here prove only migration/FK/uniqueness mechanics;
// none is offered to a semantic auditor as financial provenance.
#[tokio::test]
async fn app_upgrade_preserves_history_and_enforces_independent_global_consumption() {
    let mut c = prior().await;
    for schema in [FINANCIAL_WORKFLOW, super::SUPPLIER_WORKFLOW] {
        let mut tx = c.begin().await.unwrap();
        tx.execute(sqlx::AssertSqlSafe(schema)).await.unwrap();
        finish_financial_schema(&mut tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let before: (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT operation_bytes,manifest_bytes,receipt_bytes FROM workflow_operations",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    let mut tx = c.begin().await.unwrap();
    tx.execute(sqlx::AssertSqlSafe(super::APP_WORKFLOW))
        .await
        .unwrap();
    finish_financial_schema(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let after: (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT operation_bytes,manifest_bytes,receipt_bytes FROM workflow_operations",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(before, after);
    c.execute("INSERT INTO workflow_documents VALUES('workflow-work','work','w','opaque',X'01'),('workflow-fact-attestation','obs','o','opaque',X'01'),('workflow-fact-attestation','obs2','o2','opaque',X'01'),('workflow-fact-attestation','evidence','e','opaque',X'01'),('workflow-cost','claim','c','opaque',X'01'),('workflow-cost','consume','s','opaque',X'01'),('workflow-verification','consent','v','opaque',X'01')").await.unwrap();
    for sql in [
        "INSERT INTO workflow_app_call_claims VALUES('store','namespace','call','workflow-fact-attestation','obs','o')",
        "INSERT INTO workflow_app_component_observations VALUES('store','workflow-work','work','w','component','workflow-fact-attestation','obs','o')",
        "INSERT INTO workflow_app_provider_calls VALUES('store','provider','account','request','workflow-fact-attestation','obs','o')",
        "INSERT INTO workflow_app_cost_claims VALUES('store','workflow-work','work','w','component','workflow-cost','claim','c')",
        "INSERT INTO workflow_app_evidence_consumptions VALUES('store','workflow-fact-attestation','evidence','e','workflow-cost','consume','s')",
        "INSERT INTO workflow_app_observation_revisions VALUES('store','workflow-fact-attestation','obs','o','1','workflow-cost','consume','s')",
        "INSERT INTO workflow_financial_heads VALUES('store','tenant','environment','app_cost','workflow-cost','claim','c','','1','workflow-cost','claim','c')",
    ] {
        c.execute(sql).await.unwrap();
    }
    // Distinct immutable results cannot reuse any occupied permanent key.
    for table in [
        "workflow_app_call_claims",
        "workflow_app_component_observations",
        "workflow_app_provider_calls",
    ] {
        let sql = format!(
            "INSERT INTO {table} SELECT {} FROM {table}",
            match table {
                "workflow_app_call_claims" =>
                    "logical_store_id,source_namespace,call_id,'workflow-fact-attestation','obs2','o2'",
                "workflow_app_component_observations" =>
                    "logical_store_id,work_kind,work_id,work_digest,component_slot,'workflow-fact-attestation','obs2','o2'",
                _ =>
                    "logical_store_id,provider,account,request_id,'workflow-fact-attestation','obs2','o2'",
            }
        );
        assert!(c.execute(sqlx::AssertSqlSafe(sql.as_str())).await.is_err());
    }
    for table in [
        "workflow_app_cost_claims",
        "workflow_app_evidence_consumptions",
        "workflow_app_observation_revisions",
    ] {
        let sql = format!("UPDATE {table} SET record_id='consume'");
        assert!(c.execute(sqlx::AssertSqlSafe(sql.as_str())).await.is_err());
    }
    assert!(c.execute("INSERT INTO workflow_app_call_claims VALUES('store','namespace','different','workflow-fact-attestation','missing','missing')").await.is_err());
    assert!(
        c.execute(
            "INSERT INTO workflow_documents VALUES('unknown-kind','bad','bad','opaque',X'01')"
        )
        .await
        .is_err()
    );
    c.execute("INSERT INTO workflow_documents VALUES('workflow-authority','party-one','p1','opaque',X'01'),('workflow-authority','party-two','p2','opaque',X'02')").await.unwrap();
    c.execute("INSERT INTO workflow_party_authorization_heads VALUES('store','tenant','environment','customer','agreement','mapping','1','workflow-authority','party-one','p1')").await.unwrap();
    assert!(
        c.execute("UPDATE workflow_party_authorization_heads SET authorization_id='other'")
            .await
            .is_err()
    );
    assert!(
        c.execute("UPDATE workflow_party_authorization_heads SET revision='3'")
            .await
            .is_err()
    );
    c.execute("UPDATE workflow_party_authorization_heads SET revision='2',record_id='party-two',record_digest='p2'").await.unwrap();
    assert!(
        c.execute("DELETE FROM workflow_party_authorization_heads")
            .await
            .is_err()
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut c)
            .await
            .unwrap()
            .is_none()
    );
}
