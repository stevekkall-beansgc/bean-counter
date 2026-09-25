//! Candidate argument handling only; all semantics live in the core.
use crate::{error, Args};
use ledgerlab::{local, zen_candidate as pilot};
use serde_json::Value;
#[cfg(feature = "zen-charge-e2e-hooks")]
use serde_json::json;
use std::path::Path;
#[cfg(feature = "zen-charge-e2e-hooks")]
use std::{path::PathBuf, time::Duration};
#[cfg(feature = "zen-charge-e2e-hooks")]
use tokio::time::Instant;

pub async fn run(args: &Args) -> Result<(Value, u8), local::LocalError> {
    if !args.json || args.config != Path::new("ledger.json") {
        return Ok(error("USAGE", "candidate requires --json and no --config", 2));
    }
    let r = &args.rest;
    if r.len() < 2 || !Path::new(&r[1]).is_absolute() {
        return Ok(error("USAGE", "zen-charge-candidate init|submit DIR FILE --json; statement DIR --json; DIR must be absolute", 2));
    }
    let path = Path::new(&r[1]);
    let result = match r[0].as_str() {
        "init" | "submit" if r.len() == 3 => {
            let raw = local::read_file(Path::new(&r[2]), 16 * 1024)?;
            if r[0] == "init" { pilot::init(path, &raw).await } else { pilot::execute(path, Some(&raw)).await }
        }
        "statement" if r.len() == 2 => pilot::execute(path, None).await,
        #[cfg(feature = "zen-charge-e2e-hooks")]
        "e2e-cancel-submit" if r.len() == 3 => {
            let raw = local::read_file(Path::new(&r[2]), 16 * 1024)?;
            cancel_submit(path, &raw).await
        }
        #[cfg(feature = "zen-charge-e2e-hooks")]
        "e2e-bounded-api-input" if r.len() == 2 => {
            let raw = vec![b' '; 16 * 1024 + 1];
            let init = pilot::init(path, &raw).await;
            let execute = pilot::execute(path, Some(&raw)).await;
            match (init, execute) {
                (Err(init), Err(execute)) if init.code == "LIMIT" && execute.code == "LIMIT" => {
                    Ok(json!({"candidate":true,"status":"oversized-api-input-rejected"}))
                }
                _ => Err(pilot::CandidateError { code: "E2E_PROBE", exit: 3 }),
            }
        }
        _ => return Ok(error("USAGE", "unknown candidate operation or argument count", 2)),
    };
    Ok(match result { Ok(value) => (value, 0), Err(e) => pilot::error(e) })
}

#[cfg(feature = "zen-charge-e2e-hooks")]
async fn cancel_submit(path: &Path, raw: &[u8]) -> Result<Value, pilot::CandidateError> {
    let directory = std::env::var_os("LEDGER_ZEN_E2E_PAUSE_DIR")
        .map(PathBuf::from)
        .ok_or(pilot::CandidateError { code: "E2E_PROBE", exit: 3 })?;
    let owned_path = path.to_path_buf();
    let owned_raw = raw.to_vec();
    let caller = tokio::spawn(async move {
        pilot::execute(&owned_path, Some(&owned_raw)).await
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if directory.join("ready").exists() {
            break;
        }
        if caller.is_finished() || Instant::now() >= deadline {
            caller.abort();
            let _ = caller.await;
            return Err(pilot::CandidateError { code: "E2E_PROBE", exit: 3 });
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    caller.abort();
    let _ = caller.await;
    std::fs::write(directory.join("cancelled"), b"cancelled")
        .map_err(|_| pilot::CandidateError { code: "E2E_PROBE", exit: 3 })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !directory.join("release").exists() {
        if Instant::now() >= deadline {
            return Err(pilot::CandidateError { code: "E2E_PROBE", exit: 3 });
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while !directory.join("done").exists() {
        if Instant::now() >= deadline {
            return Err(pilot::CandidateError { code: "E2E_PROBE", exit: 3 });
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Ok(json!({"candidate":true,"status":"caller-cancelled"}))
}
