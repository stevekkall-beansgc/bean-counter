use super::*;
fn m(n: i128) -> Money {
    Money::new("USD", 2, n).unwrap()
}
fn fixed(n: i128) -> o::Amount {
    o::Amount::Fixed(m(n))
}
fn pct(n: i128) -> o::Amount {
    o::Amount::Percent(ExactRatio::integer(n))
}
fn family(agreement: &str, name: &str, binding: usize, amounts: &[(&str, o::Amount)]) -> Family {
    Family {
        key: FamilyKey {
            agreement: agreement.into(),
            family: name.into(),
        },
        binding,
        codes: amounts
            .iter()
            .map(|(c, a)| o::Code {
                code: (*c).into(),
                amount: a.clone(),
            })
            .collect(),
        replacements: amounts.iter().map(|(c, _)| (*c).into()).collect(),
        allow_reversal: true,
    }
}
fn basis(id: &str, book: Book, net: i128, limit: i128) -> BindingBasis {
    BindingBasis {
        binding_id: id.into(),
        agreement: id.into(),
        book,
        roles: Roles::new(
            ["host", "host", "customer", "customer", "customer", "host"],
            None,
        )
        .unwrap(),
        booked_net: m(net),
        premium_limit: m(limit),
    }
}
fn activity() -> ComparisonActivity {
    ComparisonActivity {
        provenance: ComparisonProvenance {
            snapshot: "snap".into(),
            activity: "activity".into(),
            semantics: ALGORITHM.into(),
        },
        scope: Scope::new("demo", "sandbox").unwrap(),
        target: "historical target reference".into(),
        historical_policy: "original policy reference".into(),
        historical_version: "original".into(),
        retail_basis: m(8000),
        bindings: vec![
            basis("retail", Book::Retail, 8000, 5000),
            basis("supplier", Book::Supplier, 3000, 12000),
        ],
        families: vec![
            family(
                "retail",
                "fee",
                0,
                &[
                    ("success", fixed(2500)),
                    ("zero", fixed(0)),
                    ("corrected", fixed(1000)),
                ],
            ),
            family("retail", "rebate", 0, &[("rebate", pct(-10))]),
            family(
                "supplier",
                "supplier-fee",
                1,
                &[("success", fixed(2500)), ("zero", fixed(0))],
            ),
            family("supplier", "unobserved", 1, &[("success", fixed(0))]),
        ],
        economic: vec![],
        chronology: vec![],
        reservations: vec![ReservationBasis {
            binding_id: "supplier".into(),
            maximum: m(15000),
            consumed: m(3000),
            held: m(12000),
            released: m(0),
        }],
    }
}
fn claim(a: &mut ComparisonActivity, family: usize, code: &str, amount: i128) {
    a.chronology.push(ActivityEntry::Outcome {
        decision_index: a.economic.len(),
    });
    a.economic.push(EconomicStep {
        family,
        change: o::Change::Claim { code: code.into() },
        revision: 1,
        booked: amount,
        booked_delta: amount,
    });
}
fn correct(a: &mut ComparisonActivity, family: usize, code: Option<&str>, amount: i128) {
    let old = a
        .economic
        .iter()
        .rev()
        .find(|s| s.family == family)
        .unwrap();
    let revision = old.revision + 1;
    let delta = amount - old.booked;
    a.chronology.push(ActivityEntry::Outcome {
        decision_index: a.economic.len(),
    });
    a.economic.push(EconomicStep {
        family,
        change: o::Change::Correct {
            expected_revision: revision - 1,
            replacement: code.map(str::to_owned),
        },
        revision,
        booked: amount,
        booked_delta: delta,
    });
}
fn complete() -> ComparisonActivity {
    let mut a = activity();
    claim(&mut a, 0, "success", 2500);
    claim(&mut a, 1, "rebate", -800);
    claim(&mut a, 2, "success", 2500);
    correct(&mut a, 0, Some("zero"), 0);
    correct(&mut a, 2, Some("zero"), 0);
    correct(&mut a, 0, Some("corrected"), 1000);
    correct(&mut a, 0, None, 0);
    correct(&mut a, 0, Some("success"), 2500);
    a.chronology.push(ActivityEntry::Close {
        binding_id: "supplier".into(),
    });
    a
}
fn candidate(
    a: &ComparisonActivity,
    key: &str,
    fee: o::Amount,
    rebate: o::Amount,
) -> AmountCandidate {
    let mut amounts = a.original_amounts();
    amounts
        .iter_mut()
        .find(|x| x.key.family == "fee" && x.key.code == "success")
        .unwrap()
        .amount = fee;
    amounts
        .iter_mut()
        .find(|x| x.key.family == "rebate")
        .unwrap()
        .amount = rebate;
    AmountCandidate {
        key: key.into(),
        amounts,
    }
}
fn result(c: &CandidateComparison) -> (&[StepEstimate], &LatestEstimate) {
    match &c.result {
        CandidateResult::Complete { steps, latest } => (steps, latest),
        other => panic!("{other:?}"),
    }
}
#[test]
fn audited_chronology_zero_corrections_reversal_reinstatement_and_reservations() {
    let a = complete();
    a.validate().unwrap();
    let matrix = compare_amount_candidates(
        &a,
        &[
            candidate(&a, "p1", pct(20), fixed(-1000)),
            candidate(&a, "p2", pct(10), pct(-10)),
        ],
    )
    .unwrap();
    assert!(!matrix.committed());
    let (original, latest) = result(&matrix.original);
    assert_eq!(
        original.iter().map(|s| s.delta).collect::<Vec<_>>(),
        vec![2500, -800, 2500, -2500, -2500, 1000, -1000, 2500, 0]
    );
    assert_eq!(latest.bindings[0].final_net, 9700);
    for (c, expected, net) in [
        (
            &matrix.candidates[0],
            vec![1600, -1000, 2500, -1600, -2500, 1000, -1000, 1600, 0],
            8600,
        ),
        (
            &matrix.candidates[1],
            vec![800, -800, 2500, -800, -2500, 1000, -1000, 800, 0],
            8000,
        ),
    ] {
        let (steps, latest) = result(c);
        assert_eq!(steps.iter().map(|s| s.delta).collect::<Vec<_>>(), expected);
        assert_eq!(latest.bindings[0].final_net, net);
        assert_eq!(latest.bindings[0].difference_from_booked, net - 9700);
        assert_eq!(steps[3].inverse, Some(-steps[0].replacement.unwrap()));
        assert_eq!(steps[3].reason, "ZERO_ROUNDED");
        assert_eq!(steps[6].reason, "CLAIM_REVERSED");
        assert_eq!(latest.families[0].revision, Some(5));
        assert_eq!(latest.families[3].amount, None);
        assert!(latest.families[3].ordinary_closed);
        assert_eq!(latest.families[2].amount, Some(0));
        for i in [2, 4] {
            let r = steps[i].reservation.as_ref().unwrap();
            assert_eq!((r.held, r.consumed, r.released), (9500, 5500, 0));
        }
        let r = &latest.reservations[0];
        assert_eq!((r.held, r.consumed, r.released), (0, 5500, 9500));
        assert_eq!(c.provenance, a.provenance);
    }
}
#[test]
fn all_candidate_permutations_duplicates_labels_and_amount_order_are_invariant() {
    let a = complete();
    let cs = [
        candidate(&a, "a", pct(20), fixed(-1000)),
        candidate(&a, "b", pct(10), pct(-10)),
        candidate(&a, "bad", fixed(9000), pct(-10)),
    ];
    let reference = compare_amount_candidates(&a, &cs).unwrap();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let reordered = compare_amount_candidates(&a, &order.map(|i| cs[i].clone())).unwrap();
        for (c, i) in reordered.candidates.iter().zip(order) {
            assert_eq!(c, &reference.candidates[i]);
        }
    }
    let mut renamed = cs[0].clone();
    renamed.key = "renamed".into();
    renamed.amounts.reverse();
    let repeats = compare_amount_candidates(&a, &[cs[0].clone(), renamed, cs[0].clone()]).unwrap();
    for c in &repeats.candidates {
        assert_eq!(c.result, reference.candidates[0].result);
        assert_eq!(c.fingerprint, reference.candidates[0].fingerprint);
    }
}
#[test]
fn signed_half_ties_zero_and_fixed_retail_basis() {
    for (percent, expected) in [(50, 1), (-50, -1), (49, 0), (-49, 0), (51, 1), (-51, -1)] {
        assert_eq!(
            o::exact_amount(Some(&pct(percent)), &m(1))
                .unwrap()
                .round_atoms()
                .unwrap(),
            expected
        );
    }
    let mut a = activity();
    a.families[2].codes[0].amount = pct(25);
    claim(&mut a, 0, "success", 2500);
    claim(&mut a, 1, "rebate", -800);
    claim(&mut a, 2, "success", 2000);
    let c = candidate(&a, "same", fixed(2500), pct(-10));
    let matrix = compare_amount_candidates(&a, &[c.clone(), c]).unwrap();
    let (steps, _) = result(&matrix.original);
    assert_eq!(steps[2].exact, Some(ExactRatio::integer(2000))); // 8000*25%, not supplier3000 or adjusted9700.
    let mut zero = activity();
    zero.retail_basis = m(0);
    zero.bindings[0].booked_net = m(0);
    zero.families[0].codes[0].amount = pct(50);
    claim(&mut zero, 0, "success", 0);
    let c = candidate(&zero, "zero", pct(-50), fixed(0));
    let matrix = compare_amount_candidates(&zero, &[c.clone(), c]).unwrap();
    assert_eq!(result(&matrix.candidates[0]).1.families[0].amount, Some(0));
}
#[test]
fn separate_gross_capacities_and_intermediate_failure_never_get_a_total() {
    let mut a = complete();
    a.bindings[0].premium_limit = m(3000);
    let bad = candidate(&a, "premium", fixed(3001), fixed(-2000));
    let worse = candidate(&a, "discount", fixed(2500), fixed(-8001));
    let matrix = compare_amount_candidates(&a, &[bad, worse]).unwrap();
    for (i, ordinal, code) in [(0, 0, "PREMIUM_LIMIT"), (1, 1, "DISCOUNT_EXCEEDS_BASIS")] {
        assert!(
            matches!(&matrix.candidates[i].result,CandidateResult::Infeasible(f) if f.ordinal==Some(ordinal) && f.reason.code==code)
        );
    }
    let mut overflow = candidate(&a, "correction", fixed(2500), fixed(-800));
    overflow
        .amounts
        .iter_mut()
        .find(|a| a.key.code == "corrected")
        .unwrap()
        .amount = fixed(3001);
    let good = candidate(&a, "ok", pct(10), pct(-10));
    let matrix = compare_amount_candidates(&a, &[overflow, good]).unwrap();
    assert!(
        matches!(&matrix.candidates[0].result,CandidateResult::Infeasible(f) if f.ordinal==Some(5))
    );
    assert!(matches!(
        matrix.candidates[1].result,
        CandidateResult::Complete { .. }
    ));
}
#[test]
fn malformed_keys_units_supplier_changes_and_limits() {
    let a = complete();
    let good = candidate(&a, "ok", pct(10), pct(-10));
    for kind in 0..6 {
        let mut c = good.clone();
        match kind {
            0 => {
                c.amounts.pop();
            }
            1 => c.amounts.push(c.amounts[0].clone()),
            2 => c.amounts[0].key.code = "unknown".into(),
            3 => c.amounts[0].amount = o::Amount::Fixed(Money::new("EUR", 2, 1).unwrap()),
            4 => c.amounts[0].amount = o::Amount::Fixed(Money::new("USD", 3, 1).unwrap()),
            _ => {
                c.amounts
                    .iter_mut()
                    .find(|x| x.key.agreement == "supplier")
                    .unwrap()
                    .amount = fixed(1)
            }
        }
        let matrix = compare_amount_candidates(&a, &[c, good.clone()]).unwrap();
        assert!(matches!(
            matrix.candidates[0].result,
            CandidateResult::Infeasible(_)
        ));
        assert!(matches!(
            matrix.candidates[1].result,
            CandidateResult::Complete { .. }
        ));
    }
    assert!(compare_amount_candidates(&a, std::slice::from_ref(&good)).is_err());
    assert!(compare_amount_candidates(&a, &vec![good.clone(); 9]).is_err());
    let mut huge = good.clone();
    huge.amounts = vec![huge.amounts[0].clone(); 1025];
    assert!(compare_amount_candidates(&a, &[huge, good]).is_err());
}
#[test]
fn chronological_mapping_zero_identity_and_closed_family_checks() {
    let a = complete();
    for kind in 0..4 {
        let mut bad = a.clone();
        match kind {
            0 => {
                bad.chronology.remove(0);
            }
            1 => bad.chronology.swap(0, 1),
            2 => bad.chronology[1] = bad.chronology[0].clone(),
            _ => bad.reservations[0].held = m(1),
        }
        assert!(bad.validate().is_err());
    }
    let mut a = activity();
    a.chronology.push(ActivityEntry::Close {
        binding_id: "supplier".into(),
    });
    claim(&mut a, 2, "zero", 0);
    assert_eq!(a.run(&a.original_amounts()).unwrap_err().ordinal, Some(1));
    let mut a = activity();
    claim(&mut a, 0, "zero", 0);
    claim(&mut a, 0, "success", 2500);
    assert_eq!(a.run(&a.original_amounts()).unwrap_err().ordinal, Some(1));
    let mut a = complete();
    a.economic[3].booked_delta = 1;
    assert!(a.run(&a.original_amounts()).is_err());
}
#[test]
fn supplier_corrections_after_close_do_not_reopen_capacity() {
    let mut a = activity();
    claim(&mut a, 2, "success", 2500);
    a.chronology.push(ActivityEntry::Close {
        binding_id: "supplier".into(),
    });
    correct(&mut a, 2, None, 0);
    correct(&mut a, 2, Some("success"), 2500);
    let (steps, latest) = a.run(&a.original_amounts()).unwrap();
    assert_eq!(steps[2].inverse, Some(-2500));
    assert_eq!(steps[3].replacement, Some(2500));
    for s in &steps[1..] {
        let r = s.reservation.as_ref().unwrap();
        assert_eq!((r.held, r.consumed, r.released), (0, 5500, 9500));
    }
    assert_eq!(latest.bindings[1].final_net, 5500);
}
#[test]
fn private_digest_domains_and_input_bounds() {
    assert_ne!(
        provenance_digest("snapshot", b"x").unwrap(),
        provenance_digest("activity", b"x").unwrap()
    );
    assert_eq!(
        provenance_digest("report", b"x").unwrap(),
        provenance_digest("report", b"x").unwrap()
    );
    assert!(provenance_digest("receipt", b"x").is_err());
    assert!(provenance_digest("snapshot", &vec![0; 16 * 1024 * 1024 + 1]).is_err());
}

