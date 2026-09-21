use crate::store::errors::StoreError;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// Never unlink this lock file. The OS lock, not its contents or PID, is authority.
pub(super) struct Owner {
    lock: File,
    pub directory: PathBuf,
    pub database: PathBuf,
}
impl Owner {
    pub fn acquire(path: &Path) -> Result<Self, StoreError> {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(StoreError::InvalidStore("symlink data directory"));
        }
        let directory = path.canonicalize()?;
        if !directory.is_dir() {
            return Err(StoreError::InvalidStore(
                "data directory is not a directory",
            ));
        }
        let lockpath = directory.join("owner.lock");
        reject_symlink(&lockpath)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lockpath)?;
        lock.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => StoreError::Owned,
            std::fs::TryLockError::Error(e) => StoreError::Io(e),
        })?;
        reject_symlink(&lockpath)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let a = lock.metadata()?;
            let b = fs::metadata(&lockpath)?;
            if a.dev() != b.dev() || a.ino() != b.ino() {
                return Err(StoreError::InvalidStore(
                    "owner lock switched during acquisition",
                ));
            }
        }
        if path.canonicalize()? != directory {
            return Err(StoreError::InvalidStore(
                "data directory switched during acquisition",
            ));
        }
        let database = directory.join("local.db");
        for name in ["local.db", "local.db-wal", "local.db-shm"] {
            reject_symlink(&directory.join(name))?;
        }
        Ok(Self {
            lock,
            directory,
            database,
        })
    }
    pub fn verify_path(&self) -> Result<(), StoreError> {
        reject_symlink(&self.database)?;
        if self.database.canonicalize()?.parent() != Some(self.directory.as_path()) {
            return Err(StoreError::InvalidStore("database outside owner directory"));
        }
        Ok(())
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Closing alone leaves the OS lock held by descriptors inherited during
        // a concurrent subprocess spawn, even with close-on-exec. Only the final
        // owner guard reaches here; transactions and pool callbacks retain it
        // until cleanup. Unlock the shared OS lock before closing our descriptor.
        // If unlock fails, descriptor close remains the fail-closed fallback.
        let _ = self.lock.unlock();
    }
}
fn reject_symlink(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(StoreError::InvalidStore("symlink in SQLite storage"))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn close_drains_transaction_before_unlocking_duplicated_descriptor() {
        use crate::store::{
            ports::{AcceptanceStore, AcceptanceTx},
            sqlite::{tests::installation, SqliteStore},
        };
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
            time::Duration,
        };
        use tokio::time::Instant;

        let directory = tempfile::tempdir().unwrap();
        let store = SqliteStore::create(directory.path(), installation())
            .await
            .unwrap();
        let inherited = store.inner._owner.lock.try_clone().unwrap();
        let tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let mut close = Box::pin(store.close());
        assert!(matches!(
            close.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        assert!(matches!(
            Owner::acquire(directory.path()),
            Err(StoreError::Owned)
        ));
        tx.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), close)
            .await
            .expect("close drained the outstanding transaction");
        let reopened = SqliteStore::open(directory.path())
            .await
            .expect("drained store released its lock");
        drop(inherited);
        assert!(matches!(
            Owner::acquire(directory.path()),
            Err(StoreError::Owned)
        ));
        reopened.close().await;
    }

    #[test]
    fn final_owner_releases_lock_despite_duplicated_descriptor() {
        let directory = tempfile::tempdir().unwrap();
        let owner = Arc::new(Owner::acquire(directory.path()).unwrap());
        let retained_owner = Arc::clone(&owner);
        // A duplicate shares the OS lock just like the descriptor inherited by
        // a concurrently spawning subprocess before close-on-exec takes effect.
        let inherited = owner.lock.try_clone().unwrap();
        drop(owner);
        assert!(matches!(
            Owner::acquire(directory.path()),
            Err(StoreError::Owned)
        ));
        drop(retained_owner);
        let replacement = Owner::acquire(directory.path()).expect("final owner released its lock");
        assert!(matches!(
            Owner::acquire(directory.path()),
            Err(StoreError::Owned)
        ));
        drop(inherited);
        assert!(matches!(
            Owner::acquire(directory.path()),
            Err(StoreError::Owned)
        ));
        drop(replacement);
        drop(Owner::acquire(directory.path()).unwrap());
    }
}
