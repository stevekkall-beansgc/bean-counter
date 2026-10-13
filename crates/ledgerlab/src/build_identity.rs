//! Compile-owned inputs only. Ordinary builds have no qualified fresh identity.
use crate::ServiceError;
use ledgerlab_core::canonical;
use serde_json::Value;
pub(crate) fn embedded_inputs() -> Result<Value, ServiceError> {
    let value = canonical::parse_bounded(
        include_bytes!(concat!(env!("OUT_DIR"), "/workflow-build-inputs.json")),
        65536,
    )
    .map_err(|_| ServiceError::Rejection("BUILD_IDENTITY_REQUIRED".into()))?;
    if value.is_null() {
        return Err(ServiceError::Rejection("BUILD_IDENTITY_REQUIRED".into()));
    }
    // The private coordinator validates the closed BuildInputs dictionary and
    // current builtin reference before creating VerifiedBuildProvenance.
    Ok(value)
}