#[test]
fn negative_prior_is_inverted_exactly_and_atomic_delta_is_bounded() {
    let mut a = activity();
    a.families[0].codes[0].amount = fixed(-2500);
    claim(&mut a, 0, "success", -2500);
    correct(&mut a, 0, Some("corrected"), 1000);
    let c = candidate(&a, "negative", fixed(-1234), pct(-10));
    let matrix = compare_amount_candidates(&a, &[c.clone(), c]).unwrap();
    let (steps, latest) = result(&matrix.candidates[0]);
    assert_eq!(
        (steps[1].inverse, steps[1].replacement, steps[1].delta),
        (Some(1234), Some(1000), 2234)
    );
    assert_eq!(latest.bindings[0].final_net, 9000);
    assert_eq!(latest.bindings[0].difference_from_booked, 0);
    assert!(o::exact_amount(
        Some(&pct(crate::money::MAX_ATOMS)),
        &m(crate::money::MAX_ATOMS)
    )
    .unwrap()
    .round_atoms()
    .is_err());
    let mut a = activity();
    a.retail_basis = m(crate::money::MAX_ATOMS);
    a.bindings[0].booked_net = m(crate::money::MAX_ATOMS);
    a.bindings[0].premium_limit = m(crate::money::MAX_ATOMS);
    a.families[0].codes[0].amount = fixed(-crate::money::MAX_ATOMS);
    a.families[0].codes[2].amount = fixed(1);
    claim(&mut a, 0, "success", -crate::money::MAX_ATOMS);
    correct(&mut a, 0, Some("corrected"), 1);
    assert_eq!(
        a.run(&a.original_amounts()).unwrap_err().reason.code,
        "ARITHMETIC_OVERFLOW"
    );
}

