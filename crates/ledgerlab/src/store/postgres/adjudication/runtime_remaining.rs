//! Native runtime branch controls under an ASSUMED TEST-ONLY physical envelope.
//! No production admission or positive writer-replacement fence is supplied.
use super::runtime_customer::{fact, hydrate, proof_key};
use super::*;
use r3::runtime as rt;
use serde_json::Value;
struct Harness {
    host: Host,
    input: Value,
    roots: BTreeMap<String, Digest>,
    ordinals: BTreeMap<String, u128>,
    proofs: BTreeMap<String, Value>,
    enrollment: Value,
    serial: usize,
    last: Option<(String, Value, wire::CommandResult)>,
    ceilings: BTreeMap<String, wire::Resource>,
}
impl Harness {
    async fn new() -> Self {
        Self::with_extension_backing(false).await
    }
    async fn with_extension_backing(extend: bool) -> Self {
        let input:Json=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap();
        let sources: Vec<wire::AuthoritySource> =
            serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
        let auth = sources
            .iter()
            .find(|s| {
                s.body_hash.as_str()
                    == input["commands"][0]["authority"]["document"]
                        .as_str()
                        .unwrap()
            })
            .unwrap()
            .clone();
        let mut host = Host {
            stores: BTreeMap::new(),
            heads: BTreeMap::new(),
            base: BaseFixture::new(&input),
            sources,
            now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
                .unwrap(),
            fault_count: Arc::new(AtomicU64::new(0)),
        };
        let mut roots = BTreeMap::new();
        let mut ordinals = BTreeMap::new();
        let mut ceilings = BTreeMap::new();
        for name in ["center", "g0", "g1", "g2", "g3"] {
            let mut f = Fixture::new_scope("synthetic").await;
            let head = provision(&mut f, &journal(name), &auth).await;
            if name == "center" {
                provision_original(&mut f, &host.base).await;
            }
            host.heads.insert(name.into(), head);
            host.stores.insert(name.into(), f);
            roots.insert(name.into(), Digest::parse(&"0".repeat(64)).unwrap());
            ordinals.insert(name.into(), 0);
            // An explicitly assumed logical ceiling for branch execution, NOT
            // a real preallocated PG quota or protected capacity observation.
            let ceiling = if extend && name == "g1" {
                budget(name)
                    .checked_add(&wire::Resource::from_dimensions(
                        [Count::new(1).unwrap(); 6],
                    ))
                    .unwrap()
            } else {
                budget(name)
            };
            ceilings.insert(name.into(), ceiling);
        }
        let mut h = Self {
            host,
            input,
            roots,
            ordinals,
            ceilings,
            proofs: BTreeMap::new(),
            enrollment: Value::Null,
            serial: 0,
            last: None,
        };
        for i in 0..5 {
            h.step(h.input["commands"][i].clone()).await;
        }
        h
    }
    fn command(&mut self, kind: &str, payload: Value) -> Value {
        self.serial += 1;
        let mut c = self.input["commands"][0].clone();
        c["kind"] = json!(kind);
        c["payload"] = payload;
        c["key"][2] = json!(format!("pg-remaining-{}", self.serial));
        c["authority"]["permission"] = json!(match kind {
            "ABORT" | "BEGIN" | "CLOSE" => "close",
            "SUPPLEMENT" | "RECEIVE" => "submit",
            "REPLACE_WRITER" => "replace",
            _ => "capacity",
        });
        c
    }
    fn proof(&self, host: &str, kind: &str, key: Value) -> Value {
        self.proofs[&serde_json::to_string(&json!([host, kind, key])).unwrap()].clone()
    }
    async fn execute(
        &mut self,
        mut c: Value,
        expected: Option<&str>,
    ) -> Option<wire::CommandResult> {
        let kind = c["kind"].as_str().unwrap().to_owned();
        let owner = match kind.as_str() {
            "PREPARE_ENROLL" | "PREPARE_ROUND" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED"
            | "LOCAL_TERMINAL" | "SEAL_BEGIN" | "SEALED" | "INSTALL" | "REPLACE_WRITER" => {
                c["payload"]["gateway"].as_str().unwrap()
            }
            "EXTEND_RESOURCES" => c["payload"]["host"].as_str().unwrap(),
            "LOCAL_GRANT" => c["payload"]["grant"]["gateway"].as_str().unwrap(),
            _ => "center",
        }
        .to_owned();
        if kind == "PREPARE_ROUND" {
            c["payload"]["enrollment"] = json!(rt::hash("enrollment", &self.enrollment).unwrap());
        }
        self.host.now = serde_json::from_value(c["authority"]["observed_at"].clone()).unwrap();
        hydrate(&mut c, &self.proofs, &self.roots[&owner]);
        if kind == "REGISTER_GRANT" {
            let proof = serde_json::from_value(c["payload"]["proof"].clone()).unwrap();
            let source = self
                .host
                .source(&SourceRequest::Exact(Box::new(proof)))
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(
                &r3::proofs::decode_base64(&source.object().body, r3::COMMAND_BYTES).unwrap(),
            )
            .unwrap();
            c["payload"]["grant"] = body["payload"]["grant"].clone();
            hydrate(&mut c, &self.proofs, &self.roots[&owner]);
        }
        // Every refusal checks all retained tables on every journal, so an
        // ownership leak or late mutation cannot hide in another host.
        let mut before = BTreeMap::new();
        for (name, f) in &self.host.stores {
            before.insert(name.clone(), all_inventory(f).await);
        }
        let result = execute_with_ceiling(
            &self.host,
            &owner,
            &c,
            None,
            Some(self.ceilings[&owner].clone()),
        )
        .await;
        if let Some(code) = expected {
            assert!(
                matches!(&result,Err(ServiceError::Rejection(actual)) if actual==code),
                "{kind} expected{code}: {result:?}"
            );
            for (name, f) in &self.host.stores {
                assert_eq!(before[name], all_inventory(f).await, "refused host{name}");
            }
            eprintln!(
                "PG_REMAINING_REFUSED {}",
                json!({"kind":kind,"owner":owner,"code":code,"all_host_inventory_unchanged":true})
            );
            return None;
        }
        let result = result.unwrap_or_else(|e| panic!("{kind} at{}: {e}", self.ordinals[&owner]));
        assert_eq!(result.status, wire::CommandResultStatus::Committed);
        assert_eq!(result.code, kind);
        self.roots.insert(owner.clone(), result.root.clone());
        *self.ordinals.get_mut(&owner).unwrap() += 1;
        for (name, f) in &self.host.stores {
            if *name != owner {
                assert_eq!(before[name], all_inventory(f).await, "foreign host{name}");
            }
        }
        if kind == "ENROLL" {
            self.enrollment = c["payload"].clone();
        }
        let object = match kind.as_str() {
            "PREPARE_ROUND" => Some((
                "ROUND_PREPARATION",
                json!(rt::hash(
                    "namespace",
                    &json!([c["payload"]["gateway"], c["payload"]["round"]])
                )
                .unwrap()),
            )),
            "RETIRE_GRANT" => Some(("RETIREMENT", c["payload"]["grant"].clone())),
            "ABORT" => Some(("TERMINAL", c["payload"]["round"].clone())),
            _ => fact(&c),
        };
        if let Some((kind, key)) = object {
            let source = self
                .host
                .export(
                    &journal(&owner),
                    Count::new(self.ordinals[&owner]).unwrap(),
                    serde_json::from_value(json!(kind)).unwrap(),
                    serde_json::from_value(key).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(source.prefix().root(), &result.root);
            let p = serde_json::to_value(source.proof()).unwrap();
            self.proofs.insert(proof_key(&p), p);
        }
        let before = all_inventory(&self.host.stores[&owner]).await;
        let mut retry = c.clone();
        retry["authority"]["permission"] = json!("read");
        let saved = execute_with_ceiling(
            &self.host,
            &owner,
            &retry,
            None,
            Some(self.ceilings[&owner].clone()),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, result.root);
        assert_eq!(saved.effects, result.effects);
        assert_eq!(before, all_inventory(&self.host.stores[&owner]).await);
        eprintln!(
            "PG_REMAINING_COMMIT {}",
            json!({"kind":kind,"owner":owner,"ordinal":self.ordinals[&owner].to_string(),"command":c,"result":result,"retry_no_growth":true})
        );
        self.last = Some((owner, retry, result.clone()));
        Some(result)
    }
    async fn step(&mut self, c: Value) -> wire::CommandResult {
        self.execute(c, None).await.unwrap()
    }
    async fn command_step(&mut self, k: &str, p: Value) -> wire::CommandResult {
        let c = self.command(k, p);
        self.step(c).await
    }
    async fn state(
        &self,
        host: &str,
        kind: HeadKind,
        point: rt::points::Point,
        _class: GuardClass,
    ) -> (ObservedHead, State) {
        let key = HeadKey {
            journal: journal(host),
            kind,
            full_key: point.key,
        };
        let h = crate::store::postgres::adjudication::read::point(
            &self.host.stores[host].owner.client,
            &key,
            r3::SEGMENT_BYTES,
        )
        .await
        .unwrap();
        let state = serde_json::from_slice(h.value.as_deref().unwrap()).unwrap();
        (h, state)
    }
    async fn reopen(&mut self) {
        for f in self.host.stores.values_mut() {
            f.store.clone().close().await;
            f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
        }
        let (owner, c, prior) = self.last.as_ref().unwrap();
        let before = all_inventory(&self.host.stores[owner]).await;
        let saved = execute_with_ceiling(
            &self.host,
            owner,
            c,
            None,
            Some(self.ceilings[owner].clone()),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, prior.root);
        assert_eq!(saved.effects, prior.effects);
        assert_eq!(before, all_inventory(&self.host.stores[owner]).await);
    }
    async fn close(self) {
        for (_, f) in self.host.stores {
            finish_retained(f).await;
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY"]
async fn native_remaining_retirement_permanently_blocks_claim_and_releases_only_proven_local_slack()
{
    let mut h = Harness::new().await;
    h.step(h.input["commands"][5].clone()).await;
    h.step(h.input["commands"][6].clone()).await;
    let grant = h.input["commands"][5]["payload"]["grant"]["id"].clone();
    h.command_step("RETIRE_GRANT", json!({"grant":grant})).await;
    h.execute(h.input["commands"][7].clone(), Some("GRANT_UNAVAILABLE"))
        .await;
    let proof = h.proof("center", "RETIREMENT", grant.clone());
    h.command_step(
        "LOCAL_TERMINAL",
        json!({"gateway":"g1","grant":grant,"proof":proof}),
    )
    .await;
    h.reopen().await;
    let (_, state) = h
        .state(
            "g1",
            HeadKind::Grant,
            rt::points::Point::id(
                rt::points::PointKind::Grant,
                *b"GRANT___",
                grant.as_str().unwrap(),
            )
            .unwrap(),
            GuardClass::CapacityAllocation,
        )
        .await;
    let State::Grant(g) = state else {
        panic!("grant")
    };
    assert!(g.terminal);
    assert_eq!(g.status, rt::points::GrantStatus::RetiredUnclaimed);
    assert!(g.token.is_none());
    h.close().await;
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY"]
async fn native_remaining_supplement_preserves_submission_receipt_and_enforces_cumulative_bound() {
    let mut h = Harness::new().await;
    for i in 5..11 {
        h.step(h.input["commands"][i].clone()).await;
    }
    let case: wire::Case =
        serde_json::from_value(h.input["commands"][9]["payload"]["submission"]["case"].clone())
            .unwrap();
    let point = rt::points::Point::case(&case).unwrap();
    let (_, State::Case(before)) = h
        .state("center", HeadKind::Case, point.clone(), GuardClass::Case)
        .await
    else {
        panic!("case")
    };
    let vector:Value=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/supplement-control.json")).unwrap();
    h.command_step(
        "SUPPLEMENT",
        json!({"case":case,"evidence":vector["commands"][8]["payload"]["evidence"]}),
    )
    .await;
    let (_, State::Case(after)) = h
        .state("center", HeadKind::Case, point.clone(), GuardClass::Case)
        .await
    else {
        panic!("case")
    };
    assert_eq!(before.input, after.input);
    assert_eq!(before.receipt, after.receipt);
    assert!(!after.supplements.is_empty());
    let mut bad = h.command(
        "SUPPLEMENT",
        json!({"case":case,"evidence":vector["commands"][8]["payload"]["evidence"]}),
    );
    bad["payload"]["evidence"][0]["sha256"] = json!("0".repeat(64));
    h.execute(bad, Some("EVIDENCE_HASH")).await;
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let needed = 16 - after.input.evidence.len() - after.supplements.len();
    let mut added: Vec<Value> = (0..needed)
        .map(|n| {
            let b = (n as u8) + 128;
            let body = String::from_utf8(vec![
                alphabet[(b >> 2) as usize],
                alphabet[((b & 3) << 4) as usize],
                b'=',
                b'=',
            ])
            .unwrap();
            json!({"body":body,"sha256":r3::raw_sha256(&[b])})
        })
        .collect();
    added.sort_by_cached_key(|v| r3::canonical_bytes(v, r3::COMMAND_BYTES).unwrap());
    h.command_step("SUPPLEMENT", json!({"case":case,"evidence":added}))
        .await;
    let before_limit = h
        .state("center", HeadKind::Case, point.clone(), GuardClass::Case)
        .await
        .0;
    let extra = json!({"body":"/w==","sha256":r3::raw_sha256(&[255])});
    let c = h.command("SUPPLEMENT", json!({"case":case,"evidence":[extra]}));
    h.execute(c, Some("EVIDENCE_LIMIT")).await;
    let after_limit = h
        .state("center", HeadKind::Case, point.clone(), GuardClass::Case)
        .await
        .0;
    assert_eq!(before_limit.value, after_limit.value);
    assert_eq!(before_limit.revision, after_limit.revision);
    h.step(h.input["commands"][11].clone()).await;
    h.reopen().await;
    let c = h.command(
        "SUPPLEMENT",
        json!({"case":case,"evidence":vector["commands"][8]["payload"]["evidence"]}),
    );
    h.execute(c, Some("CASE_FINAL")).await;
    h.close().await;
}

impl Harness {
    async fn prepare_round(&mut self, n: u128, predecessor: u128) {
        let proof = self.proof("center", "ENROLLMENT", json!("registration"));
        self.command_step("PREPARE_ROUND",json!({"round":n.to_string(),"predecessor":predecessor.to_string(),"gateway":"g1","mode":"CANCELLABLE","enrollment":rt::hash("enrollment",&self.enrollment).unwrap(),"proof":proof})).await;
    }
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
        self.command_step("BEGIN",json!({"round":n.to_string(),"predecessor":(n-1).to_string(),"mode":mode,"families":[self.enrollment["families"][0]["key"]],"gateways":["g1"],"preparations":preparations})).await;
    }
    async fn seal_begin(&mut self, n: u128, predecessor: u128) {
        let proof = self.proof("center", "BEGIN", json!(n.to_string()));
        self.command_step("SEAL_BEGIN",json!({"gateway":"g1","round":n.to_string(),"predecessor":predecessor.to_string(),"proof":proof})).await;
    }
    async fn sealed(&mut self, n: u128) {
        self.command_step("SEALED", json!({"gateway":"g1","round":n.to_string()}))
            .await;
    }
    async fn drain(&mut self, n: u128) {
        let proof = self.proof("g1", "SEAL", json!(n.to_string()));
        self.command_step(
            "DRAIN",
            json!({"gateway":"g1","round":n.to_string(),"proof":proof}),
        )
        .await;
    }
    async fn ready(&mut self, n: u128) {
        self.command_step("READY", json!({"round":n.to_string()}))
            .await;
    }
    fn terminal_command(&mut self, kind: &str, n: u128) -> Value {
        if kind == "CLOSE" {
            self.command(
                kind,
                json!({"round":n.to_string(),"closed_at":self.host.now}),
            )
        } else {
            self.command(kind, json!({"round":n.to_string()}))
        }
    }
    async fn install(&mut self, n: u128, outcome: &str) {
        let proof = self.proof("center", "TERMINAL", json!(n.to_string()));
        let begin = self.proof("center", "BEGIN", json!(n.to_string()));
        self.command_step("INSTALL",json!({"gateway":"g1","round":n.to_string(),"outcome":outcome,"proof":proof,"begin":begin})).await;
        let proof = self.proof("g1", "INSTALLATION", json!(n.to_string()));
        self.command_step(
            "ACK_INSTALL",
            json!({"gateway":"g1","round":n.to_string(),"proof":proof}),
        )
        .await;
    }
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY"]
async fn native_remaining_cancellable_round_abort_cuts_and_terminal_races_preserve_finish_rights() {
    for cut in 0..6 {
        let mut h = Harness::new().await;
        h.prepare_round(1, 0).await;
        h.begin(1, "CANCELLABLE").await;
        if cut >= 2 {
            h.seal_begin(1, 0).await;
        }
        if cut >= 3 {
            h.sealed(1).await;
        }
        if cut >= 4 {
            h.drain(1).await;
            h.ready(1).await;
        }
        if cut == 5 {
            let c = h.terminal_command("CLOSE", 1);
            h.step(c).await;
            let c = h.terminal_command("ABORT", 1);
            h.execute(c, Some("ABORT_REFUSED")).await;
            h.install(1, "COMMITTED").await;
        } else {
            let c = h.terminal_command("ABORT", 1);
            h.step(c).await;
            // The local primary has not observed the abort; its valid BEGIN
            // proof remains actionable. INSTALL later learns the terminal fact.
            if cut == 1 {
                h.seal_begin(1, 0).await;
            }
            let c = h.terminal_command("CLOSE", 1);
            h.execute(c, Some("NOT_READY")).await;
            h.install(1, "ABORTED").await;
            let (_, State::Family(f)) = h
                .state(
                    "center",
                    HeadKind::Family,
                    rt::points::Point::family(
                        &serde_json::from_value(h.enrollment["families"][0]["key"].clone())
                            .unwrap(),
                    )
                    .unwrap(),
                    GuardClass::FamilyPrerequisite,
                )
                .await
            else {
                panic!("family")
            };
            assert!(!f.closed);
            assert!(f.first_closure.is_none());
            // Aborted optional round preserves the original protected close.
            h.begin(2, "FINISH_ONLY").await;
            let c = h.terminal_command("ABORT", 2);
            h.execute(c, Some("ABORT_REFUSED")).await;
            h.seal_begin(2, 1).await;
            h.sealed(2).await;
            h.drain(2).await;
            h.ready(2).await;
            let c = h.terminal_command("CLOSE", 2);
            h.step(c).await;
            h.install(2, "COMMITTED").await;
        }
        h.reopen().await;
        h.close().await;
    }
}

#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY"]
async fn native_remaining_extension_requires_preallocated_host_ceiling_and_preserves_other_hosts() {
    let mut h = Harness::with_extension_backing(true).await;
    let point = |host: &str| {
        rt::points::Point::id(rt::points::PointKind::Resource, *b"RESOURCE", host).unwrap()
    };
    let center = h
        .state(
            "center",
            HeadKind::Resource,
            point("center"),
            GuardClass::CapacityAllocation,
        )
        .await
        .0;
    let delta = wire::Resource::from_dimensions([Count::new(1).unwrap(); 6]);
    h.command_step("EXTEND_RESOURCES", json!({"host":"g1","resources":delta}))
        .await;
    let (before, State::Resource(resources)) = h
        .state(
            "g1",
            HeadKind::Resource,
            point("g1"),
            GuardClass::CapacityAllocation,
        )
        .await
    else {
        panic!("resources")
    };
    assert_eq!(resources.provisioned, h.ceilings["g1"]);
    h.reopen().await;
    let c = h.command("EXTEND_RESOURCES", json!({"host":"g1","resources":delta}));
    h.execute(c, Some("RESOURCE_BACKING")).await;
    let after = h
        .state(
            "g1",
            HeadKind::Resource,
            point("g1"),
            GuardClass::CapacityAllocation,
        )
        .await
        .0;
    assert_eq!(before.value, after.value);
    assert_eq!(before.revision, after.revision);
    let after = h
        .state(
            "center",
            HeadKind::Resource,
            point("center"),
            GuardClass::CapacityAllocation,
        )
        .await
        .0;
    assert_eq!(center.value, after.value);
    assert_eq!(center.revision, after.revision);
    h.close().await;
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope ASSUMED TEST ONLY"]
async fn native_remaining_return_unused_is_permanent_and_missing_fence_refuses() {
    let mut h = Harness::new().await;
    for i in 5..9 {
        h.step(h.input["commands"][i].clone()).await;
    }
    let c=h.command("REPLACE_WRITER",json!({"gateway":"g1","old_epoch":"1","new_epoch":"2","journal_head":h.roots["g1"],"fence":"0".repeat(64)}));
    h.execute(c, Some("FENCE_PROOF")).await;
    let claim = h.input["commands"][7]["payload"]["token"]["claim"].clone();
    let proof = h.proof("center", "CLAIM", json!("token1"));
    h.command_step(
        "RETURN_UNUSED",
        json!({"gateway":"g1","token":"token1","claim":claim,"proof":proof}),
    )
    .await;
    h.reopen().await;
    h.execute(h.input["commands"][9].clone(), Some("TOKEN_STATE"))
        .await;
    let proof = h.proof("g1", "RETURNED_UNUSED", json!("token1"));
    h.command_step("RECONCILE", json!({"token":"token1","proof":proof}))
        .await;
    h.step(h.input["commands"][45].clone()).await;
    h.step(h.input["commands"][60].clone()).await;
    h.begin(1, "FINISH_ONLY").await;
    h.seal_begin(1, 0).await;
    h.sealed(1).await;
    h.drain(1).await;
    h.ready(1).await;
    let c = h.terminal_command("CLOSE", 1);
    h.step(c).await;
    h.install(1, "COMMITTED").await;
    h.reopen().await;
    let (_, State::Resource(r)) = h
        .state(
            "g1",
            HeadKind::Resource,
            rt::points::Point::id(rt::points::PointKind::Resource, *b"RESOURCE", "g1").unwrap(),
            GuardClass::CapacityAllocation,
        )
        .await
    else {
        panic!("resource")
    };
    assert_eq!(r.q.writer_epoch.value(), 1);
    assert_eq!(r.q.receipt.value(), 0);
    h.close().await;
}
