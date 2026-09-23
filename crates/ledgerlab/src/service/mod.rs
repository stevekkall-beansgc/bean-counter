pub(crate) mod accept;
pub(crate) mod hooks;
mod project;
#[cfg(test)]
mod tests;
use crate::{store::errors::StoreError, ServiceError};
pub(crate) fn store_error(e: StoreError) -> ServiceError {
    if matches!(e, StoreError::ReadBudgetExhausted) {
        ServiceError::ReadBudgetExhausted
    } else if e.retryable_after_rollback() {
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

pub(crate) mod demo;
pub(crate) mod inspect;
#[cfg(test)]
mod pg_transport_tests;
#[cfg(test)]
mod review_tests;

pub(crate) mod billing;
pub(crate) mod comparison;
pub(crate) mod retained;
