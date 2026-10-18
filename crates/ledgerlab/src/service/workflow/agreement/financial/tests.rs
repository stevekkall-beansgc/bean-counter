//! Disabled-shell checks. No financial acceptance, fake receipt or raw append.
use super::*;
use crate::{billing::BillingLedger, store::errors::CommitError};

async fn installation(store: &str) -> (tempfile::TempDir, LocalWorkflowLedger, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("billing");
    let mut setup: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../../examples/billing/setup.json"
    ))
    .unwrap();
    setup["store_id"] = json!(store);
    BillingLedger::init(&path, &records::bytes(&setup).unwrap())
        .await
        .unwrap();
    let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
    ledger.enable_profile_b().await.unwrap();
    (dir, ledger, path)
}
fn key(store: &str, operation: &str) -> OperationKey {
    OperationKey {
        logical_store_id: store.into(),
        scope: ["example-company".into(), "local".into()],
        source: "urn:example:work".into(),
        operation_id: operation.into(),
    }
}
fn envelope(key: &OperationKey) -> Value {
    // Deliberately only lexical-envelope data. No complete normalized economic
    // payload or snapshot is claimed by the disabled host path.
    json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"work.accept","key":key,"payload":{"schema":"ledger-workflow-work/1"}})
}

#[test]
fn financial_shell_preserves_closed_lexical_envelope_only() {
    let value = envelope(&key("disabled-store", "never-written"));
    let raw = records::bytes(&value).unwrap();
    assert_eq!(request::Request::parse_financial(&raw).unwrap().raw(), raw);
    let mut value = value.clone();
    value["prepared"] = json!(true);
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
    let duplicate = String::from_utf8(raw).unwrap().replacen(
        "\"kind\":\"work.accept\"",
        "\"kind\":\"work.accept\",\"\\u006bind\":\"work.accept\"",
        1,
    );
    assert!(request::Request::parse_financial(duplicate.as_bytes()).is_err());
    value.as_object_mut().unwrap().remove("prepared");
    value["kind"] = json!("agreement.accept");
    assert!(request::Request::parse_financial(&records::bytes(&value).unwrap()).is_err());
}

