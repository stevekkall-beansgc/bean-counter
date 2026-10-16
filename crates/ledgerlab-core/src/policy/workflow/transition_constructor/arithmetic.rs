//! One bounded exact parser/rational/rounding path, charged before execution.
use super::meter::{self, Meter};
use crate::{Error, Result};
use num_bigint::BigInt;
use num_traits::{One, ToPrimitive, Zero};
use std::cmp::Ordering;
fn scalar() -> Error {
    Error::new("INVALID_SCALAR", "selected canonical decimal")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::policy::workflow) struct Ratio {
    pub numerator: BigInt,
    pub denominator: BigInt,
}
pub(in crate::policy::workflow) fn integer(
    m: &mut Meter,
    text: &str,
    signed: bool,
) -> Result<BigInt> {
    m.visit("scalar.integer")?;
    if m.check("scalar.empty", || text.is_empty())?
        || meter::cmp(m, &text.len().into(), &64usize.into())?.is_gt()
    {
        return Err(scalar());
    }
    let negative = m.check("scalar.sign", || text.starts_with('-'))?;
    let digits = if negative {
        if m.check("scalar.signed_allowed", || !signed)? {
            return Err(scalar());
        }
        &text[1..]
    } else {
        text
    };
    if m.check("scalar.empty_digits", || digits.is_empty())?
        || (meter::cmp(m, &digits.len().into(), &1usize.into())?.is_gt()
            && m.check("scalar.leading_zero", || digits.starts_with('0'))?)
        || super::shape::same_text(m, text, "-0")?
    {
        return Err(scalar());
    }
    let mut n = BigInt::zero();
    let ten = BigInt::from(10);
    for d in digits.bytes() {
        m.visit("scalar.digit")?;
        if m.check("scalar.digit_valid", || !d.is_ascii_digit())? {
            return Err(scalar());
        }
        let product = meter::mul(m, &n, &ten)?;
        let digit = meter::sub(m, &d.into(), &b'0'.into())?;
        n = meter::add(m, &product, &digit)?;
    }
    if negative { meter::neg(m, &n) } else { Ok(n) }
}
pub(in crate::policy::workflow) fn decimal(m: &mut Meter, text: &str) -> Result<Ratio> {
    m.visit("scalar.decimal")?;
    if m.check("scalar.empty", || text.is_empty())?
        || meter::cmp(m, &text.len().into(), &64usize.into())?.is_gt()
    {
        return Err(scalar());
    }
    let negative = m.check("scalar.sign", || text.starts_with('-'))?;
    let text = if negative { &text[1..] } else { text };
    let mut parts = text.split('.');
    let whole = parts.next().ok_or_else(scalar)?;
    let fraction = parts.next();
    if m.check("scalar.extra_fraction", || parts.next().is_some())?
        || m.check("scalar.empty_whole", || whole.is_empty())?
        || (meter::cmp(m, &whole.len().into(), &1usize.into())?.is_gt()
            && m.check("scalar.leading_zero", || whole.starts_with('0'))?)
        || match fraction {
            Some(f) => {
                m.check("scalar.empty_fraction", || f.is_empty())?
                    || meter::cmp(m, &f.len().into(), &18usize.into())?.is_gt()
            }
            None => {
                m.visit("scalar.no_fraction")?;
                false
            }
        }
    {
        return Err(scalar());
    }
    let mut n = BigInt::zero();
    let mut d = BigInt::one();
    let ten = BigInt::from(10);
    let mut coefficient = BigInt::zero();
    for digit in whole.bytes().chain(fraction.unwrap_or("").bytes()) {
        m.visit("scalar.digit")?;
        if m.check("scalar.digit_valid", || !digit.is_ascii_digit())? {
            return Err(scalar());
        }
        let product = meter::mul(m, &n, &ten)?;
        let digit = meter::sub(m, &digit.into(), &b'0'.into())?;
        n = meter::add(m, &product, &digit)?;
        if !meter::cmp(m, &n, &BigInt::zero())?.is_eq() {
            coefficient = meter::add(m, &coefficient, &BigInt::one())?;
        }
    }
    if meter::cmp(m, &coefficient, &30usize.into())?.is_gt() {
        return Err(scalar());
    }
    if negative && meter::cmp(m, &n, &BigInt::zero())?.is_eq() {
        return Err(scalar());
    }
    if let Some(f) = fraction {
        m.scan_bytes(1)?;
        if m.check("scalar.trailing_zero", || f.ends_with('0'))? {
            return Err(scalar());
        }
    }
    for _ in fraction.unwrap_or("").bytes() {
        m.visit("scalar.scale_digit")?;
        d = meter::mul(m, &d, &ten)?;
    }
    if negative {
        n = meter::neg(m, &n)?;
    }
    Ok(Ratio {
        numerator: n,
        denominator: d,
    })
}
pub(in crate::policy::workflow) fn rational(m: &mut Meter, n: BigInt, d: BigInt) -> Result<Ratio> {
    let zero = BigInt::zero();
    if meter::cmp(m, &d, &zero)? != Ordering::Greater {
        return Err(scalar());
    }
    let mut a = meter::absolute(m, &n)?;
    let mut b = d.clone();
    while meter::cmp(m, &b, &zero)? != Ordering::Equal {
        let (_, r) = meter::divmod(m, &a, &b)?;
        a = b;
        b = r;
    }
    let g = if meter::cmp(m, &a, &zero)? == Ordering::Equal {
        BigInt::one()
    } else {
        a
    };
    let (nn, rn) = meter::divmod(m, &n, &g)?;
    let (dd, rd) = meter::divmod(m, &d, &g)?;
    let nr = meter::cmp(m, &rn, &zero)?;
    let dr = meter::cmp(m, &rd, &zero)?;
    if nr != Ordering::Equal || dr != Ordering::Equal {
        return Err(scalar());
    }
    Ok(Ratio {
        numerator: nn,
        denominator: dd,
    })
}
pub(in crate::policy::workflow) fn multiply(m: &mut Meter, a: &Ratio, b: &Ratio) -> Result<Ratio> {
    let n = meter::mul(m, &a.numerator, &b.numerator)?;
    let d = meter::mul(m, &a.denominator, &b.denominator)?;
    rational(m, n, d)
}
pub(in crate::policy::workflow) fn rounded(
    m: &mut Meter,
    x: &Ratio,
    atom_limit: &BigInt,
) -> Result<i128> {
    let negative = meter::cmp(m, &x.numerator, &BigInt::zero())? == Ordering::Less;
    let abs = meter::absolute(m, &x.numerator)?;
    let (mut q, r) = meter::divmod(m, &abs, &x.denominator)?;
    let twice = meter::mul(m, &r, &BigInt::from(2))?;
    if meter::cmp(m, &twice, &x.denominator)? != Ordering::Less {
        q = meter::add(m, &q, &BigInt::one())?;
    }
    if negative {
        q = meter::neg(m, &q)?;
    }
    let magnitude = meter::absolute(m, &q)?;
    if meter::cmp(m, &magnitude, atom_limit)? == Ordering::Greater {
        return Err(Error::new("AMOUNT_OVERFLOW", "selected atom bound"));
    }
    q.to_i128()
        .ok_or_else(|| Error::new("AMOUNT_OVERFLOW", "selected atom encoding"))
}
