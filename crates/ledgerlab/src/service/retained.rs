//! Exact retained-history verification shared by acceptance and private comparison.
//! Inputs contain observed values, never transactions or acceptance commands.
#![allow(dead_code)]
pub(super) mod base;
pub(super) mod economic;
pub(super) mod settlement;
use crate::store::outcomes::{
    ObservedOutcomeHead, OutcomeLock, OutcomeLockClass, OutcomeLockMode, OutcomeSnapshot,
    ScopedRecordRef, StoredCompositeDelivery,
};
use base::{
    self as b, array, bytes, check, core, hash, integrity, reference, text, Base, Records, Result,
};
use ledgerlab_core::{
    canonical::{self, outcome as codec},
    policy::chaining::outcomes as o,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct HistorySelection<'a> {
    pub scope: [String; 2],
    pub target: &'a str,
    pub invocation_id: &'a str,
    pub required: &'a [ScopedRecordRef],
}
fn lock(
    c: &HistorySelection<'_>,
    class: OutcomeLockClass,
    parts: Vec<Value>,
    mode: OutcomeLockMode,
) -> Result<OutcomeLock> {
    let mut key = vec![json!(c.scope)];
    key.extend(parts);
    Ok(OutcomeLock {
        class,
        key: bytes(&json!(key))?,
        mode,
    })
}
fn head<'a>(s: &'a OutcomeSnapshot, l: &OutcomeLock) -> Result<&'a ObservedOutcomeHead> {
    s.heads
        .iter()
        .find(|h| h.lock.class == l.class && h.lock.key == l.key)
        .ok_or_else(integrity)
}
pub(super) fn head_value(h: &ObservedOutcomeHead) -> Result<Option<Value>> {
    check(h.revision.is_some() == h.value.is_some())?;
    if let Some(rev) = &h.revision {
        core(ledgerlab_core::domain::Revision::parse(rev))?;
    }
    h.value
        .as_ref()
        .map(|v| {
            let p = core(canonical::parse_bounded(v, canonical::BUNDLE_LIMIT))?;
            check(bytes(&p)? == *v)?;
            Ok(p)
        })
        .transpose()
}
fn current(
    s: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    class: OutcomeLockClass,
    parts: Vec<Value>,
) -> Result<Option<Value>> {
    head_value(head(s, &lock(c, class, parts, OutcomeLockMode::Read)?)?)
}
pub(super) fn ref_value(r: &ScopedRecordRef) -> Result<Value> {
    Ok(json!({"kind":r.kind,"id":core(canonical::parse(&r.id))?,"content_hash":r.content_hash}))
}
pub(super) fn validate_delivery(d: &StoredCompositeDelivery, records: &Records) -> Result<()> {
    let settle = core(codec::decode(&d.settlement_receipt))?;
    check(
        settle["kind"] == "reservation-receipt"
            && settle["scope"] == json!(d.key.scope)
            && d.canonical_key.scope == d.key.scope
            && settle["body"]["source"] == d.canonical_key.source
            && settle["body"]["external_id"] == d.canonical_key.external_id,
    )?;
    records.deref(&reference(&settle))?;
    let command = core(canonical::parse(&d.command))?;
    let ingress = core(canonical::parse(&d.ingress))?;
    check(bytes(&command)? == d.command && bytes(&ingress)? == d.ingress)?;
    let observation = records.deref(&settle["body"]["observation"])?;
    let original = &observation["body"]["command"];
    check(
        command["source"] == d.key.source
            && command["external_id"] == d.key.external_id
            && command["invocation_id"] == original["invocation_id"]
            && command["kind"] == original["kind"],
    )?;
    if d.key == d.canonical_key {
        check(command == *original)?;
    } else {
        // Semantic aliases exist only for ordinary claims. Their wrapper may
        // differ, but they retain the original source, invocation and family.
        check(
            command["kind"] == "ordinary"
                && d.key.source == d.canonical_key.source
                && command["family"] == original["family"],
        )?;
    }
    match text(&command["kind"])? {
        "close" => check(
            ingress == command
                && d.ingress_hash == settlement::settlement_hash("request", &command)?,
        )?,
        "register" => {
            let base = records.one("base-evaluation")?;
            check(
                d.ingress == text(&base["body"]["original_ingress_utf8"])?.as_bytes()
                    && command["economic_ingress_hash"] == base["body"]["original_ingress_hash"]
                    && json!(d.ingress_hash) == command["economic_ingress_hash"],
            )?;
        }
        "ordinary" | "post_hoc" => {
            check(
                hash("ingress", &ingress)? == d.ingress_hash
                    && json!(d.ingress_hash) == command["economic_ingress_hash"]
                    && ingress["data"]["source"] == d.key.source
                    && ingress["data"]["external_id"] == d.key.external_id,
            )?;
        }
        _ => return Err(integrity()),
    }
    match &d.economic_receipt {
        Some(raw) => {
            let e = core(codec::decode(raw))?;
            check(settle["body"]["economic_receipt"] == reference(&e))?;
            records.deref(&reference(&e))?;
            if d.key == d.canonical_key && e["kind"] == "receipt" {
                check(records.get(&e["body"]["event_id"], "event")?["body"] == ingress)?;
            }
        }
        None => check(settle["body"].get("economic_receipt").is_none())?,
    }
    Ok(())
}

