//! Owning input acquisition for chronological original replay. Neither the
//! cursor nor this parsed key establishes an origin, an introduction or a head.
//! Recipe semantics and their complete inputs must finish before inventory use.
use super::*;
mod current_activation;
mod source_birth;
pub(super) use source_birth::SourceBirthInputs;
mod dependencies;
pub(super) use current_activation::CompletedInitialActivation;
pub(super) mod inventory;
mod members;
mod programs;
mod publication;
#[cfg(test)]
impl OriginInput {
    pub(super) fn key(&self) -> &OperationKey {
        &self.key.key
    }
}

#[cfg(test)]
impl FoldedInventory {
    /// Exact full enrollment Ref -> validated earliest introduction -> live
    /// opaque output correspondence. No hash/sequence or scratch row alone
    /// can manufacture this key; core still rebinds all selected output bytes.
    pub(super) fn initial_source_correspondence(
        &mut self,
        enrollment: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<
        &ledgerlab_core::policy::workflow::transition_constructor::InitialSourceCorrespondence,
    > {
        super::acceptance::validate_reference(enrollment, "workflow-source-enrollment", s)?;
        let sequence = self._inventory.introduction_sequence(enrollment, s)?;
        self.programs.initial_source(sequence, s)
    }
}

#[cfg(test)]
mod witness;
#[cfg(test)]
pub(super) use members::ResolvedEconomicInputs;
pub(super) use members::{ResolvedMembers, RetainedEconomicBodies};
pub(super) mod source;
use crate::store::sqlite::workflow::financial::{FinancialReadError, OriginalFrame};
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
pub(super) use publication::RecompiledPublication;

/// String payloads are allocated only after admission; their owner drops them
/// before returning the reservation. No borrowed parsed field escapes its tree.
struct OriginKey {
    key: OperationKey,
    _lease: ExternalBufferLease,
}

pub(super) struct OriginInput {
    key: OriginKey,
    pub(super) identity: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    pub(super) frame: OriginalFrame,
}
impl OriginInput {
    /// The selected recipe can consume this exact acquired frame. Neither
    /// this dispatch nor the mechanical wrapper result credits provenance.
    pub(super) async fn checked_wrappers(
        self,
        tx: &mut SqliteTx,
        session: &mut OriginalAuditSession,
    ) -> Result<LocatedWrappers> {
        let mut attempt = InputAttempt {
            failure: tx.workflow_proof_failure_handle(),
            finished: false,
        };
        let result = async {
            let OriginInput {
                key,
                identity,
                frame,
            } = self;
            let wrappers =
                wrappers::validate_frame(tx, &key.key, identity.bytes(), frame, session).await?;
            Ok(LocatedWrappers {
                key,
                identity,
                wrappers,
            })
        }
        .await;
        if result.is_ok() {
            attempt.finish();
        } else {
            session.invalidate();
        }
        result
    }
}
pub(super) struct LocatedWrappers {
    key: OriginKey,
    identity: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    wrappers: wrappers::OriginalWrappers,
}
/// New local selection validates the actual committed association of this
/// required operand without replaying unrelated originals. Body canonicality,
/// full Ref, schema and semantic relationships remain the caller's checks.
pub(super) async fn verify_selected_association(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let sequence = tx
        .workflow_selected_association_sequence(
            reference,
            &key.logical_store_id,
            &key.scope,
            before,
            session,
        )
        .await
        .map_err(read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
        .map_err(core_error)?;
    let after = sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let original = next(tx, key, after, sequence, session)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?;
    let original = original.checked_wrappers(tx, session).await?;
    let encoded = wire::reference(reference, session)?;
    let parsed = session
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    original.require_member(parsed.value(), session)
}
impl LocatedWrappers {
    pub(super) fn key(&self) -> &OperationKey {
        &self.key.key
    }
    pub(super) fn identity(&self) -> &[u8] {
        self.identity.bytes()
    }
    pub(super) fn wrappers(&self) -> &wrappers::OriginalWrappers {
        &self.wrappers
    }
    pub(super) fn require_member(
        &self,
        reference: &Value,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        publication::member(reference, self.wrappers.manifest(), session)
    }
    /// SAME party recipe recomputation from original captured operands only.
    /// This does not consult today's party head or complete original custody.
    pub(super) async fn reconstruct_party(
        &self,
        tx: &mut SqliteTx,
        inventory: &mut inventory::Inventory,
        session: &mut OriginalAuditSession,
    ) -> Result<current::party::produce::PendingPartyOperation> {
        let mut attempt = InputAttempt {
            failure: tx.workflow_proof_failure_handle(),
            finished: false,
        };
        let result = async {
            let reconstructed = current::party::produce::reconstruct(tx, self, session).await?;
            let reproduced = evidence::original_party(self, &reconstructed, session)?;
            let incoming =
                ingress::IncomingRequest::parse(self.wrappers().original_request().raw(), session)?;
            let members = reconstructed.resolve_members_in(Some(inventory), session)?;
            wire::frame_party(
                &incoming,
                &reconstructed,
                &reproduced,
                self.wrappers().sequence(),
                members,
                session,
            )?
            .compare_original(self, session)?;
            Ok(reconstructed)
        }
        .await;
        if result.is_ok() {
            attempt.finish();
        } else {
            session.invalidate();
        }
        result
    }

    /// Actual saved source/compiler replay only. This DATA result still needs
    /// chronological introduction/authority/build completion by the driver.
    pub(super) async fn recompile_publication(
        &self,
        tx: &mut SqliteTx,
        session: &mut OriginalAuditSession,
    ) -> Result<publication::RecompiledPublication> {
        let mut attempt = InputAttempt {
            failure: tx.workflow_proof_failure_handle(),
            finished: false,
        };
        let result = publication::recompile(tx, self, session).await;
        if result.is_ok() {
            attempt.finish();
        }
        result
    }
}
pub(in crate::service::workflow) fn read_error(error: FinancialReadError) -> ServiceError {
    match error {
        FinancialReadError::Store(error) => crate::service::store_error(error),
        FinancialReadError::Resource(error) => core_error(error),
    }
}
fn text_shape(text: &str, maximum: usize, session: &mut OriginalAuditSession) -> Result<()> {
    let work = session.semantic_work();
    work.arithmetic(Op::Compare, &[&text.len().into(), &0usize.into()])
        .map_err(core_error)?;
    require(!text.is_empty(), "INTEGRITY_FAILURE")?;
    work.arithmetic(Op::Compare, &[&text.len().into(), &maximum.into()])
        .map_err(core_error)?;
    require(text.len() <= maximum, "INTEGRITY_FAILURE")?;
    // This is the same control-character restriction as ingress OperationKey,
    // not an ASCII-only restriction on historical identifiers.
    work.scan_bytes(text.len()).map_err(core_error)?;
    for character in text.chars() {
        work.visit(Visit::Constraint).map_err(core_error)?;
        require(!character.is_control(), "INTEGRITY_FAILURE")?;
    }
    Ok(())
}
fn copy_text(text: &str, session: &mut OriginalAuditSession) -> Result<String> {
    let work = session.semantic_work();
    work.scan_bytes(text.len()).map_err(core_error)?;
    work.write_bytes(text.len()).map_err(core_error)?;
    Ok(text.to_owned())
}
fn origin_key(
    identity: &[u8],
    scope: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<OriginKey> {
    let parsed = session
        .parse_representation(identity, RepresentationKind::Record)
        .map_err(core_error)?;
    let value = parsed.value();
    request_wrapper::closed(
        value,
        &["logical_store_id", "scope", "source", "operation_id"],
        session,
    )?;
    let store = request_wrapper::text(value, "logical_store_id", session)?;
    let source = request_wrapper::text(value, "source", session)?;
    let operation = request_wrapper::text(value, "operation_id", session)?;
    let components = request_wrapper::field(value, "scope", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&components.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(components.len() == 2, "INTEGRITY_FAILURE")?;
    let tenant = components[0]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?;
    let environment = components[1]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?;
    for (text, maximum) in [
        (store, 128),
        (source, 256),
        (operation, 128),
        (tenant, 128),
        (environment, 128),
    ] {
        text_shape(text, maximum, session)?;
    }
    request_wrapper::same_text(store, &scope.logical_store_id, session)?;
    request_wrapper::same_text(tenant, &scope.scope[0], session)?;
    request_wrapper::same_text(environment, &scope.scope[1], session)?;
    let canonical = session
        .canonical_representation(value, RepresentationKind::Record)
        .map_err(core_error)?;
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(identity.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(canonical.bytes().len())
        .map_err(core_error)?;
    require(identity == canonical.bytes(), "INTEGRITY_FAILURE")?;
    let mut bytes = std::mem::size_of::<OperationKey>();
    for text in [store, source, operation, tenant, environment] {
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&bytes.into(), &text.len().into()])
            .map_err(core_error)?;
        bytes = bytes
            .checked_add(text.len())
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    let lease = session
        .reserve_external_payload(bytes)
        .map_err(core_error)?;
    // Construct only after every associated string/header is admitted.
    let key = OperationKey {
        logical_store_id: copy_text(store, session)?,
        scope: [
            copy_text(tenant, session)?,
            copy_text(environment, session)?,
        ],
        source: copy_text(source, session)?,
        operation_id: copy_text(operation, session)?,
    };
    Ok(OriginKey { key, _lease: lease })
}

/// One actual chronological input, bound to the same held transaction/root.
/// No index row is treated as proof and no completion marker is created here.
struct InputAttempt {
    failure: crate::store::sqlite::WorkflowProofFailureHandle,
    finished: bool,
}
impl InputAttempt {
    fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for InputAttempt {
    fn drop(&mut self) {
        if !self.finished {
            self.failure.store(true, Ordering::Release);
        }
    }
}

pub(super) async fn next(
    tx: &mut SqliteTx,
    root: &OperationKey,
    after: i64,
    through: i64,
    session: &mut OriginalAuditSession,
) -> Result<Option<OriginInput>> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = next_inner(tx, root, after, through, session).await;
    if result.is_ok() {
        attempt.finish();
    } else {
        session.invalidate();
    }
    result
}
async fn next_inner(
    tx: &mut SqliteTx,
    root: &OperationKey,
    after: i64,
    through: i64,
    session: &mut OriginalAuditSession,
) -> Result<Option<OriginInput>> {
    require(proof_session::present(tx, root)?, "INTEGRITY_FAILURE")?;
    let Some(locator) = tx
        .workflow_original_next(after, through, session)
        .await
        .map_err(read_error)?
    else {
        return Ok(None);
    };
    let input = target_inner(tx, root, locator.identity, session).await?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&input.frame.sequence.into(), &locator.sequence.into()],
        )
        .map_err(core_error)?;
    require(
        input.frame.sequence == locator.sequence,
        "INTEGRITY_FAILURE",
    )?;
    Ok(Some(input))
}

/// Resolve the submitted exact publication through its earliest scoped
/// indexed membership and literal saved frame. The returned program remains
/// DATA until the complete chronological authority/introduction audit finishes.
pub(super) async fn publication_for(
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &Value,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<RecompiledPublication> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = async {
        // Tuple ownership drops its Ref payload before its allocation lease.
        let expected = current::source::from_value(reference, "workflow-publication", session)?;
        let sequence = tx
            .workflow_financial_origin_sequence(
                &expected.0,
                &key.logical_store_id,
                &key.scope,
                before,
                session,
            )
            .await
            .map_err(read_error)?
            .ok_or_else(|| reject("INVALID_REFERENCE"))?;
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
            .map_err(core_error)?;
        let previous = sequence
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let acquired = next(tx, key, previous, sequence, session)
            .await?
            .ok_or(ServiceError::IntegrityFailure)?;
        let original = acquired.checked_wrappers(tx, session).await?;
        let publication = original.recompile_publication(tx, session).await?;
        let actual = wire::reference(publication.publication().0, session)?;
        let wanted = wire::reference(&expected.0, session)?;
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(wanted.bytes().len())
            .map_err(core_error)?;
        require(actual.bytes() == wanted.bytes(), "INVALID_REFERENCE")?;
        Ok(publication)
    }
    .await;
    if result.is_ok() {
        attempt.finish();
    } else {
        session.invalidate();
    }
    result
}

/// Actual original binding/PUB operands acquired from the held scoped journal.
/// No current compiler/acceptance or ID-only latest lookup is substituted.
/// This remains DATA until the chronological owner completes every origin.
fn retained_box<T>(
    value: T,
    s: &mut OriginalAuditSession,
) -> Result<(Box<T>, ExternalBufferLease)> {
    let bytes = std::mem::size_of::<T>();
    let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    // Tuple drop order frees the boxed payload before its allocation lease.
    Ok((Box::new(value), storage))
}
pub(super) enum AcceptanceMaterial {
    Audited(
        (
            Box<super::acceptance::ReplayedAcceptance>,
            ExternalBufferLease,
        ),
    ),
    Local(
        (
            Box<super::acceptance::TargetedAcceptance>,
            ExternalBufferLease,
        ),
    ),
}
impl std::ops::Deref for AcceptanceMaterial {
    type Target = super::acceptance::ReconstructedAcceptance;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Audited((a, _)) => a,
            Self::Local((a, _)) => a,
        }
    }
}
impl AcceptanceMaterial {
    fn audited(&self) -> Result<&super::acceptance::ReplayedAcceptance> {
        match self {
            Self::Audited((a, _)) => Ok(a),
            _ => Err(ServiceError::Unavailable),
        }
    }
    pub(super) fn accepted_reference(
        &self,
    ) -> &ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        match self {
            Self::Audited((a, _)) => a.accepted_reference(),
            Self::Local((a, _)) => a.accepted_reference(),
        }
    }
    pub(super) fn work_locator_records(
        &self,
    ) -> impl Iterator<
        Item = ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView<'_>,
    > {
        let (a, b) = match self {
            Self::Audited((a, _)) => (Some(a), None),
            Self::Local((b, _)) => (None, Some(b)),
        };
        a.into_iter()
            .flat_map(|a| a.work_locator_records())
            .chain(b.into_iter().flat_map(|a| a.work_locator_records()))
    }
    pub(super) fn require_local_admission_sources(
        &self,
        sources: &[current::source::ObservedSource],
        publication: &RecompiledPublication,
        binding: &Value,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        match self {
            Self::Local((a, _)) => {
                a.require_local_admission_sources(sources, publication, binding, at, s)
            }
            Self::Audited(_) => Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
        }
    }
    fn current_activation_source(
        &self,
        sources: &[current::source::ObservedSource],
        publication: &RecompiledPublication,
        binding: &Value,
        key: &OperationKey,
        context: (&str, &Timestamp),
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        let (actor, at) = context;
        match self {
            Self::Audited((a, _)) => {
                a.current_activation_source(sources, publication, binding, key, (actor, at), s)
            }
            Self::Local((a, _)) => {
                a.current_activation_source(sources, publication, binding, key, (actor, at), s)
            }
        }
    }
    fn activation_authority(
        &self,
        source: &current::source::ObservedSource,
        root: &wire::SourceRecord,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<wire::SourceRecord> {
        match self {
            Self::Audited((a, _)) => a.activation_authority(source, root, key, actor, at, s),
            Self::Local((a, _)) => a.activation_authority(source, root, key, actor, at, s),
        }
    }
}
pub(super) struct LocatedAppAcceptance {
    _origin: LocatedWrappers,
    pub(super) publication: RecompiledPublication,
    pub(super) acceptance: AcceptanceMaterial,
    _prefix: Option<FoldedInventory>,
    through: i64,
}
impl LocatedAppAcceptance {
    /// Extend the SAME already-completed acceptance prefix to today's held
    /// cutoff. Each later original executes once on its own existing owner;
    /// the saved acceptance is not reconstructed a second time.
    pub(super) async fn current_sources(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        setup: &crate::service::billing::Setup,
        sources: &[current::source::ObservedSource],
        before: i64,
        parent: &mut OriginalAuditSession,
    ) -> Result<()> {
        if self._prefix.is_none() {
            for source in sources {
                for r in source.record_refs() {
                    verify_selected_association(tx, key, r, before, parent).await?;
                }
            }
            self.through = before
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            return Ok(());
        }
        let mut attempt = InputAttempt {
            failure: tx.workflow_proof_failure_handle(),
            finished: false,
        };
        let result = async {
            require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
            parent
                .semantic_work()
                .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
                .map_err(core_error)?;
            let through = before
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            parent
                .semantic_work()
                .arithmetic(Op::Compare, &[&self.through.into(), &through.into()])
                .map_err(core_error)?;
            require(self.through <= through, "INTEGRITY_FAILURE")?;
            loop {
                parent
                    .semantic_work()
                    .arithmetic(Op::Compare, &[&self.through.into(), &through.into()])
                    .map_err(core_error)?;
                if self.through == through {
                    break;
                }
                let mut original = OriginalAuditSession::for_resource_profile_v2(parent.arena())
                    .map_err(core_error)?;
                let Some(input) = next(tx, key, self.through, through, &mut original).await? else {
                    return Err(ServiceError::IntegrityFailure);
                };
                let origin = input.checked_wrappers(tx, &mut original).await?;
                original
                    .semantic_work()
                    .arithmetic(
                        Op::Compare,
                        &[&origin.wrappers().sequence().into(), &self.through.into()],
                    )
                    .map_err(core_error)?;
                require(
                    origin.wrappers().sequence() > self.through,
                    "INTEGRITY_FAILURE",
                )?;
                self._prefix
                    .as_mut()
                    .ok_or(ServiceError::IntegrityFailure)?
                    ._inventory
                    .next_origin(&origin, &mut original)?;
                let kind =
                    request_wrapper::text(origin.wrappers().operation(), "kind", &mut original)?;
                if super::acceptance::same_recipe_text(kind, "agreement.activate", &mut original)? {
                    self._prefix
                        .as_mut()
                        .ok_or(ServiceError::IntegrityFailure)?
                        ._inventory
                        .begin_origin_links(origin.wrappers().sequence(), &mut original)?;
                    let replay = super::activation::reconstruct_initial(
                        tx,
                        &origin,
                        self.acceptance.audited()?,
                        &self.publication,
                        self._prefix
                            .as_mut()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        setup,
                        &mut original,
                    )
                    .await?;
                    let binding = original
                        .parse_representation(
                            self.acceptance.binding.bytes(),
                            RepresentationKind::SourceDefinition,
                        )
                        .map_err(core_error)?;
                    replay.fold(
                        &origin,
                        self._prefix
                            .as_mut()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        binding.value(),
                        &mut original,
                    )?;
                    finish_origin(
                        &origin,
                        self._prefix
                            .as_mut()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        &mut original,
                    )?;
                } else {
                    // Every later PUB registers its own exact execution reload
                    // key. The accepted program remains its original PUB.
                    let _later_publication = fold_origin(
                        tx,
                        &origin,
                        self._prefix
                            .as_mut()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        setup,
                        Some(&self.publication),
                        &mut original,
                    )
                    .await?;
                }
                self.through = origin.wrappers().sequence();
                original.check_cancelled().map_err(core_error)?;
            }
            self._prefix
                .as_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .require_record(self.acceptance.binding.reference(), parent)?;
            for source in sources {
                parent
                    .semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                self._prefix
                    .as_mut()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .match_source(source, parent)?;
            }
            parent.check_cancelled().map_err(core_error)
        }
        .await;
        if result.is_ok() {
            attempt.finish();
        } else {
            parent.invalidate();
        }
        result
    }
}
pub(super) async fn acceptance_for(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    reference: &Value,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<LocatedAppAcceptance> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = async {
        let expected = current::source::from_value(reference, "workflow-binding", session)?;
        let sequence = tx
            .workflow_financial_origin_sequence(
                &expected.0,
                &key.logical_store_id,
                &key.scope,
                before,
                session,
            )
            .await
            .map_err(read_error)?
            .ok_or_else(|| reject("INVALID_REFERENCE"))?;
        // The lookup belongs to the requesting origin; literal original
        // reconstruction gets its ONE responsible origin owner, sharing the
        // whole attempt arena rather than a second host/core allowance.
        let mut original_session =
            OriginalAuditSession::for_resource_profile_v2(session.arena()).map_err(core_error)?;
        let session = &mut original_session;
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
            .map_err(core_error)?;
        let previous = sequence
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let origin = next(tx, key, previous, sequence, session)
            .await?
            .ok_or(ServiceError::IntegrityFailure)?
            .checked_wrappers(tx, session)
            .await?;
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().operation(), "kind", session)?,
            "agreement.accept",
            session,
        )?;
        let raw_payload = request_wrapper::field(
            origin.wrappers().original_request().value(),
            "payload",
            session,
        )?;
        let schema = request_wrapper::text(raw_payload, "schema", session)?;
        if !acceptance::same_recipe_text(schema, "ledger-workflow-agreement-accept/9", session)? {
            request_wrapper::same_text(schema, "ledger-workflow-agreement-accept/8", session)?;
        }
        let evidence =
            request_wrapper::field(origin.wrappers().operation(), "request_evidence", session)?;
        let resolution = request_wrapper::field(evidence, "descriptor_resolution", session)?;
        if request_wrapper::text(resolution, "registry_digest", session)?
            == wrappers::LOCAL_REGISTRY
        {
            let publication = publication_for(
                tx,
                key,
                request_wrapper::field(raw_payload, "publication_ref", session)?,
                sequence,
                session,
            )
            .await?;
            let acceptance =
                super::acceptance::TargetedAcceptance::load(tx, &origin, &publication, session)
                    .await?;
            let actual = acceptance.binding.reference();
            for (a, b) in [
                (&expected.0.kind, &actual.kind),
                (&expected.0.id, &actual.id),
                (&expected.0.digest, &actual.digest),
            ] {
                request_wrapper::same_text(a, b, session)?;
            }
            return Ok(LocatedAppAcceptance {
                _origin: origin,
                publication,
                acceptance: AcceptanceMaterial::Local(retained_box(acceptance, session)?),
                _prefix: None,
                through: sequence,
            });
        }
        let (mut prefix, publication) = publication_before(
            ledger,
            tx,
            key,
            request_wrapper::field(raw_payload, "publication_ref", session)?,
            sequence,
            session,
        )
        .await?;
        let setup = ledger.admin.read(&ledger.store, tx, key).await?;
        prefix._inventory.next_origin(&origin, session)?;
        prefix._inventory.begin_origin_links(sequence, session)?;
        let acceptance = super::acceptance::reconstruct_held(
            tx,
            &origin,
            &publication,
            &mut prefix,
            &setup,
            session,
        )
        .await?;
        for (a, b) in [
            (&expected.0.kind, &acceptance.binding.reference().kind),
            (&expected.0.id, &acceptance.binding.reference().id),
            (&expected.0.digest, &acceptance.binding.reference().digest),
        ] {
            request_wrapper::same_text(a, b, session)?;
        }
        acceptance.fold_original(&origin, &mut prefix, session)?;
        finish_origin(&origin, &mut prefix, session)?;
        Ok(LocatedAppAcceptance {
            _origin: origin,
            publication,
            acceptance: AcceptanceMaterial::Audited(retained_box(acceptance, session)?),
            _prefix: Some(prefix),
            through: sequence,
        })
    }
    .await;
    if result.is_ok() {
        attempt.finish();
    } else {
        session.invalidate();
    }
    result
}

