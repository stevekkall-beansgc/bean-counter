pub(crate) mod accept;
pub(crate) mod hooks;
mod project;
#[cfg(test)]
mod tests;
use crate::{store::errors::StoreError, ServiceError};
pub(crate) fn store_error(e: StoreError) -> ServiceError {
    if e.retryable_after_rollback() {
        ServiceError::Retryable
    } else if matches!(e, StoreError::Integrity(_) | StoreError::InvalidStore(_)) {
        ServiceError::IntegrityFailure
    } else {
        ServiceError::Unavailable
    }
}

#[cfg(test)]
mod pg_tests;

#[cfg(test)]
mod race_tests;
