//! Narrow static dispatch seam. There is no public action append API.
use super::{
    errors::{CommitError, StoreError},
    records::*,
};
use std::future::Future;
use tokio::time::Instant;

pub(crate) trait AcceptanceStore: Send + Sync {
    type Tx: AcceptanceTx;
    fn begin(&self, deadline: Instant)
        -> impl Future<Output = Result<Self::Tx, StoreError>> + Send;
}
pub(crate) trait AcceptanceTx: Sized + Send {
    fn write(&mut self, op: &WriteOp) -> impl Future<Output = Result<(), StoreError>> + Send;
    fn load_identity(
        &mut self,
        scope: &Scope,
        source: &str,
        external_id: &str,
    ) -> impl Future<Output = Result<Option<StoredIdentity>, StoreError>> + Send;
    fn load_claim(
        &mut self,
        scope: &Scope,
        source: &str,
        operation: &str,
        kind: &str,
        token: &str,
    ) -> impl Future<Output = Result<Option<StoredClaim>, StoreError>> + Send;
    fn commit(self) -> impl Future<Output = Result<(), CommitError>> + Send;
    fn rollback(self) -> impl Future<Output = Result<(), StoreError>> + Send;
}
