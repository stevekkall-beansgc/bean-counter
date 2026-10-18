//! Operator resource policy for one local attempt. This is not financial authority.
use crate::{Error, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    pub semantic_work: Option<u64>,
    pub compile_work: Option<u64>,
    pub resolved_records: Option<usize>,
    pub resolved_bytes: Option<usize>,
    pub new_body_bytes: Option<usize>,
    pub arena_bytes: Option<usize>,
}
impl ResourceLimits {
    /// No operational budgets. Checked counters, allocation failures, record
    /// grammar, arithmetic bounds, cancellation and financial checks remain.
    pub const fn unlimited() -> Self {
        Self { semantic_work: None, compile_work: None, resolved_records: None,
            resolved_bytes: None, new_body_bytes: None, arena_bytes: None }
    }
    pub fn validate(&self) -> Result<()> {
        if self.semantic_work == Some(0) || self.compile_work == Some(0)
            || self.resolved_records == Some(0) || self.resolved_bytes == Some(0)
            || self.new_body_bytes == Some(0) || self.arena_bytes == Some(0) {
            return Err(Error::new("INVALID_RESOURCE_LIMITS", "resource limits must be positive or null"));
        }
        Ok(())
    }
}

pub(crate) fn refusal(setting: &'static str, limit: u64, observed: u64) -> Error {
    Error::new("PROGRAM_BUDGET_EXCEEDED", format!("resource limit: {setting} limit={limit} observed={observed}"))
}
