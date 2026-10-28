//! Standalone assertion of an original optional report edge. No price, claim,
//! family promise or capacity is produced. Origin/member custody is host-owned.
use super::super::selected_program::SelectedProgram;
use super::{
    authority::{self, EconomicMode},
    body,
    data::*,
    declared_links,
    emitted_record::ConstructedEnvelope,
    input_selection, meter,
    records::Records,
    representation::{InvocationToken, ParsedDraftTimes},
    shape,
    working_value::{owning_reference, WorkingValue},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::AuditArena, domain::Timestamp, validation_work as w, Error, Result,
};
use serde_json::{json, Value};
use std::mem;

pub(super) fn normalize(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    p: &mut WorkingValue,
) -> Result<()> {
    if !program.uses_component_lifecycle_recipe() {
        return Err(Error::new(
            "UNSUPPORTED_SCHEMA_PROFILE",
            "selected Component LinkAssert recipe",
        ));
    }
    shape::closed(
        m,
        p,
        &[
            "schema",
            "child",
            "product_ref",
            "binding_ref",
            "link",
            "expected_chain_revision",
            "evidence_refs",
        ],
        &[],
    )?;
    shape::equal(m, &p["schema"], &json!("ledger-workflow-link-assert/1"))?;
    for name in ["child", "product_ref", "binding_ref"] {
        shape::reference(m, &p[name])?;
    }
    shape::equal(m, &p["child"]["kind"], &json!("workflow-work"))?;
    shape::closed(m, &p["link"], &["slot", "predecessor"], &[])?;
    shape::text(m, &p["link"], "slot")?;
    shape::reference(m, &p["link"]["predecessor"])?;
    shape::equal(
        m,
        &p["link"]["predecessor"]["kind"],
        &json!("workflow-work"),
    )?;
    let n =
        super::super::facts::counter_with_work(shape::text(m, p, "expected_chain_revision")?, m)?;
    if !w::compare(m, n, 0u64)?.is_gt() {
        return Err(shape::fail("LinkAssert positive Chain revision"));
    }
    let a = arena(m)?;
    let value = super::close::counter_value(m, &a, n)?;
    p.replace(m, "expected_chain_revision", value.value())?;
    for reference in shape::array(m, p, "evidence_refs")? {
        shape::reference(m, reference)?;
    }
    let evidence_count = shape::array(m, p, "evidence_refs")?.len();
    if w::compare(m, evidence_count, 16usize)?.is_gt() {
        return Err(shape::fail("LinkAssert evidence bound"));
    }
    let mut owned = WorkingValue::copy(m, p)?.into_owned()?;
    super::intent::sort_owned_set(&mut owned, "evidence_refs", false, m, &a)?;
    *p = WorkingValue::Owned(owned);
    Ok(())
}
fn arena(m: &mut CoreMeter) -> Result<AuditArena> {
    m.representation_arena()
        .ok_or_else(|| shape::fail("owning LinkAssert producer"))
}
fn timestamp(m: &mut CoreMeter, value: &Value) -> Result<i64> {
    Ok(Timestamp::parse_with_work(
        value
            .as_str()
            .ok_or_else(|| shape::fail("LinkAssert timestamp"))?,
        m,
    )?
    .micros())
}
fn declaration<'p>(
    m: &mut CoreMeter,
    program: &'p SelectedProgram,
    work: &Value,
) -> Result<&'p Value> {
    let p = &work["payload"];
    let def = declared_links::definition(m, program, p)?
        .ok_or_else(|| shape::fail("LinkAssert original declaration"))?;
    if !declared_links::optional_wait(m, def)?
        || declared_links::validate_inputs(m, program, p)?.is_some()
    {
        return Err(shape::fail(
            "LinkAssert requires original empty optional wait slot",
        ));
    }
    // The selected late-edge recipe cannot attach a new priced report root.
    for row in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        if row.get("rule").is_some() && shape::same(m, &row["rule"]["activity"], &p["activity"])? {
            return Err(shape::fail("LinkAssert original report is priced"));
        }
    }
    Ok(def)
}
/// Authenticate original Work/Claim/initial Decision/authority and rederive its
/// original posting/evaluation rows. This does not require a fresh Work head.
fn original_work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&ExactRef, &Value, &authority::Terms),
) -> Result<()> {
    let (reference, work, terms) = selected;
    let claim_id = shape::text(m, work, "claim_id")?;
    let (claim_ref, _) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    let decision = super::work_keys::decision(m, claim_id)?;
    let id = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        &json!([decision, "1"]),
    )?;
    let (decision_ref, value) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &id,
        "ledger-workflow-decision-revision/1",
    )?;
    super::outcome::verify_original_actions(
        m,
        r,
        c,
        program,
        terms,
        reference,
        (work, &claim_ref, &decision_ref, &value),
    )
}
fn activation_open(m: &mut CoreMeter, start: i64, deadline: i64, accepted: i64) -> Result<bool> {
    Ok(w::compare(m, deadline, start)?.is_gt()
        && w::compare(m, accepted, start)?.is_ge()
        && w::compare(m, accepted, deadline)?.is_lt())
}
/// The original target and original family clock are immutable. Fresh creation
/// also repeats the exact held open head; aliases validate their old cutoff.
fn eligibility(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: (&ExactRef, &Value, &Value, &str),
    current: Option<&EconomicStateData>,
) -> Result<()> {
    let (predecessor, work, slot, at) = selected;
    let a = arena(m)?;
    let reference = owning_reference(m, &a, predecessor)?;
    let heads = input_selection::retained_work_heads(program, m, r, &c.key, reference.value())?;
    let binding_ref = shape::reference(m, &work["payload"]["binding_ref"])?;
    let binding = r.get(
        m,
        &c.key,
        &binding_ref,
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    let inst_ref = shape::reference(m, &binding["slot_instantiation_ref"])?;
    let inst = r.get(
        m,
        &c.key,
        &inst_ref,
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let mut eligible = false;
    for key in heads.family_keys() {
        m.visit("link_assert.original_family")?;
        let (target_ref, target) = r.by_identity(
            m,
            &c.key,
            "workflow-target",
            key.target_id(),
            "ledger-workflow-component-target/1",
        )?;
        let family = super::local_locators::locator_family(
            program,
            m,
            &target,
            work,
            &inst,
            key.family_slot(),
        )?;
        let path = shape::array(m, &family["selector"], "path")?;
        let Some(last) = path.last() else {
            return Err(shape::fail("LinkAssert original family path"));
        };
        if !shape::same(m, last, slot)? {
            continue;
        }
        shape::equal(m, &target["work_ref"], reference.value())?;
        for field in ["binding_ref", "product_ref", "policy_ref"] {
            shape::equal(m, &target[field], &work["payload"][field])?;
        }
        let originals = shape::array(m, &target, "families")?;
        let row = shape::named(m, originals, "family_slot", key.family_slot())?;
        shape::equal(m, &row["window_terms"], &family["window"])?;
        let work_at = shape::text(m, work, "accepted_at")?;
        let frozen = super::targets::frozen_window(m, &row["window_terms"], predecessor, work_at)?
            .ok_or_else(|| shape::fail("LinkAssert original target clock unavailable"))?;
        let accepted = Timestamp::parse_with_work(at, m)?.micros();
        let start = timestamp(m, &frozen["anchor_time"])?;
        let deadline = timestamp(m, &frozen["activation_deadline"])?;
        if !activation_open(m, start, deadline, accepted)? {
            continue;
        }
        if let Some(state) = current {
            let target_value = owning_reference(m, &a, &target_ref)?;
            let selected = super::family_context::select(
                m,
                r,
                c,
                program,
                target_value.value(),
                key.family_slot(),
                state,
            )?;
            if !shape::same(m, &selected.family_head["state"], &json!("open"))? {
                continue;
            }
            if selected.family_head.get("claim_ref").is_some()
                || selected.family_head.get("decision_ref").is_some()
            {
                return Err(shape::fail("LinkAssert open family already claimed"));
            }
            shape::equal(m, &selected.family_head["window"], &frozen)?;
        }
        eligible = true;
    }
    if !eligible {
        return Err(Error::new(
            "TARGET_CLOSED",
            "no original unclaimed family before activation deadline",
        ));
    }
    Ok(())
}
/// Late Link original authority/clock validation; completed LinkAssert producer
/// membership and its saved prestate remain the existing host origin contract.
pub(super) fn retained(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    work_ref: &ExactRef,
    work: &Value,
) -> Result<ExactRef> {
    if !program.uses_component_lifecycle_recipe() {
        return Err(shape::fail("late Link selected Component recipe"));
    }
    let def = declaration(m, program, work)?;
    let (reference, link) = r
        .declared_work_link(m, &c.key, work_ref, &def["name"])?
        .ok_or_else(|| Error::new("MISSING_DEPENDENCY", "declared optional edge is waiting"))?;
    shape::closed(
        m,
        &link,
        &[
            "schema",
            "scope",
            "child",
            "product_ref",
            "binding_ref",
            "slot",
            "predecessor",
            "classification",
            "authority_ref",
            "operation_key",
        ],
        &[],
    )?;
    let a = arena(m)?;
    let child = owning_reference(m, &a, work_ref)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    for (name, expected) in [
        ("scope", &key.value()["scope"]),
        ("child", child.value()),
        ("product_ref", &work["payload"]["product_ref"]),
        ("binding_ref", &work["payload"]["binding_ref"]),
        ("slot", &def["name"]),
    ] {
        shape::equal(m, &link[name], expected)?;
    }
    shape::equal(m, &link["classification"], &json!("monetary"))?;
    let auth_ref = shape::reference(m, &link["authority_ref"])?;
    let auth = r.get(
        m,
        &c.key,
        &auth_ref,
        "workflow-economic-authority",
        "ledger-workflow-economic-authority/8",
    )?;
    let at = shape::text(m, &auth, "observed_at")?;
    let original_key = super::working_value::WorkingKey::parse(m, &link["operation_key"])?;
    if !shape::same_text(m, &original_key.logical_store_id, &c.key.logical_store_id)? {
        return Err(shape::fail("late Link original store"));
    }
    let child_at = timestamp(m, &work["accepted_at"])?;
    let asserted_at = Timestamp::parse_with_work(at, m)?.micros();
    if w::compare(m, asserted_at, child_at)?.is_lt() {
        return Err(shape::fail("late Link before child"));
    }
    let (_, activity) = input_selection::work_activity(program, &work["payload"], m)?;
    shape::equal(m, &auth["source_slot"], &activity["source_slot"])?;
    authority::historical_namespace(
        m,
        r,
        c,
        &link["authority_ref"],
        &work["source_namespace"],
        Some(&link["operation_key"]["source"]),
        (
            "link.assert",
            &work["payload"]["binding_ref"],
            &auth["observed_at"],
        ),
    )?;
    declared_links::rights(m, r, c, &auth, &def["name"])?;
    let child_at = shape::text(m, work, "accepted_at")?;
    declared_links::endpoint(
        m,
        r,
        c,
        program,
        &work["payload"],
        &link["predecessor"],
        child_at,
    )?;
    let predecessor = shape::reference(m, &link["predecessor"])?;
    let prior = r.get(
        m,
        &c.key,
        &predecessor,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    eligibility(
        m,
        r,
        c,
        program,
        (&predecessor, &prior, &def["name"], at),
        None,
    )?;
    let id = body::semantic_id(
        m,
        "wlink_",
        "workflow-link",
        &json!([
            c.key.scope,
            work_ref,
            link["product_ref"],
            link["slot"],
            link["predecessor"]
        ]),
    )?;
    if !shape::same_text(m, &id, &reference.reference().id)? {
        return Err(shape::fail("late Link original identity"));
    }
    // Copy is paid under the enclosing Records metadata and this temporary owner.
    let _copy = a.reserve_external_payload(mem::size_of::<ExactRef>() + 64 + 128 + 71, m)?;
    for s in [
        &reference.reference().kind,
        &reference.reference().id,
        &reference.reference().digest,
    ] {
        m.scan_bytes(s.len())?;
        m.write_bytes(s.len())?;
    }
    Ok(reference.reference().clone())
}

pub(super) fn produce<'a>(
    program: &SelectedProgram,
    c: &'a DraftContextData,
    mut p: WorkingValue,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData,
    m: &mut CoreMeter,
) -> Result<Construction<'a, 'a>> {
    normalize(program, m, &mut p)?;
    super::inventory::entry::local_context(m, c)?;
    let a = arena(m)?;
    // All copies/vector controls for the metadata-only construction are owned
    // before allocation; actual canonical bodies retain their individual leases.
    let bytes = mem::size_of::<Construction>()
        + 8 * mem::size_of::<ConstructedEnvelope>()
        + 8 * (mem::size_of::<ExactRef>() + 64 + 128 + 71)
        + 4 * mem::size_of::<Vec<ExactRef>>();
    let owner = a.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut r =
        Records::borrowed_selected(m, &a, &c.key, inputs, &c.source_observation_inputs, program)?;
    let child_ref = shape::reference(m, &p["child"])?;
    let work = r.get(
        m,
        &c.key,
        &child_ref,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let def = declaration(m, program, &work)?;
    shape::equal(m, &p["product_ref"], &work["payload"]["product_ref"])?;
    shape::equal(m, &p["binding_ref"], &work["payload"]["binding_ref"])?;
    shape::equal(m, &p["link"]["slot"], &def["name"])?;
    let predecessor = shape::reference(m, &p["link"]["predecessor"])?;
    let prior = r.get(
        m,
        &c.key,
        &predecessor,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let child_at = shape::text(m, &work, "accepted_at")?;
    declared_links::endpoint(
        m,
        &mut r,
        c,
        program,
        &work["payload"],
        &p["link"]["predecessor"],
        child_at,
    )?;
    let (product, activity) = input_selection::work_activity(program, &work["payload"], m)?;
    let authority_payload = WorkingValue::copy(m, &work["payload"])?
        .with_fields(m, &[("evidence_refs", &p["evidence_refs"])])?;
    let token = InvocationToken::new();
    let times = ParsedDraftTimes::parse(m, &token, &c.received_at, &c.accepted_at)?;
    let namespace = shape::text(m, &work, "source_namespace")?;
    let resolved = authority::economic_original(
        m,
        &mut r,
        c,
        &authority_payload,
        activity,
        product,
        (
            &token,
            &times,
            program,
            EconomicMode::LinkAssert,
            &[],
            namespace,
        ),
    )?;
    let auth = resolved.authority.decoded(m, 4194304)?;
    let caller = a.string_value(&c.key.source, m)?;
    shape::equal(m, &auth["source"], caller.value())?;
    shape::equal(m, &auth["source_slot"], &activity["source_slot"])?;
    declared_links::rights(m, &mut r, c, &auth, &p["link"]["slot"])?;
    original_work(m, &mut r, c, program, (&child_ref, &work, &resolved.terms))?;
    let predecessor_row = input_selection::component_retail_row(
        program,
        &prior["payload"],
        &resolved.terms.binding,
        m,
    )?;
    let retail = owning_reference(m, &a, &resolved.terms.slot_terms_ref)?;
    shape::equal(m, &predecessor_row["binding_ref"], retail.value())?;
    original_work(
        m,
        &mut r,
        c,
        program,
        (&predecessor, &prior, &resolved.terms),
    )?;
    let old_ref = state
        .financial
        .chain_head
        .as_ref()
        .ok_or_else(|| shape::fail("LinkAssert held Chain required"))?;
    let old = r.get(
        m,
        &c.key,
        old_ref,
        "workflow-chain-state",
        "ledger-workflow-chain-state/1",
    )?;
    let (revision, admitted) = super::inventory::entry::chain_head_with_links(
        m,
        &mut r,
        c,
        &work["payload"],
        old_ref,
        &old,
        &state.financial.links,
    )?;
    let expected_raw = shape::text(m, &p, "expected_chain_revision")?;
    let expected = super::arithmetic::integer(m, expected_raw, false)?;
    if !meter::cmp(m, &revision, &expected)?.is_eq() {
        return Err(Error::new("STALE_REVISION", "LinkAssert Chain revision"));
    }
    declared_links::validate_chain(
        m,
        &mut r,
        c,
        program,
        &work["payload"],
        &state.financial.links,
    )?;
    let existing = r.declared_work_link(m, &c.key, &child_ref, &p["link"]["slot"])?;
    let row_count = w::usize_add(m, resolved.introduced.len(), 3)?;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[
            &row_count.into(),
            &mem::size_of::<ConstructedEnvelope>().into(),
        ],
    )?;
    let row_bytes = row_count
        .checked_mul(mem::size_of::<ConstructedEnvelope>())
        .ok_or_else(|| shape::fail("LinkAssert output controls"))?;
    let output_owner = a.reserve_external_payload(row_bytes, m)?;
    m.write_bytes(row_bytes)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(row_count)
        .map_err(|_| shape::fail("LinkAssert output allocation"))?;
    let mut introductions = Vec::new();
    introductions
        .try_reserve_exact(1)
        .map_err(|_| shape::fail("LinkAssert permanent slot allocation"))?;
    let mut proposed = None;
    let reused = if let Some((reference, link)) = existing {
        shape::equal(m, &link["predecessor"], &p["link"]["predecessor"])?;
        let mut member = false;
        for old in &state.financial.links {
            member |= shape::same_ref(m, old, reference.reference())?;
        }
        if !member {
            return Err(shape::fail("LinkAssert alias absent from held Chain"));
        }
        let actual = retained(m, &mut r, c, program, &child_ref, &work)?;
        if !shape::same_ref(m, &actual, reference.reference())? {
            return Err(shape::fail("LinkAssert alias retained identity"));
        }
        Some(actual)
    } else {
        declared_links::current_work(m, &mut r, c, &child_ref, &work, state.financial)?;
        declared_links::current_work(m, &mut r, c, &predecessor, &prior, state.financial)?;
        eligibility(
            m,
            &mut r,
            c,
            program,
            (&predecessor, &prior, &p["link"]["slot"], &c.accepted_at),
            Some(state),
        )?;
        check_new_edge(
            m,
            &mut r,
            c,
            &child_ref,
            &predecessor,
            &state.financial.links,
        )?;
        let key = super::cost_postings::operation_key(m, &a, c)?;
        let id = body::semantic_id(
            m,
            "wlink_",
            "workflow-link",
            &json!([
                c.key.scope,
                child_ref,
                p["product_ref"],
                p["link"]["slot"],
                p["link"]["predecessor"]
            ]),
        )?;
        let link_value = a.object_from_fields(
            &[
                ("schema", &json!("ledger-workflow-link/1")),
                ("scope", &key.value()["scope"]),
                ("child", &p["child"]),
                ("product_ref", &p["product_ref"]),
                ("binding_ref", &p["binding_ref"]),
                ("slot", &p["link"]["slot"]),
                ("predecessor", &p["link"]["predecessor"]),
                ("classification", &json!("monetary")),
                ("authority_ref", &json!(resolved.authority.reference)),
                ("operation_key", key.value()),
            ],
            m,
        )?;
        let link = body::record_borrowed(m, &c.key, "workflow-link", id, link_value.value())?;
        let count = w::usize_add(m, state.financial.links.len(), 1)?;
        w::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[&count.into(), &mem::size_of::<&ExactRef>().into()],
        )?;
        let n = count
            .checked_mul(mem::size_of::<&ExactRef>())
            .ok_or_else(|| shape::fail("LinkAssert Chain set size"))?;
        let _refs = a.reserve_external_payload(n + mem::size_of::<Vec<&ExactRef>>(), m)?;
        m.write_bytes(n)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(count)
            .map_err(|_| shape::fail("LinkAssert Chain set allocation"))?;
        refs.extend(state.financial.links.iter());
        refs.push(&link.reference);
        let set = super::app_cost::reference_set(m, &a, &refs)?;
        let root = super::records::hash(m, "workflow-link-set", set.value())?;
        let next = meter::add(m, &revision, &1.into())?;
        let next = super::cost_postings::scalar(m, &a, &next)?;
        let chain_id = body::semantic_id(
            m,
            "wchain_",
            "workflow-chain-state",
            &json!([
                c.key.logical_store_id,
                c.key.scope,
                work["payload"]["customer"],
                work["payload"]["chain"],
                next.value()
            ]),
        )?;
        let old_value = WorkingValue::copy(m, &old)?;
        let now = a.string_value(&c.accepted_at, m)?;
        let root = a.string_value(&root, m)?;
        let old_ref_value = owning_reference(m, &a, old_ref)?;
        let chain_value = old_value.with_fields(
            m,
            &[
                ("revision", next.value()),
                ("prior_ref", old_ref_value.value()),
                ("links_root", root.value()),
                ("operation_key", key.value()),
                ("accepted_at", now.value()),
            ],
        )?;
        let chain =
            body::record_borrowed(m, &c.key, "workflow-chain-state", chain_id, &chain_value)?;
        // Count is copied exactly; asserting an edge is not a Work/Outcome event.
        let _ = admitted;
        introductions.push(link.reference.clone());
        proposed = Some(chain.reference.clone());
        rows.push(link);
        rows.push(chain);
        None
    };
    rows.push(resolved.authority);
    for row in resolved.introduced {
        rows.push(row);
    }
    Ok(Construction {
        economic: EconomicConstruction {
            terms: resolved.terms,
            original_work: OriginalWorkPayload::Unattached,
        },
        original_reservations: None,
        financial_reservations: None,
        normalized_payload: p.bytes(m)?,
        new_envelopes: rows,
        reused_result: reused,
        evaluation_steps: Vec::new(),
        proposed_heads: ProposedHeads {
            decision: Vec::new(),
            family: Vec::new(),
            chain: proposed,
        },
        claim_introductions: Vec::new(),
        link_introductions: introductions,
        prospective_obligations: Vec::new(),
        records: r,
        history_edges: Vec::new(),
        app_effects: None,
        _app_causal: None,
        current_capacity: None,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: [
            Some(owner),
            Some(output_owner),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ],
    })
}
fn check_new_edge(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    child: &ExactRef,
    predecessor: &ExactRef,
    links: &[ExactRef],
) -> Result<()> {
    let a = arena(m)?;
    let mut next = owning_reference(m, &a, predecessor)?;
    let child = owning_reference(m, &a, child)?;
    let mut inbound = 1usize;
    for reference in links {
        let link = r.get(
            m,
            &c.key,
            reference,
            "workflow-link",
            "ledger-workflow-link/1",
        )?;
        if shape::same(m, &link["predecessor"], next.value())? {
            inbound = w::usize_add(m, inbound, 1)?;
        }
    }
    if w::compare(m, inbound, 128usize)?.is_gt() {
        return Err(Error::new(
            "LINK_CARDINALITY",
            "predecessor inbound ceiling",
        ));
    }
    for depth in 0..=4 {
        if shape::same(m, next.value(), child.value())? {
            return Err(Error::new("LINK_CYCLE", "asserted declared cycle"));
        }
        let mut edge = None;
        for reference in links {
            let link = r.get(
                m,
                &c.key,
                reference,
                "workflow-link",
                "ledger-workflow-link/1",
            )?;
            if shape::same(m, &link["child"], next.value())? {
                if edge.is_some() {
                    return Err(Error::new(
                        "LINK_CARDINALITY",
                        "multiple declared predecessor slots",
                    ));
                }
                edge = Some(a.clone_value(&link["predecessor"], m)?);
            }
        }
        let Some(edge) = edge else { return Ok(()) };
        if depth == 4 {
            return Err(Error::new(
                "LINK_CYCLE",
                "declared path exceeds fixed bound",
            ));
        }
        next = edge;
    }
    Err(shape::fail("LinkAssert fixed path"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::{
            document_digest,
            selected_program::{
                compile_selected_publication, load_local_selected_execution,
                SelectedCompilerSupport, SelectedSourceInput,
            },
        },
        validation_work::{OriginalAuditSession, ResourceLimits},
    };
    fn source(
        support: &SelectedCompilerSupport,
        s: &mut OriginalAuditSession,
    ) -> SelectedSourceInput {
        let input = super::super::pricing::component_tests::component_input(support, s);
        let mut docs: Vec<Value> = input
            .documents
            .iter()
            .map(|r| serde_json::from_slice(r).unwrap())
            .collect();
        let extra: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-declared-link/source.json"
        ))
        .unwrap();
        let schema = extra["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["schema_id"] == "support:report-facts")
            .unwrap()
            .clone();
        let digest = document_digest("ledger-workflow-fact-schema/1", &schema).unwrap();
        let schema_ref = json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
        docs.push(schema);
        let product = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-product/1")
            .unwrap();
        let old_digest = document_digest("ledger-workflow-product/1", product).unwrap();
        let old_ref = json!({"kind":"workflow-product","id":format!("wdoc_{}",&old_digest[7..]),"digest":old_digest});
        let activity = product["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["primitive"] == "work")
            .unwrap()
            .clone();
        for name in ["support:report-b", "support:report-c"] {
            let mut report = activity.clone();
            report["name"] = json!(name);
            report["fact_schema"] = schema_ref.clone();
            product["activities"].as_array_mut().unwrap().push(report);
        }
        product["fact_schemas"]
            .as_array_mut()
            .unwrap()
            .push(schema_ref);
        let link = |name: &str,
                    child: &str,
                    predecessor: &str,
                    primitive: &str,
                    minimum: &str,
                    missing: &str| {
            json!({
            "name":name,"child_activity":child,"child_primitive":primitive,"predecessor_activities":[predecessor],"predecessor_primitive":"work",
            "minimum":minimum,"maximum":"1","binding_relation":"same_binding","max_depth":"4","on_missing":missing})
        };
        product["links"] = json!([
            link(
                "support:assesses",
                "support:assess",
                "support:report-c",
                "outcome",
                "1",
                "refuse"
            ),
            link(
                "support:c-b",
                "support:report-c",
                "support:report-b",
                "work",
                "1",
                "refuse"
            ),
            link(
                "support:b-a",
                "support:report-b",
                "support:complete",
                "work",
                "0",
                "wait"
            )
        ]);
        let digest = document_digest("ledger-workflow-product/1", product).unwrap();
        let new_ref =
            json!({"kind":"workflow-product","id":format!("wdoc_{}",&digest[7..]),"digest":digest});
        let policy = docs
            .iter_mut()
            .find(|d| d["schema"] == "ledger-workflow-policy/4")
            .unwrap();
        assert_eq!(policy["product_ref"], old_ref);
        policy["product_ref"] = new_ref;
        for family in policy["families"].as_array_mut().unwrap() {
            family["selector"]["path"] = json!(["support:assesses", "support:c-b", "support:b-a"]);
            family["window"]["activation_deadline_us"] = json!("1000000");
        }
        let data = json!({"documents":docs,"root_policy_ref":input.root_policy_ref});
        let data = crate::policy::workflow::selected_program::stage_fixture_normalized_source(
            &data, support, s,
        )
        .unwrap();
        SelectedSourceInput {
            documents: data["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| canonical::CanonicalBytes::from_value(d).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(data["root_policy_ref"].clone()).unwrap(),
        }
    }
    fn reference(kind: &str, prefix: &str, n: char) -> Value {
        let digest = std::iter::repeat_n(n, 64).collect::<String>();
        json!({"kind":kind,"id":format!("{prefix}{digest}"),"digest":format!("sha256:{digest}")})
    }
    #[test]
    fn link_assert_actual_component_optional_birth_reload_and_normalization() {
        let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut s)
                .unwrap();
        let input = source(&support, &mut s);
        let saved_docs = input.documents.clone();
        let root = input.root_policy_ref.clone();
        let program = compile_selected_publication(input, &support, &mut s).unwrap();
        program
            .require_local_admission_with_session(&mut s)
            .unwrap();
        let plan = canonical::CanonicalBytes::from_value(program.compiled_plan().canonical_value())
            .unwrap();
        let b = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut replay = OriginalAuditSession::for_resource_profile_v2(&b).unwrap();
        let reloaded = load_local_selected_execution(
            SelectedSourceInput {
                documents: saved_docs,
                root_policy_ref: root,
            },
            plan.as_slice(),
            program.compiled_plan().reference(),
            &mut replay,
        )
        .unwrap();
        let payload =
            json!({"product_ref":reloaded.product_ref(),"activity":"support:report-b","links":[]});
        assert!(
            declared_links::validate_inputs(replay.meter_mut(), &reloaded, &payload)
                .unwrap()
                .is_none()
        );
        let mandatory =
            json!({"product_ref":reloaded.product_ref(),"activity":"support:report-c","links":[]});
        assert!(
            declared_links::validate_inputs(replay.meter_mut(), &reloaded, &mandatory).is_err()
        );
        let original = json!({"schema":"ledger-workflow-link-assert/1","child":reference("workflow-work","wevent_",'a'),"product_ref":reloaded.product_ref(),
            "binding_ref":reference("workflow-binding","wdoc_",'b'),"link":{"slot":"support:b-a","predecessor":reference("workflow-work","wevent_",'c')},
            "expected_chain_revision":"3","evidence_refs":[reference("workflow-source-grant","wdoc_",'e'),reference("workflow-source-grant","wdoc_",'d')]});
        let mut p = WorkingValue::copy(replay.meter_mut(), &original).unwrap();
        normalize(&reloaded, replay.meter_mut(), &mut p).unwrap();
        assert_ne!(*p, original); // only set order changes
        let mut changed = WorkingValue::copy(replay.meter_mut(), &original).unwrap();
        changed
            .replace(replay.meter_mut(), "expected_chain_revision", &json!("4"))
            .unwrap();
        normalize(&reloaded, replay.meter_mut(), &mut changed).unwrap();
        assert_ne!(*changed, *p);
        let mut duplicate = original.clone();
        duplicate["evidence_refs"][1] = duplicate["evidence_refs"][0].clone();
        let mut duplicate = WorkingValue::copy(replay.meter_mut(), &duplicate).unwrap();
        assert!(normalize(&reloaded, replay.meter_mut(), &mut duplicate).is_err());
        // The old exact profile still requires inline1 and cannot admit LinkAssert.
        let c = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut old = OriginalAuditSession::for_resource_profile_v2(&c).unwrap();
        let legacy_support =
            SelectedCompilerSupport::local_declared_link_execution_with_session(&mut old).unwrap();
        let raw: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-declared-link/source.json"
        ))
        .unwrap();
        let mut documents: Vec<Vec<u8>> = raw["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
            .collect();
        documents.push(legacy_support.canonical_bytes().to_vec());
        let legacy = compile_selected_publication(
            SelectedSourceInput {
                documents,
                root_policy_ref: serde_json::from_value(raw["root_policy_ref"].clone()).unwrap(),
            },
            &legacy_support,
            &mut old,
        )
        .unwrap();
        let mut p = WorkingValue::copy(old.meter_mut(), &original).unwrap();
        assert!(normalize(&legacy, old.meter_mut(), &mut p).is_err());
        let omitted =
            json!({"product_ref":legacy.product_ref(),"activity":"support:report","links":[]});
        assert!(declared_links::validate_inputs(old.meter_mut(), &legacy, &omitted).is_err());
        let d = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut bad = OriginalAuditSession::for_resource_profile_v2(&d).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut bad)
                .unwrap();
        let raw = source(&support, &mut bad);
        let mut docs: Vec<Value> = raw
            .documents
            .iter()
            .map(|r| serde_json::from_slice(r).unwrap())
            .collect();
        let policy = docs
            .iter_mut()
            .find(|v| v["schema"] == "ledger-workflow-policy/4")
            .unwrap();
        let fixed = policy["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["component_key"] == "support:priority")
            .unwrap();
        fixed["activity"] = json!("support:report-b");
        let changed = crate::policy::workflow::selected_program::stage_fixture_normalized_source(
            &json!({"documents":docs,"root_policy_ref":raw.root_policy_ref}),
            &support,
            &mut bad,
        )
        .unwrap();
        let changed = SelectedSourceInput {
            documents: changed["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(changed["root_policy_ref"].clone()).unwrap(),
        };
        assert!(compile_selected_publication(changed, &support, &mut bad).is_err());
    }
    #[test]
    fn link_assert_fixed_slot_inventory_keeps_exact_ref_and_refuses_replacement() {
        let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let support =
            SelectedCompilerSupport::local_component_lifecycle_execution_with_session(&mut s)
                .unwrap();
        let input = source(&support, &mut s);
        let program = compile_selected_publication(input, &support, &mut s).unwrap();
        // Compiler child and parent DATA selection are separate original audit
        // attempts in the same owning arena; no guard reset or new allowance.
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let key = OperationKey {
            logical_store_id: "test-store".into(),
            scope: ["test".into(), "local".into()],
            source: "reporter".into(),
            operation_id: "original-assertion".into(),
        };
        let child = reference("workflow-work", "wevent_", 'a');
        let predecessor = reference("workflow-work", "wevent_", 'b');
        let binding = reference("workflow-binding", "wdoc_", 'c');
        let assertion = reference("workflow-economic-authority", "wdoc_", 'd');
        let value = json!({"schema":"ledger-workflow-link/1","scope":key.scope,"child":child,"product_ref":program.product_ref(),"binding_ref":binding,
            "slot":"support:b-a","predecessor":predecessor,"classification":"monetary","authority_ref":assertion,"operation_key":key});
        let tuple = json!([
            key.scope,
            child,
            program.product_ref(),
            "support:b-a",
            predecessor
        ]);
        let id = body::semantic_id(s.meter_mut(), "wlink_", "workflow-link", &tuple).unwrap();
        let first =
            body::record_borrowed(s.meter_mut(), &key, "workflow-link", id, &value).unwrap();
        let original = first.reference.clone();
        let views = [EconomicRecordView {
            reference: &first.reference,
            canonical_body: &first.canonical_body,
        }];
        let child: ExactRef = serde_json::from_value(child).unwrap();
        let mut r =
            Records::borrowed_parent_selected(s.meter_mut(), &a, &key, &views, &program).unwrap();
        let (found, retained) = r
            .declared_work_link(s.meter_mut(), &key, &child, &json!("support:b-a"))
            .unwrap()
            .unwrap();
        assert_eq!(found.reference(), &original);
        assert_eq!(&*retained, &value);
        assert!(r
            .declared_work_link(s.meter_mut(), &key, &child, &json!("support:other"))
            .unwrap()
            .is_none());
        let mut replacement = value.clone();
        replacement["predecessor"] = reference("workflow-work", "wevent_", 'e');
        let tuple = json!([
            key.scope,
            child,
            program.product_ref(),
            "support:b-a",
            replacement["predecessor"]
        ]);
        let id = body::semantic_id(s.meter_mut(), "wlink_", "workflow-link", &tuple).unwrap();
        let second =
            body::record_borrowed(s.meter_mut(), &key, "workflow-link", id, &replacement).unwrap();
        let views = [
            EconomicRecordView {
                reference: &first.reference,
                canonical_body: &first.canonical_body,
            },
            EconomicRecordView {
                reference: &second.reference,
                canonical_body: &second.canonical_body,
            },
        ];
        let mut r =
            Records::borrowed_parent_selected(s.meter_mut(), &a, &key, &views, &program).unwrap();
        assert!(r
            .declared_work_link(s.meter_mut(), &key, &child, &json!("support:b-a"))
            .is_err());
        // Native canonical DATA only: this inventory test grants no authority
        // and deliberately does not claim original producer/member custody.
    }
    #[test]
    fn link_assert_original_activation_boundary_does_not_extend_or_invent_deadline() {
        let mut m = CoreMeter::new(65536).unwrap();
        assert!(activation_open(&mut m, 100, 200, 100).unwrap());
        assert!(activation_open(&mut m, 100, 200, 199).unwrap());
        assert!(!activation_open(&mut m, 100, 200, 200).unwrap());
        assert!(!activation_open(&mut m, 100, 200, 99).unwrap());
        assert!(!activation_open(&mut m, 100, 100, 100).unwrap());
    }
}
