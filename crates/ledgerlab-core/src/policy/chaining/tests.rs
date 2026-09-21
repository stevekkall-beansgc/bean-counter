use super::*;
use crate::domain::{normalize, Event, Revision, Roles, Scope, Timestamp};
use crate::money::{Decimal, Money};
use crate::wire::{EventKind as K, Relation as R};
use serde_json::{json, Value};

fn d(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}
fn money(n: i128) -> Money {
    Money::new("USD", 2, n).unwrap()
}
fn doc() -> String {
    format!("doc_{}", "a".repeat(64))
}
fn time(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}
fn sources() -> Vec<String> {
    ["urn:host", "urn:optimizer", "urn:publisher", "urn:outcome"]
        .map(str::to_owned)
        .to_vec()
}
fn binding(id: &str, book: Book) -> Binding {
    let roles = match id {
        "optimizer" => Roles::new(
            [
                "optimizer-o",
                "optimizer-o",
                "host-h",
                "host-h",
                "brand-b",
                "optimizer-o",
            ],
            None,
        ),
        "publisher" => Roles::new(
            [
                "publisher-p",
                "publisher-p",
                "host-h",
                "host-h",
                "brand-b",
                "publisher-p",
            ],
            None,
        ),
        "model" => Roles::new(
            [
                "model-supplier",
                "model-supplier",
                "host-h",
                "host-h",
                "brand-b",
                "model-supplier",
            ],
            None,
        ),
        _ => Roles::new(
            [
                "host-h", "host-h", "agency-a", "parent-q", "brand-b", "host-h",
            ],
            Some(doc()),
        ),
    }
    .unwrap();
    Binding {
        id: id.into(),
        agreement: id.into(),
        book,
        roles,
        assent: doc(),
        offer: (book == Book::Supplier).then(doc),
        sources: sources(),
        event_types: vec![
            K::Generated,
            K::Optimized,
            K::Published,
            K::ToolCompleted,
            K::Acquired,
        ],
        unit: "call".into(),
        maximum_quantity: d("100"),
        maximum_exposure: (book == Book::Supplier).then(|| money(100)),
        outcome: Some(OutcomeTerms {
            source: "urn:outcome".into(),
            window_us: 2 * 86_400_000_000,
            report_grace_us: 86_400_000_000,
            claim_namespace: "sales".into(),
        }),
        correction_sources: vec!["urn:host".into()],
        allowed_modifiers: vec![],
        allocation_view: false,
    }
}
fn rule(on: K, component: &str, operation: Operation) -> Rule {
    Rule {
        id: component.into(),
        on,
        component: component.into(),
        when: vec![],
        matcher: None,
        operation,
    }
}
fn base(on: K, component: &str, amount: &str) -> Rule {
    rule(on, component, Operation::Base(Price::Fixed(d(amount))))
}
fn discount(on: K, name: &str, component: &str, percent: &str, mode: DiscountMode) -> Rule {
    rule(
        on,
        name,
        Operation::Discount {
            amount: DiscountAmount::Percent(d(percent)),
            component: component.into(),
            mode,
        },
    )
}
fn retail(rules: Vec<Rule>) -> Bundle {
    Bundle::compile(
        "USD",
        2,
        vec![Policy {
            binding: binding("retail", Book::Retail),
            rules,
        }],
    )
    .unwrap()
}
fn context() -> Context {
    Context {
        document: doc(),
        customer: "agency-a".into(),
        funding: Funding::Platform,
        tier: Some("enterprise".into()),
        priority: Some(true),
        stage: None,
    }
}
fn event(id: &str, kind: K, source: &str, extra: Value) -> Event {
    let mut value = json!({"schema":"ledger-event/1", "id":id, "operation_id":id, "type":kind, "source":source, "customer":"agency-a", "chain":"campaign", "occurred_at":"2026-09-20T14:00:00Z"});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    normalize(
        &serde_json::to_vec(&value).unwrap(),
        Scope::new("demo", "sandbox").unwrap(),
        source,
    )
    .unwrap()
    .resolve(Some("campaign"))
    .unwrap()
}
fn link(relation: R, source: &str, id: &str) -> Value {
    json!({"relation":relation,"from":{"source":source,"id":id}})
}
fn generated() -> Event {
    event("g2", K::Generated, "urn:host", json!({}))
}
fn published() -> Event {
    event(
        "p2",
        K::Published,
        "urn:publisher",
        json!({"links":[link(R::PublishedAs,"urn:host","g2")]}),
    )
}
fn acquired(id: &str, claim: &str) -> Event {
    event(
        id,
        K::Acquired,
        "urn:outcome",
        json!({"claim_id":claim,"occurred_at":"2026-09-21T14:00:00Z","evidence":[doc()],"links":[link(R::AttributedTo,"urn:publisher","p2")]}),
    )
}
fn authority(event: &Event) -> SourceAuthority {
    SourceAuthority {
        source: event.source().into(),
        grant: doc(),
        revision: Revision::new(1).unwrap(),
        active: true,
        event_types: vec![event.dto().kind],
        relations: vec![
            R::GeneratedFrom,
            R::OptimizedFrom,
            R::PublishedAs,
            R::AttributedTo,
            R::ConsumesService,
        ],
    }
}
fn run(
    bundle: &Bundle,
    event: &Event,
    context: &Context,
    history: &[Evaluation],
    invocations: &[Invocation],
    costs: &[CostEvidence],
) -> crate::Result<Evaluation> {
    bundle.evaluate(Input {
        event,
        context,
        history,
        source_authority: &authority(event),
        invocations,
        costs,
        received_at: &time("2026-09-21T14:00:00Z"),
    })
}
fn invocation(id: &str, op: &str, source: &str) -> Invocation {
    Invocation {
        id: format!("invoke-{id}"),
        binding_id: id.into(),
        operation_id: op.into(),
        chain: "campaign".into(),
        customer: "agency-a".into(),
        source: source.into(),
        unit: "call".into(),
        maximum_quantity: d("1"),
        maximum_exposure: money(100),
        held: money(100),
        authorized_at: time("2026-09-20T13:00:00Z"),
        start_before: time("2026-09-20T15:00:00Z"),
        attested_start: time("2026-09-20T14:00:00Z"),
        outcome_deadline: Some(time("2026-09-22T14:00:00Z")),
        completion_event: None,
    }
}
fn amounts(result: &Evaluation) -> Vec<i128> {
    result
        .actions()
        .iter()
        .map(|a| a.amount().atoms())
        .collect()
}
fn multi(cap: &str, allocation: bool) -> (Bundle, Context, Vec<Event>, Vec<Invocation>) {
    let mut rules = Vec::new();
    for (kind, name, price) in [
        (K::Generated, "generation", "0.80"),
        (K::Optimized, "optimization", "0.30"),
        (K::Published, "publication", "0.40"),
    ] {
        rules.push(base(kind, &format!("{name}.base"), price));
        rules.push(discount(
            kind,
            &format!("{name}.discount"),
            &format!("{name}.base"),
            "10",
            DiscountMode::Additive,
        ));
    }
    let mut premium = rule(
        K::Acquired,
        "acquisition.premium",
        Operation::Premium(Price::Fixed(d("2"))),
    );
    premium.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Published,
    });
    rules.push(premium);
    rules.push(rule(
        K::Acquired,
        "acquisition.cap-credit",
        Operation::Cap {
            ceiling: d(cap),
            stage: "campaign-close".into(),
            component: "acquisition.premium".into(),
        },
    ));
    let mut optimizer_premium = rule(
        K::Acquired,
        "optimizer.premium",
        Operation::Premium(Price::Fixed(d("0.05"))),
    );
    optimizer_premium.matcher = Some(Matcher::AcquisitionOptimization);
    let mut share = rule(
        K::Acquired,
        "publisher.share",
        Operation::Share {
            percent: d("25"),
            ceiling: d("0.50"),
            retail_component: "acquisition.premium".into(),
        },
    );
    share.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Published,
    });
    let mut publisher = binding("publisher", Book::Supplier);
    publisher.allocation_view = allocation;
    let bundle = Bundle::compile(
        "USD",
        2,
        vec![
            Policy {
                binding: publisher,
                rules: vec![base(K::Published, "publisher.base", "0.15"), share],
            },
            Policy {
                binding: binding("retail", Book::Retail),
                rules,
            },
            Policy {
                binding: binding("optimizer", Book::Supplier),
                rules: vec![
                    base(K::Optimized, "optimizer.base", "0.10"),
                    optimizer_premium,
                ],
            },
            Policy {
                binding: binding("model", Book::CostObservation),
                rules: vec![rule(
                    K::Generated,
                    "model.observation",
                    Operation::ObserveCost,
                )],
            },
        ],
    )
    .unwrap();
    let mut context = context();
    context.stage = Some(Stage {
        id: "campaign-close".into(),
        closure_claim_namespace: "sales".into(),
        expected: vec![
            ExpectedOperation {
                source: "urn:host".into(),
                operation_id: "g2".into(),
                kind: K::Generated,
                retail_components: vec!["generation.base".into(), "generation.discount".into()],
            },
            ExpectedOperation {
                source: "urn:optimizer".into(),
                operation_id: "o2".into(),
                kind: K::Optimized,
                retail_components: vec!["optimization.base".into(), "optimization.discount".into()],
            },
            ExpectedOperation {
                source: "urn:publisher".into(),
                operation_id: "p2".into(),
                kind: K::Published,
                retail_components: vec!["publication.base".into(), "publication.discount".into()],
            },
        ],
    });
    let events = vec![
        generated(),
        event(
            "o2",
            K::Optimized,
            "urn:optimizer",
            json!({"binding_id":"optimizer","invocation_id":"invoke-optimizer","links":[link(R::OptimizedFrom,"urn:host","g2")]}),
        ),
        event(
            "p2",
            K::Published,
            "urn:publisher",
            json!({"binding_id":"publisher","invocation_id":"invoke-publisher","links":[link(R::PublishedAs,"urn:optimizer","o2")]}),
        ),
        acquired("a2", "sale-2"),
    ];
    (
        bundle,
        context,
        events,
        vec![
            invocation("optimizer", "o2", "urn:optimizer"),
            invocation("publisher", "p2", "urn:publisher"),
        ],
    )
}
fn multi_history(
    cap: &str,
    allocation: bool,
) -> (Bundle, Context, Vec<Evaluation>, Vec<Invocation>) {
    let (bundle, context, events, mut invocations) = multi(cap, allocation);
    let mut history = Vec::new();
    for event in &events {
        let result = run(
            &bundle,
            event,
            &context,
            &history,
            &invocations,
            &[CostEvidence {
                binding_id: "model".into(),
                event_id: events[0].id().into(),
                document: doc(),
                amount: money(20),
            }],
        )
        .unwrap();
        for c in result.consumptions() {
            let i = invocations
                .iter_mut()
                .find(|i| i.id == c.invocation_id)
                .unwrap();
            i.held = money(i.held.atoms() - c.consume.atoms() - c.release.atoms());
            if event.dto().kind.is_work() {
                i.completion_event = Some(event.id().into());
            }
        }
        history.push(result);
    }
    (bundle, context, history, invocations)
}

