//! Original selected-program replay from actual retained operands. The result
//! remains DATA: prefix introductions, historical authority and build provenance
//! must complete before this program may participate in financial preparation.
use super::*;
use crate::store::sqlite::workflow::financial::RetainedDocument;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    policy::workflow::{
        DefinitionRef,
        selected_program::{SelectedProgram, SelectedSourceInput, restore_selected_program},
        transition_constructor::ExactRef,
    },
};
use sha2::{Digest, Sha256};

struct Record {
    parsed: AuditedValue,
    stored: RetainedDocument,
    reference: ExactRef,
    _reference_lease: ExternalBufferLease,
}
/// Holds actual immutable operands and the recompiled DATA program. No constructor
/// for CompletedOrigin/PreparedFinancialCommit exists here. The input-copy lease
/// covers only the host's compiler input; the core must also own its outputs.
pub(in crate::service::workflow::agreement::financial) struct RecompiledPublication {
    program: SelectedProgram,
    publication: Record,
    plan: Record,
    build: Record,
    authority: Record,
    sources: Vec<Record>,
    _source_control: ExternalBufferLease,
    _compiler_input: ExternalBufferLease,
}
impl RecompiledPublication {
    pub(in crate::service::workflow::agreement::financial) fn program(&self) -> &SelectedProgram {
        &self.program
    }
    pub(in crate::service::workflow::agreement::financial) fn publication(
        &self,
    ) -> (&ExactRef, &Value) {
        (&self.publication.reference, self.publication.parsed.value())
    }
    pub(super) fn plan(&self) -> (&ExactRef, &[u8]) {
        (&self.plan.reference, self.plan.stored.body.bytes())
    }
    pub(super) fn additional_origin_refs(&self) -> [&ExactRef; 2] {
        [&self.build.reference, &self.authority.reference]
    }
    pub(super) fn source_refs(&self) -> impl ExactSizeIterator<Item = &ExactRef> {
        self.sources.iter().map(|s| &s.reference)
    }
}
fn arithmetic(
    left: usize,
    right: usize,
    op: Op,
    session: &mut OriginalAuditSession,
) -> Result<usize> {
    session
        .semantic_work()
        .arithmetic(op, &[&left.into(), &right.into()])
        .map_err(core_error)?;
    match op {
        Op::Add => left.checked_add(right),
        Op::Multiply => left.checked_mul(right),
        _ => None,
    }
    .ok_or(ServiceError::IntegrityFailure)
}
fn count(
    actual: usize,
    minimum: usize,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let work = session.semantic_work();
    work.visit(Visit::Constraint).map_err(core_error)?;
    work.arithmetic(Op::Compare, &[&actual.into(), &minimum.into()])
        .map_err(core_error)?;
    require(actual >= minimum, "INTEGRITY_FAILURE")?;
    work.arithmetic(Op::Compare, &[&actual.into(), &maximum.into()])
        .map_err(core_error)?;
    require(actual <= maximum, "INTEGRITY_FAILURE")
}
fn same_bytes(left: &[u8], right: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(left.len()).map_err(core_error)?;
    work.scan_bytes(right.len()).map_err(core_error)?;
    require(left == right, "INTEGRITY_FAILURE")
}
fn same_value(left: &Value, right: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    let a = session
        .canonical_representation(left, RepresentationKind::SourceClosure)
        .map_err(core_error)?;
    let b = session
        .canonical_representation(right, RepresentationKind::SourceClosure)
        .map_err(core_error)?;
    same_bytes(a.bytes(), b.bytes(), session)
}
fn parts<'a>(
    value: &'a Value,
    session: &mut OriginalAuditSession,
) -> Result<(&'a str, &'a str, &'a str)> {
    request_wrapper::closed(value, &["kind", "id", "digest"], session)?;
    Ok((
        request_wrapper::text(value, "digest", session)?,
        request_wrapper::text(value, "id", session)?,
        request_wrapper::text(value, "kind", session)?,
    ))
}
fn reference_cmp(
    a: &Value,
    b: &Value,
    session: &mut OriginalAuditSession,
) -> Result<std::cmp::Ordering> {
    let a = parts(a, session)?;
    let b = parts(b, session)?;
    for (left, right) in [(a.0, b.0), (a.1, b.1), (a.2, b.2)] {
        let work = session.semantic_work();
        work.visit(Visit::MemberCompare).map_err(core_error)?;
        work.scan_bytes(left.len()).map_err(core_error)?;
        work.scan_bytes(right.len()).map_err(core_error)?;
        let order = left.cmp(right);
        if order != std::cmp::Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(std::cmp::Ordering::Equal)
}
pub(super) fn member(
    reference: &Value,
    manifest: &Value,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    // validate_frame already proved the manifest's strict Ref order and exact
    // indexed association. Use its actual order without copying/re-sorting it.
    let members = request_wrapper::field(manifest, "members", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let (mut first, mut end) = (0usize, members.len());
    loop {
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&first.into(), &end.into()])
            .map_err(core_error)?;
        if first >= end {
            return Err(ServiceError::IntegrityFailure);
        }
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&end.into(), &first.into()])
            .map_err(core_error)?;
        let length = end - first;
        session
            .semantic_work()
            .arithmetic(Op::DivideRemainder, &[&length.into(), &2usize.into()])
            .map_err(core_error)?;
        let middle = arithmetic(first, length / 2, Op::Add, session)?;
        match reference_cmp(&members[middle], reference, session)? {
            std::cmp::Ordering::Equal => return Ok(()),
            std::cmp::Ordering::Less => first = arithmetic(middle, 1, Op::Add, session)?,
            std::cmp::Ordering::Greater => end = middle,
        }
    }
}
fn owned_reference(
    value: &Value,
    session: &mut OriginalAuditSession,
) -> Result<(ExactRef, ExternalBufferLease)> {
    let (digest, id, kind) = parts(value, session)?;
    for (text, maximum) in [(kind, 48), (id, 104), (digest, 71)] {
        count(text.len(), 1, maximum, session)?;
    }
    let mut bytes = std::mem::size_of::<ExactRef>();
    for text in [kind, id, digest] {
        bytes = arithmetic(bytes, text.len(), Op::Add, session)?;
    }
    let lease = session
        .reserve_external_payload(bytes)
        .map_err(core_error)?;
    let mut copy = |text: &str| -> Result<String> {
        let work = session.semantic_work();
        work.scan_bytes(text.len()).map_err(core_error)?;
        work.write_bytes(text.len()).map_err(core_error)?;
        Ok(text.to_owned())
    };
    Ok((
        ExactRef {
            kind: copy(kind)?,
            id: copy(id)?,
            digest: copy(digest)?,
        },
        lease,
    ))
}
async fn record(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    reference: &Value,
    kind: &str,
    representation: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<Record> {
    member(reference, origin.wrappers().manifest(), session)?;
    request_wrapper::same_text(
        request_wrapper::text(reference, "kind", session)?,
        kind,
        session,
    )?;
    let (reference, lease) = owned_reference(reference, session)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Add,
            &[&origin.wrappers().sequence().into(), &1i64.into()],
        )
        .map_err(core_error)?;
    let before = origin
        .wrappers()
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let stored = tx
        .workflow_financial_document(
            &reference,
            &origin.key().logical_store_id,
            &origin.key().scope,
            before,
            session,
        )
        .await
        .map_err(read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let parsed = session
        .parse_representation(stored.body.bytes(), representation)
        .map_err(core_error)?;
    let encoded = session
        .canonical_representation(parsed.value(), representation)
        .map_err(core_error)?;
    same_bytes(stored.body.bytes(), encoded.bytes(), session)?;
    same_bytes(
        stored.schema.bytes(),
        request_wrapper::text(parsed.value(), "schema", session)?.as_bytes(),
        session,
    )?;
    Ok(Record {
        parsed,
        stored,
        reference,
        _reference_lease: lease,
    })
}
fn update(hash: &mut Sha256, bytes: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    let work = session.semantic_work();
    work.scan_bytes(bytes.len()).map_err(core_error)?;
    work.hash_bytes(bytes.len()).map_err(core_error)?;
    hash.update(bytes);
    Ok(())
}
fn update_value(
    hash: &mut Sha256,
    value: &Value,
    kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let bytes = session
        .canonical_representation(value, kind)
        .map_err(core_error)?;
    update(hash, bytes.bytes(), session)
}
fn digest(
    reference: &ExactRef,
    hash: [u8; 32],
    prefix: Option<&str>,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    request_wrapper::digest_text(&hash, &reference.digest, true, session)?;
    if let Some(prefix) = prefix {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(prefix.len())
            .map_err(core_error)?;
        let hex = reference
            .id
            .strip_prefix(prefix)
            .ok_or(ServiceError::IntegrityFailure)?;
        request_wrapper::digest_text(&hash, hex, false, session)?;
    }
    Ok(())
}
fn source_identity(record: &Record, session: &mut OriginalAuditSession) -> Result<()> {
    let mut hash = Sha256::new();
    update(&mut hash, b"bean-counter/workflow-document/1\0[", session)?;
    update_value(
        &mut hash,
        request_wrapper::field(record.parsed.value(), "schema", session)?,
        RepresentationKind::SourceDefinition,
        session,
    )?;
    update(&mut hash, b",", session)?;
    update(&mut hash, record.stored.body.bytes(), session)?;
    update(&mut hash, b"]", session)?;
    digest(
        &record.reference,
        hash.finalize().into(),
        Some("wdoc_"),
        session,
    )
}
fn definition(value: &Value, session: &mut OriginalAuditSession) -> Result<DefinitionRef> {
    let (d, i, k) = parts(value, session)?;
    DefinitionRef::new_with_work(k, i, d, session.semantic_work()).map_err(core_error)
}
fn expected_kind(schema: &str, session: &mut OriginalAuditSession) -> Result<&'static str> {
    for (candidate, kind) in [
        ("ledger-workflow-policy/1", "workflow-policy"),
        ("ledger-workflow-policy/2", "workflow-policy"),
        ("ledger-workflow-policy/3", "workflow-policy"),
        ("ledger-workflow-policy/4", "workflow-policy"),
        ("ledger-workflow-product/1", "workflow-product"),
        ("ledger-workflow-fact-schema/1", "workflow-fact-schema"),
        ("ledger-workflow-slot-template/1", "workflow-slot-template"),
        (
            "ledger-workflow-compiler-descriptor/1",
            "workflow-compiler-descriptor",
        ),
        (
            "ledger-workflow-compiler-descriptor/2",
            "workflow-compiler-descriptor",
        ),
        (
            "ledger-workflow-compiler-descriptor/3",
            "workflow-compiler-descriptor",
        ),
        (
            "ledger-workflow-compiler-descriptor/4",
            "workflow-compiler-descriptor",
        ),
        (
            "ledger-workflow-compiler-descriptor/5",
            "workflow-compiler-descriptor",
        ),
        (
            "ledger-workflow-compiler-descriptor/6",
            "workflow-compiler-descriptor",
        ),
    ] {
        if text_is(schema, candidate, session)? {
            return Ok(kind);
        }
    }
    Err(reject("HISTORICAL_VERIFIER_UNAVAILABLE"))
}
fn copied(bytes: &[u8], session: &mut OriginalAuditSession) -> Result<Vec<u8>> {
    session
        .semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .write_bytes(bytes.len())
        .map_err(core_error)?;
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| ServiceError::Unavailable)?;
    out.extend_from_slice(bytes);
    Ok(out)
}
/// Recompile from literal inline source values, or exact historically retained
/// source operands. The saved plan is comparison-only. Never current support.
pub(super) async fn recompile(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    session: &mut OriginalAuditSession,
) -> Result<RecompiledPublication> {
    let result = recompile_inner(tx, origin, session).await;
    if result.is_err() {
        session.invalidate();
    }
    result
}
async fn recompile_inner(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    session: &mut OriginalAuditSession,
) -> Result<RecompiledPublication> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        origin.wrappers().class() == wrappers::WireClass::Publication,
        "INTEGRITY_FAILURE",
    )?;
    let op = origin.wrappers().operation();
    let results = request_wrapper::field(op, "result_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(results.len(), 2, 2, session)?;
    let (mut publication_ref, mut plan_ref) = (None, None);
    for r in results {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        match request_wrapper::text(r, "kind", session)? {
            "workflow-publication" => {
                require(publication_ref.replace(r).is_none(), "INTEGRITY_FAILURE")?;
            }
            "workflow-compiled-plan" => {
                require(plan_ref.replace(r).is_none(), "INTEGRITY_FAILURE")?;
            }
            _ => return Err(ServiceError::IntegrityFailure),
        }
    }
    let publication = record(
        tx,
        origin,
        publication_ref.ok_or(ServiceError::IntegrityFailure)?,
        "workflow-publication",
        RepresentationKind::SourceClosure,
        session,
    )
    .await?;
    let body = publication.parsed.value();
    request_wrapper::closed(
        body,
        &[
            "schema",
            "root_policy_ref",
            "source_refs",
            "compiled_plan_ref",
            "compiler_ref",
            "provenance",
        ],
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(body, "schema", session)?,
        "ledger-workflow-publication/1",
        session,
    )?;
    same_value(
        request_wrapper::field(body, "compiled_plan_ref", session)?,
        plan_ref.ok_or(ServiceError::IntegrityFailure)?,
        session,
    )?;
    let plan = record(
        tx,
        origin,
        plan_ref.ok_or(ServiceError::IntegrityFailure)?,
        "workflow-compiled-plan",
        RepresentationKind::Record,
        session,
    )
    .await?;
    let mut plan_hash = Sha256::new();
    update(&mut plan_hash, b"bean-counter/workflow-plan/1\0", session)?;
    update(&mut plan_hash, plan.stored.body.bytes(), session)?;
    digest(
        &plan.reference,
        plan_hash.finalize().into(),
        Some("wplan_"),
        session,
    )?;
    // Publication ID is a scoped root-policy identity; its digest commits the
    // complete retained body. Neither is the policy's document digest.
    let key_value = request_wrapper::field(op, "key", session)?;
    let mut identity = Sha256::new();
    update(
        &mut identity,
        b"bean-counter/workflow-publication/1\0[",
        session,
    )?;
    update_value(
        &mut identity,
        request_wrapper::field(key_value, "logical_store_id", session)?,
        RepresentationKind::Record,
        session,
    )?;
    update(&mut identity, b",", session)?;
    update_value(
        &mut identity,
        request_wrapper::field(key_value, "scope", session)?,
        RepresentationKind::Record,
        session,
    )?;
    update(&mut identity, b",", session)?;
    update_value(
        &mut identity,
        request_wrapper::field(body, "root_policy_ref", session)?,
        RepresentationKind::Record,
        session,
    )?;
    update(&mut identity, b"]", session)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let id_hex = publication
        .reference
        .id
        .strip_prefix("wpub_")
        .ok_or(ServiceError::IntegrityFailure)?;
    request_wrapper::digest_text(&identity.finalize().into(), id_hex, false, session)?;
    let mut publication_hash = Sha256::new();
    update(
        &mut publication_hash,
        b"bean-counter/workflow-record/1\0[\"workflow-publication\",",
        session,
    )?;
    update_value(
        &mut publication_hash,
        request_wrapper::field(key_value, "scope", session)?,
        RepresentationKind::Record,
        session,
    )?;
    update(&mut publication_hash, b",", session)?;
    update(
        &mut publication_hash,
        publication.stored.body.bytes(),
        session,
    )?;
    update(&mut publication_hash, b"]", session)?;
    digest(
        &publication.reference,
        publication_hash.finalize().into(),
        None,
        session,
    )?;
    let source_refs = request_wrapper::field(body, "source_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(source_refs.len(), 1, 128, session)?;
    let source_control = arithmetic(
        arithmetic(
            source_refs.len(),
            std::mem::size_of::<Record>(),
            Op::Multiply,
            session,
        )?,
        std::mem::size_of::<Vec<Record>>(),
        Op::Add,
        session,
    )?;
    let source_control = session
        .reserve_external_payload(source_control)
        .map_err(core_error)?;
    let mut sources = Vec::new();
    sources
        .try_reserve_exact(source_refs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    let mut total = 0usize;
    let mut previous = None;
    for reference in source_refs {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        if let Some(prior) = previous {
            require(
                reference_cmp(prior, reference, session)?.is_lt(),
                "INTEGRITY_FAILURE",
            )?;
        }
        previous = Some(reference);
        let kind = request_wrapper::text(reference, "kind", session)?;
        let source = record(
            tx,
            origin,
            reference,
            kind,
            RepresentationKind::SourceDefinition,
            session,
        )
        .await?;
        source_identity(&source, session)?;
        let schema = request_wrapper::text(source.parsed.value(), "schema", session)?;
        request_wrapper::same_text(kind, expected_kind(schema, session)?, session)?;
        total = arithmetic(total, source.stored.body.bytes().len(), Op::Add, session)?;
        count(total, 1, 1048576, session)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<Record>())
            .map_err(core_error)?;
        sources.push(source);
    }
    let descriptor_ref = request_wrapper::field(body, "compiler_ref", session)?;
    let mut descriptor = None;
    for source in &sources {
        session
            .semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let (d, i, k) = parts(descriptor_ref, session)?;
        if same_ref(&source.reference, k, i, d, session)? {
            descriptor = Some(source);
            break;
        }
    }
    let descriptor = descriptor.ok_or(ServiceError::IntegrityFailure)?;
    request_wrapper::same_text(
        &descriptor.reference.kind,
        "workflow-compiler-descriptor",
        session,
    )?;
    let raw = request_wrapper::field(
        origin.wrappers().original_request().value(),
        "payload",
        session,
    )?;
    let normalized = request_wrapper::field(
        request_wrapper::field(
            request_wrapper::field(op, "request_evidence", session)?,
            "normalized_request",
            session,
        )?,
        "payload",
        session,
    )?;
    let mode = request_wrapper::text(raw, "mode", session)?;
    let field = if text_is(mode, "inline", session)? {
        "documents"
    } else if text_is(mode, "retained", session)? {
        "document_refs"
    } else {
        return Err(ServiceError::IntegrityFailure);
    };
    for payload in [raw, normalized] {
        request_wrapper::closed(
            payload,
            &[
                "schema",
                "mode",
                field,
                "root_policy_ref",
                "expected_plan_digest",
            ],
            session,
        )?;
    }
    for name in ["schema", "mode", "root_policy_ref", "expected_plan_digest"] {
        same_value(
            request_wrapper::field(raw, name, session)?,
            request_wrapper::field(normalized, name, session)?,
            session,
        )?;
    }
    same_value(
        request_wrapper::field(raw, "root_policy_ref", session)?,
        request_wrapper::field(body, "root_policy_ref", session)?,
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(raw, "expected_plan_digest", session)?,
        &plan.reference.digest,
        session,
    )?;
    let raw_documents = request_wrapper::field(raw, field, session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(
        raw_documents.len(),
        source_refs.len(),
        source_refs.len(),
        session,
    )?;
    let mut input_bytes = arithmetic(
        raw_documents.len(),
        std::mem::size_of::<Vec<u8>>(),
        Op::Multiply,
        session,
    )?;
    input_bytes = arithmetic(
        input_bytes,
        std::mem::size_of::<SelectedSourceInput>(),
        Op::Add,
        session,
    )?;
    for source in &sources {
        input_bytes = arithmetic(
            input_bytes,
            source.stored.body.bytes().len(),
            Op::Add,
            session,
        )?;
    }
    // Inline raw source spelling can be longer than normalized source. Its
    // original enclosing request is already bounded256KiB; reserve that whole
    // payload in addition to the retained source budget before copying either.
    if mode == "inline" {
        input_bytes = arithmetic(input_bytes, 262144, Op::Add, session)?;
    }
    let (d, i, k) = parts(
        request_wrapper::field(body, "root_policy_ref", session)?,
        session,
    )?;
    for text in [d, i, k] {
        input_bytes = arithmetic(input_bytes, text.len(), Op::Add, session)?;
    }
    let compiler_input = session
        .reserve_external_payload(input_bytes)
        .map_err(core_error)?;
    let root = definition(
        request_wrapper::field(body, "root_policy_ref", session)?,
        session,
    )?;
    let mut documents = Vec::new();
    documents
        .try_reserve_exact(raw_documents.len())
        .map_err(|_| ServiceError::Unavailable)?;
    if mode == "inline" {
        for source in raw_documents {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let raw = session
                .canonical_representation(source, RepresentationKind::SourceDefinition)
                .map_err(core_error)?;
            documents.push(copied(raw.bytes(), session)?);
        }
    } else {
        equal_ref_sets(raw_documents, source_refs, session)?;
        for source in &sources {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            documents.push(copied(source.stored.body.bytes(), session)?);
        }
    }
    let program = restore_selected_program(
        SelectedSourceInput {
            documents,
            root_policy_ref: root,
        },
        descriptor.stored.body.bytes(),
        plan.stored.body.bytes(),
        session,
    )
    .map_err(core_error)?;
    count(
        program.source_documents().len(),
        sources.len(),
        sources.len(),
        session,
    )?;
    for source in program.source_documents() {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let mut found = None;
        for retained in &sources {
            session
                .semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            if same_ref(
                &retained.reference,
                source.reference().kind(),
                source.reference().id(),
                source.reference().digest(),
                session,
            )? {
                found = Some(retained);
                break;
            }
        }
        same_bytes(
            source.canonical_bytes(),
            found
                .ok_or(ServiceError::IntegrityFailure)?
                .stored
                .body
                .bytes(),
            session,
        )?;
    }
    if mode == "inline" {
        let normalized_sources = request_wrapper::field(normalized, "documents", session)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        count(
            normalized_sources.len(),
            program.source_documents().len(),
            program.source_documents().len(),
            session,
        )?;
        for (expected, actual) in program.source_documents().iter().zip(normalized_sources) {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            same_value(expected.canonical_value(), actual, session)?;
        }
    } else {
        let normalized_refs = request_wrapper::field(normalized, "document_refs", session)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        count(
            normalized_refs.len(),
            source_refs.len(),
            source_refs.len(),
            session,
        )?;
        for (actual, expected) in normalized_refs.iter().zip(source_refs) {
            require(
                reference_cmp(actual, expected, session)?.is_eq(),
                "INTEGRITY_FAILURE",
            )?;
        }
    }
    let (build, authority) =
        original_metadata(tx, origin, &publication, &descriptor.reference, session).await?;
    validate_descriptors(origin, &program, &sources, &publication, session)?;
    Ok(RecompiledPublication {
        program,
        publication,
        plan,
        build,
        authority,
        sources,
        _source_control: source_control,
        _compiler_input: compiler_input,
    })
}
fn same_ref(
    reference: &ExactRef,
    kind: &str,
    id: &str,
    digest: &str,
    session: &mut OriginalAuditSession,
) -> Result<bool> {
    for (a, b) in [
        (&reference.kind, kind),
        (&reference.id, id),
        (&reference.digest, digest),
    ] {
        let work = session.semantic_work();
        work.visit(Visit::MemberCompare).map_err(core_error)?;
        work.scan_bytes(a.len()).map_err(core_error)?;
        work.scan_bytes(b.len()).map_err(core_error)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
fn equal_ref_sets(
    left: &[Value],
    right: &[Value],
    session: &mut OriginalAuditSession,
) -> Result<()> {
    count(left.len(), right.len(), right.len(), session)?;
    let bytes = arithmetic(
        arithmetic(
            left.len(),
            std::mem::size_of::<&Value>(),
            Op::Multiply,
            session,
        )?,
        std::mem::size_of::<Vec<&Value>>(),
        Op::Add,
        session,
    )?;
    let _lease = session
        .reserve_external_payload(bytes)
        .map_err(core_error)?;
    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(left.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for value in left {
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<&Value>())
            .map_err(core_error)?;
        ordered.push(value);
    }
    // A fallible in-place insertion sort of at most128 locators. No cloned
    // canonical Ref graph or unaccounted BTree comparisons are used.
    for index in 1..ordered.len() {
        let mut j = index;
        loop {
            session
                .semantic_work()
                .arithmetic(Op::Compare, &[&j.into(), &0usize.into()])
                .map_err(core_error)?;
            if j == 0 {
                break;
            }
            session
                .semantic_work()
                .arithmetic(Op::Subtract, &[&j.into(), &1usize.into()])
                .map_err(core_error)?;
            let prior = j - 1;
            if !reference_cmp(ordered[prior], ordered[j], session)?.is_gt() {
                break;
            }
            session
                .semantic_work()
                .visit(Visit::SortMove)
                .map_err(core_error)?;
            session
                .semantic_work()
                .write_bytes(2 * std::mem::size_of::<&Value>())
                .map_err(core_error)?;
            ordered.swap(prior, j);
            j = prior;
        }
    }
    for (a, b) in ordered.iter().zip(right) {
        require(reference_cmp(a, b, session)?.is_eq(), "INTEGRITY_FAILURE")?;
    }
    Ok(())
}

fn bounded_text<'a>(
    value: &'a Value,
    name: &str,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<&'a str> {
    let text = request_wrapper::text(value, name, session)?;
    count(text.len(), 1, maximum, session)?;
    session
        .semantic_work()
        .scan_bytes(text.len())
        .map_err(core_error)?;
    for character in text.chars() {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(!character.is_control(), "INTEGRITY_FAILURE")?;
    }
    Ok(text)
}
fn hex_shape(
    value: &str,
    prefix: &str,
    length: usize,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(prefix.len())
        .map_err(core_error)?;
    let value = value
        .strip_prefix(prefix)
        .ok_or(ServiceError::IntegrityFailure)?;
    count(value.len(), length, length, session)?;
    for byte in value.bytes() {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        session.semantic_work().scan_bytes(1).map_err(core_error)?;
        require(
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
            "INTEGRITY_FAILURE",
        )?;
    }
    Ok(())
}
fn positive_counter(
    value: &str,
    positive: bool,
    session: &mut OriginalAuditSession,
) -> Result<i64> {
    count(value.len(), 1, 19, session)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(value == "0" || !value.starts_with('0'), "INTEGRITY_FAILURE")?;
    let mut n = 0i64;
    for byte in value.bytes() {
        session
            .semantic_work()
            .visit(Visit::ScalarDigit)
            .map_err(core_error)?;
        require(byte.is_ascii_digit(), "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&n.into(), &10i64.into()])
            .map_err(core_error)?;
        n = n.checked_mul(10).ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&byte.into(), &b'0'.into()])
            .map_err(core_error)?;
        let digit = byte - b'0';
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &digit.into()])
            .map_err(core_error)?;
        n = n
            .checked_add(i64::from(digit))
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    if positive {
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&n.into(), &0i64.into()])
            .map_err(core_error)?;
        require(n > 0, "INTEGRITY_FAILURE")?;
    }
    Ok(n)
}
fn string_array(
    value: &Value,
    maximum: usize,
    text_max: usize,
    ascii_sorted: bool,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let array = value.as_array().ok_or(ServiceError::IntegrityFailure)?;
    count(array.len(), 0, maximum, session)?;
    let mut prior: Option<&str> = None;
    for item in array {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        let text = item.as_str().ok_or(ServiceError::IntegrityFailure)?;
        count(text.len(), 1, text_max, session)?;
        session
            .semantic_work()
            .scan_bytes(text.len())
            .map_err(core_error)?;
        for character in text.chars() {
            session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(!character.is_control(), "INTEGRITY_FAILURE")?;
        }
        if ascii_sorted {
            session
                .semantic_work()
                .scan_bytes(text.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(text.is_ascii(), "INTEGRITY_FAILURE")?;
            if let Some(previous) = prior {
                let work = session.semantic_work();
                work.visit(Visit::MemberCompare).map_err(core_error)?;
                work.scan_bytes(previous.len()).map_err(core_error)?;
                work.scan_bytes(text.len()).map_err(core_error)?;
                require(previous < text, "INTEGRITY_FAILURE")?;
            }
        }
        prior = Some(text);
    }
    Ok(())
}
async fn original_metadata(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    publication: &Record,
    descriptor: &ExactRef,
    session: &mut OriginalAuditSession,
) -> Result<(Record, Record)> {
    let op = origin.wrappers().operation();
    let manifest = origin.wrappers().manifest();
    let members = request_wrapper::field(manifest, "members", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let source_count = request_wrapper::field(publication.parsed.value(), "source_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
        .len();
    let expected_members = arithmetic(source_count, 5, Op::Add, session)?;
    count(members.len(), expected_members, expected_members, session)?;
    let mut build_ref = None;
    for member in members {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let kind = request_wrapper::text(member, "kind", session)?;
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(kind.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes("workflow-build-manifest".len())
            .map_err(core_error)?;
        if kind == "workflow-build-manifest" {
            require(build_ref.replace(member).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    let build = record(
        tx,
        origin,
        build_ref.ok_or(ServiceError::IntegrityFailure)?,
        "workflow-build-manifest",
        RepresentationKind::Authority,
        session,
    )
    .await?;
    source_identity(&build, session)?;
    let manifest = build.parsed.value();
    request_wrapper::closed(manifest, &["schema", "build_inputs", "executable"], session)?;
    request_wrapper::same_text(
        request_wrapper::text(manifest, "schema", session)?,
        "ledger-workflow-build-manifest/1",
        session,
    )?;
    let inputs = request_wrapper::field(manifest, "build_inputs", session)?;
    request_wrapper::closed(inputs, &["schema", "source", "recipe"], session)?;
    request_wrapper::same_text(
        request_wrapper::text(inputs, "schema", session)?,
        "ledger-workflow-build-inputs/1",
        session,
    )?;
    let source = request_wrapper::field(inputs, "source", session)?;
    request_wrapper::closed(
        source,
        &[
            "base_commit",
            "state",
            "inventory_digest",
            "file_count",
            "total_bytes",
        ],
        session,
    )?;
    hex_shape(
        request_wrapper::text(source, "base_commit", session)?,
        "",
        40,
        session,
    )?;
    let state = request_wrapper::text(source, "state", session)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        matches!(state, "clean_commit" | "worktree_snapshot"),
        "INTEGRITY_FAILURE",
    )?;
    hex_shape(
        request_wrapper::text(source, "inventory_digest", session)?,
        "sha256:",
        64,
        session,
    )?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let file_count = request_wrapper::field(source, "file_count", session)?
        .as_u64()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(
        usize::try_from(file_count).map_err(|_| ServiceError::IntegrityFailure)?,
        1,
        8192,
        session,
    )?;
    positive_counter(
        request_wrapper::text(source, "total_bytes", session)?,
        false,
        session,
    )?;
    let recipe = request_wrapper::field(inputs, "recipe", session)?;
    request_wrapper::closed(
        recipe,
        &[
            "package_name",
            "package_version",
            "build_profile",
            "target",
            "features",
            "rustc_version",
            "cargo_version",
            "rustflags",
            "cargo_lock_digest",
            "compiler_ref",
        ],
        session,
    )?;
    for field in ["package_name", "package_version", "build_profile", "target"] {
        bounded_text(recipe, field, 256, session)?;
    }
    for field in ["rustc_version", "cargo_version"] {
        bounded_text(recipe, field, 4096, session)?;
    }
    string_array(
        request_wrapper::field(recipe, "features", session)?,
        128,
        256,
        true,
        session,
    )?;
    string_array(
        request_wrapper::field(recipe, "rustflags", session)?,
        128,
        4096,
        false,
        session,
    )?;
    hex_shape(
        request_wrapper::text(recipe, "cargo_lock_digest", session)?,
        "sha256:",
        64,
        session,
    )?;
    let (d, i, k) = parts(
        request_wrapper::field(recipe, "compiler_ref", session)?,
        session,
    )?;
    require(same_ref(descriptor, k, i, d, session)?, "INTEGRITY_FAILURE")?;
    let executable = request_wrapper::field(manifest, "executable", session)?;
    request_wrapper::closed(executable, &["sha256", "bytes", "observation"], session)?;
    hex_shape(
        request_wrapper::text(executable, "sha256", session)?,
        "",
        64,
        session,
    )?;
    positive_counter(
        request_wrapper::text(executable, "bytes", session)?,
        false,
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(executable, "observation", session)?,
        "stable-current-executable-file/1",
        session,
    )?;
    let projection = request_wrapper::field(publication.parsed.value(), "provenance", session)?;
    request_wrapper::closed(
        projection,
        &["source_sha", "binary_sha256", "build_manifest_digest"],
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(projection, "source_sha", session)?,
        request_wrapper::text(source, "base_commit", session)?,
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(projection, "binary_sha256", session)?,
        request_wrapper::text(executable, "sha256", session)?,
        session,
    )?;
    let raw_hash = session
        .hash_representation(build.stored.body.bytes(), RepresentationKind::Authority)
        .map_err(core_error)?;
    request_wrapper::digest_text(
        &raw_hash,
        request_wrapper::text(projection, "build_manifest_digest", session)?,
        true,
        session,
    )?;
    let observed = request_wrapper::field(op, "authority_snapshot", session)?;
    request_wrapper::closed(
        observed,
        &[
            "schema",
            "principal_id",
            "source",
            "authority_refs",
            "rights",
            "validated_at",
        ],
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(observed, "schema", session)?,
        "ledger-workflow-publication-authority/1",
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(observed, "source", session)?,
        &origin.key().source,
        session,
    )?;
    same_value(
        request_wrapper::field(observed, "validated_at", session)?,
        request_wrapper::field(op, "accepted_at", session)?,
        session,
    )?;
    let rights = request_wrapper::field(observed, "rights", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(rights.len(), 1, 1, session)?;
    request_wrapper::same_text(
        rights[0].as_str().ok_or(ServiceError::IntegrityFailure)?,
        "admin",
        session,
    )?;
    let refs = request_wrapper::field(observed, "authority_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(refs.len(), 1, 1, session)?;
    let authority = record(
        tx,
        origin,
        &refs[0],
        "workflow-authority",
        RepresentationKind::Authority,
        session,
    )
    .await?;
    source_identity(&authority, session)?;
    let root = authority.parsed.value();
    request_wrapper::closed(
        root,
        &[
            "schema",
            "basis",
            "basis_digest",
            "verification_profile",
            "verified_at",
        ],
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(root, "schema", session)?,
        "ledger-workflow-local-authority-snapshot/1",
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(root, "verification_profile", session)?,
        "local-private-directory-owner/1",
        session,
    )?;
    let basis = request_wrapper::field(root, "basis", session)?;
    request_wrapper::closed(
        basis,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "rights",
            "source_grant_revision",
        ],
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(basis, "schema", session)?,
        "ledger-workflow-local-authority-basis/1",
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(basis, "logical_store_id", session)?,
        &origin.key().logical_store_id,
        session,
    )?;
    same_value(
        request_wrapper::field(basis, "scope", session)?,
        request_wrapper::field(
            request_wrapper::field(op, "key", session)?,
            "scope",
            session,
        )?,
        session,
    )?;
    let actor = bounded_text(basis, "principal_id", 128, session)?;
    request_wrapper::same_text(
        actor,
        request_wrapper::text(observed, "principal_id", session)?,
        session,
    )?;
    request_wrapper::same_text(
        bounded_text(basis, "source", 256, session)?,
        &origin.key().source,
        session,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(basis, "canonical_economic_namespace", session)?,
        &origin.key().source,
        session,
    )?;
    let rights = request_wrapper::field(basis, "rights", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    count(rights.len(), 2, 2, session)?;
    for (actual, expected) in rights.iter().zip(["admin", "read"]) {
        request_wrapper::same_text(
            actual.as_str().ok_or(ServiceError::IntegrityFailure)?,
            expected,
            session,
        )?;
    }
    positive_counter(
        request_wrapper::text(basis, "source_grant_revision", session)?,
        true,
        session,
    )?;
    let mut hash = Sha256::new();
    update(
        &mut hash,
        b"bean-counter/local-authority-basis/1\0",
        session,
    )?;
    update_value(&mut hash, basis, RepresentationKind::Authority, session)?;
    request_wrapper::digest_text(
        &hash.finalize().into(),
        request_wrapper::text(root, "basis_digest", session)?,
        true,
        session,
    )?;
    let at = Timestamp::parse_with_work(
        request_wrapper::text(root, "verified_at", session)?,
        session.semantic_work(),
    )
    .map_err(core_error)?;
    let accepted = Timestamp::parse_with_work(
        request_wrapper::text(op, "accepted_at", session)?,
        session.semantic_work(),
    )
    .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&at.micros().into(), &accepted.micros().into()],
        )
        .map_err(core_error)?;
    require(at.micros() == accepted.micros(), "INTEGRITY_FAILURE")?;
    Ok((build, authority))
}

struct EncodedSet {
    items: Vec<ledgerlab_core::canonical::audit_arena::AuditedBytes>,
    maximum: usize,
    _lease: ExternalBufferLease,
}
impl EncodedSet {
    fn new(maximum: usize, session: &mut OriginalAuditSession) -> Result<Self> {
        let bytes = arithmetic(
            arithmetic(
                maximum,
                std::mem::size_of::<ledgerlab_core::canonical::audit_arena::AuditedBytes>(),
                Op::Multiply,
                session,
            )?,
            std::mem::size_of::<Vec<ledgerlab_core::canonical::audit_arena::AuditedBytes>>(),
            Op::Add,
            session,
        )?;
        let lease = session
            .reserve_external_payload(bytes)
            .map_err(core_error)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(maximum)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            items,
            maximum,
            _lease: lease,
        })
    }
    fn push(
        &mut self,
        value: ledgerlab_core::canonical::audit_arena::AuditedBytes,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&self.items.len().into(), &self.maximum.into()],
            )
            .map_err(core_error)?;
        require(self.items.len() < self.maximum, "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of_val(&value))
            .map_err(core_error)?;
        self.items.push(value);
        Ok(())
    }
    fn encode(
        mut self,
        session: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        for index in 1..self.items.len() {
            let mut j = index;
            loop {
                session
                    .semantic_work()
                    .arithmetic(Op::Compare, &[&j.into(), &0usize.into()])
                    .map_err(core_error)?;
                if j == 0 {
                    break;
                }
                session
                    .semantic_work()
                    .arithmetic(Op::Subtract, &[&j.into(), &1usize.into()])
                    .map_err(core_error)?;
                let prior = j - 1;
                let left = self.items[prior].bytes();
                let right = self.items[j].bytes();
                let work = session.semantic_work();
                work.visit(Visit::MemberCompare).map_err(core_error)?;
                work.scan_bytes(left.len()).map_err(core_error)?;
                work.scan_bytes(right.len()).map_err(core_error)?;
                match left.cmp(right) {
                    std::cmp::Ordering::Equal => return Err(ServiceError::IntegrityFailure),
                    std::cmp::Ordering::Less => break,
                    std::cmp::Ordering::Greater => {}
                }
                work.visit(Visit::SortMove).map_err(core_error)?;
                work.write_bytes(2 * std::mem::size_of_val(&self.items[j]))
                    .map_err(core_error)?;
                self.items.swap(prior, j);
                j = prior;
            }
        }
        let mut length = 2usize;
        for item in &self.items {
            length = arithmetic(length, item.bytes().len(), Op::Add, session)?;
            length = arithmetic(length, 1, Op::Add, session)?;
        }
        count(length, 2, 8388608, session)?;
        let mut output = {
            let (arena, work) = session.parts();
            arena.byte_builder(length, work).map_err(core_error)?
        };
        output
            .append(b"[", session.semantic_work())
            .map_err(core_error)?;
        for (index, item) in self.items.iter().enumerate() {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            session
                .semantic_work()
                .arithmetic(Op::Compare, &[&index.into(), &0usize.into()])
                .map_err(core_error)?;
            if index != 0 {
                output
                    .append(b",", session.semantic_work())
                    .map_err(core_error)?;
            }
            output
                .append(item.bytes(), session.semantic_work())
                .map_err(core_error)?;
        }
        output
            .append(b"]", session.semantic_work())
            .map_err(core_error)?;
        output.finish(session.semantic_work()).map_err(core_error)
    }
}
struct RefInputs<'a> {
    items: Vec<&'a ExactRef>,
    maximum: usize,
    _lease: ExternalBufferLease,
}
impl<'a> RefInputs<'a> {
    fn new(maximum: usize, session: &mut OriginalAuditSession) -> Result<Self> {
        let bytes = arithmetic(
            arithmetic(
                maximum,
                std::mem::size_of::<&ExactRef>(),
                Op::Multiply,
                session,
            )?,
            std::mem::size_of::<Vec<&ExactRef>>(),
            Op::Add,
            session,
        )?;
        let lease = session
            .reserve_external_payload(bytes)
            .map_err(core_error)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(maximum)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            items,
            maximum,
            _lease: lease,
        })
    }
    fn push(&mut self, r: &'a ExactRef, session: &mut OriginalAuditSession) -> Result<()> {
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&self.items.len().into(), &self.maximum.into()],
            )
            .map_err(core_error)?;
        require(self.items.len() < self.maximum, "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<&ExactRef>())
            .map_err(core_error)?;
        self.items.push(r);
        Ok(())
    }
    fn encode(
        &self,
        session: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        super::super::wire::RefSet::new(
            self.items.iter().copied(),
            self.items.len(),
            false,
            session,
        )?
        .encode(session)
    }
}
fn text_is(left: &str, right: &str, session: &mut OriginalAuditSession) -> Result<bool> {
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(left.len()).map_err(core_error)?;
    work.scan_bytes(right.len()).map_err(core_error)?;
    Ok(left == right)
}
fn source_for<'a>(
    reference: &Value,
    sources: &'a [Record],
    session: &mut OriginalAuditSession,
) -> Result<&'a Record> {
    let (d, i, k) = parts(reference, session)?;
    for source in sources {
        session
            .semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        if same_ref(&source.reference, k, i, d, session)? {
            return Ok(source);
        }
    }
    Err(ServiceError::IntegrityFailure)
}
fn validate_descriptors(
    origin: &LocatedWrappers,
    program: &SelectedProgram,
    sources: &[Record],
    publication: &Record,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let op = origin.wrappers().operation();
    let evidence = request_wrapper::field(op, "request_evidence", session)?;
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", session)?;
    let normalized = request_wrapper::field(
        request_wrapper::field(evidence, "normalized_request", session)?,
        "payload",
        session,
    )?;
    let payload_schema = request_wrapper::text(normalized, "schema", session)?;
    let registry = request_wrapper::text(resolution, "registry_digest", session)?;
    // Dispatch the original binary-owned execution recipe. A descriptor schema,
    // current builtin, or an arbitrary saved registry cannot select support.
    if let Some(expected) = program.execution_registry() {
        request_wrapper::same_text(registry, expected, session)?;
        let expected_payload = match expected {
            "sha256:efd33a606f95b0b8614fe657d13a617620c8c33329f98207cbfc6e70376f9c93"
            | "sha256:8ebed26695ae3594d40f91d733839bddcc31b94dcb688b2aa5890af7775af0da"
            | "sha256:d3fd78be2a734b4096c60f03869ff4da366a8fbf5f505b61d1188bbf420cd2d2" => {
                "ledger-workflow-policy-publication/3"
            }
            "sha256:b6b4aa2542bb571fe45142e5f1fbc7369aef95ee2a7c181c236496466571245f" => {
                "ledger-workflow-policy-publication/4"
            }
            "sha256:9a6379b0f0d20005f1406ca6ed3a5b55c40d275cec5435e1648d9b482332509b" => {
                "ledger-workflow-policy-publication/5"
            }
            "sha256:42368b84a652527c58389014fefa69140d6a40c2217e193694f162441699cdc2" => {
                "ledger-workflow-policy-publication/6"
            }
            _ => return Err(reject("HISTORICAL_VERIFIER_UNAVAILABLE")),
        };
        request_wrapper::same_text(payload_schema, expected_payload, session)?;
    } else {
        // Unchanged selected draft recipe has no financial capability. Restoring
        // it never upgrades that absence, even if its sources price correctly.
        request_wrapper::same_text(
            program.compiler_descriptor_ref().id(),
            "wdoc_f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b",
            session,
        )?;
        request_wrapper::same_text(
            payload_schema,
            "ledger-workflow-policy-publication/1",
            session,
        )?;
        let frozen = session
            .parse_representation(
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../contracts/candidates/full-suite-v1/core/registry-contract.json"
                )),
                RepresentationKind::SourceClosure,
            )
            .map_err(core_error)?;
        let canonical = session
            .canonical_representation(frozen.value(), RepresentationKind::SourceClosure)
            .map_err(core_error)?;
        let hash = session
            .hash_representation(canonical.bytes(), RepresentationKind::SourceClosure)
            .map_err(core_error)?;
        request_wrapper::digest_text(&hash, registry, true, session)?;
    }
    let mut descriptors = RefInputs::new(sources.len(), session)?;
    for source in sources {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        if text_is(&source.reference.kind, "workflow-fact-schema", session)?
            || text_is(
                &source.reference.kind,
                "workflow-compiler-descriptor",
                session,
            )?
        {
            descriptors.push(&source.reference, session)?;
        }
    }
    let expected_descriptors = descriptors.encode(session)?;
    let descriptor_values = request_wrapper::field(resolution, "descriptors", session)?;
    let actual = session
        .canonical_representation(descriptor_values, RepresentationKind::SourceClosure)
        .map_err(core_error)?;
    same_bytes(expected_descriptors.bytes(), actual.bytes(), session)?;
    let mut anchors = EncodedSet::new(sources.len(), session)?;
    let mut locators = EncodedSet::new(sources.len(), session)?;
    let inline = text_is(
        request_wrapper::text(normalized, "mode", session)?,
        "inline",
        session,
    )?;
    for source in sources {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let mut refs = RefInputs::new(17, session)?;
        if text_is(&source.reference.kind, "workflow-policy", session)? {
            let policy = source.parsed.value();
            let product = source_for(
                request_wrapper::field(policy, "product_ref", session)?,
                sources,
                session,
            )?;
            request_wrapper::same_text(&product.reference.kind, "workflow-product", session)?;
            let activities = request_wrapper::field(product.parsed.value(), "activities", session)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            count(activities.len(), 1, 16, session)?;
            for activity in activities {
                session
                    .semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let schema = source_for(
                    request_wrapper::field(activity, "fact_schema", session)?,
                    sources,
                    session,
                )?;
                request_wrapper::same_text(
                    &schema.reference.kind,
                    "workflow-fact-schema",
                    session,
                )?;
                refs.push(&schema.reference, session)?;
            }
            let context = source_for(
                request_wrapper::field(policy, "context_schema_ref", session)?,
                sources,
                session,
            )?;
            request_wrapper::same_text(&context.reference.kind, "workflow-fact-schema", session)?;
            refs.push(&context.reference, session)?;
        }
        let refs = refs.encode(session)?;
        let reference = super::super::wire::reference(&source.reference, session)?;
        let anchor = super::super::wire::object(
            &[
                ("descriptor_refs", refs.bytes()),
                ("record_ref", reference.bytes()),
                ("relation", b"\"publication_source\""),
                ("role", b"\"inline_document\""),
            ],
            1048576,
            session,
        )?;
        anchors.push(anchor, session)?;
        let locator = if inline {
            let source_value = session
                .canonical_representation(
                    source.parsed.value(),
                    RepresentationKind::SourceDefinition,
                )
                .map_err(core_error)?;
            super::super::wire::object(
                &[
                    ("inline_document", source_value.bytes()),
                    ("role", b"\"inline_document\""),
                ],
                1048576,
                session,
            )?
        } else {
            super::super::wire::object(
                &[("ref", reference.bytes()), ("role", b"\"inline_document\"")],
                1048576,
                session,
            )?
        };
        locators.push(locator, session)?;
    }
    let expected_anchors = anchors.encode(session)?;
    let anchor_values = request_wrapper::field(resolution, "anchors", session)?;
    let actual = session
        .canonical_representation(anchor_values, RepresentationKind::OperationFrame)
        .map_err(core_error)?;
    same_bytes(expected_anchors.bytes(), actual.bytes(), session)?;
    let locators = locators.encode(session)?;
    let mut hash = Sha256::new();
    update(
        &mut hash,
        b"bean-counter/descriptor-locators/1\0[\"policy.publish\",",
        session,
    )?;
    update_value(
        &mut hash,
        request_wrapper::field(normalized, "schema", session)?,
        RepresentationKind::OriginalRequest,
        session,
    )?;
    update(&mut hash, b",", session)?;
    update(&mut hash, locators.bytes(), session)?;
    update(&mut hash, b"]", session)?;
    request_wrapper::digest_text(
        &hash.finalize().into(),
        request_wrapper::text(resolution, "locator_digest", session)?,
        true,
        session,
    )?;
    // Independent field association checks, beyond complete plan bytes.
    let (d, i, k) = parts(
        request_wrapper::field(publication.parsed.value(), "compiler_ref", session)?,
        session,
    )?;
    request_wrapper::same_text(k, program.compiler_descriptor_ref().kind(), session)?;
    request_wrapper::same_text(i, program.compiler_descriptor_ref().id(), session)?;
    request_wrapper::same_text(d, program.compiler_descriptor_ref().digest(), session)?;
    let (d, i, k) = parts(
        request_wrapper::field(publication.parsed.value(), "root_policy_ref", session)?,
        session,
    )?;
    request_wrapper::same_text(k, program.root_policy_ref().kind(), session)?;
    request_wrapper::same_text(i, program.root_policy_ref().id(), session)?;
    request_wrapper::same_text(d, program.root_policy_ref().digest(), session)?;
    Ok(())
}
