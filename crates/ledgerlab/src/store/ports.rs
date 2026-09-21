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
    fn load_installation(
        &mut self,
    ) -> impl Future<Output = Result<Installation, StoreError>> + Send;
    fn load_chain(
        &mut self,
        scope: &Scope,
        id: &str,
    ) -> impl Future<Output = Result<Option<Chain>, StoreError>> + Send;
    fn load_authority(
        &mut self,
        scope: &Scope,
        id: &str,
    ) -> impl Future<Output = Result<Option<AuthorityHead>, StoreError>> + Send;
    fn load_binding(
        &mut self,
        scope: &Scope,
        id: &str,
    ) -> impl Future<Output = Result<Option<BindingHead>, StoreError>> + Send;
    fn load_document(
        &mut self,
        scope: &Scope,
        id: &str,
    ) -> impl Future<Output = Result<Option<(String, CanonicalRecord)>, StoreError>> + Send;
    fn load_grant_document(
        &mut self,
        scope: &Scope,
        id: &str,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;
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
