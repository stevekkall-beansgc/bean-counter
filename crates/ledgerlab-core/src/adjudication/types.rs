//! Checked R3 scalar encodings. Legacy `domain::Revision` remains unchanged.
use super::{ensure, Validate};
use crate::{money, Error, Result};
use serde::{Deserialize, Serialize};

macro_rules! string_type {
    ($name:ident, $check:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self> {
                ($check)(value)?;
                Ok(Self(value.into()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = Error;
            fn try_from(value: String) -> Result<Self> {
                Self::parse(&value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl Validate for $name {
            fn validate(&self) -> Result<()> {
                Ok(())
            }
        }
    };
}
fn text(value: &str, max: usize) -> Result<()> {
    ensure(
        !value.is_empty()
            && value.len() <= max
            && !value.chars().any(|c| (c as u32) < 32 || c == '\u{7f}'),
        "IDENTIFIER",
        "bounded R3 UTF-8 text",
    )
}
pub(super) fn is_hex(value: &str, n: usize) -> bool {
    value.len() == n
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
string_type!(Id, |v: &str| text(v, 128));
string_type!(Source, |v: &str| text(v, 256));
// Unstructured text is still bounded by its enclosing canonical envelope.
string_type!(Text, |v: &str| ensure(
    !v.is_empty() && v.len() <= 4096,
    "LIMIT",
    "text bytes"
));
string_type!(Digest, |v: &str| ensure(
    is_hex(v, 64),
    "DIGEST",
    "lowercase SHA-256"
));
string_type!(Time, |v: &str| {
    let time = crate::domain::Timestamp::parse(v)?;
    ensure(
        time.as_str() == v,
        "TIMESTAMP",
        "canonical UTC microseconds",
    )
});

/// A consumed or reserved value in [0, 10^30-1], encoded as a decimal string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Count(u128);
impl Count {
    pub const MAX: u128 = money::MAX_ATOMS as u128;
    pub const ZERO: Self = Self(0);
    pub fn new(n: u128) -> Result<Self> {
        ensure(n <= Self::MAX, "COUNTER_EXHAUSTED", "R3 counter range")?;
        Ok(Self(n))
    }
    pub fn parse(v: &str) -> Result<Self> {
        ensure(
            !v.is_empty()
                && v.len() <= 30
                && v.bytes().all(|b| b.is_ascii_digit())
                && (v == "0" || !v.starts_with('0')),
            "COUNTER",
            "canonical unsigned decimal",
        )?;
        Self::new(v.parse().map_err(|_| Error::new("COUNTER", "decimal"))?)
    }
    pub fn value(self) -> u128 {
        self.0
    }
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        Self::new(
            self.0
                .checked_add(rhs.0)
                .ok_or_else(|| Error::new("COUNTER_EXHAUSTED", "addition"))?,
        )
    }
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        Self::new(
            self.0
                .checked_sub(rhs.0)
                .ok_or_else(|| Error::new("COUNTER_EXHAUSTED", "subtraction"))?,
        )
    }
    pub fn checked_mul(self, rhs: Self) -> Result<Self> {
        Self::new(
            self.0
                .checked_mul(rhs.0)
                .ok_or_else(|| Error::new("COUNTER_EXHAUSTED", "multiplication"))?,
        )
    }
}
impl TryFrom<String> for Count {
    type Error = Error;
    fn try_from(v: String) -> Result<Self> {
        Self::parse(&v)
    }
}
impl From<Count> for String {
    fn from(v: Count) -> Self {
        v.0.to_string()
    }
}
impl Validate for Count {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Atoms(i128);
impl Atoms {
    pub fn new(n: i128) -> Result<Self> {
        Ok(Self(money::checked_atoms(n)?))
    }
    pub fn value(self) -> i128 {
        self.0
    }
}
impl TryFrom<String> for Atoms {
    type Error = Error;
    fn try_from(v: String) -> Result<Self> {
        Ok(Self(money::parse_atoms(&v)?))
    }
}
impl From<Atoms> for String {
    fn from(v: Atoms) -> Self {
        v.0.to_string()
    }
}
impl Validate for Atoms {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}
