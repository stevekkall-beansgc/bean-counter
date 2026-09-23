//! Actual-store scale scenarios retain the frozen schedules while using the real
//! original-29 customer enrollment and four-gateway routing. No fixture is edited.
use super::*;

fn budgets(n: usize) -> BTreeMap<String, wire::Resource> {
    let ws = Worksheet::frozen().unwrap();
    ["center", "g0", "g1", "g2", "g3"]
        .into_iter()
        .map(|host| {
            let mut total = wire::Resource::zero();
            let mut add = |slots: &[String], count: usize| {
                let mut cost = wire::Resource::zero();
                for kind in slots {
                    let mut r = ws.template(kind).unwrap().resources().unwrap();
                    let peak = r.workspace_bytes;
                    r.workspace_bytes = Count::ZERO;
                    cost = cost.checked_add(&r).unwrap();
                    cost.workspace_bytes = cost.workspace_bytes.max(peak);
                }
                for _ in 0..count {
                    total = total.checked_add(&cost).unwrap();
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
                ws.bundle(if host == "center" {
                    "finish_central"
                } else {
                    "finish_gateway"
                })
                .unwrap(),
                32,
            );
            if host == "center" {
                add(ws.bundle("central_grant").unwrap(), n);
                add(ws.bundle("central_token").unwrap(), n);
            } else if host == "g1" {
                add(ws.bundle("local_grant").unwrap(), n);
            }
            (host.to_owned(), total)
        })
        .collect()
}
fn vector(name: &str) -> Value {
    let raw = match name {
        "pending255" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/pending255.json"),
        "pending256" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/pending256.json"),
        "pending257" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/pending257.json"),
        "pending1025" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/pending1025.json"),
        "mixed257" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/mixed257.json"),
        "all-unused257" => include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/all-unused257.json"),
        _ => panic!("scale vector"),
    };
    serde_json::from_str(raw).unwrap()
}
fn remap(v: &mut Value, namespace: &Value, family: &Value) {
    if *v == json!(["demo", "sandbox"]) {
        *v = json!(["synthetic", "sandbox"]);
        return;
    }
    if v.as_array()
        .is_some_and(|a| a.len() == 4 && a[2] == "resolution")
    {
        *v = family.clone();
        return;
    }
    match v {
        Value::String(s) if s == "g0" => *s = "g1".into(),
        Value::String(s) if s.contains("11111111111111111111111111111111") => {
            *s = s.replace(
                "11111111111111111111111111111111",
                namespace["tag"].as_str().unwrap(),
            )
        }
        Value::Array(a) => {
            for x in a {
                remap(x, namespace, family);
            }
        }
        Value::Object(m) => {
            for x in m.values_mut() {
                remap(x, namespace, family);
            }
        }
        _ => {}
    }
}
fn routed_case(original: &str, family: &Value) -> Value {
    for suffix in 0..10000 {
        let case = json!([family, "source", format!("scale-{original}-{suffix}")]);
        let hash = rt::hash("route", &case).unwrap();
        if u8::from_str_radix(&hash.as_str()[62..], 16).unwrap() % 4 == 1 {
            return case;
        }
    }
    panic!("route search")
}
async fn scenario(name: &str, n: usize, smoke: bool) -> BTreeMap<String, u128> {
    scenario_with_comparison(name, n, smoke, false).await
}
async fn scenario_with_comparison(
    name: &str,
    n: usize,
    smoke: bool,
    comparison: bool,
) -> BTreeMap<String, u128> {
    let input = vector(name);
    let mut commands: Vec<Value> = input["commands"].as_array().unwrap()[2..].to_vec();
    if smoke {
        // Same command pattern, smaller admitted prefix; only a quick plumbing
        // check. Required exact boundary tests below never use this branch.
        commands = commands[..8 * n]
            .iter()
            .cloned()
            .chain(commands[8 * 255..8 * 255 + n].iter().cloned())
            .chain(commands[9 * 255..9 * 255 + n].iter().cloned())
            .chain(commands[10 * 255..].iter().cloned())
            .collect();
    }
    let mut h = Harness::with_budgets(budgets(n), false, true).await;
    let namespace = h.input["commands"][5]["payload"]["grant"]["namespace"].clone();
    let family = h.enrollment["families"][0]["key"].clone();
    let mut claims = BTreeMap::new();
    let mut close_delta = BTreeMap::new();
    let mut local_grants = 0;
    let mut first_grant = None;
    for (index, original) in commands.iter().enumerate() {
        let kind = original["kind"].as_str().unwrap();
        let mut payload = original["payload"].clone();
        remap(&mut payload, &namespace, &family);
        if kind == "LOCAL_GRANT" || kind == "REGISTER_GRANT" {
            payload["grant"]["namespace"] = namespace.clone();
        }
        if kind == "ISSUE" {
            let t = &payload["token"];
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
            claims.insert(t["id"].as_str().unwrap().to_owned(), json!(claim));
            payload["token"]["claim"] = json!(claim);
        }
        if kind == "RETURN_UNUSED" {
            payload["claim"] = claims[payload["token"].as_str().unwrap()].clone();
        }
        if kind == "RECEIVE" {
            payload["submission"]["case"] = routed_case(
                original["payload"]["submission"]["case"][2]
                    .as_str()
                    .unwrap(),
                &family,
            );
        }
        let mut c = h.command(kind, payload);
        if kind == "RECEIVE" {
            c["key"] = c["payload"]["delivery"].clone();
        }
        if kind == "LOCAL_GRANT" {
            local_grants += 1;
            first_grant.get_or_insert_with(|| c.clone());
        }
        let before = if kind == "CLOSE" {
            Some(
                h.host.0.stores["center"]
                    .test_adjudication_stats(&journal("center"))
                    .await,
            )
        } else {
            None
        };
        // The frozen mixed schedule deliberately tries alias import before the
        // original receipt's central mapping exists; its refusal is preserved.
        if name == "mixed257" && index == 775 {
            h.execute(c, Some("ORIGINAL_NOT_IMPORTED")).await;
        } else {
            h.step(c).await;
        }
        if let Some(before) = before {
            let after = h.host.0.stores["center"]
                .test_adjudication_stats(&journal("center"))
                .await;
            for key in [
                "segments",
                "objects",
                "segment_pages",
                "object_pages",
                "heads",
                "head_versions",
                "case_heads",
                "case_head_versions",
            ] {
                close_delta.insert(key.into(), after[key] - before[key]);
            }
            assert_eq!(close_delta["segments"], 1);
            assert_eq!(close_delta["case_heads"], 0);
            assert_eq!(close_delta["case_head_versions"], 0);
            let expected = if name.starts_with("pending") {
                n
            } else if name == "mixed257" {
                1
            } else {
                0
            };
            assert_eq!(after["case_heads"], expected as u128);
            assert_eq!(after["writer_maximum_pages"], after["maximum_pages"]);
            assert!(after["page_count"] <= after["maximum_pages"]);
            assert!(after["wal_bytes"] <= after["maximum_wal_bytes"]);
            eprintln!("scale {name} CLOSE {close_delta:?} retained {after:?}");
        }
        if (name == "mixed257" || name == "all-unused257")
            && local_grants == n
            && kind == "ISSUE"
            && index == 3 * n - 1
        {
            let mut extra = first_grant.clone().unwrap();
            extra["key"][2] = json!("scale-exhausted-extra");
            extra["payload"]["grant"]["id"] =
                json!(format!("gr1.{}.extra", namespace["tag"].as_str().unwrap()));
            let prior = h.roots["g1"].clone();
            let inventory = h.host.0.stores["g1"].test_full_inventory().await;
            h.execute(extra, Some("UNFUNDED")).await;
            assert_eq!(h.host.0.stores["g1"].test_full_inventory().await, inventory);
            assert_eq!(h.roots["g1"], prior);
            eprintln!("scale {name} all {n} grants prebacked; optional extra refused before mandatory completion");
        }
        if index % 256 == 0 {
            eprintln!("scale {name} progress {}/{}", index + 1, commands.len());
        }
    }
    h.reopen().await;
    historical_read(&h, &family, n).await;
    if comparison {
        let before = compare_large(&h).await;
        h.reopen().await;
        assert_eq!(
            h.host.0.stores["center"].test_full_inventory().await,
            before
        );
        eprintln!("large comparison post-report authoritative reopen inventory unchanged");
    }
    eprintln!(
        "scale {name} DONE ordinals {:?} roots {:?}",
        h.ordinals, h.roots
    );
    h.close().await;
    close_delta
}
#[tokio::test]
async fn actual_scale_smoke() {
    scenario("pending255", 2, true).await;
}
#[tokio::test]
async fn actual_pending_boundaries_keep_close_constant() {
    let mut previous = None;
    for n in [255, 256, 257, 1025] {
        let delta = scenario(&format!("pending{n}"), n, false).await;
        if let Some(prior) = previous {
            assert_eq!(
                delta, prior,
                "CLOSE retained work must not scale with pending cases"
            );
        }
        previous = Some(delta);
    }
}
#[tokio::test]
async fn actual_mixed_257_prebacked_tokens_finish_after_optional_exhaustion() {
    scenario("mixed257", 257, false).await;
}
#[tokio::test]
async fn actual_all_unused_257_prebacked_tokens_finish_after_optional_exhaustion() {
    scenario("all-unused257", 257, false).await;
}

async fn historical_read(h: &Harness, family: &Value, n: usize) {
    use crate::store::adjudication::{
        AdjudicationReadStore, AdjudicationReadTx, IndexedPageRequest, SnapshotSelection,
    };
    let store = &h.host.0.stores["center"];
    let budget = wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).unwrap(),
        pages: Count::new(1 << 24).unwrap(),
        segments: Count::new(4096).unwrap(),
    };
    let mut read = store
        .begin_adjudication_read(
            &SnapshotSelection {
                journal: journal("center"),
                historical: None,
            },
            &budget,
            deadline(),
        )
        .await
        .unwrap();
    let current = read.expected_prefix().expected().clone();
    assert_eq!(current.ordinal.value(), h.ordinals["center"]);
    if n >= 255 {
        assert!(
            current.ordinal.value() > 1024,
            "actual retained history crosses old 1024 boundary"
        );
    }
    let family: wire::Family = serde_json::from_value(family.clone()).unwrap();
    let cert = read
        .family_certificate(&family, &current)
        .await
        .unwrap()
        .unwrap();
    assert!(cert.unavailable.contains(&family));
    read.finish().await.unwrap();
    // Source export came from the actual primary when token1 was issued. Select
    // that immutable historical prefix directly, without loading its suffix.
    let proof: wire::Proof =
        serde_json::from_value(h.proof("center", "CLAIM", json!("token1"))).unwrap();
    let mut old = current.clone();
    old.ordinal = proof.ordinal;
    old.segment = proof.segment.clone();
    old.root = proof.root;
    let mut read = store
        .begin_adjudication_read(
            &SnapshotSelection {
                journal: journal("center"),
                historical: Some(old.clone()),
            },
            &budget,
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(read.expected_prefix().expected(), &old);
    assert!(read
        .family_certificate(&family, &old)
        .await
        .unwrap()
        .is_none());
    let mut bytes = Vec::new();
    loop {
        let page = read
            .segment_page(&IndexedPageRequest {
                address: r3::reads::PageAddress {
                    segment: old.segment.clone(),
                    page: Count::new((bytes.len() / 4096) as u128).unwrap(),
                },
                offset: 0,
                max_bytes: 4096,
            })
            .await
            .unwrap();
        bytes.extend(page.bytes);
        if bytes.len() as u128 == page.total_bytes.value() {
            break;
        }
    }
    let segment: wire::Segment = r3::parse_exact(&bytes, r3::SEGMENT_BYTES).unwrap();
    assert_eq!(rt::hash("segment", &segment).unwrap(), old.segment);
    assert_eq!(segment.result.root, old.root);
    assert_eq!(segment.ordinal, old.ordinal);
    assert!(read
        .segment_page(&IndexedPageRequest {
            address: r3::reads::PageAddress {
                segment: current.segment,
                page: Count::ZERO
            },
            offset: 0,
            max_bytes: 4096
        })
        .await
        .is_err());
    drop(read);
    eprintln!(
        "scale bounded historical prefix {} selected from {} retained segments",
        old.ordinal.value(),
        current.ordinal.value()
    );
}

