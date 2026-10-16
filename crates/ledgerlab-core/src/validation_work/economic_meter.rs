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
    original_inner_started: bool,
    original_owner: Option<Arc<AtomicU8>>,
    original_child_token: Option<AuditChildToken>,
    phase: Phase,
    usage: Usage,
    resolved: BTreeMap<String, Vec<u8>>,
    #[cfg(test)]
    diagnostic: Option<Diagnostic>,
}
impl Meter {
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
            original_inner_started: false,
            original_owner: None,
            original_child_token: None,
            phase: Phase::ParseNormalize,
            resolved: BTreeMap::new(),
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
    pub(crate) fn original_audit() -> Result<Self> {
        let mut meter = Self::new(MAXIMUM_WORK)?;
        meter.domain = BudgetDomain::OriginalAudit;
        meter.original_owner = Some(Arc::new(AtomicU8::new(0)));
        meter.maximum = 1_048_576;
        Ok(meter)
    }
    pub(crate) fn original_remaining_work(&self) -> u64 {
        self.maximum.saturating_sub(self.usage.semantic_work)
    }
    /// Reserve a single tighter original economic invocation inside this audit.
    /// Its lower maximum ensures the composed cap BEFORE each inner operation.
    pub(crate) fn original_economic_meter(&mut self) -> Result<Self> {
        if self.domain != BudgetDomain::OriginalAudit
            || self.maximum != 1_048_576
            || self.original_inner_started
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original economic phase already consumed",
            ));
        }
        let remaining = self
            .maximum
            .checked_sub(self.usage.semantic_work)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original audit exhausted"))?;
        let mut inner = Self::new(remaining.min(MAXIMUM_WORK))?;
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
        self.original_inner_started = true;
        Ok(inner)
    }
    /// Saved publication recipes execute their saved compiler under the same
    /// per-origin allowance; this is not a second compiler allowance.
    pub(crate) fn original_compile_meter(&mut self) -> Result<Self> {
        if self.domain != BudgetDomain::OriginalAudit
            || self.maximum != 1_048_576
            || self.original_inner_started
        {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "original compiler phase already consumed",
            ));
        }
        let remaining = self
            .maximum
            .checked_sub(self.usage.semantic_work)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original audit exhausted"))?;
        let mut inner = Self::for_compile(remaining.min(MAXIMUM_COMPILE_WORK))?;
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
            || self.maximum != 1_048_576
            || !self.original_inner_started
            || inner.domain != BudgetDomain::Compile
            || inner.maximum > MAXIMUM_COMPILE_WORK
        {
            if let Some(owner) = &self.original_owner {
                owner.store(3, AtomicOrdering::SeqCst);
            }
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "wrong original compiler meter",
            ));
        }
        let allowance = 1_048_576u64
            .checked_sub(inner.usage.semantic_work)
            .ok_or_else(|| {
                Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed compiler work")
            })?;
        if self.usage.semantic_work > allowance {
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
        Ok(inner.finish())
    }
    /// Subsequent framing retains only the actual remaining composed allowance.
    /// This is counter composition, not a charge refund or semantic exemption.
    pub(crate) fn finish_original_economic(&mut self, inner: Self) -> Result<Usage> {
        let same_owner = match (&self.original_owner, &inner.original_owner) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same_owner
            || self.domain != BudgetDomain::OriginalAudit
            || self.maximum != 1_048_576
            || !self.original_inner_started
            || inner.domain != BudgetDomain::Draft
            || inner.maximum > MAXIMUM_WORK
        {
            if let Some(owner) = &self.original_owner {
                owner.store(3, AtomicOrdering::SeqCst);
            }
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "wrong original economic meter",
            ));
        }
        let allowance = 1_048_576u64
            .checked_sub(inner.usage.semantic_work)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "original composed work"))?;
        if self.usage.semantic_work > allowance {
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
            original_inner_started: false,
            original_owner: None,
            original_child_token: None,
            phase: Phase::Compile,
            resolved: BTreeMap::new(),
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
            || self.maximum > MAXIMUM_COMPILE_WORK
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
        if next > 8388608 || self.usage.unique_resolved_refs >= 4096 {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved closure"));
        }
        self.resolved.insert(identity.to_owned(), bytes.to_vec());
        self.usage.unique_resolved_bytes = next;
        self.usage.unique_resolved_refs += 1;
        Ok(())
    }
    pub fn scan_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        self.usage.scanned_bytes = self
            .usage
            .scanned_bytes
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "scan overflow"))?;
        Ok(())
    }
    pub fn hash_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        self.usage.hashed_bytes = self
            .usage
            .hashed_bytes
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "hash overflow"))?;
        Ok(())
    }
    pub(crate) fn remaining_new_body(&self) -> Result<usize> {
        8388608u64
            .checked_sub(self.usage.new_body_bytes)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "new body byte allowance"))
    }
    pub fn new_body(&mut self, n: usize) -> Result<()> {
        let next = self
            .usage
            .new_body_bytes
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "new body overflow"))?;
        if next > 8388608 {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "new body bytes"));
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
    fn require_original_lifecycle(&self) -> Result<()> {
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
            if next > self.maximum {
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
        if next > self.maximum {
            return Err(Error::new(
                "PROGRAM_BUDGET_EXCEEDED",
                "selected semantic work exhausted",
            ));
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
        let limbs = operands
            .iter()
            .map(|n| {
                if n.bits() > MAXIMUM_BITS {
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
        self.usage.body_bytes_read = self
            .usage
            .body_bytes_read
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "byte count overflow"))?;
        Ok(())
    }
    pub fn write_bytes(&mut self, n: usize) -> Result<()> {
        self.require_original_lifecycle()?;
        self.usage.canonical_bytes_written = self
            .usage
            .canonical_bytes_written
            .checked_add(n as u64)
            .ok_or_else(|| Error::new("PROGRAM_BUDGET_EXCEEDED", "byte count overflow"))?;
        Ok(())
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
    bounded(a + b)
}
#[track_caller]
pub(crate) fn sub(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.sub", &[a, b])?;
    bounded(a - b)
}
#[track_caller]
pub(crate) fn mul(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.mul", &[a, b])?;
    bounded(a * b)
}
#[track_caller]
pub(crate) fn div(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.div", &[a, b])?;
    if cmp(m, b, &BigInt::from(0))? == std::cmp::Ordering::Equal {
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    bounded(a / b)
}
#[track_caller]
pub(crate) fn rem(m: &mut Meter, a: &BigInt, b: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.rem", &[a, b])?;
    if cmp(m, b, &BigInt::from(0))? == std::cmp::Ordering::Equal {
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    bounded(a % b)
}
#[track_caller]
pub(crate) fn neg(m: &mut Meter, a: &BigInt) -> Result<BigInt> {
    m.arithmetic("arith.neg", &[a])?;
    bounded(-a)
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
        return Err(Error::new("ARITHMETIC_OVERFLOW", "zero divisor"));
    }
    let (q, r) = a.div_mod_floor(b);
    Ok((bounded(q)?, bounded(r)?))
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
