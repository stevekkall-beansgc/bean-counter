//! Executed semantic work, separate from byte reads/encodings. This meter is
//! resource refusal data only; it is never a proof of capacity or authority.
use crate::{Error, Result};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::Signed;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering as AtomicOrdering},
    },
};

pub const MAXIMUM_WORK: u64 = 65_536;
pub const MAXIMUM_BITS: u64 = 512;
pub const MAXIMUM_COMPILE_WORK: u64 = 1_048_576;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Compile,
    ParseNormalize,
    Resolve,
    OriginalAuthority,
    CurrentAuthority,
    FactsEvidence,
    GraphSelect,
    Pricing,
    Construct,
    Compare,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Charge {
    pub phase: Phase,
    pub kind: &'static str,
    pub operands: Vec<u64>,
    pub cost: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub raw_request_bytes: u64,
    pub unique_resolved_bytes: u64,
    pub unique_resolved_refs: u64,
    pub new_body_bytes: u64,
    pub scanned_bytes: u64,
    pub hashed_bytes: u64,
    pub semantic_work: u64,
    pub body_bytes_read: u64,
    pub canonical_bytes_written: u64,
    pub trace: Vec<Charge>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDomain {
    Compile,
    Draft,
    OriginalAudit,
    #[cfg(test)]
    Calibration,
}
// Code-owned lifecycle:0 unused,1 child outstanding,2 consumed,3 failed.
// Dropping an unfinished child irreversibly closes the parent. No allowance is
// refunded and no public state/reset accessor exists.
struct AuditChildToken(Arc<AtomicU8>);
#[derive(Clone, Copy, PartialEq, Eq)]
enum EconomicChildFlavor {
    Original,
    Local,
}
impl Drop for AuditChildToken {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(1, 3, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst);
    }
}
pub struct Meter {
    domain: BudgetDomain,
    maximum: u64,
    work_limit: Option<u64>,
    work_setting: &'static str,
    work_offset: u64,
    resource_limits: Option<super::ResourceLimits>,
    completed_child_work: u64,
    original_inner_started: bool,
    original_owner: Option<Arc<AtomicU8>>,
    original_child_token: Option<AuditChildToken>,
    economic_child_flavor: Option<EconomicChildFlavor>,
    resource_v2: bool,
    representation_arena: Option<crate::canonical::audit_arena::AuditArena>,
    cancellation: Option<crate::canonical::audit_arena::AuditCancellation>,
    resource_vector: Option<super::original_audit::representation::ResourceLedger>,
    phase: Phase,
    usage: Usage,
    resolved: BTreeMap<String, Vec<u8>>,
    borrowed_inventory_claimed: bool,
    #[cfg(test)]
    diagnostic: Option<Diagnostic>,
}
impl Meter {
    /// Fixed-v2 owner wiring only. This is the SAME whole-attempt arena, not a
    /// replacement/reset factory or an authorization capability.
    pub(in crate::validation_work) fn attach_representation_arena(
        &mut self,
        arena: &crate::canonical::audit_arena::AuditArena,
    ) -> Result<()> {
        if !self.resource_v2 || self.representation_arena.is_some() {
            self.fail_original();
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "representation arena owner already fixed",
            ));
        }
        self.representation_arena = Some(arena.shared_handle());
        Ok(())
    }
    pub(crate) fn representation_arena(&self) -> Option<crate::canonical::audit_arena::AuditArena> {
        self.representation_arena
            .as_ref()
            .map(|a| a.shared_handle())
    }
    /// A lower caller budget may refuse computation; it cannot widen the fixed
    /// profile ceiling or qualify any draft for persistence.
    pub fn new(maximum: u64) -> Result<Self> {
        if maximum > MAXIMUM_WORK {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "selected semantic ceiling",
            ));
        }
        Ok(Self {
            domain: BudgetDomain::Draft,
            maximum,
            work_limit: Some(maximum),
            work_setting: "semantic_work",
            work_offset: 0,
            resource_limits: None,
            completed_child_work: 0,
            original_inner_started: false,
            original_owner: None,
            original_child_token: None,
            economic_child_flavor: None,
            resource_v2: false,
            representation_arena: None,
            cancellation: None,
            resource_vector: None,
            phase: Phase::ParseNormalize,
            resolved: BTreeMap::new(),
            borrowed_inventory_claimed: false,
            #[cfg(test)]
            diagnostic: None,
            usage: Usage {
                raw_request_bytes: 0,
                unique_resolved_bytes: 0,
                unique_resolved_refs: 0,
                new_body_bytes: 0,
                scanned_bytes: 0,
                hashed_bytes: 0,
                semantic_work: 0,
                body_bytes_read: 0,
                canonical_bytes_written: 0,
                trace: vec![],
            },
        })
    }
    /// AUTH02 original-recipe framing domain. It cannot enter compile/draft.
    /// No caller-selected maximum or production admission is exposed.
    pub(crate) fn original_audit() -> Result<Self> { Self::original_audit_with_limits(None) }
    pub(crate) fn original_audit_with_limits(limits: Option<super::ResourceLimits>) -> Result<Self> {
        if let Some(limits) = limits { limits.validate()?; }
        let mut meter = Self::new(MAXIMUM_WORK)?;
        meter.domain = BudgetDomain::OriginalAudit;
        meter.original_owner = Some(Arc::new(AtomicU8::new(0)));
        meter.maximum = 1_048_576;
        meter.resource_limits = limits;
        meter.work_limit = limits.map_or(Some(1_048_576), |l| l.semantic_work);
        Ok(meter)
    }
    pub(crate) fn original_remaining_work(&self) -> u64 { self.maximum.saturating_sub(self.usage.semantic_work) }
    pub(crate) fn original_remaining_budget(&self) -> Option<u64> { self.work_limit.map(|n| n.saturating_sub(self.usage.semantic_work)) }
    pub(crate) fn original_total_work(&self) -> u64 { self.usage.semantic_work + self.completed_child_work }
    pub(crate) fn resource_limits(&self) -> Option<&super::ResourceLimits> { self.resource_limits.as_ref() }
    fn work_refusal(&self, next:u64)->Error {
        if let Some(limits)=self.resource_limits {
            let (limit,observed)=if self.work_setting=="compile_work" {(limits.compile_work,next)}else{(limits.semantic_work,match next.checked_add(self.work_offset).and_then(|n|n.checked_add(self.completed_child_work)){Some(n)=>n,None=>return Error::new("PROGRAM_BUDGET_EXCEEDED","semantic counter overflow")})};
            if let Some(limit)=limit { return super::resource_limits::refusal(self.work_setting,limit,observed); }
        }
        Error::new("PROGRAM_BUDGET_EXCEEDED","selected semantic work exhausted or counter overflow")
    }
    fn exhausted(&self, next: u64) -> bool { self.completed_child_work.checked_add(next).and_then(|n|n.checked_add(self.work_offset)).is_none() || self.work_limit.is_some_and(|limit| next > limit) }
    pub(crate) fn resolved_limits(&self) -> (Option<usize>, Option<usize>) {
        self.resource_limits.map_or((Some(4096),Some(8388608)), |l| (l.resolved_records,l.resolved_bytes))
    }
    pub(crate) fn check_resolved_dimensions(&mut self, records: usize, bytes: usize) -> Result<()> {
        let (record_limit,byte_limit)=self.resolved_limits();
        if let Some(limit)=record_limit { if crate::validation_work::compare(self, records, limit)?.is_gt() { return Err(super::resource_limits::refusal("resolved_records",limit as u64,records as u64)); } }
        if let Some(limit)=byte_limit { if crate::validation_work::compare(self, bytes, limit)?.is_gt() { return Err(super::resource_limits::refusal("resolved_bytes",limit as u64,bytes as u64)); } }
        Ok(())
    }
    /// Reserve a single tighter original economic invocation inside this audit.
    /// Its lower maximum ensures the composed cap BEFORE each inner operation.
    pub(crate) fn original_economic_meter(&mut self) -> Result<Self> {
        self.begin_economic_child(EconomicChildFlavor::Original)
    }
    /// The exact local recipe shares its parent's actual remaining allowance;
    /// it has no independent 65,536 ceiling or replacement allowance.
    pub(crate) fn local_economic_meter(&mut self) -> Result<Self> {
        self.begin_economic_child(EconomicChildFlavor::Local)
    }
    fn begin_economic_child(&mut self, flavor: EconomicChildFlavor) -> Result<Self> {
        if self.domain != BudgetDomain::OriginalAudit
            || (self.resource_limits.is_none() && self.maximum != 1_048_576)
            || self.original_inner_started
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original economic phase already consumed",
            ));
        }
        let remaining = self.original_remaining_budget().unwrap_or(MAXIMUM_COMPILE_WORK);
        let mut inner = Self::new(remaining.min(MAXIMUM_WORK))?;
        if flavor == EconomicChildFlavor::Local {
            inner.maximum = remaining;
            inner.work_limit = self.original_remaining_budget();
            inner.resource_limits = self.resource_limits;
            inner.work_offset = self.original_total_work();
        } else if self.resource_limits.is_some() {
            return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE", "operator resources require local child"));
        }
        inner.economic_child_flavor = Some(flavor);
        let owner = self
            .original_owner
            .as_ref()
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "missing original owner"))?;
        if owner
            .compare_exchange(0, 1, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
            .is_err()
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original child lifecycle closed",
            ));
        }
        inner.original_owner = Some(Arc::clone(owner));
        inner.original_child_token = Some(AuditChildToken(Arc::clone(owner)));
        inner.resource_v2 = self.resource_v2;
        inner.representation_arena = self
            .representation_arena
            .as_ref()
            .map(|a| a.shared_handle());
        inner.cancellation = self.cancellation.clone();
        inner.resource_vector = self.resource_vector.take();
        self.original_inner_started = true;
        Ok(inner)
    }
    /// Saved publication recipes execute their saved compiler under the same
    /// per-origin allowance; this is not a second compiler allowance.
    pub(crate) fn original_compile_meter(&mut self) -> Result<Self> {
        if self.domain != BudgetDomain::OriginalAudit
            || (self.resource_limits.is_none() && self.maximum != 1_048_576)
            || self.original_inner_started
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original compiler phase already consumed",
            ));
        }
        let remaining = self.original_remaining_budget().unwrap_or(MAXIMUM_COMPILE_WORK);
        let mut inner = Self::for_compile(remaining.min(MAXIMUM_COMPILE_WORK))?;
        if let Some(limits) = self.resource_limits {
            inner.resource_limits = Some(limits);
            inner.work_offset = self.original_total_work();
            if limits.compile_work.is_some_and(|n|self.original_remaining_budget().is_none_or(|remaining|n<remaining)) {
                inner.work_setting="compile_work";
            }
            inner.work_limit = match (self.original_remaining_budget(),limits.compile_work) {
                (Some(a),Some(b))=>Some(a.min(b)), (Some(a),None)=>Some(a), (None,Some(b))=>Some(b), (None,None)=>None
            };
        }
        let owner = self
            .original_owner
            .as_ref()
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "missing original owner"))?;
        if owner
            .compare_exchange(0, 1, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
            .is_err()
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original child lifecycle closed",
            ));
        }
        inner.original_owner = Some(Arc::clone(owner));
        inner.original_child_token = Some(AuditChildToken(Arc::clone(owner)));
        inner.resource_v2 = self.resource_v2;
        inner.representation_arena = self
            .representation_arena
            .as_ref()
            .map(|a| a.shared_handle());
        inner.cancellation = self.cancellation.clone();
        inner.resource_vector = self.resource_vector.take();
        self.original_inner_started = true;
        Ok(inner)
    }
    pub(crate) fn finish_original_compile(&mut self, inner: Self) -> Result<Usage> {
        let same_owner = match (&self.original_owner, &inner.original_owner) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same_owner
            || self.domain != BudgetDomain::OriginalAudit
            || (self.resource_limits.is_none() && self.maximum != 1_048_576)
            || !self.original_inner_started
            || inner.domain != BudgetDomain::Compile
            || inner.resource_limits != self.resource_limits
            || (inner.resource_limits.is_none() && inner.maximum > MAXIMUM_COMPILE_WORK)
        {
            if let Some(owner) = &self.original_owner {
                owner.store(3, AtomicOrdering::SeqCst);
            }
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "wrong original compiler meter",
            ));
        }
        let remaining_limit = match self.work_limit { Some(n)=>Some(n.checked_sub(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed work"))?), None=>None };
        let allowance = if self.resource_limits.is_none() { self.maximum.checked_sub(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed work"))? } else { self.maximum };
        if remaining_limit.is_some_and(|n| self.usage.semantic_work > n) {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original composed compiler work",
            ));
        }
        let owner = self
            .original_owner
            .as_ref()
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "missing original owner"))?;
        if inner.original_child_token.is_none()
            || owner
                .compare_exchange(1, 2, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
                .is_err()
        {
            owner.store(3, AtomicOrdering::SeqCst);
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original child did not complete",
            ));
        }
        self.maximum = allowance;
        self.work_limit = remaining_limit;
        self.completed_child_work = self.completed_child_work.checked_add(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "composed usage overflow"))?;
        let mut inner = inner;
        self.resource_vector = inner.resource_vector.take();
        Ok(inner.finish())
    }
    /// Subsequent framing retains only the actual remaining composed allowance.
    /// This is counter composition, not a charge refund or semantic exemption.
    pub(crate) fn finish_original_economic(&mut self, inner: Self) -> Result<Usage> {
        self.finish_economic_child(inner, EconomicChildFlavor::Original)
    }
    pub(crate) fn finish_local_economic(&mut self, inner: Self) -> Result<Usage> {
        self.finish_economic_child(inner, EconomicChildFlavor::Local)
    }
    fn finish_economic_child(&mut self, inner: Self, flavor: EconomicChildFlavor) -> Result<Usage> {
        let same_owner = match (&self.original_owner, &inner.original_owner) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same_owner
            || self.domain != BudgetDomain::OriginalAudit
            || (self.resource_limits.is_none() && self.maximum != 1_048_576)
            || !self.original_inner_started
            || inner.domain != BudgetDomain::Draft
            || inner.economic_child_flavor != Some(flavor)
            || match flavor {
                EconomicChildFlavor::Original => inner.maximum > MAXIMUM_WORK,
                EconomicChildFlavor::Local => self.original_remaining_budget() != inner.work_limit || self.resource_limits != inner.resource_limits,
            }
        {
            if let Some(owner) = &self.original_owner {
                owner.store(3, AtomicOrdering::SeqCst);
            }
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "wrong original economic meter",
            ));
        }
        let remaining_limit = match self.work_limit { Some(n)=>Some(n.checked_sub(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed work"))?), None=>None };
        let allowance = if self.resource_limits.is_none() { self.maximum.checked_sub(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed work"))? } else { self.maximum };
        if remaining_limit.is_some_and(|n| self.usage.semantic_work > n) {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original composed work",
            ));
        }
        let owner = self
            .original_owner
            .as_ref()
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "missing original owner"))?;
        if inner.original_child_token.is_none()
            || owner
                .compare_exchange(1, 2, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
                .is_err()
        {
            owner.store(3, AtomicOrdering::SeqCst);
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original child did not complete",
            ));
        }
        self.maximum = allowance;
        self.work_limit = remaining_limit;
        self.completed_child_work = self.completed_child_work.checked_add(inner.usage.semantic_work).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "composed usage overflow"))?;
        let mut inner = inner;
        self.resource_vector = inner.resource_vector.take();
        Ok(inner.finish())
    }
    pub fn for_compile(maximum: u64) -> Result<Self> {
        if maximum > MAXIMUM_COMPILE_WORK {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "selected compile ceiling",
            ));
        }
        Ok(Self {
            domain: BudgetDomain::Compile,
            maximum,
            work_limit: Some(maximum),
            work_setting: "semantic_work",
            work_offset: 0,
            resource_limits: None,
            completed_child_work: 0,
            original_inner_started: false,
            original_owner: None,
            original_child_token: None,
            economic_child_flavor: None,
            resource_v2: false,
            representation_arena: None,
            cancellation: None,
            resource_vector: None,
            phase: Phase::Compile,
            resolved: BTreeMap::new(),
            borrowed_inventory_claimed: false,
            #[cfg(test)]
            diagnostic: None,
            usage: Usage {
                raw_request_bytes: 0,
                unique_resolved_bytes: 0,
                unique_resolved_refs: 0,
                new_body_bytes: 0,
                scanned_bytes: 0,
                hashed_bytes: 0,
                semantic_work: 0,
                body_bytes_read: 0,
                canonical_bytes_written: 0,
                trace: vec![],
            },
        })
    }
    pub(crate) fn enter_compile(&mut self) -> Result<()> {
        if self.domain != BudgetDomain::Compile
            || (self.resource_limits.is_none() && self.maximum > MAXIMUM_COMPILE_WORK)
            || self.usage.semantic_work != 0
            || !self.resolved.is_empty()
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "fresh compile invocation meter required",
            ));
        }
        self.phase = Phase::Compile;
        Ok(())
    }
    pub(crate) fn enter_draft(&mut self) -> Result<()> {
        if self.domain != BudgetDomain::Draft
            || self.maximum > MAXIMUM_WORK
            || self.usage.semantic_work != 0
            || !self.resolved.is_empty()
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "fresh draft invocation meter required",
            ));
        }
        self.phase = Phase::ParseNormalize;
        Ok(())
    }
    pub fn begin_request(&mut self, raw: &[u8]) -> Result<()> {
        self.visit("request.bound")?;
        if raw.len() > 262144 {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "raw request bytes"));
        }
        self.usage.raw_request_bytes = raw.len() as u64;
        Ok(())
    }
    /// Exactly one invocation-local borrowed inventory owns uniqueness. This
    /// changes neither semantic work nor quota and cannot be caller-selected.
    pub(crate) fn claim_borrowed_inventory(&mut self) -> Result<()> {
        self.visit("resolve.borrowed_inventory")?;
        if !self.resource_v2
            || self.domain != BudgetDomain::Draft
            || self.borrowed_inventory_claimed
        {
            self.fail_original();
            return Err(Error::new(
                "AUDIT_INTEGRITY_OR_PROFILE_FAILURE",
                "borrowed inventory ownership",
            ));
        }
        self.borrowed_inventory_claimed = true;
        Ok(())
    }
    /// Parent-paid DATA selection before the sole economic child. This grants
    /// no borrowed-economic-inventory claim, new quota, phase credit or child
    /// continuation. Every callback still uses this original's current ledger.
    pub(crate) fn require_original_data_selection(&mut self)->Result<()> {
        self.visit("resolve.parent_data_selection")?;
        if !self.resource_v2||self.domain!=BudgetDomain::OriginalAudit||self.original_inner_started{
            self.fail_original();
            return Err(Error::new("AUDIT_INTEGRITY_OR_PROFILE_FAILURE","parent DATA selection lifecycle"));
        }
        Ok(())
    }
    /// Only Records' unique immutable operand table calls this, once per body
    /// before consumption. The bytes are real borrowed bytes, not numeric work
    /// credits, caller authority or a cross-invocation cache. Exact frame/hash
    /// validation remains mandatory in the same table before any candidate.
    pub(crate) fn borrowed_body(&mut self, bytes: &[u8]) -> Result<()> {
        self.visit("resolve.unique_borrowed_body")?;
        if !self.resource_v2 || !self.borrowed_inventory_claimed {
            self.fail_original();
            return Err(Error::new(
                "AUDIT_INTEGRITY_OR_PROFILE_FAILURE",
                "no borrowed inventory owner",
            ));
        }
        let old_bytes = self.usage.unique_resolved_bytes as usize;
        let old_refs = self.usage.unique_resolved_refs;
        let next = crate::validation_work::usize_add(self, old_bytes, bytes.len())?;
        if self.resource_limits.is_some() {
            let refs = old_refs.checked_add(1).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED","resolved count overflow"))?;
            self.check_resolved_dimensions(usize::try_from(refs).map_err(|_|Error::new("PROGRAM_BUDGET_EXCEEDED","resolved count overflow"))?, next)?;
        } else if crate::validation_work::compare(self, next, 8_388_608usize)?.is_gt()
            || !crate::validation_work::compare(self, old_refs, 4096u64)?.is_lt()
        {
            self.fail_original();
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved closure"));
        }
        let refs = crate::validation_work::usize_add(self, old_refs as usize, 1)?;
        self.usage.unique_resolved_bytes = next as u64;
        self.usage.unique_resolved_refs = refs as u64;
        Ok(())
    }
    pub fn resolved_body(&mut self, identity: &str, bytes: &[u8]) -> Result<()> {
        self.visit("resolve.identity")?;
        if self.resolved.contains_key(identity) {
            self.visit("resolve.body_compare")?;
            if self.resolved.get(identity).expect("existing identity") != bytes {
                return Err(Error::new(
                    "INTEGRITY_FAILURE",
                    "conflicting resolved identity",
                ));
            }
            return Ok(());
        }
        let next = self
            .usage
            .unique_resolved_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved byte overflow"))?;
        if self.resource_limits.is_some() {
            let refs=self.usage.unique_resolved_refs.checked_add(1).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED","resolved count overflow"))?;
            self.check_resolved_dimensions(usize::try_from(refs).map_err(|_| Error::new("PROGRAM_BUDGET_EXCEEDED","resolved count overflow"))?,usize::try_from(next).map_err(|_| Error::new("PROGRAM_BUDGET_EXCEEDED","resolved byte overflow"))?)?;
        } else if next > 8388608 || self.usage.unique_resolved_refs >= 4096 {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved closure"));
        }
        self.resolved.insert(identity.to_owned(), bytes.to_vec());
        self.usage.unique_resolved_bytes = next;
        self.usage.unique_resolved_refs = self.usage.unique_resolved_refs.checked_add(1).ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED","resolved count overflow"))?;
        Ok(())
    }
    pub fn scan_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        let result = (|| {
            let next = self
                .usage
                .scanned_bytes
                .checked_add(n as u64)
                .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "scan overflow"))?;
            if let Some(vector) = self.resource_vector.as_mut() {
                vector.bytes(n as u64, 0, 0)?;
            }
            self.usage.scanned_bytes = next;
            Ok(())
        })();
        if result.is_err() {
            self.fail_original();
        }
        result
    }
    pub fn hash_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        let result = (|| {
            let next = self
                .usage
                .hashed_bytes
                .checked_add(n as u64)
                .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "hash overflow"))?;
            if let Some(vector) = self.resource_vector.as_mut() {
                vector.bytes(0, 0, n as u64)?;
            }
            self.usage.hashed_bytes = next;
            Ok(())
        })();
        if result.is_err() {
            self.fail_original();
        }
        result
    }
    pub(crate) fn remaining_new_body(&self) -> Result<Option<usize>> {
        let limit=self.resource_limits.map_or(Some(8388608), |l| l.new_body_bytes);
        let Some(limit)=limit else { return Ok(None); };
        (limit as u64)
            .checked_sub(self.usage.new_body_bytes)
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "new body byte allowance"))
    }
    pub fn new_body(&mut self, n: usize) -> Result<()> {
        let next = self
            .usage
            .new_body_bytes
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "new body overflow"))?;
        if self.resource_limits.map_or(Some(8388608u64), |l| l.new_body_bytes.map(|n|n as u64)).is_some_and(|limit|next>limit) {
            return Err(if let Some(limits)=self.resource_limits {super::resource_limits::refusal("new_body_bytes",limits.new_body_bytes.expect("finite exceeded limit") as u64,next)}else{Error::new("PROGRAM_BUDGET_EXCEEDED", "new body bytes")});
        }
        self.usage.new_body_bytes = next;
        Ok(())
    }
    pub fn phase(&mut self, phase: Phase) {
        self.phase = phase;
    }
    pub fn usage(&self) -> &Usage {
        &self.usage
    }
    pub fn finish(self) -> Usage {
        self.usage
    }
    pub(crate) fn enable_resource_profile_v2(&mut self) -> Result<()> {
        self.require_original_lifecycle()?;
        if self.domain != BudgetDomain::OriginalAudit
            || self.usage.semantic_work != 0
            || self.original_inner_started
            || self.resource_v2
        {
            return Err(Error::new(
                "AUDIT_INTEGRITY_OR_PROFILE_FAILURE",
                "resource profile selection must precede execution",
            ));
        }
        self.resource_v2 = true;
        self.resource_vector = Some(if let Some(limits)=self.resource_limits{super::original_audit::representation::ResourceLedger::with_semantic_limit(limits.semantic_work)}else{super::original_audit::representation::ResourceLedger::new()});
        Ok(())
    }
    pub(crate) fn begin_representation(&mut self) -> Result<()> {
        self.require_original_lifecycle()?;
        self.resource_vector
            .as_mut()
            .ok_or_else(|| {
                Error::new(
                    "UNSUPPORTED_SCHEMA_PROFILE",
                    "representation operations require v2",
                )
            })?
            .begin()
    }
    pub(crate) fn resource_vector_usage(
        &self,
    ) -> Option<super::original_audit::representation::ResourceVectorUsage> {
        self.resource_vector.as_ref().map(|v| v.usage())
    }
    pub(crate) fn representation_event(
        &mut self,
        kind: &'static str,
        operands: &[&BigInt],
        cost: u64,
    ) -> Result<()> {
        self.require_original_lifecycle()?;
        self.resource_vector
            .as_mut()
            .ok_or_else(|| {
                Error::new(
                    "UNSUPPORTED_SCHEMA_PROFILE",
                    "representation operations require v2",
                )
            })?
            .event(
                true,
                kind,
                cost,
                operands.iter().map(|v| v.bits().div_ceil(64).max(1)),
            )
    }
    fn v2_semantic_event(
        &mut self,
        kind: &'static str,
        cost: u64,
        operands: impl Iterator<Item = u64>,
    ) -> Result<()> {
        self.require_original_lifecycle()?;
        let next = self
            .usage
            .semantic_work
            .checked_add(cost)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "work overflow"))?;
        if self.exhausted(next) {
            self.fail_original();
            return Err(if self.resource_limits.is_some(){self.work_refusal(next)}else{Error::new("PROGRAM_BUDGET_EXCEEDED","selected semantic work exhausted")});
        }
        let result = self
            .resource_vector
            .as_mut()
            .ok_or_else(|| {
                Error::new(
                    "AUDIT_INTEGRITY_OR_PROFILE_FAILURE",
                    "missing owned resource vector",
                )
            })?
            .event(false, kind, cost, operands);
        if result.is_err() {
            self.fail_original();
            return result;
        }
        self.usage.semantic_work = next;
        Ok(())
    }
    pub(crate) fn fail_original(&self) {
        if let Some(owner) = &self.original_owner {
            owner.store(3, AtomicOrdering::SeqCst);
        }
    }
    fn arithmetic_result(&self, value: BigInt) -> Result<BigInt> {
        bounded(value).inspect_err(|_| self.fail_original())
    }
    pub(crate) fn set_attempt_cancellation(
        &mut self,
        handle: crate::canonical::audit_arena::AuditCancellation,
    ) {
        self.cancellation = Some(handle);
    }
    pub(crate) fn check_attempt_cancellation(&self) -> Result<()> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|handle| handle.is_cancelled())
        {
            self.fail_original();
            return Err(Error::new(
                "AUDIT_CANCELLED",
                "audit attempt interrupted without financial effect",
            ));
        }
        Ok(())
    }
    fn require_original_lifecycle(&self) -> Result<()> {
        self.check_attempt_cancellation()?;
        if let Some(owner) = &self.original_owner {
            let state = owner.load(AtomicOrdering::SeqCst);
            if state == 3 || (self.domain == BudgetDomain::OriginalAudit && state == 1) {
                owner.store(3, AtomicOrdering::SeqCst);
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "original child outstanding or failed",
                ));
            }
        }
        Ok(())
    }
    #[track_caller]
    fn charge(&mut self, kind: &'static str, operands: Vec<u64>, cost: u64) -> Result<()> {
        self.require_original_lifecycle()?;
        if self.resource_v2 {
            return self.v2_semantic_event(kind, cost, operands.into_iter());
        }
        let next = self
            .usage
            .semantic_work
            .checked_add(cost)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "work overflow"))?;
        #[cfg(test)]
        if let Some(diagnostic) = &mut self.diagnostic {
            let location = std::panic::Location::caller();
            if operands.capacity() > DIAGNOSTIC_OPERANDS
                || diagnostic.sites.len() > DIAGNOSTIC_SITES
            {
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "diagnostic metadata containment",
                ));
            }
            // Outer capacity is preallocated. Nested capacities and a fixed
            // worst-case reservation for both saved Attempts plus one temporary
            // are checked BEFORE cloning sites/operands or pushing either vector.
            let nested = operands.capacity() * std::mem::size_of::<u64>()
                + diagnostic.sites.len() * std::mem::size_of::<&str>();
            let storage = diagnostic.storage.checked_add(nested).ok_or_else(|| {
                Error::new("PROGRAM_BUDGET_EXCEEDED", "diagnostic storage overflow")
            })?;
            if next <= self.maximum
                && (self.usage.trace.len() >= diagnostic.event_capacity
                    || storage > DIAGNOSTIC_STORAGE)
            {
                diagnostic.refusal = Some(diagnostic.attempt(
                    location,
                    self.phase,
                    kind,
                    &operands,
                    cost,
                    self.usage.semantic_work,
                    next,
                ));
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "diagnostic trace containment",
                ));
            }
            if diagnostic.crossing.is_none() && next > MAXIMUM_WORK {
                diagnostic.crossing = Some(diagnostic.attempt(
                    location,
                    self.phase,
                    kind,
                    &operands,
                    cost,
                    self.usage.semantic_work,
                    next,
                ));
            }
            if self.work_limit.is_some_and(|limit|next>limit) {
                diagnostic.refusal = Some(diagnostic.attempt(
                    location,
                    self.phase,
                    kind,
                    &operands,
                    cost,
                    self.usage.semantic_work,
                    next,
                ));
            } else {
                diagnostic.storage = storage;
                diagnostic.peak = diagnostic.peak.max(storage);
                let mut sites = Vec::with_capacity(diagnostic.sites.len());
                sites.extend_from_slice(&diagnostic.sites);
                diagnostic.callers.push(Caller {
                    file: location.file(),
                    line: location.line(),
                    sites,
                });
            }
        }
        if self.exhausted(next) {
            self.fail_original();
            return Err(if self.resource_limits.is_some(){self.work_refusal(next)}else{Error::new("PROGRAM_BUDGET_EXCEEDED","selected semantic work exhausted")});
        }
        self.usage.semantic_work = next;
        self.usage.trace.push(Charge {
            phase: self.phase,
            kind,
            operands,
            cost,
        });
        Ok(())
    }
    #[track_caller]
    pub fn visit(&mut self, kind: &'static str) -> Result<()> {
        self.charge(kind, vec![], 1)
    }
    #[track_caller]
    pub fn check(&mut self, kind: &'static str, test: impl FnOnce() -> bool) -> Result<bool> {
        self.visit(kind)?;
        Ok(test())
    }
    #[track_caller]
    pub fn arithmetic(&mut self, kind: &'static str, operands: &[&BigInt]) -> Result<()> {
        if self.resource_v2 {
            let mut cost = 1u64;
            for value in operands {
                if value.bits() > MAXIMUM_BITS {
                    self.fail_original();
                    return Err(Error::new(
                        "ARITHMETIC_OVERFLOW",
                        "512-bit selected operand",
                    ));
                }
                cost = cost
                    .checked_add(value.bits().div_ceil(64).max(1))
                    .ok_or_else(|| {
                        Error::new("PROGRAM_BUDGET_EXCEEDED", "arithmetic cost overflow")
                    })?;
            }
            return self.v2_semantic_event(
                kind,
                cost,
                operands.iter().map(|v| v.bits().div_ceil(64).max(1)),
            );
        }
        let limbs = operands
            .iter()
            .map(|n| {
                if n.bits() > MAXIMUM_BITS {
                    self.fail_original();
                    return Err(Error::new(
                        "ARITHMETIC_OVERFLOW",
                        "512-bit selected operand",
                    ));
                }
                Ok(n.bits().div_ceil(64).max(1))
            })
            .collect::<Result<Vec<_>>>()?;
        let cost = limbs
            .iter()
            .try_fold(1u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "arithmetic cost overflow"))?;
        self.charge(kind, limbs, cost)
    }
    pub fn read_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        let next = self
            .usage
            .body_bytes_read
            .checked_add(n as u64)
            .ok_or_else(|| {
                self.fail_original();
                Error::new("PROGRAM_BUDGET_EXCEEDED", "byte count overflow")
            })?;
        self.usage.body_bytes_read = next;
        Ok(())
    }
    pub fn write_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        let result = (|| {
            let next = self
                .usage
                .canonical_bytes_written
                .checked_add(n as u64)
                .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "byte count overflow"))?;
            if let Some(vector) = self.resource_vector.as_mut() {
                vector.bytes(0, n as u64, 0)?;
            }
            self.usage.canonical_bytes_written = next;
            Ok(())
        })();
        if result.is_err() {
            self.fail_original();
        }
        result
    }
}
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct Attempt {
    pub phase: Phase,
    pub kind: &'static str,
    pub operands: Vec<u64>,
    pub cost: u64,
    pub before: u64,
    pub after: u64,
    pub file: &'static str,
    pub line: u32,
    pub sites: Vec<&'static str>,
}
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct Caller {
    pub file: &'static str,
    pub line: u32,
    pub sites: Vec<&'static str>,
}
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct Diagnostic {
    pub sites: Vec<&'static str>,
    pub crossing: Option<Attempt>,
    pub refusal: Option<Attempt>,
    pub callers: Vec<Caller>,
    pub storage: usize,
    pub peak: usize,
    pub event_capacity: usize,
    pub usage_copy_bytes: usize,
}
#[cfg(test)]
impl Meter {
    // This private domain cannot enter the product constructor. It is only a
    // finite non-admitting experiment over the same executing implementation.
    pub(crate) fn calibration(maximum: u64) -> Result<Self> {
        if maximum > 262144 {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "diagnostic containment ceiling",
            ));
        }
        let mut meter = Self::new(MAXIMUM_WORK)?;
        meter.domain = BudgetDomain::Calibration;
        meter.maximum = maximum;
        meter.work_limit = Some(maximum);
        meter.attach_diagnostic()?;
        Ok(meter)
    }
    pub(crate) fn diagnosed_ordinary() -> Result<Self> {
        let mut meter = Self::new(MAXIMUM_WORK)?;
        meter.attach_diagnostic()?;
        Ok(meter)
    }
    fn attach_diagnostic(&mut self) -> Result<()> {
        let n = usize::try_from(self.maximum)
            .map_err(|_| Error::new("PROGRAM_BUDGET_EXCEEDED", "diagnostic capacity"))?;
        let outer = n
            .checked_mul(std::mem::size_of::<Charge>() + std::mem::size_of::<Caller>())
            .and_then(|x| x.checked_add(DIAGNOSTIC_FIXED))
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "diagnostic capacity"))?;
        if outer > DIAGNOSTIC_STORAGE {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "diagnostic initial storage",
            ));
        }
        self.usage.trace = Vec::with_capacity(n);
        let callers = Vec::with_capacity(n);
        let sites = Vec::with_capacity(DIAGNOSTIC_SITES);
        // Vec's backing capacity is recorded, not its current length. Pinned
        // Rust uses requested element capacity; allocator overhead/RSS is a
        // separate externally sampled limit, never claimed as this payload cap.
        let actual = self.usage.trace.capacity() * std::mem::size_of::<Charge>()
            + callers.capacity() * std::mem::size_of::<Caller>()
            + DIAGNOSTIC_FIXED;
        if actual > DIAGNOSTIC_STORAGE
            || self.usage.trace.capacity() != n
            || callers.capacity() != n
            || sites.capacity() != DIAGNOSTIC_SITES
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "diagnostic capacity mismatch",
            ));
        }
        self.diagnostic = Some(Diagnostic {
            sites,
            crossing: None,
            refusal: None,
            callers,
            storage: actual,
            peak: actual,
            event_capacity: n,
            usage_copy_bytes: 0,
        });
        Ok(())
    }
    pub(crate) fn enter_calibration(&mut self) -> Result<()> {
        if self.domain != BudgetDomain::Calibration
            || self.maximum > 262144
            || self.usage.semantic_work != 0
            || !self.resolved.is_empty()
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "fresh private calibration required",
            ));
        }
        Ok(())
    }
    pub(crate) fn diagnostic_phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn diagnostic_work_stop(&self) -> u64 {
        self.maximum
    }
    pub(crate) fn diagnostic(&self) -> Option<&Diagnostic> {
        self.diagnostic.as_ref()
    }
    pub(crate) fn diagnostic_scope<T>(
        &mut self,
        site: &'static str,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if let Some(d) = &mut self.diagnostic {
            if d.sites.len() >= 32 {
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "diagnostic caller depth",
                ));
            }
            d.sites.push(site);
        }
        let result = f(self);
        if let Some(d) = &mut self.diagnostic {
            d.sites.pop();
        }
        result
    }
}
impl Meter {
    pub(crate) fn scoped<T>(
        &mut self,
        site: &'static str,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        #[cfg(test)]
        {
            return self.diagnostic_scope(site, f);
        }
        #[cfg(not(test))]
        {
            let _ = site;
            f(self)
        }
    }
    #[track_caller]
    pub(crate) fn snapshot_usage(&mut self) -> Result<Usage> {
        #[cfg(test)]
        if let Some(d) = &mut self.diagnostic {
            // Vec::clone copies len elements into len capacity. Each nested
            // operand Vec is copied too. Reserve live returned projection cost
            // before executing the clone; reservation is never refunded here.
            let bytes = std::mem::size_of::<Usage>()
                + self.usage.trace.len() * std::mem::size_of::<Charge>()
                + self
                    .usage
                    .trace
                    .iter()
                    .map(|e| e.operands.len() * std::mem::size_of::<u64>())
                    .sum::<usize>();
            let next = d.storage.checked_add(bytes).ok_or_else(|| {
                Error::new("PROGRAM_BUDGET_EXCEEDED", "diagnostic projection storage")
            })?;
            if next > DIAGNOSTIC_STORAGE {
                d.refusal = Some(d.attempt(
                    std::panic::Location::caller(),
                    self.phase,
                    "diagnostic.projection.copy",
                    &[],
                    0,
                    self.usage.semantic_work,
                    self.usage.semantic_work,
                ));
                return Err(Error::new(
                    "PROGRAM_BUDGET_EXCEEDED",
                    "diagnostic projection storage",
                ));
            }
            d.storage = next;
            d.peak = d.peak.max(next);
            d.usage_copy_bytes = d
                .usage_copy_bytes
                .checked_add(bytes)
                .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "diagnostic copy bytes"))?;
        }
        Ok(self.usage.clone())
    }
}
#[cfg(test)]
const DIAGNOSTIC_STORAGE: usize = 48 * 1024 * 1024;
#[cfg(test)]
const DIAGNOSTIC_SITES: usize = 32;
#[cfg(test)]
const DIAGNOSTIC_OPERANDS: usize = 8;
// Header objects, the active site capacity and three full metadata Attempts
// (first crossing + refusal + transient construction) are always reserved.
#[cfg(test)]
const DIAGNOSTIC_FIXED: usize = std::mem::size_of::<Diagnostic>()
    + DIAGNOSTIC_SITES * std::mem::size_of::<&str>()
    + 3 * (std::mem::size_of::<Attempt>()
        + DIAGNOSTIC_SITES * std::mem::size_of::<&str>()
        + DIAGNOSTIC_OPERANDS * std::mem::size_of::<u64>());
