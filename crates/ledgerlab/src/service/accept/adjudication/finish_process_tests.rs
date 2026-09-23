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
