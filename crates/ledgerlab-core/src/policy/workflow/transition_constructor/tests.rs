use super::*;
use num_bigint::BigInt;
use num_traits::One;
fn compute(limit: u64) -> crate::Result<(i128, MeterUsage)> {
    let mut meter = CoreMeter::new(limit)?;
    meter.phase(Phase::Pricing);
    let quantity = arithmetic::integer(&mut meter, "3", false)?;
    let rate = arithmetic::decimal(&mut meter, "0.37")?;
    let ratio = arithmetic::multiply(
        &mut meter,
        &arithmetic::Ratio {
            numerator: quantity,
            denominator: BigInt::one(),
        },
        &rate,
    )?;
    let scale = arithmetic::integer(&mut meter, "1000000000000000000", false)?;
    let ratio = arithmetic::multiply(
        &mut meter,
        &ratio,
        &arithmetic::Ratio {
            numerator: scale,
            denominator: BigInt::one(),
        },
    )?;
    let limit = arithmetic::integer(&mut meter, "999999999999999999999999999999", false)?;
    let value = arithmetic::rounded(&mut meter, &ratio, &limit)?;
    Ok((value, meter.finish()))
}
#[test]
fn actual_scalar_rational_function_reexecutes_exact_work_and_one_less_refuses_without_result() {
    let (atoms, usage) = compute(65_536).unwrap();
    assert_eq!(atoms, 1_110_000_000_000_000_000);
    assert_eq!(
        compute(usage.semantic_work).unwrap(),
        (atoms, usage.clone())
    );
    assert_eq!(
        compute(usage.semantic_work - 1).unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert!(
        usage
            .trace
            .iter()
            .all(|charge| charge.phase == Phase::Pricing)
    );
}
#[test]
fn signed_ties_away_and_512bit_refusal_are_executed_before_a_result() {
    let mut m = CoreMeter::new(65_536).unwrap();
    let limit = BigInt::from(9999);
    for (n, expected) in [(1, 1), (-1, -1), (3, 2), (-3, -2)] {
        let r = arithmetic::rational(&mut m, BigInt::from(n), BigInt::from(2)).unwrap();
        assert_eq!(arithmetic::rounded(&mut m, &r, &limit).unwrap(), expected);
    }
    let mut m = CoreMeter::new(65_536).unwrap();
    let too_big = BigInt::one() << 512usize;
    assert_eq!(
        m.arithmetic("arith.add", &[&too_big, &BigInt::one()])
            .unwrap_err()
            .code,
        "ARITHMETIC_OVERFLOW"
    );
    assert_eq!(m.usage().semantic_work, 0);
}
#[test]
fn compile_meter_cannot_raise_a_draft_invocation_ceiling() {
    let mut compile = CoreMeter::for_compile(1_048_576).unwrap();
    assert_eq!(
        compile.enter_draft().unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    let mut draft = CoreMeter::new(65_536).unwrap();
    draft.enter_draft().unwrap();
    draft.visit("prior.work").unwrap();
    assert_eq!(
        draft.enter_draft().unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
}
#[test]
fn exact_resolved_document_detects_conflicting_input_and_corrupt_body() {
    use super::data::{ExactRef, OperationKey, RecordInput};
    use serde_json::json;
    let body = json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"literal"});
    let mut meter = CoreMeter::new(65536).unwrap();
    let bytes = codec::encode(&mut meter, &body, 262144).unwrap();
    let digest = records::hash(
        &mut meter,
        "workflow-document",
        &json!([body["schema"], body]),
    )
    .unwrap();
    let reference = ExactRef {
        kind: "workflow-evidence".into(),
        id: format!("wdoc_{}", &digest[7..]),
        digest,
    };
    let input = RecordInput {
        reference: reference.clone(),
        canonical_body: bytes,
    };
    let key = OperationKey {
        logical_store_id: "store".into(),
        scope: ["t".into(), "n".into()],
        source: "actor".into(),
        operation_id: "one".into(),
    };
    let mut meter = CoreMeter::new(65536).unwrap();
    let mut resolver = records::Records::new(&mut meter, std::slice::from_ref(&input)).unwrap();
    assert_eq!(
        resolver
            .get(
                &mut meter,
                &key,
                &reference,
                "workflow-evidence",
                "ledger-workflow-evidence-retain/1"
            )
            .unwrap(),
        body
    );
    let mut meter = CoreMeter::new(65536).unwrap();
    assert!(records::Records::new(&mut meter, &[input.clone(), input.clone()]).is_err());
    let mut corrupted = input;
    corrupted.canonical_body = codec::encode(
        &mut meter,
        &json!({"schema":"ledger-workflow-evidence-retain/1","purpose":"altered"}),
        262144,
    )
    .unwrap();
    let mut meter = CoreMeter::new(65536).unwrap();
    let mut resolver = records::Records::new(&mut meter, &[corrupted]).unwrap();
    assert_eq!(
        resolver
            .get(
                &mut meter,
                &key,
                &reference,
                "workflow-evidence",
                "ledger-workflow-evidence-retain/1"
            )
            .unwrap_err()
            .code,
        "INTEGRITY_FAILURE"
    );
}
#[test]
fn selected_fixed_unit_and_named_additive_discount_execute_the_actual_pricing_path() {
    use super::super::selected_program::{
        SelectedCompilerSupport, SelectedSourceInput, compile_selected_program,
    };
    use crate::canonical;
    use serde_json::{Value, json};
    let source: Value = canonical::parse(include_bytes!(
        "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
    ))
    .unwrap();
    let mut compile = CoreMeter::for_compile(1_048_576).unwrap();
    let program = compile_selected_program(
        SelectedSourceInput {
            documents: source["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        },
        &SelectedCompilerSupport::builtin().unwrap(),
        &mut compile,
    )
    .unwrap();
    for quantity in [1, 2, 3, 4, 100] {
        let mut meter = CoreMeter::new(65536).unwrap();
        let result = pricing::work(
            &program,
            "fixture.measure:measured",
            &json!({"fixture.measure:quantity":quantity.to_string()}),
            &mut meter,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].atoms,
            i128::from(quantity) * 370_000_000_000_000_000
        );
        assert_eq!(
            result[1].atoms,
            -i128::from(quantity) * 37_000_000_000_000_000
        );
        assert_eq!(result[1].basis_components, vec!["fixture.measure:base"]);
        let work = meter.finish();
        let mut exact = CoreMeter::new(work.semantic_work).unwrap();
        pricing::work(
            &program,
            "fixture.measure:measured",
            &json!({"fixture.measure:quantity":quantity.to_string()}),
            &mut exact,
        )
        .unwrap();
        assert_eq!(exact.finish(), work);
        let mut less = CoreMeter::new(work.semantic_work - 1).unwrap();
        assert_eq!(
            pricing::work(
                &program,
                "fixture.measure:measured",
                &json!({"fixture.measure:quantity":quantity.to_string()}),
                &mut less
            )
            .unwrap_err()
            .code,
            "PROGRAM_BUDGET_EXCEEDED"
        );
    }
    let mut meter = CoreMeter::new(65536).unwrap();
    assert_eq!(
        pricing::work(&program, "fixture.measure:setup", &json!({}), &mut meter).unwrap()[0].atoms,
        410_000_000_000_000_000
    );
}
#[test]
fn actual_calendar_callback_adapter_reexecutes_exact_work_and_preserves_budget_error() {
    use crate::domain::Timestamp;
    let execute = |maximum| {
        let mut meter = CoreMeter::new(maximum)?;
        let timestamp = Timestamp::parse_with_work("2026-11-02T06:00:01+05:00", &mut meter)?;
        Ok::<_, crate::Error>((timestamp.as_str().to_owned(), meter.finish()))
    };
    let (canonical, usage) = execute(65536).unwrap();
    assert_eq!(canonical, "2026-11-02T01:00:01.000000Z");
    assert_eq!(
        execute(usage.semantic_work).unwrap(),
        (canonical, usage.clone())
    );
    assert_eq!(
        execute(usage.semantic_work - 1).unwrap_err().code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert!(usage.trace.iter().any(|c| c.kind == "arith.divmod"));
    assert!(usage.hashed_bytes == 0);
}
#[test]
fn private_pricing_algorithm_keeps_distinct_binding_bases_separate() {
    use serde_json::json;
    let base = |slot: &str, rate: &str, ordinal: &str| json!({"opcode":"evaluate_rule","ordinal":ordinal,"rule":{"activity":"example:work","component_key":"example:base","binding_slot":slot,"formula":{"kind":"unit","quantity_field":"example:quantity","rate":rate}}});
    let discount = |slot: &str, ordinal: &str| json!({"opcode":"evaluate_rule","ordinal":ordinal,"rule":{"activity":"example:work","component_key":"example:discount","binding_slot":slot,"formula":{"kind":"discount","basis":{"component_key":"example:base"},"amount":{"percent":"20"}}}});
    let components = vec![
        json!({"key":"example:base","root_key":"example:base","role_slot":"example:role"}),
        json!({"key":"example:discount","root_key":"example:base","role_slot":"example:role"}),
    ];
    // Algorithm-only defensive data; the selected source compiler rejects a
    // second retail binding. This does not mint an unchecked SelectedProgram.
    let mut meter = CoreMeter::new(65536).unwrap();
    let result = pricing::price_instructions(
        &[
            base("example:a", "1", "0"),
            base("example:b", "2", "1"),
            discount("example:a", "2"),
            discount("example:b", "3"),
        ],
        &components,
        "example:work",
        &json!({"example:quantity":"1"}),
        &mut meter,
    )
    .unwrap();
    assert_eq!(result[2].atoms, -200_000_000_000_000_000);
    assert_eq!(result[3].atoms, -400_000_000_000_000_000);
}

#[test]
fn capped_output_checks_body_and_remaining_new_bytes_without_count_walk() {
    use serde_json::json;
    let value = json!({"escaped":"\n\u{001f}😀","integer":-42});
    let old = crate::canonical::CanonicalBytes::from_value(&value)
        .unwrap()
        .into_vec();
    let mut m = CoreMeter::new(65536).unwrap();
    let bytes = codec::encode(&mut m, &value, old.len()).unwrap();
    assert_eq!(bytes, old);
    assert!(!m.usage().trace.iter().any(|x| x.kind.starts_with("codec.")));
    let n = m.usage().semantic_work;
    let mut exact = CoreMeter::new(n).unwrap();
    assert_eq!(codec::encode(&mut exact, &value, old.len()).unwrap(), old);
    let mut short = CoreMeter::new(n - 1).unwrap();
    assert_eq!(
        codec::encode(&mut short, &value, old.len())
            .unwrap_err()
            .code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    let mut short_bytes = CoreMeter::new(65536).unwrap();
    assert_eq!(
        codec::encode(&mut short_bytes, &value, old.len() - 1)
            .unwrap_err()
            .code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    let mut aggregate = CoreMeter::new(65536).unwrap();
    aggregate.new_body(8388606).unwrap();
    assert_eq!(
        codec::encode_new(&mut aggregate, &json!("x"), 4194304)
            .unwrap_err()
            .code,
        "PROGRAM_BUDGET_EXCEEDED"
    );
    assert_eq!(aggregate.usage().new_body_bytes, 8388606);
    let mut null = CoreMeter::new(65536).unwrap();
    assert_eq!(
        codec::encode(&mut null, &serde_json::Value::Null, 4194304)
            .unwrap_err()
            .code,
        "INVALID_SHAPE"
    );
}
