use super::*;
use ledgerlab::billing::BillingLedger;
use std::io::{IsTerminal, Write};
pub const HELP: &str = "\nOrdinary local billing (separate installation):\n  billing setup DIR --setup FILE       Guided, confirmed private setup\n  billing init DIR --setup FILE        Noninteractive JSON setup\n  billing [--directory DIR] accept FILE|-\n  billing [--directory DIR] outcome FILE|-\n  billing [--directory DIR] correct FILE|-\n  billing [--directory DIR] permissions [FILE|-]\n  billing [--directory DIR] explain TARGET_ID\n  billing [--directory DIR] statement --customer CUSTOMER\n  billing [--directory DIR] export-csv --customer CUSTOMER --snapshot HASH --mapping FILE --output FILE\nBilling output is JSON (pretty-printed unless --json). No payment or tax invoice.\n";
pub async fn run(a: &Args) -> Result<(Value, u8), LocalError> {
    if a.config != Path::new("ledger.json") {
        return Ok(error(
            "USAGE",
            "billing uses --directory DIR, not --config",
            2,
        ));
    }
    let mut args = a.rest.clone();
    let mut directory = PathBuf::from(".");
    if let Some(i) = args.iter().position(|s| s == "--directory") {
        if i + 1 >= args.len() {
            return Ok(error("USAGE", "--directory requires DIR", 2));
        }
        directory = PathBuf::from(args.remove(i + 1));
        args.remove(i);
    }
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    if let ["setup", dir, "--setup", file] = args.as_slice() {
        let tested_os = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|version| version.trim() == "26.6.2");
        if std::env::consts::OS != "macos" || std::env::consts::ARCH != "aarch64" || !tested_os {
            return Ok(error(
                "UNSUPPORTED_PLATFORM",
                "guided setup is limited to the tested macOS 26.6.2 Apple-silicon profile",
                2,
            ));
        }
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Ok(error(
                "SETUP_REQUIRES_TERMINAL",
                "guided setup needs an interactive terminal; use billing init DIR --setup FILE for explicit noninteractive setup",
                2,
            ));
        }
        let path = Path::new(dir);
        if path.exists() || std::fs::symlink_metadata(path).is_ok() {
            return Ok(error(
                "BILLING_DIRECTORY_EXISTS",
                "guided setup will not reuse or modify an existing path; choose a new private directory",
                2,
            ));
        }
        let raw = local::read_file(Path::new(file), local::CONFIG_LIMIT)?;
        let summary = BillingLedger::setup_summary(&raw)?;
        eprintln!("Bean Counter local billing setup");
        eprintln!("The terms below are operator-provided. Review the assent and attestations in the input file; this command does not obtain customer consent.");
        eprintln!("Supported profile: tested macOS 26.6.2 on Apple silicon; this binary reports macOS/aarch64.");
        eprintln!("Source build version: {}", env!("CARGO_PKG_VERSION"));
        eprintln!("Destination: {dir}");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&summary).expect("setup summary JSON")
        );
        eprint!("Type CREATE to initialize this new private installation: ");
        std::io::stderr().flush().map_err(LocalError::from)?;
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(LocalError::from)?;
        if answer.trim() != "CREATE" {
            return Ok(error(
                "SETUP_CANCELLED",
                "setup cancelled; no installation was created",
                2,
            ));
        }
        BillingLedger::init(path, &raw).await?;
        return Ok((
            json!({
                "status": "initialized",
                "directory": dir,
                "profile": "local-retail",
                "dispatch": "disabled",
                "customer": summary["customer"],
                "agreement": summary["agreement"],
                "price_usd": summary["price_usd"],
                "version": env!("CARGO_PKG_VERSION")
            }),
            0,
        ));
    }
    if let ["init", dir, "--setup", file] = args.as_slice() {
        let raw = local::read_file(Path::new(file), local::CONFIG_LIMIT)?;
        BillingLedger::init(Path::new(dir), &raw).await?;
        return Ok((
            json!({"status":"initialized","directory":dir,"profile":"local-retail","dispatch":"disabled"}),
            0,
        ));
    }
    let input = if let ["accept" | "outcome" | "correct" | "permissions", file] = args.as_slice() {
        Some(if *file == "-" {
            local::read_bounded(std::io::stdin().lock(), local::EVENT_LIMIT)?
        } else {
            local::read_file(Path::new(file), local::EVENT_LIMIT)?
        })
    } else {
        None
    };
    if !matches!(
        args.as_slice(),
        ["accept" | "outcome" | "correct" | "permissions", _]
            | ["explain", _]
            | ["permissions"]
            | ["statement", "--customer", _]
            | [
                "export-csv",
                "--customer",
                _,
                "--snapshot",
                _,
                "--mapping",
                _,
                "--output",
                _
            ]
    ) {
        return Ok(error("USAGE", HELP, 2));
    }
    let ledger = BillingLedger::open(&directory).await?;
    if let ["export-csv", "--customer", customer, "--snapshot", snapshot, "--mapping", mapping, "--output", output] =
        args.as_slice()
    {
        let result = match local::read_file(Path::new(mapping), local::CONFIG_LIMIT) {
            Ok(raw) => {
                ledger
                    .export_csv(customer, snapshot, &raw, Path::new(output))
                    .await
            }
            Err(e) => Err(e),
        };
        ledger.close().await;
        return Ok(match result {
            Ok(value) => (value, 0),
            Err(e) => {
                let (mut value, code) = super::output::local_error(e);
                value["complete"] = json!(false);
                value["export_warning"] = json!("No successful export was acknowledged. A complete file may exist after a final directory-sync failure; inspect it before retrying to a new path. Billing history was not changed by this export.");
                (value, code)
            }
        });
    }
    let result = match args.as_slice() {
        ["permissions"] => ledger.permission_status().await,
        ["permissions", _] => ledger.permissions(input.as_ref().unwrap()).await,
        ["accept", _] => ledger.accept(input.as_ref().unwrap()).await,
        ["outcome", _] => ledger.outcome(input.as_ref().unwrap()).await,
        ["correct", _] => ledger.correct(input.as_ref().unwrap()).await,
        ["explain", id] => ledger.explain(id).await,
        ["statement", "--customer", customer] => ledger.statement(customer, None).await,
        _ => unreachable!(),
    };
    ledger.close().await;
    result.map(|v| (v, 0))
}
