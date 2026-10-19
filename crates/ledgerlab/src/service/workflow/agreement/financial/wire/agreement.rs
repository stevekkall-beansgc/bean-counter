//! SAME R1 administrative journal framing for actual AppAccept material.
//! This owns bytes and exact closure only; private preparation/atomic storage
//! and the complete resource certificate remain mandatory outside this module.
use super::super::{
    evidence::RequestEvidence,
    ingress::IncomingRequest,
    prefix::{LocatedWrappers, ResolvedMembers},
};
use super::*;
pub(in super::super) struct AgreementJournalData {
    operation: AuditedBytes,
    manifest: AuditedBytes,
    receipt: AuditedBytes,
    operation_ref: WireRef,
    manifest_ref: WireRef,
    receipt_ref: WireRef,
    members: ResolvedMembers,
}
impl AgreementJournalData {
    pub(in super::super) fn receipt_bytes(&self) -> &[u8] {
        self.receipt.bytes()
    }
    pub(in super::super) fn members(&self) -> &ResolvedMembers {
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
        s: &mut OriginalAuditSession,
    ) -> Result<FinancialJournal<'a>> {
        Ok(FinancialJournal {
            identity: incoming.identity(),
            kind: incoming.kind(),
            operation_ref: &self.operation_ref.reference,
            operation_bytes: self.operation.bytes(),
            request_hash: super::super::request_wrapper::text(evidence.value(), "request_hash", s)?,
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
        origin: &LocatedWrappers,
        s: &mut OriginalAuditSession,
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
            let actual = s
                .canonical_representation(actual, kind)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            require(expected == actual.bytes(), "INTEGRITY_FAILURE")?;
        }
        Ok(())
    }
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn frame_acceptance(
    incoming: &IncomingRequest,
    normalized_payload: &[u8],
    authority: &SourceRecord,
    at: &Timestamp,
    sequence: i64,
    result_refs: [&ExactRef; 3],
    evidence: &RequestEvidence,
    members: ResolvedMembers,
    s: &mut OriginalAuditSession,
) -> Result<AgreementJournalData> {
    super::super::request_wrapper::same_text(incoming.kind(), "agreement.accept", s)
        .inspect_err(|_| s.invalidate())?;
    frame_agreement(
        incoming,
        normalized_payload,
        authority,
        at,
        sequence,
        result_refs,
        evidence,
        members,
        s,
    )
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn frame_activation(
    incoming: &IncomingRequest,
    normalized_payload: &[u8],
    authority: &SourceRecord,
    at: &Timestamp,
    sequence: i64,
    result_refs: [&ExactRef; 3],
    evidence: &RequestEvidence,
    members: ResolvedMembers,
    s: &mut OriginalAuditSession,
) -> Result<AgreementJournalData> {
    super::super::request_wrapper::same_text(incoming.kind(), "agreement.activate", s)
        .inspect_err(|_| s.invalidate())?;
    frame_agreement(
        incoming,
        normalized_payload,
        authority,
        at,
        sequence,
        result_refs,
        evidence,
        members,
        s,
    )
}
#[allow(clippy::too_many_arguments)]
fn frame_agreement(
    incoming: &IncomingRequest,
    normalized_payload: &[u8],
    authority: &SourceRecord,
    at: &Timestamp,
    sequence: i64,
    result_refs: [&ExactRef; 3],
    evidence: &RequestEvidence,
    mut members: ResolvedMembers,
    s: &mut OriginalAuditSession,
) -> Result<AgreementJournalData> {
    let result = (|| {
        use super::super::request_wrapper as check;
        let normalized = check::field(evidence.value(), "normalized_request", s)?;
        let key = check::field(normalized, "key", s)?;
        check::key(key, incoming.key(), s)?;
        check::same_text(check::text(normalized, "kind", s)?, incoming.kind(), s)?;
        let payload = check::field(normalized, "payload", s)?;
        let bytes = s
            .canonical_representation(payload, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(bytes.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(normalized_payload.len())
            .map_err(core_error)?;
        require(bytes.bytes() == normalized_payload, "INTEGRITY_FAILURE")?;
        let resolution = check::field(evidence.value(), "descriptor_resolution", s)?;
        let recipe = wrappers::recipe(
            incoming.kind(),
            check::text(payload, "schema", s)?,
            check::text(resolution, "registry_digest", s)?,
            s,
        )?;
        check::same_text(recipe.operation, "ledger-workflow-operation-record/1", s)?;
        check::same_text(recipe.manifest, "ledger-workflow-agreement-manifest/1", s)?;
        check::same_text(recipe.authority, "ledger-workflow-agreement-authority/5", s)?;
        check::same_text(recipe.closure, "absent", s)?;
        let key_bytes = s
            .canonical_representation(key, RepresentationKind::Record)
            .map_err(core_error)?;
        let scope_value = check::field(key, "scope", s)?;
        let scope = s
            .canonical_representation(scope_value, RepresentationKind::Record)
            .map_err(core_error)?;
        let authority_body = s
            .parse_representation(authority.bytes(), RepresentationKind::Authority)
            .map_err(core_error)?;
        check::same_text(
            check::text(authority_body.value(), "schema", s)?,
            recipe.authority,
            s,
        )?;
        let sequence = sequence_string(sequence, s)?;
        let at = literal(at.as_str(), s)?;
        let results = RefSet::new(result_refs.into_iter(), 3, true, s)?.encode(s)?;
        let operation_kind = literal(incoming.kind(), s)?;
        let operation = object(
            &[
                ("accepted_at", at.bytes()),
                ("authority_snapshot", authority.bytes()),
                ("commit_sequence", sequence.bytes()),
                ("contract", b"\"workflow/1\""),
                ("key", key_bytes.bytes()),
                ("kind", operation_kind.bytes()),
                ("request_evidence", evidence.bytes()),
                ("result_refs", results.bytes()),
                ("schema", b"\"ledger-workflow-operation-record/1\""),
            ],
            8388608,
            s,
        )?;
        let id = hash(
            b"bean-counter/workflow-operation/1\0",
            &[key_bytes.bytes()],
            s,
        )?;
        let digest = hash(
            b"bean-counter/workflow-record/1\0",
            &[
                b"[\"workflow-operation\",",
                scope.bytes(),
                b",",
                operation.bytes(),
                b"]",
            ],
            s,
        )?;
        let operation_ref = owned_reference("workflow-operation", "wop_", &id, &digest, s)?;
        members.add_record_ref(&operation_ref.reference, s)?;
        let member_bytes = members.encode(s)?;
        let manifest = object(
            &[
                ("key", key_bytes.bytes()),
                ("members", member_bytes.bytes()),
                ("schema", b"\"ledger-workflow-agreement-manifest/1\""),
            ],
            4194304,
            s,
        )?;
        let digest = hash(
            b"bean-counter/workflow-manifest/1\0",
            &[manifest.bytes()],
            s,
        )?;
        let manifest_ref = owned_reference("workflow-manifest", "wmanifest_", &digest, &digest, s)?;
        let (receipt, receipt_ref) = receipt(
            key_bytes.bytes(),
            at.bytes(),
            sequence.bytes(),
            &manifest_ref,
            b"\"administrative\"",
            results.bytes(),
            evidence.value(),
            s,
        )?;
        Ok(AgreementJournalData {
            operation,
            manifest,
            receipt,
            operation_ref,
            manifest_ref,
            receipt_ref,
            members,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
