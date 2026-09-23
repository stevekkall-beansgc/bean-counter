//! Finite paid transition slots derived from the immutable accepted worksheet.
use super::points::{AllocationState, ResourceState};
use crate::adjudication::{
    commands::{Counters, Resource},
    types::Count,
};
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Deserialize)]
pub struct Worksheet {
    pub transitions: BTreeMap<String, Template>,
    pub bundles: BTreeMap<String, Bundle>,
}
#[derive(Clone, Deserialize)]
pub struct Template {
    pub segment_bytes: u64,
    pub new_trusted_bytes: u64,
    pub records: u64,
    pub index_path_pages: u64,
    pub index_value_pages: u64,
    pub logical_workspace_bytes: u64,
    pub counter_increments: BTreeMap<String, u64>,
}
#[derive(Deserialize)]
pub struct Bundle {
    pub slots: Vec<String>,
}
const NAMES: [&str; 16] = [
    "segment",
    "head_revision",
    "grant",
    "grant_registry",
    "allocation",
    "receipt",
    "control",
    "round",
    "import",
    "terminal",
    "allocation_prefix",
    "receipt_prefix",
    "index_cardinality",
    "writer_epoch",
    "economic_revision",
    "resource_revision",
];
fn err(detail: &str) -> Error {
    Error::new("UNFUNDED", detail)
}
impl Worksheet {
    pub fn frozen() -> Result<Self> {
        serde_json::from_str(include_str!("../../../../../contracts/candidates/central-adjudication-r3-candidate1/protocol/resources.json")).map_err(|e|err(&e.to_string()))
    }
    pub fn template(&self, kind: &str) -> Result<&Template> {
        self.transitions
            .get(kind)
            .ok_or_else(|| err("transition template"))
    }
    pub fn bundle(&self, name: &str) -> Result<&[String]> {
        self.bundles
            .get(name)
            .map(|b| b.slots.as_slice())
            .ok_or_else(|| err("bundle template"))
    }
}
impl Template {
    pub fn resources(&self) -> Result<Resource> {
        Ok(Resource::from_dimensions(
            [
                self.segment_bytes,
                self.new_trusted_bytes,
                self.records,
                self.index_path_pages,
                self.index_value_pages,
                self.logical_workspace_bytes,
            ]
            .map(|v| Count::new(v.into()).expect("u64 fits counter")),
        ))
    }
    pub fn counters(&self) -> Result<Counters> {
        let mut a = [Count::ZERO; 16];
        for (i, n) in NAMES.iter().enumerate() {
            a[i] = Count::new(
                (*self
                    .counter_increments
                    .get(*n)
                    .ok_or_else(|| err("counter template"))?)
                .into(),
            )?;
        }
        Ok(Counters::from_dimensions(a))
    }
}
impl ResourceState {
    pub fn genesis(provisioned: Resource, epoch: Count) -> Self {
        let mut q = Counters::zero();
        q.writer_epoch = epoch;
        Self {
            provisioned,
            used: Resource::zero(),
            held: Resource::zero(),
            q,
            reserved: Counters::zero(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !self.used.checked_add(&self.held)?.fits(&self.provisioned) {
            return Err(err("resource conservation"));
        }
        self.q.checked_add(&self.reserved)?;
        Ok(())
    }
    /// Caller persists the aggregate and new immutable owner in the SAME plan.
    pub fn reserve(
        &mut self,
        owner: String,
        slots: &[String],
        work: &Worksheet,
    ) -> Result<AllocationState> {
        let mut held = Resource::zero();
        let mut counters = Counters::zero();
        for kind in slots {
            let t = work.template(kind)?;
            let mut r = t.resources()?;
            let peak = r.workspace_bytes;
            r.workspace_bytes = Count::ZERO;
            held = held.checked_add(&r)?;
            held.workspace_bytes = held.workspace_bytes.max(peak);
            counters = counters.checked_add(&t.counters()?)?;
        }
        let mut next = self.clone();
        next.held = next.held.checked_add(&held)?;
        next.reserved = next.reserved.checked_add(&counters)?;
        next.validate()?;
        *self = next;
        Ok(AllocationState {
            owner,
            slots: slots.to_vec(),
            held,
        })
    }
    pub fn spend(
        &mut self,
        allocation: &mut AllocationState,
        kind: &str,
        actual: &Counters,
        work: &Worksheet,
    ) -> Result<()> {
        let Some(position) = allocation.slots.iter().position(|k| k == kind) else {
            return Err(err("slot already spent"));
        };
        let t = work.template(kind)?;
        let maximum = t.counters()?;
        if !actual
            .dimensions()
            .into_iter()
            .zip(maximum.dimensions())
            .all(|(a, b)| a <= b)
        {
            return Err(err("actual counter envelope"));
        }
        let mut cost = t.resources()?;
        if cost.workspace_bytes > allocation.held.workspace_bytes {
            return Err(err("protected workspace"));
        }
        cost.workspace_bytes = Count::ZERO;
        let mut next = self.clone();
        let mut owned = allocation.clone();
        owned.held = owned.held.checked_sub(&cost)?;
        owned.slots.remove(position);
        next.held = next.held.checked_sub(&cost)?;
        next.used = next.used.checked_add(&cost)?;
        next.reserved = next.reserved.checked_sub(&maximum)?;
        next.q = next.q.checked_add(actual)?;
        next.validate()?;
        *self = next;
        *allocation = owned;
        Ok(())
    }
    pub fn terminal_slack(
        &mut self,
        allocation: &mut AllocationState,
        work: &Worksheet,
    ) -> Result<()> {
        let mut credits = Counters::zero();
        for kind in &allocation.slots {
            credits = credits.checked_add(&work.template(kind)?.counters()?)?;
        }
        let mut next = self.clone();
        next.held = next.held.checked_sub(&allocation.held)?;
        next.reserved = next.reserved.checked_sub(&credits)?;
        next.validate()?;
        *self = next;
        allocation.held = Resource::zero();
        allocation.slots.clear();
        Ok(())
    }
}
