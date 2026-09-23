use super::*;
use ledgerlab::billing::BillingLedger;
pub const HELP:&str="\nOrdinary local billing (separate installation):\n  billing init DIR --setup FILE\n  billing [--directory DIR] accept FILE|-\n  billing [--directory DIR] explain TARGET_ID\n  billing [--directory DIR] statement --customer CUSTOMER\nBilling output is JSON (pretty-printed unless --json). No payment or tax invoice.\n";
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
    if let ["init", dir, "--setup", file] = args.as_slice() {
        let raw = local::read_file(Path::new(file), local::CONFIG_LIMIT)?;
        BillingLedger::init(Path::new(dir), &raw).await?;
        return Ok((
            json!({"status":"initialized","directory":dir,"profile":"local-retail","dispatch":"disabled"}),
            0,
        ));
    }
    let input = if let ["accept", file] = args.as_slice() {
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
        ["accept", _] | ["explain", _] | ["statement", "--customer", _]
    ) {
        return Ok(error("USAGE", HELP, 2));
    }
    let ledger = BillingLedger::open(&directory).await?;
    let result = match args.as_slice() {
        ["accept", _] => ledger.accept(input.as_ref().unwrap()).await,
        ["explain", id] => ledger.explain(id).await,
        ["statement", "--customer", customer] => ledger.statement(customer, None).await,
        _ => unreachable!(),
    };
    ledger.close().await;
    result.map(|v| (v, 0))
}
