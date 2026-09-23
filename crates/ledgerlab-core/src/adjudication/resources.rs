//! Resource and finite-counter conservation; no physical-store capability claim.
use super::{
    commands::{Counters, Resource},
    ensure,
    types::Count,
};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterCredit {
    pub consumed: Count,
    pub held: Count,
}
impl CounterCredit {
    pub fn new(consumed: Count, held: Count) -> Result<Self> {
        consumed.checked_add(held)?;
        Ok(Self { consumed, held })
    }
    /// All checks precede mutation. A saved retry calls neither reserve nor spend.
    pub fn reserve(&mut self, additional: Count) -> Result<()> {
        let held = self.held.checked_add(additional)?;
        self.consumed.checked_add(held)?;
        self.held = held;
        Ok(())
    }
    pub fn spend(&mut self, discharged: Count, actual: Count) -> Result<()> {
        ensure(
            actual <= discharged,
            "COUNTER",
            "actual increment exceeds prepaid slot",
        )?;
        let held = self.held.checked_sub(discharged)?;
        let consumed = self.consumed.checked_add(actual)?;
        consumed.checked_add(held)?;
        self.held = held;
        self.consumed = consumed;
        Ok(())
    }
}
impl Resource {
    pub fn dimensions(&self) -> [Count; 6] {
        [
            self.canonical_bytes,
            self.trusted_bytes,
            self.records,
            self.index_pages,
            self.index_values,
            self.workspace_bytes,
        ]
    }
    pub fn fits(&self, upper: &Self) -> bool {
        self.dimensions()
            .into_iter()
            .zip(upper.dimensions())
            .all(|(a, b)| a <= b)
    }
    pub fn checked_add(&self, rhs: &Self) -> Result<Self> {
        Ok(Self {
            canonical_bytes: self.canonical_bytes.checked_add(rhs.canonical_bytes)?,
            trusted_bytes: self.trusted_bytes.checked_add(rhs.trusted_bytes)?,
            records: self.records.checked_add(rhs.records)?,
            index_pages: self.index_pages.checked_add(rhs.index_pages)?,
            index_values: self.index_values.checked_add(rhs.index_values)?,
            workspace_bytes: self.workspace_bytes.checked_add(rhs.workspace_bytes)?,
        })
    }
}
impl Counters {
    pub fn dimensions(&self) -> [Count; 16] {
        [
            self.segment,
            self.head_revision,
            self.grant,
            self.grant_registry,
            self.allocation,
            self.receipt,
            self.control,
            self.round,
            self.import,
            self.terminal,
            self.allocation_prefix,
            self.receipt_prefix,
            self.index_cardinality,
            self.writer_epoch,
            self.economic_revision,
            self.resource_revision,
        ]
    }
}
/// Retained dimensions are additive; workspace is an exclusively leased peak lane.
/// Store capability observations additionally cover concrete page/WAL/reader costs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceAccount {
    pub provisioned: Resource,
    pub used: Resource,
    pub held: Resource,
}
impl ResourceAccount {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.used.checked_add(&self.held)?.fits(&self.provisioned),
            "RESOURCE",
            "used plus held exceeds backing",
        )
    }
}
