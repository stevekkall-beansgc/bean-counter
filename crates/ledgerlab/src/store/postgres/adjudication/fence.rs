//! Trusted-host rollback-independent publication record. This filesystem slice
//! does not observe PostgreSQL, authenticate a DatabaseWitness or admit storage.
//! Native gate/SQL witness/read visibility integration is separately required.
use crate::store::errors::StoreError;
use ledgerlab_core::adjudication::{raw_sha256, types::Digest};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};
const MAX_RECORD: usize = 8192;
const MAX_BINDING: usize = 4096;

/// Parsed backend observation only. Construction is not proof of SQL authority,
/// database lineage, ownership, external recovery, or physical backing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::store::postgres) struct DatabaseWitness {
    anchor: Digest,
    witness: Digest,
}
impl DatabaseWitness {
    pub(in crate::store::postgres) fn from_database(
        anchor: &str,
        witness: &str,
    ) -> Result<Self, StoreError> {
        Ok(Self {
            anchor: Digest::parse(anchor).map_err(|_| invalid())?,
            witness: Digest::parse(witness).map_err(|_| invalid())?,
        })
    }
    pub(in crate::store::postgres) fn anchor(&self) -> &Digest {
        &self.anchor
    }
    pub(in crate::store::postgres) fn witness(&self) -> &Digest {
        &self.witness
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::store::postgres) enum RecoveryDisposition {
    UnchangedStable,
    RecoveredOld,
    RecoveredNew,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
enum State {
    Stable { witness: Digest },
    Pending { old: Digest, new: Digest },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    anchor: Digest,
    publication: State,
}
struct Memory {
    state: Option<State>,
    poisoned: bool,
}
pub(super) struct Fence {
    owner: File,
    directory: File,
    path: PathBuf,
    identity: Digest,
    memory: Mutex<Memory>,
}
fn invalid() -> StoreError {
    StoreError::InvalidStore("PostgreSQL trusted anchor requires authoritative recovery")
}
fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}
fn regular(path: &Path) -> Result<fs::Metadata, StoreError> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() || m.nlink() != 1 {
        return Err(invalid());
    }
    Ok(m)
}
/// Open without truncating first; validate exact inode before any write. The
/// directory is a trusted host allocation, not writable by request principals.
fn writable(path: &Path) -> Result<File, StoreError> {
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            regular(path)?;
            OpenOptions::new().read(true).write(true).open(path)?
        }
        Err(e) => return Err(e.into()),
    };
    if !same(&regular(path)?, &file.metadata()?) {
        return Err(invalid());
    }
    Ok(file)
}
impl Fence {
    pub(super) fn acquire(trusted_directory: &Path) -> Result<Self, StoreError> {
        let selected = fs::symlink_metadata(trusted_directory)?;
        if !selected.is_dir() || selected.file_type().is_symlink() {
            return Err(invalid());
        }
        let path = trusted_directory.canonicalize()?;
        let directory = File::open(&path)?;
        if !same(&selected, &directory.metadata()?) {
            return Err(invalid());
        }
        let owner = writable(&path.join("owner.lock"))?;
        owner.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => StoreError::Owned,
            std::fs::TryLockError::Error(e) => StoreError::Io(e),
        })?;
        if !same(&regular(&path.join("owner.lock"))?, &owner.metadata()?) {
            return Err(invalid());
        }
        let m = owner.metadata()?;
        if m.len() != 0 {
            return Err(invalid());
        }
        // Persist the inode that defines anchor identity before exposing it to
        // the SQL bootstrap coordinator. The host supplied directory preexists.
        owner.sync_all()?;
        directory.sync_all()?;
        let identity =
            raw_sha256(format!("postgres-r3-anchor/1:{}:{}", m.dev(), m.ino()).as_bytes());
        let state_path = path.join("state.json");
        let state = match fs::symlink_metadata(&state_path) {
            Ok(_) => {
                let m = regular(&state_path)?;
                if m.len() > MAX_RECORD as u64 {
                    return Err(invalid());
                }
                let file = File::open(&state_path)?;
                if !same(&m, &file.metadata()?) {
                    return Err(invalid());
                }
                let mut bytes = Vec::with_capacity(m.len() as usize);
                file.take((MAX_RECORD + 1) as u64).read_to_end(&mut bytes)?;
                if bytes.len() > MAX_RECORD {
                    return Err(invalid());
                }
                let r: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if r.version != 1 || r.anchor != identity || r.anchor.as_str() == "0".repeat(64) {
                    return Err(invalid());
                }
                if match &r.publication {
                    State::Stable { witness } => witness.as_str() == "0".repeat(64),
                    State::Pending { old, new } => {
                        old == new
                            || old.as_str() == "0".repeat(64)
                            || new.as_str() == "0".repeat(64)
                    }
                } {
                    return Err(invalid());
                }
                Some(r.publication)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let fence = Self {
            owner,
            directory,
            path,
            identity,
            memory: Mutex::new(Memory {
                state,
                poisoned: false,
            }),
        };
        fence.check_identity()?;
        Ok(fence)
    }
    pub(super) fn identity(&self) -> &Digest {
        &self.identity
    }
    fn check_identity(&self) -> Result<(), StoreError> {
        let d = fs::symlink_metadata(&self.path)?;
        if !d.is_dir()
            || d.file_type().is_symlink()
            || !same(&d, &self.directory.metadata()?)
            || !same(
                &regular(&self.path.join("owner.lock"))?,
                &self.owner.metadata()?,
            )
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn check_observation(&self, observed: &DatabaseWitness) -> Result<(), StoreError> {
        self.check_identity()?;
        if observed.anchor != self.identity
            || observed.anchor.as_str() == "0".repeat(64)
            || observed.witness.as_str() == "0".repeat(64)
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn publish(&self, memory: &mut Memory, state: State) -> Result<(), StoreError> {
        if memory.poisoned {
            return Err(invalid());
        }
        self.check_identity()?;
        // If any IO fails, this owner cannot assume which bytes became durable.
        // Reopening and authoritative SQL observation are required before reuse.
        memory.poisoned = true;
        let bytes = serde_json::to_vec(&Record {
            version: 1,
            anchor: self.identity.clone(),
            publication: state.clone(),
        })
        .map_err(|_| invalid())?;
        if bytes.len() > MAX_RECORD {
            return Err(invalid());
        }
        let staging = self.path.join("state.next");
        let mut f = writable(&staging)?;
        if f.metadata()?.len() > MAX_RECORD as u64 {
            return Err(invalid());
        }
        f.set_len(0)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        let target = self.path.join("state.json");
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                regular(&target)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.check_identity()?;
        fs::rename(staging, target)?;
        self.directory.sync_all()?;
        memory.state = Some(state);
        memory.poisoned = false;
        Ok(())
    }
    pub(super) fn needs_initialization(&self) -> Result<bool, StoreError> {
        self.check_identity()?;
        let m = self.memory.lock().map_err(|_| invalid())?;
        if m.poisoned {
            return Err(invalid());
        }
        Ok(m.state.is_none())
    }
    /// Caller must bootstrap this exact witness in SQL under migration-owner
    /// lineage/exclusion rules. This method cannot certify those premises.
    pub(super) fn initialize(&self, observed: &DatabaseWitness) -> Result<(), StoreError> {
        self.check_observation(observed)?;
        let mut m = self.memory.lock().map_err(|_| invalid())?;
        if m.state.is_some() || m.poisoned {
            return Err(invalid());
        }
        self.publish(
            &mut m,
            State::Stable {
                witness: observed.witness.clone(),
            },
        )
    }
    pub(super) fn recover(
        &self,
        observed: &DatabaseWitness,
    ) -> Result<RecoveryDisposition, StoreError> {
        self.check_observation(observed)?;
        let mut m = self.memory.lock().map_err(|_| invalid())?;
        if m.poisoned {
            return Err(invalid());
        }
        let disposition = match m.state.as_ref() {
            Some(State::Stable { witness }) if witness == &observed.witness => {
                return Ok(RecoveryDisposition::UnchangedStable)
            }
            Some(State::Pending { old, .. }) if old == &observed.witness => {
                RecoveryDisposition::RecoveredOld
            }
            Some(State::Pending { new, .. }) if new == &observed.witness => {
                RecoveryDisposition::RecoveredNew
            }
            _ => return Err(invalid()),
        };
        self.publish(
            &mut m,
            State::Stable {
                witness: observed.witness.clone(),
            },
        )?;
        Ok(disposition)
    }
    /// Publish PENDING before the caller's conditional SQL witness update and
    /// COMMIT. A rolled-back SQL transaction must recover observed old first.
    pub(super) fn prepare(
        &self,
        observed: &DatabaseWitness,
        binding: &[u8],
    ) -> Result<DatabaseWitness, StoreError> {
        self.check_observation(observed)?;
        if binding.is_empty() || binding.len() > MAX_BINDING {
            return Err(invalid());
        }
        let mut m = self.memory.lock().map_err(|_| invalid())?;
        if m.poisoned
            || !matches!(&m.state,Some(State::Stable{witness}) if witness==&observed.witness)
        {
            return Err(invalid());
        }
        let mut nonce = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let bytes = serde_json::to_vec(&(
            "postgres-r3-publication/1",
            &self.identity,
            &observed.witness,
            nonce,
            raw_sha256(binding),
        ))
        .map_err(|_| invalid())?;
        let new = raw_sha256(&bytes);
        if new == observed.witness || new.as_str() == "0".repeat(64) {
            return Err(invalid());
        }
        self.publish(
            &mut m,
            State::Pending {
                old: observed.witness.clone(),
                new: new.clone(),
            },
        )?;
        Ok(DatabaseWitness {
            anchor: self.identity.clone(),
            witness: new,
        })
    }
    /// Only exact observed pending-new may finish. Stable/no-pending and wrong
    /// observations reject without overwriting the persisted record.
    pub(super) fn finish(&self, observed: &DatabaseWitness) -> Result<(), StoreError> {
        self.check_observation(observed)?;
        let mut m = self.memory.lock().map_err(|_| invalid())?;
        if m.poisoned || !matches!(&m.state,Some(State::Pending{new,..}) if new==&observed.witness)
        {
            return Err(invalid());
        }
        self.publish(
            &mut m,
            State::Stable {
                witness: observed.witness.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        time::Duration,
    };
    fn observed(f: &Fence, tag: u8) -> DatabaseWitness {
        DatabaseWitness::from_database(f.identity().as_str(), raw_sha256(&[tag]).as_str()).unwrap()
    }
    fn bytes(d: &Path) -> Vec<u8> {
        fs::read(d.join("state.json")).unwrap()
    }
    #[test]
    fn stable_pending_exact_recovery_and_no_growth_refusals() {
        let d = tempfile::tempdir().unwrap();
        let f = Fence::acquire(d.path()).unwrap();
        let old = observed(&f, 1);
        assert_eq!(old.anchor(), f.identity());
        assert_eq!(old.witness(), &raw_sha256(&[1]));
        assert!(f.recover(&old).is_err());
        f.initialize(&old).unwrap();
        let stable = bytes(d.path());
        assert!(f.initialize(&old).is_err());
        assert!(f.finish(&old).is_err());
        assert!(f.prepare(&old, &[]).is_err());
        assert!(f.prepare(&old, &vec![0; MAX_BINDING + 1]).is_err());
        assert_eq!(stable, bytes(d.path()));
        assert_eq!(
            f.recover(&old).unwrap(),
            RecoveryDisposition::UnchangedStable
        );
        assert_eq!(stable, bytes(d.path()));
        let new = f.prepare(&old, &vec![1; MAX_BINDING]).unwrap();
        let pending = bytes(d.path());
        assert_ne!(new, old);
        assert!(pending.len() < MAX_RECORD);
        assert!(f.prepare(&old, b"again").is_err());
        assert!(f.finish(&old).is_err());
        assert!(f.finish(&observed(&f, 3)).is_err());
        assert!(f.recover(&observed(&f, 4)).is_err());
        assert_eq!(pending, bytes(d.path()));
        drop(f);
        let f = Fence::acquire(d.path()).unwrap();
        assert_eq!(f.recover(&old).unwrap(), RecoveryDisposition::RecoveredOld);
        assert_eq!(stable, bytes(d.path()));
        let second = f.prepare(&old, b"another").unwrap();
        assert_ne!(second, new);
        drop(f);
        let f = Fence::acquire(d.path()).unwrap();
        assert_eq!(
            f.recover(&second).unwrap(),
            RecoveryDisposition::RecoveredNew
        );
        assert!(f.recover(&old).is_err());
        assert!(f.finish(&second).is_err());
        let third = f.prepare(&second, b"committed SQL").unwrap();
        f.finish(&third).unwrap();
        assert_eq!(
            f.recover(&third).unwrap(),
            RecoveryDisposition::UnchangedStable
        );
        assert!(!d.path().join("state.next").exists());
    }
    #[test]
    fn unbound_and_malformed_observations_never_initialize_or_overwrite() {
        let d = tempfile::tempdir().unwrap();
        let f = Fence::acquire(d.path()).unwrap();
        assert!(DatabaseWitness::from_database("A", &"a".repeat(64)).is_err());
        assert!(DatabaseWitness::from_database(&"A".repeat(64), &"a".repeat(64)).is_err());
        let unbound = DatabaseWitness::from_database(&"0".repeat(64), &"0".repeat(64)).unwrap();
        assert!(f.initialize(&unbound).is_err());
        let zero = DatabaseWitness::from_database(f.identity().as_str(), &"0".repeat(64)).unwrap();
        assert!(f.initialize(&zero).is_err());
        assert!(!d.path().join("state.json").exists());
        let old = observed(&f, 1);
        f.initialize(&old).unwrap();
        let before = bytes(d.path());
        for x in [unbound, zero] {
            assert!(f.recover(&x).is_err());
            assert!(f.prepare(&x, b"bad").is_err());
            assert!(f.finish(&x).is_err());
        }
        assert_eq!(before, bytes(d.path()));
    }
    #[test]
    fn record_width_closed_shapes_and_copied_identity_reject() {
        for bad in [
            serde_json::json!({"state":"Stable","witness":"a".repeat(63)}),
            serde_json::json!({"state":"Stable","witness":"A".repeat(64)}),
            serde_json::json!({"state":"Stable","witness":"0".repeat(64)}),
            serde_json::json!({"state":"Pending","old":"a".repeat(64),"new":"a".repeat(64)}),
            serde_json::json!({"state":"Stable","witness":"a".repeat(64),"extra":1}),
        ] {
            let d = tempfile::tempdir().unwrap();
            let f = Fence::acquire(d.path()).unwrap();
            let id = f.identity().clone();
            drop(f);
            fs::write(
                d.path().join("state.json"),
                serde_json::to_vec(&serde_json::json!({"version":1,"anchor":id,"publication":bad}))
                    .unwrap(),
            )
            .unwrap();
            assert!(Fence::acquire(d.path()).is_err());
        }
        let d = tempfile::tempdir().unwrap();
        let f = Fence::acquire(d.path()).unwrap();
        f.initialize(&observed(&f, 1)).unwrap();
        let valid = bytes(d.path());
        drop(f);
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("state.json"), &valid).unwrap();
        assert!(Fence::acquire(other.path()).is_err());
        fs::write(d.path().join("state.json"), vec![b' '; MAX_RECORD + 1]).unwrap();
        assert!(Fence::acquire(d.path()).is_err());
        let mut v: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        v["version"] = serde_json::json!(2);
        fs::write(d.path().join("state.json"), serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(Fence::acquire(d.path()).is_err());
    }
    #[test]
    fn symlinks_and_replaced_owner_fail_without_writing_targets() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join("anchor");
        fs::create_dir(&d).unwrap();
        let alias = root.path().join("alias");
        symlink(&d, &alias).unwrap();
        assert!(Fence::acquire(&alias).is_err());
        let target = root.path().join("unrelated");
        fs::write(&target, b"untouched").unwrap();
        symlink(&target, d.join("owner.lock")).unwrap();
        assert!(Fence::acquire(&d).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        fs::remove_file(d.join("owner.lock")).unwrap();
        let f = Fence::acquire(&d).unwrap();
        let old = observed(&f, 1);
        f.initialize(&old).unwrap();
        symlink(&target, d.join("state.next")).unwrap();
        assert!(f.prepare(&old, b"staging").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        assert!(f.recover(&old).is_err());
        drop(f);
        fs::remove_file(d.join("state.next")).unwrap();
        let f = Fence::acquire(&d).unwrap();
        assert_eq!(
            f.recover(&old).unwrap(),
            RecoveryDisposition::UnchangedStable
        );
        fs::rename(d.join("owner.lock"), d.join("old-owner.lock")).unwrap();
        fs::write(d.join("owner.lock"), []).unwrap();
        assert!(f.prepare(&old, b"wrong inode").is_err());
        drop(f);
        assert!(Fence::acquire(&d).is_err());
        fs::remove_file(d.join("state.json")).unwrap();
        symlink(&target, d.join("state.json")).unwrap();
        assert!(Fence::acquire(&d).is_err());
        assert_eq!(fs::read(target).unwrap(), b"untouched");
    }
    fn child(path: &Path, mode: &str) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "store::postgres::adjudication::fence::tests::filesystem_child",
            "--ignored",
            "--nocapture",
        ])
        .env("LEDGERLAB_PG_FENCE_TEST_DIR", path)
        .env("LEDGERLAB_PG_FENCE_TEST_MODE", mode);
        c
    }
    #[test]
    fn actual_process_owner_exclusion_and_release() {
        let d = tempfile::tempdir().unwrap();
        let f = Fence::acquire(d.path()).unwrap();
        let first = child(d.path(), "owned").output().unwrap();
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        drop(f);
        let second = child(d.path(), "available").output().unwrap();
        assert!(
            second.status.success(),
            "{}",
            String::from_utf8_lossy(&second.stderr)
        );
    }
    #[test]
    fn actual_process_kill_preserves_pending_old_new_and_stable_cuts() {
        for mode in ["pending-old", "pending-new", "stable"] {
            let d = tempfile::tempdir().unwrap();
            let f = Fence::acquire(d.path()).unwrap();
            let old = observed(&f, 1);
            f.initialize(&old).unwrap();
            drop(f);
            let mut p = child(d.path(), mode)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let output = p.stdout.take().unwrap();
            let (send, recv) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(output).lines() {
                    let line = line.unwrap();
                    if let Some(v) = line.strip_prefix("FENCE_READY ") {
                        let _ = send.send(v.to_owned());
                        return;
                    }
                }
            });
            let result = recv.recv_timeout(Duration::from_secs(5));
            p.kill().unwrap();
            let status = p.wait().unwrap();
            reader.join().unwrap();
            assert!(!status.success());
            let new = DatabaseWitness::from_database(
                old.anchor().as_str(),
                &result.expect("child publication before kill"),
            )
            .unwrap();
            let f = Fence::acquire(d.path()).unwrap();
            match mode {
                "pending-old" => {
                    assert_eq!(f.recover(&old).unwrap(), RecoveryDisposition::RecoveredOld)
                }
                "pending-new" => {
                    assert_eq!(f.recover(&new).unwrap(), RecoveryDisposition::RecoveredNew)
                }
                _ => {
                    assert!(f.recover(&old).is_err());
                    assert_eq!(
                        f.recover(&new).unwrap(),
                        RecoveryDisposition::UnchangedStable
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "subprocess helper only; invoked by bounded filesystem tests"]
    fn filesystem_child() {
        let d = std::env::var_os("LEDGERLAB_PG_FENCE_TEST_DIR").unwrap();
        let mode = std::env::var("LEDGERLAB_PG_FENCE_TEST_MODE").unwrap();
        if mode == "owned" {
            assert!(matches!(
                Fence::acquire(Path::new(&d)),
                Err(StoreError::Owned)
            ));
            return;
        }
        let f = Fence::acquire(Path::new(&d)).unwrap();
        if mode == "available" {
            return;
        }
        let old = observed(&f, 1);
        assert_eq!(
            f.recover(&old).unwrap(),
            RecoveryDisposition::UnchangedStable
        );
        let new = f.prepare(&old, b"actual process publication cut").unwrap();
        if mode == "stable" {
            f.finish(&new).unwrap();
        }
        println!("FENCE_READY {}", new.witness().as_str());
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
