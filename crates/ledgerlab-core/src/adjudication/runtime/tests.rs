use super::{accounting::Worksheet, points::*, transition::*};
use crate::adjudication::{
    commands as w,
    proofs::decode_base64,
    types::{Count, Digest, Id},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn id(s: &str) -> Id {
    Id::parse(s).unwrap()
}
fn b64(raw: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in raw.chunks(3) {
        s.push(A[(c[0] >> 2) as usize] as char);
        s.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        s.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}
#[test]
fn frozen_first_intake_uses_separate_host_points_and_paid_slots() {
    let trace:Value=serde_json::from_str(include_str!("../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap();
    run_trace(&trace, 11, &[]);
}
fn run_trace(trace: &Value, limit: usize, refusals: &[(usize, &str)]) {
    let enroll = trace["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "ENROLL")
        .unwrap()["payload"]
        .clone();
    let center = enroll["store"].as_str().unwrap();
    let registration = enroll["registration"].as_str().unwrap();
    // This in-memory map is a TEST store only. Production resolves bounded point
    // requests and never loads a journal's lifetime state into the coordinator.
    let mut stores: BTreeMap<String, BTreeMap<Point, (Count, State)>> = BTreeMap::new();
    let mut ordinals: BTreeMap<String, u128> = BTreeMap::new();
    let mut facts: Vec<(w::FactKind, Id, Value, Vec<w::Effect>)> = Vec::new();
    for (host, resource) in trace["initial"]["initial_resources"].as_object().unwrap() {
        let mut resource: w::Resource = serde_json::from_value(resource.clone()).unwrap();
        // Runtime independently acquires enrollment terms at a previously idle
        // gateway; frozen model input budgets intentionally remain untouched.
        // This TEST provision prices the published runtime envelope addition.
        if host != center {
            let extra = Worksheet::frozen()
                .unwrap()
                .enrollment_cache_augmentation()
                .unwrap();
            let dims = resource.dimensions();
            resource = w::Resource::from_dimensions(std::array::from_fn(|n| {
                Count::new(
                    dims[n]
                        .value()
                        .saturating_add(u128::from(extra[n]) * 64)
                        .min(Count::MAX),
                )
                .unwrap()
            }));
        }
        stores.entry(host.clone()).or_default().insert(
            Point::id(PointKind::Resource, *b"RESOURCE", host).unwrap(),
            (
                Count::new(1).unwrap(),
                State::Resource(Box::new(ResourceState::genesis(
                    resource,
                    Count::new(u128::from(host != center)).unwrap(),
                ))),
            ),
        );
    }
    'commands: for (position, raw) in trace["commands"].as_array().unwrap()[..limit]
        .iter()
        .enumerate()
    {
        let command: w::Command = serde_json::from_value(raw.clone()).unwrap();
        let kind = raw["kind"].as_str().unwrap();
        let host = match kind {
            "PREPARE_ENROLL" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED" | "LOCAL_TERMINAL"
            | "SEAL_BEGIN" | "SEALED" | "INSTALL" => raw["payload"]["gateway"].as_str().unwrap(),
            "LOCAL_GRANT" => raw["payload"]["grant"]["gateway"].as_str().unwrap(),
            _ => center,
        };
        let mut proofs = Vec::new();
        if let Some(p) = raw["payload"].get("proof") {
            proofs.push(p.clone());
        }
        if let Some(p) = raw["payload"].get("preparations") {
            proofs.extend(p.as_array().unwrap().clone());
        }
        if let Some(p) = raw["payload"].get("begin") {
            proofs.push(p.clone());
        }
        let mut sources: Vec<_> = proofs
            .into_iter()
            .map(|p| {
                let proof: w::Proof = serde_json::from_value(p).unwrap();
                let entry = facts
                    .iter()
                    .find(|(kind, key, _, _)| {
                        *kind == proof.fact_kind
                            && serde_json::to_value(key).unwrap()
                                == serde_json::to_value(&proof.full_key).unwrap()
                    })
                    .unwrap();
                let bytes = crate::adjudication::canonical_bytes(
                    &json!({"payload":entry.2,"effects":entry.3}),
                    262144,
                )
                .unwrap();
                assert_eq!(crate::adjudication::raw_sha256(&bytes), proof.body_hash);
                SourceFact {
                    object: w::RetainedObject {
                        origin: w::ObjectOrigin {
                            store: proof.store.clone(),
                            scope: proof.scope.clone(),
                            registration: proof.registration.clone(),
                            host: proof.host.clone(),
                            ordinal: proof.ordinal,
                        },
                        kind: proof.fact_kind.clone(),
                        full_key: serde_json::from_value(
                            serde_json::to_value(&proof.full_key).unwrap(),
                        )
                        .unwrap(),
                        body: b64(&bytes),
                        body_hash: proof.body_hash.clone(),
                        bytes: proof.bytes,
                    },
                    proof,
                }
            })
            .collect();
        // Input SourceFact is a pure-engine witness, not a production source
        // factory. An actual backend must authenticate its own source prefix.
        if host != center
            && kind != "PREPARE_ENROLL"
            && sources
                .iter()
                .all(|s| s.object.kind != w::FactKind::Enrollment)
        {
            let proof_template = trace["commands"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|c| c["payload"].get("proof"))
                .find(|p| p["fact_kind"] == "ENROLLMENT");
            if let Some(p) = proof_template {
                let proof: w::Proof = serde_json::from_value(p.clone()).unwrap();
                let bytes = crate::adjudication::canonical_bytes(
                    &json!({"payload":enroll,"effects":[]}),
                    262144,
                )
                .unwrap();
                sources.push(SourceFact {
                    object: w::RetainedObject {
                        origin: w::ObjectOrigin {
                            store: proof.store.clone(),
                            scope: proof.scope.clone(),
                            registration: proof.registration.clone(),
                            host: proof.host.clone(),
                            ordinal: proof.ordinal,
                        },
                        kind: w::FactKind::Enrollment,
                        full_key: serde_json::from_value(
                            serde_json::to_value(&proof.full_key).unwrap(),
                        )
                        .unwrap(),
                        body: b64(&bytes),
                        body_hash: crate::adjudication::raw_sha256(&bytes),
                        bytes: Count::new(bytes.len() as u128).unwrap(),
                    },
                    proof,
                });
            }
        }
        let state = stores.get_mut(host).unwrap();
        let seal = if kind == "SEALED" {
            let n = Count::parse(raw["payload"]["round"].as_str().unwrap()).unwrap();
            let r = state
                .values()
                .find_map(|(_, s)| match s {
                    State::Round(r) if r.begin.round == n => Some(r),
                    _ => None,
                })
                .unwrap();
            let gw = state
                .values()
                .find_map(|(_, s)| match s {
                    State::Gateway(g) => Some(g),
                    _ => None,
                })
                .unwrap();
            let cutoff = r
                .gateways
                .iter()
                .find(|g| g.gateway.as_str() == host)
                .unwrap()
                .cutoff;
            let mut fold = super::seal::SealFold::new(id(host), n, cutoff, gw.receipt);
            while let Some(position) = fold.next_allocation() {
                let State::Position(t) = &state
                    .get(&Point::position(PointKind::Allocation, &id(host), position).unwrap())
                    .unwrap()
                    .1
                else {
                    panic!()
                };
                let State::Token(t) = &state
                    .get(&Point::id(PointKind::Token, *b"TOKEN___", t.as_str()).unwrap())
                    .unwrap()
                    .1
                else {
                    panic!()
                };
                fold.disposition(t).unwrap();
            }
            while let Some(position) = fold.next_receipt() {
                let State::Position(t) = &state
                    .get(&Point::position(PointKind::Receipt, &id(host), position).unwrap())
                    .unwrap()
                    .1
                else {
                    panic!()
                };
                let State::Token(t) = &state
                    .get(&Point::id(PointKind::Token, *b"TOKEN___", t.as_str()).unwrap())
                    .unwrap()
                    .1
                else {
                    panic!()
                };
                fold.receipt(t.receipt.as_ref().unwrap()).unwrap();
            }
            Some(fold.finish().unwrap())
        } else {
            None
        };
        let mut observations = Vec::new();
        let next = *ordinals.get(host).unwrap_or(&0) + 1;
        let prior = Digest::parse(raw["authority"]["head"].as_str().unwrap()).unwrap();
        let h = id(host);
        let store = id(center);
        let reg = id(registration);
        let delta = loop {
            assert!(observations.len() <= 256);
            let input = Input {
                command: &command,
                store: &store,
                registration: &reg,
                host: &h,
                prior_root: &prior,
                next_ordinal: Count::new(next).unwrap(),
                writer_epoch: Count::new(u128::from(host != center)).unwrap(),
                observations: &observations,
                sources: &sources,
                introduced_objects: 0,
                seal: seal.as_ref(),
            };
            let result = step(&input);
            if let Err(e) = &result {
                if let Some((_, expected)) = refusals.iter().find(|(n, _)| *n == position) {
                    assert_eq!(e.code, *expected);
                    continue 'commands;
                }
            }
            match result.unwrap_or_else(|e| panic!("command {position} {kind}: {e:?}")) {
                Step::Need(points) => {
                    for p in points {
                        let found = state.get(&p);
                        observations.push(Observation {
                            point: p,
                            revision: found.map(|x| x.0),
                            state: found.map(|x| x.1.clone()),
                        });
                    }
                }
                Step::Ready(d) => break d,
            }
        };
        if let Some((kind, key)) = &delta.fact {
            facts.push((
                kind.clone(),
                key.clone(),
                raw["payload"].clone(),
                delta.effects.clone(),
            ));
        }
        if kind == "CLOSE" {
            assert!(delta
                .mutations
                .iter()
                .all(|m| m.point.kind != PointKind::Case));
        }
        for m in delta.mutations {
            state.insert(m.point, (m.revision, m.state));
        }
        ordinals.insert(host.into(), next);
        let mut held = w::Resource::zero();
        let mut credits = w::Counters::zero();
        let worksheet = Worksheet::frozen().unwrap();
        for (_, s) in state.values() {
            if let State::Allocation(a) = s {
                held = held.checked_add(&a.held).unwrap();
                for slot in &a.slots {
                    credits = credits
                        .checked_add(&worksheet.template(slot).unwrap().counters().unwrap())
                        .unwrap();
                }
            }
        }
        let State::Resource(resources) = &state
            .get(&Point::id(PointKind::Resource, *b"RESOURCE", host).unwrap())
            .unwrap()
            .1
        else {
            panic!()
        };
        assert_eq!(held, resources.held);
        assert_eq!(credits, resources.reserved);
        resources.validate().unwrap();
    }
    if limit == 11 {
        let central = stores.get(center).unwrap();
        let cases: Vec<_> = central
            .values()
            .filter_map(|(_, s)| {
                if let State::Case(c) = s {
                    Some(c)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].status, CaseStatus::OrdinaryPending);
        assert_eq!(cases[0].signed.value(), 0);
    }
    let ws = Worksheet::frozen().unwrap();
    assert!(ws
        .bundle("central_token")
        .unwrap()
        .iter()
        .any(|s| s == "ADVANCE_RECEIPT"));
    for source in trace["initial"]["authority_sources"].as_array().unwrap() {
        let raw = decode_base64(source["body"].as_str().unwrap(), 16384).unwrap();
        assert!(!raw.is_empty());
    }
}

#[test]
fn frozen_returned_unused_finish_matches_exact_source_bodies() {
    let trace:Value=serde_json::from_str(include_str!("../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/seal-scan.json")).unwrap();
    run_trace(&trace, trace["commands"].as_array().unwrap().len(), &[]);
}
#[test]
fn frozen_high_water_finish_preserves_actual_receipt_fence() {
    let trace:Value=serde_json::from_str(include_str!("../../../../../contracts/candidates/central-adjudication-r3-candidate1/vectors/high-water.json")).unwrap();
    run_trace(
        &trace,
        trace["commands"].as_array().unwrap().len(),
        &[(10, "UNRECONCILED_FENCE")],
    );
}
