//! Actual enrolled-customer/fullterms/retained-assent joins for AppAccept8.
//! The output owns raw retained operands, never a completed original or consent
//! flag. Fresh/replay callers separately authenticate the captured actor/time
//! and every dependency's earliest original before private preparation.
use super::super::{
    current::source::{self, SourceRecord},
    request_wrapper as check,
};
use super::*;
use ledgerlab_core::canonical::audit_arena::ExternalBufferLease;
use std::ops::Deref;

pub(in super::super) struct ObservedCustomerConsent {
    registration: SourceRecord,
    evidence: SourceRecord,
    attestation: SourceRecord,
    assent: wire::SourceRecord,
}
impl ObservedCustomerConsent {
    pub(in super::super) fn records(&self) -> [&SourceRecord; 3] {
        [&self.registration, &self.evidence, &self.attestation]
    }
    pub(in super::super) fn assent(&self) -> &wire::SourceRecord {
        &self.assent
    }
}
struct OwnedTime {
    value: Timestamp,
    _lease: ExternalBufferLease,
}
impl Deref for OwnedTime {
    type Target = Timestamp;
    fn deref(&self) -> &Timestamp {
        &self.value
    }
}
fn time(value: &Value, key: &str, s: &mut OriginalAuditSession) -> Result<OwnedTime> {
    // Same pinned fixed UTC render/transient requested payload as owned_facts.
    let lease = s
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    let value = Timestamp::parse_with_work(check::text(value, key, s)?, s.semantic_work())
        .map_err(core_error)?;
    Ok(OwnedTime {
        value,
        _lease: lease,
    })
}
fn before(a: &Timestamp, b: &Timestamp, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.micros().into(), &b.micros().into()])
        .map_err(core_error)?;
    require(a.micros() <= b.micros(), "ASSENT_TERMS_MISMATCH")
}
fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn scope(v: &Value, key: &OperationKey, s: &mut OriginalAuditSession) -> Result<()> {
    let values = v.as_array().ok_or(ServiceError::IntegrityFailure)?;
    super::size(values.len(), 2, 2, s)?;
    for (a, b) in values.iter().zip(&key.scope) {
        check::same_text(a.as_str().ok_or(ServiceError::IntegrityFailure)?, b, s)?;
    }
    Ok(())
}
fn id(v: &Value, key: &str, s: &mut OriginalAuditSession) -> Result<()> {
    let value = check::text(v, key, s)?;
    super::size(value.len(), 1, 128, s)?;
    s.semantic_work()
        .scan_bytes(value.len())
        .map_err(core_error)?;
    for c in value.chars() {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(!c.is_control(), "INVALID_SHAPE")?;
    }
    Ok(())
}
fn evidence<'a>(
    record: &'a SourceRecord,
    terms: &OwnedAcceptanceTerms,
    purpose: &str,
    acceptor: &str,
    actor: &str,
    at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let body = record.body();
    check::closed(
        body,
        &[
            "schema",
            "purpose",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
            "retained_by",
            "retained_at",
            "installation_mode",
            "authority_ref",
        ],
        s,
    )?;
    for (k, expected) in [
        ("schema", "ledger-workflow-retained-evidence/1"),
        ("purpose", purpose),
        ("retained_by", actor),
        ("installation_mode", "real_local"),
    ] {
        check::same_text(check::text(body, k, s)?, expected, s)?;
    }
    let subject = check::field(body, "subject", s)?;
    check::closed(subject, &["customer", "agreement_id"], s)?;
    for name in ["customer", "agreement_id"] {
        check::same_text(
            check::text(subject, name, s)?,
            check::text(terms.canonical_value(), name, s)?,
            s,
        )?;
    }
    let retained = time(body, "retained_at", s)?;
    before(&retained, at, s)?;
    let encoded = check::text(body, "content_base64", s)?;
    super::size(encoded.len(), 1, 5464, s)?;
    let raw = s
        .decode_base64_representation(encoded, Kind::EvidenceContent)
        .map_err(core_error)?;
    super::size(raw.bytes().len(), 1, 4096, s)?;
    let hash = s
        .hash_representation(raw.bytes(), Kind::EvidenceContent)
        .map_err(core_error)?;
    check::digest_text(&hash, check::text(body, "content_sha256", s)?, false, s)?;
    let media = check::text(body, "media_type", s)?;
    if equal(media, "text/plain;charset=utf-8", s)? {
        s.semantic_work()
            .scan_bytes(raw.bytes().len())
            .map_err(core_error)?;
        std::str::from_utf8(raw.bytes()).map_err(|_| reject("INVALID_SHAPE"))?;
    } else {
        check::same_text(media, "application/octet-stream", s)?;
    }
    let metadata = check::field(body, "metadata", s)?;
    for (k, expected) in [
        ("terms_digest", terms.terms_digest()),
        ("acceptor", acceptor),
    ] {
        check::same_text(check::text(metadata, k, s)?, expected, s)?;
    }
    super::validate_reference(
        check::field(body, "authority_ref", s)?,
        "workflow-authority",
        s,
    )?;
    Ok(metadata)
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn observe(
    tx: &mut SqliteTx,
    key: &OperationKey,
    terms: &OwnedAcceptanceTerms,
    enrollment: &SourceRecord,
    registration: SourceRecord,
    assent: &Value,
    actor: &str,
    at: &Timestamp,
    cut: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedCustomerConsent> {
    let result = observe_inner(
        tx,
        key,
        terms,
        enrollment,
        registration,
        assent,
        actor,
        at,
        cut,
        s,
    )
    .await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
#[allow(clippy::too_many_arguments)]
async fn observe_inner(
    tx: &mut SqliteTx,
    key: &OperationKey,
    terms: &OwnedAcceptanceTerms,
    enrollment: &SourceRecord,
    registration: SourceRecord,
    assent: &Value,
    actor: &str,
    at: &Timestamp,
    cut: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedCustomerConsent> {
    check::closed(
        assent,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "terms_digest",
            "mode",
            "acceptor",
            "accepted_at",
            "evidence_ref",
            "operator",
            "operator_attestation_ref",
        ],
        s,
    )?;
    for (k, expected) in [
        ("schema", "ledger-workflow-assent/1"),
        ("mode", "real"),
        ("operator", actor),
        ("terms_digest", terms.terms_digest()),
    ] {
        check::same_text(check::text(assent, k, s)?, expected, s)?;
    }
    for name in ["customer", "agreement_id"] {
        check::same_text(
            check::text(assent, name, s)?,
            check::text(terms.canonical_value(), name, s)?,
            s,
        )?;
    }
    scope(check::field(assent, "scope", s)?, key, s)?;
    id(assent, "acceptor", s)?;
    let accepted = time(assent, "accepted_at", s)?;
    before(&accepted, at, s)?;
    check::same_text(
        &enrollment.reference().kind,
        "workflow-source-enrollment",
        s,
    )?;
    check::same_text(
        check::text(enrollment.body(), "schema", s)?,
        "ledger-workflow-source-enrollment/1",
        s,
    )?;
    scope(check::field(enrollment.body(), "scope", s)?, key, s)?;
    let customers = check::field(enrollment.body(), "customer_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    super::size(customers.len(), 1, 64, s)?;
    let expected = wire::reference(registration.reference(), s)?;
    let mut found = false;
    for customer in customers {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let candidate = super::encode(customer, s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(candidate.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        if candidate.bytes() == expected.bytes() {
            found = true;
            break;
        }
    }
    require(found, "AUTHORITY_REQUIRED")?;
    let body = registration.body();
    check::closed(
        body,
        &[
            "schema",
            "scope",
            "customer",
            "acceptor",
            "evidence_ref",
            "registered_by",
            "registered_at",
            "authority_ref",
            "installation_mode",
        ],
        s,
    )?;
    scope(check::field(body, "scope", s)?, key, s)?;
    for (name, expected) in [
        ("schema", "ledger-workflow-customer-registration/1"),
        ("installation_mode", "real_local"),
    ] {
        check::same_text(check::text(body, name, s)?, expected, s)?;
    }
    for name in ["customer", "acceptor"] {
        check::same_text(
            check::text(body, name, s)?,
            check::text(assent, name, s)?,
            s,
        )?;
    }
    for name in ["customer", "acceptor", "registered_by"] {
        id(body, name, s)?;
    }
    let _registered = time(body, "registered_at", s)?;
    super::validate_reference(
        check::field(body, "evidence_ref", s)?,
        "workflow-evidence",
        s,
    )?;
    super::validate_reference(
        check::field(body, "authority_ref", s)?,
        "workflow-authority",
        s,
    )?;
    let consent = source::retained_record(
        tx,
        key,
        check::field(assent, "evidence_ref", s)?,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1",
        cut,
        s,
    )
    .await?;
    let attestation = source::retained_record(
        tx,
        key,
        check::field(assent, "operator_attestation_ref", s)?,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1",
        cut,
        s,
    )
    .await?;
    let acceptor = check::text(assent, "acceptor", s)?;
    let material = evidence(&consent, terms, "assent", acceptor, actor, at, s)?;
    check::closed(material, &["terms_digest", "acceptor", "accepted_at"], s)?;
    check::same_text(
        check::text(material, "accepted_at", s)?,
        accepted.as_str(),
        s,
    )?;
    let metadata = evidence(
        &attestation,
        terms,
        "operator_attestation",
        acceptor,
        actor,
        at,
        s,
    )?;
    check::closed(
        metadata,
        &[
            "terms_digest",
            "acceptor",
            "assent_evidence_ref",
            "statement",
        ],
        s,
    )?;
    check::same_text(
        check::text(metadata, "statement", s)?,
        "operator_verified_exact_terms_acceptance",
        s,
    )?;
    let prior = super::encode(check::field(metadata, "assent_evidence_ref", s)?, s)?;
    let actual = wire::reference(consent.reference(), s)?;
    super::same_bytes(prior.bytes(), actual.bytes(), s)?;
    const FIELDS: [&str; 11] = [
        "accepted_at",
        "acceptor",
        "agreement_id",
        "customer",
        "evidence_ref",
        "mode",
        "operator",
        "operator_attestation_ref",
        "schema",
        "scope",
        "terms_digest",
    ];
    let mut parts: [Option<AuditedBytes>; 11] = std::array::from_fn(|_| None);
    let mut fields = [("", &[] as &[u8]); 11];
    for (i, name) in FIELDS.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        parts[i] = Some(if equal(name, "accepted_at", s)? {
            wire::string(accepted.as_str(), s)?
        } else {
            super::encode(check::field(assent, name, s)?, s)?
        });
    }
    for (i, name) in FIELDS.iter().enumerate() {
        s.semantic_work()
            .write_bytes(mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[i] = (
            name,
            parts[i]
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .bytes(),
        );
    }
    let canonical = wire::object(&fields, 262144, s)?;
    let assent = wire::source_record("workflow-assent", "ledger-workflow-assent/1", canonical, s)?;
    Ok(ObservedCustomerConsent {
        registration,
        evidence: consent,
        attestation,
        assent,
    })
}
