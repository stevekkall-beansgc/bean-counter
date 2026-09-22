//! Deterministic v2 projection of the existing pure outcome Decision.
use super::base::*;
use ledgerlab_core::{canonical::outcome as codec, policy::chaining::outcomes as o};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn add(rows: &mut Vec<Value>, scope: &Value, kind: &str, b: Value) -> Result<Value> {
    let r = row(kind, scope, b)?;
    rows.push(r.clone());
    Ok(r)
}
fn id(kind: &str, s: &Value, b: Value) -> Result<Value> {
    core(codec::key(codec::ECONOMIC, kind, s, &b))
}
fn dependency(
    rows: &mut Vec<Value>,
    s: &Value,
    dependent: &Value,
    input: &Value,
    reason: &str,
) -> Result<()> {
    add(
        rows,
        s,
        "dependency",
        json!({"dependent":dependent,"input":reference(input),"reason":reason}),
    )?;
    Ok(())
}
fn amount(base: &Base, n: i128) -> Value {
    json!({"currency":base.basis["body"]["amount"]["currency"],"scale":base.basis["body"]["amount"]["scale"],"atoms":n.to_string()})
}
pub(in crate::service) fn project(
    records: &Records,
    base: &Base,
    event_body: Value,
    authority: Value,
    admission: Value,
    decision: &o::Decision,
    evidence: Vec<Value>,
) -> Result<Vec<Value>> {
    let s = &records.scope;
    let mut rows = evidence;
    let event = add(&mut rows, s, "event", event_body)?;
    let data = &event["body"]["data"];
    let e = &event["id"];
    let auth = add(&mut rows, s, "authority-decision", authority)?;
    let au = &auth["body"];
    let adm = add(&mut rows, s, "admission", admission)?;
    let ab = &adm["body"];
    let policy = records
        .rows
        .iter()
        .find(|r| {
            r["kind"] == "policy-snapshot"
                && r["body"]["binding_id"] == decision.binding().id
                && r["body"]["family_id"] == decision.key().family
        })
        .ok_or_else(integrity)?;
    let p = &policy["body"];
    let binding = records
        .rows
        .iter()
        .find(|r| {
            r["kind"] == "binding-snapshot" && r["body"]["binding_id"] == decision.binding().id
        })
        .ok_or_else(integrity)?;
    let obligation = records
        .rows
        .iter()
        .find(|r| {
            r["kind"] == "obligation"
                && r["body"]["agreement_id"] == p["agreement_id"]
                && r["body"]["book"] == p["book"]
        })
        .ok_or_else(integrity)?;
    let target = &base.snapshot["body"]["target"];
    let basis = &base.basis;
    let clid = id(
        "claim",
        s,
        json!({"agreement_id":p["agreement_id"],"family_id":p["family_id"],"target":target}),
    )?;
    let rid = id(
        "claim-revision",
        s,
        json!({"claim_id":clid,"number":decision.revision().to_string()}),
    )?;
    let receipt_id = id("receipt", s, json!({"event_id":e}))?;
    let decision_id = id("decision-manifest", s, json!({"event_id":e}))?;
    let prior = records
        .rows
        .iter()
        .filter(|r| r["kind"] == "claim-revision" && r["body"]["claim_id"] == clid)
        .max_by_key(|r| {
            r["body"]["number"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        });
    check((data["type"] == "correction") == prior.is_some())?;
    if let Some(prior) = prior {
        check(
            data["expected_revision"] == prior["id"]
                && data["expected_revision_number"] == prior["body"]["number"]
                && data["claim_id"] == clid,
        )?;
    }
    for (k, expected) in [
        ("event_id", e),
        ("target", target),
        ("agreement_id", &data["agreement_id"]),
        ("family_id", &data["family_id"]),
        ("source", &data["source"]),
    ] {
        check(au[k] == *expected)?;
    }
    for (k, expected) in [
        ("event_id", e),
        ("principal", &au["principal"]),
        ("grant", &au["grant"]),
        ("grant_revision", &au["grant_revision"]),
        ("authorized_source", &data["source"]),
        ("agreement_id", &p["agreement_id"]),
        ("payer", &p["roles"]["payer"]),
        ("book", &p["book"]),
        ("family_id", &p["family_id"]),
        ("permission", &data["type"]),
        ("target_snapshot", &base.snapshot["id"]),
        ("authority_decision", &auth["id"]),
        ("binding_id", &p["binding_id"]),
        ("received_at", &au["received_at"]),
        ("accepted_at", &au["accepted_at"]),
        ("policy_snapshot", &policy["id"]),
        ("basis", &basis["id"]),
    ] {
        check(ab[k] == *expected)?;
    }
    check(ab["target_state"] == "final_unreversed" && ab["decision"] == "allow")?;
    for (key, purpose) in [("authentication", "authentication"), ("grant", "grant")] {
        check(records.get(&ab[key], "evidence")?["body"]["purpose"] == purpose)?;
    }
    let original_receipt = if prior.is_none() {
        let mut facts = data.clone();
        facts.as_object_mut().unwrap().remove("external_id");
        facts["evidence"] = json!(ordered(
            records
                .documents_for(&data["evidence"])?
                .into_iter()
                .map(Value::String)
                .collect()
        )?);
        add(
            &mut rows,
            s,
            "claim",
            json!({"target":target,"agreement_id":p["agreement_id"],"book":p["book"],"family_id":p["family_id"],"first_event":e,"original_receipt":receipt_id,"facts_hash":hash("claim-facts",&facts)?}),
        )?;
        receipt_id.clone()
    } else {
        records.get(&clid, "claim")?["body"]["original_receipt"].clone()
    };
    add(
        &mut rows,
        s,
        "link",
        json!({"event_id":e,"target":target,"relation":"outcome_of"}),
    )?;
    let mut actions = vec![];
    let mut inverses = vec![];
    let mut replacements = vec![];
    for posting in decision.postings() {
        let slot = if posting.reverses_revision.is_some() {
            "inverse"
        } else {
            "replacement"
        };
        let effect = id(
            "effect",
            s,
            json!({"claim_id":clid,"revision_id":rid,"slot":slot}),
        )?;
        let mut body = json!({"binding_id":p["binding_id"],"binding_snapshot":binding["id"],"component":p["family_id"],"event_id":e,"effect_id":effect,"revision_id":rid,"claim_id":clid,"slot":slot,"amount":amount(base,posting.amount.atoms()),"obligation_id":obligation["id"],"book":p["book"],"agreement_id":p["agreement_id"],"family_id":p["family_id"],"roles":p["roles"],"policy_snapshot":policy["id"],"basis":basis["id"]});
        let reverse = if posting.reverses_revision.is_some() {
            let ids = array(&prior.ok_or_else(integrity)?["body"]["live_action_ids"])?;
            check(ids.len() == 1)?;
            let r = records.get(&ids[0], "action")?;
            check(
                money(&r["body"]["amount"])?.atoms().checked_neg() == Some(posting.amount.atoms()),
            )?;
            body["reverses"] = r["id"].clone();
            Some(r)
        } else {
            None
        };
        let action = add(&mut rows, s, "action", body)?;
        let mut facts = action["body"].clone();
        for k in ["schema", "event_id", "effect_id", "policy_snapshot"] {
            facts.as_object_mut().unwrap().remove(k);
        }
        add(
            &mut rows,
            s,
            "effect",
            json!({"claim_id":clid,"revision_id":rid,"slot":slot,"action_id":action["id"],"binding_id":p["binding_id"],"binding_snapshot":binding["id"],"component":p["family_id"],"facts_hash":hash("effect-facts",&facts)?}),
        )?;
        dependency(&mut rows, s, &action["id"], basis, "frozen_basis")?;
        if let Some(reverse) = reverse {
            dependency(&mut rows, s, &action["id"], reverse, "exact_inverse")?;
            inverses.push(action["id"].clone());
        } else {
            replacements.push(action["id"].clone());
        }
        actions.push(action);
    }
    let mut rb = json!({"claim_id":clid,"number":decision.revision().to_string(),"event_id":e,"state":if decision.current_code().is_some(){"active"}else{"reversed"},"binding_id":p["binding_id"],"target_snapshot":base.snapshot["id"],"policy_snapshot":policy["id"],"basis":basis["id"],"admission":adm["id"],"live_action_ids":ordered(replacements.clone())?,"inverse_action_ids":ordered(inverses.clone())?,"amount":amount(base,decision.current().atoms()),"original_receipt":original_receipt});
    if let Some(code) = decision.current_code() {
        rb["code"] = json!(code);
    }
    if let Some(prior) = prior {
        rb["previous"] = prior["id"].clone();
    }
    let revision = add(&mut rows, s, "claim-revision", rb)?;
    let mut heads = BTreeMap::new();
    for r in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "claim-revision" && r["body"]["binding_id"] == p["binding_id"])
    {
        let n = text(&r["body"]["number"])?
            .parse::<u64>()
            .map_err(|_| integrity())?;
        let entry = heads
            .entry(bytes(&r["body"]["claim_id"])?)
            .or_insert((0, r));
        if n > entry.0 {
            *entry = (n, r);
        }
    }
    let before = heads.values().map(|(_, r)| *r).collect::<Vec<_>>();
    let mut after = before
        .iter()
        .copied()
        .filter(|r| r["body"]["claim_id"] != clid)
        .collect::<Vec<_>>();
    after.push(&revision);
    let totals = |rows: &[&Value]| -> Result<(i128, i128)> {
        rows.iter()
            .try_fold((0i128, 0i128), |(premium, discount), r| {
                let n = money(&r["body"]["amount"])?.atoms();
                Ok((
                    core(ledgerlab_core::money::add_atoms(premium, n.max(0)))?,
                    core(ledgerlab_core::money::add_atoms(discount, (-n).max(0)))?,
                ))
            })
    };
    let (bp, bd) = totals(&before)?;
    let (ap, ad) = totals(&after)?;
    let mut lf = json!({"binding_id":p["binding_id"],"target_snapshot":base.snapshot["id"],"event_id":e,"target":target,"agreement_id":p["agreement_id"],"book":p["book"],"currency":p["currency"],"scale":p["scale"],"current_before":ordered(before.iter().map(|r|reference(r)).collect())?,"before_premium":bp.to_string(),"before_discount":bd.to_string(),"after_premium":ap.to_string(),"after_discount":ad.to_string(),"maximum_premium":p["max_premium_atoms"],"maximum_discount":p["max_discount_atoms"]});
    if let Some(prior) = prior {
        lf["replacing"] = prior["id"].clone();
    }
    let limit = add(&mut rows, s, "limit-evidence", lf)?;
    let mut explanations = vec![];
    for (ordinal, x) in decision.explanations().iter().enumerate() {
        let aids = if prior.is_some() && ordinal == 0 {
            &inverses
        } else {
            &replacements
        };
        let xp = add(
            &mut rows,
            s,
            "explanation",
            json!({"event_id":e,"ordinal":ordinal,"authority_decision":auth["id"],"evidence":data["evidence"],"claim_id":clid,"revision_id":rid,"code":x.code,"policy_snapshot":policy["id"],"basis":basis["id"],"basis_amount":basis["body"]["amount"],"unrounded_atoms":x.exact,"rounded_atoms":x.rounded.atoms().to_string(),"rounding":"nearest_ties_away","action_ids":ordered(aids.clone())?,"limit_evidence":limit["id"]}),
        )?;
        dependency(&mut rows, s, &xp["id"], basis, "frozen_basis")?;
        dependency(&mut rows, s, &xp["id"], &limit, "aggregate_limit")?;
        if let Some(prior) = prior {
            dependency(&mut rows, s, &xp["id"], prior, "prior_revision")?;
        }
        explanations.push(xp["id"].clone());
    }
    // Evidence already present in Records is excluded from the prior set below.
    let new_evidence = rows
        .iter()
        .filter(|r| r["kind"] == "evidence")
        .map(|r| r["id"].clone())
        .collect::<Vec<_>>();
    let mut replay_refs = records
        .rows
        .iter()
        .filter(|r| !new_evidence.contains(&r["id"]))
        .map(reference)
        .collect::<Vec<_>>();
    replay_refs.extend([reference(&event), reference(&adm), reference(&auth)]);
    replay_refs.extend(
        rows.iter()
            .filter(|r| r["kind"] == "evidence")
            .map(reference),
    );
    let replay_refs = ordered(replay_refs)?;
    add(
        &mut rows,
        s,
        "replay-input",
        json!({"event_id":e,"semantics":"ledger-outcome-semantics/2-candidate.4","target_snapshot":base.snapshot["id"],"authority_decision":auth["id"],"inputs":replay_refs,"received_at":au["received_at"],"accepted_at":au["accepted_at"]}),
    )?;
    let net = actions.iter().try_fold(0i128, |n, a| {
        core(ledgerlab_core::money::add_atoms(
            n,
            money(&a["body"]["amount"])?.atoms(),
        ))
    })?;
    let mut intention_ids = vec![];
    if net != 0 {
        let aids = ordered(actions.iter().map(|a| a["id"].clone()).collect())?;
        let iid = id(
            "intention",
            s,
            json!({"destination":"fake","obligation_id":obligation["id"],"action_ids":aids}),
        )?;
        let previous_events = records
            .rows
            .iter()
            .filter(|r| r["kind"] == "claim-revision" && r["body"]["claim_id"] == clid)
            .map(|r| r["body"]["event_id"].clone())
            .collect::<Vec<_>>();
        let deps = records
            .rows
            .iter()
            .filter(|r| {
                r["kind"] == "intention" && previous_events.contains(&r["body"]["event_id"])
            })
            .map(|r| r["id"].clone())
            .collect();
        add(
            &mut rows,
            s,
            "intention",
            json!({"event_id":e,"destination":"fake","obligation_id":obligation["id"],"action_ids":aids,"amount":amount(base,net),"depends_on":ordered(deps)?,"idempotency_key":iid,"payload":{"schema":"ledger-obligation-delta/2-candidate.4","obligation_id":obligation["id"],"amount":amount(base,net),"actions":ordered(actions.iter().map(|a|json!({"action_id":a["id"],"amount":a["body"]["amount"]})).collect())?}}),
        )?;
        intention_ids.push(iid);
    }
    add(
        &mut rows,
        s,
        "delivery-key",
        json!({"source":data["source"],"external_id":data["external_id"],"event_id":e,"ingress":event["body"],"ingress_hash":hash("ingress",&event["body"] )?,"original_receipt":receipt_id}),
    )?;
    let chain_revision = (records
        .rows
        .iter()
        .filter(|r| r["kind"] == "receipt")
        .count()
        + 1)
    .to_string();
    add(
        &mut rows,
        s,
        "chain-revision",
        json!({"chain_id":data["chain_id"],"number":chain_revision,"event_id":e,"decision_id":decision_id}),
    )?;
    let mut membership = BTreeMap::new();
    for r in rows.iter().map(reference).chain(replay_refs) {
        membership.insert(bytes(&json!([r["kind"], r["id"]]))?, r);
    }
    let mut refs = membership.into_values().collect::<Vec<_>>();
    refs.sort_by(|a, b| {
        (text(&a["kind"]).unwrap(), bytes(&a["id"]).unwrap())
            .cmp(&(text(&b["kind"]).unwrap(), bytes(&b["id"]).unwrap()))
    });
    let manifest = add(
        &mut rows,
        s,
        "decision-manifest",
        json!({"event_id":e,"chain_id":data["chain_id"],"revision":chain_revision,"accepted_at":au["accepted_at"],"members":refs,"explanation_ids":explanations}),
    )?;
    add(
        &mut rows,
        s,
        "receipt",
        json!({"event_id":e,"decision_id":decision_id,"chain_id":data["chain_id"],"revision":chain_revision,"accepted_at":au["accepted_at"],"event_hash":event["content_hash"],"decision_hash":manifest["content_hash"],"claim_id":clid,"claim_revision":rid,"action_ids":ordered(actions.iter().map(|a|a["id"].clone()).collect())?,"intention_ids":intention_ids}),
    )?;
    rows.sort_by(|a, b| {
        (text(&a["kind"]).unwrap(), bytes(&a["id"]).unwrap())
            .cmp(&(text(&b["kind"]).unwrap(), bytes(&b["id"]).unwrap()))
    });
    for r in &rows {
        core(codec::decode(&bytes(r)?))?;
    }
    Ok(rows)
}

