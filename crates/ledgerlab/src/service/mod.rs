pub(crate) mod accept;
pub(crate) mod hooks;
mod project;
#[cfg(test)]
mod tests;
use crate::{store::errors::StoreError, ServiceError};
pub(crate) fn store_error(e: StoreError) -> ServiceError {
    if matches!(e, StoreError::ReadBudgetExhausted) {
        ServiceError::ReadBudgetExhausted
    } else if matches!(e, StoreError::BillingUpgradeRequired) {
        ServiceError::Rejection("BILLING_UPGRADE_REQUIRED".into())
    } else if matches!(e, StoreError::BillingHistoryLimit) {
        ServiceError::Rejection("BILLING_HISTORY_LIMIT".into())
    } else if matches!(e, StoreError::BillingPeriod) {
        ServiceError::Rejection("BILLING_M5_PERIOD".into())
    } else if e.retryable_after_rollback() {
        ServiceError::Retryable
    } else if matches!(e, StoreError::Integrity(_) | StoreError::InvalidStore(_)) {
        ServiceError::IntegrityFailure
    } else {
        ServiceError::Unavailable
    }
}

#[cfg(test)]
mod billing_limit_tests {
    use super::store_error;
    use crate::{store::errors::StoreError, ServiceError};

    #[test]
    fn valid_billing_byte_limit_maps_to_a_caller_refusal() {
        assert_eq!(
            store_error(StoreError::BillingHistoryLimit),
            ServiceError::Rejection("BILLING_HISTORY_LIMIT".into())
        );
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
