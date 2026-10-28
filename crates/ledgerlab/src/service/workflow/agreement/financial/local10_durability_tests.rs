//! Opt-in actual local financial service tests. Run from a captured test build
//! with LEDGER_WORKFLOW_TEST_FIXTURE naming a quiescent maintained synthetic
//! runner output (RUN.json, main-work.request.json, installation/).
//! Only a private temporary copy is opened; no raw financial append is used.
use super::*;
use crate::service::workflow::local::deadline;
use crate::store::ports::AcceptanceStore;
use ledgerlab_core::validation_work::RepresentationKind;
use sqlx::{AssertSqlSafe, Connection, SqliteConnection, sqlite::SqliteConnectOptions};
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
    assert!(
        run["classification"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("synthetic")
    );
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

async fn retain_platform_test_input(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    raw: &[u8],
) -> Result<crate::store::sqlite::workflow::RecordRef> {
    let request = super::super::request::Request::parse(raw)?;
    let key = request.key();
    ledger.admin.read(&ledger.store, tx, key).await?;
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let root = match tx
        .workflow_agreement_root_candidate(&key.logical_store_id, &key.scope)
        .await
        .map_err(crate::service::store_error)?
    {
        Some(root) => {
            verify::document_origin(tx, key, &root.reference).await?;
            verify::root(&canonical_body(&root)?, key, None)?;
            if canonical_body(&root)?["basis"] == super::super::journal::basis(&setup) {
                root
            } else {
                super::super::journal::fresh_root(&setup, &at)?
            }
        }
        None => super::super::journal::fresh_root(&setup, &at)?,
    };
    let payload = super::super::service::normalize_payload(&request)?;
    let prepared = super::super::service::prepare_evidence(
        tx,
        &request,
        &payload,
        (&setup.operator, &at, "real_local", &root.reference),
    )
    .await?;
    let result = prepared.results[0].clone();
    let sequence = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let input = super::super::parse_evidence_payload(&payload)?;
    let append = super::super::journal::assemble(
        &request,
        super::super::journal::evidence(&request, &payload)?,
        &setup,
        &at,
        sequence,
        root,
        prepared.documents,
        prepared.results,
        super::super::evidence_identity(key, &input)?,
        None,
        Projection::None,
    )?;
    tx.workflow_agreement_append(&append)
        .await
        .map_err(crate::service::store_error)?;
    proof_session::after_append(tx, key)?;
    verify::retained(tx, key, &append.operation).await?;
    Ok(result)
}

#[tokio::test]
#[ignore = "requires stopped16-op Stage fixture; actual retention/account/originals then whole rollback"]
async fn platform_evidence_retention_account_original_route_rolls_back() {
    assert!(
        crate::build_identity::embedded_inputs().is_ok(),
        "actual captured inputs required"
    );
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let failed = fs::read(root.join("supplier-owner-evidence.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&failed),
        "6303f0a2a661acd7549b18ff30c6452db88d6ef899f6d7cfb8cf40c4c220472d"
    );
    let mut retention: Value = serde_json::from_slice(&failed).unwrap();
    retention["key"]["operation_id"] = json!("supplier-owner-evidence-platform");
    retention["payload"]["schema"] = json!("ledger-workflow-evidence-retain/2");
    retention["payload"]["media_type"] = json!("text/plain;charset=utf-8");
    assert_eq!(
        retention["payload"]["content_sha256"],
        "f3e93b76cc85261c56714d7f3a58a6df247823925b03c2870b869baa8106f138"
    );
    let raw = records::bytes(&retention).unwrap();
    let key = OperationKey::parse(&records::bytes(&retention["key"]).unwrap()).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let before = snapshot_tables(&installation, true).await;
    let config = fs::read(installation.join("billing.json")).unwrap();
    assert_eq!(before.len(), 117);
    assert_eq!(operations(&before), 16);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let resource = ledger.resource_snapshot().unwrap();
    let arena = resource.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let mut tx = resource.begin(&ledger.store, &mut s).await.unwrap();
    let result = proof_session::with_proofs(&mut tx, &key, |tx| Box::pin(async {
        let retained_ref = retain_platform_test_input(&ledger, tx, &raw).await?;
        // Prerequisite retention owns its read attempt before the account
        // coordinator starts its main attempt.
        tx.workflow_main_session = true;
        let retained = loaded(tx, &key, &retained_ref).await?;
        let body = canonical_body(&retained)?;
        require(body["schema"] == "ledger-workflow-retained-evidence/2", "INTEGRITY_FAILURE")?;
        let proof_root: crate::store::sqlite::workflow::RecordRef = serde_json::from_value(body["authority_ref"].clone()).map_err(|_| ServiceError::IntegrityFailure)?;
        let root_record = loaded(tx, &key, &proof_root).await?;
        let input = super::super::evidence_input(&body)?;
        let assertion: Value = serde_json::from_slice(input.content()).map_err(|_| ServiceError::IntegrityFailure)?;
        let mut enrollment = retention.clone();
        enrollment["kind"] = json!("authority.change");
        enrollment["key"]["operation_id"] = json!("supplier-account-enroll");
        enrollment["payload"] = json!({"schema":"ledger-cost-authority-enroll/1", "authority_id":"stage-supplier-owner", "provider_account_identities":assertion["provider_account_identities"], "evidence_refs":[retained_ref]});
        let enrollment_raw = records::bytes(&enrollment)?;
        let incoming = ingress::IncomingRequest::parse(&enrollment_raw, &mut s)?;
        let management = ManagementObservation::capture(&ledger, tx, incoming.key(), request_wrapper::field(incoming.value(), "key", &mut s)?, &mut s).await?;
        local_cost_authority::prepare_append(&ledger, tx, &incoming, &management, &mut s).await?;
        proof_session::after_append(tx, incoming.key())?;
        // A saved-original query is a distinct audit attempt. The fresh
        // account attempt has already consumed its sole economic child.
        let mut original_session = ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
        let identity = original_session.canonical_representation(&enrollment["key"], ledgerlab_core::validation_work::RepresentationKind::Record).map_err(core_error)?;
        let acquired = prefix::target(tx, incoming.key(), identity, &mut original_session).await?;
        let original = acquired.checked_wrappers(tx, &mut original_session).await?;
        require(local_cost_authority::verify_original(tx, &original, None, &mut original_session).await?, "INTEGRITY_FAILURE")?;
        let op = original.wrappers().operation();
        let snapshot = &op["authority_snapshot"];
        let basis = &snapshot["local_admin_snapshot"]["basis"];
        let business: Value = serde_json::from_slice(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/workflow-stage/business.json"))).map_err(|_| ServiceError::IntegrityFailure)?;
        let mut leg = business["binding"]["funding_legs"].as_array().ok_or(ServiceError::IntegrityFailure)?.iter().find(|v| v["mode"] == "platform").ok_or(ServiceError::IntegrityFailure)?.clone();
        leg["evidence_refs"] = json!([retained_ref]);
        Ok(json!({"context":{"key":enrollment["key"],"actor":basis["principal_id"],"canonical_economic_namespace":basis["canonical_economic_namespace"],"installation_mode":snapshot["installation_mode"],"received_at":op["received_at"],"accepted_at":op["accepted_at"],"authorities":[],"source_observation_inputs":[]},"records":[{"reference":retained_ref,"body":body},{"reference":proof_root,"body":canonical_body(&root_record)?}],"binding":{"customer":business["customer"],"agreement_id":business["agreement"],"accepted_at":op["accepted_at"],"funding_legs":[leg]}}))
    })).await;
    tx.rollback().await.unwrap();
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    let output = result.unwrap();
    if let Some(path) = std::env::var_os("WAVE_PLATFORM_FUNDING_OUTPUT") {
        fs::write(path, records::bytes(&output).unwrap()).unwrap();
    }
}

#[tokio::test]
#[ignore = "requires exact stopped Stage publication fixture; private transaction rolls back"]
async fn saved_stage_publication_prepares_and_verifies_original_then_rolls_back() {
    assert!(
        crate::build_identity::embedded_inputs().is_ok(),
        "requires actual captured build inputs"
    );
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("publication.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "e3d493df3354d2b24c897c16b8e792d8a85ee79a303484ed4ca6e409c13bde26"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let before = snapshot_tables(&installation, true).await;
    let config = fs::read(installation.join("billing.json")).unwrap();
    assert_eq!(before.len(), 117);
    assert_eq!(operations(&before), 15);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let resource = ledger.resource_snapshot().unwrap();
    let arena = resource.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let incoming = ingress::IncomingRequest::parse_publication(&raw, &mut s).unwrap();
    let mut tx = resource.begin(&ledger.store, &mut s).await.unwrap();
    tx.workflow_main_session = true;
    let result = proof_session::with_proofs(&mut tx, incoming.key(), |tx| {
        Box::pin(async {
            let reply = local_publication::prepare_append(
                &ledger,
                tx,
                &raw,
                publication_prepare::local_support,
                &mut s,
            )
            .await?;
            let saved = reply.into_legacy_value();
            let identity = s
                .canonical_representation(
                    &json!(incoming.key()),
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let acquired = prefix::target(tx, incoming.key(), identity, &mut s).await?;
            let original = acquired.checked_wrappers(tx, &mut s).await?;
            require(
                local_publication::verify_original(tx, &original, &mut s)
                    .await?
                    .is_some(),
                "INTEGRITY_FAILURE",
            )?;
            let evidence = request_wrapper::field(
                original.wrappers().operation(),
                "request_evidence",
                &mut s,
            )?;
            require(saved["request_evidence"] == *evidence, "INTEGRITY_FAILURE")?;
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
#[ignore = "requires the exact stopped synthetic Stage source3 request; private copy only"]
async fn saved_source3_enrollment_prepares_then_rolls_back_without_append() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("alias-enroll.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "10fc380333fc6e0f4fe5964782250edd5a1f096375c65d56f7d5f24fd0ceaaaa"
    );
    let request = super::super::request::Request::parse(&raw).unwrap();
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
    assert_eq!(operations(&before), 14);
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let mut tx = ledger
        .store
        .begin(crate::service::workflow::local::deadline())
        .await
        .unwrap();
    let key = request.key();
    let result = proof_session::with_proofs(&mut tx, key, |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, key).await?;
            let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
            let root = tx
                .workflow_agreement_root_candidate(&key.logical_store_id, &key.scope)
                .await
                .map_err(crate::service::store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            verify::document_origin(tx, key, &root.reference).await?;
            verify::root(&canonical_body(&root)?, key, Some(&key.source))?;
            let payload = super::super::service::normalize_payload(&request)?;
            let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
            let prepared = super::super::source::prepare(
                tx,
                key,
                &payload,
                &setup,
                &at,
                "real_local",
                &root.reference,
            )
            .await?;
            require(prepared.results.len() == 3, "INTEGRITY_FAILURE")?;
            let target = prepared
                .documents
                .iter()
                .find(|d| {
                    d.reference.kind == "workflow-source-enrollment"
                        && prepared.results.contains(&d.reference)
                })
                .ok_or(ServiceError::IntegrityFailure)?;
            let body = canonical_body(target)?;
            require(
                body["ceiling"] == payload["ceiling"]
                    && body["evidence_ref"] == payload["evidence_ref"],
                "INTEGRITY_FAILURE",
            )?;
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
#[ignore = "requires the explicit completed lifecycle fixture; reads a private copy only"]
async fn current_reads_preserve_completed_lifecycle_and_use_actual_owner() {
    let root = PathBuf::from(
        std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE")
            .expect("explicit completed lifecycle fixture"),
    );
    let run: Value = serde_json::from_slice(&fs::read(root.join("RUN.json")).unwrap()).unwrap();
    assert!(
        run["classification"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("synthetic")
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(
        &root.join("recovery-completed-quiescent-installation"),
        &installation,
    );
    let before = snapshot_tables(&installation, true).await;
    let config = fs::read(installation.join("billing.json")).unwrap();
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let result=async {
        for (name,revision,status) in [("fixed-work","1",None),("failed-work","3",Some("reversed")),("rounded-work","2",Some("reversed"))] {
            let command=run["commands"].as_array().unwrap().iter().find(|v|v["name"]==name).unwrap();
            let receipt:Value=serde_json::from_slice(&fs::read(root.join(command["stdout"].as_str().unwrap())).unwrap()).unwrap();
            let subject=receipt["receipt"]["result_refs"].as_array().unwrap().iter().find(|v|v["kind"]=="workflow-work").unwrap();
            let read=json!({"schema":"ledger-read/1","kind":"work.current","payload":{"subject_ref":subject}});
            let result=ledger.read(&records::bytes(&read).unwrap()).await?;
            assert_eq!(result["kind"],"workflow-decision");
            assert_eq!(result["body"]["subject_ref"],*subject);
            assert_eq!(result["body"]["revision"],revision);
            if let Some(status)=status {assert_eq!(result["body"]["status"],status);}
        }
        Result::<()>::Ok(())
    }.await;
    ledger.close().await;
    assert_eq!(snapshot_tables(&installation, true).await, before);
    assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
    result.unwrap();
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
    assert!(
        reopened
            .prerequisite(&records::bytes(&request).unwrap())
            .await
            .is_err()
    );
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
    assert!(
        run["classification"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("synthetic")
    );
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
    assert!(
        run["classification"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("synthetic")
    );
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
    assert!(
        run["classification"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("synthetic")
    );
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

#[tokio::test]
#[ignore = "requires exact quiescent Stage acceptance fixture; private whole-copy rollback"]
async fn stage_acceptance_actual_coordinator_rolls_back() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("acceptance.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "533c51228653f917545aa9c19f7c06ae96dd064058d039cb09d044eac9a97f09"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("prepared-quiescent-installation"), &installation);
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    assert_eq!(operations(&before), 35);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let incoming = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    tx.workflow_main_session = true;
    let result = proof_session::with_proofs(&mut tx, incoming.key(), |tx| {
        Box::pin(async {
            let management = ManagementObservation::capture(
                &ledger,
                tx,
                incoming.key(),
                request_wrapper::field(incoming.value(), "key", &mut s)?,
                &mut s,
            )
            .await?;
            let reply =
                local_acceptance::prepare_append(&ledger, tx, &incoming, &management, &mut s)
                    .await?;
            drop(reply);
            proof_session::after_append(tx, incoming.key())?;
            let mut original_session =
                ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(
                    &arena,
                )
                .map_err(core_error)?;
            let key_body = request_wrapper::field(incoming.value(), "key", &mut original_session)?;
            let identity = original_session
                .canonical_representation(
                    key_body,
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let selected =
                prefix::target(tx, incoming.key(), identity, &mut original_session).await?;
            let original = selected.checked_wrappers(tx, &mut original_session).await?;
            require(
                local_acceptance::verify_original(
                    &ledger,
                    tx,
                    &original,
                    None,
                    &mut original_session,
                )
                .await?,
                "INTEGRITY_FAILURE",
            )?;
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
#[ignore = "requires exact quiescent Stage authorization fixture; private whole-copy rollback"]
async fn stage_authorization_actual_coordinator_rolls_back() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let raw = fs::read(root.join("stage-authorize.request.json")).unwrap();
    assert_eq!(
        records::raw_hash(&raw),
        "4c6c58eb787cdce5bff9566a88967d8204ca8b9ffe8b1f7b8b7cbd520d7f5822"
    );
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    assert_eq!(operations(&before), 37);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let incoming = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    tx.workflow_main_session = true;
    let result = proof_session::with_proofs(&mut tx, incoming.key(), |tx| {
        Box::pin(async {
            let management = ManagementObservation::capture(
                &ledger,
                tx,
                incoming.key(),
                request_wrapper::field(incoming.value(), "key", &mut s)?,
                &mut s,
            )
            .await?;
            let reply =
                local_financial::prepare_work_append(&ledger, tx, &incoming, &management, &mut s)
                    .await?;
            drop(reply);
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
#[ignore = "requires exact private Stage38 Work-occurrence fixture; actual Work-fact prepare/append rollback"]
async fn stage_work_occurrence_bootstrap_rolls_back() {
    let root = PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let operation = std::env::var("LEDGER_WORKFLOW_TEST_OPERATION")
        .unwrap_or_else(|_| "work-occurrence".into());
    let (hash, count) = match operation.as_str() {
        "work-occurrence" => (
            "c0e5c0e2a8573bae83547d416a3885ba7961d38c4d9cddf628eac57353e20809",
            38,
        ),
        "member-work" => (
            "37474d9a7d659753ed586d243a09a7191fd180b1b73ddb4eaab1c7d9a18d1c95",
            39,
        ),
        "shared-work-alias" => (
            "f837cf5b2ba4b2b3a6ecc7fff333f54792f3ce05c1330381733b4730f6a2fb9f",
            40,
        ),
        _ => panic!("exact maintained Stage operand required"),
    };
    let raw = fs::read(root.join(format!("{operation}.request.json"))).unwrap();
    assert_eq!(records::raw_hash(&raw), hash);
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_private_tree(&root.join("failed-quiescent-installation"), &installation);
    let before = snapshot_tables(&installation, true).await;
    assert_eq!(before.len(), 117);
    assert_eq!(operations(&before), count);
    let config = fs::read(installation.join("billing.json")).unwrap();
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    if operation == "member-work" {
        // Full DDL rollback under the same held local-owner writer, then the
        // actual explicit public enable path. No original request is replayed.
        let key: OperationKey = serde_json::from_slice::<Value>(&raw)
            .map(|v| serde_json::from_value(v["key"].clone()).unwrap())
            .unwrap();
        let mut migration = ledger.store.begin(deadline()).await.unwrap();
        ledger
            .admin
            .read(&ledger.store, &mut migration, &key)
            .await
            .unwrap();
        migration.workflow_enable_invocation_schema().await.unwrap();
        assert_eq!(migration.billing_schema_version().await.unwrap(), 19);
        migration.rollback().await.unwrap();
        assert_eq!(snapshot_tables(&installation, true).await, before);
        assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
        let enabled = ledger.enable_profile_b().await;
        if enabled.is_err() {
            assert_eq!(snapshot_tables(&installation, true).await, before);
            assert_eq!(fs::read(installation.join("billing.json")).unwrap(), config);
        }
        enabled.unwrap();
        let options = SqliteConnectOptions::new()
            .filename(installation.join(".ledger/local.db"))
            .read_only(true);
        let mut integrity = SqliteConnection::connect_with(&options).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA user_version")
                .fetch_one(&mut integrity)
                .await
                .unwrap(),
            19
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut integrity)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_all(&mut integrity)
                .await
                .unwrap(),
            vec!["ok"]
        );
        integrity.close().await.unwrap();
        let upgraded = snapshot_tables(&installation, true).await;
        assert_eq!(upgraded.len(), before.len());
        for ((old_name, old_rows), (new_name, new_rows)) in before.iter().zip(&upgraded) {
            assert_eq!(old_name, new_name);
            if old_name != "_sqlx_migrations" {
                assert_eq!(old_rows, new_rows, "old-column rows changed: {old_name}");
            } else {
                assert_eq!(new_rows.len(), old_rows.len() + 1);
                for row in old_rows {
                    assert!(new_rows.contains(row));
                }
            }
        }
    }
    let before = snapshot_tables(&installation, true).await;
    let snapshot = ledger.resource_snapshot().unwrap();
    let arena = snapshot.arena().unwrap();
    let mut s =
        ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
            .unwrap();
    let request = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
    let mut tx = snapshot.begin(&ledger.store, &mut s).await.unwrap();
    tx.workflow_main_session = true;
    let result = proof_session::with_proofs(&mut tx, request.key(), |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, request.key()).await?;
            let held = ledger
                .admin
                .read_management(&ledger.store, tx, request.key(), &mut s)
                .await?;
            let reply = fresh_reply_in(&ledger, tx, &request, held, &mut s).await?;
            require(
                matches!(reply, TransactionResult::Appended(_)),
                "INTEGRITY_FAILURE",
            )?;
            proof_session::after_append(tx, request.key())?;
            let mut original_session =
                ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(
                    &arena,
                )
                .map_err(core_error)?;
            let key_body = request_wrapper::field(request.value(), "key", &mut original_session)?;
            let identity = original_session
                .canonical_representation(key_body, RepresentationKind::Record)
                .map_err(core_error)?;
            let selected =
                prefix::target(tx, request.key(), identity, &mut original_session).await?;
            let original = selected.checked_wrappers(tx, &mut original_session).await?;
            let _reproduced = prefix::fold_original_prefix(
                &ledger,
                tx,
                request.key(),
                &original,
                &mut original_session,
            )
            .await?;
            if operation == "member-work" {
                let id = request.value()["payload"]["invocation_refs"][0]["id"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?;
                tx.test_invocation_revision_storage_controls(
                    id,
                    &request.key().logical_store_id,
                    &request.key().scope,
                )
                .await;
            }
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
