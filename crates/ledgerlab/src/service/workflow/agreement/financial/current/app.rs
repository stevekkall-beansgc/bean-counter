//! Actual app work operands from the literal original Binding2/Accept8/PUB6.
//! Acquiring DATA does not establish activation, original custody or eligibility.
use super::super::request_wrapper as check;
use super::*;
use ledgerlab_core::validation_work::ValidationVisit;

pub(in super::super) struct ObservedAppWork {
    pub(super) original: prefix::LocatedAppAcceptance,
    pub(super) sources: Vec<source::ObservedSource>,
    pub(super) at: Timestamp,
    pub(super) before: i64,
    pub(super) setup: crate::service::billing::Setup,
    _parsed_binding: AuditedValue,
    _source_storage: ExternalBufferLease,
    _time_storage: ExternalBufferLease,
}
pub(in super::super) async fn observe(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    session: &mut OriginalAuditSession,
) -> Result<ObservedAppWork> {
    let result = async {
        session.check_cancelled().map_err(core_error)?;
        let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
        let installation = tx
            .load_installation()
            .await
            .map_err(crate::service::store_error)?;
        require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
        let before = tx
            .workflow_sequence()
            .await
            .map_err(crate::service::store_error)?;
        let time_storage = session
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
        let original = prefix::acceptance_for(
            ledger,
            tx,
            key,
            check::field(payload, "binding_ref", session)?,
            before,
            session,
        )
        .await?;
        let binding = session
            .parse_representation(
                original.acceptance.binding.bytes(),
                RepresentationKind::SourceDefinition,
            )
            .map_err(core_error)?;
        check::same_text(
            check::text(binding.value(), "schema", session)?,
            "ledger-workflow-binding/2",
            session,
        )?;
        check::same_text(
            check::text(payload, "customer", session)?,
            check::text(binding.value(), "customer", session)?,
            session,
        )?;
        let refs = check::field(binding.value(), "authority_refs", session)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&refs.len().into(), &1usize.into()])
            .map_err(core_error)?;
        require(!refs.is_empty(), "AUTHORITY_REQUIRED")?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&refs.len().into(), &32usize.into()])
            .map_err(core_error)?;
        require(refs.len() <= 32, "AUTHORITY_REQUIRED")?;
        session
            .semantic_work()
            .arithmetic(
                Op::Multiply,
                &[
                    &refs.len().into(),
                    &std::mem::size_of::<source::ObservedSource>().into(),
                ],
            )
            .map_err(core_error)?;
        let bytes = refs
            .len()
            .checked_mul(std::mem::size_of::<source::ObservedSource>())
            .ok_or(ServiceError::Unavailable)?;
        let source_storage = session
            .reserve_external_payload(bytes)
            .map_err(core_error)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for reference in refs {
            session
                .semantic_work()
                .visit(ValidationVisit::Element)
                .map_err(core_error)?;
            let observed = source::observe_value(tx, key, reference, before, session).await?;
            session
                .semantic_work()
                .write_bytes(std::mem::size_of::<source::ObservedSource>())
                .map_err(core_error)?;
            sources.push(observed);
        }
        Ok(ObservedAppWork {
            original,
            sources,
            at,
            before,
            setup,
            _parsed_binding: binding,
            _source_storage: source_storage,
            _time_storage: time_storage,
        })
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
