use super::*;
use ledgerlab::billing::BillingLedger;
use std::io::{self, IsTerminal, Write};
pub const HELP: &str = "\nOrdinary local billing (separate installation):\n  billing setup DIR [--setup FILE]     Guided, confirmed private setup\n  billing init DIR --setup FILE        Noninteractive JSON setup\n  billing [--directory DIR] accept FILE|-\n  billing [--directory DIR] outcome FILE|-\n  billing [--directory DIR] correct FILE|-\n  billing [--directory DIR] permissions [FILE|-]\n  billing [--directory DIR] explain TARGET_ID\n  billing [--directory DIR] statement --customer CUSTOMER\n  billing [--directory DIR] export-csv --customer CUSTOMER --snapshot HASH --mapping FILE --output FILE\nBilling output is JSON (pretty-printed unless --json). No payment or tax invoice.\n";
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
    if let ["setup", dir] | ["setup", dir, "--setup", _] = args.as_slice() {
        let profile = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "macOS arm64",
            ("linux", "x86_64") => "Linux x86-64",
            _ => "",
        };
        if profile.is_empty() {
            return Ok(error(
                "INCOMPATIBLE_PLATFORM",
                "guided setup requires a native macOS Apple-silicon or Linux x86-64 executable; use a matching native build",
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
        let raw = if let [_, _, "--setup", file] = args.as_slice() {
            // Retained compatibility path for users who already prepared strict JSON.
            local::read_file(Path::new(file), local::CONFIG_LIMIT)?
        } else {
            guided_setup_input()?
        };
        let summary = BillingLedger::setup_summary(&raw)?;
        eprintln!("Bean Counter local billing setup");
        eprintln!("These are the terms and assertions you supplied. This program does not contact the customer, obtain assent, or verify your authority.");
        if profile == "macOS arm64" {
            let host_version = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();
            if host_version.trim() == "26.6.2" {
                eprintln!("Host: macOS 26.6.2 / Apple silicon (previously tested host; this guided wizard still requires its focused behavior check).");
            } else {
                eprintln!("Host: macOS {} / Apple silicon. This version is untested; this warning is not a compatibility verdict. The running binary has passed the OS loader, but storage durability and other host behavior are not certified.", host_version.trim());
            }
        } else {
            let release = std::fs::read_to_string("/etc/os-release")
                .unwrap_or_else(|_| "distribution unknown".into());
            let pretty = release
                .lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                .unwrap_or("distribution/version unknown")
                .trim_matches('"');
            eprintln!("Host: {profile}, {pretty}. This Linux host is untested until the approved Ubuntu x86-64 package and installation path pass on the standard runner. The running binary has passed the loader; other ABI and storage behavior are not yet certified.");
        }
        eprintln!("Source build version: {}", env!("CARGO_PKG_VERSION"));
        eprintln!("Destination: {dir}");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&summary).expect("setup summary JSON")
        );
        let supplied: Value = serde_json::from_slice(&raw).expect("validated setup JSON");
        eprintln!(
            "Assent evidence supplied: {:?}",
            supplied["assent_evidence"]
        );
        eprintln!(
            "Operator authority attestation supplied: {:?}",
            supplied["operator_attestation"]
        );
        eprintln!(
            "Finality attestation supplied: {:?}",
            supplied["finality_attestation"]
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
        let (setup_config_retained, setup_config_failed_target) =
            match write_private_setup_copy(path, &raw) {
                Ok(()) => (true, Value::Null),
                Err(recovery) => (false, json!(recovery.display().to_string())),
            };
        return Ok((
            json!({
                "status": "initialized",
                "directory": dir,
                "profile": "local-retail",
                "dispatch": "disabled",
                "customer": summary["customer"],
                "agreement": summary["agreement"],
                "price_usd": summary["price_usd"],
                "setup_config_retained": setup_config_retained,
                "setup_config_failed_target": setup_config_failed_target,
                "setup_config_warning": if setup_config_retained { Value::Null } else { json!("installation succeeded, but setup.json was not retained at the reported target; consult the actual agreement and retained assent evidence to prepare a private repeatable configuration") },
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

fn ask(label: &str, guidance: &str) -> Result<String, LocalError> {
    eprint!("{label} ({guidance}): ");
    io::stderr().flush().map_err(LocalError::from)?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(LocalError::from)?
        == 0
    {
        return Err(LocalError::Config("setup cancelled at end of input"));
    }
    let value = value.trim().to_owned();
    if value.eq_ignore_ascii_case("cancel") {
        return Err(LocalError::Config(
            "setup cancelled; no installation was created",
        ));
    }
    if value.is_empty() {
        return Err(LocalError::Config(
            "setup requires a non-empty answer; no installation was created",
        ));
    }
    Ok(value)
}

fn amount_atoms(prompt: &str, guidance: &str) -> Result<i128, LocalError> {
    let input = ask(prompt, guidance)?;
    exact_amount_atoms(&input)
}

fn exact_amount_atoms(input: &str) -> Result<i128, LocalError> {
    let (negative, digits) = input
        .strip_prefix('-')
        .map_or((false, input), |v| (true, v));
    let mut parts = digits.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("0");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.is_empty()
        || fraction.len() > 2
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(LocalError::Config("amount must be an exact decimal with at most two fractional digits; no installation was created"));
    }
    let cents = format!("{fraction:0<2}");
    let whole: i128 = whole
        .parse()
        .map_err(|_| LocalError::Config("amount is out of range; no installation was created"))?;
    let fraction: i128 = cents
        .parse()
        .map_err(|_| LocalError::Config("invalid exact decimal; no installation was created"))?;
    let atoms = whole
        .checked_mul(100)
        .and_then(|n| n.checked_add(fraction))
        .ok_or(LocalError::Config(
            "amount is out of range; no installation was created",
        ))?;
    if negative && atoms == 0 {
        return Err(LocalError::Config(
            "negative zero is not a valid amount; no installation was created",
        ));
    }
    Ok(if negative { -atoms } else { atoms })
}

fn price_decimal(atoms: i128) -> String {
    let sign = if atoms < 0 { "-" } else { "" };
    let magnitude = atoms.unsigned_abs();
    format!("{sign}{}.{:02}", magnitude / 100, magnitude % 100)
}

fn guided_setup_input() -> Result<Vec<u8>, LocalError> {
    eprintln!("Answer each question using the real agreement and your actual authority. Type cancel at any prompt to stop. Amounts are exact USD decimals; the ordinary and correction windows are fixed absolute UTC cutoffs.");
    let scope_tenant = ask(
        "Tenant / organization identifier",
        "short stable identifier",
    )?;
    let scope_environment = ask(
        "Environment identifier",
        "for example production or staging",
    )?;
    let store = ask("Customer identifier", "the customer bound by the agreement")?;
    let host = ask(
        "Your receiving organization identifier",
        "must differ from the customer",
    )?;
    let operator = ask("Operator identifier", "your real operator identity")?;
    let source = ask(
        "Work source URI",
        "stable absolute URI used by submitted events",
    )?;
    let agreement = ask("Agreement identifier", "identifier in the real agreement")?;
    let acceptor = ask(
        "Authorized acceptor identifier",
        "person/entity that actually accepted",
    )?;
    let price_atoms = amount_atoms(
        "Agreed fixed price per call",
        "exact positive USD amount, for example 12.34; no rounding",
    )?;
    if price_atoms <= 0 {
        return Err(LocalError::Config(
            "agreed price must be positive; no installation was created",
        ));
    }
    let accepted_at = ask("Agreement acceptance time", "RFC3339 UTC timestamp with Z")?;
    eprintln!("Enter each window as four UTC timestamps in order: starts_at, occurs_before (exclusive), received_by, accepted_by. The work event must occur no later than both window starts; cutoffs are immutable.");
    let window = |kind: &str| -> Result<Value, LocalError> {
        Ok(json!({
            "starts_at": ask(&format!("{kind} window starts_at"), "RFC3339 UTC")?,
            "occurs_before": ask(&format!("{kind} window occurs_before"), "exclusive RFC3339 UTC cutoff")?,
            "received_by": ask(&format!("{kind} window received_by"), "inclusive RFC3339 UTC cutoff")?,
            "accepted_by": ask(&format!("{kind} window accepted_by"), "inclusive RFC3339 UTC cutoff")?
        }))
    };
    let ordinary = window("Ordinary")?;
    let corrections = window("Correction")?;
    let family = ask(
        "Outcome family identifier",
        "lowercase letters, digits, dot, underscore, hyphen",
    )?;
    eprintln!("Define the available fixed adjustment codes exactly as agreed. Enter at least one. A zero amount means no adjustment for that outcome; negative amounts are discounts and positive amounts are premiums.");
    let mut codes = Vec::new();
    loop {
        let code = ask("Outcome code", "stable lowercase identifier")?;
        let atoms = amount_atoms(
            &format!("Fixed adjustment for {code}"),
            "exact USD amount; negative discount, positive premium, zero no adjustment",
        )?;
        codes.push(json!({"code":code,"amount":{"kind":"fixed","money":{"currency":"USD","scale":2,"atoms":atoms.to_string()}}}));
        if codes.len() == 32 {
            eprintln!("Maximum of 32 codes reached.");
            break;
        }
        let more = ask("Add another outcome code?", "yes or no")?;
        match more.to_ascii_lowercase().as_str() {
            "yes" | "y" => continue,
            "no" | "n" => break,
            _ => {
                return Err(LocalError::Config(
                    "answer yes or no; no installation was created",
                ))
            }
        }
    }
    eprintln!("Enter replacement-eligible codes as a comma-separated list from the codes just entered. This list is contractual; an empty list means no replacement is authorized.");
    let repl = ask(
        "Replacement-eligible outcome codes",
        "comma-separated exact code names, or none",
    )?;
    let replacements: Vec<Value> = if repl.eq_ignore_ascii_case("none") {
        vec![]
    } else {
        repl.split(',').map(|s| json!(s.trim())).collect()
    };
    let premium_atoms = amount_atoms(
        "Maximum premium per call",
        "exact nonnegative USD amount; no rounding",
    )?;
    if premium_atoms < 0 {
        return Err(LocalError::Config(
            "premium ceiling cannot be negative; no installation was created",
        ));
    }
    let reversal = ask("May a final outcome be reversed?", "yes or no, as agreed")?;
    let allow_reversal = match reversal.to_ascii_lowercase().as_str() {
        "yes" | "y" => true,
        "no" | "n" => false,
        _ => {
            return Err(LocalError::Config(
                "answer yes or no; no installation was created",
            ))
        }
    };
    let assent = ask("Retained assent evidence", "accurate description/reference to actual retained evidence; not a claim that this program obtained consent")?;
    let authority = ask(
        "Authority attestation",
        "truthful statement of your authority to configure these exact terms",
    )?;
    let finality = ask(
        "Finality attestation",
        "truthful statement of the agreed finality rule",
    )?;
    eprintln!("Read and submit permissions are required for billing operations. Correction permission is separate; enable it only if your agreement authorizes you to submit corrections.");
    let correct_answer = ask("Grant correction permission?", "yes or no")?;
    let mut permissions = vec!["read", "submit"];
    match correct_answer.to_ascii_lowercase().as_str() {
        "yes" | "y" => permissions.push("correct"),
        "no" | "n" => (),
        _ => {
            return Err(LocalError::Config(
                "answer yes or no; no installation was created",
            ))
        }
    }
    let permission_text = permissions.join(", ");
    let confirm_permissions = ask(
        "Confirm operator permissions",
        &format!("type exactly {permission_text}"),
    )?;
    if confirm_permissions != permission_text {
        return Err(LocalError::Config(
            "permission confirmation did not match; no installation was created",
        ));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| LocalError::Config("system clock is before Unix epoch"))?
        .as_nanos();
    let setup = json!({
        "schema":"ledger-local-billing/1", "scope":[scope_tenant,scope_environment],
        "store_id":format!("local-{nanos}"), "operator":operator, "source":source.clone(),
        "customer":store, "host":host, "agreement":agreement,
        "binding":format!("binding-{nanos}"), "price": price_decimal(price_atoms),
        "accepted_at":accepted_at, "acceptor":acceptor,
        "assent_evidence":assent, "operator_attestation":authority, "finality_attestation":finality,
        "permissions":permissions,
        "outcome_policy":{"version":"1","families":[{"family":family,"binding_id":format!("binding-{nanos}"),"source":source.clone(),"correction_source":source,"evidence_required":true,"ordinary":ordinary,"corrections":corrections,"codes":codes,"replacement_codes":replacements,"allow_reversal":allow_reversal}],"limits":[{"binding_id":format!("binding-{nanos}"),"premium":{"currency":"USD","scale":2,"atoms":premium_atoms.to_string()}}]}
    });
    serde_json::to_vec_pretty(&setup).map_err(|_| LocalError::Config("could not serialize setup"))
}

#[cfg(unix)]
fn write_private_setup_copy(path: &Path, raw: &[u8]) -> Result<(), PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let target = path.join("setup.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&target)
        .map_err(|_| target.clone())?;
    if file.write_all(raw).and_then(|()| file.sync_all()).is_err() {
        drop(file);
        let _ = std::fs::remove_file(&target);
        return Err(target);
    }
    Ok(())
}

#[cfg(test)]
mod setup_input_tests {
    use super::*;

    #[test]
    fn exact_money_input_does_not_round_and_preserves_minor_units() {
        assert_eq!(exact_amount_atoms("12").unwrap(), 1200);
        assert_eq!(exact_amount_atoms("0.1").unwrap(), 10);
        assert_eq!(exact_amount_atoms("-0.05").unwrap(), -5);
        assert_eq!(price_decimal(exact_amount_atoms("12.30").unwrap()), "12.30");
        assert!(exact_amount_atoms("1.005").is_err());
        assert!(exact_amount_atoms("-0.00").is_err());
        assert!(exact_amount_atoms("1e2").is_err());
    }
}
#[cfg(not(unix))]
fn write_private_setup_copy(path: &Path, raw: &[u8]) -> Result<(), PathBuf> {
    let target = path.join("setup.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|_| target.clone())?;
    if file.write_all(raw).and_then(|()| file.sync_all()).is_err() {
        drop(file);
        let _ = std::fs::remove_file(&target);
        return Err(target);
    }
    Ok(())
}
