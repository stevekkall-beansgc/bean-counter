//! Separate non-admitting semantic regressions and finite legacy diagnostics.
use super::*;
use serde_json::{json, Value};
use std::io::{self, Write};
struct Capped<W> {
    inner: W,
    written: usize,
}
impl<W: Write> Write for Capped<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("calibration output overflow"))?;
        if next > 128 * 1024 * 1024 {
            return Err(io::Error::other("calibration output cap"));
        }
        self.inner.write_all(bytes)?;
        self.written = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
#[test]
fn private_calibration_domain_and_first_crossing_remain_non_admitting() {
    assert!(CoreMeter::new(65537).is_err());
    assert!(CoreMeter::calibration(262145).is_err());
    let mut m = CoreMeter::calibration(65537).unwrap();
    assert!(m.enter_draft().is_err());
    m.enter_calibration().unwrap();
    for _ in 0..65536 {
        m.visit("test.synthetic_instruction").unwrap();
    }
    assert!(m.diagnostic().unwrap().crossing.is_none());
    let mut executed = false;
    m.diagnostic_scope("test.threshold", |m| {
        m.check("test.crossing", || {
            executed = true;
            true
        })
    })
    .unwrap();
    assert!(executed);
    let crossing = m.diagnostic().unwrap().crossing.as_ref().unwrap();
    assert_eq!((crossing.before, crossing.after), (65536, 65537));
    assert_eq!(crossing.sites, vec!["test.threshold"]);
    executed = false;
    assert!(m
        .check("test.refused", || {
            executed = true;
            true
        })
        .is_err());
    assert!(!executed);
    assert_eq!(m.usage().trace.len(), m.diagnostic().unwrap().callers.len());
    let mut writer = Capped {
        inner: vec![],
        written: 128 * 1024 * 1024,
    };
    assert!(writer.write(b"x").is_err());
    assert!(writer.inner.is_empty());
}

fn independent(draft: &DraftTransition, expected: &Value, m: &mut CoreMeter) -> crate::Result<()> {
    m.phase(Phase::Compare);
    let payload = codec::encode(m, &expected["normalized_payload"], 262144)?;
    if !shape::same_bytes(m, &draft.normalized_payload, &payload)? {
        return Err(shape::fail("independent normalized payload"));
    }
    let rows = expected["new_records"].as_array().unwrap();
    let wanted: Vec<ExactRef> = serde_json::from_value(expected["new_refs"].clone()).unwrap();
    if !meter::cmp(m, &draft.new_envelopes.len().into(), &wanted.len().into())?.is_eq()
        || !meter::cmp(m, &rows.len().into(), &wanted.len().into())?.is_eq()
    {
        return Err(shape::fail("independent new set count"));
    }
    // Unique actual references plus every sealed expected reference exactly once
    // establish a bijection. Equal cardinality and repeated membership do not.
    for (i, actual) in draft.new_envelopes.iter().enumerate() {
        for prior in &draft.new_envelopes[..i] {
            m.visit("calibration.actual_unique")?;
            if shape::same_ref(m, &actual.reference, &prior.reference)? {
                return Err(shape::fail("duplicate independent actual Ref"));
            }
        }
    }
    for (i, want) in wanted.iter().enumerate() {
        for prior in &wanted[..i] {
            m.visit("calibration.expected_unique")?;
            if shape::same_ref(m, want, prior)? {
                return Err(shape::fail("duplicate sealed Ref"));
            }
        }
        let mut found = None;
        for actual in &draft.new_envelopes {
            m.visit("calibration.expected_member")?;
            if shape::same_ref(m, &actual.reference, want)? {
                found = Some(actual);
            }
        }
        let actual = found.ok_or_else(|| shape::fail("missing independent Ref"))?;
        let mut body = None;
        for row in rows {
            m.visit("calibration.sealed_body")?;
            let reference: ExactRef = serde_json::from_value(row["ref"].clone()).unwrap();
            if shape::same_ref(m, &reference, want)? {
                body = Some(&row["body"]);
            }
        }
        let encoded = codec::encode(
            m,
            body.ok_or_else(|| shape::fail("missing sealed body"))?,
            4194304,
        )?;
        if !shape::same_bytes(m, &actual.canonical_body, &encoded)? {
            return Err(shape::fail("independent body bytes"));
        }
    }
    let mut decisions = vec![];
    let mut chain = None;
    let mut claims = vec![];
    let mut steps = None;
    for row in rows {
        m.visit("calibration.sealed_projection")?;
        let reference: ExactRef = serde_json::from_value(row["ref"].clone()).unwrap();
        let body = &row["body"];
        if shape::same_text(m, &reference.kind, "workflow-decision")? {
            decisions.push((
                body["decision_id"].as_str().unwrap().to_owned(),
                body["revision"].as_str().unwrap().to_owned(),
                reference.clone(),
            ));
        }
        if shape::same_text(m, &reference.kind, "workflow-chain-state")? {
            chain = Some(reference.clone());
        }
        if shape::same_text(m, &reference.kind, "workflow-work-claim")? {
            claims.push(reference.clone());
        }
        if shape::same_text(m, &reference.kind, "workflow-evaluation")? {
            steps = Some(body["steps"].clone());
        }
    }
    let heads = json!({"decision":decisions,"family":[],"chain":chain});
    let actual_heads = json!({"decision":draft.proposed_heads.decision,"family":draft.proposed_heads.family,"chain":draft.proposed_heads.chain});
    for (actual, want) in [
        (actual_heads, heads),
        (json!(draft.claim_introductions), json!(claims)),
        (
            json!(draft.evaluation_steps),
            steps.ok_or_else(|| shape::fail("sealed evaluation"))?,
        ),
        (json!(draft.link_introductions), json!([])),
        (json!(draft.prospective_obligations), json!([])),
        (json!(draft.history_edges), json!([])),
    ] {
        shape::equal(m, &actual, &want)?;
    }
    // Rounded money/rational operands and action postings are checked through
    // complete independently sealed Action/Evaluation/Decision body equality.
    Ok(())
}
#[test]
fn selected_a_setup_non_admitting_semantic_regression() {
    let (program, c, p, inputs) = first_work_tests::test_inputs();
    let state = DraftStateData::default();
    let mut m = CoreMeter::semantic_regression(None).unwrap();
    let draft = construct::semantic_regression(&program, &c, &p, &inputs, &state, &mut m).unwrap();
    let oracle: Value =
        serde_json::from_slice(include_bytes!("fixtures/first-work-expected.json")).unwrap();
    let mut comparison = CoreMeter::semantic_regression(None).unwrap();
    comparison.enter_semantic_regression().unwrap();
    independent(&draft, &oracle["expected"], &mut comparison).unwrap();

    // One actual construction fixes exact N; this is not a search for a
    // larger production budget or a claim about the legacy profile's fit.
    let n = m.usage().semantic_work;
    let mut exact = CoreMeter::semantic_regression(Some(n)).unwrap();
    let replay =
        construct::semantic_regression(&program, &c, &p, &inputs, &state, &mut exact).unwrap();
    construct::compare_drafts(&replay, &draft, &mut comparison).unwrap();
    assert_eq!(m.usage(), exact.usage());
    let mut less = CoreMeter::semantic_regression(Some(n - 1)).unwrap();
    let refused = construct::semantic_regression(&program, &c, &p, &inputs, &state, &mut less);
    assert_eq!(refused.unwrap_err().code, "PROGRAM_BUDGET_EXCEEDED");
    let used = less.usage();
    assert!(used.semantic_work <= n - 1);
    assert_eq!(used.trace, m.usage().trace[..used.trace.len()]);
    assert_eq!(
        draft.unresolved_admission_obligations,
        data::UNRESOLVED_ADMISSION_OBLIGATIONS
    );
}

