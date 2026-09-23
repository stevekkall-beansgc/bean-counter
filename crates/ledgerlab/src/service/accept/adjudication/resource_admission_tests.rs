//! Six independently isolated admission frontiers using fresh real histories.
use super::*;
#[tokio::test]
async fn actual_each_resource_dimension_exhaustion_preserves_outstanding_token_finish() {
    let worksheet = Worksheet::frozen().unwrap();
    let mut bundle = wire::Resource::zero();
    for slot in worksheet.bundle("local_grant").unwrap() {
        let mut cost = worksheet.template(slot).unwrap().resources().unwrap();
        let peak = cost.workspace_bytes;
        cost.workspace_bytes = Count::ZERO;
        bundle = bundle.checked_add(&cost).unwrap();
        bundle.workspace_bytes = bundle.workspace_bytes.max(peak);
    }
    for dimension in 0..6 {
        for short in [true, false] {
            let mut budgets = super::super::scale_tests::budgets(2);
            let mut values = budgets["g1"].dimensions();
            // PREPARE's transient workspace was fully released in its creating
            // commit. Only the 32 finish peaks and two grant peaks coexist.
            values[5] = Count::new(
                values[5].value()
                    - worksheet
                        .template("PREPARE_ENROLL")
                        .unwrap()
                        .resources()
                        .unwrap()
                        .workspace_bytes
                        .value(),
            )
            .unwrap();
            if short {
                values[dimension] = Count::new(values[dimension].value() - 1).unwrap();
            }
            budgets.insert("g1".into(), wire::Resource::from_dimensions(values));
            let mut h = Harness::with_budgets(budgets, false, true).await;
            h.grant_and_register(1).await;
            let c = h.issue_command(1);
            h.step(c).await;
            // Observe retained account bytes directly; the general test point
            // helper quotes an entire ENROLL read, larger than this gateway's
            // deliberately minimal index-value allocation.
            let observed = h.host.0.stores["g1"].test_full_inventory().await;
            let journals = super::super::owner_accounting_tests::heads(&observed);
            let value = journals
                .values()
                .next()
                .unwrap()
                .iter()
                .find(|v| v["kind"] == "Resource")
                .unwrap();
            let State::Resource(account) = serde_json::from_value(value.clone()).unwrap() else {
                panic!("resource");
            };
            for (i, ((provisioned, used), held)) in account
                .provisioned
                .dimensions()
                .into_iter()
                .zip(account.used.dimensions())
                .zip(account.held.dimensions())
                .enumerate()
            {
                let slack = provisioned.value() - used.value() - held.value();
                assert_eq!(
                    slack + u128::from(short && dimension == i),
                    bundle.dimensions()[i].value(),
                    "sole admission deficit {dimension}/{i}"
                );
            }
            let before = h.host.0.stores["g1"].test_full_inventory().await;
            let root = h.roots["g1"].clone();
            let mut payload = h.input["commands"][5]["payload"].clone();
            payload["grant"]["id"] = json!(h.grant_name(2));
            let grant = h.command("LOCAL_GRANT", payload);
            if short {
                h.refuses_unchanged("g1", grant, "UNFUNDED").await;
                assert_eq!(h.roots["g1"], root);
                assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, before);
            } else {
                h.step(grant).await;
                let proof = h.proof("g1", "GRANT", json!(h.grant_name(2)));
                h.command_step(
                    "REGISTER_GRANT",
                    json!({"grant":h.input["commands"][5]["payload"]["grant"],"proof":proof}),
                )
                .await;
                h.command_step("RETIRE_GRANT", json!({"grant":h.grant_name(2)}))
                    .await;
                let proof = h.proof("center", "RETIREMENT", json!(h.grant_name(2)));
                h.command_step(
                    "LOCAL_TERMINAL",
                    json!({"gateway":"g1","grant":h.grant_name(2),"proof":proof}),
                )
                .await;
            }
            h.assert_all_owner_accounts().await;
            let c = h.return_command(1);
            h.step(c).await;
            h.settle_token(1, false, None).await;
            h.finish(1, 0).await;
            h.reopen().await;
            h.assert_all_owner_accounts().await;
            h.close().await;
            eprintln!("actual resource frontier dimension{dimension} short{short}:exact sole deficit, optionalgrant {}, outstandingtoken prepaid return/settlement/finish/reopen", if short {"refused unchanged"} else {"admitted and retired"});
        }
    }
}
