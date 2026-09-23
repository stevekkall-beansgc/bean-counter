//! Protected completion after finite optional-round exhaustion. Test-side budget
//! derivation never changes a live resource/counter row.
use super::*;
fn optional_round_budget(host: &str) -> wire::Resource {
    let w = Worksheet::frozen().unwrap();
    let mut sum = wire::Resource::zero();
    let mut add = |slots: &[String], copies: usize| {
        let mut cost = wire::Resource::zero();
        for kind in slots {
            let mut r = w.template(kind).unwrap().resources().unwrap();
            let peak = r.workspace_bytes;
            r.workspace_bytes = Count::ZERO;
            cost = cost.checked_add(&r).unwrap();
            cost.workspace_bytes = cost.workspace_bytes.max(peak);
        }
        for _ in 0..copies {
            sum = sum.checked_add(&cost).unwrap();
        }
    };
    add(
        &[if host == "center" {
            "ENROLL"
        } else {
            "PREPARE_ENROLL"
        }
        .into()],
        1,
    );
    add(
        w.bundle(if host == "center" {
            "finish_central"
        } else {
            "finish_gateway"
        })
        .unwrap(),
        32,
    );
    add(
        w.bundle(if host == "center" {
            "cancel_central"
        } else {
            "cancel_gateway"
        })
        .unwrap(),
        2,
    );
    if host != "center" {
        add(&["PREPARE_ROUND".into()], 2);
    }
    // The shared host observation helper quotes one ENROLL-sized point read.
    // Provision that finite read envelope up front as well; it never mutates a
    // live budget and does not bypass optional-round reservation accounting.
    let read = w.template("ENROLL").unwrap().resources().unwrap();
    let mut dimensions = sum.dimensions();
    for (value, floor) in dimensions.iter_mut().zip(read.dimensions()) {
        *value = (*value).max(floor);
    }
    wire::Resource::from_dimensions(dimensions)
}
fn sorted(mut values: Vec<Value>) -> Vec<Value> {
    values.sort_by_cached_key(|v| r3::canonical_bytes(v, r3::COMMAND_BYTES).unwrap());
    values
}
impl Harness {
    fn full_begin_command(&mut self, n: u128, mode: &str, preparations: Vec<Value>) -> Value {
        let families = sorted(
            self.enrollment["families"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["key"].clone())
                .collect(),
        );
        self.command("BEGIN",json!({"round":n.to_string(),"predecessor":(n-1).to_string(),"mode":mode,"families":families,"gateways":["g0","g1","g2","g3"],"preparations":sorted(preparations)}))
    }
    async fn full_seal_ready(&mut self, n: u128) {
        for gateway in ["g0", "g1", "g2", "g3"] {
            let proof = self.proof("center", "BEGIN", json!(n.to_string()));
            self.command_step("SEAL_BEGIN",json!({"gateway":gateway,"round":n.to_string(),"predecessor":(n-1).to_string(),"proof":proof})).await;
            self.command_step("SEALED", json!({"gateway":gateway,"round":n.to_string()}))
                .await;
            let proof = self.proof(gateway, "SEAL", json!(n.to_string()));
            self.command_step(
                "DRAIN",
                json!({"gateway":gateway,"round":n.to_string(),"proof":proof}),
            )
            .await;
        }
        self.ready(n).await;
    }
    fn full_install_command(&mut self, n: u128, gateway: &str, outcome: &str) -> Value {
        let proof = self.proof("center", "TERMINAL", json!(n.to_string()));
        let begin = self.proof("center", "BEGIN", json!(n.to_string()));
        self.command("INSTALL",json!({"gateway":gateway,"round":n.to_string(),"outcome":outcome,"proof":proof,"begin":begin}))
    }
    async fn full_ack(&mut self, n: u128, gateway: &str) {
        let proof = self.proof(gateway, "INSTALLATION", json!(n.to_string()));
        self.command_step(
            "ACK_INSTALL",
            json!({"gateway":gateway,"round":n.to_string(),"proof":proof}),
        )
        .await;
    }
    async fn full_install_all(&mut self, n: u128, outcome: &str) {
        for gateway in ["g0", "g1", "g2", "g3"] {
            let c = self.full_install_command(n, gateway, outcome);
            self.step(c).await;
            self.full_ack(n, gateway).await;
        }
    }
    async fn assert_actual_accounts(&self) {
        self.assert_all_owner_accounts().await;
        for host in ["center", "g0", "g1", "g2", "g3"] {
            let (_, State::Resource(a)) = self
                .state(
                    host,
                    HeadKind::Resource,
                    rt::points::Point::id(rt::points::PointKind::Resource, *b"RESOURCE", host)
                        .unwrap(),
                    GuardClass::CapacityAllocation,
                )
                .await
            else {
                panic!("resource")
            };
            assert!(a.used.checked_add(&a.held).unwrap().fits(&a.provisioned));
            for (q, r) in a.q.dimensions().into_iter().zip(a.reserved.dimensions()) {
                assert!(q.checked_add(r).unwrap().value() <= Count::MAX);
            }
            assert_eq!(a.q.segment.value(), self.ordinals[host]);
            assert_eq!(
                a.q.writer_epoch.value(),
                if host == "center" { 0 } else { 1 }
            );
        }
    }
    async fn accept_observed(&mut self, owner: &str, mut c: Value, result: wire::CommandResult) {
        *self.ordinals.get_mut(owner).unwrap() += 1;
        self.roots.insert(owner.into(), result.root.clone());
        let fact = match c["kind"].as_str().unwrap() {
            "PREPARE_ROUND" => Some((
                "ROUND_PREPARATION",
                json!(rt::hash(
                    "namespace",
                    &json!([c["payload"]["gateway"], c["payload"]["round"]])
                )
                .unwrap()),
            )),
            "BEGIN" => Some(("BEGIN", c["payload"]["round"].clone())),
            "CLOSE" => Some(("TERMINAL", c["payload"]["round"].clone())),
            "INSTALL" => Some(("INSTALLATION", c["payload"]["round"].clone())),
            "ISSUE" => Some(("CLAIM", c["payload"]["token"]["id"].clone())),
            "RETURN_UNUSED" => Some(("RETURNED_UNUSED", c["payload"]["token"].clone())),
            "RECONCILE" => Some(("RECONCILIATION", c["payload"]["token"].clone())),
            "ACTIVATE" => None,
            _ => panic!("observed transition"),
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
        let before = self.host.0.stores[owner].test_full_inventory().await;
        c["authority"]["permission"] = json!("read");
        let retry = run(
            &configured,
            &self.host,
            journal(owner),
            parsed(&c),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(retry.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(retry.root, result.root);
        assert_eq!(retry.effects, result.effects);
        assert_eq!(
            self.host.0.stores[owner].test_full_inventory().await,
            before
        );
        self.last = Some((owner.into(), c, result));
    }
    async fn optional_attempt(&mut self, owner: &str, mut c: Value) -> bool {
        hydrate(&mut c, &self.proofs, &self.roots[owner]);
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
        let before = self.host.0.stores[owner].test_full_inventory().await;
        match run(
            &configured,
            &self.host,
            journal(owner),
            parsed(&c),
            deadline(),
        )
        .await
        {
            Ok(result) => {
                assert_eq!(result.status, wire::CommandResultStatus::Committed);
                self.accept_observed(owner, c, result).await;
                true
            }
            Err(ServiceError::Rejection(code)) if code == "UNFUNDED" => {
                assert_eq!(
                    self.host.0.stores[owner].test_full_inventory().await,
                    before
                );
                eprintln!("actual optional exhaustion {} {}", owner, c["kind"]);
                false
            }
            other => panic!("optional outcome {other:?}"),
        }
    }
}
#[tokio::test]
async fn actual_all_family_finish_survives_cancellable_round_exhaustion() {
    let budgets = ["center", "g0", "g1", "g2", "g3"]
        .into_iter()
        .map(|host| (host.into(), optional_round_budget(host)))
        .collect();
    let mut h = Harness::with_budgets(budgets, false, true).await;
    let mut terminal_round = 0;
    let mut exhausted = false;
    for n in 1..=32 {
        let mut preparations = Vec::new();
        for gateway in ["g0", "g1", "g2", "g3"] {
            let proof = h.proof("center", "ENROLLMENT", json!("registration"));
            let c=h.command("PREPARE_ROUND",json!({"gateway":gateway,"round":n.to_string(),"predecessor":(n-1).to_string(),"mode":"CANCELLABLE","enrollment":rt::hash("enrollment",&h.enrollment).unwrap(),"proof":proof}));
            if !h.optional_attempt(gateway, c).await {
                exhausted = true;
                break;
            }
            preparations.push(h.proof(
                gateway,
                "ROUND_PREPARATION",
                json!(rt::hash("namespace", &json!([gateway, n.to_string()])).unwrap()),
            ));
        }
        if exhausted {
            break;
        }
        let c = h.full_begin_command(n, "CANCELLABLE", preparations);
        if !h.optional_attempt("center", c).await {
            exhausted = true;
            break;
        }
        let abort = h.terminal_command("ABORT", n);
        h.step(abort).await;
        h.full_install_all(n, "ABORTED").await;
        terminal_round = n;
        h.assert_actual_accounts().await;
    }
    assert!(
        exhausted,
        "finite optional budget must exhaust within test bound"
    );
    assert!(terminal_round >= 1);
    let n = terminal_round + 1;
    let c = h.full_begin_command(n, "FINISH_ONLY", vec![]);
    h.step(c).await;
    let before = h.host.0.stores["center"].test_full_inventory().await;
    let c = h.terminal_command("ABORT", n);
    h.execute(c, Some("ABORT_REFUSED")).await;
    assert_eq!(
        h.host.0.stores["center"].test_full_inventory().await,
        before
    );
    h.full_seal_ready(n).await;
    let c = h.terminal_command("CLOSE", n);
    let result = h.step(c).await;
    let wire::Effect::Closure { body } = &result.effects[0] else {
        panic!("closure")
    };
    assert_eq!(body.families.len(), 5);
    assert_eq!(body.cutoffs.len(), 4);
    h.full_install_all(n, "COMMITTED").await;
    h.reopen().await;
    h.assert_actual_accounts().await;
    for row in h.enrollment["families"].as_array().unwrap() {
        let family = serde_json::from_value(row["key"].clone()).unwrap();
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
        assert!(f.closed);
    }
    eprintln!("actual protected FINISH complete after {terminal_round} aborted optional rounds: five explicit families, four gateway seals/installations/acks, unchanged refusal inventories and all16 counter conservation");
    h.close().await;
}
#[path = "counter_boundary_tests.rs"]
mod counter_boundary_tests;
#[path = "finish_process_tests.rs"]
mod finish_process_tests;