#[test]
fn frozen_multi_tool_goldens_keep_roles_books_and_totals() {
    for (cap, bytes) in [
        (
            "3.35",
            include_str!("../../../../../fixtures/journals/third-party/economics.json"),
        ),
        (
            "3.15",
            include_str!("../../../../../fixtures/journals/capped/economics.json"),
        ),
    ] {
        let (_, _, history, _) = multi_history(cap, false);
        let golden: Value = serde_json::from_str(bytes).unwrap();
        let actions: Vec<_> = history.iter().flat_map(|r| r.actions()).collect();
        assert_eq!(actions.len(), golden["postings"].as_array().unwrap().len());
        for posting in golden["postings"].as_array().unwrap() {
            let action = actions
                .iter()
                .find(|a| a.component() == posting["component"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                action.amount().atoms().to_string(),
                posting["atoms"].as_str().unwrap()
            );
            assert_eq!(action.book().name(), posting["book"].as_str().unwrap());
            let mut roles = serde_json::to_value(&action.binding().roles).unwrap();
            roles.as_object_mut().unwrap().remove("payer_delegation");
            assert_eq!(roles, posting["roles"]);
        }
        for (book, total) in golden["expected_book_totals"].as_object().unwrap() {
            let actual: i128 = actions
                .iter()
                .filter(|a| a.book().name() == book)
                .map(|a| a.amount().atoms())
                .sum();
            assert_eq!(actual.to_string(), total.as_str().unwrap());
        }
        assert_eq!(history[3].closed_stage(), Some("campaign-close"));
        assert!(history[3]
            .explanations()
            .iter()
            .any(|s| s.code == golden["cap_reason"].as_str().unwrap()));
        assert_eq!(
            history
                .iter()
                .flat_map(|d| d.deltas())
                .filter(|d| d.book == Book::CostObservation)
                .count(),
            0
        );
        assert!(history[3]
            .actions()
            .iter()
            .find(|a| a.kind() == ActionKind::Share)
            .unwrap()
            .inputs()
            .iter()
            .any(|id| history[3]
                .actions()
                .iter()
                .any(|a| a.id() == id && a.component() == "acquisition.premium")));
    }
}
#[test]
fn first_party_priority_and_tier_use_separate_named_base() {
    let mut priority = rule(
        K::Generated,
        "generation.priority",
        Operation::Premium(Price::Fixed(d("0.20"))),
    );
    priority.when.push(Predicate::Priority(true));
    let mut tier = discount(
        K::Generated,
        "generation.discount",
        "generation.base",
        "20",
        DiscountMode::Additive,
    );
    tier.when.push(Predicate::Tier("enterprise".into()));
    let bundle = retail(vec![
        tier,
        priority,
        base(K::Generated, "generation.base", "1"),
    ]);
    assert_eq!(
        amounts(&run(&bundle, &generated(), &context(), &[], &[], &[]).unwrap()),
        vec![100, 20, -20]
    );
    let mut absent = context();
    absent.tier = None;
    absent.priority = None;
    let result = run(&bundle, &generated(), &absent, &[], &[], &[]).unwrap();
    assert_eq!(amounts(&result), vec![100]);
    assert_eq!(
        result
            .explanations()
            .iter()
            .filter(|e| e.code == "PREDICATE_FALSE")
            .count(),
        2
    );
}
#[test]
fn exact_unit_rounding_and_discount_modes() {
    let bundle = retail(vec![rule(
        K::Generated,
        "generation.base",
        Operation::Base(Price::Unit {
            rate: d("0.07"),
            unit: "call".into(),
        }),
    )]);
    let event = event(
        "fraction",
        K::Generated,
        "urn:host",
        json!({"quantity":"1.5"}),
    );
    let r = run(&bundle, &event, &context(), &[], &[], &[]).unwrap();
    assert_eq!(amounts(&r), vec![11]);
    assert_eq!(
        serde_json::to_value(r.explanations()[0].unrounded.as_ref().unwrap()).unwrap(),
        json!({"numerator":"21","denominator":"2"})
    );
    for (mode, expected) in [(DiscountMode::Additive, 80), (DiscountMode::Sequential, 81)] {
        let b = retail(vec![
            base(K::Generated, "generation.base", "1"),
            discount(K::Generated, "discount.one", "generation.base", "10", mode),
            discount(K::Generated, "discount.two", "generation.base", "10", mode),
        ]);
        let r = run(&b, &generated(), &context(), &[], &[], &[]).unwrap();
        assert_eq!(r.deltas()[0].amount.atoms(), expected);
        if mode == DiscountMode::Sequential {
            assert!(r.actions()[2]
                .inputs()
                .contains(&r.actions()[1].id().into()));
        }
    }
}
#[test]
fn excessive_fixed_or_additive_discount_fails_whole_evaluation() {
    for amount in [
        DiscountAmount::Fixed(d("1.01")),
        DiscountAmount::Percent(d("60")),
    ] {
        let mut rules = vec![
            base(K::Generated, "generation.base", "1"),
            rule(
                K::Generated,
                "discount.one",
                Operation::Discount {
                    amount: amount.clone(),
                    component: "generation.base".into(),
                    mode: DiscountMode::Additive,
                },
            ),
        ];
        if matches!(amount, DiscountAmount::Percent(_)) {
            rules.push(discount(
                K::Generated,
                "discount.two",
                "generation.base",
                "60",
                DiscountMode::Additive,
            ));
        }
        assert_eq!(
            run(&retail(rules), &generated(), &context(), &[], &[], &[])
                .unwrap_err()
                .code,
            "DISCOUNT_EXCEEDS_BASIS"
        );
    }
}
#[test]
fn net_zero_keeps_provenance_but_no_payable_and_round_zero_has_no_action() {
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        discount(
            K::Generated,
            "discount",
            "generation.base",
            "100",
            DiscountMode::Additive,
        ),
    ]);
    let r = run(&b, &generated(), &context(), &[], &[], &[]).unwrap();
    assert_eq!(amounts(&r), vec![100, -100]);
    assert!(r.deltas().is_empty());
    let b = retail(vec![rule(
        K::Generated,
        "generation.base",
        Operation::Base(Price::Unit {
            rate: d("0.004"),
            unit: "call".into(),
        }),
    )]);
    let r = run(&b, &generated(), &context(), &[], &[], &[]).unwrap();
    assert!(r.actions().is_empty());
    assert_eq!(r.explanations()[0].code, "ZERO_ROUNDED");
}
#[test]
fn funding_changes_price_and_preserves_unknown_cost() {
    let mut byok = base(K::Generated, "byok.base", "0.10");
    byok.when = vec![Predicate::Funding(Funding::Byok)];
    let mut platform = base(K::Generated, "platform.base", "0.20");
    platform.when = vec![Predicate::Funding(Funding::Platform)];
    let b = Bundle::compile(
        "USD",
        2,
        vec![
            Policy {
                binding: binding("retail", Book::Retail),
                rules: vec![byok, platform],
            },
            Policy {
                binding: binding("model", Book::CostObservation),
                rules: vec![rule(
                    K::Generated,
                    "model.observation",
                    Operation::ObserveCost,
                )],
            },
        ],
    )
    .unwrap();
    let e = generated();
    let mut c = context();
    c.funding = Funding::Byok;
    let evidence = [CostEvidence {
        binding_id: "model".into(),
        event_id: e.id().into(),
        document: doc(),
        amount: money(7),
    }];
    let r = run(&b, &e, &c, &[], &[], &evidence).unwrap();
    assert_eq!(amounts(&r), vec![10]);
    assert_eq!(r.explanations().last().unwrap().code, "BYOK_NO_HOST_COST");
    c.funding = Funding::Platform;
    let r = run(&b, &e, &c, &[], &[], &[]).unwrap();
    assert_eq!(amounts(&r), vec![20]);
    assert_eq!(r.explanations().last().unwrap().code, "COST_UNKNOWN");
    let r = run(&b, &e, &c, &[], &[], &evidence).unwrap();
    assert_eq!(amounts(&r), vec![20, 7]);
    assert_eq!(r.deltas().len(), 1);
}
#[test]
fn later_linked_discount_appends_and_cannot_overdraw_previous_component() {
    let mut adjustment = rule(
        K::Acquired,
        "acquisition.discount",
        Operation::LinkedDiscount {
            amount: DiscountAmount::Percent(d("20")),
            component: "publication.base".into(),
        },
    );
    adjustment.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Published,
    });
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        base(K::Published, "publication.base", "0.50"),
        adjustment,
    ]);
    let c = context();
    let mut history = vec![];
    for e in [generated(), published()] {
        history.push(run(&b, &e, &c, &history, &[], &[]).unwrap());
    }
    let original = history[1].actions()[0].clone();
    for i in 0..5 {
        let r = run(
            &b,
            &acquired(&format!("a{i}"), &format!("sale-{i}")),
            &c,
            &history,
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(amounts(&r), vec![-10]);
        assert!(r.actions()[0].inputs().contains(&original.id().into()));
        history.push(r);
    }
    assert_eq!(history[1].actions()[0], original);
    assert_eq!(
        run(&b, &acquired("a6", "sale-6"), &c, &history, &[], &[])
            .unwrap_err()
            .code,
        "DISCOUNT_EXCEEDS_BASIS"
    );
}
#[test]
fn outcome_authority_window_and_canonical_claim_are_explicit() {
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        base(K::Published, "publication.base", "0.50"),
        rule(
            K::Acquired,
            "acquisition.premium",
            Operation::Premium(Price::Fixed(d("5"))),
        ),
    ]);
    let c = context();
    let mut h = vec![];
    for e in [generated(), published(), acquired("a1", "sale-1")] {
        h.push(run(&b, &e, &c, &h, &[], &[]).unwrap());
    }
    assert_eq!(
        run(
            &b,
            &acquired("renamed-delivery", "sale-1"),
            &c,
            &h,
            &[],
            &[]
        )
        .unwrap_err()
        .code,
        "CLAIM_CONFLICT"
    );
    let wrong = event(
        "a2",
        K::Acquired,
        "urn:host",
        json!({"claim_id":"sale-2","evidence":[doc()],"links":[link(R::AttributedTo,"urn:publisher","p2")]}),
    );
    assert_eq!(
        run(&b, &wrong, &c, &h[..2], &[], &[]).unwrap_err().code,
        "OUTCOME_AUTHORITY"
    );
    let end = event(
        "a2",
        K::Acquired,
        "urn:outcome",
        json!({"claim_id":"sale-2","occurred_at":"2026-09-22T14:00:00Z","evidence":[doc()],"links":[link(R::AttributedTo,"urn:publisher","p2")]}),
    );
    assert_eq!(
        run(&b, &end, &c, &h[..2], &[], &[]).unwrap_err().code,
        "OUTCOME_WINDOW"
    );
    let e = acquired("late", "sale-late");
    assert_eq!(
        b.evaluate(Input {
            event: &e,
            context: &c,
            history: &h[..2],
            source_authority: &authority(&e),
            invocations: &[],
            costs: &[],
            received_at: &time("2026-09-23T14:00:00.000001Z")
        })
        .unwrap_err()
        .code,
        "AUTHORITY_REVIEW_REQUIRED"
    );
}
#[test]
fn missing_links_wait_and_wrong_types_or_grants_reject() {
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        base(K::Published, "publication.base", "0.50"),
    ]);
    let c = context();
    let p = published();
    assert_eq!(
        run(&b, &p, &c, &[], &[], &[]).unwrap_err().code,
        "WAITING_DEPENDENCIES"
    );
    let g = run(&b, &generated(), &c, &[], &[], &[]).unwrap();
    let mut auth = authority(&p);
    auth.relations.clear();
    assert_eq!(
        b.evaluate(Input {
            event: &p,
            context: &c,
            history: &[g],
            source_authority: &auth,
            invocations: &[],
            costs: &[],
            received_at: &time("2026-09-21T14:00:00Z")
        })
        .unwrap_err()
        .code,
        "LINK_UNAUTHORIZED"
    );
}
#[test]
fn cap_waits_only_at_closure_and_rejects_below_booked() {
    let (b, c, events, invocations) = multi("1.00", false);
    let g = run(&b, &events[0], &c, &[], &invocations, &[]).unwrap();
    assert_eq!(amounts(&g), vec![80, -8]);
    let (_, _, h, _) = multi_history("3.15", false);
    // Same economic prefix evaluated with the independently pinned low cap.
    let mut low_history = vec![g];
    let mut iv = invocations;
    for e in &events[1..3] {
        let r = run(&b, e, &c, &low_history, &iv, &[]).unwrap();
        for cc in r.consumptions() {
            let i = iv.iter_mut().find(|i| i.id == cc.invocation_id).unwrap();
            i.held = money(i.held.atoms() - cc.consume.atoms());
            i.completion_event = Some(e.id().into());
        }
        low_history.push(r);
    }
    assert_eq!(
        run(&b, &events[3], &c, &low_history, &iv, &[])
            .unwrap_err()
            .code,
        "CAP_BELOW_BOOKED"
    );
    let (b, c, _, iv) = multi("3.15", false);
    let mut missing = h[..3].to_vec();
    missing.remove(0);
    assert_eq!(
        run(&b, &events[3], &c, &missing, &iv, &[])
            .unwrap_err()
            .code,
        "WAITING_DEPENDENCIES"
    );
}
#[test]
fn share_allocation_is_information_and_full_reversal_negates_entries() {
    let (_, _, history, _) = multi_history("3.15", true);
    let closure = &history[3];
    let share = closure
        .actions()
        .iter()
        .find(|a| a.kind() == ActionKind::Share)
        .unwrap();
    let allocations: Vec<_> = closure
        .actions()
        .iter()
        .filter(|a| a.book() == Book::Allocation)
        .collect();
    assert_eq!(
        allocations.iter().map(|a| a.amount().atoms()).sum::<i128>(),
        180
    );
    assert_eq!(
        allocations
            .iter()
            .map(|a| a.amount().atoms())
            .collect::<Vec<_>>(),
        vec![45, 135]
    );
    assert_eq!(allocations[1].allocation_recipient(), Some("host-h"));
    assert!(allocations
        .iter()
        .all(|a| a.allocation_parent() == Some(share.id())));
    assert_eq!(closure.deltas().len(), 3);
    let e = event(
        "reverse",
        K::Reversal,
        "urn:host",
        json!({"targets":[closure.event().id()],"reason":"invalid outcome","evidence":[doc()]}),
    );
    let r = reverse(&e, &history, &authority(&e)).unwrap();
    for a in r.actions() {
        let original = closure
            .actions()
            .iter()
            .find(|old| Some(old.id()) == a.reverses())
            .unwrap();
        assert_eq!(a.amount().atoms(), -original.amount().atoms());
        assert_eq!(a.obligation_id(), original.obligation_id());
    }
    assert!(r.consumptions().is_empty());
    assert!(r.closed_stage().is_none());
}
#[test]
fn reversal_requires_full_economic_closure_and_correction_authority() {
    let (b, c, mut h, iv) = multi_history("3.15", false);
    let only_g = event(
        "reverse",
        K::Reversal,
        "urn:host",
        json!({"targets":[h[0].event().id()],"reason":"incorrect","evidence":[doc()]}),
    );
    assert_eq!(
        reverse(&only_g, &h, &authority(&only_g)).unwrap_err().code,
        "REVERSAL_DEPENDENTS_REQUIRED"
    );
    let e = event(
        "reverse",
        K::Reversal,
        "urn:host",
        json!({"targets":h.iter().map(|d| d.event().id()).collect::<Vec<_>>(),"reason":"incorrect","evidence":[doc()]}),
    );
    let r = reverse(&e, &h, &authority(&e)).unwrap();
    for book in [Book::Retail, Book::Supplier, Book::CostObservation] {
        assert_eq!(
            h.iter()
                .flat_map(|d| d.actions())
                .chain(r.actions())
                .filter(|a| a.book() == book)
                .map(|a| a.amount().atoms())
                .sum::<i128>(),
            0
        );
    }
    h.push(r);
    let again = event(
        "reverse-again",
        K::Reversal,
        "urn:host",
        json!({"targets":[h[0].event().id()],"reason":"again","evidence":[doc()]}),
    );
    assert_eq!(
        reverse(&again, &h, &authority(&again)).unwrap_err().code,
        "ALREADY_REVERSED"
    );
    let new_work = event("new-work", K::Generated, "urn:host", json!({}));
    assert_eq!(
        run(&b, &new_work, &c, &h, &iv, &[]).unwrap_err().code,
        "STAGE_CLOSED"
    );
}
#[test]
fn supplier_evidence_cannot_replace_invocation_or_fund_more_work() {
    let (b, c, e, mut iv) = multi("3.15", false);
    let h = vec![run(&b, &e[0], &c, &[], &[], &[]).unwrap()];
    assert_eq!(
        run(&b, &e[1], &c, &h, &[], &[]).unwrap_err().code,
        "INVOCATION_REQUIRED"
    );
    iv[0].held = money(9);
    assert_eq!(
        run(&b, &e[1], &c, &h, &iv, &[]).unwrap_err().code,
        "EXPOSURE_EXCEEDED"
    );
    iv[0].held = money(100);
    iv[0].attested_start = iv[0].start_before.clone();
    assert_eq!(
        run(&b, &e[1], &c, &h, &iv, &[]).unwrap_err().code,
        "INVOCATION_EXPIRED"
    );
}
#[test]
fn failed_work_releases_supplier_reservation_and_never_prices() {
    let (b, c, _, iv) = multi("3.15", false);
    let e = event(
        "o2",
        K::Optimized,
        "urn:optimizer",
        json!({"status":"failed","quantity":"0","binding_id":"optimizer","invocation_id":"invoke-optimizer"}),
    );
    let r = run(&b, &e, &c, &[], &iv, &[]).unwrap();
    assert!(r.actions().is_empty());
    assert_eq!(r.explanations()[0].code, "FAILED_WORK");
    assert_eq!(r.consumptions()[0].consume.atoms(), 0);
    assert_eq!(r.consumptions()[0].release.atoms(), 100);
}
#[test]
fn compilation_rejects_unsupported_or_ambiguous_economics() {
    let mut b = binding("retail", Book::Retail);
    b.roles = Roles::new(["h", "h", "a", "q", "a", "h"], None).unwrap();
    assert_eq!(
        Bundle::compile(
            "USD",
            2,
            vec![Policy {
                binding: b,
                rules: vec![base(K::Generated, "base", "1")]
            }]
        )
        .unwrap_err()
        .code,
        "PAYER_DELEGATION_REQUIRED"
    );
    let rules = vec![
        base(K::Generated, "base", "1"),
        discount(
            K::Generated,
            "discount",
            "base",
            "101",
            DiscountMode::Additive,
        ),
    ];
    assert_eq!(
        Bundle::compile(
            "USD",
            2,
            vec![Policy {
                binding: binding("retail", Book::Retail),
                rules
            }]
        )
        .unwrap_err()
        .code,
        "POLICY_PERCENT_RANGE"
    );
    let mut bad = base(K::Generated, "base", "1");
    bad.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Generated,
    });
    assert_eq!(
        Bundle::compile(
            "USD",
            2,
            vec![Policy {
                binding: binding("retail", Book::Retail),
                rules: vec![bad]
            }]
        )
        .unwrap_err()
        .code,
        "POLICY_AMBIGUOUS_MATCH"
    );
    let (b, _, _, _) = multi("3.15", false);
    let mut policies = b.policies().to_vec();
    let p = policies
        .iter_mut()
        .find(|p| p.binding.id == "publisher")
        .unwrap();
    let mut another = p.rules[1].clone();
    another.component = "publisher.second".into();
    another.id = "second".into();
    p.rules.push(another);
    assert_eq!(
        Bundle::compile("USD", 2, policies).unwrap_err().code,
        "POLICY_LIMIT"
    );
}
#[test]
fn determinism_stable_effect_identity_and_checked_totals() {
    let b = retail(vec![base(K::Generated, "generation.base", "1")]);
    let e = generated();
    let c = context();
    let first = run(&b, &e, &c, &[], &[], &[]).unwrap();
    for _ in 0..10 {
        assert_eq!(
            first.actions(),
            run(&b, &e, &c, &[], &[], &[]).unwrap().actions()
        );
    }
    let mut rules = b.policies()[0].rules.clone();
    rules[0].id = "renamed-rule".into();
    rules[0].operation = Operation::Base(Price::Fixed(d("2")));
    let new = run(&retail(rules), &e, &c, &[], &[], &[]).unwrap();
    assert_eq!(first.actions()[0].id(), new.actions()[0].id());
    assert_ne!(first.actions()[0].amount(), new.actions()[0].amount());
    let b = Bundle::compile(
        "USD",
        0,
        vec![Policy {
            binding: binding("retail", Book::Retail),
            rules: vec![
                base(K::Generated, "first", "999999999999999999999999999999"),
                base(K::Generated, "second", "1"),
            ],
        }],
    )
    .unwrap();
    assert_eq!(
        run(&b, &e, &c, &[], &[], &[]).unwrap_err().code,
        "ARITHMETIC_OVERFLOW"
    );
}
#[test]
fn exhaustive_discount_rounding_and_reversal_cancellation_property() {
    // Independent integer oracle: percent * base / 100, ties away from zero.
    // 1,111 combinations cover zero, ties, every percentage and net zero.
    for base_atoms in [1, 2, 3, 7, 10, 11, 49, 50, 99, 100, 101] {
        for percent in 0..=100 {
            let amount = format!("{}.{:02}", base_atoms / 100, base_atoms % 100);
            let b = retail(vec![
                base(K::Generated, "generation.base", &amount),
                discount(
                    K::Generated,
                    "discount",
                    "generation.base",
                    &percent.to_string(),
                    DiscountMode::Additive,
                ),
            ]);
            let e = generated();
            let c = context();
            let r = run(&b, &e, &c, &[], &[], &[]).unwrap();
            let expected_discount = (base_atoms * percent + 50) / 100;
            assert_eq!(
                r.actions().iter().map(|a| a.amount().atoms()).sum::<i128>(),
                i128::from(base_atoms - expected_discount)
            );
            let reversal = event(
                "r",
                K::Reversal,
                "urn:host",
                json!({"targets":[e.id()],"reason":"property","evidence":[doc()]}),
            );
            let h = [r];
            let reversed = reverse(&reversal, &h, &authority(&reversal)).unwrap();
            assert_eq!(
                h[0].actions()
                    .iter()
                    .chain(reversed.actions())
                    .map(|a| a.amount().atoms())
                    .sum::<i128>(),
                0
            );
        }
    }
}

