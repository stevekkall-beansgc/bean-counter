//! Fallible work callbacks for executing pure validators. No callback grants
//! authority or changes a validator's accepted language.
pub(crate) mod economic_meter;
mod original_audit;
pub use original_audit::{OriginalAuditSession, OriginalAuditUsage};

use crate::Result;
// Same existing operand type across the pure/facade callback boundary.
pub use num_bigint::BigInt;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationVisit {
    Value,
    Field,
    Element,
    Constraint,
    ScalarDigit,
    MemberCompare,
    RefLookup,
    Edge,
    SortMove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticOperation {
    Add,
    Subtract,
    Multiply,
    DivideRemainder,
    Compare,
    Negate,
    Absolute,
}

pub trait ValidationWork {
    fn visit(&mut self, kind: ValidationVisit) -> Result<()>;
    fn arithmetic(&mut self, kind: ArithmeticOperation, operands: &[&BigInt]) -> Result<()>;
    fn scan_bytes(&mut self, bytes: usize) -> Result<()>;
    fn write_bytes(&mut self, bytes: usize) -> Result<()>;
    fn hash_bytes(&mut self, bytes: usize) -> Result<()>;
}

/// Compatibility adapter. Existing validation still runs in full.
pub struct NoopValidationWork;
impl ValidationWork for NoopValidationWork {
    fn visit(&mut self, _: ValidationVisit) -> Result<()> {
        Ok(())
    }
    fn arithmetic(&mut self, _: ArithmeticOperation, _: &[&BigInt]) -> Result<()> {
        Ok(())
    }
    fn scan_bytes(&mut self, _: usize) -> Result<()> {
        Ok(())
    }
    fn write_bytes(&mut self, _: usize) -> Result<()> {
        Ok(())
    }
    fn hash_bytes(&mut self, _: usize) -> Result<()> {
        Ok(())
    }
}

/// Stable insertion sort: exhaustion aborts before the compared operation or
/// swap. The partially ordered local slice must be discarded on failure.
pub fn try_sort_by<T>(
    values: &mut [T],
    work: &mut dyn ValidationWork,
    mut compare: impl FnMut(&T, &T, &mut dyn ValidationWork) -> Result<Ordering>,
) -> Result<()> {
    for i in 1..values.len() {
        let mut j = i;
        while j > 0 {
            if compare(&values[j - 1], &values[j], work)? != Ordering::Greater {
                break;
            }
            work.visit(ValidationVisit::SortMove)?;
            values.swap(j - 1, j);
            j -= 1;
        }
    }
    Ok(())
}

pub(crate) fn compare(
    work: &mut dyn ValidationWork,
    left: impl Into<BigInt>,
    right: impl Into<BigInt>,
) -> Result<Ordering> {
    let (left, right) = (left.into(), right.into());
    work.arithmetic(ArithmeticOperation::Compare, &[&left, &right])?;
    Ok(left.cmp(&right))
}

pub(crate) fn i64_operation(
    work: &mut dyn ValidationWork,
    kind: ArithmeticOperation,
    left: i64,
    right: i64,
) -> Result<i64> {
    work.arithmetic(kind, &[&BigInt::from(left), &BigInt::from(right)])?;
    Ok(match kind {
        ArithmeticOperation::Add => left + right,
        ArithmeticOperation::Subtract => left - right,
        ArithmeticOperation::Multiply => left * right,
        ArithmeticOperation::DivideRemainder => left / right,
        _ => unreachable!("binary calendar operation"),
    })
}

pub(crate) fn i64_remainder(work: &mut dyn ValidationWork, left: i64, right: i64) -> Result<i64> {
    work.arithmetic(
        ArithmeticOperation::DivideRemainder,
        &[&BigInt::from(left), &BigInt::from(right)],
    )?;
    Ok(left % right)
}

pub(crate) fn usize_add(work: &mut dyn ValidationWork, left: usize, right: usize) -> Result<usize> {
    work.arithmetic(
        ArithmeticOperation::Add,
        &[&BigInt::from(left), &BigInt::from(right)],
    )?;
    Ok(left + right)
}

pub(crate) fn usize_sub(work: &mut dyn ValidationWork, left: usize, right: usize) -> Result<usize> {
    work.arithmetic(
        ArithmeticOperation::Subtract,
        &[&BigInt::from(left), &BigInt::from(right)],
    )?;
    Ok(left - right)
}

pub(crate) fn power_ten(work: &mut dyn ValidationWork, scale: u8) -> Result<BigInt> {
    let mut value = BigInt::from(1);
    let ten = BigInt::from(10);
    for _ in 0..scale {
        work.arithmetic(ArithmeticOperation::Multiply, &[&value, &ten])?;
        value *= &ten;
    }
    Ok(value)
}

/// Decimal text construction without an opaque numeric formatter.
pub(crate) fn unsigned_string(mut value: u128, work: &mut dyn ValidationWork) -> Result<String> {
    let mut digits = Vec::new();
    loop {
        work.arithmetic(
            ArithmeticOperation::DivideRemainder,
            &[&BigInt::from(value), &BigInt::from(10)],
        )?;
        let digit = value % 10;
        work.arithmetic(
            ArithmeticOperation::Add,
            &[&BigInt::from(digit), &BigInt::from(b'0')],
        )?;
        let byte = digit as u8 + b'0';
        work.write_bytes(1)?;
        digits.push(byte);
        work.arithmetic(
            ArithmeticOperation::DivideRemainder,
            &[&BigInt::from(value), &BigInt::from(10)],
        )?;
        value /= 10;
        if compare(work, value, 0u128)? == Ordering::Equal {
            break;
        }
    }
    work.write_bytes(digits.len())?;
    let bytes = digits.into_iter().rev().collect();
    Ok(String::from_utf8(bytes).expect("decimal ASCII digits"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Error;

    pub(crate) struct CountingWork {
        pub used: usize,
        pub maximum: usize,
        pub operations: Vec<(ArithmeticOperation, Vec<BigInt>)>,
        pub visits: Vec<ValidationVisit>,
        pub scanned: usize,
        pub written: usize,
        pub hashed: usize,
    }
    impl CountingWork {
        pub fn new(maximum: usize) -> Self {
            Self {
                used: 0,
                maximum,
                operations: Vec::new(),
                visits: Vec::new(),
                scanned: 0,
                written: 0,
                hashed: 0,
            }
        }
        fn charge(&mut self, amount: usize) -> Result<()> {
            if amount > self.maximum - self.used {
                return Err(Error::new("TEST_EXHAUSTED", "executing validator callback"));
            }
            self.used += amount;
            Ok(())
        }
    }
    impl ValidationWork for CountingWork {
        fn visit(&mut self, kind: ValidationVisit) -> Result<()> {
            self.charge(1)?;
            self.visits.push(kind);
            Ok(())
        }
        fn arithmetic(&mut self, kind: ArithmeticOperation, operands: &[&BigInt]) -> Result<()> {
            let cost = 1 + operands
                .iter()
                .map(|operand| operand.bits().div_ceil(64).max(1) as usize)
                .sum::<usize>();
            self.charge(cost)?;
            self.operations
                .push((kind, operands.iter().map(|n| (*n).clone()).collect()));
            Ok(())
        }
        fn scan_bytes(&mut self, n: usize) -> Result<()> {
            self.scanned += n;
            Ok(())
        }
        fn write_bytes(&mut self, n: usize) -> Result<()> {
            self.written += n;
            Ok(())
        }
        fn hash_bytes(&mut self, bytes: usize) -> Result<()> {
            self.hashed += bytes;
            Ok(())
        }
    }

    #[test]
    fn fallible_sort_stops_at_failed_comparison_without_a_move() {
        let mut values = vec![3, 2, 1];
        let mut work = CountingWork::new(3);
        let error = try_sort_by(&mut values, &mut work, |a, b, w| {
            w.visit(ValidationVisit::MemberCompare)?;
            Ok(a.cmp(b))
        })
        .unwrap_err();
        assert_eq!(error.code, "TEST_EXHAUSTED");
        assert_eq!(values, [2, 3, 1]);
        assert_eq!(
            work.visits,
            [
                ValidationVisit::MemberCompare,
                ValidationVisit::SortMove,
                ValidationVisit::MemberCompare
            ]
        );
        let mut values = vec![3, 2, 1];
        try_sort_by(&mut values, &mut NoopValidationWork, |a, b, _| Ok(a.cmp(b))).unwrap();
        assert_eq!(values, [1, 2, 3]);
    }
}
