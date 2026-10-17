//! Executing operations for the SAME acceptance Projection normalizer. These
//! helpers provide no origin/authority or allocation quota; its owning caller
//! must pre-admit typed maps, strings, key buffers and serialization overlap.
use super::*;
use crate::validation_work::{self as w, ValidationVisit as Visit, ValidationWork};
use std::cmp::Ordering;

pub(in crate::policy::workflow) fn sort<T>(
    values: &mut [T],
    work: &mut dyn ValidationWork,
    mut compare: impl FnMut(&T, &T, &mut dyn ValidationWork) -> Result<Ordering>,
) -> Result<()> {
    let mut i = 1usize;
    while w::compare(work, i, values.len())?.is_lt() {
        let mut j = i;
        while w::compare(work, j, 0usize)?.is_gt() {
            let prior = w::usize_sub(work, j, 1)?;
            if !compare(&values[prior], &values[j], work)?.is_gt() {
                break;
            }
            work.visit(Visit::SortMove)?;
            let moved = w::usize_add(work, std::mem::size_of::<T>(), std::mem::size_of::<T>())?;
            work.write_bytes(moved)?;
            values.swap(prior, j);
            j = prior;
        }
        i = w::usize_add(work, i, 1)?;
    }
    Ok(())
}
pub(super) fn check(work: &mut dyn ValidationWork, valid: bool, detail: &str) -> Result<()> {
    work.visit(Visit::Constraint)?;
    if valid { Ok(()) } else { Err(fail(detail)) }
}
pub(in crate::policy::workflow) fn compare_text(
    a: &str,
    b: &str,
    work: &mut dyn ValidationWork,
) -> Result<Ordering> {
    work.visit(Visit::MemberCompare)?;
    work.scan_bytes(a.len())?;
    work.scan_bytes(b.len())?;
    Ok(a.cmp(b))
}
pub(in crate::policy::workflow) fn text(
    value: &str,
    maximum: usize,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    work.visit(Visit::Constraint)?;
    if value.is_empty() || w::compare(work, value.len(), maximum)?.is_gt() {
        return Err(Error::new(
            "IDENTIFIER",
            "invalid UTF-8 byte length or control character",
        ));
    }
    work.scan_bytes(value.len())?;
    for character in value.chars() {
        work.visit(Visit::Constraint)?;
        if character.is_control() {
            return Err(Error::new(
                "IDENTIFIER",
                "invalid UTF-8 byte length or control character",
            ));
        }
    }
    Ok(())
}
pub(super) fn copy(value: &str, work: &mut dyn ValidationWork) -> Result<String> {
    work.scan_bytes(value.len())?;
    work.write_bytes(value.len())?;
    let mut out = String::new();
    out.try_reserve_exact(value.len())
        .map_err(|_| fail("normalization string allocation"))?;
    out.push_str(value);
    Ok(out)
}
pub(super) fn member(value: &str, allowed: &[&str], work: &mut dyn ValidationWork) -> Result<bool> {
    for candidate in allowed {
        if compare_text(value, candidate, work)?.is_eq() {
            return Ok(true);
        }
    }
    Ok(false)
}
fn field<'a>(value: &'a Value, key: &str, work: &mut dyn ValidationWork) -> Result<&'a Value> {
    work.visit(Visit::Constraint)?;
    let object = value.as_object().ok_or_else(|| fail("reference object"))?;
    for (name, value) in object {
        work.visit(Visit::RefLookup)?;
        if compare_text(name, key, work)?.is_eq() {
            return Ok(value);
        }
    }
    Err(fail("reference member"))
}
pub(super) fn reference(
    value: &Value,
    kind: &str,
    work: &mut dyn ValidationWork,
) -> Result<AgreementRef> {
    work.visit(Visit::Constraint)?;
    let object = value.as_object().ok_or_else(|| fail("reference object"))?;
    if !w::compare(work, object.len(), 3usize)?.is_eq() {
        return Err(fail("reference members"));
    }
    let mut string = |name| -> Result<&str> {
        work.visit(Visit::Constraint)?;
        field(value, name, work)?
            .as_str()
            .ok_or_else(|| fail("reference string"))
    };
    let supplied_kind = string("kind")?;
    let id = string("id")?;
    let digest = string("digest")?;
    let reference = AgreementRef::from_parts_with_work(supplied_kind, id, digest, work)?;
    if !compare_text(reference.kind(), kind, work)?.is_eq() {
        return Err(fail("reference family"));
    }
    // The already-decoded closed Ref object has exactly these canonical scalar
    // fields. The legacy serde round-trip cannot change it, so no Value clone
    // or independent reference normalizer is needed here.
    Ok(reference)
}
fn sorted_strings(values: &mut Vec<String>, work: &mut dyn ValidationWork) -> Result<()> {
    let mut keyed = Vec::new();
    for value in values.drain(..) {
        work.visit(Visit::Element)?;
        // Move the existing String into a scalar Value. The SAME complete
        // emitter borrows it, so no opaque serde copy precedes accounting.
        let scalar = Value::String(value);
        let key = CanonicalBytes::from_representation_with_work(&scalar, work, 262144)?.into_vec();
        let Value::String(value) = scalar else {
            unreachable!("constructed string scalar")
        };
        work.write_bytes(std::mem::size_of::<(Vec<u8>, String)>())?;
        keyed.push((key, value));
    }
    sort(&mut keyed, work, |a, b, work| {
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(a.0.len())?;
        work.scan_bytes(b.0.len())?;
        Ok(a.0.cmp(&b.0))
    })?;
    for pair in keyed.windows(2) {
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(pair[0].0.len())?;
        work.scan_bytes(pair[1].0.len())?;
        if pair[0].0 == pair[1].0 {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    *values = keyed.into_iter().map(|(_, v)| v).collect();
    Ok(())
}
pub(super) fn refs(
    values: &mut Vec<Value>,
    kind: &str,
    min: usize,
    max: usize,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    if w::compare(work, values.len(), min)?.is_lt() || w::compare(work, values.len(), max)?.is_gt()
    {
        return Err(fail("reference count"));
    }
    for value in values.iter() {
        work.visit(Visit::Element)?;
        reference(value, kind, work)?;
    }
    sort(values, work, |a, b, work| {
        // Exact closed unescaped digest-derived Ref JCS order is digest,id,kind.
        for name in ["digest", "id", "kind"] {
            let a = field(a, name, work)?
                .as_str()
                .ok_or_else(|| fail("reference string"))?;
            let b = field(b, name, work)?
                .as_str()
                .ok_or_else(|| fail("reference string"))?;
            let order = compare_text(a, b, work)?;
            if !order.is_eq() {
                return Ok(order);
            }
        }
        Ok(Ordering::Equal)
    })?;
    for pair in values.windows(2) {
        work.visit(Visit::MemberCompare)?;
        let mut equal = true;
        for name in ["digest", "id", "kind"] {
            let a = field(&pair[0], name, work)?
                .as_str()
                .ok_or_else(|| fail("reference string"))?;
            let b = field(&pair[1], name, work)?
                .as_str()
                .ok_or_else(|| fail("reference string"))?;
            if !compare_text(a, b, work)?.is_eq() {
                equal = false;
                break;
            }
        }
        if equal {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    Ok(())
}
pub(super) fn names<'a>(
    values: impl Iterator<Item = &'a str> + Clone,
    min: usize,
    max: usize,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    let mut count = 0usize;
    for value in values.clone() {
        work.visit(Visit::Element)?;
        super::super::facts::name_with_work(value, work)?;
        for prior in values.clone().take(count) {
            if compare_text(prior, value, work)?.is_eq() {
                return Err(fail("duplicate named entry"));
            }
        }
        count = w::usize_add(work, count, 1)?;
    }
    if w::compare(work, count, min)?.is_lt() || w::compare(work, count, max)?.is_gt() {
        return Err(fail("named entry count"));
    }
    Ok(())
}
pub(super) fn quantities(
    rows: &mut Vec<QuantityCeiling>,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    if w::compare(work, rows.len(), 64usize)?.is_gt() {
        return Err(Error::new("INVALID_SHAPE", "quantity ceiling count"));
    }
    for row in rows.iter_mut() {
        work.visit(Visit::Element)?;
        for s in [&row.activity, &row.field, &row.unit] {
            super::super::facts::name_with_work(s, work).map_err(|e| {
                if e.code == "FACT_SCHEMA" {
                    Error::new("INVALID_SHAPE", "quantity name")
                } else {
                    e
                }
            })?;
        }
        work.visit(Visit::Constraint)?;
        work.scan_bytes(1)?;
        if row.maximum.starts_with('-') {
            return Err(Error::new("INVALID_SCALAR", "nonnegative quantity ceiling"));
        }
        let scalar = super::super::facts::SignedScalar::parse_with_work(&row.maximum, 18, work)
            .map_err(|e| {
                if e.code == "FACT_PRECISION" {
                    Error::new("INVALID_SCALAR", "quantity decimal")
                } else {
                    e
                }
            })?;
        row.maximum = copy(scalar.as_str(), work)?;
    }
    for (index, row) in rows.iter().enumerate() {
        work.visit(Visit::Element)?;
        for prior in &rows[..index] {
            if compare_text(&prior.activity, &row.activity, work)?.is_eq()
                && compare_text(&prior.field, &row.field, work)?.is_eq()
            {
                return Err(Error::new(
                    "INVALID_SHAPE",
                    "duplicate quantity activity/field",
                ));
            }
        }
    }
    // Unique(activity,field) is established first. These ASCII qualified names
    // are the first JCS object fields, so this is the SAME canonical set order.
    sort(rows, work, |a, b, work| {
        let order = compare_text(&a.activity, &b.activity, work)?;
        if order.is_eq() {
            compare_text(&a.field, &b.field, work)
        } else {
            Ok(order)
        }
    })
}
pub(super) fn amount(value: &Amount, work: &mut dyn ValidationWork) -> Result<()> {
    if !compare_text(&value.currency, "USD", work)?.is_eq()
        || !w::compare(work, value.scale, 18u8)?.is_eq()
    {
        return Err(Error::new("INVALID_SCALAR", "nonnegative USD18 amount"));
    }
    let scalar = super::super::facts::SignedScalar::parse_with_work(&value.atoms, 0, work)
        .map_err(|e| {
            if e.code == "FACT_PRECISION" {
                Error::new("INVALID_SCALAR", "canonical bounded amount")
            } else {
                e
            }
        })?;
    if !compare_text(scalar.as_str(), &value.atoms, work)?.is_eq() {
        return Err(Error::new("INVALID_SCALAR", "canonical bounded amount"));
    }
    work.visit(Visit::Constraint)?;
    work.scan_bytes(1)?;
    if value.atoms.starts_with('-') {
        return Err(Error::new("INVALID_SCALAR", "nonnegative amount"));
    }
    Ok(())
}
pub(super) fn exposures(rows: &mut Vec<Exposure>, work: &mut dyn ValidationWork) -> Result<()> {
    if w::compare(work, rows.len(), 16usize)?.is_gt() {
        return Err(Error::new("INVALID_SHAPE", "exposure ceiling count"));
    }
    for (index, row) in rows.iter().enumerate() {
        work.visit(Visit::Element)?;
        super::super::facts::name_with_work(&row.binding_slot, work).map_err(|e| {
            if e.code == "FACT_SCHEMA" {
                Error::new("INVALID_SHAPE", "exposure slot name")
            } else {
                e
            }
        })?;
        for prior in &rows[..index] {
            if compare_text(&prior.binding_slot, &row.binding_slot, work)?.is_eq() {
                return Err(Error::new(
                    "INVALID_SHAPE",
                    "duplicate exposure binding slot",
                ));
            }
        }
        amount(&row.maximum, work)?;
    }
    sort(rows, work, |a, b, work| {
        compare_text(&a.binding_slot, &b.binding_slot, work)
    })
}
pub(in crate::policy::workflow) fn string_set(
    values: &mut Vec<String>,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    sorted_strings(values, work)
}