#[test]
fn inactive_cap_still_requires_its_economic_inputs_in_reversal_closure() {
    let (_, _, h, _) = multi_history("3.35", false);
    assert!(!h[3]
        .actions()
        .iter()
        .any(|a| a.kind() == ActionKind::Credit));
    let e = event(
        "reverse",
        K::Reversal,
        "urn:host",
        json!({"targets":[h[0].event().id()],"reason":"invalid input","evidence":[doc()]}),
    );
    assert_eq!(
        reverse(&e, &h, &authority(&e)).unwrap_err().code,
        "REVERSAL_DEPENDENTS_REQUIRED"
    );
}

#[test]
fn pay_per_external_tool_is_a_separate_obligation_even_under_byok() {
    let b = Bundle::compile(
        "USD",
        2,
        vec![
            Policy {
                binding: binding("retail", Book::Retail),
                rules: vec![rule(
                    K::ToolCompleted,
                    "call.base",
                    Operation::Base(Price::Unit {
                        rate: d("0.10"),
                        unit: "call".into(),
                    }),
                )],
            },
            Policy {
                binding: binding("optimizer", Book::Supplier),
                rules: vec![rule(
                    K::ToolCompleted,
                    "tool.fee",
                    Operation::Base(Price::Unit {
                        rate: d("0.03"),
                        unit: "call".into(),
                    }),
                )],
            },
        ],
    )
    .unwrap();
    let e = event(
        "tool-call",
        K::ToolCompleted,
        "urn:optimizer",
        json!({"binding_id":"optimizer","invocation_id":"invoke-optimizer"}),
    );
    let mut c = context();
    c.funding = Funding::Byok;
    let iv = [invocation("optimizer", "tool-call", "urn:optimizer")];
    let r = run(&b, &e, &c, &[], &iv, &[]).unwrap();
    assert_eq!(amounts(&r), vec![10, 3]);
    assert_eq!(r.deltas().len(), 2);
    assert_ne!(
        r.actions()[0].obligation_id(),
        r.actions()[1].obligation_id()
    );
    assert_eq!(r.actions()[0].binding().roles.payer(), "parent-q");
    assert_eq!(r.actions()[1].binding().roles.payer(), "host-h");
    assert_eq!(r.consumptions()[0].consume.atoms(), 3);
    assert_eq!(r.consumptions()[0].release.atoms(), 97);
}

