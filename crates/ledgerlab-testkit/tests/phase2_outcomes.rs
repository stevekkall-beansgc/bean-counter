//! Approved outcomes: compare the entire journal after EVERY attempt with a
//! separately written Python Fraction oracle. This adapter performs no pricing.
use ledgerlab_core::{
    canonical::{self, Domain},
    domain::{normalize, Event, Revision, Roles, Scope, Timestamp},
    money::{Decimal, ExactRatio, Money},
    policy::chaining::{self as c, outcomes as o},
    wire::EventKind as K,
};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn s<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or(default)
}
fn b(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(true)
}
fn n(v: &Value, key: &str, default: u64) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(default)
}
fn time(n: u64) -> Timestamp {
    Timestamp::parse(&format!("1970-01-01T00:00:00.{n:06}Z")).unwrap()
}
fn doc(alias: &str) -> String {
    canonical::identity(Domain::Document, &json!(["outcome-test", alias])).unwrap()
}
fn money(n: &str) -> Money {
    Money::new("USD", 2, n.parse().unwrap()).unwrap()
}
fn decimal_atoms(n: &str) -> Decimal {
    let mut value = format!("{n:0>3}");
    value.insert(value.len() - 2, '.');
    Decimal::parse(&value).unwrap()
}
fn roles(v: &Value) -> Roles {
    let names: Vec<_> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    Roles::new(names.try_into().unwrap(), None).unwrap()
}
fn binding(name: &str, book: c::Book, story: &Value) -> c::Binding {
    c::Binding {
        id: name.into(),
        agreement: name.into(),
        book,
        roles: roles(&story["roles"][name]),
        assent: doc(name),
        offer: (book == c::Book::Supplier).then(|| doc("offer")),
        sources: vec!["urn:work".into(), "urn:outcome".into()],
        event_types: vec![K::Generated, K::ToolCompleted, K::Acquired],
        unit: "call".into(),
        maximum_quantity: Decimal::parse("1").unwrap(),
        maximum_exposure: (book == c::Book::Supplier).then(|| money("1000")),
        outcome: Some(c::OutcomeTerms {
            source: "urn:outcome".into(),
            window_us: 1000,
            report_grace_us: 10,
            claim_namespace: "sale".into(),
        }),
        correction_sources: vec!["urn:correction".into()],
        allowed_modifiers: vec![],
        allocation_view: false,
    }
}
fn rule(on: K, component: &str, operation: c::Operation) -> c::Rule {
    c::Rule {
        id: component.into(),
        on,
        component: component.into(),
        when: vec![],
        matcher: None,
        operation,
    }
}
fn base(story: &Value) -> c::Evaluation {
    let input = &story["base"];
    let kind = if b(input, "supplier_path") {
        K::ToolCompleted
    } else {
        K::Generated
    };
    let mut retail = c::Policy {
        binding: binding("retail", c::Book::Retail, story),
        rules: vec![
            rule(
                kind,
                "base",
                c::Operation::Base(c::Price::Fixed(decimal_atoms(s(input, "atoms", "100")))),
            ),
            rule(
                kind,
                "discount",
                c::Operation::Discount {
                    amount: c::DiscountAmount::Fixed(decimal_atoms(s(
                        input,
                        "booking_discount",
                        "20",
                    ))),
                    component: "base".into(),
                    mode: c::DiscountMode::Additive,
                },
            ),
        ],
    };
    if input["capped"] == true {
        retail.rules.push(rule(
            K::Acquired,
            "premium",
            c::Operation::Premium(c::Price::Fixed(Decimal::parse("1").unwrap())),
        ));
        retail.rules.push(rule(
            K::Acquired,
            "cap",
            c::Operation::Cap {
                ceiling: Decimal::parse("100").unwrap(),
                stage: "stage".into(),
                component: "premium".into(),
            },
        ));
    }
    let supplier = c::Policy {
        binding: binding("supplier", c::Book::Supplier, story),
        rules: vec![rule(
            K::ToolCompleted,
            "supplier",
            c::Operation::Base(c::Price::Fixed(decimal_atoms(s(input, "supplier", "30")))),
        )],
    };
    let bundle = c::Bundle::compile("USD", 2, vec![retail, supplier]).unwrap();
    let mut wire = json!({"schema":"ledger-event/1","id":"base","source":"urn:work","operation_id":"work","type":kind,
        "customer":"customer","chain":"chain","occurred_at":time(10),"status":s(input,"status","succeeded")});
    let mut invocations = vec![];
    if kind == K::ToolCompleted {
        wire["invocation_id"] = json!("invocation");
        wire["binding_id"] = json!("supplier");
        invocations.push(c::Invocation {
            id: "invocation".into(),
            binding_id: "supplier".into(),
            operation_id: "work".into(),
            chain: "chain".into(),
            customer: "customer".into(),
            source: "urn:work".into(),
            unit: "call".into(),
            maximum_quantity: Decimal::parse("1").unwrap(),
            maximum_exposure: money("1000"),
            held: money("1000"),
            authorized_at: time(0),
            start_before: time(100),
            attested_start: time(10),
            outcome_deadline: Some(time(2000)),
            completion_event: None,
        });
    }
    let event = normalize(
        &serde_json::to_vec(&wire).unwrap(),
        Scope::new("demo", "sandbox").unwrap(),
        "urn:work",
    )
    .unwrap()
    .resolve(None)
    .unwrap();
    let context = c::Context {
        document: doc("context"),
        customer: "customer".into(),
        funding: c::Funding::Byok,
        tier: None,
        priority: None,
        stage: if input["capped"] == true {
            Some(c::Stage {
                id: "stage".into(),
                closure_claim_namespace: "sale".into(),
                expected: vec![c::ExpectedOperation {
                    source: "urn:work".into(),
                    operation_id: "work".into(),
                    kind,
                    retail_components: vec!["base".into(), "discount".into()],
                }],
            })
        } else {
            None
        },
    };
    bundle
        .evaluate(c::Input {
            event: &event,
            context: &context,
            history: &[],
            source_authority: &authority(&event),
            invocations: &invocations,
            costs: &[],
            received_at: &time(11),
        })
        .unwrap()
}
fn authority(event: &Event) -> c::SourceAuthority {
    c::SourceAuthority {
        source: event.source().into(),
        grant: doc("grant"),
        revision: Revision::new(1).unwrap(),
        active: true,
        event_types: vec![event.dto().kind],
        relations: vec![],
    }
}
fn window(v: &Value, default: [u64; 4]) -> o::Window {
    let a: Vec<u64> = v
        .as_array()
        .map(|a| a.iter().map(|n| n.as_u64().unwrap()).collect())
        .unwrap_or(default.to_vec());
    o::Window {
        starts_at: time(a[0]),
        occurs_before: time(a[1]),
        received_by: time(a[2]),
        accepted_by: time(a[3]),
    }
}
fn policy(v: &Value) -> o::Policy {
    o::Policy {
        version: s(v, "version", "v1").into(),
        document: doc(s(v, "version", "v1")),
        families: v["families"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| o::Family {
                family: s(f, "family", "success").into(),
                binding_id: s(f, "binding", "retail").into(),
                source: "urn:outcome".into(),
                correction_source: "urn:correction".into(),
                evidence_required: b(f, "evidence_required"),
                ordinary: window(&f["ordinary"], [10, 100, 110, 120]),
                corrections: window(&f["corrections"], [10, 1000, 1100, 1200]),
                codes: f["codes"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(code, a)| o::Code {
                        code: code.clone(),
                        amount: if let Some(fixed) = a.get("fixed") {
                            o::Amount::Fixed(money(fixed.as_str().unwrap()))
                        } else {
                            let p = a["percent"].as_str().unwrap();
                            let r = if let Some((numerator, denominator)) =
                                p.trim_start_matches('-').split_once('/')
                            {
                                ExactRatio::from_canonical(numerator, denominator).unwrap()
                            } else {
                                Decimal::parse(p.trim_start_matches('-')).unwrap().ratio()
                            };
                            o::Amount::Percent(if p.starts_with('-') { r.negated() } else { r })
                        },
                    })
                    .collect(),
                replacement_codes: f["replacements"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap().into()).collect())
                    .unwrap_or_else(|| f["codes"].as_object().unwrap().keys().cloned().collect()),
                allow_reversal: b(f, "allow_reversal"),
            })
            .collect(),
        limits: [("retail", "premium_limit"), ("supplier", "supplier_limit")]
            .into_iter()
            .filter_map(|(binding, k)| {
                v[k].as_str().map(|a| o::Limit {
                    binding_id: binding.into(),
                    premium: money(a),
                })
            })
            .collect(),
    }
}
fn freeze(base: &c::Evaluation, p: &Value, verified: bool) -> ledgerlab_core::Result<o::Target> {
    let policy = policy(p);
    o::Target::freeze(
        base,
        policy.clone(),
        o::TargetVerification {
            rated_final: true,
            accepted_at: time(12),
            policy_document: policy.document,
            verified_offers: vec![doc("offer")],
            verified_delegations: vec![],
            verified_assents: if verified {
                vec![doc("retail"), doc("supplier")]
            } else {
                vec![]
            },
        },
    )
}
fn names(evidence: &Value) -> Vec<String> {
    evidence
        .as_array()
        .map(|a| a.iter().map(|v| v.as_str().unwrap().into()).collect())
        .unwrap_or(vec!["proof".into()])
}
fn request(step: &Value, base: &c::Evaluation) -> o::Request {
    o::Request {
        scope: Scope::new(s(step, "tenant", "demo"), "sandbox").unwrap(),
        id: s(step, "id", "").into(),
        target: if s(step, "target", "base") == "base" {
            base.event().id().into()
        } else {
            format!("ev_{}", "f".repeat(64))
        },
        agreement: s(step, "agreement", "retail").into(),
        family: s(step, "family", "success").into(),
        source: s(
            step,
            "source",
            if step.get("revision").is_some() {
                "urn:correction"
            } else {
                "urn:outcome"
            },
        )
        .into(),
        occurred_at: time(n(step, "occurred", 20)),
        evidence: names(&step["evidence"]).iter().map(|v| doc(v)).collect(),
        change: if let Some(revision) = step.get("revision") {
            o::Change::Correct {
                expected_revision: revision.as_u64().unwrap(),
                replacement: step["replacement"].as_str().map(str::to_owned),
            }
        } else {
            o::Change::Claim {
                code: s(step, "code", "yes").into(),
            }
        },
    }
}
fn verified(step: &Value, r: &o::Request) -> o::Verified {
    o::Verified {
        scope: r.scope.clone(),
        target: if b(step, "scope_verified") {
            r.target.clone()
        } else {
            "wrong".into()
        },
        agreement: r.agreement.clone(),
        family: r.family.clone(),
        source: r.source.clone(),
        principal: "principal".into(),
        grant: doc("grant"),
        grant_revision: Revision::new(1).unwrap(),
        active: b(step, "active"),
        may_read: b(step, "may_read"),
        may_submit: b(step, "may_submit"),
        may_correct: b(step, "may_correct"),
        verified_evidence: if b(step, "evidence_verified") {
            r.evidence.clone()
        } else {
            vec![]
        },
        received_at: time(n(step, "received", 21)),
        accepted_at: time(n(step, "accepted", 22)),
    }
}
fn projected(d: &o::Decision) -> Value {
    let r = d.request();
    let change = match &r.change {
        o::Change::Claim { code } => json!({"code":code}),
        o::Change::Correct {
            expected_revision,
            replacement,
        } => json!({"revision":expected_revision,"replacement":replacement}),
    };
    let evidence: Vec<_> = r
        .evidence
        .iter()
        .map(|v| {
            assert_eq!(v, &doc("proof"));
            "proof"
        })
        .collect();
    let book = match d.binding().book {
        c::Book::Retail => "retail",
        c::Book::Supplier => "supplier",
        _ => panic!("nonpayable"),
    };
    let roles = serde_json::to_value(&d.binding().roles).unwrap();
    json!({"request":{"tenant":r.scope.tenant(),"id":r.id,"target":"base","agreement":r.agreement,"family":r.family,"source":r.source,"occurred":r.occurred_at.micros(),"evidence":evidence,"change":change},
        "key":[d.key().scope.tenant(),d.key().scope.environment(),d.key().agreement,d.key().family,"base"],"revision":d.revision(),"current":d.current().atoms().to_string(),"code":d.current_code(),
        "binding":d.binding().id,"book":book,"version":d.target().policy().version,"basis":d.target().retail_basis().atoms().to_string(),
        "roles":[roles["provider"],roles["cost_originator"],roles["bearer"],roles["payer"],roles["beneficiary"],roles["recipient"]],
        "received":d.verified().received_at.micros(),"accepted":d.verified().accepted_at.micros(),
        "postings":d.postings().iter().map(|p|json!({"atoms":p.amount.atoms().to_string(),"reverses_revision":p.reverses_revision})).collect::<Vec<_>>(),
        "explanations":d.explanations().iter().map(|x|json!({"code":x.code,"basis":x.basis.atoms().to_string(),"exact":x.exact,"rounded":x.rounded.atoms().to_string()})).collect::<Vec<_>>()})
}
#[test]
fn approved_outcomes_match_independent_oracle_after_every_submission() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/phase2-outcomes-v0/histories.json");
    let input: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let output = Command::new("python3")
        .arg("-B")
        .arg(root.join("oracle/phase2/outcomes.py"))
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let mut compared = 0;
    for (story, expected) in input["histories"].as_array().unwrap().iter().zip(oracle) {
        let name = s(story, "name", "");
        let base = base(story);
        let mut frozen = freeze(&base, &story["policy"], b(&story["base"], "verified_terms"));
        if !b(&story["base"], "final") {
            let p = policy(&story["policy"]);
            frozen = o::Target::freeze(
                &base,
                p.clone(),
                o::TargetVerification {
                    rated_final: false,
                    accepted_at: time(12),
                    policy_document: p.document,
                    verified_assents: vec![doc("retail"), doc("supplier")],
                    verified_offers: vec![doc("offer")],
                    verified_delegations: vec![],
                },
            );
        }
        assert_eq!(
            frozen.as_ref().err().map_or("FROZEN", |e| e.code),
            expected["freeze"].as_str().unwrap(),
            "{name}: freeze"
        );
        let original = frozen.ok();
        let mut targets: Vec<_> = original
            .clone()
            .filter(|_| b(story, "available"))
            .into_iter()
            .collect();
        let mut bases = vec![base.clone()];
        let mut history = vec![];
        for (i, (step, want)) in story["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["steps"].as_array().unwrap())
            .enumerate()
        {
            let before: Vec<_> = history.iter().map(projected).collect();
            let mut actual = match step["operation"].as_str() {
                Some("make_available") => {
                    targets = original.clone().into_iter().collect();
                    json!({"status":"target_available"})
                }
                Some("reverse_base") => {
                    let wire = json!({"schema":"ledger-event/1","id":"reverse-base","source":"urn:correction","operation_id":"reverse-base","type":"economic.reversal","customer":"customer","chain":"chain","targets":[base.event().id()],"reason":"correction","evidence":[doc("proof")]});
                    let event = normalize(
                        &serde_json::to_vec(&wire).unwrap(),
                        Scope::new("demo", "sandbox").unwrap(),
                        "urn:correction",
                    )
                    .unwrap()
                    .resolve(None)
                    .unwrap();
                    bases.push(c::reverse(&event, &bases, &authority(&event)).unwrap());
                    json!({"status":"base_reversed"})
                }
                _ => {
                    let r = request(step, &base);
                    // Actively attempt to supply a changed target snapshot after a
                    // claim, proving correction/other families still use the original.
                    let mut changed = story.clone();
                    if let Some(v) = step.get("current_version") {
                        changed["policy"]["version"] = v.clone();
                    }
                    if let Some(p) = step.get("current_price") {
                        changed["policy"]["families"][0]["codes"]["yes"] = json!({"fixed":p});
                    }
                    if step.get("current_roles").is_some() {
                        changed["roles"]["retail"] = json!([
                            "attacker", "attacker", "customer", "customer", "customer", "attacker"
                        ]);
                    }
                    let alternate = if changed != *story {
                        vec![freeze(&self::base(&changed), &changed["policy"], true).unwrap()]
                    } else {
                        targets.clone()
                    };
                    match o::evaluate(&r, &verified(step, &r), &alternate, &bases, &history) {
                        Ok(o::Submission::Accepted(d)) => {
                            history.push(*d);
                            json!({"status":"accepted"})
                        }
                        Ok(o::Submission::Duplicate(index)) => {
                            json!({"status":"duplicate","original":index})
                        }
                        Err(e) => json!({"status":"rejected","code":e.code}),
                    }
                }
            };
            let journal: Vec<_> = history.iter().map(projected).collect();
            assert_eq!(
                &journal[..before.len()],
                &before,
                "{name}/{i}: immutable history"
            );
            actual["journal"] = json!(journal);
            assert_eq!(&actual,want,"{name}/{i}: every result, posting, explanation, role, current revision and replay time");
            if let Some(anchor) = story["anchors"].get(i) {
                assert_eq!(
                    actual["status"], anchor["status"],
                    "{name}/{i}: hand-authored status"
                );
                if !anchor["code"].is_null() {
                    assert_eq!(actual["code"], anchor["code"]);
                }
            }
            compared += 1;
        }
        if name == "percentage-retail-net" {
            assert_eq!(history[0].current().atoms(), -20);
        }
        if name == "correction-reinstatement" {
            assert_eq!(history.last().unwrap().current().atoms(), 40);
        }
        // Retained snapshots suffice for deterministic replay, independent of
        // today's configuration. Every revision uses its own original observations.
        let mut replay = vec![];
        for d in &history {
            match o::evaluate(
                d.request(),
                d.verified(),
                std::slice::from_ref(d.target()),
                std::slice::from_ref(&base),
                &replay,
            )
            .unwrap()
            {
                o::Submission::Accepted(rebuilt) => {
                    assert_eq!(projected(&rebuilt), projected(d));
                    replay.push(*rebuilt);
                }
                _ => panic!("replay incorrectly deduplicated"),
            }
        }
    }
    println!(
        "Compared {compared} attempts across {} approved-v0 histories",
        input["histories"].as_array().unwrap().len()
    );
}

