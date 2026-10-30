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
    let commit_result = tx.commit().await;
    assert!(
        matches!(
            commit_result,
            Err(CommitError::RolledBack(StoreError::BillingHistoryLimit))
        ),
        "unexpected post-mutation capacity result: {commit_result:?}"
    );
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

#[tokio::test]
async fn management_guards_accept_migrated_statement20_and_reject_unknown_schema() {
    use crate::store::sqlite::workflow::financial::FinancialReadError;
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_dir, path, store) = create().await;
    enable(&store).await;
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.workflow_enable_financial_schema().await.unwrap();
    tx.workflow_enable_supplier_schema().await.unwrap();
    tx.workflow_enable_app_schema().await.unwrap();
    tx.workflow_enable_assertion_schema().await.unwrap();
    tx.workflow_enable_resource_schema().await.unwrap();
    tx.workflow_enable_stage_schema().await.unwrap();
    // Retain the previously supported Stage18 positive management checks
    // before adopting Invocation19 and Statement20 in this same real fixture.
    {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert_eq!(tx.billing_schema_version().await.unwrap(), 18);
        let before = tx.workflow_management_changes(&mut session).await.unwrap();
        assert_eq!(tx.workflow_management_sequence(&mut session).await.unwrap(), 1);
        sqlx::query("CREATE TEMP TABLE stage18_management_probe(value INTEGER)")
            .execute(tx.conn()).await.unwrap();
        sqlx::query("INSERT INTO stage18_management_probe VALUES(1)")
            .execute(tx.conn()).await.unwrap();
        tx.workflow_management_commit_guard(Some(before), &mut session).await.unwrap();
        sqlx::query("DROP TABLE stage18_management_probe")
            .execute(tx.conn()).await.unwrap();
    }
    tx.workflow_enable_invocation_schema().await.unwrap();
    tx.workflow_enable_statement_schema().await.unwrap();
    // Both released billing readers remain valid after the additive migration.
    crate::service::billing::validate_snapshot(&tx.billing_snapshot().await.unwrap()).unwrap();
    crate::service::billing::validate_snapshot(&tx.billing_snapshot_for_upgrade().await.unwrap()).unwrap();
    // Enabling an already retained earlier profile must neither downgrade nor
    // rebuild any tables after schema20 has been adopted.
    tx.workflow_enable_profile_b().await.unwrap();
    tx.workflow_enable_financial_schema().await.unwrap();
    tx.workflow_enable_supplier_schema().await.unwrap();
    tx.workflow_enable_app_schema().await.unwrap();
    tx.workflow_enable_assertion_schema().await.unwrap();
    tx.workflow_enable_resource_schema().await.unwrap();
    tx.workflow_enable_stage_schema().await.unwrap();
    tx.workflow_enable_invocation_schema().await.unwrap();
    tx.workflow_enable_statement_schema().await.unwrap();
    tx.commit().await.unwrap();
    for (version, sequence) in [(20i64, true), (21, true), (21, false)] {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut tx = store
            .begin_with_resource_session(deadline(), &mut session)
            .await
            .unwrap();
        assert_eq!(tx.billing_schema_version().await.unwrap(), 20);
        let before = tx.workflow_management_changes(&mut session).await.unwrap();
        if version == 21 {
            sqlx::query("PRAGMA user_version=21")
                .execute(tx.conn())
                .await
                .unwrap();
        }
        if sequence {
            let result = tx.workflow_management_sequence(&mut session).await;
            if version == 20 {
                assert_eq!(result.unwrap(), 1);
            } else {
                assert!(matches!(
                    result,
                    Err(FinancialReadError::Store(
                        StoreError::BillingUpgradeRequired
                    ))
                ));
            }
        }
        if version == 20 || !sequence {
            // Connection-local temporary data changes total_changes(), exercising
            // the real post-mutation capacity/version branch without appending
            // an economic or administrative operation to the migrated store.
            sqlx::query("CREATE TEMP TABLE management_guard_probe(value INTEGER)")
                .execute(tx.conn())
                .await
                .unwrap();
            sqlx::query("INSERT INTO management_guard_probe VALUES(1)")
                .execute(tx.conn())
                .await
                .unwrap();
            let result = tx
                .workflow_management_commit_guard(Some(before), &mut session)
                .await;
            if version == 20 {
                result.unwrap();
            } else {
                assert!(matches!(
                    result,
                    Err(FinancialReadError::Store(StoreError::InvalidStore(
                        "unsupported guarded workflow schema"
                    )))
                ));
            }
        }
        tx.rollback().await.unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&store.inner.writer)
            .await
            .unwrap(),
        20
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_operations")
            .fetch_one(&store.inner.writer)
            .await
            .unwrap(),
        0
    );
    store.close().await;
    let reopened = BillingLedger::open(&path).await.unwrap();
    reopened.close().await;
}

