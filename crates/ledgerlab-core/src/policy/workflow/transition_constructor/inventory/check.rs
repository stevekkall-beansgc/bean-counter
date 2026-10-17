macro_rules! data_field { ($m:expr,$v:expr,$($key:expr),+ $(,)?) => {{ let v=$v; $(let v=shape::field($m,v,$key)?;)+ v }}; }
macro_rules! invoke {
    ($f:path,$m:expr,$a0:expr) => {{
        let (a0,) = ($a0,);
        $f($m, a0)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr) => {{
        let (a0, a1) = ($a0, $a1);
        $f($m, a0, a1)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr) => {{
        let (a0, a1, a2) = ($a0, $a1, $a2);
        $f($m, a0, a1, a2)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr) => {{
        let (a0, a1, a2, a3) = ($a0, $a1, $a2, $a3);
        $f($m, a0, a1, a2, a3)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr) => {{
        let (a0, a1, a2, a3, a4) = ($a0, $a1, $a2, $a3, $a4);
        $f($m, a0, a1, a2, a3, a4)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr) => {{
        let (a0, a1, a2, a3, a4, a5) = ($a0, $a1, $a2, $a3, $a4, $a5);
        $f($m, a0, a1, a2, a3, a4, a5)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr) => {{
        let (a0, a1, a2, a3, a4, a5, a6) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6);
        $f($m, a0, a1, a2, a3, a4, a5, a6)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7);
        $f($m, a0, a1, a2, a3, a4, a5, a6, a7)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8) = ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8);
        $f($m, a0, a1, a2, a3, a4, a5, a6, a7, a8)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9) =
            ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9);
        $f($m, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9)
    }};
    ($f:path,$m:expr,$a0:expr,$a1:expr,$a2:expr,$a3:expr,$a4:expr,$a5:expr,$a6:expr,$a7:expr,$a8:expr,$a9:expr,$a10:expr) => {{
        let (a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10) =
            ($a0, $a1, $a2, $a3, $a4, $a5, $a6, $a7, $a8, $a9, $a10);
        $f($m, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10)
    }};
}
// Paid DATA predicates. These never confer host authentication or admission.
use super::super::record_body::RecordBody;
use super::super::{
    CoreMeter, arithmetic,
    data::*,
    meter,
    records::Records,
    representation::{InvocationToken, ParsedRef},
    shape,
};
use crate::{Result, domain::Timestamp};
use num_bigint::BigInt;
use serde_json::{Value, json};
pub(super) fn eq(m: &mut CoreMeter, v: &Value, k: &str, w: &Value) -> Result<()> {
    let got = invoke!(shape::field, m, v, k)?;
    invoke!(shape::equal, m, got, w)
}
pub(super) fn copy(m: &mut CoreMeter, a: &Value, k: &str, b: &Value, j: &str) -> Result<()> {
    let a = invoke!(shape::field, m, a, k)?;
    let b = invoke!(shape::field, m, b, j)?;
    invoke!(shape::equal, m, a, b)
}
pub(super) fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<RecordBody> {
    let pr = invoke!(ParsedRef::parse, m, o, v)?;
    r.get_parsed(m, o, &c.key, &pr, kind, schema)
}
pub(super) fn number(m: &mut CoreMeter, v: &Value, k: &str, positive: bool) -> Result<BigInt> {
    let raw = shape::text(m, v, k)?;
    m.visit("inventory.numeric_dictionary")?;
    if matches!(
        k,
        "revision"
            | "version"
            | "commit_sequence"
            | "old_revision"
            | "new_revision"
            | "source_revision"
            | "head_revision"
            | "verified_through_sequence"
            | "origin_count"
            | "document_count"
            | "audited_retained_bytes"
    ) {
        bounds(m, raw.len(), 1, 19)?;
    }
    let n = arithmetic::integer(m, raw, false)?;
    if positive && !meter::cmp(m, &n, &0.into())?.is_gt() {
        return Err(shape::fail("positive counter"));
    }
    Ok(n)
}
pub(super) fn time(m: &mut CoreMeter, v: &Value, k: &str) -> Result<BigInt> {
    let raw = shape::text(m, v, k)?;
    Ok(m.scoped(super::super::meter::timestamp_site(k), |m| {
        Timestamp::parse_with_work(raw, m)
    })?
    .micros()
    .into())
}
pub(super) fn decimal(m: &mut CoreMeter, v: &Value, k: &str) -> Result<arithmetic::Ratio> {
    let text = shape::text(m, v, k)?;
    let value = arithmetic::decimal(m, text)?;
    if meter::cmp(m, &value.numerator, &0.into())?.is_lt() {
        return Err(shape::fail("nonnegative declaration"));
    }
    Ok(value)
}
pub(super) fn before(m: &mut CoreMeter, a: &Value, ak: &str, b: &Value, bk: &str) -> Result<()> {
    let a = invoke!(time, m, a, ak)?;
    let b = invoke!(time, m, b, bk)?;
    if invoke!(meter::cmp, m, &a, &b)?.is_gt() {
        return Err(shape::fail("captured time order"));
    }
    Ok(())
}
pub(super) fn count(m: &mut CoreMeter, a: &[Value], n: usize) -> Result<()> {
    if !invoke!(meter::cmp, m, &a.len().into(), &n.into())?.is_eq() {
        return Err(shape::fail("selected count"));
    }
    Ok(())
}
pub(super) fn name(m: &mut CoreMeter, v: &Value, k: &str) -> Result<()> {
    opaque(m, v, k, 128)
}
pub(super) fn key(m: &mut CoreMeter, v: &Value, c: &DraftContextData) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        v,
        &["logical_store_id", "scope", "source", "operation_id"],
        &[]
    )?;
    invoke!(eq, m, v, "logical_store_id", &json!(c.key.logical_store_id))?;
    invoke!(eq, m, v, "scope", &json!(c.key.scope))?;
    invoke!(opaque, m, v, "source", 256)?;
    invoke!(name, m, v, "operation_id")
}
pub(super) fn amount(m: &mut CoreMeter, v: &Value) -> Result<BigInt> {
    invoke!(shape::closed, m, v, &["currency", "scale", "atoms"], &[])?;
    invoke!(eq, m, v, "currency", &json!("USD"))?;
    invoke!(eq, m, v, "scale", &json!(18))?;
    let atoms = invoke!(number, m, v, "atoms", false)?;
    let max = invoke!(
        arithmetic::integer,
        m,
        "999999999999999999999999999999",
        false
    )?;
    if invoke!(meter::cmp, m, &atoms, &max)?.is_gt() {
        return Err(shape::fail("amount magnitude"));
    }
    Ok(atoms)
}
pub(super) fn set(m: &mut CoreMeter, a: &[Value]) -> Result<()> {
    for (i, v) in a.iter().enumerate() {
        m.visit("inventory.set_member")?;
        for prior in &a[..i] {
            if invoke!(shape::same, m, v, prior)? {
                return Err(shape::fail("duplicate set member"));
            }
        }
    }
    Ok(())
}

