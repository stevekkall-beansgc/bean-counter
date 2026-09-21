use crate::store::errors::StoreError;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// Never unlink this lock file. The OS lock, not its contents or PID, is authority.
pub(super) struct Owner {
    _lock: File,
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
            _lock: lock,
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
