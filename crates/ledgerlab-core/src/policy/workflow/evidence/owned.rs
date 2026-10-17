//! Owning entry to the SAME R1 evidence metadata normalizer and strict codec.
//! Declared evidence is DATA; original custody, authorizations and purpose-
//! specific dependency semantics remain the chronological host's obligation.
use super::super::terms::owned::{self as input, Encode, Storage};
use super::*;
use crate::{
    canonical::audit_arena::{AuditArena, AuditedBytes, AuditedValue},
    validation_work::{OriginalAuditSession, RepresentationKind as Kind},
};

pub struct OwnedEvidenceInput {
    body: AuditedValue,
    bytes: AuditedBytes,
    content: AuditedBytes,
    _storage: Storage,
}
fn optional<'a>(v: &'a Value, key: &str, w: &mut dyn ValidationWork) -> Result<Option<&'a Value>> {
    w.visit(Visit::Constraint)?;
    for (name, value) in v.as_object().ok_or_else(|| fail("object"))? {
        w.visit(Visit::RefLookup)?;
        if same_text(name, key, w)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn string(
    v: &Value,
    name: &str,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<String> {
    input::string(v, name, s, a, w)
}
fn reference(
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Reference> {
    input::closed(v, &["kind", "id", "digest"], &[], w)?;
    Ok(Reference {
        kind: string(v, "kind", s, a, w)?,
        id: string(v, "id", s, a, w)?,
        digest: string(v, "digest", s, a, w)?,
    })
}
fn metadata(
    purpose: &str,
    v: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Metadata> {
    if same_text(purpose, "customer_registration", w)? {
        input::closed(v, &["acceptor"], &[], w)?;
        Ok(Metadata::Registration(Registration {
            acceptor: string(v, "acceptor", s, a, w)?,
        }))
    } else if same_text(purpose, "source_enrollment", w)? {
        input::closed(v, &["source", "principal_id"], &[], w)?;
        Ok(Metadata::Enrollment(Enrollment {
            source: string(v, "source", s, a, w)?,
            principal_id: string(v, "principal_id", s, a, w)?,
        }))
    } else if same_text(purpose, "assent", w)? {
        input::closed(v, &["terms_digest", "acceptor", "accepted_at"], &[], w)?;
        s.reserve(4 * 64 + 27, a, w)?;
        let accepted_at =
            Timestamp::parse_with_work(input::text(input::field(v, "accepted_at", w)?, w)?, w)?;
        Ok(Metadata::Assent(Assent {
            terms_digest: string(v, "terms_digest", s, a, w)?,
            acceptor: string(v, "acceptor", s, a, w)?,
            accepted_at,
        }))
    } else if same_text(purpose, "operator_attestation", w)? {
        input::closed(
            v,
            &[
                "terms_digest",
                "acceptor",
                "assent_evidence_ref",
                "statement",
            ],
            &[],
            w,
        )?;
        Ok(Metadata::Attestation(Attestation {
            terms_digest: string(v, "terms_digest", s, a, w)?,
            acceptor: string(v, "acceptor", s, a, w)?,
            assent_evidence_ref: reference(input::field(v, "assent_evidence_ref", w)?, s, a, w)?,
            statement: string(v, "statement", s, a, w)?,
        }))
    } else if same_text(purpose, "funding", w)? {
        input::closed(v, &["resource_owner", "payer", "mode"], &[], w)?;
        Ok(Metadata::Funding(Funding {
            resource_owner: string(v, "resource_owner", s, a, w)?,
            payer: string(v, "payer", s, a, w)?,
            mode: string(v, "mode", s, a, w)?,
        }))
    } else if same_text(purpose, "admission_stop", w)? {
        input::closed(
            v,
            &["binding_ref", "target_transition_ref", "reason"],
            &[],
            w,
        )?;
        Ok(Metadata::Stop(Stop {
            binding_ref: reference(input::field(v, "binding_ref", w)?, s, a, w)?,
            target_transition_ref: reference(
                input::field(v, "target_transition_ref", w)?,
                s,
                a,
                w,
            )?,
            reason: string(v, "reason", s, a, w)?,
        }))
    } else {
        Err(fail("unsupported purpose"))
    }
}
fn encode_ref(
    v: &Reference,
    e: &mut Encode<'_>,
    w: &mut dyn ValidationWork,
) -> Result<AuditedBytes> {
    let digest = e.text(&v.digest, w)?;
    let id = e.text(&v.id, w)?;
    let kind = e.text(&v.kind, w)?;
    e.object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        w,
    )
}
fn encode_metadata(
    m: &Metadata,
    e: &mut Encode<'_>,
    w: &mut dyn ValidationWork,
) -> Result<AuditedBytes> {
    w.visit(Visit::Constraint)?;
    match m {
        Metadata::Registration(v) => {
            let acceptor = e.text(&v.acceptor, w)?;
            e.object(&[("acceptor", acceptor.bytes())], w)
        }
        Metadata::Enrollment(v) => {
            let principal = e.text(&v.principal_id, w)?;
            let source = e.text(&v.source, w)?;
            e.object(
                &[
                    ("principal_id", principal.bytes()),
                    ("source", source.bytes()),
                ],
                w,
            )
        }
        Metadata::Assent(v) => {
            let at = e.text(v.accepted_at.as_str(), w)?;
            let acceptor = e.text(&v.acceptor, w)?;
            let terms = e.text(&v.terms_digest, w)?;
            e.object(
                &[
                    ("accepted_at", at.bytes()),
                    ("acceptor", acceptor.bytes()),
                    ("terms_digest", terms.bytes()),
                ],
                w,
            )
        }
        Metadata::Attestation(v) => {
            let acceptor = e.text(&v.acceptor, w)?;
            let evidence = encode_ref(&v.assent_evidence_ref, e, w)?;
            let statement = e.text(&v.statement, w)?;
            let terms = e.text(&v.terms_digest, w)?;
            e.object(
                &[
                    ("acceptor", acceptor.bytes()),
                    ("assent_evidence_ref", evidence.bytes()),
                    ("statement", statement.bytes()),
                    ("terms_digest", terms.bytes()),
                ],
                w,
            )
        }
        Metadata::Funding(v) => {
            let mode = e.text(&v.mode, w)?;
            let payer = e.text(&v.payer, w)?;
            let owner = e.text(&v.resource_owner, w)?;
            e.object(
                &[
                    ("mode", mode.bytes()),
                    ("payer", payer.bytes()),
                    ("resource_owner", owner.bytes()),
                ],
                w,
            )
        }
        Metadata::Stop(v) => {
            let binding = encode_ref(&v.binding_ref, e, w)?;
            let reason = e.text(&v.reason, w)?;
            let target = encode_ref(&v.target_transition_ref, e, w)?;
            e.object(
                &[
                    ("binding_ref", binding.bytes()),
                    ("reason", reason.bytes()),
                    ("target_transition_ref", target.bytes()),
                ],
                w,
            )
        }
    }
}
impl OwnedEvidenceInput {
    pub fn normalize(value: &Value, session: &mut OriginalAuditSession) -> Result<Self> {
        let result = Self::inner(value, session);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    fn inner(value: &Value, session: &mut OriginalAuditSession) -> Result<Self> {
        let arena = session.arena().shared_handle();
        let mut storage = Storage::new();
        let w = session.meter_mut();
        super::super::facts::reject_null_with_work(value, w)?;
        input::closed(
            value,
            &[
                "schema",
                "purpose",
                "subject",
                "media_type",
                "content_base64",
                "content_sha256",
                "metadata",
            ],
            &[],
            w,
        )?;
        if !same_text(
            input::text(input::field(value, "schema", w)?, w)?,
            "ledger-workflow-evidence-retain/1",
            w,
        )? {
            return Err(fail("schema"));
        }
        let subject = input::field(value, "subject", w)?;
        input::closed(subject, &["customer"], &["agreement_id"], w)?;
        let customer = string(subject, "customer", &mut storage, &arena, w)?;
        id_with_work(&customer, 128, w)?;
        let agreement = optional(subject, "agreement_id", w)?
            .map(|v| -> Result<String> {
                let v = input::text(v, w)?;
                id_with_work(v, 128, w)?;
                storage.text(v, &arena, w)
            })
            .transpose()?;
        let media = string(value, "media_type", &mut storage, &arena, w)?;
        let text = same_text(&media, "text/plain;charset=utf-8", w)?;
        if !text && !same_text(&media, "application/octet-stream", w)? {
            return Err(fail("media type"));
        }
        let encoded = string(value, "content_base64", &mut storage, &arena, w)?;
        // Any canonical base64 of the existing decoded 4096-byte domain has
        // at most 5464 bytes. This is a pre-allocation consequence, not a cap
        // change; the SAME decoder still checks all lexical/padding rules.
        if work::compare(w, encoded.len(), 5464usize)?.is_gt() {
            return Err(fail("content limit"));
        }
        let content = session.decode_base64_representation(&encoded, Kind::EvidenceContent)?;
        let w = session.meter_mut();
        if work::compare(w, content.bytes().len(), 0usize)?.is_eq() {
            return Err(fail("empty content"));
        }
        if work::compare(w, content.bytes().len(), 4096usize)?.is_gt() {
            return Err(fail("content limit"));
        }
        if text {
            w.scan_bytes(content.bytes().len())?;
            std::str::from_utf8(content.bytes()).map_err(|_| fail("text encoding"))?;
        }
        let expected_hash = string(value, "content_sha256", &mut storage, &arena, w)?;
        if !raw_digest_with_work(&expected_hash, w)? {
            return Err(fail("content digest"));
        }
        let hash = session.hash_representation(content.bytes(), Kind::EvidenceContent)?;
        let w = session.meter_mut();
        // The SAME hex emitter's Vec capacities are 8,16,32,64; old+new
        // requested overlap peaks at 96 bytes, with no format scratch.
        storage.reserve(96, &arena, w)?;
        w.scan_bytes(64)?; // String::from_utf8's final ASCII validation pass.
        let actual_hash = canonical::hex_with_work(&hash, w)?;
        if !same_text(&actual_hash, &expected_hash, w)? {
            return Err(fail("content digest"));
        }
        let purpose = string(value, "purpose", &mut storage, &arena, w)?;
        let metadata = metadata(
            &purpose,
            input::field(value, "metadata", w)?,
            &mut storage,
            &arena,
            w,
        )?;
        normalize_metadata(&metadata, agreement.is_some(), w)?;
        let mut e = Encode {
            storage: &mut storage,
            arena: &arena,
        };
        let customer = e.text(&customer, w)?;
        let subject = if let Some(agreement) = &agreement {
            let agreement = e.text(agreement, w)?;
            e.object(
                &[
                    ("agreement_id", agreement.bytes()),
                    ("customer", customer.bytes()),
                ],
                w,
            )?
        } else {
            e.object(&[("customer", customer.bytes())], w)?
        };
        let metadata = encode_metadata(&metadata, &mut e, w)?;
        let media = e.text(&media, w)?;
        let encoded = e.text(&encoded, w)?;
        let hash = e.text(&expected_hash, w)?;
        let purpose = e.text(&purpose, w)?;
        let schema = e.text("ledger-workflow-evidence-retain/1", w)?;
        let bytes = e.object(
            &[
                ("content_base64", encoded.bytes()),
                ("content_sha256", hash.bytes()),
                ("media_type", media.bytes()),
                ("metadata", metadata.bytes()),
                ("purpose", purpose.bytes()),
                ("schema", schema.bytes()),
                ("subject", subject.bytes()),
            ],
            w,
        )?;
        let body = session.parse_representation(bytes.bytes(), Kind::SourceDefinition)?;
        Ok(Self {
            body,
            bytes,
            content,
            _storage: storage,
        })
    }
    pub fn canonical_value(&self) -> &Value {
        self.body.value()
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }
    pub fn content(&self) -> &[u8] {
        self.content.bytes()
    }
}
