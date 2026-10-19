//! Private non-admitting calibration. Never selected by ordinary focused filters.
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
fn attempt(a: &meter::Attempt) -> Value {
    json!({"phase":format!("{:?}",a.phase),"kind":a.kind,"operand_limbs":a.operands,"cost":a.cost,"before":a.before,"after":a.after,"caller":{"file":a.file,"line":a.line,"sites":a.sites}})
}
fn report(m: &CoreMeter, result: &crate::Result<DraftTransition>) -> Value {
    let d = m.diagnostic().expect("private calibration");
    let usage = m.usage();
    json!({"status":if result.is_ok(){"DATA-projection-completed"}else{"STOP"},"error":result.as_ref().err().map(|e|json!({"code":e.code,"detail":e.detail})),"production_ceiling":meter::MAXIMUM_WORK,"diagnostic_stop":m.diagnostic_work_stop(),"maximum_diagnostic_stop":262144,"semantic_work":usage.semantic_work,"events":usage.trace.len(),"current_phase":format!("{:?}",m.diagnostic_phase()),"last_accepted":usage.trace.last().zip(d.callers.last()).map(|(e,c)|json!({"phase":format!("{:?}",e.phase),"kind":e.kind,"cost":e.cost,"operand_limbs":e.operands,"caller":{"file":c.file,"line":c.line,"sites":c.sites}})),"first_production_crossing":d.crossing.as_ref().map(attempt),"diagnostic_refusal":d.refusal.as_ref().map(attempt),"accounted_event_storage":d.storage,"peak_trace_payload":d.peak,"outer_event_capacity":d.event_capacity,"usage_trace_copy_bytes":d.usage_copy_bytes,"bytes":{"raw":usage.raw_request_bytes,"unique_resolved":usage.unique_resolved_bytes,"unique_refs":usage.unique_resolved_refs,"new_bodies":usage.new_body_bytes,"read":usage.body_bytes_read,"scanned":usage.scanned_bytes,"written":usage.canonical_bytes_written,"hashed":usage.hashed_bytes},"non_admitting":true})
}
fn emit(m: &CoreMeter, result: &crate::Result<DraftTransition>, extra: Value) {
    let mut writer = Capped {
        inner: io::stdout().lock(),
        written: 0,
    };
    writer.write_all(b"R2_CALIBRATION_REPORT ").unwrap();
    serde_json::to_writer(&mut writer,&json!({"schema":"bean-counter-r2-calibration-report/1","result":report(m,result),"comparison":extra,"unresolved":data::UNRESOLVED_ADMISSION_OBLIGATIONS})).unwrap();
    writer.write_all(b"\nR2_CALIBRATION_TRACE [").unwrap();
    for (i, (event, caller)) in m
        .usage()
        .trace
        .iter()
        .zip(&m.diagnostic().unwrap().callers)
        .enumerate()
    {
        if i > 0 {
            writer.write_all(b",").unwrap();
        }
        serde_json::to_writer(&mut writer,&json!({"phase":format!("{:?}",event.phase),"kind":event.kind,"operand_limbs":event.operands,"cost":event.cost,"caller":{"file":caller.file,"line":caller.line,"sites":caller.sites}})).unwrap();
    }
    writer.write_all(b"]\n").unwrap();
    writer.flush().unwrap();
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
fn fail_stage(
    m: &CoreMeter,
    result: &crate::Result<DraftTransition>,
    stage: &str,
    probe: &CoreMeter,
    error: &crate::Error,
) -> ! {
    emit(
        m,
        result,
        json!({"performed":true,"status":"STOP","stage":stage,"error":{"code":error.code,"detail":error.detail},"probe":report(probe,&Err(error.clone()))}),
    );
    panic!("calibration {stage} stopped");
}
#[test]
fn selected_a_setup_non_admitting_calibration() {
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
    let Ok(draft) = &result else {
        emit(
            &m,
            &result,
            json!({"performed":false,"stage":"fresh","reason":"no DATA projection returned"}),
        );
        panic!("calibration stopped");
    };
    let oracle: Value =
        serde_json::from_slice(include_bytes!("fixtures/first-work-expected.json")).unwrap();
    let mut comparison = CoreMeter::calibration(262144).unwrap();
    comparison.enter_calibration().unwrap();
    if let Err(e) = comparison.scoped("independent.DATA-comparison", |m| {
        independent(draft, &oracle["expected"], m)
    }) {
        fail_stage(&m, &result, "independent", &comparison, &e);
    }
    let n = m.usage().semantic_work;
    // Exact-N is also the same-constructor fresh/replay check. No extra recipe,
    // optimistic budget or second algorithm is used.
    let mut exact = CoreMeter::calibration(n).unwrap();
    let replay = construct::calibrate(
        &program,
        &c,
        &p,
        &inputs,
        &DraftStateData::default(),
        &mut exact,
    );
    let other = match &replay {
        Ok(v) => v,
        Err(e) => fail_stage(&m, &result, "exact-N-replay", &exact, e),
    };
    if let Err(e) = comparison.scoped("replay.DATA-comparison", |m| {
        construct::compare_drafts(other, draft, m)
    }) {
        fail_stage(&m, &result, "replay-comparison", &comparison, &e);
    }
    if m.usage() != exact.usage() {
        fail_stage(
            &m,
            &result,
            "construction-trace",
            &exact,
            &shape::fail("fresh/replay construction counters"),
        );
    }
    let exact_report = report(&exact, &replay);
    drop(replay);
    drop(exact);
    let mut less = CoreMeter::calibration(n - 1).unwrap();
    let refused = construct::calibrate(
        &program,
        &c,
        &p,
        &inputs,
        &DraftStateData::default(),
        &mut less,
    );
    let refusal = match &refused {
        Err(e) if e.code == "PROGRAM_BUDGET_EXCEEDED" => e,
        Err(e) => fail_stage(&m, &result, "N-minus-1-unexpected", &less, e),
        Ok(_) => fail_stage(
            &m,
            &result,
            "N-minus-1-returned-Draft",
            &less,
            &shape::fail("N-minus-1 succeeded"),
        ),
    };
    if less.diagnostic().unwrap().refusal.is_none() {
        fail_stage(&m, &result, "N-minus-1-not-work", &less, refusal);
    }
    let less_report = report(&less, &refused);
    drop(refused);
    drop(less);
    let mut ordinary = CoreMeter::diagnosed_ordinary().unwrap();
    let normal = ordinary.scoped("selected.A.setup", |m| {
        construct::draft_transition(&program, &c, &p, &inputs, &DraftStateData::default(), m)
    });
    if n <= meter::MAXIMUM_WORK {
        match &normal {
            Ok(v) => {
                if let Err(e) = comparison.scoped("ordinary.DATA-comparison", |m| {
                    construct::compare_drafts(v, draft, m)
                }) {
                    fail_stage(&m, &result, "ordinary-comparison", &comparison, &e)
                }
            }
            Err(e) => fail_stage(&m, &result, "ordinary65536", &ordinary, e),
        };
    } else {
        match &normal {
            Err(e)
                if e.code == "PROGRAM_BUDGET_EXCEEDED"
                    && ordinary.diagnostic().unwrap().crossing.is_some() => {}
            Err(e) => fail_stage(&m, &result, "ordinary-unexpected", &ordinary, e),
            Ok(_) => fail_stage(
                &m,
                &result,
                "ordinary-widened",
                &ordinary,
                &shape::fail("ordinary meter exceeded fixed ceiling"),
            ),
        };
    }
    let ordinary_report = report(&ordinary, &normal);
    drop(normal);
    drop(ordinary);
    // No more than3 diagnostic meters/projections coexist. Their trace copies
    // are reserved inside each48MiB per-invocation payload cap; allocator/RSS is
    // separately sampled by the external runner, not claimed as this bound.
    emit(
        &m,
        &result,
        json!({"performed":true,"independent_bijection":true,"independent_projections":true,"new_set":draft.new_envelopes.iter().map(|d|&d.reference).collect::<Vec<_>>(),"exact_N_replay":exact_report,"N_minus_1":less_report,"ordinary65536":ordinary_report,"construction_trace_equal":true,"comparison_work_separate":comparison.usage().semantic_work,"comparison_peak_payload":comparison.diagnostic().unwrap().peak,"maximum_live_diagnostic_trace_payload":3*48*1024*1024,"product_would_refuse":n>meter::MAXIMUM_WORK}),
    );
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
