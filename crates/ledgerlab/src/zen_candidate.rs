//! Explicitly opt-in synthetic candidate coordinator. Separate store and record family.
//!
//! `init` and `execute` parse bounded input before spawning task-owned database
//! work. Dropping the caller's future does not cancel a spawned mutation while
//! its Tokio runtime remains live and driven. Runtime shutdown can abort active
//! work; callers must await the result, or use statement/replay reconciliation
//! when the result is unknown.
use crate::{local, store::sqlite::zen_candidate::CandidateStore};
use ledgerlab_core::zen_candidate::{self as core, Command, Setup, State};
use serde_json::{json, Value};
use std::{fs, path::Path, time::{SystemTime, UNIX_EPOCH}};
#[cfg(feature = "zen-charge-e2e-hooks")]
use std::time::Duration;

#[derive(Debug)]
pub struct CandidateError { pub code: &'static str, pub exit: u8 }
fn unavailable(_: impl std::fmt::Debug) -> CandidateError { CandidateError { code: "UNAVAILABLE", exit: 7 } }
fn integrity(_: impl std::fmt::Debug) -> CandidateError { CandidateError { code: "INTEGRITY", exit: 9 } }
fn semantic(e: ledgerlab_core::Error) -> CandidateError {
    CandidateError { code: e.code, exit: if e.code == "BINDING_CONFLICT" { 4 } else if e.code == "UNAUTHORIZED_ORDER" { 6 } else { 3 } }
}
fn unknown(_: ()) -> CandidateError { CandidateError { code: "OUTCOME_UNKNOWN", exit: 8 } }
type Result<T> = std::result::Result<T, CandidateError>;
const MARKER: &[u8] = b"synthetic-only admission/1-candidate.1\n";

pub async fn init(path: &Path, raw: &[u8]) -> Result<Value> {
    let setup: Setup = core::parse(raw).map_err(semantic)?;
    let path = path.to_path_buf();
    tokio::spawn(async move { init_owned(&path, setup).await })
        .await
        .map_err(|_| unknown(()))?
}

async fn init_owned(path: &Path, setup: Setup) -> Result<Value> {
    let state = State::new(setup.clone()).map_err(semantic)?;
    // Never reuse an installation; failure leaves its incomplete directory visible.
    local::private_dir(path).map_err(unavailable)?;
    local::write_new(&path.join("candidate.marker"), MARKER).map_err(unavailable)?;
    fs::File::open(path.parent().unwrap_or(Path::new("."))).and_then(|f| f.sync_all()).map_err(unavailable)?;
    let mut store = CandidateStore::open(path, true).await.map_err(unavailable)?;
    let result = async {
        store.begin().await.map_err(unavailable)?;
        store.initialize(&core::bytes(&setup).map_err(semantic)?).await.map_err(unavailable)?;
        store.end(true).await.map_err(unknown)?;
        fs::File::open(path).and_then(|f| f.sync_all()).map_err(|_| unknown(()))?;
        state.statement().map_err(semantic)
    }.await;
    store.close().await;
    result
}

pub async fn execute(path: &Path, raw: Option<&[u8]>) -> Result<Value> {
    let command: Option<Command> = raw.map(core::parse).transpose().map_err(semantic)?;
    let path = path.to_path_buf();
    tokio::spawn(async move { execute_owned(&path, command).await })
        .await
        .map_err(|_| unknown(()))?
}

async fn execute_owned(path: &Path, command: Option<Command>) -> Result<Value> {
    local::private_existing(path, true).map_err(unavailable)?;
    local::private_existing(&path.join("candidate.marker"), false).map_err(integrity)?;
    if fs::read(path.join("candidate.marker")).map_err(integrity)? != MARKER { return Err(integrity(())); }
    let mut store = CandidateStore::open(path, false).await.map_err(unavailable)?;
    let result = transact(&mut store, command.as_ref()).await;
    // On errors, explicit rollback where possible, then discard the connection.
    if result.is_err() { let _ = store.end(false).await; }
    store.close().await;
    #[cfg(feature = "zen-charge-e2e-hooks")]
    if let Ok(directory) = std::env::var("LEDGER_ZEN_E2E_PAUSE_DIR") {
        let _ = fs::write(Path::new(&directory).join("done"), b"done");
    }
    result
}
async fn transact(store: &mut CandidateStore, command: Option<&Command>) -> Result<Value> {
    store.begin().await.map_err(unavailable)?;
    let setup_raw = store.setup().await.map_err(integrity)?;
    let setup: Setup = core::parse(&setup_raw).map_err(integrity)?;
    if core::bytes(&setup).map_err(integrity)? != setup_raw { return Err(integrity(())); }
    let mut state = State::new(setup).map_err(integrity)?;
    let rows = store.rows().await.map_err(integrity)?;
    if rows.len() > 32 { return Err(integrity(())); }
    for (index, (seq, raw, at, saved)) in rows.iter().enumerate() {
        let cmd: Command = core::parse(raw).map_err(integrity)?;
        let time: u64 = at.parse().map_err(integrity)?;
        if *seq != index as i64 + 1 || time.to_string() != *at || core::bytes(&cmd).map_err(integrity)? != *raw { return Err(integrity(())); }
        let decision = state.apply(&cmd, time).map_err(integrity)?;
        if !decision.changed || core::bytes(&decision.response).map_err(integrity)? != *saved { return Err(integrity(())); }
    }
    let Some(command) = command else {
        let statement = state.statement().map_err(semantic)?;
        store.end(false).await.map_err(unavailable)?;
        return Ok(statement);
    };
    let at = observed_time()?;
    let decision = state.apply(command, at).map_err(semantic)?;
    if decision.changed {
        if rows.len() >= 32 { return Err(CandidateError { code: "HISTORY_LIMIT", exit: 3 }); }
        store.append(rows.len() as i64 + 1, &core::bytes(command).map_err(semantic)?, at,
            &core::bytes(&decision.response).map_err(semantic)?).await.map_err(unavailable)?;
        #[cfg(feature = "zen-charge-e2e-hooks")]
        if let Ok(directory) = std::env::var("LEDGER_ZEN_E2E_PAUSE_DIR") {
            let directory = Path::new(&directory);
            fs::write(directory.join("ready"), b"ready").map_err(unavailable)?;
            while !directory.join("release").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        crash("before_commit", 91);
        store.end(true).await.map_err(unknown)?;
        crash("after_commit", 92);
    } else {
        store.end(false).await.map_err(unavailable)?;
    }
    Ok(decision.response)
}
fn crash(point: &str, code: i32) {
    // Neither hook exists in the default or ordinary candidate runtime build.
    #[cfg(feature = "zen-charge-e2e-hooks")]
    if std::env::var("LEDGER_ZEN_CANDIDATE_CRASH").ok().as_deref() == Some(point) { std::process::exit(code); }
    #[cfg(not(feature = "zen-charge-e2e-hooks"))]
    let _ = (point, code);
}
fn observed_time() -> Result<u64> {
    #[cfg(feature = "zen-charge-e2e-hooks")]
    if let Ok(raw) = std::env::var("LEDGER_ZEN_E2E_TIME_US") {
        let invalid = || CandidateError { code: "E2E_CLOCK_INPUT", exit: 2 };
        let at: u64 = raw.parse().map_err(|_| invalid())?;
        if at.to_string() != raw || at > i64::MAX as u64 { return Err(invalid()); }
        return Ok(at);
    }
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(unavailable)?.as_micros()).map_err(unavailable)
}
pub fn error(e: CandidateError) -> (Value, u8) {
    (json!({"schema":core::PROFILE,"candidate":true,"status":"error","code":e.code}), e.exit)
}
