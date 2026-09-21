//! Typed transaction proof. No economics, store schema, retries or pooling.
use std::time::Duration;

use tokio::time::timeout;
use tokio_postgres::{types::Type, Error, IsolationLevel, Transaction};

use crate::connect::Session;

#[derive(Debug)]
pub enum CommitOutcome {
    Acknowledged,
    // Only explicit transaction-abort SQLSTATEs establish a retryable abort.
    RetryableAbort(Error),
    // Includes transport loss, a lost acknowledgment, and a drain deadline.
    // Never turn this into Rejected or retry with a new identity.
    Unknown(Option<Error>),
}

pub async fn commit(transaction: Transaction<'_>, drain: Duration) -> CommitOutcome {
    match timeout(drain, transaction.commit()).await {
        Ok(Ok(())) => CommitOutcome::Acknowledged,
        Ok(Err(error)) if matches!(error.code().map(|c| c.code()), Some("40001" | "40P01")) => {
            CommitOutcome::RetryableAbort(error)
        }
        Ok(Err(error)) => CommitOutcome::Unknown(Some(error)),
        Err(_) => CommitOutcome::Unknown(None),
    }
}

#[derive(Debug)]
pub enum ProbeOutcome {
    Committed(i64),
    BeforeCommit(Error),
    Commit(CommitOutcome),
    BeforeCommitDeadline,
}

/// Runs one typed parameter round-trip and a SERIALIZABLE commit. A session is
/// consumed and always discarded, so even uncertain cleanup cannot reach a next
/// borrower. A caller must not cancel the commit phase: allow the bounded drain,
/// then preserve Unknown if acknowledgment could not be established.
pub async fn round_trip(
    mut session: Session,
    value: i64,
    operation_bound: Duration,
    commit_drain: Duration,
) -> ProbeOutcome {
    let result = async {
        let work = async {
            let tx = session
                .client
                .build_transaction()
                .isolation_level(IsolationLevel::Serializable)
                .start()
                .await?;
            let row = tx
                .query_typed_one("SELECT $1::INT8", &[(&value, Type::INT8)])
                .await?;
            let received: i64 = row.try_get(0)?;
            Ok::<_, Error>((tx, received))
        };
        let (tx, received) = match timeout(operation_bound, work).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => return ProbeOutcome::BeforeCommit(error),
            Err(_) => return ProbeOutcome::BeforeCommitDeadline,
        };
        match commit(tx, commit_drain).await {
            CommitOutcome::Acknowledged => ProbeOutcome::Committed(received),
            outcome => ProbeOutcome::Commit(outcome),
        }
    }
    .await;
    session.discard().await;
    result
}
