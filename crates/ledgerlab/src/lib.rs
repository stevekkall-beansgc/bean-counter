//! One bounded acceptance coordinator over concrete durable stores.
#![forbid(unsafe_code)]
mod service;
mod store;

use ledgerlab_core::domain::{Scope, Timestamp};
use std::path::Path;

/// Authenticated host context. These values must come from the embedding host,
/// never from the untrusted event body. Retained grants are rechecked under lock.
#[derive(Clone, Debug)]
pub struct PrincipalContext {
    pub scope: Scope,
    pub principal_id: String,
    pub source: String,
    pub authority_head: String,
    pub can_submit: bool,
    pub can_read: bool,
}
#[derive(Clone, Debug)]
pub struct AcceptCommand {
    pub bytes: Vec<u8>,
    pub principal: PrincipalContext,
    pub received_at: Timestamp,
    /// Trusted selector identity; its document and active head are read in the transaction.
    pub binding_selector: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateKind {
    Identity,
    Semantic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    Identity,
    Semantic,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptResult {
    Accepted {
        receipt: Vec<u8>,
    },
    Duplicate {
        kind: DuplicateKind,
        receipt: Vec<u8>,
    },
    Conflict(ConflictKind),
    Rejected {
        code: String,
    },
    /// No identity or economic reservation is made. Pending promotion is later scope.
    Waiting {
        missing: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceError {
    Retryable,
    OutcomeUnknown {
        scope: [String; 2],
        source: String,
        external_id: String,
    },
    Unavailable,
    IntegrityFailure,
    #[doc(hidden)]
    Rejection(String),
    #[cfg(test)]
    Injected,
    #[cfg(test)]
    ResponseLost,
}
impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ServiceError {}

#[derive(Clone)]
pub struct Ledger {
    store: store::sqlite::SqliteStore,
}
impl Ledger {
    /// Open a previously initialized durable directory. Does not create or reseed it.
    pub async fn open_sqlite(path: &Path) -> Result<Self, ServiceError> {
        Ok(Self {
            store: store::sqlite::SqliteStore::open(path)
                .await
                .map_err(service::store_error)?,
        })
    }
    pub async fn accept(&self, command: AcceptCommand) -> Result<AcceptResult, ServiceError> {
        service::accept::run(&self.store, &command, &service::hooks::Hooks::default()).await
    }
    pub async fn close(self) {
        self.store.close().await;
    }
}
