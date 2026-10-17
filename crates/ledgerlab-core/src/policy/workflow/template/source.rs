//! Same template normalization, with private typed producers admitted before
//! cloning. Old source constructors and their paid recipes remain separate.
use super::super::facts::source as paid;
use super::*;
use std::mem;
pub(super) fn bound(
    v: &Value,
    previous: Option<&SlotTemplate>,
    w: &mut dyn ValidationWork,
) -> Result<usize> {
    let mut total = crate::canonical::audit_arena::value_copy_payload_bound(v, w)?;
    total = paid::add(total, paid::error_payload(v, w)?, w)?;
    for (name, dto, node) in [
        (
            "components",
            mem::size_of::<ComponentDto>(),
            mem::size_of::<TemplateComponent>(),
        ),
        (
            "families",
            mem::size_of::<FamilyDto>(),
            mem::size_of::<TemplateFamily>(),
        ),
    ] {
        if let Some(rows) = paid::member(v, name, w)?.and_then(Value::as_array) {
            let controls = paid::mul(rows.len(), dto, w)?;
            total = paid::add(total, controls, w)?;
            let nodes = if name == "components" {
                paid::map_payload::<String, TemplateComponent>(rows.len(), w)?
            } else {
                paid::map_payload::<String, TemplateFamily>(rows.len(), w)?
            };
            total = paid::add(total, nodes, w)?;
            // The typed payload moves its own key/slot/aliases, independently
            // from the DTO and normalized JSON body retained during emission.
            for row in rows {
                w.visit(Visit::Element)?;
                for field in ["key", "root_key", "role_slot", "target_component_key"] {
                    if let Some(value) = paid::member(row, field, w)?.and_then(Value::as_str) {
                        total = paid::add(total, value.len(), w)?;
                    }
                }
                if let Some(key) = paid::member(row, "key", w)?.and_then(Value::as_str) {
                    total = paid::add(total, key.len(), w)?;
                }
                if let Some(aliases) = paid::member(row, "aliases", w)?.and_then(Value::as_array) {
                    // DTO and independent typed Vec<String> both coexist.
                    let controls = paid::mul(aliases.len(), 2 * mem::size_of::<String>(), w)?;
                    total = paid::add(total, controls, w)?;
                    for alias in aliases {
                        w.visit(Visit::Element)?;
                        if let Some(alias) = alias.as_str() {
                            total = paid::add(total, alias.len(), w)?;
                        }
                    }
                }
            }
            let _ = node; // Concrete private node layout is used by map_payload.
        }
    }
    if previous.is_some() {
        // Existing4MiB canonical output can temporarily grow to twice its
        // final ceiling; digest/id render buffers are fixed and overlapping.
        total = paid::add(total, 2 * canonical::BUNDLE_LIMIT + 1024, w)?;
    }
    Ok(total)
}
pub(super) fn decode(v: &Value, w: &mut dyn ValidationWork) -> Result<()> {
    paid::error_work(v, w)?;
    paid::object_decode_work(
        v,
        &[
            "schema",
            "template_id",
            "version",
            "previous",
            "components",
            "families",
        ],
        w,
    )?;
    for name in ["components", "families"] {
        if let Some(rows) = paid::member(v, name, w)?.and_then(Value::as_array) {
            let stride = if name == "components" {
                mem::size_of::<ComponentDto>()
            } else {
                mem::size_of::<FamilyDto>()
            };
            let bytes = paid::mul(rows.len(), stride, w)?;
            w.write_bytes(bytes)?;
            for row in rows {
                w.visit(Visit::Element)?;
                paid::object_decode_work(
                    row,
                    &[
                        "key",
                        "book",
                        "root_key",
                        "role_slot",
                        "aliases",
                        "target_component_key",
                    ],
                    w,
                )?;
                if let Some(book) = paid::member(row, "book", w)?.and_then(Value::as_str) {
                    for name in ["retail", "supplier", "cost_observation"] {
                        w.visit(Visit::MemberCompare)?;
                        w.scan_bytes(book.len())?;
                        w.scan_bytes(name.len())?;
                    }
                }
                if let Some(aliases) = paid::member(row, "aliases", w)?.and_then(Value::as_array) {
                    let bytes = paid::mul(aliases.len(), mem::size_of::<String>(), w)?;
                    w.write_bytes(bytes)?;
                }
            }
        }
    }
    if let Some(reference) = paid::member(v, "previous", w)? {
        paid::object_decode_work(reference, &["kind", "id", "digest"], w)?;
    }
    Ok(())
}
pub(super) fn component(v: &ComponentDto, w: &mut dyn ValidationWork) -> Result<TemplateComponent> {
    Ok(TemplateComponent {
        key: paid::text(&v.key, w)?,
        book: v.book,
        root_key: paid::text(&v.root_key, w)?,
        role_slot: paid::text(&v.role_slot, w)?,
        aliases: paid::strings(&v.aliases, w)?,
    })
}
pub(super) fn family(v: &FamilyDto, w: &mut dyn ValidationWork) -> Result<TemplateFamily> {
    Ok(TemplateFamily {
        key: paid::text(&v.key, w)?,
        target_component_key: paid::text(&v.target_component_key, w)?,
        role_slot: paid::text(&v.role_slot, w)?,
        aliases: paid::strings(&v.aliases, w)?,
    })
}
fn book(v: TemplateBook) -> &'static str {
    match v {
        TemplateBook::Retail => "retail",
        TemplateBook::Supplier => "supplier",
        TemplateBook::CostObservation => "cost_observation",
    }
}
pub(super) fn body(dto: &TemplateDto, w: &mut dyn ValidationWork) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (name, value) in [
        ("schema", dto.schema.as_str()),
        ("template_id", &dto.template_id),
        ("version", &dto.version),
    ] {
        paid::put(&mut out, name, paid::literal(value, w)?, w)?;
    }
    if let Some(reference) = &dto.previous {
        let mut value = serde_json::Map::new();
        for (name, part) in [
            ("kind", &reference.kind),
            ("id", &reference.id),
            ("digest", &reference.digest),
        ] {
            paid::put(&mut value, name, paid::literal(part, w)?, w)?;
        }
        paid::put(&mut out, "previous", Value::Object(value), w)?;
    }
    let bytes = paid::mul(dto.components.len(), mem::size_of::<Value>(), w)?;
    w.write_bytes(bytes)?;
    let mut components = Vec::new();
    components
        .try_reserve_exact(dto.components.len())
        .map_err(|_| fail("TEMPLATE_LIMIT", "component body allocation"))?;
    for v in &dto.components {
        w.visit(Visit::Element)?;
        let mut value = serde_json::Map::new();
        for (name, part) in [
            ("key", v.key.as_str()),
            ("book", book(v.book)),
            ("root_key", &v.root_key),
            ("role_slot", &v.role_slot),
        ] {
            paid::put(&mut value, name, paid::literal(part, w)?, w)?;
        }
        paid::put(&mut value, "aliases", paid::string_array(&v.aliases, w)?, w)?;
        w.write_bytes(mem::size_of::<Value>())?;
        components.push(Value::Object(value));
    }
    paid::put(&mut out, "components", Value::Array(components), w)?;
    let bytes = paid::mul(dto.families.len(), mem::size_of::<Value>(), w)?;
    w.write_bytes(bytes)?;
    let mut families = Vec::new();
    families
        .try_reserve_exact(dto.families.len())
        .map_err(|_| fail("TEMPLATE_LIMIT", "family body allocation"))?;
    for v in &dto.families {
        w.visit(Visit::Element)?;
        let mut value = serde_json::Map::new();
        for (name, part) in [
            ("key", v.key.as_str()),
            ("target_component_key", &v.target_component_key),
            ("role_slot", &v.role_slot),
        ] {
            paid::put(&mut value, name, paid::literal(part, w)?, w)?;
        }
        paid::put(&mut value, "aliases", paid::string_array(&v.aliases, w)?, w)?;
        w.write_bytes(mem::size_of::<Value>())?;
        families.push(Value::Object(value));
    }
    paid::put(&mut out, "families", Value::Array(families), w)?;
    Ok(Value::Object(out))
}
pub(super) fn aliases(values: &mut Vec<String>, w: &mut dyn ValidationWork) -> Result<()> {
    if vw::compare(w, values.len(), 64usize)?.is_gt() {
        return Err(fail("TEMPLATE_LIMIT", "aliases"));
    }
    for value in values.iter() {
        name_with_work(value, w)?;
    }
    paid::sort(values, w, |a, b, w| paid::text_cmp(a, b, w))?;
    for pair in values.windows(2) {
        if paid::text_cmp(&pair[0], &pair[1], w)?.is_eq() {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    Ok(())
}
pub(super) fn id(value: &SlotTemplate, w: &mut dyn ValidationWork) -> Result<String> {
    let digest = paid::document_digest_value(SCHEMA, &value.body, w)?;
    w.scan_bytes(64)?;
    w.write_bytes(69)?;
    Ok(format!("wdoc_{}", &digest[7..]))
}
pub(super) fn reference_matches(
    reference: &TemplateReference,
    prior: &SlotTemplate,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<bool> {
    if paid::different(&reference.kind, "workflow-slot-template", w, complete)? {
        return Ok(false);
    }
    let id = if complete {
        id(prior, w)?
    } else {
        prior.document_id_with_work(w)?
    };
    if paid::different(&reference.id, &id, w, complete)? {
        return Ok(false);
    }
    let digest = if complete {
        paid::document_digest_value(SCHEMA, &prior.body, w)?
    } else {
        prior.content_digest_with_work(w)?
    };
    Ok(!paid::different(&reference.digest, &digest, w, complete)?)
}
