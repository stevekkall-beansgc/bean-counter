//! Synthetic, non-admitting client of existing public core pricing APIs.
//! This example has no financial append port, commit token, or R2 bypass.
#[path = "non_admitting_billing_journey/archive.rs"]
mod archive;
#[path = "non_admitting_billing_journey/engine.rs"]
mod engine;
#[path = "non_admitting_billing_journey/input.rs"]
mod input;
use serde_json::Value;
use std::{error::Error, io, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const CLASSIFICATION: &str = "synthetic_local_acknowledgement";
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, message).into())
    }
}
fn run() -> Result<Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, a, source, b, dir] if mode == "start" && a == "--input" && b == "--artifacts" => {
            archive::start(Path::new(source), Path::new(dir))
        }
        [mode, a, dir] if mode == "replay" && a == "--artifacts" => archive::replay(Path::new(dir)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "start --input FILE --artifacts NEWDIR | replay --artifacts DIR",
        )
        .into()),
    }
}
fn main() {
    match run().and_then(|report| Ok(serde_json::to_string(&report)?)) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("Non-admitting journey refused: {error}");
            std::process::exit(1);
        }
    }
}