#[test]
fn collective_input_size_bound_is_enforced_before_computation() {
    let a = complete();
    let mut c = candidate(&a, "bounded", fixed(0), fixed(0));
    let mut row = c.amounts[0].clone();
    row.key.agreement = "a".repeat(128);
    row.key.family = "f".repeat(128);
    row.key.code = "c".repeat(128);
    c.amounts = vec![row; 1024];
    assert_eq!(
        compare_amount_candidates(&a, &vec![c; 8]).unwrap_err().code,
        "COMPARISON_LIMIT"
    );
}

#[test]
fn retained_original_base_projects_without_new_activity_and_rejects_spurious_chronology() {
    // Read only immutable original source bytes; no candidate is frozen as a target.
    let evaluations: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../contracts/candidates/v2/original-evaluations.json"
    ))
    .unwrap();
    let bytes = crate::canonical::CanonicalBytes::from_value(
        &evaluations["evaluations"]["fixed-success-fee"],
    )
    .unwrap();
    let base = super::super::retained::decode_evaluation(bytes.as_slice(), &[]).unwrap();
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../contracts/candidates/v2/goldens/fixed-success-fee.json"
    ))
    .unwrap();
    let snapshot = &golden["seed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "target-snapshot")
        .unwrap()["body"];
    let policy: serde_json::Value =
        serde_json::from_str(snapshot["policy_utf8"].as_str().unwrap()).unwrap();
    let time =
        |v: &serde_json::Value| crate::domain::Timestamp::parse(v.as_str().unwrap()).unwrap();
    let window = |v: &serde_json::Value| o::Window {
        starts_at: time(&v["starts_at"]),
        occurs_before: time(&v["occurs_before"]),
        received_by: time(&v["received_by"]),
        accepted_by: time(&v["accepted_by"]),
    };
    let f = &policy["families"][0];
    let target = o::Target::freeze(
        &base,
        o::Policy {
            version: policy["version"].as_str().unwrap().into(),
            document: policy["document"].as_str().unwrap().into(),
            families: vec![o::Family {
                family: f["family"].as_str().unwrap().into(),
                binding_id: f["binding_id"].as_str().unwrap().into(),
                source: f["source"].as_str().unwrap().into(),
                correction_source: f["correction_source"].as_str().unwrap().into(),
                evidence_required: true,
                ordinary: window(&f["ordinary"]),
                corrections: window(&f["corrections"]),
                codes: f["codes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| o::Code {
                        code: c["code"].as_str().unwrap().into(),
                        amount: o::Amount::Fixed(
                            serde_json::from_value(c["amount"]["money"].clone()).unwrap(),
                        ),
                    })
                    .collect(),
                replacement_codes: serde_json::from_value(f["replacement_codes"].clone()).unwrap(),
                allow_reversal: true,
            }],
            limits: vec![o::Limit {
                binding_id: policy["limits"][0]["binding_id"].as_str().unwrap().into(),
                premium: serde_json::from_value(policy["limits"][0]["premium"].clone()).unwrap(),
            }],
        },
        o::TargetVerification {
            rated_final: true,
            accepted_at: time(&snapshot["accepted_at"]),
            policy_document: policy["document"].as_str().unwrap().into(),
            verified_assents: base
                .bundle()
                .policies
                .iter()
                .map(|p| p.binding.assent.clone())
                .collect(),
            verified_offers: vec![],
            verified_delegations: vec![],
        },
    )
    .unwrap();
    let provenance = activity().provenance;
    let a = ComparisonActivity::from_history(&target, &[], &[], &[], provenance.clone()).unwrap();
    assert_eq!(a.retail_basis().atoms(), 10000);
    assert_eq!(a.historical_version(), "1");
    assert_eq!(a.target(), base.event().id());
    let c = AmountCandidate {
        key: "original amounts only".into(),
        amounts: a.original_amounts(),
    };
    let comparison = compare_amount_candidates(&a, &[c.clone(), c]).unwrap();
    assert_eq!(result(&comparison.original).1.families[0].amount, None);
    assert!(ComparisonActivity::from_history(
        &target,
        &[],
        &[ActivityEntry::Outcome { decision_index: 0 }],
        &[],
        provenance.clone()
    )
    .is_err());
    assert!(ComparisonActivity::from_history(
        &target,
        &[],
        &[ActivityEntry::Close {
            binding_id: "unknown".into()
        }],
        &[],
        provenance
    )
    .is_err());
}
#[test]
fn supplier_discount_capacity_is_its_booked_net_even_with_retail_percentage_basis() {
    assert_eq!(
        o::exact_amount(Some(&pct(-50)), &m(8000))
            .unwrap()
            .round_atoms()
            .unwrap(),
        -4000
    );
    assert_eq!(
        o::active_net(3000, -4000, 2500, 12000).unwrap_err().code,
        "DISCOUNT_EXCEEDS_BASIS"
    );
    assert_eq!(o::active_net(3000, -3000, 2500, 12000).unwrap(), 2500);
}