#[test]
fn share_rounds_then_ceilings_and_cannot_be_charged_to_customer() {
    let (b, c, e, mut iv) = multi("100", false);
    let mut policies = b.policies().to_vec();
    let premium = policies[0]
        .rules
        .iter_mut()
        .find(|r| r.component == "acquisition.premium")
        .unwrap();
    premium.operation = Operation::Premium(Price::Fixed(d("10")));
    let b = Bundle::compile("USD", 2, policies).unwrap();
    let mut h = vec![];
    for event in &e[..3] {
        let r = run(&b, event, &c, &h, &iv, &[]).unwrap();
        for cc in r.consumptions() {
            let i = iv.iter_mut().find(|i| i.id == cc.invocation_id).unwrap();
            i.held = money(i.held.atoms() - cc.consume.atoms());
            i.completion_event = Some(event.id().into());
        }
        h.push(r);
    }
    let r = run(&b, &e[3], &c, &h, &iv, &[]).unwrap();
    let step = r
        .explanations()
        .iter()
        .find(|s| s.rule_id.as_deref() == Some("publisher.share"))
        .unwrap();
    assert_eq!(step.code, "SHARE_CEILING");
    assert_eq!(step.rounded, Some(50));
    assert_eq!(step.unrounded.as_ref().unwrap().round_atoms().unwrap(), 250);
    let a = r
        .actions()
        .iter()
        .find(|a| a.kind() == ActionKind::Share)
        .unwrap();
    assert_eq!(a.book(), Book::Supplier);
    assert_eq!(a.binding().roles.bearer(), "host-h");
}