#[test]
fn outcome_percentage_arithmetic_has_one_signed_rounding() {
    // Independent small-integer oracle (no production calculations as expected).
    for basis in 0..=41_i128 {
        for rate in -100..=100_i128 {
            let exact = ExactRatio::integer(basis)
                .mul(&ExactRatio::integer(rate))
                .unwrap()
                .div(&ExactRatio::integer(100))
                .unwrap();
            let magnitude = (basis * rate).abs();
            let expected =
                (magnitude / 100 + i128::from(magnitude % 100 >= 50)) * (basis * rate).signum();
            assert_eq!(exact.round_atoms().unwrap(), expected);
        }
    }
}

#[test]
fn verified_terms_target_type_and_base_reversal_boundary_are_required() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/phase2-outcomes-v0/histories.json"
    ))
    .unwrap();
    let story = &fixture["histories"][0];
    let base = base(story);
    let target = freeze(&base, &story["policy"], true).unwrap();
    let request = request(&story["steps"][0], &base);
    let grant = verified(&story["steps"][0], &request);
    let bases = vec![base.clone()];
    let original =
        match o::evaluate(&request, &grant, std::slice::from_ref(&target), &bases, &[]).unwrap() {
            o::Submission::Accepted(d) => *d,
            _ => unreachable!(),
        };
    let wire = json!({"schema":"ledger-event/1","id":"reversal","source":"urn:correction","operation_id":"reversal","type":"economic.reversal","customer":"customer","chain":"chain","targets":[base.event().id()],"reason":"full reversal","evidence":[doc("proof")]});
    let event = normalize(
        &serde_json::to_vec(&wire).unwrap(),
        Scope::new("demo", "sandbox").unwrap(),
        "urn:correction",
    )
    .unwrap()
    .resolve(None)
    .unwrap();
    let source = authority(&event);
    assert_eq!(
        o::reverse_base(&event, &bases, std::slice::from_ref(&original), &source)
            .unwrap_err()
            .code,
        "REVERSAL_DEPENDENTS_REQUIRED"
    );
    let mut correction = request.clone();
    correction.id = "correction".into();
    correction.source = "urn:correction".into();
    correction.change = o::Change::Correct {
        expected_revision: 1,
        replacement: None,
    };
    let mut verified = grant.clone();
    verified.source = correction.source.clone();
    let replacement = match o::evaluate(
        &correction,
        &verified,
        std::slice::from_ref(&target),
        &bases,
        std::slice::from_ref(&original),
    )
    .unwrap()
    {
        o::Submission::Accepted(d) => *d,
        _ => unreachable!(),
    };
    let inverse =
        o::reverse_base(&event, &bases, &[original.clone(), replacement], &source).unwrap();
    assert_eq!(
        o::Target::freeze(
            &inverse,
            target.policy().clone(),
            target.verification().clone()
        )
        .unwrap_err()
        .code,
        "TARGET_INELIGIBLE"
    );
    assert_eq!(
        o::evaluate(
            &correction,
            &verified,
            std::slice::from_ref(&target),
            &bases,
            &[original.clone(), original.clone()]
        )
        .unwrap_err()
        .code,
        "HISTORY_CONFLICT"
    );
    for field in ["active", "may_read", "may_submit"] {
        let mut v = grant.clone();
        match field {
            "active" => v.active = false,
            "may_read" => v.may_read = false,
            _ => v.may_submit = false,
        }
        assert_eq!(
            o::evaluate(&request, &v, std::slice::from_ref(&target), &bases, &[])
                .unwrap_err()
                .code,
            "OUTCOME_AUTHORITY"
        );
    }
    let mut v = grant.clone();
    v.principal.clear();
    assert!(o::evaluate(&request, &v, std::slice::from_ref(&target), &bases, &[]).is_err());
    let mut v = grant.clone();
    v.grant.clear();
    assert!(o::evaluate(&request, &v, std::slice::from_ref(&target), &bases, &[]).is_err());
    let supplier_story = fixture["histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "supplier-separation")
        .unwrap();
    let supplier_target = freeze(&base, &supplier_story["policy"], true).unwrap();
    let mut v = supplier_target.verification().clone();
    v.verified_offers.clear();
    assert_eq!(
        o::Target::freeze(&base, supplier_target.policy().clone(), v)
            .unwrap_err()
            .code,
        "TERMS_NOT_VERIFIED"
    );
    let mut p = target.policy().clone();
    p.families[0].source = "urn:unaccepted".into();
    assert_eq!(
        o::Target::freeze(&base, p, target.verification().clone())
            .unwrap_err()
            .code,
        "OUTCOME_AUTHORITY"
    );
    let mut p = target.policy().clone();
    p.families.push(p.families[0].clone());
    assert_eq!(
        o::Target::freeze(&base, p, target.verification().clone())
            .unwrap_err()
            .code,
        "POLICY_AMBIGUOUS_MATCH"
    );
    let mut p = target.policy().clone();
    p.families[0].ordinary.accepted_by = time(10);
    assert_eq!(
        o::Target::freeze(&base, p, target.verification().clone())
            .unwrap_err()
            .code,
        "OUTCOME_WINDOW"
    );
}
