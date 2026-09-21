//! Reservation accounting projects an already validated economic result. It
//! never evaluates prices, decides authority, or replenishes post-hoc capacity.
use super::outcome_base::*;
use ledgerlab_core::{
    canonical::{self, outcome as codec},
    money::parse_atoms,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub(super) fn settlement_hash(domain: &str, v: &Value) -> Result<String> {
    core(canonical::outcome_digest(codec::SETTLEMENT, domain, v))
}
fn number(v: &Value) -> Result<i128> {
    let n = core(parse_atoms(text(v)?))?;
    check(n >= 0)?;
    Ok(n)
}
fn make(kind: &str, scope: &Value, body: Value) -> Result<Value> {
    core(codec::envelope(codec::SETTLEMENT, kind, scope, body))
}
pub(super) fn checkpoint(
    records: &Records,
    base: &Base,
    invocation: &str,
) -> Result<(Value, Value, Value)> {
    let i = base
        .evaluation
        .invocations()
        .iter()
        .find(|i| i.id == invocation)
        .ok_or_else(integrity)?;
    check(
        base.evaluation.event().dto().invocation_id.as_deref() == Some(invocation)
            && i.held == i.maximum_exposure,
    )?;
    let binding = records
        .rows
        .iter()
        .find(|r| r["kind"] == "binding-snapshot" && r["body"]["binding_id"] == i.binding_id)
        .ok_or_else(integrity)?;
    let policies = records
        .rows
        .iter()
        .filter(|r| r["kind"] == "policy-snapshot" && r["body"]["binding_id"] == i.binding_id)
        .collect::<Vec<_>>();
    check(!policies.is_empty() && policies.len() <= 32)?;
    let max = i.maximum_exposure.atoms();
    let mut consume = 0;
    let mut release = 0;
    let mut entries = 0;
    for c in base
        .evaluation
        .consumptions()
        .iter()
        .filter(|c| c.invocation_id == invocation)
    {
        consume = core(ledgerlab_core::money::add_atoms(consume, c.consume.atoms()))?;
        release = core(ledgerlab_core::money::add_atoms(release, c.release.atoms()))?;
        entries += 1;
    }
    check(entries == 1 && consume >= 0 && release >= 0 && consume + release <= max)?;
    let held = max - consume - release;
    check(
        policies
            .iter()
            .all(|p| number(&p["body"]["max_premium_atoms"]).is_ok_and(|n| n <= held)),
    )?;
    let auth = records.get(&policies[0]["body"]["supplier_authorization"], "evidence")?;
    for p in &policies {
        check(p["body"]["supplier_authorization"] == auth["id"])?;
    }
    let families=policies.iter().map(|p|json!({"key":{"agreement_id":p["body"]["agreement_id"],"family_id":p["body"]["family_id"],"target":base.snapshot["body"]["target"]},"accepted_by":p["body"]["ordinary"]["accepted_by"],"status":"open"})).collect();
    let state = json!({"revision":"0","maximum":max.to_string(),"consumed":consume.to_string(),"held":held.to_string(),"released":release.to_string(),"families":ordered(families)?});
    let anchor = json!({"base_acceptance":reference(&base.acceptance),"binding_snapshot":reference(binding),"target_snapshot":reference(&base.snapshot),"invocation_authorization":reference(auth)});
    let unit = json!({"currency":i.maximum_exposure.currency(),"scale":i.maximum_exposure.scale()});
    Ok((state, anchor, unit))
}
/// A current authority proof must already have been verified by the coordinator.
pub(super) struct SettlementInput<'a> {
    pub command: Value,
    pub authority: Value,
    pub received: Value,
    pub accepted: Value,
    pub economic: Option<&'a Value>,
    pub economic_prefix: &'a [Value],
    pub prior: &'a [Value],
    pub amount: Option<i128>,
    pub authorized_close: bool,
}
pub(super) fn project(
    records: &Records,
    base: &Base,
    input: SettlementInput<'_>,
) -> Result<Vec<Value>> {
    let SettlementInput {
        command,
        authority,
        received,
        accepted,
        economic,
        economic_prefix,
        prior,
        amount,
        authorized_close,
    } = input;
    let kind = text(&command["kind"])?;
    let invocation = text(&command["invocation_id"])?;
    let (initial, anchor, unit) = checkpoint(records, base, invocation)?;
    let previous = prior
        .iter()
        .rev()
        .find(|r| r["kind"] == "reservation-receipt");
    let registration = prior.iter().find(|r| r["kind"] == "reservation-receipt");
    check((kind == "register") == previous.is_none())?;
    let before = previous
        .map(|r| r["body"]["result"].clone())
        .unwrap_or(initial);
    check(time(&received)?.micros() <= time(&accepted)?.micros())?;
    if let Some(last) = prior
        .iter()
        .rev()
        .find(|r| r["kind"] == "reservation-observation")
    {
        check(time(&last["body"]["accepted_at"])?.micros() <= time(&accepted)?.micros())?;
    }
    check(authority["active"] == true && number(&authority["grant_revision"])? > 0)?;
    let perms = array(&authority["permissions"])?;
    let permission = match kind {
        "register" | "ordinary" => "submit",
        "post_hoc" => "correct",
        "close" => "close",
        _ => return Err(integrity()),
    };
    check(perms.contains(&json!("read")) && perms.contains(&json!(permission)))?;
    let mut result = before.clone();
    let mut consume = 0;
    let mut release = 0;
    match kind {
        "ordinary" | "post_hoc" => {
            let family = result["families"]
                .as_array_mut()
                .ok_or_else(integrity)?
                .iter_mut()
                .find(|f| f["key"] == command["family"])
                .ok_or_else(integrity)?;
            if kind == "ordinary" {
                check(
                    family["status"] == "open"
                        && time(&accepted)?.micros() <= time(&family["accepted_by"])?.micros(),
                )?;
                consume = amount.ok_or_else(integrity)?.max(0);
                check(consume <= number(&before["held"])?)?;
                family["status"] = json!("claimed");
                family["ordinary_receipt"] = reference(economic.ok_or_else(integrity)?);
            } else {
                check(family["status"] == "claimed" && family.get("ordinary_receipt").is_some())?;
            }
        }
        "close" => {
            check(command["expected_revision"] == before["revision"])?;
            if command["reason"] == "deadline" {
                let latest = array(&before["families"])?
                    .iter()
                    .map(|f| time(&f["accepted_by"]).map(|t| t.micros()))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .max()
                    .ok_or_else(integrity)?;
                check(time(&accepted)?.micros() > latest)?;
            } else {
                check(command["reason"] == "authorized" && authorized_close)?;
            }
            for f in result["families"].as_array_mut().ok_or_else(integrity)? {
                if f["status"] == "open" {
                    f["status"] = json!("closed")
                }
            }
            release = number(&before["held"])?;
        }
        "register" => {}
        _ => return Err(integrity()),
    }
    result["consumed"] = json!(core(ledgerlab_core::money::add_atoms(
        number(&before["consumed"])?,
        consume
    ))?
    .to_string());
    result["released"] = json!(core(ledgerlab_core::money::add_atoms(
        number(&before["released"])?,
        release
    ))?
    .to_string());
    result["held"] = json!((number(&before["held"])? - consume - release).to_string());
    result["families"] = json!(ordered(array(&result["families"])?.clone())?);
    let changed = result != before;
    if changed {
        let n = core(ledgerlab_core::domain::Revision::parse(text(
            &before["revision"],
        )?))?;
        let n = text(&serde_json::to_value(n).map_err(|_| integrity())?)?
            .parse::<u64>()
            .map_err(|_| integrity())?;
        check(n < i64::MAX as u64)?;
        result["revision"] = json!((n + 1).to_string());
    }
    check(
        (kind != "post_hoc" || result == before)
            && number(&result["consumed"])?
                + number(&result["held"])?
                + number(&result["released"])?
                == number(&result["maximum"])?,
    )?;
    let mut observation = json!({"command":command,"request_hash":settlement_hash("request",&command)?,"anchor":anchor,"unit":unit,"before":before,"authority":authority,"received_at":received,"accepted_at":accepted});
    if let Some(e) = economic {
        observation["economic_receipt"] = reference(e);
    }
    check((kind == "close") == economic.is_none())?;
    if let Some(reg) = registration {
        observation["registration"] = reference(reg);
    }
    if let Some(prev) = previous {
        observation["previous"] = reference(prev);
    }
    let observation = make("reservation-observation", &records.scope, observation)?;
    let mut rows = vec![observation.clone()];
    if changed {
        rows.push(make("reservation-transition",&records.scope,json!({"invocation_id":invocation,"observation":reference(&observation),"before":before,"after":result,"consume":consume.to_string(),"release":release.to_string()}))?);
    }
    let mut refs = BTreeMap::new();
    let mut put = |r: Value| -> Result<()> {
        let key = bytes(&json!([r["kind"], r["id"]]))?;
        if let Some(old) = refs.insert(key, r.clone()) {
            check(old == r)?;
        }
        Ok(())
    };
    // Full retained prefix, not just the immediate external economic receipt.
    for r in economic_prefix.iter().chain(prior).chain(rows.iter()) {
        put(reference(r))?;
    }
    for r in anchor.as_object().ok_or_else(integrity)?.values() {
        put(r.clone())?;
    }
    put(authority["grant"].clone())?;
    for r in array(&authority["evidence"])? {
        put(r.clone())?;
    }
    if let Some(e) = economic {
        put(reference(e))?;
    }
    let mut receipt = json!({"source":command["source"],"external_id":command["external_id"],"invocation_id":invocation,"request_hash":observation["body"]["request_hash"],"observation":reference(&observation),"result":result,"replay":ordered(refs.into_values().collect())?});
    if changed {
        receipt["transition"] = reference(&rows[1]);
    }
    if let Some(e) = economic {
        receipt["economic_receipt"] = reference(e);
    }
    rows.push(make("reservation-receipt", &records.scope, receipt)?);
    for r in &rows {
        core(codec::decode(&bytes(r)?))?;
    }
    Ok(rows)
}