#[test]
fn reversal_of_linked_discount_restores_only_its_actual_balance() {
    let mut adjustment = rule(
        K::Acquired,
        "acquisition.discount",
        Operation::LinkedDiscount {
            amount: DiscountAmount::Fixed(d("0.50")),
            component: "publication.base".into(),
        },
    );
    adjustment.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Published,
    });
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        base(K::Published, "publication.base", "0.50"),
        adjustment,
    ]);
    let c = context();
    let mut h = vec![];
    for e in [generated(), published(), acquired("a1", "sale-1")] {
        h.push(run(&b, &e, &c, &h, &[], &[]).unwrap());
    }
    let e = event(
        "r",
        K::Reversal,
        "urn:host",
        json!({"targets":[h[2].event().id()],"reason":"invalid outcome","evidence":[doc()]}),
    );
    h.push(reverse(&e, &h, &authority(&e)).unwrap());
    h.push(run(&b, &acquired("a2", "sale-2"), &c, &h, &[], &[]).unwrap());
    assert_eq!(
        run(&b, &acquired("a3", "sale-3"), &c, &h, &[], &[])
            .unwrap_err()
            .code,
        "DISCOUNT_EXCEEDS_BASIS"
    );
}

#[test]
fn invocation_capacity_and_terms_remain_pinned_after_completion() {
    let (b, c, e, mut iv) = multi("3.15", false);
    let mut h = vec![];
    for event in &e[..3] {
        let r = run(&b, event, &c, &h, &iv, &[]).unwrap();
        for cc in r.consumptions() {
            let i = iv.iter_mut().find(|i| i.id == cc.invocation_id).unwrap();
            i.held = money(i.held.atoms() - cc.consume.atoms());
            i.completion_event = Some(event.id().into());
        }
        h.push(r);
    }
    iv[0].held = money(100);
    assert_eq!(
        run(&b, &e[3], &c, &h, &iv, &[]).unwrap_err().code,
        "EXPOSURE_EXCEEDED"
    );
    iv[0].held = money(90);
    iv[0].maximum_quantity = d("2");
    assert_eq!(
        run(&b, &e[3], &c, &h, &iv, &[]).unwrap_err().code,
        "INVOCATION_CONFLICT"
    );
    iv[0].maximum_quantity = d("1");
    let mut auth = authority(&e[3]);
    auth.active = false;
    assert_eq!(
        b.evaluate(Input {
            event: &e[3],
            context: &c,
            history: &h,
            source_authority: &auth,
            invocations: &iv,
            costs: &[],
            received_at: &time("2026-09-21T14:00:00Z")
        })
        .unwrap_err()
        .code,
        "SOURCE_UNAUTHORIZED"
    );
    let mut changed = c.clone();
    changed.funding = Funding::Byok;
    assert_eq!(
        run(&b, &e[3], &changed, &h, &iv, &[]).unwrap_err().code,
        "PINNED_CONTEXT"
    );
}

