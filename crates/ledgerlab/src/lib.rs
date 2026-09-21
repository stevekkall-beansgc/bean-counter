//! One bounded acceptance coordinator over concrete durable stores.
#![forbid(unsafe_code)]
pub mod local;
pub mod maintenance;
pub mod outbox;
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
/// Noncommitting estimate; never contains a receipt. Records are prospective
/// canonical envelopes from the core, excluding the candidate receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewResult {
    WouldAccept {
        records: Vec<serde_json::Value>,
    },
    Duplicate {
        kind: DuplicateKind,
        event_id: String,
    },
    Conflict(ConflictKind),
    Rejected {
        code: String,
    },
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
    store: Backend,
}
#[derive(Clone)]
enum Backend {
    Sqlite(store::sqlite::SqliteStore),
    Postgres(store::postgres::PostgresStore),
}
pub use store::postgres::{PostgresConfig, PostgresTrust};
impl Ledger {
    /// Open a previously initialized durable directory. Does not create or reseed it.
    pub async fn open_sqlite(path: &Path) -> Result<Self, ServiceError> {
        Ok(Self {
            store: Backend::Sqlite(
                store::sqlite::SqliteStore::open(path)
                    .await
                    .map_err(service::store_error)?,
            ),
        })
    }
    pub async fn open_postgres(config: PostgresConfig) -> Result<Self, ServiceError> {
        Ok(Self {
            store: Backend::Postgres(
                store::postgres::PostgresStore::open(config)
                    .await
                    .map_err(service::store_error)?,
            ),
        })
    }
    pub async fn accept(&self, command: AcceptCommand) -> Result<AcceptResult, ServiceError> {
        let hooks = service::hooks::Hooks::default();
        match &self.store {
            Backend::Sqlite(store) => service::accept::run(store, &command, &hooks).await,
            Backend::Postgres(store) => service::accept::run(store, &command, &hooks).await,
        }
    }
    /// Evaluate using the current locked context without journal writes or commit.
    /// Store opening/locking can still touch filesystem metadata and SQLite WAL.
    pub async fn preview(&self, command: AcceptCommand) -> Result<PreviewResult, ServiceError> {
        match &self.store {
            Backend::Sqlite(store) => service::accept::preview(store, &command).await,
            Backend::Postgres(store) => service::accept::preview(store, &command).await,
        }
    }
    pub async fn close(self) {
        match self.store {
            Backend::Sqlite(store) => store.close().await,
            Backend::Postgres(store) => store.close().await,
        }
    }
}
