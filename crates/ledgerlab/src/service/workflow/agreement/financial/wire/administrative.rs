//! Exact Operation1/AgreementManifest1/Receipt2 framing for the real party
//! producer. These bytes remain DATA until chronological custody completes.
use super::super::{
    current::party::produce::PendingPartyOperation, evidence::RequestEvidence,
    ingress::IncomingRequest,
};
use super::*;

pub(in super::super) struct PartyJournalData {
    operation: AuditedBytes,
    manifest: AuditedBytes,
    receipt: AuditedBytes,
    operation_ref: WireRef,
    manifest_ref: WireRef,
    receipt_ref: WireRef,
    members: super::super::prefix::ResolvedMembers,
}
impl PartyJournalData {
    pub(in super::super) fn receipt_bytes(&self)->&[u8] {self.receipt.bytes()}
    pub(in super::super) fn members(&self) -> &super::super::prefix::ResolvedMembers {
        &self.members
    }
    pub(in super::super) fn operation_reference(&self) -> &ExactRef {
        &self.operation_ref.reference
    }
    pub(in super::super) fn storage_view<'a>(
        &'a self,
        incoming: &'a IncomingRequest,
        evidence: &'a RequestEvidence,
        members: &'a [&'a ExactRef],
        sequence: i64,
        accepted_at_us: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<FinancialJournal<'a>> {
        Ok(FinancialJournal {
            identity: incoming.identity(),
            kind: incoming.kind(),
            operation_ref: &self.operation_ref.reference,
            operation_bytes: self.operation.bytes(),
            request_hash: super::super::request_wrapper::text(
                evidence.value(),
                "request_hash",
                session,
            )?,
            manifest_ref: &self.manifest_ref.reference,
            manifest_bytes: self.manifest.bytes(),
            receipt_ref: &self.receipt_ref.reference,
            receipt_bytes: self.receipt.bytes(),
            members,
            sequence,
            accepted_at_us,
        })
    }
    pub(in super::super) fn compare_original(
        &self,
        origin: &super::super::prefix::LocatedWrappers,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        for (expected, actual, kind) in [
            (
                self.operation.bytes(),
                origin.wrappers().operation(),
                RepresentationKind::OperationFrame,
            ),
            (
                self.manifest.bytes(),
                origin.wrappers().manifest(),
                RepresentationKind::ManifestFrame,
            ),
            (
                self.receipt.bytes(),
                origin.wrappers().receipt(),
                RepresentationKind::ReceiptFrame,
            ),
        ] {
            let actual = session
                .canonical_representation(actual, kind)
                .map_err(core_error)?;
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(expected.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            require(expected == actual.bytes(), "INTEGRITY_FAILURE")?;
        }
        Ok(())
    }
}
pub(in super::super) fn frame_party(
    incoming: &IncomingRequest,
    pending: &PendingPartyOperation,
    evidence: &RequestEvidence,
    sequence: i64,
    members: super::super::prefix::ResolvedMembers,
    session: &mut OriginalAuditSession,
) -> Result<PartyJournalData> {
    let result = frame_inner(incoming, pending, evidence, sequence, members, session);
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn frame_inner(
    incoming: &IncomingRequest,
    pending: &PendingPartyOperation,
    evidence: &RequestEvidence,
    sequence: i64,
    mut members: super::super::prefix::ResolvedMembers,
    session: &mut OriginalAuditSession,
) -> Result<PartyJournalData> {
    use super::super::request_wrapper as request;
    let normalized = request::field(evidence.value(), "normalized_request", session)?;
    request::key(
        request::field(normalized, "key", session)?,
        incoming.key(),
        session,
    )?;
    request::same_text(
        request::text(normalized, "kind", session)?,
        incoming.kind(),
        session,
    )?;
    let payload = request::field(normalized, "payload", session)?;
    let normalized_payload = session
        .canonical_representation(payload, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(normalized_payload.bytes().len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(pending.normalized_payload().len())
        .map_err(core_error)?;
    require(
        normalized_payload.bytes() == pending.normalized_payload(),
        "INTEGRITY_FAILURE",
    )?;
    let resolution = request::field(evidence.value(), "descriptor_resolution", session)?;
    let recipe = wrappers::recipe(
        incoming.kind(),
        request::text(payload, "schema", session)?,
        request::text(resolution, "registry_digest", session)?,
        session,
    )?;
    request::same_text(
        recipe.operation,
        "ledger-workflow-operation-record/1",
        session,
    )?;
    request::same_text(
        recipe.manifest,
        "ledger-workflow-agreement-manifest/1",
        session,
    )?;
    request::same_text(
        recipe.authority,
        "ledger-workflow-party-management-authority/1",
        session,
    )?;
    request::same_text(recipe.closure, "absent", session)?;
    let key = request::field(normalized, "key", session)?;
    let key_bytes = session
        .canonical_representation(key, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let scope_value = request::field(key, "scope", session)?;
    let scope = session
        .canonical_representation(scope_value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let sequence_bytes = sequence_string(sequence, session)?;
    let at = literal(pending.at.as_str(), session)?;
    let kind = literal(incoming.kind(), session)?;
    let results = RefSet::new(
        std::iter::once(pending.result.reference()),
        1,
        true,
        session,
    )?
    .encode(session)?;
    let operation = object(
        &[
            ("accepted_at", at.bytes()),
            ("authority_snapshot", pending.authority.bytes()),
            ("commit_sequence", sequence_bytes.bytes()),
            ("contract", b"\"workflow/1\""),
            ("key", key_bytes.bytes()),
            ("kind", kind.bytes()),
            ("request_evidence", evidence.bytes()),
            ("result_refs", results.bytes()),
            ("schema", b"\"ledger-workflow-operation-record/1\""),
        ],
        8388608,
        session,
    )?;
    let id_hash = hash(
        b"bean-counter/workflow-operation/1\0",
        &[key_bytes.bytes()],
        session,
    )?;
    let op_hash = hash(
        b"bean-counter/workflow-record/1\0",
        &[
            b"[\"workflow-operation\",",
            scope.bytes(),
            b",",
            operation.bytes(),
            b"]",
        ],
        session,
    )?;
    let operation_ref = owned_reference("workflow-operation", "wop_", &id_hash, &op_hash, session)?;
    members.add_record_ref(&operation_ref.reference, session)?;
    let member_bytes = members.encode(session)?;
    let manifest = object(
        &[
            ("key", key_bytes.bytes()),
            ("members", member_bytes.bytes()),
            ("schema", b"\"ledger-workflow-agreement-manifest/1\""),
        ],
        4194304,
        session,
    )?;
    let manifest_hash = hash(
        b"bean-counter/workflow-manifest/1\0",
        &[manifest.bytes()],
        session,
    )?;
    let manifest_ref = owned_reference(
        "workflow-manifest",
        "wmanifest_",
        &manifest_hash,
        &manifest_hash,
        session,
    )?;
    let (receipt, receipt_ref) = receipt(
        key_bytes.bytes(),
        at.bytes(),
        sequence_bytes.bytes(),
        &manifest_ref,
        b"\"administrative\"",
        results.bytes(),
        evidence.value(),
        session,
    )?;
    Ok(PartyJournalData {
        operation,
        manifest,
        receipt,
        operation_ref,
        manifest_ref,
        receipt_ref,
        members,
    })
}