pub(in crate::service) fn replay(
    records: &Records,
    base: &Base,
    decisions: &[Vec<Value>],
) -> Result<Vec<o::Decision>> {
    replay_checked(records, base, decisions, &mut || Ok(()))
}
pub(in crate::service) fn replay_checked(
    records: &Records,
    base: &Base,
    decisions: &[Vec<Value>],
    checkpoint: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<o::Decision>> {
    let seed = array(&base.acceptance["body"]["members"])?
        .iter()
        .map(|r| records.deref(r).cloned())
        .chain(std::iter::once(Ok(base.acceptance.clone())))
        .collect::<Result<Vec<_>>>()?;
    let mut prior = Records::new(
        &seed.iter().map(bytes).collect::<Result<Vec<_>>>()?,
        records.scope.clone(),
    )?;
    let mut history = vec![];
    for rows in decisions {
        checkpoint()?;
        let one = |k: &str| -> Result<&Value> {
            let v = rows.iter().filter(|r| r["kind"] == k).collect::<Vec<_>>();
            check(v.len() == 1)?;
            Ok(v[0])
        };
        let evidence = rows
            .iter()
            .filter(|r| r["kind"] == "evidence")
            .cloned()
            .collect::<Vec<_>>();
        for e in &evidence {
            prior.insert(e.clone())?;
        }
        prior.documents()?;
        let event = one("event")?;
        let req = request(&prior, base, &event["body"]["data"])?;
        let ver = verified(&prior, &req, &one("authority-decision")?["body"])?;
        let result = core(o::evaluate(
            &req,
            &ver,
            std::slice::from_ref(&base.target),
            std::slice::from_ref(&base.evaluation),
            &history,
        ))?;
        let o::Submission::Accepted(result) = result else {
            return Err(integrity());
        };
        let projected = project(
            &prior,
            base,
            event["body"].clone(),
            one("authority-decision")?["body"].clone(),
            one("admission")?["body"].clone(),
            &result,
            evidence,
        )?;
        check(projected == *rows)?;
        for r in rows.iter().filter(|r| r["kind"] != "evidence") {
            prior.insert(r.clone())?;
        }
        history.push(*result);
    }
    Ok(history)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_frozen_decisions_replay_and_project_exactly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/candidates/v2/goldens");
        let mut count = 0;
        for path in std::fs::read_dir(root).unwrap() {
            let path = path.unwrap().path();
            let h: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if h["target_admission"] != "accepted" {
                continue;
            }
            let seed = h["seed"].as_array().unwrap();
            let mut all = seed.clone();
            let decisions = h["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["records"].as_array().unwrap().clone())
                .collect::<Vec<_>>();
            for d in &decisions {
                all.extend(d.clone());
            }
            let records = Records::new(
                &all.iter().map(|r| bytes(r).unwrap()).collect::<Vec<_>>(),
                json!(["synthetic", "sandbox"]),
            )
            .unwrap();
            let anchor = reference(records.one("base-acceptance").unwrap());
            let base = decode_base(&records, &anchor).unwrap();
            let history = replay(&records, &base, &decisions)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            count += history.len();
        }
        assert_eq!(count, 43);
    }
}
