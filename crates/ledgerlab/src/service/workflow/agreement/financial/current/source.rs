//! Owning original/current source operands selected by the actual held writer.
//! No legacy SourceGrant/SourceState DTO/Noop parse, current maximum revision or
//! caller authorization bit enters this path. Full original custody and core
//! ceiling/subset/rights checks remain separate mandatory obligations.
use super::*;
use crate::store::sqlite::workflow::financial::{RetainedDocument, SourceHeadLocator};
use ledgerlab_core::policy::workflow::{AgreementRef, transition_constructor::EconomicRecordView};
use sha2::{Digest, Sha256};
pub(in super::super) struct SourceRecord {
    parsed: AuditedValue,
    stored: RetainedDocument,
    reference: ExactRef,
    _reference_lease: ExternalBufferLease,
}
impl SourceRecord {
    pub(in super::super) fn body(&self) -> &Value {
        self.parsed.value()
    }
    pub(in super::super) fn reference(&self) -> &ExactRef {
        &self.reference
    }
    pub(in super::super) fn canonical_body(&self) -> &[u8] {
        self.stored.body.bytes()
    }
}
pub(in super::super) struct ObservedSource {
    original: SourceRecord,
    enrollment: SourceRecord,
    state: SourceRecord,
    grant: Option<SourceRecord>,
    head: SourceHeadLocator,
}
impl ObservedSource {
    pub(in super::super) fn enrollment_record(&self) -> &SourceRecord {
        &self.enrollment
    }
    pub(in super::super) fn enrollment_ref(&self) -> &ExactRef {
        &self.enrollment.reference
    }
    pub(in super::super) fn original_grant_ref(&self) -> &ExactRef {
        &self.original.reference
    }
    pub(in super::super) fn state_ref(&self) -> &ExactRef {
        &self.state.reference
    }
    pub(in super::super) fn current_grant_ref(&self) -> Option<&ExactRef> {
        self.grant.as_ref().map(|r| &r.reference)
    }
    pub(in super::super) fn records(&self) -> impl Iterator<Item = EconomicRecordView<'_>> {
        [&self.original, &self.enrollment, &self.state]
            .into_iter()
            .chain(self.grant.iter())
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: r.stored.body.bytes(),
            })
    }
    pub(in super::super) fn head(&self) -> &SourceHeadLocator {
        &self.head
    }
}
fn raw_text<'a>(b: &'a [u8], s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    std::str::from_utf8(b).map_err(|_| ServiceError::IntegrityFailure)
}
fn same(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "INTEGRITY_FAILURE")
}
fn equal_text(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn own(
    k: &str,
    id: &str,
    d: &str,
    s: &mut OriginalAuditSession,
) -> Result<(ExactRef, ExternalBufferLease)> {
    let mut n = std::mem::size_of::<ExactRef>();
    for part in [k, id, d] {
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&part.len().into(), &2usize.into()])
            .map_err(core_error)?;
        let size = part.len().checked_mul(2).ok_or(ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &size.into()])
            .map_err(core_error)?;
        n = n.checked_add(size).ok_or(ServiceError::Unavailable)?;
    }
    let lease = s.reserve_external_payload(n).map_err(core_error)?;
    let checked =
        AgreementRef::from_parts_with_work(k, id, d, s.semantic_work()).map_err(core_error)?;
    let mut copy = |t: &str| -> Result<String> {
        s.semantic_work().scan_bytes(t.len()).map_err(core_error)?;
        s.semantic_work().write_bytes(t.len()).map_err(core_error)?;
        let mut v = String::new();
        v.try_reserve_exact(t.len())
            .map_err(|_| ServiceError::Unavailable)?;
        v.push_str(t);
        Ok(v)
    };
    let r = ExactRef {
        kind: copy(checked.kind())?,
        id: copy(checked.id())?,
        digest: copy(checked.digest())?,
    };
    drop(checked);
    Ok((r, lease))
}
pub(in super::super) fn from_value(
    v: &Value,
    kind: &str,
    s: &mut OriginalAuditSession,
) -> Result<(ExactRef, ExternalBufferLease)> {
    // Kept as executing field access rather than an opaque Ref serde clone.
    super::super::request_wrapper::closed(v, &["kind", "id", "digest"], s)?;
    let k = super::super::request_wrapper::text(v, "kind", s)?;
    super::super::request_wrapper::same_text(k, kind, s)?;
    own(
        k,
        super::super::request_wrapper::text(v, "id", s)?,
        super::super::request_wrapper::text(v, "digest", s)?,
        s,
    )
}
fn hash_check(
    domain: &[u8],
    parts: &[&[u8]],
    expected: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let mut h = Sha256::new();
    s.semantic_work()
        .hash_bytes(domain.len())
        .map_err(core_error)?;
    h.update(domain);
    for p in parts {
        s.semantic_work().scan_bytes(p.len()).map_err(core_error)?;
        s.semantic_work().hash_bytes(p.len()).map_err(core_error)?;
        h.update(p);
    }
    super::super::request_wrapper::digest_text(&h.finalize().into(), expected, true, s)
}
async fn load(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: ExactRef,
    lease: ExternalBufferLease,
    schema: &str,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<SourceRecord> {
    let stored = tx
        .workflow_financial_document(&reference, &key.logical_store_id, &key.scope, before, s)
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    same(stored.schema.bytes(), schema.as_bytes(), s)?;
    let parsed = s
        .parse_representation(stored.body.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let encoded = s
        .canonical_representation(parsed.value(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    same(stored.body.bytes(), encoded.bytes(), s)?;
    super::super::request_wrapper::same_text(
        super::super::request_wrapper::text(parsed.value(), "schema", s)?,
        schema,
        s,
    )?;
    let schema_value = super::super::request_wrapper::field(parsed.value(), "schema", s)?;
    let schema = s
        .canonical_representation(schema_value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    if equal_text(&reference.kind, "workflow-source-state", s)? {
        let kind = super::super::wire::string(&reference.kind, s)?;
        let key_scope = super::super::request_wrapper::field(
            super::super::request_wrapper::field(parsed.value(), "operation_key", s)?,
            "scope",
            s,
        )?;
        let scope = s
            .canonical_representation(key_scope, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        hash_check(
            b"bean-counter/workflow-record/1\0",
            &[
                b"[",
                kind.bytes(),
                b",",
                scope.bytes(),
                b",",
                stored.body.bytes(),
                b"]",
            ],
            &reference.digest,
            s,
        )?;
    } else {
        hash_check(
            b"bean-counter/workflow-document/1\0",
            &[b"[", schema.bytes(), b",", stored.body.bytes(), b"]"],
            &reference.digest,
            s,
        )?;
    }
    Ok(SourceRecord {
        parsed,
        stored,
        reference,
        _reference_lease: lease,
    })
}
fn same_ref(value: &Value, reference: &ExactRef, s: &mut OriginalAuditSession) -> Result<()> {
    let a = s
        .canonical_representation(value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let b = super::super::wire::reference(reference, s)?;
    same(a.bytes(), b.bytes(), s)
}
pub(in super::super) async fn observe(
    tx: &mut SqliteTx,
    key: &OperationKey,
    original: &RecordRef,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedSource> {
    let result = observe_inner(tx, key, original, before, s).await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
async fn observe_inner(
    tx: &mut SqliteTx,
    key: &OperationKey,
    original: &RecordRef,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedSource> {
    use super::super::request_wrapper as check;
    check::same_text(&original.kind, "workflow-source-grant", s)?;
    let (r, l) = own(&original.kind, &original.id, &original.digest, s)?;
    let original = load(tx, key, r, l, "ledger-workflow-source-grant/1", before, s).await?;
    observe_original(tx, key, original, before, s).await
}
/// Same current-source observer from an owning request's exact Ref value. No
/// DTO clone/Noop parser or new meaning is introduced for acceptance callers.
pub(in super::super) async fn observe_value(
    tx: &mut SqliteTx,
    key: &OperationKey,
    value: &Value,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedSource> {
    let result = async {
        let (reference, lease) = from_value(value, "workflow-source-grant", s)?;
        let original = load(
            tx,
            key,
            reference,
            lease,
            "ledger-workflow-source-grant/1",
            before,
            s,
        )
        .await?;
        observe_original(tx, key, original, before, s).await
    }
    .await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
async fn observe_original(
    tx: &mut SqliteTx,
    key: &OperationKey,
    original: SourceRecord,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<ObservedSource> {
    use super::super::request_wrapper as check;
    let (r, l) = from_value(
        check::field(original.body(), "original_ceiling_ref", s)?,
        "workflow-source-enrollment",
        s,
    )?;
    let enrollment = load(
        tx,
        key,
        r,
        l,
        "ledger-workflow-source-enrollment/1",
        before,
        s,
    )
    .await?;
    let enrollment_id = check::text(enrollment.body(), "enrollment_id", s)?;
    check::same_text(
        check::text(original.body(), "grant_id", s)?,
        enrollment_id,
        s,
    )?;
    let scope_value = check::field(enrollment.body(), "scope", s)?;
    let scope = s
        .canonical_representation(scope_value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    // Compare scope via fixed borrowed strings, with no new JSON key clone.
    let scopes = check::field(enrollment.body(), "scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&scopes.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(scopes.len() == 2, "INTEGRITY_FAILURE")?;
    for (a, b) in scopes.iter().zip(&key.scope) {
        check::same_text(a.as_str().ok_or(ServiceError::IntegrityFailure)?, b, s)?;
    }
    let head = tx
        .workflow_financial_source_head(&key.logical_store_id, &key.scope, enrollment_id, s)
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let (r, l) = own(
        raw_text(head.kind.bytes(), s)?,
        raw_text(head.state_id.bytes(), s)?,
        raw_text(head.digest.bytes(), s)?,
        s,
    )?;
    check::same_text(&r.kind, "workflow-source-state", s)?;
    let state = load(tx, key, r, l, "ledger-workflow-source-state/1", before, s).await?;
    same_ref(
        check::field(state.body(), "enrollment_ref", s)?,
        &enrollment.reference,
        s,
    )?;
    let state_revision =
        super::super::acceptance::counter(check::text(state.body(), "revision", s)?, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&state_revision.into(), &head.revision.into()],
        )
        .map_err(core_error)?;
    require(
        state_revision as i128 == i128::from(head.revision),
        "INTEGRITY_FAILURE",
    )?;
    let status = check::text(state.body(), "status", s)?;
    same(status.as_bytes(), head.status.bytes(), s)?;
    let operation = check::field(state.body(), "operation_key", s)?;
    check::same_text(
        check::text(operation, "logical_store_id", s)?,
        &key.logical_store_id,
        s,
    )?;
    let state_scope_value = check::field(operation, "scope", s)?;
    let state_scope = s
        .canonical_representation(state_scope_value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    same(scope.bytes(), state_scope.bytes(), s)?;
    let store = super::super::wire::string(&key.logical_store_id, s)?;
    let enrollment_string = super::super::wire::string(enrollment_id, s)?;
    let revision_value = check::field(state.body(), "revision", s)?;
    let revision = s
        .canonical_representation(revision_value, RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let id = state
        .reference
        .id
        .strip_prefix("wsource_")
        .ok_or(ServiceError::IntegrityFailure)?;
    // The ID domain's digest is compared without allocating a prefixed copy.
    let mut h = Sha256::new();
    s.semantic_work()
        .hash_bytes(b"bean-counter/workflow-source-state/1\0".len())
        .map_err(core_error)?;
    h.update(b"bean-counter/workflow-source-state/1\0");
    for p in [
        b"[".as_slice(),
        store.bytes(),
        b",",
        scope.bytes(),
        b",",
        enrollment_string.bytes(),
        b",",
        revision.bytes(),
        b"]",
    ] {
        s.semantic_work().scan_bytes(p.len()).map_err(core_error)?;
        s.semantic_work().hash_bytes(p.len()).map_err(core_error)?;
        h.update(p);
    }
    check::digest_text(&h.finalize().into(), id, false, s)?;
    let grant = if equal_text(status, "active", s)? {
        let (r, l) = from_value(
            check::field(state.body(), "current_grant_ref", s)?,
            "workflow-source-grant",
            s,
        )?;
        same(
            r.id.as_bytes(),
            head.grant_id
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .bytes(),
            s,
        )?;
        let grant = load(tx, key, r, l, "ledger-workflow-source-grant/1", before, s).await?;
        same_ref(
            check::field(grant.body(), "original_ceiling_ref", s)?,
            &enrollment.reference,
            s,
        )?;
        check::same_text(check::text(grant.body(), "grant_id", s)?, enrollment_id, s)?;
        check::same_text(
            check::text(grant.body(), "revision", s)?,
            check::text(state.body(), "revision", s)?,
            s,
        )?;
        Some(grant)
    } else {
        check::same_text(status, "revoked", s)?;
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
            .map_err(core_error)?;
        require(head.grant_id.is_none(), "INTEGRITY_FAILURE")?;
        // The revoked shape has no current-grant member. The core additionally
        // validates terminal lineage/rights from completed historical inputs.
        for (name, _) in state
            .body()
            .as_object()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(name.len())
                .map_err(core_error)?;
            s.semantic_work().scan_bytes(17).map_err(core_error)?;
            require(name != "current_grant_ref", "INTEGRITY_FAILURE")?;
        }
        None
    };
    Ok(ObservedSource {
        original,
        enrollment,
        state,
        grant,
        head,
    })
}

/// Same owning scoped reader for acceptance's genuine retained operands. This
/// does not authenticate introductions or derive source/assent permissions.
pub(in super::super) async fn retained_record(
    tx: &mut SqliteTx,
    key: &OperationKey,
    value: &Value,
    kind: &str,
    schema: &str,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<SourceRecord> {
    let (r, l) = from_value(value, kind, s)?;
    load(tx, key, r, l, schema, before, s).await
}

/// Point-select the existing immutable customer registration under this SAME
/// writer. Its body/earliest origin and original-enrollment membership still
/// require the shared acceptance/chronology checks; index presence is DATA only.
pub(in super::super) async fn customer_record(
    tx: &mut SqliteTx,
    key: &OperationKey,
    customer: &str,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<SourceRecord> {
    let head = tx
        .workflow_financial_customer_ref(&key.logical_store_id, &key.scope, customer, s)
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let (r, l) = own(
        raw_text(head.kind.bytes(), s)?,
        raw_text(head.id.bytes(), s)?,
        raw_text(head.digest.bytes(), s)?,
        s,
    )?;
    super::super::request_wrapper::same_text(&r.kind, "workflow-customer", s)?;
    let record = load(
        tx,
        key,
        r,
        l,
        "ledger-workflow-customer-registration/1",
        before,
        s,
    )
    .await?;
    super::super::request_wrapper::same_text(
        super::super::request_wrapper::text(record.body(), "customer", s)?,
        customer,
        s,
    )?;
    Ok(record)
}
