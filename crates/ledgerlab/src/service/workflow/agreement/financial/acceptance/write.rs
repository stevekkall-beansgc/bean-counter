//! Owning control storage for exact borrowed acceptance write operands. This
//! is DATA, never PreparedAcceptance: complete source/resource/original/current
//! authentication must finish before the private coordinator calls storage.
use super::*;
use crate::store::sqlite::workflow::financial::{AppAcceptanceAppend, FinancialDocument};
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;

pub(in super::super) struct AcceptanceWriteData<'a> {
    pending: &'a PendingAppAcceptance,
    incoming: &'a ingress::IncomingRequest,
    journal: &'a wire::AgreementJournalData,
    evidence: &'a evidence::RequestEvidence,
    documents: Vec<FinancialDocument<'a>>,
    members: Vec<&'a ExactRef>,
    semantic_key: AuditedBytes,
    customer: &'a str,
    agreement: &'a str,
    expected_revision: i64,
    revision: i64,
    _storage: ExternalBufferLease,
}
fn arithmetic(a: usize, b: usize, op: Op, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(op, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    match op {
        Op::Add => a.checked_add(b),
        Op::Multiply => a.checked_mul(b),
        _ => None,
    }
    .ok_or(ServiceError::Unavailable)
}
fn equal_ref(a: &ExactRef, b: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
    for (a, b) in [(&a.kind, &b.kind), (&a.id, &b.id), (&a.digest, &b.digest)] {
        if !same_recipe_text(a, b, s)? {
            return Ok(false);
        }
    }
    Ok(true)
}
impl<'a> AcceptanceWriteData<'a> {
    pub(in super::super) fn new(
        pending: &'a PendingAppAcceptance,
        incoming: &'a ingress::IncomingRequest,
        journal: &'a wire::AgreementJournalData,
        evidence: &'a evidence::RequestEvidence,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        const DOCUMENTS: usize = 7 + 128 + 3 * 16;
        let count = journal.members().len();
        size(count, 1, 4096, s)?;
        let documents = arithmetic(
            DOCUMENTS,
            mem::size_of::<FinancialDocument>(),
            Op::Multiply,
            s,
        )?;
        let refs = arithmetic(count, mem::size_of::<&ExactRef>(), Op::Multiply, s)?;
        let seen = arithmetic(count, mem::size_of::<Option<usize>>(), Op::Multiply, s)?;
        let storage = arithmetic(arithmetic(documents, refs, Op::Add, s)?, seen, Op::Add, s)?;
        let storage = s.reserve_external_payload(storage).map_err(core_error)?;
        let mut documents = Vec::new();
        documents
            .try_reserve_exact(DOCUMENTS)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut seen: Vec<Option<usize>> = Vec::new();
        seen.try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        let seen_bytes = arithmetic(count, mem::size_of::<Option<usize>>(), Op::Multiply, s)?;
        s.semantic_work()
            .write_bytes(seen_bytes)
            .map_err(core_error)?;
        seen.resize(count, None);
        for r in pending.generated_records() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let position = journal.members().position(r.reference(), s)?;
            if let Some(prior) = seen[position] {
                let prior: &FinancialDocument = &documents[prior];
                let work = s.semantic_work();
                work.visit(Visit::MemberCompare).map_err(core_error)?;
                work.scan_bytes(prior.canonical_bytes.len())
                    .map_err(core_error)?;
                work.scan_bytes(r.bytes().len()).map_err(core_error)?;
                require(prior.canonical_bytes == r.bytes(), "INTEGRITY_FAILURE")?;
                request_wrapper::same_text(prior.schema, r.schema(), s)?;
                continue;
            }
            size(documents.len(), 0, DOCUMENTS - 1, s)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<Option<usize>>())
                .map_err(core_error)?;
            seen[position] = Some(documents.len());
            s.semantic_work()
                .write_bytes(mem::size_of::<FinancialDocument>())
                .map_err(core_error)?;
            documents.push(FinancialDocument {
                reference: r.reference(),
                schema: r.schema(),
                canonical_bytes: r.bytes(),
            });
        }
        let mut own = 0usize;
        for index in 0..count {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let r = journal.members().at(index, s)?;
            if equal_ref(r, journal.operation_reference(), s)? {
                own = arithmetic(own, 1, Op::Add, s)?;
            } else {
                s.semantic_work()
                    .write_bytes(mem::size_of::<&ExactRef>())
                    .map_err(core_error)?;
                members.push(r);
            }
        }
        size(own, 1, 1, s)?;
        let binding = pending.binding_value.value();
        let customer = request_wrapper::text(binding, "customer", s)?;
        let agreement = request_wrapper::text(binding, "agreement_id", s)?;
        let version = request_wrapper::field(binding, "version", s)?;
        let customer_json = wire::string(customer, s)?;
        let agreement_json = wire::string(agreement, s)?;
        let version_json = s
            .canonical_representation(version, Kind::SourceDefinition)
            .map_err(core_error)?;
        let payload = arithmetic(
            arithmetic(
                customer_json.bytes().len(),
                agreement_json.bytes().len(),
                Op::Add,
                s,
            )?,
            version_json.bytes().len(),
            Op::Add,
            s,
        )?;
        let payload = arithmetic(payload, 4, Op::Add, s)?;
        let (arena, work) = s.parts();
        let mut semantic = arena.byte_builder(payload, work).map_err(core_error)?;
        for part in [
            b"[".as_slice(),
            customer_json.bytes(),
            b",",
            agreement_json.bytes(),
            b",",
            version_json.bytes(),
            b"]",
        ] {
            semantic
                .append(part, s.semantic_work())
                .map_err(core_error)?;
        }
        let semantic_key = semantic.finish(s.semantic_work()).map_err(core_error)?;
        let expected = pending.input.expected_revision();
        s.semantic_work()
            .arithmetic(Op::Compare, &[&expected.into(), &(i64::MAX as u64).into()])
            .map_err(core_error)?;
        require(expected < i64::MAX as u64, "INVALID_SCALAR")?;
        s.semantic_work()
            .arithmetic(Op::Add, &[&expected.into(), &1u64.into()])
            .map_err(core_error)?;
        let revision = expected + 1;
        Ok(Self {
            pending,
            incoming,
            journal,
            evidence,
            documents,
            members,
            semantic_key,
            customer,
            agreement,
            expected_revision: expected as i64,
            revision: revision as i64,
            _storage: storage,
        })
    }
    pub(in super::super) fn append_view(
        &self,
        s: &mut OriginalAuditSession,
    ) -> Result<AppAcceptanceAppend<'_>> {
        let key = self.incoming.key();
        Ok(AppAcceptanceAppend {
            logical_store_id: &key.logical_store_id,
            scope: &key.scope,
            source: &key.source,
            operation_id: &key.operation_id,
            documents: &self.documents,
            journal: self.journal.storage_view(
                self.incoming,
                self.evidence,
                &self.members,
                self.pending.before,
                self.pending.at.micros(),
                s,
            )?,
            customer: self.customer,
            agreement: self.agreement,
            semantic_key: self.semantic_key.bytes(),
            expected_revision: self.expected_revision,
            revision: self.revision,
            version: self.revision,
            head: self.pending.head_records.head.reference(),
            accepted: self.pending.head_records.accepted.reference(),
            binding: self.pending.binding.reference(),
            intent: self.pending.normalized.intent.reference(),
        })
    }
}
