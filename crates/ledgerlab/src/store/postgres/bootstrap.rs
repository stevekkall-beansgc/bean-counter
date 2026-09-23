//! Explicit trusted-owner binding, never invoked by normal open. It neither
//! migrates nor admits physical resources. The embedding owner supplies a
//! preexisting private anchor outside the database backup/restore domain.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Private owner provisioning awaits host integration"
    )
)]
use super::{
    adjudication::{
        publication::{read_witness, PublicationOwner},
        recovery::Gate,
    },
    PostgresConfig,
};
use crate::store::errors::StoreError;
use std::{
    io::Read,
    path::Path,
    sync::{atomic::AtomicI32, Arc},
    time::Duration,
};
use tokio::time::{timeout_at, Instant};

fn refused() -> StoreError {
    StoreError::InvalidStore(
        "trusted PostgreSQL bootstrap requires a frozen original installation without R3 promises",
    )
}

pub(super) async fn bind(
    config: PostgresConfig,
    directory: &Path,
    expected_id: &str,
) -> Result<Arc<PublicationOwner>, StoreError> {
    let owner = PublicationOwner::acquire(directory)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = Arc::new(AtomicI32::new(0));
    let mut gate = Gate::acquire_raw(&config, deadline, &pid).await?;
    let mut session = config.connect().await?;
    let result = timeout_at(deadline, async {
        super::verify_server(&session.client).await?;
        super::migrate::verify(&session.client).await?;
        let tx = session.client.transaction().await?;
        tx.query_one("SELECT pg_advisory_xact_lock(714215261)", &[]).await?;
        tx.query_one("SELECT singleton FROM ledgerlab.installation WHERE singleton=1 FOR UPDATE", &[]).await?;
        let install = super::read::installation(&tx).await?;
        let stopped: bool = tx.query_one("SELECT owner IS NULL AND lease_until_us IS NULL AND enabled=0 FROM ledgerlab.dispatcher_head WHERE singleton=1 FOR UPDATE", &[]).await?.try_get(0)?;
        if expected_id.is_empty() || install.logical_store_id != expected_id || install.admission != "frozen" || !install.dispatch_hold || install.dispatch_enabled || !stopped { return Err(refused()); }
        // Even an orphan legacy COMMIT must release this row before observation.
        tx.query_one("SELECT singleton FROM ledgerlab.r3_commit_witness WHERE singleton=1 FOR UPDATE", &[]).await?;
        let idle: bool = tx.query_one("SELECT state='IDLE' FROM ledgerlab.r3_unresolved_work WHERE singleton=1", &[]).await?.try_get(0)?;
        if !idle { return Err(refused()); }
        for table in ["r3_scope_locks","r3_journals","r3_storage_profile","r3_segments","r3_segment_pages","r3_objects","r3_object_pages","r3_heads","r3_head_versions","r3_commands","r3_namespaces","r3_deliveries","r3_index_pages","r3_index_roots","r3_held_intentions"] {
            if tx.query_one(&format!("SELECT EXISTS(SELECT 1 FROM ledgerlab.{table})"), &[]).await?.try_get::<_,bool>(0)? { return Err(refused()); }
        }
        let observed = read_witness(&tx).await?;
        let zero = "0".repeat(64);
        if observed.anchor().as_str() == zero && observed.witness().as_str() == zero {
            if !owner.needs_initialization()? { return Err(refused()); }
            let mut entropy = [0u8;32];
            std::fs::File::open("/dev/urandom")?.read_exact(&mut entropy)?;
            let witness = ledgerlab_core::adjudication::raw_sha256(&entropy);
            if witness.as_str() == zero { return Err(refused()); }
            let changed = tx.execute("UPDATE ledgerlab.r3_commit_witness SET anchor=$1,witness=$2 WHERE singleton=1 AND anchor=$3 AND witness=$3", &[&owner.identity().as_str(), &witness.as_str(), &zero]).await?;
            if changed != 1 { return Err(refused()); }
        } else if observed.anchor() != owner.identity() { return Err(refused()); }
        let observed = read_witness(&tx).await?;
        tx.commit().await?;
        // A crash between SQL binding and initialization leaves ordinary open
        // disabled. Only this same owner, while still empty/frozen, may retry.
        if owner.needs_initialization()? { owner.initialize(&observed).await?; }
        owner.recover_under_gate(&mut gate, deadline).await?;
        gate.resolve_after_publication().await?;
        Ok::<_,StoreError>(())
    }).await.map_err(|_| StoreError::Deadline).and_then(|r|r);
    session.discard().await;
    result?;
    gate.finish().await?;
    Ok(owner)
}