pub(super) async fn target(
    tx: &mut SqliteTx,
    root: &OperationKey,
    identity: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    session: &mut OriginalAuditSession,
) -> Result<OriginInput> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = target_inner(tx, root, identity, session).await;
    if result.is_ok() {
        attempt.finish();
    } else {
        session.invalidate();
    }
    result
}
async fn target_inner(
    tx: &mut SqliteTx,
    root: &OperationKey,
    identity: ledgerlab_core::canonical::audit_arena::AuditedBytes,
    session: &mut OriginalAuditSession,
) -> Result<OriginInput> {
    require(proof_session::present(tx, root)?, "INTEGRITY_FAILURE")?;
    let key = origin_key(identity.bytes(), root, session)?;
    let frame = tx
        .workflow_original_frame(
            identity.bytes(),
            &root.logical_store_id,
            &root.scope,
            session,
        )
        .await
        .map_err(read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    Ok(OriginInput {
        key,
        identity,
        frame,
    })
}

/// Owning transient DATA after the supported chronological recipes have run.
/// No constructor/export turns this inventory into a prepared commit.
pub(super) struct FoldedInventory {
    pub(super) _inventory: inventory::Inventory,
    programs: programs::CompletedPrograms,
    pointers: ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers,
}
/// Reuse the selected origin's SAME parent session, including its already-paid
/// framing/identity work. Each earlier origin receives exactly its own session.
pub(super) async fn fold_original_prefix(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    selected: &LocatedWrappers,
    parent: &mut OriginalAuditSession,
) -> Result<FoldedInventory> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = async {
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        let setup = ledger.admin.read(&ledger.store, tx, key).await?;
        let through = selected.wrappers().sequence();
        parent
            .semantic_work()
            .arithmetic(Op::Subtract, &[&through.into(), &1i64.into()])
            .map_err(core_error)?;
        let prior = through
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut after = 0i64;
        let mut inventory: Option<FoldedInventory> = None;
        let mut publication = None;
        loop {
            let mut session = OriginalAuditSession::for_resource_profile_v2(parent.arena())
                .map_err(core_error)?;
            let Some(input) = next(tx, key, after, prior, &mut session).await? else {
                break;
            };
            let origin = input.checked_wrappers(tx, &mut session).await?;
            session
                .semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&origin.wrappers().sequence().into(), &after.into()],
                )
                .map_err(core_error)?;
            require(origin.wrappers().sequence() > after, "INTEGRITY_FAILURE")?;
            match &mut inventory {
                Some(inventory) => inventory::Inventory::next_origin(
                    &mut inventory._inventory,
                    &origin,
                    &mut session,
                )?,
                None => {
                    let files = tx
                        .workflow_original_scratch(&mut session)
                        .map_err(read_error)?;
                    inventory = Some(FoldedInventory {
                        _inventory: inventory::Inventory::new(files, &origin, &mut session)?,
                        programs: programs::CompletedPrograms::new(),
                        pointers: ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers::new(&mut session).map_err(core_error)?,
                    });
                }
            }
            if let Some(program) = fold_origin(
                tx,
                &origin,
                inventory.as_mut().ok_or(ServiceError::IntegrityFailure)?,
                &setup,
                publication.as_ref(),
                &mut session,
            )
            .await?
            {
                publication = Some(program);
            }
            after = origin.wrappers().sequence();
            session.check_cancelled().map_err(core_error)?;
        }
        match &mut inventory {
            Some(inventory) => {
                inventory::Inventory::next_origin(&mut inventory._inventory, selected, parent)?
            }
            None => {
                let files = tx.workflow_original_scratch(parent).map_err(read_error)?;
                inventory = Some(FoldedInventory {
                    _inventory: inventory::Inventory::new(files, selected, parent)?,
                    programs: programs::CompletedPrograms::new(),
                    pointers: ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers::new(parent).map_err(core_error)?,
                });
            }
        }
        fold_origin(
            tx,
            selected,
            inventory.as_mut().ok_or(ServiceError::IntegrityFailure)?,
            &setup,
            publication.as_ref(),
            parent,
        )
        .await?;
        parent.check_cancelled().map_err(core_error)?;
        inventory.ok_or(ServiceError::IntegrityFailure)
    }
    .await;
    if result.is_ok() {
        attempt.finish();
    } else {
        parent.invalidate();
    }
    result
}

