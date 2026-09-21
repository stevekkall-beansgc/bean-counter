//! Local composition root: argument parsing and presentation, no economics.
#![forbid(unsafe_code)]
mod output;
use ledgerlab::local::{self, ExplainTarget, LocalError, LocalLedger};
use serde_json::{json, Value};
use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "Ledger Lab — chain events, compose pricing, explain the result locally.\n\nUsage: ledger [--config PATH] [--format text|json] COMMAND\n\n  init [DIR] --demo          Create a new/empty local sandbox (default: .)\n  accept FILE|-             Accept one event from a JSON file or stdin\n  preview FILE|-            Estimate without journal writes or commit\n  explain EVENT_ID          Read a stored event (external or canonical ID)\n  explain --chain CHAIN_ID  Read accepted decisions in chain order\n\nGlobal options may appear before or after the command:\n  --config PATH             Config file (default: ./ledger.yaml)\n  --format text|json        Human output or one stable ledger-cli/1 JSON object\n  --json                    Alias for --format json\n  --no-color                Accepted; output is always plain text\n  --help, -h                Show help, including after a command\n  --version                 Show internal development version\n\nStart: ledger init ledger-demo --demo\nThen: cd ledger-demo\n      ledger preview examples/generated.json\n      ledger accept examples/generated.json\n      ledger explain --chain demo-slice\n\nAn AI generation is charged initially; a later linked outcome can add a\npremium or discount. Optional paid tools and BYOK/platform-funded responsibility\nfit the same chain. This branch runs the Phase 1 generation slice only:\n100 USD atoms charged, -20 discount, 80 held for fake export. Later linked\noutcomes and supplier authority require Phase 2/3. No payment is executed.\n\nNo Docker, Node, cloud account, provider key, or server is required.\nConfig uses strict JSON syntax in ledger.yaml (a YAML 1.2 subset).\nPreview is an estimate, never a receipt; opening/locking may touch SQLite\nsidecars. File inputs are regular files, at most 256 KiB.\n\nExit codes: 0 success; 2 usage/config/input-file; 3 rejected/not found;\n4 conflict; 5 waiting; 6 unauthorized; 7 busy/unavailable; 8 outcome unknown;\n9 integrity failure. Retry unknown outcomes with the same event identity.\n";
struct Args {
    command: String,
    rest: Vec<String>,
    config: PathBuf,
    json: bool,
}
fn parse(raw: &[String]) -> Result<Args, &'static str> {
    let mut rest = Vec::new();
    let mut config = PathBuf::from("ledger.yaml");
    let mut json = false;
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
            _ => rest.push(raw[i].clone()),
        }
        i += 1;
    }
    if rest.is_empty() {
        return Err("a command is required; use ledger --help");
    }
    let command = rest.remove(0);
    Ok(Args {
        command,
        rest,
        config,
        json,
    })
}
fn error(code: &str, message: &str, exit: u8) -> (Value, u8) {
    (
        json!({"schema":"ledger-cli/1","status":"error","code":code,"message":message}),
        exit,
    )
}
async fn run(a: &Args) -> Result<(Value, u8), LocalError> {
    match a.command.as_str() {
        "init" => {
            if a.rest.iter().filter(|s| s.as_str() == "--demo").count() != 1
                || a.rest.len() > 2
                || a.rest.iter().any(|s| s.starts_with('-') && s != "--demo")
            {
                return Ok(error("USAGE", "usage: ledger init [DIR] --demo", 2));
            }
            if a.config != Path::new("ledger.yaml") {
                return Ok(error(
                    "USAGE",
                    "init creates DIR/ledger.yaml; --config is for application commands",
                    2,
                ));
            }
            let path = a
                .rest
                .iter()
                .find(|s| s.as_str() != "--demo")
                .map(Path::new)
                .unwrap_or(Path::new("."));
            LocalLedger::init_demo(path).await?;
            return Ok((
                json!({"schema":"ledger-cli/1","status":"initialized","config":path.join("ledger.yaml"),"demo":true,"chain":"demo-slice","dispatch":"held"}),
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
        Some(if a.rest[0] == "-" {
            local::read_bounded(std::io::stdin().lock(), local::EVENT_LIMIT)?
        } else {
            local::read_file(Path::new(&a.rest[0]), local::EVENT_LIMIT)?
        })
    } else {
        None
    };
    let ledger = LocalLedger::open(&a.config).await?;
    let result = match a.command.as_str() {
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
    ledger.close().await;
    result
}
fn main() -> ExitCode {
    let raw: Vec<String> = env::args().skip(1).collect();
    if raw.iter().any(|s| matches!(s.as_str(), "--help" | "-h")) {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if raw == ["--version"] {
        println!("ledger {} (local development)", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let wants_json =
        raw.iter().any(|s| s == "--json") || raw.windows(2).any(|s| s == ["--format", "json"]);
    let (result, code, json_output) = match parse(&raw) {
        Err(message) => {
            let (v, c) = error("USAGE", message, 2);
            (v, c, wants_json)
        }
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
            (result.0, result.1, a.json)
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
    } else {
        print!("{}", output::text(&result));
    }
    ExitCode::from(code)
}
