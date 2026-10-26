//! Opt-in actual local financial service tests. Run from a captured test build
//! with LEDGER_WORKFLOW_TEST_FIXTURE naming a quiescent maintained synthetic
//! runner output (RUN.json, main-work.request.json, installation/).
//! Only a private temporary copy is opened; no raw financial append is used.
use super::*;
use sqlx::{sqlite::SqliteConnectOptions, AssertSqlSafe, Connection, SqliteConnection};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn copy_private_tree(source: &Path, destination: &Path) {
    let metadata = fs::symlink_metadata(source).unwrap();
    assert!(
        !metadata.file_type().is_symlink(),
        "synthetic fixture must not contain symlinks"
    );
    if metadata.is_dir() {
        fs::create_dir(destination).unwrap();
        fs::set_permissions(destination, metadata.permissions()).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            copy_private_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    } else {
        assert!(
            metadata.is_file(),
            "regular synthetic fixture file required"
        );
        fs::copy(source, destination).unwrap();
        fs::set_permissions(destination, metadata.permissions()).unwrap();
    }
}
fn fixture(case: &str) -> (tempfile::TempDir, PathBuf, Vec<u8>, OperationKey, Vec<u8>) {
    assert!(
        crate::build_identity::embedded_inputs().is_ok(),
        "requires captured build inputs"
    );
    let root = PathBuf::from(
        std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE")
            .expect("explicit maintained synthetic fixture required"),
    );
    let run: Value = serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(run["classification"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("synthetic"));
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("installation"), &path);
    let config = fs::read(path.join("billing.json")).unwrap();
    let operation =
        std::env::var("LEDGER_WORKFLOW_TEST_OPERATION").unwrap_or_else(|_| "main-work".into());
    assert!(matches!(
        operation.as_str(),
        "main-work" | "main-quantity-correct"
    ));
    let mut request: Value =
        serde_json::from_slice(&fs::read(root.join(format!("{operation}.request.json"))).unwrap())
            .unwrap();
    let tag = format!(
        "local10-{case}-{}",
        temporary.path().file_name().unwrap().to_str().unwrap()
    );
    if operation == "main-work" {
        assert_eq!(request["kind"], "work.accept");
        assert_eq!(request["payload"]["schema"], "ledger-workflow-work/1");
        for field in ["external_event_id", "business_operation_id", "chain"] {
            request["payload"][field] = json!(tag);
        }
        request["payload"]["occurred_at"] = json!(crate::local::now().unwrap().as_str());
    } else {
        assert_eq!(request["kind"], "quantity.correct");
        assert_eq!(
            request["payload"]["schema"],
            "ledger-workflow-quantity-correction/2"
        );
    }
    request["key"]["operation_id"] = json!(tag);
    let key = OperationKey::parse(&records::bytes(&request["key"]).unwrap()).unwrap();
    (
        temporary,
        path,
        records::bytes(&request).unwrap(),
        key,
        config,
    )
}
/// Read-only equality of retained workflow bytes and all workflow projections.
/// This is test observation, never input to financial construction.
async fn snapshot(path: &Path) -> Vec<(String, Vec<String>)> {
    snapshot_tables(path, false).await
}
async fn snapshot_tables(path: &Path, all: bool) -> Vec<(String, Vec<String>)> {
    let options = SqliteConnectOptions::new()
        .filename(path.join(".ledger/local.db"))
        .read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type='table' AND name LIKE ? AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .bind(if all { "%" } else { "workflow_%" })
    .fetch_all(&mut connection)
    .await
    .unwrap();
    let mut result = Vec::new();
    for table in tables {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut connection)
                .await
                .unwrap();
        let quoted = columns
            .iter()
            .map(|column| format!("quote(\"{}\")", column.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join("||'|'||");
        let query = format!(
            "SELECT {quoted} FROM \"{}\" ORDER BY 1",
            table.replace('"', "\"\"")
        );
        let rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(query))
            .fetch_all(&mut connection)
            .await
            .unwrap();
        result.push((table, rows));
    }
    connection.close().await.unwrap();
    result
}
fn operations(snapshot: &[(String, Vec<String>)]) -> usize {
    snapshot
        .iter()
        .find(|(table, _)| table == "workflow_operations")
        .unwrap()
        .1
        .len()
}

