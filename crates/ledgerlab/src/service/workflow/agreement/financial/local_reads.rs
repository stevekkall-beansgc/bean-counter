//! Current local-owner reads return committed immutable core results. Subject
//! source is never used as the identity of the authenticated reader.
use super::*;
use ledgerlab_core::validation_work::{OriginalAuditSession, RepresentationKind};

async fn certified_origin(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<prefix::LocatedWrappers> {
    let sequence = tx
        .workflow_selected_association_sequence(
            reference,
            &key.logical_store_id,
            &key.scope,
            before,
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let prior = sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let origin = prefix::next(tx, key, prior, sequence, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let encoded = wire::reference(reference, s)?;
    let encoded = s
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    origin.require_member(encoded.value(), s)?;
    let results = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut present = false;
    for value in results {
        let actual = s
            .canonical_representation(value, RepresentationKind::Record)
            .map_err(core_error)?;
        let expected = wire::reference(reference, s)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len() + expected.bytes().len())
            .map_err(core_error)?;
        present |= actual.bytes() == expected.bytes();
    }
    require(present, "INTEGRITY_FAILURE")?;
    require(
        local_financial_original::verify(tx, &origin, None, s).await?,
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    Ok(origin)
}

pub(super) async fn current(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    kind: &str,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    request_wrapper::closed(payload, &["subject_ref"], s)?;
    ledger.admin.read(&ledger.store, tx, key).await?;
    let (subject, _subject_owner) =
        current::source::from_value_local(request_wrapper::field(payload, "subject_ref", s)?, s)?;
    require(
        matches!(
            (kind, subject.kind.as_str()),
            ("work.current", "workflow-work") | ("outcome.current", "workflow-outcome-claim")
        ),
        "INVALID_REFERENCE",
    )?;
    let before = tx
        .workflow_management_sequence(s)
        .await
        .map_err(prefix::read_error)?;
    let origin = certified_origin(tx, key, &subject, before, s).await?;
    let mut pool = local_inputs::LocalInputs::new(s)?;
    pool.add(&subject, s)?;
    let mut initial = None;
    for reference in request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(reference, "kind", s)? == "workflow-decision" {
            let (r, _owner) = current::source::from_value_local(reference, s)?;
            pool.add(&r, s)?;
        }
    }
    pool.complete(tx, key, before, s).await?;
    let expected = wire::reference(&subject, s)?;
    for row in pool.iter_views() {
        if row.reference.kind != "workflow-decision" {
            continue;
        }
        let body = s
            .parse_representation(row.canonical_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let selected_subject = request_wrapper::field(body.value(), "subject_ref", s)?;
        let selected = s
            .canonical_representation(selected_subject, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(selected.bytes().len() + expected.bytes().len())
            .map_err(core_error)?;
        if selected.bytes() == expected.bytes() {
            require(initial.is_none(), "INTEGRITY_FAILURE")?;
            let value = wire::reference(row.reference, s)?;
            initial = Some((value, body));
        }
    }
    let (_initial_ref, initial) = initial.ok_or(ServiceError::IntegrityFailure)?;
    request_wrapper::same_text(
        request_wrapper::text(initial.value(), "revision", s)?,
        "1",
        s,
    )?;
    let decision_id = request_wrapper::text(initial.value(), "decision_id", s)?;
    let head = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            crate::store::sqlite::workflow::financial::HeadKey::Decision(decision_id),
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let decode = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| ServiceError::IntegrityFailure)
    };
    let strings_owner = s
        .reserve_external_payload(
            std::mem::size_of::<ledgerlab_core::policy::workflow::transition_constructor::ExactRef>() + head.kind.bytes().len() + head.id.bytes().len() + head.digest.bytes().len(),
        )
        .map_err(core_error)?;
    s.semantic_work().write_bytes(head.kind.bytes().len()+head.id.bytes().len()+head.digest.bytes().len()).map_err(core_error)?;
    let reference = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        kind: decode(head.kind.bytes())?,
        id: decode(head.id.bytes())?,
        digest: decode(head.digest.bytes())?,
    };
    certified_origin(tx, key, &reference, before, s).await?;
    pool.add(&reference, s)?;
    pool.complete(tx, key, before, s).await?;
    let body = s
        .parse_representation(pool.body(&reference)?, RepresentationKind::Record)
        .map_err(core_error)?;
    request_wrapper::same_text(
        request_wrapper::text(body.value(), "decision_id", s)?,
        decision_id,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(body.value(), "revision", s)?,
        std::str::from_utf8(head.revision.bytes()).map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let selected_subject = request_wrapper::field(body.value(), "subject_ref", s)?;
    let actual = s
        .canonical_representation(selected_subject, RepresentationKind::Record)
        .map_err(core_error)?;
    require(actual.bytes() == expected.bytes(), "INTEGRITY_FAILURE")?;
    // Pay the bounded legacy Value response before cloning the parsed tree.
    // Forty-eight bytes per input byte conservatively cover node/map/string
    // control storage, including the smallest possible JSON scalar members.
    let output_bytes=pool.body(&reference)?.len().checked_mul(48).and_then(|v|v.checked_add(4096)).ok_or(ServiceError::Unavailable)?;
    let _response_owner=s.reserve_external_payload(output_bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(output_bytes).map_err(core_error)?;
    let reply = serde_json::json!({"kind":reference.kind,"logical_store_id":key.logical_store_id,"scope":key.scope,"id":reference.id,"digest":reference.digest,"body":body.value()});
    let _output = s
        .canonical_representation(&reply, RepresentationKind::Record)
        .map_err(core_error)?;
    let _owner = strings_owner;
    Ok(reply)
}
