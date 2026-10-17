//! Explicit owning app-acceptance terms. This executes the SAME Projection and
//! fact normalizers; custody, original publication membership and assent remain
//! host obligations. The legacy administrative parser/paid recipe is unchanged.
use super::*;
use crate::{
    canonical::audit_arena::{AuditArena, AuditedBytes, AuditedValue, ExternalBufferLease},
    validation_work::{
        self as w, ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as Kind,
        ValidationVisit as Visit, ValidationWork,
    },
};
use std::mem;

// Payload fields precede leases in every owner. Metadata growth itself is
// pre-admitted, with old+new Vec storage held across reserve_exact.
pub(super) struct Storage {
    leases: Vec<ExternalBufferLease>,
    metadata: Option<ExternalBufferLease>,
}
impl Storage {
    pub(super) fn new() -> Self {
        Self {
            leases: Vec::new(),
            metadata: None,
        }
    }
    pub(super) fn reserve(
        &mut self,
        n: usize,
        arena: &AuditArena,
        work: &mut dyn ValidationWork,
    ) -> Result<()> {
        if !w::compare(work, self.leases.len(), self.leases.capacity())?.is_lt() {
            let next = w::usize_add(work, self.leases.len(), 1)?;
            let bytes = mul(work, next, mem::size_of::<ExternalBufferLease>())?;
            let lease = arena.reserve_external_payload(bytes, work)?;
            let moved = mul(
                work,
                self.leases.len(),
                mem::size_of::<ExternalBufferLease>(),
            )?;
            work.write_bytes(moved)?;
            self.leases
                .try_reserve_exact(1)
                .map_err(|_| fail("acceptance lease metadata"))?;
            // Requested-payload accounting is not an allocator/RSS promise.
            if w::compare(work, self.leases.capacity(), next)?.is_gt() {
                return Err(fail("unexpected acceptance metadata capacity"));
            }
            self.metadata = Some(lease);
        }
        let lease = arena.reserve_external_payload(n, work)?;
        work.write_bytes(mem::size_of::<ExternalBufferLease>())?;
        self.leases.push(lease);
        Ok(())
    }
    pub(super) fn text(
        &mut self,
        v: &str,
        arena: &AuditArena,
        work: &mut dyn ValidationWork,
    ) -> Result<String> {
        self.reserve(v.len(), arena, work)?;
        paid::copy(v, work)
    }
    fn value(
        &mut self,
        v: &Value,
        arena: &AuditArena,
        work: &mut dyn ValidationWork,
    ) -> Result<Value> {
        let bytes = crate::canonical::audit_arena::value_copy_payload_bound(v, work)?;
        self.reserve(bytes, arena, work)?;
        Ok(v.clone())
    }
    pub(super) fn vector<T>(
        &mut self,
        n: usize,
        arena: &AuditArena,
        work: &mut dyn ValidationWork,
    ) -> Result<Vec<T>> {
        let bytes = mul(work, n, mem::size_of::<T>())?;
        self.reserve(bytes, arena, work)?;
        let mut out = Vec::new();
        out.try_reserve_exact(n)
            .map_err(|_| fail("acceptance vector allocation"))?;
        if w::compare(work, out.capacity(), n)?.is_gt() && mem::size_of::<T>() != 0 {
            return Err(fail("unexpected acceptance vector capacity"));
        }
        Ok(out)
    }
}
fn mul(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(Op::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| fail("acceptance allocation product"))
}
pub(super) fn field<'a>(
    v: &'a Value,
    key: &str,
    work: &mut dyn ValidationWork,
) -> Result<&'a Value> {
    work.visit(Visit::Constraint)?;
    for (name, v) in v.as_object().ok_or_else(|| fail("terms object"))? {
        work.visit(Visit::RefLookup)?;
        if paid::compare_text(name, key, work)?.is_eq() {
            return Ok(v);
        }
    }
    Err(fail("terms member"))
}
fn optional<'a>(
    v: &'a Value,
    key: &str,
    work: &mut dyn ValidationWork,
) -> Result<Option<&'a Value>> {
    work.visit(Visit::Constraint)?;
    for (name, v) in v.as_object().ok_or_else(|| fail("terms object"))? {
        work.visit(Visit::RefLookup)?;
        if paid::compare_text(name, key, work)?.is_eq() {
            return Ok(Some(v));
        }
    }
    Ok(None)
}
pub(super) fn closed(
    v: &Value,
    required: &[&str],
    optional: &[&str],
    work: &mut dyn ValidationWork,
) -> Result<()> {
    work.visit(Visit::Constraint)?;
    let object = v.as_object().ok_or_else(|| fail("terms object"))?;
    for (key, _) in object {
        work.visit(Visit::Field)?;
        if !paid::member(key, required, work)? && !paid::member(key, optional, work)? {
            return Err(fail("unknown terms field"));
        }
    }
    for key in required {
        field(v, key, work)?;
    }
    Ok(())
}
pub(super) fn text<'a>(v: &'a Value, work: &mut dyn ValidationWork) -> Result<&'a str> {
    work.visit(Visit::Constraint)?;
    v.as_str().ok_or_else(|| fail("terms string"))
}
pub(super) fn array<'a>(
    v: &'a Value,
    min: usize,
    max: usize,
    work: &mut dyn ValidationWork,
) -> Result<&'a [Value]> {
    work.visit(Visit::Constraint)?;
    let a = v.as_array().ok_or_else(|| fail("terms array"))?;
    if w::compare(work, a.len(), min)?.is_lt() || w::compare(work, a.len(), max)?.is_gt() {
        return Err(fail("terms array count"));
    }
    Ok(a)
}
pub(super) fn string(
    v: &Value,
    key: &str,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<String> {
    let v = text(field(v, key, w)?, w)?;
    s.text(v, a, w)
}
fn maybe_string(
    v: &Value,
    key: &str,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Option<String>> {
    optional(v, key, w)?
        .map(|v| s.text(text(v, w)?, a, w))
        .transpose()
}
fn values(
    v: &Value,
    key: &str,
    min: usize,
    max: usize,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Vec<Value>> {
    let rows = array(field(v, key, w)?, min, max, w)?;
    let mut out = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        let value = s.value(row, a, w)?;
        w.write_bytes(mem::size_of::<Value>())?;
        out.push(value);
    }
    Ok(out)
}
fn strings(
    v: &Value,
    key: &str,
    min: usize,
    max: usize,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Vec<String>> {
    let rows = array(field(v, key, w)?, min, max, w)?;
    let mut out = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        let value = s.text(text(row, w)?, a, w)?;
        w.write_bytes(mem::size_of::<String>())?;
        out.push(value);
    }
    Ok(out)
}
fn timestamp(
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Timestamp> {
    let bytes = mul(w, 4, 64)?;
    let bytes = w::usize_add(w, bytes, 27)?;
    s.reserve(bytes, a, w)?;
    Timestamp::parse_with_work(text(v, w)?, w)
}
pub(super) fn amount(
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Amount> {
    closed(v, &["currency", "scale", "atoms"], &[], w)?;
    let currency = string(v, "currency", s, a, w)?;
    let scale = field(v, "scale", w)?;
    w.visit(Visit::Constraint)?;
    let scale = scale.as_u64().ok_or_else(|| fail("terms scale"))?;
    if w::compare(w, scale, u64::from(u8::MAX))?.is_gt() {
        return Err(fail("terms scale"));
    }
    Ok(Amount {
        currency,
        scale: scale as u8,
        atoms: string(v, "atoms", s, a, w)?,
    })
}
pub(super) fn quantity_rows(
    value: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Vec<QuantityCeiling>> {
    let rows = array(value, 0, 64, w)?;
    let mut quantity_ceilings = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        closed(row, &["activity", "field", "unit", "maximum"], &[], w)?;
        let result = QuantityCeiling {
            activity: string(row, "activity", s, a, w)?,
            field: string(row, "field", s, a, w)?,
            unit: string(row, "unit", s, a, w)?,
            maximum: string(row, "maximum", s, a, w)?,
        };
        w.write_bytes(mem::size_of::<QuantityCeiling>())?;
        quantity_ceilings.push(result);
    }
    Ok(quantity_ceilings)
}
fn projection(
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Projection> {
    closed(
        v,
        &[
            "agreement_id",
            "version",
            "customer",
            "source_namespace",
            "slot_catalog_ref",
            "slot_instantiation_ref",
            "roles_refs",
            "source_slots",
            "accepted_context",
            "bindings",
            "funding_legs",
            "starts_at",
            "quantity_ceilings",
            "exposure_ceilings",
        ],
        &["ends_at"],
        w,
    )?;
    let agreement_id = string(v, "agreement_id", s, a, w)?;
    let version = string(v, "version", s, a, w)?;
    let customer = string(v, "customer", s, a, w)?;
    let source_namespace = string(v, "source_namespace", s, a, w)?;
    let slot_catalog_ref = s.value(field(v, "slot_catalog_ref", w)?, a, w)?;
    let slot_instantiation_ref = s.value(field(v, "slot_instantiation_ref", w)?, a, w)?;
    let roles_refs = values(v, "roles_refs", 1, 16, s, a, w)?;
    let rows = array(field(v, "source_slots", w)?, 1, 32, w)?;
    let mut source_slots = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        closed(row, &["name", "sources", "evidence_kinds"], &[], w)?;
        let result = SourceSlot {
            name: string(row, "name", s, a, w)?,
            sources: strings(row, "sources", 1, 16, s, a, w)?,
            evidence_kinds: strings(row, "evidence_kinds", 0, 5, s, a, w)?,
        };
        w.write_bytes(mem::size_of::<SourceSlot>())?;
        source_slots.push(result);
    }
    let context = field(v, "accepted_context", w)?;
    closed(context, &["fact_schema_ref", "facts"], &[], w)?;
    let accepted_context = Context {
        fact_schema_ref: s.value(field(context, "fact_schema_ref", w)?, a, w)?,
        facts: s.value(field(context, "facts", w)?, a, w)?,
    };
    let rows = array(field(v, "bindings", w)?, 1, 16, w)?;
    let mut bindings = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        closed(row, &["slot", "binding_ref"], &[], w)?;
        let result = BindingEntry {
            slot: string(row, "slot", s, a, w)?,
            binding_ref: s.value(field(row, "binding_ref", w)?, a, w)?,
        };
        w.write_bytes(mem::size_of::<BindingEntry>())?;
        bindings.push(result);
    }
    let rows = array(field(v, "funding_legs", w)?, 1, 16, w)?;
    let mut funding_legs = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        closed(
            row,
            &["name", "mode", "resource_owner", "payer", "evidence_refs"],
            &["provider", "supplier_binding_slot"],
            w,
        )?;
        let result = FundingLeg {
            name: string(row, "name", s, a, w)?,
            mode: string(row, "mode", s, a, w)?,
            resource_owner: string(row, "resource_owner", s, a, w)?,
            payer: string(row, "payer", s, a, w)?,
            provider: maybe_string(row, "provider", s, a, w)?,
            supplier_binding_slot: maybe_string(row, "supplier_binding_slot", s, a, w)?,
            evidence_refs: values(row, "evidence_refs", 0, 16, s, a, w)?,
        };
        w.write_bytes(mem::size_of::<FundingLeg>())?;
        funding_legs.push(result);
    }
    let starts_at = timestamp(field(v, "starts_at", w)?, s, a, w)?;
    let ends_at = optional(v, "ends_at", w)?
        .map(|v| timestamp(v, s, a, w))
        .transpose()?;
    let quantity_ceilings = quantity_rows(field(v, "quantity_ceilings", w)?, s, a, w)?;
    let rows = array(field(v, "exposure_ceilings", w)?, 0, 16, w)?;
    let mut exposure_ceilings = s.vector(rows.len(), a, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        closed(row, &["binding_slot", "maximum"], &[], w)?;
        let result = Exposure {
            binding_slot: string(row, "binding_slot", s, a, w)?,
            maximum: amount(field(row, "maximum", w)?, s, a, w)?,
        };
        w.write_bytes(mem::size_of::<Exposure>())?;
        exposure_ceilings.push(result);
    }
    Ok(Projection {
        agreement_id,
        version,
        customer,
        source_namespace,
        slot_catalog_ref,
        slot_instantiation_ref,
        roles_refs,
        source_slots,
        accepted_context,
        bindings,
        funding_legs,
        starts_at,
        ends_at,
        quantity_ceilings,
        exposure_ceilings,
    })
}
fn decode(
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<TermsInput> {
    closed(
        v,
        &[
            "schema",
            "scope",
            "publication_ref",
            "agreement_id",
            "customer",
            "version",
            "binding_inputs",
        ],
        &[],
        w,
    )?;
    let scope = array(field(v, "scope", w)?, 2, 2, w)?;
    Ok(TermsInput {
        schema: string(v, "schema", s, a, w)?,
        scope: [
            s.text(text(&scope[0], w)?, a, w)?,
            s.text(text(&scope[1], w)?, a, w)?,
        ],
        publication_ref: s.value(field(v, "publication_ref", w)?, a, w)?,
        agreement_id: string(v, "agreement_id", s, a, w)?,
        customer: string(v, "customer", s, a, w)?,
        version: string(v, "version", s, a, w)?,
        binding_inputs: projection(field(v, "binding_inputs", w)?, s, a, w)?,
    })
}

// Fixed DTO composition only; strings and arbitrary Values always use the SAME
// complete canonical emitter. No second general JSON/string encoder is used.
pub(super) struct Encode<'a> {
    pub(super) storage: &'a mut Storage,
    pub(super) arena: &'a AuditArena,
}
impl Encode<'_> {
    pub(super) fn value(&mut self, v: &Value, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        self.arena.canonical_representation_bytes(v, w, 262144)
    }
    pub(super) fn text(&mut self, v: &str, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        let value = Value::String(self.storage.text(v, self.arena, w)?);
        self.value(&value, w)
    }
    pub(super) fn list<T>(
        &mut self,
        rows: &[T],
        w: &mut dyn ValidationWork,
        mut encode: impl FnMut(&mut Self, &T, &mut dyn ValidationWork) -> Result<AuditedBytes>,
    ) -> Result<AuditedBytes> {
        let mut parts = self
            .storage
            .vector::<AuditedBytes>(rows.len(), self.arena, w)?;
        let mut n = 2usize;
        for row in rows {
            w.visit(Visit::Element)?;
            let part = encode(self, row, w)?;
            n = w::usize_add(w, n, part.bytes().len())?;
            if !parts.is_empty() {
                n = w::usize_add(w, n, 1)?;
            }
            w.write_bytes(mem::size_of::<AuditedBytes>())?;
            parts.push(part);
        }
        self.join_list(&parts, n, w)
    }
    pub(super) fn join_list(
        &self,
        parts: &[AuditedBytes],
        n: usize,
        w: &mut dyn ValidationWork,
    ) -> Result<AuditedBytes> {
        if w::compare(w, n, 262144usize)?.is_gt() {
            return Err(fail("normalized terms byte limit"));
        }
        let mut b = self.arena.byte_builder(n, w)?;
        b.append(b"[", w)?;
        for (i, p) in parts.iter().enumerate() {
            w.visit(Visit::Element)?;
            if !w::compare(w, i, 0usize)?.is_eq() {
                b.append(b",", w)?;
            }
            b.append(p.bytes(), w)?;
        }
        b.append(b"]", w)?;
        b.finish(w)
    }
    pub(super) fn object(
        &self,
        parts: &[(&str, &[u8])],
        w: &mut dyn ValidationWork,
    ) -> Result<AuditedBytes> {
        let mut n = 2usize;
        for (i, (key, part)) in parts.iter().enumerate() {
            w.visit(Visit::Field)?;
            if !w::compare(w, i, 0usize)?.is_eq() {
                n = w::usize_add(w, n, 1)?;
            }
            n = w::usize_add(w, n, key.len())?;
            n = w::usize_add(w, n, 3)?;
            n = w::usize_add(w, n, part.len())?;
            if !w::compare(w, i, 0usize)?.is_eq()
                && !paid::compare_text(parts[i - 1].0, key, w)?.is_lt()
            {
                return Err(fail("terms field order"));
            }
        }
        if w::compare(w, n, 262144usize)?.is_gt() {
            return Err(fail("normalized terms byte limit"));
        }
        let mut b = self.arena.byte_builder(n, w)?;
        b.append(b"{", w)?;
        for (i, (key, part)) in parts.iter().enumerate() {
            w.visit(Visit::Field)?;
            if !w::compare(w, i, 0usize)?.is_eq() {
                b.append(b",", w)?;
            }
            b.append(b"\"", w)?;
            b.append(key.as_bytes(), w)?;
            b.append(b"\":", w)?;
            b.append(part, w)?;
        }
        b.append(b"}", w)?;
        b.finish(w)
    }
    fn object_optional(
        &self,
        parts: &[(&str, Option<&[u8]>)],
        w: &mut dyn ValidationWork,
    ) -> Result<AuditedBytes> {
        let mut n = 2usize;
        let mut prior = None;
        for (key, part) in parts {
            w.visit(Visit::Constraint)?;
            if let Some(part) = part {
                w.visit(Visit::Field)?;
                if let Some(old) = prior {
                    n = w::usize_add(w, n, 1)?;
                    if !paid::compare_text(old, key, w)?.is_lt() {
                        return Err(fail("terms field order"));
                    }
                }
                n = w::usize_add(w, n, key.len())?;
                n = w::usize_add(w, n, 3)?;
                n = w::usize_add(w, n, part.len())?;
                prior = Some(*key);
            }
        }
        if w::compare(w, n, 262144usize)?.is_gt() {
            return Err(fail("normalized terms byte limit"));
        }
        let mut b = self.arena.byte_builder(n, w)?;
        b.append(b"{", w)?;
        let mut first = true;
        for (key, part) in parts {
            w.visit(Visit::Constraint)?;
            if let Some(part) = part {
                w.visit(Visit::Field)?;
                w.visit(Visit::Constraint)?;
                if !first {
                    b.append(b",", w)?;
                }
                first = false;
                b.append(b"\"", w)?;
                b.append(key.as_bytes(), w)?;
                b.append(b"\":", w)?;
                b.append(part, w)?;
            }
        }
        b.append(b"}", w)?;
        b.finish(w)
    }
    fn amount(&mut self, v: &Amount, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        let atoms = self.text(&v.atoms, w)?;
        let currency = self.text(&v.currency, w)?;
        let scale = self.value(&Value::from(v.scale), w)?;
        self.object(
            &[
                ("atoms", atoms.bytes()),
                ("currency", currency.bytes()),
                ("scale", scale.bytes()),
            ],
            w,
        )
    }
    pub(super) fn quantities(
        &mut self,
        rows: &[QuantityCeiling],
        w: &mut dyn ValidationWork,
    ) -> Result<AuditedBytes> {
        self.list(rows, w, |e, r, w| {
            let activity = e.text(&r.activity, w)?;
            let field = e.text(&r.field, w)?;
            let maximum = e.text(&r.maximum, w)?;
            let unit = e.text(&r.unit, w)?;
            e.object(
                &[
                    ("activity", activity.bytes()),
                    ("field", field.bytes()),
                    ("maximum", maximum.bytes()),
                    ("unit", unit.bytes()),
                ],
                w,
            )
        })
    }
    fn projection(&mut self, v: &Projection, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        let facts = self.value(&v.accepted_context.facts, w)?;
        let fact_ref = self.value(&v.accepted_context.fact_schema_ref, w)?;
        let context = self.object(
            &[
                ("fact_schema_ref", fact_ref.bytes()),
                ("facts", facts.bytes()),
            ],
            w,
        )?;
        let agreement = self.text(&v.agreement_id, w)?;
        let bindings = self.list(&v.bindings, w, |e, r, w| {
            let slot = e.text(&r.slot, w)?;
            let rf = e.value(&r.binding_ref, w)?;
            e.object(&[("binding_ref", rf.bytes()), ("slot", slot.bytes())], w)
        })?;
        let customer = self.text(&v.customer, w)?;
        let ends = v
            .ends_at
            .as_ref()
            .map(|t| self.text(t.as_str(), w))
            .transpose()?;
        let exposures = self.list(&v.exposure_ceilings, w, |e, r, w| {
            let slot = e.text(&r.binding_slot, w)?;
            let amount = e.amount(&r.maximum, w)?;
            e.object(
                &[("binding_slot", slot.bytes()), ("maximum", amount.bytes())],
                w,
            )
        })?;
        let funding = self.list(&v.funding_legs, w, |e, r, w| {
            let evidence = e.list(&r.evidence_refs, w, |e, v, w| e.value(v, w))?;
            let mode = e.text(&r.mode, w)?;
            let name = e.text(&r.name, w)?;
            let payer = e.text(&r.payer, w)?;
            let provider = r.provider.as_ref().map(|s| e.text(s, w)).transpose()?;
            let resource = e.text(&r.resource_owner, w)?;
            let slot = r
                .supplier_binding_slot
                .as_ref()
                .map(|s| e.text(s, w))
                .transpose()?;
            e.object_optional(
                &[
                    ("evidence_refs", Some(evidence.bytes())),
                    ("mode", Some(mode.bytes())),
                    ("name", Some(name.bytes())),
                    ("payer", Some(payer.bytes())),
                    ("provider", provider.as_ref().map(AuditedBytes::bytes)),
                    ("resource_owner", Some(resource.bytes())),
                    (
                        "supplier_binding_slot",
                        slot.as_ref().map(AuditedBytes::bytes),
                    ),
                ],
                w,
            )
        })?;
        let quantities = self.quantities(&v.quantity_ceilings, w)?;
        let roles = self.list(&v.roles_refs, w, |e, v, w| e.value(v, w))?;
        let catalog = self.value(&v.slot_catalog_ref, w)?;
        let inst = self.value(&v.slot_instantiation_ref, w)?;
        let ns = self.text(&v.source_namespace, w)?;
        let sources = self.list(&v.source_slots, w, |e, r, w| {
            let kinds = e.list(&r.evidence_kinds, w, |e, v, w| e.text(v, w))?;
            let name = e.text(&r.name, w)?;
            let sources = e.list(&r.sources, w, |e, v, w| e.text(v, w))?;
            e.object(
                &[
                    ("evidence_kinds", kinds.bytes()),
                    ("name", name.bytes()),
                    ("sources", sources.bytes()),
                ],
                w,
            )
        })?;
        let starts = self.text(v.starts_at.as_str(), w)?;
        let version = self.text(&v.version, w)?;
        self.object_optional(
            &[
                ("accepted_context", Some(context.bytes())),
                ("agreement_id", Some(agreement.bytes())),
                ("bindings", Some(bindings.bytes())),
                ("customer", Some(customer.bytes())),
                ("ends_at", ends.as_ref().map(AuditedBytes::bytes)),
                ("exposure_ceilings", Some(exposures.bytes())),
                ("funding_legs", Some(funding.bytes())),
                ("quantity_ceilings", Some(quantities.bytes())),
                ("roles_refs", Some(roles.bytes())),
                ("slot_catalog_ref", Some(catalog.bytes())),
                ("slot_instantiation_ref", Some(inst.bytes())),
                ("source_namespace", Some(ns.bytes())),
                ("source_slots", Some(sources.bytes())),
                ("starts_at", Some(starts.bytes())),
                ("version", Some(version.bytes())),
            ],
            w,
        )
    }
    fn terms(&mut self, v: &TermsInput, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        let agreement = self.text(&v.agreement_id, w)?;
        let binding = self.projection(&v.binding_inputs, w)?;
        let customer = self.text(&v.customer, w)?;
        let publication = self.value(&v.publication_ref, w)?;
        let schema = self.text(&v.schema, w)?;
        let scope = self.list(&v.scope, w, |e, s, w| e.text(s, w))?;
        let version = self.text(&v.version, w)?;
        self.object(
            &[
                ("agreement_id", agreement.bytes()),
                ("binding_inputs", binding.bytes()),
                ("customer", customer.bytes()),
                ("publication_ref", publication.bytes()),
                ("schema", schema.bytes()),
                ("scope", scope.bytes()),
                ("version", version.bytes()),
            ],
            w,
        )
    }
}
// Fixed-domain streaming hashes; source/body bytes are not concatenated or
// copied into an ordinary unleased frame.
fn digest(
    domain: &[u8],
    parts: &[&[u8]],
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    w.hash_bytes(domain.len())?;
    hash.update(domain);
    for p in parts {
        w.scan_bytes(p.len())?;
        w.hash_bytes(p.len())?;
        hash.update(p);
    }
    w.write_bytes(32)?;
    let bytes = hash.finalize();
    s.reserve(64, a, w)?;
    let hex = canonical::hex_with_work(&bytes, w)?;
    s.reserve(71, a, w)?;
    w.write_bytes(71)?;
    w.scan_bytes(hex.len())?;
    let mut out = String::new();
    out.try_reserve_exact(71)
        .map_err(|_| fail("digest allocation"))?;
    out.push_str("sha256:");
    out.push_str(&hex);
    Ok(out)
}
fn document_reference(
    schema: &str,
    body: &[u8],
    kind: &str,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<AgreementRef> {
    let schema = Encode {
        storage: s,
        arena: a,
    }
    .text(schema, w)?;
    let digest = digest(
        b"bean-counter/workflow-document/1\0",
        &[b"[", schema.bytes(), b",", body, b"]"],
        s,
        a,
        w,
    )?;
    s.reserve(69, a, w)?;
    w.write_bytes(69)?;
    w.scan_bytes(64)?;
    let mut id = String::new();
    id.try_reserve_exact(69)
        .map_err(|_| fail("document ID allocation"))?;
    id.push_str("wdoc_");
    id.push_str(&digest[7..]);
    let n = w::usize_add(w, kind.len(), id.len())?;
    let n = w::usize_add(w, n, digest.len())?;
    s.reserve(n, a, w)?;
    AgreementRef::from_parts_with_work(kind, &id, &digest, w)
}
/// No Clone/Serde/into-Value escape. Canonical output and DATA references retain
/// their actual owning source/control/copy reservations for the entire lifetime.
pub struct OwnedAcceptanceTerms {
    body: AuditedValue,
    bytes: AuditedBytes,
    reference: AgreementRef,
    terms_digest: String,
    _storage: Storage,
}
impl OwnedAcceptanceTerms {
    pub fn parse(
        raw: &[u8],
        context: &super::super::FactSchema,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let result = Self::parse_inner(raw, context, session);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    fn parse_inner(
        raw: &[u8],
        context: &super::super::FactSchema,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let source = session.parse_representation(raw, Kind::SourceDefinition)?;
        let arena = session.arena().shared_handle();
        let work = session.meter_mut();
        let mut storage = Storage::new();
        let mut input = decode(source.value(), &mut storage, &arena, work)?;
        if !paid::compare_text(&input.schema, "ledger-workflow-acceptance-terms/2", work)?.is_eq() {
            return Err(fail("owning app acceptance terms schema"));
        }
        for id in [
            &input.scope[0],
            &input.scope[1],
            &input.agreement_id,
            &input.customer,
        ] {
            paid::text(id, 128, work)?;
        }
        // All temporary Ref strings are admitted before the SAME Ref codec.
        reference_scratch(&mut storage, &arena, work)?;
        paid::reference(&input.publication_ref, "workflow-publication", work)?;
        let schema =
            arena.canonical_representation_bytes(context.canonical_value(), work, 262144)?;
        let context_ref = document_reference(
            "ledger-workflow-fact-schema/1",
            schema.bytes(),
            "workflow-fact-schema",
            &mut storage,
            &arena,
            work,
        )?;
        normalization_scratch(&input.binding_inputs, &mut storage, &arena, work)?;
        input
            .binding_inputs
            .normalize_with_work(&context_ref, true, work, |facts, m| {
                context.validate_context_schema_with_work(m)?;
                let owned = super::super::transition_constructor::owned_facts::normalize(
                    &arena, m, context, facts,
                )?;
                storage.value(owned.facts().canonical_value(), &arena, m)
            })?;
        input.version = super::super::facts::counter_with_work(&input.version, work)
            .and_then(|n| w::unsigned_string(n.into(), work))?;
        if !paid::compare_text(
            &input.agreement_id,
            &input.binding_inputs.agreement_id,
            work,
        )?
        .is_eq()
            || !paid::compare_text(&input.customer, &input.binding_inputs.customer, work)?.is_eq()
            || !paid::compare_text(&input.version, &input.binding_inputs.version, work)?.is_eq()
        {
            return Err(fail("exact duplicated agreement/customer/version"));
        }
        let bytes = Encode {
            storage: &mut storage,
            arena: &arena,
        }
        .terms(&input, work)?;
        let reference = document_reference(
            &input.schema,
            bytes.bytes(),
            "workflow-acceptance-terms",
            &mut storage,
            &arena,
            work,
        )?;
        let terms_digest = digest(
            b"bean-counter/workflow-acceptance-terms/1\0",
            &[bytes.bytes()],
            &mut storage,
            &arena,
            work,
        )?;
        // Destroy all ordinary typed payload before its reservation owner can
        // escape. Only the final immutable owning artifacts are returned.
        drop(input);
        drop(source);
        let body = session.parse_representation(bytes.bytes(), Kind::SourceDefinition)?;
        Ok(Self {
            body,
            bytes,
            reference,
            terms_digest,
            _storage: storage,
        })
    }
    pub fn canonical_value(&self) -> &Value {
        self.body.value()
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.reference
    }
    pub fn terms_digest(&self) -> &str {
        &self.terms_digest
    }
}
pub(super) fn reference_scratch(
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    // Sequential closed Ref codec temporaries: kind<=128,id<=128,digest71;
    // two simultaneous refs at context comparison, each three String payloads.
    let n = w::usize_add(w, 128, 128)?;
    let n = w::usize_add(w, n, 71)?;
    let n = mul(w, 2, n)?;
    s.reserve(n, a, w)
}
fn normalization_scratch(
    v: &Projection,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    // Each string-set key uses the existing bounded canonical string emitter.
    // For valid control-free input L: copied Value L + emitter old/new4L+4
    // +retained key2L+2. Tuple/result Vec overlap is pre-admitted separately.
    for row in &v.source_slots {
        for rows in [&row.sources, &row.evidence_kinds] {
            let mut n = 0;
            for value in rows {
                let b = mul(w, 7, value.len())?;
                let b = w::usize_add(w, b, 6)?;
                n = w::usize_add(w, n, b)?;
            }
            let tuples = mul(w, 4, rows.len())?;
            let tuples = mul(w, tuples, mem::size_of::<(Vec<u8>, String)>())?;
            n = w::usize_add(w, n, tuples)?;
            let result = mul(w, rows.len(), mem::size_of::<String>())?;
            n = w::usize_add(w, n, result)?;
            s.reserve(n, a, w)?;
        }
    }
    scalar_scratch(v.quantity_ceilings.len(), s, a, w)
}
pub(super) fn scalar_scratch(
    quantity_count: usize,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    // Version unsigned formatting, and the SAME sequential SignedScalar parser
    // requested capacities (digits,reverse,padding,insertions,canonical copy).
    s.reserve(128, a, w)?;
    let digits = mul(w, 2, 64)?;
    let unsigned = w::usize_add(w, 8, 16)?;
    let unsigned = w::usize_add(w, unsigned, 64)?;
    let pad = w::usize_add(w, 19, 32)?;
    let pad = mul(w, 2, pad)?;
    let inserts = w::usize_add(w, 32, 64)?;
    let inserts = w::usize_add(w, inserts, 128)?;
    let inserts = mul(w, 2, inserts)?;
    let n = w::usize_add(w, digits, unsigned)?;
    let n = w::usize_add(w, n, 19)?;
    let n = w::usize_add(w, n, pad)?;
    let n = w::usize_add(w, n, inserts)?;
    let n = w::usize_add(w, n, 32)?;
    // Replaced quantity strings survive normalization. Each is<=32 bytes after
    // scalar bounds; originals and all other decoded strings remain admitted.
    let outputs = mul(w, quantity_count, 32)?;
    let n = w::usize_add(w, n, outputs)?;
    s.reserve(n, a, w)
}
