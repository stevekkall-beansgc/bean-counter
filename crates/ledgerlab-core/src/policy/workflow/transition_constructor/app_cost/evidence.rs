//! Actual app assertion/verification consumer and the shared retained-assertion
//! validators. No caller verification flag or AP cost line enters eligibility.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    arithmetic, data::*, meter, record_body::RecordBody, records::Records, shape,
    working_value::owning_reference, CoreMeter,
};
use super::original::OriginalApp;
use crate::{
    canonical,
    canonical::audit_arena::ExternalBufferLease,
    domain::Timestamp,
    validation_work::{self as w, RepresentationKind, ValidationWork},
    Error, Result,
};
use num_bigint::BigInt;
use serde_json::Value;
macro_rules! call {
    ($f:path,$m:expr,$a:expr) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn literal(m: &mut CoreMeter, v: &Value, key: &str, expected: &str) -> Result<()> {
    let value = shape::text(m, v, key)?;
    if !shape::same_text(m, value, expected)? {
        return Err(shape::fail("app assertion literal"));
    }
    Ok(())
}
fn time(m: &mut CoreMeter, v: &Value, key: &str) -> Result<i64> {
    Ok(Timestamp::parse_with_work(shape::text(m, v, key)?, m)?.micros())
}
fn id(m: &mut CoreMeter, raw: &str, maximum: usize) -> Result<()> {
    if w::compare(m, raw.len(), 1usize)?.is_lt() || w::compare(m, raw.len(), maximum)?.is_gt() {
        return Err(shape::fail("app assertion identity bound"));
    }
    for ch in raw.chars() {
        m.scan_bytes(ch.len_utf8())?;
        if m.check("app.assertion_identifier_control", || {
            ch <= '\u{1f}' || ch == '\u{7f}'
        })? {
            return Err(shape::fail("app assertion identifier control"));
        }
    }
    Ok(())
}
pub(super) fn subject(
    m: &mut CoreMeter,
    c: &DraftContextData,
    original: &OriginalApp,
    subject: &Value,
) -> Result<()> {
    shape::closed(
        m,
        subject,
        &[
            "logical_store_id",
            "scope",
            "customer",
            "agreement_id",
            "original_work_ref",
            "original_binding_ref",
            "component_slot",
            "component_key",
            "app_call",
        ],
        &[],
    )?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app subject"))?;
    let work = owning_reference(m, &arena, &original.work_ref)?;
    let binding = owning_reference(m, &arena, &original.binding_ref)?;
    let key = super::super::cost_postings::operation_key(m, &arena, c)?;
    for (actual, expected) in [
        (
            &subject["logical_store_id"],
            &key.value()["logical_store_id"],
        ),
        (&subject["scope"], &key.value()["scope"]),
        (&subject["customer"], &original.terms.binding["customer"]),
        (
            &subject["agreement_id"],
            &original.terms.binding["agreement_id"],
        ),
        (&subject["original_work_ref"], work.value()),
        (&subject["original_binding_ref"], binding.value()),
        (
            &subject["component_key"],
            &original.declaration["component_key"],
        ),
    ] {
        eq(m, actual, expected)?;
    }
    let slot = shape::text(m, subject, "component_slot")?;
    if !shape::same_text(m, slot, &original.component_slot)? {
        return Err(shape::fail("app original component slot"));
    }
    let call = shape::field(m, subject, "app_call")?;
    shape::closed(m, call, &["source_namespace", "call_id"], &[])?;
    eq(
        m,
        &call["source_namespace"],
        &original.work["source_namespace"],
    )?;
    call!(id, m, shape::text(m, call, "call_id")?, 128)
}
pub(super) fn material(m: &mut CoreMeter, material: &Value) -> Result<()> {
    shape::closed(
        m,
        material,
        &["media_type", "content_base64", "content_sha256"],
        &[],
    )?;
    let media = shape::text(m, material, "media_type")?;
    let text = shape::same_text(m, media, "text/plain;charset=utf-8")?;
    if !text && !shape::same_text(m, media, "application/octet-stream")? {
        return Err(shape::fail("app material media type"));
    }
    let encoded = shape::text(m, material, "content_base64")?;
    if w::compare(m, encoded.len(), 1usize)?.is_lt()
        || w::compare(m, encoded.len(), 5464usize)?.is_gt()
    {
        return Err(shape::fail("app material wire bound"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app source material"))?;
    let decoded =
        m.decode_owned_base64_representation(&arena, encoded, RepresentationKind::EvidenceContent)?;
    if !w::compare(m, decoded.bytes().len(), 0usize)?.is_gt() {
        return Err(shape::fail("empty app source material"));
    }
    if text {
        m.scan_bytes(decoded.bytes().len())?;
        if !m.check("app.source_material_utf8", || {
            std::str::from_utf8(decoded.bytes()).is_ok()
        })? {
            return Err(shape::fail("app material UTF8"));
        }
    }
    let digest =
        m.hash_owned_representation(&arena, decoded.bytes(), RepresentationKind::EvidenceContent)?;
    let _scratch = arena.reserve_external_payload(64, m)?;
    let digest = canonical::hex_with_work(&digest, m)?;
    let expected = shape::text(m, material, "content_sha256")?;
    if !shape::same_text(m, &digest, expected)? {
        return Err(shape::fail("app source material raw digest"));
    }
    Ok(())
}
pub(super) fn observation(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    att: &Value,
) -> Result<arithmetic::Ratio> {
    shape::closed(
        m,
        att,
        &[
            "schema",
            "attestation_id",
            "source",
            "evidence_kind",
            "subject",
            "usage_field",
            "quantity",
            "unit",
            "observed_at",
            "material",
        ],
        &["provider_key"],
    )?;
    literal(m, att, "schema", "ledger-workflow-app-observation/1")?;
    literal(m, att, "evidence_kind", "app_usage")?;
    call!(id, m, shape::text(m, att, "attestation_id")?, 128)?;
    call!(id, m, shape::text(m, att, "source")?, 256)?;
    subject(m, c, original, &att["subject"])?;
    eq(m, &att["usage_field"], &original.declaration["usage_field"])?;
    eq(m, &att["unit"], &original.declaration["usage_unit"])?;
    let raw = shape::text(m, att, "quantity")?;
    let observed = arithmetic::decimal(m, raw)?;
    let original_quantity = original.observed_quantity(m, program)?;
    let left = meter::mul(m, &observed.numerator, &original_quantity.denominator)?;
    let right = meter::mul(m, &original_quantity.numerator, &observed.denominator)?;
    if !meter::cmp(m, &left, &right)?.is_eq() {
        return Err(shape::fail(
            "app observation differs from immutable original usage",
        ));
    }
    let actual = time(m, att, "observed_at")?;
    let original_at = original.occurrence(m)?;
    if !w::compare(m, actual, original_at)?.is_eq() {
        return Err(shape::fail("app observation original occurrence"));
    }
    if let Some(provider) = att.get("provider_key") {
        shape::closed(m, provider, &["provider", "account", "request_id"], &[])?;
        for key in ["provider", "account", "request_id"] {
            call!(id, m, shape::text(m, provider, key)?, 128)?;
        }
    }
    material(m, &att["material"])?;
    Ok(observed)
}
pub(super) fn final_amount(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    observation: &Value,
    att: &Value,
) -> Result<BigInt> {
    shape::closed(
        m,
        att,
        &[
            "schema",
            "attestation_id",
            "source",
            "evidence_kind",
            "subject",
            "observation_ref",
            "cost_revision",
            "finality",
            "amount",
            "quantity_comparison",
            "issued_at",
            "material",
        ],
        &["prior_final_evidence_ref"],
    )?;
    literal(m, att, "schema", "ledger-workflow-app-final-cost/1")?;
    literal(m, att, "evidence_kind", "app_final_cost")?;
    literal(m, att, "finality", "final_attributable_amount")?;
    call!(id, m, shape::text(m, att, "attestation_id")?, 128)?;
    call!(id, m, shape::text(m, att, "source")?, 256)?;
    subject(m, c, original, &att["subject"])?;
    eq(m, &att["subject"], &observation["subject"])?;
    let revision = crate::policy::workflow::facts::counter_with_work(
        shape::text(m, att, "cost_revision")?,
        m,
    )?;
    if !w::compare(m, revision, 0u64)?.is_gt() {
        return Err(shape::fail("positive app final cost revision"));
    }
    if w::compare(m, revision, 1u64)?.is_eq() {
        if att.get("prior_final_evidence_ref").is_some() {
            return Err(shape::fail("initial app evidence has no predecessor"));
        }
    } else {
        call!(
            shape::reference,
            m,
            shape::field(m, att, "prior_final_evidence_ref")?
        )?;
    }
    shape::closed(m, &att["amount"], &["currency", "scale", "atoms"], &[])?;
    literal(m, &att["amount"], "currency", "USD")?;
    if !shape::same(m, &att["amount"]["scale"], &Value::from(18))? {
        return Err(shape::fail("app final amount scale"));
    }
    let eligible = call!(
        arithmetic::integer,
        m,
        shape::text(m, &att["amount"], "atoms")?,
        false
    )?;
    let maximum = call!(
        arithmetic::integer,
        m,
        shape::text(m, &original.declaration["maximum_final_amount"], "atoms")?,
        false
    )?;
    let scalar_max = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &eligible, &0.into())?.is_lt()
        || meter::cmp(m, &eligible, &maximum)?.is_gt()
        || meter::cmp(m, &eligible, &scalar_max)?.is_gt()
    {
        return Err(Error::new(
            "EXPOSURE_CEILING_EXCEEDED",
            "actual app final amount above original maximum",
        ));
    }
    let comparison = shape::field(m, att, "quantity_comparison")?;
    let kind = shape::text(m, comparison, "kind")?;
    if shape::same_text(m, kind, "comparable")? {
        shape::closed(m, comparison, &["kind", "quantity", "unit"], &[])?;
        eq(m, &comparison["unit"], &observation["unit"])?;
        let quantity = call!(
            arithmetic::decimal,
            m,
            shape::text(m, comparison, "quantity")?
        )?;
        let observed = original.observed_quantity(m, program)?;
        let left = meter::mul(m, &quantity.numerator, &observed.denominator)?;
        let right = meter::mul(m, &observed.numerator, &quantity.denominator)?;
        if meter::cmp(m, &quantity.numerator, &0.into())?.is_lt()
            || meter::cmp(m, &left, &right)?.is_gt()
        {
            return Err(shape::fail(
                "comparable billed quantity exceeds original observed usage",
            ));
        }
    } else if shape::same_text(m, kind, "unavailable")? {
        shape::closed(m, comparison, &["kind", "reason"], &[])?;
        let reason = shape::text(m, comparison, "reason")?;
        if !shape::same_text(m, reason, "not_reported")?
            && !shape::same_text(m, reason, "not_comparable")?
        {
            return Err(shape::fail("app unavailable comparison reason"));
        }
    } else {
        return Err(shape::fail("app quantity comparison variant"));
    }
    material(m, &att["material"])?;
    Ok(eligible)
}

pub(super) struct EvidencePair {
    pub attestation_ref: ExactRef,
    pub attestation: RecordBody,
    pub _verification_ref: ExactRef,
    pub _verification: RecordBody,
    _ownership: ExternalBufferLease,
}
/// The original retention origin must be completed by the host before this
/// DATA pair is used. Current authority is checked separately by the caller;
/// verifier fields do not create an origin capability by matching themselves.
pub(super) fn pair(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    original: &OriginalApp,
    proof: &Value,
    schema: &str,
    observation_ref: Option<&ExactRef>,
) -> Result<EvidencePair> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app proof"))?;
    let row = w::usize_add(m, 64, 128)?;
    let row = w::usize_add(m, row, 71)?;
    ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&10usize.into(), &row.into()],
    )?;
    let refs = 10usize
        .checked_mul(row)
        .ok_or_else(|| shape::fail("app proof reference metadata"))?;
    let bytes = w::usize_add(m, refs, std::mem::size_of::<EvidencePair>())?;
    let bytes = w::usize_add(m, bytes, 512)?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    shape::closed(m, proof, &["attestation_ref", "verification_ref"], &[])?;
    let attestation_ref = shape::reference(m, &proof["attestation_ref"])?;
    let verification_ref = shape::reference(m, &proof["verification_ref"])?;
    let att = r.get(
        m,
        &c.key,
        &attestation_ref,
        "workflow-fact-attestation",
        schema,
    )?;
    let verification = r.get(
        m,
        &c.key,
        &verification_ref,
        "workflow-evidence-verification",
        "ledger-workflow-evidence-verification/1",
    )?;
    shape::closed(
        m,
        &verification,
        &[
            "schema",
            "attestation_ref",
            "attestation_raw_sha256",
            "attestation_digest",
            "verified_source",
            "verification_primitive",
            "source_grant_ref",
            "source_grant_revision",
            "authenticated_principal_ref",
            "trusted_received_at",
            "subject_binding_refs",
            "verification_evidence_refs",
        ],
        &[],
    )?;
    literal(
        m,
        &verification,
        "verification_primitive",
        "authenticated-nominated-source-retention/1",
    )?;
    eq(
        m,
        &verification["attestation_ref"],
        &proof["attestation_ref"],
    )?;
    if !call!(
        shape::same_text,
        m,
        shape::text(m, &verification, "attestation_digest")?,
        &attestation_ref.digest
    )? {
        return Err(shape::fail("app evidence digest association"));
    }
    eq(m, &verification["verified_source"], &att["source"])?;
    let raw = r.canonical(m, &attestation_ref)?;
    let digest = m.hash_owned_representation(&arena, raw, RepresentationKind::Record)?;
    let digest = canonical::hex_with_work(&digest, m)?;
    if !call!(
        shape::same_text,
        m,
        shape::text(m, &verification, "attestation_raw_sha256")?,
        &digest
    )? {
        return Err(shape::fail("app evidence retained raw digest"));
    }
    let grant_ref = shape::reference(m, &verification["source_grant_ref"])?;
    let grant = r.get(
        m,
        &c.key,
        &grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    eq(m, &grant["source"], &att["source"])?;
    eq(
        m,
        &grant["revision"],
        &verification["source_grant_revision"],
    )?;
    eq(
        m,
        &grant["original_ceiling_ref"],
        &verification["authenticated_principal_ref"],
    )?;
    let subjects = shape::array(m, &verification, "subject_binding_refs")?;
    let expected_count = if observation_ref.is_some() { 3 } else { 2 };
    if !w::compare(m, subjects.len(), expected_count)?.is_eq() {
        return Err(shape::fail("app exact subject binding count"));
    }
    for expected in [
        Some(&original.work_ref),
        Some(&original.binding_ref),
        observation_ref,
    ]
    .into_iter()
    .flatten()
    {
        let value = owning_reference(m, &arena, expected)?;
        let mut hits = 0usize;
        for actual in subjects {
            m.visit("app.proof_subject_binding")?;
            if shape::same(m, actual, value.value())? {
                hits = w::usize_add(m, hits, 1)?;
            }
        }
        if !w::compare(m, hits, 1usize)?.is_eq() {
            return Err(shape::fail("app exact subject binding set"));
        }
    }
    let received = time(m, &verification, "trusted_received_at")?;
    let current = Timestamp::parse_with_work(&c.received_at, m)?.micros();
    let starts = time(m, &grant, "starts_at")?;
    let ends = time(m, &grant, "ends_at")?;
    if w::compare(m, received, current)?.is_gt()
        || w::compare(m, received, starts)?.is_lt()
        || !w::compare(m, received, ends)?.is_lt()
    {
        return Err(shape::fail("app retained source grant time"));
    }
    let at = if att.get("issued_at").is_some() {
        time(m, &att, "issued_at")?
    } else {
        time(m, &att, "observed_at")?
    };
    if w::compare(m, at, received)?.is_gt() {
        return Err(shape::fail("app assertion after actual trusted retention"));
    }
    let extras = shape::array(m, &verification, "verification_evidence_refs")?;
    if w::compare(m, extras.len(), 16usize)?.is_gt() {
        return Err(shape::fail("app verifier supporting evidence bound"));
    }
    for extra in extras {
        let reference = shape::reference(m, extra)?;
        r.provenance(m, &c.key, &reference)?;
    }
    Ok(EvidencePair {
        attestation_ref,
        attestation: att,
        _verification_ref: verification_ref,
        _verification: verification,
        _ownership: ownership,
    })
}

/// Same scalar normalization for fresh retention and retry, before identity is
/// computed. The parsed input and every replacement remain owning artifacts.
pub(super) fn normalize(
    m: &mut CoreMeter,
    att: &Value,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app assertion normalization"))?;
    let mut normalized = arena.clone_value(att, m)?;
    fn scalar(
        m: &mut CoreMeter,
        arena: &crate::canonical::audit_arena::AuditArena,
        value: &Value,
        scale: u8,
    ) -> Result<crate::canonical::audit_arena::AuditedValue> {
        let _scratch = arena.reserve_external_payload(1024, m)?;
        let raw = value
            .as_str()
            .ok_or_else(|| shape::fail("app scalar text"))?;
        let value = crate::policy::workflow::SignedScalar::parse_with_work(raw, scale, m)?;
        let parsed = arithmetic::decimal(m, value.as_str())?;
        if meter::cmp(m, &parsed.numerator, &0.into())?.is_lt() {
            return Err(shape::fail("negative app assertion scalar"));
        }
        arena.string_value(value.as_str(), m)
    }
    let schema = shape::text(m, att, "schema")?;
    if shape::same_text(m, schema, "ledger-workflow-app-observation/1")? {
        let quantity = scalar(m, &arena, &att["quantity"], 18)?;
        normalized.replace_existing_owned("quantity", quantity, m)?;
    } else if shape::same_text(m, schema, "ledger-workflow-app-final-cost/1")? {
        let counter = crate::policy::workflow::facts::counter_with_work(
            shape::text(m, att, "cost_revision")?,
            m,
        )?;
        if !w::compare(m, counter, 0u64)?.is_gt() {
            return Err(shape::fail("positive app cost revision"));
        }
        let counter = super::super::close::counter_value(m, &arena, counter)?;
        normalized.replace_existing_owned("cost_revision", counter, m)?;
        let mut amount = arena.clone_value(&att["amount"], m)?;
        let atoms = scalar(m, &arena, &att["amount"]["atoms"], 0)?;
        amount.replace_existing_owned("atoms", atoms, m)?;
        normalized.replace_existing_owned("amount", amount, m)?;
        let comparison = &att["quantity_comparison"];
        if call!(
            shape::same_text,
            m,
            shape::text(m, comparison, "kind")?,
            "comparable"
        )? {
            let mut comparison = arena.clone_value(comparison, m)?;
            let quantity = scalar(m, &arena, &att["quantity_comparison"]["quantity"], 18)?;
            comparison.replace_existing_owned("quantity", quantity, m)?;
            normalized.replace_existing_owned("quantity_comparison", comparison, m)?;
        }
    } else {
        return Err(shape::fail("app assertion normalization schema"));
    }
    Ok(normalized)
}

pub(super) struct AppObservation {
    pub pair: EvidencePair,
    pub _quantity: arithmetic::Ratio,
}
pub(super) struct AppFinalAmount {
    pub pair: EvidencePair,
    pub eligible: BigInt,
}
pub(super) fn resolve_observation(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    proof: &Value,
) -> Result<AppObservation> {
    let pair = pair(
        m,
        r,
        c,
        original,
        proof,
        "ledger-workflow-app-observation/1",
        None,
    )?;
    let quantity = observation(m, c, program, original, &pair.attestation)?;
    Ok(AppObservation {
        pair,
        _quantity: quantity,
    })
}
pub(super) fn resolve_final(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    original: &OriginalApp,
    observation: &AppObservation,
    proof: &Value,
) -> Result<AppFinalAmount> {
    let pair = pair(
        m,
        r,
        c,
        original,
        proof,
        "ledger-workflow-app-final-cost/1",
        Some(&observation.pair.attestation_ref),
    )?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app final proof"))?;
    let actual = owning_reference(m, &arena, &observation.pair.attestation_ref)?;
    eq(m, &pair.attestation["observation_ref"], actual.value())?;
    let eligible = final_amount(
        m,
        c,
        program,
        original,
        &observation.pair.attestation,
        &pair.attestation,
    )?;
    Ok(AppFinalAmount { pair, eligible })
}

#[cfg(test)]
#[path = "timing_boundary_tests.rs"]
mod timing_boundary_tests;