#[tokio::test]
#[ignore = "requires captured build and explicit quiescent maintained synthetic fixture"]
async fn local10_precommit_cut_leaves_no_partial_financial_append() {
    let (_temporary, path, raw, key, config) = fixture("precommit");
    let before = snapshot(&path).await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.store.test_commit_cut(1);
    assert!(matches!(
        ledger.financial(&raw).await,
        Err(ServiceError::OutcomeUnknown { .. })
    ));
    ledger.close().await;
    assert_eq!(
        snapshot(&path).await,
        before,
        "operations, documents, heads and claims must all roll back"
    );
    assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    reopened.close().await;
    assert_eq!(snapshot(&path).await, before);
}

#[tokio::test]
#[ignore = "requires captured build and explicit quiescent maintained synthetic fixture"]
async fn local10_committed_reply_loss_reopens_and_identity_retries_once() {
    let (_temporary, path, raw, key, config) = fixture("reply-loss");
    let before = snapshot(&path).await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    inject_committed_reply_loss(key.bytes().unwrap());
    assert_eq!(
        ledger.financial(&raw).await.unwrap_err(),
        ServiceError::ResponseLost
    );
    ledger.close().await;
    let committed = snapshot(&path).await;
    assert_eq!(operations(&committed), operations(&before) + 1);
    assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let original = reopened.status(&key.bytes().unwrap()).await.unwrap();
    assert_eq!(original["status"], "committed");
    assert_eq!(
        original["receipt"]["key"],
        serde_json::to_value(&key).unwrap()
    );
    let retry = reopened.financial(&raw).await.unwrap();
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], original["receipt"]);
    reopened.close().await;
    assert_eq!(
        snapshot(&path).await,
        committed,
        "status and identity retry cannot append or mutate retained workflow data"
    );
    assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
}

#[tokio::test]
#[ignore = "requires captured build and explicit quiescent maintained synthetic fixture"]
async fn local_root_dependency_allows_monotone_source_restriction_and_exact_retry() {
    let (_temporary, path, _, key, config) = fixture("restriction");
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let original: Value =
        serde_json::from_slice(&fs::read(root.join("source-enroll.request.json")).unwrap())
            .unwrap();
    let enrollment: Value =
        serde_json::from_slice(&fs::read(root.join("source-enroll.committed.json")).unwrap())
            .unwrap();
    let enrollment_ref = enrollment["receipt"]["result_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "workflow-source-enrollment")
        .unwrap();
    let mut ceiling = original["payload"]["ceiling"].clone();
    ceiling["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v != "correct");
    ceiling["operations"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v != "cost.correct");
    let mut restricted_key = key.clone();
    restricted_key.operation_id = "local-root-restriction-regression".into();
    let mut request = json!({"schema":"ledger-operation/1","contract":"workflow/1",
        "kind":"authority.change","key":restricted_key,
        "payload":{"schema":"ledger-workflow-source-restrict/1","action":"source.restrict",
        "enrollment_ref":enrollment_ref,"expected_revision":"1","replacement_ceiling":ceiling}});
    let raw = records::bytes(&request).unwrap();
    let before = snapshot(&path).await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let committed = ledger.prerequisite(&raw).await.unwrap();
    assert_eq!(committed["receipt"]["result_kind"], "administrative");
    ledger.close().await;
    let after = snapshot(&path).await;
    assert_eq!(operations(&after), operations(&before) + 1);
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let retry = reopened.prerequisite(&raw).await.unwrap();
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], committed["receipt"]);
    assert_eq!(
        reopened
            .status(&restricted_key.bytes().unwrap())
            .await
            .unwrap()["receipt"],
        committed["receipt"]
    );
    request["key"]["operation_id"] = json!("local-root-cannot-restore-original-rights");
    request["payload"]["expected_revision"] = json!("2");
    request["payload"]["replacement_ceiling"] = original["payload"]["ceiling"].clone();
    assert!(reopened
        .prerequisite(&records::bytes(&request).unwrap())
        .await
        .is_err());
    reopened.close().await;
    assert_eq!(snapshot(&path).await, after);
    assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
}

