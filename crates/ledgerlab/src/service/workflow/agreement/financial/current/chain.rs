//! Locked current Chain locator plus its owning immutable body. This does not
//! establish index completeness, authenticated provenance or admission.
use super::*;
use crate::store::sqlite::workflow::financial::{HeadKey, HeadLocator, RetainedDocument};

pub(in super::super) struct ObservedChain {
    parsed: AuditedValue,
    document: RetainedDocument,
    reference: ExactRef,
    _reference_lease: ExternalBufferLease,
    locator: HeadLocator,
}
impl ObservedChain {
    pub(in super::super) fn reference(&self) -> &ExactRef {
        &self.reference
    }
    pub(in super::super) fn canonical_body(&self) -> &[u8] {
        self.document.body.bytes()
    }
    pub(in super::super) fn body(&self) -> &Value {
        self.parsed.value()
    }
    pub(in super::super) fn revision(&self, session: &mut OriginalAuditSession) -> Result<&str> {
        text(self.locator.revision.bytes(), session)
    }
}
fn text<'a>(raw: &'a [u8], session: &mut OriginalAuditSession) -> Result<&'a str> {
    session
        .semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    std::str::from_utf8(raw).map_err(|_| ServiceError::IntegrityFailure)
}
fn same_bytes(left: &[u8], right: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    let work = session.semantic_work();
    work.visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    work.scan_bytes(left.len()).map_err(core_error)?;
    work.scan_bytes(right.len()).map_err(core_error)?;
    require(left == right, "INTEGRITY_FAILURE")
}
fn own_reference(
    locator: &HeadLocator,
    session: &mut OriginalAuditSession,
) -> Result<(ExactRef, ExternalBufferLease)> {
    let mut bytes = std::mem::size_of::<ExactRef>();
    for raw in [
        locator.kind.bytes(),
        locator.id.bytes(),
        locator.digest.bytes(),
    ] {
        let work = session.semantic_work();
        work.arithmetic(Op::Add, &[&bytes.into(), &raw.len().into()])
            .map_err(core_error)?;
        bytes = bytes
            .checked_add(raw.len())
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    let lease = session
        .reserve_external_payload(bytes)
        .map_err(core_error)?;
    let mut copy = |raw: &[u8]| -> Result<String> {
        let work = session.semantic_work();
        work.scan_bytes(raw.len()).map_err(core_error)?;
        let value = std::str::from_utf8(raw).map_err(|_| ServiceError::IntegrityFailure)?;
        work.write_bytes(raw.len()).map_err(core_error)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(raw.len())
            .map_err(|_| ServiceError::Unavailable)?;
        owned.push_str(value);
        Ok(owned)
    };
    Ok((
        ExactRef {
            kind: copy(locator.kind.bytes())?,
            id: copy(locator.id.bytes())?,
            digest: copy(locator.digest.bytes())?,
        },
        lease,
    ))
}

pub(super) async fn observe(
    tx: &mut SqliteTx,
    key: &OperationKey,
    customer: &str,
    payload: &Value,
    before_sequence: i64,
    session: &mut OriginalAuditSession,
) -> Result<Option<ObservedChain>> {
    let chain = request_wrapper::text(payload, "chain", session)?;
    let Some(locator) = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Chain { customer, chain },
            session,
        )
        .await
        .map_err(prefix::read_error)?
    else {
        return Ok(None);
    };
    request_wrapper::same_text(
        text(locator.kind.bytes(), session)?,
        "workflow-chain-state",
        session,
    )?;
    let (reference, lease) = own_reference(&locator, session)?;
    let document = tx
        .workflow_financial_document(
            &reference,
            &key.logical_store_id,
            &key.scope,
            before_sequence,
            session,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let parsed = session
        .parse_representation(document.body.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let canonical = session
        .canonical_representation(parsed.value(), RepresentationKind::Record)
        .map_err(core_error)?;
    request_wrapper::same_text(
        text(document.schema.bytes(), session)?,
        "ledger-workflow-chain-state/1",
        session,
    )?;
    same_bytes(canonical.bytes(), document.body.bytes(), session)?;
    let body = parsed.value();
    for (field, expected) in [
        ("schema", "ledger-workflow-chain-state/1"),
        ("logical_store_id", key.logical_store_id.as_str()),
        ("customer", customer),
        ("chain", chain),
        ("revision", text(locator.revision.bytes(), session)?),
    ] {
        let actual = request_wrapper::text(body, field, session)?;
        request_wrapper::same_text(actual, expected, session)?;
    }
    let scope = request_wrapper::field(body, "scope", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&scope.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    for (actual, expected) in scope.iter().zip(&key.scope) {
        request_wrapper::same_text(
            actual.as_str().ok_or(ServiceError::IntegrityFailure)?,
            expected,
            session,
        )?;
    }
    Ok(Some(ObservedChain {
        parsed,
        document,
        reference,
        _reference_lease: lease,
        locator,
    }))
}
