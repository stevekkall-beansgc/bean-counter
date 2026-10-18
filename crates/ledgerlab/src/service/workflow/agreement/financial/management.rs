//! Private held-owner fingerprint. Exact legacy comparison bytes are framed
//! from borrowed typed fields without cloning a JSON/Setup/key graph.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use std::mem;

pub(super) struct ManagementObservation {
    owner: Arc<AtomicBool>,
    key_body: ledgerlab_core::canonical::audit_arena::AuditedValue,
    fingerprint: AuditedBytes,
    _control: ExternalBufferLease,
}
impl ManagementObservation {
    pub(super) fn fingerprint_bytes(&self) -> &[u8] {
        self.fingerprint.bytes()
    }
    pub(super) async fn capture(
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        key_body: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        request_wrapper::key(key_body, key, s)?;
        let control = s
            .reserve_external_payload(mem::size_of::<Self>())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<Self>())
            .map_err(core_error)?;
        let encoded_key = s
            .canonical_representation(key_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let key_body = s
            .parse_representation(encoded_key.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let fingerprint = fingerprint(ledger, tx, key, key_body.value(), s).await?;
        Ok(Self {
            owner: tx.workflow_proof_failure_handle(),
            key_body,
            fingerprint,
            _control: control,
        })
    }
    pub(super) async fn recheck(
        &self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        let provided = tx.workflow_proof_failure_handle();
        if !Arc::ptr_eq(&self.owner, &provided) {
            self.owner.store(true, Ordering::Release);
            provided.store(true, Ordering::Release);
            return Err(ServiceError::IntegrityFailure);
        }
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        request_wrapper::key(self.key_body.value(), key, s)?;
        let actual = fingerprint(ledger, tx, key, self.key_body.value(), s).await?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(self.fingerprint.bytes().len())
            .map_err(core_error)?;
        require(actual.bytes() == self.fingerprint.bytes(), "STALE_REVISION")
    }
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
/// Only already canonical parts from this fixed private producer enter here.
/// Punctuation/copies use the same owning builder as the existing wire object.
fn array(parts: &[&[u8]], s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let mut length = 2usize;
    for part in parts {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        length = add(length, part.len(), s)?;
        length = add(length, 1, s)?;
    }
    s.semantic_work()
        .arithmetic(Op::Compare, &[&length.into(), &1048576usize.into()])
        .map_err(core_error)?;
    require(length <= 1048576, "INTEGRITY_FAILURE")?;
    let (arena, work) = s.parts();
    let mut out = arena.byte_builder(length, work).map_err(core_error)?;
    out.append(b"[", s.semantic_work()).map_err(core_error)?;
    for (i, part) in parts.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])
            .map_err(core_error)?;
        if i != 0 {
            out.append(b",", s.semantic_work()).map_err(core_error)?;
        }
        out.append(part, s.semantic_work()).map_err(core_error)?;
    }
    out.append(b"]", s.semantic_work()).map_err(core_error)?;
    out.finish(s.semantic_work()).map_err(core_error)
}
fn permissions(values: &[String], s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&values.len().into(), &3usize.into()])
        .map_err(core_error)?;
    require(values.len() <= 3, "INTEGRITY_FAILURE")?;
    let bytes = mem::size_of::<[Option<AuditedBytes>; 3]>() + mem::size_of::<[&[u8]; 3]>();
    let _control = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut encoded: [Option<AuditedBytes>; 3] = [const { None }; 3];
    for (target, value) in encoded.iter_mut().zip(values) {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        *target = Some(wire::string(value, s)?);
    }
    let mut parts = [b"".as_slice(); 3];
    for (target, value) in parts.iter_mut().zip(&encoded).take(values.len()) {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        *target = value
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .bytes();
    }
    array(&parts[..values.len()], s)
}
fn setup_bytes(
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    // Both owning and borrowed field-vector controls precede every field copy.
    let bytes = 2 * mem::size_of::<Vec<(&str, AuditedBytes)>>()
        + 19 * mem::size_of::<(&str, AuditedBytes)>()
        + 19 * mem::size_of::<(&str, &[u8])>();
    let _control = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(19)
        .map_err(|_| ServiceError::Unavailable)?;
    macro_rules! field {
        ($name:literal,$value:expr) => {{
            s.semantic_work().visit(Visit::Field).map_err(core_error)?;
            owned.push(($name, $value));
        }};
    }
    field!("accepted_at", wire::string(setup.accepted_at.as_str(), s)?);
    field!("acceptor", wire::string(&setup.acceptor, s)?);
    field!("agreement", wire::string(&setup.agreement, s)?);
    field!("assent_evidence", wire::string(&setup.assent_evidence, s)?);
    field!("binding", wire::string(&setup.binding, s)?);
    field!("customer", wire::string(&setup.customer, s)?);
    field!(
        "finality_attestation",
        wire::string(&setup.finality_attestation, s)?
    );
    field!("host", wire::string(&setup.host, s)?);
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if let Some(v) = &setup.maximum_quantity {
        field!("maximum_quantity", wire::string(v, s)?);
    }
    field!("operator", wire::string(&setup.operator, s)?);
    field!(
        "operator_attestation",
        wire::string(&setup.operator_attestation, s)?
    );
    field!(
        "outcome_policy",
        s.canonical_representation(&setup.outcome_policy, RepresentationKind::Record)
            .map_err(core_error)?
    );
    field!("permissions", permissions(&setup.permissions, s)?);
    field!("price", wire::string(&setup.price, s)?);
    field!("schema", wire::string(&setup.schema, s)?);
    let tenant = wire::string(setup.scope.tenant(), s)?;
    let environment = wire::string(setup.scope.environment(), s)?;
    field!("scope", array(&[tenant.bytes(), environment.bytes()], s)?);
    field!("source", wire::string(&setup.source, s)?);
    field!("store_id", wire::string(&setup.store_id, s)?);
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if let Some(v) = &setup.unit {
        field!("unit", wire::string(v, s)?);
    }
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(19)
        .map_err(|_| ServiceError::Unavailable)?;
    for (name, value) in &owned {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        fields.push((*name, value.bytes()));
    }
    wire::object(&fields, 262144, s)
}
async fn fingerprint(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    key_body: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    let controls = 12 * mem::size_of::<AuditedBytes>() + 12 * mem::size_of::<&[u8]>();
    let _control = s.reserve_external_payload(controls).map_err(core_error)?;
    s.semantic_work().write_bytes(controls).map_err(core_error)?;
    let key = s
        .canonical_representation(key_body, RepresentationKind::Record)
        .map_err(core_error)?;
    let revision = wire::counter_string(setup.grant_revision, s)?;
    let setup = setup_bytes(&setup, s)?;
    let store = wire::string(&installation.logical_store_id, s)?;
    let tenant = wire::string(&installation.scope.tenant, s)?;
    let environment = wire::string(&installation.scope.environment, s)?;
    let mode = wire::string(&installation.mode, s)?;
    let admission = wire::string(&installation.admission, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&installation.generation.into(), &0i64.into()],
        )
        .map_err(core_error)?;
    require(installation.generation >= 0, "INTEGRITY_FAILURE")?;
    let generation = wire::counter_string(installation.generation as u64, s)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let hold = if installation.dispatch_hold {
        b"true".as_slice()
    } else {
        b"false".as_slice()
    };
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let dispatch = if installation.dispatch_enabled {
        b"true".as_slice()
    } else {
        b"false".as_slice()
    };
    array(
        &[
            key.bytes(),
            revision.bytes(),
            setup.bytes(),
            store.bytes(),
            tenant.bytes(),
            environment.bytes(),
            mode.bytes(),
            admission.bytes(),
            hold,
            dispatch,
            generation.bytes(),
        ],
        s,
    )
}
