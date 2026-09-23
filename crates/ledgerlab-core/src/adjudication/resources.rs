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

impl Resource {
    pub fn zero() -> Self {
        Self::from_dimensions([Count::ZERO; 6])
    }
    pub fn from_dimensions(v: [Count; 6]) -> Self {
        Self {
            canonical_bytes: v[0],
            trusted_bytes: v[1],
            records: v[2],
            index_pages: v[3],
            index_values: v[4],
            workspace_bytes: v[5],
        }
    }
    pub fn checked_sub(&self, rhs: &Self) -> Result<Self> {
        let a = self.dimensions();
        let b = rhs.dimensions();
        let mut c = [Count::ZERO; 6];
        for i in 0..c.len() {
            c[i] = a[i].checked_sub(b[i])?;
        }
        Ok(Self::from_dimensions(c))
    }
}

impl Counters {
    pub fn zero() -> Self {
        Self::from_dimensions([Count::ZERO; 16])
    }
    pub fn from_dimensions(v: [Count; 16]) -> Self {
        Self {
            segment: v[0],
            head_revision: v[1],
            grant: v[2],
            grant_registry: v[3],
            allocation: v[4],
            receipt: v[5],
            control: v[6],
            round: v[7],
            import: v[8],
            terminal: v[9],
            allocation_prefix: v[10],
            receipt_prefix: v[11],
            index_cardinality: v[12],
            writer_epoch: v[13],
            economic_revision: v[14],
            resource_revision: v[15],
        }
    }
    pub fn checked_sub(&self, rhs: &Self) -> Result<Self> {
        let a = self.dimensions();
        let b = rhs.dimensions();
        let mut c = [Count::ZERO; 16];
        for i in 0..c.len() {
            c[i] = a[i].checked_sub(b[i])?;
        }
        Ok(Self::from_dimensions(c))
    }
    pub fn checked_add(&self, rhs: &Self) -> Result<Self> {
        let a = self.dimensions();
        let b = rhs.dimensions();
        let mut c = [Count::ZERO; 16];
        for i in 0..16 {
            c[i] = a[i].checked_add(b[i])?;
        }
        Ok(Self::from_dimensions(c))
    }
}
