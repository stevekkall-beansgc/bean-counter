//! Real child-process publication interruptions on CLOSE and local INSTALL.
//! No timeout is interpreted as a commit result; actual primary recovery decides.
use super::*;
#[tokio::test]
async fn protected_process_entry() {
    let Ok(path) = std::env::var("LEDGERLAB_PROTECTED_PROCESS") else {
        return;
    };
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let input = fixture();
    let sources: Vec<wire::AuthoritySource> =
        serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
    let mut stores = BTreeMap::new();
    let mut heads = BTreeMap::new();
    for host in ["center", "g0", "g1", "g2", "g3"] {
        let store = SqliteStore::open_fenced(
            std::path::Path::new(v["paths"][host][0].as_str().unwrap()),
            std::path::Path::new(v["paths"][host][1].as_str().unwrap()),
        )
        .await
        .unwrap();
        // Existing exact current document, CAS at its actual retained revision1;
        // identical-body provisioning rolls back and returns the observed head.
        let head = store
            .provision_adjudication_authority(
                &journal(host),
                &sources[0],
                Some(Count::new(1).unwrap()),
            )
            .await
            .unwrap();
        heads.insert(host.into(), head);
        stores.insert(host.into(), store);
    }
    let host = RemainingHost(FlowHost {
        stores,
        heads,
        sources,
        base: BaseFixture::new(&input),
        now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
            .unwrap(),
    });
    let owner = v["owner"].as_str().unwrap();
    let configured = host.0.stores[owner]
        .provision_adjudication(
            journal(owner),
            flow_budget(owner),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    host.0.stores[owner].test_publication_cut(v["cut"].as_u64().unwrap() as u8);
    let result = run(
        &configured,
        &host,
        journal(owner),
        parsed(&v["command"]),
        deadline(),
    )
    .await;
    panic!("real publication cut did not exit: {result:?}");
}
impl Harness {
    async fn process_commit(&mut self, owner: &str, mut c: Value, cut: u8) {
        hydrate(&mut c, &self.proofs, &self.roots[owner]);
        let mut before = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for (name, (db, anchor)) in ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .zip(&self._dirs)
        {
            before.insert(
                name.to_owned(),
                self.host.0.stores[name].test_full_inventory().await,
            );
            paths.insert(name, json!([db.path(), anchor.path()]));
        }
        let before_count = self.host.0.stores[owner]
            .test_adjudication_stats(&journal(owner))
            .await["segments"];
        for store in std::mem::take(&mut self.host.0.stores).into_values() {
            store.close().await;
        }
        let input = tempfile::tempdir().unwrap();
        let path = input.path().join("cut.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"owner":owner,"command":c,"cut":cut,"paths":paths}))
                .unwrap(),
        )
        .unwrap();
        let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","service::accept::adjudication::sqlite_tests::remaining_tests::protected_tests::finish_process_tests::protected_process_entry","--nocapture"]).env("LEDGERLAB_PROTECTED_PROCESS",&path).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "child cut{cut} stdout:{} stderr:{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for (name, (db, anchor)) in ["center", "g0", "g1", "g2", "g3"]
            .into_iter()
            .zip(&self._dirs)
        {
            self.host.0.stores.insert(
                name.into(),
                SqliteStore::open_fenced(db.path(), anchor.path())
                    .await
                    .unwrap(),
            );
        }
        for name in ["center", "g0", "g1", "g2", "g3"] {
            if cut == 11 || name != owner {
                assert_eq!(
                    self.host.0.stores[name].test_full_inventory().await,
                    before[name],
                    "cut recovery {cut}/{name}"
                );
            }
        }
        let stats = self.host.0.stores[owner]
            .test_adjudication_stats(&journal(owner))
            .await;
        assert_eq!(stats["segments"], before_count + u128::from(cut != 11));
        let recovered = self.host.0.stores[owner].test_full_inventory().await;
        let configured = self.host.0.stores[owner]
            .provision_adjudication(
                journal(owner),
                flow_budget(owner),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        if cut != 11 {
            c["authority"]["permission"] = json!("read");
        }
        let result = run(
            &configured,
            &self.host,
            journal(owner),
            parsed(&c),
            deadline(),
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
        if cut != 11 {
            assert_eq!(
                self.host.0.stores[owner].test_full_inventory().await,
                recovered
            );
        }
        self.accept_observed(owner, c.clone(), result).await;
        assert_eq!(
            self.host.0.stores[owner]
                .test_adjudication_stats(&journal(owner))
                .await["segments"],
            before_count + 1
        );
        self.assert_actual_accounts().await;
        eprintln!("actual child-process {} cut{cut}: recovered {}, exactly one durable segment; retry/inventory/account checks PASS",c["kind"],if cut==11 {"old then committed"}else{"new saved outcome"});
    }
}
#[tokio::test]
async fn actual_protected_close_and_install_process_cuts_finish_exactly_once() {
    for kind in ["CLOSE", "INSTALL"] {
        for cut in [11, 12, 13] {
            let mut h = Harness::new().await;
            let c = h.full_begin_command(1, "FINISH_ONLY", vec![]);
            h.step(c).await;
            h.full_seal_ready(1).await;
            if kind == "CLOSE" {
                let c = h.terminal_command("CLOSE", 1);
                h.process_commit("center", c, cut).await;
                h.full_install_all(1, "COMMITTED").await;
            } else {
                let c = h.terminal_command("CLOSE", 1);
                h.step(c).await;
                let c = h.full_install_command(1, "g1", "COMMITTED");
                h.process_commit("g1", c, cut).await;
                h.full_ack(1, "g1").await;
                for gateway in ["g0", "g2", "g3"] {
                    let c = h.full_install_command(1, gateway, "COMMITTED");
                    h.step(c).await;
                    h.full_ack(1, gateway).await;
                }
            }
            h.reopen().await;
            h.assert_actual_accounts().await;
            h.close().await;
        }
    }
}

impl Harness {
    async fn cut_or_step(&mut self, owner: &str, c: Value, selected: &str, cut: u8) {
        if c["kind"] == selected {
            self.process_commit(owner, c, cut).await;
        } else {
            self.step(c).await;
        }
    }
}
#[tokio::test]
async fn actual_loaded_token_process_cuts_preserve_prepaid_finish() {
    for selected in [
        "ISSUE",
        "ACTIVATE",
        "RETURN_UNUSED",
        "RECONCILE",
        "CLOSE",
        "INSTALL",
    ] {
        for cut in [11, 12, 13] {
            let mut h = Harness::new().await;
            h.quiet = true;
            // First token is consumed and remains unimported at the seal. A
            // second uniquely backed token is issued but never activated.
            for i in 5..10 {
                let c = h.input["commands"][i].clone();
                if i == 8 {
                    h.cut_or_step("g1", c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
            }
            for i in 26..29 {
                let c = h.input["commands"][i].clone();
                if i == 28 {
                    h.cut_or_step("center", c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
            }
            let c = h.full_begin_command(1, "FINISH_ONLY", vec![]);
            h.step(c).await;
            for gateway in ["g0", "g1", "g2", "g3"] {
                let proof = h.proof("center", "BEGIN", json!("1"));
                h.command_step(
                    "SEAL_BEGIN",
                    json!({"gateway":gateway,"round":"1","predecessor":"0","proof":proof}),
                )
                .await;
            }
            let proof = h.proof("center", "CLAIM", json!("token4"));
            let claim = h.input["commands"][28]["payload"]["token"]["claim"].clone();
            let c = h.command(
                "RETURN_UNUSED",
                json!({"gateway":"g1","token":"token4","claim":claim,"proof":proof}),
            );
            h.cut_or_step("g1", c, selected, cut).await;
            h.step(h.input["commands"][10].clone()).await;
            h.step(h.input["commands"][44].clone()).await;
            h.step(h.input["commands"][45].clone()).await;
            let proof = h.proof("g1", "RETURNED_UNUSED", json!("token4"));
            let c = h.command("RECONCILE", json!({"token":"token4","proof":proof}));
            h.cut_or_step("center", c, selected, cut).await;
            h.step(h.input["commands"][51].clone()).await;
            for i in [60, 61, 62] {
                h.step(h.input["commands"][i].clone()).await;
            }
            for gateway in ["g0", "g1", "g2", "g3"] {
                h.command_step("SEALED", json!({"gateway":gateway,"round":"1"}))
                    .await;
                let proof = h.proof(gateway, "SEAL", json!("1"));
                h.command_step(
                    "DRAIN",
                    json!({"gateway":gateway,"round":"1","proof":proof}),
                )
                .await;
            }
            h.ready(1).await;
            let c = h.terminal_command("CLOSE", 1);
            h.cut_or_step("center", c, selected, cut).await;
            for gateway in ["g0", "g1", "g2", "g3"] {
                let c = h.full_install_command(1, gateway, "COMMITTED");
                if gateway == "g1" {
                    h.cut_or_step(gateway, c, selected, cut).await;
                } else {
                    h.step(c).await;
                }
                h.full_ack(1, gateway).await;
            }
            h.reopen().await;
            h.assert_actual_accounts().await;
            for token in ["token1", "token4"] {
                for owner in ["center", "g1"] {
                    let (_, State::Token(t)) = h
                        .state(
                            owner,
                            HeadKind::Token,
                            rt::points::Point::id(
                                rt::points::PointKind::Token,
                                *b"TOKEN___",
                                token,
                            )
                            .unwrap(),
                            GuardClass::GatewayRound,
                        )
                        .await
                    else {
                        panic!("token");
                    };
                    assert_eq!(
                        t.status,
                        if token == "token1" {
                            rt::points::TokenStatus::NewCase
                        } else {
                            rt::points::TokenStatus::ReturnedUnused
                        }
                    );
                    if owner == "center" {
                        assert!(t.reconciled && t.advanced);
                        assert_eq!(t.imported, token == "token1");
                        assert_eq!(t.receipt_advanced, token == "token1");
                    }
                    assert_eq!(t.receipt.is_some(), token == "token1");
                }
            }
            eprintln!("loaded actual {selected} cut{cut}: consumed receipt imported; unactivated token permanently returned; all five families/four gateways finished and reopened");
            h.close().await;
        }
    }
}
