//! Offline coordinator state/lifetime checks. These do not claim actual SQL
//! snapshot, orphan barrier, CAS, or end-to-end PostgreSQL cut coverage.
use super::*;
fn witness(owner: &PublicationOwner, byte: char) -> DatabaseWitness {
    DatabaseWitness::from_database(owner.identity().as_str(), &byte.to_string().repeat(64)).unwrap()
}
#[tokio::test]
async fn bootstrap_is_explicit_and_owner_survives_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let owner = PublicationOwner::acquire(dir.path()).unwrap();
    assert!(owner.needs_initialization().unwrap());
    assert!(!owner.available().await);
    let original = witness(&owner, '1');
    owner.initialize(&original).await.unwrap();
    assert!(!owner.needs_initialization().unwrap());
    assert!(owner.available().await);
    assert!(owner.initialize(&original).await.is_err());
    // Rejected initialize refuses visibility until explicit authoritative
    // recovery; it cannot accidentally erase or replace a durable record.
    assert!(!owner.available().await);
    assert_eq!(
        owner.fence.recover(&original).unwrap(),
        RecoveryDisposition::UnchangedStable
    );
    let pin = SnapshotPin {
        owner: owner.clone(),
        observed: original,
    };
    drop(owner);
    assert!(matches!(
        PublicationOwner::acquire(dir.path()),
        Err(StoreError::Owned)
    ));
    drop(pin);
    let reopened = PublicationOwner::acquire(dir.path()).unwrap();
    assert!(!reopened.needs_initialization().unwrap());
    assert!(!reopened.available().await); // open never silently trusts local FS
}
#[tokio::test]
async fn pending_drop_keeps_new_reads_unavailable_and_preserves_record() {
    let dir = tempfile::tempdir().unwrap();
    let owner = PublicationOwner::acquire(dir.path()).unwrap();
    let old = witness(&owner, '2');
    owner.initialize(&old).await.unwrap();
    let mut guard = owner.visibility.clone().write_owned().await;
    *guard = Published::Unavailable;
    let proposed = owner
        .fence
        .prepare(&old, b"offline state boundary")
        .unwrap();
    let mut lease = WritePublication {
        owner: owner.clone(),
        gate_identity: Arc::new(()),
        original: old.clone(),
        pending: Some(proposed),
        visibility: Some(guard),
        dirty: false,
    };
    lease.mark_mutation();
    drop(lease); // cancellation cannot restore memory visibility
    assert!(!owner.available().await);
    assert_eq!(
        owner.fence.recover(&old).unwrap(),
        RecoveryDisposition::RecoveredOld
    );
    assert!(!owner.available().await); // only SQL-aware coordinator exposes it
}
#[tokio::test]
async fn snapshot_pin_does_not_hold_visibility_guard() {
    let dir = tempfile::tempdir().unwrap();
    let owner = PublicationOwner::acquire(dir.path()).unwrap();
    let old = witness(&owner, '3');
    owner.initialize(&old).await.unwrap();
    let pin = {
        let shared = owner.visibility.read().await;
        assert!(matches!(&*shared, Published::Stable(w) if w == &old));
        SnapshotPin {
            owner: owner.clone(),
            observed: old.clone(),
        }
    };
    let mut exclusive = owner.visibility.try_write().unwrap();
    *exclusive = Published::Unavailable;
    assert_eq!(pin.observed, old);
    assert!(Arc::ptr_eq(&pin.owner, &owner));
    drop(exclusive);
    assert!(!owner.available().await);
}
#[test]
fn commit_classification_never_invents_saved_outcome() {
    let old = DatabaseWitness::from_database(&"1".repeat(64), &"2".repeat(64)).unwrap();
    let new = DatabaseWitness::from_database(&"1".repeat(64), &"3".repeat(64)).unwrap();
    let other = DatabaseWitness::from_database(&"1".repeat(64), &"4".repeat(64)).unwrap();
    for sql in [
        SqlCommitOutcome::Committed,
        SqlCommitOutcome::RolledBack,
        SqlCommitOutcome::Unknown,
    ] {
        assert!(matches!(
            classify(&old, None, false, sql, &old),
            Ok(PublicationOutcome::StableOld)
        ));
        assert!(classify(&old, Some(&new), true, sql, &other).is_err());
    }
    assert!(classify(&old, Some(&new), true, SqlCommitOutcome::Committed, &old).is_err());
    assert!(classify(&old, Some(&new), true, SqlCommitOutcome::RolledBack, &new).is_err());
    assert!(matches!(
        classify(&old, Some(&new), true, SqlCommitOutcome::Unknown, &old),
        Ok(PublicationOutcome::StableOld)
    ));
    assert!(matches!(
        classify(&old, Some(&new), true, SqlCommitOutcome::Unknown, &new),
        Ok(PublicationOutcome::StableNew)
    ));
    assert!(matches!(
        classify(&old, None, true, SqlCommitOutcome::RolledBack, &old),
        Ok(PublicationOutcome::StableOld)
    ));
    assert!(classify(&old, None, true, SqlCommitOutcome::Unknown, &old).is_err());
    assert!(classify(&old, None, true, SqlCommitOutcome::Committed, &new).is_err());
}
#[test]
fn unbound_is_only_a_bootstrap_or_legacy_observation() {
    let zero = "0".repeat(64);
    assert!(!bound(
        &DatabaseWitness::from_database(&zero, &zero).unwrap()
    ));
    assert!(!bound(
        &DatabaseWitness::from_database(&"1".repeat(64), &zero).unwrap()
    ));
    assert!(DatabaseWitness::from_database("invalid", &zero).is_err());
}
/// Compile the precise root-facing SQL API without a fake GenericClient or a
/// database connection. Actual SQL execution belongs to the integration wave.
#[test]
fn native_hook_signatures_compile() {
    let _ = read_witness::<tokio_postgres::Client>;
    let _ = PublicationOwner::recover_under_gate;
    let _ = PublicationOwner::pin_snapshot::<tokio_postgres::Client>;
    let _ = PublicationOwner::begin_write::<tokio_postgres::Transaction<'_>>;
    let _ = WritePublication::prepare_commit::<tokio_postgres::Transaction<'_>>;
    let _ = WritePublication::settle_after_exit;
    let result = PublicationOutcome::Unresolved(StoreError::Deadline);
    assert!(matches!(
        result,
        PublicationOutcome::Unresolved(StoreError::Deadline)
    ));
}
