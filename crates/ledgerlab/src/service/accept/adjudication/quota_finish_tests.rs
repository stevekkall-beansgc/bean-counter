//! Every distinct prepaid FINISH transition, with actual child process exit at
//! before publication / prepared commit / published-before-response boundaries.
use super::*;
#[tokio::test]
async fn actual_all_finish_kinds_recover_after_optional_grant_exhaustion() {
    let kinds = [
        "BEGIN",
        "SEAL_BEGIN",
        "SEALED",
        "DRAIN",
        "READY",
        "CLOSE",
        "INSTALL",
        "ACK_INSTALL",
    ];
    let raw: Value = serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/protocol/resources.json")).unwrap();
    let expected: std::collections::BTreeSet<_> = ["finish_central", "finish_gateway"]
        .into_iter()
        .flat_map(|bundle| {
            raw["bundles"][bundle]["slots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
        })
        .collect();
    assert_eq!(expected, kinds.into_iter().collect());
    for selected in kinds {
        for cut in [11, 12, 13] {
            let mut h =
                Harness::with_budgets(super::super::super::scale_tests::budgets(2), false, true)
                    .await;
            for i in (5..10).chain(26..29) {
                h.step(h.input["commands"][i].clone()).await;
            }
            let mut payload = h.input["commands"][5]["payload"].clone();
            payload["grant"]["id"] = json!("gr1.22222222222222222222222222222222.exhausted");
            let c = h.command("LOCAL_GRANT", payload);
            let before = h.host.0.stores["g1"].test_full_inventory().await;
            h.execute(c, Some("UNFUNDED")).await;
            assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, before);
            h.assert_all_owner_accounts().await;
            let c = h.full_begin_command(1, "FINISH_ONLY", vec![]);
            h.cut_or_step("center", c, selected, cut).await;
            for gateway in ["g0", "g1", "g2", "g3"] {
                let proof = h.proof("center", "BEGIN", json!("1"));
                let c = h.command(
                    "SEAL_BEGIN",
                    json!({"gateway":gateway,"round":"1","predecessor":"0","proof":proof}),
                );
                if gateway == "g1" {
                    h.cut_or_step(gateway, c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
            }
            let proof = h.proof("center", "CLAIM", json!("token4"));
            let claim = h.input["commands"][28]["payload"]["token"]["claim"].clone();
            h.command_step(
                "RETURN_UNUSED",
                json!({"gateway":"g1","token":"token4","claim":claim,"proof":proof}),
            )
            .await;
            for i in [10, 44, 45] {
                h.step(h.input["commands"][i].clone()).await;
            }
            let proof = h.proof("g1", "RETURNED_UNUSED", json!("token4"));
            h.command_step("RECONCILE", json!({"token":"token4","proof":proof}))
                .await;
            for i in [51, 60, 61, 62] {
                h.step(h.input["commands"][i].clone()).await;
            }
            for gateway in ["g0", "g1", "g2", "g3"] {
                let c = h.command("SEALED", json!({"gateway":gateway,"round":"1"}));
                if gateway == "g1" {
                    h.cut_or_step(gateway, c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
                let proof = h.proof(gateway, "SEAL", json!("1"));
                let c = h.command(
                    "DRAIN",
                    json!({"gateway":gateway,"round":"1","proof":proof}),
                );
                if gateway == "g1" {
                    h.cut_or_step("center", c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
            }
            let c = h.command("READY", json!({"round":"1"}));
            h.cut_or_step("center", c, selected, cut).await;
            let c = h.terminal_command("CLOSE", 1);
            h.cut_or_step("center", c, selected, cut).await;
            for gateway in ["g0", "g1", "g2", "g3"] {
                let c = h.full_install_command(1, gateway, "COMMITTED");
                if gateway == "g1" {
                    h.cut_or_step(gateway, c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
                let proof = h.proof(gateway, "INSTALLATION", json!("1"));
                let c = h.command(
                    "ACK_INSTALL",
                    json!({"gateway":gateway,"round":"1","proof":proof}),
                );
                if gateway == "g1" {
                    h.cut_or_step("center", c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
            }
            h.reopen().await;
            h.assert_actual_accounts().await;
            h.close().await;
            eprintln!("actual exhausted optional grant capacity: {selected} cut{cut}; consumed receipt and never-delivered token disposed; fivefamilies/fourgateway prepaidfinish/retry/reopen preserved");
        }
    }
}