/// Fresh acceptance consumes the program produced by its actual earlier PUB
/// fold. It does not restore it again under the acceptance origin's allowance.
/// This selects one exact requested publication, not an assertion that all
/// affected ancestors share it; their own-program selection remains required.
pub(super) async fn publication_before(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    wanted: &Value,
    before: i64,
    parent: &mut OriginalAuditSession,
) -> Result<(FoldedInventory, RecompiledPublication)> {
    let (inventory, publication) =
        before_inputs(ledger, tx, key, Some(wanted), before, None, parent).await?;
    Ok((
        inventory.ok_or(ServiceError::IntegrityFailure)?,
        publication.ok_or_else(|| reject("INVALID_REFERENCE"))?,
    ))
}
/// The existing workflow sequence domain is 1..=100000. Under the held writer,
/// this exclusive bound visits every actual operation, including a full store;
/// it does not allocate a prospective sequence or introduce an origin.
pub(super) async fn authenticate_migration_prefix(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    parent: &mut OriginalAuditSession,
) -> Result<Option<FoldedInventory>> {
    let (inventory, _) = before_inputs(ledger, tx, key, None, 100001, None, parent).await?;
    Ok(inventory)
}
/// The actual fresh party caller already owns this normalized current setup.
/// Historical recipes still execute independently under their own origin owner.
#[cfg(test)]
pub(super) async fn party_before_observed(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    before: i64,
    management: &crate::service::workflow::local::OwnedLocalManagement,
    parent: &mut OriginalAuditSession,
) -> Result<Option<FoldedInventory>> {
    ledger
        .admin
        .recheck_management_fresh(management, &ledger.store, tx, key, parent)
        .await?;
    let (inventory, _) =
        before_inputs(ledger, tx, key, None, before, Some(management), parent).await?;
    Ok(inventory)
}
async fn before_inputs(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    wanted: Option<&Value>,
    before: i64,
    management: Option<&crate::service::workflow::local::OwnedLocalManagement>,
    parent: &mut OriginalAuditSession,
) -> Result<(Option<FoldedInventory>, Option<RecompiledPublication>)> {
    let mut attempt = InputAttempt {
        failure: tx.workflow_proof_failure_handle(),
        finished: false,
    };
    let result = async {
        require(proof_session::present(tx, key)?, "INTEGRITY_FAILURE")?;
        if let Some(wanted) = wanted {
            super::acceptance::validate_reference(wanted, "workflow-publication", parent)?;
        }
        let owned_setup=if management.is_none() {Some(ledger.admin.read(&ledger.store,tx,key).await?)} else {None};
        let setup=match management {
            Some(management)=>management.setup(),
            None=>owned_setup.as_ref().ok_or(ServiceError::IntegrityFailure)?,
        };
        parent
            .semantic_work()
            .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
            .map_err(core_error)?;
        let through = before
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut inventory: Option<FoldedInventory> = None;
        let mut publication = None;
        let mut after = 0i64;
        loop {
            let mut session = OriginalAuditSession::for_resource_profile_v2(parent.arena())
                .map_err(core_error)?;
            let Some(input) = next(tx, key, after, through, &mut session).await? else {
                break;
            };
            let origin = input.checked_wrappers(tx, &mut session).await?;
            session
                .semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&origin.wrappers().sequence().into(), &after.into()],
                )
                .map_err(core_error)?;
            require(origin.wrappers().sequence() > after, "INTEGRITY_FAILURE")?;
            match &mut inventory {
                Some(inventory) => inventory::Inventory::next_origin(
                    &mut inventory._inventory,
                    &origin,
                    &mut session,
                )?,
                None => {
                    let files = tx
                        .workflow_original_scratch(&mut session)
                        .map_err(read_error)?;
                    inventory = Some(FoldedInventory {
                        _inventory: inventory::Inventory::new(files, &origin, &mut session)?,
                        programs: programs::CompletedPrograms::new(),
                        pointers: ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers::new(&mut session).map_err(core_error)?,
                    });
                }
            }
            if let Some(program) = fold_origin(
                tx,
                &origin,
                inventory.as_mut().ok_or(ServiceError::IntegrityFailure)?,
                setup,
                publication.as_ref(),
                &mut session,
            )
            .await?
            {
                if let Some(wanted) = wanted {
                    let actual = wire::reference(program.publication().0, &mut session)?;
                    let expected = session
                        .canonical_representation(wanted, RepresentationKind::Record)
                        .map_err(core_error)?;
                    session
                        .semantic_work()
                        .visit(Visit::MemberCompare)
                        .map_err(core_error)?;
                    session
                        .semantic_work()
                        .scan_bytes(actual.bytes().len())
                        .map_err(core_error)?;
                    session
                        .semantic_work()
                        .scan_bytes(expected.bytes().len())
                        .map_err(core_error)?;
                    if actual.bytes() == expected.bytes() {
                        session
                            .semantic_work()
                            .visit(Visit::Constraint)
                            .map_err(core_error)?;
                        require(publication.is_none(), "INTEGRITY_FAILURE")?;
                        publication = Some(program);
                    }
                } else {
                    publication = Some(program);
                }
            }
            after = origin.wrappers().sequence();
            session.check_cancelled().map_err(core_error)?;
        }
        parent.check_cancelled().map_err(core_error)?;
        if wanted.is_some() {
            parent
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(publication.is_some(), "INVALID_REFERENCE")?;
        }
        // Actual absence of earlier workflow rows gives an empty prefix.
        // Do not create a fake origin, tree page or W/WB allowance for it.
        Ok((inventory, publication))
    }
    .await;
    if result.is_ok() {
        attempt.finish();
    } else {
        parent.invalidate();
    }
    result
}

