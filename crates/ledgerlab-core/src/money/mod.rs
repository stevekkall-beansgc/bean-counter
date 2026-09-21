//! Bounded decimal and reduced rational arithmetic, with one signed rounding rule.
use crate::{Error, Result};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_ATOMS: i128 = 999_999_999_999_999_999_999_999_999_999;
fn overflow() -> Error {
    Error::new("ARITHMETIC_OVERFLOW", "bounded exact arithmetic")
}
pub fn checked_atoms(n: i128) -> Result<i128> {
    if (-MAX_ATOMS..=MAX_ATOMS).contains(&n) {
        Ok(n)
    } else {
        Err(overflow())
    }
}
pub fn add_atoms(a: i128, b: i128) -> Result<i128> {
    checked_atoms(a.checked_add(b).ok_or_else(overflow)?)
}
pub fn parse_atoms(text: &str) -> Result<i128> {
    canonical_integer(text)?;
    checked_atoms(text.parse().map_err(|_| overflow())?)
}
fn canonical_integer(s: &str) -> Result<()> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || s == "-0"
    {
        return Err(Error::new(
            "INPUT_PRECISION",
            "canonical integer string required",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decimal {
    coefficient: u128,
    scale: u8,
}
impl Decimal {
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() || s.len() > 64 {
            return Err(Error::new("INPUT_PRECISION", "decimal length"));
        }
        let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 18
            || (s.contains('.') && fraction.is_empty())
        {
            return Err(Error::new(
                "INPUT_PRECISION",
                "nonnegative decimal required",
            ));
        }
        let fraction = fraction.trim_end_matches('0');
        let coefficient = format!("{whole}{fraction}");
        let coefficient = coefficient.trim_start_matches('0');
        if coefficient.len() > 30 {
            return Err(Error::new("INPUT_PRECISION", "decimal coefficient"));
        }
        Ok(Self {
            coefficient: if coefficient.is_empty() {
                0
            } else {
                coefficient.parse().map_err(|_| overflow())?
            },
            scale: if coefficient.is_empty() {
                0
            } else {
                fraction.len() as u8
            },
        })
    }
    pub fn is_zero(&self) -> bool {
        self.coefficient == 0
    }
    pub fn ratio(&self) -> ExactRatio {
        ExactRatio {
            numerator: BigInt::from(self.coefficient),
            denominator: BigInt::from(10u128.pow(u32::from(self.scale))),
        }
        .reduced()
    }
    pub fn atoms_exact(&self, scale: u8) -> Result<i128> {
        let ratio = self.atoms_ratio(scale)?;
        if !ratio.denominator.is_one() {
            return Err(Error::new(
                "INPUT_PRECISION",
                "fixed amount not representable at book scale",
            ));
        }
        ratio.round_atoms()
    }
    pub fn atoms_ratio(&self, scale: u8) -> Result<ExactRatio> {
        if scale > 18 {
            return Err(Error::new("INPUT_PRECISION", "book scale"));
        }
        self.ratio()
            .mul(&ExactRatio::integer(10i128.pow(u32::from(scale))))
    }
    pub fn percent(&self) -> Result<ExactRatio> {
        let r = self.ratio();
        if r.numerator > &r.denominator * 100 {
            return Err(Error::new("POLICY_PERCENT_RANGE", "percent exceeds 100"));
        }
        r.div(&ExactRatio::integer(100))
    }
}
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = self.coefficient.to_string();
        let scale = usize::from(self.scale);
        if scale > 0 {
            if s.len() <= scale {
                s = format!("{}{}", "0".repeat(scale + 1 - s.len()), s);
            }
            s.insert(s.len() - scale, '.');
        }
        f.write_str(&s)
    }
}
impl Serialize for Decimal {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactRatio {
    numerator: BigInt,
    denominator: BigInt,
}
impl ExactRatio {
    pub fn integer(n: i128) -> Self {
        Self {
            numerator: BigInt::from(n),
            denominator: BigInt::one(),
        }
    }
    pub fn new(numerator: BigInt, denominator: BigInt) -> Result<Self> {
        if denominator <= BigInt::zero() {
            return Err(Error::new(
                "INPUT_PRECISION",
                "positive denominator required",
            ));
        }
        bounded(&numerator)?;
        bounded(&denominator)?;
        Ok(Self {
            numerator,
            denominator,
        }
        .reduced())
    }
    pub fn from_canonical(n: &str, d: &str) -> Result<Self> {
        canonical_integer(n)?;
        canonical_integer(d)?;
        if n.len() > 156 || d.len() > 155 {
            return Err(overflow());
        }
        let numerator = n.parse().map_err(|_| overflow())?;
        let denominator = d.parse().map_err(|_| overflow())?;
        let result = Self::new(numerator, denominator)?;
        if result.numerator.to_string() != n || result.denominator.to_string() != d {
            return Err(Error::new("INPUT_PRECISION", "ratio must be reduced"));
        }
        Ok(result)
    }
    fn reduced(mut self) -> Self {
        let g = self.numerator.gcd(&self.denominator);
        self.numerator /= &g;
        self.denominator /= g;
        self
    }
    pub fn is_positive(&self) -> bool {
        self.numerator.is_positive()
    }
    pub fn negated(&self) -> Self {
        Self {
            numerator: -&self.numerator,
            denominator: self.denominator.clone(),
        }
    }
    pub fn mul(&self, other: &Self) -> Result<Self> {
        let g1 = self.numerator.gcd(&other.denominator);
        let g2 = other.numerator.gcd(&self.denominator);
        let n = (&self.numerator / g1.clone()) * (&other.numerator / g2.clone());
        let d = (&self.denominator / g2) * (&other.denominator / g1);
        Self::new(n, d)
    }
    pub fn div(&self, other: &Self) -> Result<Self> {
        if other.numerator.is_zero() {
            return Err(Error::new("INPUT_PRECISION", "division by zero"));
        }
        let reciprocal = Self {
            numerator: if other.numerator.is_negative() {
                -&other.denominator
            } else {
                other.denominator.clone()
            },
            denominator: other.numerator.abs(),
        };
        self.mul(&reciprocal)
    }
    pub fn add(&self, other: &Self) -> Result<Self> {
        let g = self.denominator.gcd(&other.denominator);
        let left = &self.numerator * (&other.denominator / &g);
        bounded(&left)?;
        let right = &other.numerator * (&self.denominator / &g);
        bounded(&right)?;
        let numerator = left + right;
        bounded(&numerator)?;
        let denominator = (&self.denominator / &g) * &other.denominator;
        bounded(&denominator)?;
        Self::new(numerator, denominator)
    }
    pub fn round_atoms(&self) -> Result<i128> {
        let (mut q, r) = self.numerator.abs().div_rem(&self.denominator);
        // r >= d-r is 2*r >= d, without introducing a 513-bit temporary.
        if r >= &self.denominator - &r {
            q += 1;
            bounded(&q)?;
        }
        if self.numerator.is_negative() {
            q = -q;
        }
        checked_atoms(q.to_i128().ok_or_else(overflow)?)
    }
}
fn bounded(n: &BigInt) -> Result<()> {
    if n.bits() > 512 {
        Err(overflow())
    } else {
        Ok(())
    }
}
impl Serialize for ExactRatio {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Repr {
            numerator: String,
            denominator: String,
        }
        Repr {
            numerator: self.numerator.to_string(),
            denominator: self.denominator.to_string(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for ExactRatio {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            numerator: String,
            denominator: String,
        }
        let r = Repr::deserialize(d)?;
        Self::from_canonical(&r.numerator, &r.denominator).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Money {
    currency: String,
    scale: u8,
    #[serde(serialize_with = "atom_string")]
    atoms: i128,
}
fn atom_string<S: serde::Serializer>(n: &i128, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&n.to_string())
}
impl Money {
    pub fn new(currency: &str, scale: u8, atoms: i128) -> Result<Self> {
        validate_currency(currency, scale)?;
        Ok(Self {
            currency: currency.into(),
            scale,
            atoms: checked_atoms(atoms)?,
        })
    }
    pub fn atoms(&self) -> i128 {
        self.atoms
    }
    pub fn currency(&self) -> &str {
        &self.currency
    }
    pub fn scale(&self) -> u8 {
        self.scale
    }
    pub fn checked_add(&self, other: &Self) -> Result<Self> {
        if self.currency != other.currency || self.scale != other.scale {
            return Err(Error::new(
                "CURRENCY_MISMATCH",
                "currency and scale must agree",
            ));
        }
        Self::new(
            &self.currency,
            self.scale,
            add_atoms(self.atoms, other.atoms)?,
        )
    }
}
pub(crate) fn validate_currency(currency: &str, scale: u8) -> Result<()> {
    if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_uppercase()) || scale > 18 {
        return Err(Error::new("CURRENCY_MISMATCH", "currency or scale"));
    }
    Ok(())
}
impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            currency: String,
            scale: u8,
            atoms: String,
        }
        let r = Repr::deserialize(d)?;
        parse_atoms(&r.atoms)
            .and_then(|n| Self::new(&r.currency, r.scale, n))
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
