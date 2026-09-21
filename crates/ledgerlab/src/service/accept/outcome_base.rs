//! Complete retained base decoding and identity bridge for the bounded final-base path.
use crate::ServiceError;
use ledgerlab_core::{
    canonical::{self, outcome as codec, Domain},
    domain::{Scope, Timestamp},
    money::{ExactRatio, Money},
    policy::chaining::{self as c, outcomes as o},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub(super) type Result<T> = std::result::Result<T, ServiceError>;
pub(super) fn integrity() -> ServiceError {
    ServiceError::IntegrityFailure
}
#[track_caller]
pub(super) fn check(v: bool) -> Result<()> {
    if v {
        Ok(())
    } else {
        Err(integrity())
    }
}
pub(super) fn core<T>(r: ledgerlab_core::Result<T>) -> Result<T> {
    r.map_err(|_| integrity())
}
pub(super) fn text(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(integrity)
}
pub(super) fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(integrity)
}
pub(super) fn parse(v: &Value) -> Result<Value> {
    core(canonical::parse_bounded(
        text(v)?.as_bytes(),
        canonical::BUNDLE_LIMIT,
    ))
}
pub(super) fn bytes(v: &Value) -> Result<Vec<u8>> {
    core(codec::bytes(v))
}
pub(super) fn ordered(v: Vec<Value>) -> Result<Vec<Value>> {
    core(codec::ordered(v))
}
pub(super) fn time(v: &Value) -> Result<Timestamp> {
    core(Timestamp::parse(text(v)?))
}
pub(super) fn money(v: &Value) -> Result<Money> {
    serde_json::from_value(v.clone()).map_err(|_| integrity())
}
pub(super) fn hash(domain: &str, v: &Value) -> Result<String> {
    core(canonical::outcome_digest(codec::ECONOMIC, domain, v))
}
pub(super) fn row(kind: &str, scope: &Value, body: Value) -> Result<Value> {
    core(codec::envelope(codec::ECONOMIC, kind, scope, body))
}
pub(super) fn reference(v: &Value) -> Value {
    codec::reference(v)
}
pub(super) fn members(v: &[Value]) -> Result<Vec<Value>> {
    let mut refs = v
        .iter()
        .map(reference)
        .map(|v| Ok((text(&v["kind"])?.to_owned(), bytes(&v["id"])?, v)))
        .collect::<Result<Vec<_>>>()?;
    refs.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    Ok(refs.into_iter().map(|v| v.2).collect())
}
#[derive(Clone)]
pub(super) struct Records {
    pub rows: Vec<Value>,
    map: BTreeMap<Vec<u8>, usize>,
    pub scope: Value,
}
impl Records {
    pub fn new(raw: &[Vec<u8>], scope: Value) -> Result<Self> {
        check(raw.len() <= 4096 && raw.iter().map(Vec::len).sum::<usize>() <= 16 * 1024 * 1024)?;
        let mut out = Self {
            rows: vec![],
            map: BTreeMap::new(),
            scope,
        };
        for raw in raw {
            out.insert(core(codec::decode(raw))?)?;
        }
        Ok(out)
    }
    pub fn insert(&mut self, r: Value) -> Result<()> {
        check(r["scope"] == self.scope)?;
        let id = bytes(&r["id"])?;
        check(!self.map.contains_key(&id))?;
        self.map.insert(id, self.rows.len());
        self.rows.push(r);
        Ok(())
    }
    pub fn get(&self, id: &Value, kind: &str) -> Result<&Value> {
        let r = self
            .rows
            .get(*self.map.get(&bytes(id)?).ok_or_else(integrity)?)
            .ok_or_else(integrity)?;
        check(r["kind"] == kind)?;
        Ok(r)
    }
    pub fn deref(&self, r: &Value) -> Result<&Value> {
        let row = self.get(&r["id"], text(&r["kind"])?)?;
        check(reference(row) == *r)?;
        Ok(row)
    }
    pub fn one(&self, kind: &str) -> Result<&Value> {
        let mut rows = self.rows.iter().filter(|r| r["kind"] == kind);
        let r = rows.next().ok_or_else(integrity)?;
        check(rows.next().is_none())?;
        Ok(r)
    }
    pub fn documents(&self) -> Result<BTreeMap<String, String>> {
        let mut map = BTreeMap::new();
        for r in &self.rows {
            if r["kind"] != "evidence" {
                continue;
            }
            let b = &r["body"];
            let v = parse(&b["utf8"])?;
            let h = core(canonical::digest(
                Domain::Document,
                &json!([b["document_type"], 1, v]),
            ))?;
            check(
                b["document_version"] == 1
                    && b["document_hash"] == h
                    && b["document_id"] == format!("doc_{}", &h[7..])
                    && bytes(&v)? == text(&b["utf8"])?.as_bytes(),
            )?;
            let entry = map
                .entry(text(&b["document_id"])?.to_owned())
                .or_insert_with(|| text(&r["id"]).unwrap().to_owned());
            if text(&r["id"])? < entry.as_str() {
                *entry = text(&r["id"])?.into()
            }
        }
        Ok(map)
    }
    pub fn document(&self, id: &Value) -> Result<String> {
        Ok(text(&self.get(id, "evidence")?["body"]["document_id"])?.into())
    }
    pub fn documents_for(&self, ids: &Value) -> Result<Vec<String>> {
        let docs = array(ids)?
            .iter()
            .map(|id| self.document(id))
            .collect::<Result<Vec<_>>>()?;
        check(docs.iter().collect::<BTreeSet<_>>().len() == docs.len())?;
        Ok(docs)
    }
}
fn tagged(v: &Value) -> Result<(&str, Value)> {
    let o = v.as_object().ok_or_else(integrity)?;
    check(o.len() == 1)?;
    let (k, v) = o.iter().next().unwrap();
    Ok((k, v.clone()))
}
fn object_tag(kind: &str, mut v: Value) -> Result<Value> {
    v.as_object_mut()
        .ok_or_else(integrity)?
        .insert("kind".into(), json!(kind));
    Ok(v)
}
fn binding(v: &mut Value) -> Result<()> {
    v["book"] = json!(book(text(&v["book"])?)?);
    Ok(())
}
fn book(v: &str) -> Result<&str> {
    match v {
        "Retail" => Ok("retail"),
        "Supplier" => Ok("supplier"),
        "CostObservation" => Ok("cost_observation"),
        "Allocation" => Ok("allocation"),
        _ => Err(integrity()),
    }
}
fn price(v: &Value) -> Result<Value> {
    let (k, v) = tagged(v)?;
    if k == "Fixed" {
        Ok(json!({"kind":"fixed","value":v}))
    } else {
        object_tag(&k.to_lowercase(), v)
    }
}
pub(super) fn projection(original: &Value) -> Result<Value> {
    let mut v = original.clone();
    v["event"] = parse(&v["event"]["event_utf8"])?;
    for p in v["bundle"]["policies"]
        .as_array_mut()
        .ok_or_else(integrity)?
    {
        binding(&mut p["binding"])?;
        for r in p["rules"].as_array_mut().ok_or_else(integrity)? {
            let op = &r["operation"];
            r["operation"] = if op == "ObserveCost" {
                json!({"kind":"observe_cost"})
            } else {
                let (k, mut x) = tagged(op)?;
                match k {
                    "Base" | "Premium" => json!({"kind":k.to_lowercase(),"price":price(&x)?}),
                    "Discount" | "LinkedDiscount" => {
                        let (amount_kind, a) = tagged(&x["amount"])?;
                        let amount_kind = amount_kind.to_lowercase();
                        x["amount"] = json!({"kind":amount_kind,"value":a});
                        if x.get("mode").is_some() {
                            x["mode"] = json!(text(&x["mode"])?.to_lowercase());
                        }
                        object_tag(
                            if k == "LinkedDiscount" {
                                "linked_discount"
                            } else {
                                "discount"
                            },
                            x,
                        )?
                    }
                    _ => object_tag(&k.to_lowercase(), x)?,
                }
            };
            for p in r["when"].as_array_mut().ok_or_else(integrity)? {
                let (k, v) = tagged(p)?;
                *p = json!({"kind":k.to_lowercase(),"value":if k=="Funding"{json!(text(&v)?.to_lowercase())}else{v}})
            }
            if let Some(m) = r.get_mut("matcher") {
                *m = if *m == "AcquisitionOptimization" {
                    json!({"kind":"acquisition_optimization"})
                } else {
                    object_tag("direct", m["Direct"].clone())?
                }
            }
        }
    }
    v["context"]["funding"] = json!(text(&v["context"]["funding"])?.to_lowercase());
    for a in v["actions"].as_array_mut().ok_or_else(integrity)? {
        binding(&mut a["binding"])?;
        a["book"] = json!(book(text(&a["book"])?)?);
        a["kind"] = json!(text(&a["kind"])?.to_lowercase());
    }
    for d in v["deltas"].as_array_mut().ok_or_else(integrity)? {
        d["book"] = json!(book(text(&d["book"])?)?)
    }
    Ok(v)
}
fn window(v: &Value) -> Result<o::Window> {
    Ok(o::Window {
        starts_at: time(&v["starts_at"])?,
        occurs_before: time(&v["occurs_before"])?,
        received_by: time(&v["received_by"])?,
        accepted_by: time(&v["accepted_by"])?,
    })
}
pub(super) fn policy(v: &Value) -> Result<o::Policy> {
    Ok(o::Policy {
        version: text(&v["version"])?.into(),
        document: text(&v["document"])?.into(),
        families: array(&v["families"])?
            .iter()
            .map(|f| {
                Ok(o::Family {
                    family: text(&f["family"])?.into(),
                    binding_id: text(&f["binding_id"])?.into(),
                    source: text(&f["source"])?.into(),
                    correction_source: text(&f["correction_source"])?.into(),
                    evidence_required: f["evidence_required"].as_bool().ok_or_else(integrity)?,
                    ordinary: window(&f["ordinary"])?,
                    corrections: window(&f["corrections"])?,
                    codes: array(&f["codes"])?
                        .iter()
                        .map(|c| {
                            Ok(o::Code {
                                code: text(&c["code"])?.into(),
                                amount: if c["amount"]["kind"] == "fixed" {
                                    o::Amount::Fixed(money(&c["amount"]["money"])?)
                                } else {
                                    o::Amount::Percent(core(ExactRatio::from_canonical(
                                        text(&c["amount"]["rate"]["numerator"])?,
                                        text(&c["amount"]["rate"]["denominator"])?,
                                    ))?)
                                },
                            })
                        })
                        .collect::<Result<_>>()?,
                    replacement_codes: array(&f["replacement_codes"])?
                        .iter()
                        .map(|v| Ok(text(v)?.into()))
                        .collect::<Result<_>>()?,
                    allow_reversal: f["allow_reversal"].as_bool().ok_or_else(integrity)?,
                })
            })
            .collect::<Result<_>>()?,
        limits: array(&v["limits"])?
            .iter()
            .map(|v| {
                Ok(o::Limit {
                    binding_id: text(&v["binding_id"])?.into(),
                    premium: money(&v["premium"])?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

pub(super) struct Base {
    pub evaluation: c::Evaluation,
    pub target: o::Target,
    pub acceptance: Value,
    pub snapshot: Value,
    pub basis: Value,
}
pub(super) fn decode_base(records: &Records, anchor: &Value) -> Result<Base> {
    #[cfg(test)]
    let _trace = crate::store::postgres::trace::Span::new("decode_base");
    let acceptance = records.deref(anchor)?.clone();
    check(acceptance["kind"] == "base-acceptance")?;
    let ab = &acceptance["body"];
    let snapshot = records.deref(&ab["target_snapshot"])?.clone();
    let tb = &snapshot["body"];
    let evaluation = records.deref(&ab["base_evaluation"])?;
    let bb = &evaluation["body"];
    let seed = array(&ab["members"])?
        .iter()
        .map(|r| records.deref(r).cloned())
        .collect::<Result<Vec<_>>>()?;
    check(members(&seed)? == *array(&ab["members"])?)?;
    for kind in ["event", "base-evaluation", "target-snapshot"] {
        check(seed.iter().filter(|r| r["kind"] == kind).count() == 1)?;
    }
    check(!seed.iter().any(|r| r["kind"] == "base-acceptance"))?;
    check(
        bb["postings"]
            == json!(ordered(
                seed.iter()
                    .filter(|r| r["kind"] == "base-posting")
                    .map(reference)
                    .collect()
            )?),
    )?;
    let receipt = json!({"schema":"ledger-base-receipt/2-candidate.4","target":ab["target"],"base_evaluation":ab["base_evaluation"],"target_snapshot":ab["target_snapshot"],"accepted_at":ab["accepted_at"],"membership_hash":hash("base-membership",&ab["members"])?});
    check(bytes(&receipt)? == text(&ab["original_receipt_utf8"])?.as_bytes())?;
    check(
        tb["base_evaluation"] == reference(evaluation)
            && ab["accepted_at"] == tb["accepted_at"]
            && ab["target"] == tb["target"]
            && tb["target"] == bb["event_id"],
    )?;
    check(bb["source_state"] == "accepted" && array(&bb["predecessors"])?.is_empty())?; // bounded single-final-base path
    let original = parse(&bb["original_evaluation_utf8"])?;
    let replay = core(c::retained::decode_evaluation(
        text(&bb["original_evaluation_utf8"])?.as_bytes(),
        &[],
    ))?;
    let material = projection(&original)?;
    check(bytes(&material)? == text(&bb["evaluation_utf8"])?.as_bytes())?;
    for key in [
        "event_id",
        "event_hash",
        "event_utf8",
        "ingress_hash",
        "ingress_utf8",
    ] {
        check(original["event"][key] == bb[format!("original_{key}")])?
    }
    check(
        original["event"]["scope"] == records.scope && material["received_at"] == bb["received_at"],
    )?;
    let aliases = records.documents()?;
    let doc =
        |id: &Value| -> Result<Value> { Ok(json!(aliases.get(text(id)?).ok_or_else(integrity)?)) };
    let ev = &material["event"];
    let event = records.get(&tb["target"], "event")?;
    let mut data = json!({"type":"base","source":replay.event().source(),"external_id":ev["id"],"chain_id":ev["chain"],"work_type":ev["type"]});
    for k in [
        "customer",
        "operation_id",
        "occurred_at",
        "status",
        "quantity",
        "unit",
    ] {
        data[k] = ev[k].clone();
    }
    data["evidence"] = json!(ordered(
        array(&ev["evidence"])?
            .iter()
            .map(doc)
            .collect::<Result<_>>()?
    )?);
    check(event["body"]["data"] == data)?;
    let bindings = seed
        .iter()
        .filter(|r| r["kind"] == "binding-snapshot")
        .collect::<Vec<_>>();
    check(
        tb["bindings"] == bb["bindings"]
            && tb["bindings"] == json!(ordered(bindings.iter().map(|r| reference(r)).collect())?),
    )?;
    let mut originals = BTreeMap::new();
    for p in array(&material["bundle"]["policies"])? {
        check(
            originals
                .insert(text(&p["binding"]["id"])?, &p["binding"])
                .is_none(),
        )?;
    }
    check(
        bindings.len()
            == originals
                .values()
                .filter(|v| v["book"] == "retail" || v["book"] == "supplier")
                .count(),
    )?;
    for r in &bindings {
        let b = &r["body"];
        let original = originals
            .get(text(&b["binding_id"])?)
            .ok_or_else(integrity)?;
        check(parse(&b["binding_utf8"])? == **original)?;
        let mut expected = (*original).clone();
        let obj = expected.as_object_mut().ok_or_else(integrity)?;
        obj.remove("id");
        obj.remove("agreement");
        obj.insert("binding_id".into(), b["binding_id"].clone());
        obj.insert("agreement_id".into(), original["agreement"].clone());
        obj.insert("assent".into(), doc(&original["assent"])?);
        if original.get("offer").is_some() {
            obj.insert("offer".into(), doc(&original["offer"])?);
        }
        if original["roles"].get("payer_delegation").is_some() {
            obj.insert(
                "delegation".into(),
                doc(&original["roles"]["payer_delegation"])?,
            );
        }
        for k in [
            "sources",
            "event_types",
            "correction_sources",
            "allowed_modifiers",
        ] {
            expected[k] = json!(ordered(array(&original[k])?.clone())?);
        }
        let mut actual = b.clone();
        for k in [
            "schema",
            "binding_utf8",
            "booked_net",
            "supplier_invocation",
        ] {
            actual.as_object_mut().unwrap().remove(k);
        }
        check(expected == actual)?;
        let booked = replay
            .actions()
            .iter()
            .filter(|a| a.binding().id == text(&b["binding_id"]).unwrap())
            .try_fold(0i128, |sum, a| {
                core(ledgerlab_core::money::add_atoms(sum, a.amount().atoms()))
            })?;
        check(
            money(&b["booked_net"])?
                == core(Money::new(
                    text(&material["bundle"]["currency"])?,
                    material["bundle"]["scale"]
                        .as_u64()
                        .ok_or_else(integrity)?
                        .try_into()
                        .map_err(|_| integrity())?,
                    booked,
                ))?,
        )?;
        if b["book"] == "supplier" {
            let invocation = replay
                .invocations()
                .iter()
                .find(|i| {
                    i.binding_id == text(&b["binding_id"]).unwrap()
                        && replay.event().dto().invocation_id.as_deref() == Some(&i.id)
                })
                .ok_or_else(integrity)?;
            check(
                b["supplier_invocation"]
                    == serde_json::to_value(invocation).map_err(|_| integrity())?,
            )?;
        }
    }
    validate_mappings(records, bb, &material, &seed)?;
    let policies = seed
        .iter()
        .filter(|r| r["kind"] == "policy-snapshot")
        .collect::<Vec<_>>();
    check(tb["families"] == json!(ordered(policies.iter().map(|r| reference(r)).collect())?))?;
    let source_policy = parse(&tb["policy_utf8"])?;
    check(
        source_policy["document"] == tb["policy_document"]
            && tb["verified_policy_document"] == tb["policy_document"],
    )?;
    let mut terms = source_policy.clone();
    terms.as_object_mut().unwrap().remove("document");
    let proof = records.get(&doc(&source_policy["document"])?, "evidence")?;
    check(
        parse(&proof["body"]["utf8"])? == terms
            && proof["body"]["document_hash"] == tb["policy_document_hash"]
            && array(&tb["policy_evidence"])?.contains(&proof["id"]),
    )?;
    let tv = o::TargetVerification {
        rated_final: tb["rated_final"] == true,
        accepted_at: time(&tb["accepted_at"])?,
        policy_document: text(&tb["verified_policy_document"])?.into(),
        verified_assents: records.documents_for(&tb["verified_assents"])?,
        verified_offers: records.documents_for(&tb["verified_offers"])?,
        verified_delegations: records.documents_for(&tb["verified_delegations"])?,
    };
    let target = core(o::Target::freeze(&replay, policy(&source_policy)?, tv))?;
    let basis = records.deref(&tb["retail_basis"])?.clone();
    check(
        basis["kind"] == "target-basis"
            && basis["body"]["book"] == "retail"
            && basis["body"]["amount"]
                == serde_json::to_value(target.retail_basis()).map_err(|_| integrity())?,
    )?;
    for r in seed.iter().filter(|r| r["kind"] == "target-basis") {
        let b = &r["body"];
        let postings = seed
            .iter()
            .filter(|p| {
                p["kind"] == "base-posting"
                    && p["body"]["book"] == b["book"]
                    && p["body"]["agreement_id"] == b["agreement_id"]
            })
            .collect::<Vec<_>>();
        check(
            b["postings"] == json!(ordered(postings.iter().map(|p| reference(p)).collect())?)
                && b["target"] == tb["target"]
                && b["finality"] == "final"
                && b["finality_evidence"] == tb["finality_evidence"],
        )?;
        let sum = postings.iter().try_fold(0i128, |n, p| {
            core(ledgerlab_core::money::add_atoms(
                n,
                money(&p["body"]["amount"])?.atoms(),
            ))
        })?;
        check(sum >= 0 && money(&b["amount"])?.atoms() == sum)?;
    }
    validate_families(records, tb, &source_policy, &policies, &bindings)?;
    Ok(Base {
        evaluation: replay,
        target,
        acceptance,
        snapshot,
        basis,
    })
}
fn validate_mappings(records: &Records, bb: &Value, m: &Value, seed: &[Value]) -> Result<()> {
    let mut identities = BTreeSet::new();
    fn walk(v: &Value, ids: &mut BTreeSet<(String, String)>) {
        match v {
            Value::Object(o) => {
                for (k, v) in o {
                    if k != "extensions" {
                        walk(v, ids)
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, ids)
                }
            }
            Value::String(s) => {
                if let Some((p, h)) = s.split_once('_') {
                    if h.len() == 64
                        && h.bytes()
                            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    {
                        let kind = match p {
                            "ev" => "event",
                            "ac" => "action",
                            "ef" => "effect",
                            "ob" => "obligation",
                            "cl" => "claim",
                            "lk" => "link",
                            "doc" => "document",
                            _ => return,
                        };
                        ids.insert((kind.into(), s.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    walk(m, &mut identities);
    identities.insert(("event".into(), text(&bb["original_event_id"])?.into()));
    for p in array(&m["bundle"]["policies"])? {
        identities.insert(("binding".into(), text(&p["binding"]["id"])?.into()));
    }
    for i in array(&m["invocations"])? {
        identities.insert(("invocation".into(), text(&i["id"])?.into()));
    }
    let mut seen = BTreeSet::new();
    let mut projections = BTreeSet::new();
    let mut postings = vec![];
    let mut index = vec![];
    for map in array(&bb["identity_mappings"])? {
        let kind = text(&map["original_kind"])?;
        let id = text(&map["original_id"])?;
        check(
            seen.insert((kind.to_owned(), id.to_owned()))
                && projections.insert(bytes(&map["projection"]["id"])?),
        )?;
        check(
            map["target"] == bb["event_id"] && map["original_target"] == bb["original_event_id"],
        )?;
        let r = records.deref(&map["projection"])?;
        let b = &r["body"];
        let action = array(&m["actions"])?.iter().find(|a| a["id"] == id);
        match kind {
            "event" if map["original_id"] == bb["original_event_id"] => {
                check(r["kind"] == "event" && r["id"] == bb["event_id"])?
            }
            "action"
                if action.is_some_and(|a| a["book"] == "retail" || a["book"] == "supplier") =>
            {
                let a = action.unwrap();
                let binding = &a["binding"];
                let ordinal = array(&m["actions"])?
                    .iter()
                    .filter(|x| x["binding"]["id"] == binding["id"])
                    .position(|x| x["id"] == id)
                    .ok_or_else(integrity)?;
                check(
                    *r == row(
                        "base-posting",
                        &records.scope,
                        json!({"event_id":bb["event_id"],"agreement_id":binding["agreement"],"book":a["book"],"ordinal":ordinal,"binding_id":binding["id"],"amount":a["amount"],"roles":binding["roles"]}),
                    )?,
                )?;
                postings.push(reference(r));
            }
            "obligation"
                if array(&m["actions"])?.iter().any(|a| {
                    a["obligation_id"] == id && (a["book"] == "retail" || a["book"] == "supplier")
                }) =>
            {
                for a in array(&m["actions"])?
                    .iter()
                    .filter(|a| a["obligation_id"] == id)
                {
                    check(
                        *r == row(
                            "obligation",
                            &records.scope,
                            json!({"agreement_id":a["binding"]["agreement"],"book":a["book"],"currency":a["amount"]["currency"],"scale":a["amount"]["scale"],"roles":a["binding"]["roles"]}),
                        )?,
                    )?;
                }
            }
            "binding"
                if array(&m["bundle"]["policies"])?.iter().any(|p| {
                    p["binding"]["id"] == id
                        && (p["binding"]["book"] == "retail" || p["binding"]["book"] == "supplier")
                }) =>
            {
                check(r["kind"] == "binding-snapshot" && b["binding_id"] == id)?
            }
            "document" => check(r["kind"] == "evidence" && b["document_id"] == id)?,
            _ => {
                check(
                    *r == row(
                        "base-identity",
                        &records.scope,
                        json!({"target":map["target"],"original_target":map["original_target"],"original_kind":kind,"original_id":id}),
                    )?,
                )?;
                index.push(r["id"].clone());
            }
        }
    }
    check(
        seen == identities
            && json!(ordered(postings)?) == bb["postings"]
            && ordered(index)?
                == ordered(
                    seed.iter()
                        .filter(|r| r["kind"] == "base-identity")
                        .map(|r| r["id"].clone())
                        .collect(),
                )?,
    )
}
fn validate_families(
    records: &Records,
    tb: &Value,
    p: &Value,
    policies: &[&Value],
    bindings: &[&Value],
) -> Result<()> {
    check(policies.len() == array(&p["families"])?.len())?;
    let mut limits = vec![];
    for limit in array(&tb["limits"])? {
        let binding = bindings
            .iter()
            .find(|b| b["body"]["binding_id"] == limit["binding_id"])
            .ok_or_else(integrity)?;
        check(limit["discount_capacity"] == binding["body"]["booked_net"])?;
        limits.push(json!({"binding_id":limit["binding_id"],"premium":limit["premium"]}));
    }
    check(ordered(limits)? == ordered(array(&p["limits"])?.clone())?)?;
    let mut slots = BTreeSet::new();
    for f in array(&p["families"])? {
        let matches = policies
            .iter()
            .filter(|r| {
                r["body"]["family_id"] == f["family"] && r["body"]["binding_id"] == f["binding_id"]
            })
            .collect::<Vec<_>>();
        check(matches.len() == 1)?;
        let r = matches[0];
        let b = &r["body"];
        check(slots.insert((text(&b["agreement_id"])?, text(&b["family_id"])?)))?;
        let binding = &bindings
            .iter()
            .find(|r| r["body"]["binding_id"] == f["binding_id"])
            .ok_or_else(integrity)?["body"];
        for k in ["book", "agreement_id", "roles", "assent"] {
            check(b[k] == binding[k])?
        }
        for k in [
            "binding_id",
            "evidence_required",
            "ordinary",
            "corrections",
            "allow_reversal",
        ] {
            check(b[k] == f[k])?
        }
        check(
            b["policy_version"] == p["version"]
                && b["submission_source"] == f["source"]
                && b["correction_source"] == f["correction_source"]
                && b["replacement_codes"]
                    == json!(ordered(array(&f["replacement_codes"])?.clone())?),
        )?;
        let rules=array(&f["codes"])?.iter().map(|c|if c["amount"]["kind"]=="fixed"{json!({"code":c["code"],"kind":"fixed","fixed_atoms":c["amount"]["money"]["atoms"]})}else{json!({"code":c["code"],"kind":"percentage","rate":c["amount"]["rate"]})}).collect();
        check(b["rules"] == json!(ordered(rules)?))?;
        let limit = array(&tb["limits"])?
            .iter()
            .find(|l| l["binding_id"] == f["binding_id"])
            .ok_or_else(integrity)?;
        check(
            b["max_premium_atoms"] == limit["premium"]["atoms"]
                && b["max_discount_atoms"] == limit["discount_capacity"]["atoms"]
                && b["currency"] == limit["premium"]["currency"]
                && b["scale"] == limit["premium"]["scale"],
        )?;
        if binding["book"] == "supplier" {
            records.get(&b["supplier_authorization"], "evidence")?;
            let i = &binding["supplier_invocation"];
            check(i["binding_id"] == f["binding_id"])?;
        }
    }
    Ok(())
}
pub(super) fn request(records: &Records, base: &Base, data: &Value) -> Result<o::Request> {
    check(data["target"] == base.snapshot["body"]["target"])?;
    Ok(o::Request {
        scope: core(Scope::new(
            text(&records.scope[0])?,
            text(&records.scope[1])?,
        ))?,
        id: text(&data["external_id"])?.into(),
        target: base.evaluation.event().id().into(),
        agreement: text(&data["agreement_id"])?.into(),
        family: text(&data["family_id"])?.into(),
        source: text(&data["source"])?.into(),
        occurred_at: time(&data["occurred_at"])?,
        evidence: records.documents_for(&data["evidence"])?,
        change: if data["type"] == "outcome" {
            o::Change::Claim {
                code: text(&data["code"])?.into(),
            }
        } else {
            o::Change::Correct {
                expected_revision: text(&data["expected_revision_number"])?
                    .parse()
                    .map_err(|_| integrity())?,
                replacement: data["replacement"]["code"].as_str().map(Into::into),
            }
        },
    })
}
pub(super) fn verified(records: &Records, r: &o::Request, v: &Value) -> Result<o::Verified> {
    check(
        v["target"] == records.one("target-snapshot")?["body"]["target"]
            && v["agreement_id"] == r.agreement
            && v["family_id"] == r.family
            && v["source"] == r.source,
    )?;
    Ok(o::Verified {
        scope: r.scope.clone(),
        target: r.target.clone(),
        agreement: r.agreement.clone(),
        family: r.family.clone(),
        source: r.source.clone(),
        principal: text(&v["principal"])?.into(),
        grant: records.document(&v["grant"])?,
        grant_revision: core(ledgerlab_core::domain::Revision::parse(text(
            &v["grant_revision"],
        )?))?,
        active: v["active"] == true,
        may_read: v["may_read"] == true,
        may_submit: v["may_submit"] == true,
        may_correct: v["may_correct"] == true,
        verified_evidence: records.documents_for(&v["verified_evidence"])?,
        received_at: time(&v["received_at"])?,
        accepted_at: time(&v["accepted_at"])?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_base_history_bridges_all_originals() {
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
            let records = Records::new(
                &seed.iter().map(|r| bytes(r).unwrap()).collect::<Vec<_>>(),
                json!(["synthetic", "sandbox"]),
            )
            .unwrap();
            let anchor = reference(records.one("base-acceptance").unwrap());
            decode_base(&records, &anchor).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            count += 1;
        }
        assert_eq!(count, 23);
    }
}
