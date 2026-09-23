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
