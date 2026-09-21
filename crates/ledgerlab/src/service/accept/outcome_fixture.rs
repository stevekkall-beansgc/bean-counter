//! Synthetic, coordinator-owned integration fixture. Every plan runs the same
//! complete decoder, pure evaluator, guards and projector as production build.
//! Synthetic evidence certifies no real-world assent or external authority.
use super::*;
#[derive(Clone)]
pub(crate) struct Fixture {
    pub provisioned_heads: ObservedOutcomeHeads,
    pub provisioned_records: Vec<Vec<u8>>,
    pub plans: Vec<ValidatedOutcomePlan>,
    pub commands: Vec<OutcomeCommand>,
    pub proofs: Vec<AuthorityProof>,
}
pub(crate) fn lifecycle() -> Fixture {
    let h = seed::fresh_history();
    from_history(&h)
}
fn from_history(h: &Value) -> Fixture {
    let seed = h["seed"].as_array().unwrap();
    let raw = seed.iter().map(|r| bytes(r).unwrap()).collect::<Vec<_>>();
    let records = Records::new(&raw, json!(["synthetic", "sandbox"])).unwrap();
    let base = b::decode_base(
        &records,
        &reference(records.one("base-acceptance").unwrap()),
    )
    .unwrap();
    let docs = seed
        .iter()
        .filter(|r| r["kind"] == "evidence")
        .cloned()
        .collect::<Vec<_>>();
    let proof = |purpose: &str| {
        reference(
            docs.iter()
                .find(|r| r["body"]["purpose"] == purpose)
                .unwrap(),
        )
    };
    let mut command = OutcomeCommand {
        principal: PrincipalContext {
            scope: ledgerlab_core::domain::Scope::new("synthetic", "sandbox").unwrap(),
            principal_id: "synthetic-authorized-principal".into(),
            source: base.evaluation.event().source().into(),
            authority_head: "operator".into(),
            can_read: true,
            can_submit: true,
        },
        target: text(&base.snapshot["body"]["target"]).unwrap().into(),
        invocation_id: "invocation-supplier".into(),
        operation: OutcomeOperation::FinalBase { seed: raw },
        received_at: base.target.verification().accepted_at.clone(),
        accepted_at: base.target.verification().accepted_at.clone(),
        required: docs
            .iter()
            .map(|r| scoped_ref(["synthetic".into(), "sandbox".into()], &reference(r)).unwrap())
            .collect(),
    };
    let locks = full_locks(&command, &base, &records).unwrap();
    let mut heads = locks
        .iter()
        .map(|l| ObservedOutcomeHead {
            lock: l.clone(),
            revision: None,
            value: None,
        })
        .collect::<Vec<_>>();
    for head in &mut heads {
        let value = match head.lock.class {
            OutcomeLockClass::Authority => Some(json!({"active":true,"grant":proof("grant")})),
            OutcomeLockClass::Binding => {
                let key: Value = serde_json::from_slice(&head.lock.key).unwrap();
                let r = seed
                    .iter()
                    .find(|r| r["kind"] == "binding-snapshot" && r["body"]["binding_id"] == key[1])
                    .unwrap();
                Some(json!({"active":true,"binding":reference(r)}))
            }
            _ => None,
        };
        if let Some(v) = value {
            head.revision = Some("1".into());
            head.value = Some(bytes(&v).unwrap());
        }
    }
    let initial_heads = heads.clone();
    let initial_records = docs.iter().map(|r| bytes(r).unwrap()).collect::<Vec<_>>();
    let mut snapshot = OutcomeSnapshot {
        anchors: vec![],
        records: initial_records.clone(),
        heads,
    };
    let mut plans = vec![];
    let mut commands = vec![];
    let mut proofs = vec![];
    let mut operations = vec![command.operation.clone()];
    for d in h["decisions"].as_array().unwrap() {
        let rows = d["records"].as_array().unwrap();
        let event = rows.iter().find(|r| r["kind"] == "event").unwrap();
        if event["body"]["data"]["agreement_id"] == "agreement-supplier" {
            operations.push(OutcomeOperation::Economic {
                ingress: bytes(&event["body"]).unwrap(),
                evidence: rows
                    .iter()
                    .filter(|r| r["kind"] == "evidence")
                    .map(|r| bytes(r).unwrap())
                    .collect(),
            });
        }
    }
    operations.push(OutcomeOperation::Close {
        source: "urn:synthetic:close".into(),
        external_id: "explicit-closure".into(),
        expected_revision: "1".into(),
        reason: "authorized".into(),
    });
    for (index, op) in operations.into_iter().enumerate() {
        command.operation = op;
        let time = if index == 0 {
            "2026-09-21T12:00:00.000000Z"
        } else if index == 1 {
            "2026-09-21T13:01:00.000000Z"
        } else if index == 2 {
            "2026-09-21T13:03:00.000000Z"
        } else {
            "2026-09-21T14:00:00.000000Z"
        };
        command.received_at = Timestamp::parse(time).unwrap();
        command.accepted_at = command.received_at.clone();
        command.principal.source = match &command.operation {
            OutcomeOperation::FinalBase { .. } => base.evaluation.event().source().into(),
            OutcomeOperation::Economic { ingress, .. } => serde_json::from_slice::<Value>(ingress)
                .unwrap()["data"]["source"]
                .as_str()
                .unwrap()
                .into(),
            OutcomeOperation::Close { source, .. } => source.clone(),
        };
        let auth = AuthorityProof {
            scope: scoped(&command),
            target: command.target.clone(),
            invocation_id: command.invocation_id.clone(),
            source: command.principal.source.clone(),
            authority: json!({"principal":command.principal.principal_id,"grant":proof("grant"),"grant_revision":"1","active":true,"permissions":["close","correct","read","submit"],"evidence":b::ordered(docs.iter().map(reference).collect()).unwrap()}),
            authentication: proof("authentication"),
            verified_terms: docs
                .iter()
                .map(|r| text(&r["body"]["document_id"]).unwrap().to_owned())
                .collect(),
            finality: true,
            authorized_early_close: true,
        };
        let (event, normalized, key) = operation(&command).unwrap();
        let resolve = OutcomeResolve {
            delivery: key.clone(),
            target: command.target.clone(),
            invocation_id: command.invocation_id.clone(),
            family_key: normalized.get("family").map(|f| {
                bytes(&json!([
                    scoped(&command),
                    f["agreement_id"],
                    f["family_id"],
                    f["target"]
                ]))
                .unwrap()
            }),
            required: command.required.clone(),
            locks: locks.clone(),
        };
        let all = Records::new(&snapshot.records, json!(key.scope)).unwrap();
        let mut proposal = Records::new(
            &seed.iter().map(|r| bytes(r).unwrap()).collect::<Vec<_>>(),
            json!(key.scope),
        )
        .unwrap();
        let result = build(
            &command,
            &event,
            &normalized,
            &key,
            &snapshot,
            &resolve,
            &mut proposal,
            &base,
            &all,
            &auth,
            false,
        )
        .unwrap_or_else(|e| panic!("lifecycle step {index}: {e}"));
        let Some(Prepared::Append(plan, false)) = result else {
            panic!("expected validated new decision")
        };
        apply(&mut snapshot, &plan);
        plans.push(plan);
        commands.push(command.clone());
        proofs.push(auth);
    }
    let all = Records::new(&snapshot.records, json!(scoped(&command))).unwrap();
    validate_registered(&snapshot, &command, &all).unwrap();
    Fixture {
        provisioned_heads: initial_heads,
        provisioned_records: initial_records,
        plans,
        commands,
        proofs,
    }
}
pub(crate) fn apply(snapshot: &mut OutcomeSnapshot, plan: &ValidatedOutcomePlan) {
    for raw in plan
        .economic_records()
        .iter()
        .chain(plan.settlement_records())
    {
        if !snapshot.records.contains(raw) {
            snapshot.records.push(raw.clone());
        }
    }
    for w in plan.head_writes() {
        let h = snapshot
            .heads
            .iter_mut()
            .find(|h| h.lock.class == w.lock.class && h.lock.key == w.lock.key)
            .unwrap();
        h.revision = Some(w.revision.clone());
        h.value = Some(w.value.clone());
    }
    snapshot.anchors = plan.anchors().to_vec();
}
#[test]
fn validated_composite_lifecycle() {
    let f = lifecycle();
    assert_eq!(f.plans.len(), 4);
    for p in &f.plans {
        for w in p.head_writes() {
            assert_eq!(w.lock.mode, OutcomeLockMode::Write);
            assert!(p.resolution().locks.contains(&w.lock));
        }
    }
    let state = |i: usize| {
        serde_json::from_slice::<Value>(&f.plans[i].delivery().settlement_receipt).unwrap()["body"]
            ["result"]
            .clone()
    };
    assert_eq!(state(0)["held"], "12000");
    assert_eq!(state(1)["held"], "9500");
    assert_eq!(state(1)["revision"], "1");
    assert_eq!(state(1), state(2));
    assert_eq!(state(3)["held"], "0");
    assert_eq!(state(3)["released"], "9500");
    assert!(f.plans[2]
        .head_writes()
        .iter()
        .all(|w| w.lock.class != OutcomeLockClass::Reservation));
    assert!(f.plans[3].economic_records().is_empty());
}

#[path = "outcome_fixture_seed.rs"]
mod seed;
