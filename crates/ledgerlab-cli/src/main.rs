//! Local composition root: argument parsing and presentation, no economics.
#![forbid(unsafe_code)]
mod billing;
mod output;
#[cfg(feature = "zen-charge-candidate")]
mod zen_candidate;
use ledgerlab::local::{self, ExplainTarget, LocalError, LocalLedger};
use serde_json::{json, Value};
use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "Bean Counter — local SQLite billing with retained receipts and explanations.

Usage: ledger [--format text|json] COMMAND

Current billing profile:
  billing --help            Setup, work, outcomes, corrections and reports
  billing setup DIR         Guided private setup
  billing init DIR --setup FILE
                            Noninteractive setup with explicit terms
  billing accept --help     Record completed work
  billing outcome --help    Record an authorized outcome
  billing correct --help    Correct an outcome or cumulative quantity
  billing explain --help    Read retained target history
  billing statement --help  Read a reconciled customer statement

Legacy synthetic generation profile (separate installation):
  init [DIR] --demo          Create the Phase 1 generation demo
  accept FILE|-             Accept a legacy generation event
  preview FILE|-            Estimate a legacy generation event
  explain EVENT_ID          Read legacy stored history
  explain --chain CHAIN_ID  Read legacy chain history
  Use ledger COMMAND --help for that legacy command's limitations.

Global options may appear before or after the command:
  --format text|json        Output format; billing results are JSON in either mode
  --json                    Alias for --format json
  --config PATH             Legacy profile config (default: ./ledger.json)
  --no-color                Accepted; output is always plain text
  --help, -h                Show help for the named command, or root help
  --version                 Show internal development version

Help is plain text and does not open or create an installation. Known commands
may omit their required arguments when requesting help. Unknown command paths
and malformed global options return USAGE, exit 2 (JSON when requested).
Billing --directory DIR may appear anywhere after billing; default: .
See START-HERE.md and docs/billing-quickstart.md for current billing.
No payment collection, tax invoice, hosted service or remote authentication.

Exit codes: 0 success; 2 usage/config/input-file; 3 rejected/not found;
4 conflict; 5 waiting; 6 unauthorized; 7 busy/unavailable; 8 outcome unknown;
9 integrity failure. Retry unknown outcomes with the original identity/input.
";
const LEGACY_NOTES: &str =
    "Legacy synthetic Phase 1 generation profile only; separate from ledger billing.
