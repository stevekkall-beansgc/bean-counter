//! Selected complete source construction uses the SAME Product normalizer.
//! Terminal-unit refusal precedes serde, exactly as the existing selected
//! compiler: unsupported ratio serde never executes under this entry.
use super::super::facts::source as paid;
use super::*;
use std::mem;
pub(super) fn terminal(v: &Value, w: &mut dyn ValidationWork) -> Result<()> {
    if let Some(units) = paid::member(v, "units", w)?.and_then(Value::as_array) {
        for unit in units {
            w.visit(Visit::Element)?;
            w.visit(Visit::Constraint)?;
            if paid::member(unit, "normalization", w)?.is_some() {
                return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "selected terminal unit"));
            }
        }
    }
    Ok(())
}
fn string_payload(v: &Value, w: &mut dyn ValidationWork) -> Result<usize> {
    w.visit(Visit::Value)?;
    let mut bytes = 0;
    match v {
        Value::String(s) => bytes = s.len(),
        Value::Array(rows) => {
            for row in rows {
                w.visit(Visit::Element)?;
                bytes = paid::add(bytes, string_payload(row, w)?, w)?;
            }
        }
        Value::Object(rows) => {
            for row in rows.values() {
                w.visit(Visit::Field)?;
                bytes = paid::add(bytes, string_payload(row, w)?, w)?;
            }
        }
        _ => {}
    }
    Ok(bytes)
}
pub(super) fn bound(
    v: &Value,
    supplied: &[FactSchema],
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    terminal(v, w)?;
    let mut total = crate::canonical::audit_arena::value_copy_payload_bound(v, w)?;
    total = paid::add(total, paid::error_payload(v, w)?, w)?;
    for (name, stride) in [
        ("units", mem::size_of::<UnitDto>()),
        ("activities", mem::size_of::<ActivityDto>()),
        ("fact_schemas", mem::size_of::<Reference>()),
        ("links", mem::size_of::<LinkDto>()),
    ] {
        if let Some(rows) = paid::member(v, name, w)?.and_then(Value::as_array) {
            total = paid::add(total, paid::mul(rows.len(), stride, w)?, w)?;
            let nodes = match name {
                "units" => paid::map_payload::<String, UnitDefinition>(rows.len(), w)?,
                "activities" => paid::map_payload::<String, Activity>(rows.len(), w)?,
                "fact_schemas" => paid::map_payload::<String, FactSchema>(rows.len(), w)?,
                _ => paid::map_payload::<String, LinkDefinition>(rows.len(), w)?,
            };
            total = paid::add(total, nodes, w)?;
            for row in rows {
                w.visit(Visit::Element)?;
                // Independent typed map payload, besides DTO and body copies.
                if name != "fact_schemas" {
                    total = paid::add(total, string_payload(row, w)?, w)?;
                }
                if let Some(key) =
                    paid::member(row, if name == "fact_schemas" { "id" } else { "name" }, w)?
                        .and_then(Value::as_str)
                {
                    total = paid::add(total, key.len(), w)?;
                }
                if let Some(predecessors) =
                    paid::member(row, "predecessor_activities", w)?.and_then(Value::as_array)
                {
                    total = paid::add(
                        total,
                        paid::mul(predecessors.len(), 2 * mem::size_of::<String>(), w)?,
                        w,
                    )?;
                }
            }
            if name == "fact_schemas" {
                // Keyed canonical Ref sort and its rebuilt Reference Vec.
                total = paid::add(
                    total,
                    paid::mul(
                        rows.len(),
                        mem::size_of::<(Vec<u8>, Reference)>()
                            + mem::size_of::<Reference>()
                            + mem::size_of::<(&str, &str)>(),
                        w,
                    )?,
                    w,
                )?;
                let mut text = 0;
                for row in rows {
                    text = paid::add(text, string_payload(row, w)?, w)?;
                }
                // At most six escape bytes per input byte, two geometrically
                // overlapping emitter capacities, plus fixed Ref punctuation.
                let keys = paid::add(paid::mul(12, text, w)?, paid::mul(rows.len(), 256, w)?, w)?;
                total = paid::add(total, keys, w)?;
            }
        }
    }
    total = paid::add(
        total,
        paid::map_payload::<String, &FactSchema>(supplied.len(), w)?,
        w,
    )?;
    total = paid::add(total, paid::mul(supplied.len(), 69, w)?, w)?;
    // At most one clone of each supplied exact schema can be selected (the
    // normalizer refuses duplicate references). Selection needs no unleased
    // pre-pass hashing: sum the concrete private candidate layouts first.
    for schema in supplied {
        w.visit(Visit::Element)?;
        total = paid::add(total, schema.clone_payload_bound_with_work(w)?, w)?;
    }
    // Canonical schema source-byte/digest/id checks run sequentially. Their
    // existing4MiB emitter ceiling admits at most8MiB realloc overlap; the
    // fixed digest/id buffers and key-order scratch are separately included.
    total = paid::add(total, 2 * canonical::BUNDLE_LIMIT + 1024, w)?;
    Ok(total)
}
pub(super) fn decode(v: &Value, w: &mut dyn ValidationWork) -> Result<()> {
    paid::error_work(v, w)?;
    paid::object_decode_work(
        v,
        &[
            "schema",
            "product_id",
            "version",
            "labels",
            "profile",
            "evaluator",
            "grammar",
            "limits_profile",
            "units",
            "activities",
            "fact_schemas",
            "links",
        ],
        w,
    )?;
    if let Some(labels) = paid::member(v, "labels", w)? {
        paid::object_decode_work(labels, &["title", "description"], w)?;
    }
    for (field, keys, stride) in [
        (
            "units",
            &["name", "labels", "dimension", "normalization"][..],
            mem::size_of::<UnitDto>(),
        ),
        (
            "activities",
            &[
                "name",
                "labels",
                "primitive",
                "fact_schema",
                "source_slot",
                "funding_leg",
            ][..],
            mem::size_of::<ActivityDto>(),
        ),
        (
            "fact_schemas",
            &["kind", "id", "digest"][..],
            mem::size_of::<Reference>(),
        ),
        (
            "links",
            &[
                "name",
                "child_activity",
                "predecessor_activities",
                "child_primitive",
                "predecessor_primitive",
                "minimum",
                "maximum",
                "binding_relation",
                "supplier_slot",
                "on_missing",
                "max_depth",
            ][..],
            mem::size_of::<LinkDto>(),
        ),
    ] {
        if let Some(rows) = paid::member(v, field, w)?.and_then(Value::as_array) {
            let bytes = paid::mul(rows.len(), stride, w)?;
            w.write_bytes(bytes)?;
            for row in rows {
                w.visit(Visit::Element)?;
                paid::object_decode_work(row, keys, w)?;
                if let Some(labels) = paid::member(row, "labels", w)? {
                    paid::object_decode_work(labels, &["title", "description"], w)?;
                }
                if let Some(reference) = paid::member(row, "fact_schema", w)? {
                    paid::object_decode_work(reference, &["kind", "id", "digest"], w)?;
                }
                for name in [
                    "primitive",
                    "child_primitive",
                    "predecessor_primitive",
                    "binding_relation",
                    "on_missing",
                ] {
                    if let Some(value) = paid::member(row, name, w)?.and_then(Value::as_str) {
                        for candidate in [
                            "work",
                            "outcome",
                            "same_binding",
                            "same_agreement",
                            "declared_supplier",
                            "refuse",
                            "wait",
                        ] {
                            w.visit(Visit::MemberCompare)?;
                            w.scan_bytes(value.len())?;
                            w.scan_bytes(candidate.len())?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
fn labels(v: &Labels, w: &mut dyn ValidationWork) -> Result<Labels> {
    Ok(Labels {
        title: paid::text(&v.title, w)?,
        description: v
            .description
            .as_deref()
            .map(|v| paid::text(v, w))
            .transpose()?,
    })
}
pub(super) fn unit(v: &UnitDto, w: &mut dyn ValidationWork) -> Result<UnitDefinition> {
    if v.normalization.is_some() {
        return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "selected terminal unit"));
    }
    Ok(UnitDefinition(UnitDto {
        name: paid::text(&v.name, w)?,
        labels: labels(&v.labels, w)?,
        dimension: paid::text(&v.dimension, w)?,
        normalization: None,
    }))
}
fn reference(v: &Reference, w: &mut dyn ValidationWork) -> Result<Reference> {
    Ok(Reference {
        kind: paid::text(&v.kind, w)?,
        id: paid::text(&v.id, w)?,
        digest: paid::text(&v.digest, w)?,
    })
}
pub(super) fn activity(v: &ActivityDto, w: &mut dyn ValidationWork) -> Result<Activity> {
    Ok(Activity(ActivityDto {
        name: paid::text(&v.name, w)?,
        labels: labels(&v.labels, w)?,
        primitive: v.primitive,
        fact_schema: reference(&v.fact_schema, w)?,
        source_slot: paid::text(&v.source_slot, w)?,
        funding_leg: v
            .funding_leg
            .as_deref()
            .map(|v| paid::text(v, w))
            .transpose()?,
    }))
}
pub(super) fn link(v: &LinkDto, w: &mut dyn ValidationWork) -> Result<LinkDto> {
    Ok(LinkDto {
        name: paid::text(&v.name, w)?,
        child_activity: paid::text(&v.child_activity, w)?,
        predecessor_activities: paid::strings(&v.predecessor_activities, w)?,
        child_primitive: v.child_primitive,
        predecessor_primitive: v.predecessor_primitive,
        minimum: paid::text(&v.minimum, w)?,
        maximum: paid::text(&v.maximum, w)?,
        binding_relation: v.binding_relation,
        supplier_slot: v
            .supplier_slot
            .as_deref()
            .map(|v| paid::text(v, w))
            .transpose()?,
        on_missing: v.on_missing,
        max_depth: paid::text(&v.max_depth, w)?,
    })
}
fn labels_body(v: &Labels, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    paid::put(&mut out, "title", paid::literal(&v.title, w)?, w)?;
    if let Some(v) = &v.description {
        paid::put(&mut out, "description", paid::literal(v, w)?, w)?;
    }
    Ok(Value::Object(out))
}
fn reference_body(v: &Reference, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (name, value) in [("kind", &v.kind), ("id", &v.id), ("digest", &v.digest)] {
        paid::put(&mut out, name, paid::literal(value, w)?, w)?;
    }
    Ok(Value::Object(out))
}
fn primitive(v: Primitive) -> &'static str {
    match v {
        Primitive::Work => "work",
        Primitive::Outcome => "outcome",
    }
}
fn relation(v: BindingRelation) -> &'static str {
    match v {
        BindingRelation::SameBinding => "same_binding",
        BindingRelation::SameAgreement => "same_agreement",
        BindingRelation::DeclaredSupplier => "declared_supplier",
    }
}
fn missing(v: MissingDependency) -> &'static str {
    match v {
        MissingDependency::Refuse => "refuse",
        MissingDependency::Wait => "wait",
    }
}
fn array<T>(
    values: &[T],
    w: &mut dyn ValidationWork,
    mut body: impl FnMut(&T, &mut dyn ValidationWork) -> Result<Value>,
) -> Result<Value> {
    let bytes = paid::mul(values.len(), mem::size_of::<Value>(), w)?;
    w.write_bytes(bytes)?;
    let mut out = Vec::new();
    out.try_reserve_exact(values.len())
        .map_err(|_| fail("PRODUCT_LIMIT", "output array allocation"))?;
    for value in values {
        w.visit(Visit::Element)?;
        out.push(body(value, w)?);
    }
    Ok(Value::Array(out))
}
pub(super) fn body(dto: &ProductDto, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in [
        ("schema", dto.schema.as_str()),
        ("product_id", &dto.product_id),
        ("version", &dto.version),
        ("profile", &dto.profile),
        ("evaluator", &dto.evaluator),
        ("grammar", &dto.grammar),
        ("limits_profile", &dto.limits_profile),
    ] {
        paid::put(&mut out, key, paid::literal(value, w)?, w)?;
    }
    paid::put(&mut out, "labels", labels_body(&dto.labels, w)?, w)?;
    let units = array(&dto.units, w, |v, w| {
        let mut out = serde_json::Map::new();
        if v.normalization.is_some() {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "selected terminal unit"));
        }
        for (key, value) in [("name", &v.name), ("dimension", &v.dimension)] {
            paid::put(&mut out, key, paid::literal(value, w)?, w)?;
        }
        paid::put(&mut out, "labels", labels_body(&v.labels, w)?, w)?;
        Ok(Value::Object(out))
    })?;
    paid::put(&mut out, "units", units, w)?;
    let activities = array(&dto.activities, w, |v, w| {
        let mut out = serde_json::Map::new();
        for (key, value) in [
            ("name", v.name.as_str()),
            ("primitive", primitive(v.primitive)),
            ("source_slot", &v.source_slot),
        ] {
            paid::put(&mut out, key, paid::literal(value, w)?, w)?;
        }
        paid::put(&mut out, "labels", labels_body(&v.labels, w)?, w)?;
        paid::put(
            &mut out,
            "fact_schema",
            reference_body(&v.fact_schema, w)?,
            w,
        )?;
        if let Some(v) = &v.funding_leg {
            paid::put(&mut out, "funding_leg", paid::literal(v, w)?, w)?;
        }
        Ok(Value::Object(out))
    })?;
    paid::put(&mut out, "activities", activities, w)?;
    let schemas = array(&dto.fact_schemas, w, reference_body)?;
    paid::put(&mut out, "fact_schemas", schemas, w)?;
    let links = array(&dto.links, w, |v, w| {
        let mut out = serde_json::Map::new();
        for (key, value) in [
            ("name", v.name.as_str()),
            ("child_activity", &v.child_activity),
            ("child_primitive", primitive(v.child_primitive)),
            ("predecessor_primitive", primitive(v.predecessor_primitive)),
            ("minimum", &v.minimum),
            ("maximum", &v.maximum),
            ("binding_relation", relation(v.binding_relation)),
            ("on_missing", missing(v.on_missing)),
            ("max_depth", &v.max_depth),
        ] {
            paid::put(&mut out, key, paid::literal(value, w)?, w)?;
        }
        paid::put(
            &mut out,
            "predecessor_activities",
            paid::string_array(&v.predecessor_activities, w)?,
            w,
        )?;
        if let Some(v) = &v.supplier_slot {
            paid::put(&mut out, "supplier_slot", paid::literal(v, w)?, w)?;
        }
        Ok(Value::Object(out))
    })?;
    paid::put(&mut out, "links", links, w)?;
    Ok(Value::Object(out))
}
pub(super) fn references(values: &mut Vec<Reference>, w: &mut dyn ValidationWork) -> Result<()> {
    let bytes = paid::mul(values.len(), mem::size_of::<(Vec<u8>, Reference)>(), w)?;
    w.write_bytes(bytes)?;
    let mut keys = Vec::new();
    keys.try_reserve_exact(values.len())
        .map_err(|_| fail("PRODUCT_LIMIT", "schema sort allocation"))?;
    for value in values.drain(..) {
        w.visit(Visit::Element)?;
        let body = reference_body(&value, w)?;
        let key = CanonicalBytes::from_representation_with_work(&body, w, canonical::BUNDLE_LIMIT)?
            .into_vec();
        keys.push((key, value));
    }
    paid::sort(&mut keys, w, |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(a.0.len())?;
        w.scan_bytes(b.0.len())?;
        Ok(a.0.cmp(&b.0))
    })?;
    for pair in keys.windows(2) {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(pair[0].0.len())?;
        w.scan_bytes(pair[1].0.len())?;
        if pair[0].0 == pair[1].0 {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    let bytes = paid::mul(keys.len(), mem::size_of::<Reference>(), w)?;
    w.write_bytes(bytes)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(keys.len())
        .map_err(|_| fail("PRODUCT_LIMIT", "schema set allocation"))?;
    for (_, value) in keys {
        w.visit(Visit::Element)?;
        refs.push(value);
    }
    *values = refs;
    Ok(())
}
pub(super) fn names(values: &mut Vec<String>, w: &mut dyn ValidationWork) -> Result<()> {
    paid::sort(values, w, |a, b, w| paid::text_cmp(a, b, w))?;
    for pair in values.windows(2) {
        if paid::text_cmp(&pair[0], &pair[1], w)?.is_eq() {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    Ok(())
}
pub(super) fn compare(
    a: &str,
    b: &str,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<Ordering> {
    if complete {
        paid::text_cmp(a, b, w)
    } else {
        member_compare(a, b, w)
    }
}
pub(super) fn digest_diff(
    supplied: &str,
    schema: &FactSchema,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<bool> {
    let expected = if complete {
        schema.content_digest_complete_with_work(w)?
    } else {
        schema.content_digest_with_work(w)?
    };
    paid::different(supplied, &expected, w, complete)
}
