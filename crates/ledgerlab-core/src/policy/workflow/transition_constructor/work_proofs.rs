//! Required Work fact evidence, using the existing attestation/verification and
//! immutable VerifiedFact contracts. Retention authority is historical; current
//! work/correction authority remains the consuming interpreter's obligation.
use super::{
    authority,
    data::*,
    emitted_record::{ConstructedEnvelope, EncodedBody},
    records::Records,
    representation::InvocationToken,
    shape,
    working_value::{owning_reference, NormalizedInput},
    CoreMeter,
};
use crate::{
    canonical::{
        self,
        audit_arena::{AuditedValue, ExternalBufferLease},
    },
    domain::Timestamp,
    policy::workflow::{selected_program::SelectedProgram, FactSchema, FactValue},
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
    Result,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::mem;

pub(super) struct WorkProofRequest<'a> {
    pub payload: &'a Value,
    pub facts: &'a Value,
    pub namespace: &'a str,
    pub received_at: &'a str,
    pub only_field: Option<&'a str>,
    pub worker_authority: Option<&'a Value>,
}
pub(super) struct WorkProofs {
    pub references: Vec<ExactRef>,
    pub introduced: Vec<ConstructedEnvelope>,
    owned_bytes: usize,
    owner: ExternalBufferLease,
}
fn mul(m: &mut CoreMeter, n: usize, size: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&n.into(), &size.into()])?;
    n.checked_mul(size)
        .ok_or_else(|| shape::fail("work proof metadata product"))
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn copy_reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
impl WorkProofs {
    fn new(m: &mut CoreMeter, count: usize) -> Result<Self> {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning Work proofs"))?;
        // Returned Ref plus the emitted envelope's independently copied Ref.
        let row = mem::size_of::<ExactRef>()
            + 2 * (64 + 128 + 71)
            + mem::size_of::<ConstructedEnvelope>();
        let bytes = mul(m, count, row)?;
        let bytes = w::usize_add(m, bytes, mem::size_of::<Self>())?;
        let owner = arena.reserve_external_payload(bytes, m)?;
        m.write_bytes(bytes)?;
        let mut references = Vec::new();
        let mut introduced = Vec::new();
        references
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("work proof reference allocation"))?;
        introduced
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("work proof output allocation"))?;
        Ok(Self {
            references,
            introduced,
            owned_bytes: bytes,
            owner,
        })
    }
    pub(super) fn attach(
        &mut self,
        m: &mut CoreMeter,
        out: &mut Vec<ConstructedEnvelope>,
    ) -> Result<()> {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning Work proof append"))?;
        let count = w::usize_add(m, out.len(), self.introduced.len())?;
        let controls = mul(m, count, mem::size_of::<ConstructedEnvelope>())?;
        let bytes = w::usize_add(m, self.owned_bytes, controls)?;
        let owner = arena.reserve_external_payload(bytes, m)?;
        m.write_bytes(controls)?;
        out.try_reserve_exact(self.introduced.len())
            .map_err(|_| shape::fail("Work proof append allocation"))?;
        self.owner = owner;
        self.owned_bytes = bytes;
        out.append(&mut self.introduced);
        Ok(())
    }
    pub(super) fn into_owner(self) -> ExternalBufferLease {
        self.owner
    }
}
fn schema<'p>(
    m: &mut CoreMeter,
    p: &'p SelectedProgram,
    request: &Value,
) -> Result<&'p FactSchema> {
    for (r, s) in p.fact_schemas() {
        m.visit("work.proof_schema")?;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning Work schema"))?;
        let expected = super::working_value::owning_definition(m, &arena, r)?;
        if shape::same(m, &request["fact_schema_ref"], expected.value())? {
            return Ok(s);
        }
    }
    Err(shape::fail("Work proof original schema"))
}
fn subject(
    m: &mut CoreMeter,
    c: &DraftContextData,
    q: &WorkProofRequest<'_>,
) -> Result<AuditedValue> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work proof subject"))?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let kind = a.string_value("work", m)?;
    let namespace = a.string_value(q.namespace, m)?;
    a.object_from_fields(
        &[
            ("kind", kind.value()),
            ("logical_store_id", &key.value()["logical_store_id"]),
            ("scope", &key.value()["scope"]),
            ("canonical_source_namespace", namespace.value()),
            ("business_operation_id", &q.payload["business_operation_id"]),
            ("customer", &q.payload["customer"]),
            ("activity", &q.payload["activity"]),
            ("product_ref", &q.payload["product_ref"]),
            ("binding_ref", &q.payload["binding_ref"]),
        ],
        m,
    )
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, super::record_body::RecordBody)> {
    let reference = shape::reference(m, v)?;
    let b = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, b))
}
fn scalar(m: &mut CoreMeter, s: &FactSchema, name: &str, value: &Value) -> Result<AuditedValue> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work claim scalar"))?;
    let bytes = w::usize_add(
        m,
        value.as_str().map(str::len).unwrap_or(0),
        1024 + mem::size_of::<FactValue>(),
    )?;
    let _scratch = a.reserve_external_payload(bytes, m)?;
    let normalized = s.normalize_attested_field_complete_with_work(name, value, m)?;
    match &normalized {
        FactValue::Boolean(v) => a.clone_value(&Value::Bool(*v), m),
        FactValue::String(v) | FactValue::Enum(v) => a.string_value(v, m),
        FactValue::Integer(v) | FactValue::Decimal(v) => a.string_value(v.as_str(), m),
        FactValue::Timestamp(v) => a.string_value(v.as_str(), m),
        FactValue::Missing => Err(shape::fail("missing Work attested value")),
    }
}
fn required_field(m: &mut CoreMeter, names: &[String], field: &str) -> Result<bool> {
    for name in names {
        if shape::same_text(m, name, field)? {
            return Ok(true);
        }
    }
    Ok(false)
}
struct ProofUse<'a> {
    program: &'a SelectedProgram,
    request: &'a WorkProofRequest<'a>,
    schema: &'a FactSchema,
    field: &'a str,
}
fn checked_value(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    u: &ProofUse<'_>,
    att_ref: &ExactRef,
    verification_ref: &ExactRef,
) -> Result<AuditedValue> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work proof value"))?;
    let att = r.get(
        m,
        &c.key,
        att_ref,
        "workflow-fact-attestation",
        "ledger-workflow-fact-attestation/1",
    )?;
    shape::closed(
        m,
        &att,
        &[
            "schema",
            "attestation_id",
            "source",
            "evidence_kind",
            "subject",
            "subject_schema_ref",
            "claims",
            "issued_at",
            "supporting_evidence_refs",
        ],
        &[],
    )?;
    let expected = subject(m, c, u.request)?;
    shape::equal(m, &att["subject"], expected.value())?;
    shape::equal(
        m,
        &att["subject_schema_ref"],
        &u.request.payload["fact_schema_ref"],
    )?;
    let fields = shape::array(m, u.schema.canonical_value(), "fields")?;
    let definition = shape::named(m, fields, "name", u.field)?;
    let provenance = &definition["provenance"];
    let provenance_kind = shape::text(m, provenance, "kind")?;
    if !shape::same_text(m, provenance_kind, "attested")? {
        return Err(shape::fail("Work proof caller field"));
    }
    shape::equal(m, &att["evidence_kind"], &provenance["evidence_kind"])?;
    let (_, binding) = load(
        m,
        r,
        c,
        &u.request.payload["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let slot = shape::text(m, provenance, "source_slot")?;
    let nominations = shape::array(m, &binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    let kinds = shape::array(m, nomination, "evidence_kinds")?;
    if !shape::member(m, sources, &att["source"])?
        || !shape::member(m, kinds, &att["evidence_kind"])?
    {
        return Err(shape::fail("Work proof original nomination"));
    }
    let claims = shape::array(m, &att, "claims")?;
    if !w::compare(m, claims.len(), 0usize)?.is_gt()
        || w::compare(m, claims.len(), 64usize)?.is_gt()
    {
        return Err(shape::fail("Work attestation claim count"));
    }
    let mut found = false;
    for (i, claim) in claims.iter().enumerate() {
        m.visit("work.attested_claim")?;
        shape::closed(m, claim, &["field", "value"], &[])?;
        let name = shape::text(m, claim, "field")?;
        for previous in &claims[..i] {
            let previous_name = shape::text(m, previous, "field")?;
            if shape::same_text(m, name, previous_name)? {
                return Err(shape::fail("duplicate Work attested field"));
            }
        }
        let claim_definition = shape::named(m, fields, "name", name)?;
        shape::equal(m, &claim_definition["provenance"], provenance)?;
        let normalized = scalar(m, u.schema, name, &claim["value"])?;
        shape::equal(m, &claim["value"], normalized.value())?;
        if shape::same_text(m, name, u.field)? {
            shape::equal(m, normalized.value(), &u.request.facts[u.field])?;
            found = true;
        }
    }
    if !found {
        return Err(shape::fail("Work attestation required field missing"));
    }
    let supporting = shape::array(m, &att, "supporting_evidence_refs")?;
    if !w::compare(m, supporting.len(), 0usize)?.is_eq() {
        return Err(shape::fail("unsupported Work supporting fact evidence"));
    }
    let verification = r.get(
        m,
        &c.key,
        verification_ref,
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
    let att_value = owning_reference(m, &a, att_ref)?;
    shape::equal(m, &verification["attestation_ref"], att_value.value())?;
    let digest = a.string_value(&att_ref.digest, m)?;
    shape::equal(m, &verification["attestation_digest"], digest.value())?;
    shape::equal(m, &verification["verified_source"], &att["source"])?;
    let primitive = shape::text(m, &verification, "verification_primitive")?;
    if !shape::same_text(m, primitive, "authenticated-nominated-source-retention/1")? {
        return Err(shape::fail("Work proof verification primitive"));
    }
    let bindings = a.array_from_values(&[&u.request.payload["binding_ref"]], m)?;
    shape::equal(m, &verification["subject_binding_refs"], bindings.value())?;
    let supplemental = shape::array(m, &verification, "verification_evidence_refs")?;
    if !w::compare(m, supplemental.len(), 0usize)?.is_eq() {
        return Err(shape::fail("Work proof supplemental verification evidence"));
    }
    let raw = r.canonical(m, att_ref)?;
    m.hash_bytes(raw.len())?;
    let raw_hash = canonical::hex_with_work(&Sha256::digest(raw), m)?;
    let declared_hash = shape::text(m, &verification, "attestation_raw_sha256")?;
    if !shape::same_text(m, declared_hash, &raw_hash)? {
        return Err(shape::fail("Work proof raw attestation hash"));
    }
    let (_, grant) = load(
        m,
        r,
        c,
        &verification["source_grant_ref"],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let (enrollment_ref, enrollment) = load(
        m,
        r,
        c,
        &verification["authenticated_principal_ref"],
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    let enrollment_value = owning_reference(m, &a, &enrollment_ref)?;
    shape::equal(m, &grant["original_ceiling_ref"], enrollment_value.value())?;
    shape::equal(m, &grant["grant_id"], &enrollment["enrollment_id"])?;
    // Bind historical retention to the originally accepted worker identity.
    // Today's corrector grant is deliberately not a retention-time grant.
    if let Some(original) = u.request.worker_authority {
        let (_, worker) = load(
            m,
            r,
            c,
            original,
            "workflow-economic-authority",
            "ledger-workflow-economic-authority/8",
        )?;
        let operation = a.string_value("work.accept", m)?;
        let slot_value = a.string_value(slot, m)?;
        shape::equal(m, &worker["operation"], operation.value())?;
        shape::equal(
            m,
            &worker["original_binding_ref"],
            &u.request.payload["binding_ref"],
        )?;
        shape::equal(m, &worker["source_slot"], slot_value.value())?;
        shape::equal(m, &worker["enrollment_ref"], enrollment_value.value())?;
        shape::equal(m, &worker["source"], &att["source"])?;
    } else {
        let worker = c
            .authorities
            .first()
            .ok_or_else(|| shape::fail("Work proof worker authority"))?;
        if !shape::same_text(m, &worker.source_slot, slot)?
            || !shape::same_ref(m, &worker.enrollment_ref, &enrollment_ref)?
        {
            return Err(shape::fail("Work proof accepted worker identity"));
        }
    }

    shape::equal(
        m,
        &grant["revision"],
        &verification["source_grant_revision"],
    )?;
    let ceiling = &enrollment["ceiling"];
    for field in ["source", "canonical_economic_namespace", "principal_id"] {
        shape::equal(m, &grant[field], &ceiling[field])?;
    }
    shape::equal(m, &grant["source"], &att["source"])?;
    let namespace = a.string_value(u.request.namespace, m)?;
    shape::equal(m, &grant["canonical_economic_namespace"], namespace.value())?;
    let trusted =
        Timestamp::parse_with_work(shape::text(m, &verification, "trusted_received_at")?, m)?
            .micros();
    let issued = Timestamp::parse_with_work(shape::text(m, &att, "issued_at")?, m)?.micros();
    let received = Timestamp::parse_with_work(u.request.received_at, m)?.micros();
    if w::compare(m, issued, trusted)?.is_gt() || w::compare(m, trusted, received)?.is_gt() {
        return Err(shape::fail("Work proof historical trusted time"));
    }
    // Revalidate the actual retention-time grant, never today's corrector
    // grant. Completed committed-origin association remains host-owned.
    authority::grant_shape(m, &grant)?;
    let token = InvocationToken::new();
    authority::grant_intersection(m, &token, &grant, ceiling, &trusted.into())?;
    let mut product = None;
    for document in u.program.source_documents() {
        let reference = super::working_value::owning_definition(m, &a, document.reference())?;
        if shape::same(m, reference.value(), &u.request.payload["product_ref"])? {
            product = Some(document.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("Work proof original product"))?;
    let activity = shape::text(m, u.request.payload, "activity")?;
    let product_id = shape::text(m, product, "product_id")?;
    authority::grant_coverage(
        m,
        &grant,
        &u.request.payload["customer"],
        activity,
        product_id,
        slot,
        ("evidence.retain", &["read", "submit"]),
    )?;
    let operation = a.string_value("work.accept", m)?;
    let operations = shape::array(m, &grant, "operations")?;
    if !shape::member(m, operations, operation.value())? {
        return Err(shape::fail("Work proof original retention operation"));
    }
    let verification_value = owning_reference(m, &a, verification_ref)?;
    let constants = m.parse_owned_representation(
        &a,
        br#"{"schema":"ledger-workflow-verified-fact/1"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let field = a.string_value(u.field, m)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    a.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("schema_ref", &u.request.payload["fact_schema_ref"]),
            ("field", field.value()),
            ("value", &u.request.facts[u.field]),
            ("evidence_ref", att_value.value()),
            ("evidence_digest", digest.value()),
            ("source", &att["source"]),
            ("verification_ref", verification_value.value()),
            ("scope", &key.value()["scope"]),
            (
                "binding_or_invocation_ref",
                &u.request.payload["binding_ref"],
            ),
            ("subject", expected.value()),
        ],
        m,
    )
}
fn emit_or_match(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    value: &AuditedValue,
) -> Result<(ExactRef, Option<ConstructedEnvelope>)> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work derived proof"))?;
    let bytes = m.canonical_owned_representation(&a, value.value(), RepresentationKind::Record)?;
    let schema = m.canonical_owned_representation(
        &a,
        &value.value()["schema"],
        RepresentationKind::SourceDefinition,
    )?;
    let digest = super::records::hash_parts(
        m,
        "workflow-document",
        &[schema.bytes(), bytes.bytes()],
        true,
    )?;
    let id_size = w::usize_add(m, 5usize, digest.len() - 7)?;
    m.scan_bytes(digest.len() - 7)?;
    m.write_bytes(id_size)?;
    let reference = ExactRef {
        kind: copy(m, "workflow-verified-fact")?,
        id: format!("wdoc_{}", &digest[7..]),
        digest,
    };
    if let Some(old) = r.optional(
        m,
        &c.key,
        &reference,
        "workflow-verified-fact",
        "ledger-workflow-verified-fact/1",
    )? {
        shape::equal(m, &old, value.value())?;
        return Ok((reference, None));
    }
    m.new_body(bytes.bytes().len())?;
    let encoded = EncodedBody::owned(&a, bytes, m)?;
    let output = ConstructedEnvelope::from_encoded(
        copy_reference(m, &reference)?,
        encoded,
        Some(&a),
        m,
        RepresentationKind::Record,
    )?;
    Ok((reference, Some(output)))
}

/// Fresh proofs, or only the explicitly named replacement field. Existing app
/// proof declarations in a root operation are not Work proof declarations.
pub(super) fn resolve(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    q: &WorkProofRequest<'_>,
    proofs: &[Value],
    verifications: &[ExactRef],
) -> Result<Option<WorkProofs>> {
    let s = schema(m, program, q.payload)?;
    let normalized = NormalizedInput::normalize(m, s, q.facts)?;
    let required = normalized.required_proof_fields();
    let count = if let Some(field) = q.only_field {
        usize::from(required_field(m, required, field)?)
    } else {
        required.len()
    };
    if count == 0 && proofs.is_empty() {
        return Ok(None);
    }
    if !w::compare(m, proofs.len(), count)?.is_eq() {
        return Err(shape::fail("one Work proof per required attested field"));
    }
    let mut out = WorkProofs::new(m, count)?;
    for (i, proof) in proofs.iter().enumerate() {
        shape::closed(m, proof, &["field", "attestation_ref"], &[])?;
        let field = shape::text(m, proof, "field")?;
        let named = match q.only_field {
            Some(want) => shape::same_text(m, want, field)?,
            None => true,
        };
        if !required_field(m, required, field)? || !named {
            return Err(shape::fail("unmatched Work field proof"));
        }
        for old in &proofs[..i] {
            let old_name = shape::text(m, old, "field")?;
            if shape::same_text(m, field, old_name)? {
                return Err(shape::fail("duplicate Work field proof"));
            }
        }
        let att = shape::reference(m, &proof["attestation_ref"])?;
        let mut selected = None;
        for vr in verifications {
            let v = r.get(
                m,
                &c.key,
                vr,
                "workflow-evidence-verification",
                "ledger-workflow-evidence-verification/1",
            )?;
            let arena = m
                .representation_arena()
                .ok_or_else(|| shape::fail("owning Work proof verification"))?;
            let expected = owning_reference(m, &arena, &att)?;
            if shape::same(m, &v["attestation_ref"], expected.value())? {
                if selected.is_some() {
                    return Err(shape::fail("ambiguous Work proof verification"));
                }
                selected = Some(vr);
            }
        }
        let selected =
            selected.ok_or_else(|| shape::fail("retained Work proof verification missing"))?;
        let value = checked_value(
            m,
            r,
            c,
            &ProofUse {
                program,
                request: q,
                schema: s,
                field,
            },
            &att,
            selected,
        )?;
        let (reference, output) = emit_or_match(m, r, c, &value)?;
        out.references.push(reference);
        if let Some(output) = output {
            out.introduced.push(output);
        }
    }
    if q.only_field.is_none() && !w::compare(m, verifications.len(), proofs.len())?.is_eq() {
        return Err(shape::fail("unmatched Work verification declaration"));
    }
    Ok(Some(out))
}

/// Validate an effective retained proof set against its actual facts. For the
/// original Work also require the exact original payload's assertion refs.
pub(super) fn retained(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    q: &WorkProofRequest<'_>,
    authority_ref: &Value,
    original: bool,
) -> Result<Option<WorkProofs>> {
    let s = schema(m, program, q.payload)?;
    let normalized = NormalizedInput::normalize(m, s, q.facts)?;
    let required = normalized.required_proof_fields();
    if required.is_empty() && shape::array(m, q.payload, "fact_proofs")?.is_empty() {
        return Ok(None);
    }
    let (_, authority) = load(
        m,
        r,
        c,
        authority_ref,
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let refs = shape::array(m, &authority, "verified_fact_refs")?;
    if !w::compare(m, refs.len(), required.len())?.is_eq() {
        return Err(shape::fail("retained Work verified fact completeness"));
    }
    let mut out = WorkProofs::new(m, refs.len())?;
    for (i, v) in refs.iter().enumerate() {
        for old in &refs[..i] {
            if shape::same(m, v, old)? {
                return Err(shape::fail("duplicate retained Work verified fact"));
            }
        }
        let (reference, fact) = load(
            m,
            r,
            c,
            v,
            "workflow-verified-fact",
            "ledger-workflow-verified-fact/1",
        )?;
        let field = shape::text(m, &fact, "field")?;
        if !required_field(m, required, field)? {
            return Err(shape::fail("unmatched retained Work proof field"));
        }
        for old in &refs[..i] {
            let (_, old) = load(
                m,
                r,
                c,
                old,
                "workflow-verified-fact",
                "ledger-workflow-verified-fact/1",
            )?;
            let old_field = shape::text(m, &old, "field")?;
            if shape::same_text(m, field, old_field)? {
                return Err(shape::fail("duplicate retained Work proof field"));
            }
        }
        let att = shape::reference(m, &fact["evidence_ref"])?;
        let verification = shape::reference(m, &fact["verification_ref"])?;
        let expected = checked_value(
            m,
            r,
            c,
            &ProofUse {
                program,
                request: q,
                schema: s,
                field,
            },
            &att,
            &verification,
        )?;
        shape::equal(m, &fact, expected.value())?;
        if original {
            let proofs = shape::array(m, q.payload, "fact_proofs")?;
            let proof = shape::named(m, proofs, "field", field)?;
            shape::closed(m, proof, &["field", "attestation_ref"], &[])?;
            shape::equal(m, &proof["attestation_ref"], &fact["evidence_ref"])?;
        }
        out.references.push(reference);
    }
    let declared = shape::array(m, q.payload, "fact_proofs")?;
    if original && !w::compare(m, declared.len(), required.len())?.is_eq() {
        return Err(shape::fail("original Work proof completeness"));
    }
    Ok(Some(out))
}

/// Replace only the named field. Unchanged proof identities remain exact.
pub(super) fn replace(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    mut prior: WorkProofs,
    mut replacement: WorkProofs,
    field: &str,
) -> Result<WorkProofs> {
    if !w::compare(m, replacement.references.len(), 1usize)?.is_eq() {
        return Err(shape::fail("one replacement Work proof"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning Work proof replacement"))?;
    let bytes = w::usize_add(m, prior.owned_bytes, replacement.owned_bytes)?;
    let owner = arena.reserve_external_payload(bytes, m)?;
    let mut position = None;
    for (i, reference) in prior.references.iter().enumerate() {
        let fact = r.get(
            m,
            &c.key,
            reference,
            "workflow-verified-fact",
            "ledger-workflow-verified-fact/1",
        )?;
        let name = shape::text(m, &fact, "field")?;
        if shape::same_text(m, name, field)? && position.replace(i).is_some() {
            return Err(shape::fail("duplicate effective Work proof field"));
        }
    }
    let position = position.ok_or_else(|| shape::fail("replacement of unattested Work field"))?;
    prior
        .introduced
        .try_reserve_exact(replacement.introduced.len())
        .map_err(|_| shape::fail("Work replacement append"))?;
    prior.references[position] = replacement.references.remove(0);
    prior.introduced.append(&mut replacement.introduced);
    prior.owner = owner;
    prior.owned_bytes = bytes;
    Ok(prior)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canonical::audit_arena::AuditArena,
        policy::workflow::{
            document_digest,
            selected_program::{
                compile_selected_publication, SelectedCompilerSupport, SelectedSourceInput,
            },
            DefinitionRef,
        },
        validation_work::{OriginalAuditSession, ResourceLimits},
    };
    use serde_json::json;

    fn definition(kind: &str, value: &Value) -> Value {
        let digest = document_digest(value["schema"].as_str().unwrap(), value).unwrap();
        json!({"kind":kind,"id":format!("wdoc_{}",&digest[7..]),"digest":digest})
    }
    fn replace_ref(value: &mut Value, old: &Value, new: &Value) {
        if value == old {
            *value = new.clone();
            return;
        }
        match value {
            Value::Array(a) => {
                for v in a {
                    replace_ref(v, old, new)
                }
            }
            Value::Object(a) => {
                for v in a.values_mut() {
                    replace_ref(v, old, new)
                }
            }
            _ => {}
        }
    }
    fn program(session: &mut OriginalAuditSession) -> SelectedProgram {
        let program = compile_with_provenance(
            session,
            json!({"kind":"attested","source_slot":"support:worker","evidence_kind":"meter_quantity"}),
        );
        program.unwrap()
    }
    fn compile_with_provenance(
        session: &mut OriginalAuditSession,
        provenance: Value,
    ) -> Result<SelectedProgram> {
        let support = SelectedCompilerSupport::local_execution_with_session(session).unwrap();
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../selected_program/fixtures/local-first-policy-input.json"
        ))
        .unwrap();
        let mut docs = fixture["documents"].as_array().unwrap().clone();
        let old_schema = definition("workflow-fact-schema", &docs[0]);
        let old_product = definition("workflow-product", &docs[3]);
        docs[0]["fields"][0]["provenance"] = provenance;
        let new_schema = definition("workflow-fact-schema", &docs[0]);
        for doc in &mut docs {
            replace_ref(doc, &old_schema, &new_schema);
        }
        // Source normalizers order definition Ref sets before deriving IDs.
        docs[3]["fact_schemas"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec());
        let new_product = definition("workflow-product", &docs[3]);
        for doc in &mut docs {
            replace_ref(doc, &old_product, &new_product);
        }
        let policy = docs
            .iter_mut()
            .find(|v| v["schema"] == "ledger-workflow-policy/4")
            .unwrap();
        policy["compiler_ref"] = json!(support.reference());
        policy["rules"][0]["formula"] = json!({"kind":"unit","quantity_field":"support:quantity","unit":"support:request","rate":"1"});
        let root: DefinitionRef =
            serde_json::from_value(definition("workflow-policy", policy)).unwrap();
        docs.push(support.canonical_value().clone());
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents: docs
                    .iter()
                    .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                    .collect(),
                root_policy_ref: root,
            },
            &support,
            session,
        );
        program
    }
    fn document(m: &mut CoreMeter, kind: &str, value: Value) -> ConstructedEnvelope {
        super::super::body::document(m, kind, value).unwrap()
    }
    struct Fixture {
        context: DraftContextData,
        payload: Value,
        rows: Vec<ConstructedEnvelope>,
        worker: ExactRef,
        first_verifier: ExactRef,
        second_verifier: ExactRef,
        first_proofs: Vec<Value>,
        second_proofs: Vec<Value>,
    }
    fn fixture(m: &mut CoreMeter, p: &SelectedProgram, bad_grant: bool) -> Fixture {
        let key = OperationKey {
            logical_store_id: "proof-store".into(),
            scope: ["proof-company".into(), "proof-scope".into()],
            source: "urn:example:proof".into(),
            operation_id: "consume-original".into(),
        };
        let mut rows = Vec::new();
        let ceiling = json!({"source":key.source,"principal_id":"worker","canonical_economic_namespace":"proof-economic","customer_scope":["customer"],"operations":["evidence.retain","work.accept"],"product_selectors":[{"names":["support:product"]}],"activity_selectors":[{"names":["support:complete"]}],"link_selectors":[],"source_slots":["support:worker"],"permissions":["read","submit"],"starts_at":"2026-10-21T00:00:00.000000Z","ends_at":"2026-10-25T00:00:00.000000Z"});
        let enrollment = document(
            m,
            "workflow-source-enrollment",
            json!({"schema":"ledger-workflow-source-enrollment/1","enrollment_id":"worker","ceiling":ceiling}),
        );
        let enrollment_ref = enrollment.reference.clone();
        rows.push(enrollment);
        let mut grant = ceiling.clone();
        grant["schema"] = json!("ledger-workflow-source-grant/1");
        grant["grant_id"] = json!("worker");
        grant["revision"] = json!("1");
        grant["original_ceiling_ref"] = json!(enrollment_ref);
        if bad_grant {
            grant["operations"] = json!(["work.accept"]);
        }
        let grant = document(m, "workflow-source-grant", grant);
        let grant_ref = grant.reference.clone();
        rows.push(grant);
        let binding = document(
            m,
            "workflow-binding",
            json!({"schema":"ledger-workflow-binding/2","source_slots":[{"name":"support:worker","sources":[key.source],"evidence_kinds":["meter_quantity"]}]}),
        );
        let binding_ref = binding.reference.clone();
        rows.push(binding);
        let worker = document(
            m,
            "workflow-economic-authority",
            json!({"schema":"ledger-workflow-economic-authority/8","operation":"work.accept","original_binding_ref":binding_ref,"source_slot":"support:worker","source":key.source,"enrollment_ref":enrollment_ref,"verified_fact_refs":[]}),
        );
        let worker_ref = worker.reference.clone();
        rows.push(worker);
        let product = p
            .source_documents()
            .iter()
            .find(|d| d.canonical_value()["schema"] == "ledger-workflow-product/1")
            .unwrap();
        let schema = p
            .fact_schemas()
            .iter()
            .find(|(_, s)| s.canonical_value()["schema_id"] == "support:work-facts")
            .unwrap();
        let payload = json!({"business_operation_id":"original-business","customer":"customer","activity":"support:complete","product_ref":product.reference(),"binding_ref":binding_ref,"fact_schema_ref":schema.0,"facts":{"support:quantity":"3"},"fact_proofs":[]});
        let context = DraftContextData {
            key,
            actor: "worker".into(),
            canonical_economic_namespace: "proof-economic".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-22T00:00:03.000000Z".into(),
            accepted_at: "2026-10-22T00:00:04.000000Z".into(),
            authorities: vec![AuthorityContextData {
                source_slot: "support:worker".into(),
                enrollment_ref: enrollment_ref.clone(),
                current_state_ref: grant_ref.clone(),
                current_grant_ref: grant_ref.clone(),
                original_grant_refs: vec![grant_ref.clone()],
                binding_ref: binding_ref.clone(),
                accepted_agreement_ref: worker_ref.clone(),
                activation_ref: worker_ref.clone(),
                authority_observation_ref: None,
            }],
            source_observation_inputs: vec![],
        };
        let mut proof_refs = Vec::new();
        let mut verifier_refs = Vec::new();
        for (id, value, issued, retained) in [
            (
                "original",
                "3",
                "2026-10-22T00:00:01.000000Z",
                "2026-10-22T00:00:02.000000Z",
            ),
            (
                "replacement",
                "2",
                "2026-10-22T00:00:05.000000Z",
                "2026-10-22T00:00:06.000000Z",
            ),
        ] {
            let subject = json!({"kind":"work","logical_store_id":context.key.logical_store_id,"scope":context.key.scope,"canonical_source_namespace":context.canonical_economic_namespace,"business_operation_id":payload["business_operation_id"],"customer":payload["customer"],"activity":payload["activity"],"product_ref":payload["product_ref"],"binding_ref":payload["binding_ref"]});
            let assertion = document(
                m,
                "workflow-fact-attestation",
                json!({"schema":"ledger-workflow-fact-attestation/1","attestation_id":id,"source":context.key.source,"evidence_kind":"meter_quantity","subject":subject,"subject_schema_ref":payload["fact_schema_ref"],"claims":[{"field":"support:quantity","value":value}],"issued_at":issued,"supporting_evidence_refs":[]}),
            );
            let raw_hash =
                canonical::hex_with_work(&Sha256::digest(assertion.canonical_body.as_ref()), m)
                    .unwrap();
            let verification = document(
                m,
                "workflow-evidence-verification",
                json!({"schema":"ledger-workflow-evidence-verification/1","attestation_ref":assertion.reference,"attestation_raw_sha256":raw_hash,"attestation_digest":assertion.reference.digest,"verified_source":context.key.source,"verification_primitive":"authenticated-nominated-source-retention/1","source_grant_ref":grant_ref,"source_grant_revision":"1","authenticated_principal_ref":enrollment_ref,"trusted_received_at":retained,"subject_binding_refs":[binding_ref],"verification_evidence_refs":[]}),
            );
            proof_refs
                .push(json!({"field":"support:quantity","attestation_ref":assertion.reference}));
            verifier_refs.push(verification.reference.clone());
            rows.push(assertion);
            rows.push(verification);
        }
        Fixture {
            context,
            payload,
            rows,
            worker: worker_ref,
            first_verifier: verifier_refs.remove(0),
            second_verifier: verifier_refs.remove(0),
            first_proofs: vec![proof_refs.remove(0)],
            second_proofs: vec![proof_refs.remove(0)],
        }
    }
    fn views(rows: &[ConstructedEnvelope]) -> Vec<EconomicRecordView<'_>> {
        rows.iter()
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: &r.canonical_body,
            })
            .collect()
    }
    #[test]
    fn attested_unit_saved_bytes_reload_and_new_admission_restricts_provenance() {
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let p = program(&mut session);
        let read_arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut read_session = OriginalAuditSession::for_resource_profile_v2(&read_arena).unwrap();
        let loaded = crate::policy::workflow::selected_program::load_local_selected_execution(
            SelectedSourceInput {
                documents: p
                    .source_documents()
                    .iter()
                    .map(|d| d.canonical_bytes().to_vec())
                    .collect(),
                root_policy_ref: p.root_policy_ref().clone(),
            },
            p.compiled_plan().canonical_bytes(),
            p.compiled_plan().reference(),
            &mut read_session,
        )
        .unwrap();
        assert_eq!(
            loaded.compiled_plan().canonical_bytes(),
            p.compiled_plan().canonical_bytes()
        );
        for provenance in [
            json!({"kind":"attested","source_slot":"support:corrector","evidence_kind":"meter_quantity"}),
            json!({"kind":"attested","source_slot":"support:worker","evidence_kind":"outcome"}),
        ] {
            let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert_eq!(
                compile_with_provenance(&mut session, provenance)
                    .err()
                    .unwrap()
                    .code,
                "UNSUPPORTED_COMPOSITION"
            );
        }
    }
    #[test]
    fn work_proof_original_replacement_and_retained_identity_use_actual_consumption_times() {
        // A pure proof-consumer fixture, not a committed financial replay.
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let program = program(&mut session);
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut consumer = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let m = consumer.meter_mut();
        let mut f = fixture(m, &program, false);
        f.payload["fact_proofs"] = json!(f.first_proofs);
        let first_ref;
        {
            let v = views(&f.rows);
            let mut records = Records::borrowed_parent_app(m, &arena, &f.context.key, &v).unwrap();
            let q = WorkProofRequest {
                payload: &f.payload,
                facts: &f.payload["facts"],
                namespace: &f.context.canonical_economic_namespace,
                received_at: &f.context.received_at,
                only_field: None,
                worker_authority: None,
            };
            let mut proof = resolve(
                m,
                &mut records,
                &f.context,
                &program,
                &q,
                &f.first_proofs,
                std::slice::from_ref(&f.first_verifier),
            )
            .unwrap()
            .unwrap();
            first_ref = proof.references[0].clone();
            assert_eq!(proof.introduced.len(), 1);
            f.rows.append(&mut proof.introduced);
        }
        let original = document(
            m,
            "workflow-economic-authority",
            json!({"schema":"ledger-workflow-economic-authority/8","operation":"work.accept","original_binding_ref":f.payload["binding_ref"],"source_slot":"support:worker","source":f.context.key.source,"enrollment_ref":f.context.authorities[0].enrollment_ref,"verified_fact_refs":[first_ref]}),
        );
        let original_ref = json!(original.reference);
        f.rows.push(original);
        let mut replacement_facts = f.payload["facts"].clone();
        replacement_facts["support:quantity"] = json!("2");
        let second_ref;
        {
            let v = views(&f.rows);
            let mut r = Records::borrowed_parent_app(m, &arena, &f.context.key, &v).unwrap();
            let original_q = WorkProofRequest {
                payload: &f.payload,
                facts: &f.payload["facts"],
                namespace: &f.context.canonical_economic_namespace,
                received_at: &f.context.received_at,
                only_field: None,
                worker_authority: Some(&original_ref),
            };
            let prior = retained(
                m,
                &mut r,
                &f.context,
                &program,
                &original_q,
                &original_ref,
                true,
            )
            .unwrap()
            .unwrap();
            assert_eq!(prior.references, vec![first_ref.clone()]);
            let replacement_q = WorkProofRequest {
                payload: &f.payload,
                facts: &replacement_facts,
                namespace: &f.context.canonical_economic_namespace,
                received_at: "2026-10-22T00:00:07.000000Z",
                only_field: Some("support:quantity"),
                worker_authority: Some(&original_ref),
            };
            let new = resolve(
                m,
                &mut r,
                &f.context,
                &program,
                &replacement_q,
                &f.second_proofs,
                std::slice::from_ref(&f.second_verifier),
            )
            .unwrap()
            .unwrap();
            let mut effective =
                replace(m, &mut r, &f.context, prior, new, "support:quantity").unwrap();
            second_ref = effective.references[0].clone();
            assert_ne!(first_ref, second_ref);
            f.rows.append(&mut effective.introduced);
        }
        let current = document(
            m,
            "workflow-economic-authority",
            json!({"schema":"ledger-workflow-economic-authority/8","verified_fact_refs":[second_ref]}),
        );
        let current_ref = json!(current.reference);
        f.rows.push(current);
        let v = views(&f.rows);
        let mut r = Records::borrowed_parent_app(m, &arena, &f.context.key, &v).unwrap();
        let mut q = WorkProofRequest {
            payload: &f.payload,
            facts: &replacement_facts,
            namespace: &f.context.canonical_economic_namespace,
            received_at: "2026-10-22T00:00:08.000000Z",
            only_field: None,
            worker_authority: Some(&original_ref),
        };
        let effective = retained(m, &mut r, &f.context, &program, &q, &current_ref, false)
            .unwrap()
            .unwrap();
        assert_eq!(effective.references, vec![second_ref]);
        assert!(effective.introduced.is_empty());
        q.received_at = &f.context.received_at;
        assert_eq!(
            retained(m, &mut r, &f.context, &program, &q, &current_ref, false)
                .err()
                .unwrap()
                .detail,
            "Work proof historical trusted time"
        );
        q.facts = &f.payload["facts"];
        assert_eq!(
            retained(m, &mut r, &f.context, &program, &q, &original_ref, true)
                .unwrap()
                .unwrap()
                .references,
            vec![first_ref]
        );
    }
    #[test]
    fn work_proof_refusals_preserve_exact_field_subject_grant_and_verification_joins() {
        for case in [
            "missing",
            "duplicate",
            "unmatched",
            "value",
            "subject",
            "verification",
            "worker",
            "grant",
        ] {
            let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let program = program(&mut session);
            let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
            let mut consumer = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let m = consumer.meter_mut();
            let mut f = fixture(m, &program, case == "grant");
            let mut proofs = f.first_proofs.clone();
            let mut verifiers = vec![f.first_verifier.clone()];
            match case {
                "missing" => proofs.clear(),
                "duplicate" => proofs.push(proofs[0].clone()),
                "unmatched" => proofs[0]["field"] = json!("support:other"),
                "value" => f.payload["facts"]["support:quantity"] = json!("2"),
                "subject" => f.payload["business_operation_id"] = json!("different-business"),
                "verification" => verifiers[0] = f.second_verifier.clone(),
                "worker" => f.context.authorities[0].enrollment_ref = f.worker.clone(),
                _ => {}
            }
            let v = views(&f.rows);
            let mut r = Records::borrowed_parent_app(m, &arena, &f.context.key, &v).unwrap();
            let q = WorkProofRequest {
                payload: &f.payload,
                facts: &f.payload["facts"],
                namespace: &f.context.canonical_economic_namespace,
                received_at: &f.context.received_at,
                only_field: None,
                worker_authority: None,
            };
            let error = resolve(m, &mut r, &f.context, &program, &q, &proofs, &verifiers)
                .err()
                .unwrap();
            assert_eq!(error.code, "INTEGRITY_FAILURE", "{case}: {error:?}");
        }
    }
}