The demo records demo-customer -> demo-host USD 0.80 (1.00 charge - 0.20 discount).
Linked events, outcomes and reversals cannot be saved or previewed in this profile.
Dispatch is held; no payment is executed. See docs/quickstart.md.
";
fn help(a: &Args) -> Result<String, &'static str> {
    if a.command == "billing" {
        if a.config != Path::new("ledger.json") {
            return Err("billing uses --directory DIR, not --config");
        }
        return billing::help(&a.rest);
    }
    let usage = match a.command.as_str() {
        "" => return Ok(HELP.into()),
        "init" => "Usage: ledger init [DIR] --demo\nCreate a new or empty legacy demo directory; default: .\n",
        "accept" => "Usage: ledger [--config PATH] accept FILE|-\nAccept one legacy generation event from a regular JSON file or stdin.\n",
        "preview" => "Usage: ledger [--config PATH] preview FILE|-\nEstimate one legacy generation event; no journal writes or committed receipt.\nOpening or locking for a preview may touch SQLite sidecars.\n",
        "explain" => "Usage: ledger [--config PATH] explain EVENT_ID\n       ledger [--config PATH] explain --chain CHAIN_ID\nRead retained legacy history without repricing.\n",
        #[cfg(feature = "zen-charge-candidate")]
        "zen-charge-candidate" => return Ok("Usage: ledger zen-charge-candidate init|submit DIR FILE --json\n       ledger zen-charge-candidate statement DIR --json\nExperimental candidate profile; DIR must be absolute.\n".into()),
        _ => return Err("unknown command; use ledger --help"),
    };
    Ok(format!("{usage}\n{LEGACY_NOTES}\nGlobal options: --config PATH, --format text|json, --json, --no-color, --help, -h.\nFile inputs are regular files of at most 256 KiB; legacy config is strict JSON.\n"))
}
struct Args {
    command: String,
    rest: Vec<String>,
    config: PathBuf,
    json: bool,
    help: bool,
}
fn parse(raw: &[String]) -> Result<Args, &'static str> {
    let mut rest = Vec::new();
    let mut config = PathBuf::from("ledger.json");
    let mut json = false;
    let mut help = false;
    let mut i = 0;
    let mut seen_config = false;
    let mut seen_format = false;
    while i < raw.len() {
        match raw[i].as_str() {
            "--config" => {
                if seen_config {
                    return Err("--config supplied twice");
                }
                seen_config = true;
                i += 1;
                let value = raw
                    .get(i)
                    .filter(|v| !v.starts_with('-'))
                    .ok_or("--config requires PATH")?;
                config = value.into();
            }
            "--format" => {
                if seen_format {
                    return Err("output format supplied twice");
                }
                seen_format = true;
                i += 1;
                json = match raw.get(i).map(String::as_str) {
                    Some("text") => false,
                    Some("json") => true,
                    _ => return Err("--format requires text or json"),
                };
            }
            "--json" => {
                if seen_format {
                    return Err("output format supplied twice");
                }
                seen_format = true;
                json = true;
            }
            "--no-color" => (),
            "--help" | "-h" => help = true,
            _ => rest.push(raw[i].clone()),
        }
        i += 1;
    }
    if rest.is_empty() && !help {
        return Err("a command is required; use ledger --help");
    }
    let command = if rest.is_empty() {
        String::new()
    } else {
        rest.remove(0)
    };
    Ok(Args {
        command,
        rest,
        config,
        json,
        help,
    })
}
fn error(code: &str, message: &str, exit: u8) -> (Value, u8) {
    (
        json!({"schema":"ledger-cli/1","status":"error","code":code,"message":message}),
        exit,
    )
}
async fn run(a: &Args) -> Result<(Value, u8), LocalError> {
    #[cfg(feature = "zen-charge-candidate")]
    if a.command == "zen-charge-candidate" {
        return zen_candidate::run(a).await;
    }
    if a.command == "billing" {
        return billing::run(a).await;
    }
    match a.command.as_str() {
        "init" => {
            if a.rest.iter().filter(|s| s.as_str() == "--demo").count() != 1
                || a.rest.len() > 2
                || a.rest.iter().any(|s| s.starts_with('-') && s != "--demo")
            {
                return Ok(error("USAGE", "usage: ledger init [DIR] --demo", 2));
            }
            if a.config != Path::new("ledger.json") {
                return Ok(error(
                    "USAGE",
                    "init creates DIR/ledger.json; --config is for application commands",
                    2,
                ));
            }
            let path = a
                .rest
                .iter()
                .find(|s| s.as_str() != "--demo")
                .map(Path::new)
                .unwrap_or(Path::new("."));
            if let Err(e) = LocalLedger::init_demo(path).await {
                return Ok(output::file_error(
                    e,
                    path,
                    "choose a new or empty directory with an existing writable parent",
                ));
            }
            return Ok((
                json!({"schema":"ledger-cli/1","status":"initialized","config":path.join("ledger.json"),"demo":true,"chain":"demo-slice","dispatch":"held"}),
                0,
            ));
        }
        "accept" | "preview"
            if a.rest.len() == 1 && (a.rest[0] == "-" || !a.rest[0].starts_with('-')) => {}
        "explain"
            if (a.rest.len() == 1 && !a.rest[0].starts_with('-'))
                || (a.rest.len() == 2 && a.rest[0] == "--chain" && !a.rest[1].starts_with('-')) => {
        }
        _ => {
            return Ok(error(
                "USAGE",
                "unknown command or arguments; use ledger --help",
                2,
            ))
        }
    }
    // Read before opening so stdin is never held under the SQLite owner lock.
    let input = if matches!(a.command.as_str(), "accept" | "preview") {
        let read = if a.rest[0] == "-" {
            local::read_bounded(std::io::stdin().lock(), local::EVENT_LIMIT)
        } else {
            local::read_file(Path::new(&a.rest[0]), local::EVENT_LIMIT)
        };
        match read {
            Ok(bytes) => Some(bytes),
            Err(e) => return Ok(output::file_error(
                e,
                Path::new(&a.rest[0]),
                "supply a readable regular JSON event file of at most 256 KiB, or use - for stdin",
            )),
        }
    } else {
        None
    };
    let ledger = match LocalLedger::open(&a.config).await {
        Ok(ledger) => ledger,
        Err(e) => return Ok(output::file_error(e, &a.config, "check this config and storage.data_dir; use --config PATH for an existing config or ledger init NEW_DIR --demo")),
    };
    let mut result = match a.command.as_str() {
        "accept" => ledger.accept(input.unwrap()).await.map(output::accept),
        "preview" => ledger.preview(input.unwrap()).await.map(output::preview),
        "explain" => {
            let target = if a.rest.len() == 2 {
                ExplainTarget::Chain(a.rest[1].clone())
            } else {
                ExplainTarget::Event(a.rest[0].clone())
            };
            ledger.explain(target).await.map(|mut value| {
                if value["events"].as_array().is_none_or(Vec::is_empty) {
                    error(
                        "NOT_FOUND",
                        "no accepted event found in this source/scope",
                        3,
                    )
                } else {
                    value["schema"] = json!("ledger-cli/1");
                    value["status"] = json!("explained");
                    (value, 0)
                }
            })
        }
        _ => unreachable!(),
    };
    if let Ok((value, _)) = &mut result {
        if value["status"] == "rejected" || value["outcome"] == "rejected" {
            output::event_guidance(value, &a.rest[0]);
        }
        // Human receipts use the verified retained projection. This is a read,
        // never repricing, and a read failure cannot undo successful acceptance.
        if !a.json && (value["status"] == "accepted" || value["status"] == "duplicate") {
            if let Some(id) = value["receipt"]["event_id"].as_str() {
                match ledger.explain(ExplainTarget::Event(id.into())).await {
                    Ok(history) => value["human_history"] = history,
                    Err(_) => value["human_warning"] = json!("Receipt committed, but its breakdown could not be read. Retry identical input with --json to retrieve the receipt, then ledger explain EVENT_ID."),
                }
            }
        }
    }
    ledger.close().await;
    result
}
fn main() -> ExitCode {
    let raw: Vec<String> = env::args().skip(1).collect();
    if raw == ["--version"] {
        println!("ledger {} (local development)", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let wants_json =
        raw.iter().any(|s| s == "--json") || raw.windows(2).any(|s| s == ["--format", "json"]);
    let (result, code, json_output, billing_output) = match parse(&raw) {
        Err(message) => {
            let (v, c) = error("USAGE", message, 2);
            (v, c, wants_json, false)
        }
        Ok(a) if a.help => match help(&a) {
            Ok(text) => {
                print!("{text}");
                return ExitCode::SUCCESS;
            }
            Err(message) => {
                let (v, c) = error("USAGE", message, 2);
                (v, c, a.json, false)
            }
        },
        Ok(a) => {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime
                    .block_on(run(&a))
                    .unwrap_or_else(output::local_error),
                Err(_) => error("UNAVAILABLE", "could not start local runtime", 7),
            };
            (result.0, result.1, a.json, a.command == "billing")
        }
    };
    if json_output {
        println!("{}", serde_json::to_string(&result).expect("JSON output"));
    } else if result["status"] == "error" {
        eprintln!(
            "{}: {}",
            result["code"].as_str().unwrap_or("ERROR"),
            result["message"].as_str().unwrap_or("operation failed")
        );
    } else if billing_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).expect("billing JSON")
        );
    } else {
        print!("{}", output::text(&result));
    }
    ExitCode::from(code)
}
