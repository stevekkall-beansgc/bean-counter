use super::super::selected_program::{
    SelectedCompilerSupport, SelectedSourceInput, compile_selected_program,
};
use super::*;
use crate::canonical;
use serde_json::{Value, json};
fn decode(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn record(v: &Value) -> RecordInput {
    RecordInput {
        reference: serde_json::from_value(v["reference"].clone()).unwrap(),
        canonical_body: decode(v["canonical_body_hex"].as_str().unwrap()),
    }
}
fn value(v: &Value) -> Vec<u8> {
    canonical::CanonicalBytes::from_value(v).unwrap().into_vec()
}
pub(super) fn test_inputs() -> (
    super::super::selected_program::SelectedProgram,
    DraftContextData,
    Vec<u8>,
    Vec<RecordInput>,
) {
    let src: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
    ))
    .unwrap();
    let commands: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/command-input.json"
    ))
    .unwrap();
    let audit:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-audit-input.json")).unwrap();
    let input: Value =
        serde_json::from_slice(include_bytes!("fixtures/prerequisite-input-data.json")).unwrap();
    let observations: Value = serde_json::from_slice(include_bytes!(
        "fixtures/source-observation-input-data.json"
    ))
    .unwrap();
    let support = SelectedCompilerSupport::builtin().unwrap();
    let mut cm = CoreMeter::for_compile(1048576).unwrap();
    let program = compile_selected_program(
        SelectedSourceInput {
            documents: src["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(value)
                .collect(),
            root_policy_ref: serde_json::from_value(src["root_policy_ref"].clone()).unwrap(),
        },
        &support,
        &mut cm,
    )
    .unwrap();
    let mut named = std::collections::BTreeMap::new();
    let mut records = vec![];
    for row in input["records"].as_array().unwrap() {
        let r = record(row);
        named.insert(
            row["name"].as_str().unwrap().to_owned(),
            r.reference.clone(),
        );
        records.push(r);
    }
    fn resolve(v: &Value, names: &std::collections::BTreeMap<String, ExactRef>) -> Value {
        if let Some(n) = v.get("fixture_ref") {
            return json!(names[n.as_str().unwrap()]);
        }
        match v {
            Value::Array(a) => json!(a.iter().map(|v| resolve(v, names)).collect::<Vec<_>>()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (k.clone(), resolve(v, names)))
                    .collect(),
            ),
            _ => v.clone(),
        }
    }
    let command = &commands["histories"][0]["operations"][0];
    let p = resolve(&command["payload"], &named);
    let observation = record(&observations["commands"][0]["observation_input"]);
    let verifier = record(&observations["verifier_input"]);
    let ob: Value = serde_json::from_slice(&observation.canonical_body).unwrap();
    let context = DraftContextData {
        key: serde_json::from_value(command["key"].clone()).unwrap(),
        actor: audit["trusted_context"]["local_authority_basis"]["principal_id"]
            .as_str()
            .unwrap()
            .into(),
        canonical_economic_namespace:
            audit["trusted_context"]["local_authority_basis"]["canonical_economic_namespace"]
                .as_str()
                .unwrap()
                .into(),
        installation_mode: audit["trusted_context"]["installation_mode"]
            .as_str()
            .unwrap()
            .into(),
        received_at: command["received_at"].as_str().unwrap().into(),
        accepted_at: command["accepted_at"].as_str().unwrap().into(),
        authorities: vec![AuthorityContextData {
            source_slot: "fixture.measure:worker".into(),
            enrollment_ref: serde_json::from_value(ob["enrollment_ref"].clone()).unwrap(),
            current_state_ref: serde_json::from_value(ob["current_state_ref"].clone()).unwrap(),
            current_grant_ref: serde_json::from_value(ob["current_grant_ref"].clone()).unwrap(),
            original_grant_refs: serde_json::from_value(ob["original_grant_refs"].clone()).unwrap(),
            binding_ref: named["A.binding"].clone(),
            accepted_agreement_ref: named["A.accepted"].clone(),
            activation_ref: named["A.activation"].clone(),
            authority_observation_ref: Some(observation.reference.clone()),
        }],
        source_observation_inputs: vec![verifier, observation],
    };
    (program, context, value(&p), records)
}
#[test]
fn first_work_executes_source_data_and_matches_independent_complete_bodies() {
    let (program, c, p, records) = test_inputs();
    let mut m = CoreMeter::new(65536).unwrap();
    let result = draft_transition(
        &program,
        &c,
        &p,
        &records,
        &DraftStateData::default(),
        &mut m,
    );
    if let Err(e) = &result {
        println!(
            "FIRST_DRAFT_FAILURE {e:?} work={} last={:?}",
            m.usage().semantic_work,
            m.usage().trace.last()
        );
        let mut phases = std::collections::BTreeMap::<String, u64>::new();
        let mut events = std::collections::BTreeMap::<String, u64>::new();
        for charge in &m.usage().trace {
            *phases.entry(format!("{:?}", charge.phase)).or_default() += charge.cost;
            *events
                .entry(format!("{:?}:{}", charge.phase, charge.kind))
                .or_default() += charge.cost;
        }
        println!("REFUSAL_PHASES {}", serde_json::to_string(&phases).unwrap());
        println!("REFUSAL_EVENTS {}", serde_json::to_string(&events).unwrap());
        println!("REFUSAL_FULL_TRACE {}",serde_json::to_string(&m.usage().trace.iter().map(|x|json!({"phase":format!("{:?}",x.phase),"kind":x.kind,"operand_limbs":x.operands,"cost":x.cost})).collect::<Vec<_>>()).unwrap());
    }
    let draft = result.unwrap();
    println!(
        "FIRST_DRAFT_WORK {} refs={} resolvedbytes={} newbytes={}",
        draft.usage.semantic_work,
        draft.usage.unique_resolved_refs,
        draft.usage.unique_resolved_bytes,
        draft.usage.new_body_bytes
    );
    let expected: Value =
        serde_json::from_slice(include_bytes!("fixtures/first-work-expected.json")).unwrap();
    let expected = &expected["expected"];
    assert_eq!(
        draft.normalized_payload,
        value(&expected["normalized_payload"])
    );
    assert_eq!(
        draft.new_envelopes.len(),
        expected["new_records"].as_array().unwrap().len()
    );
    for actual in &draft.new_envelopes {
        let row = expected["new_records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["ref"] == json!(actual.reference))
            .unwrap_or_else(|| {
                panic!(
                    "unexpected {:?} body {}",
                    actual.reference,
                    String::from_utf8_lossy(&actual.canonical_body)
                )
            });
        assert_eq!(actual.canonical_body, value(&row["body"]));
    }
    let n = draft.usage.semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    let exact_draft = draft_transition(
        &program,
        &c,
        &p,
        &records,
        &DraftStateData::default(),
        &mut exact,
    )
    .unwrap();
    assert_eq!(draft, exact_draft);
    let mut less = CoreMeter::new(n - 1).unwrap();
    assert_eq!(
        draft_transition(
            &program,
            &c,
            &p,
            &records,
            &DraftStateData::default(),
            &mut less
        )
        .unwrap_err()
        .code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    let mut replay = CoreMeter::new(65536).unwrap();
    let replayed = replay_draft(
        &program,
        &c,
        &p,
        &records,
        &DraftStateData::default(),
        &draft,
        &mut replay,
    )
    .unwrap();
    assert_eq!(replayed, draft);
    assert!(replay.usage().semantic_work > n);
}
