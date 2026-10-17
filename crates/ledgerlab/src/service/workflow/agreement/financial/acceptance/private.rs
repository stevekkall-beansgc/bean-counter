//! Owning private-document normalization for actual AppAccept8 input. Each
//! family uses its SAME shared normalizer, never a canonical-only input rule.
//! Remaining families/complete graph still precede any prepared acceptance.
use super::*;
use ledgerlab_core::policy::workflow::RoleAssignment;
pub(super) struct InlineRoles {
    documents: Vec<wire::SourceRecord>,
    _control: ExternalBufferLease,
}
impl InlineRoles {
    pub(super) fn documents(&self) -> &[wire::SourceRecord] {
        &self.documents
    }
}
pub(super) fn inline_roles(
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    s: &mut OriginalAuditSession,
) -> Result<InlineRoles> {
    let payload = incoming.payload(s)?;
    let documents = request_wrapper::field(payload, "documents", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(documents.len(), 0, 128, s)?;
    let projected = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let refs = request_wrapper::field(projected, "roles_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(refs.len(), 1, 16, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[
                &refs.len().into(),
                &mem::size_of::<wire::SourceRecord>().into(),
            ],
        )
        .map_err(core_error)?;
    let n = refs
        .len()
        .checked_mul(mem::size_of::<wire::SourceRecord>())
        .ok_or(ServiceError::Unavailable)?;
    let control = s.reserve_external_payload(n).map_err(core_error)?;
    let mut out = InlineRoles {
        documents: Vec::new(),
        _control: control,
    };
    out.documents
        .try_reserve_exact(refs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for body in documents {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let schema = request_wrapper::text(body, "schema", s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(schema.len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes("ledger-workflow-role-assignment/1".len())
            .map_err(core_error)?;
        if schema != "ledger-workflow-role-assignment/1" {
            continue;
        }
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&out.documents.len().into(), &refs.len().into()],
            )
            .map_err(core_error)?;
        require(out.documents.len() < refs.len(), "INVALID_REFERENCE")?;
        let raw = encode(body, s)?;
        let normalized =
            RoleAssignment::normalize_private_with_session(raw.bytes(), s).map_err(core_error)?;
        let doc = wire::source_record(
            "workflow-roles",
            "ledger-workflow-role-assignment/1",
            normalized,
            s,
        )?;
        let expected = wire::reference(doc.reference(), s)?;
        let mut found = false;
        for r in refs {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            let actual = encode(r, s)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            if expected.bytes() == actual.bytes() {
                found = true;
                break;
            }
        }
        require(found, "INVALID_REFERENCE")?;
        for prior in &out.documents {
            let actual = wire::reference(prior.reference(), s)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            require(actual.bytes() != expected.bytes(), "INVALID_SHAPE")?;
        }
        s.semantic_work()
            .write_bytes(mem::size_of::<wire::SourceRecord>())
            .map_err(core_error)?;
        out.documents.push(doc);
    }
    Ok(out)
}