#[tokio::test]
async fn actual_stored_comparison_crosses_1024_segments() {
    scenario_with_comparison("pending255", 255, false, true).await;
}
async fn compare_large(h: &Harness) -> Vec<(String, Vec<String>, Vec<String>)> {
    use crate::store::adjudication::{
        AdjudicationReadStore, AdjudicationReadTx, SnapshotSelection,
    };
    let store = &h.host.0.stores["center"];
    let budget = wire::ReadBudget {
        bytes: Count::new(16 * 1024 * 1024).unwrap(),
        pages: Count::new(1 << 24).unwrap(),
        segments: Count::new(1).unwrap(),
    };
    let read = store
        .begin_adjudication_read(
            &SnapshotSelection {
                journal: journal("center"),
                historical: None,
            },
            &budget,
            deadline(),
        )
        .await
        .unwrap();
    let expected = read.expected_prefix().expected().clone();
    read.finish().await.unwrap();
    assert!(expected.ordinal.value() > 1024);
    let before = store.test_full_inventory().await;
    let coverage: Vec<_> = h.enrollment["gateways"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| wire::Coverage::UnknownGatewayCoverage {
            gateway: serde_json::from_value(g["gateway"].clone()).unwrap(),
        })
        .collect();
    for amount in [1200, 1500] {
        let mut request = wire::ComparisonRequest {
            expected: expected.clone(),
            policy: wire::ComparisonPolicy {
                resolution_atoms: Count::new(amount).unwrap(),
            },
            budget: budget.clone(),
            coverage: coverage.clone(),
            cursor: None,
        };
        let mut comparison = SqliteComparison::from_store(store, &request, &budget)
            .await
            .unwrap();
        let mut segments = 0;
        let mut calls = 0;
        loop {
            calls += 1;
            match comparison
                .advance(&request)
                .await
                .unwrap_or_else(|e| panic!("large report policy{amount} call{calls}: {e}"))
            {
                wire::ComparisonResponse::Incomplete {
                    cursor, measured, ..
                } => {
                    segments += measured.segments.value();
                    request.cursor = Some(*cursor);
                }
                wire::ComparisonResponse::Comparable {
                    actual,
                    alternative,
                    difference,
                    supplier_booked,
                    coverage: reported,
                    measured,
                    ..
                } => {
                    segments += measured.segments.value();
                    // These histories contain admissions and closure, no economic
                    // ALLOW. Changing the award amount cannot invent an award.
                    assert_eq!(
                        (
                            actual.value(),
                            alternative.value(),
                            difference.value(),
                            supplier_booked.value()
                        ),
                        (10000, 10000, 0, 3000)
                    );
                    assert_eq!(reported, coverage);
                    assert_eq!(segments, expected.ordinal.value());
                    assert_eq!(calls, expected.ordinal.value());
                    break;
                }
                other => panic!("large stored report {other:?}"),
            }
            if calls % 256 == 0 {
                eprintln!(
                    "actual comparison policy{amount} validated{calls}/{} central segments",
                    expected.ordinal.value()
                );
            }
        }
        drop(comparison);
        assert_eq!(store.test_full_inventory().await, before);
        eprintln!("actual comparison policy{amount} COMPLETE{segments} segments/{calls} calls; actual10000 alternative10000 supplier3000; explicit unknown offline coverage; inventory unchanged");
    }
    before
}