/// Original evidence is an economic local10 frame even though its operation
/// kind also names legacy administrative evidence. Status must select its
/// authenticated original recipe, exactly as the retry path already does.
#[tokio::test]
#[ignore = "requires captured build and explicit completed attested synthetic fixture"]
async fn local10_attested_administrative_originals_reopen_status_and_retry() {
    assert!(crate::build_identity::embedded_inputs().is_ok());
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let run: Value = serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(run["classification"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("synthetic"));
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("installation"), &path);
    let config = fs::read(path.join("billing.json")).unwrap();
    let before = snapshot(&path).await;
    for operation in [
        "main-quantity-proof",
        "main-proof-source-narrow",
        "main-quantity-replacement-proof",
    ] {
        let raw = fs::read(root.join(format!("{operation}.request.json"))).unwrap();
        let request: Value = serde_json::from_slice(&raw).unwrap();
        let saved: Value = serde_json::from_slice(
            &fs::read(root.join(format!("{operation}.committed.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["status"], "committed");
        assert_eq!(saved["receipt"]["key"], request["key"]);
        let key = records::bytes(&request["key"]).unwrap();
        let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
        let status = ledger.status(&key).await.unwrap();
        assert_eq!(status["status"], "committed");
        assert_eq!(status["receipt"], saved["receipt"]);
        ledger.close().await;
        let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
        let retry = if request["kind"] == "authority.change" {
            reopened.prerequisite(&raw).await.unwrap()
        } else {
            assert_eq!(
                request["payload"]["schema"],
                "ledger-workflow-fact-attest/1"
            );
            reopened.financial(&raw).await.unwrap()
        };
        assert_eq!(retry["duplicate_kind"], "identity");
        assert_eq!(retry["receipt"], saved["receipt"]);
        reopened.close().await;
        assert_eq!(snapshot(&path).await, before);
        assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
    }
}

#[tokio::test]
#[ignore = "requires a captured synthetic local acceptance-control fixture"]
async fn local10_local_acceptance_original_status_and_identity_retry() {
    assert!(crate::build_identity::embedded_inputs().is_ok());
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let run: Value = serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(run["classification"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("synthetic"));
    let raw = fs::read(root.join("acceptance-control.request.json")).unwrap();
    let request: Value = serde_json::from_slice(&raw).unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("acceptance-control.committed.json")).unwrap())
            .unwrap();
    let changed =
        fs::read(root.join("inputs/changed-original-accepted-context.request.json")).unwrap();
    let changed_request: Value = serde_json::from_slice(&changed).unwrap();
    assert_eq!(saved["receipt"]["key"], request["key"]);
    assert_eq!(changed_request["key"], request["key"]);
    assert_ne!(
        changed_request["payload"]["binding_inputs"]["accepted_context"],
        request["payload"]["binding_inputs"]["accepted_context"]
    );
    let key = records::bytes(&request["key"]).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("installation"), &path);
    let config = fs::read(path.join("billing.json")).unwrap();
    let before = snapshot(&path).await;
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let status = ledger.status(&key).await.unwrap();
    assert_eq!(status["receipt"], saved["receipt"]);
    ledger.close().await;
    assert_eq!(snapshot(&path).await, before);
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    let retry = ledger.financial(&raw).await.unwrap();
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], saved["receipt"]);
    ledger.close().await;
    assert_eq!(snapshot(&path).await, before);
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert!(
        matches!(ledger.financial(&changed).await, Err(ServiceError::Rejection(code)) if code == "IDENTITY_CONFLICT")
    );
    ledger.close().await;
    assert_eq!(snapshot(&path).await, before);
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        ledger.status(&key).await.unwrap()["receipt"],
        saved["receipt"]
    );
    ledger.close().await;
    assert_eq!(snapshot(&path).await, before);
    assert_eq!(fs::read(path.join("billing.json")).unwrap(), config);
}

