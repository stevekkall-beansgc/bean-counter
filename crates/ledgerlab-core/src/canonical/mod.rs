//! The integer-only ledger-canonical-v1 profile of RFC 8785.
//! Strings are JSON escaped; object keys compare UTF-16 code units, not UTF-8.
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

const SAFE: i64 = 9_007_199_254_740_991;
pub const CANDIDATE_LIMIT: usize = 262_144;
pub const BUNDLE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalBytes(Vec<u8>);
impl CanonicalBytes {
    pub fn from_value(value: &impl Serialize) -> Result<Self> {
        let value = serde_json::to_value(value).map_err(|e| Error::new("JSON", e.to_string()))?;
        let mut bytes = Vec::new();
        emit(&value, &mut bytes, 0)?;
        if bytes.len() > BUNDLE_LIMIT {
            return Err(Error::new("LIMIT", "canonical bundle bytes"));
        }
        Ok(Self(bytes))
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Reject ambiguous lexical JSON before constructing any wire DTO.
pub fn parse(bytes: &[u8]) -> Result<Value> {
    parse_bounded(bytes, CANDIDATE_LIMIT)
}
pub fn parse_bounded(bytes: &[u8], limit: usize) -> Result<Value> {
    if bytes.len() > limit {
        return Err(Error::new("LIMIT", "JSON bytes"));
    }
    std::str::from_utf8(bytes).map_err(|_| Error::new("JSON", "invalid UTF-8"))?;
    let mut p = Parser { bytes, pos: 0 };
    let result = p.value(0)?;
    p.space();
    if p.pos != bytes.len() {
        return Err(Error::new("JSON", "trailing input"));
    }
    Ok(result)
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|c| b" \t\r\n".contains(c))
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, c: u8) -> Result<()> {
        self.space();
        if self.bytes.get(self.pos) != Some(&c) {
            return Err(Error::new("JSON", "unexpected token"));
        }
        self.pos += 1;
        Ok(())
    }
    fn string(&mut self) -> Result<String> {
        self.space();
        let start = self.pos;
        self.take(b'"')?;
        loop {
            match self.bytes.get(self.pos) {
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 2;
                }
                Some(_) => self.pos += 1,
                None => return Err(Error::new("JSON", "unterminated string")),
            }
        }
        serde_json::from_slice(&self.bytes[start..self.pos])
            .map_err(|e| Error::new("JSON", e.to_string()))
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.space();
        match self.bytes.get(self.pos).copied() {
            Some(b'{') => {
                if depth >= 32 {
                    return Err(Error::new("LIMIT", "JSON depth"));
                }
                self.pos += 1;
                self.space();
                let mut map = Map::new();
                if self.bytes.get(self.pos) == Some(&b'}') {
                    self.pos += 1;
                    return Ok(Value::Object(map));
                }
                loop {
                    let key = self.string()?;
                    self.take(b':')?;
                    let value = self.value(depth + 1)?;
                    if map.insert(key, value).is_some() {
                        return Err(Error::new("JSON_DUPLICATE_KEY", "duplicate object key"));
                    }
                    self.space();
                    if self.bytes.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Object(map))
            }
            Some(b'[') => {
                if depth >= 32 {
                    return Err(Error::new("LIMIT", "JSON depth"));
                }
                self.pos += 1;
                self.space();
                let mut out = Vec::new();
                if self.bytes.get(self.pos) == Some(&b']') {
                    self.pos += 1;
                    return Ok(Value::Array(out));
                }
                loop {
                    out.push(self.value(depth + 1)?);
                    self.space();
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Array(out))
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                if self.bytes[self.pos] == b'-' {
                    self.pos += 1;
                }
                while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                    self.pos += 1;
                }
                let token = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| Error::new("JSON", "number"))?;
                let digits = token.strip_prefix('-').unwrap_or(token);
                if digits.is_empty()
                    || (digits.len() > 1 && digits.starts_with('0'))
                    || token == "-0"
                {
                    return Err(Error::new("JSON_NUMBER", "noncanonical numeric token"));
                }
                let n: i64 = token
                    .parse()
                    .map_err(|_| Error::new("JSON_NUMBER", "unsafe integer"))?;
                if !(-SAFE..=SAFE).contains(&n)
                    || self.bytes.get(self.pos).is_some_and(|b| b".eE".contains(b))
                {
                    return Err(Error::new(
                        "JSON_NUMBER",
                        "only safe integer tokens allowed",
                    ));
                }
                Ok(Value::from(n))
            }
            _ => Err(Error::new("JSON", "unexpected token or BOM")),
        }
    }
    fn literal(&mut self, token: &[u8], value: Value) -> Result<Value> {
        if self.bytes.get(self.pos..self.pos + token.len()) != Some(token) {
            return Err(Error::new("JSON", "literal"));
        }
        self.pos += token.len();
        Ok(value)
    }
}
fn emit(v: &Value, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    if depth > 32 {
        return Err(Error::new("LIMIT", "JSON depth"));
    }
    match v {
        Value::Null => out.extend(b"null"),
        Value::Bool(true) => out.extend(b"true"),
        Value::Bool(false) => out.extend(b"false"),
        Value::Number(n) => {
            let i = n
                .as_i64()
                .filter(|i| (-SAFE..=SAFE).contains(i))
                .ok_or_else(|| Error::new("JSON_NUMBER", "unsafe integer"))?;
            out.extend(i.to_string().as_bytes());
        }
        Value::String(s) => {
            out.extend(serde_json::to_vec(s).map_err(|e| Error::new("JSON", e.to_string()))?)
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                emit(item, out, depth + 1)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push(b'{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                emit(&Value::String(key.clone()), out, depth + 1)?;
                out.push(b':');
                emit(&map[key], out, depth + 1)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

pub(crate) fn no_null(v: &Value) -> Result<()> {
    match v {
        Value::Null => return Err(Error::new("SCHEMA", "null is forbidden")),
        Value::Array(a) => {
            for v in a {
                no_null(v)?;
            }
        }
        Value::Object(o) => {
            for v in o.values() {
                no_null(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn dto<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    no_null(&v)?;
    serde_json::from_value(v).map_err(|e| Error::new("SCHEMA", e.to_string()))
}
pub fn sort_set<T: Serialize>(values: &mut Vec<T>) -> Result<()> {
    let mut keyed: Vec<_> = values
        .drain(..)
        .map(|v| Ok((CanonicalBytes::from_value(&v)?.into_vec(), v)))
        .collect::<Result<_>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    if keyed.windows(2).any(|p| p[0].0 == p[1].0) {
        return Err(Error::new("DUPLICATE", "set member"));
    }
    *values = keyed.into_iter().map(|(_, v)| v).collect();
    Ok(())
}

/// Fixed domains; callers cannot invent a new hash domain from untrusted strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Domain {
    Event,
    Ingress,
    EventContent,
    Document,
    Claim,
    ClaimFacts,
    Link,
    Decision,
    Effect,
    EffectFacts,
    Action,
    Obligation,
    Intention,
    Receipt,
    DecisionContent,
    RecordContent,
    Chain,
    SnapshotRef,
    Explanation,
    ControlTransition,
    IntentionPayload,
    ReversalEffect,
}
impl Domain {
    pub fn name(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Ingress => "ingress",
            Self::EventContent => "event-content",
            Self::Document => "document",
            Self::Claim => "claim",
            Self::ClaimFacts => "claim-facts",
            Self::Link => "link",
            Self::Decision => "decision",
            Self::Effect => "effect",
            Self::EffectFacts => "effect-facts",
            Self::Action => "action",
            Self::Obligation => "obligation",
            Self::Intention => "intention",
            Self::Receipt => "receipt",
            Self::DecisionContent => "decision-content",
            Self::RecordContent => "record-content",
            Self::Chain => "chain",
            Self::SnapshotRef => "snapshot-ref",
            Self::Explanation => "explanation",
            Self::ControlTransition => "control-transition",
            Self::IntentionPayload => "intention-payload",
            Self::ReversalEffect => "reversal-effect",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        [
            Self::Event,
            Self::Ingress,
            Self::EventContent,
            Self::Document,
            Self::Claim,
            Self::ClaimFacts,
            Self::Link,
            Self::Decision,
            Self::Effect,
            Self::EffectFacts,
            Self::Action,
            Self::Obligation,
            Self::Intention,
            Self::Receipt,
            Self::DecisionContent,
            Self::RecordContent,
            Self::Chain,
            Self::SnapshotRef,
            Self::Explanation,
            Self::ControlTransition,
            Self::IntentionPayload,
            Self::ReversalEffect,
        ]
        .into_iter()
        .find(|d| d.name() == name)
        .ok_or_else(|| Error::new("HASH_DOMAIN", name))
    }
    pub fn prefix(self) -> Result<&'static str> {
        Ok(match self {
            Self::Event => "ev",
            Self::Document => "doc",
            Self::Claim => "cl",
            Self::Link => "ln",
            Self::Decision => "dc",
            Self::Effect | Self::ReversalEffect => "ef",
            Self::Action => "ac",
            Self::Obligation => "ob",
            Self::Intention => "in",
            Self::Receipt => "rc",
            Self::SnapshotRef => "sr",
            Self::Explanation => "xp",
            Self::ControlTransition => "ct",
            _ => return Err(Error::new("HASH_DOMAIN", "domain has no ID")),
        })
    }
}
pub fn hash_input(domain: Domain, value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = format!("ledgerlab/{}/1\0", domain.name()).into_bytes();
    bytes.extend(CanonicalBytes::from_value(value)?.as_slice());
    Ok(bytes)
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn hash(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(hex(&Sha256::digest(hash_input(domain, value)?)))
}
pub fn digest(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(format!("sha256:{}", hash(domain, value)?))
}
pub fn identity(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(format!("{}_{}", domain.prefix()?, hash(domain, value)?))
}

/// Frozen outcome profiles use explicit domains; this does not change v1 hashes.
pub fn outcome_digest(profile: &str, domain: &str, value: &impl Serialize) -> Result<String> {
    if !["2-candidate.4", "reservation-settlement/1"].contains(&profile)
        || domain.is_empty()
        || !domain.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
    {
        return Err(Error::new("HASH_DOMAIN", "unsupported outcome domain"));
    }
    let mut h = Sha256::new();
    h.update(format!("ledgerlab/{domain}/{profile}\0").as_bytes());
    h.update(CanonicalBytes::from_value(value)?.as_slice());
    Ok(format!("sha256:{}", hex(&h.finalize())))
}

pub mod outcome;
