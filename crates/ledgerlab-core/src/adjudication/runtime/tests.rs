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
    // This in-memory map is a TEST store only. Production resolves bounded point
    // requests and never loads a journal's lifetime state into the coordinator.
    let mut stores: BTreeMap<String, BTreeMap<Point, (Count, State)>> = BTreeMap::new();
    let mut ordinals: BTreeMap<String, u128> = BTreeMap::new();
    let mut facts: Vec<(w::FactKind, Id, Value, Vec<w::Effect>)> = Vec::new();
    for (host, resource) in trace["initial"]["initial_resources"].as_object().unwrap() {
        let resource = serde_json::from_value(resource.clone()).unwrap();
        stores.entry(host.clone()).or_default().insert(
            Point::id(PointKind::Resource, *b"RESOURCE", host).unwrap(),
            (
                Count::new(1).unwrap(),
                State::Resource(Box::new(ResourceState::genesis(
                    resource,
                    Count::new(u128::from(host != "center")).unwrap(),
                ))),
            ),
        );
    }
    for (position, raw) in trace["commands"].as_array().unwrap()[..11]
        .iter()
        .enumerate()
    {
        let command: w::Command = serde_json::from_value(raw.clone()).unwrap();
        let kind = raw["kind"].as_str().unwrap();
        let host = match kind {
            "PREPARE_ENROLL" | "ACTIVATE" | "RECEIVE" => {
                raw["payload"]["gateway"].as_str().unwrap()
            }
            "LOCAL_GRANT" => raw["payload"]["grant"]["gateway"].as_str().unwrap(),
            _ => "center",
        };
        let mut proofs = Vec::new();
        if let Some(p) = raw["payload"].get("proof") {
            proofs.push(p.clone());
        }
        if let Some(p) = raw["payload"].get("preparations") {
            proofs.extend(p.as_array().unwrap().clone());
        }
        let sources: Vec<_> = proofs
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
        let state = stores.get_mut(host).unwrap();
        let mut observations = Vec::new();
        let next = *ordinals.get(host).unwrap_or(&0) + 1;
        let prior = Digest::parse(raw["authority"]["head"].as_str().unwrap()).unwrap();
        let h = id(host);
        let store = id("center");
        let reg = id("registration");
        let delta = loop {
            assert!(observations.len() <= 256);
            let input = Input {
                command: &command,
                store: &store,
                registration: &reg,
                host: &h,
                prior_root: &prior,
                next_ordinal: Count::new(next).unwrap(),
                writer_epoch: Count::new(u128::from(host != "center")).unwrap(),
                observations: &observations,
                sources: &sources,
                introduced_objects: 0,
            };
            match step(&input).unwrap_or_else(|e| panic!("command {position} {kind}: {e:?}")) {
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
        for m in delta.mutations {
            state.insert(m.point, (m.revision, m.state));
        }
        ordinals.insert(host.into(), next);
        let mut held = w::Resource::zero();
        for (_, s) in state.values() {
            if let State::Allocation(a) = s {
                held = held.checked_add(&a.held).unwrap();
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
        resources.validate().unwrap();
    }
    let central = stores.get("center").unwrap();
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
