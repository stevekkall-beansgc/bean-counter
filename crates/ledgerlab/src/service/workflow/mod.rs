//! Local generic publication coordinator. Publications create no economic debt.
mod local;
mod publication;
mod records;
mod request;
use crate::ServiceError;
pub use local::LocalWorkflowLedger;
use serde_json::Value;
type Result<T> = std::result::Result<T, ServiceError>;
fn reject(code: &str) -> ServiceError {
    ServiceError::Rejection(code.into())
}
fn require(ok: bool, code: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reject(code))
    }
}
