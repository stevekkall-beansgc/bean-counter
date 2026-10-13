use crate::validation_work::{
    self as vw, ArithmeticOperation as Op, NoopValidationWork, ValidationVisit as Visit,
    ValidationWork,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
/// Checked Gregorian UTC microseconds; parsing never reads a clock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp {
    canonical: String,
    micros: i64,
}

fn fail() -> Error {
    Error::new(
        "TIMESTAMP",
        "expected Gregorian timestamp with explicit offset and at most six fractional digits",
    )
}
fn add(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<i64> {
    vw::i64_operation(w, Op::Add, a, b)
}
fn sub(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<i64> {
    vw::i64_operation(w, Op::Subtract, a, b)
}
fn mul(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<i64> {
    vw::i64_operation(w, Op::Multiply, a, b)
}
fn div(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<i64> {
    vw::i64_operation(w, Op::DivideRemainder, a, b)
}
fn rem(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<i64> {
    vw::i64_remainder(w, a, b)
}
fn lt(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<bool> {
    Ok(vw::compare(w, a, b)? == Ordering::Less)
}
fn le(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<bool> {
    Ok(vw::compare(w, a, b)? != Ordering::Greater)
}
fn eq(w: &mut dyn ValidationWork, a: i64, b: i64) -> Result<bool> {
    Ok(vw::compare(w, a, b)? == Ordering::Equal)
}
fn number(s: &str, w: &mut dyn ValidationWork) -> Result<i64> {
    let mut value = 0;
    for b in s.bytes() {
        w.scan_bytes(1)?;
        w.visit(Visit::ScalarDigit)?;
        if !b.is_ascii_digit() {
            return Err(fail());
        }
        value = mul(w, value, 10)?;
        let digit = sub(w, i64::from(b), i64::from(b'0'))?;
        value = add(w, value, digit)?;
    }
    Ok(value)
}
fn days_before_year(y: i64, w: &mut dyn ValidationWork) -> Result<i64> {
    let y = sub(w, y, 1)?;
    let mut days = mul(w, y, 365)?;
    let leap4 = div(w, y, 4)?;
    days = add(w, days, leap4)?;
    let leap100 = div(w, y, 100)?;
    days = sub(w, days, leap100)?;
    let leap400 = div(w, y, 400)?;
    add(w, days, leap400)
}
fn month_days(y: i64, m: i64, w: &mut dyn ValidationWork) -> Result<i64> {
    w.visit(Visit::Constraint)?;
    match m {
        4 | 6 | 9 | 11 => Ok(30),
        2 => {
            let r4 = rem(w, y, 4)?;
            let leap = if eq(w, r4, 0)? {
                let r100 = rem(w, y, 100)?;
                if !eq(w, r100, 0)? {
                    true
                } else {
                    let r400 = rem(w, y, 400)?;
                    eq(w, r400, 0)?
                }
            } else {
                false
            };
            Ok(if leap { 29 } else { 28 })
        }
        _ => Ok(31),
    }
}
fn render(day: i64, within: i64, fraction: i64, w: &mut dyn ValidationWork) -> Result<String> {
    let year_guess = div(w, day, 366)?;
    let year_guess = add(w, year_guess, 1)?;
    let mut year = if lt(w, year_guess, 9999)? {
        year_guess
    } else {
        9999
    };
    loop {
        let next = add(w, year, 1)?;
        let next_day = days_before_year(next, w)?;
        if !le(w, next_day, day)? {
            break;
        }
        year = add(w, year, 1)?;
    }
    let year_day = days_before_year(year, w)?;
    let mut rest = sub(w, day, year_day)?;
    let mut month = 1;
    loop {
        let days = month_days(year, month, w)?;
        if lt(w, rest, days)? {
            break;
        }
        // Keep both actual calls from the existing calendar loop visible.
        let days = month_days(year, month, w)?;
        rest = sub(w, rest, days)?;
        month = add(w, month, 1)?;
    }
    let date = add(w, rest, 1)?;
    let hour = div(w, within, 3600)?;
    let minute = div(w, within, 60)?;
    let minute = rem(w, minute, 60)?;
    let second = rem(w, within, 60)?;
    // Rendering has a fixed ASCII width after all numeric range checks.
    w.write_bytes(27)?;
    Ok(format!(
        "{year:04}-{month:02}-{date:02}T{hour:02}:{minute:02}:{second:02}.{fraction:06}Z"
    ))
}
impl Timestamp {
    pub fn from_micros(micros: i64) -> Result<Self> {
        Self::from_micros_with_work(micros, &mut NoopValidationWork)
    }
    pub fn from_micros_with_work(micros: i64, w: &mut dyn ValidationWork) -> Result<Self> {
        w.arithmetic(
            Op::DivideRemainder,
            &[&BigInt::from(micros), &BigInt::from(1_000_000)],
        )?;
        let seconds = micros.div_euclid(1_000_000);
        w.arithmetic(
            Op::DivideRemainder,
            &[&BigInt::from(micros), &BigInt::from(1_000_000)],
        )?;
        let fraction = micros.rem_euclid(1_000_000);
        w.arithmetic(
            Op::DivideRemainder,
            &[&BigInt::from(seconds), &BigInt::from(86_400)],
        )?;
        let day = seconds.div_euclid(86_400);
        let epoch = days_before_year(1970, w)?;
        let day = add(w, day, epoch)?;
        w.arithmetic(
            Op::DivideRemainder,
            &[&BigInt::from(seconds), &BigInt::from(86_400)],
        )?;
        let within = seconds.rem_euclid(86_400);
        let outside = if lt(w, day, 0)? {
            true
        } else {
            let max_day = days_before_year(10000, w)?;
            !lt(w, day, max_day)?
        };
        if outside {
            return Err(Error::new(
                "TIMESTAMP",
                "timestamp is outside year 0001..9999",
            ));
        }
        let rendered = render(day, within, fraction, w)?;
        Self::parse_with_work(&rendered, w)
    }
    pub fn parse(s: &str) -> Result<Self> {
        Self::parse_with_work(s, &mut NoopValidationWork)
    }
    pub fn parse_with_work(s: &str, w: &mut dyn ValidationWork) -> Result<Self> {
        for b in s.bytes() {
            w.scan_bytes(1)?;
            w.visit(Visit::Constraint)?;
            if !b.is_ascii() {
                return Err(fail());
            }
        }
        w.visit(Visit::Constraint)?;
        if lt(w, s.len() as i64, 20)? {
            return Err(fail());
        }
        for (start, expected) in [(4, "-"), (7, "-"), (10, "T"), (13, ":"), (16, ":")] {
            w.visit(Visit::Constraint)?;
            if s.get(start..start + 1) != Some(expected) {
                return Err(fail());
            }
        }
        let y = number(&s[0..4], w)?;
        let m = number(&s[5..7], w)?;
        let d = number(&s[8..10], w)?;
        let h = number(&s[11..13], w)?;
        let min = number(&s[14..16], w)?;
        let sec = number(&s[17..19], w)?;
        if lt(w, y, 1)? || lt(w, 9999, y)? || lt(w, m, 1)? || lt(w, 12, m)? || lt(w, d, 1)? {
            return Err(fail());
        }
        let days_in_month = month_days(y, m, w)?;
        if lt(w, days_in_month, d)? || lt(w, 23, h)? || lt(w, 59, min)? || lt(w, 59, sec)? {
            return Err(fail());
        }
        let tail = &s[19..];
        w.visit(Visit::Constraint)?;
        let (fraction, offset) = if let Some(t) = tail.strip_prefix('.') {
            let mut n = 0usize;
            for b in t.bytes() {
                w.scan_bytes(1)?;
                w.visit(Visit::ScalarDigit)?;
                if !b.is_ascii_digit() {
                    break;
                }
                n = vw::usize_add(w, n, 1)?;
            }
            if eq(w, n as i64, 0)? || lt(w, 6, n as i64)? {
                return Err(fail());
            }
            (&t[..n], &t[n..])
        } else {
            ("", tail)
        };
        w.visit(Visit::Constraint)?;
        let frac = if fraction.is_empty() {
            0
        } else {
            let value = number(fraction, w)?;
            let scale = sub(w, 6, fraction.len() as i64)?;
            let mut power = 1;
            for _ in 0..scale {
                power = mul(w, power, 10)?;
            }
            mul(w, value, power)?
        };
        w.visit(Visit::MemberCompare)?;
        let shift = if offset == "Z" {
            0
        } else {
            w.visit(Visit::Constraint)?;
            if !eq(w, offset.len() as i64, 6)?
                || !matches!(offset.as_bytes()[0], b'+' | b'-')
                || &offset[3..4] != ":"
            {
                return Err(fail());
            }
            let oh = number(&offset[1..3], w)?;
            let om = number(&offset[4..6], w)?;
            if lt(w, 23, oh)? || lt(w, 59, om)? {
                return Err(fail());
            }
            let hours = mul(w, oh, 60)?;
            let total = add(w, hours, om)?;
            w.visit(Visit::Constraint)?;
            mul(w, total, if offset.starts_with('-') { -1 } else { 1 })?
        };
        let mut month_sum = 0;
        let mut mm = 1;
        while lt(w, mm, m)? {
            let days = month_days(y, mm, w)?;
            month_sum = add(w, month_sum, days)?;
            mm = add(w, mm, 1)?;
        }
        let year_days = days_before_year(y, w)?;
        let days = add(w, year_days, month_sum)?;
        let days = add(w, days, d)?;
        let days = sub(w, days, 1)?;
        let mut seconds = mul(w, days, 86400)?;
        let hours = mul(w, h, 3600)?;
        seconds = add(w, seconds, hours)?;
        let minutes = mul(w, min, 60)?;
        seconds = add(w, seconds, minutes)?;
        seconds = add(w, seconds, sec)?;
        let shift = mul(w, shift, 60)?;
        seconds = sub(w, seconds, shift)?;
        let max_day = days_before_year(10000, w)?;
        let max_seconds = mul(w, max_day, 86400)?;
        if lt(w, seconds, 0)? || !lt(w, seconds, max_seconds)? {
            return Err(fail());
        }
        let day = div(w, seconds, 86400)?;
        let within = rem(w, seconds, 86400)?;
        let canonical = render(day, within, frac, w)?;
        let epoch = days_before_year(1970, w)?;
        let epoch = mul(w, epoch, 86400)?;
        let micros = sub(w, seconds, epoch)?;
        let micros = mul(w, micros, 1_000_000)?;
        let micros = add(w, micros, frac)?;
        Ok(Self { canonical, micros })
    }
    pub fn micros(&self) -> i64 {
        self.micros
    }
    pub fn as_str(&self) -> &str {
        &self.canonical
    }
}
impl TryFrom<String> for Timestamp {
    type Error = Error;
    fn try_from(v: String) -> Result<Self> {
        Self::parse(&v)
    }
}
impl From<Timestamp> for String {
    fn from(v: Timestamp) -> Self {
        v.canonical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micros_round_trip_across_supported_utc_range() {
        let cases = [
            (0, "1970-01-01T00:00:00.000000Z"),
            (-1, "1969-12-31T23:59:59.999999Z"),
            (-62_135_596_800_000_000, "0001-01-01T00:00:00.000000Z"),
            (253_402_300_799_999_999, "9999-12-31T23:59:59.999999Z"),
        ];
        for (micros, expected) in cases {
            let timestamp = Timestamp::from_micros(micros).unwrap();
            assert_eq!(timestamp.as_str(), expected);
            assert_eq!(timestamp.micros(), micros);
        }
    }

    #[test]
    fn micros_refuses_values_outside_supported_utc_range() {
        assert!(Timestamp::from_micros(-62_135_596_800_000_001).is_err());
        assert!(Timestamp::from_micros(253_402_300_800_000_000).is_err());
    }

    #[test]
    fn executing_calendar_exact_budget_and_first_failure_preserve_errors() {
        use crate::validation_work::tests::CountingWork;
        for (raw, expected) in [
            (
                "2000-03-01T00:15:00.12+01:00",
                "2000-02-29T23:15:00.120000Z",
            ),
            ("1900-03-01T00:15:00-01:00", "1900-03-01T01:15:00.000000Z"),
            ("0001-01-01T00:00:00Z", "0001-01-01T00:00:00.000000Z"),
            ("9999-12-31T23:59:59.999999Z", "9999-12-31T23:59:59.999999Z"),
        ] {
            let mut probe = CountingWork::new(usize::MAX);
            let metered = Timestamp::parse_with_work(raw, &mut probe).unwrap();
            assert_eq!(metered.as_str(), expected);
            assert_eq!(metered, Timestamp::parse(raw).unwrap());
            assert!(probe
                .operations
                .iter()
                .any(|(kind, operands)| *kind == Op::DivideRemainder
                    && operands[1] == BigInt::from(400)));
            let mut exact = CountingWork::new(probe.used);
            assert_eq!(
                Timestamp::parse_with_work(raw, &mut exact).unwrap(),
                metered
            );
            assert_eq!(exact.operations, probe.operations);
            let mut short = CountingWork::new(probe.used - 1);
            assert_eq!(
                Timestamp::parse_with_work(raw, &mut short)
                    .unwrap_err()
                    .code,
                "TEST_EXHAUSTED"
            );
        }
        let mut exhausted = CountingWork::new(0);
        assert_eq!(
            Timestamp::parse_with_work("not a timestamp", &mut exhausted)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
        assert_eq!(
            Timestamp::parse("not a timestamp").unwrap_err().code,
            "TIMESTAMP"
        );
    }

    #[test]
    fn executing_micros_conversion_charges_negative_euclidean_operands() {
        use crate::validation_work::tests::CountingWork;
        let mut work = CountingWork::new(usize::MAX);
        assert_eq!(
            Timestamp::from_micros_with_work(-1, &mut work)
                .unwrap()
                .as_str(),
            "1969-12-31T23:59:59.999999Z"
        );
        assert_eq!(
            work.operations[0],
            (
                Op::DivideRemainder,
                vec![BigInt::from(-1), BigInt::from(1_000_000)]
            )
        );
        let mut short = CountingWork::new(work.used - 1);
        assert_eq!(
            Timestamp::from_micros_with_work(-1, &mut short)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
    }
}
