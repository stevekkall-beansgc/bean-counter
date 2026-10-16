//! Owning input acquisition for chronological original replay. Neither the
//! cursor nor this parsed key establishes an origin, an introduction or a head.
//! Recipe semantics and their complete inputs must finish before inventory use.
use super::*;
use crate::store::sqlite::workflow::financial::{FinancialReadError, OriginalFrame};
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};

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
}
fn read_error(error: FinancialReadError) -> ServiceError {
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
