//! One attempt-local retained acceptance, published by the chronological driver
//! only after that original's complete finish_origin checks. It retains the
//! actual replayed consent/private-input owners, never a targeted-load flag.
use super::*;

struct CompletedAcceptance {
    sequence: i64,
    accepted: super::super::acceptance::ReplayedAcceptance,
}

pub(super) struct RetainedAcceptance {
    value: Box<CompletedAcceptance>,
    _storage: ExternalBufferLease,
}

impl RetainedAcceptance {
    /// Stage owning DATA; the caller must not install it in FoldedInventory
    /// until finish_origin has succeeded for this exact acceptance.
    pub(super) fn stage(
        sequence: i64,
        accepted: super::super::acceptance::ReplayedAcceptance,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let body = std::mem::size_of::<CompletedAcceptance>();
        let control = std::mem::size_of::<Option<Self>>();
        s.semantic_work()
            .arithmetic(Op::Add, &[&body.into(), &control.into()])
            .map_err(core_error)?;
        let bytes = body.checked_add(control).ok_or(ServiceError::Unavailable)?;
        let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        Ok(Self {
            value: Box::new(CompletedAcceptance { sequence, accepted }),
            _storage: storage,
        })
    }

    pub(super) fn accepted(&self) -> &super::super::acceptance::ReplayedAcceptance {
        &self.value.accepted
    }

    pub(super) fn matches(
        &self,
        wanted: &Value,
        origin: &LocatedWrappers,
        prefix: &mut FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        let accepted = self.accepted();
        if !matches_binding(accepted.binding.reference(), wanted, s)? {
            return Ok(false);
        }
        if !matches_context(accepted.original_key(), origin.key(), s)? {
            return Ok(false);
        }
        let introduced = prefix._inventory.introduction_sequence(wanted, s)?;
        check_origin(
            self.value.sequence,
            origin.wrappers().sequence(),
            introduced,
            s,
        )?;
        prefix._inventory.compare_record(
            accepted.binding.reference(),
            accepted.binding.bytes(),
            s,
        )?;
        Ok(true)
    }
}

fn matches_binding(
    held: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    wanted: &Value,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    super::super::acceptance::validate_reference(wanted, "workflow-binding", s)?;
    for (name, actual) in [
        ("kind", &held.kind),
        ("id", &held.id),
        ("digest", &held.digest),
    ] {
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if !super::super::acceptance::same_recipe_text(
            actual,
            request_wrapper::text(wanted, name, s)?,
            s,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn matches_context(
    accepted: &OperationKey,
    activation: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    for (a, b) in [
        (&accepted.logical_store_id, &activation.logical_store_id),
        (&accepted.scope[0], &activation.scope[0]),
        (&accepted.scope[1], &activation.scope[1]),
    ] {
        if !super::super::acceptance::same_recipe_text(a, b, s)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn check_origin(
    sequence: i64,
    before: i64,
    introduced: i64,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &introduced.into()])
        .map_err(core_error)?;
    require(sequence == introduced, "INTEGRITY_FAILURE")?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &before.into()])
        .map_err(core_error)?;
    require(sequence < before, "INTEGRITY_FAILURE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_acceptance_reuse_requires_exact_binding_scope_and_prior_introduction() {
        use ledgerlab_core::{
            canonical::audit_arena::AuditArena, policy::workflow::transition_constructor::ExactRef,
        };
        let arena = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let digest = format!("sha256:{}", "a".repeat(64));
        let held = ExactRef {
            kind: "workflow-binding".into(),
            id: format!("wdoc_{}", "a".repeat(64)),
            digest,
        };
        let wanted = serde_json::json!({"kind":held.kind,"id":held.id,"digest":held.digest});
        assert!(matches_binding(&held, &wanted, &mut s).unwrap());
        let mut other = wanted.clone();
        other["id"] = serde_json::json!(format!("wdoc_{}", "b".repeat(64)));
        assert!(matches_binding(&held, &other, &mut s).is_err());
        other = wanted.clone();
        other["digest"] = serde_json::json!(format!("sha256:{}", "b".repeat(64)));
        assert!(matches_binding(&held, &other, &mut s).is_err());
        other["id"] = serde_json::json!(format!("wdoc_{}", "b".repeat(64)));
        assert!(!matches_binding(&held, &other, &mut s).unwrap());
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["tenant".into(), "environment".into()],
            source: "source".into(),
            operation_id: "accept".into(),
        };
        let mut next = key.clone();
        next.operation_id = "activate".into();
        assert!(matches_context(&key, &next, &mut s).unwrap());
        check_origin(15, 16, 15, &mut s).unwrap();
        for (before, introduced) in [(15, 15), (14, 15), (16, 14), (16, 16)] {
            assert!(check_origin(15, before, introduced, &mut s).is_err());
        }
        for field in 0..3 {
            let mut wrong = next.clone();
            match field {
                0 => wrong.logical_store_id = "other".into(),
                1 => wrong.scope[0] = "other".into(),
                _ => wrong.scope[1] = "other".into(),
            }
            assert!(!matches_context(&key, &wrong, &mut s).unwrap());
        }
    }
}
