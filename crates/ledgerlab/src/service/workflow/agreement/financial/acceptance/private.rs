//! Owning private-document normalization for actual AppAccept8 input. Each
//! family uses its SAME shared normalizer, never a canonical-only input rule.
//! Remaining families/complete graph still precede any prepared acceptance.
use super::*;
use ledgerlab_core::policy::workflow::{
    RoleAssignment, SlotBindingTerms, SlotCatalog, SlotInstantiation,
};
enum Document {
    Inline {
        parsed: AuditedValue,
        record: wire::SourceRecord,
    },
    Retained(current::source::SourceRecord),
}
impl Document {
    fn reference(&self) -> &ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        match self {
            Self::Inline { record, .. } => record.reference(),
            Self::Retained(r) => r.reference(),
        }
    }
    fn body(&self) -> &Value {
        match self {
            Self::Inline { parsed, .. } => parsed.value(),
            Self::Retained(r) => r.body(),
        }
    }
}
pub(super) struct InlineDocuments {
    documents: Vec<Document>,
    _control: ExternalBufferLease,
}
impl InlineDocuments {
    pub(super) fn record_views(
        &self,
    ) -> impl Iterator<
        Item = ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView<'_>,
    > {
        self.documents.iter().map(|d| match d {
            Document::Inline { record, .. } => {
                ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView {
                    reference: record.reference(),
                    canonical_body: record.bytes(),
                }
            }
            Document::Retained(r) => {
                ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView {
                    reference: r.reference(),
                    canonical_body: r.canonical_body(),
                }
            }
        })
    }
    pub(super) fn inline_records(&self) -> impl Iterator<Item = &wire::SourceRecord> {
        self.documents.iter().filter_map(|d| match d {
            Document::Inline { record, .. } => Some(record),
            Document::Retained(_) => None,
        })
    }
    /// Inline bodies are newly produced at this acceptance. Retained bodies
    /// must instead belong to the actual completed earlier prefix.
    pub(super) fn match_retained(
        &self,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        for document in &self.documents {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if let Document::Retained(record) = document {
                prefix.require_record(record.reference(), s)?;
            }
        }
        Ok(())
    }
    /// Exact immutable body lookup in this attempt. Matching retained bytes is
    /// DATA; chronological origin completion is a separate mandatory gate.
    fn find<'a>(&'a self, reference: &Value, s: &mut OriginalAuditSession) -> Result<&'a Document> {
        let expected = encode(reference, s)?;
        for doc in &self.documents {
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let actual = wire::reference(doc.reference(), s)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            if expected.bytes() == actual.bytes() {
                return Ok(doc);
            }
        }
        Err(reject("INVALID_REFERENCE"))
    }
    pub(super) fn body<'a>(
        &'a self,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<&'a Value> {
        Ok(self.find(reference, s)?.body())
    }
    pub(super) fn reference<'a>(
        &'a self,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<&'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef> {
        Ok(self.find(reference, s)?.reference())
    }
    async fn resolve(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        reference: &Value,
        kind: &str,
        schema: &str,
        before: i64,
        maximum: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let expected = encode(reference, s)?;
        for doc in &self.documents {
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let actual = wire::reference(doc.reference(), s)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            if expected.bytes() == actual.bytes() {
                return Ok(());
            }
        }
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&self.documents.len().into(), &maximum.into()],
            )
            .map_err(core_error)?;
        require(self.documents.len() < maximum, "INVALID_REFERENCE")?;
        let record =
            current::source::retained_record(tx, key, reference, kind, schema, before, s).await?;
        s.semantic_work()
            .write_bytes(mem::size_of::<Document>())
            .map_err(core_error)?;
        self.documents.push(Document::Retained(record));
        Ok(())
    }
}
pub(super) fn normalize_inline(
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    s: &mut OriginalAuditSession,
) -> Result<InlineDocuments> {
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
    let bindings = request_wrapper::field(projected, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(bindings.len(), 1, 16, s)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&refs.len().into(), &bindings.len().into()])
        .map_err(core_error)?;
    let ref_count = refs
        .len()
        .checked_add(bindings.len())
        .ok_or(ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&ref_count.into(), &2usize.into()])
        .map_err(core_error)?;
    let maximum = ref_count.checked_add(2).ok_or(ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&maximum.into(), &mem::size_of::<Document>().into()],
        )
        .map_err(core_error)?;
    let n = maximum
        .checked_mul(mem::size_of::<Document>())
        .ok_or(ServiceError::Unavailable)?;
    let control = s.reserve_external_payload(n).map_err(core_error)?;
    let mut out = InlineDocuments {
        documents: Vec::new(),
        _control: control,
    };
    out.documents
        .try_reserve_exact(maximum)
        .map_err(|_| ServiceError::Unavailable)?;
    for body in documents {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let schema = request_wrapper::text(body, "schema", s)?;
        let mut family = None;
        for (expected, index) in [
            ("ledger-workflow-role-assignment/1", 0u8),
            ("ledger-workflow-slot-catalog/1", 1),
            ("ledger-workflow-slot-instantiation/1", 2),
            ("ledger-workflow-slot-binding-terms/7", 3),
        ] {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(schema.len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.len())
                .map_err(core_error)?;
            if schema == expected {
                family = Some(index);
                break;
            }
        }
        let Some(family) = family else {
            continue;
        };
        s.semantic_work()
            .arithmetic(Op::Compare, &[&out.documents.len().into(), &maximum.into()])
            .map_err(core_error)?;
        require(out.documents.len() < maximum, "INVALID_REFERENCE")?;
        let raw = encode(body, s)?;
        let (kind, schema, normalized) = match family {
            0 => (
                "workflow-roles",
                "ledger-workflow-role-assignment/1",
                RoleAssignment::normalize_private_with_session(raw.bytes(), s)
                    .map_err(core_error)?,
            ),
            1 => (
                "workflow-slot-catalog",
                "ledger-workflow-slot-catalog/1",
                SlotCatalog::normalize_private_with_session(raw.bytes(), s).map_err(core_error)?,
            ),
            2 => (
                "workflow-slot-instantiation",
                "ledger-workflow-slot-instantiation/1",
                SlotInstantiation::normalize_private_with_session(raw.bytes(), s)
                    .map_err(core_error)?,
            ),
            3 => (
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/7",
                SlotBindingTerms::normalize_private_with_session(raw.bytes(), s)
                    .map_err(core_error)?,
            ),
            _ => return Err(ServiceError::IntegrityFailure),
        };
        let doc = wire::source_record(kind, schema, normalized, s)?;
        let expected = wire::reference(doc.reference(), s)?;
        let mut found = false;
        if family == 0 {
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
        } else if family == 3 {
            for row in bindings {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let actual = encode(request_wrapper::field(row, "binding_ref", s)?, s)?;
                s.semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
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
        } else {
            let name = if family == 1 {
                "slot_catalog_ref"
            } else {
                "slot_instantiation_ref"
            };
            let actual = encode(request_wrapper::field(projected, name, s)?, s)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            found = expected.bytes() == actual.bytes();
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
            .write_bytes(mem::size_of::<Document>())
            .map_err(core_error)?;
        let parsed = s
            .parse_representation(doc.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        out.documents.push(Document::Inline {
            parsed,
            record: doc,
        });
    }
    Ok(out)
}

/// Resolve every originally nominated graph body from inline normalized
/// documents or the SAME scoped/exclusive-before retained reader. No missing
/// inline body causes a latest/current-policy fallback.
pub(super) async fn resolve_graph(
    mut documents: InlineDocuments,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &AppAcceptanceInput,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<InlineDocuments> {
    let projected = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let roles = request_wrapper::field(projected, "roles_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let bindings = request_wrapper::field(projected, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(roles.len(), 1, 16, s)?;
    size(bindings.len(), 1, 16, s)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&roles.len().into(), &bindings.len().into()])
        .map_err(core_error)?;
    let count = roles
        .len()
        .checked_add(bindings.len())
        .ok_or(ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&count.into(), &2usize.into()])
        .map_err(core_error)?;
    let maximum = count.checked_add(2).ok_or(ServiceError::Unavailable)?;
    for (field, kind, schema) in [
        (
            "slot_catalog_ref",
            "workflow-slot-catalog",
            "ledger-workflow-slot-catalog/1",
        ),
        (
            "slot_instantiation_ref",
            "workflow-slot-instantiation",
            "ledger-workflow-slot-instantiation/1",
        ),
    ] {
        documents
            .resolve(
                tx,
                key,
                request_wrapper::field(projected, field, s)?,
                kind,
                schema,
                before,
                maximum,
                s,
            )
            .await?;
    }
    for role in roles {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        documents
            .resolve(
                tx,
                key,
                role,
                "workflow-roles",
                "ledger-workflow-role-assignment/1",
                before,
                maximum,
                s,
            )
            .await?;
    }
    for binding in bindings {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        documents
            .resolve(
                tx,
                key,
                request_wrapper::field(binding, "binding_ref", s)?,
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/7",
                before,
                maximum,
                s,
            )
            .await?;
    }
    Ok(documents)
}

/// Exact caller associations, before the SAME selected slot predicate. No
/// origin/current-source eligibility is inferred from a matching Ref or name.
pub(super) fn associate_graph(
    documents: &InlineDocuments,
    input: &AppAcceptanceInput,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let binding = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let roles = request_wrapper::field(binding, "roles_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let bindings = request_wrapper::field(binding, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    for row in bindings {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let terms = documents.body(request_wrapper::field(row, "binding_ref", s)?, s)?;
        request_wrapper::same_text(
            request_wrapper::text(row, "slot", s)?,
            request_wrapper::text(terms, "slot", s)?,
            s,
        )?;
        let role_ref = request_wrapper::field(terms, "role_ref", s)?;
        let expected = encode(role_ref, s)?;
        let mut nominated = false;
        for r in roles {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            let candidate = encode(r, s)?;
            s.semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(candidate.bytes().len())
                .map_err(core_error)?;
            if expected.bytes() == candidate.bytes() {
                nominated = true;
                break;
            }
        }
        require(nominated, "INVALID_REFERENCE")?;
        let role = documents.body(role_ref, s)?;
        request_wrapper::same_text(
            request_wrapper::text(terms, "role_slot", s)?,
            request_wrapper::text(role, "role_slot", s)?,
            s,
        )?;
    }
    Ok(())
}

/// Execute the SAME compiled structural/terms/funding projection for every
/// actual accepted binding slot. This completes no source/head/origin proof.
pub(super) fn validate_slots(
    documents: &InlineDocuments,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    binding: &Value,
    scope: &[String; 2],
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let projected = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let catalog = documents.body(request_wrapper::field(projected, "slot_catalog_ref", s)?, s)?;
    let instantiation = documents.body(
        request_wrapper::field(projected, "slot_instantiation_ref", s)?,
        s,
    )?;
    let bindings = request_wrapper::field(projected, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    for row in bindings {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let terms = documents.body(request_wrapper::field(row, "binding_ref", s)?, s)?;
        let role = documents.body(request_wrapper::field(terms, "role_ref", s)?, s)?;
        ledgerlab_core::policy::workflow::transition_constructor::validate_app_slot_projection(
            publication.program(),
            scope,
            binding,
            catalog,
            instantiation,
            terms,
            role,
            s,
        )
        .map_err(core_error)?;
    }
    Ok(())
}
