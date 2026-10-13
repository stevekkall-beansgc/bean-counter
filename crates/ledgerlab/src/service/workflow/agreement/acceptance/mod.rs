//! Agreement input preparation only until the private resolver and atomic
//! coordinator authenticate origins, assent, current grants and head pins.
pub(super) mod authority;
pub(super) mod construct;
mod execute;
mod activation;
mod payload;
mod prepare;
mod resolve;

pub(super) use payload::AcceptInput;
pub(super) use resolve::PublicationInputs;

#[cfg(test)]
mod runtime_tests;

impl crate::service::workflow::local::LocalWorkflowLedger {
    /// Accept explicit terms against earlier enrolled sources and local proof
    /// material. The held owner capability authorizes administration, not assent.
    pub async fn agreement(&self, raw: &[u8]) -> super::super::Result<serde_json::Value> {
        execute::execute(self, raw).await
    }
}
