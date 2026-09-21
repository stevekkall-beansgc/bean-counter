use crate::domain::Revision;
use crate::money::{add_atoms, parse_atoms, Decimal, ExactRatio, Money, MAX_ATOMS};
use num_bigint::BigInt;
use serde_json::json;

#[test]
fn signed_rounding_matches_frozen_tie_and_fraction_oracles() {
    let cases = [("1.005", 101), ("1.0049", 100), ("0.004", 0), ("0.012", 1)];
    for (decimal, atoms) in cases {
        let ratio = Decimal::parse(decimal).unwrap().atoms_ratio(2).unwrap();
        assert_eq!(ratio.round_atoms().unwrap(), atoms);
        assert_eq!(ratio.negated().round_atoms().unwrap(), -atoms);
    }
    let unit = Decimal::parse("0.07")
        .unwrap()
        .atoms_ratio(2)
        .unwrap()
        .mul(&Decimal::parse("1.5").unwrap().ratio())
        .unwrap();
    assert_eq!(unit.round_atoms().unwrap(), 11);
    let base = ExactRatio::integer(100);
    let discount = base
        .mul(&Decimal::parse("20").unwrap().percent().unwrap())
        .unwrap()
        .negated()
        .round_atoms()
        .unwrap();
    assert_eq!(discount, -20);
    assert_eq!(add_atoms(100, discount).unwrap(), 80);
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../fixtures/math/vectors.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(
        vectors.iter().find(|v| v["name"] == "first_slice").unwrap()["expected"],
        json!([100, discount, add_atoms(100, discount).unwrap()])
    );
    assert_eq!(
        vectors
            .iter()
            .find(|v| v["name"] == "unit_fraction")
            .unwrap()["expected"],
        json!(unit.round_atoms().unwrap())
    );
}
#[test]
fn normalization_checks_raw_precision_before_trimming() {
    for (raw, expected) in [
        ("00001.000", "1"),
        ("0000.0000", "0"),
        ("000.0100", "0.01"),
        (
            "100000000000000000000000000000.0",
            "100000000000000000000000000000",
        ),
    ] {
        assert_eq!(Decimal::parse(raw).unwrap().to_string(), expected);
    }
    for raw in [
        "-0",
        "-1",
        "+1",
        " 1",
        "1 ",
        "1e0",
        ".1",
        "1.",
        "1.2.3",
        "1,000",
        "1.0000000000000000000",
        "1000000000000000000000000000000",
    ] {
        assert!(Decimal::parse(raw).is_err(), "{raw}");
    }
    assert!(Decimal::parse(&"0".repeat(65)).is_err());
    assert!(Decimal::parse("1.001").unwrap().atoms_exact(2).is_err());
    assert!(Decimal::parse("100.000000000000000001")
        .unwrap()
        .percent()
        .is_err());
    assert_eq!(
        Decimal::parse("100").unwrap().percent().unwrap(),
        ExactRatio::integer(1)
    );
}
#[test]
fn booked_atoms_and_totals_never_wrap_or_clamp() {
    assert_eq!(parse_atoms(&MAX_ATOMS.to_string()).unwrap(), MAX_ATOMS);
    assert_eq!(parse_atoms(&(-MAX_ATOMS).to_string()).unwrap(), -MAX_ATOMS);
    assert!(add_atoms(MAX_ATOMS, 1).is_err());
    assert!(add_atoms(-MAX_ATOMS, -1).is_err());
    assert!(add_atoms(i128::MAX, 1).is_err());
    assert!(parse_atoms("1000000000000000000000000000000").is_err());
    for raw in ["-0", "00", "+1", "01", "-01", "1.0", "1e0"] {
        assert!(parse_atoms(raw).is_err());
    }
    assert!(ExactRatio::integer(MAX_ATOMS)
        .add(&ExactRatio::from_canonical("1", "2").unwrap())
        .unwrap()
        .round_atoms()
        .is_err());
    assert!(Money::new("USD", 2, MAX_ATOMS)
        .unwrap()
        .checked_add(&Money::new("USD", 2, 1).unwrap())
        .is_err());
    assert!(Money::new("USD", 2, 1)
        .unwrap()
        .checked_add(&Money::new("EUR", 2, 1).unwrap())
        .is_err());
    assert!(Money::new("USD", 2, 1)
        .unwrap()
        .checked_add(&Money::new("USD", 3, 1).unwrap())
        .is_err());
}
#[test]
fn ratio_wire_requires_reduced_positive_denominator_and_unique_zero() {
    for (n, d) in [
        ("2", "4"),
        ("0", "2"),
        ("1", "0"),
        ("1", "-1"),
        ("-0", "1"),
        ("01", "1"),
    ] {
        assert!(ExactRatio::from_canonical(n, d).is_err());
    }
    assert_eq!(
        serde_json::to_value(ExactRatio::from_canonical("0", "1").unwrap()).unwrap(),
        json!({"numerator":"0","denominator":"1"})
    );
    assert!(serde_json::from_value::<ExactRatio>(
        json!({"numerator":"1","denominator":"2","extra":0})
    )
    .is_err());
}
#[test]
fn five_hundred_twelve_bit_intermediates_are_bounded_after_cancellation() {
    let max: BigInt = (BigInt::from(1u8) << 512usize) - 1;
    let huge = ExactRatio::new(max.clone(), BigInt::from(1)).unwrap();
    assert!(huge.mul(&ExactRatio::integer(2)).is_err());
    assert!(huge.add(&ExactRatio::integer(1)).is_err());
    assert!(ExactRatio::new(BigInt::from(1) << 512usize, BigInt::from(1)).is_err());
    let inverse = ExactRatio::new(BigInt::from(1), max.clone()).unwrap();
    assert_eq!(huge.mul(&inverse).unwrap(), ExactRatio::integer(1));
    let nearly_one = ExactRatio::new(max.clone() - 1, max).unwrap();
    assert_eq!(nearly_one.round_atoms().unwrap(), 1);
    assert_eq!(nearly_one.negated().round_atoms().unwrap(), -1);
    assert!(huge.div(&ExactRatio::integer(0)).is_err());
}
#[test]
fn revision_bound_is_distinct_from_safe_json_numbers() {
    let max = Revision::new(i64::MAX as u64).unwrap();
    assert!(max.next().is_err());
    assert_eq!(
        serde_json::to_value(max).unwrap(),
        json!("9223372036854775807")
    );
    for v in ["9223372036854775808", "01", "-1", "+1"] {
        assert!(Revision::parse(v).is_err());
    }
}
