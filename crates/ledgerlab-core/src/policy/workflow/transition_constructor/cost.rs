//! Supplier /1 saved-intent normalization. Unconnected producer source is
//! preserved in docs/archive/workflow-disconnected-supplier-cost.
mod plan;
pub(super) use plan::normalize_request;
#[cfg(test)]
mod compatibility;
#[cfg(test)]
use compatibility::{conserved, high_water, nonnegative, InvocationAmounts};
#[cfg(test)]
mod operands;
#[cfg(test)]
mod records;
