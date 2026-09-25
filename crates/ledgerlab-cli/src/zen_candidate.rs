//! Candidate argument handling only; all semantics live in the core.
use crate::{error, Args};
use ledgerlab::{local, zen_candidate as pilot};
use serde_json::Value;
use std::path::Path;

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
        _ => return Ok(error("USAGE", "unknown candidate operation or argument count", 2)),
    };
    Ok(match result { Ok(value) => (value, 0), Err(e) => pilot::error(e) })
}
