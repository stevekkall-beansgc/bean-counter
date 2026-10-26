//! Lifecycle-only fixed immutable completion. Endpoint facts stay opaque;
//! explicit typed field groups are selected separately by the operation owner.
use super::super::{current::source, request_wrapper as check};
use super::*;
use ledgerlab_core::{
    policy::workflow::{
        selected_program::SelectedProgram, transition_constructor::select_lifecycle_input_edges,
    },
    validation_work::{ArithmeticOperation as Op, ValidationVisit},
};

fn decode(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| ServiceError::IntegrityFailure)
}
fn locator_reference(
    row: &crate::store::sqlite::workflow::financial::AppClaimLocator,
    s: &mut OriginalAuditSession,
) -> Result<ledgerlab_core::canonical::audit_arena::AuditedValue> {
    let kind = wire::string(decode(row.kind.bytes())?, s)?;
    let id = wire::string(decode(row.id.bytes())?, s)?;
    let digest = wire::string(decode(row.digest.bytes())?, s)?;
    let _fields = s
        .reserve_external_payload(2 * std::mem::size_of::<[(&str, &[u8]); 3]>())
        .map_err(core_error)?;
    let value = wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )?;
    s.parse_representation(value.bytes(), RepresentationKind::Record)
        .map_err(core_error)
}
impl LocalInputs {
    pub(in super::super) async fn complete_lifecycle(
        &mut self,
        program: &SelectedProgram,
        tx: &mut SqliteTx,
        key: &OperationKey,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.complete(tx, key, before, s).await?;
        let (core_key, _key_owner) = core_key_with_owner(key, s)?;
        let mut index = 0;
        while index < self.rows.len() {
            if self.rows[index].lifecycle_processed {
                index += 1;
                continue;
            }
            let row = &self.rows[index];
            let body = row.body.as_ref().ok_or(ServiceError::IntegrityFailure)?;
            let parsed = s
                .parse_representation(body.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let native = row
                .native_schema
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .scan_bytes(native.bytes().len())
                .map_err(core_error)?;
            let native =
                std::str::from_utf8(native.bytes()).map_err(|_| ServiceError::IntegrityFailure)?;
            check::same_text(check::text(parsed.value(), "schema", s)?, native, s)?;
            if acceptance::same_recipe_text(&row.reference.kind, "workflow-action", s)? {
                let producer_key = check::field(parsed.value(), "operation_key", s)?;
                let identity = s
                    .canonical_representation(producer_key, RepresentationKind::Record)
                    .map_err(core_error)?;
                let origin = prefix::target(tx, key, identity, s)
                    .await?
                    .checked_wrappers(tx, s)
                    .await?;
                require(origin.wrappers().sequence() < before, "INTEGRITY_FAILURE")?;
                let producer_kind = check::text(origin.wrappers().operation(), "kind", s)?;
                require(
                    matches!(
                        producer_kind,
                        "work.accept"
                            | "work.reverse"
                            | "quantity.correct"
                            | "outcome.submit"
                            | "outcome.correct"
                            | "cost.finalize"
                            | "cost.correct"
                    ),
                    "INTEGRITY_FAILURE",
                )?;
                let value = wire::reference(&row.reference, s)?;
                let value = s
                    .parse_representation(value.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                origin.require_member(value.value(), s)?;
                result_member(origin.wrappers().receipt(), value.value(), s)?;
                let evidence = check::field(origin.wrappers().operation(), "request_evidence", s)?;
                let normalized = check::field(evidence, "normalized_request", s)?;
                equal(
                    check::field(normalized, "key", s)?,
                    check::field(parsed.value(), "operation_key", s)?,
                    s,
                )?;
            }
            drop(parsed);
            let edges = select_lifecycle_input_edges(
                program,
                &core_key,
                EconomicRecordView {
                    reference: &row.reference,
                    canonical_body: body.bytes(),
                },
                s,
            )
            .map_err(core_error)?;
            for reference in edges.direct_refs() {
                self.add(reference, s)?;
            }
            for identity in edges.record_identities() {
                let mut hit = None;
                for schema in identity.schemas() {
                    if let Some(row) = tx
                        .workflow_local_record_identity(
                            &key.logical_store_id,
                            &key.scope,
                            identity.kind(),
                            identity.id(),
                            schema,
                            s,
                        )
                        .await
                        .map_err(prefix::read_error)?
                    {
                        require(hit.is_none(), "INTEGRITY_FAILURE")?;
                        hit = Some(row);
                    }
                }
                let row = hit.ok_or(ServiceError::IntegrityFailure)?;
                let reference = locator_reference(&row, s)?;
                self.add_value(reference.value(), s)?;
            }
            if acceptance::same_recipe_text(&self.rows[index].reference.kind, "workflow-work", s)? {
                let value = wire::reference(&self.rows[index].reference, s)?;
                let value = s
                    .parse_representation(value.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let work = source::retained_local_record(
                    tx,
                    key,
                    value.value(),
                    ("workflow-work", "ledger-workflow-work-record/1"),
                    before,
                    s,
                )
                .await?;
                let producer_key = check::field(work.body(), "key", s)?;
                let identity = s
                    .canonical_representation(producer_key, RepresentationKind::Record)
                    .map_err(core_error)?;
                let origin = prefix::target(tx, key, identity, s)
                    .await?
                    .checked_wrappers(tx, s)
                    .await?;
                retain_work_claim(tx, key, before, &work, &origin, self, s).await?;
                let initial = unique_result(origin.wrappers().receipt(), "workflow-decision", s)?;
                origin.require_member(initial, s)?;
                let initial = source::retained_local_record(
                    tx,
                    key,
                    initial,
                    ("workflow-decision", "ledger-workflow-decision-revision/1"),
                    before,
                    s,
                )
                .await?;
                check::same_text(check::text(initial.body(), "revision", s)?, "1", s)?;
                equal(
                    check::field(initial.body(), "subject_ref", s)?,
                    value.value(),
                    s,
                )?;
                equal(
                    check::field(initial.body(), "operation_key", s)?,
                    check::field(work.body(), "key", s)?,
                    s,
                )?;
                self.add(initial.reference(), s)?;
            }
            self.rows[index].lifecycle_processed = true;
            self.complete(tx, key, before, s).await?;
            index += 1;
        }
        Ok(())
    }
}
pub(in super::super) fn core_key_with_owner(
    key: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<(CoreKey, ExternalBufferLease)> {
    let texts = [
        key.logical_store_id.as_str(),
        key.scope[0].as_str(),
        key.scope[1].as_str(),
        key.source.as_str(),
        key.operation_id.as_str(),
    ];
    let mut bytes = std::mem::size_of::<CoreKey>();
    for text in texts {
        s.semantic_work()
            .arithmetic(Op::Add, &[&bytes.into(), &text.len().into()])
            .map_err(core_error)?;
        bytes = bytes
            .checked_add(text.len())
            .ok_or(ServiceError::Unavailable)?;
    }
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    Ok((
        CoreKey {
            logical_store_id: texts[0].to_owned(),
            scope: [texts[1].to_owned(), texts[2].to_owned()],
            source: texts[3].to_owned(),
            operation_id: texts[4].to_owned(),
        },
        owner,
    ))
}
fn equal(left: &Value, right: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let left = s
        .canonical_representation(left, RepresentationKind::Record)
        .map_err(core_error)?;
    let right = s
        .canonical_representation(right, RepresentationKind::Record)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(left.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(right.bytes().len())
        .map_err(core_error)?;
    require(left.bytes() == right.bytes(), "INTEGRITY_FAILURE")
}
fn result_member(receipt: &Value, reference: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let expected = s
        .canonical_representation(reference, RepresentationKind::Record)
        .map_err(core_error)?;
    let mut count = 0usize;
    for r in check::field(receipt, "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let actual = s
            .canonical_representation(r, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .visit(ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        if actual.bytes() == expected.bytes() {
            s.semantic_work()
                .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            count = count.checked_add(1).ok_or(ServiceError::Unavailable)?;
        }
    }
    require(count == 1, "INTEGRITY_FAILURE")
}
fn unique_result<'a>(
    receipt: &'a Value,
    kind: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let mut selected = None;
    for reference in check::field(receipt, "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if check::text(reference, "kind", s)? == kind {
            require(selected.replace(reference).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    selected.ok_or(ServiceError::IntegrityFailure)
}
/// Exact immutable Work birth support; it never acquires a mutable ancestor
/// head or confers a dependent monetary/closure right.
pub(in super::super) async fn retain_work_claim(
    tx: &mut SqliteTx,
    key: &OperationKey,
    before: i64,
    work: &source::SourceRecord,
    origin: &prefix::LocatedWrappers,
    pool: &mut LocalInputs,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&origin.wrappers().sequence().into(), &before.into()],
        )
        .map_err(core_error)?;
    require(origin.wrappers().sequence() < before, "INTEGRITY_FAILURE")?;
    check::same_text(
        check::text(origin.wrappers().operation(), "kind", s)?,
        "work.accept",
        s,
    )?;
    let work_ref = wire::reference(work.reference(), s)?;
    let work_ref = s
        .parse_representation(work_ref.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    origin.require_member(work_ref.value(), s)?;
    result_member(origin.wrappers().receipt(), work_ref.value(), s)?;
    let evidence = check::field(origin.wrappers().operation(), "request_evidence", s)?;
    let normalized = check::field(evidence, "normalized_request", s)?;
    equal(
        check::field(normalized, "key", s)?,
        check::field(work.body(), "key", s)?,
        s,
    )?;
    equal(
        check::field(normalized, "payload", s)?,
        check::field(work.body(), "payload", s)?,
        s,
    )?;
    let claim = unique_result(origin.wrappers().receipt(), "workflow-work-claim", s)?;
    origin.require_member(claim, s)?;
    check::same_text(
        check::text(claim, "id", s)?,
        check::text(work.body(), "claim_id", s)?,
        s,
    )?;
    let claim = source::retained_local_record(
        tx,
        key,
        claim,
        ("workflow-work-claim", "ledger-workflow-work-claim/1"),
        before,
        s,
    )
    .await?;
    equal(
        check::field(claim.body(), "work_ref", s)?,
        work_ref.value(),
        s,
    )?;
    pool.add(claim.reference(), s)
}
