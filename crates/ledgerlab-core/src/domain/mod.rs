mod event;
mod records;
mod time;
use crate::{Error, Result};
pub use event::*;
pub use records::*;
use serde::{Deserialize, Serialize};
pub use time::Timestamp;

pub(crate) fn text(s: &str, max: usize) -> Result<()> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        return Err(Error::new(
            "IDENTIFIER",
            "invalid UTF-8 byte length or control character",
        ));
    }
    Ok(())
}
pub(crate) fn slug(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 64
        || !s.as_bytes()[0].is_ascii_lowercase()
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
    {
        return Err(Error::new("IDENTIFIER", "invalid slug"));
    }
    Ok(())
}
pub(crate) fn prefixed(s: &str, prefix: &str) -> Result<()> {
    let value = s
        .strip_prefix(prefix)
        .ok_or_else(|| Error::new("IDENTIFIER", "wrong internal ID prefix"))?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::new("IDENTIFIER", "expected lowercase SHA-256"));
    }
    Ok(())
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[String;2]", into = "[String;2]")]
pub struct Scope([String; 2]);
impl Scope {
    pub fn new(tenant: &str, environment: &str) -> Result<Self> {
        text(tenant, 128)?;
        text(environment, 128)?;
        Ok(Self([tenant.into(), environment.into()]))
    }
    pub fn tenant(&self) -> &str {
        &self.0[0]
    }
    pub fn environment(&self) -> &str {
        &self.0[1]
    }
}
impl TryFrom<[String; 2]> for Scope {
    type Error = Error;
    fn try_from(v: [String; 2]) -> Result<Self> {
        Self::new(&v[0], &v[1])
    }
}
impl From<Scope> for [String; 2] {
    fn from(s: Scope) -> Self {
        s.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Revision(u64);
impl Revision {
    pub fn new(value: u64) -> Result<Self> {
        if value > i64::MAX as u64 {
            return Err(Error::new("REVISION_EXHAUSTED", "counter bound"));
        }
        Ok(Self(value))
    }
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(Error::new("INPUT_PRECISION", "counter string"));
        }
        Self::new(
            value
                .parse()
                .map_err(|_| Error::new("REVISION_EXHAUSTED", "counter"))?,
        )
    }
    pub fn next(self) -> Result<Self> {
        Self::new(self.0 + 1)
    }
    pub fn value(self) -> u64 {
        self.0
    }
}
impl Serialize for Revision {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
