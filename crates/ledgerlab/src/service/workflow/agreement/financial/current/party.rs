//! Exact retained party mapping/evidence joins and locked current selection.
//! The returned owning DATA still requires each actual earliest origin to
//! complete in the chronological auditor; no stored verification is authority.
use super::super::request_wrapper as check;
use super::*;
use crate::store::sqlite::workflow::financial::{HeadLocator, PartyHeadKey, RetainedDocument};
use sha2::{Digest, Sha256};
struct PartyRecord {
    parsed: AuditedValue,
    stored: RetainedDocument,
    reference: ExactRef,
    _lease: ExternalBufferLease,
}
impl PartyRecord {
    fn body(&self) -> &Value {
        self.parsed.value()
    }
}
pub(in super::super) struct ObservedPartyAuthorization {
    original: PartyRecord,
    evidence: PartyRecord,
    attestation: PartyRecord,
    current: HeadLocator,
}
impl ObservedPartyAuthorization {
    pub(in super::super) fn reference(&self) -> &ExactRef {
        &self.original.reference
    }
    pub(in super::super) fn body(&self) -> &Value {
        self.original.body()
    }
    pub(in super::super) fn current_head(&self) -> &HeadLocator {
        &self.current
    }
    pub(in super::super) fn evidence_refs(&self) -> [&ExactRef; 2] {
        [&self.evidence.reference, &self.attestation.reference]
    }
    pub(in super::super) fn original_authority_refs(
        &self,
        session: &mut OriginalAuditSession,
    ) -> Result<[&Value; 3]> {
        Ok([
            check::field(self.original.body(), "authority_ref", session)?,
            check::field(self.evidence.body(), "authority_ref", session)?,
            check::field(self.attestation.body(), "authority_ref", session)?,
        ])
    }
}
fn number(a: usize, b: usize, op: Op, session: &mut OriginalAuditSession) -> Result<usize> {
    session
        .semantic_work()
        .arithmetic(op, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    match op {
        Op::Add => a.checked_add(b),
        Op::Multiply => a.checked_mul(b),
        _ => None,
    }
    .ok_or(ServiceError::IntegrityFailure)
}
fn size(n: usize, min: usize, max: usize, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
        .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &min.into()])
        .map_err(core_error)?;
    require(n >= min, "INVALID_SHAPE")?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &max.into()])
        .map_err(core_error)?;
    require(n <= max, "INVALID_SHAPE")
}
fn same(left: &[u8], right: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(left.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(right.len())
        .map_err(core_error)?;
    require(left == right, "ORIGINAL_TERMS_MISMATCH")
}
fn same_value(left: &Value, right: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    let a = session
        .canonical_representation(left, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let b = session
        .canonical_representation(right, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    same(a.bytes(), b.bytes(), session)
}
fn text<'a>(raw: &'a [u8], session: &mut OriginalAuditSession) -> Result<&'a str> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    std::str::from_utf8(raw).map_err(|_| ServiceError::IntegrityFailure)
}
fn id<'a>(
    body: &'a Value,
    key: &str,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<&'a str> {
    let value = check::text(body, key, session)?;
    size(value.len(), 1, maximum, session)?;
    session
        .semantic_work()
        .scan_bytes(value.len())
        .map_err(core_error)?;
    for character in value.chars() {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
            .map_err(core_error)?;
        require(!character.is_control(), "INVALID_SHAPE")?;
    }
    Ok(value)
}
fn scope(value: &Value, key: &OperationKey, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
        .map_err(core_error)?;
    let values = value.as_array().ok_or(ServiceError::IntegrityFailure)?;
    size(values.len(), 2, 2, session)?;
    for (a, b) in values.iter().zip(&key.scope) {
        check::same_text(
            a.as_str().ok_or(ServiceError::IntegrityFailure)?,
            b,
            session,
        )?;
    }
    Ok(())
}
fn time(body: &Value, name: &str, session: &mut OriginalAuditSession) -> Result<Timestamp> {
    Timestamp::parse_with_work(check::text(body, name, session)?, session.semantic_work())
        .map_err(core_error)
}
fn at_most(a: &Timestamp, b: &Timestamp, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&a.micros().into(), &b.micros().into()])
        .map_err(core_error)?;
    require(a.micros() <= b.micros(), "AUTHORITY_REQUIRED")
}
fn roles<'a>(spec: &'a Value, session: &mut OriginalAuditSession) -> Result<&'a [Value]> {
    let values = check::field(spec, "roles", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(values.len(), 1, 3, session)?;
    let mut prior: Option<&str> = None;
    for value in values {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        let value = value.as_str().ok_or(ServiceError::IntegrityFailure)?;
        let mut allowed = false;
        for role in ["bearer", "payer", "provider"] {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(value.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(role.len())
                .map_err(core_error)?;
            if value == role {
                allowed = true;
                break;
            }
        }
        require(allowed, "AUTHORITY_REQUIRED")?;
        if let Some(prior) = prior {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(value.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(prior.len())
                .map_err(core_error)?;
            require(prior < value, "INVALID_SHAPE")?;
        }
        prior = Some(value);
    }
    Ok(values)
}
fn authorization_spec<'a>(
    value: &'a Value,
    key: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    check::closed(
        value,
        &[
            "authorization_id",
            "scope",
            "customer",
            "agreement_id",
            "party",
            "acceptor",
            "roles",
            "starts_at",
            "ends_at",
        ],
        session,
    )?;
    scope(check::field(value, "scope", session)?, key, session)?;
    for key in [
        "authorization_id",
        "customer",
        "agreement_id",
        "party",
        "acceptor",
    ] {
        id(value, key, 128, session)?;
    }
    roles(value, session)?;
    let start = time(value, "starts_at", session)?;
    let end = time(value, "ends_at", session)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&start.micros().into(), &end.micros().into()])
        .map_err(core_error)?;
    require(start.micros() < end.micros(), "AUTHORITY_REQUIRED")?;
    Ok(value)
}
fn ref_parts<'a>(
    value: &'a Value,
    kind: &str,
    session: &mut OriginalAuditSession,
) -> Result<(&'a str, &'a str, &'a str)> {
    check::closed(value, &["digest", "id", "kind"], session)?;
    let k = check::text(value, "kind", session)?;
    check::same_text(k, kind, session)?;
    let i = check::text(value, "id", session)?;
    let d = check::text(value, "digest", session)?;
    size(i.len(), 69, 69, session)?;
    size(d.len(), 71, 71, session)?;
    session.semantic_work().scan_bytes(12).map_err(core_error)?;
    let ih = i
        .strip_prefix("wdoc_")
        .ok_or(ServiceError::IntegrityFailure)?;
    let dh = d
        .strip_prefix("sha256:")
        .ok_or(ServiceError::IntegrityFailure)?;
    same(ih.as_bytes(), dh.as_bytes(), session)?;
    for byte in ih.bytes() {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
            .map_err(core_error)?;
        session.semantic_work().scan_bytes(1).map_err(core_error)?;
        require(
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
            "INVALID_REFERENCE",
        )?;
    }
    Ok((k, i, d))
}
async fn record(
    tx: &mut SqliteTx,
    key: &OperationKey,
    value: &Value,
    kind: &str,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<PartyRecord> {
    let (k, i, d) = ref_parts(value, kind, session)?;
    let mut payload = std::mem::size_of::<ExactRef>();
    for text in [k, i, d] {
        payload = number(payload, text.len(), Op::Add, session)?;
    }
    let lease = session
        .reserve_external_payload(payload)
        .map_err(core_error)?;
    let mut copy = |v: &str| -> Result<String> {
        session
            .semantic_work()
            .scan_bytes(v.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(v.len())
            .map_err(core_error)?;
        Ok(v.to_owned())
    };
    let reference = ExactRef {
        kind: copy(k)?,
        id: copy(i)?,
        digest: copy(d)?,
    };
    let stored = tx
        .workflow_financial_document(
            &reference,
            &key.logical_store_id,
            &key.scope,
            before,
            session,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let parsed = session
        .parse_representation(
            stored.body.bytes(),
            if kind == "workflow-authority" {
                RepresentationKind::Authority
            } else {
                RepresentationKind::SourceDefinition
            },
        )
        .map_err(core_error)?;
    let canonical = session
        .canonical_representation(parsed.value(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    same(canonical.bytes(), stored.body.bytes(), session)?;
    let schema = check::field(parsed.value(), "schema", session)?;
    check::same_text(
        schema.as_str().ok_or(ServiceError::IntegrityFailure)?,
        text(stored.schema.bytes(), session)?,
        session,
    )?;
    let encoded_schema = session
        .canonical_representation(schema, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let mut hash = Sha256::new();
    for bytes in [
        b"bean-counter/workflow-document/1\0[".as_slice(),
        encoded_schema.bytes(),
        b",",
        stored.body.bytes(),
        b"]",
    ] {
        session
            .semantic_work()
            .scan_bytes(bytes.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .hash_bytes(bytes.len())
            .map_err(core_error)?;
        hash.update(bytes);
    }
    check::digest_text(&hash.finalize().into(), &reference.digest, true, session)?;
    Ok(PartyRecord {
        parsed,
        stored,
        reference,
        _lease: lease,
    })
}
fn evidence<'a>(
    record: &'a PartyRecord,
    key: &OperationKey,
    actor: &str,
    at: &Timestamp,
    session: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let body = record.body();
    check::closed(
        body,
        &[
            "schema",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
            "scope",
            "retained_by",
            "retained_at",
            "installation_mode",
            "authority_ref",
        ],
        session,
    )?;
    for (name, expected) in [
        ("schema", "ledger-workflow-party-evidence/1"),
        ("retained_by", actor),
        ("installation_mode", "real_local"),
    ] {
        check::same_text(check::text(body, name, session)?, expected, session)?;
    }
    scope(check::field(body, "scope", session)?, key, session)?;
    let subject = check::field(body, "subject", session)?;
    check::closed(
        subject,
        &["customer", "agreement_id", "party", "acceptor"],
        session,
    )?;
    for name in ["customer", "agreement_id", "party", "acceptor"] {
        id(subject, name, 128, session)?;
    }
    let encoded = check::text(body, "content_base64", session)?;
    size(encoded.len(), 1, 5464, session)?;
    let raw = session
        .decode_base64_representation(encoded, RepresentationKind::EvidenceContent)
        .map_err(core_error)?;
    size(raw.bytes().len(), 1, 4096, session)?;
    let hash = session
        .hash_representation(raw.bytes(), RepresentationKind::EvidenceContent)
        .map_err(core_error)?;
    check::digest_text(
        &hash,
        check::text(body, "content_sha256", session)?,
        false,
        session,
    )?;
    let media = check::text(body, "media_type", session)?;
    if media == "text/plain;charset=utf-8" {
        text(raw.bytes(), session)?;
    } else {
        check::same_text(media, "application/octet-stream", session)?;
    }
    at_most(&time(body, "retained_at", session)?, at, session)?;
    ref_parts(
        check::field(body, "authority_ref", session)?,
        "workflow-authority",
        session,
    )?;
    Ok(body)
}
/// Consumes the exact ORIGINAL authorization Ref named by retained assent and
/// reads its actual current projection under this writer. This performs real
/// evidence joins, but leaves every referenced origin for chronological proof.
pub(in super::super) async fn observe(
    tx: &mut SqliteTx,
    key: &OperationKey,
    original_ref: &Value,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<ObservedPartyAuthorization> {
    let result = observe_inner(tx, key, original_ref, at, before, session).await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
async fn observe_inner(
    tx: &mut SqliteTx,
    key: &OperationKey,
    original_ref: &Value,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<ObservedPartyAuthorization> {
    let original = record(tx, key, original_ref, "workflow-authority", before, session).await?;
    let body = original.body();
    check::closed(
        body,
        &[
            "schema",
            "authorization",
            "revision",
            "status",
            "evidence_ref",
            "operator_attestation_ref",
            "recorded_by",
            "recorded_at",
            "installation_mode",
            "authority_ref",
        ],
        session,
    )?;
    for (name, expected) in [
        ("schema", "ledger-workflow-party-authorization/1"),
        ("revision", "1"),
        ("status", "active"),
        ("installation_mode", "real_local"),
    ] {
        check::same_text(check::text(body, name, session)?, expected, session)?;
    }
    let spec = authorization_spec(check::field(body, "authorization", session)?, key, session)?;
    let actor = id(body, "recorded_by", 128, session)?;
    let recorded_at = time(body, "recorded_at", session)?;
    at_most(&recorded_at, at, session)?;
    let start = time(spec, "starts_at", session)?;
    let end = time(spec, "ends_at", session)?;
    at_most(&start, at, session)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&at.micros().into(), &end.micros().into()])
        .map_err(core_error)?;
    require(at.micros() < end.micros(), "AUTHORITY_REQUIRED")?;
    ref_parts(
        check::field(body, "authority_ref", session)?,
        "workflow-authority",
        session,
    )?;
    let retained = record(
        tx,
        key,
        check::field(body, "evidence_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let attestation = record(
        tx,
        key,
        check::field(body, "operator_attestation_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let input = evidence(&retained, key, actor, &recorded_at, session)?;
    let attested = evidence(&attestation, key, actor, &recorded_at, session)?;
    for evidence in [input, attested] {
        let subject = check::field(evidence, "subject", session)?;
        for name in ["customer", "agreement_id", "party", "acceptor"] {
            check::same_text(
                check::text(subject, name, session)?,
                check::text(spec, name, session)?,
                session,
            )?;
        }
    }
    let metadata = check::field(input, "metadata", session)?;
    check::closed(metadata, &["purpose", "authorization"], session)?;
    check::same_text(
        check::text(metadata, "purpose", session)?,
        "party_authorization",
        session,
    )?;
    same_value(
        check::field(metadata, "authorization", session)?,
        spec,
        session,
    )?;
    let statement = check::field(attested, "metadata", session)?;
    check::closed(
        statement,
        &["purpose", "target_evidence_ref", "statement"],
        session,
    )?;
    check::same_text(
        check::text(statement, "purpose", session)?,
        "party_operator_attestation",
        session,
    )?;
    check::same_text(
        check::text(statement, "statement", session)?,
        "operator_verified_party_acceptor_authority",
        session,
    )?;
    same_value(
        check::field(statement, "target_evidence_ref", session)?,
        check::field(body, "evidence_ref", session)?,
        session,
    )?;
    at_most(
        &time(input, "retained_at", session)?,
        &time(attested, "retained_at", session)?,
        session,
    )?;
    let head_key = PartyHeadKey {
        store: &key.logical_store_id,
        scope: &key.scope,
        customer: check::text(spec, "customer", session)?,
        agreement: check::text(spec, "agreement_id", session)?,
        authorization_id: check::text(spec, "authorization_id", session)?,
    };
    let current = tx
        .workflow_party_authorization_head(&head_key, session)
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    for (actual, expected) in [
        (current.revision.bytes(), "1"),
        (current.kind.bytes(), original.reference.kind.as_str()),
        (current.id.bytes(), original.reference.id.as_str()),
        (current.digest.bytes(), original.reference.digest.as_str()),
    ] {
        check::same_text(text(actual, session)?, expected, session)?;
    }
    Ok(ObservedPartyAuthorization {
        original,
        evidence: retained,
        attestation,
        current,
    })
}

/// Actual original/fullterms and locked-current party joins, still unresolved
/// with respect to earliest-origin authentication. No public verified flag.
pub(in super::super) struct ObservedPlatformConsent {
    mapping: ObservedPartyAuthorization,
    evidence: PartyRecord,
    attestation: PartyRecord,
    assent_bytes: ledgerlab_core::canonical::audit_arena::AuditedBytes,
}
impl ObservedPlatformConsent {
    pub(in super::super) fn mapping(&self) -> &ObservedPartyAuthorization {
        &self.mapping
    }
    pub(in super::super) fn assent_bytes(&self) -> &[u8] {
        self.assent_bytes.bytes()
    }
    pub(in super::super) fn evidence_refs(&self) -> [&ExactRef; 2] {
        [&self.evidence.reference, &self.attestation.reference]
    }
    pub(in super::super) fn original_authority_refs(
        &self,
        session: &mut OriginalAuditSession,
    ) -> Result<[&Value; 2]> {
        Ok([
            check::field(self.evidence.body(), "authority_ref", session)?,
            check::field(self.attestation.body(), "authority_ref", session)?,
        ])
    }
}
fn equal_text(a: &str, b: &str, session: &mut OriginalAuditSession) -> Result<bool> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(a.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(b.len())
        .map_err(core_error)?;
    Ok(a == b)
}
/// `terms` is the actual normalized AcceptanceTerms2 and the resource leg name
/// comes from the selected original app declaration. These are DATA operands,
/// never a caller assertion that platform assent/currentness succeeded.
pub(in super::super) async fn platform_consent(
    tx: &mut SqliteTx,
    key: &OperationKey,
    terms: &ledgerlab_core::policy::workflow::AcceptanceTerms,
    resource_leg: &str,
    assent: &Value,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<ObservedPlatformConsent> {
    let result =
        platform_consent_inner(tx, key, terms, resource_leg, assent, at, before, session).await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
#[allow(clippy::too_many_arguments)]
async fn platform_consent_inner(
    tx: &mut SqliteTx,
    key: &OperationKey,
    terms: &ledgerlab_core::policy::workflow::AcceptanceTerms,
    resource_leg: &str,
    assent: &Value,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<ObservedPlatformConsent> {
    let terms_body = terms.canonical_value();
    check::same_text(
        check::text(terms_body, "schema", session)?,
        "ledger-workflow-acceptance-terms/2",
        session,
    )?;
    scope(check::field(terms_body, "scope", session)?, key, session)?;
    let binding = check::field(terms_body, "binding_inputs", session)?;
    let legs = check::field(binding, "funding_legs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(legs.len(), 1, 16, session)?;
    let mut selected = None;
    for leg in legs {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::RefLookup)
            .map_err(core_error)?;
        if equal_text(check::text(leg, "name", session)?, resource_leg, session)? {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
                .map_err(core_error)?;
            require(selected.replace(leg).is_none(), "INVALID_REFERENCE")?;
        }
    }
    let leg = selected.ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    check::same_text(check::text(leg, "mode", session)?, "platform", session)?;
    let platform = id(leg, "payer", 128, session)?;
    check::same_text(id(leg, "resource_owner", 128, session)?, platform, session)?;
    // The exact app-only leg has no supplier binding. Check field absence by
    // actual candidate comparisons, without an opaque JSON map lookup.
    for (name, _) in leg.as_object().ok_or(ServiceError::IntegrityFailure)? {
        require(
            !equal_text(name, "supplier_binding_slot", session)?,
            "UNSUPPORTED_COMPOSITION",
        )?;
    }
    check::closed(
        assent,
        &[
            "schema",
            "consent",
            "mode",
            "evidence_ref",
            "operator",
            "operator_attestation_ref",
        ],
        session,
    )?;
    check::same_text(
        check::text(assent, "schema", session)?,
        "ledger-workflow-party-assent/1",
        session,
    )?;
    check::same_text(check::text(assent, "mode", session)?, "real", session)?;
    let operator = id(assent, "operator", 128, session)?;
    let consent = check::field(assent, "consent", session)?;
    check::closed(
        consent,
        &[
            "scope",
            "customer",
            "agreement_id",
            "party",
            "acceptor",
            "roles",
            "authorization_ref",
            "basis",
            "accepted_at",
        ],
        session,
    )?;
    scope(check::field(consent, "scope", session)?, key, session)?;
    for name in ["customer", "agreement_id"] {
        check::same_text(
            check::text(consent, name, session)?,
            check::text(terms_body, name, session)?,
            session,
        )?;
    }
    check::same_text(id(consent, "party", 128, session)?, platform, session)?;
    id(consent, "acceptor", 128, session)?;
    let required_roles = roles(consent, session)?;
    let (mut bearer, mut payer) = (false, false);
    for role in required_roles {
        let role = role.as_str().ok_or(ServiceError::IntegrityFailure)?;
        if equal_text(role, "bearer", session)? {
            bearer = true;
        }
        if equal_text(role, "payer", session)? {
            payer = true;
        }
    }
    require(bearer && payer, "AUTHORITY_REQUIRED")?;
    let basis = check::field(consent, "basis", session)?;
    check::closed(basis, &["kind", "terms_ref", "terms_digest"], session)?;
    check::same_text(
        check::text(basis, "kind", session)?,
        "acceptance_terms",
        session,
    )?;
    let (k, i, d) = ref_parts(
        check::field(basis, "terms_ref", session)?,
        "workflow-acceptance-terms",
        session,
    )?;
    for (a, b) in [
        (k, terms.reference().kind()),
        (i, terms.reference().id()),
        (d, terms.reference().digest()),
    ] {
        check::same_text(a, b, session)?;
    }
    check::same_text(
        check::text(basis, "terms_digest", session)?,
        terms.terms_digest(),
        session,
    )?;
    let accepted_at = time(consent, "accepted_at", session)?;
    at_most(&accepted_at, at, session)?;
    let mapping = observe(
        tx,
        key,
        check::field(consent, "authorization_ref", session)?,
        at,
        before,
        session,
    )
    .await?;
    let spec = check::field(mapping.body(), "authorization", session)?;
    for name in ["customer", "agreement_id", "party", "acceptor"] {
        check::same_text(
            check::text(consent, name, session)?,
            check::text(spec, name, session)?,
            session,
        )?;
    }
    let original_roles = roles(spec, session)?;
    for role in required_roles {
        let role = role.as_str().ok_or(ServiceError::IntegrityFailure)?;
        let mut found = false;
        for allowed in original_roles {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::RefLookup)
                .map_err(core_error)?;
            if equal_text(
                role,
                allowed.as_str().ok_or(ServiceError::IntegrityFailure)?,
                session,
            )? {
                found = true;
                break;
            }
        }
        require(found, "AUTHORITY_REQUIRED")?;
    }
    let start = time(spec, "starts_at", session)?;
    let end = time(spec, "ends_at", session)?;
    at_most(&start, &accepted_at, session)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&accepted_at.micros().into(), &end.micros().into()],
        )
        .map_err(core_error)?;
    require(accepted_at.micros() < end.micros(), "AUTHORITY_REQUIRED")?;
    let retained = record(
        tx,
        key,
        check::field(assent, "evidence_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let attestation = record(
        tx,
        key,
        check::field(assent, "operator_attestation_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let material = evidence(&retained, key, operator, at, session)?;
    let attested = evidence(&attestation, key, operator, at, session)?;
    for body in [material, attested] {
        let subject = check::field(body, "subject", session)?;
        for name in ["customer", "agreement_id", "party", "acceptor"] {
            check::same_text(
                check::text(subject, name, session)?,
                check::text(consent, name, session)?,
                session,
            )?;
        }
    }
    let metadata = check::field(material, "metadata", session)?;
    check::closed(metadata, &["purpose", "consent"], session)?;
    check::same_text(
        check::text(metadata, "purpose", session)?,
        "party_consent",
        session,
    )?;
    same_value(
        check::field(metadata, "consent", session)?,
        consent,
        session,
    )?;
    let statement = check::field(attested, "metadata", session)?;
    check::closed(
        statement,
        &["purpose", "target_evidence_ref", "statement"],
        session,
    )?;
    check::same_text(
        check::text(statement, "purpose", session)?,
        "party_operator_attestation",
        session,
    )?;
    check::same_text(
        check::text(statement, "statement", session)?,
        "operator_verified_party_exact_consent",
        session,
    )?;
    same_value(
        check::field(statement, "target_evidence_ref", session)?,
        check::field(assent, "evidence_ref", session)?,
        session,
    )?;
    let retained_at = time(material, "retained_at", session)?;
    let attested_at = time(attested, "retained_at", session)?;
    at_most(&accepted_at, &retained_at, session)?;
    at_most(&retained_at, &attested_at, session)?;
    let assent_bytes = session
        .canonical_representation(assent, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    Ok(ObservedPlatformConsent {
        mapping,
        evidence: retained,
        attestation,
        assent_bytes,
    })
}
