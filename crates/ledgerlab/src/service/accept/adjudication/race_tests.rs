//! Overlapping coordinator calls on real fenced SQLite. The forwarding barrier
//! changes scheduling only; transactions, capabilities and source reads are real.
use super::*;
use crate::store::{adjudication::WorkRequest, errors::StoreError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Barrier, Notify};

struct AtAdmission<S> {
    inner: S,
    gate: Arc<Barrier>,
    first: AtomicBool,
    ordered: Option<(Arc<Notify>, Arc<Notify>)>,
}
impl<S: AdjudicationStore<Tx = SqliteTx>> AdjudicationStore for AtAdmission<S> {
    type Tx = SqliteTx;
    async fn begin_adjudication(
        &self,
        w: &WorkRequest,
        d: Instant,
    ) -> Result<SqliteTx, StoreError> {
        if self.first.swap(false, Ordering::SeqCst) {
            self.gate.wait().await;
            if let Some((allow, attempted)) = &self.ordered {
                allow.notified().await;
                let mut pending = Box::pin(self.inner.begin_adjudication(w, d));
                let mut entered = false;
                return std::future::poll_fn(|cx| {
                    let poll = std::future::Future::poll(pending.as_mut(), cx);
                    if !entered {
                        assert!(
                            poll.is_pending(),
                            "actual admission must wait behind live validated transaction"
                        );
                        entered = true;
                        attempted.notify_one();
                    }
                    poll
                })
                .await;
            }
        }
        self.inner.begin_adjudication(w, d).await
    }
}
impl Harness {
    fn race_host(&self) -> RemainingHost {
        RemainingHost(FlowHost {
            stores: self.host.0.stores.clone(),
            heads: self.host.0.heads.clone(),
            sources: self.host.0.sources.clone(),
            base: BaseFixture::new(&self.input),
            now: self.host.0.now.clone(),
        })
    }
    async fn retry_exact(&self, owner: &str, c: &Value, prior: &wire::CommandResult) {
        let configured = self.host.0.stores[owner]
            .provision_adjudication_with_ceiling(
                journal(owner),
                self.budgets[owner].clone(),
                self.ceilings[owner].clone(),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        let inventory = self.host.0.stores[owner].test_full_inventory().await;
        let mut retry = c.clone();
        retry["authority"]["permission"] = json!("read");
        let saved = run(
            &configured,
            &self.host,
            journal(owner),
            parsed(&retry),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, self.roots[owner]);
        assert_eq!(saved.effects, prior.effects);
        assert_eq!(
            self.host.0.stores[owner].test_full_inventory().await,
            inventory
        );
    }
    async fn refuses_unchanged(&mut self, owner: &str, c: Value, code: &str) {
        let before = self.host.0.stores[owner].test_full_inventory().await;
        self.execute(c, Some(code)).await;
        assert_eq!(
            self.host.0.stores[owner].test_full_inventory().await,
            before
        );
    }
    async fn race(
        &mut self,
        owner: &str,
        commands: [Value; 2],
    ) -> (usize, Value, wire::CommandResult) {
        self.race_ordered(owner, commands, None).await
    }
    async fn race_ordered(
        &mut self,
        owner: &str,
        mut commands: [Value; 2],
        preferred: Option<usize>,
    ) -> (usize, Value, wire::CommandResult) {
        for c in &mut commands {
            hydrate(c, &self.proofs, &self.roots[owner]);
        }
        let before = self.host.0.stores[owner]
            .test_adjudication_stats(&journal(owner))
            .await;
        let barrier = Arc::new(Barrier::new(3));
        let mut jobs = Vec::new();
        let allow = Arc::new(Notify::new());
        let attempted = Arc::new(Notify::new());
        let pause = preferred.map(|_| self.host.0.stores[owner].test_pause_next_append());
        for (index, c) in commands.iter().enumerate() {
            let command = parsed(c);
            let inner = self.host.0.stores[owner]
                .provision_adjudication_with_ceiling(
                    journal(owner),
                    self.budgets[owner].clone(),
                    self.ceilings[owner].clone(),
                    65536,
                    Count::new(1u128 << 40).unwrap(),
                )
                .await
                .unwrap();
            let store = AtAdmission {
                inner,
                gate: barrier.clone(),
                first: AtomicBool::new(true),
                ordered: preferred
                    .filter(|winner| index != *winner)
                    .map(|_| (allow.clone(), attempted.clone())),
            };
            let host = self.race_host();
            let j = journal(owner);
            jobs.push(tokio::spawn(async move {
                run(&store, &host, j, command, deadline()).await
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), barrier.wait())
            .await
            .expect("both live calls entered actual admission");
        if let Some(pause) = pause {
            tokio::time::timeout(Duration::from_secs(10), pause.reached.notified())
                .await
                .unwrap();
            allow.notify_one();
            tokio::time::timeout(Duration::from_secs(10), attempted.notified())
                .await
                .unwrap();
            pause.release.notify_one();
        }
        let mut results = Vec::new();
        for job in jobs {
            results.push(job.await.unwrap());
        }
        let winners: Vec<_> = results
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r,Ok(v) if v.status==wire::CommandResultStatus::Committed))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(winners.len(), 1, "{results:?}");
        let winner = winners[0];
        assert!(
            matches!(&results[1 - winner], Err(ServiceError::Retryable))
                || matches!(&results[1-winner],Err(ServiceError::Rejection(code)) if code=="AUTH_HEAD"),
            "{results:?}"
        );
        eprintln!("overlapping loser {:?}", results[1 - winner]);
        let result = results.remove(winner).unwrap();
        let c = commands[winner].clone();
        *self.ordinals.get_mut(owner).unwrap() += 1;
        self.roots.insert(owner.into(), result.root.clone());
        let fact = match c["kind"].as_str().unwrap() {
            "ABORT" | "CLOSE" => Some(("TERMINAL", c["payload"]["round"].clone())),
            "ISSUE" => Some(("CLAIM", c["payload"]["token"]["id"].clone())),
            "RETIRE_GRANT" => Some(("RETIREMENT", c["payload"]["grant"].clone())),
            "RECEIVE" => Some(("RECEIPT", c["payload"]["token"].clone())),
            "RETURN_UNUSED" => Some(("RETURNED_UNUSED", c["payload"]["token"].clone())),
            "DECIDE" => None,
            _ => panic!("race kind"),
        };
        if let Some((kind, key)) = fact {
            let source = self.host.0.stores[owner]
                .adjudication_source(
                    &journal(owner),
                    Count::new(self.ordinals[owner]).unwrap(),
                    &serde_json::from_value(json!(kind)).unwrap(),
                    &serde_json::from_value(key).unwrap(),
                )
                .await
                .unwrap();
            let proof = serde_json::to_value(source.proof()).unwrap();
            self.proofs.insert(proof_key(&proof), proof);
        }
        let after = self.host.0.stores[owner]
            .test_adjudication_stats(&journal(owner))
            .await;
        assert_eq!(after["segments"], before["segments"] + 1);
        self.retry_exact(owner, &c, &result).await;
        self.last = Some((owner.into(), c.clone(), result.clone()));
        eprintln!(
            "actual overlapping race {} vs {}: winner {} root {}",
            commands[0]["kind"],
            commands[1]["kind"],
            winner,
            result.root.as_str()
        );
        (winner, c, result)
    }
    async fn finish(&mut self, n: u128, predecessor: u128) {
        self.begin(n, "FINISH_ONLY").await;
        self.seal_begin(n, predecessor).await;
        self.sealed(n).await;
        self.drain(n).await;
        self.ready(n).await;
        let close = self.terminal_command("CLOSE", n);
        self.step(close).await;
        self.install(n, "COMMITTED").await;
    }
    fn grant_name(&self, n: usize) -> String {
        format!(
            "gr1.{}.race{n}",
            self.input["commands"][5]["payload"]["grant"]["namespace"]["tag"]
                .as_str()
                .unwrap()
        )
    }
    async fn grant_and_register(&mut self, n: usize) {
        let mut p = self.input["commands"][5]["payload"].clone();
        p["grant"]["id"] = json!(self.grant_name(n));
        self.command_step("LOCAL_GRANT", p).await;
        let proof = self.proof("g1", "GRANT", json!(self.grant_name(n)));
        let p = json!({"grant":self.input["commands"][5]["payload"]["grant"],"proof":proof});
        self.command_step("REGISTER_GRANT", p).await;
    }
    fn issue_command(&mut self, n: usize) -> Value {
        let grant = self.grant_name(n);
        let token = format!("race{n}");
        let claim = rt::hash(
            "claim",
            &json!([grant, token, "g1", n.to_string(), "ORDINARY"]),
        )
        .unwrap();
        self.command("ISSUE",json!({"grant":grant,"token":{"id":token,"grant":grant,"gateway":"g1","allocation":n.to_string(),"category":"ORDINARY","claim":claim}}))
    }
    async fn activate_token(&mut self, n: usize) {
        let token = format!("race{n}");
        let proof = self.proof("center", "CLAIM", json!(token));
        self.command_step(
            "ACTIVATE",
            json!({"gateway":"g1","token":token,"proof":proof}),
        )
        .await;
    }
    fn receive_command(&mut self, n: usize, epoch: u128) -> Value {
        let mut p = self.input["commands"][9]["payload"].clone();
        p["token"] = json!(format!("race{n}"));
        p["epoch"] = json!(epoch.to_string());
        p["delivery"][2] = json!(format!(
            "gw1.{}.race{n}",
            self.input["commands"][5]["payload"]["grant"]["namespace"]["tag"]
                .as_str()
                .unwrap()
        ));
        p["submission"]["case"] =
            routed_case(&format!("race{n}"), &self.enrollment["families"][0]["key"]);
        let mut c = self.command("RECEIVE", p);
        c["key"] = c["payload"]["delivery"].clone();
        c
    }
    fn return_command(&mut self, n: usize) -> Value {
        let token = format!("race{n}");
        let grant = self.grant_name(n);
        let proof = self.proof("center", "CLAIM", json!(token));
        let claim = rt::hash(
            "claim",
            &json!([grant, token, "g1", n.to_string(), "ORDINARY"]),
        )
        .unwrap();
        self.command(
            "RETURN_UNUSED",
            json!({"gateway":"g1","token":token,"claim":claim,"proof":proof}),
        )
    }
    async fn settle_token(&mut self, n: usize, received: bool, receipt_position: Option<u128>) {
        let token = format!("race{n}");
        let proof = self.proof(
            "g1",
            if received {
                "RECEIPT"
            } else {
                "RETURNED_UNUSED"
            },
            json!(token),
        );
        if received {
            self.command_step("IMPORT", json!({"token":token,"proof":proof}))
                .await;
        }
        self.command_step("RECONCILE", json!({"token":token,"proof":proof}))
            .await;
        let proof = self.proof("center", "RECONCILIATION", json!(token));
        self.command_step(
            "LOCAL_TERMINAL",
            json!({"gateway":"g1","grant":self.grant_name(n),"proof":proof}),
        )
        .await;
        self.command_step("ADVANCE", json!({"gateway":"g1","through":n.to_string()}))
            .await;
        if let Some(p) = receipt_position {
            self.command_step(
                "ADVANCE_RECEIPT",
                json!({"gateway":"g1","through":p.to_string()}),
            )
            .await;
        }
    }
}
#[tokio::test]
async fn actual_overlapping_abort_close_has_one_terminal_and_preserves_finish() {
    for reverse in [false, true] {
        let mut h = Harness::new().await;
        h.prepare_round(1, 0).await;
        h.begin(1, "CANCELLABLE").await;
        h.seal_begin(1, 0).await;
        h.sealed(1).await;
        h.drain(1).await;
        h.ready(1).await;
        let mut commands = [
            h.terminal_command("ABORT", 1),
            h.terminal_command("CLOSE", 1),
        ];
        if reverse {
            commands.swap(0, 1);
        }
        let (winner, saved, result) = h.race("center", commands.clone()).await;
        let aborted = saved["kind"] == "ABORT";
        h.refuses_unchanged(
            "center",
            commands[1 - winner].clone(),
            if aborted {
                "NOT_READY"
            } else {
                "ABORT_REFUSED"
            },
        )
        .await;
        h.install(1, if aborted { "ABORTED" } else { "COMMITTED" })
            .await;
        if aborted {
            h.finish(2, 1).await;
        }
        h.reopen().await;
        h.retry_exact("center", &saved, &result).await;
        h.close().await;
    }
}
#[tokio::test]
async fn actual_overlapping_issue_retire_preserves_single_claim_or_permanent_retirement() {
    for reverse in [false, true] {
        let mut h = Harness::new().await;
        h.grant_and_register(1).await;
        let mut commands = [
            h.issue_command(1),
            h.command("RETIRE_GRANT", json!({"grant":h.grant_name(1)})),
        ];
        if reverse {
            commands.swap(0, 1);
        }
        let (winner, saved, result) = h.race("center", commands.clone()).await;
        let issued = saved["kind"] == "ISSUE";
        h.refuses_unchanged(
            "center",
            commands[1 - winner].clone(),
            if issued {
                "GRANT_CLAIMED"
            } else {
                "GRANT_UNAVAILABLE"
            },
        )
        .await;
        if issued {
            let c = h.return_command(1);
            h.step(c).await;
            h.settle_token(1, false, None).await;
        } else {
            let proof = h.proof("center", "RETIREMENT", json!(h.grant_name(1)));
            h.command_step(
                "LOCAL_TERMINAL",
                json!({"gateway":"g1","grant":h.grant_name(1),"proof":proof}),
            )
            .await;
        }
        h.finish(1, 0).await;
        h.reopen().await;
        h.retry_exact("center", &saved, &result).await;
        h.close().await;
    }
}
#[tokio::test]
async fn actual_overlapping_receive_return_has_one_disposition_and_finishes() {
    for reverse in [false, true] {
        let mut h = Harness::new().await;
        h.grant_and_register(1).await;
        let c = h.issue_command(1);
        h.step(c).await;
        h.activate_token(1).await;
        let mut commands = [h.receive_command(1, 1), h.return_command(1)];
        if reverse {
            commands.swap(0, 1);
        }
        let (winner, saved, result) = h.race("g1", commands.clone()).await;
        let received = saved["kind"] == "RECEIVE";
        h.refuses_unchanged("g1", commands[1 - winner].clone(), "TOKEN_STATE")
            .await;
        h.settle_token(1, received, received.then_some(1)).await;
        h.finish(1, 0).await;
        h.reopen().await;
        h.retry_exact("g1", &saved, &result).await;
        h.close().await;
    }
}
#[tokio::test]
async fn actual_overlapping_same_family_allow_consumes_one_entitlement() {
    let mut h = Harness::new().await;
    let mut receives = Vec::new();
    for n in 1..=2 {
        h.grant_and_register(n).await;
        let c = h.issue_command(n);
        h.step(c).await;
        h.activate_token(n).await;
        let c = h.receive_command(n, 1);
        receives.push(c.clone());
        h.step(c).await;
        h.settle_token(n, true, Some(n as u128)).await;
    }
    let mut commands = Vec::new();
    for receive in &receives {
        let mut c = h.input["commands"][11].clone();
        c["payload"]["case"] = receive["payload"]["submission"]["case"].clone();
        c["key"][2] = json!(format!("allow-{}", commands.len()));
        commands.push(c);
    }
    let (winner, saved, result) = h
        .race("center", [commands[0].clone(), commands[1].clone()])
        .await;
    assert_eq!(result.effects.len(), 1);
    assert_eq!(
        serde_json::to_value(&result.effects[0]).unwrap()["body"]["signed_atoms"],
        "1200"
    );
    h.refuses_unchanged("center", commands[1 - winner].clone(), "ENTITLEMENT")
        .await;
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
    assert_eq!(f.ordinary_positive.value(), 1200);
    let wire::EntitlementHead::Consumed { case, revision, .. } = f.entitlement else {
        panic!("consumed entitlement")
    };
    assert_eq!(
        serde_json::to_value(case).unwrap(),
        commands[winner]["payload"]["case"]
    );
    assert_eq!(revision.value(), 1);
    for (index, command) in commands.iter().enumerate() {
        let case: wire::Case = serde_json::from_value(command["payload"]["case"].clone()).unwrap();
        let (_, State::Case(state)) = h
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
        assert_eq!(
            state.status,
            if index == winner {
                rt::points::CaseStatus::FinalAllow
            } else {
                rt::points::CaseStatus::OrdinaryPending
            }
        );
        assert_eq!(state.revision.value(), if index == winner { 1 } else { 0 });
        assert_eq!(state.signed.value(), if index == winner { 1200 } else { 0 });
    }
    h.finish(1, 0).await;
    h.reopen().await;
    h.retry_exact("center", &saved, &result).await;
    h.close().await;
}

fn routed_case(name: &str, family: &Value) -> Value {
    for suffix in 0..10000 {
        let case = json!([family, "source", format!("{name}-{suffix}")]);
        let hash = rt::hash("route", &case).unwrap();
        if u8::from_str_radix(&hash.as_str()[62..], 16).unwrap() % 4 == 1 {
            return case;
        }
    }
    panic!("route search")
}

#[path = "replacement_tests.rs"]
mod replacement_tests;

#[tokio::test]
async fn actual_ordered_overlap_issue_wins_before_retirement() {
    let mut h = Harness::new().await;
    h.grant_and_register(1).await;
    let commands = [
        h.issue_command(1),
        h.command("RETIRE_GRANT", json!({"grant":h.grant_name(1)})),
    ];
    let (winner, saved, result) = h.race_ordered("center", commands.clone(), Some(0)).await;
    assert_eq!(winner, 0);
    h.refuses_unchanged("center", commands[1].clone(), "GRANT_CLAIMED")
        .await;
    let c = h.return_command(1);
    h.step(c).await;
    h.settle_token(1, false, None).await;
    h.finish(1, 0).await;
    h.reopen().await;
    h.retry_exact("center", &saved, &result).await;
    h.close().await;
    eprintln!("ordered actual overlap: validated ISSUE held live SQL transaction while RETIRE entered and blocked; one claim; fresh-head retirement refused; prepaid disposal/finish/reopen PASS");
}

#[path = "replacement_process_tests.rs"]
mod replacement_process_tests;

#[path = "resource_admission_tests.rs"]
mod resource_admission_tests;
