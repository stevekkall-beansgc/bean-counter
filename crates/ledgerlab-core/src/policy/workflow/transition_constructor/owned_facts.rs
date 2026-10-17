//! Owning output of the EXISTING fact normalizer. This reservation covers its
//! two typed maps, copied strings, proof-name vector and scalar string scratch.
//! It is not an origin, contextual attestation or whole-interpreter arena proof.
use super::super::{FactSchema, FactValue, NormalizedFacts};
use super::{CoreMeter, shape};
use crate::{
    Result,
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, ValidationVisit, ValidationWork},
};
use serde_json::Value;
use std::mem;

pub(in crate::policy::workflow) struct OwnedNormalizedFacts {
    // Payload precedes reservation: failure/drop destroys all maps and strings
    // before returning their owned requested-payload allowance.
    facts: NormalizedFacts,
    _reservation: ExternalBufferLease,
}
impl OwnedNormalizedFacts {
    pub(in crate::policy::workflow) fn facts(&self) -> &NormalizedFacts {
        &self.facts
    }
}
fn add(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    w::usize_add(work, a, b)
}
fn mul(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("fact allocation product"))
}
fn max(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    Ok(if w::compare(work, a, b)?.is_gt() {
        a
    } else {
        b
    })
}
// The SAME pinned alloc BTree layout used by canonical/audit_arena: B=6,
// 11 key/value positions, 12 child edges. Two internal-node upper bounds per
// entry cover retained nodes and a complete split/root transient. FactValue
// has its own (possibly i128-aligned) layout; do not assume Value's alignment.
fn typed_entry(
    work: &mut dyn ValidationWork,
    value_size: usize,
    value_align: usize,
) -> Result<usize> {
    let align = max(work, value_align, mem::align_of::<String>())?;
    let align = max(work, align, mem::align_of::<usize>())?;
    let header = add(work, mem::size_of::<usize>(), 4)?;
    let keys = mul(work, 11, mem::size_of::<String>())?;
    let values = mul(work, 11, value_size)?;
    let edges = mul(work, 12, mem::size_of::<usize>())?;
    let slack = w::usize_sub(work, align, 1)?;
    let padding = mul(work, 8, slack)?;
    let n = add(work, header, keys)?;
    let n = add(work, n, values)?;
    let n = add(work, n, edges)?;
    let n = add(work, n, padding)?;
    let n = add(work, n, slack)?;
    work.arithmetic(
        ArithmeticOperation::DivideRemainder,
        &[&n.into(), &align.into()],
    )?;
    let n = n / align;
    let n = mul(work, n, align)?;
    mul(work, 2, n)
}
fn output_reservation(m: &mut CoreMeter, schema: &FactSchema, input: &Value) -> Result<usize> {
    let fields = shape::array(m, schema.canonical_value(), "fields")?;
    let typed = typed_entry(m, mem::size_of::<FactValue>(), mem::align_of::<FactValue>())?;
    let canonical = typed_entry(m, mem::size_of::<Value>(), mem::align_of::<Value>())?;
    let maps = add(m, typed, canonical)?;
    let mut bytes = mul(m, fields.len(), maps)?;
    // required_proofs uses controlled exact capacities4,8,... . Reserving
    // 4*n String positions covers old+new growth, including the first cap4.
    // All name payload copies are counted independently below.
    let slots = mul(m, 4, fields.len())?;
    let vector = mul(m, slots, mem::size_of::<String>())?;
    bytes = add(m, bytes, vector)?;
    bytes = add(m, bytes, mem::size_of::<NormalizedFacts>())?;
    for field in fields {
        m.visit("owned_facts.field_allocation")?;
        let name = shape::text(m, field, "name")?;
        // Typed key, canonical key and (when attested) proof name. Counting all
        // three is conservative even for a missing optional or caller field.
        let names = mul(m, 3, name.len())?;
        bytes = add(m, bytes, names)?;
        let kind = shape::text(m, &field["type"], "kind")?;
        let scalar = if shape::same_text(m, kind, "string")? || shape::same_text(m, kind, "enum")? {
            let length = input
                .get(name)
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0);
            mul(m, 2, length)? // FactValue + canonical Value's independent copy
        } else if shape::same_text(m, kind, "integer")? || shape::same_text(m, kind, "decimal")? {
            // SignedScalar rejects raw>64 and normalized coefficient>30 BEFORE
            // string construction. Sum of all requested capacities (stronger
            // than peak): formatted digits<=2*64; unsigned Vec growth8+16+32
            // plus reversed32; zero padding<=19; pad format<=2*(19+32);
            // two possible String insert growth series each<=32+64+128;
            // canonical body copy<=32. Stack arithmetic scratch is separate.
            let digits = mul(m, 2, 64)?;
            let u1 = add(m, 8, 16)?;
            let u2 = add(m, 32, 32)?;
            let unsigned = add(m, u1, u2)?;
            let pad = add(m, 19, 32)?;
            let padded = mul(m, 2, pad)?;
            let ins = add(m, 32, 64)?;
            let ins = add(m, ins, 128)?;
            let inserts = mul(m, 2, ins)?;
            let n = add(m, digits, unsigned)?;
            let n = add(m, n, 19)?;
            let n = add(m, n, padded)?;
            let n = add(m, n, inserts)?;
            add(m, n, 32)?
        } else if shape::same_text(m, kind, "timestamp")? {
            // Timestamp::parse's single fixed-width UTC render is27 bytes;
            // dynamic-format growth up through64 consumes<4*64 requested
            // bytes in total; canonical body is an independent27-byte copy.
            let timestamp = mul(m, 4, 64)?;
            add(m, timestamp, 27)?
        } else if shape::same_text(m, kind, "boolean")? {
            0
        } else {
            return Err(shape::fail("unrecognized owned fact kind"));
        };
        bytes = add(m, bytes, scalar)?;
    }
    Ok(bytes)
}
pub(in crate::policy::workflow) fn normalize(
    arena: &AuditArena,
    m: &mut CoreMeter,
    schema: &FactSchema,
    input: &Value,
) -> Result<OwnedNormalizedFacts> {
    m.visit("owned_facts.preflight")?;
    let bytes = output_reservation(m, schema, input)?;
    let reservation = arena.reserve_external_payload(bytes, m)?;
    let facts = schema.normalize_value_complete_with_work(input, m)?;
    Ok(OwnedNormalizedFacts {
        facts,
        _reservation: reservation,
    })
}
