//! Rollback-independent local publication fence, selected by the trusted host.
//! STABLE is the authoritative commit point. SQLite COMMIT while PENDING is
//! prepared and cannot be read, exported or acknowledged. All access retains
//! the common OS owner and the database's exclusive physical gate until STABLE.
use crate::store::errors::StoreError;
use ledgerlab_core::adjudication::raw_sha256;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Witness {
    hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
enum State {
    Stable { witness: Witness },
    Pending { old: Witness, new: Witness },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    anchor: String,
    state: State,
}
pub(super) struct Fence {
    lock: File,
    directory: PathBuf,
    id: String,
    state: Mutex<Option<State>>,
}
fn invalid() -> StoreError {
    StoreError::InvalidStore("R3 anchored storage requires authoritative recovery")
}
fn reject_link(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(invalid()),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
impl Fence {
    /// A trusted local receipt-commit exclusion witness. The live OS lock is
    /// retained by the owner; this digest binds its STABLE publication to the
    /// exact recovered journal/epoch/incarnation selected under the SQL gate.
    pub(super) fn observation(
        &self,
        binding: &[u8],
    ) -> Result<ledgerlab_core::adjudication::types::Digest, StoreError> {
        let state = self.state.lock().map_err(|_| invalid())?;
        let Some(State::Stable { witness }) = &*state else {
            return Err(invalid());
        };
        let bytes = ledgerlab_core::adjudication::canonical_bytes(
            &serde_json::json!([
                "sqlite-recovered-writer/1",
                self.id,
                witness.hash,
                raw_sha256(binding)
            ]),
            4096,
        )
        .map_err(|_| invalid())?;
        Ok(raw_sha256(&bytes))
    }
    pub(super) fn is_stable(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|s| matches!(*s, Some(State::Stable { .. })))
    }
    pub(super) fn acquire(directory: &Path, database_directory: &Path) -> Result<Self, StoreError> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        reject_link(directory)?;
        let directory = directory.canonicalize()?;
        // The host must preserve this allocation outside the database backup.
        if directory.starts_with(database_directory.canonicalize()?) {
            return Err(invalid());
        }
        let path = directory.join("owner.lock");
        reject_link(&path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)?;
        lock.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => StoreError::Owned,
            std::fs::TryLockError::Error(e) => StoreError::Io(e),
        })?;
        let m = lock.metadata()?;
        let actual = fs::metadata(&path)?;
        if m.dev() != actual.dev() || m.ino() != actual.ino() {
            return Err(invalid());
        }
        let id = raw_sha256(format!("sqlite-r3-anchor/1:{}:{}", m.dev(), m.ino()).as_bytes())
            .as_str()
            .to_owned();
        let state_path = directory.join("state.json");
        reject_link(&state_path)?;
        let state = match fs::metadata(&state_path) {
            Ok(meta) => {
                if meta.len() > 8192 {
                    return Err(invalid());
                }
                let record: Record =
                    serde_json::from_slice(&fs::read(state_path)?).map_err(|_| invalid())?;
                if record.version != 1 || record.anchor != id {
                    return Err(invalid());
                }
                Some(record.state)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            lock,
            directory,
            id,
            state: Mutex::new(state),
        })
    }
    fn publish(&self, state: State) -> Result<(), StoreError> {
        use std::os::unix::fs::OpenOptionsExt;
        let record = Record {
            version: 1,
            anchor: self.id.clone(),
            state: state.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|_| invalid())?;
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let staging = self.directory.join("state.next");
        reject_link(&staging)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&staging)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(staging, self.directory.join("state.json"))?;
        File::open(&self.directory)?.sync_all()?;
        *self.state.lock().map_err(|_| invalid())? = Some(state);
        Ok(())
    }
    pub(super) async fn initialize(&self, c: &mut SqliteConnection) -> Result<(), StoreError> {
        if self.state.lock().map_err(|_| invalid())?.is_some() {
            return Err(invalid());
        }
        let logical: String = sqlx::query_scalar("SELECT logical_store_id FROM installation")
            .fetch_one(&mut *c)
            .await?;
        let witness = Witness {
            hash: raw_sha256(format!("sqlite-r3-genesis/1:{}:{logical}", self.id).as_bytes())
                .as_str()
                .to_owned(),
        };
        sqlx::query("INSERT INTO r3_commit_witness VALUES (1,?,?)")
            .bind(&self.id)
            .bind(&witness.hash)
            .execute(&mut *c)
            .await?;
        self.publish(State::Stable { witness })
    }
    async fn observed(&self, c: &mut SqliteConnection) -> Result<Witness, StoreError> {
        let (id, hash): (String, String) =
            sqlx::query_as("SELECT anchor,hash FROM r3_commit_witness WHERE singleton=1")
                .fetch_one(c)
                .await?;
        if id != self.id || hash.len() != 64 {
            return Err(invalid());
        }
        Ok(Witness { hash })
    }
    pub(super) async fn recover(&self, c: &mut SqliteConnection) -> Result<(), StoreError> {
        let actual = self.observed(c).await?;
        let state = self
            .state
            .lock()
            .map_err(|_| invalid())?
            .clone()
            .ok_or_else(invalid)?;
        match state {
            State::Stable { witness } if witness == actual => Ok(()),
            State::Pending { old, new } if actual == old || actual == new => {
                self.publish(State::Stable { witness: actual })
            }
            _ => Err(invalid()),
        }
    }
    pub(super) async fn prepare(&self, c: &mut SqliteConnection) -> Result<Witness, StoreError> {
        let old = self.observed(c).await?;
        if !matches!(self.state.lock().map_err(|_|invalid())?.as_ref(),Some(State::Stable{witness}) if witness==&old)
        {
            return Err(invalid());
        }
        // A private opaque commit nonce has no finite sequence headroom that
        // could exhaust before a funded mandatory transition. Exact retries do
        // not create one because they perform no mutation.
        let mut nonce = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let heads: Vec<(Vec<u8>, Vec<u8>, String, String)> = sqlx::query_as(
            "SELECT journal,ordinal,segment,replay_root FROM r3_journals ORDER BY journal LIMIT 2",
        )
        .fetch_all(&mut *c)
        .await?;
        if heads.len() > 1 {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(&(&self.id, &old, nonce, heads)).map_err(|_| invalid())?;
        let new = Witness {
            hash: raw_sha256(&bytes).as_str().to_owned(),
        };
        let changed = sqlx::query(
            "UPDATE r3_commit_witness SET hash=? WHERE singleton=1 AND anchor=? AND hash=?",
        )
        .bind(&new.hash)
        .bind(&self.id)
        .bind(&old.hash)
        .execute(c)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(invalid());
        }
        self.publish(State::Pending {
            old,
            new: new.clone(),
        })?;
        Ok(new)
    }
    pub(super) fn finish(&self, new: Witness) -> Result<(), StoreError> {
        self.publish(State::Stable { witness: new })
    }
}
impl Drop for Fence {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        comparison::ComparisonReadStore,
        errors::CommitError,
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::{tests::installation, SqliteStore},
    };
    use std::{sync::atomic::Ordering, time::Duration};
    use tokio::time::Instant;
    fn locations() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("journal");
        let anchor = dir.path().join("trusted-anchor");
        fs::create_dir(&db).unwrap();
        fs::create_dir(&anchor).unwrap();
        (dir, db, anchor)
    }
    #[tokio::test]
    async fn anchored_commit_cuts_recover_only_authoritative_publication() {
        for cut in 1..=3 {
            let (_dir, db, anchor) = locations();
            let store = SqliteStore::create_fenced(&db, installation(), &anchor)
                .await
                .unwrap();
            let mut tx = store
                .begin(Instant::now() + Duration::from_secs(5))
                .await
                .unwrap();
            sqlx::query("UPDATE installation SET generation=generation+1")
                .execute(tx.conn())
                .await
                .unwrap();
            store.inner.fence_cut.store(cut, Ordering::Release);
            assert!(matches!(
                tx.commit().await,
                Err(CommitError::OutcomeUnknown)
            ));
            assert!(store.local_installation().await.is_err());
            assert!(store
                .begin_read(Instant::now() + Duration::from_secs(1))
                .await
                .is_err());
            store.close().await;
            assert!(
                SqliteStore::open(&db).await.is_err(),
                "anchor cannot be silently omitted"
            );
            let recovered = SqliteStore::open_fenced(&db, &anchor).await.unwrap();
            assert_eq!(
                recovered.local_installation().await.unwrap().generation,
                if cut == 1 { 0 } else { 1 }
            );
            let state_before = fs::read(anchor.join("state.json")).unwrap();
            recovered.close().await;
            let repeated = SqliteStore::open_fenced(&db, &anchor).await.unwrap();
            assert_eq!(
                fs::read(anchor.join("state.json")).unwrap(),
                state_before,
                "ordinary reopen adds no anchor state"
            );
            repeated.close().await;
        }
    }
    #[tokio::test]
    async fn anchored_same_inode_stale_restore_is_refused() {
        use std::os::unix::fs::MetadataExt;
        let (_dir, db, anchor) = locations();
        let store = SqliteStore::create_fenced(&db, installation(), &anchor)
            .await
            .unwrap();
        store.close().await;
        let path = db.join("local.db");
        let old = fs::read(&path).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        let store = SqliteStore::open_fenced(&db, &anchor).await.unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        sqlx::query("UPDATE installation SET generation=generation+1")
            .execute(tx.conn())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        store.close().await;
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.write_all(&old).unwrap();
        file.sync_all().unwrap();
        drop(file);
        // Restore the complete old SQLite state, not just its base file while
        // leaving the newer WAL available to recover the acknowledged commit.
        for name in ["local.db-wal", "local.db-shm"] {
            match fs::remove_file(db.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert!(SqliteStore::open_fenced(&db, &anchor).await.is_err());
    }
    #[tokio::test]
    async fn anchored_owner_excludes_copy_in_another_process() {
        let (dir, db, anchor) = locations();
        let store = SqliteStore::create_fenced(&db, installation(), &anchor)
            .await
            .unwrap();
        let copy = dir.path().join("copy");
        fs::create_dir(&copy).unwrap();
        fs::copy(db.join("local.db"), copy.join("local.db")).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::sqlite::fence::tests::anchored_process_probe",
                "--nocapture",
            ])
            .env("LEDGERLAB_FENCE_PROBE_DB", &copy)
            .env("LEDGERLAB_FENCE_PROBE_ANCHOR", &anchor)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        store.close().await;
    }
    #[tokio::test]
    async fn anchored_process_probe() {
        let Ok(db) = std::env::var("LEDGERLAB_FENCE_PROBE_DB") else {
            return;
        };
        let anchor = std::env::var("LEDGERLAB_FENCE_PROBE_ANCHOR").unwrap();
        assert!(matches!(
            SqliteStore::open_fenced(Path::new(&db), Path::new(&anchor)).await,
            Err(StoreError::Owned)
        ));
    }
}

