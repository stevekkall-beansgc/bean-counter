//! Accepted central-adjudication-r3/1 boundary types and exact bounded codecs.
//! This module does not yet execute economic transitions or certify stored replay.
pub mod commands;
pub mod proofs;
pub mod reads;
pub mod resources;
pub mod runtime;
pub mod types;
use crate::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

pub const PROFILE: &str = "central-adjudication-r3/1";
pub const COMMAND_BYTES: usize = 256 * 1024;
pub const SEGMENT_BYTES: usize = 8 * 1024 * 1024;
pub const INTRODUCED_TRUST_BYTES: usize = 2 * 1024 * 1024;
pub const PAGE_BYTES: usize = 4096;
pub const MAX_KEY_BYTES: usize = 1115;
pub const MAX_INDEX_PATH_PAGES: usize = 8 * MAX_KEY_BYTES + 2;

pub trait Validate {
    fn validate(&self) -> Result<()>;
}
pub(super) fn ensure(ok: bool, code: &'static str, detail: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::new(code, detail))
    }
}
pub(super) fn check_set<T: Serialize>(values: &[T]) -> Result<()> {
    let mut prior: Option<Vec<u8>> = None;
    for value in values {
        let bytes = canonical_bytes(value, SEGMENT_BYTES)?;
        ensure(
            prior.as_ref().is_none_or(|p| p < &bytes),
            "SET_ORDER",
            "strict canonical-byte set order",
        )?;
        prior = Some(bytes);
    }
    Ok(())
}
pub(super) fn check_unique<T: Serialize>(values: &[T]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        ensure(
            seen.insert(canonical_bytes(value, SEGMENT_BYTES)?),
            "SET_ORDER",
            "duplicate ordered gateway",
        )?;
    }
    Ok(())
}
/// Explicit R3 bound, independent of the old profile's 4 MiB encoder limit.
/// The same integer-only/UTF-16 ordering rules apply. No ambient input is read.
pub fn canonical_bytes(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    ensure(limit <= SEGMENT_BYTES, "LIMIT", "R3 encoder bound")?;
    let value = serde_json::to_value(value).map_err(|e| Error::new("JSON", e.to_string()))?;
    fn emit(v: &Value, bytes: &mut Vec<u8>, depth: usize, limit: usize) -> Result<()> {
        ensure(depth <= 32, "LIMIT", "JSON depth")?;
        match v {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                bytes.push(b'{');
                for (i, key) in keys.into_iter().enumerate() {
                    if i > 0 {
                        bytes.push(b',');
                    }
                    emit(&Value::String(key.clone()), bytes, depth + 1, limit)?;
                    bytes.push(b':');
                    emit(&map[key], bytes, depth + 1, limit)?;
                }
                bytes.push(b'}');
            }
            Value::Array(items) => {
                bytes.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        bytes.push(b',');
                    }
                    emit(item, bytes, depth + 1, limit)?;
                }
                bytes.push(b']');
            }
            Value::Number(n) => {
                ensure(
                    n.as_i64().is_some_and(|n| {
                        (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&n)
                    }),
                    "JSON_NUMBER",
                    "safe integer only",
                )?;
                bytes.extend(n.to_string().as_bytes());
            }
            _ => {
                bytes.extend(serde_json::to_vec(v).map_err(|e| Error::new("JSON", e.to_string()))?)
            }
        }
        ensure(bytes.len() <= limit, "LIMIT", "canonical bytes")
    }
    let mut bytes = Vec::new();
    emit(&value, &mut bytes, 0, limit)?;
    Ok(bytes)
}
/// A shape/encoding checked command is NOT a validated economic plan.
#[derive(Clone, Debug)]
pub struct ParsedCommand {
    command: commands::Command,
    bytes: Vec<u8>,
}
impl ParsedCommand {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let command = parse_exact::<commands::Command>(bytes, COMMAND_BYTES)?;
        Ok(Self {
            command,
            bytes: bytes.to_vec(),
        })
    }
    pub fn command(&self) -> &commands::Command {
        &self.command
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
pub fn parse_exact<T: DeserializeOwned + Validate + Serialize>(
    bytes: &[u8],
    limit: usize,
) -> Result<T> {
    ensure(limit <= SEGMENT_BYTES, "LIMIT", "R3 decode bound")?;
    let value = crate::canonical::parse_bounded(bytes, limit)?;
    crate::canonical::no_null(&value)?;
    ensure(
        canonical_bytes(&value, limit)? == bytes,
        "CANONICAL",
        "exact canonical bytes required",
    )?;
    let parsed: T =
        serde_json::from_value(value).map_err(|e| Error::new("SHAPE", e.to_string()))?;
    parsed.validate()?;
    Ok(parsed)
}
pub fn raw_sha256(bytes: &[u8]) -> types::Digest {
    types::Digest::parse(&crate::canonical::hex(&Sha256::digest(bytes))).expect("SHA-256 is hex")
}
#[cfg(test)]
mod tests;
