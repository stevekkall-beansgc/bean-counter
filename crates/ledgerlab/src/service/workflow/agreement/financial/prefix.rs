//! Owning input acquisition for chronological original replay. Neither the
//! cursor nor this parsed key establishes an origin, an introduction or a head.
//! Recipe semantics and their complete inputs must finish before inventory use.
use super::*;
pub(super) mod inventory;
mod publication;
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
    pub(super) fn key(&self) -> &OperationKey {
        &self.key.key
    }
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
            wire::frame_party(
                &incoming,
                &reconstructed,
                &reproduced,
                self.wrappers().sequence(),
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

    /// SAME AppAccept8 normalization against the original compiled program.
    /// Every earlier introduction/assent/head remains the chronological driver's
    /// obligation; this result has no completed-origin or receipt capability.
    pub(super) async fn reconstruct_app_acceptance(
        &self,
        tx: &mut SqliteTx,
        publication: &RecompiledPublication,
        session: &mut OriginalAuditSession,
    ) -> Result<super::acceptance::ReconstructedAcceptance> {
        super::acceptance::reconstruct(tx, self, publication, session).await
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
pub(super) fn read_error(error: FinancialReadError) -> ServiceError {
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
    failure: Arc<AtomicBool>,
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
pub(super) struct LocatedAppAcceptance {
    pub(super) origin: LocatedWrappers,
    pub(super) publication: RecompiledPublication,
    pub(super) acceptance: super::acceptance::ReconstructedAcceptance,
}
pub(super) async fn acceptance_for(
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
        let publication = publication_for(
            tx,
            key,
            request_wrapper::field(raw_payload, "publication_ref", session)?,
            sequence,
            session,
        )
        .await?;
        let acceptance = origin
            .reconstruct_app_acceptance(tx, &publication, session)
            .await?;
        for (a, b) in [
            (&expected.0.kind, &acceptance.binding.reference().kind),
            (&expected.0.id, &acceptance.binding.reference().id),
            (&expected.0.digest, &acceptance.binding.reference().digest),
        ] {
            request_wrapper::same_text(a, b, session)?;
        }
        Ok(LocatedAppAcceptance {
            origin,
            publication,
            acceptance,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::{CanonicalBytes, audit_arena::AuditArena};
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

/// Owning transient DATA after the supported chronological recipes have run.
/// No constructor/export turns this inventory into a prepared commit.
pub(super) struct FoldedInventory {
    _inventory: inventory::Inventory,
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
        let mut inventory = None;
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
                Some(inventory) => {
                    inventory::Inventory::next_origin(inventory, &origin, &mut session)?
                }
                None => {
                    let files = tx
                        .workflow_original_scratch(&mut session)
                        .map_err(read_error)?;
                    inventory = Some(inventory::Inventory::new(files, &origin, &mut session)?);
                }
            }
            source::fold(
                tx,
                &origin,
                inventory.as_mut().ok_or(ServiceError::IntegrityFailure)?,
                &setup,
                &mut session,
            )
            .await?;
            after = origin.wrappers().sequence();
            session.check_cancelled().map_err(core_error)?;
        }
        match &mut inventory {
            Some(inventory) => inventory::Inventory::next_origin(inventory, selected, parent)?,
            None => {
                let files = tx.workflow_original_scratch(parent).map_err(read_error)?;
                inventory = Some(inventory::Inventory::new(files, selected, parent)?);
            }
        }
        source::fold(
            tx,
            selected,
            inventory.as_mut().ok_or(ServiceError::IntegrityFailure)?,
            &setup,
            parent,
        )
        .await?;
        parent.check_cancelled().map_err(core_error)?;
        Ok(FoldedInventory {
            _inventory: inventory.ok_or(ServiceError::IntegrityFailure)?,
        })
    }
    .await;
    if result.is_ok() {
        attempt.finish();
    } else {
        parent.invalidate();
    }
    result
}
