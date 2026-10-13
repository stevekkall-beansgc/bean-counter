//! Non-authoritative workflow ingress scaffold. Publication authority remains
//! blocked pending reviewed owner-admin/source-revocation contract disposition.
//! There is no publish, pricing or raw append endpoint in this increment.
mod request;
use crate::ServiceError;
use serde_json::Value;
type Result<T> = std::result::Result<T, ServiceError>;
fn reject(code: &str) -> ServiceError {
    ServiceError::Rejection(code.into())
}
fn require(ok: bool, code: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(reject(code)) }
}
