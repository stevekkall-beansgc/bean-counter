//! One explicit read-only retained-set regression; no financial submit.
use super::*;
use crate::{service::workflow::local::deadline, store::ports::AcceptanceStore};
use ledgerlab_core::canonical::audit_arena::AuditArena;
use std::{fs, path::Path};
fn copy_tree(from: &Path, to: &Path) {
    let m = fs::symlink_metadata(from).unwrap();
    assert!(!m.file_type().is_symlink());
    if m.is_dir() {
        fs::create_dir(to).unwrap();
        fs::set_permissions(to, m.permissions()).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            copy_tree(&e.path(), &to.join(e.file_name()));
        }
    } else {
        assert!(m.is_file());
        fs::copy(from, to).unwrap();
    }
}
#[tokio::test]
#[ignore = "requires explicit quiescent synthetic second-report fixture"]
async fn first_nonempty_declared_chain_uses_economic_record_provenance() {
    let root = std::path::PathBuf::from(std::env::var_os("LEDGER_WORKFLOW_TEST_FIXTURE").unwrap());
    let request: Value =
        serde_json::from_slice(&fs::read(root.join("main-report-2-work.request.json")).unwrap())
            .unwrap();
    let original: Value =
        serde_json::from_slice(&fs::read(root.join("main-report-1-work.committed.json")).unwrap())
            .unwrap();
    let key = OperationKey::parse(&records::bytes(&request["key"]).unwrap()).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let installation = temporary
        .path()
        .canonicalize()
        .unwrap()
        .join("installation");
    copy_tree(
        &root.join("failed-publication-resume-e3df384-installation"),
        &installation,
    );
    let ledger = LocalWorkflowLedger::open_local(&installation)
        .await
        .unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &key, |tx| {
        Box::pin(async {
            ledger.admin.read(&ledger.store, tx, &key).await?;
            let cutoff = tx.workflow_sequence().await.unwrap();
            let arena = AuditArena::with_resource_limits(
                ledgerlab_core::validation_work::ResourceLimits::unlimited(),
            )
            .unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let (binding, _lease) =
                source::from_value_local(&request["payload"]["binding_ref"], &mut s).unwrap();
            let mut pool =
                super::super::super::super::local_inputs::LocalInputs::new(&mut s).unwrap();
            let selected = acquire(
                tx,
                &key,
                Selection {
                    binding: &binding,
                    customer: request["payload"]["customer"].as_str().unwrap(),
                    chain: request["payload"]["chain"].as_str().unwrap(),
                    before: cutoff,
                },
                &mut pool,
                &mut s,
            )
            .await
            .unwrap();
            assert_eq!(selected.references.len(), 1);
            let expected = original["receipt"]["result_refs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["kind"] == "workflow-link")
                .unwrap();
            let (expected, _lease) = source::from_value_local(expected, &mut s).unwrap();
            assert_eq!(selected.references[0], expected);
            for kind in [
                "workflow-link",
                "workflow-work",
                "workflow-decision",
                "workflow-work-claim",
                "workflow-economic-authority",
            ] {
                assert!(
                    pool.iter_views().any(|r| r.reference.kind == kind),
                    "missing {kind}"
                );
            }
            // The SAME production-called local kernel refuses corrupt framing. Each
            // refusal has a fresh session because a failed child poisons its owner.
            let raw = pool.body(&expected).unwrap();
            for wrong_domain in [false, true] {
                let arena = AuditArena::new().unwrap();
                let mut meter = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let mut bad = expected.clone();
                if wrong_domain {
                    bad.kind = "workflow-work".into();
                } else {
                    bad.digest = "sha256:".to_owned() + &"0".repeat(64);
                }
                assert!(source::local_record_value(
                    &key,
                    &bad,
                    raw,
                    "ledger-workflow-link/1",
                    &mut meter
                )
                .is_err());
            }
            assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
            Ok(())
        })
    })
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    ledger.close().await;
}
