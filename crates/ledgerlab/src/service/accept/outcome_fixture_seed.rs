//! Test-only creation of a fresh base with explicit contingent supplier terms.
//! Frozen files remain unchanged; these new inputs are rated by the real core.
use super::*;
use ledgerlab_core::{canonical::Domain, money::Decimal, policy::chaining as c, wire::EventKind};
pub(super) fn fresh_history() -> Value {
    let mut h: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/candidates/v2/goldens/supplier-separation.json"
    ))
    .unwrap();
    let seed = h["seed"].as_array().unwrap();
    let r = Records::new(
        &seed.iter().map(|v| bytes(v).unwrap()).collect::<Vec<_>>(),
        json!(["synthetic", "sandbox"]),
    )
    .unwrap();
    let old = b::decode_base(&r, &reference(r.one("base-acceptance").unwrap())).unwrap();
    let mut policies = old.evaluation.bundle().policies().to_vec();
    let supplier = policies
        .iter_mut()
        .find(|p| p.binding.book == c::Book::Supplier)
        .unwrap();
    supplier.binding.event_types.push(EventKind::Acquired);
    supplier.binding.outcome = Some(c::OutcomeTerms {
        source: "urn:synthetic:outcome".into(),
        window_us: 86_400_000_000,
        report_grace_us: 86_400_000_000,
        claim_namespace: "supplier-rebate".into(),
    });
    supplier.rules.push(c::Rule {
        id: "contingent-held".into(),
        on: EventKind::Acquired,
        component: "contingent-held".into(),
        when: vec![],
        matcher: None,
        operation: c::Operation::Premium(c::Price::Fixed(Decimal::parse("0").unwrap())),
    });
    let bundle = c::Bundle::compile("USD", 2, policies).unwrap();
    let e = &old.evaluation;
    let evaluated = bundle
        .evaluate(c::Input {
            event: e.event(),
            context: e.context(),
            history: &[],
            source_authority: e.source_authority(),
            invocations: e.invocations(),
            costs: e.costs(),
            received_at: e.received_at().unwrap(),
        })
        .unwrap();
    let original = serde_json::to_value(&evaluated).unwrap();
    let material = b::projection(&original).unwrap();
    let mut source_policy = b::parse(&old.snapshot["body"]["policy_utf8"]).unwrap();
    let family = source_policy["families"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["binding_id"] == "binding-supplier")
        .unwrap();
    family["codes"].as_array_mut().unwrap().push(json!({"code":"fee","amount":{"kind":"fixed","money":{"currency":"USD","scale":2,"atoms":"2500"}}}));
    family["replacement_codes"]
        .as_array_mut()
        .unwrap()
        .push(json!("fee"));
    family["codes"] = json!(b::ordered(family["codes"].as_array().unwrap().clone()).unwrap());
    family["replacement_codes"] =
        json!(b::ordered(family["replacement_codes"].as_array().unwrap().clone()).unwrap());
    let mut terms = source_policy.clone();
    terms.as_object_mut().unwrap().remove("document");
    let dh = canonical::digest(Domain::Document, &json!(["policy", 1, terms])).unwrap();
    let doc_id = format!("doc_{}", &dh[7..]);
    source_policy["document"] = json!(doc_id);
    let seed = h["seed"].as_array_mut().unwrap();
    for row in seed.iter_mut() {
        let kind = row["kind"].as_str().unwrap().to_owned();
        let b = &mut row["body"];
        match kind.as_str() {
            "binding-snapshot" if b["binding_id"] == "binding-supplier" => {
                let binding = &material["bundle"]["policies"][1]["binding"];
                b["binding_utf8"] = json!(String::from_utf8(bytes(binding).unwrap()).unwrap());
                b["event_types"] =
                    json!(b::ordered(binding["event_types"].as_array().unwrap().clone()).unwrap());
                b["outcome"] = binding["outcome"].clone();
            }
            "base-evaluation" => {
                b["original_evaluation_utf8"] =
                    json!(String::from_utf8(bytes(&original).unwrap()).unwrap());
                b["evaluation_utf8"] = json!(String::from_utf8(bytes(&material).unwrap()).unwrap());
            }
            "target-snapshot" => {
                b["policy_utf8"] =
                    json!(String::from_utf8(bytes(&source_policy).unwrap()).unwrap());
                b["policy_document"] = json!(doc_id);
                b["verified_policy_document"] = json!(doc_id);
                b["policy_document_hash"] = json!(dh);
            }
            "policy-snapshot" if b["binding_id"] == "binding-supplier" => {
                b["rules"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"code":"fee","kind":"fixed","fixed_atoms":"2500"}));
                b["rules"] = json!(b::ordered(b["rules"].as_array().unwrap().clone()).unwrap());
                b["replacement_codes"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("fee"));
                b["replacement_codes"] =
                    json!(b::ordered(b["replacement_codes"].as_array().unwrap().clone()).unwrap());
            }
            "evidence" if b["purpose"] == "policy" => {
                b["document_id"] = json!(doc_id);
                b["document_hash"] = json!(dh);
                b["utf8"] = json!(String::from_utf8(bytes(&terms).unwrap()).unwrap());
            }
            _ => {}
        }
    }
    // Rehash this newly authored fixture only. No production append accepts a
    // rewritten historical root; runtime still validates original trusted roots.
    let mut substitutions = BTreeMap::new();
    fn replace(v: &mut Value, m: &BTreeMap<String, Value>) {
        match v {
            Value::String(s) => {
                if let Some(x) = m.get(s) {
                    *v = x.clone();
                } else if s.starts_with('{') || s.starts_with('[') {
                    if let Ok(mut inner) = serde_json::from_str::<Value>(s) {
                        replace(&mut inner, m);
                        *s = String::from_utf8(bytes(&inner).unwrap()).unwrap();
                    }
                }
            }
            Value::Object(o) => {
                for x in o.values_mut() {
                    replace(x, m)
                }
            }
            Value::Array(a) => {
                for x in a {
                    replace(x, m)
                }
            }
            _ => {}
        }
    }
    fn order_sets(v: &mut Value) {
        match v {
            Value::Object(o) => {
                for (k, v) in o.iter_mut() {
                    order_sets(v);
                    if [
                        "families",
                        "bindings",
                        "identity_mappings",
                        "postings",
                        "rules",
                        "replacement_codes",
                        "sources",
                        "event_types",
                        "correction_sources",
                        "allowed_modifiers",
                        "verified_assents",
                        "verified_offers",
                        "verified_delegations",
                        "policy_evidence",
                    ]
                    .contains(&k.as_str())
                    {
                        if let Some(a) = v.as_array() {
                            *v = json!(b::ordered(a.clone()).unwrap());
                        }
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    order_sets(v)
                }
            }
            _ => {}
        }
    }
    for _ in 0..64 {
        let before = bytes(&json!(seed)).unwrap();
        for r in seed.iter_mut() {
            replace(&mut r["body"], &substitutions);
            order_sets(&mut r["body"]);
        }
        let members = b::members(
            &seed
                .iter()
                .filter(|r| r["kind"] != "base-acceptance")
                .cloned()
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let acceptance = seed
            .iter_mut()
            .find(|r| r["kind"] == "base-acceptance")
            .unwrap();
        let ab = &mut acceptance["body"];
        ab["members"] = json!(members);
        let receipt = json!({"schema":"ledger-base-receipt/2-candidate.4","target":ab["target"],"base_evaluation":ab["base_evaluation"],"target_snapshot":ab["target_snapshot"],"accepted_at":ab["accepted_at"],"membership_hash":hash("base-membership",&ab["members"]).unwrap()});
        ab["original_receipt_utf8"] = json!(String::from_utf8(bytes(&receipt).unwrap()).unwrap());
        for r in seed.iter_mut() {
            let next = b::row(text(&r["kind"]).unwrap(), &r["scope"], r["body"].clone()).unwrap();
            for k in ["id", "content_hash"] {
                if r[k] != next[k] {
                    substitutions.insert(text(&r[k]).unwrap().into(), next[k].clone());
                }
            }
            *r = next;
        }
        if bytes(&json!(seed)).unwrap() == before {
            break;
        }
    }
    seed.sort_by(|a, b| {
        (text(&a["kind"]).unwrap(), bytes(&a["id"]).unwrap())
            .cmp(&(text(&b["kind"]).unwrap(), bytes(&b["id"]).unwrap()))
    });
    // The same frozen request shape, with a newly authorized positive outcome.
    let ds = h["decisions"].as_array_mut().unwrap();
    for d in ds {
        for r in d["records"].as_array_mut().unwrap() {
            if r["kind"] == "event"
                && r["body"]["data"]["agreement_id"] == "agreement-supplier"
                && r["body"]["data"]["type"] == "outcome"
            {
                r["body"]["data"]["code"] = json!("fee");
            }
        }
    }
    h
}
