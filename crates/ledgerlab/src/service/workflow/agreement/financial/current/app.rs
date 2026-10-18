//! Actual app work operands from the literal original Binding2/Accept8/PUB6.
//! Acquiring DATA does not establish activation, original custody or eligibility.
use super::super::request_wrapper as check;
use super::*;
use ledgerlab_core::validation_work::ValidationVisit;

pub(in super::super) struct ObservedAppBinding {
    pub(super) original: prefix::LocatedAppAcceptance,
    pub(super) sources: Vec<source::ObservedSource>,
    pub(super) at: Timestamp,
    pub(super) before: i64,
    pub(super) setup: crate::service::billing::Setup,
    _parsed_binding: AuditedValue,
    _source_storage: ExternalBufferLease,
    _time_storage: ExternalBufferLease,
}
pub(in super::super) struct ObservedAppActivation {
    pub(in super::super) binding: ObservedAppBinding,
    pub(in super::super) normalized_payload: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    pub(in super::super) records: super::super::activation::ActivationRecords,
    _held_head: Option<crate::store::sqlite::workflow::financial::HeadLocator>,
}
pub(in super::super) async fn observe(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    session: &mut OriginalAuditSession,
) -> Result<ObservedAppBinding> {
    let binding = check::field(payload, "binding_ref", session)?;
    let customer = check::field(payload, "customer", session)?;
    observe_binding(ledger, tx, key, binding, Some(customer), None, session).await
}
/// Activation supplies its literal accepted Binding and agreement identity.
/// This reuses source identity/current-state acquisition only; the activation
/// recipe must still check its own admit/stop rights, consent and schedule.
pub(in super::super) async fn observe_activation(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    incoming: &super::super::ingress::IncomingRequest,
    payload: &Value,
    session: &mut OriginalAuditSession,
) -> Result<ObservedAppActivation> {
    let key = incoming.key();
    let kind = incoming.kind();
    let binding = check::field(payload, "accepted_binding_ref", session)?;
    let agreement = check::field(payload, "agreement_id", session)?;
    let observed =
        observe_binding(ledger, tx, key, binding, None, Some(agreement), session).await?;
    let normalized_payload = super::super::activation::normalize(
        kind,
        payload,
        observed.original.acceptance.binding.reference(),
        observed._parsed_binding.value(),
        session,
    )
    .inspect_err(|_| {
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    })?;
    let built = async {
        let payload = session
            .parse_representation(
                normalized_payload.bytes(),
                RepresentationKind::SourceDefinition,
            )
            .map_err(core_error)?;
        let binding = observed._parsed_binding.value();
        let customer = check::text(binding, "customer", session)?;
        let agreement = check::text(binding, "agreement_id", session)?;
        let head = tx
            .workflow_financial_head(
                &key.logical_store_id,
                &key.scope,
                crate::store::sqlite::workflow::financial::HeadKey::AgreementActivation {
                    customer,
                    agreement,
                },
                session,
            )
            .await
            .map_err(prefix::read_error)?;
        let expected = check::text(payload.value(), "expected_revision", session)?;
        let prior = if let Some(head) = &head {
            let revision = std::str::from_utf8(head.revision.bytes())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            check::same_text(revision, expected, session)?;
            let kind = wire::string(
                std::str::from_utf8(head.kind.bytes())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
                session,
            )?;
            let id = wire::string(
                std::str::from_utf8(head.id.bytes()).map_err(|_| ServiceError::IntegrityFailure)?,
                session,
            )?;
            let digest = wire::string(
                std::str::from_utf8(head.digest.bytes())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
                session,
            )?;
            Some(wire::object(
                &[
                    ("digest", digest.bytes()),
                    ("id", id.bytes()),
                    ("kind", kind.bytes()),
                ],
                262144,
                session,
            )?)
        } else {
            check::same_text(expected, "0", session)?;
            None
        };
        let prior = prior
            .as_ref()
            .map(|v| {
                session
                    .parse_representation(v.bytes(), RepresentationKind::SourceDefinition)
                    .map_err(core_error)
            })
            .transpose()?;
        let key_body = check::field(incoming.value(), "key", session)?;
        let records = super::super::activation::build_records(
            kind,
            payload.value(),
            key_body,
            observed.original.acceptance.binding.reference(),
            binding,
            &observed.setup.operator,
            &observed.at,
            observed.before,
            prior.as_ref().map(|v| v.value()),
            session,
        )?;
        Ok((head, records))
    }
    .await;
    let (head, records) = built.inspect_err(|_| {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    })?;
    Ok(ObservedAppActivation {
        binding: observed,
        normalized_payload,
        records,
        _held_head: head,
    })
}
async fn observe_binding(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    binding_ref: &Value,
    expected_customer: Option<&Value>,
    expected_agreement: Option<&Value>,
    session: &mut OriginalAuditSession,
) -> Result<ObservedAppBinding> {
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
        let mut original =
            prefix::acceptance_for(ledger, tx, key, binding_ref, before, session).await?;
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
        for (field, expected) in [
            ("customer", expected_customer),
            ("agreement_id", expected_agreement),
        ] {
            if let Some(expected) = expected {
                session
                    .semantic_work()
                    .visit(ValidationVisit::Constraint)
                    .map_err(core_error)?;
                let expected = expected.as_str().ok_or_else(|| reject("INVALID_SHAPE"))?;
                check::same_text(
                    expected,
                    check::text(binding.value(), field, session)?,
                    session,
                )?;
            }
        }
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
        original
            .current_sources(tx, key, &setup, &sources, before, session)
            .await?;
        Ok(ObservedAppBinding {
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