#[test]
fn selected_a_setup_finite_diagnostic_stops_without_a_projection() {
    let (program, c, p, inputs) = first_work_tests::test_inputs();
    let mut m = CoreMeter::calibration(262144).unwrap();
    let result = construct::calibrate(
        &program,
        &c,
        &p,
        &inputs,
        &DraftStateData::default(),
        &mut m,
    );
    assert_eq!(
        result.unwrap_err(),
        crate::Error::new(
            "PROGRAM_BUDGET_EXCEEDED",
            "selected semantic work exhausted"
        )
    );
    assert_eq!(m.usage().semantic_work, 262144);
    assert_eq!(m.diagnostic_work_stop(), 262144);
    let d = m.diagnostic().unwrap();
    assert_eq!(d.event_capacity, 262144);
    assert!(d.peak <= 48 * 1024 * 1024);
    assert!(d.crossing.is_some());
    assert!(d.refusal.is_some());
}

#[test]
fn independent_projection_comparison_rejects_duplicates_and_wrong_heads() {
    let oracle: Value =
        serde_json::from_slice(include_bytes!("fixtures/first-work-expected.json")).unwrap();
    let e = &oracle["expected"];
    let records = e["new_records"].as_array().unwrap();
    let mut draft = DraftTransition {
        normalized_payload: crate::canonical::CanonicalBytes::from_value(&e["normalized_payload"])
            .unwrap()
            .into_vec(),
        new_envelopes: records
            .iter()
            .map(|r| DraftEnvelope {
                reference: serde_json::from_value(r["ref"].clone()).unwrap(),
                canonical_body: crate::canonical::CanonicalBytes::from_value(&r["body"])
                    .unwrap()
                    .into_vec(),
            })
            .collect(),
        evaluation_steps: vec![],
        proposed_heads: ProposedHeads {
            decision: vec![],
            family: vec![],
            chain: None,
        },
        claim_introductions: vec![],
        link_introductions: vec![],
        prospective_obligations: vec![],
        consumed_refs: vec![],
        history_edges: vec![],
        usage: CoreMeter::new(65536).unwrap().finish(),
        unresolved_admission_obligations: data::UNRESOLVED_ADMISSION_OBLIGATIONS,
    };
    for row in records {
        let r: ExactRef = serde_json::from_value(row["ref"].clone()).unwrap();
        let b = &row["body"];
        match r.kind.as_str() {
            "workflow-decision" => draft.proposed_heads.decision.push((
                b["decision_id"].as_str().unwrap().into(),
                b["revision"].as_str().unwrap().into(),
                r,
            )),
            "workflow-chain-state" => draft.proposed_heads.chain = Some(r),
            "workflow-work-claim" => draft.claim_introductions.push(r),
            "workflow-evaluation" => {
                draft.evaluation_steps = serde_json::from_value(b["steps"].clone()).unwrap()
            }
            _ => {}
        }
    }
    independent(&draft, e, &mut CoreMeter::new(65536).unwrap()).unwrap();
    let mut duplicate = draft.clone();
    duplicate.new_envelopes[1] = duplicate.new_envelopes[0].clone();
    assert_eq!(
        independent(&duplicate, e, &mut CoreMeter::new(65536).unwrap())
            .unwrap_err()
            .detail,
        "duplicate independent actual Ref"
    );
    let mut wrong = draft.clone();
    wrong.proposed_heads.chain = None;
    assert_eq!(
        independent(&wrong, e, &mut CoreMeter::new(65536).unwrap())
            .unwrap_err()
            .detail,
        "data relation"
    );
    wrong = draft;
    wrong.claim_introductions.clear();
    assert!(independent(&wrong, e, &mut CoreMeter::new(65536).unwrap()).is_err());
}
