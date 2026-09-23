//! Explicit offline owner maintenance; never invoked by normal open.
//!
//! Stop all application processes, verify a restorable backup, then durably set
//! admission=frozen, dispatch_hold=1, dispatch_enabled=0 and clear the dispatcher
//! owner/lease with enabled=0. Review historical rejection downgrades separately.
//! Supply the expected logical store identity and, for PostgreSQL, the existing
//! restricted runtime role. These functions do not create a store, infer past
//! delivery outcomes, change economics, unfreeze admission or resume dispatch.
//! On an unknown result retain the fences and repeat the same upgrade: exact
//! schema/history verification resolves a committed result without reapplying SQL.
use crate::{store, PostgresConfig};
use std::{path::Path, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeResult {
    Upgraded,
    AlreadyCurrent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeError {
    /// Validation or DDL failed before commit; no successful upgrade reported.
    Refused,
    /// Completion was not acknowledged. Reopen/repeat while still fenced.
    OutcomeUnknown,
}
impl std::fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for UpgradeError {}
impl From<store::errors::StoreError> for UpgradeError {
    fn from(_: store::errors::StoreError) -> Self {
        Self::Refused
    }
}
impl From<sqlx::Error> for UpgradeError {
    fn from(_: sqlx::Error) -> Self {
        Self::Refused
    }
}
impl From<tokio_postgres::Error> for UpgradeError {
    fn from(_: tokio_postgres::Error) -> Self {
        Self::Refused
    }
}

/// Upgrade exact SQLite schema 1/2/3/4/5 to 6 under the directory's exclusive owner.
/// Once started, cancellation of the caller does not cancel maintenance. The
/// supervised operation retains ownership through completion and driver cleanup.
pub async fn upgrade_sqlite(
    path: &Path,
    expected_store_id: &str,
) -> Result<UpgradeResult, UpgradeError> {
    let path = path.to_owned();
    let id = expected_store_id.to_owned();
    tokio::spawn(async move { store::sqlite::migrate::upgrade(&path, &id).await })
        .await
        .unwrap_or(Err(UpgradeError::OutcomeUnknown))
}

/// Upgrade exact unbound PostgreSQL schema 1/2/3/4/5 to 6 with a migration-owner connection.
/// The runtime role remains non-owning and cannot perform this operation.
/// Cancellation of the caller leaves the bounded supervisor running.
pub async fn upgrade_postgres(
    owner: PostgresConfig,
    expected_store_id: &str,
    runtime_role: &str,
) -> Result<UpgradeResult, UpgradeError> {
    let id = expected_store_id.to_owned();
    let role = runtime_role.to_owned();
    tokio::spawn(async move {
        tokio::time::timeout(
            Duration::from_secs(60),
            store::postgres::migrate::upgrade(owner, &id, &role),
        )
        .await
        .unwrap_or(Err(UpgradeError::OutcomeUnknown))
    })
    .await
    .unwrap_or(Err(UpgradeError::OutcomeUnknown))
}

#[cfg(test)]
pub(crate) async fn assert_original_retry(ledger: &crate::Ledger) {
    let oracle = ledgerlab_testkit::FixtureOracle::workspace().unwrap();
    let input = ledgerlab_testkit::stores::Command::fixture(&oracle, "input").unwrap();
    let command = crate::AcceptCommand {
        bytes: input.bytes,
        principal: crate::PrincipalContext {
            scope: ledgerlab_core::domain::Scope::new("demo", "sandbox").unwrap(),
            principal_id: "demo-app".into(),
            source: "urn:demo:app".into(),
            authority_head: "demo-source-grant-v1".into(),
            can_submit: true,
            can_read: true,
        },
        received_at: ledgerlab_core::domain::Timestamp::parse(input.received_at).unwrap(),
        binding_selector: "demo-retail-selector".into(),
    };
    assert_eq!(
        ledger.accept(command).await.unwrap(),
        crate::AcceptResult::Duplicate {
            kind: crate::DuplicateKind::Identity,
            receipt: include_bytes!("../../../fixtures/journals/first-slice/receipt.json").to_vec(),
        }
    );
}
