//! Missing transition pairs with controlled overlap on actual SQLite writers.
use super::*;
#[tokio::test]
async fn actual_ordered_activation_return_race_preserves_unused_tombstone() {
    for preferred in [0, 1] {
        let mut h = Harness::new().await;
        h.grant_and_register(1).await;
        let c = h.issue_command(1);
        h.step(c).await;
        let proof = h.proof("center", "CLAIM", json!("race1"));
        let activate = h.command(
            "ACTIVATE",
            json!({"gateway":"g1","token":"race1","proof":proof}),
        );
        let commands = [activate, h.return_command(1)];
        let (winner, saved, result) = h
            .race_ordered_admission("g1", commands.clone(), Some(preferred), true)
            .await;
        assert_eq!(winner, preferred);
        if winner == 0 {
            h.step(commands[1].clone()).await;
        } else {
            h.refuses_unchanged("g1", commands[0].clone(), "TOKEN_STATE")
                .await;
        }
        let (_, State::Token(t)) = h
            .state(
                "g1",
                HeadKind::Token,
                rt::points::Point::id(rt::points::PointKind::Token, *b"TOKEN___", "race1").unwrap(),
                GuardClass::CapacityAllocation,
            )
            .await
        else {
            panic!("token")
        };
        assert_eq!(t.status, rt::points::TokenStatus::ReturnedUnused);
        assert!(t.receipt.is_none() && t.submission.is_none() && t.delivery.is_none());
        assert_eq!(t.token.grant.as_str(), h.grant_name(1));
        let (_, State::Grant(g)) = h
            .state(
                "g1",
                HeadKind::Grant,
                rt::points::Point::id(rt::points::PointKind::Grant, *b"GRANT___", &h.grant_name(1))
                    .unwrap(),
                GuardClass::CapacityAllocation,
            )
            .await
        else {
            panic!("grant")
        };
        assert_eq!(g.token.as_ref().unwrap().as_str(), "race1");
        h.settle_token(1, false, None).await;
        h.finish(1, 0).await;
        h.reopen().await;
        h.retry_exact("g1", &saved, &result).await;
        let fresh = h.command("ACTIVATE", commands[0]["payload"].clone());
        h.refuses_unchanged("g1", fresh, "TOKEN_STATE").await;
        h.assert_all_owner_accounts().await;
        h.close().await;
        eprintln!("actual ordered ACTIVATE/RETURN_UNUSED winner{winner}: permanent unused token, no receipt, prepaid finish/reopen");
    }
}
#[tokio::test]
async fn actual_ordered_ordinary_award_close_serializes_entitlement() {
    for preferred in [0, 1] {
        let mut h = Harness::new().await;
        h.grant_and_register(1).await;
        let c = h.issue_command(1);
        h.step(c).await;
        h.activate_token(1).await;
        let receive = h.receive_command(1, 1);
        h.step(receive.clone()).await;
        h.settle_token(1, true, Some(1)).await;
        h.begin(1, "FINISH_ONLY").await;
        h.seal_begin(1, 0).await;
        h.sealed(1).await;
        h.drain(1).await;
        h.ready(1).await;
        let mut award = h.input["commands"][11].clone();
        award["payload"]["case"] = receive["payload"]["submission"]["case"].clone();
        award["key"][2] = json!("award-close-race");
        let commands = [award, h.terminal_command("CLOSE", 1)];
        let (winner, saved, result) = h
            .race_ordered("center", commands.clone(), Some(preferred))
            .await;
        assert_eq!(winner, preferred);
        if winner == 0 {
            h.step(commands[1].clone()).await;
        } else {
            h.refuses_unchanged("center", commands[0].clone(), "ORDINARY_CLOSED")
                .await;
        }
        let family: wire::Family =
            serde_json::from_value(h.enrollment["families"][0]["key"].clone()).unwrap();
        let (_, State::Family(f)) = h
            .state(
                "center",
                HeadKind::Family,
                rt::points::Point::family(&family).unwrap(),
                GuardClass::FamilyPrerequisite,
            )
            .await
        else {
            panic!("family")
        };
        assert!(f.closed && f.first_closure.is_some());
        assert_eq!(
            f.ordinary_positive.value(),
            if winner == 0 { 1200 } else { 0 }
        );
        assert_eq!(
            matches!(f.entitlement, wire::EntitlementHead::Consumed { .. }),
            winner == 0
        );
        let case: wire::Case =
            serde_json::from_value(receive["payload"]["submission"]["case"].clone()).unwrap();
        let (_, State::Case(c)) = h
            .state(
                "center",
                HeadKind::Case,
                rt::points::Point::case(&case).unwrap(),
                GuardClass::Case,
            )
            .await
        else {
            panic!("case")
        };
        assert_eq!(c.signed.value(), if winner == 0 { 1200 } else { 0 });
        assert_eq!(c.revision.value(), u128::from(winner == 0));
        h.install(1, "COMMITTED").await;
        h.reopen().await;
        h.retry_exact("center", &saved, &result).await;
        h.assert_all_owner_accounts().await;
        h.close().await;
        eprintln!("actual ordered ordinary ALLOW/CLOSE winner{winner}: closure lineage retained, one or zero award, no implicit adjustment, reopen/retry");
    }
}
#[tokio::test]
async fn actual_ordered_independent_entitlements_share_one_adjustment_pool() {
    for preferred in [0, 1] {
        let mut h = Harness::with_budgets(
            ["center", "g0", "g1", "g2", "g3"]
                .into_iter()
                .map(|s| (s.into(), flow_budget(s)))
                .collect(),
            false,
            true,
        )
        .await;
        for index in 5..91 {
            h.step(h.input["commands"][index].clone()).await;
        }
        let first = h.input["commands"][91].clone();
        let mut second = first.clone();
        second["key"][2] = json!("shared-positive-pool-competitor");
        second["payload"]["case"] = h.input["commands"][93]["payload"]["case"].clone();
        let commands = [first, second];
        let mut prior_cases = Vec::new();
        for command in &commands {
            let case: wire::Case =
                serde_json::from_value(command["payload"]["case"].clone()).unwrap();
            prior_cases.push(
                h.state(
                    "center",
                    HeadKind::Case,
                    rt::points::Point::case(&case).unwrap(),
                    GuardClass::Case,
                )
                .await
                .0,
            );
        }
        let (winner, saved, result) = h
            .race_ordered("center", commands.clone(), Some(preferred))
            .await;
        assert_eq!(winner, preferred);
        h.refuses_unchanged(
            "center",
            commands[1 - winner].clone(),
            "ADJUSTMENT_CAPACITY",
        )
        .await;
        let (_, State::Adjustment(pool)) = h
            .state(
                "center",
                HeadKind::Adjustment,
                rt::points::Point::id(rt::points::PointKind::Adjustment, *b"ADJPOOL_", "positive")
                    .unwrap(),
                GuardClass::AdjustmentPool,
            )
            .await
        else {
            panic!("pool")
        };
        assert_eq!(pool.terms.funding.value(), 100);
        assert_eq!(pool.terms.gross.value(), 100);
        assert_eq!(
            (
                pool.funding_used.value(),
                pool.gross_used.value(),
                pool.positive_used.value(),
                pool.negative_used.value()
            ),
            (100, 100, 100, 0)
        );
        for (index, command) in commands.iter().enumerate() {
            let case: wire::Case =
                serde_json::from_value(command["payload"]["case"].clone()).unwrap();
            let (observed, State::Case(c)) = h
                .state(
                    "center",
                    HeadKind::Case,
                    rt::points::Point::case(&case).unwrap(),
                    GuardClass::Case,
                )
                .await
            else {
                panic!("case")
            };
            assert_eq!(c.signed.value(), if index == winner { 100 } else { 0 });
            assert_eq!(c.revision.value(), u128::from(index == winner));
            if index != winner {
                assert_eq!(observed.value, prior_cases[index].value);
                assert_eq!(observed.revision, prior_cases[index].revision);
            }
            let (_, State::Family(f)) = h
                .state(
                    "center",
                    HeadKind::Family,
                    rt::points::Point::family(&case.0).unwrap(),
                    GuardClass::FamilyPrerequisite,
                )
                .await
            else {
                panic!("family")
            };
            assert!(f.closed && f.first_closure.is_some());
            assert_eq!(
                matches!(f.entitlement, wire::EntitlementHead::Consumed { .. }),
                index == winner
            );
        }
        h.reopen().await;
        h.retry_exact("center", &saved, &result).await;
        h.refuses_unchanged(
            "center",
            commands[1 - winner].clone(),
            "ADJUSTMENT_CAPACITY",
        )
        .await;
        h.assert_all_owner_accounts().await;
        h.close().await;
        eprintln!("actual ordered independent entitlement shared-pool winner{winner}: each100 fits alone, combined200 refused against100, exact funding/gross/directional usage and unchanged loser, reopen/retry");
    }
}
