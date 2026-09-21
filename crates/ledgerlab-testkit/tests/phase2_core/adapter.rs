//! Test-only representation bridge. No pricing, rounding, cap, share or reversal
//! equations live here. `expected` is never supplied to input construction.
use ledgerlab_core::{
    canonical::{self, Domain},
    domain::{self, Event, Revision, Roles, Scope, Timestamp},
    money::{Decimal, Money},
    policy::chaining::*,
    wire::{EventKind, Relation},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap().into()
}
fn strings(v: &Value) -> Vec<String> {
    serde_json::from_value(v.clone()).unwrap()
}
fn decimal(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}
fn kind(s: &str) -> EventKind {
    serde_json::from_value(json!(s)).unwrap()
}
fn component(k: &str) -> &'static str {
    match k {
        "content.generated" => "generation",
        "tool.optimized" => "optimization",
        "content.published" => "publication",
        "tool.completed" => "call",
        _ => panic!("not a work kind: {k}"),
    }
}
fn document(alias: &str) -> String {
    // Opaque synthetic reference, not a claim to encode an accepted document.
    canonical::identity(Domain::Document, &json!(["comparison-alias", 1, alias])).unwrap()
}
fn time(value: &str) -> Timestamp {
    let micros: u64 = value.parse().unwrap();
    assert!(micros < 1_000_000, "fixtures use relative microseconds");
    Timestamp::parse(&format!("1970-01-01T00:00:00.{micros:06}Z")).unwrap()
}
fn money(cfg: &Value, atoms: i128) -> Money {
    Money::new(
        &text(cfg, "currency"),
        cfg["scale"].as_u64().unwrap() as u8,
        atoms,
    )
    .unwrap()
}
fn fixed(cfg: &Value, atoms: &str) -> Decimal {
    // Representation conversion only: atom digits -> major-unit decimal digits.
    assert!(!atoms.starts_with('-'));
    let scale = cfg["scale"].as_u64().unwrap() as usize;
    let mut digits = format!("{atoms:0>width$}", width = scale + 1);
    if scale != 0 {
        digits.insert(digits.len() - scale, '.');
    }
    decimal(&digits)
}
fn roles(v: &Value) -> Roles {
    serde_json::from_value(v.clone()).unwrap()
}
fn binding(cfg: &Value, v: &Value, book: Book) -> Binding {
    Binding {
        id: text(v, "id"),
        agreement: v["agreement"]
            .as_str()
            .unwrap_or("model-observation")
            .into(),
        book,
        roles: roles(&v["roles"]),
        assent: document(v["assent_ref"].as_str().unwrap_or("assent-retail")),
        offer: v["offer_ref"].as_str().map(document),
        sources: cfg["grants"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|g| g["source"] != cfg["quality_source"])
            .map(|g| text(g, "source"))
            .collect(),
        event_types: vec![
            EventKind::Generated,
            EventKind::Optimized,
            EventKind::Published,
            EventKind::ToolCompleted,
            EventKind::Acquired,
        ],
        unit: "call".into(),
        // Story fixtures omit standing retail quantity ceilings. This explicit
        // synthetic ceiling accommodates the submitted quantities; no real assent.
        maximum_quantity: decimal("100"),
        maximum_exposure: v["invocation"]["maximum_atoms"]
            .as_str()
            .map(|s| money(cfg, s.parse().unwrap())),
        outcome: Some(OutcomeTerms {
            source: text(cfg, "outcome_source"),
            window_us: text(cfg, "outcome_window").parse().unwrap(),
            report_grace_us: text(cfg, "report_grace").parse().unwrap(),
            claim_namespace: "sale".into(),
        }),
        correction_sources: strings(&v["correction_sources"]),
        allowed_modifiers: vec![],
        allocation_view: false,
    }
}
fn rule(on: EventKind, name: &str, operation: Operation) -> Rule {
    Rule {
        id: name.into(),
        on,
        component: name.into(),
        when: vec![],
        matcher: None,
        operation,
    }
}
fn acquisition_match(k: EventKind) -> Matcher {
    match k {
        EventKind::Optimized => Matcher::AcquisitionOptimization,
        EventKind::Published => Matcher::Direct {
            relation: Relation::AttributedTo,
            target: k,
        },
        _ => panic!("fixture has no supported contingent path for {k:?}"),
    }
}
pub fn compile(cfg: &Value) -> ledgerlab_core::Result<Bundle> {
    // Resolve the story's one tariff for its pinned tier into concrete rules.
    // Do not implement an independent runtime preset or change wire DSL enums.
    let mut retail = Policy {
        binding: binding(cfg, &cfg["retail"], Book::Retail),
        rules: vec![],
    };
    for rate in cfg["rates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["tier"] == cfg["tier"])
    {
        let k = text(rate, "kind");
        let base = format!("{}.base", component(&k));
        retail.rules.push(rule(
            kind(&k),
            &base,
            Operation::Base(Price::Unit {
                rate: decimal(&text(rate, "unit_price")),
                unit: "call".into(),
            }),
        ));
        if !decimal(&text(rate, "discount_percent")).is_zero() {
            retail.rules.push(rule(
                kind(&k),
                &format!("{}.discount", component(&k)),
                Operation::Discount {
                    amount: DiscountAmount::Percent(decimal(&text(rate, "discount_percent"))),
                    component: base,
                    mode: DiscountMode::Additive,
                },
            ));
        }
    }
    let mut premium = rule(
        EventKind::Acquired,
        "acquisition.premium",
        Operation::Premium(Price::Fixed(fixed(cfg, &text(cfg, "premium_atoms")))),
    );
    premium.matcher = Some(acquisition_match(EventKind::Published));
    retail.rules.push(premium);
    if let Some(stage) = cfg.get("stage") {
        retail.rules.push(rule(
            EventKind::Acquired,
            "acquisition.cap-credit",
            Operation::Cap {
                ceiling: fixed(cfg, &text(stage, "cap_atoms")),
                stage: text(stage, "id"),
                component: "acquisition.premium".into(),
            },
        ));
    }
    let mut policies = vec![retail];
    for supplier in cfg["suppliers"].as_array().unwrap() {
        let k = kind(&text(supplier, "kind"));
        let name = text(supplier, "component");
        let mut rules = vec![rule(
            k,
            &format!("{name}.base"),
            Operation::Base(Price::Fixed(fixed(cfg, &text(supplier, "fee_atoms")))),
        )];
        if text(supplier, "premium_atoms") != "0" {
            let mut r = rule(
                EventKind::Acquired,
                &format!("{name}.premium"),
                Operation::Premium(Price::Fixed(fixed(cfg, &text(supplier, "premium_atoms")))),
            );
            r.matcher = Some(acquisition_match(k));
            rules.push(r);
        }
        if !decimal(&text(supplier, "share_percent")).is_zero() {
            let mut r = rule(
                EventKind::Acquired,
                &format!("{name}.share"),
                Operation::Share {
                    percent: decimal(&text(supplier, "share_percent")),
                    ceiling: fixed(cfg, &text(supplier, "share_ceiling_atoms")),
                    retail_component: "acquisition.premium".into(),
                },
            );
            r.matcher = Some(acquisition_match(k));
            rules.push(r);
        }
        policies.push(Policy {
            binding: binding(cfg, supplier, Book::Supplier),
            rules,
        });
    }
    let unknown = json!({"id":"model-cost-v1", "roles":cfg["retail"]["roles"],
        "correction_sources":cfg["retail"]["correction_sources"]});
    policies.push(Policy {
        binding: binding(
            cfg,
            cfg.get("model_cost").unwrap_or(&unknown),
            Book::CostObservation,
        ),
        rules: vec![rule(
            EventKind::Generated,
            "model.observation",
            Operation::ObserveCost,
        )],
    });
    Bundle::compile(
        &text(cfg, "currency"),
        cfg["scale"].as_u64().unwrap() as u8,
        policies,
    )
}

pub struct Adapter {
    cfg: Value,
    events: Value,
    bundle: Bundle,
    context: Context,
    pub history: Vec<Evaluation>,
}
impl Adapter {
    pub fn new(cfg: Value, events: Value) -> ledgerlab_core::Result<Self> {
        let bundle = compile(&cfg)?;
        let stage = cfg.get("stage").map(|s| Stage {
            id: text(s, "id"),
            closure_claim_namespace: "sale".into(),
            expected: strings(&s["expected"])
                .iter()
                .map(|id| {
                    let e = &events[id];
                    ExpectedOperation {
                        source: text(e, "source"),
                        operation_id: text(e, "operation"),
                        kind: kind(&text(e, "kind")),
                        retail_components: bundle
                            .policies()
                            .iter()
                            .find(|p| p.binding.book == Book::Retail)
                            .unwrap()
                            .rules
                            .iter()
                            .filter(|r| r.on == kind(&text(e, "kind")))
                            .map(|r| r.component.clone())
                            .collect(),
                    }
                })
                .collect(),
        });
        let context = Context {
            document: document(&text(&cfg, "id")),
            customer: text(&cfg, "customer"),
            funding: if cfg["funding"] == "byok" {
                Funding::Byok
            } else {
                Funding::Platform
            },
            tier: Some(text(&cfg, "tier")),
            priority: None,
            stage,
        };
        Ok(Self {
            cfg,
            events,
            bundle,
            context,
            history: vec![],
        })
    }
    fn event_id(&self, alias: &str) -> String {
        let e = &self.events[alias];
        canonical::identity(
            Domain::Event,
            &json!(["comparison", "sandbox", e["source"], e["id"]]),
        )
        .unwrap()
    }
    fn event(&self, e: &Value) -> ledgerlab_core::Result<Event> {
        let k = kind(&text(e, "kind"));
        let mut wire = json!({"schema":"ledger-event/1", "id":e["id"], "type":e["kind"],
            "source":e["source"], "operation_id":e["operation"], "customer":e["customer"],
            "chain":e["chain"], "occurred_at":time(&text(e,"occurred")),
            "evidence":strings(&e["evidence"]).iter().map(|s|document(s)).collect::<Vec<_>>()});
        if k.is_work() {
            wire["quantity"] = e["quantity"].clone();
            wire["status"] = e["status"].clone();
            wire["unit"] = json!("call");
        }
        if k == EventKind::Reversal {
            wire["targets"] = json!(strings(&e["targets"])
                .iter()
                .map(|s| self.event_id(s))
                .collect::<Vec<_>>());
            wire["reason"] = json!("synthetic comparison reversal");
        } else {
            wire["links"] = json!(e["links"].as_array().unwrap().iter().map(|l|
                json!({"relation":l["relation"],"from":{"source":l["source"],"id":l["event"]}})).collect::<Vec<_>>());
        }
        if k == EventKind::Acquired {
            wire["claim_id"] = e["claim"].clone();
        }
        if let Some(i) = e.get("invocation") {
            wire["invocation_id"] = i.clone();
            // The proposal omits binding_id on events; its trusted invocation
            // supplies the pinned binding. This mapping is not authority discovery.
            wire["binding_id"] = self.cfg["suppliers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["invocation"]["id"] == *i)
                .unwrap()["id"]
                .clone();
        }
        domain::normalize(
            &serde_json::to_vec(&wire).unwrap(),
            Scope::new("comparison", "sandbox")?,
            &text(e, "source"),
        )?
        .resolve(None)
    }
    pub fn evaluate(&self, alias: &str, received: &str) -> ledgerlab_core::Result<Evaluation> {
        let e = &self.events[alias];
        let event = self.event(e)?;
        let g = self.cfg["grants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["source"] == e["source"])
            .unwrap();
        let authority = SourceAuthority {
            source: text(g, "source"),
            grant: document(&text(g, "id")),
            revision: Revision::new(1)?,
            active: g["active"].as_bool().unwrap(),
            event_types: serde_json::from_value(g["kinds"].clone()).unwrap(),
            relations: serde_json::from_value(g["relations"].clone()).unwrap(),
        };
        if event.dto().kind == EventKind::Reversal {
            return reverse(&event, &self.history, &authority);
        }
        let invocations = self.cfg["suppliers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let i = &s["invocation"];
                let max: i128 = text(i, "maximum_atoms").parse().unwrap();
                // Feed back the core's actual consumption/release proposals as locked
                // input for the next pure call. No expected-value or oracle arithmetic.
                let spent: i128 = self
                    .history
                    .iter()
                    .flat_map(|d| d.consumptions())
                    .filter(|c| c.invocation_id == text(i, "id"))
                    .map(|c| c.consume.atoms() + c.release.atoms())
                    .sum();
                let completion = self
                    .history
                    .iter()
                    .find(|d| d.event().dto().id == text(i, "event"));
                let original = &self.events[text(i, "event")];
                Invocation {
                    id: text(i, "id"),
                    binding_id: text(i, "binding"),
                    operation_id: text(i, "operation"),
                    chain: text(i, "chain"),
                    customer: text(i, "customer"),
                    source: text(s, "source"),
                    unit: "call".into(),
                    maximum_quantity: decimal(&text(i, "maximum_quantity")),
                    maximum_exposure: money(&self.cfg, max),
                    held: money(&self.cfg, max - spent),
                    authorized_at: time(&text(i, "authorized_at")),
                    start_before: time(&text(i, "start_before")),
                    attested_start: time(&text(original, "occurred")),
                    // Fixture outcome window starts at the publication and is <1ms;
                    // explicit synthetic invocation deadline never narrows that window.
                    outcome_deadline: Some(time("999999")),
                    completion_event: completion.map(|d| d.event().id().into()),
                }
            })
            .collect::<Vec<_>>();
        let costs = self
            .cfg
            .get("model_cost")
            .map(|c| CostEvidence {
                binding_id: text(c, "id"),
                event_id: event.id().into(),
                document: document(&text(c, "evidence_ref")),
                amount: money(&self.cfg, text(c, "atoms").parse().unwrap()),
            })
            .into_iter()
            .collect::<Vec<_>>();
        self.bundle.evaluate(Input {
            event: &event,
            context: &self.context,
            history: &self.history,
            source_authority: &authority,
            invocations: &invocations,
            costs: &costs,
            received_at: &time(received),
        })
    }
    pub fn posting_aliases(&self, result: &Evaluation) -> BTreeMap<String, String> {
        let mut aliases = BTreeMap::new();
        for d in self.history.iter().chain([result]) {
            for a in d.actions() {
                let alias = if let Some(original) = a.reverses() {
                    format!("{}/reverse/{}", d.event().dto().id, aliases[original])
                } else {
                    format!("{}/{}", d.event().dto().id, a.component())
                };
                aliases.insert(a.id().into(), alias);
            }
        }
        aliases
    }
    pub fn projection(&self, d: &Evaluation) -> Value {
        let aliases = self.posting_aliases(d);
        let refs = |ids: &[String]| {
            let mut v: Vec<_> = ids.iter().map(|id| aliases[id].clone()).collect();
            v.sort();
            v
        };
        let postings = d.actions().iter().map(|a| {
            assert_eq!(a.amount().currency(), text(&self.cfg, "currency"));
            assert_eq!(u64::from(a.amount().scale()), self.cfg["scale"].as_u64().unwrap());
            let mut p = json!({"id":aliases[a.id()],"component":a.component(),"book":book(a.book()),
                "kind":action_kind(a.kind()),"atoms":a.amount().atoms().to_string(),
                "binding":a.binding().id,"roles":a.binding().roles,"inputs":refs(a.inputs())});
            if let Some(r) = a.reverses() { p["reverses"] = json!(aliases[r]); }
            p
        }).collect::<Vec<_>>();
        let explanations = d.explanations().iter().map(|x| {
            let component = x.rule_id.clone().unwrap_or_else(||
                x.action_ids.first().map(|id|d.actions().iter().find(|a|a.id()==id).unwrap().component().into())
                    .unwrap_or("completion".into()));
            let mut v = json!({"component":component,"code":if x.code == "COST_OBSERVED" {"BASE_APPLIED"} else {x.code}});
            if let Some(n) = &x.unrounded { v["unrounded"] = serde_json::to_value(n).unwrap(); }
            if let Some(n) = x.rounded { v["rounded_atoms"] = json!(n.to_string()); }
            if let Some(b) = &x.basis {
                let b = serde_json::to_value(b).unwrap();
                assert_eq!(b["denominator"],"1");
                v["basis_atoms"] = b["numerator"].clone();
            }
            // The oracle omits inactive-cap numeric diagnostics; assert the
            // richer core values separately before projecting their common part.
            if x.code == "CAP_NOT_BINDING" {
                assert_eq!(x.rounded,Some(0));
                assert_eq!(serde_json::to_value(x.unrounded.as_ref().unwrap()).unwrap(),json!({"numerator":"0","denominator":"1"}));
                v.as_object_mut().unwrap().retain(|k,_|k=="component" || k=="code");
            }
            v
        }).collect::<Vec<_>>();
        let obligations = d
            .deltas()
            .iter()
            .map(|g| {
                assert_eq!(g.amount.currency(), text(&self.cfg, "currency"));
                assert_eq!(
                    u64::from(g.amount.scale()),
                    self.cfg["scale"].as_u64().unwrap()
                );
                let first = d.actions().iter().find(|a| a.id() == g.actions[0]).unwrap();
                json!({"binding":first.binding().id,"book":book(g.book),"roles":g.roles,
                "atoms":g.amount.atoms().to_string(),"posting_ids":refs(&g.actions)})
            })
            .collect::<Vec<_>>();
        let inputs = d
            .actions()
            .iter()
            .flat_map(|a| a.inputs())
            .chain(d.explanations().iter().flat_map(|x| &x.inputs));
        let depends: BTreeSet<_> = inputs
            .filter_map(|id| {
                self.history
                    .iter()
                    .find(|prior| {
                        prior.event().id() != d.event().id()
                            && prior.actions().iter().any(|a| a.id() == id)
                    })
                    .map(|p| p.event().dto().id.clone())
            })
            .collect();
        json!({"id":d.event().dto().id,"postings":sorted(postings),
            "explanations":sorted(explanations),"obligations":sorted(obligations),"depends_on":depends})
    }
    pub fn state(&self) -> Value {
        let mut consumed: BTreeMap<String, i128> = self.cfg["suppliers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (text(&s["invocation"], "id"), 0))
            .collect();
        let mut totals =
            BTreeMap::from([("retail", 0i128), ("supplier", 0), ("cost_observation", 0)]);
        let mut reversed = BTreeSet::new();
        for d in &self.history {
            for c in d.consumptions() {
                *consumed.get_mut(&c.invocation_id).unwrap() += c.consume.atoms();
            }
            for a in d.actions() {
                *totals.get_mut(book(a.book())).unwrap() += a.amount().atoms();
                if let Some(id) = a.reverses() {
                    reversed.insert(
                        self.history
                            .iter()
                            .find(|p| p.actions().iter().any(|a| a.id() == id))
                            .unwrap()
                            .event()
                            .dto()
                            .id
                            .clone(),
                    );
                }
            }
        }
        json!({"revision":self.history.len().to_string(), "closed":self.history.iter().any(|d|d.closed_stage().is_some()),
            "reversed":reversed,"consumed":consumed.into_iter().map(|(k,v)|(k,v.to_string())).collect::<BTreeMap<_,_>>(),
            "totals":totals.into_iter().map(|(k,v)|(k,v.to_string())).collect::<BTreeMap<_,_>>()})
    }
}
pub fn sorted(mut rows: Vec<Value>) -> Vec<Value> {
    rows.sort_by_key(Value::to_string);
    rows
}
fn book(b: Book) -> &'static str {
    match b {
        Book::Retail => "retail",
        Book::Supplier => "supplier",
        Book::CostObservation => "cost_observation",
        Book::Allocation => "allocation",
    }
}
fn action_kind(k: ActionKind) -> &'static str {
    match k {
        ActionKind::Charge => "charge",
        ActionKind::Cost => "cost",
        ActionKind::Premium => "premium",
        ActionKind::Discount => "discount",
        ActionKind::Credit => "credit",
        ActionKind::Share => "share",
        ActionKind::Allocation => "allocation",
        ActionKind::Reversal => "reversal",
    }
}
