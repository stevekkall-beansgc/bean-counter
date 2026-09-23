//! Physical histories stay genuine q0/epoch1 genesis. Near-M arithmetic below is
//! explicitly a pure production-code probe, never a fabricated persisted prefix.
use super::*;
#[test]
fn production_counter_m_minus_exact_plus_for_all_sixteen_worksheet_dimensions() {
    let worksheet = Worksheet::frozen().unwrap();
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../../../contracts/candidates/central-adjudication-r3-candidate1/protocol/schema.json"
    ))
    .unwrap();
    let names = schema["x-counters"].as_array().unwrap();
    assert_eq!(names.len(), 16);
    let counter_names = [
        "segment",
        "head_revision",
        "grant",
        "grant_registry",
        "allocation",
        "receipt",
        "control",
        "round",
        "import",
        "terminal",
        "allocation_prefix",
        "receipt_prefix",
        "index_cardinality",
        "writer_epoch",
        "economic_revision",
        "resource_revision",
    ];
    for (index, name) in counter_names.iter().enumerate() {
        assert!(names.iter().any(|v| v == name));
        let (kind, t) = worksheet
            .transitions
            .iter()
            .find(|(_, t)| t.counter_increments[*name] > 0)
            .unwrap();
        let cost = t.counter_increments[*name] as u128;
        for offset in [-1i128, 0, 1] {
            let mut account = rt::points::ResourceState::genesis(
                wire::Resource::from_dimensions([Count::new(Count::MAX).unwrap(); 6]),
                Count::ZERO,
            );
            let mut q = [Count::ZERO; 16];
            q[index] = Count::new((Count::MAX - cost).checked_add_signed(offset).unwrap()).unwrap();
            account.q = wire::Counters::from_dimensions(q);
            let before = account.clone();
            let slots = vec![kind.clone()];
            let result = account.reserve("pure-boundary".into(), &slots, &worksheet);
            if offset == 1 {
                assert!(result.is_err());
                assert_eq!(account, before);
            } else {
                let mut owner = result.unwrap();
                account
                    .spend(&mut owner, kind, &t.counters().unwrap(), &worksheet)
                    .unwrap();
                account.terminal_slack(&mut owner, &worksheet).unwrap();
                assert_eq!(
                    account.q.dimensions()[index].value(),
                    Count::MAX.checked_add_signed(offset).unwrap()
                );
                assert_eq!(account.reserved, wire::Counters::zero());
                account.validate().unwrap();
            }
        }
        let mut c = r3::resources::CounterCredit::new(
            Count::new(Count::MAX - 1).unwrap(),
            Count::new(1).unwrap(),
        )
        .unwrap();
        let before = c.clone();
        assert!(c.reserve(Count::new(1).unwrap()).is_err());
        assert_eq!(c, before);
        assert!(c
            .spend(Count::new(1).unwrap(), Count::new(2).unwrap())
            .is_err());
        assert_eq!(c, before);
        c.spend(Count::new(1).unwrap(), Count::new(1).unwrap())
            .unwrap();
        assert_eq!(c.consumed.value(), Count::MAX);
        assert_eq!(c.held, Count::ZERO);
        eprintln!("pure production counter boundary {name}: M-1/M/M+1 and refused-reserve/spend atomicity PASS");
    }
    assert_eq!(
        Count::new(Count::MAX + 1).unwrap_err().code,
        "COUNTER_EXHAUSTED"
    );
}
#[tokio::test]
async fn actual_counter_boundary_inputs_preserve_genuine_history_and_prepaid_finish() {
    let mut h = Harness::new().await;
    h.assert_actual_accounts().await;
    for value in [Count::MAX - 1, Count::MAX] {
        let mut c = h.full_begin_command(1, "FINISH_ONLY", vec![]);
        c["payload"]["round"] = json!(value.to_string());
        let before = h.host.0.stores["center"].test_full_inventory().await;
        h.execute(c, Some("ROUND_PREDECESSOR")).await;
        assert_eq!(
            h.host.0.stores["center"].test_full_inventory().await,
            before
        );
        let mut delta = wire::Resource::zero();
        delta.canonical_bytes = Count::new(value).unwrap();
        let c = h.command("EXTEND_RESOURCES", json!({"host":"g1","resources":delta}));
        let before = h.host.0.stores["g1"].test_full_inventory().await;
        h.execute(c, Some("COUNTER_EXHAUSTED")).await;
        assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, before);
    }
    let mut too_large = h.full_begin_command(1, "FINISH_ONLY", vec![]);
    too_large["payload"]["round"] = json!((Count::MAX + 1).to_string());
    let before = h.host.0.stores["center"].test_full_inventory().await;
    assert!(
        ParsedCommand::parse(&r3::canonical_bytes(&too_large, r3::COMMAND_BYTES).unwrap()).is_err()
    );
    assert_eq!(
        h.host.0.stores["center"].test_full_inventory().await,
        before
    );
    h.step(h.input["commands"][5].clone()).await;
    h.step(h.input["commands"][6].clone()).await;
    let mut issue = h.input["commands"][7].clone();
    issue["payload"]["token"]["allocation"] = json!(Count::MAX.to_string());
    let t = &issue["payload"]["token"];
    let claim = rt::hash(
        "claim",
        &json!([
            t["grant"],
            t["id"],
            t["gateway"],
            t["allocation"],
            t["category"]
        ]),
    )
    .unwrap();
    issue["payload"]["token"]["claim"] = json!(claim);
    let before = h.host.0.stores["center"].test_full_inventory().await;
    h.execute(issue, Some("ALLOCATION_GAP")).await;
    assert_eq!(
        h.host.0.stores["center"].test_full_inventory().await,
        before
    );
    h.assert_actual_accounts().await;
    let c = h.full_begin_command(1, "FINISH_ONLY", vec![]);
    h.step(c).await;
    h.full_seal_ready(1).await;
    let c = h.terminal_command("CLOSE", 1);
    h.step(c).await;
    h.full_install_all(1, "COMMITTED").await;
    h.reopen().await;
    h.assert_actual_accounts().await;
    eprintln!("actual counter boundary inputs refused without growth; genuine q/R state and all-family four-gateway FINISH retained");
    h.close().await;
}
