//! Closed data inspection used by the executing draft path.
use super::{CoreMeter, data::ExactRef};
use crate::{Error, Result};
use serde_json::Value;
pub(super) fn fail(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
pub(super) fn field<'a>(m: &mut CoreMeter, v: &'a Value, k: &str) -> Result<&'a Value> {
    m.visit("data.field")?;
    v.get(k)
        .ok_or_else(|| Error::new("INTEGRITY_FAILURE", format!("missing field {k}")))
}
pub(super) fn text<'a>(m: &mut CoreMeter, v: &'a Value, k: &str) -> Result<&'a str> {
    field(m, v, k)?.as_str().ok_or_else(|| fail("text field"))
}
pub(super) fn array<'a>(m: &mut CoreMeter, v: &'a Value, k: &str) -> Result<&'a [Value]> {
    field(m, v, k)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| fail("array field"))
}
pub(super) fn closed(
    m: &mut CoreMeter,
    v: &Value,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    m.visit("data.object")?;
    let o = v.as_object().ok_or_else(|| fail("object"))?;
    for k in required {
        if !m.check("data.required", || o.contains_key(*k))? {
            return Err(fail("missing field"));
        }
    }
    for k in o.keys() {
        let mut found = false;
        for allowed in required.iter().chain(optional) {
            if same_text(m, k, allowed)? {
                found = true;
                break;
            }
        }
        if !found {
            return Err(fail("unknown field"));
        }
    }
    Ok(())
}
/// Paid structural identity; no hidden serde/BTree recursive equality.
pub(super) fn same(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<bool> {
    same_at(m, a, b, 0)
}
fn same_at(m: &mut CoreMeter, a: &Value, b: &Value, depth: usize) -> Result<bool> {
    m.visit("compare.left_value")?;
    m.visit("compare.right_value")?;
    if super::meter::cmp(m, &depth.into(), &32usize.into())?.is_gt() {
        return Err(fail("comparison depth"));
    }
    m.visit("compare.type_constraint")?;
    match (a, b) {
        (Value::Null, Value::Null) => Ok(true),
        (Value::Bool(a), Value::Bool(b)) => m.check("compare.boolean", || a == b),
        (Value::String(a), Value::String(b)) => same_text(m, a, b),
        (Value::Number(a), Value::Number(b)) => {
            m.visit("compare.left_integer")?;
            let a = a.as_i64().ok_or_else(|| fail("comparison integer"))?;
            m.visit("compare.right_integer")?;
            let b = b.as_i64().ok_or_else(|| fail("comparison integer"))?;
            Ok(super::meter::cmp(m, &a.into(), &b.into())?.is_eq())
        }
        (Value::Array(a), Value::Array(b)) => {
            if !super::meter::cmp(m, &a.len().into(), &b.len().into())?.is_eq() {
                return Ok(false);
            }
            for (a, b) in a.iter().zip(b) {
                m.visit("compare.left_element")?;
                m.visit("compare.right_element")?;
                m.visit("compare.descent")?;
                let next = next_depth(m, depth)?;
                if !same_at(m, a, b, next)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Value::Object(a), Value::Object(b)) => {
            if !super::meter::cmp(m, &a.len().into(), &b.len().into())?.is_eq() {
                return Ok(false);
            }
            // Existing parsed map order is deterministic. No new sort or fabricated
            // comparator result; key identity and each typed descent remain paid.
            for ((ak, av), (bk, bv)) in a.iter().zip(b) {
                m.visit("compare.left_field")?;
                m.visit("compare.right_field")?;
                if !same_text(m, ak, bk)? {
                    return Ok(false);
                }
                m.visit("compare.descent")?;
                let next = next_depth(m, depth)?;
                if !same_at(m, av, bv, next)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
fn next_depth(m: &mut CoreMeter, depth: usize) -> Result<usize> {
    use num_traits::ToPrimitive;
    super::meter::add(m, &depth.into(), &1usize.into())?
        .to_usize()
        .ok_or_else(|| fail("comparison depth"))
}
/// One typed string comparison, with only the bytes actually examined scanned.
pub(super) fn same_text(m: &mut CoreMeter, a: &str, b: &str) -> Result<bool> {
    same_bytes(m, a.as_bytes(), b.as_bytes())
}
pub(super) fn same_bytes(m: &mut CoreMeter, a: &[u8], b: &[u8]) -> Result<bool> {
    m.visit("compare.string")?;
    if !super::meter::cmp(m, &a.len().into(), &b.len().into())?.is_eq() {
        return Ok(false);
    }
    for (left, right) in a.iter().zip(b) {
        m.scan_bytes(2)?;
        // This byte test implements the single typed identity operation above;
        // it does not interpret characters as extra semantic JSON values.
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn same_ref(m: &mut CoreMeter, a: &ExactRef, b: &ExactRef) -> Result<bool> {
    Ok(same_text(m, &a.kind, &b.kind)?
        && same_text(m, &a.id, &b.id)?
        && same_text(m, &a.digest, &b.digest)?)
}
/// Canonical byte order with actual examined-byte accounting, not index order.
pub(super) fn byte_order(m: &mut CoreMeter, a: &[u8], b: &[u8]) -> Result<std::cmp::Ordering> {
    m.visit("compare.canonical_order")?;
    for (left, right) in a.iter().zip(b) {
        m.scan_bytes(2)?;
        if left < right {
            return Ok(std::cmp::Ordering::Less);
        }
        if left > right {
            return Ok(std::cmp::Ordering::Greater);
        }
    }
    super::meter::cmp(m, &a.len().into(), &b.len().into())
}
pub(super) fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if !same(m, a, b)? {
        return Err(fail("data relation"));
    }
    Ok(())
}
pub(super) fn member(m: &mut CoreMeter, values: &[Value], value: &Value) -> Result<bool> {
    for v in values {
        m.visit("data.member")?;
        if same(m, v, value)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn contains(m: &mut CoreMeter, values: &[Value], value: &Value) -> Result<()> {
    if !member(m, values, value)? {
        return Err(fail("required membership"));
    }
    Ok(())
}
pub(super) fn subset(m: &mut CoreMeter, a: &[Value], b: &[Value]) -> Result<()> {
    for v in a {
        m.visit("data.subset_element")?;
        contains(m, b, v)?;
    }
    Ok(())
}
pub(super) fn reference(m: &mut CoreMeter, v: &Value) -> Result<ExactRef> {
    closed(m, v, &["kind", "id", "digest"], &[])?;
    let r = ExactRef {
        kind: text(m, v, "kind")?.into(),
        id: text(m, v, "id")?.into(),
        digest: text(m, v, "digest")?.into(),
    };
    super::records::reference(m, &r)?;
    Ok(r)
}
pub(super) fn named<'a>(
    m: &mut CoreMeter,
    values: &'a [Value],
    key: &str,
    name: &str,
) -> Result<&'a Value> {
    let mut result = None;
    for v in values {
        m.visit("data.named")?;
        let member = field(m, v, key)?;
        if same(m, member, &Value::String(name.to_owned()))? {
            if result.is_some() {
                return Err(fail("ambiguous named member"));
            }
            result = Some(v);
        }
    }
    result.ok_or_else(|| fail("missing named member"))
}
