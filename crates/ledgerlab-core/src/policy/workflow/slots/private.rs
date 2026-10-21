//! Owning normalization transforms shared with the legacy structural parser.
//! No DTO/JSON clone or new validation engine. Complete original template,
//! catalog, role and terms validation remains the shared slot validator's job.
use super::*;
use crate::{
    canonical::audit_arena::{AuditedBytes, AuditedValue, ExternalBufferLease},
    validation_work::{
        self as w, OriginalAuditSession, RepresentationKind as Kind, ValidationVisit as Visit,
        ValidationWork,
    },
};
use std::mem;
pub(super) enum Family {
    Catalog,
    SlotInstance,
}
fn field<'a>(v: &'a Value, name: &str, work: &mut dyn ValidationWork) -> Result<&'a Value> {
    work.visit(Visit::Constraint)?;
    for (key, value) in v
        .as_object()
        .ok_or_else(|| fail("JSON", "structural object"))?
    {
        work.visit(Visit::RefLookup)?;
        if role_equal(key, name, work)? {
            return Ok(value);
        }
    }
    Err(fail("JSON", "missing structural field"))
}
fn closed(v: &Value, names: &[&str], work: &mut dyn ValidationWork) -> Result<()> {
    work.visit(Visit::Constraint)?;
    let object = v
        .as_object()
        .ok_or_else(|| fail("JSON", "structural object"))?;
    if !w::compare(work, object.len(), names.len())?.is_eq() {
        return Err(fail("JSON", "structural fields"));
    }
    for name in names {
        field(v, name, work)?;
    }
    Ok(())
}
fn array<'a>(
    v: &'a Value,
    min: usize,
    max: usize,
    work: &mut dyn ValidationWork,
) -> Result<&'a [Value]> {
    work.visit(Visit::Constraint)?;
    let values = v
        .as_array()
        .ok_or_else(|| fail("JSON", "structural array"))?;
    if w::compare(work, values.len(), min)?.is_lt() || w::compare(work, values.len(), max)?.is_gt()
    {
        return Err(fail("SLOT_LIMIT", "structural array count"));
    }
    Ok(values)
}
struct Order {
    indices: Vec<usize>,
    destination: Vec<usize>,
    _lease: ExternalBufferLease,
}
fn mul(w: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    w.arithmetic(
        crate::validation_work::ArithmeticOperation::Multiply,
        &[&a.into(), &b.into()],
    )?;
    a.checked_mul(b)
        .ok_or_else(|| fail("PROGRAM_BUDGET_EXCEEDED", "slot order size"))
}
fn reorder(
    tree: &mut AuditedValue,
    name: &str,
    key: &str,
    catalog: bool,
    min: usize,
    max: usize,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let rows = array(
        field(tree.value(), name, s.semantic_work())?,
        min,
        max,
        s.semantic_work(),
    )?;
    let bytes = mul(s.semantic_work(), rows.len(), mem::size_of::<usize>())?;
    let bytes = w::usize_add(s.semantic_work(), bytes, bytes)?;
    let lease = s.reserve_external_payload(bytes)?;
    let mut order = Order {
        indices: Vec::new(),
        destination: Vec::new(),
        _lease: lease,
    };
    order
        .indices
        .try_reserve_exact(rows.len())
        .map_err(|_| fail("PROGRAM_BUDGET_EXCEEDED", "slot order allocation"))?;
    order
        .destination
        .try_reserve_exact(rows.len())
        .map_err(|_| fail("PROGRAM_BUDGET_EXCEEDED", "slot destination allocation"))?;
    // Borrowed key/type validation precedes every comparison and mutation.
    for (i, row) in rows.iter().enumerate() {
        s.semantic_work().visit(Visit::Element)?;
        role_string(field(row, key, s.semantic_work())?, s.semantic_work())?;
        s.semantic_work().write_bytes(2 * mem::size_of::<usize>())?;
        order.indices.push(i);
        order.destination.push(0);
    }
    // Fallible same-order sort; failures propagate BEFORE a comparison/move.
    let mut i = 1usize;
    while w::compare(s.semantic_work(), i, order.indices.len())?.is_lt() {
        let mut j = i;
        while w::compare(s.semantic_work(), j, 0usize)?.is_gt() {
            let prior = w::usize_sub(s.semantic_work(), j, 1)?;
            let a = role_string(
                field(&rows[order.indices[prior]], key, s.semantic_work())?,
                s.semantic_work(),
            )?;
            let b = role_string(
                field(&rows[order.indices[j]], key, s.semantic_work())?,
                s.semantic_work(),
            )?;
            s.semantic_work().visit(Visit::MemberCompare)?;
            // Conservative possible key-byte passes, not a measured short scan.
            let bytes = w::usize_add(s.semantic_work(), a.len(), b.len())?;
            s.semantic_work().scan_bytes(bytes)?;
            let comparison = if catalog {
                catalog_key_order(a, b)
            } else {
                mapping_key_order(a, b)
            };
            if !comparison.is_gt() {
                break;
            }
            s.semantic_work().visit(Visit::SortMove)?;
            s.semantic_work().write_bytes(2 * mem::size_of::<usize>())?;
            order.indices.swap(prior, j);
            j = prior;
        }
        i = w::usize_add(s.semantic_work(), i, 1)?;
    }
    for (destination, original) in order.indices.iter().enumerate() {
        s.semantic_work().visit(Visit::Element)?;
        s.semantic_work().write_bytes(mem::size_of::<usize>())?;
        order.destination[*original] = destination;
    }
    tree.reorder_existing_array(name, &mut order.destination, s.semantic_work())
}
pub(super) fn normalize(
    raw: &[u8],
    family: Family,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        if w::compare(s.semantic_work(), raw.len(), 262144usize)?.is_gt() {
            return Err(fail("SLOT_LIMIT", "private structural byte limit"));
        }
        let mut tree = s.parse_representation(raw, Kind::SourceDefinition)?;
        super::super::facts::reject_null_with_work(tree.value(), s.semantic_work())?;
        let (schema, keys) = match family {
            Family::Catalog => (
                "ledger-workflow-slot-catalog/1",
                &[
                    "schema",
                    "agreement_id",
                    "customer",
                    "revision",
                    "components",
                    "families",
                ][..],
            ),
            Family::SlotInstance => (
                "ledger-workflow-slot-instantiation/1",
                &[
                    "schema",
                    "agreement_id",
                    "customer",
                    "revision",
                    "template_ref",
                    "catalog_ref",
                    "component_map",
                    "family_map",
                ][..],
            ),
        };
        // A previous structural revision is already unsupported by the old
        // parser. No successor/history authority is introduced by this codec.
        closed(tree.value(), keys, s.semantic_work())?;
        let actual = role_string(
            field(tree.value(), "schema", s.semantic_work())?,
            s.semantic_work(),
        )?;
        if !role_equal(actual, schema, s.semantic_work())? {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "structural schema"));
        }
        let revision = role_string(
            field(tree.value(), "revision", s.semantic_work())?,
            s.semantic_work(),
        )?;
        initial_value(revision, false, s.semantic_work())?;
        let one = s.parse_representation(b"\"1\"", Kind::SourceDefinition)?;
        tree.replace_existing_owned("revision", one, s.semantic_work())?;
        match family {
            Family::Catalog => {
                reorder(&mut tree, "components", "slot_id", true, 1, 128, s)?;
                reorder(&mut tree, "families", "slot_id", true, 0, 64, s)?;
            }
            Family::SlotInstance => {
                reorder(&mut tree, "component_map", "template_key", false, 0, 128, s)?;
                reorder(&mut tree, "family_map", "template_key", false, 0, 64, s)?;
            }
        }
        s.canonical_representation(tree.value(), Kind::SourceDefinition)
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
