//! All mutations use the public host facade. Private ports below only observe
//! exact retained points; no synthetic capability, source proof or base insertion.
use super::*;
use r3::types::Digest;
use std::collections::BTreeSet;
fn context() -> HostContext {
    let mut c = super::context();
    // Trusted synthetic host clock matches the independent customer's noon events.
    c.observed_at = Time::parse("2026-09-22T12:00:00.000000Z").unwrap();
    c
}
struct Path {
    f: Fixture,
    roots: BTreeMap<String, Digest>,
    ordinals: BTreeMap<String, u128>,
    proofs: BTreeMap<String, Value>,
    grants: BTreeMap<String, Value>,
    enrollment: Value,
    kinds: BTreeSet<String>,
    serial: usize,
    last: Option<(String, Value, wire::CommandResult)>,
}
fn pk(p: &Value) -> String {
    serde_json::to_string(&json!([p["host"], p["fact_kind"], p["full_key"]])).unwrap()
}
fn owner(c: &Value) -> String {
    match c["kind"].as_str().unwrap() {
        "PREPARE_ENROLL" | "PREPARE_ROUND" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED"
        | "LOCAL_TERMINAL" | "SEAL_BEGIN" | "SEALED" | "INSTALL" | "REPLACE_WRITER" => {
            c["payload"]["gateway"].as_str().unwrap()
        }
        "LOCAL_GRANT" => c["payload"]["grant"]["gateway"].as_str().unwrap(),
        "EXTEND_RESOURCES" => c["payload"]["host"].as_str().unwrap(),
        _ => "center",
    }
    .into()
}
impl Path {
    async fn new(extend: bool) -> Self {
        let f = Fixture::with_extension(extend).await;
        f.host
            .provision_original(&Id::parse("center").unwrap(), f.setup.clone())
            .await
            .unwrap();
        let roots = ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .map(|h| (h.into(), Digest::parse(&"0".repeat(64)).unwrap()))
            .collect::<BTreeMap<String, _>>();
        let ordinals = roots.keys().map(|h| (h.clone(), 0)).collect();
        Self {
            f,
            roots,
            ordinals,
            proofs: BTreeMap::new(),
            grants: BTreeMap::new(),
            enrollment: Value::Null,
            kinds: BTreeSet::new(),
            serial: 0,
            last: None,
        }
    }
    async fn enrolled(extend: bool) -> Self {
        let mut p = Self::new(extend).await;
        for i in 0..5 {
            p.step(p.f.v["commands"][i].clone()).await;
        }
        p
    }
    fn command(&mut self, kind: &str, payload: Value) -> Value {
        self.serial += 1;
        json!({"kind":kind,"key":[self.f.v["commands"][0]["key"][0],"urn:synthetic:work",format!("public-extra-{}",self.serial)],"payload":payload})
    }
    fn proof(&self, host: &str, kind: &str, key: Value) -> Value {
        self.proofs[&pk(&json!({"host":host,"fact_kind":kind,"full_key":key}))].clone()
    }
    fn hydrate(&self, c: &mut Value) {
        for field in ["proof", "begin"] {
            if let Some(p) = c["payload"].get(field) {
                c["payload"][field] = self.proofs[&pk(p)].clone();
            }
        }
        if let Some(ps) = c["payload"]
            .get_mut("preparations")
            .and_then(Value::as_array_mut)
        {
            for p in ps.iter_mut() {
                *p = self.proofs[&pk(p)].clone();
            }
            ps.sort_by_cached_key(|p| r3::canonical_bytes(p, r3::COMMAND_BYTES).unwrap());
        }
        if c["kind"] == "LOCAL_GRANT" {
            c["payload"]["grant"]["journal_head"] = json!(self.roots[&owner(c)]);
            let mut g = c["payload"]["grant"].clone();
            g.as_object_mut().unwrap().remove("authentication");
            c["payload"]["grant"]["authentication"] = json!(rt::hash("grant", &g).unwrap());
        }
        if c["kind"] == "REGISTER_GRANT" {
            c["payload"]["grant"] =
                self.grants[c["payload"]["grant"]["id"].as_str().unwrap()].clone();
        }
        if c["kind"] == "PREPARE_ROUND" {
            c["payload"]["enrollment"] = json!(rt::hash("enrollment", &self.enrollment).unwrap());
        }
    }
    async fn refuse(&self, mut c: Value, code: &str) {
        self.hydrate(&mut c);
        let h = owner(&c);
        let before = self.f.host.entries[&h].store.test_full_inventory().await;
        let r = self
            .f
            .host
            .execute(
                &Id::parse(&h).unwrap(),
                &proposal(&c),
                context(),
                Some(&self.f.base),
                Duration::from_secs(30),
            )
            .await;
        assert!(
            matches!(&r,Err(ServiceError::Rejection(v)) if v==code),
            "{} expected {code}: {r:?}",
            c["kind"]
        );
        assert_eq!(
            before,
            self.f.host.entries[&h].store.test_full_inventory().await
        );
    }
    async fn step(&mut self, mut c: Value) -> wire::CommandResult {
        self.hydrate(&mut c);
        let h = owner(&c);
        let kind = c["kind"].as_str().unwrap().to_owned();
        let id = Id::parse(&h).unwrap();
        let r = self
            .f
            .host
            .execute(
                &id,
                &proposal(&c),
                context(),
                Some(&self.f.base),
                Duration::from_secs(30),
            )
            .await
            .unwrap_or_else(|e| panic!("public {kind} {h} ordinal{}: {e}", self.ordinals[&h]));
        assert_eq!(r.status, wire::CommandResultStatus::Committed);
        assert_eq!(r.code, kind);
        assert_ne!(r.root, self.roots[&h]);
        *self.ordinals.get_mut(&h).unwrap() += 1;
        self.roots.insert(h.clone(), r.root.clone());
        self.kinds.insert(kind.clone());
        if kind == "LOCAL_GRANT" {
            self.grants.insert(
                c["payload"]["grant"]["id"].as_str().unwrap().into(),
                c["payload"]["grant"].clone(),
            );
        }
        if kind == "ENROLL" {
            self.enrollment = c["payload"].clone();
        }
        let fact = match kind.as_str() {
            "PREPARE_ENROLL" => Some(("ENROLL_PREPARATION", c["payload"]["gateway"].clone())),
            "PREPARE_ROUND" => Some((
                "ROUND_PREPARATION",
                json!(rt::hash(
                    "namespace",
                    &json!([c["payload"]["gateway"], c["payload"]["round"]])
                )
                .unwrap()),
            )),
            "ENROLL" => Some(("ENROLLMENT", json!("registration"))),
            "LOCAL_GRANT" => Some(("GRANT", c["payload"]["grant"]["id"].clone())),
            "ISSUE" => Some(("CLAIM", c["payload"]["token"]["id"].clone())),
            "RECEIVE" => Some(("RECEIPT", c["payload"]["token"].clone())),
            "RETURN_UNUSED" => Some(("RETURNED_UNUSED", c["payload"]["token"].clone())),
            "RETIRE_GRANT" => Some(("RETIREMENT", c["payload"]["grant"].clone())),
            "RECONCILE" => Some(("RECONCILIATION", c["payload"]["token"].clone())),
            "BEGIN" => Some(("BEGIN", c["payload"]["round"].clone())),
            "SEALED" => Some(("SEAL", c["payload"]["round"].clone())),
            "CLOSE" | "ABORT" => Some(("TERMINAL", c["payload"]["round"].clone())),
            "INSTALL" => Some(("INSTALLATION", c["payload"]["round"].clone())),
            _ => None,
        };
        if let Some((kind, key)) = fact {
            let p = self
                .f
                .host
                .source_proof(
                    &id,
                    Count::new(self.ordinals[&h]).unwrap(),
                    serde_json::from_value(json!(kind)).unwrap(),
                    serde_json::from_value(key).unwrap(),
                    context(),
                    Duration::from_secs(5),
                )
                .await
                .unwrap();
            assert_eq!(p.root, r.root);
            assert_eq!(p.ordinal.value(), self.ordinals[&h]);
            let p = serde_json::to_value(p).unwrap();
            self.proofs.insert(pk(&p), p);
        }
        let before = self.f.host.entries[&h].store.test_full_inventory().await;
        let saved = self
            .f
            .host
            .execute(&id, &proposal(&c), context(), None, Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, r.root);
        assert_eq!(saved.effects, r.effects);
        assert_eq!(
            before,
            self.f.host.entries[&h].store.test_full_inventory().await
        );
        eprintln!(
            "public {kind} {h} ordinal={} root={}",
            self.ordinals[&h],
            r.root.as_str()
        );
        self.last = Some((h, c, r.clone()));
        r
    }
    async fn cmd(&mut self, k: &str, v: Value) -> wire::CommandResult {
        let c = self.command(k, v);
        self.step(c).await
    }
    async fn reopen(&mut self) {
        for e in std::mem::take(&mut self.f.host.entries).into_values() {
            e.store.close().await;
        }
        self.f.host = LocalHost::open(
            self.f.configs.clone(),
            self.f.sources.clone(),
            Id::parse("operator").unwrap(),
            self.f.verifier.clone(),
        )
        .await
        .unwrap();
        let (h, c, r) = self.last.as_ref().unwrap();
        let saved = self
            .f
            .host
            .execute(
                &Id::parse(h).unwrap(),
                &proposal(c),
                context(),
                None,
                Duration::from_secs(30),
            )
            .await
            .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, r.root);
        assert_eq!(saved.effects, r.effects);
    }
    async fn state(
        &self,
        kind: HeadKind,
        point: rt::points::Point,
        class: GuardClass,
    ) -> (ObservedHead, State) {
        let e = &self.f.host.entries["center"];
        let key = HeadKey {
            journal: e.journal.clone(),
            kind,
            full_key: point.key,
        };
        let work = WorkRequest {
            journal: e.journal.clone(),
            owner: Id::parse("public-test-read").unwrap(),
            transition: r3::raw_sha256(b"public-test-read"),
            mandatory: true,
            maximum: Worksheet::frozen()
                .unwrap()
                .template("ENROLL")
                .unwrap()
                .resources()
                .unwrap(),
        };
        let mut tx = e
            .configured
            .begin_adjudication(&work, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let mut guards = vec![
            Guard::Legacy(OutcomeLock {
                class: OutcomeLockClass::Admission,
                key: r3::canonical_bytes(&json!([e.journal.scope]), 4096).unwrap(),
                mode: OutcomeLockMode::Write,
            }),
            Guard::R3 {
                class,
                host: e.journal.host.clone(),
                key: key.full_key.clone(),
            },
        ];
        guards.sort_by_key(Guard::order_key);
        tx.lock_adjudication(&guards).await.unwrap();
        let Resolution::Complete(i) = tx
            .resolve_adjudication(&ResolveRequest {
                journal: e.journal.clone(),
                key: wire::Delivery(
                    e.journal.scope.clone(),
                    context().source,
                    Id::parse("public-test-read").unwrap(),
                ),
                guards,
                objects: vec![],
                heads: vec![key],
            })
            .await
            .unwrap()
        else {
            panic!("read")
        };
        assert_eq!(i.prefix.root(), &self.roots["center"]);
        assert_eq!(i.prefix.ordinal().value(), self.ordinals["center"]);
        let head = i.heads[0].clone();
        let state = serde_json::from_slice(head.value.as_deref().unwrap()).unwrap();
        tx.rollback().await.unwrap();
        (head, state)
    }
}
#[tokio::test]
async fn public_exact_customer_ninety_five_and_independent_economic_checkpoints() {
    let mut p = Path::new(false).await;
    let mut retail = 10000i128;
    let mut supplier = 3000i128;
    let mut actions = Vec::new();
    let mut checkpoints = 0;
    let mut pending = Vec::new();
    let mut close_coverage = Vec::new();
    // Independent original oracle EXPECTATIONS.json SHA256 8d43d92d2f31be079527e93874bbcb5f8ce05945ba673fe0c47588a89ec4fd67.
    const THROUGH: [usize; 15] = [5, 11, 12, 18, 19, 25, 26, 32, 38, 44, 91, 92, 93, 94, 95];
    const RETAIL: [i128; 15] = [
        10000, 10000, 11200, 11200, 11200, 11200, 11700, 11700, 11700, 11700, 11700, 11800, 11650,
        11650, 11450,
    ];
    const CONSUMED: [usize; 15] = [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 4, 5, 5];
    for n in 0..95 {
        let c = p.f.v["commands"][n].clone();
        let r = p.step(c).await;
        if p.f.v["commands"][n]["kind"] == "CLOSE" {
            close_coverage = serde_json::from_value(
                serde_json::to_value(&r.effects[0]).unwrap()["body"]["cutoffs"].clone(),
            )
            .unwrap();
        }
        for e in &r.effects {
            if let wire::Effect::Action { body } = e {
                assert_eq!(
                    body.magnitude.value(),
                    body.signed_atoms.value().unsigned_abs()
                );
                match body.book {
                    wire::ActionBook::Retail => retail += body.signed_atoms.value(),
                    wire::ActionBook::Supplier => supplier += body.signed_atoms.value(),
                };
                actions.push(body.clone());
            }
        }
        if let Some(s) = THROUGH.iter().position(|v| *v == n + 1) {
            checkpoints += 1;
            assert_eq!(retail, RETAIL[s], "S{s:02}");
            assert_eq!(supplier, 3000);
            let mut families = Vec::new();
            for f in p.enrollment["families"].as_array().unwrap() {
                let key: wire::Family = serde_json::from_value(f["key"].clone()).unwrap();
                let (_, State::Family(f)) = p
                    .state(
                        HeadKind::Family,
                        rt::points::Point::family(&key).unwrap(),
                        GuardClass::FamilyPrerequisite,
                    )
                    .await
                else {
                    panic!("family")
                };
                families.push(f);
            }
            assert_eq!(
                families
                    .iter()
                    .filter(|f| matches!(f.entitlement, wire::EntitlementHead::Consumed { .. }))
                    .count(),
                CONSUMED[s]
            );
            assert_eq!(
                families.iter().filter(|f| f.closed).count(),
                if s >= 10 { 5 } else { 0 }
            );
            assert_eq!(
                families
                    .iter()
                    .map(|f| f.ordinary_positive.value())
                    .sum::<u128>(),
                if s >= 6 {
                    1700
                } else if s >= 2 {
                    1200
                } else {
                    0
                }
            );
            if s >= 4 {
                let case: wire::Case = serde_json::from_value(
                    p.f.v["commands"][16]["payload"]["submission"]["case"].clone(),
                )
                .unwrap();
                let (_, State::Case(c)) = p
                    .state(
                        HeadKind::Case,
                        rt::points::Point::case(&case).unwrap(),
                        GuardClass::Case,
                    )
                    .await
                else {
                    panic!("case")
                };
                assert_eq!(c.status, rt::points::CaseStatus::FinalDeny);
                assert!(!actions.iter().any(|a| a.case == case));
            }
            if s == 4 || s == 13 {
                assert!(r.effects.is_empty());
            }
            if s == 9 || s == 10 {
                for (j, i) in [30, 36, 42].into_iter().enumerate() {
                    let case: wire::Case = serde_json::from_value(
                        p.f.v["commands"][i]["payload"]["submission"]["case"].clone(),
                    )
                    .unwrap();
                    let (head, State::Case(c)) = p
                        .state(
                            HeadKind::Case,
                            rt::points::Point::case(&case).unwrap(),
                            GuardClass::Case,
                        )
                        .await
                    else {
                        panic!("case")
                    };
                    if s == 9 {
                        pending.push(head);
                    } else {
                        assert_eq!(head.value, pending[j].value);
                        assert_eq!(head.revision, pending[j].revision);
                        assert_eq!(
                            c.effective_status(&families[j + 2]),
                            rt::points::CaseStatus::AdjustmentPending
                        );
                    }
                }
            }
            if s >= 11 {
                let mut gross = 0;
                for name in ["positive", "negative", "zero"] {
                    let (_, State::Adjustment(a)) = p
                        .state(
                            HeadKind::Adjustment,
                            rt::points::Point::id(
                                rt::points::PointKind::Adjustment,
                                *b"ADJPOOL_",
                                name,
                            )
                            .unwrap(),
                            GuardClass::AdjustmentPool,
                        )
                        .await
                    else {
                        panic!("pool")
                    };
                    gross += a.gross_used.value();
                }
                assert_eq!(gross, if s == 11 { 100 } else { 250 });
            }
            if s == 14 {
                assert_eq!(r.effects.len(), 2);
                assert_eq!(actions.len(), 6);
                assert_eq!(actions[4].signed_atoms.value(), -500);
                assert_eq!(actions[5].signed_atoms.value(), 300);
            }
            eprintln!(
                "public oracle S{s:02} through={} retail={retail} supplier={supplier}",
                n + 1
            );
        }
    }
    assert_eq!(checkpoints, 15);
    assert_eq!(p.ordinals.values().sum::<u128>(), 95);
    assert_eq!(p.kinds.len(), 22);
    p.reopen().await;
    public_comparisons(&mut p, close_coverage).await;
    p.f.host.close().await;
}

