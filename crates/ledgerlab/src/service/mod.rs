pub(crate) mod accept;
pub(crate) mod hooks;
mod project;
#[cfg(test)]
mod tests;
use crate::{store::errors::StoreError, ServiceError};
// Temporary, opt-in synthetic Linux CI diagnosis; no payloads are emitted.
pub(crate) fn workflow_ci_diagnostic(label: &'static str) {
    if std::env::var_os("LEDGER_WORKFLOW_CI_DIAGNOSTIC").as_deref()
        == Some(std::ffi::OsStr::new("mapping-only"))
    {
        eprintln!("WORKFLOW-CI-DIAG {label}");
    }
}
pub(crate) fn workflow_ci_unavailable(label: &'static str) -> ServiceError {
    workflow_ci_diagnostic(label);
    ServiceError::Unavailable
}
pub(crate) fn store_error(e: StoreError) -> ServiceError {
    workflow_ci_diagnostic(match &e {
        StoreError::ResourceQuota { .. } => "store.ResourceQuota",
        StoreError::Io(_) => "store.Io",
        StoreError::Database(sqlx::Error::PoolTimedOut) => "store.Database.PoolTimedOut",
        StoreError::Database(sqlx::Error::PoolClosed) => "store.Database.PoolClosed",
        StoreError::Database(sqlx::Error::Database(error)) => match error
            .code()
            .and_then(|code| code.parse::<u32>().ok())
            .map(|code| code & 255)
        {
            Some(5) => "store.Database.SQLiteBusy",
            Some(6) => "store.Database.SQLiteLocked",
            Some(10) => "store.Database.SQLiteIo",
            Some(11) => "store.Database.SQLiteCorrupt",
            Some(13) => "store.Database.SQLiteFull",
            _ => "store.Database.DatabaseOther",
        },
        StoreError::Database(_) => "store.Database.Other",
        StoreError::Postgres(_) => "store.Postgres",
        StoreError::Migration(_) => "store.Migration",
        StoreError::Owned => "store.Owned",
        StoreError::Overloaded => "store.Overloaded",
        StoreError::ReadBudgetExhausted => "store.ReadBudgetExhausted",
        StoreError::BillingHistoryLimit => "store.BillingHistoryLimit",
        StoreError::BillingPeriod => "store.BillingPeriod",
        StoreError::BillingClockNotAdvanced => "store.BillingClockNotAdvanced",
        StoreError::BillingUpgradeRequired => "store.BillingUpgradeRequired",
        StoreError::BillingM5SchemaRequired => "store.BillingM5SchemaRequired",
        StoreError::Deadline => "store.Deadline",
        StoreError::InvalidStore(_) => "store.InvalidStore",
        StoreError::Integrity(_) => "store.Integrity",
        StoreError::WritesDisabled => "store.WritesDisabled",
        StoreError::ExpectedCurrent => "store.ExpectedCurrent",
        StoreError::DeliveryConflict => "store.DeliveryConflict",
    });
    if let StoreError::ResourceQuota {
        setting,
        limit,
        observed,
    } = e
    {
        return ServiceError::ResourceLimit {
            setting,
            limit,
            observed,
        };
    }
    #[cfg(test)]
    if e.retryable_after_rollback() {
        eprintln!("ACCEPT-DIAG retryable store class={e:?}");
    }
    if matches!(e, StoreError::ReadBudgetExhausted) {
        ServiceError::ReadBudgetExhausted
    } else if matches!(e, StoreError::BillingM5SchemaRequired) {
        ServiceError::Rejection("BILLING_M5_SCHEMA_REQUIRED".into())
    } else if matches!(e, StoreError::BillingUpgradeRequired) {
        ServiceError::Rejection("BILLING_UPGRADE_REQUIRED".into())
    } else if matches!(e, StoreError::BillingHistoryLimit) {
        ServiceError::Rejection("BILLING_HISTORY_LIMIT".into())
    } else if matches!(e, StoreError::BillingPeriod) {
        ServiceError::Rejection("BILLING_M5_PERIOD".into())
    } else if matches!(e, StoreError::BillingClockNotAdvanced) {
        ServiceError::Rejection("BILLING_CLOCK_NOT_ADVANCED".into())
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

pub(crate) mod workflow;
