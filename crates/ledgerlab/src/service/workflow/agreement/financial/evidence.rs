//! Literal request evidence for the actual closed party-management producer.
//! Descriptor anchors derive from normalized original operands, never saved
//! resolution/expected results. The output is DATA, with no commit authority.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as Kind,
        ValidationVisit as Visit,
    },
};
const REGISTRY: &str = "sha256:42368b84a652527c58389014fefa69140d6a40c2217e193694f162441699cdc2";
pub(super) struct RequestEvidence {
    parsed: AuditedValue,
    bytes: AuditedBytes,
}
impl RequestEvidence {
    pub(super) fn value(&self) -> &Value {
        self.parsed.value()
    }
    pub(super) fn bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }
}
struct Entry {
    locator: AuditedBytes,
    anchor: AuditedBytes,
}
struct Anchors {
    entries: [Option<Entry>; 4],
    len: usize,
}
fn cmp(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<std::cmp::Ordering> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a.cmp(b))
}
fn encode(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.canonical_representation(v, Kind::SourceDefinition)
        .map_err(core_error)
}
fn join(parts: &[&[u8]], s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let mut size = 2usize;
    for (i, p) in parts.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Add, &[&size.into(), &p.len().into()])
            .map_err(core_error)?;
        size = size.checked_add(p.len()).ok_or(ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])
            .map_err(core_error)?;
        if i != 0 {
            s.semantic_work()
                .arithmetic(Op::Add, &[&size.into(), &1usize.into()])
                .map_err(core_error)?;
            size = size.checked_add(1).ok_or(ServiceError::Unavailable)?;
        }
    }
    let mut b = {
        let (a, w) = s.parts();
        a.byte_builder(size, w).map_err(core_error)?
    };
    b.append(b"[", s.semantic_work()).map_err(core_error)?;
    for (i, p) in parts.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])
            .map_err(core_error)?;
        if i != 0 {
            b.append(b",", s.semantic_work()).map_err(core_error)?;
        }
        b.append(p, s.semantic_work()).map_err(core_error)?;
    }
    b.append(b"]", s.semantic_work()).map_err(core_error)?;
    b.finish(s.semantic_work()).map_err(core_error)
}
impl Anchors {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            len: 0,
        }
    }
    fn add(
        &mut self,
        role: &str,
        relation: &str,
        value: &Value,
        kind: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let reference = current::source::from_value(value, kind, s)?;
        let reference = wire::reference(&reference.0, s)?;
        let role = wire::string(role, s)?;
        let relation = wire::string(relation, s)?;
        let locator = wire::object(
            &[("ref", reference.bytes()), ("role", role.bytes())],
            262144,
            s,
        )?;
        let anchor = wire::object(
            &[
                ("descriptor_refs", b"[]"),
                ("record_ref", reference.bytes()),
                ("relation", relation.bytes()),
                ("role", role.bytes()),
            ],
            262144,
            s,
        )?;
        let mut position = self.len;
        for (i, prior) in self.entries[..self.len].iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            match cmp(
                locator.bytes(),
                prior
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .locator
                    .bytes(),
                s,
            )? {
                std::cmp::Ordering::Equal => return Ok(()),
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                std::cmp::Ordering::Greater => {}
            }
        }
        s.semantic_work()
            .arithmetic(Op::Compare, &[&self.len.into(), &4usize.into()])
            .map_err(core_error)?;
        require(self.len < 4, "REFERENCE_LIMIT")?;
        let mut i = self.len;
        while {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&i.into(), &position.into()])
                .map_err(core_error)?;
            i > position
        } {
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&i.into(), &1usize.into()])
                .map_err(core_error)?;
            let prior = i - 1;
            s.semantic_work()
                .visit(Visit::SortMove)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(std::mem::size_of::<Option<Entry>>())
                .map_err(core_error)?;
            self.entries[i] = self.entries[prior].take();
            i = prior;
        }
        s.semantic_work()
            .write_bytes(std::mem::size_of::<Option<Entry>>())
            .map_err(core_error)?;
        self.entries[position] = Some(Entry { locator, anchor });
        s.semantic_work()
            .arithmetic(Op::Add, &[&self.len.into(), &1usize.into()])
            .map_err(core_error)?;
        self.len += 1;
        Ok(())
    }
    fn arrays(&self, s: &mut OriginalAuditSession) -> Result<(AuditedBytes, AuditedBytes)> {
        let mut locators: [&[u8]; 4] = [b""; 4];
        let mut anchors: [&[u8]; 4] = [b""; 4];
        for (i, item) in self.entries[..self.len].iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let item = item.as_ref().ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .write_bytes(2 * std::mem::size_of::<&[u8]>())
                .map_err(core_error)?;
            locators[i] = item.locator.bytes();
            anchors[i] = item.anchor.bytes();
        }
        let mut i = 1usize;
        while {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&i.into(), &self.len.into()])
                .map_err(core_error)?;
            i < self.len
        } {
            let mut j = i;
            while {
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&j.into(), &0usize.into()])
                    .map_err(core_error)?;
                j > 0
            } {
                s.semantic_work()
                    .arithmetic(Op::Subtract, &[&j.into(), &1usize.into()])
                    .map_err(core_error)?;
                let prior = j - 1;
                if !cmp(anchors[prior], anchors[j], s)?.is_gt() {
                    break;
                }
                s.semantic_work()
                    .visit(Visit::SortMove)
                    .map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(2 * std::mem::size_of::<&[u8]>())
                    .map_err(core_error)?;
                anchors.swap(prior, j);
                j = prior;
            }
            s.semantic_work()
                .arithmetic(Op::Add, &[&i.into(), &1usize.into()])
                .map_err(core_error)?;
            i += 1;
        }
        Ok((
            join(&locators[..self.len], s)?,
            join(&anchors[..self.len], s)?,
        ))
    }
}
fn party_anchors(
    req: &ingress::IncomingRequest,
    p: &Value,
    registry: &str,
    s: &mut OriginalAuditSession,
) -> Result<Anchors> {
    let mut out = Anchors::new();
    let schema = request_wrapper::text(p, "schema", s)?;
    // The code-owned registered recipe is checked, not inferred from the kind.
    let recipe = wrappers::recipe(req.kind(), schema, registry, s)?;
    request_wrapper::same_text(
        recipe.authority,
        "ledger-workflow-party-management-authority/1",
        s,
    )?;
    request_wrapper::same_text(
        wrappers::descriptor_schema(req.kind(), schema, registry, s)?,
        "ledger-operation-descriptors/3",
        s,
    )?;
    if cmp(schema.as_bytes(), b"ledger-workflow-party-authorize/1", s)?.is_eq() {
        for field in ["evidence_ref", "operator_attestation_ref"] {
            out.add(
                "evidence",
                "evidence_input",
                request_wrapper::field(p, field, s)?,
                "workflow-evidence",
                s,
            )?;
        }
    } else if cmp(schema.as_bytes(), b"ledger-workflow-party-revoke/1", s)?.is_eq() {
        out.add(
            "authority",
            "direct",
            request_wrapper::field(p, "authorization_ref", s)?,
            "workflow-authority",
            s,
        )?;
    } else {
        request_wrapper::same_text(schema, "ledger-workflow-party-evidence-retain/1", s)?;
        let m = request_wrapper::field(p, "metadata", s)?;
        let purpose = request_wrapper::text(m, "purpose", s)?;
        if cmp(purpose.as_bytes(), b"party_operator_attestation", s)?.is_eq() {
            out.add(
                "evidence",
                "evidence_input",
                request_wrapper::field(m, "target_evidence_ref", s)?,
                "workflow-evidence",
                s,
            )?;
        } else if cmp(purpose.as_bytes(), b"party_consent", s)?.is_eq() {
            let consent = request_wrapper::field(m, "consent", s)?;
            out.add(
                "authority",
                "direct",
                request_wrapper::field(consent, "authorization_ref", s)?,
                "workflow-authority",
                s,
            )?;
            let basis = request_wrapper::field(consent, "basis", s)?;
            if cmp(
                request_wrapper::text(basis, "kind", s)?.as_bytes(),
                b"acceptance_terms",
                s,
            )?
            .is_eq()
            {
                out.add(
                    "private_document",
                    "agreement_input",
                    request_wrapper::field(basis, "terms_ref", s)?,
                    "workflow-acceptance-terms",
                    s,
                )?;
            } else {
                request_wrapper::same_text(
                    request_wrapper::text(basis, "kind", s)?,
                    "supplier_commitment",
                    s,
                )?;
                for (field, kind) in [
                    ("commitment_ref", "workflow-offer"),
                    ("offer_ref", "workflow-offer"),
                    ("terms_ref", "workflow-slot-terms"),
                ] {
                    out.add(
                        "private_document",
                        "agreement_input",
                        request_wrapper::field(basis, field, s)?,
                        kind,
                        s,
                    )?;
                }
            }
        } else {
            request_wrapper::same_text(purpose, "party_authorization", s)?;
        }
    }
    Ok(out)
}
pub(super) fn party(
    req: &ingress::IncomingRequest,
    pending: &current::party::produce::PendingPartyOperation,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    party_with_registry(req, pending, REGISTRY, s)
}
fn party_with_registry(
    req: &ingress::IncomingRequest,
    pending: &current::party::produce::PendingPartyOperation,
    registry: &str,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let payload = s
            .parse_representation(pending.normalized_payload(), Kind::OriginalRequest)
            .map_err(core_error)?;
        let set = party_anchors(req, payload.value(), registry, s)?;
        let (locators, anchors) = set.arrays(s)?;
        let kind = wire::string(req.kind(), s)?;
        let schema = encode(request_wrapper::field(payload.value(), "schema", s)?, s)?;
        let locator_digest = wire::domain_digest(
            b"bean-counter/descriptor-locators/1\0",
            &[
                b"[",
                kind.bytes(),
                b",",
                schema.bytes(),
                b",",
                locators.bytes(),
                b"]",
            ],
            s,
        )?;
        let registry = wire::string(registry, s)?;
        let base = [
            ("anchors", anchors.bytes()),
            ("descriptors", b"[]".as_slice()),
            ("locator_digest", locator_digest.bytes()),
            (
                "normalization_profile",
                br#""ledger-operation-normalization/2""#,
            ),
            ("operation_kind", kind.bytes()),
            ("payload_schema", schema.bytes()),
            ("registry_digest", registry.bytes()),
            ("schema", br#""ledger-operation-descriptors/3""#),
        ];
        let resolution = wire::object(&base, 262144, s)?;
        let closure = wire::domain_digest(
            b"bean-counter/descriptor-closure/1\0",
            &[resolution.bytes()],
            s,
        )?;
        let resolution = wire::object(
            &[
                ("anchors", anchors.bytes()),
                ("closure_digest", closure.bytes()),
                ("descriptors", b"[]"),
                ("locator_digest", locator_digest.bytes()),
                (
                    "normalization_profile",
                    br#""ledger-operation-normalization/2""#,
                ),
                ("operation_kind", kind.bytes()),
                ("payload_schema", schema.bytes()),
                ("registry_digest", registry.bytes()),
                ("schema", br#""ledger-operation-descriptors/3""#),
            ],
            262144,
            s,
        )?;
        let key = encode(request_wrapper::field(req.value(), "key", s)?, s)?;
        let normalized = wire::object(
            &[
                ("contract", br#""workflow/1""#),
                ("key", key.bytes()),
                ("kind", kind.bytes()),
                ("payload", pending.normalized_payload()),
                ("schema", br#""ledger-operation/1""#),
            ],
            262144,
            s,
        )?;
        let request_hash = wire::domain_digest(
            b"bean-counter/operation-request/2\0",
            &[b"[", closure.bytes(), b",", normalized.bytes(), b"]"],
            s,
        )?;
        let bundle = wire::domain_digest(b"bean-counter/schema-bundle/1\0", &[b"[]"], s)?;
        let raw = wire::raw_request_base64(req.raw(), s)?;
        let raw_hash = wire::raw_request_digest(req.raw(), s)?;
        let bytes = wire::object(
            &[
                ("descriptor_resolution", resolution.bytes()),
                (
                    "normalization_profile",
                    br#""ledger-operation-normalization/2""#,
                ),
                ("normalized_request", normalized.bytes()),
                ("original_request_bytes_base64", raw.bytes()),
                ("original_request_bytes_sha256", raw_hash.bytes()),
                ("request_hash", request_hash.bytes()),
                ("schema", br#""ledger-operation-request-evidence/2""#),
                ("schema_bundle_digest", bundle.bytes()),
            ],
            4194304,
            s,
        )?;
        let parsed = s
            .parse_representation(bytes.bytes(), Kind::Record)
            .map_err(core_error)?;
        Ok(RequestEvidence { parsed, bytes })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

/// SAME original party evidence derivation from literal original raw and saved
/// exact code-owned tuple, never the current registry or stored anchor answers.
/// Equality completes this local DATA relation only; historical custody/heads
/// and complete chronology still precede any receipt return.
pub(super) fn original_party(
    origin: &prefix::LocatedWrappers,
    pending: &current::party::produce::PendingPartyOperation,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let saved = request_wrapper::field(origin.wrappers().operation(), "request_evidence", s)?;
        let resolution = request_wrapper::field(saved, "descriptor_resolution", s)?;
        let registry = request_wrapper::text(resolution, "registry_digest", s)?;
        let req = ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), s)?;
        request_wrapper::key(
            request_wrapper::field(req.value(), "key", s)?,
            origin.key(),
            s,
        )?;
        let reproduced = party_with_registry(&req, pending, registry, s)?;
        let saved = s
            .canonical_representation(saved, Kind::Record)
            .map_err(core_error)?;
        require(
            cmp(saved.bytes(), reproduced.bytes(), s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        Ok(reproduced)
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