pub(super) fn economic_only(records: &Records) -> Result<Records> {
    Records::new(
        &records
            .rows
            .iter()
            .filter(|r| !text(&r["kind"]).unwrap_or("").starts_with("reservation-"))
            .map(bytes)
            .collect::<Result<Vec<_>>>()?,
        records.scope.clone(),
    )
}
pub(super) fn decision_groups(records: &Records, base: &Base) -> Result<Vec<Vec<Value>>> {
    let mut prior: BTreeSet<Vec<u8>> = array(&base.acceptance["body"]["members"])?
        .iter()
        .map(|r| bytes(&r["id"]))
        .collect::<Result<_>>()?;
    prior.insert(bytes(&base.acceptance["id"])?);
    let mut receipts = records
        .rows
        .iter()
        .filter(|r| r["kind"] == "receipt")
        .collect::<Vec<_>>();
    receipts.sort_by_key(|r| {
        r["body"]["revision"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    });
    let mut result = vec![];
    for receipt in receipts {
        let manifest = records.get(&receipt["body"]["decision_id"], "decision-manifest")?;
        let mut rows = vec![];
        for r in array(&manifest["body"]["members"])? {
            if !prior.contains(&bytes(&r["id"])?) {
                rows.push(records.deref(r)?.clone());
            }
        }
        rows.extend([manifest.clone(), receipt.clone()]);
        rows.sort_by(|a, b| {
            (text(&a["kind"]).unwrap(), bytes(&a["id"]).unwrap())
                .cmp(&(text(&b["kind"]).unwrap(), bytes(&b["id"]).unwrap()))
        });
        for r in &rows {
            check(prior.insert(bytes(&r["id"])?))?;
        }
        result.push(rows);
    }
    check(prior.len() == records.rows.len())?;
    Ok(result)
}
pub(super) fn historical_records(
    snapshot: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    all: &Records,
) -> Result<Records> {
    #[cfg(test)]
    let _trace = crate::store::postgres::trace::Span::new("historical_records");
    let t = current(snapshot, c, OutcomeLockClass::Target, vec![json!(c.target)])?
        .ok_or_else(integrity)?;
    let refs = array(&t["records"])?;
    check(b::ordered(refs.clone())? == *refs)?;
    let raw = refs
        .iter()
        .map(|r| all.deref(r).and_then(bytes))
        .collect::<Result<Vec<_>>>()?;
    let required = c
        .required
        .iter()
        .map(|r| {
            check(r.scope == c.scope)?;
            ref_value(r)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut expected = refs.clone();
    for r in required {
        if !expected.contains(&r) {
            expected.push(r);
        }
    }
    check(expected.len() == all.rows.len())?;
    for r in &expected {
        all.deref(r)?;
    }
    let roots = vec![t["base"].clone(), t["registration"].clone()];
    check(snapshot.anchors.len() == 2 && snapshot.anchors.iter().all(|r| r.scope == c.scope))?;
    check(
        b::ordered(
            snapshot
                .anchors
                .iter()
                .map(ref_value)
                .collect::<Result<_>>()?,
        )? == b::ordered(roots)?,
    )?;
    Records::new(&raw, all.scope.clone())
}
// A replay result belongs only to this locked snapshot. Reuse it for identity,
// semantic lookup and fresh planning; never carry it across rollback/re-resolution.
pub(super) struct Registered {
    pub(super) records: Records,
    pub(super) base: Base,
    pub(super) decisions: Vec<o::Decision>,
    pub(super) settlement: Vec<Value>,
}
pub(super) fn registered_history(
    snapshot: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    all: &Records,
) -> Result<Registered> {
    let records = historical_records(snapshot, c, all)?;
    let economic = economic_only(&records)?;
    let target = current(snapshot, c, OutcomeLockClass::Target, vec![json!(c.target)])?
        .ok_or_else(integrity)?;
    let base = b::decode_base(&economic, &target["base"])?;
    validate_registered(snapshot, c, records, economic, base)
}
pub(super) fn validate_registered(
    snapshot: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    records: Records,
    economic: Records,
    base: Base,
) -> Result<Registered> {
    validate_registered_checked(snapshot, c, records, economic, base, &mut || Ok(()))
}
pub(super) fn validate_registered_checked(
    snapshot: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    records: Records,
    economic: Records,
    base: Base,
    checkpoint: &mut impl FnMut() -> Result<()>,
) -> Result<Registered> {
    #[cfg(test)]
    let _trace = crate::store::postgres::trace::Span::new("validate_registered");
    let target = current(snapshot, c, OutcomeLockClass::Target, vec![json!(c.target)])?
        .ok_or_else(integrity)?;
    check(base.snapshot["body"]["target"] == c.target)?;
    let groups = decision_groups(&economic, &base)?;
    let decisions = economic::replay_checked(&economic, &base, &groups, checkpoint)?;
    let mut prefix = vec![];
    let mut econ_prefix = array(&base.acceptance["body"]["members"])?
        .iter()
        .map(|r| economic.deref(r).cloned())
        .collect::<Result<Vec<_>>>()?;
    econ_prefix.push(base.acceptance.clone());
    let mut used = BTreeSet::new();
    let mut economic_used = BTreeSet::new();
    loop {
        checkpoint()?;
        let next = records
            .rows
            .iter()
            .filter(|r| r["kind"] == "reservation-observation")
            .filter(|r| {
                if prefix.is_empty() {
                    r["body"].get("previous").is_none()
                } else {
                    r["body"]["previous"] == reference(prefix.last().unwrap())
                }
            })
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        check(next.len() == 1)?;
        let obs = next[0];
        check(used.insert(bytes(&obs["id"])?))?;
        let ob = &obs["body"];
        let command = &ob["command"];
        let receipt = records
            .rows
            .iter()
            .find(|r| {
                r["kind"] == "reservation-receipt" && r["body"]["observation"] == reference(obs)
            })
            .ok_or_else(integrity)?;
        let econ = ob
            .get("economic_receipt")
            .map(|r| economic.deref(r))
            .transpose()?;
        let mut amount = None;
        if let Some(e) = econ {
            check(economic_used.insert(bytes(&e["id"])?))?;
            if e["kind"] == "receipt" {
                let group = groups
                    .iter()
                    .find(|g| g.iter().any(|r| r["id"] == e["id"]))
                    .ok_or_else(integrity)?;
                let au = &group
                    .iter()
                    .find(|r| r["kind"] == "authority-decision")
                    .ok_or_else(integrity)?["body"];
                for key in ["principal", "grant_revision", "active"] {
                    check(ob["authority"][key] == au[key])?
                }
                check(
                    ob["authority"]["grant"] == reference(economic.get(&au["grant"], "evidence")?)
                        && ob["received_at"] == au["received_at"]
                        && ob["accepted_at"] == au["accepted_at"],
                )?;
                let event = group
                    .iter()
                    .find(|r| r["kind"] == "event")
                    .ok_or_else(integrity)?;
                let data = &event["body"]["data"];
                check(
                    command["economic_ingress_hash"] == hash("ingress", &event["body"])?
                        && command["source"] == data["source"]
                        && command["external_id"] == data["external_id"]
                        && command["family"]
                            == json!({"agreement_id":data["agreement_id"],"family_id":data["family_id"],"target":data["target"]}),
                )?;
                check((command["kind"] == "post_hoc") == (data["type"] == "correction"))?;
                let rev = group
                    .iter()
                    .find(|r| r["kind"] == "claim-revision")
                    .ok_or_else(integrity)?;
                amount = Some(b::money(&rev["body"]["amount"])?.atoms());
                econ_prefix.extend(group.clone());
            } else {
                check(
                    *e == base.acceptance
                        && command["kind"] == "register"
                        && command["economic_ingress_hash"]
                            == base.evaluation.event().candidate().ingress_hash()
                        && command["source"] == base.evaluation.event().source()
                        && command["external_id"]
                            == base.evaluation.event().candidate().external_id(),
                )?;
            }
        }
        let projected = settlement::project(
            &economic,
            &base,
            settlement::SettlementInput {
                command: command.clone(),
                authority: ob["authority"].clone(),
                received: ob["received_at"].clone(),
                accepted: ob["accepted_at"].clone(),
                economic: econ,
                economic_prefix: &econ_prefix,
                prior: &prefix,
                amount,
                authorized_close: command["reason"] == "authorized",
            },
        )?;
        for r in &projected {
            check(*records.deref(&reference(r))? == *r)?;
        }
        check(projected.last() == Some(receipt))?;
        prefix.extend(projected);
    }
    check(
        prefix.len()
            == records
                .rows
                .iter()
                .filter(|r| text(&r["kind"]).unwrap().starts_with("reservation-"))
                .count()
            && economic_used.len() == groups.len() + 1,
    )?;
    check(!prefix.is_empty() && target["registration"] == reference(&prefix[1]))?;
    let steps = prefix
        .iter()
        .filter(|r| r["kind"] == "reservation-receipt")
        .count();
    check(
        head(
            snapshot,
            &lock(
                c,
                OutcomeLockClass::Target,
                vec![json!(c.target)],
                OutcomeLockMode::Read,
            )?,
        )?
        .revision
        .as_deref()
            == Some((steps - 1).to_string().as_str()),
    )?;
    let last = prefix.last().ok_or_else(integrity)?;
    let reservation = head(
        snapshot,
        &lock(
            c,
            OutcomeLockClass::Reservation,
            vec![json!(c.invocation_id)],
            OutcomeLockMode::Read,
        )?,
    )?;
    check(
        head_value(reservation)? == Some(last["body"]["result"].clone())
            && reservation.revision.as_deref() == last["body"]["result"]["revision"].as_str(),
    )?;
    validate_economic_heads(snapshot, c, &economic, &base)?;
    Ok(Registered {
        records: economic,
        base,
        decisions,
        settlement: prefix,
    })
}
fn validate_economic_heads(
    s: &OutcomeSnapshot,
    c: &HistorySelection<'_>,
    r: &Records,
    base: &Base,
) -> Result<()> {
    use OutcomeLockClass as L;
    check(
        current(s, c, L::BaseReversal, vec![json!(c.target)])? == Some(json!({"reversed":false})),
    )?;
    check(
        current(s, c, L::InvocationConsumption, vec![json!(c.invocation_id)])?
            == Some(
                json!({"target":c.target,"registration":current(s,c,L::Target,vec![json!(c.target)])?.ok_or_else(integrity)?["registration"]}),
            ),
    )?;
    for binding in r.rows.iter().filter(|r| r["kind"] == "binding-snapshot") {
        // Current selectors cannot reprice or hide a retained receipt. Their
        // exact locked bytes remain observed guards; the host verifier checks
        // current write rights against the original frozen binding.
        let mut latest: BTreeMap<String, &Value> = BTreeMap::new();
        for rv in r.rows.iter().filter(|rv| {
            rv["kind"] == "claim-revision"
                && rv["body"]["binding_id"] == binding["body"]["binding_id"]
        }) {
            let id = text(&rv["body"]["claim_id"])?;
            if latest.get(id).is_none_or(|old| {
                text(&old["body"]["number"])
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    < text(&rv["body"]["number"]).unwrap().parse::<u64>().unwrap()
            }) {
                latest.insert(id.into(), rv);
            }
        }
        let expected =
            json!({"revisions":b::ordered(latest.values().map(|r|reference(r)).collect())?});
        check(
            current(
                s,
                c,
                L::BindingAggregate,
                vec![json!(c.target), binding["body"]["binding_id"].clone()],
            )? == Some(expected),
        )?;
    }
    for f in &base.target.policy().families {
        let binding = base
            .evaluation
            .bundle()
            .policies()
            .iter()
            .find(|p| p.binding.id == f.binding_id)
            .ok_or_else(integrity)?;
        let rv = r
            .rows
            .iter()
            .filter(|r| r["kind"] == "claim-revision")
            .filter(|rv| {
                r.get(&rv["body"]["claim_id"], "claim").is_ok_and(|cl| {
                    cl["body"]["family_id"] == f.family
                        && cl["body"]["agreement_id"] == binding.binding.agreement
                })
            })
            .max_by_key(|r| text(&r["body"]["number"]).unwrap().parse::<u64>().unwrap());
        let expected=rv.map(|rv|->Result<Value>{Ok(json!({"revision":reference(rv),"original_receipt":reference(r.get(&rv["body"]["original_receipt"],"receipt")?)}))}).transpose()?;
        check(
            current(
                s,
                c,
                L::Claim,
                vec![
                    json!(binding.binding.agreement),
                    json!(f.family),
                    json!(c.target),
                ],
            )? == expected,
        )?;
    }
    Ok(())
}
