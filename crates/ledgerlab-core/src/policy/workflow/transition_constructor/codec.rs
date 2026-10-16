//! Raw-input structural checks and one executing capped output emitter.
//! Output has no separate count/prevalidation traversal.
use super::meter::Meter;
use crate::{Error, Result, canonical};
use num_traits::ToPrimitive;
use serde_json::Value;
fn limit() -> Error {
    Error::new("PROGRAM_BUDGET_EXCEEDED", "canonical destination bound")
}
fn plus(total: &mut usize, n: usize, maximum: usize) -> Result<()> {
    // Formatting byte accounting is a separate byte-domain mechanism.
    *total = total.checked_add(n).ok_or_else(limit)?;
    if *total > maximum {
        return Err(limit());
    }
    Ok(())
}
fn string_count(m: &mut Meter, s: &str, total: &mut usize, maximum: usize) -> Result<()> {
    plus(total, 2, maximum)?;
    m.read_bytes(s.len())?;
    m.scan_bytes(s.len())?;
    for c in s.chars() {
        let n = match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if c <= '\u{1f}' => 6,
            _ => c.len_utf8(),
        };
        plus(total, n, maximum)?;
    }
    Ok(())
}
fn count(m: &mut Meter, v: &Value, total: &mut usize, maximum: usize, depth: usize) -> Result<()> {
    m.visit("codec.value")?;
    if super::meter::cmp(m, &depth.into(), &32usize.into())?.is_gt() {
        return Err(limit());
    }
    match v {
        Value::Null => return Err(Error::new("INVALID_SHAPE", "null selected body")),
        Value::Bool(b) => plus(total, if *b { 4 } else { 5 }, maximum)?,
        Value::Number(n) => {
            if m.check("codec.integer", || !n.is_i64() && !n.is_u64())? {
                return Err(Error::new("INVALID_SCALAR", "noninteger JSON"));
            }
            plus(total, n.to_string().len(), maximum)?;
        }
        Value::String(s) => string_count(m, s, total, maximum)?,
        Value::Array(a) => {
            plus(total, 2, maximum)?;
            for (i, v) in a.iter().enumerate() {
                m.visit("codec.element")?;
                if i > 0 {
                    plus(total, 1, maximum)?;
                }
                let next = super::meter::add(m, &depth.into(), &1usize.into())?
                    .to_usize()
                    .ok_or_else(limit)?;
                count(m, v, total, maximum, next)?;
            }
        }
        Value::Object(o) => {
            plus(total, 2, maximum)?;
            for (i, (k, v)) in o.iter().enumerate() {
                m.visit("codec.field")?;
                if i > 0 {
                    plus(total, 1, maximum)?;
                }
                string_count(m, k, total, maximum)?;
                plus(total, 1, maximum)?;
                let next = super::meter::add(m, &depth.into(), &1usize.into())?
                    .to_usize()
                    .ok_or_else(limit)?;
                count(m, v, total, maximum, next)?;
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow) fn encode(
    m: &mut Meter,
    v: &Value,
    maximum: usize,
) -> Result<Vec<u8>> {
    Ok(m.scoped("CanonicalBytes::emit.value", |m| {
        canonical::CanonicalBytes::from_value_with_work_bounded(v, m, maximum)
    })?
    .into_vec())
}

pub(in crate::policy::workflow) fn parse(
    m: &mut Meter,
    raw: &[u8],
    maximum: usize,
) -> Result<Value> {
    m.read_bytes(raw.len())?;
    m.scan_bytes(raw.len())?;
    let value = canonical::parse_bounded(raw, maximum)?;
    // Inspect all structure before typed decode; lexical parser rejects duplicate
    // decoded keys/invalid JSON, while byte-domain accounting covers its scan.
    let mut n = 0;
    count(m, &value, &mut n, maximum, 0)?;
    Ok(value)
}

/// The same paid emitter is capped by both destination and remaining aggregate
/// allowance before each append/reserve. Completed logical bytes are admitted
/// once; failed output is discarded and introduces no new body.
pub(in crate::policy::workflow) fn encode_new(
    m: &mut Meter,
    v: &Value,
    maximum: usize,
) -> Result<Vec<u8>> {
    let remaining = m.remaining_new_body()?;
    let bound = maximum.min(remaining);
    let bytes = m
        .scoped("CanonicalBytes::emit.new_body", |m| {
            canonical::CanonicalBytes::from_value_with_work_bounded(v, m, bound)
        })?
        .into_vec();
    m.new_body(bytes.len())?;
    Ok(bytes)
}
