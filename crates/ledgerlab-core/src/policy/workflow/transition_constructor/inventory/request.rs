//! Closed saved normalized DATA framing; raw/certificate replay stays a host obligation.
use super::super::{shape, CoreMeter};
use super::{binding, check::*, publication};
use crate::Result;
use serde_json::{json, Value};
const ROLES: &[&str] = &[
    "accepted_binding",
    "activation_target",
    "assent",
    "authority",
    "binding",
    "claim",
    "customer",
    "dependent_decision",
    "evidence",
    "fact_schema",
    "family",
    "inline_document",
    "policy",
    "private_document",
    "product",
    "publication",
    "quantity_field",
    "slot_catalog",
    "slot_instantiation",
    "source_enrollment",
    "stage_instance",
    "target",
    "transition",
    "work",
];
const RELATIONS: &[&str] = &[
    "accepted_binding",
    "activation_target",
    "agreement_input",
    "context_schema",
    "customer_registration",
    "dependent_decision",
    "direct",
    "evidence_input",
    "original_binding",
    "original_claim",
    "original_target",
    "original_transition",
    "original_work",
    "publication_source",
    "source_enrollment",
];
// Closed saved framing rows, pinned by AGREEMENT-READY-AMENDMENT49–65
// and CAUSAL-REVERSAL-02-REPAIR7–17. Causal02 has no activation row.
// All three retain evidence/2, normalization/2, envelope/1, operation-record/1,
// authority/2 admit, agreement-manifest/1 and NO economic closure_profile field.
// Complete manifest/receipt authentication remains the host auditor obligation.
const RECIPES: &[(&str, &str, &str)] = &[
    (
        "agreement.accept",
        "ledger-workflow-agreement-accept/2",
        "sha256:e023e7bf17aeed815f30b6fdce35fc0bf68cc9efcc0612a019b0b109494ac169",
    ),
    (
        "agreement.accept",
        "ledger-workflow-agreement-accept/3",
        "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
    ),
    (
        "agreement.accept",
        "ledger-workflow-agreement-accept/4",
        "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4",
    ),
    (
        "agreement.activate",
        "ledger-workflow-activation/1",
        "sha256:e023e7bf17aeed815f30b6fdce35fc0bf68cc9efcc0612a019b0b109494ac169",
    ),
];
fn recipe(m: &mut CoreMeter, kind: &str, schema: &str) -> Result<&'static str> {
    for (expected_kind, expected_schema, registry) in RECIPES {
        m.visit("inventory.saved_request_recipe")?;
        if shape::same_text(m, kind, expected_kind)?
            && shape::same_text(m, schema, expected_schema)?
        {
            return Ok(registry);
        }
    }
    Err(crate::Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "unsupported initial saved request tuple",
    ))
}
pub(super) fn validate(
    m: &mut CoreMeter,
    e: &Value,
    kind: &str,
    payload_schema: &str,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::request::validate", |m| {
            validate_data(m, e, kind, payload_schema, None)
        });
    }
    #[cfg(not(test))]
    validate_data(m, e, kind, payload_schema, None)
}
pub(super) fn validate_family_close_acceptance(
    m: &mut CoreMeter,
    e: &Value,
    kind: &str,
    payload_schema: &str,
) -> Result<()> {
    if !shape::same_text(m, kind, "agreement.accept")?
        || !shape::same_text(m, payload_schema, "ledger-workflow-agreement-accept/4")?
    {
        return Err(shape::fail("family-close acceptance saved tuple"));
    }
    validate_data(
        m,
        e,
        kind,
        payload_schema,
        Some("sha256:3aae5efefe5a28f8e1bd5382609652e67d9cf710250c8d29874715399f9449dc"),
    )
}
fn validate_data(
    m: &mut CoreMeter,
    e: &Value,
    kind: &str,
    payload_schema: &str,
    fixed_registry: Option<&'static str>,
) -> Result<()> {
    for field in ["original_request_bytes_sha256"] {
        let raw = invoke!(shape::text, m, e, field)?;
        bounds(m, raw.len(), 64, 64)?;
        for byte in raw.bytes() {
            m.scan_bytes(1)?;
            if !m.check("inventory.raw_sha_hex", || {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            })? {
                return Err(shape::fail("raw SHA shape"));
            }
        }
    }
    invoke!(
        binding::string,
        m,
        data_field!(m, e, "original_request_bytes_base64"),
        349528
    )?;
    for field in ["request_hash", "schema_bundle_digest"] {
        invoke!(publication::digest, m, data_field!(m, e, field))?;
    }
    let descriptors = data_field!(m, e, "descriptor_resolution");
    shape::closed(
        m,
        descriptors,
        &[
            "schema",
            "registry_digest",
            "operation_kind",
            "payload_schema",
            "locator_digest",
            "anchors",
            "descriptors",
            "normalization_profile",
            "closure_digest",
        ],
        &[],
    )?;
    eq(
        m,
        descriptors,
        "schema",
        &json!("ledger-operation-descriptors/1"),
    )?;
    eq(m, descriptors, "operation_kind", &json!(kind))?;
    eq(m, descriptors, "payload_schema", &json!(payload_schema))?;
    eq(
        m,
        descriptors,
        "normalization_profile",
        &json!("ledger-operation-normalization/2"),
    )?;
    // Registry and schema bundle are distinct domains; cryptographic replay
    // belongs to the host original-certificate auditor, never false equality.
    let registry = match fixed_registry {
        Some(registry) => registry,
        None => recipe(m, kind, payload_schema)?,
    };
    eq(m, descriptors, "registry_digest", &json!(registry))?;
    for field in ["registry_digest", "locator_digest", "closure_digest"] {
        invoke!(publication::digest, m, data_field!(m, descriptors, field))?;
    }
    let declarations = data_field!(m, descriptors, "descriptors");
    binding::refs(m, declarations, 0, 128)?;
    for r in declarations.as_array().expect("Ref array checked") {
        let parsed = shape::reference(m, r)?;
        if !shape::same_text(m, &parsed.id[..5], "wdoc_")? {
            return Err(shape::fail("descriptor document identity"));
        }
    }
    let anchors = invoke!(shape::array, m, descriptors, "anchors")?;
    bounds(m, anchors.len(), 0, 128)?;
    binding::ordered(m, anchors)?;
    for anchor in anchors {
        shape::closed(
            m,
            anchor,
            &["role", "record_ref", "relation", "descriptor_refs"],
            &[],
        )?;
        invoke!(one_of, m, invoke!(shape::text, m, anchor, "role")?, ROLES)?;
        invoke!(
            one_of,
            m,
            invoke!(shape::text, m, anchor, "relation")?,
            RELATIONS
        )?;
        invoke!(shape::reference, m, data_field!(m, anchor, "record_ref"))?;
        let refs = data_field!(m, anchor, "descriptor_refs");
        binding::refs(m, refs, 0, 128)?;
        for r in refs.as_array().expect("Ref array checked") {
            let parsed = shape::reference(m, r)?;
            if !shape::same_text(m, &parsed.id[..5], "wdoc_")? {
                return Err(shape::fail("anchor descriptor identity"));
            }
            shape::contains(m, declarations.as_array().expect("Ref array checked"), r)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
