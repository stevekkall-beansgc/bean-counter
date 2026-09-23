//! Real fenced SQLite branches beyond the customer chronology. Command patterns
//! are pinned by grant-retirement, supplement-control, abort-*, terminal-race-*
//! and unseen-abort-delayed-begin frozen vectors; original customer base stays exact.
use super::*;
use serde_json::json;
struct RemainingHost(FlowHost);
impl AdjudicationAuthority for RemainingHost {
    fn current(
        &self,
        c: &ParsedCommand,
        i: &LockedInputs,
        a: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let mut observed = self.0.current(c, i, a)?;
        if matches!(a, AuthorityAccess::NewTransition) {
            observed.permission = match c.command() {
                wire::Command::Abort { .. } => wire::AuthorityPermission::Close,
                wire::Command::Supplement { .. } => wire::AuthorityPermission::Submit,
                wire::Command::ReplaceWriter { .. } => wire::AuthorityPermission::Replace,
                _ => observed.permission,
            };
        }
        Ok(observed)
    }
}
impl AdjudicationHost<SqliteTx> for RemainingHost {
    fn guards(&self, c: &ParsedCommand, j: &JournalIdentity) -> Result<Vec<Guard>, ServiceError> {
        self.0.guards(c, j)
    }
    async fn source(&self, r: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        self.0.source(r).await
    }
    async fn fresh_base(
        &self,
        t: &mut SqliteTx,
        e: &wire::Enroll,
        i: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        self.0.fresh_base(t, e, i).await
    }
}
struct Harness {
    host: RemainingHost,
    input: Value,
    roots: BTreeMap<String, Digest>,
    ordinals: BTreeMap<String, u128>,
    proofs: BTreeMap<String, Value>,
    enrollment: Value,
    serial: usize,
    last: Option<(String, Value, wire::CommandResult)>,
    ceilings: BTreeMap<String, wire::Resource>,
    budgets: BTreeMap<String, wire::Resource>,
    quiet: bool,
    _dirs: Vec<(tempfile::TempDir, tempfile::TempDir)>,
}
impl Harness {
    async fn new() -> Self {
        Self::with_extension_backing(false).await
    }
    async fn with_extension_backing(extend: bool) -> Self {
        let budgets = ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .map(|h| (h.into(), flow_budget(h)))
            .collect();
        Self::with_budgets(budgets, extend, false).await
    }
    async fn with_budgets(
        budgets: BTreeMap<String, wire::Resource>,
        extend: bool,
        quiet: bool,
    ) -> Self {
        let input = fixture();
        let base = BaseFixture::new(&input);
        let sources: Vec<wire::AuthoritySource> =
            serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
        let (mut stores, mut heads, mut dirs) = (BTreeMap::new(), BTreeMap::new(), vec![]);
        let mut ceilings = BTreeMap::new();
        for name in ["center", "g0", "g1", "g2", "g3"] {
            let dir = tempfile::tempdir().unwrap();
            let anchor = tempfile::tempdir().unwrap();
            let mut install = crate::store::sqlite::tests::installation();
            install.logical_store_id = "center".into();
            install.scope.tenant = "synthetic".into();
            install.scope.environment = "sandbox".into();
            let store = SqliteStore::create_fenced(dir.path(), install, anchor.path())
                .await
                .unwrap();
            let ceiling = if extend && name == "g1" {
                budgets[name]
                    .clone()
                    .checked_add(&wire::Resource::from_dimensions(
                        [Count::new(1).unwrap(); 6],
                    ))
                    .unwrap()
            } else {
                budgets[name].clone()
            };
            ceilings.insert(name.to_owned(), ceiling.clone());
            drop(
                store
                    .provision_adjudication_with_ceiling(
                        journal(name),
                        budgets[name].clone(),
                        ceiling,
                        65536,
                        Count::new(1u128 << 40).unwrap(),
                    )
                    .await
                    .unwrap(),
            );
            heads.insert(
                name.into(),
                store
                    .provision_adjudication_authority(&journal(name), &sources[0], None)
                    .await
                    .unwrap(),
            );
            if name == "center" {
                store
                    .test_provision_outcome_evidence(&base.evidence, &base.heads)
                    .await
                    .unwrap();
            }
            stores.insert(name.into(), store);
            dirs.push((dir, anchor));
        }
        let roots = ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .map(|h| (h.into(), Digest::parse(&"0".repeat(64)).unwrap()))
            .collect::<BTreeMap<String, _>>();
        let ordinals = roots.keys().map(|h| (h.clone(), 0)).collect();
        let mut h = Self {
            host: RemainingHost(FlowHost {
                stores,
                heads,
                sources,
                base,
                now: serde_json::from_value(
                    input["commands"][0]["authority"]["observed_at"].clone(),
                )
                .unwrap(),
            }),
            input,
            roots,
            ordinals,
            proofs: BTreeMap::new(),
            enrollment: Value::Null,
            serial: 0,
            last: None,
            ceilings,
            budgets,
            quiet,
            _dirs: dirs,
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
        c["key"][2] = json!(format!("remaining-{}", self.serial));
        c["authority"]["permission"] = json!(match kind {
            "ABORT" | "BEGIN" | "CLOSE" => "close",
            "SUPPLEMENT" | "RECEIVE" => "submit",
            "REPLACE_WRITER" => "replace",
            _ => "capacity",
        });
        c
    }
    fn proof(&self, host: &str, kind: &str, key: Value) -> Value {
        self.proofs[&proof_key(&json!({"host":host,"fact_kind":kind,"full_key":key}))].clone()
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
        hydrate(&mut c, &self.proofs, &self.roots[&owner]);
        if kind == "REGISTER_GRANT" {
            let proof: wire::Proof = serde_json::from_value(c["payload"]["proof"].clone()).unwrap();
            let source = self.host.0.stores[proof.host.as_str()]
                .adjudication_exact_source(&proof)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(
                &r3::proofs::decode_base64(&source.object().body, r3::COMMAND_BYTES).unwrap(),
            )
            .unwrap();
            c["payload"]["grant"] = body["payload"]["grant"].clone();
            hydrate(&mut c, &self.proofs, &self.roots[&owner]);
        }
        let configured = self.host.0.stores[&owner]
            .provision_adjudication_with_ceiling(
                journal(&owner),
                self.budgets[&owner].clone(),
                self.ceilings[&owner].clone(),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        let result = run(
            &configured,
            &self.host,
            journal(&owner),
            parsed(&c),
            deadline(),
        )
        .await;
        if let Some(code) = expected {
            assert!(
                matches!(&result,Err(ServiceError::Rejection(actual)) if actual==code),
                "{kind} expected {code}: {result:?}"
            );
            return None;
        }
        let result = result.unwrap_or_else(|e| panic!("{kind} at {}: {e}", self.ordinals[&owner]));
        assert_eq!(result.status, wire::CommandResultStatus::Committed);
        *self.ordinals.get_mut(&owner).unwrap() += 1;
        self.roots.insert(owner.clone(), result.root.clone());
        if kind == "ENROLL" {
            self.enrollment = c["payload"].clone();
        }
        let key = match kind.as_str() {
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
            "RECEIVE" => Some((
                if serde_json::to_value(&result.effects)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["kind"] == "RECEIPT" && e["body"]["token"] != c["payload"]["token"])
                {
                    "ALIAS"
                } else {
                    "RECEIPT"
                },
                c["payload"]["token"].clone(),
            )),
            "RETURN_UNUSED" => Some(("RETURNED_UNUSED", c["payload"]["token"].clone())),
            "RETIRE_GRANT" => Some(("RETIREMENT", c["payload"]["grant"].clone())),
            "RECONCILE" => Some(("RECONCILIATION", c["payload"]["token"].clone())),
            "BEGIN" => Some(("BEGIN", c["payload"]["round"].clone())),
            "SEALED" => Some(("SEAL", c["payload"]["round"].clone())),
            "CLOSE" | "ABORT" => Some(("TERMINAL", c["payload"]["round"].clone())),
            "INSTALL" => Some(("INSTALLATION", c["payload"]["round"].clone())),
            _ => None,
        };
        if let Some((kind, key)) = key {
            let source = self.host.0.stores[&owner]
                .adjudication_source(
                    &journal(&owner),
                    Count::new(self.ordinals[&owner]).unwrap(),
                    &serde_json::from_value(json!(kind)).unwrap(),
                    &serde_json::from_value(key).unwrap(),
                )
                .await
                .unwrap();
            let proof = serde_json::to_value(source.proof()).unwrap();
            self.proofs.insert(proof_key(&proof), proof);
        }
        c["authority"]["permission"] = json!("read");
        let saved = run(
            &configured,
            &self.host,
            journal(&owner),
            parsed(&c),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, result.root);
        assert_eq!(saved.effects, result.effects);
        self.last = Some((owner.clone(), c, result.clone()));
        if !self.quiet {
            eprintln!(
                "remaining {kind} {owner} ordinal{} root{}",
                self.ordinals[&owner],
                result.root.as_str()
            );
        }
        Some(result)
    }
    async fn step(&mut self, c: Value) -> wire::CommandResult {
        self.execute(c, None).await.unwrap()
    }
    async fn command_step(&mut self, kind: &str, payload: Value) -> wire::CommandResult {
        let c = self.command(kind, payload);
        self.step(c).await
    }
    async fn state(
        &self,
        host: &str,
        kind: HeadKind,
        point: rt::points::Point,
        class: GuardClass,
    ) -> (ObservedHead, State) {
        let configured = self.host.0.stores[host]
            .provision_adjudication_with_ceiling(
                journal(host),
                self.budgets[host].clone(),
                self.ceilings[host].clone(),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        let head = actual_point(
            &configured,
            HeadKey {
                journal: journal(host),
                kind,
                full_key: point.key,
            },
            class,
        )
        .await;
        let state = serde_json::from_slice(head.value.as_deref().unwrap()).unwrap();
        (head, state)
    }
    async fn reopen(&mut self) {
        for store in std::mem::take(&mut self.host.0.stores).into_values() {
            store.close().await;
        }
        for (name, (dir, anchor)) in ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .zip(&self._dirs)
        {
            self.host.0.stores.insert(
                name.into(),
                SqliteStore::open_fenced(dir.path(), anchor.path())
                    .await
                    .unwrap(),
            );
        }
        let (owner, c, prior) = self.last.as_ref().unwrap();
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
        let saved = run(
            &configured,
            &self.host,
            journal(owner),
            parsed(c),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, prior.root);
        assert_eq!(saved.effects, prior.effects);
    }
    async fn close(self) {
        for store in self.host.0.stores.into_values() {
            store.close().await;
        }
    }
}
#[tokio::test]
async fn actual_retirement_permanently_blocks_claim_and_releases_only_proven_local_slack() {
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
async fn actual_supplement_preserves_submission_receipt_and_enforces_cumulative_bound() {
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
                json!({"round":n.to_string(),"closed_at":self.host.0.now}),
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
async fn actual_cancellable_round_abort_cuts_and_terminal_races_preserve_finish_rights() {
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
async fn actual_extension_requires_preallocated_host_ceiling_and_preserves_other_hosts() {
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
async fn actual_replacement_excludes_stale_epoch_and_retains_prepaid_token_finish() {
    let mut h = Harness::new().await;
    for i in 5..9 {
        h.step(h.input["commands"][i].clone()).await;
    }
    let configured = h.host.0.stores["g1"]
        .provision_adjudication_with_ceiling(
            journal("g1"),
            flow_budget("g1"),
            h.ceilings["g1"].clone(),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    let fence = configured.writer_fence(deadline()).await.unwrap();
    drop(configured);
    let payload = json!({"gateway":"g1","old_epoch":"1","new_epoch":"2","journal_head":h.roots["g1"],"fence":fence});
    let mut invalid = payload.clone();
    invalid["fence"] = json!("0".repeat(64));
    let c = h.command("REPLACE_WRITER", invalid);
    h.execute(c, Some("FENCE_PROOF")).await;
    let mut invalid = payload.clone();
    invalid["journal_head"] = json!("0".repeat(64));
    let c = h.command("REPLACE_WRITER", invalid);
    h.execute(c, Some("FENCE_PROOF")).await;
    let mut invalid = payload.clone();
    invalid["new_epoch"] = json!("3");
    let c = h.command("REPLACE_WRITER", invalid);
    h.execute(c, Some("WRITER_EPOCH")).await;
    h.command_step("REPLACE_WRITER", payload).await;
    h.reopen().await;
    h.execute(h.input["commands"][9].clone(), Some("WRITER_EPOCH"))
        .await;
    let mut receive = h.input["commands"][9].clone();
    receive["payload"]["epoch"] = json!("2");
    let result = h.step(receive).await;
    let wire::Effect::Receipt { body } = &result.effects[0] else {
        panic!("receipt")
    };
    assert_eq!(body.epoch.value(), 2);
    h.step(h.input["commands"][10].clone()).await;
    for index in [44, 45, 60, 62] {
        h.step(h.input["commands"][index].clone()).await;
    }
    h.begin(1, "FINISH_ONLY").await;
    h.seal_begin(1, 0).await;
    h.sealed(1).await;
    h.drain(1).await;
    h.ready(1).await;
    let c = h.terminal_command("CLOSE", 1);
    h.step(c).await;
    h.install(1, "COMMITTED").await;
    let (_, State::Resource(resource)) = h
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
    assert_eq!(resource.q.writer_epoch.value(), 2);
    h.close().await;
}

#[path = "gateway_tests.rs"]
mod gateway_tests;
#[path = "scale_tests.rs"]
mod scale_tests;

#[path = "race_tests.rs"]
mod race_tests;
