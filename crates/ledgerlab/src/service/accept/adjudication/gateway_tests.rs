//! Production offline facade with all peers closed and real process publication cuts.
use super::*;
use crate::gateway::{GatewayConfig, GatewayContext, OfflineGateway};

async fn offline_setup() -> (
    Harness,
    GatewayConfig,
    GatewayContext,
    wire::Delivery,
    wire::Receive,
) {
    let mut h = Harness::new().await;
    for i in 5..9 {
        h.step(h.input["commands"][i].clone()).await;
    }
    let source = &h.host.0.sources[0];
    let body: Value =
        serde_json::from_slice(&r3::proofs::decode_base64(&source.body, 16384).unwrap()).unwrap();
    let config = GatewayConfig {
        database: h._dirs[2].0.path().into(),
        anchor: h._dirs[2].1.path().into(),
        central_store: Id::parse("center").unwrap(),
        scope: journal("g1").scope,
        registration: Id::parse("registration").unwrap(),
        gateway: Id::parse("g1").unwrap(),
        resources: flow_budget("g1"),
        legacy_pages: 65536,
        backing_bytes: Count::new(1u128 << 40).unwrap(),
        authority_source: serde_json::from_value(body["source"].clone()).unwrap(),
        authority_id: serde_json::from_value(body["id"].clone()).unwrap(),
    };
    let context = GatewayContext {
        principal: serde_json::from_value(body["principal"].clone()).unwrap(),
        observed_at: h.host.0.now.clone(),
    };
    let key = serde_json::from_value(h.input["commands"][9]["key"].clone()).unwrap();
    let payload = serde_json::from_value(h.input["commands"][9]["payload"].clone()).unwrap();
    for store in std::mem::take(&mut h.host.0.stores).into_values() {
        store.close().await;
    }
    (h, config, context, key, payload)
}
#[tokio::test]
async fn actual_offline_gateway_receipt_retry_status_and_reopen() {
    let (_h, config, context, key, payload) = offline_setup().await;
    let gateway = OfflineGateway::open(config.clone()).await.unwrap();
    assert!(OfflineGateway::open(config.clone()).await.is_err());
    assert!(gateway
        .status(key.clone(), context.clone(), Duration::from_secs(5))
        .await
        .unwrap()
        .is_none());
    let optional = gateway.test_store().test_hold_optional_slots().await;
    let receipt = gateway
        .receive(
            key.clone(),
            payload.clone(),
            context.clone(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(receipt.status, wire::CommandResultStatus::Committed);
    // Independently accepted actual customer step 9; the public offline entry
    // point produces the exact same committed canonical transition.
    assert_eq!(
        receipt.root.as_str(),
        "02b4c8a5b5402bf4a81b2c3168faa4028cdf52d2cc1839fa87c8b52ddaec2ea0"
    );
    assert!(matches!(
        receipt.effects.as_slice(),
        [wire::Effect::Receipt { .. }]
    ));
    let before = gateway.test_store().test_full_inventory().await;
    let retry = gateway
        .receive(
            key.clone(),
            payload.clone(),
            context.clone(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(retry.status, wire::CommandResultStatus::Duplicate);
    assert_eq!(retry.effects, receipt.effects);
    let status = gateway
        .status(key.clone(), context.clone(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.effects, receipt.effects);
    let mut wrong = context.clone();
    wrong.principal = Id::parse("other-principal").unwrap();
    assert!(gateway
        .status(key.clone(), wrong, Duration::from_secs(5))
        .await
        .is_err());
    assert_eq!(gateway.test_store().test_full_inventory().await, before);
    drop(optional);
    gateway.close().await;
    let gateway = OfflineGateway::open(config).await.unwrap();
    assert_eq!(
        gateway
            .status(key, context, Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap()
            .effects,
        receipt.effects
    );
    assert_eq!(gateway.test_store().test_full_inventory().await, before);
    gateway.close().await;
}
#[tokio::test]
async fn gateway_process_entry() {
    let Ok(path) = std::env::var("LEDGERLAB_GATEWAY_PROCESS") else {
        return;
    };
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let gateway = OfflineGateway::open(serde_json::from_value(v["config"].clone()).unwrap())
        .await
        .unwrap();
    gateway
        .test_store()
        .test_publication_cut(v["cut"].as_u64().unwrap() as u8);
    let result = gateway
        .receive(
            serde_json::from_value(v["key"].clone()).unwrap(),
            serde_json::from_value(v["payload"].clone()).unwrap(),
            serde_json::from_value(v["context"].clone()).unwrap(),
            Duration::from_secs(30),
        )
        .await;
    panic!("publication cut failed to exit: {result:?}");
}
#[tokio::test]
async fn actual_offline_gateway_process_cuts_recover_receipt_identity() {
    for cut in [11, 12, 13] {
        let (_h, config, context, key, payload) = offline_setup().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.json");
        std::fs::write(
            &path,
            serde_json::to_vec(
                &json!({"config":config,"context":context,"key":key,"payload":payload,"cut":cut}),
            )
            .unwrap(),
        )
        .unwrap();
        let output=std::process::Command::new(std::env::current_exe().unwrap()).arg("--exact").arg("service::accept::adjudication::sqlite_tests::remaining_tests::gateway_tests::gateway_process_entry").arg("--nocapture").env("LEDGERLAB_GATEWAY_PROCESS",&path).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "child cut{cut}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let gateway = OfflineGateway::open(config).await.unwrap();
        let status = gateway
            .status(key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(status.is_some(), cut != 11);
        let result = gateway
            .receive(
                key.clone(),
                payload,
                context.clone(),
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert_eq!(
            result.status,
            if cut == 11 {
                wire::CommandResultStatus::Committed
            } else {
                wire::CommandResultStatus::Duplicate
            }
        );
        let stats = gateway
            .test_store()
            .test_adjudication_stats(&journal("g1"))
            .await;
        assert_eq!(stats["segments"], 4);
        let saved = gateway
            .status(key, context, Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.effects, result.effects);
        eprintln!("offline gateway actual process cut{cut}: four exact segments, one receipt, savedidentity PASS");
        gateway.close().await;
    }
}

// A disconnected receipt lookup must remain truthful as the actual central
// journal advances through admission, settlement, seal, closure and correction.
#[tokio::test]
async fn actual_offline_receipt_lifecycle_stays_unknown_across_customer_history() {
    let (mut h, config, context, key, payload) = offline_setup().await;
    h.reopen().await;
    let mut original = None;
    for i in 9..95 {
        let result = h.step(h.input["commands"][i].clone()).await;
        if i == 9 {
            let [wire::Effect::Receipt { body }] = result.effects.as_slice() else {
                panic!("receipt");
            };
            original = Some(body.clone());
        }
        if ![9, 10, 11, 31, 73, 82, 85, 91, 92, 93, 94].contains(&i) {
            continue;
        }
        let before = h.host.0.stores["g1"].test_full_inventory().await;
        for store in std::mem::take(&mut h.host.0.stores).into_values() {
            store.close().await;
        }
        // All four peers are closed. The facade has no transport or central
        // lifecycle cache; even a genuine unseen decision cannot become a guess.
        let gateway = OfflineGateway::open(config.clone()).await.unwrap();
        let status = gateway
            .receipt_status(key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&status.receipt, original.as_ref().unwrap());
        assert_eq!(status.knowledge, wire::RetryResponseKnowledge::Unknown);
        assert_eq!(
            status.current_lifecycle,
            wire::RetryResponseCurrentLifecycle::Unknown
        );
        assert_eq!(
            status.central_admission,
            wire::RetryResponseCentralAdmission::Unknown
        );
        assert!(status.prefix.is_none());
        assert!(status.coverage.is_empty());
        assert_eq!(
            gateway
                .receive_receipt(
                    key.clone(),
                    payload.clone(),
                    context.clone(),
                    Duration::from_secs(30)
                )
                .await
                .unwrap(),
            status
        );
        let mut wrong = context.clone();
        wrong.principal = Id::parse("unauthorized").unwrap();
        assert!(gateway
            .receipt_status(key.clone(), wrong, Duration::from_secs(30))
            .await
            .is_err());
        assert_eq!(gateway.test_store().test_full_inventory().await, before);
        gateway.close().await;
        h.reopen().await;
        assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, before);
        eprintln!("offline truthful retry after actual customer step{i}: exact original receipt, UNKNOWN central lifecycle/admission, unchanged inventory and reopen");
    }
    h.close().await;
}

#[tokio::test]
async fn actual_offline_receipt_alias_uses_second_token_and_preserves_first_receipt() {
    let (mut h, config, context, key, payload) = offline_setup().await;
    h.reopen().await;
    for i in 26..30 {
        h.step(h.input["commands"][i].clone()).await;
    }
    let alias_key: wire::Delivery =
        serde_json::from_value(h.input["commands"][30]["key"].clone()).unwrap();
    let mut alias: wire::Receive =
        serde_json::from_value(h.input["commands"][30]["payload"].clone()).unwrap();
    alias.submission = payload.submission.clone();
    for store in std::mem::take(&mut h.host.0.stores).into_values() {
        store.close().await;
    }
    let gateway = OfflineGateway::open(config.clone()).await.unwrap();
    assert!(gateway
        .receipt_status(key.clone(), context.clone(), Duration::from_secs(30))
        .await
        .unwrap()
        .is_none());
    let fresh_inventory = gateway.test_store().test_full_inventory().await;
    let mut mismatched = payload.clone();
    mismatched.delivery.2 = Id::parse("different-delivery").unwrap();
    assert!(
        matches!(gateway.receive_receipt(key.clone(), mismatched.clone(), context.clone(), Duration::from_secs(30)).await,
        Err(ServiceError::Rejection(code)) if code == "TOKEN_STATE")
    );
    assert_eq!(
        gateway.test_store().test_full_inventory().await,
        fresh_inventory
    );
    let original = gateway
        .receive_receipt(
            key.clone(),
            payload.clone(),
            context.clone(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(original.knowledge, wire::RetryResponseKnowledge::Unknown);
    assert_eq!(
        original.current_lifecycle,
        wire::RetryResponseCurrentLifecycle::Unknown
    );
    assert_eq!(
        original.central_admission,
        wire::RetryResponseCentralAdmission::Unknown
    );
    assert!(original.prefix.is_none() && original.coverage.is_empty());
    let accepted_inventory = gateway.test_store().test_full_inventory().await;
    assert!(
        matches!(gateway.receive_receipt(key.clone(), mismatched, context.clone(), Duration::from_secs(30)).await,
        Err(ServiceError::Rejection(code)) if code == "TOKEN_STATE")
    );
    assert_eq!(
        gateway.test_store().test_full_inventory().await,
        accepted_inventory
    );
    let segments = gateway
        .test_store()
        .test_adjudication_stats(&journal("g1"))
        .await["segments"];
    let aliased = gateway
        .receive_receipt(
            alias_key.clone(),
            alias.clone(),
            context.clone(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(aliased, original);
    assert_ne!(aliased.receipt.token, alias.token);
    assert_ne!(aliased.receipt.delivery, alias_key);
    assert_eq!(
        gateway
            .test_store()
            .test_adjudication_stats(&journal("g1"))
            .await["segments"],
        segments + 1
    );
    let before = gateway.test_store().test_full_inventory().await;
    assert_eq!(
        gateway
            .receive_receipt(
                alias_key.clone(),
                alias.clone(),
                context.clone(),
                Duration::from_secs(30)
            )
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        gateway
            .receipt_status(alias_key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap(),
        Some(original.clone())
    );
    let mut changed = alias.clone();
    changed.submission.case.2 = Id::parse("changed-case").unwrap();
    assert!(matches!(
        gateway
            .receive_receipt(
                alias_key.clone(),
                changed,
                context.clone(),
                Duration::from_secs(30)
            )
            .await,
        Err(ServiceError::Rejection(_))
    ));
    let mut expired = context.clone();
    let authority: Value = serde_json::from_slice(
        &r3::proofs::decode_base64(&h.host.0.sources[0].body, 16384).unwrap(),
    )
    .unwrap();
    expired.observed_at = serde_json::from_value(authority["ends_at"].clone()).unwrap();
    assert!(gateway
        .receipt_status(alias_key.clone(), expired, Duration::from_secs(30))
        .await
        .is_err());
    assert_eq!(gateway.test_store().test_full_inventory().await, before);
    gateway.close().await;
    let gateway = OfflineGateway::open(config).await.unwrap();
    assert_eq!(
        gateway
            .receipt_status(alias_key, context, Duration::from_secs(30))
            .await
            .unwrap(),
        Some(original)
    );
    assert_eq!(gateway.test_store().test_full_inventory().await, before);
    gateway.close().await;
    eprintln!("offline alias: second prepaid token, one new alias segment, identical first receipt/time/position, immutable retries and reopen; occupied-key and expired-authority refusals");
}

#[tokio::test]
async fn actual_offline_receipt_refuses_unused_and_frozen_tokens_without_mutation() {
    for state in ["RETURNED_UNUSED", "SEALING", "SEALED"] {
        let (mut h, config, context, key, payload) = offline_setup().await;
        h.reopen().await;
        if state != "RETURNED_UNUSED" {
            h.begin(1, "FINISH_ONLY").await;
            h.seal_begin(1, 0).await;
        }
        if state != "SEALING" {
            let proof = h.proof("center", "CLAIM", json!(payload.token));
            let claim = h.input["commands"][7]["payload"]["token"]["claim"].clone();
            h.command_step(
                "RETURN_UNUSED",
                json!({"gateway":"g1","token":payload.token,"claim":claim,"proof":proof}),
            )
            .await;
        }
        if state == "SEALED" {
            h.sealed(1).await;
        }
        let before = h.host.0.stores["g1"].test_full_inventory().await;
        for store in std::mem::take(&mut h.host.0.stores).into_values() {
            store.close().await;
        }
        let gateway = OfflineGateway::open(config.clone()).await.unwrap();
        assert!(
            matches!(gateway.receive_receipt(key.clone(), payload.clone(), context.clone(), Duration::from_secs(30)).await,
            Err(ServiceError::Rejection(code)) if code == "TOKEN_STATE")
        );
        assert!(gateway
            .receipt_status(key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap()
            .is_none());
        assert_eq!(gateway.test_store().test_full_inventory().await, before);
        gateway.close().await;
        let gateway = OfflineGateway::open(config).await.unwrap();
        assert!(gateway
            .receipt_status(key, context, Duration::from_secs(30))
            .await
            .unwrap()
            .is_none());
        assert_eq!(gateway.test_store().test_full_inventory().await, before);
        gateway.close().await;
        eprintln!("offline {state}: refused new receipt, no invented saved result, unchanged inventory after reopen");
    }
}
