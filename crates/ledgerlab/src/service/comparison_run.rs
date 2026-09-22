//! Supplier chronology foundation only. No canonical profile or acceptance path.
use super::*;
use ledgerlab_core::{
    money::{parse_atoms, Money},
    policy::chaining::{comparison as c, Book},
};
use std::io::Write;

pub(crate) struct FoundationReport(Vec<u8>);
impl FoundationReport {
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Outer admission survives acquisition, detached evaluation and bounded output.
pub(crate) async fn compare_retained<S: ComparisonReadStore, A: ComparisonReadAuthority>(
    reader: &S,
    authority: &A,
    who: &AuthenticatedReadContext,
    selection: RetainedSelection,
    candidates: &[c::AmountCandidate],
    cancellation: Cancellation,
) -> Result<FoundationReport, ComparisonError> {
    let operation = ComparisonOperation::begin(cancellation)?;
    candidate_budget(candidates)?;
    let workspace = load_workspace(reader, authority, who, selection, &operation).await?;
    evaluate_workspace(&workspace, candidates, &operation).await
}

pub(crate) fn activity(
    workspace: &ComparisonWorkspace,
) -> Result<c::ComparisonActivity, ComparisonError> {
    check(
        workspace
            .decisions()
            .iter()
            .all(|d| d.binding().book == Book::Supplier),
    )?;
    let invocation = workspace
        .target()
        .base()
        .invocations()
        .iter()
        .find(|i| i.id == workspace.selection().invocation_id)
        .ok_or(ComparisonError::Integrity)?;
    let mut chronology = vec![];
    let mut initial = None;
    let mut next = 0;
    for row in workspace
        .settlement_rows()
        .iter()
        .filter(|r| r["kind"] == "reservation-observation")
    {
        let body = &row["body"];
        let kind = body["command"]["kind"]
            .as_str()
            .ok_or(ComparisonError::Integrity)?;
        match kind {
            "register" => {
                check(initial.is_none() && chronology.is_empty())?;
                let value = |name: &str| -> Result<Money, ComparisonError> {
                    let atoms = parse_atoms(
                        body["before"][name]
                            .as_str()
                            .ok_or(ComparisonError::Integrity)?,
                    )
                    .map_err(|_| ComparisonError::Integrity)?;
                    Money::new(
                        invocation.maximum_exposure.currency(),
                        invocation.maximum_exposure.scale(),
                        atoms,
                    )
                    .map_err(|_| ComparisonError::Integrity)
                };
                initial = Some(c::ReservationBasis {
                    binding_id: invocation.binding_id.clone(),
                    maximum: value("maximum")?,
                    consumed: value("consumed")?,
                    held: value("held")?,
                    released: value("released")?,
                });
            }
            "ordinary" | "post_hoc" => {
                check(initial.is_some())?;
                let decision = workspace
                    .decisions()
                    .get(next)
                    .ok_or(ComparisonError::Integrity)?;
                let request = decision.request();
                check(
                    body["command"]["source"] == request.source
                        && body["command"]["external_id"] == request.id
                        && body["command"]["family"]["agreement_id"] == request.agreement
                        && body["command"]["family"]["family_id"] == request.family
                        && body["command"]["family"]["target"] == workspace.selection().target
                        && ((kind == "ordinary")
                            == matches!(request.change, o::Change::Claim { .. })),
                )?;
                chronology.push(c::ActivityEntry::Outcome {
                    decision_index: next,
                });
                next += 1;
            }
            "close" => {
                check(initial.is_some())?;
                chronology.push(c::ActivityEntry::Close {
                    binding_id: invocation.binding_id.clone(),
                });
            }
            _ => return Err(ComparisonError::Integrity),
        }
    }
    check(next == workspace.decisions().len())?;
    c::ComparisonActivity::from_history(
        workspace.target(),
        workspace.decisions(),
        &chronology,
        &[initial.ok_or(ComparisonError::Integrity)?],
        c::ComparisonProvenance {
            snapshot: workspace.fingerprint().0.clone(),
            activity: workspace.activity_fingerprint().into(),
            semantics: SEMANTICS.into(),
        },
    )
    .map_err(|_| ComparisonError::Integrity)
}

struct CandidateSize(usize);
impl Write for CandidateSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_CANDIDATE_BYTES)
            .ok_or_else(|| std::io::Error::other("candidate limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn candidate_budget(candidates: &[c::AmountCandidate]) -> Result<(), ComparisonError> {
    limit((2..=8).contains(&candidates.len()))?;
    let mut lengths = vec![];
    for candidate in candidates {
        limit(candidate.key.len() <= 128 && candidate.amounts.len() <= 1024)?;
        let mut writer = CandidateSize(0);
        raw(&mut writer, b"{\"key\":")?;
        emit(&mut writer, &json!(candidate.key))?;
        raw(&mut writer, b",\"amounts\":[")?;
        for (i, amount) in candidate.amounts.iter().enumerate() {
            limit(
                [&amount.key.agreement, &amount.key.family, &amount.key.code]
                    .iter()
                    .all(|s| s.len() <= 128),
            )?;
            if i > 0 {
                raw(&mut writer, b",")?;
            }
            emit(&mut writer, &amount_value(amount))?;
        }
        raw(&mut writer, b"]}")?;
        lengths.push(writer.0);
    }
    admit_candidates(&lengths)
}
fn amount_value(a: &c::CandidateAmount) -> Value {
    let amount = match &a.amount {
        o::Amount::Fixed(m) => json!({"fixed":m}),
        o::Amount::Percent(p) => json!({"percent":p}),
    };
    json!({"agreement":a.key.agreement,"family":a.key.family,"code":a.key.code,"amount":amount})
}
fn emit(w: &mut impl Write, v: &Value) -> Result<(), ComparisonError> {
    serde_json::to_writer(w, v).map_err(|_| ComparisonError::Limit)
}
fn raw(w: &mut impl Write, bytes: &[u8]) -> Result<(), ComparisonError> {
    w.write_all(bytes).map_err(|_| ComparisonError::Limit)
}
fn binding(b: &c::BindingEstimate) -> Value {
    json!({"binding":b.binding_id,"premium":b.premium.to_string(),"discount":b.discount.to_string(),"adjustment":b.adjustment.to_string(),"net":b.final_net.to_string(),"difference_from_booked":b.difference_from_booked.to_string()})
}
fn reservation(r: &c::ReservationEstimate) -> Value {
    json!({"binding":r.binding_id,"maximum":r.maximum.to_string(),"consumed":r.consumed.to_string(),"held":r.held.to_string(),"released":r.released.to_string()})
}
fn step(s: &c::StepEstimate) -> Value {
    let entry = match &s.entry {
        c::ActivityEntry::Outcome { decision_index } => json!({"outcome_index":decision_index}),
        c::ActivityEntry::Close { binding_id } => json!({"close_binding":binding_id}),
    };
    json!({"ordinal":s.ordinal,"entry":entry,"family":s.family.as_ref().map(|f|json!({"agreement":f.agreement,"family":f.family})),"revision":s.revision.map(|n|n.to_string()),"exact":s.exact,"inverse":s.inverse.map(|n|n.to_string()),"replacement":s.replacement.map(|n|n.to_string()),"delta":s.delta.to_string(),"difference_from_booked_delta":s.difference_from_booked_delta.to_string(),"reason":s.reason,"binding":binding(&s.binding),"projected_reservation":s.reservation.as_ref().map(reservation)})
}
async fn write_candidate(
    writer: &mut BoundedReport,
    candidate: &c::CandidateComparison,
    operation: &ComparisonOperation,
) -> Result<(), ComparisonError> {
    raw(writer, b"{\"key\":")?;
    emit(writer, &json!(candidate.key))?;
    raw(writer, b",\"fingerprint\":")?;
    emit(writer, &json!(candidate.fingerprint))?;
    raw(writer, b",\"amounts\":[")?;
    for (i, a) in candidate.amounts.iter().enumerate() {
        if i > 0 {
            raw(writer, b",")?;
        }
        emit(writer, &amount_value(a))?;
    }
    raw(writer, b"],\"result\":")?;
    match &candidate.result {
        c::CandidateResult::Infeasible(f) => emit(
            writer,
            &json!({"status":"infeasible","ordinal":f.ordinal,"reason":f.reason.code}),
        )?,
        c::CandidateResult::Complete { steps, latest } => {
            raw(writer, b"{\"status\":\"complete\",\"steps\":[")?;
            for (i, s) in steps.iter().enumerate() {
                operation.yield_and_checkpoint().await?;
                if i > 0 {
                    raw(writer, b",")?;
                }
                emit(writer, &step(s))?;
            }
            raw(writer, b"],\"latest\":")?;
            emit(
                writer,
                &json!({"bindings":latest.bindings.iter().map(binding).collect::<Vec<_>>(),"families":latest.families.iter().map(|f|json!({"agreement":f.key.agreement,"family":f.key.family,"observation":if f.amount.is_none(){"unobserved"}else{"observed"},"revision":f.revision.map(|n|n.to_string()),"current_code":f.current_code,"amount":f.amount.map(|n|n.to_string()),"ordinary_closed":f.ordinary_closed})).collect::<Vec<_>>(),"projected_reservations":latest.reservations.iter().map(reservation).collect::<Vec<_>>()}),
            )?;
            raw(writer, b"}")?;
        }
    }
    raw(writer, b"}")
}

pub(crate) async fn evaluate_workspace(
    workspace: &ComparisonWorkspace,
    candidates: &[c::AmountCandidate],
    operation: &ComparisonOperation,
) -> Result<FoundationReport, ComparisonError> {
    operation.yield_and_checkpoint().await?;
    candidate_budget(candidates)?;
    let activity = activity(workspace)?;
    let mut driver =
        c::ComparisonDriver::new(&activity, candidates).map_err(|_| ComparisonError::Limit)?;
    loop {
        operation.yield_and_checkpoint().await?;
        if driver.advance().map_err(|_| ComparisonError::Integrity)? {
            break;
        }
    }
    let matrix = driver.finish().map_err(|_| ComparisonError::Integrity)?;
    // The original control also agrees with retained supplier capacity at EVERY prefix.
    if let c::CandidateResult::Complete { steps, .. } = &matrix.original.result {
        let receipts: Vec<_> = workspace
            .settlement_rows()
            .iter()
            .filter(|r| r["kind"] == "reservation-receipt")
            .skip(1)
            .collect();
        check(receipts.len() == steps.len())?;
        for (receipt, s) in receipts.iter().zip(steps) {
            operation.yield_and_checkpoint().await?;
            let r = s.reservation.as_ref().ok_or(ComparisonError::Integrity)?;
            for (name, amount) in [
                ("maximum", r.maximum),
                ("consumed", r.consumed),
                ("held", r.held),
                ("released", r.released),
            ] {
                check(
                    receipt["body"]["result"][name].as_str() == Some(amount.to_string().as_str()),
                )?;
            }
        }
    } else {
        return Err(ComparisonError::Integrity);
    }
    let provenance = ReportProvenance::new(
        workspace,
        matrix
            .candidates
            .iter()
            .map(|c| c.fingerprint.clone())
            .collect(),
        concat!(
            "ledgerlab/",
            env!("CARGO_PKG_VERSION"),
            "/supplier-foundation-1"
        )
        .into(),
    )?;
    let mut writer = BoundedReport::new();
    raw(
        &mut writer,
        b"{\"milestone\":\"PHASE-4 FOUNDATION ONLY\",\"committed\":false,\"provenance\":",
    )?;
    emit(&mut writer, &provenance.descriptor())?;
    raw(&mut writer, b",\"basis\":")?;
    emit(&mut writer, &json!(activity.retail_basis()))?;
    raw(&mut writer, b",\"historical_receipt_refs\":")?;
    emit(&mut writer, &json!(workspace.historical_receipts()))?;
    raw(&mut writer, b",\"bindings\":")?;
    emit(&mut writer,&json!(activity.bindings().iter().map(|b|json!({"binding":b.binding_id,"agreement":b.agreement,"book":match b.book{Book::Retail=>"retail",Book::Supplier=>"supplier",_=>"unsupported"},"roles":b.roles,"booked_net":b.booked_net,"premium_limit":b.premium_limit})).collect::<Vec<_>>()))?;
    raw(&mut writer, b",\"original\":")?;
    write_candidate(&mut writer, &matrix.original, operation).await?;
    raw(&mut writer, b",\"candidates\":[")?;
    for (i, c) in matrix.candidates.iter().enumerate() {
        if i > 0 {
            raw(&mut writer, b",")?;
        }
        write_candidate(&mut writer, c, operation).await?;
    }
    raw(&mut writer, b"]}")?;
    operation.checkpoint()?;
    Ok(FoundationReport(writer.finish()?))
}

#[cfg(test)]
#[path = "comparison_foundation_tests.rs"]
pub(crate) mod tests;
