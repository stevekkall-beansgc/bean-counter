//! Same immutable report certificate joins for a fixed declared path <=4.
use super::*;
use crate::canonical::audit_arena::AuditArena;
fn hop(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    selected: (&ExactRef, &Value, &Value, &Value),
    current: Option<&DraftStateData>,
) -> Result<ReportHop> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning component report hop"))?;
    let reference_value = super::super::working_value::owning_reference(m, &arena, selected.0)?;
    let reference = shape::reference(m, reference_value.value())?;
    let work = r.get(
        m,
        &c.key,
        &reference,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::equal(m, &work["payload"]["binding_ref"], &p["binding_ref"])?;
    shape::equal(m, &work["payload"]["policy_ref"], &p["policy_ref"])?;
    shape::equal(m, &work["payload"]["product_ref"], &p["product_ref"])?;
    shape::equal(m, &work["payload"]["status"], &json!("succeeded"))?;
    let input = validate_inputs(m, program, &work["payload"])?
        .ok_or_else(|| shape::fail("report declared predecessor"))?;
    shape::equal(m, &input["slot"], selected.3)?;
    let product = product(m, program, p)?;
    let defs = shape::array(m, product, "links")?;
    let slot = selected
        .2
        .as_str()
        .ok_or_else(|| shape::fail("component declared path slot"))?;
    let def = shape::named(m, defs, "name", slot)?;
    shape::equal(m, &def["child_activity"], selected.1)?;
    let predecessor_activities = shape::array(m, def, "predecessor_activities")?;
    if !shape::member(m, predecessor_activities, &work["payload"]["activity"])? {
        return Err(shape::fail("component declared predecessor activity"));
    }
    let link = retained_work(m, r, c, program, &reference, &work)?;
    let claim_id = shape::text(m, &work, "claim_id")?;
    let (claim, _) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    let decision_id = super::super::work_keys::decision(m, claim_id)?;
    let id = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision_id, "1"]),
    )?;
    let (decision, _) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &id,
        "ledger-workflow-decision-revision/1",
    )?;
    if let Some(state) = current {
        current_work(m, r, c, &reference, &work, state)?;
    }
    Ok(ReportHop {
        reference,
        work,
        link,
        claim,
        decision,
    })
}
pub(super) fn select(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    selected: (&ExactRef, &Value),
    current: Option<&DraftStateData>,
) -> Result<Option<ReportPath>> {
    let (target, family) = selected;
    let path = shape::array(m, &family["selector"], "path")?;
    if path.len() == 1 {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("Component direct selector owner"))?;
        let work = super::super::working_value::owning_reference(m, &arena, target)?;
        shape::equal(m, &p["selector_start_work_ref"], work.value())?;
        shape::equal(m, &family["target_link_slot"], &path[0])?;
        return Ok(None);
    }
    if path.len() < 2 || w::compare(m, path.len(), 4usize)?.is_gt() {
        return Err(shape::fail("component declared path bound"));
    }
    shape::equal(m, &family["target_link_slot"], &path[0])?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning component report path"))?;
    let count = path.len() - 1;
    let per =
        std::mem::size_of::<ReportHop>() + 4 * (std::mem::size_of::<ExactRef>() + 64 + 128 + 71);
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&count.into(), &per.into()],
    )?;
    let bytes = count
        .checked_mul(per)
        .ok_or_else(|| shape::fail("component report storage"))?;
    let bytes = w::usize_add(m, bytes, std::mem::size_of::<ReportPath>())?;
    let owner = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut reports: Vec<ReportHop> = Vec::new();
    reports
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("component report allocation"))?;
    let mut next = arena.clone_value(&p["selector_start_work_ref"], m)?;
    let mut child = arena.clone_value(&p["activity"], m)?;
    for index in 0..count {
        m.visit("component.exact_report_step")?;
        let reference = shape::reference(m, next.value())?;
        for old in &reports {
            if shape::same_ref(m, &old.reference, &reference)? {
                return Err(shape::fail("component report cycle"));
            }
        }
        let value = hop(
            m,
            r,
            c,
            program,
            p,
            (&reference, child.value(), &path[index], &path[index + 1]),
            current,
        )?;
        let input = validate_inputs(m, program, &value.work["payload"])?
            .ok_or_else(|| shape::fail("component report link missing"))?;
        next = arena.clone_value(&input["predecessor"], m)?;
        child = arena.clone_value(&value.work["payload"]["activity"], m)?;
        reports.push(value);
    }
    let expected = super::super::working_value::owning_reference(m, &arena, target)?;
    shape::equal(m, next.value(), expected.value())?;
    let target_work = r.get(
        m,
        &c.key,
        target,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let product = product(m, program, p)?;
    let defs = shape::array(m, product, "links")?;
    let slot = path
        .last()
        .and_then(Value::as_str)
        .ok_or_else(|| shape::fail("component target path slot"))?;
    let def = shape::named(m, defs, "name", slot)?;
    shape::equal(m, &def["child_activity"], child.value())?;
    let allowed = shape::array(m, def, "predecessor_activities")?;
    if !shape::member(m, allowed, &target_work["payload"]["activity"])? {
        return Err(shape::fail("component selected target activity"));
    }
    for _ in 1..reports.len() {
        m.visit("component.report_move")?;
        m.scan_bytes(std::mem::size_of::<ReportHop>())?;
        m.write_bytes(std::mem::size_of::<ReportHop>())?;
    }
    let first = reports.remove(0);
    Ok(Some(ReportPath {
        reference: first.reference,
        work: first.work,
        link: first.link,
        claim: first.claim,
        decision: first.decision,
        extra: reports,
        _owner: owner,
    }))
}
pub(in crate::policy::workflow::transition_constructor) fn effect(
    m: &mut CoreMeter,
    scope: &[String; 2],
    agreement: &Value,
    root: &str,
    claim: &ExactRef,
    family: &Value,
    selected: (&ExactRef, Option<&ReportPath>),
) -> Result<String> {
    let (closing, report) = selected;
    let second = report.map(|r| &r.link);
    if report.is_none_or(|r| r.extra.is_empty()) {
        return super::super::outcome::original_family_effect_path(
            m,
            scope,
            agreement,
            root,
            claim,
            family,
            (closing, second),
        );
    }
    let report = report.ok_or_else(|| shape::fail("full report path unavailable"))?;
    let arena: AuditArena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning selector effect"))?;
    let count = w::usize_add(m, report.extra.len(), 2)?;
    let per = std::mem::size_of::<crate::canonical::audit_arena::AuditedValue>()
        + std::mem::size_of::<&Value>();
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&count.into(), &per.into()],
    )?;
    let bytes = count
        .checked_mul(per)
        .ok_or_else(|| shape::fail("selector effect dimensions"))?;
    let bytes = w::usize_add(m, bytes, 2 * std::mem::size_of::<Vec<usize>>())?;
    let _storage = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(count)
        .map_err(|_| shape::fail("selector effect allocation"))?;
    ids.push(arena.string_value(&closing.id, m)?);
    ids.push(arena.string_value(&report.link.id, m)?);
    for hop in &report.extra {
        ids.push(arena.string_value(&hop.link.id, m)?);
    }
    let mut refs = Vec::new();
    refs.try_reserve_exact(count)
        .map_err(|_| shape::fail("selector effect references"))?;
    for id in &ids {
        m.visit("selector.effect_ref")?;
        refs.push(id.value());
    }
    let links = arena.array_from_values(&refs, m)?;
    let mut holder = arena.object_from_fields(&[("links", links.value())], m)?;
    super::super::intent::sort_owned_set(&mut holder, "links", false, m, &arena)?;
    let first = arena.string_value(&scope[0], m)?;
    let second = arena.string_value(&scope[1], m)?;
    let scope = arena.array_from_values(&[first.value(), second.value()], m)?;
    let root = arena.string_value(root, m)?;
    let claim = arena.string_value(&claim.id, m)?;
    let matched = arena.array_from_values(&[family, &holder.value()["links"], root.value()], m)?;
    let original = arena.string_value("original", m)?;
    let tuple = arena.array_from_values(
        &[
            scope.value(),
            agreement,
            root.value(),
            claim.value(),
            matched.value(),
            original.value(),
        ],
        m,
    )?;
    body::semantic_id(m, "weffect_", "workflow-effect", tuple.value())
}