impl Path {
    async fn begin(&mut self, n: u128, mode: &str) {
        let preparations = if mode == "CANCELLABLE" {
            vec![self.proof(
                "g1",
                "ROUND_PREPARATION",
                json!(rt::hash("namespace", &json!(["g1", n.to_string()])).unwrap()),
            )]
        } else {
            vec![]
        };
        self.cmd("BEGIN",json!({"round":n.to_string(),"predecessor":(n-1).to_string(),"mode":mode,"families":[self.enrollment["families"][0]["key"]],"gateways":["g1"],"preparations":preparations})).await;
    }
    async fn install(&mut self, n: u128, outcome: &str) {
        self.cmd("INSTALL",json!({"gateway":"g1","round":n.to_string(),"outcome":outcome,"proof":self.proof("center","TERMINAL",json!(n.to_string())),"begin":self.proof("center","BEGIN",json!(n.to_string()))})).await;
        self.cmd("ACK_INSTALL",json!({"gateway":"g1","round":n.to_string(),"proof":self.proof("g1","INSTALLATION",json!(n.to_string()))})).await;
    }
    async fn finish(&mut self, n: u128, predecessor: u128) {
        self.begin(n, "FINISH_ONLY").await;
        let bad = self.command("ABORT", json!({"round":n.to_string()}));
        self.refuse(bad, "ABORT_REFUSED").await;
        self.cmd("SEAL_BEGIN",json!({"gateway":"g1","round":n.to_string(),"predecessor":predecessor.to_string(),"proof":self.proof("center","BEGIN",json!(n.to_string()))})).await;
        self.cmd("SEALED", json!({"gateway":"g1","round":n.to_string()}))
            .await;
        self.cmd("DRAIN",json!({"gateway":"g1","round":n.to_string(),"proof":self.proof("g1","SEAL",json!(n.to_string()))})).await;
        self.cmd("READY", json!({"round":n.to_string()})).await;
        self.cmd(
            "CLOSE",
            json!({"round":n.to_string(),"closed_at":context().observed_at}),
        )
        .await;
        self.install(n, "COMMITTED").await;
    }
}
#[tokio::test]
async fn public_retirement_blocks_claim_and_keeps_exact_tombstone_after_reopen() {
    let mut p = Path::enrolled(false).await;
    for i in 5..7 {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    let grant = p.f.v["commands"][5]["payload"]["grant"]["id"].clone();
    p.cmd("RETIRE_GRANT", json!({"grant":grant})).await;
    p.refuse(p.f.v["commands"][7].clone(), "GRANT_UNAVAILABLE")
        .await;
    p.cmd(
        "LOCAL_TERMINAL",
        json!({"gateway":"g1","grant":grant,"proof":p.proof("center","RETIREMENT",grant.clone())}),
    )
    .await;
    p.reopen().await;
    p.refuse(p.f.v["commands"][7].clone(), "GRANT_UNAVAILABLE")
        .await;
    p.finish(1, 0).await;
    p.reopen().await;
    p.f.host.close().await;
}
#[tokio::test]
async fn public_cancellable_prepare_abort_and_protected_finish() {
    let mut p = Path::enrolled(false).await;
    p.cmd("PREPARE_ROUND",json!({"gateway":"g1","round":"1","predecessor":"0","mode":"CANCELLABLE","enrollment":rt::hash("enrollment",&p.enrollment).unwrap(),"proof":p.proof("center","ENROLLMENT",json!("registration"))})).await;
    p.begin(1, "CANCELLABLE").await;
    p.cmd("SEAL_BEGIN",json!({"gateway":"g1","round":"1","predecessor":"0","proof":p.proof("center","BEGIN",json!("1"))})).await;
    p.cmd("ABORT", json!({"round":"1"})).await;
    let bad = p.command(
        "CLOSE",
        json!({"round":"1","closed_at":context().observed_at}),
    );
    p.refuse(bad, "NOT_READY").await;
    p.install(1, "ABORTED").await;
    p.reopen().await;
    p.finish(2, 1).await;
    p.reopen().await;
    p.f.host.close().await;
}
#[tokio::test]
async fn public_extension_replacement_supplement_return_and_finish() {
    let mut p = Path::enrolled(true).await;
    let delta = wire::Resource::from_dimensions([Count::new(1).unwrap(); 6]);
    p.cmd("EXTEND_RESOURCES", json!({"host":"g1","resources":delta}))
        .await;
    let bad = p.command("EXTEND_RESOURCES", json!({"host":"g1","resources":delta}));
    p.refuse(bad, "RESOURCE_BACKING").await;
    for i in 5..9 {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    let fence =
        p.f.host
            .writer_fence(&Id::parse("g1").unwrap(), context(), Duration::from_secs(5))
            .await
            .unwrap();
    let payload = json!({"gateway":"g1","old_epoch":"1","new_epoch":"2","journal_head":p.roots["g1"],"fence":fence});
    let mut wrong = payload.clone();
    wrong["fence"] = json!("0".repeat(64));
    let bad = p.command("REPLACE_WRITER", wrong);
    p.refuse(bad, "FENCE_PROOF").await;
    p.cmd("REPLACE_WRITER", payload).await;
    p.reopen().await;
    p.refuse(p.f.v["commands"][9].clone(), "WRITER_EPOCH").await;
    let mut receive = p.f.v["commands"][9].clone();
    receive["payload"]["epoch"] = json!("2");
    let r = p.step(receive).await;
    let [wire::Effect::Receipt { body }] = r.effects.as_slice() else {
        panic!("receipt")
    };
    assert_eq!(body.epoch.value(), 2);
    p.step(p.f.v["commands"][10].clone()).await;
    let vector:Value=serde_json::from_str(include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/supplement-control.json")).unwrap();
    let case = p.f.v["commands"][9]["payload"]["submission"]["case"].clone();
    let evidence = vector["commands"][8]["payload"]["evidence"].clone();
    p.cmd("SUPPLEMENT", json!({"case":case,"evidence":evidence}))
        .await;
    let mut bad = p.command("SUPPLEMENT", json!({"case":case,"evidence":evidence}));
    bad["payload"]["evidence"][0]["sha256"] = json!("0".repeat(64));
    p.refuse(bad, "EVIDENCE_HASH").await;
    p.step(p.f.v["commands"][11].clone()).await;
    let bad = p.command("SUPPLEMENT", json!({"case":case,"evidence":evidence}));
    p.refuse(bad, "CASE_FINAL").await;
    for i in 12..16 {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    p.cmd("RETURN_UNUSED",json!({"gateway":"g3","token":"token2","claim":p.f.v["commands"][14]["payload"]["token"]["claim"],"proof":p.proof("center","CLAIM",json!("token2"))})).await;
    for i in [44, 45] {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    p.cmd(
        "RECONCILE",
        json!({"token":"token2","proof":p.proof("g3","RETURNED_UNUSED",json!("token2"))}),
    )
    .await;
    for i in [47, 60, 62, 64] {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    p.finish(1, 0).await;
    p.reopen().await;
    for k in [
        "EXTEND_RESOURCES",
        "REPLACE_WRITER",
        "SUPPLEMENT",
        "RETURN_UNUSED",
    ] {
        assert!(p.kinds.contains(k));
    }
    p.f.host.close().await;
}

#[tokio::test]
async fn public_host_to_peer_closed_gateway_then_primary_import_and_finish() {
    use crate::gateway::{GatewayConfig, GatewayContext, OfflineGateway};
    let mut p = Path::enrolled(false).await;
    for i in 5..9 {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    let c = p.f.v["commands"][9].clone();
    let key: wire::Delivery = serde_json::from_value(c["key"].clone()).unwrap();
    let payload: wire::Receive = serde_json::from_value(c["payload"].clone()).unwrap();
    let config =
        p.f.configs
            .iter()
            .find(|c| c.host.as_str() == "g1")
            .unwrap()
            .clone();
    let gateway_config = GatewayConfig {
        database: config.database,
        anchor: config.anchor,
        central_store: config.store,
        scope: config.scope,
        registration: config.registration,
        gateway: config.host,
        resources: config.resources,
        legacy_pages: config.legacy_pages,
        backing_bytes: config.backing_bytes,
        authority_source: config.authority_source,
        authority_id: config.authority_id,
    };
    let context = GatewayContext {
        principal: context().principal,
        observed_at: context().observed_at,
    };
    p.f.host.close().await; // Every central and peer handle is closed before intake.
    let gateway = OfflineGateway::open(gateway_config.clone()).await.unwrap();
    assert!(gateway
        .receipt_status(key.clone(), context.clone(), Duration::from_secs(5))
        .await
        .unwrap()
        .is_none());
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
    let before = gateway.test_store().test_full_inventory().await;
    let response = gateway
        .receive_receipt(
            key.clone(),
            payload.clone(),
            context.clone(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
    assert_eq!(response.knowledge, wire::RetryResponseKnowledge::Unknown);
    assert_eq!(
        response.current_lifecycle,
        wire::RetryResponseCurrentLifecycle::Unknown
    );
    assert_eq!(
        response.central_admission,
        wire::RetryResponseCentralAdmission::Unknown
    );
    assert!(response.prefix.is_none());
    assert!(response.coverage.is_empty());
    let [wire::Effect::Receipt { body }] = receipt.effects.as_slice() else {
        panic!("receipt")
    };
    assert_eq!(response.receipt, *body);
    assert_eq!(
        gateway
            .receipt_status(key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap(),
        Some(response.clone())
    );
    assert_eq!(before, gateway.test_store().test_full_inventory().await);
    gateway.close().await;
    let gateway = OfflineGateway::open(gateway_config.clone()).await.unwrap();
    assert_eq!(
        gateway
            .receipt_status(key.clone(), context.clone(), Duration::from_secs(30))
            .await
            .unwrap(),
        Some(response.clone())
    );
    gateway.close().await;
    p.f.host = LocalHost::open(
        p.f.configs.clone(),
        p.f.sources.clone(),
        Id::parse("operator").unwrap(),
        p.f.verifier.clone(),
    )
    .await
    .unwrap();
    *p.ordinals.get_mut("g1").unwrap() += 1;
    p.roots.insert("g1".into(), receipt.root.clone());
    let proof =
        p.f.host
            .source_proof(
                &Id::parse("g1").unwrap(),
                Count::new(p.ordinals["g1"]).unwrap(),
                wire::FactKind::Receipt,
                wire::ProofFullKey::V2(Id::parse("token1").unwrap()),
                super::public_paths::context(),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
    assert_eq!(proof.root, receipt.root);
    let proof = serde_json::to_value(proof).unwrap();
    p.proofs.insert(pk(&proof), proof);
    for i in [10, 11, 44, 45, 60, 62] {
        p.step(p.f.v["commands"][i].clone()).await;
    }
    p.finish(1, 0).await;
    p.reopen().await;
    p.f.host.close().await;
    let gateway = OfflineGateway::open(gateway_config).await.unwrap();
    let after = gateway
        .receipt_status(key, context, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.receipt, response.receipt);
    assert_eq!(after.knowledge, wire::RetryResponseKnowledge::Unknown);
    assert_eq!(
        after.central_admission,
        wire::RetryResponseCentralAdmission::Unknown
    );
    gateway.close().await;
}

async fn public_comparisons(p: &mut Path, coverage: Vec<wire::Coverage>) {
    use crate::store::adjudication::{
        AdjudicationReadStore, AdjudicationReadTx, SnapshotSelection,
    };
    let config =
        p.f.configs
            .iter()
            .find(|c| c.host.as_str() == "center")
            .unwrap()
            .clone();
    let budget = wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).unwrap(),
        pages: Count::new(1 << 24).unwrap(),
        segments: Count::new(1).unwrap(),
    };
    // This private SELECT observes the requested cutoff; all report operations
    // below go through the exported Ledger API on an independently reopened owner.
    let read = p.f.host.entries["center"]
        .store
        .begin_adjudication_read(
            &SnapshotSelection {
                journal: config.identity(),
                historical: None,
            },
            &budget,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    let expected = read.expected_prefix().expected().clone();
    read.finish().await.unwrap();
    assert_eq!(expected.root, p.roots["center"]);
    assert_eq!(expected.ordinal.value(), 55);
    assert_eq!(coverage.len(), 4);
    let mut before = BTreeMap::new();
    for (name, e) in &p.f.host.entries {
        before.insert(name.clone(), e.store.test_full_inventory().await);
    }
    for e in std::mem::take(&mut p.f.host.entries).into_values() {
        e.store.close().await;
    }
    for amount in [1200u128, 1500, 6000] {
        let ledger = crate::Ledger::open_sqlite_fenced(&config.database, &config.anchor)
            .await
            .unwrap();
        let mut request = wire::ComparisonRequest {
            expected: expected.clone(),
            policy: wire::ComparisonPolicy {
                resolution_atoms: Count::new(amount).unwrap(),
            },
            budget: budget.clone(),
            coverage: coverage.clone(),
            cursor: None,
        };
        let mut comparison = ledger.start_comparison(&request, &budget).await.unwrap();
        let mut calls = 0;
        loop {
            calls += 1;
            assert!(calls <= 56, "public continuation must progress");
            match comparison.advance(&request).await.unwrap() {
                wire::ComparisonResponse::Incomplete { cursor, .. } => {
                    request.cursor = Some(*cursor)
                }
                wire::ComparisonResponse::Comparable {
                    expected: at,
                    actual,
                    alternative,
                    difference,
                    supplier_booked,
                    coverage: reported,
                    ..
                } => {
                    assert_ne!(amount, 6000);
                    assert_eq!(at, expected);
                    assert_eq!(actual.value(), 11450);
                    assert_eq!(
                        alternative.value(),
                        if amount == 1200 { 11450 } else { 11750 }
                    );
                    assert_eq!(difference.value(), if amount == 1200 { 0 } else { 300 });
                    assert_eq!(supplier_booked.value(), 3000);
                    assert_eq!(reported, coverage);
                    break;
                }
                wire::ComparisonResponse::PolicyFailure { reason, .. } => {
                    assert_eq!(amount, 6000);
                    assert_eq!(reason, "PREMIUM_CAP at central ordinal 5");
                    break;
                }
                other => panic!("unexpected public comparison: {other:?}"),
            }
        }
        assert_eq!(calls, 55);
        eprintln!("public Ledger comparison policy{amount}:55segments/55calls, exact result, peers closed");
        drop(comparison);
        ledger.close().await;
    }
    p.reopen().await;
    for (name, e) in &p.f.host.entries {
        assert_eq!(
            e.store.test_full_inventory().await,
            before[name],
            "public comparison/reopen mutation at {name}"
        );
    }
}
