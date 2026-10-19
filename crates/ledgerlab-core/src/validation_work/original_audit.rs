//! One original-recipe resource owner, shared by host framing and core replay.
//! It conveys no source/origin authority. The containing host attempt owns one
//! arena; each per-origin session shares that SAME quota, never a fresh pool.
pub(super) mod representation;
use super::{economic_meter::Meter, ValidationWork};
use crate::{canonical::audit_arena::AuditArena, Result};
pub use representation::{RepresentationKind, ResourceVectorUsage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalAuditUsage {
    pub semantic_work: u64,
    pub remaining_work: Option<u64>,
    pub framing_scanned_bytes: u64,
    pub framing_hashed_bytes: u64,
    pub framing_written_bytes: u64,
}
/// No Clone/Serde/maximum/reset/paid-credit or replacement setters. The same
/// mutable session passes from host framing through core replay to completion.
pub struct OriginalAuditSession {
    arena: AuditArena,
    meter: Meter,
}
impl OriginalAuditSession {
    pub fn new(attempt_arena: &AuditArena) -> Result<Self> {
        let _active_arena = attempt_arena.usage()?;
        let mut meter =
            Meter::original_audit_with_limits(attempt_arena.resource_limits().copied())?;
        meter.set_attempt_cancellation(attempt_arena.cancellation_handle());
        Ok(Self {
            arena: attempt_arena.shared_handle(),
            meter,
        })
    }
    /// Host checks this same terminal flag at every I/O boundary. Callback paths
    /// poll before EVERY S/C operation, tighter than the required4096 cadence.
    pub fn check_cancelled(&mut self) -> Result<()> {
        self.meter.check_attempt_cancellation()
    }
    pub fn reserve_external_payload(
        &mut self,
        bytes: usize,
    ) -> Result<crate::canonical::audit_arena::ExternalBufferLease> {
        self.arena
            .reserve_external_payload(bytes, &mut self.meter)
            .inspect_err(|_| self.meter.fail_original())
    }
    /// Retained owning DATA must remain charged to this same attempt.
    pub fn require_external_owner(
        &mut self,
        lease: &crate::canonical::audit_arena::ExternalBufferLease,
    ) -> Result<()> {
        self.arena
            .require_external_owner(lease, &mut self.meter)
            .inspect_err(|_| self.meter.fail_original())
    }
    pub fn parts(&mut self) -> (&AuditArena, &mut dyn ValidationWork) {
        (&self.arena, &mut self.meter)
    }
    pub fn work(&mut self) -> &mut dyn ValidationWork {
        &mut self.meter
    }
    pub fn resource_limits(&self) -> Option<&super::ResourceLimits> {
        self.arena.resource_limits()
    }
    pub fn arena(&self) -> &AuditArena {
        &self.arena
    }
    /// Irreversibly discard this resource attempt after any host/core failure.
    /// This grants no success state and cannot restore its allowance.
    pub fn invalidate(&mut self) {
        self.meter.fail_original();
    }
    pub fn usage(&self) -> OriginalAuditUsage {
        let remaining = self.meter.original_remaining_budget();
        let u = self.meter.usage();
        OriginalAuditUsage {
            semantic_work: self.meter.original_total_work(),
            remaining_work: remaining,
            framing_scanned_bytes: u.scanned_bytes,
            framing_hashed_bytes: u.hashed_bytes,
            framing_written_bytes: u.canonical_bytes_written,
        }
    }
    pub(crate) fn meter_mut(&mut self) -> &mut Meter {
        &mut self.meter
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_core_child_and_host_post_share_one_allowance_and_arena() {
        use crate::validation_work::ValidationVisit;
        fn must_send<T: Send>() {}
        must_send::<OriginalAuditSession>();
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::new(&arena).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        let mut child = session.meter_mut().original_economic_meter().unwrap();
        child.visit("test.economic_child").unwrap();
        let _completed_child = session.meter_mut().finish_original_economic(child).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        assert_eq!(session.usage().semantic_work, 3);
        assert_eq!(session.usage().remaining_work, Some(1_048_573));
        assert!(session.meter_mut().original_economic_meter().is_err());
        let (owned, work) = session.parts();
        let bytes = owned.copy_bytes(b"same owner", work).unwrap();
        assert_eq!(owned.usage().unwrap(), arena.usage().unwrap());
        drop(bytes);
    }
    #[test]
    fn historical_compile_child_consumes_origin_allowance_before_host_completion() {
        use crate::validation_work::ValidationVisit;
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::new(&arena).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        let mut compile = session.meter_mut().original_compile_meter().unwrap();
        compile.enter_compile().unwrap();
        compile.visit("test.saved_compiler").unwrap();
        let usage = session
            .meter_mut()
            .finish_original_compile(compile)
            .unwrap();
        assert_eq!(usage.semantic_work, 1);
        session.work().visit(ValidationVisit::Constraint).unwrap();
        assert_eq!(session.usage().semantic_work, 3);
        assert!(session.meter_mut().original_economic_meter().is_err());
        assert!(session.meter_mut().original_compile_meter().is_err());
    }
    #[test]
    fn caught_arithmetic_failure_cannot_complete_or_resume_origin() {
        use crate::validation_work::ValidationVisit;
        use num_bigint::BigInt;
        let arena = AuditArena::new().unwrap();
        for result_overflow in [false, true] {
            let mut session = OriginalAuditSession::new(&arena).unwrap();
            let mut child = session.meter_mut().original_economic_meter().unwrap();
            if result_overflow {
                let operand = BigInt::from(1) << 511usize;
                assert!(super::super::economic_meter::mul(&mut child, &operand, &operand).is_err());
            } else {
                let operand = BigInt::from(1) << 513usize;
                assert!(child.arithmetic("test.overflow", &[&operand]).is_err());
            }
            assert!(session.meter_mut().finish_original_economic(child).is_err());
            assert!(session.work().visit(ValidationVisit::Constraint).is_err());
            assert!(session.work().scan_bytes(1).is_err());
        }
    }
    #[test]
    fn outstanding_dropped_or_failed_child_cannot_leave_parent_spendable() {
        use crate::validation_work::ValidationVisit;
        let arena = AuditArena::new().unwrap();
        let mut dropped = OriginalAuditSession::new(&arena).unwrap();
        let child = dropped.meter_mut().original_economic_meter().unwrap();
        drop(child);
        assert!(dropped.work().visit(ValidationVisit::Constraint).is_err());
        assert!(dropped.work().scan_bytes(1).is_err());
        let mut outstanding = OriginalAuditSession::new(&arena).unwrap();
        let mut child = outstanding.meter_mut().original_economic_meter().unwrap();
        assert!(outstanding.work().hash_bytes(1).is_err());
        assert!(child.visit("test.after_parent_violation").is_err());
        assert!(outstanding
            .meter_mut()
            .finish_original_economic(child)
            .is_err());
        assert!(outstanding
            .work()
            .visit(ValidationVisit::Constraint)
            .is_err());
        let mut wrong = OriginalAuditSession::new(&arena).unwrap();
        let child = wrong.meter_mut().original_compile_meter().unwrap();
        assert!(wrong.meter_mut().finish_original_economic(child).is_err());
        assert!(wrong.work().write_bytes(1).is_err());
    }
}