impl SqliteTx {
    /// Fixed physical controls after the genuine Work append and owning replay.
    /// These rejected SQL writes never serve as economic or authority inputs.
    pub(crate) async fn test_invocation_revision_storage_controls(
        &mut self,
        invocation_id: &str,
        store: &str,
        scope: &[String; 2],
    ) {
        let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT digest,schema_name,canonical_bytes FROM workflow_documents WHERE kind='workflow-invocation' AND id=? ORDER BY digest",
        ).bind(invocation_id).fetch_all(self.conn()).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.1 == "ledger-workflow-invocation/1"));
        let bodies: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::from_slice(&r.2).unwrap())
            .collect();
        let first = bodies.iter().position(|v| v["revision"] == "1").unwrap();
        let second = bodies.iter().position(|v| v["revision"] == "2").unwrap();
        assert_eq!(bodies[second]["prior_ref"]["id"], invocation_id);
        assert_eq!(bodies[second]["prior_ref"]["digest"], rows[first].0);
        let bytes: i64 = sqlx::query_scalar("SELECT sum(octet_length(canonical_bytes)) FROM workflow_documents WHERE kind='workflow-invocation' AND id=?")
            .bind(invocation_id).fetch_one(self.conn()).await.unwrap();
        assert_eq!(bytes as usize, rows[0].2.len() + rows[1].2.len());
        let ordinary:(String,String,String,String,Vec<u8>)=sqlx::query_as("SELECT kind,id,digest,schema_name,canonical_bytes FROM workflow_documents WHERE kind<>'workflow-invocation' ORDER BY id LIMIT 1")
            .fetch_one(self.conn()).await.unwrap();
        let mut changed = rows[first].2.clone();
        changed.push(b' ');
        let controls = [
            (
                "workflow-invocation",
                invocation_id,
                rows[first].0.as_str(),
                "ledger-workflow-invocation/1",
                changed.as_slice(),
                "immutable workflow full reference",
            ),
            (
                ordinary.0.as_str(),
                ordinary.1.as_str(),
                rows[second].0.as_str(),
                ordinary.3.as_str(),
                ordinary.4.as_slice(),
                "immutable workflow document identity",
            ),
            (
                ordinary.0.as_str(),
                invocation_id,
                rows[second].0.as_str(),
                ordinary.3.as_str(),
                ordinary.4.as_slice(),
                "immutable workflow document identity",
            ),
            (
                "workflow-invocation",
                ordinary.1.as_str(),
                rows[second].0.as_str(),
                "ledger-workflow-invocation/1",
                rows[second].2.as_slice(),
                "immutable workflow document identity",
            ),
            (
                "workflow-invocation",
                invocation_id,
                ordinary.2.as_str(),
                "ledger-workflow-invocation/2",
                rows[second].2.as_slice(),
                "immutable workflow document identity",
            ),
        ];
        for (kind, id, digest, schema, body, expected) in controls {
            let error=sqlx::query("INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) VALUES(?,?,?,?,?)")
                .bind(kind).bind(id).bind(digest).bind(schema).bind(body).execute(self.conn()).await.unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(self.conn())
                .await
                .unwrap()
                .is_empty()
        );
        super::workflow::capacity::check(self.conn(), 0, 0)
            .await
            .unwrap();
        let error = self
            .workflow_document_by_id(invocation_id, store, &scope[0], &scope[1])
            .await
            .err()
            .expect("ambiguous ID must refuse");
        assert!(matches!(
            error,
            StoreError::Integrity("ambiguous workflow document id")
        ));
        // That intentional failed read poisons this attempt. Only rollback
        // follows; no guard is reset to continue financial execution.
    }
}
