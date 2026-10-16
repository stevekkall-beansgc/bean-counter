//! Relational checks on explicit source/assent data. These are not host proofs.
use super::{
    CoreMeter, Phase, arithmetic, body,
    data::{DraftContextData, DraftEnvelope, ExactRef},
    meter,
    records::Records,
    representation::{InvocationToken, ParsedDraftTimes, ParsedGrantInterval, ParsedRef},
    shape::{self, *},
};
use crate::{Result, domain::Timestamp};
use num_bigint::BigInt;
use serde_json::{Value, json};
macro_rules! invoke {
    ($f:ident,$m:expr,$v0:expr) => {{
        let (v0,) = ($v0,);
        $f($m, v0)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr) => {{
        let (v0, v1) = ($v0, $v1);
        $f($m, v0, v1)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr) => {{
        let (v0, v1, v2) = ($v0, $v1, $v2);
        $f($m, v0, v1, v2)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr) => {{
        let (v0, v1, v2, v3) = ($v0, $v1, $v2, $v3);
        $f($m, v0, v1, v2, v3)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr) => {{
        let (v0, v1, v2, v3, v4) = ($v0, $v1, $v2, $v3, $v4);
        $f($m, v0, v1, v2, v3, v4)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr,$v5:expr) => {{
        let (v0, v1, v2, v3, v4, v5) = ($v0, $v1, $v2, $v3, $v4, $v5);
        $f($m, v0, v1, v2, v3, v4, v5)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr,$v5:expr,$v6:expr) => {{
        let (v0, v1, v2, v3, v4, v5, v6) = ($v0, $v1, $v2, $v3, $v4, $v5, $v6);
        $f($m, v0, v1, v2, v3, v4, v5, v6)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr,$v5:expr,$v6:expr,$v7:expr) => {{
        let (v0, v1, v2, v3, v4, v5, v6, v7) = ($v0, $v1, $v2, $v3, $v4, $v5, $v6, $v7);
        $f($m, v0, v1, v2, v3, v4, v5, v6, v7)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr,$v5:expr,$v6:expr,$v7:expr,$v8:expr) => {{
        let (v0, v1, v2, v3, v4, v5, v6, v7, v8) = ($v0, $v1, $v2, $v3, $v4, $v5, $v6, $v7, $v8);
        $f($m, v0, v1, v2, v3, v4, v5, v6, v7, v8)
    }};
    ($f:ident,$m:expr,$v0:expr,$v1:expr,$v2:expr,$v3:expr,$v4:expr,$v5:expr,$v6:expr,$v7:expr,$v8:expr,$v9:expr) => {{
        let (v0, v1, v2, v3, v4, v5, v6, v7, v8, v9) =
            ($v0, $v1, $v2, $v3, $v4, $v5, $v6, $v7, $v8, $v9);
        $f($m, v0, v1, v2, v3, v4, v5, v6, v7, v8, v9)
    }};
}
#[derive(Clone, Copy)]
pub(super) enum EconomicMode {
    Work,
    OutcomeSubmit,
    OutcomeCorrect,
}
impl EconomicMode {
    fn operation(self) -> &'static str {
        match self {
            Self::Work => "work.accept",
            Self::OutcomeSubmit => "outcome.submit",
            Self::OutcomeCorrect => "outcome.correct",
        }
    }
    fn rights(self) -> &'static [&'static str] {
        match self {
            Self::Work | Self::OutcomeSubmit => &["read", "submit"],
            Self::OutcomeCorrect => &["correct", "read"],
        }
    }
}
pub(super) struct Operands {
    pub binding: Value,
    pub accepted: Value,
    pub activation: Value,
    pub catalog: Value,
    pub instantiation: Value,
    pub slot_terms: Value,
    pub slot_terms_ref: ExactRef,
    pub role_ref: ExactRef,
    pub authority: DraftEnvelope,
    pub introduced: Vec<DraftEnvelope>,
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    owner: &InvocationToken,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, Value)> {
    let reference = ParsedRef::parse(m, owner, v)?;
    let body = r.get_parsed(m, owner, &c.key, &reference, kind, schema)?;
    Ok((reference.exact().clone(), body))
}
fn time(m: &mut CoreMeter, v: &Value, k: &str) -> Result<BigInt> {
    let raw = text(m, v, k)?;
    Ok(m.scoped(meter::timestamp_site(k), |m| {
        Timestamp::parse_with_work(raw, m)
    })?
    .micros()
    .into())
}
fn interval(
    m: &mut CoreMeter,
    owner: &InvocationToken,
    v: &ParsedGrantInterval<'_>,
    at: &BigInt,
) -> Result<()> {
    let (start, end) = v.get(m, owner)?;
    let start = BigInt::from(start.micros());
    let end = BigInt::from(end.micros());
    if meter::cmp(m, at, &start)?.is_lt() || !meter::cmp(m, at, &end)?.is_lt() {
        return Err(fail("grant interval"));
    }
    Ok(())
}
fn selector(m: &mut CoreMeter, v: &Value, k: &str, name: &str) -> Result<()> {
    let mut found = false;
    for s in invoke!(array, m, v, k)? {
        closed(m, s, &["names"], &[])?;
        if invoke!(member, m, invoke!(array, m, s, "names")?, &json!(name))? {
            found = true;
        }
    }
    if !found {
        return Err(fail("selector coverage"));
    }
    Ok(())
}
fn grant(
    m: &mut CoreMeter,
    owner: &InvocationToken,
    g: &Value,
    ceiling: &Value,
    c: &DraftContextData,
    customer: &Value,
    activity: &str,
    product: &str,
    slot: &str,
    at: &BigInt,
    mode: EconomicMode,
) -> Result<()> {
    closed(
        m,
        g,
        &[
            "schema",
            "grant_id",
            "revision",
            "original_ceiling_ref",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "customer_scope",
            "operations",
            "product_selectors",
            "activity_selectors",
            "link_selectors",
            "source_slots",
            "permissions",
            "starts_at",
            "ends_at",
        ],
        &[],
    )?;
    for (k, want) in [
        ("principal_id", json!(c.actor)),
        ("source", json!(c.key.source)),
        (
            "canonical_economic_namespace",
            json!(c.canonical_economic_namespace),
        ),
    ] {
        invoke!(equal, m, invoke!(field, m, g, k)?, &want)?;
        invoke!(equal, m, invoke!(field, m, ceiling, k)?, &want)?;
    }
    for k in [
        "customer_scope",
        "operations",
        "source_slots",
        "permissions",
    ] {
        invoke!(
            subset,
            m,
            invoke!(array, m, g, k)?,
            invoke!(array, m, ceiling, k)?
        )?;
    }
    for k in ["product_selectors", "activity_selectors", "link_selectors"] {
        invoke!(
            subset,
            m,
            invoke!(array, m, g, k)?,
            invoke!(array, m, ceiling, k)?
        )?;
    }
    let prepared = ParsedGrantInterval::parse(m, owner, g)?;
    let (gs, ge) = prepared.get(m, owner)?;
    let gs = BigInt::from(gs.micros());
    let ge = BigInt::from(ge.micros());
    let cs = invoke!(time, m, ceiling, "starts_at")?;
    let ce = invoke!(time, m, ceiling, "ends_at")?;
    if meter::cmp(m, &gs, &cs)?.is_lt() || meter::cmp(m, &ge, &ce)?.is_gt() {
        return Err(fail("original ceiling time"));
    }
    invoke!(interval, m, owner, &prepared, at)?;
    invoke!(
        contains,
        m,
        invoke!(array, m, g, "customer_scope")?,
        customer
    )?;
    invoke!(
        contains,
        m,
        invoke!(array, m, g, "operations")?,
        &json!(mode.operation())
    )?;
    for right in mode.rights() {
        invoke!(
            contains,
            m,
            invoke!(array, m, g, "permissions")?,
            &json!(right)
        )?;
    }
    invoke!(
        contains,
        m,
        invoke!(array, m, g, "source_slots")?,
        &json!(slot)
    )?;
    invoke!(selector, m, g, "activity_selectors", activity)?;
    invoke!(selector, m, g, "product_selectors", product)?;
    Ok(())
}
pub(super) fn work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &Value,
    activity: &Value,
    product: &Value,
    owner: &InvocationToken,
    times: &ParsedDraftTimes<'_>,
    program: &super::super::selected_program::SelectedProgram,
) -> Result<Operands> {
    economic(
        m,
        r,
        c,
        p,
        activity,
        product,
        owner,
        times,
        program,
        EconomicMode::Work,
        &[],
    )
}
pub(super) fn economic(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &Value,
    activity: &Value,
    product: &Value,
    owner: &InvocationToken,
    times: &ParsedDraftTimes<'_>,
    program: &super::super::selected_program::SelectedProgram,
    mode: EconomicMode,
    verified_facts: &[ExactRef],
) -> Result<Operands> {
    m.phase(Phase::OriginalAuthority);
    if !meter::cmp(m, &c.authorities.len().into(), &1usize.into())?.is_eq() {
        return Err(fail("one required nominated work slot"));
    }
    let a = &c.authorities[0];
    let source_slot = invoke!(text, m, activity, "source_slot")?;
    invoke!(equal, m, &json!(a.source_slot), &json!(source_slot))?;
    invoke!(
        equal,
        m,
        &json!(a.binding_ref),
        invoke!(field, m, p, "binding_ref")?
    )?;
    let binding = r.get(
        m,
        &c.key,
        &a.binding_ref,
        "workflow-binding",
        "ledger-workflow-binding/1",
    )?;
    closed(
        m,
        &binding,
        &[
            "schema",
            "agreement_id",
            "version",
            "customer",
            "source_namespace",
            "slot_catalog_ref",
            "slot_instantiation_ref",
            "roles_refs",
            "assent_refs",
            "authority_refs",
            "source_slots",
            "accepted_context",
            "bindings",
            "funding_legs",
            "starts_at",
            "quantity_ceilings",
            "exposure_ceilings",
            "policy_ref",
            "product_ref",
            "fact_schema_refs",
            "compiled_plan_ref",
            "evaluator",
            "accepted_by",
            "accepted_at",
        ],
        &["ends_at"],
    )?;
    for k in ["customer", "product_ref", "policy_ref"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &binding, k)?,
            invoke!(field, m, p, k)?
        )?;
    }
    invoke!(
        equal,
        m,
        invoke!(field, m, &binding, "source_namespace")?,
        &json!(c.canonical_economic_namespace)
    )?;
    invoke!(
        contains,
        m,
        invoke!(array, m, &binding, "fact_schema_refs")?,
        invoke!(field, m, p, "fact_schema_ref")?
    )?;
    let (_, accepted_at) = times.get(m, owner)?;
    let at = BigInt::from(accepted_at.micros());
    let bs = invoke!(time, m, &binding, "starts_at")?;
    if meter::cmp(m, &at, &bs)?.is_lt() {
        return Err(fail("binding start"));
    }
    if binding.get("ends_at").is_some() {
        let e = invoke!(time, m, &binding, "ends_at")?;
        if !meter::cmp(m, &at, &e)?.is_lt() {
            return Err(fail("binding end"));
        }
    }
    let accepted = r.get(
        m,
        &c.key,
        &a.accepted_agreement_ref,
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
    )?;
    for k in ["customer", "agreement_id", "version"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &accepted, k)?,
            invoke!(field, m, &binding, k)?
        )?;
    }
    invoke!(
        equal,
        m,
        invoke!(field, m, &accepted, "scope")?,
        &json!(c.key.scope)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &accepted, "binding_ref")?,
        &json!(a.binding_ref)
    )?;
    let (terms_ref, terms) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &accepted, "terms_ref")?,
        "workflow-acceptance-terms",
        "ledger-workflow-acceptance-terms/1"
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &terms, "scope")?,
        &json!(c.key.scope)
    )?;
    for k in ["customer", "agreement_id", "version"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &terms, k)?,
            invoke!(field, m, &binding, k)?
        )?;
    }
    let terms_raw = r.canonical(m, &terms_ref)?;
    let terms_digest =
        super::records::hash_parts(m, "workflow-acceptance-terms", &[terms_raw], false)?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &accepted, "terms_digest")?,
        &json!(terms_digest)
    )?;
    let projected = invoke!(field, m, &terms, "binding_inputs")?;
    super::inventory::binding::validate(m, &binding, &terms)?;
    super::inventory::binding::selected_declarations(m, &binding)?;
    super::inventory::publication::validate(m, r, c, owner, program, &binding, &accepted, &terms)?;
    for k in [
        "agreement_id",
        "version",
        "customer",
        "source_namespace",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "roles_refs",
        "source_slots",
        "accepted_context",
        "bindings",
        "funding_legs",
        "starts_at",
        "quantity_ceilings",
        "exposure_ceilings",
    ] {
        invoke!(
            equal,
            m,
            invoke!(field, m, projected, k)?,
            invoke!(field, m, &binding, k)?
        )?;
    }
    let activation = r.get(
        m,
        &c.key,
        &a.activation_ref,
        "workflow-activation",
        "ledger-workflow-activation-transition/1",
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &activation, "binding_ref")?,
        &json!(a.binding_ref)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &activation, "action")?,
        &json!("activate")
    )?;
    let effective = invoke!(time, m, &activation, "effective_at")?;
    if meter::cmp(m, &at, &effective)?.is_lt() {
        return Err(fail("activation not effective"));
    }
    let (_, head) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &activation, "head_transition_ref")?,
        "workflow-head-transition",
        "ledger-workflow-head-transition/1"
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &head, "scope")?,
        &json!(c.key.scope)
    )?;
    for k in ["customer", "agreement_id"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &head, k)?,
            invoke!(field, m, &binding, k)?
        )?;
    }
    // Acceptance and activation are explicit data, not permission to substitute terms.
    if !meter::cmp(m, &a.original_grant_refs.len().into(), &0usize.into())?.is_gt() {
        return Err(fail("original nomination"));
    }
    for gr in &a.original_grant_refs {
        invoke!(
            contains,
            m,
            invoke!(array, m, &binding, "authority_refs")?,
            &json!(gr)
        )?;
    }
    let enrollment = r.get(
        m,
        &c.key,
        &a.enrollment_ref,
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &enrollment, "scope")?,
        &json!(c.key.scope)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &enrollment, "installation_mode")?,
        &json!(c.installation_mode)
    )?;
    let ceiling = invoke!(field, m, &enrollment, "ceiling")?;
    let state = r.get(
        m,
        &c.key,
        &a.current_state_ref,
        "workflow-source-state",
        "ledger-workflow-source-state/1",
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &state, "enrollment_ref")?,
        &json!(a.enrollment_ref)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &state, "status")?,
        &json!("active")
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &state, "current_grant_ref")?,
        &json!(a.current_grant_ref)
    )?;
    invoke!(
        equal,
        m,
        invoke!(
            field,
            m,
            invoke!(field, m, &state, "operation_key")?,
            "logical_store_id"
        )?,
        &json!(c.key.logical_store_id)
    )?;
    // Initial lineage is implemented here; successors must supply the whole lineage
    // through the pending explicit resolver, never be accepted by hash equality.
    invoke!(
        equal,
        m,
        invoke!(field, m, &state, "revision")?,
        &json!("1")
    )?;
    let enrollment_id = invoke!(text, m, &enrollment, "enrollment_id")?;
    let state_id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, "1"]),
    )?;
    invoke!(equal, m, &json!(state_id), &json!(a.current_state_ref.id))?;
    let current = r.get(
        m,
        &c.key,
        &a.current_grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let registered = super::inventory::source::initial(
        m,
        r,
        c,
        owner,
        &enrollment,
        &state,
        &current,
        &p["customer"],
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &current, "original_ceiling_ref")?,
        &json!(a.enrollment_ref)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &current, "revision")?,
        &json!("1")
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &current, "grant_id")?,
        invoke!(field, m, &enrollment, "enrollment_id")?
    )?;
    invoke!(
        grant,
        m,
        owner,
        &current,
        ceiling,
        c,
        invoke!(field, m, p, "customer")?,
        invoke!(text, m, p, "activity")?,
        invoke!(text, m, product, "product_id")?,
        source_slot,
        &at,
        mode
    )?;
    if !same(
        m,
        &json!(a.original_grant_refs),
        &json!([a.current_grant_ref.clone()]),
    )? {
        return Err(fail("initial original grant nominations"));
    }
    if !same(m, &binding["authority_refs"], &json!(a.original_grant_refs))? {
        return Err(fail("initial binding grant nominations"));
    }
    for gr in &a.original_grant_refs {
        let g = r.get(
            m,
            &c.key,
            gr,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        invoke!(
            equal,
            m,
            invoke!(field, m, &g, "original_ceiling_ref")?,
            &json!(a.enrollment_ref)
        )?;
        equal(m, &g, &current)?;
        invoke!(
            grant,
            m,
            owner,
            &g,
            ceiling,
            c,
            invoke!(field, m, p, "customer")?,
            invoke!(text, m, p, "activity")?,
            invoke!(text, m, product, "product_id")?,
            source_slot,
            &at,
            mode
        )?;
    }
    m.phase(Phase::CurrentAuthority);
    let nomination = invoke!(
        named,
        m,
        invoke!(array, m, &binding, "source_slots")?,
        "name",
        source_slot
    )?;
    invoke!(
        contains,
        m,
        invoke!(array, m, nomination, "sources")?,
        &json!(c.key.source)
    )?;
    let (_, catalog) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &binding, "slot_catalog_ref")?,
        "workflow-slot-catalog",
        "ledger-workflow-slot-catalog/1"
    )?;
    let (_, instantiation) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &binding, "slot_instantiation_ref")?,
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1"
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &instantiation, "catalog_ref")?,
        invoke!(field, m, &binding, "slot_catalog_ref")?
    )?;
    for v in [&catalog, &instantiation] {
        for k in ["agreement_id", "customer"] {
            invoke!(
                equal,
                m,
                invoke!(field, m, v, k)?,
                invoke!(field, m, &binding, k)?
            )?;
        }
    }
    let bindings = invoke!(array, m, &binding, "bindings")?;
    if !meter::cmp(m, &bindings.len().into(), &1usize.into())?.is_eq() {
        return Err(fail("retail binding count"));
    }
    let binding_slot = invoke!(text, m, &bindings[0], "slot")?;
    let (slot_terms_ref, slot_terms) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &bindings[0], "binding_ref")?,
        "workflow-slot-terms",
        "ledger-workflow-slot-binding-terms/2"
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &slot_terms, "scope")?,
        &json!(c.key.scope)
    )?;
    for k in ["agreement_id", "customer"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &slot_terms, k)?,
            invoke!(field, m, &binding, k)?
        )?;
    }
    invoke!(
        equal,
        m,
        invoke!(field, m, &slot_terms, "slot")?,
        &json!(binding_slot)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &slot_terms, "book")?,
        &json!("retail")
    )?;
    invoke!(
        contains,
        m,
        invoke!(array, m, &slot_terms, "allowed_activities")?,
        invoke!(field, m, p, "activity")?
    )?;
    let (role_ref, role) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &slot_terms, "role_ref")?,
        "workflow-roles",
        "ledger-workflow-role-assignment/1"
    )?;
    equal(
        m,
        &slot_terms["source_grant_refs"],
        &json!(a.original_grant_refs),
    )?;
    super::inventory::slots::validate(
        m,
        program,
        &binding,
        &catalog,
        &instantiation,
        &slot_terms,
        &role,
        &current,
    )?;
    invoke!(
        contains,
        m,
        invoke!(array, m, &binding, "roles_refs")?,
        &json!(role_ref)
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &role, "scope")?,
        &json!(c.key.scope)
    )?;
    for k in ["agreement_id", "customer", "role_slot"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, &role, k)?,
            invoke!(field, m, &slot_terms, k)?
        )?;
    }
    for k in [
        "provider",
        "cost_originator",
        "bearer",
        "payer",
        "beneficiary",
        "recipient",
    ] {
        invoke!(text, m, &role, k)?;
    }
    let funding = invoke!(
        named,
        m,
        invoke!(array, m, &binding, "funding_legs")?,
        "name",
        invoke!(text, m, &slot_terms, "funding_leg")?
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, funding, "mode")?,
        &json!("customer_local")
    )?;
    for k in ["payer", "resource_owner"] {
        invoke!(
            equal,
            m,
            invoke!(field, m, funding, k)?,
            invoke!(field, m, p, "customer")?
        )?;
    }
    for ev in invoke!(array, m, funding, "evidence_refs")? {
        let (_, e) = invoke!(
            load,
            m,
            &mut *r,
            c,
            owner,
            ev,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1"
        )?;
        let subject =
            json!({"customer":binding["customer"],"agreement_id":binding["agreement_id"]});
        let metadata = json!({"mode":"customer_local","payer":funding["payer"],"resource_owner":funding["resource_owner"]});
        super::inventory::evidence::retained(
            m,
            r,
            c,
            owner,
            &e,
            "funding",
            &subject,
            &metadata,
            &binding,
            "accepted_at",
        )?;
        invoke!(
            equal,
            m,
            invoke!(field, m, &e, "purpose")?,
            &json!("funding")
        )?;
        invoke!(
            equal,
            m,
            invoke!(field, m, invoke!(field, m, &e, "subject")?, "customer")?,
            invoke!(field, m, p, "customer")?
        )?;
    }
    for assent_ref in invoke!(array, m, &binding, "assent_refs")? {
        let (_, assent) = invoke!(
            load,
            m,
            &mut *r,
            c,
            owner,
            assent_ref,
            "workflow-assent",
            "ledger-workflow-assent/1"
        )?;
        invoke!(
            equal,
            m,
            invoke!(field, m, &assent, "scope")?,
            &json!(c.key.scope)
        )?;
        for k in ["agreement_id", "customer"] {
            invoke!(
                equal,
                m,
                invoke!(field, m, &assent, k)?,
                invoke!(field, m, &binding, k)?
            )?;
        }
        invoke!(
            equal,
            m,
            invoke!(field, m, &assent, "terms_digest")?,
            &json!(terms_digest)
        )?;
        invoke!(
            equal,
            m,
            invoke!(field, m, &assent, "operator")?,
            &binding["accepted_by"]
        )?;
        let expected_mode = if same_text(m, &c.installation_mode, "isolated_synthetic")? {
            "synthetic"
        } else {
            "real"
        };
        invoke!(
            equal,
            m,
            invoke!(field, m, &assent, "mode")?,
            &json!(expected_mode)
        )?;
        let consent_at = invoke!(time, m, &assent, "accepted_at")?;
        if meter::cmp(m, &consent_at, &at)?.is_gt() {
            return Err(fail("future assent"));
        }
        for (k, purpose) in [
            ("evidence_ref", "assent"),
            ("operator_attestation_ref", "operator_attestation"),
        ] {
            let (_, ev) = invoke!(
                load,
                m,
                &mut *r,
                c,
                owner,
                invoke!(field, m, &assent, k)?,
                "workflow-evidence",
                "ledger-workflow-retained-evidence/1"
            )?;
            super::inventory::evidence::assent(
                m,
                r,
                c,
                owner,
                &assent,
                &ev,
                purpose,
                &registered,
                &binding,
            )?;
            invoke!(
                equal,
                m,
                invoke!(field, m, &ev, "purpose")?,
                &json!(purpose)
            )?;
            invoke!(
                equal,
                m,
                invoke!(field, m, invoke!(field, m, &ev, "subject")?, "customer")?,
                invoke!(field, m, p, "customer")?
            )?;
            invoke!(
                equal,
                m,
                invoke!(
                    field,
                    m,
                    invoke!(field, m, &ev, "metadata")?,
                    "terms_digest"
                )?,
                &json!(terms_digest)
            )?;
        }
    }
    let observation = r.get(
        m,
        &c.key,
        &a.authority_observation_ref,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    for (k, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("installation_mode", json!(c.installation_mode)),
        ("enrollment_ref", json!(a.enrollment_ref)),
        ("current_state_ref", json!(a.current_state_ref)),
        ("current_grant_ref", json!(a.current_grant_ref)),
        ("original_grant_refs", json!(a.original_grant_refs)),
        ("observed_at", json!(c.accepted_at)),
    ] {
        invoke!(equal, m, invoke!(field, m, &observation, k)?, &want)?;
    }
    let (verifier_ref, verifier) = invoke!(
        load,
        m,
        &mut *r,
        c,
        owner,
        invoke!(field, m, &observation, "verifier_ref")?,
        "workflow-authority-audit-verifier",
        "ledger-authority-audit-verifier/2"
    )?;
    let root_value = invoke!(field, m, &observation, "root_ref")?;
    let root = super::inventory::evidence::root(
        m,
        r,
        c,
        owner,
        root_value,
        &json!({"at":c.accepted_at}),
        "at",
        &json!(c.actor),
        Some(&json!(c.key.source)),
    )?;
    super::inventory::observation::validate(m, c, &state, &observation, &verifier, &root)?;
    super::inventory::transitions::validate(
        m,
        r,
        c,
        owner,
        program,
        &binding,
        &accepted,
        &activation,
        &head,
    )?;
    let basis = invoke!(field, m, &root, "basis")?;
    for (key, want) in [
        ("scope", json!(c.key.scope)),
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("principal_id", json!(c.actor)),
        ("source", json!(c.key.source)),
        (
            "canonical_economic_namespace",
            json!(c.canonical_economic_namespace),
        ),
    ] {
        invoke!(equal, m, invoke!(field, m, basis, key)?, &want)?;
    }
    // Every pending representation must be one of the two actually consumed
    // observation/verifier operands. Extra same-kind rows are not silently used.
    for input in &c.source_observation_inputs {
        m.visit("authority.pending_required")?;
        if !shape::same_ref(m, &input.reference, &verifier_ref)?
            && !shape::same_ref(m, &input.reference, &a.authority_observation_ref)?
        {
            return Err(crate::Error::new(
                "UNSUPPORTED_COMPOSITION",
                "extra pending authority representation",
            ));
        }
    }
    let mut introduced = vec![];
    for reference in [verifier_ref, a.authority_observation_ref.clone()] {
        let mut pending = false;
        for input in &c.source_observation_inputs {
            m.visit("authority.pending_representation")?;
            if shape::same_ref(m, &input.reference, &reference)? {
                pending = true;
                break;
            }
        }
        if pending {
            let raw = r.canonical(m, &reference)?;
            m.new_body(raw.len())?;
            m.scan_bytes(raw.len())?;
            m.write_bytes(raw.len())?;
            introduced.push(DraftEnvelope {
                reference,
                canonical_body: raw.to_vec(),
            });
        }
    }
    let authority = body::document(
        m,
        "workflow-economic-authority",
        json!({"schema":"ledger-workflow-economic-authority/2","operation":mode.operation(),"actor":c.actor,"source":c.key.source,"scope":c.key.scope,"customer":p["customer"],"canonical_economic_namespace":c.canonical_economic_namespace,"installation_mode":c.installation_mode,"enrollment_ref":a.enrollment_ref,"current_state_ref":a.current_state_ref,"current_grant_ref":a.current_grant_ref,"original_binding_ref":a.binding_ref,"original_accepted_agreement_ref":a.accepted_agreement_ref,"original_grant_refs":a.original_grant_refs,"source_slot":a.source_slot,"rights":mode.rights(),"evidence_refs":p["evidence_refs"],"verified_fact_refs":verified_facts,"observed_at":c.accepted_at,"authority_observation_ref":a.authority_observation_ref}),
    )?;
    let _ = terms_ref;
    Ok(Operands {
        binding,
        accepted,
        activation,
        catalog,
        instantiation,
        slot_terms,
        slot_terms_ref,
        role_ref,
        authority,
        introduced,
    })
}