pub(super) fn opaque(m: &mut CoreMeter, v: &Value, k: &str, max: usize) -> Result<()> {
    let text = shape::text(m, v, k)?;
    if !meter::cmp(m, &text.len().into(), &0usize.into())?.is_gt()
        || meter::cmp(m, &text.len().into(), &max.into())?.is_gt()
    {
        return Err(shape::fail("opaque identity bound"));
    }
    for ch in text.chars() {
        m.scan_bytes(ch.len_utf8())?;
        if !m.check("inventory.opaque_character", || !ch.is_control())? {
            return Err(shape::fail("opaque identity character"));
        }
    }
    Ok(())
}
pub(super) fn bounds(m: &mut CoreMeter, length: usize, min: usize, max: usize) -> Result<()> {
    if meter::cmp(m, &length.into(), &min.into())?.is_lt()
        || meter::cmp(m, &length.into(), &max.into())?.is_gt()
    {
        return Err(shape::fail("set cardinality"));
    }
    Ok(())
}
pub(super) fn qualified(m: &mut CoreMeter, text: &str) -> Result<()> {
    crate::policy::workflow::facts::name_with_work(text, m)
}
pub(super) fn one_of(m: &mut CoreMeter, text: &str, allowed: &[&str]) -> Result<()> {
    for a in allowed {
        if shape::same_text(m, text, a)? {
            return Ok(());
        }
    }
    Err(shape::fail("closed enum"))
}
