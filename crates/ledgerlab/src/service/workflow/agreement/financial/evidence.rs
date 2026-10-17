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
const R1_REGISTRY: &str = "sha256:e023e7bf17aeed815f30b6fdce35fc0bf68cc9efcc0612a019b0b109494ac169";
const COMMITMENT_REGISTRY: &str =
    "sha256:762b708044ffad44717074a9b3456b91ebbe62d7a12d76cfa80d6a37ecbfe276";
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
struct Anchors<const MAXIMUM: usize = 4> {
    entries: [Option<Entry>; MAXIMUM],
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
pub(super) fn join(parts: &[&[u8]], s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
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
impl<const MAXIMUM: usize> Anchors<MAXIMUM> {
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
        self.add_with_descriptors(role, relation, value, kind, b"[]", s)
    }
    fn add_with_descriptors(
        &mut self,
        role: &str,
        relation: &str,
        value: &Value,
        kind: &str,
        descriptors: &[u8],
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
                ("descriptor_refs", descriptors),
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
            .arithmetic(Op::Compare, &[&self.len.into(), &MAXIMUM.into()])
            .map_err(core_error)?;
        require(self.len < MAXIMUM, "REFERENCE_LIMIT")?;
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
        let mut locators: [&[u8]; MAXIMUM] = [b""; MAXIMUM];
        let mut anchors: [&[u8]; MAXIMUM] = [b""; MAXIMUM];
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
    let commitment = cmp(
        schema.as_bytes(),
        b"ledger-workflow-party-evidence-retain/2",
        s,
    )?
    .is_eq();
    request_wrapper::same_text(
        wrappers::descriptor_schema(req.kind(), schema, registry, s)?,
        if commitment {
            "ledger-operation-descriptors/7"
        } else {
            "ledger-operation-descriptors/3"
        },
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
        request_wrapper::same_text(
            schema,
            if commitment {
                "ledger-workflow-party-evidence-retain/2"
            } else {
                "ledger-workflow-party-evidence-retain/1"
            },
            s,
        )?;
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
            if commitment {
                request_wrapper::same_text(
                    request_wrapper::text(basis, "kind", s)?,
                    "acceptance_terms",
                    s,
                )?;
                return Ok(out);
            }
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
                    ("supplier_terms_ref", "workflow-slot-terms"),
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
    let p = s
        .parse_representation(pending.normalized_payload(), Kind::OriginalRequest)
        .map_err(core_error)?;
    let schema = request_wrapper::text(p.value(), "schema", s)?;
    let registry = if cmp(
        schema.as_bytes(),
        b"ledger-workflow-party-evidence-retain/2",
        s,
    )?
    .is_eq()
    {
        COMMITMENT_REGISTRY
    } else {
        REGISTRY
    };
    party_with_registry(req, pending, registry, s)
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
        request_evidence(
            req,
            pending.normalized_payload(),
            payload.value(),
            &set,
            registry,
            b"[]",
            262144,
            s,
        )
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
fn request_evidence<const MAXIMUM: usize>(
    req: &ingress::IncomingRequest,
    normalized_payload: &[u8],
    payload: &Value,
    set: &Anchors<MAXIMUM>,
    registry: &str,
    descriptors: &[u8],
    normalized_limit: usize,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let (locators, anchors) = set.arrays(s)?;
        let kind = wire::string(req.kind(), s)?;
        let schema = encode(request_wrapper::field(payload, "schema", s)?, s)?;
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
        let descriptor_schema = wire::string(
            wrappers::descriptor_schema(
                req.kind(),
                request_wrapper::text(payload, "schema", s)?,
                registry,
                s,
            )?,
            s,
        )?;
        let registry = wire::string(registry, s)?;
        let base = [
            ("anchors", anchors.bytes()),
            ("descriptors", descriptors),
            ("locator_digest", locator_digest.bytes()),
            (
                "normalization_profile",
                br#""ledger-operation-normalization/2""#,
            ),
            ("operation_kind", kind.bytes()),
            ("payload_schema", schema.bytes()),
            ("registry_digest", registry.bytes()),
            ("schema", descriptor_schema.bytes()),
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
                ("descriptors", descriptors),
                ("locator_digest", locator_digest.bytes()),
                (
                    "normalization_profile",
                    br#""ledger-operation-normalization/2""#,
                ),
                ("operation_kind", kind.bytes()),
                ("payload_schema", schema.bytes()),
                ("registry_digest", registry.bytes()),
                ("schema", descriptor_schema.bytes()),
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
                ("payload", normalized_payload),
                ("schema", br#""ledger-operation/1""#),
            ],
            normalized_limit,
            s,
        )?;
        let request_hash = wire::domain_digest(
            b"bean-counter/operation-request/2\0",
            &[b"[", closure.bytes(), b",", normalized.bytes(), b"]"],
            s,
        )?;
        let bundle = wire::domain_digest(b"bean-counter/schema-bundle/1\0", &[descriptors], s)?;
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

// Existing acceptance locator recipe, over the actual normalized payload and
// descriptors retained by the original publication owner. No saved answers or
// current compiler selection enters the resolution.
pub(super) fn acceptance(
    req: &ingress::IncomingRequest,
    pending: &acceptance::PendingAppAcceptance,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    acceptance_material(req, pending.normalized_payload(), &pending.publication, s)
}
pub(super) fn acceptance_material(
    req: &ingress::IncomingRequest,
    normalized_payload: &[u8],
    publication: &prefix::RecompiledPublication,
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let payload = s
            .parse_representation(normalized_payload, Kind::Record)
            .map_err(core_error)?;
        let p = payload.value();
        let binding = request_wrapper::field(p, "binding_inputs", s)?;
        let context = request_wrapper::field(
            request_wrapper::field(binding, "accepted_context", s)?,
            "fact_schema_ref",
            s,
        )?;
        let context_bytes = encode(context, s)?;
        let mut refs = [None; 128];
        let mut count = 0usize;
        s.semantic_work()
            .write_bytes(std::mem::size_of_val(&refs))
            .map_err(core_error)?;
        for reference in publication.source_refs() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let is_schema = cmp(reference.kind.as_bytes(), b"workflow-fact-schema", s)?.is_eq();
            let is_compiler = cmp(
                reference.kind.as_bytes(),
                b"workflow-compiler-descriptor",
                s,
            )?
            .is_eq();
            if is_schema || is_compiler {
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&count.into(), &refs.len().into()])
                    .map_err(core_error)?;
                require(count < refs.len(), "REFERENCE_LIMIT")?;
                s.semantic_work()
                    .write_bytes(std::mem::size_of::<
                        Option<&ledgerlab_core::policy::workflow::transition_constructor::ExactRef>,
                    >())
                    .map_err(core_error)?;
                refs[count] = Some(reference);
                s.semantic_work()
                    .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                    .map_err(core_error)?;
                count = count.checked_add(1).ok_or(ServiceError::Unavailable)?;
            }
        }
        let descriptors =
            wire::RefSet::new(refs[..count].iter().filter_map(|r| *r), count, true, s)?
                .encode(s)?;
        let mut context_found = false;
        for r in refs[..count].iter().filter_map(|r| *r) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let actual = wire::reference(r, s)?;
            if cmp(actual.bytes(), context_bytes.bytes(), s)?.is_eq() {
                context_found = true;
            }
        }
        require(context_found, "INVALID_REFERENCE")?;
        // Bound from the existing closed terms fields: publication1, direct
        // catalog/instantiation2, assents16, grants32, roles16, context1,
        // slot terms16 and16 funding legs each with16 evidence Refs.
        let mut set = Anchors::<340>::new();
        set.add_with_descriptors(
            "publication",
            "publication_source",
            request_wrapper::field(p, "publication_ref", s)?,
            "workflow-publication",
            descriptors.bytes(),
            s,
        )?;
        fn private(
            set: &mut Anchors<340>,
            reference: &Value,
            context: &[u8],
            s: &mut OriginalAuditSession,
        ) -> Result<()> {
            let kind = request_wrapper::text(reference, "kind", s)?;
            let raw = encode(reference, s)?;
            if cmp(raw.bytes(), context, s)?.is_eq() {
                let single = join(&[raw.bytes()], s)?;
                set.add_with_descriptors(
                    "private_document",
                    "context_schema",
                    reference,
                    kind,
                    single.bytes(),
                    s,
                )
            } else {
                set.add("private_document", "agreement_input", reference, kind, s)
            }
        }
        for field in ["slot_catalog_ref", "slot_instantiation_ref"] {
            private(
                &mut set,
                request_wrapper::field(p, field, s)?,
                context_bytes.bytes(),
                s,
            )?;
            private(
                &mut set,
                request_wrapper::field(binding, field, s)?,
                context_bytes.bytes(),
                s,
            )?;
        }
        for (body, field) in [
            (p, "assent_refs"),
            (p, "authority_refs"),
            (binding, "assent_refs"),
            (binding, "authority_refs"),
            (binding, "roles_refs"),
        ] {
            for r in request_wrapper::field(body, field, s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                private(&mut set, r, context_bytes.bytes(), s)?;
            }
        }
        private(&mut set, context, context_bytes.bytes(), s)?;
        for row in request_wrapper::field(binding, "bindings", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            private(
                &mut set,
                request_wrapper::field(row, "binding_ref", s)?,
                context_bytes.bytes(),
                s,
            )?;
        }
        for leg in request_wrapper::field(binding, "funding_legs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            for r in request_wrapper::field(leg, "evidence_refs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                private(&mut set, r, context_bytes.bytes(), s)?;
            }
        }
        let registry = publication
            .program()
            .execution_registry()
            .ok_or(ServiceError::Unavailable)?;
        request_evidence(
            req,
            normalized_payload,
            p,
            &set,
            registry,
            descriptors.bytes(),
            4194304,
            s,
        )
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

/// R1 administration uses its original immutable registry and literal original
/// request. Anchors are freshly extracted from the normalized operands, never
/// from the stored resolution. This does not certify an earlier dependency.
pub(super) fn original_administration(
    origin: &prefix::LocatedWrappers,
    normalized_payload: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let saved = request_wrapper::field(origin.wrappers().operation(), "request_evidence", s)?;
        let resolution = request_wrapper::field(saved, "descriptor_resolution", s)?;
        request_wrapper::same_text(
            request_wrapper::text(resolution, "registry_digest", s)?,
            R1_REGISTRY,
            s,
        )?;
        let req = ingress::IncomingRequest::parse_original_administration(
            origin.wrappers().original_request().raw(),
            s,
        )?;
        request_wrapper::key(
            request_wrapper::field(req.value(), "key", s)?,
            origin.key(),
            s,
        )?;
        let reproduced = administration(&req, normalized_payload, s)?;
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
pub(super) fn administration(
    req: &ingress::IncomingRequest,
    normalized_payload: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<RequestEvidence> {
    let result = (|| {
        let payload = s
            .parse_representation(normalized_payload, Kind::OriginalRequest)
            .map_err(core_error)?;
        let p = payload.value();
        let mut set = Anchors::<65>::new();
        let schema = request_wrapper::text(p, "schema", s)?;
        if cmp(req.kind().as_bytes(), b"evidence.retain", s)?.is_eq() {
            request_wrapper::same_text(schema, "ledger-workflow-evidence-retain/1", s)?;
            let purpose = request_wrapper::text(p, "purpose", s)?;
            let metadata = request_wrapper::field(p, "metadata", s)?;
            if cmp(purpose.as_bytes(), b"operator_attestation", s)?.is_eq() {
                set.add(
                    "evidence",
                    "evidence_input",
                    request_wrapper::field(metadata, "assent_evidence_ref", s)?,
                    "workflow-evidence",
                    s,
                )?;
            } else if cmp(purpose.as_bytes(), b"admission_stop", s)?.is_eq() {
                set.add(
                    "accepted_binding",
                    "accepted_binding",
                    request_wrapper::field(metadata, "binding_ref", s)?,
                    "workflow-binding",
                    s,
                )?;
                set.add(
                    "activation_target",
                    "activation_target",
                    request_wrapper::field(metadata, "target_transition_ref", s)?,
                    "workflow-activation",
                    s,
                )?;
            }
        } else {
            request_wrapper::same_text(req.kind(), "authority.change", s)?;
            if cmp(schema.as_bytes(), b"ledger-workflow-source-enroll/1", s)?.is_eq() {
                let refs = request_wrapper::field(p, "customer_refs", s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&refs.len().into(), &64usize.into()])
                    .map_err(core_error)?;
                require(refs.len() <= 64, "INTEGRITY_FAILURE")?;
                for reference in refs {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    set.add(
                        "customer",
                        "customer_registration",
                        reference,
                        "workflow-customer",
                        s,
                    )?;
                }
                set.add(
                    "evidence",
                    "evidence_input",
                    request_wrapper::field(p, "evidence_ref", s)?,
                    "workflow-evidence",
                    s,
                )?;
            } else if cmp(schema.as_bytes(), b"ledger-workflow-customer-register/1", s)?.is_eq() {
                set.add(
                    "evidence",
                    "evidence_input",
                    request_wrapper::field(p, "registration_evidence_ref", s)?,
                    "workflow-evidence",
                    s,
                )?;
            } else {
                if !cmp(schema.as_bytes(), b"ledger-workflow-source-restrict/1", s)?.is_eq() {
                    request_wrapper::same_text(schema, "ledger-workflow-source-revoke/1", s)?;
                }
                set.add(
                    "source_enrollment",
                    "source_enrollment",
                    request_wrapper::field(p, "enrollment_ref", s)?,
                    "workflow-source-enrollment",
                    s,
                )?;
            }
        }
        request_evidence(
            req,
            normalized_payload,
            p,
            &set,
            R1_REGISTRY,
            b"[]",
            262144,
            s,
        )
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

#[cfg(test)]
mod commitment_tests {
    use super::*;
    #[test]
    fn commitment_anchors_keep_authorization_immediate_and_terms_in_original_recipe() {
        let reference = |kind: &str| serde_json::json!({"kind":kind,"id":format!("wdoc_{}","1".repeat(64)),"digest":format!("sha256:{}","1".repeat(64))});
        let mut value = serde_json::json!({
            "schema":"ledger-operation/1","contract":"workflow/1","kind":"evidence.retain",
            "key":{"logical_store_id":"test-store","scope":["test","local"],"source":"owner","operation_id":"consent-test"},
            "payload":{"schema":"ledger-workflow-party-evidence-retain/2","metadata":{"purpose":"party_consent","consent":{"authorization_ref":reference("workflow-authority"),"basis":{"kind":"acceptance_terms","terms_ref":reference("workflow-acceptance-terms"),"terms_digest":format!("sha256:{}","2".repeat(64))}}}}
        });
        for (schema, registry, expected) in [
            (
                "ledger-workflow-party-evidence-retain/2",
                COMMITMENT_REGISTRY,
                1,
            ),
            ("ledger-workflow-party-evidence-retain/1", REGISTRY, 2),
        ] {
            value["payload"]["schema"] = schema.into();
            let raw = serde_json::to_vec(&value).unwrap();
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let request = ingress::IncomingRequest::parse(&raw, &mut s).unwrap();
            let anchors =
                party_anchors(&request, request.payload(&mut s).unwrap(), registry, &mut s)
                    .unwrap();
            assert_eq!(anchors.len, expected);
            let (locators, _) = anchors.arrays(&mut s).unwrap();
            let parsed = s
                .parse_representation(locators.bytes(), Kind::SourceDefinition)
                .unwrap();
            let text = std::str::from_utf8(locators.bytes()).unwrap();
            assert!(text.contains("workflow-authority"));
            assert_eq!(text.contains("workflow-acceptance-terms"), expected == 2);
            assert_eq!(parsed.value().as_array().unwrap().len(), expected);
        }
    }
}
