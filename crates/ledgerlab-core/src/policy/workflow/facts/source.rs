//! Private producer accounting for the SAME FactSchema semantic normalizer.
//! The caller reserves this bound before its input clone and retains its lease
//! through the typed schema lifetime. This module owns no allowance or arena.
use super::*;
use std::mem;
pub(in crate::policy::workflow) fn add(
    a: usize,
    b: usize,
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    vw::usize_add(w, a, b)
}
pub(in crate::policy::workflow) fn mul(
    a: usize,
    b: usize,
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    copy_size(w, a, b)
}

// Same pinned Rust48a229c B=6 node layout as the owning JSON parser. Keys and
// values can have different private typed layouts here. Two nodes per entry
// plus the possible root admit split overlap, not only dense final occupancy.
pub(in crate::policy::workflow) fn map_payload<K, V>(
    count: usize,
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    if vw::compare(w, count, 0usize)?.is_eq() {
        return Ok(0);
    }
    let align = if vw::compare(w, mem::align_of::<K>(), mem::align_of::<V>())?.is_gt() {
        mem::align_of::<K>()
    } else {
        mem::align_of::<V>()
    };
    let align = if vw::compare(w, align, mem::align_of::<usize>())?.is_gt() {
        align
    } else {
        mem::align_of::<usize>()
    };
    let header = add(mem::size_of::<usize>(), 4, w)?;
    let fields = add(
        mul(11, mem::size_of::<K>(), w)?,
        mul(11, mem::size_of::<V>(), w)?,
        w,
    )?;
    let edges = mul(12, mem::size_of::<usize>(), w)?;
    let padding = mul(8, vw::usize_sub(w, align, 1)?, w)?;
    let node = add(add(add(header, fields, w)?, edges, w)?, padding, w)?;
    let rounded = add(node, vw::usize_sub(w, align, 1)?, w)?;
    w.arithmetic(Op::DivideRemainder, &[&rounded.into(), &align.into()])?;
    let node = mul(rounded / align, align, w)?;
    mul(mul(2, add(count, 1, w)?, w)?, node, w)
}
pub(in crate::policy::workflow) fn member<'a>(
    v: &'a Value,
    key: &str,
    w: &mut dyn ValidationWork,
) -> Result<Option<&'a Value>> {
    let Some(map) = v.as_object() else {
        return Ok(None);
    };
    for (k, v) in map {
        w.visit(Visit::RefLookup)?;
        if text_cmp(k, key, w)?.is_eq() {
            return Ok(Some(v));
        }
    }
    Ok(None)
}
fn array_len(v: Option<&Value>) -> usize {
    v.and_then(Value::as_array).map_or(0, Vec::len)
}
pub(in crate::policy::workflow) fn text_cmp(
    a: &str,
    b: &str,
    w: &mut dyn ValidationWork,
) -> Result<Ordering> {
    w.scan_bytes(a.len())?;
    w.scan_bytes(b.len())?;
    member_compare(a, b, w)
}
pub(super) fn bound(value: &Value, w: &mut dyn ValidationWork) -> Result<usize> {
    // This one body graph also dominates the nonconcurrent serde tagged
    // Content buffer: Content has at most the Value layout, and object Content
    // pairs use contiguous slots rather than the larger admitted B-tree nodes.
    w.visit(Visit::Constraint)?;
    if vw::compare(
        w,
        mem::size_of::<serde::__private229::de::Content<'static>>(),
        mem::size_of::<Value>(),
    )?
    .is_gt()
    {
        return Err(fail(
            "FACT_SCHEMA",
            "unsupported pinned serde Content layout",
        ));
    }
    let mut total = crate::canonical::audit_arena::value_copy_payload_bound(value, w)?;
    total = add(total, error_payload(value, w)?, w)?;
    let fields = member(value, "fields", w)?.and_then(Value::as_array);
    let n = fields.map_or(0, Vec::len);
    let stride = add(
        add(mem::size_of::<FieldDto>(), mem::size_of::<Field>(), w)?,
        mem::size_of::<String>(),
        w,
    )?;
    total = add(total, mul(n, stride, w)?, w)?;
    total = add(total, mem::size_of::<SchemaDto>(), w)?;
    if let Some(fields) = fields {
        for field in fields {
            w.visit(Visit::Field)?;
            if let Some(name) = member(field, "name", w)?.and_then(Value::as_str) {
                total = add(total, mul(2, name.len(), w)?, w)?;
            }
            if let Some(t) = member(field, "type", w)? {
                let values = member(t, "values", w)?;
                let count = array_len(values);
                total = add(total, mul(count, mem::size_of::<String>(), w)?, w)?;
                total = add(total, map_payload::<String, ()>(count, w)?, w)?;
                if let Some(values) = values.and_then(Value::as_array) {
                    for v in values {
                        w.visit(Visit::Element)?;
                        if let Some(v) = v.as_str() {
                            total = add(total, v.len(), w)?;
                        }
                    }
                }
                if let Some(unit) = member(t, "unit", w)?.and_then(Value::as_str) {
                    total = add(total, unit.len(), w)?;
                }
                // Two numeric bounds: digit join, canonical coefficient, leading
                // padding, signed/dotted replacement, and final typed/body strings.
                // All producer widths follow the existing64-byte scalar syntax.
                let scalar_scratch = 64 + 64 + 20 + 66;
                let scalar_outputs = 2 * 64;
                let time_outputs = 4 * 27;
                total = add(
                    total,
                    add(
                        mul(2, scalar_scratch, w)?,
                        add(scalar_outputs, time_outputs, w)?,
                        w,
                    )?,
                    w,
                )?;
            }
        }
    }
    Ok(total)
}
/// Prepay the actual serde struct/tagged dictionary selection before its
/// execution. Owned String deserialization moves its input allocation; the
/// DTO/control storage is separately admitted by bound().
pub(super) fn decode_work(v: &Value, w: &mut dyn ValidationWork) -> Result<()> {
    error_work(v, w)?;
    fn object(v: &Value, keys: &[&str], w: &mut dyn ValidationWork) -> Result<()> {
        if let Some(map) = v.as_object() {
            for key in map.keys() {
                w.visit(Visit::Field)?;
                for candidate in keys {
                    w.visit(Visit::MemberCompare)?;
                    w.scan_bytes(key.len())?;
                    w.scan_bytes(candidate.len())?;
                }
            }
        }
        Ok(())
    }
    // Separate reservations for the existing closed-dictionary walk and the
    // derived serde dictionary dispatch; no later walk pays earlier checks.
    object(
        v,
        &["schema", "schema_id", "version", "normalization", "fields"],
        w,
    )?;
    object(
        v,
        &["schema", "schema_id", "version", "normalization", "fields"],
        w,
    )?;
    if let Some(fields) = member(v, "fields", w)?.and_then(Value::as_array) {
        let bytes = mul(fields.len(), mem::size_of::<FieldDto>(), w)?;
        w.write_bytes(bytes)?;
        for field in fields {
            w.visit(Visit::Field)?;
            object(field, &["name", "required", "type", "provenance"], w)?;
            object(field, &["name", "required", "type", "provenance"], w)?;
            for (key, names) in [
                (
                    "type",
                    &[
                        "kind",
                        "min_bytes",
                        "max_bytes",
                        "values",
                        "min",
                        "max",
                        "unit",
                        "max_scale",
                        "record_kind",
                        "relation",
                    ][..],
                ),
                (
                    "provenance",
                    &["kind", "source_slot", "evidence_kind", "caller", "attested"][..],
                ),
            ] {
                if let Some(v) = member(field, key, w)? {
                    object(v, names, w)?;
                    object(v, names, w)?;
                    if let Some(map) = v.as_object() {
                        let slots = mul(map.len(), 2, w)?;
                        let bytes = mul(
                            slots,
                            mem::size_of::<serde::__private229::de::Content<'static>>(),
                            w,
                        )?;
                        w.write_bytes(bytes)?;
                    }
                    if let Some(values) = member(v, "values", w)?.and_then(Value::as_array) {
                        let bytes = mul(values.len(), mem::size_of::<String>(), w)?;
                        w.write_bytes(bytes)?;
                    }
                    if let Some(evidence) = member(v, "evidence_kind", w)?.and_then(Value::as_str) {
                        for allowed in [
                            "occurrence",
                            "outcome",
                            "supplier_final_cost",
                            "meter_quantity",
                            "allocation_input",
                        ] {
                            w.scan_bytes(evidence.len())?;
                            w.scan_bytes(allowed.len())?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow) fn put(
    map: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Value,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    for prior in map.keys() {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(prior.len())?;
        w.scan_bytes(key.len())?;
    }
    let key = copy_text(key, w)?;
    w.write_bytes(mem::size_of::<(String, Value)>())?;
    map.insert(key, value);
    Ok(())
}
pub(in crate::policy::workflow) fn moved(s: String, w: &mut dyn ValidationWork) -> Result<Value> {
    w.write_bytes(mem::size_of::<Value>())?;
    Ok(Value::String(s))
}
pub(in crate::policy::workflow) fn literal(s: &str, w: &mut dyn ValidationWork) -> Result<Value> {
    moved(copy_text(s, w)?, w)
}
fn type_body(t: TypeDto, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    let kind = match &t {
        TypeDto::Boolean => "boolean",
        TypeDto::String { .. } => "string",
        TypeDto::Enum { .. } => "enum",
        TypeDto::Integer { .. } => "integer",
        TypeDto::Decimal { .. } => "decimal",
        TypeDto::Timestamp { .. } => "timestamp",
        TypeDto::Reference { .. } => "reference",
    };
    put(&mut out, "kind", literal(kind, w)?, w)?;
    match t {
        TypeDto::Boolean => {}
        TypeDto::String {
            min_bytes,
            max_bytes,
        } => {
            put(&mut out, "min_bytes", moved(min_bytes, w)?, w)?;
            put(&mut out, "max_bytes", moved(max_bytes, w)?, w)?;
        }
        TypeDto::Enum { values } => {
            let bytes = mul(values.len(), mem::size_of::<Value>(), w)?;
            w.write_bytes(bytes)?;
            let mut array = Vec::new();
            array
                .try_reserve_exact(values.len())
                .map_err(|_| fail("FACT_SCHEMA", "enum output allocation"))?;
            for v in values {
                w.visit(Visit::Element)?;
                array.push(moved(v, w)?);
            }
            put(&mut out, "values", Value::Array(array), w)?;
        }
        TypeDto::Integer { min, max, unit } => {
            put(&mut out, "min", moved(min, w)?, w)?;
            put(&mut out, "max", moved(max, w)?, w)?;
            if let Some(unit) = unit {
                put(&mut out, "unit", moved(unit, w)?, w)?;
            }
        }
        TypeDto::Decimal {
            min,
            max,
            max_scale,
            unit,
        } => {
            put(&mut out, "min", moved(min, w)?, w)?;
            put(&mut out, "max", moved(max, w)?, w)?;
            put(&mut out, "max_scale", moved(max_scale, w)?, w)?;
            if let Some(unit) = unit {
                put(&mut out, "unit", moved(unit, w)?, w)?;
            }
        }
        TypeDto::Timestamp { min, max } => {
            put(&mut out, "min", moved(min, w)?, w)?;
            put(&mut out, "max", moved(max, w)?, w)?;
        }
        TypeDto::Reference {
            record_kind,
            relation,
        } => {
            put(&mut out, "record_kind", moved(record_kind, w)?, w)?;
            put(&mut out, "relation", moved(relation, w)?, w)?;
        }
    }
    Ok(Value::Object(out))
}
pub(super) fn body(dto: SchemaDto, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (k, v) in [
        ("schema", dto.schema),
        ("schema_id", dto.schema_id),
        ("version", dto.version),
        ("normalization", dto.normalization),
    ] {
        put(&mut out, k, moved(v, w)?, w)?;
    }
    let bytes = mul(dto.fields.len(), mem::size_of::<Value>(), w)?;
    w.write_bytes(bytes)?;
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(dto.fields.len())
        .map_err(|_| fail("FACT_SCHEMA", "field output allocation"))?;
    for field in dto.fields {
        w.visit(Visit::Field)?;
        let mut value = serde_json::Map::new();
        put(&mut value, "name", moved(field.name, w)?, w)?;
        w.write_bytes(mem::size_of::<Value>())?;
        put(&mut value, "required", Value::Bool(field.required), w)?;
        put(&mut value, "type", type_body(field.field_type, w)?, w)?;
        let mut provenance = serde_json::Map::new();
        match field.provenance {
            ProvenanceDto::Caller => put(&mut provenance, "kind", literal("caller", w)?, w)?,
            ProvenanceDto::Attested {
                source_slot,
                evidence_kind,
            } => {
                put(&mut provenance, "kind", literal("attested", w)?, w)?;
                put(&mut provenance, "source_slot", moved(source_slot, w)?, w)?;
                put(
                    &mut provenance,
                    "evidence_kind",
                    moved(evidence_kind, w)?,
                    w,
                )?;
            }
        }
        put(&mut value, "provenance", Value::Object(provenance), w)?;
        fields.push(Value::Object(value));
    }
    put(&mut out, "fields", Value::Array(fields), w)?;
    Ok(Value::Object(out))
}
pub(super) fn enum_set(
    values: &mut Vec<String>,
    w: &mut dyn ValidationWork,
) -> Result<BTreeSet<String>> {
    // Qualified Name syntax was checked immediately before this call: its
    // ASCII alphabet contains no JSON escapes. Text order equals JCS string
    // order, so no detached serde/encoding key graph is needed here.
    sort(values, w, |a, b, w| text_cmp(a, b, w))?;
    for pair in values.windows(2) {
        if text_cmp(&pair[0], &pair[1], w)?.is_eq() {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    let mut out = BTreeSet::new();
    for value in values {
        w.visit(Visit::Element)?;
        for prior in &out {
            w.visit(Visit::MemberCompare)?;
            w.scan_bytes(String::len(prior))?;
            w.scan_bytes(value.len())?;
        }
        out.insert(copy_text(value, w)?);
    }
    Ok(out)
}

pub(in crate::policy::workflow) fn sort<T>(
    values: &mut [T],
    w: &mut dyn ValidationWork,
    mut compare: impl FnMut(&T, &T, &mut dyn ValidationWork) -> Result<Ordering>,
) -> Result<()> {
    let mut i = 1usize;
    while vw::compare(w, i, values.len())?.is_lt() {
        let mut j = i;
        while vw::compare(w, j, 0usize)?.is_gt() {
            let prior = vw::usize_sub(w, j, 1)?;
            if !compare(&values[prior], &values[j], w)?.is_gt() {
                break;
            }
            w.visit(Visit::SortMove)?;
            let copied = mul(2, mem::size_of::<T>(), w)?;
            w.write_bytes(copied)?;
            values.swap(prior, j);
            j = prior;
        }
        i = vw::usize_add(w, i, 1)?;
    }
    Ok(())
}

pub(super) fn clone_payload(schema: &FactSchema, w: &mut dyn ValidationWork) -> Result<usize> {
    let mut total = crate::canonical::audit_arena::value_copy_payload_bound(&schema.body, w)?;
    total = add(
        total,
        mul(schema.fields.len(), mem::size_of::<Field>(), w)?,
        w,
    )?;
    for field in &schema.fields {
        w.visit(Visit::Field)?;
        total = add(total, field.name.len(), w)?;
        match &field.field_type {
            FactType::Enum(values) => {
                total = add(total, map_payload::<String, ()>(values.len(), w)?, w)?;
                for value in values {
                    w.visit(Visit::Element)?;
                    total = add(total, value.len(), w)?;
                }
            }
            FactType::Number { min, max, unit, .. } => {
                total = add(total, add(min.as_str().len(), max.as_str().len(), w)?, w)?;
                if let Some(unit) = unit {
                    total = add(total, unit.len(), w)?;
                }
            }
            FactType::Time { min, max } => {
                total = add(total, add(min.as_str().len(), max.as_str().len(), w)?, w)?;
            }
            _ => {}
        }
    }
    Ok(total)
}
pub(super) fn clone_schema(schema: &FactSchema, w: &mut dyn ValidationWork) -> Result<FactSchema> {
    let payload = clone_payload(schema, w)?;
    // Pinned B-tree clone copies its nodes without comparisons. Reserving the
    // full admitted layout is a conservative byte-pass upper bound, not a
    // measurement that every padding byte was read or written.
    w.scan_bytes(payload)?;
    w.write_bytes(payload)?;
    Ok(schema.clone())
}
pub(super) fn document_digest(schema: &FactSchema, w: &mut dyn ValidationWork) -> Result<String> {
    document_digest_value("ledger-workflow-fact-schema/1", &schema.body, w)
}
pub(in crate::policy::workflow) fn document_digest_value(
    schema: &'static str,
    body: &Value,
    w: &mut dyn ValidationWork,
) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = CanonicalBytes::from_representation_with_work(body, w, canonical::BUNDLE_LIMIT)?;
    let prefix = add(4, schema.len(), w)?;
    let framed = add(add(prefix, bytes.as_slice().len(), w)?, 1, w)?;
    if vw::compare(w, framed, canonical::BUNDLE_LIMIT)?.is_gt() {
        return Err(Error::new("LIMIT", "canonical bundle bytes"));
    }
    let mut hash = Sha256::new();
    for part in [
        b"bean-counter/workflow-document/1\0".as_slice(),
        b"[\"".as_slice(),
        schema.as_bytes(),
        b"\",".as_slice(),
        bytes.as_slice(),
        b"]".as_slice(),
    ] {
        w.scan_bytes(part.len())?;
        w.hash_bytes(part.len())?;
        hash.update(part);
    }
    w.write_bytes(32)?;
    let digest = hash.finalize();
    let hex = canonical::hex_with_work(&digest, w)?;
    w.scan_bytes(hex.len())?;
    w.write_bytes(71)?;
    let mut out = String::new();
    out.try_reserve_exact(71)
        .map_err(|_| fail("FACT_SCHEMA", "digest allocation"))?;
    out.push_str("sha256:");
    out.push_str(&hex);
    Ok(out)
}
pub(in crate::policy::workflow) fn text(value: &str, w: &mut dyn ValidationWork) -> Result<String> {
    copy_text(value, w)
}
pub(in crate::policy::workflow) fn strings(
    values: &[String],
    w: &mut dyn ValidationWork,
) -> Result<Vec<String>> {
    let bytes = mul(values.len(), mem::size_of::<String>(), w)?;
    w.write_bytes(bytes)?;
    let mut out = Vec::new();
    out.try_reserve_exact(values.len())
        .map_err(|_| fail("FACT_SCHEMA", "string vector allocation"))?;
    for value in values {
        w.visit(Visit::Element)?;
        out.push(copy_text(value, w)?);
    }
    Ok(out)
}
pub(in crate::policy::workflow) fn string_array(
    values: &[String],
    w: &mut dyn ValidationWork,
) -> Result<Value> {
    let bytes = mul(values.len(), mem::size_of::<Value>(), w)?;
    w.write_bytes(bytes)?;
    let mut out = Vec::new();
    out.try_reserve_exact(values.len())
        .map_err(|_| fail("FACT_SCHEMA", "string array allocation"))?;
    for value in values {
        w.visit(Visit::Element)?;
        out.push(literal(value, w)?);
    }
    Ok(Value::Array(out))
}
pub(in crate::policy::workflow) fn prepare_insert<T>(
    map: &BTreeMap<String, T>,
    key: &str,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    for candidate in map.keys() {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(candidate.len())?;
        w.scan_bytes(key.len())?;
    }
    w.write_bytes(mem::size_of::<(String, T)>())
}
pub(in crate::policy::workflow) fn object_decode_work(
    value: &Value,
    keys: &[&str],
    w: &mut dyn ValidationWork,
) -> Result<()> {
    if let Some(map) = value.as_object() {
        for key in map.keys() {
            w.visit(Visit::Field)?;
            for candidate in keys {
                w.visit(Visit::MemberCompare)?;
                w.scan_bytes(key.len())?;
                w.scan_bytes(candidate.len())?;
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow) fn different(
    a: &str,
    b: &str,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<bool> {
    if complete {
        Ok(!text_cmp(a, b, w)?.is_eq())
    } else {
        Ok(a != b)
    }
}
pub(in crate::policy::workflow) fn lookup<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &str,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<Option<&'a T>> {
    if !complete {
        return map_lookup(map, key, w);
    }
    w.visit(Visit::RefLookup)?;
    for (name, value) in map {
        if text_cmp(name, key, w)?.is_eq() {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
/// Pinned serde_json value errors can quote one input string/field name using
/// Debug escapes (at most6 UTF8 bytes per input byte). Both its retained Message
/// and Display buffer may overlap. All fixed expected-field/variant lists in
/// these three closed DTOs plus formatting punctuation fit the explicit1KiB
/// literal envelope. This is a conservative error-path reservation, not an
/// observed byte count or a cap on input names.
pub(in crate::policy::workflow) fn error_payload(
    value: &Value,
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    fn text_bytes(v: &Value, w: &mut dyn ValidationWork) -> Result<usize> {
        w.visit(Visit::Value)?;
        let mut total = 0;
        match v {
            Value::String(v) => total = v.len(),
            Value::Array(values) => {
                for v in values {
                    w.visit(Visit::Element)?;
                    total = add(total, text_bytes(v, w)?, w)?;
                }
            }
            Value::Object(values) => {
                for (key, v) in values {
                    w.visit(Visit::Field)?;
                    total = add(total, key.len(), w)?;
                    total = add(total, text_bytes(v, w)?, w)?;
                }
            }
            _ => {}
        }
        Ok(total)
    }
    let text = text_bytes(value, w)?;
    mul(2, add(mul(6, text, w)?, 1024, w)?, w)
}
pub(in crate::policy::workflow) fn error_work(
    value: &Value,
    w: &mut dyn ValidationWork,
) -> Result<()> {
    let payload = error_payload(value, w)?;
    w.scan_bytes(payload)?;
    w.write_bytes(payload)
}