#[cfg(test)]
impl Diagnostic {
    fn attempt(
        &self,
        location: &'static std::panic::Location<'static>,
        phase: Phase,
        kind: &'static str,
        operands: &[u64],
        cost: u64,
        before: u64,
        after: u64,
    ) -> Attempt {
        Attempt {
            phase,
            kind,
            operands: operands.to_vec(),
            cost,
            before,
            after,
            file: location.file(),
            line: location.line(),
            sites: self.sites.clone(),
        }
    }
}
pub(crate) fn bounded(n: BigInt) -> Result<BigInt> {
    if n.bits() > MAXIMUM_BITS {
        Err(Error::new("ARITHMETIC_OVERFLOW", "512-bit selected result"))
    } else {
        Ok(n)
    }
}
#[track_caller]
pub(crate) fn add(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.add", &[a, b])?;
    m.arithmetic_result(a + b)
}
#[track_caller]
pub(crate) fn sub(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.sub", &[a, b])?;
    m.arithmetic_result(a - b)
}
#[track_caller]
pub(crate) fn mul(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.mul", &[a, b])?;
    m.arithmetic_result(a * b)
}
#[track_caller]
pub(crate) fn div(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.div", &[a, b])?;
    if cmp(m, b, &BigInt::from(0))? == std::cmp::Ordering::Equal {
        m.fail_original();
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    m.arithmetic_result(a / b)
}
#[track_caller]
pub(crate) fn rem(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.rem", &[a, b])?;
    if cmp(m, b, &BigInt::from(0))? == std::cmp::Ordering::Equal {
        m.fail_original();
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    m.arithmetic_result(a % b)
}
#[track_caller]
pub(crate) fn neg(m: &mut Meter, a: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.neg", &[a])?;
    m.arithmetic_result(-a)
}
#[track_caller]
pub(crate) fn cmp(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<std::cmp::Ordering> {
    m.arithmetic("arith.compare", &[a, b])?;
    Ok(a.cmp(b))
}
#[track_caller]
pub(crate) fn absolute(m: &mut Meter, a: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.abs", &[a])?;
    Ok(a.abs())
}

#[track_caller]
pub(crate) fn divmod(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<(BigInt, BigInt)> {
    m.arithmetic("arith.divmod", &[a, b])?;
    if cmp(m, b, &BigInt::from(0))? == std::cmp::Ordering::Equal {
        m.fail_original();
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    let (q, r) = a.div_mod_floor(b);
    Ok((m.arithmetic_result(q)?, m.arithmetic_result(r)?))
}

impl crate::validation_work::ValidationWork for Meter {
    fn visit(&mut self, kind: crate::validation_work::ValidationVisit) -> Result<()> {
        use crate::validation_work::ValidationVisit::*;
        Meter::visit(
            self,
            match kind {
                Value => "validator.value",
                Field => "validator.field",
                Element => "validator.element",
                Constraint => "validator.constraint",
                ScalarDigit => "validator.scalar_digit",
                MemberCompare => "validator.member_compare",
                RefLookup => "validator.ref_lookup",
                Edge => "validator.edge",
                SortMove => "validator.sort_move",
            },
        )
    }
    fn arithmetic(
        &mut self,
        kind: crate::validation_work::ArithmeticOperation,
        operands: &[&BigInt],
    ) -> Result<()> {
        use crate::validation_work::ArithmeticOperation::*;
        Meter::arithmetic(
            self,
            match kind {
                Add => "arith.add",
                Subtract => "arith.sub",
                Multiply => "arith.mul",
                DivideRemainder => "arith.divmod",
                Compare => "arith.compare",
                Negate => "arith.neg",
                Absolute => "arith.abs",
                BitOr => "arith.bit_or",
                BitAnd => "arith.bit_and",
                PopulationCount => "arith.population_count",
                LeadingZeros => "arith.leading_zeros",
            },
            operands,
        )
    }
    fn scan_bytes(&mut self, bytes: usize) -> Result<()> {
        Meter::scan_bytes(self, bytes)
    }
    fn write_bytes(&mut self, bytes: usize) -> Result<()> {
        Meter::write_bytes(self, bytes)
    }
    fn hash_bytes(&mut self, bytes: usize) -> Result<()> {
        Meter::hash_bytes(self, bytes)
    }
}

pub(crate) fn timestamp_site(field: &str) -> &'static str {
    match field {
        "accepted_at" | "at" => "Timestamp::parse_with_work.accepted_at",
        "received_at" => "Timestamp::parse_with_work.received_at",
        "starts_at" => "Timestamp::parse_with_work.starts_at",
        "ends_at" => "Timestamp::parse_with_work.ends_at",
        "verified_at" => "Timestamp::parse_with_work.verified_at",
        "enrolled_at" => "Timestamp::parse_with_work.enrolled_at",
        "registered_at" => "Timestamp::parse_with_work.registered_at",
        "retained_at" => "Timestamp::parse_with_work.retained_at",
        "effective_at" | "not_before" => "Timestamp::parse_with_work.effective_at",
        "observed_at" => "Timestamp::parse_with_work.observed_at",
        _ => "Timestamp::parse_with_work.other_field",
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn trace_capacity_projection_and_transient_payload_are_prechecked() {
        let mut m = Meter::calibration(262144).unwrap();
        let cap = m.usage.trace.capacity();
        let d = m.diagnostic.as_ref().unwrap();
        assert_eq!(cap, 262144);
        assert_eq!(d.callers.capacity(), 262144);
        assert!(d.storage >= cap * (std::mem::size_of::<Charge>() + std::mem::size_of::<Caller>()));
        m.scoped("test.payload", |m| {
            m.arithmetic("test.add", &[&1.into(), &2.into()])
        })
        .unwrap();
        let snapshot = m.snapshot_usage().unwrap();
        assert_eq!(snapshot, m.usage);
        assert!(m.diagnostic.as_ref().unwrap().usage_copy_bytes > 0);
        let mut blocked = Meter::calibration(8).unwrap();
        blocked.diagnostic.as_mut().unwrap().storage = DIAGNOSTIC_STORAGE;
        assert!(
            blocked
                .scoped("test.refusal", |m| m
                    .arithmetic("test.refused", &[&1.into()]))
                .is_err()
        );
        assert!(blocked.usage.trace.is_empty());
        assert!(blocked.diagnostic.as_ref().unwrap().callers.is_empty());
        assert!(blocked.diagnostic.as_ref().unwrap().refusal.is_some());
        assert!(blocked.snapshot_usage().is_err());
        assert_eq!(blocked.diagnostic.as_ref().unwrap().usage_copy_bytes, 0);
    }
    #[test]
    fn shared_timestamp_callback_retains_field_site_without_changing_charges() {
        let mut ordinary = Meter::new(65536).unwrap();
        crate::domain::Timestamp::parse_with_work("2026-10-13T00:00:00.000000Z", &mut ordinary)
            .unwrap();
        let mut diagnosed = Meter::calibration(65536).unwrap();
        diagnosed
            .scoped(timestamp_site("accepted_at"), |m| {
                crate::domain::Timestamp::parse_with_work("2026-10-13T00:00:00.000000Z", m)
            })
            .unwrap();
        assert_eq!(ordinary.usage, diagnosed.usage);
        assert!(
            diagnosed
                .diagnostic
                .as_ref()
                .unwrap()
                .callers
                .iter()
                .all(|c| c.sites == ["Timestamp::parse_with_work.accepted_at"])
        );
    }
}

#[cfg(test)]
mod original_audit_domain_tests {
    use super::*;
    #[test]
    fn local_child_uses_remaining_parent_without_the_original_child_ceiling() {
        let mut outer = Meter::original_audit().unwrap();
        outer.visit("test.host_before").unwrap();
        let mut inner = outer.local_economic_meter().unwrap();
        assert_eq!(inner.maximum, 1_048_575);
        assert!(outer.local_economic_meter().is_err());
        assert!(outer.original_economic_meter().is_err());
        // Synthetic boundary state tests the guard, not a workload or fit.
        inner.usage.semantic_work = MAXIMUM_WORK;
        inner.visit("test.local_beyond_original_ceiling").unwrap();
        let used = outer.finish_local_economic(inner).unwrap();
        assert_eq!(used.semantic_work, 65_537);
        assert_eq!(outer.original_remaining_work(), 1_048_576 - 65_538);
        outer.visit("test.host_after").unwrap();
        assert_eq!(outer.original_remaining_work(), 1_048_576 - 65_539);
        assert!(outer.local_economic_meter().is_err());
        assert!(outer.original_economic_meter().is_err());
        assert!(Meter::new(MAXIMUM_WORK + 1).is_err());
    }
    #[test]
    fn local_child_preserves_parent_exhaustion_and_rejects_wrong_finish_flavor() {
        let mut outer = Meter::original_audit().unwrap();
        outer.usage.semantic_work = 1_048_575;
        let mut inner = outer.local_economic_meter().unwrap();
        assert_eq!(inner.maximum, 1);
        inner.visit("test.last_local_work").unwrap();
        let used = outer.finish_local_economic(inner).unwrap();
        assert_eq!(outer.usage.semantic_work + used.semantic_work, 1_048_576);
        assert!(outer.visit("test.parent_still_bounded").is_err());

        let mut wrong = Meter::original_audit().unwrap();
        let inner = wrong.local_economic_meter().unwrap();
        assert!(wrong.finish_original_economic(inner).is_err());
        assert!(wrong.visit("test.wrong_finish_poisoned").is_err());
        let mut wrong = Meter::original_audit().unwrap();
        let inner = wrong.original_economic_meter().unwrap();
        assert!(wrong.finish_local_economic(inner).is_err());
        assert!(wrong.visit("test.old_child_not_local").is_err());
    }
    #[test]
    fn original_audit_domain_cannot_enter_product_or_reset_its_inner_invocation() {
        let mut outer = Meter::original_audit().unwrap();
        assert!(outer.enter_compile().is_err());
        assert!(outer.enter_draft().is_err());
        let mut inner = outer.original_economic_meter().unwrap();
        assert_eq!(inner.maximum, 65536);
        assert!(outer.original_economic_meter().is_err());
        inner.enter_draft().unwrap();
        inner.visit("test.economic_operation").unwrap();
        let _inner_usage = outer.finish_original_economic(inner).unwrap();
        assert_eq!(outer.maximum, 1048575);
        assert!(outer.original_economic_meter().is_err());
    }
    #[test]
    fn composition_caps_before_inner_and_post_inner_work_without_extra_allowance() {
        let mut outer = Meter::original_audit().unwrap();
        // Synthetic counter state exercises the mechanism without executing an
        // audit, economic workload, calibration or a million visit loop.
        outer.usage.semantic_work = 1048575;
        let mut inner = outer.original_economic_meter().unwrap();
        assert_eq!(inner.maximum, 1);
        inner.enter_draft().unwrap();
        inner.visit("test.last_inner_work").unwrap();
        assert!(inner.visit("test.denied_inner_work").is_err());
        assert_eq!(inner.usage.semantic_work, 1);
        let inner_usage = outer.finish_original_economic(inner).unwrap();
        assert!(outer.visit("test.denied_later_audit_work").is_err());
        assert_eq!(
            outer.usage.semantic_work + inner_usage.semantic_work,
            1048576
        );
    }
}

#[cfg(test)]
mod operator_resource_tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::{OriginalAuditSession, ResourceLimits, ValidationVisit, ValidationWork}};
    #[test]
    fn operator_unlimited_and_finite_limits_share_one_child_and_checked_accounting() {
        let limits=ResourceLimits::unlimited();
        let arena=AuditArena::with_resource_limits(limits).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        let mut child=session.meter_mut().local_economic_meter().unwrap();
        // One event through the actual v2 semantic+vector accounting path; this
        // is a boundary/lifecycle test, not a measured execution envelope.
        child.v2_semantic_event("test.operator_boundary",1_048_577,std::iter::empty()).unwrap();
        child.check_resolved_dimensions(4097,8_388_609).unwrap();
        child.new_body(8_388_609).unwrap();
        assert_eq!(child.remaining_new_body().unwrap(),None);
        session.meter_mut().finish_local_economic(child).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        assert_eq!(session.usage().semantic_work,1_048_579);
        assert_eq!(session.usage().remaining_work,None);
        assert_eq!(session.resource_usage().unwrap().semantic_work,1_048_579);
        assert!(session.meter_mut().local_economic_meter().is_err());
        assert_eq!(arena.resource_limits(),Some(&limits));
        let lease=session.reserve_external_payload(64*1024*1024+1).unwrap();
        assert!(arena.usage().unwrap().live_bytes>64*1024*1024);
        drop(lease);
        let limits=ResourceLimits{semantic_work:Some(7),..limits};
        let arena=AuditArena::with_resource_limits(limits).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        session.work().visit(ValidationVisit::Constraint).unwrap();
        let mut child=session.meter_mut().local_economic_meter().unwrap();
        child.v2_semantic_event("test.operator_finite",6,std::iter::empty()).unwrap();
        session.meter_mut().finish_local_economic(child).unwrap();
        assert_eq!(session.usage().semantic_work,7);
        assert_eq!(session.usage().remaining_work,Some(0));
        assert_eq!(session.work().visit(ValidationVisit::Constraint).unwrap_err().detail,"resource limit: semantic_work limit=7 observed=8");
        assert!(ResourceLimits{arena_bytes:Some(0),..limits}.validate().is_err());
        let limits=ResourceLimits{resolved_records:Some(2),resolved_bytes:Some(10),new_body_bytes:Some(8),arena_bytes:Some(1100),..ResourceLimits::unlimited()};
        let arena=AuditArena::with_resource_limits(limits).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert_eq!(session.meter_mut().check_resolved_dimensions(3,0).unwrap_err().detail,"resource limit: resolved_records limit=2 observed=3");
        assert_eq!(session.meter_mut().check_resolved_dimensions(2,11).unwrap_err().detail,"resource limit: resolved_bytes limit=10 observed=11");
        assert_eq!(session.meter_mut().new_body(9).unwrap_err().detail,"resource limit: new_body_bytes limit=8 observed=9");
        assert_eq!(session.reserve_external_payload(77).err().unwrap().detail,"resource limit: arena_bytes limit=1100 observed=1101");
    }
    #[test]
    fn operator_compile_limits_and_cancellation_do_not_create_extra_allowance() {
        let limits=ResourceLimits{compile_work:Some(3),semantic_work:Some(10),..ResourceLimits::unlimited()};
        let arena=AuditArena::with_resource_limits(limits).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().original_compile_meter().unwrap();
        child.enter_compile().unwrap();
        child.v2_semantic_event("test.compile",3,std::iter::empty()).unwrap();
        assert_eq!(child.v2_semantic_event("test.compile",1,std::iter::empty()).unwrap_err().detail,"resource limit: compile_work limit=3 observed=4");
        assert!(session.meter_mut().finish_original_compile(child).is_err());
        let arena=AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().original_compile_meter().unwrap();
        child.enter_compile().unwrap();
        child.v2_semantic_event("test.unlimited_compile",1_048_577,std::iter::empty()).unwrap();
        session.meter_mut().finish_original_compile(child).unwrap();
        assert_eq!(session.usage().semantic_work,1_048_577);
        assert_eq!(session.usage().remaining_work,None);
        assert!(session.meter_mut().original_compile_meter().is_err());
        let arena=AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().local_economic_meter().unwrap();
        arena.cancellation_handle().cancel();
        assert!(child.visit("test.cancel").is_err());
        assert!(session.meter_mut().finish_local_economic(child).is_err());
    }
}
