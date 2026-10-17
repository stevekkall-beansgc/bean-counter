//! Closed selected Binding/Terms declarations, distinct from host authentication.
use super::super::{CoreMeter, codec, data::*, meter, shape};
use super::check::*;
use crate::Result;
use serde_json::{Value, json};
const INPUTS: &[&str] = &[
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
];
pub(super) fn string(m: &mut CoreMeter, v: &Value, maximum: usize) -> Result<()> {
    let text = v.as_str().ok_or_else(|| shape::fail("bounded string"))?;
    bounds(m, text.len(), 1, maximum)
}
pub(super) fn id(m: &mut CoreMeter, v: &Value, maximum: usize) -> Result<()> {
    string(m, v, maximum)?;
    for ch in v.as_str().expect("string checked").chars() {
        m.scan_bytes(ch.len_utf8())?;
        if m.check("inventory.identifier_control", || {
            ch <= '\u{1f}' || ch == '\u{7f}'
        })? {
            return Err(shape::fail("identifier control"));
        }
    }
    Ok(())
}
pub(super) fn refs(m: &mut CoreMeter, v: &Value, min: usize, max: usize) -> Result<()> {
    let a = v.as_array().ok_or_else(|| shape::fail("Ref set"))?;
    bounds(m, a.len(), min, max)?;
    ordered(m, a)?;
    for member in a {
        m.visit("inventory.ref_set_member")?;
        shape::reference(m, member)?;
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn ordered(m: &mut CoreMeter, a: &[Value]) -> Result<()> {
    if let Some(arena)=m.representation_arena(){
        // SAME canonical serialization/order, now an actually executing named
        // RP01 owning operation. Semantic set/type/association checks stay S.
        // At most the current and preceding member encodings coexist.
        let mut previous:Option<crate::canonical::audit_arena::AuditedBytes>=None;
        for member in a{
            m.visit("inventory.canonical_set_member")?;
            let bytes=m.canonical_owned_representation(&arena,member,crate::validation_work::RepresentationKind::Record)?;
            if let Some(prior)=&previous{if !shape::byte_order(m,prior.bytes(),bytes.bytes())?.is_lt(){return Err(shape::fail("noncanonical declared set"));}}
            previous=Some(bytes);
        }
        return Ok(());
    }
    let mut previous: Option<Vec<u8>> = None;
    for member in a {
        m.visit("inventory.canonical_set_member")?;
        let bytes = codec::encode(m, member, 4194304)?;
        if let Some(prior) = &previous {
            if !shape::byte_order(m, prior, &bytes)?.is_lt() {
                return Err(shape::fail("noncanonical declared set"));
            }
        }
        previous = Some(bytes);
    }
    Ok(())
}
pub(super) fn inputs(m: &mut CoreMeter, v: &Value, assented: bool) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::binding::inputs", |m| {
            inputs_data(m, v, assented)
        });
    }
    #[cfg(not(test))]
    inputs_data(m, v, assented)
}
fn inputs_data(m: &mut CoreMeter, v: &Value, assented: bool) -> Result<()> {
    let mut required = INPUTS.to_vec();
    if !assented {
        required.extend(["assent_refs", "authority_refs"]);
    }
    shape::closed(m, v, &required, &["ends_at"])?;
    for field in ["agreement_id", "customer"] {
        invoke!(id, m, data_field!(m, v, field), 128)?;
    }
    invoke!(number, m, v, "version", true)?;
    invoke!(id, m, data_field!(m, v, "source_namespace"), 256)?;
    for field in ["slot_catalog_ref", "slot_instantiation_ref"] {
        invoke!(shape::reference, m, data_field!(m, v, field))?;
    }
    for (field, min, max) in [
        ("roles_refs", 1, 16),
        ("assent_refs", 1, 16),
        ("authority_refs", 1, 32),
    ] {
        if assented && field != "roles_refs" {
            continue;
        }
        invoke!(refs, m, data_field!(m, v, field), min, max)?;
    }
    let sources = invoke!(shape::array, m, v, "source_slots")?;
    bounds(m, sources.len(), 1, 32)?;
    ordered(m, sources)?;
    for source in sources {
        shape::closed(m, source, &["name", "sources", "evidence_kinds"], &[])?;
        invoke!(qualified, m, invoke!(shape::text, m, source, "name")?)?;
        invoke!(
            shape::named,
            m,
            sources,
            "name",
            invoke!(shape::text, m, source, "name")?
        )?;
        let names = invoke!(shape::array, m, source, "sources")?;
        bounds(m, names.len(), 1, 16)?;
        ordered(m, names)?;
        for name in names {
            id(m, name, 256)?;
        }
        let kinds = invoke!(shape::array, m, source, "evidence_kinds")?;
        bounds(m, kinds.len(), 0, 5)?;
        ordered(m, kinds)?;
        for kind in kinds {
            one_of(
                m,
                kind.as_str().ok_or_else(|| shape::fail("evidence kind"))?,
                &[
                    "occurrence",
                    "outcome",
                    "supplier_final_cost",
                    "meter_quantity",
                    "allocation_input",
                ],
            )?;
        }
    }
    let bindings = invoke!(shape::array, m, v, "bindings")?;
    count(m, bindings, 1)?;
    for slot in bindings {
        shape::closed(m, slot, &["slot", "binding_ref"], &[])?;
        invoke!(qualified, m, invoke!(shape::text, m, slot, "slot")?)?;
        invoke!(shape::reference, m, data_field!(m, slot, "binding_ref"))?;
    }
    let funding = invoke!(shape::array, m, v, "funding_legs")?;
    bounds(m, funding.len(), 1, 16)?;
    ordered(m, funding)?;
    for leg in funding {
        shape::closed(
            m,
            leg,
            &["name", "mode", "resource_owner", "payer", "evidence_refs"],
            &["provider", "supplier_binding_slot"],
        )?;
        invoke!(qualified, m, invoke!(shape::text, m, leg, "name")?)?;
        eq(m, leg, "mode", &json!("customer_local"))?;
        invoke!(
            shape::named,
            m,
            funding,
            "name",
            invoke!(shape::text, m, leg, "name")?
        )?;
        for field in ["resource_owner", "payer"] {
            invoke!(id, m, data_field!(m, leg, field), 128)?;
        }
        if let Some(provider) = leg.get("provider") {
            id(m, provider, 128)?;
        }
        if let Some(slot) = leg.get("supplier_binding_slot") {
            qualified(
                m,
                slot.as_str().ok_or_else(|| shape::fail("supplier slot"))?,
            )?;
        }
        invoke!(refs, m, data_field!(m, leg, "evidence_refs"), 1, 16)?;
    }
    let start = time(m, v, "starts_at")?;
    if v.get("ends_at").is_some() {
        let end = time(m, v, "ends_at")?;
        if !meter::cmp(m, &start, &end)?.is_lt() {
            return Err(shape::fail("binding interval"));
        }
    }
    let quantities = invoke!(shape::array, m, v, "quantity_ceilings")?;
    bounds(m, quantities.len(), 0, 64)?;
    ordered(m, quantities)?;
    for q in quantities {
        shape::closed(m, q, &["activity", "field", "unit", "maximum"], &[])?;
        for name in ["activity", "field", "unit"] {
            invoke!(qualified, m, invoke!(shape::text, m, q, name)?)?;
        }
        decimal(m, q, "maximum")?;
    }
    let exposures = invoke!(shape::array, m, v, "exposure_ceilings")?;
    bounds(m, exposures.len(), 0, 16)?;
    ordered(m, exposures)?;
    for exposure in exposures {
        shape::closed(m, exposure, &["binding_slot", "maximum"], &[])?;
        invoke!(
            qualified,
            m,
            invoke!(shape::text, m, exposure, "binding_slot")?
        )?;
        invoke!(amount, m, data_field!(m, exposure, "maximum"))?;
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    b: &Value,
    terms: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::binding::validate", |m| {
            validate_data(m, b, terms)
        });
    }
    #[cfg(not(test))]
    validate_data(m, b, terms)
}
fn validate_data(m: &mut CoreMeter, b: &Value, terms: &Value) -> Result<()> {
    // Projection creates no trusted document. Every consumed input field still
    // enters the paid closed declaration validator and exact terms comparison.
    let mut projected = serde_json::Map::new();
    for field in INPUTS
        .iter()
        .copied()
        .chain(["assent_refs", "authority_refs", "ends_at"])
    {
        m.visit("inventory.binding_input_projection")?;
        if let Some(value) = b.get(field) {
            projected.insert(field.into(), value.clone());
        }
    }
    inputs(m, &Value::Object(projected), false)?;
    shape::closed(
        m,
        terms,
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
    )?;
    let signed = data_field!(m, terms, "binding_inputs");
    inputs(m, signed, true)?;
    for field in INPUTS {
        invoke!(copy, m, signed, field, b, field)?;
    }
    m.visit("inventory.optional_signed_end")?;
    match (signed.get("ends_at"), b.get("ends_at")) {
        (None, None) => {}
        (Some(left), Some(right)) => shape::equal(m, left, right)?,
        _ => return Err(shape::fail("signed binding end")),
    }
    Ok(())
}

/// The one selected initial composition has one fully consumed role, funding
/// leg, assent and grant. General multi-party/multi-grant layouts are not guessed.
pub(in crate::policy::workflow::transition_constructor) fn selected_declarations(
    m: &mut CoreMeter,
    b: &Value,
) -> Result<()> {
    for field in [
        "roles_refs",
        "funding_legs",
        "assent_refs",
        "authority_refs",
    ] {
        let rows = shape::array(m, b, field)?;
        if !meter::cmp(m, &rows.len().into(), &1usize.into())?.is_eq() {
            return Err(crate::Error::new(
                "UNSUPPORTED_COMPOSITION",
                format!("extra selected declaration {field}"),
            ));
        }
    }
    Ok(())
}