#[test]
fn ambiguous_service_links_and_chain_depth_fail_closed() {
    let mut charge = base(K::Generated, "generation.base", "1");
    charge.matcher = Some(Matcher::Direct {
        relation: R::ConsumesService,
        target: K::ToolCompleted,
    });
    let b = retail(vec![base(K::ToolCompleted, "tool.base", "0.10"), charge]);
    let c = context();
    let mut h = vec![];
    for id in ["t1", "t2"] {
        let e = event(id, K::ToolCompleted, "urn:host", json!({}));
        h.push(run(&b, &e, &c, &h, &[], &[]).unwrap());
    }
    let e = event(
        "g",
        K::Generated,
        "urn:host",
        json!({"links":[link(R::ConsumesService,"urn:host","t1"),link(R::ConsumesService,"urn:host","t2")]}),
    );
    assert_eq!(
        run(&b, &e, &c, &h, &[], &[]).unwrap_err().code,
        "POLICY_AMBIGUOUS_MATCH"
    );
    let b = retail(vec![base(K::Generated, "generation.base", "1")]);
    let mut h = vec![];
    for i in 0..=16 {
        let extra = if i == 0 {
            json!({})
        } else {
            json!({"links":[link(R::GeneratedFrom,"urn:host",&format!("g{}",i-1))]})
        };
        let e = event(&format!("g{i}"), K::Generated, "urn:host", extra);
        h.push(run(&b, &e, &c, &h, &[], &[]).unwrap());
    }
    let e = event(
        "g17",
        K::Generated,
        "urn:host",
        json!({"links":[link(R::GeneratedFrom,"urn:host","g16")]}),
    );
    assert_eq!(run(&b, &e, &c, &h, &[], &[]).unwrap_err().code, "LIMIT");
}

