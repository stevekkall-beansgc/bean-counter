//! Production pure-core vs independently authored proposal histories.
//! This is semantic projection conformance, not persistence or canonical bytes.
#[path = "phase2_core/adapter.rs"]
mod adapter;
use adapter::{sorted, text, Adapter};
use ledgerlab_core::canonical;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

const HISTORIES: &[&str] = &[
    "cap-below-booked",
    "cap-not-binding",
    "cap-zero",
    "component-rounding",
    "exposure-exceeded",
    "failed-completion",
    "funding-byok",
    "funding-platform-known",
    "funding-platform-unknown",
    "generation",
    "invalid-links-authority-order",
    "later-acquisition",
    "missing-invocation",
    "multi-capped",
    "multi-uncapped",
    "out-of-order",
    "pay-per-service",
    "retroactive-invocation",
    "share-ceiling",
    "tier-enterprise",
    "tier-standard",
];
const BOUNDARIES: &[(&str, &str)] = &[
    (
        "later-quality",
        "quality_of / prior booked-net proposal differs from core LinkedDiscount",
    ),
    (
        "quality-not-v0",
        "no accepted quality event/relation in the production wire enum",
    ),
    (
        "missing-assent",
        "retained evidence authenticity is coordinator work, not a pure-core input lookup",
    ),
    (
        "unknown-config",
        "story-context is test-only, not a production input format",
    ),
    (
        "unknown-policy",
        "story-tariff is test-only, not the production wire DSL",
    ),
    (
        "unknown-semantics",
        "proposal version is test-only, not a production evaluator selector",
    ),
    (
        "multiple-shares",
        "core rejects at bundle compilation, oracle at acquisition; checked separately",
    ),
    (
        "payer-delegation-required",
        "core rejects at bundle compilation; checked separately",
    ),
];
fn load(name: &str) -> Value {
    canonical::parse_bounded(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("fixtures/phase2-proposed-v1/{name}.json")),
        )
        .unwrap(),
        canonical::BUNDLE_LIMIT,
    )
    .unwrap()
}
fn expected_projection(d: &Value) -> Value {
    json!({"id":d["id"],"depends_on":d["depends_on"],
        "postings":sorted(d["postings"].as_array().unwrap().clone()),
        "explanations":sorted(d["explanations"].as_array().unwrap().clone()),
        "obligations":sorted(d["obligations"].as_array().unwrap().clone())})
}
fn core_refusal(oracle: &str) -> &str {
    match oracle {
        "MISSING_LINK" | "AMBIGUOUS_LINK" => "LINK_CARDINALITY",
        "LINK_CYCLE" => "LINK_SELF",
        "DEPENDENCIES_MISSING" => "WAITING_DEPENDENCIES",
        "OUTCOME_AUTHORITY_CONFLICT" => "OUTCOME_AUTHORITY",
        "OUTCOME_EVIDENCE_REQUIRED" => "SCHEMA",
        "CONTEXT_MISMATCH" => "CHAIN_MISMATCH",
        "INVOCATION_MISMATCH" => "INVOCATION_CONFLICT",
        "INVOCATION_ORDER" => "INVOCATION_EXPIRED",
        "INVOCATION_EXPOSURE" => "EXPOSURE_EXCEEDED",
        other => other,
    }
}
#[test]
fn production_core_matches_aligned_proposed_histories_step_by_step() {
    let mut accepted = 0;
    let mut refused = 0;
    let mut duplicate_guards = 0;
    let mut boundary_steps = 0;
    for name in HISTORIES {
        let f = load(name);
        let mut a = Adapter::new(f["config"].clone(), f["events"].clone()).unwrap();
        let mut journal = Vec::new();
        assert_eq!(
            f["attempts"].as_array().unwrap().len(),
            f["expected"]["results"].as_array().unwrap().len(),
            "{name}: every attempt needs a comparison"
        );
        for (attempt, expected) in f["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .zip(f["expected"]["results"].as_array().unwrap())
        {
            let alias = text(attempt, "event");
            // A wrong source is a different scoped endpoint in production. The
            // oracle's globally unique aliases report LINK_SOURCE instead of
            // missing endpoint. Missing retained evidence is outside this kernel.
            if *name == "invalid-links-authority-order"
                && ["wrong-source", "missing-proof"].contains(&alias.as_str())
            {
                boundary_steps += 1;
                assert_eq!(a.state(), expected["state"]);
                continue;
            }
            let before = journal.clone();
            let result = a.evaluate(&alias, &text(attempt, "received"));
            match expected["status"].as_str().unwrap() {
                "accepted" => {
                    let result = result.unwrap_or_else(|e| panic!("{name}/{alias}: {e:?}"));
                    let projected = a.projection(&result);
                    let expected_decision = f["expected"]["journal"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|d| d["id"] == alias)
                        .unwrap();
                    assert_eq!(
                        projected,
                        expected_projection(expected_decision),
                        "{name}/{alias}: complete common projection"
                    );
                    journal.push(projected);
                    a.history.push(result);
                    accepted += 1;
                }
                "duplicate" => {
                    // Pure evaluation deliberately refuses an existing identity/
                    // claim. Receipt lookup/semantic aliases belong to the future
                    // coordinator bridge and are NOT reimplemented in this test.
                    assert_eq!(
                        result.unwrap_err().code,
                        "CLAIM_CONFLICT",
                        "{name}/{alias}: kernel duplicate guard"
                    );
                    duplicate_guards += 1;
                }
                "rejected" | "waiting" => {
                    assert_eq!(
                        result.unwrap_err().code,
                        core_refusal(&text(expected, "code")),
                        "{name}/{alias}"
                    );
                    refused += 1;
                }
                other => panic!("unmapped fixture status {other}"),
            }
            assert_eq!(
                journal[..before.len()],
                before,
                "{name}/{alias}: prior projections unchanged"
            );
            assert_eq!(
                a.state(),
                expected["state"],
                "{name}/{alias}: full projected state"
            );
            // Reproject all prior core values to detect history mutation too.
            for (index, original) in a.history.iter().enumerate() {
                assert_eq!(
                    a.projection(original),
                    journal[index],
                    "{name}/{alias}: original history immutable"
                );
            }
        }
        assert_eq!(
            journal.len(),
            f["expected"]["journal"].as_array().unwrap().len(),
            "{name}: every proposed decision was compared"
        );
        assert_eq!(a.state(), f["expected"]["state"], "{name}: final state");
    }
    println!("{} histories: {accepted} accepted, {refused} refused/waiting, {duplicate_guards} kernel duplicate guards, {boundary_steps} explicitly excluded steps",HISTORIES.len());
}
#[test]
fn proposal_inventory_and_compile_boundaries_are_explicit() {
    let paths =
        fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/phase2-proposed-v1"))
            .unwrap();
    let actual: BTreeSet<_> = paths
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|s| s == "json")
                && p.file_name().unwrap() != "history.schema.json"
        })
        .map(|p| p.file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    let declared: BTreeSet<_> = HISTORIES
        .iter()
        .copied()
        .chain(BOUNDARIES.iter().map(|(n, r)| {
            assert!(!r.is_empty());
            *n
        }))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        actual, declared,
        "every proposed history requires an explicit integration disposition"
    );
    for (name, code) in [
        ("multiple-shares", "POLICY_LIMIT"),
        ("payer-delegation-required", "PAYER_DELEGATION_REQUIRED"),
    ] {
        assert_eq!(
            adapter::compile(&load(name)["config"]).unwrap_err().code,
            code
        );
    }
}