#[test]
fn resumable_driver_matches_batch_and_never_finishes_partial_work() {
    let a = complete();
    let cs = [
        candidate(&a, "p1", pct(20), fixed(-1000)),
        candidate(&a, "p2", pct(10), pct(-10)),
    ];
    let expected = compare_amount_candidates(&a, &cs).unwrap();
    let mut driver = ComparisonDriver::new(&a, &cs).unwrap();
    let mut calls = 0;
    loop {
        calls += 1;
        if driver.advance().unwrap() {
            break;
        }
    }
    assert_eq!(calls, 27); // Exactly one of nine retained steps per scenario.
    assert!(driver.advance().unwrap());
    assert_eq!(driver.finish().unwrap(), expected);
    for interrupted_after in [0, 1, 8, 9, 10, 17, 26] {
        let mut driver = ComparisonDriver::new(&a, &cs).unwrap();
        for _ in 0..interrupted_after {
            assert!(!driver.advance().unwrap());
        }
        assert_eq!(driver.finish().unwrap_err().code, "COMPARISON_INCOMPLETE");
        let mut dropped = ComparisonDriver::new(&a, &cs).unwrap();
        for _ in 0..interrupted_after {
            dropped.advance().unwrap();
        }
        drop(dropped);
        assert_eq!(compare_amount_candidates(&a, &cs).unwrap(), expected);
    }
}
#[test]
fn driver_original_parity_failure_is_terminal_before_any_candidate_report() {
    let mut a = complete();
    a.economic[3].booked_delta += 1;
    let cs = [
        candidate(&a, "p1", pct(20), fixed(-1000)),
        candidate(&a, "p2", pct(10), pct(-10)),
    ];
    let mut driver = ComparisonDriver::new(&a, &cs).unwrap();
    for _ in 0..3 {
        assert!(!driver.advance().unwrap());
    }
    let error = driver.advance().unwrap_err();
    assert_eq!(error.code, "COMPARISON_HISTORY");
    assert_eq!(driver.advance().unwrap_err(), error);
    assert_eq!(driver.finish().unwrap_err(), error);
    assert!(compare_amount_candidates(&a, &cs).is_err());
}
