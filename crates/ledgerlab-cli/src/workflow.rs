//! Thin local-owner workflow commands. All validation and economics stay in
//! the existing host/core; this module never constructs a journal append.
use super::*;
use ledgerlab::{LocalWorkflowLedger, ServiceError};

const HELP: &str = "Bean Counter generic local workflow\n\nUsage: ledger workflow [--directory DIR] COMMAND\n\n  enable             Explicitly opt this installation into Profile B\n  submit FILE|-      Submit an original workflow request or its exact retry\n  status KEYFILE|-   Reconcile an original identity without a new request\n  read FILE|-        Read an exact retained document or descriptor request\n  resources show     Show effective operator resource settings\n  resources set FILE Atomically set trusted installation resource settings\n\nRequests are strict JSON, at most 256 KiB. The host selects supported policy,\nauthority and financial recipes; unsupported profiles refuse without a write.\nExisting billing installation and private local-owner access are required.\nNo payment collection, tax invoice or remote authentication.\n--directory DIR may appear anywhere after workflow; default: .\nHelp does not open an installation or read an input file.\n";

fn arguments(raw: &[String]) -> Result<(PathBuf, Vec<&str>), &'static str> {
    let mut directory = PathBuf::from(".");
    let mut seen = false;
    let mut words = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == "--directory" {
            if seen {
                return Err("--directory supplied twice");
            }
            seen = true;
            i += 1;
            let path = raw
                .get(i)
                .filter(|s| !s.starts_with('-'))
                .ok_or("--directory requires DIR")?;
            directory = PathBuf::from(path);
        } else {
            words.push(raw[i].as_str());
        }
        i += 1;
    }
    Ok((directory, words))
}
pub fn help(raw: &[String]) -> Result<String, &'static str> {
    let (_, words) = arguments(raw)?;
    match words.first().copied().unwrap_or("") {
        "" | "enable" | "submit" | "status" | "read" | "resources" => Ok(HELP.into()),
        _ => Err("unknown workflow command; use ledger workflow --help"),
    }
}
pub(super) enum CommandOutput {
    Legacy(Value),
    Workflow(ledgerlab::WorkflowReply),
}
fn legacy((value, code): (Value, u8)) -> (CommandOutput, u8) {
    (CommandOutput::Legacy(value), code)
}
pub async fn run(args: &Args) -> Result<(CommandOutput, u8), LocalError> {
    if args.config != Path::new("ledger.json") {
        return Ok(legacy(error(
            "USAGE",
            "workflow uses --directory DIR, not --config",
            2,
        )));
    }
    let (directory, words) = match arguments(&args.rest) {
        Ok(parsed) => parsed,
        Err(message) => return Ok(legacy(error("USAGE", message, 2))),
    };
    if words.first() == Some(&"resources") {
        let update = match words.as_slice() {
            ["resources", "show"] => None,
            ["resources", "set", file] => {
                Some(local::read_file(Path::new(file), local::CONFIG_LIMIT)?)
            }
            _ => {
                return Ok(legacy(error(
                    "USAGE",
                    "use workflow resources show|set FILE",
                    2,
                )))
            }
        };
        let ledger = LocalWorkflowLedger::open_local(&directory).await?;
        let result = match update {
            Some(raw) => ledger.set_resource_settings(&raw).await,
            None => ledger.resource_settings().await,
        };
        ledger.close().await;
        return result
            .map(|v| (CommandOutput::Legacy(v), 0))
            .map_err(LocalError::from);
    }
    let (command, input) = match words.as_slice() {
        ["enable"] => ("enable", None),
        [command @ ("submit" | "status" | "read"), path]
            if *path == "-" || !path.starts_with('-') =>
        {
            (*command, Some(*path))
        }
        _ => {
            return Ok(legacy(error(
                "USAGE",
                "use workflow enable, or workflow submit|status|read FILE|-",
                2,
            )));
        }
    };
    // Read stdin/file completely before acquiring the existing owner/writer.
    let raw = input
        .map(|path| {
            if path == "-" {
                local::read_bounded(std::io::stdin().lock(), local::EVENT_LIMIT)
            } else {
                local::read_file(Path::new(path), local::EVENT_LIMIT)
            }
        })
        .transpose()?;
    let ledger = LocalWorkflowLedger::open_local(&directory).await?;
    let result: Result<CommandOutput, ServiceError> = match (command, raw.as_deref()) {
        ("enable", None) => ledger.enable_profile_b().await.map(|()| CommandOutput::Legacy(json!({"schema":"ledger-cli/1","status":"workflow_enabled","profile":"workflow-dual-legacy-capacity/1"}))),
        ("status", Some(raw)) => ledger.status(raw).await.map(CommandOutput::Legacy),
        ("read", Some(raw)) => ledger.read(raw).await.map(CommandOutput::Legacy),
        ("submit", Some(raw)) => submit(&ledger, raw).await,
        _ => Err(ServiceError::IntegrityFailure),
    };
    ledger.close().await;
    result.map(|result| (result, 0)).map_err(LocalError::from)
}
async fn submit(ledger: &LocalWorkflowLedger, raw: &[u8]) -> Result<CommandOutput, ServiceError> {
    // This header is only routing. Each actual host entry revalidates its
    // closed request and global original identity before fresh authorization.
    let header: Value =
        serde_json::from_slice(raw).map_err(|_| ServiceError::Rejection("INVALID_SHAPE".into()))?;
    let payload_schema = header
        .get("payload")
        .and_then(|p| p.get("schema"))
        .and_then(Value::as_str);
    let kind = match header.get("kind").and_then(Value::as_str) {
        Some("policy.publish")
            if payload_schema == Some("ledger-workflow-policy-publication/6") =>
        {
            "local-publication"
        }
        Some("policy.publish") => "policy.publish",
        Some("evidence.retain")
            if matches!(
                payload_schema,
                Some(
                    "ledger-workflow-fact-attest/1"
                        | "ledger-workflow-family-close-attest/1"
                        | "ledger-workflow-supplier-cost-attest/1"
                        | "ledger-workflow-app-observe/1"
                        | "ledger-workflow-app-cost-attest/1"
                        | "ledger-workflow-party-evidence-retain/1"
                        | "ledger-workflow-party-evidence-retain/2"
                )
            ) =>
        {
            "financial"
        }
        Some("authority.change")
            if matches!(
                payload_schema,
                Some(
                    "ledger-cost-authority-enroll/1"
                        | "ledger-workflow-party-authorize/1"
                        | "ledger-workflow-party-revoke/1"
                )
            ) =>
        {
            "financial"
        }
        Some("evidence.retain" | "authority.change") => "prerequisite",
        Some("agreement.accept")
            if matches!(
                payload_schema,
                Some("ledger-workflow-agreement-accept/8" | "ledger-workflow-agreement-accept/9")
            ) =>
        {
            "financial"
        }
        Some("agreement.accept" | "agreement.activate" | "agreement.revoke") => "agreement",
        Some(
            "work.accept"
            | "work.reverse"
            | "quantity.correct"
            | "outcome.submit"
            | "outcome.correct"
            | "outcome.close"
            | "link.assert"
            | "invocation.authorize"
            | "invocation.close"
            | "cost.finalize"
            | "cost.correct",
        ) => "financial",
        _ => return Err(ServiceError::Rejection("INVALID_SHAPE".into())),
    };
    drop(header);

    match kind {
        "policy.publish" => ledger.publish(raw).await.map(CommandOutput::Legacy),
        "local-publication" => ledger
            .publish_local_owned(raw)
            .await
            .map(CommandOutput::Workflow),
        "prerequisite" => ledger.prerequisite(raw).await.map(CommandOutput::Legacy),
        "agreement" => match ledger.local_activation_owned(raw).await? {
            Some(reply) => Ok(CommandOutput::Workflow(reply)),
            None => ledger.agreement(raw).await.map(CommandOutput::Legacy),
        },
        "financial" => ledger
            .financial_owned_reply(raw)
            .await
            .map(CommandOutput::Workflow),
        _ => Err(ServiceError::IntegrityFailure),
    }
}