#[test]
fn frozen_first_slice_economic_ids_are_compatible() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../../../fixtures/canonical/valid/first-slice-input.json"
    ))
    .unwrap();
    let e = normalize(
        &serde_json::to_vec(&input).unwrap(),
        Scope::new("demo", "sandbox").unwrap(),
        "urn:demo:app",
    )
    .unwrap()
    .resolve(Some("demo-slice"))
    .unwrap();
    let mut terms = binding("demo-retail-v1", Book::Retail);
    terms.agreement = "demo-retail".into();
    terms.sources = vec!["urn:demo:app".into()];
    terms.roles = Roles::new(
        [
            "demo-host",
            "demo-host",
            "demo-customer",
            "demo-customer",
            "demo-customer",
            "demo-host",
        ],
        None,
    )
    .unwrap();
    let b = Bundle::compile(
        "USD",
        2,
        vec![Policy {
            binding: terms,
            rules: vec![
                base(K::Generated, "generation.base", "1"),
                discount(
                    K::Generated,
                    "generation.discount",
                    "generation.base",
                    "20",
                    DiscountMode::Additive,
                ),
            ],
        }],
    )
    .unwrap();
    let mut c = context();
    c.customer = "demo-customer".into();
    c.funding = Funding::Byok;
    let r = run(&b, &e, &c, &[], &[], &[]).unwrap();
    let rows = include_str!("../../../../../fixtures/journals/first-slice/accepted-records.jsonl");
    let expected: Vec<Value> = rows
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|row: &Value| row["kind"] == "action")
        .collect();
    for a in r.actions() {
        let row = expected
            .iter()
            .find(|row| row["body"]["component"] == a.component())
            .unwrap();
        assert_eq!(a.id(), row["id"].as_str().unwrap());
        assert_eq!(a.effect_id(), row["body"]["effect_id"].as_str().unwrap());
        assert_eq!(
            a.obligation_id(),
            row["body"]["obligation_id"].as_str().unwrap()
        );
    }
    assert_eq!(r.deltas()[0].amount.atoms(), 80);
}

