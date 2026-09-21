use std::{error::Error, fmt};

/// Classification never turns a database conflict into an economic duplicate.
#[derive(Debug)]
pub(crate) enum StoreError {
    Io(std::io::Error),
    Database(sqlx::Error),
    Postgres(tokio_postgres::Error),
    Migration(sqlx::migrate::MigrateError),
    Owned,
    Overloaded,
    Deadline,
    InvalidStore(&'static str),
    Integrity(&'static str),
    WritesDisabled,
}
impl StoreError {
    pub fn retryable_after_rollback(&self) -> bool {
        match self {
            Self::Postgres(e) => matches!(
                e.code().map(|c| c.code()),
                Some("40001" | "40P01" | "55P03" | "23505")
            ),
            Self::Overloaded | Self::Deadline | Self::Database(sqlx::Error::PoolTimedOut) => true,
            Self::Database(sqlx::Error::Database(e)) => e
                .code()
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|c| matches!(c & 255, 5 | 6)),
            _ => false,
        }
    }
    pub fn disables_writes(&self) -> bool {
        match self {
            Self::Integrity(_) => true,
            Self::Database(sqlx::Error::Database(e)) => e
                .code()
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|c| matches!(c & 255, 10 | 11 | 13 | 26)),
            Self::Database(_) => true,
            _ => false,
        }
    }
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}
impl From<sqlx::migrate::MigrateError> for StoreError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(e)
    }
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Store IO boundary: {e}"),
            Self::Migration(e) => write!(f, "Store migration boundary: {e}"),
            Self::InvalidStore(reason) | Self::Integrity(reason) => {
                write!(f, "Store boundary: {reason}")
            }
            _ => write!(f, "Store boundary: {self:?}"),
        }
    }
}
impl Error for StoreError {}

#[derive(Debug)]
pub(crate) enum CommitError {
    /// Acknowledged explicit rollback before commit was sent.
    RolledBack(StoreError),
    /// Commit may have reached durable storage. Resolve using original identity.
    OutcomeUnknown,
}

impl From<tokio_postgres::Error> for StoreError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Postgres(e)
    }
}
