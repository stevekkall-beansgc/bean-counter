use super::*;
use serde_json::json;

fn schema(fields: Value) -> FactSchema {
    FactSchema::parse(
        &serde_json::to_vec(&json!({
            "schema":"ledger-workflow-fact-schema/1", "schema_id":"example:work", "version":"1",
            "normalization":"ledger-facts-normalization/1", "fields":fields
        }))
        .unwrap(),
    )
    .unwrap()
}
fn field(name: &str, required: bool, kind: Value) -> Value {
    json!({"name":name,"required":required,"type":kind,"provenance":{"kind":"caller"}})
}
fn integer() -> FactSchema {
    schema(json!([field(
        "example:quantity",
        true,
        json!({"kind":"integer","min":"0","max":"999999999999999999999999999999","unit":"example:work"})
    )]))
}
fn decimal() -> FactSchema {
    schema(json!([field(
        "example:delta",
        true,
        json!({"kind":"decimal","min":"-2","max":"2","max_scale":"2"})
    )]))
}
fn error<T: std::fmt::Debug>(result: Result<T>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}

#[test]
fn required_and_missing_are_distinct_without_invented_defaults() {
    let s = schema(json!([
        field("example:required", true, json!({"kind":"boolean"})),
        field("example:optional", false, json!({"kind":"boolean"}))
    ]));
    error(s.normalize(b"{}"), "FACT_REQUIRED");
    let facts = s.normalize(br#"{"example:required":false}"#).unwrap();
    assert_eq!(facts.get("example:optional"), Some(&FactValue::Missing));
    assert_eq!(facts.canonical_value(), &json!({"example:required":false}));
    error(
        s.normalize(br#"{"example:required":false,"example:optional":null}"#),
        "FACT_TYPE",
    );
}

#[test]
fn strict_lexical_parser_runs_before_deserialization() {
    let s = integer();
    for bytes in [
        br#"{"example:quantity":"1","example:quantity":"2"}"#.as_slice(),
        br#"{"example:quantity":-0}"#,
        br#"{"example:quantity":1e0}"#,
        br#"{"example:quantity":1.0}"#,
    ] {
        assert!(s.normalize(bytes).is_err(), "{:?}", bytes);
    }
    error(s.normalize(br#"{"example:quantity":1}"#), "FACT_TYPE");
}

#[test]
fn integer_limits_and_signed_bounds_are_exact() {
    let s = integer();
    assert_eq!(
        s.normalize(br#"{"example:quantity":"0001"}"#)
            .unwrap()
            .canonical_value(),
        &json!({"example:quantity":"1"})
    );
    s.normalize(br#"{"example:quantity":"999999999999999999999999999999"}"#)
        .unwrap();
    error(
        s.normalize(br#"{"example:quantity":"1000000000000000000000000000000"}"#),
        "FACT_PRECISION",
    );
    error(s.normalize(br#"{"example:quantity":"-1"}"#), "FACT_RANGE");
    for value in ["-0", "-00", "1.0", "+1", "1e1"] {
        assert!(s
            .normalize(&serde_json::to_vec(&json!({"example:quantity":value})).unwrap())
            .is_err());
    }
    let s = schema(json!([field(
        "example:delta",
        true,
        json!({"kind":"integer","min":"-999999999999999999999999999999","max":"-1"})
    )]));
    s.normalize(br#"{"example:delta":"-999999999999999999999999999999"}"#)
        .unwrap();
    error(s.normalize(br#"{"example:delta":"0"}"#), "FACT_RANGE");
}

#[test]
fn decimal_raw_scale_and_precision_precede_normalization() {
    let s = decimal();
    assert_eq!(
        s.normalize(br#"{"example:delta":"-01.20"}"#)
            .unwrap()
            .canonical_value(),
        &json!({"example:delta":"-1.2"})
    );
    for value in ["-0.00", "1.000", "1.", ".1", "+1", "1e-2"] {
        error(
            s.normalize(&serde_json::to_vec(&json!({"example:delta":value})).unwrap()),
            "FACT_PRECISION",
        );
    }
    error(s.normalize(br#"{"example:delta":"2.01"}"#), "FACT_RANGE");
    error(s.normalize(br#"{"example:delta":"-2.01"}"#), "FACT_RANGE");
    s.normalize(br#"{"example:delta":"-2"}"#).unwrap();
    s.normalize(br#"{"example:delta":"2"}"#).unwrap();
    assert!(SignedScalar::parse("999999999999999999999999999999.0", 18).is_ok());
    assert!(SignedScalar::parse("1000000000000000000000000000000", 18).is_err());
}

#[test]
fn comparison_does_not_overflow_at_30_digits_and_scale_18() {
    let large = SignedScalar::parse("999999999999999999999999999999", 18).unwrap();
    let tiny = SignedScalar::parse("0.000000000000000001", 18).unwrap();
    assert!(large > tiny);
    assert!(SignedScalar::parse("-999999999999999999999999999999", 18).unwrap() < tiny);
    assert_eq!(
        serde_json::to_value(tiny.ratio()).unwrap(),
        json!({"numerator":"1","denominator":"1000000000000000000"})
    );
}

#[test]
fn closed_shapes_reject_unknown_and_explicit_null_recursively() {
    let mut value = schema(json!([field(
        "example:n",
        true,
        json!({"kind":"integer","min":"0","max":"2"})
    )]))
    .body;
    for path in [0, 1, 2] {
        let mut wrong = value.clone();
        match path {
            0 => {
                wrong["extra"] = json!(true);
            }
            1 => {
                wrong["fields"][0]["type"]["extra"] = json!(true);
            }
            _ => {
                wrong["fields"][0]["provenance"]["extra"] = json!(true);
            }
        }
        assert!(FactSchema::parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
    }
    value["fields"][0]["type"]["unit"] = Value::Null;
    error(
        FactSchema::parse(&serde_json::to_vec(&value).unwrap()),
        "FACT_SCHEMA",
    );
    error(
        integer().normalize(br#"{"example:quantity":"1","example:price":"0"}"#),
        "FACT_UNKNOWN",
    );
}

#[test]
fn schema_sets_normalize_deterministically_and_refuse_duplicates() {
    let a = field(
        "example:a",
        true,
        json!({"kind":"enum","values":["example:z","example:a"]}),
    );
    let b = field("example:b", false, json!({"kind":"boolean"}));
    let left = schema(json!([a.clone(), b.clone()]));
    let right = schema(json!([b, a.clone()]));
    assert_eq!(
        left.canonical_bytes().unwrap(),
        right.canonical_bytes().unwrap()
    );
    let mut duplicate = left.body.clone();
    duplicate["fields"] = json!([a.clone(), a]);
    error(
        FactSchema::parse(&serde_json::to_vec(&duplicate).unwrap()),
        "FACT_SCHEMA",
    );
    duplicate["fields"] = json!([field(
        "example:e",
        true,
        json!({"kind":"enum","values":["example:a","example:a"]})
    )]);
    assert!(FactSchema::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
}

#[test]
fn strings_enums_names_and_timestamps_remain_typed() {
    let s = schema(json!([
        field(
            "example:text",
            true,
            json!({"kind":"string","min_bytes":"1","max_bytes":"4"})
        ),
        field(
            "example:enum",
            true,
            json!({"kind":"enum","values":["example:yes"]})
        ),
        field(
            "example:time",
            true,
            json!({"kind":"timestamp","min":"2026-01-01T00:00:00Z","max":"2027-01-01T00:00:00Z"})
        )
    ]));
    let good = json!({"example:text":"éé","example:enum":"example:yes","example:time":"2026-01-01T01:00:00+01:00"});
    let facts = s.normalize(&serde_json::to_vec(&good).unwrap()).unwrap();
    assert_eq!(
        facts.canonical_value()["example:time"],
        "2026-01-01T00:00:00.000000Z"
    );
    for (field, value) in [
        ("example:text", "ééé"),
        ("example:text", "a\n"),
        ("example:enum", "example:Yes"),
        ("example:time", "2025-12-31T23:59:59Z"),
    ] {
        let mut wrong = good.clone();
        wrong[field] = json!(value);
        assert!(s.normalize(&serde_json::to_vec(&wrong).unwrap()).is_err());
    }
    for value in ["unqualified", "a:b:c", "1bad:name", "a:bad space"] {
        assert!(name(value).is_err());
    }
}

#[test]
fn quantity_corrections_use_original_field_descriptor() {
    let old = integer();
    let new = schema(json!([field(
        "example:quantity",
        true,
        json!({"kind":"decimal","min":"0","max":"100","max_scale":"2","unit":"example:work"})
    )]));
    assert_eq!(
        old.quantity_unit("example:quantity").unwrap(),
        "example:work"
    );
    old.normalize_quantity("example:quantity", br#""2""#)
        .unwrap();
    error(
        old.normalize_quantity("example:quantity", br#""2.5""#),
        "FACT_PRECISION",
    );
    new.normalize_quantity("example:quantity", br#""2.5""#)
        .unwrap();
    error(decimal().quantity_unit("example:delta"), "QUANTITY_TYPE");
    let optional = schema(json!([field(
        "example:quantity",
        false,
        json!({"kind":"integer","min":"0","max":"2","unit":"example:work"})
    )]));
    error(optional.quantity_unit("example:quantity"), "QUANTITY_TYPE");
}

#[test]
fn attested_facts_require_proof_and_reference_types_fail_closed() {
    let mut f = field(
        "example:cost",
        true,
        json!({"kind":"integer","min":"0","max":"2"}),
    );
    f["provenance"] =
        json!({"kind":"attested","source_slot":"example:meter","evidence_kind":"meter_quantity"});
    let s = schema(json!([f]));
    assert_eq!(
        s.normalize(br#"{"example:cost":"1"}"#)
            .unwrap()
            .required_proof_fields(),
        &["example:cost"]
    );
    let mut unsupported = s.body.clone();
    unsupported["fields"][0]["type"] =
        json!({"kind":"reference","record_kind":"anything","relation":"same_binding"});
    error(
        FactSchema::parse(&serde_json::to_vec(&unsupported).unwrap()),
        "FACT_UNSUPPORTED_TYPE",
    );
}

#[test]
fn independently_derived_schema_bytes_and_document_identity() {
    // Independent stdlib oracle by /root/adversarial_money, 2026-10-12:
    // review-fixtures/independent-expected.json vector fact-schema-v1.
    // The expected values are not obtained from the production normalizer/hash.
    let expected = br#"{"fields":[{"name":"review.measure:units","provenance":{"kind":"caller"},"required":true,"type":{"kind":"integer","max":"9","min":"1","unit":"review.measure:item"}}],"normalization":"ledger-facts-normalization/1","schema":"ledger-workflow-fact-schema/1","schema_id":"review.measure:work-facts","version":"1"}"#;
    let schema = FactSchema::parse(expected).unwrap();
    assert_eq!(schema.canonical_bytes().unwrap().as_slice(), expected);
    assert_eq!(
        schema.document_id().unwrap(),
        "wdoc_f291139089808d576591c70182ee5891eae994d278d8a75ffd94bc44aa9256e7"
    );
    assert_eq!(
        schema.content_digest().unwrap(),
        "sha256:f291139089808d576591c70182ee5891eae994d278d8a75ffd94bc44aa9256e7"
    );
    assert_eq!(
        CanonicalBytes::from_value(
            schema
                .normalize(br#"{"review.measure:units":"003"}"#)
                .unwrap()
                .canonical_value()
        )
        .unwrap()
        .as_slice(),
        br#"{"review.measure:units":"3"}"#
    );
}

#[test]
fn descriptor_domains_and_exact_field_ceiling_are_validated_first() {
    let fields: Vec<_> = (0..64)
        .map(|i| {
            field(
                &format!("example:field{i}"),
                false,
                json!({"kind":"boolean"}),
            )
        })
        .collect();
    let s = schema(json!(fields));
    s.normalize(b"{}").unwrap();
    let mut oversized = s.body.clone();
    oversized["fields"].as_array_mut().unwrap().push(field(
        "example:extra",
        false,
        json!({"kind":"boolean"}),
    ));
    error(
        FactSchema::parse(&serde_json::to_vec(&oversized).unwrap()),
        "FACT_LIMIT",
    );
    for kind in [
        json!({"kind":"integer","min":"2","max":"1"}),
        json!({"kind":"integer","min":"1.0","max":"2"}),
        json!({"kind":"decimal","min":"0","max":"1.000","max_scale":"2"}),
        json!({"kind":"decimal","min":"0","max":"1","max_scale":"19"}),
        json!({"kind":"string","min_bytes":"0","max_bytes":"1025"}),
        json!({"kind":"boolean","default":false}),
        json!({"kind":"array","items":{"kind":"integer"}}),
    ] {
        let mut wrong = integer().body.clone();
        wrong["fields"][0]["type"] = kind;
        assert!(FactSchema::parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
    }
}

#[test]
fn executing_signed_scalar_preserves_normalization_and_exact_budget() {
    use crate::validation_work::tests::CountingWork;
    for (raw, scale, expected) in [
        ("0001", 0, "1"),
        ("000.000", 3, "0"),
        ("-0001.2300", 4, "-1.23"),
        (
            "999999999999999999999999999999.0",
            1,
            "999999999999999999999999999999",
        ),
        ("0.000000000000000001", 18, "0.000000000000000001"),
    ] {
        let mut probe = CountingWork::new(usize::MAX);
        let scalar = SignedScalar::parse_with_work(raw, scale, &mut probe).unwrap();
        assert_eq!(scalar.as_str(), expected);
        assert_eq!(scalar, SignedScalar::parse(raw, scale).unwrap());
        if expected != "0" {
            assert!(probe
                .operations
                .iter()
                .any(|(kind, _)| *kind == Op::Multiply));
        }
        let mut exact = CountingWork::new(probe.used);
        assert_eq!(
            SignedScalar::parse_with_work(raw, scale, &mut exact).unwrap(),
            scalar
        );
        assert_eq!(exact.operations, probe.operations);
        let mut short = CountingWork::new(probe.used - 1);
        assert_eq!(
            SignedScalar::parse_with_work(raw, scale, &mut short)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
    }
    let mut work = CountingWork::new(0);
    assert_eq!(
        SignedScalar::parse_with_work("-0", 0, &mut work)
            .unwrap_err()
            .code,
        "TEST_EXHAUSTED"
    );
    assert_eq!(
        SignedScalar::parse("-0", 0).unwrap_err().code,
        "FACT_PRECISION"
    );
}

#[test]
fn executing_scalar_comparison_and_reduction_use_actual_operands() {
    use crate::validation_work::tests::CountingWork;
    let a = SignedScalar::parse("-1.20", 2).unwrap();
    let b = SignedScalar::parse("-1.199", 3).unwrap();
    let mut work = CountingWork::new(usize::MAX);
    assert_eq!(a.compare_with_work(&b, &mut work).unwrap(), Ordering::Less);
    let last = work.operations.last().unwrap();
    assert_eq!(
        last,
        &(
            Op::Compare,
            vec![BigInt::from(-12000), BigInt::from(-11990)]
        )
    );
    let mut ratio_work = CountingWork::new(usize::MAX);
    let ratio = a.ratio_with_work(&mut ratio_work).unwrap();
    assert_eq!(
        serde_json::to_value(ratio).unwrap(),
        json!({"numerator":"-6","denominator":"5"})
    );
    assert!(ratio_work
        .operations
        .iter()
        .any(|(kind, operands)| *kind == Op::DivideRemainder
            && operands == &[BigInt::from(12), BigInt::from(10)]));
    let mut short = CountingWork::new(ratio_work.used - 1);
    assert_eq!(
        a.ratio_with_work(&mut short).unwrap_err().code,
        "TEST_EXHAUSTED"
    );
}

#[test]
fn executing_schema_and_facts_cover_selected_types_without_redecoding() {
    use crate::validation_work::tests::CountingWork;
    let raw=serde_json::to_vec(&json!({
        "schema":"ledger-workflow-fact-schema/1","schema_id":"example:metered","version":"0001",
        "normalization":"ledger-facts-normalization/1","fields":[
            field("example:z",true,json!({"kind":"timestamp","min":"2000-01-01T00:00:00Z","max":"2001-01-01T00:00:00Z"})),
            {"name":"example:m","required":true,"type":{"kind":"enum","values":["example:z","example:a"]},"provenance":{"kind":"attested","source_slot":"example:source","evidence_kind":"outcome"}},
            field("example:a",true,json!({"kind":"integer","min":"0000","max":"00010","unit":"example:unit"}))
        ]
    })).unwrap();
    let mut work = CountingWork::new(usize::MAX);
    let schema = FactSchema::parse_with_work(&raw, &mut work).unwrap();
    assert_eq!(schema.body["version"], "1");
    assert_eq!(schema.body["fields"][0]["type"]["max"], "10");
    assert_eq!(
        schema.body["fields"][1]["type"]["values"],
        json!(["example:a", "example:z"])
    );
    assert!(work.visits.contains(&Visit::SortMove));
    assert!(work
        .operations
        .iter()
        .any(|(op, _)| *op == Op::DivideRemainder));
    let mut exact = CountingWork::new(work.used);
    assert_eq!(
        FactSchema::parse_with_work(&raw, &mut exact).unwrap().body,
        schema.body
    );
    let mut short = CountingWork::new(work.used - 1);
    error(
        FactSchema::parse_with_work(&raw, &mut short),
        "TEST_EXHAUSTED",
    );
    let mut decoded_work = CountingWork::new(usize::MAX);
    let decoded =
        FactSchema::from_value_with_work(canonical::parse(&raw).unwrap(), &mut decoded_work)
            .unwrap();
    assert_eq!(decoded.body, schema.body);
    assert_eq!(decoded_work.used, work.used);
    assert_eq!(decoded_work.scanned + raw.len(), work.scanned);
    let input =
        json!({"example:a":"0002","example:m":"example:z","example:z":"2000-02-29T01:00:00+01:00"});
    let bytes = serde_json::to_vec(&input).unwrap();
    let mut facts_work = CountingWork::new(usize::MAX);
    let facts = schema.normalize_with_work(&bytes, &mut facts_work).unwrap();
    assert_eq!(
        facts.body,
        json!({"example:a":"2","example:m":"example:z","example:z":"2000-02-29T00:00:00.000000Z"})
    );
    assert_eq!(facts.required_proofs, ["example:m"]);
    let mut exact = CountingWork::new(facts_work.used);
    assert_eq!(
        schema.normalize_with_work(&bytes, &mut exact).unwrap().body,
        facts.body
    );
    let mut short = CountingWork::new(facts_work.used - 1);
    error(
        schema.normalize_with_work(&bytes, &mut short),
        "TEST_EXHAUSTED",
    );
    let mut no_work = CountingWork::new(0);
    error(
        schema.normalize_with_work(br#"{"unknown:field":"x"}"#, &mut no_work),
        "TEST_EXHAUSTED",
    );
    error(
        schema.normalize(br#"{"unknown:field":"x"}"#),
        "FACT_UNKNOWN",
    );
}

#[test]
fn complete_normalization_pays_copies_and_preserves_legacy_paid_trace() {
    use crate::validation_work::tests::CountingWork;
    let s = schema(json!([{"name":"example:code","required":true,
        "type":{"kind":"enum","values":["example:a"]},
        "provenance":{"kind":"attested","source_slot":"example:source","evidence_kind":"outcome"}}]));
    let input = json!({"example:code":"example:a"});
    let mut old = CountingWork::new(usize::MAX);
    let legacy = s.normalize_value_with_work(&input, &mut old).unwrap();
    assert_eq!((old.used, old.scanned, old.written), (8, 0, 0));
    assert!(old.operations.is_empty());
    let mut full = CountingWork::new(usize::MAX);
    let result = s
        .normalize_value_complete_with_work(&input, &mut full)
        .unwrap();
    assert_eq!(result.body, legacy.body);
    assert_eq!(result.values, legacy.values);
    assert_eq!(result.required_proofs, legacy.required_proofs);
    assert_eq!((full.scanned, full.written), (54, 78));
    assert!(full.used > old.used);
    struct CopyLimit {
        inner: CountingWork,
        maximum: usize,
    }
    impl ValidationWork for CopyLimit {
        fn visit(&mut self, k: Visit) -> Result<()> {
            self.inner.visit(k)
        }
        fn arithmetic(&mut self, k: Op, v: &[&BigInt]) -> Result<()> {
            self.inner.arithmetic(k, v)
        }
        fn scan_bytes(&mut self, n: usize) -> Result<()> {
            self.inner.scan_bytes(n)
        }
        fn write_bytes(&mut self, n: usize) -> Result<()> {
            if n > self.maximum - self.inner.written {
                return Err(Error::new("TEST_COPY_REFUSED", "pre-copy callback"));
            }
            self.inner.write_bytes(n)
        }
        fn hash_bytes(&mut self, n: usize) -> Result<()> {
            self.inner.hash_bytes(n)
        }
    }
    let mut exact = CopyLimit {
        inner: CountingWork::new(usize::MAX),
        maximum: full.written,
    };
    assert_eq!(
        s.normalize_value_complete_with_work(&input, &mut exact)
            .unwrap()
            .body,
        result.body
    );
    let mut short = CopyLimit {
        inner: CountingWork::new(usize::MAX),
        maximum: full.written - 1,
    };
    error(
        s.normalize_value_complete_with_work(&input, &mut short),
        "TEST_COPY_REFUSED",
    );
    assert!(short.inner.written < full.written);
    let mut semantic_short = CountingWork::new(full.used - 1);
    error(
        s.normalize_value_complete_with_work(&input, &mut semantic_short),
        "TEST_EXHAUSTED",
    );
}

#[test]
fn attested_subset_uses_same_field_type_without_inventing_required_facts() {
    use crate::validation_work::tests::CountingWork;
    let s = schema(json!([
        {"name":"example:attested","required":true,
         "type":{"kind":"enum","values":["example:a"]},
         "provenance":{"kind":"attested","source_slot":"example:source","evidence_kind":"outcome"}},
        field("example:other", true, json!({"kind":"boolean"}))
    ]));
    error(
        s.normalize(br#"{"example:attested":"example:a"}"#),
        "FACT_REQUIRED",
    );
    let value = json!("example:a");
    let mut work = CountingWork::new(usize::MAX);
    let result = s
        .normalize_attested_field_complete_with_work("example:attested", &value, &mut work)
        .unwrap();
    assert_eq!(result, FactValue::Enum("example:a".into()));
    assert_eq!(work.written, "example:a".len());
    let mut exact = CountingWork::new(work.used);
    assert_eq!(
        s.normalize_attested_field_complete_with_work("example:attested", &value, &mut exact)
            .unwrap(),
        result
    );
    let mut short = CountingWork::new(work.used - 1);
    error(
        s.normalize_attested_field_complete_with_work("example:attested", &value, &mut short),
        "TEST_EXHAUSTED",
    );
    error(
        s.normalize_attested_field_complete_with_work(
            "example:other",
            &json!(true),
            &mut NoopValidationWork,
        ),
        "FACT_PROVENANCE",
    );
    error(
        s.normalize_attested_field_complete_with_work(
            "example:unknown",
            &value,
            &mut NoopValidationWork,
        ),
        "FACT_UNKNOWN",
    );
    error(
        s.normalize_attested_field_complete_with_work(
            "example:attested",
            &json!("example:wrong"),
            &mut NoopValidationWork,
        ),
        "FACT_RANGE",
    );
}

#[test]
fn complete_normalization_accounts_attested_vector_relocation_and_scalar_bodies() {
    use crate::validation_work::tests::CountingWork;
    let fields: Vec<_> = (0..6)
        .map(|i| {
            json!({"name":format!("example:f{i}"),"required":true,
        "type":{"kind":"enum","values":["example:a"]},
        "provenance":{"kind":"attested","source_slot":"example:source","evidence_kind":"outcome"}})
        })
        .collect();
    let s = schema(json!(fields));
    let input = Value::Object(
        (0..6)
            .map(|i| (format!("example:f{i}"), json!("example:a")))
            .collect(),
    );
    let mut old = CountingWork::new(usize::MAX);
    let legacy = s.normalize_value_with_work(&input, &mut old).unwrap();
    let mut work = CountingWork::new(usize::MAX);
    let result = s
        .normalize_value_complete_with_work(&input, &mut work)
        .unwrap();
    assert_eq!(result.body, legacy.body);
    assert_eq!(result.required_proofs.len(), 6);
    assert!(work.written >= 4 * std::mem::size_of::<String>() + 6 * std::mem::size_of::<String>());
    let mixed = schema(json!([
        field(
            "example:int",
            true,
            json!({"kind":"integer","min":"0","max":"10","unit":"example:unit"})
        ),
        field(
            "example:time",
            true,
            json!({"kind":"timestamp","min":"2000-01-01T00:00:00Z","max":"2001-01-01T00:00:00Z"})
        ),
        field(
            "example:string",
            true,
            json!({"kind":"string","min_bytes":"1","max_bytes":"32"})
        ),
        field("example:bool", true, json!({"kind":"boolean"}))
    ]));
    let value = json!({"example:int":"0002","example:time":"2000-01-01T01:00:00+01:00","example:string":"hi","example:bool":false});
    let mut old = CountingWork::new(usize::MAX);
    let legacy = mixed.normalize_value_with_work(&value, &mut old).unwrap();
    let mut full = CountingWork::new(usize::MAX);
    let result = mixed
        .normalize_value_complete_with_work(&value, &mut full)
        .unwrap();
    assert_eq!(result.body, legacy.body);
    assert_eq!(result.values, legacy.values);
    assert!(full.written > old.written);
}

#[test]
fn complete_source_schema_preserves_scalar_bytes_and_rejections() {
    let raw = json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"example:all","version":"0001",
        "normalization":"ledger-facts-normalization/1","fields":[
        field("example:time",false,json!({"kind":"timestamp","min":"2020-01-01T00:00:00Z","max":"2030-01-01T00:00:00+00:00"})),
        field("example:string",false,json!({"kind":"string","min_bytes":"00","max_bytes":"001024"})),
        field("example:enum",true,json!({"kind":"enum","values":["example:z","example:a"]})),
        field("example:integer",true,json!({"kind":"integer","min":"-0010","max":"000100","unit":"example:unit"})),
        field("example:decimal",false,json!({"kind":"decimal","min":"-01.20","max":"002.500","max_scale":"0003"})),
        {"name":"example:attested","required":true,"type":{"kind":"boolean"},"provenance":{"kind":"attested","source_slot":"example:source","evidence_kind":"outcome"}}
    ]});
    let bound =
        FactSchema::normalization_payload_bound_with_work(&raw, &mut NoopValidationWork).unwrap();
    assert!(bound > 0);
    let old = FactSchema::from_value_with_work(raw.clone(), &mut NoopValidationWork).unwrap();
    let complete =
        FactSchema::from_value_complete_with_work(raw.clone(), &mut NoopValidationWork).unwrap();
    assert_eq!(
        old.canonical_bytes().unwrap(),
        complete.canonical_bytes().unwrap()
    );
    assert_eq!(
        old.content_digest().unwrap(),
        complete
            .content_digest_complete_with_work(&mut NoopValidationWork)
            .unwrap()
    );
    assert_eq!(
        old.document_id().unwrap(),
        complete
            .document_id_complete_with_work(&mut NoopValidationWork)
            .unwrap()
    );
    let cloned = complete
        .clone_complete_with_work(&mut NoopValidationWork)
        .unwrap();
    assert_eq!(old.canonical_value(), cloned.canonical_value());

    let facts =
        json!({"example:enum":"example:a","example:integer":"0003","example:attested":true});
    assert_eq!(
        old.normalize_value_with_work(&facts, &mut NoopValidationWork)
            .unwrap()
            .canonical_value(),
        complete
            .normalize_value_with_work(&facts, &mut NoopValidationWork)
            .unwrap()
            .canonical_value()
    );
    for mutate in [0, 1, 2] {
        let mut invalid = raw.clone();
        match mutate {
            0 => invalid["fields"][2]["type"]["values"] = json!(["example:a", "example:a"]),
            1 => invalid["fields"][5]["provenance"]["extra"] = json!(true),
            _ => invalid["fields"][4]["type"]["max_scale"] = json!("19"),
        }
        let old =
            FactSchema::from_value_with_work(invalid.clone(), &mut NoopValidationWork).unwrap_err();
        let new = FactSchema::from_value_complete_with_work(invalid, &mut NoopValidationWork)
            .unwrap_err();
        assert_eq!(old.code, new.code);
    }
}