#[cfg(test)]
mod crash_tests {
    use super::*;
    use crate::store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::{tests::installation, SqliteStore},
    };
    use std::{sync::atomic::Ordering, time::Duration};
    use tokio::time::Instant;
    #[tokio::test]
    async fn anchored_process_crash_each_publication_boundary() {
        for cut in 11..=13 {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("journal");
            let anchor = dir.path().join("anchor");
            fs::create_dir(&db).unwrap();
            fs::create_dir(&anchor).unwrap();
            SqliteStore::create_fenced(&db, installation(), &anchor)
                .await
                .unwrap()
                .close()
                .await;
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::sqlite::fence::crash_tests::anchored_crash_child",
                    "--nocapture",
                ])
                .env("LEDGERLAB_FENCE_CRASH_DB", &db)
                .env("LEDGERLAB_FENCE_CRASH_ANCHOR", &anchor)
                .env("LEDGERLAB_FENCE_CRASH_CUT", cut.to_string())
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let recovered = SqliteStore::open_fenced(&db, &anchor).await.unwrap();
            assert_eq!(
                recovered.local_installation().await.unwrap().generation,
                if cut == 11 { 0 } else { 1 }
            );
            recovered.close().await;
        }
    }
    #[tokio::test]
    async fn anchored_crash_child() {
        let Ok(db) = std::env::var("LEDGERLAB_FENCE_CRASH_DB") else {
            return;
        };
        let anchor = std::env::var("LEDGERLAB_FENCE_CRASH_ANCHOR").unwrap();
        let cut = std::env::var("LEDGERLAB_FENCE_CRASH_CUT")
            .unwrap()
            .parse::<u8>()
            .unwrap();
        let store = SqliteStore::open_fenced(Path::new(&db), Path::new(&anchor))
            .await
            .unwrap();
        store.inner.fence_cut.store(cut, Ordering::Release);
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        sqlx::query("UPDATE installation SET generation=generation+1")
            .execute(tx.conn())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        panic!("did not reach injected process cut");
    }
}
