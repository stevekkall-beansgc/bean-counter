use super::{
    records,
    representation::{InvocationToken, ParsedDraftTimes, ParsedGrantInterval, ParsedRef},
    shape, *,
};
use serde_json::{Value, json};
#[test]
fn private_parsed_references_require_owner_and_preserve_raw_validation() {
    let owner = InvocationToken::new();
    let other = InvocationToken::new();
    let mut m = CoreMeter::new(65536).unwrap();
    let body = json!({"schema":"ledger-workflow-retained-evidence/1","purpose":"literal"});
    let digest =
        records::hash(&mut m, "workflow-document", &json!([body["schema"], body])).unwrap();
    let raw =
        json!({"kind":"workflow-evidence","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
    let parsed = ParsedRef::parse(&mut m, &owner, &raw).unwrap();
    let input = RecordInput {
        reference: parsed.exact().clone(),
        canonical_body: codec::encode(&mut m, &body, 4194304).unwrap(),
    };
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["test".into(), "scope".into()],
        source: "source".into(),
        operation_id: "operation".into(),
    };
    let mut r = records::Records::new(&mut m, &[input]).unwrap();
    assert!(
        r.get_parsed(
            &mut m,
            &other,
            &key,
            &parsed,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1"
        )
        .is_err()
    );
    let a = r
        .get_parsed(
            &mut m,
            &owner,
            &key,
            &parsed,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
        )
        .unwrap();
    let b = r
        .get(
            &mut m,
            &key,
            parsed.exact(),
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
        )
        .unwrap();
    assert_eq!(a, b);
    assert!(
        r.get_parsed(
            &mut m,
            &owner,
            &key,
            &parsed,
            "workflow-roles",
            "ledger-workflow-retained-evidence/1"
        )
        .is_err()
    );
    for edit in 0..4 {
        let mut bad = raw.clone();
        match edit {
            0 => bad["other"] = json!(true),
            1 => bad["digest"] = json!("sha256:0"),
            2 => bad["id"] = json!("broken"),
            _ => bad["id"] = Value::Null,
        };
        assert!(ParsedRef::parse(&mut m, &owner, &bad).is_err());
    }
    let duplicate =
        br#"{"kind":"workflow-evidence","\u006bind":"workflow-evidence","id":"x","digest":"x"}"#;
    assert!(codec::parse(&mut m, duplicate, 262144).is_err());
}
#[test]
fn private_times_parse_once_but_reject_cross_invocation_and_bad_dates() {
    let a = InvocationToken::new();
    let b = InvocationToken::new();
    let mut m = CoreMeter::new(65536).unwrap();
    let times = ParsedDraftTimes::parse(
        &mut m,
        &a,
        "2026-10-13T01:00:00.000000Z",
        "2026-10-13T01:00:00.000000Z",
    )
    .unwrap();
    assert!(times.get(&mut m, &b).is_err());
    let (received, accepted) = times.get(&mut m, &a).unwrap();
    assert_eq!(received, accepted);
    let g = ParsedGrantInterval::parse(
        &mut m,
        &a,
        &json!({"starts_at":"2026-10-12T00:00:00.000000Z","ends_at":"2027-01-01T00:00:00.000000Z"}),
    )
    .unwrap();
    assert!(g.get(&mut m, &b).is_err());
    g.get(&mut m, &a).unwrap();
    assert!(ParsedGrantInterval::parse(&mut m,&a,&json!({"starts_at":"2026-02-30T00:00:00.000000Z","ends_at":"2027-01-01T00:00:00.000000Z"})).is_err());
    assert!(
        ParsedGrantInterval::parse(
            &mut m,
            &a,
            &json!({"starts_at":"2026-10-12T00:00:00.000000Z"})
        )
        .is_err()
    );
}
#[test]
fn nested_equality_membership_and_subset_are_paid_and_short_circuit() {
    let value =
        json!({"a":[{"kind":"workflow-binding","numbers":[18,true,"text"]}],"b":{"name":"value"}});
    let mut m = CoreMeter::new(65536).unwrap();
    assert!(shape::same(&mut m, &value, &value).unwrap());
    let n = m.usage().semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    assert!(shape::same(&mut exact, &value, &value).unwrap());
    assert_eq!(m.usage(), exact.usage());
    let mut less = CoreMeter::new(n - 1).unwrap();
    assert_eq!(
        shape::same(&mut less, &value, &value).unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    for change in 0..4 {
        let mut other = value.clone();
        match change {
            0 => other["a"][0]["numbers"][0] = json!(19),
            1 => {
                other.as_object_mut().unwrap().remove("b");
            }
            2 => other["a"][0]["numbers"] = json!([true, 18, "text"]),
            _ => other["b"] = json!("value"),
        };
        let mut meter = CoreMeter::new(65536).unwrap();
        assert!(!shape::same(&mut meter, &value, &other).unwrap());
    }
    let mut meter = CoreMeter::new(65536).unwrap();
    assert!(shape::member(&mut meter, &[json!(false), value.clone()], &value).unwrap());
    shape::subset(&mut meter, &[value.clone()], &[value]).unwrap();
    assert!(
        meter
            .usage()
            .trace
            .iter()
            .any(|x| x.kind == "compare.descent")
    );
}

#[test]
fn noninitial_inventory_remains_explicitly_unsupported() {
    let (_, c, p, _) = super::first_work_tests::test_inputs();
    let payload: serde_json::Value = serde_json::from_slice(&p).unwrap();
    let mut state = super::DraftStateData::default();
    state.chain_head = Some(c.authorities[0].activation_ref.clone());
    assert_eq!(
        super::inventory::entry::validate(
            &mut CoreMeter::new(65536).unwrap(),
            &c,
            &payload,
            &state
        )
        .unwrap_err()
        .code,
        "UNSUPPORTED_COMPOSITION"
    );
}

#[test]
fn byte_identity_counts_actual_mismatch_prefix_without_character_semantics() {
    for (a, b, bytes) in [
        ("abc", "longer", 0),
        ("abc", "xbc", 2),
        ("abc", "axc", 4),
        ("abc", "abc", 6),
    ] {
        let mut m = CoreMeter::new(65536).unwrap();
        assert_eq!(shape::same_text(&mut m, a, b).unwrap(), a == b);
        assert_eq!(m.usage().scanned_bytes, bytes);
        assert_eq!(
            m.usage()
                .trace
                .iter()
                .filter(|x| x.kind == "compare.string")
                .count(),
            1
        );
    }
    let mut zero = CoreMeter::new(0).unwrap();
    assert_eq!(
        shape::same_text(&mut zero, "a", "b").unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert_eq!(zero.usage().scanned_bytes, 0);
}
#[test]
fn connected_causal_repairs_effect_uses_original_work_claim_and_jcs_object_sets() {
    let mut m = CoreMeter::new(65_536).unwrap();
    let work = ExactRef {
        kind: "workflow-work-claim".into(),
        id: "wclaim_b8db304f23c38a48f9cd1b9fa1f211d383366e9bfeb87e3068f315ba34b48763".into(),
        digest: format!("sha256:{}", "1".repeat(64)),
    };
    let link = ExactRef {
        kind: "workflow-link".into(),
        id: "wlink_030ff376439a99b38715616ce2da6a64ae4ea5fe28f0352b314905e2b1799750".into(),
        digest: format!("sha256:{}", "2".repeat(64)),
    };
    let scope = ["fixture".into(), "sandbox".into()];
    let effect = outcome::original_family_effect(
        &mut m,
        &scope,
        &json!("agreement-A"),
        "A-base",
        &work,
        &json!("A-assessment"),
        &link,
    )
    .unwrap();
    // Comparison-only identity from independently sealed A.success action.
    assert_eq!(
        effect,
        "weffect_79fb509b5931f0122224b55cbf32e28b658cb869470697eaa82bb091ae8a433a"
    );
    let wrong = ExactRef {
        kind: "workflow-outcome-claim".into(),
        id: "woclaim_58353788ae61c0899ca753c27ea9db4e3d03a4d3c4d10f7c9dc0cbd5a41098b1".into(),
        digest: work.digest.clone(),
    };
    let other = outcome::original_family_effect(
        &mut m,
        &scope,
        &json!("agreement-A"),
        "A-base",
        &wrong,
        &json!("A-assessment"),
        &link,
    )
    .unwrap();
    assert_ne!(effect, other);
    let a = json!({"family_state_ref":{"id":"z"},"expected_revision":"2"});
    let b = json!({"family_state_ref":{"id":"a"},"expected_revision":"1"});
    assert_eq!(
        body::object_set(&mut m, vec![a.clone(), b.clone()]).unwrap(),
        vec![b, a.clone()]
    );
    assert_eq!(
        body::object_set(&mut m, vec![a.clone(), a])
            .unwrap_err()
            .code,
        "DUPLICATE"
    );
}
#[test]
fn connected_causal_repairs_chain_validates_actual_nonempty_immutable_links() {
    fn rf(kind: &str, prefix: &str) -> ExactRef {
        ExactRef {
            kind: kind.into(),
            id: format!("{prefix}{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        }
    }
    let key = OperationKey {
        logical_store_id: "s".into(),
        scope: ["t".into(), "s".into()],
        source: "source".into(),
        operation_id: "current".into(),
    };
    let binding = rf("workflow-binding", "wdoc_");
    let a = AuthorityContextData {
        source_slot: "test:worker".into(),
        enrollment_ref: rf("workflow-source-enrollment", "wdoc_"),
        current_state_ref: rf("workflow-source-state", "wsource_"),
        current_grant_ref: rf("workflow-source-grant", "wdoc_"),
        original_grant_refs: vec![],
        binding_ref: binding.clone(),
        accepted_agreement_ref: rf("workflow-accepted-agreement", "waccepted_"),
        activation_ref: rf("workflow-activation", "wactivation_"),
        authority_observation_ref: rf("workflow-authority-prefix-observation", "wdoc_"),
    };
    let c = DraftContextData {
        key: key.clone(),
        actor: "actor".into(),
        canonical_economic_namespace: "source".into(),
        installation_mode: "isolated_synthetic".into(),
        received_at: "2026-10-16T00:00:00.000000Z".into(),
        accepted_at: "2026-10-16T00:00:01.000000Z".into(),
        authorities: vec![a],
        source_observation_inputs: vec![],
    };
    let mut m = CoreMeter::new(65_536).unwrap();
    let link=body::record(&mut m,&key,"workflow-link",format!("wlink_{}","3".repeat(64)),json!({"schema":"ledger-workflow-link/1","scope":key.scope,"child":rf("workflow-outcome-claim","woclaim_"),"product_ref":rf("workflow-product","wdoc_"),"binding_ref":binding,"slot":"test:assesses","predecessor":rf("workflow-work","wevent_"),"classification":"monetary","authority_ref":rf("workflow-economic-authority","wdoc_"),"operation_key":key})).unwrap();
    let root = records::hash(&mut m, "workflow-link-set", &json!([link.reference])).unwrap();
    let p = json!({"customer":"customer","chain":"chain","binding_ref":binding});
    let id = body::semantic_id(
        &mut m,
        "wchain_",
        "workflow-chain-state",
        &json!([key.logical_store_id, key.scope, "customer", "chain", "2"]),
    )
    .unwrap();
    let mut prior = json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":key.logical_store_id,"scope":key.scope,"customer":"customer","chain":"chain","binding_ref":binding,"accepted_agreement_ref":c.authorities[0].accepted_agreement_ref,"activation_ref":c.authorities[0].activation_ref,"revision":"2","admitted_events":"2","links_root":root,"operation_key":key,"accepted_at":c.received_at,"prior_ref":rf("workflow-chain-state","wchain_")});
    let reference = body::record(&mut m, &key, "workflow-chain-state", id, prior.clone())
        .unwrap()
        .reference;
    let mut r = records::Records::new(
        &mut m,
        &[RecordInput {
            reference: link.reference.clone(),
            canonical_body: link.canonical_body,
        }],
    )
    .unwrap();
    assert_eq!(
        inventory::entry::chain_head_with_links(
            &mut m,
            &mut r,
            &c,
            &p,
            &reference,
            &prior,
            &[link.reference.clone()]
        )
        .unwrap()
        .0,
        2.into()
    );
    assert!(inventory::entry::chain_head(&mut m, &c, &p, &reference, &prior).is_err());
    // Fully recomputed chain framing with the wrong empty root remains invalid
    // against the actually supplied immutable link, rather than failing digest.
    prior["links_root"] = json!(records::hash(&mut m, "workflow-link-set", &json!([])).unwrap());
    let rebound = body::record(
        &mut m,
        &key,
        "workflow-chain-state",
        reference.id.clone(),
        prior.clone(),
    )
    .unwrap();
    assert!(
        inventory::entry::chain_head_with_links(
            &mut m,
            &mut r,
            &c,
            &p,
            &rebound.reference,
            &prior,
            &[link.reference]
        )
        .is_err()
    );
}
