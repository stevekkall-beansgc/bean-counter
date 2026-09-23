//! Explicit finite host allocation. Provisioning is never an acceptance command.
use super::*;
use crate::store::ports::{AcceptanceStore, AcceptanceTx};
use r3::{
    runtime::points::{ResourceState, State},
    Validate,
};
use std::sync::atomic::Ordering;

fn incarnation(
    store: &super::super::SqliteStore,
    j: &JournalIdentity,
) -> Result<Digest, StoreError> {
    use std::os::unix::fs::MetadataExt;
    store.inner._owner.verify_path()?;
    let m = std::fs::metadata(&store.inner._owner.database)?;
    Ok(r3::raw_sha256(
        &r3::canonical_bytes(
            &json!([
                "sqlite-r3-storage/1",
                m.dev(),
                m.ino(),
                j.store,
                j.scope,
                j.registration,
                j.host
            ]),
            4096,
        )
        .map_err(core)?,
    ))
}

impl super::super::SqliteStore {
    /// Called during exclusive host setup, before exposing acceptance handles.
    /// `backing_bytes` is an assigned host storage/work allocation, never df/free
    /// space. The physical projection remains subject to independent acceptance.
    pub(crate) async fn provision_adjudication(
        &self,
        j: JournalIdentity,
        logical: wire::Resource,
        legacy_pages: u32,
        backing_bytes: Count,
    ) -> Result<tx::SqliteAdjudicationStore, StoreError> {
        logical.validate().map_err(core)?;
        if logical.workspace_bytes.value() < r3::SEGMENT_BYTES as u128 {
            return Err(StoreError::Overloaded);
        }
        let incarnation = incarnation(self, &j)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut transaction = self.begin(deadline).await?;
        let installation = transaction.load_installation().await?;
        if installation.logical_store_id != j.store.as_str()
            || installation.scope.tenant != j.scope.0.as_str()
            || installation.scope.environment != j.scope.1.as_str()
        {
            return Err(invalid());
        }
        let existing:Option<(Vec<u8>,i64,i64)>=sqlx::query_as("SELECT profile,maximum_pages,legacy_allowance FROM r3_storage_profile WHERE singleton=1").fetch_optional(transaction.conn()).await?;
        let pages;
        if let Some((bytes, maximum, legacy)) = existing {
            let v = ledgerlab_core::canonical::parse_bounded(&bytes, 8192).map_err(core)?;
            if bytes != r3::canonical_bytes(&v, 8192).map_err(core)?
                || v != json!({"journal":[j.store,j.scope,j.registration,j.host],"logical":logical,"backing_bytes":backing_bytes,"incarnation":incarnation})
                || legacy != i64::from(legacy_pages)
            {
                return Err(invalid());
            }
            pages = u32::try_from(maximum).map_err(|_| invalid())?;
            transaction.rollback().await?;
        } else {
            let baseline: i64 = sqlx::query_scalar("PRAGMA page_count")
                .fetch_one(transaction.conn())
                .await?;
            pages = physical::total_pages(&logical, baseline as u128, u128::from(legacy_pages))?;
            let wal = 32 + (u128::from(pages) + 2 + 65536u128.div_ceil(4120)) * 4120;
            let needed = u128::from(pages) * 4096 + wal + 2 * logical.workspace_bytes.value();
            if needed > backing_bytes.value() {
                return Err(StoreError::Overloaded);
            }
            let journal = journal_key(&j)?;
            let identity =
                r3::canonical_bytes(&json!([j.store, j.scope, j.registration, j.host]), 4096)
                    .map_err(core)?;
            sqlx::query("INSERT INTO r3_journals VALUES (?,?,?,?,?)")
                .bind(&journal)
                .bind(identity)
                .bind(0u128.to_be_bytes().as_slice())
                .bind("0".repeat(64))
                .bind("0".repeat(64))
                .execute(transaction.conn())
                .await?;
            let state = State::Resource(Box::new(ResourceState::genesis(
                logical.clone(),
                Count::new(1).map_err(core)?,
            )));
            let key = index_key(*b"RESOURCE", &[j.host.as_str().as_bytes()]).map_err(core)?;
            sqlx::query("INSERT INTO r3_heads VALUES (?,?,?,?,?)")
                .bind(&journal)
                .bind(head_tag(HeadKind::Resource))
                .bind(key)
                .bind(0u128.to_be_bytes().as_slice())
                .bind(r3::canonical_bytes(&state, r3::COMMAND_BYTES).map_err(core)?)
                .execute(transaction.conn())
                .await?;
            let profile=r3::canonical_bytes(&json!({"journal":[j.store,j.scope,j.registration,j.host],"logical":logical,"backing_bytes":backing_bytes,"incarnation":incarnation}),8192).map_err(core)?;
            sqlx::query("INSERT INTO r3_storage_profile VALUES (1,?,?,?,?,?)")
                .bind(&journal)
                .bind(profile)
                .bind(i64::from(pages))
                .bind(i64::from(legacy_pages))
                .bind(0u128.to_be_bytes().as_slice())
                .execute(transaction.conn())
                .await?;
            transaction
                .commit()
                .await
                .map_err(|_| StoreError::WritesDisabled)?;
        }
        self.inner
            ._owner
            .adjudication_max_pages
            .store(pages, Ordering::Release);
        let mut writer = self.inner.writer.acquire().await?;
        let actual: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "PRAGMA max_page_count={pages}"
        )))
        .fetch_one(&mut *writer)
        .await?;
        if actual != i64::from(pages) {
            self.inner.disabled.store(true, Ordering::Release);
            return Err(invalid());
        }
        self.inner
            .adjudication_enabled
            .store(true, Ordering::Release);
        Ok(tx::SqliteAdjudicationStore {
            store: self.clone(),
            journal: j,
            workspace: logical.workspace_bytes,
            logical,
            pages,
            incarnation,
        })
    }
}

pub(super) fn verify_incarnation(
    store: &super::super::SqliteStore,
    j: &JournalIdentity,
    expected: &Digest,
) -> Result<(), StoreError> {
    if incarnation(store, j)? != *expected {
        return Err(invalid());
    }
    Ok(())
}
