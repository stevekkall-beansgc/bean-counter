//! Actual app work operands from the literal original Binding2/Accept8/PUB6.
//! Acquiring DATA does not establish activation, original custody or eligibility.
use super::super::request_wrapper as check;
use super::*;
mod work_candidate;
mod work_capture;
mod work_context;
mod work_heads;
mod work_seeds;
mod nonwork;
use ledgerlab_core::validation_work::ValidationVisit;

pub(in super::super) struct ObservedAppBinding {
    pub(in super::super) original: prefix::LocatedAppAcceptance,
    pub(in super::super) sources: Vec<source::ObservedSource>,
    pub(in super::super) parties: Option<super::super::acceptance::CurrentPlatformConsents>,
    pub(super) activation: Option<prefix::CompletedInitialActivation>,
    pub(super) work_source_slot: Option<ledgerlab_core::canonical::audit_arena::AuditedBytes>,
    pub(super) work_source_index: Option<usize>,
    pub(super) work_source_birth: Option<prefix::SourceBirthInputs>,
    pub(super) work_root: Option<source::SourceRecord>,
    pub(super) work_received_at: Option<Timestamp>,
    _work_received_storage: Option<ExternalBufferLease>,
    pub(super) work_input_seeds: Option<ledgerlab_core::canonical::audit_arena::AuditedBytes>,
    pub(super) work_input_bodies: Option<prefix::RetainedEconomicBodies>,
    pub(super) local_extra:Option<nonwork::ExtraState>,
    pub(super) local_locators:Option<ledgerlab_core::policy::workflow::transition_constructor::LocalOperationLocators>,
    pub(super) local_input_bodies:Option<super::super::local_inputs::LocalInputs>,
    work_heads: Option<work_heads::ObservedWorkHeads>,
    pub(super) work_state: Option<work_heads::OwnedWorkState>,
    pub(super) work_head_locators:
        Option<ledgerlab_core::policy::workflow::transition_constructor::WorkHeadLocators>,
    pub(in super::super) at: Timestamp,
    pub(in super::super) before: i64,
    pub(super) setup: crate::service::billing::Setup,
    _parsed_binding: AuditedValue,
    _source_storage: ExternalBufferLease,
    _time_storage: ExternalBufferLease,
}
/// Newly emitted management DATA; neither row establishes admission.
pub(in super::super) struct ActivationAdmissionRecords {
    pub(in super::super) root: wire::SourceRecord,
    pub(in super::super) authority: wire::SourceRecord,
}
pub(in super::super) struct ObservedAppActivation {
    pub(in super::super) binding: ObservedAppBinding,
    pub(in super::super) normalized_payload: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    pub(in super::super) records: super::super::activation::ActivationRecords,
    pub(in super::super) admission_source: Option<usize>,
    pub(in super::super) admission_records: Option<ActivationAdmissionRecords>,
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
    let _received_storage = session
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    let received_at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let mut observed = observe_binding(
        ledger,
        tx,
        key,
        "work.accept",
        binding,
        Some(customer),
        None,
        session,
    )
    .await?;
    let result = async {
        let index = work_capture::source_index(&observed, key, session)?;
        session
            .semantic_work()
            .visit(ValidationVisit::RefLookup)
            .map_err(core_error)?;
        let source = observed
            .sources
            .get(index)
            .ok_or(ServiceError::IntegrityFailure)?;
        let birth = observed
            .original
            .source_birth_inputs(tx, key, source.enrollment_ref(), observed.before, session)
            .await?;
        observed.work_source_birth = Some(birth);
        let root = work_capture::retained_root(&mut observed, tx, key, index, session).await?;
        observed.work_root = Some(root);
        let (slot, seeds, heads) =
            work_capture::select(&observed, key, payload, index, &received_at, session)?;
        let observed_heads = work_heads::observe(
            tx,
            &mut observed.original,
            key,
            &heads,
            observed.parties.as_ref().ok_or(ServiceError::Unavailable)?,
            session,
        )
        .await?;
        let state = observed_heads.typed_state(
            &heads,
            observed.parties.as_ref().ok_or(ServiceError::Unavailable)?,
            session,
        )?;
        let parsed_heads = session
            .parse_representation(observed_heads.seeds.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let parsed_seeds = session
            .parse_representation(seeds.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        if observed.original.publication.program().uses_local_execution_recipe(){
            let mut pool=super::super::local_inputs::LocalInputs::new(session)?;
            pool.payload_refs(parsed_seeds.value(),session)?;pool.payload_refs(parsed_heads.value(),session)?;
            for r in observed.original.publication.required_refs().chain(observed.original.acceptance.work_locator_records().map(|v|v.reference))
              .chain(observed.sources.iter().flat_map(source::ObservedSource::record_refs))
              .chain(observed.parties.iter().flat_map(|p|p.selected_refs())) {pool.add(r,session)?;}
            if let Some(root)=observed.work_root.as_ref(){pool.add(root.reference(),session)?;}
            if let Some(birth)=observed.work_source_birth.as_ref(){pool.add(birth.state.reference(),session)?;pool.add(birth.grant.reference(),session)?;}
            if let Some(active)=observed.activation.as_ref(){pool.add(active.head.reference(),session)?;pool.add(active.activation.reference(),session)?;}
            pool.payload_refs(payload,session)?;pool.complete(tx,key,observed.before,session).await?;
            observed.local_input_bodies=Some(pool);
        }else{
        let explicit = observed.original.work_explicit_inputs(
            key,
            parsed_seeds.value(),
            parsed_heads.value(),
            session,
        )?;
        let input_bodies = observed
            .original
            .retain_economic_inputs(explicit, session)?;
        observed.work_input_bodies = Some(input_bodies);
        }
        observed.work_source_slot = Some(slot);
        observed.work_source_index = Some(index);
        observed.work_input_seeds = Some(seeds);
        observed.work_head_locators = Some(heads);
        observed.work_heads = Some(observed_heads);
        observed.work_state = Some(state);
        observed.work_received_at = Some(received_at);
        observed._work_received_storage = Some(_received_storage);
        Ok(observed)
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
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
    let observed = observe_binding(
        ledger,
        tx,
        key,
        kind,
        binding,
        None,
        Some(agreement),
        session,
    )
    .await?;
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
        let admission_source =
            if super::super::acceptance::same_recipe_text(kind, "agreement.revoke", session)? {
                None
            } else {
                super::super::activation::binding_interval(
                    payload.value(),
                    binding,
                    &observed.at,
                    session,
                )?;
                Some(observed.original.current_activation_source(
                    &observed.sources,
                    binding,
                    key,
                    &observed.setup.operator,
                    &observed.at,
                    session,
                )?)
            };
        let admission_records = if let Some(index) = admission_source {
            session
                .semantic_work()
                .visit(ValidationVisit::RefLookup)
                .map_err(core_error)?;
            let source = observed
                .sources
                .get(index)
                .ok_or(ServiceError::IntegrityFailure)?;
            let key_body = check::field(incoming.value(), "key", session)?;
            let scope = check::field(key_body, "scope", session)?;
            let root = super::party::produce::root(&observed.setup, scope, &observed.at, session)?;
            let authority = observed.original.activation_authority(
                source,
                &root,
                key,
                &observed.setup.operator,
                &observed.at,
                session,
            )?;
            Some(ActivationAdmissionRecords { root, authority })
        } else {
            None
        };
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
        Ok((head, records, admission_source, admission_records))
    }
    .await;
    let (head, records, admission_source, admission_records) = built.inspect_err(|_| {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    })?;
    Ok(ObservedAppActivation {
        binding: observed,
        normalized_payload,
        records,
        admission_source,
        admission_records,
        _held_head: head,
    })
}
async fn observe_binding(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    operation: &str,
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
            if check::text(reference,"kind",session)?=="workflow-authority"{
                let mapping=source::retained_record(tx,key,reference,"workflow-authority","ledger-workflow-party-authorization/1",before,session).await?;
                prefix::verify_selected_association(tx,key,mapping.reference(),before,session).await?;
                continue;
            }
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
        // Stop remains the existing Authority2 recipe: current admission
        // mapping expiry/revocation cannot erase the right to stop accrued
        // work. Its explicit stop evidence and target checks remain required.
        let parties = if super::super::acceptance::same_recipe_text(
            operation,
            "agreement.revoke",
            session,
        )? {
            None
        } else {
            Some(
                original
                    .current_platforms(tx, key, binding.value(), &at, before, session)
                    .await?,
            )
        };
        let activation =
            if super::super::acceptance::same_recipe_text(operation, "work.accept", session)? {
                Some(
                    original
                        .current_initial_activation(tx, key, binding.value(), &at, before, session)
                        .await?,
                )
            } else {
                None
            };
        Ok(ObservedAppBinding {
            original,
            sources,
            parties,
            activation,
            work_source_slot: None,
            work_source_index: None,
            work_source_birth: None,
            work_root: None,
            work_received_at: None,
            _work_received_storage: None,
            work_input_seeds: None,
            work_input_bodies: None,
            local_extra:None,local_locators:None,
            local_input_bodies:None,
            work_head_locators: None,
            work_heads: None,
            work_state: None,
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

impl ObservedAppActivation {
    pub(in super::super) fn publication(&self) -> &prefix::RecompiledPublication {
        &self.binding.original.publication
    }
    pub(in super::super) fn at(&self) -> &Timestamp {
        &self.binding.at
    }
    pub(in super::super) fn before(&self) -> i64 {
        self.binding.before
    }
    pub(in super::super) fn result_refs(&self) -> [&ExactRef; 3] {
        [
            self.records.intent.reference(),
            self.records.head.reference(),
            self.records.activation.reference(),
        ]
    }
    pub(in super::super) fn authority(&self) -> Result<&wire::SourceRecord> {
        self.admission_records
            .as_ref()
            .map(|v| &v.authority)
            .ok_or(ServiceError::Unavailable)
    }
    pub(in super::super) fn resolve_members(
        &mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<prefix::ResolvedMembers> {
        let admission = self
            .admission_records
            .as_ref()
            .ok_or(ServiceError::Unavailable)?;
        let _control = s
            .reserve_external_payload(2 * std::mem::size_of::<[&wire::SourceRecord; 5]>())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<[&wire::SourceRecord; 5]>())
            .map_err(core_error)?;
        self.binding.original.activation_members(
            &[
                &admission.root,
                &admission.authority,
                &self.records.intent,
                &self.records.head,
                &self.records.activation,
            ],
            s,
        )
    }
}

impl ObservedAppBinding {
 pub(in super::super) fn construct_local(&self,key:&OperationKey,payload:&Value,raw_request:&[u8],s:&mut OriginalAuditSession)->Result<(ledgerlab_core::policy::workflow::transition_constructor::EconomicCandidate,ledgerlab_core::policy::workflow::transition_constructor::LocalFinancialRequestEvidence)>{work_capture::construct_local(self,key,payload,raw_request,s)}
 pub(in super::super) fn received_at(&self)->Result<&Timestamp>{self.work_received_at.as_ref().ok_or(ServiceError::IntegrityFailure)}
}

pub(in super::super) use nonwork::observe as observe_nonwork;