#[tokio::test]
async fn financial_shell_denies_fresh_with_zero_delta_and_reopens_not_observed() {
    let (_dir, ledger, path) = installation("disabled-financial").await;
    let key = key("disabled-financial", "never-written");
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        ledger
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    let mut wrong = key.clone();
    wrong.scope[0] = "another-tenant".into();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&wrong)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="AUTHORITY_REQUIRED")
    );
    ledger.close().await;
    let reopened = LocalWorkflowLedger::open_local(&path).await.unwrap();
    assert_eq!(
        reopened.status(&key.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    assert_eq!(
        reopened
            .financial(&records::bytes(&envelope(&key)).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    reopened.close().await;
}

#[tokio::test]
async fn financial_shell_global_original_lookup_precedes_frozen_fresh_gate() {
    let (_dir, ledger, _path) = installation("disabled-original-order").await;
    let key = key("disabled-original-order", "existing-administrative-key");
    let content = b"Explicit operator test material; no commercial financial authority.";
    let mut proof = envelope(&key);
    proof["kind"] = json!("evidence.retain");
    proof["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"test-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"test-acceptor"}});
    let original = ledger
        .prerequisite(&records::bytes(&proof).unwrap())
        .await
        .unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    tx.workflow_test_freeze_admission().await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        matches!(ledger.financial(&records::bytes(&envelope(&key)).unwrap()).await,Err(ServiceError::Rejection(code)) if code=="IDENTITY_CONFLICT")
    );
    assert_eq!(
        ledger.status(&key.bytes().unwrap()).await.unwrap()["receipt"],
        original["receipt"]
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    ledger.close().await;
}

#[tokio::test]
async fn financial_shell_management_frame_wrong_transaction_poisons_both_handles() {
    let (_a, first, _path_a) = installation("frame-store").await;
    let (_b, second, _path_b) = installation("frame-store").await;
    let key = key("frame-store", "frame-only");
    let mut a = first.store.begin(deadline()).await.unwrap();
    let frame = proof_session::with_proofs(&mut a, &key, |tx| {
        Box::pin(ManagementObservation::capture(&first, tx, &key))
    })
    .await
    .unwrap();
    let mut b = second.store.begin(deadline()).await.unwrap();
    assert_eq!(
        frame.recheck(&second, &mut b, &key).await.unwrap_err(),
        ServiceError::IntegrityFailure
    );
    assert!(matches!(a.commit().await, Err(CommitError::RolledBack(_))));
    assert!(matches!(b.commit().await, Err(CommitError::RolledBack(_))));
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn chronological_owning_frames_follow_actual_public_prerequisite_history() {
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_directory, ledger, _path) = installation("actual-prefix").await;
    let mut receipts = Vec::new();
    for (operation, content) in [
        ("first", b"Original customer evidence".as_slice()),
        ("second", b"Second original customer evidence".as_slice()),
    ] {
        let mut input = envelope(&key("actual-prefix", operation));
        input["kind"] = json!("evidence.retain");
        input["payload"] = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"customer_registration","subject":{"customer":"prefix-customer"},"media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(content),"content_sha256":records::raw_hash(content),"metadata":{"acceptor":"prefix-acceptor"}});
        receipts.push(
            ledger
                .prerequisite(&records::bytes(&input).unwrap())
                .await
                .unwrap(),
        );
    }
    let through = receipts[1]["receipt"]["commit_sequence"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let root = key("actual-prefix", "prefix-reader");
    let arena = AuditArena::new().unwrap();
    let baseline = arena.usage().unwrap().live_bytes;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &root, |tx| {
        Box::pin(async {
            let mut after = 0;
            for (operation, receipt) in [("first", &receipts[0]), ("second", &receipts[1])] {
                let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let next = prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .unwrap();
                assert_eq!(next.key(), &key("actual-prefix", operation));
                after = next.frame.sequence;
                let resolved = next.checked_wrappers(tx, &mut session).await?;
                assert_eq!(
                    resolved.wrappers().class(),
                    wrappers::WireClass::Prerequisite
                );
                assert_eq!(resolved.wrappers().receipt(), &receipt["receipt"]);
                // Exact saved request is read from the original row, not recreated
                // from the receipt or supplied as an expected construction operand.
                assert_eq!(
                    resolved.wrappers().original_request().value()["key"]["operation_id"],
                    operation
                );
                // The connected current/prefix document reader is exercised
                // only with a genuine public prerequisite's retained member.
                // No synthetic financial history or provenance token is made.
                let reference: ledgerlab_core::policy::workflow::transition_constructor::ExactRef =
                    serde_json::from_value(
                        receipt["receipt"]["result_refs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["kind"] == "workflow-evidence")
                            .unwrap()
                            .clone(),
                    )
                    .unwrap();
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let foreign = ["foreign-tenant".into(), root.scope[1].clone()];
                assert!(
                    tx.workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &foreign,
                        through + 1,
                        &mut session
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .is_none()
                );
                let live_before = arena.usage().unwrap().live_bytes;
                let retained = tx
                    .workflow_financial_document(
                        &reference,
                        &root.logical_store_id,
                        &root.scope,
                        after + 1,
                        &mut session,
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .unwrap();
                assert_eq!(
                    retained.schema.bytes(),
                    b"ledger-workflow-retained-evidence/1"
                );
                let body = canonical::parse_bounded(retained.body.bytes(), 4194304).unwrap();
                assert_eq!(
                    body["content_sha256"],
                    resolved.wrappers().original_request().value()["payload"]["content_sha256"]
                );
                assert!(arena.usage().unwrap().live_bytes > live_before);
                drop(retained);
                assert_eq!(arena.usage().unwrap().live_bytes, live_before);
                drop(resolved);
            }
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(
                prefix::next(tx, &root, after, through, &mut session)
                    .await?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    // This test qualifies mechanical chronological acquisition only. It creates
    // neither an AUTH02 CompletedOrigin nor a financial receipt/candidate.
    ledger.close().await;
}

#[tokio::test]
async fn actual_party_retention_derives_custody_bytes_and_refuses_without_append() {
    use ledgerlab_core::{
        canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession,
    };
    let (_directory, ledger, _path) = installation("actual-party-input").await;
    let operation = key("actual-party-input", "party-authorization-proof");
    let material = b"Explicit local operator representation material";
    let spec = json!({"authorization_id":"platform-owner-mapping","scope":operation.scope,
        "customer":"customer-1","agreement_id":"agreement-1","party":"platform-1",
        "acceptor":"platform-representative","roles":["bearer","payer"],
        "starts_at":"2026-01-01T00:00:00.000000Z","ends_at":"2099-01-01T00:00:00.000000Z"});
    let mut input = envelope(&operation);
    input["kind"] = json!("evidence.retain");
    input["payload"] = json!({"schema":"ledger-workflow-party-evidence-retain/1",
        "subject":{"customer":"customer-1","agreement_id":"agreement-1","party":"platform-1","acceptor":"platform-representative"},
        "media_type":"text/plain;charset=utf-8","content_base64":super::super::super::publication::base64(material),
        "content_sha256":records::raw_hash(material),"metadata":{"purpose":"party_authorization","authorization":spec}});
    let raw = records::bytes(&input).unwrap();
    let arena = AuditArena::new().unwrap();
    let baseline = arena.usage().unwrap().live_bytes;
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let before = tx.workflow_test_meter().await.unwrap();
    proof_session::with_proofs(&mut tx, &operation, |tx| {
        Box::pin(async {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let request = ingress::IncomingRequest::parse(&raw, &mut session)?;
            let pending =
                current::party::produce::prepare(&ledger, tx, &request, &mut session).await?;
            for record in [&pending.root, &pending.authority, &pending.result] {
                let body: Value = serde_json::from_slice(record.bytes()).unwrap();
                let expected = records::source_document(&record.reference().kind, body).unwrap();
                assert_eq!(record.bytes(), expected.bytes);
                assert_eq!(record.reference().kind, expected.reference.kind);
                assert_eq!(record.reference().id, expected.reference.id);
                assert_eq!(record.reference().digest, expected.reference.digest);
            }
            let body: Value = serde_json::from_slice(pending.result.bytes()).unwrap();
            assert_eq!(body["retained_by"], "example-operator");
            assert_eq!(body["installation_mode"], "real_local");
            assert_eq!(body["scope"], json!(operation.scope));
            assert_eq!(body["metadata"], input["payload"]["metadata"]);
            assert_eq!(body["retained_at"], json!(pending.at));
            assert_eq!(pending.dependency_refs().count(), 0);
            assert_eq!(
                pending.normalized_payload(),
                records::bytes(&input["payload"]).unwrap()
            );
            drop(pending);
            drop(request);
            drop(session);
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    assert_eq!(
        ledger
            .financial(&records::bytes(&input).unwrap())
            .await
            .unwrap_err(),
        ServiceError::Unavailable
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    // The empty-prefix case covers only a recipe with no retained operands.
    // A genuine immediate target cannot disappear into that representation.
    let mut missing = input.clone();
    missing["payload"]["metadata"] = json!({
        "purpose":"party_operator_attestation",
        "statement":"operator_verified_party_acceptor_authority",
        "target_evidence_ref":{"kind":"workflow-evidence","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))}
    });
    assert_eq!(
        ledger
            .financial(&records::bytes(&missing).unwrap())
            .await
            .unwrap_err(),
        reject("AUTHORITY_REQUIRED")
    );
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    assert_eq!(tx.workflow_test_meter().await.unwrap(), before);
    tx.rollback().await.unwrap();
    input["payload"]["content_sha256"] = json!("0".repeat(64));
    assert!(
        ledger
            .financial(&records::bytes(&input).unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        ledger.status(&operation.bytes().unwrap()).await.unwrap()["status"],
        "not_observed"
    );
    ledger.close().await;
}

#[tokio::test]
async fn chronological_fold_replays_real_administrative_source_chain_under_writer() {
    let (_dir, ledger, _path) = installation("chronological-real-source").await;
    let envelope = |id: &str, kind: &str, payload: Value| json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":kind,"key":key("chronological-real-source",id),"payload":payload});
    let retain = |id: &str, purpose: &str, metadata: Value| {
        envelope(
            id,
            "evidence.retain",
            json!({"schema":"ledger-workflow-evidence-retain/1","purpose":purpose,"subject":{"customer":"fold-customer"},"media_type":"text/plain;charset=utf-8","content_base64":"eA==","content_sha256":records::raw_hash(b"x"),"metadata":metadata}),
        )
    };
    let customer_proof = ledger
        .prerequisite(
            &records::bytes(&retain(
                "customer-proof",
                "customer_registration",
                json!({"acceptor":"fold-acceptor"}),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let registered=ledger.prerequisite(&records::bytes(&envelope("customer","authority.change",json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":"fold-customer","acceptor":"fold-acceptor","registration_evidence_ref":customer_proof["receipt"]["result_refs"][0]}))).unwrap()).await.unwrap();
    let source_proof = ledger
        .prerequisite(
            &records::bytes(&retain(
                "source-proof",
                "source_enrollment",
                json!({"source":"urn:example:work","principal_id":"example-operator"}),
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    let ceiling = json!({"principal_id":"example-operator","source":"urn:example:work","canonical_economic_namespace":"urn:example:work","customer_scope":["fold-customer"],"operations":["work.accept","quantity.correct"],"product_selectors":[{"names":["fold:product"]}],"activity_selectors":[{"names":["fold:work"]}],"link_selectors":[],"source_slots":["fold:worker"],"permissions":["read","submit","correct"],"starts_at":"2026-01-01T00:00:00Z","ends_at":"2030-01-01T00:00:00Z"});
    let enrolled=ledger.prerequisite(&records::bytes(&envelope("enroll","authority.change",json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"fold-source","customer_refs":[registered["receipt"]["result_refs"][0]],"ceiling":ceiling,"evidence_ref":source_proof["receipt"]["result_refs"][0]}))).unwrap()).await.unwrap();
    assert_eq!(enrolled["receipt"]["commit_sequence"], "4");
    let selected = key("chronological-real-source", "enroll");
    let raw_key = records::bytes(&serde_json::to_value(&selected).unwrap()).unwrap();
    let mut tx = ledger.store.begin(deadline()).await.unwrap();
    let failure = tx.workflow_proof_failure_handle();
    proof_session::with_proofs(&mut tx, &selected, |tx| {
        Box::pin(async {
            let arena =
                ledgerlab_core::canonical::audit_arena::AuditArena::new().map_err(core_error)?;
            let mut session =
                ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(
                    &arena,
                )
                .map_err(core_error)?;
            let identity = session
                .copy_representation(
                    &raw_key,
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let selected_origin = prefix::target(tx, &selected, identity, &mut session)
                .await?
                .checked_wrappers(tx, &mut session)
                .await?;
            let mut folded = prefix::fold_original_prefix(
                &ledger,
                tx,
                &selected,
                &selected_origin,
                &mut session,
            )
            .await?;
            // This lookup can return only the live key associated AFTER the
            // actual enrollment origin and its required-edge/pointer finish.
            let enrollment_ref = &enrolled["receipt"]["result_refs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["kind"] == "workflow-source-enrollment")
                .unwrap();
            {
                let correspondence = std::ptr::from_ref(
                    folded.initial_source_correspondence(enrollment_ref, &mut session)?,
                );
                assert_eq!(
                    correspondence,
                    std::ptr::from_ref(
                        folded.initial_source_correspondence(enrollment_ref, &mut session)?
                    )
                );
            }
            // These pointers come from the actual four public committed
            // prerequisite recipes, after their original semantic/custody fold.
            // They are not a financial witness membership or audit digest.
            for (sequence, id) in [
                (1, "customer-proof"),
                (2, "customer"),
                (3, "source-proof"),
                (4, "enroll"),
            ] {
                let raw = folded._inventory.origin_pointer(sequence, &mut session)?;
                let pointer = session
                    .parse_representation(
                        raw.bytes(),
                        ledgerlab_core::validation_work::RepresentationKind::Record,
                    )
                    .map_err(core_error)?;
                assert_eq!(pointer.value()["commit_sequence"], sequence.to_string());
                assert_eq!(pointer.value()["operation_key"]["operation_id"], id);
                assert_eq!(
                    pointer.value()["operation_key"]["logical_store_id"],
                    "chronological-real-source"
                );
                assert_eq!(
                    pointer.value()["operation_ref"]["kind"],
                    "workflow-operation"
                );
                assert_eq!(pointer.value()["manifest_ref"]["kind"], "workflow-manifest");
                assert_eq!(pointer.value()["receipt_ref"]["kind"], "workflow-receipt");
            }
            // Adjacencies come from the Ref/body associations checked by
            // those same recipes, not from an asserted pointer list. Every
            // edge goes strictly backward; own introductions add no edge.
            for (sequence, required) in
                [(1, &[][..]), (2, &[1][..]), (3, &[][..]), (4, &[2, 3][..])]
            {
                let links = folded._inventory.origin_links(sequence, &mut session)?;
                let n = u32::from_le_bytes(links.bytes()[..4].try_into().unwrap()) as usize;
                let middle = 8 + 8 * n;
                let mut values: Vec<u32> = links.bytes()[8..middle]
                    .chunks_exact(8)
                    .flat_map(|b| {
                        let word = u32::from_le_bytes(b[..4].try_into().unwrap());
                        let mask = u32::from_le_bytes(b[4..8].try_into().unwrap());
                        (0..32)
                            .filter(move |bit| mask & (1 << bit) != 0)
                            .map(move |bit| word * 32 + bit + 1)
                    })
                    .collect();
                values.extend(
                    links.bytes()[middle..]
                        .chunks_exact(256)
                        .map(|b| u32::from_le_bytes(b[228..232].try_into().unwrap())),
                );
                assert!(
                    values
                        .iter()
                        .all(|prior| *prior > 0 && *prior < sequence as u32)
                );
                for prior in required {
                    assert!(values.contains(prior));
                }
                // Later operations can reuse an earlier exact immutable
                // authority body; its earliest origin remains an extra edge.
                if sequence == 1 {
                    assert!(values.is_empty());
                }
            }
            let first = prefix::next(tx, &selected, 0, 1, &mut session)
                .await?
                .ok_or(ServiceError::IntegrityFailure)?
                .checked_wrappers(tx, &mut session)
                .await?;
            let root = &first.wrappers().operation()["authority_snapshot"]["authority_refs"][0];
            // Independent retained SQL meter + exact manifest sets check the
            // append-derived accounting, including reused documents once.
            let expected_meter = tx
                .workflow_test_meter()
                .await
                .map_err(crate::service::store_error)?;
            let mut expected_documents = std::collections::BTreeSet::new();
            for sequence in 1..=4 {
                let saved = prefix::next(tx, &selected, sequence - 1, sequence, &mut session)
                    .await?
                    .ok_or(ServiceError::IntegrityFailure)?
                    .checked_wrappers(tx, &mut session)
                    .await?;
                for member in saved.wrappers().manifest()["members"].as_array().unwrap() {
                    if member["kind"] != "workflow-operation" {
                        expected_documents.insert(records::bytes(member).unwrap());
                    }
                }
            }
            let retained_root = current::source::retained_record(
                tx,
                &selected,
                root,
                "workflow-authority",
                "ledger-workflow-local-authority-snapshot/1",
                5,
                &mut session,
            )
            .await?;
            let root_body = session
                .copy_representation(
                    retained_root.canonical_body(),
                    ledgerlab_core::validation_work::RepresentationKind::Record,
                )
                .map_err(core_error)?;
            let accounting_root = wire::source_record(
                "workflow-authority",
                "ledger-workflow-local-authority-snapshot/1",
                root_body,
                &mut session,
            )?;
            let cut = prefix::inventory::ConfiguredRoot::from_value(root, &mut session)?;
            assert_eq!(
                folded._inventory.seed_origin(root, &cut, &mut session)?,
                None
            );
            assert_eq!(
                folded._inventory.seed_origin(
                    &registered["receipt"]["result_refs"][0],
                    &cut,
                    &mut session
                )?,
                Some(2)
            );
            let _reader = session
                .reserve_external_payload(prefix::inventory::REQUIRED_EDGE_READER_PAYLOAD)
                .map_err(core_error)?;
            // These genuine committed prefixes use dense sequences1..4;
            // the stored masks themselves are ordinal keyed, not sequence keyed.
            for (ordinal, expected) in [(1, vec![1]), (2, vec![])] {
                let mut direct = Vec::new();
                folded._inventory.required_word_masks_at(
                    ordinal,
                    &cut,
                    &mut |word, mask, _| {
                        for bit in 0..32 {
                            if mask & (1 << bit) != 0 {
                                direct.push((word * 32 + bit + 1) as i64);
                            }
                        }
                        Ok(())
                    },
                    &mut session,
                )?;
                assert_eq!(direct, expected);
            }
            folded.assert_packed_reachable(
                root,
                &[&registered["receipt"]["result_refs"][0]],
                &[0b11],
                &mut session,
            )?;
            folded.assert_packed_reachable(
                root,
                &[&enrolled["receipt"]["result_refs"][0]],
                &[0b1111],
                &mut session,
            )?;
            folded.assert_packed_accounting(
                &accounting_root,
                &[0b1111],
                (
                    expected_documents.len() as u64,
                    expected_meter.payload_bytes as u64,
                ),
                &mut session,
            )?;
            // A cut root outside an empty selected set is included exactly
            // once; neither unrelated journal bytes nor its origin are added.
            folded.assert_packed_accounting(
                &accounting_root,
                &[0],
                (1, retained_root.canonical_body().len() as u64),
                &mut session,
            )?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(!failure.load(Ordering::Acquire));
    tx.rollback().await.unwrap();
    // This changed-route check creates no economic/prepared/financial result.
    assert_eq!(
        ledger.status(&raw_key).await.unwrap()["receipt"],
        enrolled["receipt"]
    );
}