impl FoldedInventory {
    pub(super) fn require_record(
        &mut self,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(self._inventory.has(r, session)?, "INTEGRITY_FAILURE")
    }
    pub(super) fn match_source(
        &mut self,
        source: &current::source::ObservedSource,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        for record in source.records() {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            self._inventory
                .compare_record(record.reference, record.canonical_body, session)?;
        }
        let enrollment = source.enrollment_record();
        let id = request_wrapper::field(enrollment.body(), "enrollment_id", session)?;
        let id = session
            .canonical_representation(id, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let permanent = inventory::StateKey::new("enrollment", &[id.bytes()], session)?;
        self._inventory
            .match_head(&permanent, enrollment.canonical_body(), session)?;
        let head = inventory::StateKey::new("source-head", &[id.bytes()], session)?;
        let state = session
            .canonical_representation(source.state_body(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        self._inventory.match_head(&head, state.bytes(), session)
    }
    pub(super) fn match_customer(
        &mut self,
        registration: &current::source::SourceRecord,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        self._inventory.compare_record(
            registration.reference(),
            registration.canonical_body(),
            session,
        )?;
        let name = request_wrapper::field(registration.body(), "customer", session)?;
        let name = session
            .canonical_representation(name, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let key = inventory::StateKey::new("customer", &[name.bytes()], session)?;
        self._inventory
            .match_head(&key, registration.canonical_body(), session)
    }
    pub(super) fn match_party(
        &mut self,
        mapping: &current::party::ObservedPartyAuthorization,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.require_record(mapping.reference(), session)?;
        let spec = request_wrapper::field(mapping.body(), "authorization", session)?;
        let id = request_wrapper::field(spec, "authorization_id", session)?;
        let id = session
            .canonical_representation(id, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let key = inventory::StateKey::new("party-head", &[id.bytes()], session)?;
        let body = session
            .canonical_representation(mapping.body(), RepresentationKind::Authority)
            .map_err(core_error)?;
        self._inventory.match_head(&key, body.bytes(), session)
    }
}

impl FoldedInventory {
    pub(super) fn acceptance_head(
        &mut self,
        customer: &Value,
        agreement: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ledgerlab_core::canonical::audit_arena::AuditedBytes>> {
        let customer = s
            .canonical_representation(customer, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let agreement = s
            .canonical_representation(agreement, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let key = inventory::StateKey::new(
            "agreement-acceptance-head",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        self._inventory.head_body(&key, s)
    }
}

impl FoldedInventory {
    /// Select the already folded source state at this exact original cutoff,
    /// then run the SAME scoped source framing/association validator. No SQL
    /// current-head fallback or fresh-time/grant selection is permitted here.
    pub(super) async fn source_at(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        grant: &Value,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<current::source::ObservedSource> {
        let original = current::source::retained_record(
            tx,
            key,
            grant,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
            before,
            s,
        )
        .await?;
        self.require_record(original.reference(), s)?;
        let id = request_wrapper::text(original.body(), "grant_id", s)?;
        let encoded = wire::string(id, s)?;
        let control = inventory::StateKey::new("source-head", &[encoded.bytes()], s)?;
        let raw = self
            ._inventory
            .head_body(&control, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let body = s
            .parse_representation(raw.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let revision = request_wrapper::text(body.value(), "revision", s)?;
        let numeric = super::acceptance::counter(revision, s)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&numeric.into(), &i64::MAX.into()])
            .map_err(core_error)?;
        require(numeric <= i64::MAX as u64, "INTEGRITY_FAILURE")?;
        let copied = s
            .copy_representation(raw.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let source_key = request_wrapper::field(body.value(), "operation_key", s)?;
        let scope = request_wrapper::field(source_key, "scope", s)?;
        let state =
            wire::source_state_record(copied, &key.logical_store_id, scope, id, revision, s)?;
        self.require_record(state.reference(), s)?;
        let status = request_wrapper::text(body.value(), "status", s)?;
        let active = super::acceptance::same_recipe_text(status, "active", s)?;
        let grant_id = if active {
            let r = request_wrapper::field(body.value(), "current_grant_ref", s)?;
            let r = request_wrapper::text(r, "id", s)?;
            Some(
                s.copy_representation(r.as_bytes(), RepresentationKind::Record)
                    .map_err(core_error)?,
            )
        } else {
            request_wrapper::same_text(status, "revoked", s)?;
            None
        };
        let head = crate::store::sqlite::workflow::financial::SourceHeadLocator {
            revision: numeric as i64,
            state_id: s
                .copy_representation(state.reference().id.as_bytes(), RepresentationKind::Record)
                .map_err(core_error)?,
            grant_id,
            status: s
                .copy_representation(status.as_bytes(), RepresentationKind::Record)
                .map_err(core_error)?,
            kind: s
                .copy_representation(
                    state.reference().kind.as_bytes(),
                    RepresentationKind::Record,
                )
                .map_err(core_error)?,
            digest: s
                .copy_representation(
                    state.reference().digest.as_bytes(),
                    RepresentationKind::Record,
                )
                .map_err(core_error)?,
        };
        let observed =
            current::source::observe_historical_original(tx, key, original, head, before, s)
                .await?;
        self.match_source(&observed, s)?;
        Ok(observed)
    }
}

impl FoldedInventory {
    pub(super) fn original_platform_at(
        &mut self,
        material: current::party::OriginalPlatformConsent,
        s: &mut OriginalAuditSession,
    ) -> Result<current::party::ObservedPlatformConsent> {
        for r in material.records() {
            self.require_record(r, s)?;
        }
        let spec = request_wrapper::field(material.mapping_body(), "authorization", s)?;
        let id = request_wrapper::field(spec, "authorization_id", s)?;
        let id = s
            .canonical_representation(id, RepresentationKind::Record)
            .map_err(core_error)?;
        let key = inventory::StateKey::new("party-head", &[id.bytes()], s)?;
        let head = self
            ._inventory
            .head_body(&key, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let parsed = s
            .parse_representation(head.bytes(), RepresentationKind::Authority)
            .map_err(core_error)?;
        material.with_asof_head(parsed.value(), s)
    }
    pub(super) async fn original_customer_at(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        customer: &Value,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<current::source::SourceRecord> {
        let customer = s
            .canonical_representation(customer, RepresentationKind::Record)
            .map_err(core_error)?;
        let control = inventory::StateKey::new("customer", &[customer.bytes()], s)?;
        let body = self
            ._inventory
            .head_body(&control, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let body = wire::source_record(
            "workflow-customer",
            "ledger-workflow-customer-registration/1",
            body,
            s,
        )?;
        let r = wire::reference(body.reference(), s)?;
        let r = s
            .parse_representation(r.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let record = current::source::retained_record(
            tx,
            key,
            r.value(),
            "workflow-customer",
            "ledger-workflow-customer-registration/1",
            before,
            s,
        )
        .await?;
        self.match_customer(&record, s)?;
        Ok(record)
    }
}

impl FoldedInventory {
    pub(super) fn resolve_members<'a>(
        &mut self,
        records: &[&wire::SourceRecord],
        old: impl Iterator<
            Item = &'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        >,
        s: &mut OriginalAuditSession,
    ) -> Result<ResolvedMembers> {
        members::ResolvedMembers::resolve(self, records, old, s)
    }
}

/// Select the exact original completed PUB, using its live compile key only
/// when the immediately retained owner differs. Neither a scratch locator nor
/// a matching hash can mint that key or substitute a current compiler.
async fn folded_publication(
    tx: &mut SqliteTx,
    key: &OperationKey,
    wanted: &Value,
    prefix: &mut FoldedInventory,
    s: &mut OriginalAuditSession,
) -> Result<RecompiledPublication> {
    super::acceptance::validate_reference(wanted, "workflow-publication", s)?;
    let sequence = prefix._inventory.introduction_sequence(wanted, s)?;
    let compiled = prefix.programs.key(sequence, s)?;
    s.semantic_work()
        .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
        .map_err(core_error)?;
    let previous = sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let original = next(tx, key, previous, sequence, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let loaded = publication::reload(tx, &original, compiled, s).await?;
    let actual = wire::reference(loaded.publication().0, s)?;
    let expected = s
        .canonical_representation(wanted, RepresentationKind::Record)
        .map_err(core_error)?;
    let work = s.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(actual.bytes().len()).map_err(core_error)?;
    work.scan_bytes(expected.bytes().len())
        .map_err(core_error)?;
    require(actual.bytes() == expected.bytes(), "INTEGRITY_FAILURE")?;
    loaded.compare_completed_inputs(&mut prefix._inventory, s)?;
    Ok(loaded)
}
async fn fold_origin(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    prefix: &mut FoldedInventory,
    setup: &crate::service::billing::Setup,
    publication: Option<&RecompiledPublication>,
    s: &mut OriginalAuditSession,
) -> Result<Option<RecompiledPublication>> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    prefix
        ._inventory
        .begin_origin_links(origin.wrappers().sequence(), s)?;
    let mut initial_source = None;
    let publication = if origin.wrappers().class() == wrappers::WireClass::Agreement {
        let literal =
            request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
        let wanted = request_wrapper::field(literal, "publication_ref", s)?;
        let mut matches = false;
        if let Some(publication) = publication {
            let actual = wire::reference(publication.publication().0, s)?;
            let wanted = s
                .canonical_representation(wanted, RepresentationKind::Record)
                .map_err(core_error)?;
            let work = s.semantic_work();
            work.visit(Visit::MemberCompare).map_err(core_error)?;
            work.scan_bytes(actual.bytes().len()).map_err(core_error)?;
            work.scan_bytes(wanted.bytes().len()).map_err(core_error)?;
            matches = actual.bytes() == wanted.bytes();
        }
        let loaded;
        let publication = if matches {
            publication.ok_or(ServiceError::IntegrityFailure)?
        } else {
            loaded = folded_publication(tx, origin.key(), wanted, prefix, s).await?;
            &loaded
        };
        let reconstructed =
            super::acceptance::reconstruct_held(tx, origin, publication, prefix, setup, s).await?;
        reconstructed.fold_original(origin, prefix, s)?;
        None
    } else {
        match source::fold(tx, origin, &mut prefix._inventory, setup, s).await? {
            source::FoldedSource::Publication((program, _box_storage)) => {
                // source::fold has finished original semantic/member/custody
                // checks and staged this exact PUB before live-key association.
                prefix
                    .programs
                    .register(origin.wrappers().sequence(), program.program(), s)?;
                Some(*program)
            }
            source::FoldedSource::InitialSource(key) => {
                initial_source = Some(key);
                None
            }
            source::FoldedSource::Other => None,
        }
    };
    finish_origin(origin, prefix, s)?;
    if let Some(key) = initial_source {
        prefix
            .programs
            .register_initial_source(origin.wrappers().sequence(), key, s)?;
    }
    Ok(publication)
}
fn finish_origin(
    origin: &LocatedWrappers,
    prefix: &mut FoldedInventory,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    prefix
        ._inventory
        .finish_origin_links(origin.wrappers().sequence(), s)?;
    let pointer = origin.wrappers().origin_pointer(s)?;
    let parsed = s
        .parse_representation(pointer.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    // Intrinsic representation facts are registered only AFTER this original's
    // actual semantic/custody/as-of fold. They do not replace that completion.
    // The table and this scratch association belong to the same held attempt.
    let (offset, _length) = prefix._inventory.remember_origin_pointer(
        origin.wrappers().sequence(),
        pointer.bytes(),
        s,
    )?;
    let ordinal = prefix
        .pointers
        .record_at(&parsed, offset, s)
        .map_err(core_error)?;
    prefix._inventory.check_origin_ordinal(ordinal, s)?;
    drop(parsed);
    Ok(())
}

impl LocatedAppAcceptance {
    /// Current maps are checked only after extending this same chronological
    /// owner to the actual held cutoff; saved verification fields are not
    /// today's mapping authority.
    pub(super) async fn current_platforms(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<super::acceptance::CurrentPlatformConsents> {
        if let AcceptanceMaterial::Local((a, _)) = &self.acceptance {
            return a.current_platforms(tx, key, binding, at, before, s).await;
        }
        let result = async {
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
                .map_err(core_error)?;
            let through = before
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&self.through.into(), &through.into()])
                .map_err(core_error)?;
            require(self.through == through, "INTEGRITY_FAILURE")?;
            self.acceptance
                .audited()?
                .current_platforms(
                    tx,
                    key,
                    binding,
                    at,
                    before,
                    self._prefix
                        .as_mut()
                        .ok_or(ServiceError::IntegrityFailure)?,
                    s,
                )
                .await
        }
        .await;
        if result.is_err() {
            s.invalidate();
            tx.workflow_proof_failure_handle()
                .store(true, Ordering::Release);
        }
        result
    }
}

impl LocatedAppAcceptance {
    pub(super) fn current_activation_source(
        &self,
        sources: &[current::source::ObservedSource],
        binding: &Value,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        self.acceptance.current_activation_source(
            sources,
            &self.publication,
            binding,
            key,
            (actor, at),
            s,
        )
    }
}

impl LocatedAppAcceptance {
    pub(super) fn activation_authority(
        &self,
        source: &current::source::ObservedSource,
        root: &wire::SourceRecord,
        key: &OperationKey,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<wire::SourceRecord> {
        self.acceptance
            .activation_authority(source, root, key, actor, at, s)
    }
}

impl LocatedAppAcceptance {
    pub(super) fn activation_members(
        &mut self,
        generated: &[&wire::SourceRecord],
        s: &mut OriginalAuditSession,
    ) -> Result<ResolvedMembers> {
        // The accepted agreement is an actual required origin operand even
        // though its body points to the Binding rather than the reverse.
        self._prefix
            .as_mut()
            .ok_or(ServiceError::Unavailable)?
            .resolve_members(
                generated,
                std::iter::once(self.acceptance.accepted_reference())
                    .chain(self.publication.required_refs()),
                s,
            )
    }
}

impl FoldedInventory {
    pub(super) fn activation_head(
        &mut self,
        binding: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ledgerlab_core::canonical::audit_arena::AuditedBytes>> {
        let customer_value = request_wrapper::field(binding, "customer", s)?;
        let customer = s
            .canonical_representation(customer_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let agreement_value = request_wrapper::field(binding, "agreement_id", s)?;
        let agreement = s
            .canonical_representation(agreement_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let key = inventory::StateKey::new(
            "agreement-activation-head",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        self._inventory.head_body(&key, s)
    }
    pub(super) fn fold_initial_activation(
        &mut self,
        origin: &LocatedWrappers,
        binding: &Value,
        root: &wire::SourceRecord,
        authority: &wire::SourceRecord,
        records: &super::activation::ActivationRecords,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let customer_value = request_wrapper::field(binding, "customer", s)?;
        let customer = s
            .canonical_representation(customer_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let agreement_value = request_wrapper::field(binding, "agreement_id", s)?;
        let agreement = s
            .canonical_representation(agreement_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let revision = wire::string("1", s)?;
        let head = inventory::StateKey::new(
            "agreement-activation-head",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        let semantic = inventory::StateKey::new(
            "agreement-activation-semantic",
            &[customer.bytes(), agreement.bytes(), revision.bytes()],
            s,
        )?;
        let boundary = inventory::StateKey::new(
            "agreement-initial-activation",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        self._inventory.absent(&head, s)?;
        self._inventory.absent(&semantic, s)?;
        self._inventory.absent(&boundary, s)?;
        for record in [
            root,
            authority,
            &records.intent,
            &records.head,
            &records.activation,
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            self._inventory.introduce(
                record.reference(),
                record.bytes(),
                origin.wrappers().sequence(),
                s,
            )?;
        }
        self._inventory
            .set_head(&semantic, records.head.reference(), s)?;
        self._inventory
            .set_head(&boundary, records.activation.reference(), s)?;
        self._inventory.set_head(&head, records.head.reference(), s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::{audit_arena::AuditArena, CanonicalBytes};
    fn scope() -> OperationKey {
        OperationKey {
            logical_store_id: "local".into(),
            scope: ["tenant".into(), "test".into()],
            source: "source".into(),
            operation_id: "origin".into(),
        }
    }
    #[test]
    fn original_key_preserves_ingress_unicode_and_owns_all_copies() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut input = scope();
        input.source = "urn:source:é".into();
        input.operation_id = "作品".into();
        let raw = CanonicalBytes::from_value(&input).unwrap();
        let parsed = origin_key(raw.as_slice(), &scope(), &mut session).unwrap();
        assert_eq!(parsed.key, OperationKey::parse(raw.as_slice()).unwrap());
        assert!(arena.usage().unwrap().live_bytes > baseline);
        drop(parsed);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn original_key_refuses_foreign_scope_control_and_noncanonical_identity() {
        for (name, mut input) in [
            ("scope", scope()),
            ("control", scope()),
            ("canonical", scope()),
        ] {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            if name == "scope" {
                input.scope[0] = "foreign".into();
            }
            if name == "control" {
                input.operation_id = "bad\nkey".into();
            }
            let mut raw = CanonicalBytes::from_value(&input).unwrap().into_vec();
            if name == "canonical" {
                raw.push(b' ');
            }
            let baseline = arena.usage().unwrap().live_bytes;
            assert!(origin_key(&raw, &scope(), &mut session).is_err(), "{name}");
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        }
    }
}