#[test]
fn proposed_fixture_values_drive_economic_examples() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/proposed/phase2-economics.json"
    ))
    .unwrap();
    let atom = |section: &str, key: &str| {
        fixture[section][key]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap()
    };
    let (_, _, history, _) = multi_history("3.15", true);
    let closure = &history[3];
    let share = closure
        .actions()
        .iter()
        .find(|a| a.kind() == ActionKind::Share)
        .unwrap();
    assert_eq!(
        share.amount().atoms(),
        atom("allocation_view", "supplier_share_atoms")
    );
    for a in closure
        .actions()
        .iter()
        .filter(|a| a.book() == Book::Allocation)
    {
        let key = if a.allocation_recipient() == Some("host-h") {
            "host_allocation_atoms"
        } else {
            "supplier_allocation_atoms"
        };
        assert_eq!(a.amount().atoms(), atom("allocation_view", key));
    }
    let mut adjustment = rule(
        K::Acquired,
        "acquisition.discount",
        Operation::LinkedDiscount {
            amount: DiscountAmount::Percent(d(fixture["linked_discount"]["percent"]
                .as_str()
                .unwrap())),
            component: "publication.base".into(),
        },
    );
    adjustment.matcher = Some(Matcher::Direct {
        relation: R::AttributedTo,
        target: K::Published,
    });
    let b = retail(vec![
        base(K::Generated, "generation.base", "1"),
        base(K::Published, "publication.base", "0.50"),
        adjustment,
    ]);
    let c = context();
    let mut h = vec![];
    for e in [generated(), published(), acquired("a", "sale")] {
        h.push(run(&b, &e, &c, &h, &[], &[]).unwrap());
    }
    assert_eq!(
        h[2].actions()[0].amount().atoms(),
        atom("linked_discount", "outcome_atoms")
    );
    assert_eq!(
        h[1].actions()[0].amount().atoms(),
        atom(
            "linked_discount",
            "original_publication_after_discount_atoms"
        )
    );
}
