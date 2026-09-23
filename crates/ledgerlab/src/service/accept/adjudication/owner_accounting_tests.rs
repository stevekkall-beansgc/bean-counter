//! Read-only, independent reconciliation of every persisted allocation owner.
//! Raw frozen JSON coefficients are used instead of runtime Worksheet arithmetic.
use super::*;
const RESOURCES: [&str; 6] = [
    "canonical_bytes",
    "trusted_bytes",
    "records",
    "index_pages",
    "index_values",
    "workspace_bytes",
];
fn count(v: &Value) -> u128 {
    v.as_str().unwrap().parse().unwrap()
}
fn decode_blob(quoted: &str) -> Vec<u8> {
    let hex = quoted
        .strip_prefix("X'")
        .unwrap()
        .strip_suffix('\'')
        .unwrap();
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
fn heads(inventory: &[(String, Vec<String>, Vec<String>)]) -> BTreeMap<String, Vec<Value>> {
    let (_, columns, rows) = inventory
        .iter()
        .find(|(name, _, _)| name == "r3_heads")
        .unwrap();
    assert_eq!(
        columns,
        &["journal", "kind", "full_key", "revision", "value"]
    );
    let mut result: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for row in rows {
        // This table contains four BLOBs quoted as hex and one INTEGER. There
        // are no quoted text fields in which a delimiter could occur.
        let parts: Vec<_> = row.split('|').collect();
        assert_eq!(parts.len(), 5);
        let state: Value = serde_json::from_slice(&decode_blob(parts[4])).unwrap();
        result.entry(parts[0].to_owned()).or_default().push(state);
    }
    result
}
fn original_peaks(states: &[Value], raw: &Value, central: bool) -> BTreeMap<String, u128> {
    let mut peaks = BTreeMap::new();
    let mut add = |label: &str, key: Value, bundle: &str| {
        let owner = rt::hash("namespace", &json!([label, key]))
            .unwrap()
            .as_str()
            .to_owned();
        let peak = raw["bundles"][bundle]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slot| {
                let kind = slot.as_str().unwrap();
                let extra = match kind {
                    "SEAL_BEGIN" | "INSTALL" => 3_434_132,
                    "PREPARE_ROUND" => 1_579_812,
                    "ENROLL" => 196_608,
                    "DECIDE" => 272_992,
                    "CORRECT" => 8_192,
                    "CLOSE" => 131_072,
                    _ => 0,
                };
                u128::from(
                    raw["transitions"][kind]["logical_workspace_bytes"]
                        .as_u64()
                        .unwrap(),
                ) + extra
            })
            .max()
            .unwrap();
        peaks.insert(owner, peak);
    };
    for n in 0..32 {
        add(
            "close",
            json!(n),
            if central {
                "finish_central"
            } else {
                "finish_gateway"
            },
        );
    }
    for state in states {
        let body = &state["body"];
        match state["kind"].as_str().unwrap() {
            "Grant" => add(
                if central {
                    "grant-central"
                } else {
                    "grant-local"
                },
                body["grant"]["id"].clone(),
                if central {
                    "central_grant"
                } else {
                    "local_grant"
                },
            ),
            "Token" if central => add("token", body["token"]["id"].clone(), "central_token"),
            "RoundPreparation" => add("optional-round", body["round"].clone(), "cancel_gateway"),
            "Round" => add(
                "optional-round",
                body["begin"]["round"].clone(),
                "cancel_central",
            ),
            _ => {}
        }
    }
    peaks
}
fn reconcile(states: &[Value], raw: &Value, central: bool) -> Result<(usize, usize), String> {
    let accounts: Vec<_> = states.iter().filter(|s| s["kind"] == "Resource").collect();
    if accounts.len() != 1 {
        return Err("one resource account per journal".into());
    }
    let account = &accounts[0]["body"];
    let mut owners = std::collections::BTreeSet::new();
    let mut held = BTreeMap::<&str, u128>::new();
    let mut reserved = BTreeMap::<String, u128>::new();
    let mut slot_count = 0;
    let peaks = original_peaks(states, raw, central);
    for state in states.iter().filter(|s| s["kind"] == "Allocation") {
        let a = &state["body"];
        if !owners.insert(a["owner"].as_str().unwrap()) {
            return Err("duplicate allocation owner".into());
        }
        for name in RESOURCES {
            *held.entry(name).or_default() += count(&a["held"][name]);
        }
        let mut remaining = [0u128; 5];
        for slot in a["slots"].as_array().unwrap() {
            slot_count += 1;
            let kind = slot.as_str().unwrap();
            // Independent literal overlay for the accepted runtime source-cache
            // and bounded economic-head extensions to the frozen worksheet.
            let extra = match kind {
                "SEAL_BEGIN" | "INSTALL" => [353232, 215236, 2, 17844, 142],
                "PREPARE_ROUND" => [0, 0, 1, 8922, 54],
                "ENROLL" => [0, 0, 0, 0, 48],
                "DECIDE" => [0, 0, 0, 0, 3],
                "CORRECT" => [0, 0, 0, 0, 2],
                "CLOSE" => [0, 0, 0, 0, 32],
                _ => [0; 5],
            };
            for (i, field) in [
                "segment_bytes",
                "new_trusted_bytes",
                "records",
                "index_path_pages",
                "index_value_pages",
            ]
            .iter()
            .enumerate()
            {
                remaining[i] = remaining[i]
                    .checked_add(
                        u128::from(raw["transitions"][kind][field].as_u64().unwrap()) + extra[i],
                    )
                    .unwrap();
            }
            let counters = raw["transitions"][kind]["counter_increments"]
                .as_object()
                .unwrap();
            for (name, value) in counters {
                let mut maximum = u128::from(value.as_u64().unwrap());
                // Runtime first-use caches add two independently retained keys
                // for SEAL_BEGIN/INSTALL, one for PREPARE_ROUND. These are the
                // reviewed additive source/cache terms, not fixture rewrites.
                if name == "index_cardinality" {
                    maximum += match kind {
                        "SEAL_BEGIN" | "INSTALL" => 2,
                        "PREPARE_ROUND" => 1,
                        _ => 0,
                    };
                }
                *reserved.entry(name.clone()).or_default() += maximum;
            }
        }
        for (i, name) in RESOURCES[..5].iter().enumerate() {
            if remaining[i] != count(&a["held"][name]) {
                return Err(format!("individual owner remaining resources: {name}"));
            }
        }
        // Match the original bundle peak even after its largest slot has been
        // spent. Released owners remain durable with no remaining slots/hold.
        let workspace = count(&a["held"]["workspace_bytes"]);
        if workspace == 0 {
            if !a["slots"].as_array().unwrap().is_empty() {
                return Err("unfunded live workspace".into());
            }
        } else if peaks.get(a["owner"].as_str().unwrap()).copied() != Some(workspace) {
            return Err("original owner workspace peak".into());
        }
    }
    for name in RESOURCES {
        if held.get(name).copied().unwrap_or(0) != count(&account["held"][name]) {
            return Err(format!("held owner sum: {name}"));
        }
        if count(&account["used"][name]) + count(&account["held"][name])
            > count(&account["provisioned"][name])
        {
            return Err(format!("unfunded resource: {name}"));
        }
    }
    for (name, value) in account["reserved"].as_object().unwrap() {
        if reserved.get(name).copied().unwrap_or(0) != count(value) {
            return Err(format!("remaining slot counter sum: {name}"));
        }
        if count(&account["q"][name]) + count(value) > 999_999_999_999_999_999_999_999_999_999 {
            return Err(format!("counter headroom: {name}"));
        }
    }
    Ok((owners.len(), slot_count))
}
impl Harness {
    pub(super) async fn assert_all_owner_accounts(&self) -> (usize, usize) {
        let raw: Value = serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/protocol/resources.json")).unwrap();
        let mut totals = (0, 0);
        for (name, store) in &self.host.0.stores {
            let inventory = store.test_full_inventory().await;
            let journals = heads(&inventory);
            assert_eq!(journals.len(), 1, "fixture journal count {name}");
            for states in journals.values() {
                let checked = reconcile(states, &raw, name == "center")
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                totals.0 += checked.0;
                totals.1 += checked.1;
            }
        }
        totals
    }
}
#[tokio::test]
async fn actual_customer_every_owner_and_remaining_counter_reconcile_after_each_step() {
    let mut h = Harness::new().await;
    h.quiet = true;
    let mut peak = h.assert_all_owner_accounts().await;
    for i in 5..95 {
        h.step(h.input["commands"][i].clone()).await;
        let observed = h.assert_all_owner_accounts().await;
        peak.0 = peak.0.max(observed.0);
        peak.1 = peak.1.max(observed.1);
    }
    h.reopen().await;
    h.assert_all_owner_accounts().await;
    // Validate the independent checker with corrupted COPIES, never store edits.
    let raw: Value = serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/protocol/resources.json")).unwrap();
    let inventory = h.host.0.stores["center"].test_full_inventory().await;
    let journals = heads(&inventory);
    let states = journals.values().next().unwrap();
    let index = states.iter().position(|s| s["kind"] == "Resource").unwrap();
    let mut negatives = 0;
    for group in ["held", "reserved"] {
        for name in states[index]["body"][group].as_object().unwrap().keys() {
            let mut corrupt = states.clone();
            let value = count(&corrupt[index]["body"][group][name]);
            corrupt[index]["body"][group][name] = json!((value + 1).to_string());
            assert!(reconcile(&corrupt, &raw, true).is_err());
            negatives += 1;
        }
    }
    let allocation = states
        .iter()
        .position(|s| s["kind"] == "Allocation" && count(&s["body"]["held"]["workspace_bytes"]) > 0)
        .unwrap();
    for name in RESOURCES {
        let mut corrupt = states.clone();
        for selected in [index, allocation] {
            let value = count(&corrupt[selected]["body"]["held"][name]);
            corrupt[selected]["body"]["held"][name] = json!((value + 1).to_string());
        }
        // Aggregate still equals the sum of owners. Independent remaining
        // coefficients/original bundle peak must catch this paired corruption.
        assert!(reconcile(&corrupt, &raw, true).is_err());
        negatives += 1;
    }
    assert_eq!(negatives, 28);
    assert_eq!(
        h.host.0.stores["center"].test_full_inventory().await,
        inventory
    );
    h.close().await;
    eprintln!("independent owner reconciliation:91 chronological snapshots of all5hosts plus reopen; peak{}owners/{}remaining slots; all6held+16reserved dimensions;28 corrupted-copy controls rejected", peak.0, peak.1);
}
