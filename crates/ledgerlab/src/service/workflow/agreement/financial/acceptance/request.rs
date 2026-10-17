//! SAME normalized AppAccept payload/intent from the already validated owning
//! binding, private definitions and actual fullterms assent material.
use super::*;

pub(super) struct NormalizedAcceptance {
    pub(super) payload: AuditedBytes,
    pub(super) intent: wire::SourceRecord,
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn documents(
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    private: &private::InlineDocuments,
    consents: &consent::Coverage,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let raw = request_wrapper::field(incoming.payload(s)?, "documents", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(raw.len(), 0, 128, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&raw.len().into(), &mem::size_of::<&[u8]>().into()],
        )
        .map_err(core_error)?;
    let storage = raw
        .len()
        .checked_mul(mem::size_of::<&[u8]>())
        .ok_or(ServiceError::Unavailable)?;
    let _storage = s.reserve_external_payload(storage).map_err(core_error)?;
    let mut normalized = Vec::new();
    normalized
        .try_reserve_exact(raw.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for record in private.inline_records() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<&[u8]>())
            .map_err(core_error)?;
        normalized.push(record.bytes());
    }
    for value in raw {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        if let Some(record) = consents.normalized_inline(value, input, s)? {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&normalized.len().into(), &raw.len().into()])
                .map_err(core_error)?;
            require(normalized.len() < raw.len(), "INVALID_SHAPE")?;
            s.semantic_work()
                .write_bytes(mem::size_of::<&[u8]>())
                .map_err(core_error)?;
            normalized.push(record.bytes());
        }
    }
    // Unknown or unused private inline bodies are not silently omitted.
    s.semantic_work()
        .arithmetic(Op::Compare, &[&normalized.len().into(), &raw.len().into()])
        .map_err(core_error)?;
    require(normalized.len() == raw.len(), "INVALID_SHAPE")?;
    ledgerlab_core::validation_work::try_sort_by(&mut normalized, s.semantic_work(), |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(a.len())?;
        w.scan_bytes(b.len())?;
        Ok(a.cmp(b))
    })
    .map_err(core_error)?;
    for pair in normalized.windows(2) {
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(pair[0].len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(pair[1].len())
            .map_err(core_error)?;
        require(pair[0] != pair[1], "INVALID_SHAPE")?;
    }
    evidence::join(&normalized, s)
}
pub(super) fn build(
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    binding: &Value,
    private: &private::InlineDocuments,
    consents: &consent::Coverage,
    s: &mut OriginalAuditSession,
) -> Result<NormalizedAcceptance> {
    let docs = documents(incoming, input, private, consents, s)?;
    // These are the exact normalized BindingInputs fields; the additional
    // accepted Binding fields are not copied into request intent.
    const KEYS: [&str; 17] = [
        "accepted_context",
        "agreement_id",
        "assent_refs",
        "authority_refs",
        "bindings",
        "customer",
        "ends_at",
        "exposure_ceilings",
        "funding_legs",
        "quantity_ceilings",
        "roles_refs",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "source_namespace",
        "source_slots",
        "starts_at",
        "version",
    ];
    let mut values: [Option<AuditedBytes>; 17] = std::array::from_fn(|_| None);
    let mut fields = [("", b"".as_slice()); 17];
    let mut count = 0usize;
    for (index, name) in KEYS.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let mut found = None;
        for (key, value) in binding.as_object().ok_or(ServiceError::IntegrityFailure)? {
            if assents::equal(key, name, s)? {
                found = Some(value);
                break;
            }
        }
        if let Some(value) = found {
            values[index] = Some(encode(value, s)?);
        } else {
            require(assents::equal(name, "ends_at", s)?, "INVALID_SHAPE")?;
        }
    }
    for (name, value) in KEYS.iter().zip(&values) {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        if let Some(value) = value {
            s.semantic_work()
                .write_bytes(mem::size_of::<(&str, &[u8])>())
                .map_err(core_error)?;
            fields[count] = (name, value.bytes());
            count = add(count, 1, s)?;
        }
    }
    let projected = wire::object(&fields[..count], 262144, s)?;
    let schema = wire::string(
        if input.commitment {
            "ledger-workflow-agreement-accept/9"
        } else {
            "ledger-workflow-agreement-accept/8"
        },
        s,
    )?;
    let agreement = encode(request_wrapper::field(binding, "agreement_id", s)?, s)?;
    let assents = encode(input.assent_refs(), s)?;
    let authorities = encode(input.authority_refs(), s)?;
    let revision = wire::counter_string(input.expected_revision, s)?;
    let publication = encode(
        request_wrapper::field(input.terms.canonical_value(), "publication_ref", s)?,
        s,
    )?;
    let catalog = encode(request_wrapper::field(binding, "slot_catalog_ref", s)?, s)?;
    let instantiation = encode(
        request_wrapper::field(binding, "slot_instantiation_ref", s)?,
        s,
    )?;
    let payload = wire::object(
        &[
            ("agreement_id", agreement.bytes()),
            ("assent_refs", assents.bytes()),
            ("authority_refs", authorities.bytes()),
            ("binding_inputs", projected.bytes()),
            ("documents", docs.bytes()),
            ("expected_revision", revision.bytes()),
            ("publication_ref", publication.bytes()),
            ("schema", schema.bytes()),
            ("slot_catalog_ref", catalog.bytes()),
            ("slot_instantiation_ref", instantiation.bytes()),
        ],
        262144,
        s,
    )?;
    let intent = wire::object(
        &[
            ("kind", b"\"agreement.accept\""),
            ("payload", payload.bytes()),
            ("schema", b"\"ledger-workflow-agreement-intent/1\""),
        ],
        262144,
        s,
    )?;
    let intent = wire::source_record(
        "workflow-agreement-intent",
        "ledger-workflow-agreement-intent/1",
        intent,
        s,
    )?;
    Ok(NormalizedAcceptance { payload, intent })
}