#[tokio::test]
#[ignore = "requires explicit quiescent completed linked main synthetic fixture"]
async fn local10_initial_activation_original_status_after_linked_closure() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let run: Value = serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(run["classification"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("synthetic"));
    let original = fs::read(root.join("activation.request.json")).unwrap();
    let request: Value = serde_json::from_slice(&original).unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("activation.committed.json")).unwrap()).unwrap();
    assert_eq!(saved["receipt"]["key"], request["key"]);
    assert_eq!(
        saved["receipt"]["original_request_bytes_sha256"],
        records::raw_hash(&original)
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    // STATUS proves the existing commit before an identical original-query retry.
    // No NEW activation, source, identity or clock is constructed.
    let status = ledger
        .status(&records::bytes(&request["key"]).unwrap())
        .await
        .unwrap();
    assert_eq!(status["schema"], "ledger-operation-status/1");
    assert_eq!(status["status"], "committed");
    assert_eq!(status["receipt"], saved["receipt"]);
    assert_eq!(status["request_evidence"], saved["request_evidence"]);
    assert_eq!(snapshot_tables(&installation, true).await, before);
    let retry = ledger
        .local_activation_owned(&original)
        .await
        .unwrap()
        .unwrap()
        .into_legacy_value();
    assert_eq!(retry["schema"], "ledger-operation-result/1");
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], saved["receipt"]);
    assert_eq!(retry["request_evidence"], saved["request_evidence"]);
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
}

#[tokio::test]
#[ignore = "requires explicit quiescent synthetic preclaim report fixture"]
async fn preclaim_report_prepare_preserves_unrelated_pending_root() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("main-report-reverse.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "fe64580e4740a61dcfd47c79c6aff89fa9cdcf8f3126fbaaa16d74abec4fb314"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let req = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    let result = proof_session::with_proofs(&mut tx, req.key(), |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, req.key()).await?;
            local_financial::check_report_prepare_without_append(&ledger, tx, &req, &mut s).await
        })
    })
    .await;
    result.unwrap();
    tx.rollback().await.unwrap();
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
}

#[tokio::test]
#[ignore = "requires explicit quiescent synthetic policy acceptance fixture"]
async fn policy_independent_issuer_acceptance_prepare_without_append() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("acceptance.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "c10e75928af22b107e1c0292f826cdd0a7c2ba18c1bdc9064bb0627844973f9d"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let req = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    let result = proof_session::with_proofs(&mut tx, req.key(), |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, req.key()).await?;
            let prepared = acceptance::prepare_local(&ledger, tx, &req, &mut s).await?;
            drop(prepared);
            Ok(())
        })
    })
    .await;
    tx.rollback().await.unwrap();
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    result.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit quiescent synthetic policy issuer fixture"]
async fn policy_independent_issuer_retention_prepare_without_append() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("main-quantity-proof.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "05d36d716a8a702e9b7a24e2177a0bdedd2dfb7998804c5f4a0b97d744e00d76"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let req = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    let result = proof_session::with_proofs(&mut tx, req.key(), |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, req.key()).await?;
            local_financial::check_retention_prepare_without_append(&ledger, tx, &req, &mut s).await
        })
    })
    .await;
    tx.rollback().await.unwrap();
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    result.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit quiescent synthetic completed policy fixture"]
async fn policy_original_acceptance_status_after_issuer_revocation_and_closure() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("acceptance.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "c10e75928af22b107e1c0292f826cdd0a7c2ba18c1bdc9064bb0627844973f9d"
    );
    let request: Value = serde_json::from_slice(&raw).unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(root.join("acceptance.committed.json")).unwrap()).unwrap();
    assert_eq!(saved["receipt"]["key"], request["key"]);
    assert_eq!(
        saved["receipt"]["original_request_bytes_sha256"],
        records::raw_hash(&raw)
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let status = ledger
        .status(&records::bytes(&request["key"]).unwrap())
        .await;
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    let status = status.unwrap();
    assert_eq!(status["schema"], "ledger-operation-status/1");
    assert_eq!(status["status"], "committed");
    assert_eq!(status["receipt"], saved["receipt"]);
    assert_eq!(status["request_evidence"], saved["request_evidence"]);
    // The exact CLI acceptance route is an ORIGINAL query only after known
    // committed STATUS, with the same saved raw bytes/key and later history.
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let retry = ledger.financial_owned_reply(&raw).await;
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    let retry = retry.unwrap().into_legacy_value();
    assert_eq!(retry["schema"], "ledger-operation-result/1");
    assert_eq!(retry["status"], "committed");
    assert_eq!(retry["duplicate_kind"], "identity");
    assert_eq!(retry["receipt"], saved["receipt"]);
    assert_eq!(retry["request_evidence"], saved["request_evidence"]);
}
