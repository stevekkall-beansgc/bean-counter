//! Immutable per-kind intrinsic DATA. No factory here certifies an origin,
//! custody, snapshot authenticity, current authority or a financial commit.
//!
//! The initial factory proves ONLY retained-evidence/1's closed intrinsic
//! dictionary, canonical document identity, content grammar/hash, typed times
//! and reference grammar. Its subject/root/time/terms associations are NOT
//! proved; they remain consuming-context obligations. Other kinds fail closed.
use super::{
    data::{ExactRef, RecordInput},
    inventory::evidence,
    CoreMeter,
};
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    validation_work::{self as w, ValidationWork},
    Error, Result,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

/// Actual audit/economic callbacks, not a permission or an increased ceiling.
/// An original auditor must enforce its existing original-recipe limits. Raw
/// draft callers continue using their existing ordinary CoreMeter unchanged.
pub trait IntrinsicValidationWork: ValidationWork {
    #[track_caller]
    fn visit_named(&mut self, name: &'static str) -> Result<()>;
}
impl IntrinsicValidationWork for CoreMeter {
    #[track_caller]
    fn visit_named(&mut self, name: &'static str) -> Result<()> {
        self.visit(name)
    }
}
pub struct IntrinsicDataScope {
    logical_store_id: String,
    scope: [String; 2],
    _nonzero: u8,
}
impl IntrinsicDataScope {
    /// A pure DATA lifetime anchor. The host separately binds it to the actual
    /// held StoreRoot/snapshot in its private completed-origin capability.
    pub fn new(logical_store_id: String, scope: [String; 2]) -> Self {
        Self {
            logical_store_id,
            scope,
            _nonzero: 1,
        }
    }
    pub fn logical_store_id(&self) -> &str {
        &self.logical_store_id
    }
    pub fn scope(&self) -> &[String; 2] {
        &self.scope
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidencePurpose {
    CustomerRegistration,
    SourceEnrollment,
    Assent,
    OperatorAttestation,
    Funding,
    AdmissionStop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceInstallationMode {
    RealLocal,
    IsolatedSynthetic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceMedia {
    Utf8Text,
    OctetStream,
}
pub enum EvidenceMetadata {
    Registration {
        acceptor: String,
    },
    Enrollment {
        source: String,
        principal_id: String,
    },
    Assent {
        terms_digest: String,
        acceptor: String,
        accepted_at: Timestamp,
    },
    Operator {
        terms_digest: String,
        acceptor: String,
        assent_evidence_ref: ExactRef,
    },
    Funding {
        resource_owner: String,
        payer: String,
    },
    Stop {
        binding_ref: ExactRef,
        target_transition_ref: ExactRef,
        reason: String,
    },
}
pub struct RetainedEvidenceFacts {
    purpose: EvidencePurpose,
    customer: String,
    agreement_id: Option<String>,
    media: EvidenceMedia,
    installation_mode: EvidenceInstallationMode,
    content: Vec<u8>,
    content_sha256: String,
    metadata: EvidenceMetadata,
    retained_at: Timestamp,
    retained_by: String,
    authority_ref: ExactRef,
}
impl RetainedEvidenceFacts {
    pub fn purpose(&self) -> EvidencePurpose {
        self.purpose
    }
    pub fn customer(&self) -> &str {
        &self.customer
    }
    pub fn agreement_id(&self) -> Option<&str> {
        self.agreement_id.as_deref()
    }
    pub fn installation_mode(&self) -> EvidenceInstallationMode {
        self.installation_mode
    }
    pub fn media(&self) -> EvidenceMedia {
        self.media
    }
    pub fn content(&self) -> &[u8] {
        &self.content
    }
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
    pub fn metadata(&self) -> &EvidenceMetadata {
        &self.metadata
    }
    pub fn retained_at(&self) -> &Timestamp {
        &self.retained_at
    }
    pub fn retained_by(&self) -> &str {
        &self.retained_by
    }
    pub fn authority_ref(&self) -> &ExactRef {
        &self.authority_ref
    }
}
pub struct IntrinsicRecord<'inv> {
    owner: &'inv IntrinsicDataScope,
    input: &'inv RecordInput,
    body: Value,
    evidence: RetainedEvidenceFacts,
}
impl<'inv> IntrinsicRecord<'inv> {
    pub fn reference(&self) -> &ExactRef {
        &self.input.reference
    }
    pub fn canonical_body(&self) -> &[u8] {
        &self.input.canonical_body
    }
    pub fn body(&self) -> &Value {
        &self.body
    }
    pub fn retained_evidence(&self) -> &RetainedEvidenceFacts {
        &self.evidence
    }
    /// Paid identity/lifetime association only, never authorization. The host
    /// must separately complete the actual original recipe/custody proof.
    pub fn consume(
        &self,
        work: &mut dyn IntrinsicValidationWork,
        owner: &IntrinsicDataScope,
        expected: &ExactRef,
    ) -> Result<&RetainedEvidenceFacts> {
        work.visit_named("intrinsic.owner")?;
        if !std::ptr::eq(self.owner, owner) {
            return Err(fail("different intrinsic DATA owner"));
        }
        same_required(work, &self.input.reference.kind, &expected.kind)?;
        same_required(work, &self.input.reference.id, &expected.id)?;
        same_required(work, &self.input.reference.digest, &expected.digest)?;
        Ok(&self.evidence)
    }
}
fn fail(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
fn text<'a>(work: &mut dyn IntrinsicValidationWork, v: &'a Value, key: &str) -> Result<&'a str> {
    work.visit_named("intrinsic.field")?;
    let value = v.get(key).ok_or_else(|| fail("missing intrinsic field"))?;
    work.visit_named("intrinsic.text_type")?;
    value.as_str().ok_or_else(|| fail("intrinsic text field"))
}
fn field<'a>(work: &mut dyn IntrinsicValidationWork, v: &'a Value, key: &str) -> Result<&'a Value> {
    work.visit_named("intrinsic.field")?;
    v.get(key).ok_or_else(|| fail("missing intrinsic field"))
}
fn same(work: &mut dyn IntrinsicValidationWork, a: &str, b: &str) -> Result<bool> {
    work.visit_named("intrinsic.string_identity")?;
    if w::compare(work, a.len(), b.len())? != Ordering::Equal {
        return Ok(false);
    }
    for (a, b) in a.bytes().zip(b.bytes()) {
        work.scan_bytes(2)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
fn same_required(work: &mut dyn IntrinsicValidationWork, a: &str, b: &str) -> Result<()> {
    if same(work, a, b)? {
        Ok(())
    } else {
        Err(fail("intrinsic association mismatch"))
    }
}
fn closed(
    work: &mut dyn IntrinsicValidationWork,
    v: &Value,
    required: &[&str],
    optional: &[&str],
) -> Result<()> {
    work.visit_named("intrinsic.object_type")?;
    let map = v.as_object().ok_or_else(|| fail("intrinsic object"))?;
    for key in required {
        work.visit_named("intrinsic.required_member")?;
        if !map.contains_key(*key) {
            return Err(fail("missing intrinsic field"));
        }
    }
    for key in map.keys() {
        let mut found = false;
        for allowed in required.iter().chain(optional) {
            if same(work, key, allowed)? {
                found = true;
                break;
            }
        }
        if !found {
            return Err(fail("unknown intrinsic field"));
        }
    }
    Ok(())
}
fn bounded_text(work: &mut dyn IntrinsicValidationWork, s: &str, max: usize) -> Result<String> {
    if w::compare(work, s.len(), 0usize)? != Ordering::Greater
        || w::compare(work, s.len(), max)? == Ordering::Greater
    {
        return Err(fail("intrinsic text bound"));
    }
    work.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn opaque(work: &mut dyn IntrinsicValidationWork, s: &str, max: usize) -> Result<String> {
    if w::compare(work, s.len(), 0usize)? != Ordering::Greater
        || w::compare(work, s.len(), max)? == Ordering::Greater
    {
        return Err(fail("intrinsic text bound"));
    }
    for ch in s.chars() {
        work.scan_bytes(ch.len_utf8())?;
        work.visit_named("intrinsic.control_character")?;
        if ch.is_control() {
            return Err(fail("intrinsic control character"));
        }
    }
    work.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn digest(work: &mut dyn IntrinsicValidationWork, s: &str, prefixed: bool) -> Result<()> {
    let hex = if prefixed {
        if w::compare(work, s.len(), 71usize)? != Ordering::Equal {
            return Err(fail("digest length"));
        }
        work.visit_named("intrinsic.digest_slice")?;
        same_required(
            work,
            s.get(..7).ok_or_else(|| fail("digest prefix boundary"))?,
            "sha256:",
        )?;
        &s[7..]
    } else {
        s
    };
    if w::compare(work, hex.len(), 64usize)? != Ordering::Equal {
        return Err(fail("digest length"));
    }
    for byte in hex.bytes() {
        work.scan_bytes(1)?;
        work.visit_named("intrinsic.hex")?;
        if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte) {
            return Err(fail("digest hex"));
        }
    }
    Ok(())
}
fn reference_parts(
    work: &mut dyn IntrinsicValidationWork,
    kind_data: &str,
    id: &str,
    hash: &str,
    kind: &str,
    prefix: &str,
    doc: bool,
) -> Result<()> {
    same_required(work, kind_data, kind)?;
    let length = w::usize_add(work, prefix.len(), 64)?;
    if w::compare(work, id.len(), length)? != Ordering::Equal {
        return Err(fail("intrinsic identity length"));
    }
    work.visit_named("intrinsic.identity_slice")?;
    same_required(
        work,
        id.get(..prefix.len())
            .ok_or_else(|| fail("identity prefix boundary"))?,
        prefix,
    )?;
    digest(work, &id[prefix.len()..], false)?;
    digest(work, hash, true)?;
    if doc {
        same_required(work, &id[prefix.len()..], &hash[7..])?;
    }
    Ok(())
}
fn reference(
    work: &mut dyn IntrinsicValidationWork,
    v: &Value,
    kind: &str,
    prefix: &str,
    doc: bool,
) -> Result<ExactRef> {
    closed(work, v, &["kind", "id", "digest"], &[])?;
    let kind_data = text(work, v, "kind")?;
    let id = text(work, v, "id")?;
    let hash = text(work, v, "digest")?;
    reference_parts(work, kind_data, id, hash, kind, prefix, doc)?;
    work.write_bytes(kind_data.len())?;
    work.write_bytes(id.len())?;
    work.write_bytes(hash.len())?;
    Ok(ExactRef {
        kind: kind_data.into(),
        id: id.into(),
        digest: hash.into(),
    })
}

fn metadata(
    work: &mut dyn IntrinsicValidationWork,
    purpose: &str,
    v: &Value,
) -> Result<(EvidencePurpose, EvidenceMetadata)> {
    for (name, tag) in [
        (
            "customer_registration",
            EvidencePurpose::CustomerRegistration,
        ),
        ("source_enrollment", EvidencePurpose::SourceEnrollment),
        ("assent", EvidencePurpose::Assent),
        ("operator_attestation", EvidencePurpose::OperatorAttestation),
        ("funding", EvidencePurpose::Funding),
        ("admission_stop", EvidencePurpose::AdmissionStop),
    ] {
        if !same(work, purpose, name)? {
            continue;
        }
        work.visit_named("intrinsic.metadata_variant")?;
        let data = match tag {
            EvidencePurpose::CustomerRegistration => {
                closed(work, v, &["acceptor"], &[])?;
                let s = text(work, v, "acceptor")?;
                EvidenceMetadata::Registration {
                    acceptor: opaque(work, s, 128)?,
                }
            }
            EvidencePurpose::SourceEnrollment => {
                closed(work, v, &["source", "principal_id"], &[])?;
                let s = text(work, v, "source")?;
                let source = opaque(work, s, 256)?;
                let s = text(work, v, "principal_id")?;
                EvidenceMetadata::Enrollment {
                    source,
                    principal_id: opaque(work, s, 128)?,
                }
            }
            EvidencePurpose::Assent => {
                closed(work, v, &["terms_digest", "acceptor", "accepted_at"], &[])?;
                let d = text(work, v, "terms_digest")?;
                digest(work, d, true)?;
                work.write_bytes(d.len())?;
                let terms_digest = d.to_owned();
                let s = text(work, v, "acceptor")?;
                let acceptor = opaque(work, s, 128)?;
                let s = text(work, v, "accepted_at")?;
                let accepted_at = Timestamp::parse_with_work(s, work)?;
                EvidenceMetadata::Assent {
                    terms_digest,
                    acceptor,
                    accepted_at,
                }
            }
            EvidencePurpose::OperatorAttestation => {
                closed(
                    work,
                    v,
                    &[
                        "terms_digest",
                        "acceptor",
                        "assent_evidence_ref",
                        "statement",
                    ],
                    &[],
                )?;
                let d = text(work, v, "terms_digest")?;
                digest(work, d, true)?;
                work.write_bytes(d.len())?;
                let terms_digest = d.to_owned();
                let s = text(work, v, "acceptor")?;
                let acceptor = opaque(work, s, 128)?;
                let r = field(work, v, "assent_evidence_ref")?;
                let assent_evidence_ref = reference(work, r, "workflow-evidence", "wdoc_", true)?;
                let s = text(work, v, "statement")?;
                same_required(work, s, "operator_verified_exact_terms_acceptance")?;
                EvidenceMetadata::Operator {
                    terms_digest,
                    acceptor,
                    assent_evidence_ref,
                }
            }
            EvidencePurpose::Funding => {
                closed(work, v, &["resource_owner", "payer", "mode"], &[])?;
                let s = text(work, v, "resource_owner")?;
                let resource_owner = opaque(work, s, 128)?;
                let s = text(work, v, "payer")?;
                let payer = opaque(work, s, 128)?;
                let s = text(work, v, "mode")?;
                same_required(work, s, "customer_local")?;
                EvidenceMetadata::Funding {
                    resource_owner,
                    payer,
                }
            }
            EvidencePurpose::AdmissionStop => {
                closed(
                    work,
                    v,
                    &["binding_ref", "target_transition_ref", "reason"],
                    &[],
                )?;
                let r = field(work, v, "binding_ref")?;
                let binding_ref = reference(work, r, "workflow-binding", "wdoc_", true)?;
                let r = field(work, v, "target_transition_ref")?;
                let target_transition_ref =
                    reference(work, r, "workflow-activation", "wactivation_", false)?;
                let s = text(work, v, "reason")?;
                let reason = bounded_text(work, s, 1024)?;
                EvidenceMetadata::Stop {
                    binding_ref,
                    target_transition_ref,
                    reason,
                }
            }
        };
        return Ok((tag, data));
    }
    Err(Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "intrinsic evidence purpose",
    ))
}
fn document_digest(
    work: &mut dyn IntrinsicValidationWork,
    schema: &str,
    raw: &[u8],
) -> Result<String> {
    work.write_bytes(schema.len())?;
    let schema = CanonicalBytes::from_value_with_work_bounded(&json!(schema), work, 262144)?;
    let mut hash = Sha256::new();
    let prefix = b"bean-counter/workflow-document/1\0";
    work.hash_bytes(prefix.len())?;
    hash.update(prefix);
    for part in [b"[".as_slice(), schema.as_slice(), b",", raw, b"]"] {
        work.hash_bytes(part.len())?;
        hash.update(part);
    }
    work.write_bytes(7)?;
    Ok(format!(
        "sha256:{}",
        canonical::hex_with_work(&hash.finalize(), work)?
    ))
}
/// This does not check an introduction, original operation/manifest/receipt,
/// required root body, subject associations, current rights or time eligibility.
/// CompletedOrigin is a separate host-private result of actual origin replay.
pub fn validate_retained_evidence_with_work<'inv>(
    owner: &'inv IntrinsicDataScope,
    input: &'inv RecordInput,
    work: &mut dyn IntrinsicValidationWork,
) -> Result<IntrinsicRecord<'inv>> {
    // Validate borrowed header fields before any header/string materialization.
    reference_parts(
        work,
        &input.reference.kind,
        &input.reference.id,
        &input.reference.digest,
        "workflow-evidence",
        "wdoc_",
        true,
    )?;
    if w::compare(work, input.canonical_body.len(), 4194304usize)? == Ordering::Greater {
        return Err(fail("intrinsic body bound"));
    }
    work.scan_bytes(input.canonical_body.len())?;
    let body = canonical::parse_bounded(&input.canonical_body, 4194304)?;
    let canonical = CanonicalBytes::from_value_with_work_bounded(&body, work, 4194304)?;
    work.visit_named("intrinsic.canonical_identity")?;
    if w::compare(work, canonical.as_slice().len(), input.canonical_body.len())? != Ordering::Equal
    {
        return Err(fail("intrinsic canonical body"));
    }
    for (a, b) in canonical.as_slice().iter().zip(&input.canonical_body) {
        work.scan_bytes(2)?;
        if a != b {
            return Err(fail("intrinsic canonical body"));
        }
    }
    closed(
        work,
        &body,
        &[
            "schema",
            "installation_mode",
            "purpose",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
            "retained_at",
            "retained_by",
            "authority_ref",
        ],
        &[],
    )?;
    let schema = text(work, &body, "schema")?;
    same_required(work, schema, "ledger-workflow-retained-evidence/1")?;
    let mode = text(work, &body, "installation_mode")?;
    let installation_mode = if same(work, mode, "real_local")? {
        EvidenceInstallationMode::RealLocal
    } else if same(work, mode, "isolated_synthetic")? {
        EvidenceInstallationMode::IsolatedSynthetic
    } else {
        return Err(fail("intrinsic installation mode"));
    };
    let hash = document_digest(work, schema, &input.canonical_body)?;
    same_required(work, &hash, &input.reference.digest)?;
    let subject = field(work, &body, "subject")?;
    closed(work, subject, &["customer"], &["agreement_id"])?;
    let s = text(work, subject, "customer")?;
    let customer = opaque(work, s, 128)?;
    work.visit_named("intrinsic.optional_agreement")?;
    let agreement_id = if subject.get("agreement_id").is_some() {
        let s = text(work, subject, "agreement_id")?;
        Some(opaque(work, s, 128)?)
    } else {
        None
    };
    let purpose = text(work, &body, "purpose")?;
    let meta = field(work, &body, "metadata")?;
    let (purpose, metadata) = metadata(work, purpose, meta)?;
    work.visit_named("intrinsic.stop_subject")?;
    if purpose == EvidencePurpose::AdmissionStop && agreement_id.is_none() {
        return Err(fail("stop agreement subject"));
    }
    let media = text(work, &body, "media_type")?;
    let media = if same(work, media, "text/plain;charset=utf-8")? {
        EvidenceMedia::Utf8Text
    } else if same(work, media, "application/octet-stream")? {
        EvidenceMedia::OctetStream
    } else {
        return Err(fail("intrinsic evidence media"));
    };
    let encoded = text(work, &body, "content_base64")?;
    let content = evidence::decode_data(work, encoded)?;
    work.visit_named("intrinsic.utf8_media")?;
    if media == EvidenceMedia::Utf8Text {
        work.scan_bytes(content.len())?;
        work.visit_named("intrinsic.utf8_content")?;
        std::str::from_utf8(&content).map_err(|_| fail("intrinsic content UTF8"))?;
    }
    let hash = text(work, &body, "content_sha256")?;
    digest(work, hash, false)?;
    work.hash_bytes(content.len())?;
    let actual = canonical::hex_with_work(&Sha256::digest(&content), work)?;
    same_required(work, &actual, hash)?;
    work.write_bytes(hash.len())?;
    let content_sha256 = hash.to_owned();
    let s = text(work, &body, "retained_at")?;
    let retained_at = Timestamp::parse_with_work(s, work)?;
    let s = text(work, &body, "retained_by")?;
    let retained_by = opaque(work, s, 128)?;
    let a = field(work, &body, "authority_ref")?;
    let authority_ref = reference(work, a, "workflow-authority", "wdoc_", true)?;
    Ok(IntrinsicRecord {
        owner,
        input,
        body,
        evidence: RetainedEvidenceFacts {
            purpose,
            customer,
            agreement_id,
            media,
            installation_mode,
            content,
            content_sha256,
            metadata,
            retained_at,
            retained_by,
            authority_ref,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation_work::{ArithmeticOperation, ValidationVisit};
    use num_bigint::BigInt;
    fn funding() -> RecordInput {
        let source: Value =
            serde_json::from_str(include_str!("fixtures/prerequisite-input-data.json")).unwrap();
        let row = source["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "A.funding_evidence")
            .unwrap();
        let hex = row["canonical_body_hex"].as_str().unwrap();
        let raw = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        RecordInput {
            reference: serde_json::from_value(row["reference"].clone()).unwrap(),
            canonical_body: raw,
        }
    }
    fn rebind(input: &mut RecordInput, change: impl FnOnce(&mut Value)) {
        let mut body = canonical::parse_bounded(&input.canonical_body, 4194304).unwrap();
        change(&mut body);
        input.canonical_body = CanonicalBytes::from_value(&body).unwrap().into_vec();
        let mut m = CoreMeter::new(65536).unwrap();
        let digest = document_digest(
            &mut m,
            body["schema"].as_str().unwrap(),
            &input.canonical_body,
        )
        .unwrap();
        input.reference.id = format!("wdoc_{}", &digest[7..]);
        input.reference.digest = digest;
    }
    fn owner() -> IntrinsicDataScope {
        IntrinsicDataScope::new("fixture-store".into(), ["fixture".into(), "sandbox".into()])
    }
    #[test]
    fn evidence_intrinsic_is_immutable_data_and_each_consume_pays_association() {
        let input = funding();
        let owner = owner();
        let other =
            IntrinsicDataScope::new("fixture-store".into(), ["fixture".into(), "sandbox".into()]);
        let mut m = CoreMeter::new(65536).unwrap();
        let parsed = validate_retained_evidence_with_work(&owner, &input, &mut m).unwrap();
        assert_eq!(
            parsed.retained_evidence().purpose(),
            EvidencePurpose::Funding
        );
        assert_eq!(parsed.retained_evidence().content().len(), 58);
        assert_eq!(parsed.canonical_body(), input.canonical_body);
        let before = m.usage().semantic_work;
        parsed.consume(&mut m, &owner, &input.reference).unwrap();
        let first = m.usage().semantic_work - before;
        parsed.consume(&mut m, &owner, &input.reference).unwrap();
        assert_eq!(m.usage().semantic_work - before, first * 2);
        assert!(parsed.consume(&mut m, &other, &input.reference).is_err());
        let mut wrong = input.reference.clone();
        wrong.digest.replace_range(7..8, "0");
        assert!(parsed.consume(&mut m, &owner, &wrong).is_err());
    }
    #[test]
    fn evidence_intrinsic_rehashed_unknown_metadata_and_content_hash_refuse() {
        for variant in 0..3 {
            let mut input = funding();
            rebind(&mut input, |b| match variant {
                0 => {
                    b["metadata"]["unapproved"] = json!(true);
                }
                1 => {
                    b["content_sha256"] = json!("0".repeat(64));
                }
                _ => {
                    b["media_type"] = json!("application/json");
                }
            });
            let owner = owner();
            let mut m = CoreMeter::new(65536).unwrap();
            assert!(validate_retained_evidence_with_work(&owner, &input, &mut m).is_err());
        }
        let mut input = funding();
        input.reference.digest = format!("sha256🙂{}", "0".repeat(61));
        let owner = owner();
        let mut m = CoreMeter::new(65536).unwrap();
        assert!(validate_retained_evidence_with_work(&owner, &input, &mut m).is_err());
    }
    #[test]
    fn stop_reason_preserves_multiline_text_with_paid_length_only_bounds() {
        let data: Value =
            serde_json::from_str(include_str!("fixtures/prerequisite-input-data.json")).unwrap();
        let find = |name: &str| {
            data["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["name"] == name)
                .unwrap()["reference"]
                .clone()
        };
        for reason in [
            "line one\nline two".to_owned(),
            String::new(),
            "x".repeat(1025),
        ] {
            let mut input = funding();
            rebind(&mut input, |b| {
                b["purpose"] = json!("admission_stop");
                b["subject"]["agreement_id"] = json!("agreement-A");
                b["metadata"] = json!({"binding_ref":find("A.binding"),"target_transition_ref":find("A.activation"),"reason":reason});
            });
            let owner = owner();
            let mut m = CoreMeter::new(65536).unwrap();
            let result = validate_retained_evidence_with_work(&owner, &input, &mut m);
            if reason.is_empty() || reason.len() > 1024 {
                assert!(result.is_err());
            } else {
                match result.unwrap().retained_evidence().metadata() {
                    EvidenceMetadata::Stop { reason: got, .. } => assert_eq!(got, &reason),
                    _ => panic!("stop metadata expected"),
                }
            }
        }
    }
    #[test]
    fn checked_operand_reuse_preserves_exact_pool_owner_and_scope_association() {
        use super::super::{data::OperationKey, records::Records};
        let input = funding();
        let owner = owner();
        let key = OperationKey {
            logical_store_id: "fixture-store".into(),
            scope: ["fixture".into(), "sandbox".into()],
            source: "fixture-source".into(),
            operation_id: "consume".into(),
        };
        let mut audit = CoreMeter::new(65536).unwrap();
        let parsed = validate_retained_evidence_with_work(&owner, &input, &mut audit).unwrap();
        let mut m = CoreMeter::new(65536).unwrap();
        let mut pool = Records::new(&mut m, std::slice::from_ref(&input))
            .unwrap()
            .with_intrinsic_evidence(&mut m, &key, &owner, &[&parsed])
            .unwrap();
        let body = pool
            .get(
                &mut m,
                &key,
                &input.reference,
                "workflow-evidence",
                "ledger-workflow-retained-evidence/1",
            )
            .unwrap();
        assert_eq!(&body, parsed.body());
        assert_eq!(
            pool.evidence_facts(&mut m, &body)
                .unwrap()
                .unwrap()
                .purpose(),
            EvidencePurpose::Funding
        );
        let mut substituted = body.clone();
        substituted.legacy_mut()["subject"]["customer"] = json!("other");
        assert!(pool.evidence_facts(&mut m, &substituted).unwrap().is_none());
        let mut wrong_key = key.clone();
        wrong_key.scope[1] = "other-sandbox".into();
        assert!(Records::new(&mut m, std::slice::from_ref(&input))
            .unwrap()
            .with_intrinsic_evidence(&mut m, &wrong_key, &owner, &[&parsed])
            .is_err());
        assert!(Records::new(&mut m, std::slice::from_ref(&input))
            .unwrap()
            .with_intrinsic_evidence(&mut m, &key, &owner, &[&parsed, &parsed])
            .is_err());
    }
    #[test]
    fn checked_evidence_still_checks_current_subject_time_and_historical_root() {
        use super::super::{
            data::{DraftContextData, OperationKey},
            inventory::evidence,
            records::Records,
            representation::InvocationToken,
        };
        let input = funding();
        let owner = owner();
        let mut audit = CoreMeter::new(65536).unwrap();
        let parsed = validate_retained_evidence_with_work(&owner, &input, &mut audit).unwrap();
        let source: Value =
            serde_json::from_str(include_str!("fixtures/prerequisite-input-data.json")).unwrap();
        let row = source["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "P1.authority")
            .unwrap();
        let raw = row["canonical_body_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        let root = RecordInput {
            reference: serde_json::from_value(row["reference"].clone()).unwrap(),
            canonical_body: raw,
        };
        let c = DraftContextData {
            key: OperationKey {
                logical_store_id: "fixture-store".into(),
                scope: ["fixture".into(), "sandbox".into()],
                source: "fixture-source".into(),
                operation_id: "consume".into(),
            },
            actor: "fixture-admin".into(),
            canonical_economic_namespace: "fixture-source".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-13T00:00:07.000000Z".into(),
            accepted_at: "2026-10-13T00:00:07.000000Z".into(),
            authorities: vec![],
            source_observation_inputs: vec![],
        };
        let subject = parsed.body()["subject"].clone();
        let metadata = parsed.body()["metadata"].clone();
        for variant in 0..4 {
            let mut m = CoreMeter::new(65536).unwrap();
            let inputs = if variant == 3 {
                vec![input.clone()]
            } else {
                vec![input.clone(), root.clone()]
            };
            let mut records = Records::new(&mut m, &inputs)
                .unwrap()
                .with_intrinsic_evidence(&mut m, &c.key, &owner, &[&parsed])
                .unwrap();
            let mut expected_subject = subject.clone();
            if variant == 1 {
                expected_subject["customer"] = json!("wrong-customer");
            }
            let before = json!({"at":if variant == 2 { "2026-10-13T00:00:05.000000Z" } else { "2026-10-13T00:00:07.000000Z" }});
            let result = evidence::retained(
                &mut m,
                &mut records,
                &c,
                &InvocationToken::new(),
                parsed.body(),
                "funding",
                &expected_subject,
                &metadata,
                &before,
                "at",
            );
            assert_eq!(
                result.is_ok(),
                variant == 0,
                "variant {variant}: {result:?}"
            );
        }
    }
    struct Deny {
        meter: CoreMeter,
        attempted: bool,
    }
    impl ValidationWork for Deny {
        fn visit(&mut self, k: ValidationVisit) -> Result<()> {
            ValidationWork::visit(&mut self.meter, k)
        }
        fn arithmetic(&mut self, k: ArithmeticOperation, n: &[&BigInt]) -> Result<()> {
            ValidationWork::arithmetic(&mut self.meter, k, n)
        }
        fn scan_bytes(&mut self, n: usize) -> Result<()> {
            self.meter.scan_bytes(n)
        }
        fn write_bytes(&mut self, n: usize) -> Result<()> {
            self.meter.write_bytes(n)
        }
        fn hash_bytes(&mut self, n: usize) -> Result<()> {
            self.meter.hash_bytes(n)
        }
    }
    impl IntrinsicValidationWork for Deny {
        fn visit_named(&mut self, k: &'static str) -> Result<()> {
            if k == "evidence.base64_digit" {
                self.attempted = true;
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "intrinsic callback denied",
                ));
            }
            self.meter.visit(k)
        }
    }
    #[test]
    fn evidence_intrinsic_callback_refusal_returns_no_record() {
        let input = funding();
        let owner = owner();
        let mut work = Deny {
            meter: CoreMeter::new(65536).unwrap(),
            attempted: false,
        };
        let result = validate_retained_evidence_with_work(&owner, &input, &mut work);
        assert_eq!(result.err().unwrap().code, "PROGRAM_BUDGET_EXCEEDED");
        assert!(work.attempted);
    }
    #[test]
    fn original_raw_decoder_and_intrinsic_callback_decoder_have_identical_work() {
        let mut raw = CoreMeter::new(65536).unwrap();
        let mut generic = CoreMeter::new(65536).unwrap();
        let a = evidence::decode(&mut raw, "YWJjZA==").unwrap();
        let b = evidence::decode_data(&mut generic, "YWJjZA==").unwrap();
        assert_eq!(a, b);
        assert_eq!(raw.usage(), generic.usage());
    }
}
