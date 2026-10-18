//! Relational checks on explicit source/assent data. These are not host proofs.
use super::emitted_record::ConstructedEnvelope;
use super::record_body::RecordBody;
use super::{
    CoreMeter, Phase, arithmetic, body,
    data::{DraftContextData, ExactRef},
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
#[derive(Clone, Copy,PartialEq,Eq)]
pub(super) enum EconomicMode {
    Work,
    OutcomeSubmit,
    OutcomeCorrect,
    QuantityCorrect,
    WorkReverse,
    EvidenceRetain,
    OutcomeClose,
    DeadlineClose,
    CostFinalize,
    CostCorrect,
    InvocationAuthorize,
    InvocationClose,
}
impl EconomicMode {
    pub(super) fn operation(self) -> &'static str {
        match self {
            Self::Work => "work.accept",
            Self::OutcomeSubmit => "outcome.submit",
            Self::OutcomeCorrect => "outcome.correct",
            Self::QuantityCorrect => "quantity.correct",
            Self::WorkReverse => "work.reverse",
            Self::EvidenceRetain => "evidence.retain",
            Self::OutcomeClose => "outcome.close",
            Self::DeadlineClose => "outcome.close",
            Self::CostFinalize=>"cost.finalize",
            Self::CostCorrect=>"cost.correct",
            Self::InvocationAuthorize=>"invocation.authorize",
            Self::InvocationClose=>"invocation.close",
        }
    }
    pub(super) fn rights(self) -> &'static [&'static str] {
        match self {
            Self::Work | Self::OutcomeSubmit | Self::EvidenceRetain | Self::OutcomeClose | Self::CostFinalize => &["read", "submit"],
            Self::DeadlineClose => &["close", "read"],
            Self::OutcomeCorrect | Self::QuantityCorrect | Self::WorkReverse | Self::CostCorrect => {
                &["correct", "read"]
            }
            Self::InvocationAuthorize=>&["invocation","read"],
            Self::InvocationClose=>&["close","read"],
        }
    }
}
pub(super) struct Terms {
    pub binding: RecordBody,
    pub accepted: RecordBody,
    pub activation: RecordBody,
    pub catalog: RecordBody,
    pub instantiation: RecordBody,
    pub slot_terms: RecordBody,
    pub slot_terms_ref: ExactRef,
    pub role_ref: ExactRef,
}
pub(super) struct Operands {
    pub terms: Terms,
    pub authority: ConstructedEnvelope,
    pub introduced: Vec<ConstructedEnvelope>,
}
impl std::ops::Deref for Operands {
    type Target = Terms;
    fn deref(&self) -> &Terms {
        &self.terms
    }
}
pub(super) struct ResolvedOperands {
    pub terms: Terms,
    pub introduced: Vec<ConstructedEnvelope>,
    authority_body: super::working_value::WorkingValue,
    _metadata:Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
/// Historical identity association only. The host still completes this exact
/// authority's original recipe; a present record or matching namespace is not
/// an origin proof. Today's assessor/corrector namespace is deliberately absent
/// from this relation.
pub(super) fn historical_namespace(
    m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,authority_ref:&Value,
    namespace:&Value,source:Option<&Value>,operation:&str,binding:&Value,at:&Value,
) -> Result<()> {
    let reference=super::shape::reference(m,authority_ref)?;
    let original=r.get_schemas(m,&c.key,&reference,"workflow-economic-authority",&["ledger-workflow-economic-authority/2","ledger-workflow-economic-authority/3","ledger-workflow-economic-authority/5","ledger-workflow-economic-authority/6","ledger-workflow-economic-authority/7"])?;
    equal(m,&original["scope"],&json!(c.key.scope))?;
    equal(m,&original["operation"],&json!(operation))?;
    equal(m,&original["canonical_economic_namespace"],namespace)?;
    equal(m,&original["original_binding_ref"],binding)?;
    equal(m,&original["observed_at"],at)?;
    if let Some(source)=source{equal(m,&original["source"],source)?;}
    let enrollment_ref=super::shape::reference(m,&original["enrollment_ref"])?;
    let enrollment=r.get(m,&c.key,&enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    let ceiling=field(m,&enrollment,"ceiling")?;
    for (name,expected) in [("source",&original["source"]),("principal_id",&original["actor"]),("canonical_economic_namespace",namespace)] {
        equal(m,&ceiling[name],expected)?;
    }
    let grant_ref=super::shape::reference(m,&original["current_grant_ref"])?;
    let grant=r.get(m,&c.key,&grant_ref,"workflow-source-grant","ledger-workflow-source-grant/1")?;
    equal(m,&grant["original_ceiling_ref"],&json!(enrollment_ref))?;
    equal(m,&grant["grant_id"],&enrollment["enrollment_id"])?;
    for name in ["source","principal_id","canonical_economic_namespace"]{equal(m,&grant[name],&ceiling[name])?;}
    let at_raw=at.as_str().ok_or_else(||fail("historical source accepted time"))?;
    let at=Timestamp::parse_with_work(at_raw,m)?.micros();
    let starts=Timestamp::parse_with_work(text(m,&grant,"starts_at")?,m)?.micros();
    let ends=Timestamp::parse_with_work(text(m,&grant,"ends_at")?,m)?.micros();
    if meter::cmp(m,&at.into(),&starts.into())?.is_lt() || !meter::cmp(m,&at.into(),&ends.into())?.is_lt(){return Err(fail("historical source grant at original acceptance"));}
    Ok(())
}
impl std::ops::Deref for ResolvedOperands {
    type Target = Terms;
    fn deref(&self) -> &Terms {
        &self.terms
    }
}
impl ResolvedOperands {
    pub(super) fn owning(terms:Terms,introduced:Vec<ConstructedEnvelope>,value:crate::canonical::audit_arena::AuditedValue,metadata:crate::canonical::audit_arena::ExternalBufferLease)->Self {
        Self{terms,introduced,authority_body:super::working_value::WorkingValue::Owned(value),_metadata:Some(metadata)}
    }
    pub(super) fn app_authority_value(&self,m:&mut CoreMeter,checks:&[Value])->Result<crate::canonical::audit_arena::AuditedValue>{
        let arena=m.representation_arena().ok_or_else(||fail("owning app parent authority"))?;
        let super::working_value::WorkingValue::Owned(base)=&self.authority_body else{return Err(fail("app parent requires owning resolved authority"))};
        crate::validation_work::ValidationWork::arithmetic(m,crate::validation_work::ArithmeticOperation::Multiply,&[&checks.len().into(),&std::mem::size_of::<&Value>().into()])?;
        let bytes=checks.len().checked_mul(std::mem::size_of::<&Value>()).ok_or_else(||fail("app parent family check product"))?;
        let _lease=arena.reserve_external_payload(bytes,m)?;
        let mut fields=Vec::new();fields.try_reserve_exact(checks.len()).map_err(|_|fail("app parent family checks"))?;
        for check in checks{m.visit("app.parent_family_check")?;fields.push(check);}
        let checks=arena.array_from_values(&fields,m)?;
        let value=super::working_value::WorkingValue::copy(m,base.value())?.with_fields(m,&[("causal_closure_checks",checks.value())])?;
        let mut value=value.into_owned()?;
        super::intent::sort_owned_set(&mut value,"causal_closure_checks",false,m,&arena)?;
        Ok(value)
    }
    pub(super) fn authority_document(
        &self,
        m: &mut CoreMeter,
        causal_checks: Option<&[Value]>,
    ) -> Result<ConstructedEnvelope> {
        if let super::working_value::WorkingValue::Owned(value)=&self.authority_body {
            if let Some(checks)=causal_checks{let value=self.app_authority_value(m,checks)?;return body::document_borrowed(m,"workflow-economic-authority",value.value());}
            return body::document_borrowed(m,"workflow-economic-authority",value.value());
        }
        let super::working_value::WorkingValue::Legacy(original)=&self.authority_body else{unreachable!()};
        let mut value = original.clone();
        if let Some(checks) = causal_checks {
            let operation = shape::text(m, &value, "operation")?;
            if !shape::same_text(m, operation, "quantity.correct")?
                && !shape::same_text(m, operation, "work.reverse")?
            {
                return Err(fail("causal authority operation tuple"));
            }
            value["schema"] = json!("ledger-workflow-economic-authority/3");
            value["causal_closure_checks"] = json!(body::object_set(m, checks.to_vec())?);
        }
        body::document(m, "workflow-economic-authority", value)
    }
}

fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    owner: &InvocationToken,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, RecordBody)> {
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
pub(super) fn grant(
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
    grant_shape(m,g)?;
    grant_identity(m,g,ceiling,c)?;
    grant_intersection(m,owner,g,ceiling,at)?;
    grant_coverage(m,g,customer,activity,product,slot,mode.operation(),mode.rights())
}
pub(super) fn grant_identity(m:&mut CoreMeter,g:&Value,ceiling:&Value,c:&DraftContextData)->Result<()> {
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
    Ok(())
}
pub(super) fn grant_shape(m:&mut CoreMeter,g:&Value)->Result<()> {
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
    Ok(())
}
pub(super) fn grant_intersection(m:&mut CoreMeter,owner:&InvocationToken,g:&Value,ceiling:&Value,at:&BigInt)->Result<()> {
    super::inventory::source::current_sets(m, g)?;
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
        let current_selectors = invoke!(array, m, g, k)?;
        let original_selectors = invoke!(array, m, ceiling, k)?;
        super::inventory::source::selector_subset(
            m,
            current_selectors,
            original_selectors,
            if k == "link_selectors" { 32 } else { 64 },
        )?;
    }
    let prepared = ParsedGrantInterval::parse(m, owner, g)?;
    let (gs, ge) = prepared.get(m, owner)?;
    let gs = BigInt::from(gs.micros());
    let ge = BigInt::from(ge.micros());
    let cs = super::inventory::check::time_owned(m,owner,ceiling,"starts_at")?;
    let ce = super::inventory::check::time_owned(m,owner,ceiling,"ends_at")?;
    if meter::cmp(m, &gs, &cs)?.is_lt() || meter::cmp(m, &ge, &ce)?.is_gt() {
        return Err(fail("original ceiling time"));
    }
    invoke!(interval, m, owner, &prepared, at)?;
    Ok(())
}
pub(super) fn grant_coverage(m:&mut CoreMeter,g:&Value,customer:&Value,activity:&str,product:&str,slot:&str,operation:&str,rights:&[&str])->Result<()> {
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
        &json!(operation)
    )?;
    for right in rights {
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
fn selector_allows(m:&mut CoreMeter,g:&Value,field:&str,name:&str)->Result<bool>{
    let selectors=shape::array(m,g,field)?;
    let mut covered=false;
    for selector in selectors {
        m.visit("close.deadline_selector")?;
        shape::closed(m,selector,&["names"],&[])?;
        let names=shape::array(m,selector,"names")?;
        covered|=shape::member(m,names,&json!(name))?;
    }
    Ok(covered)
}
/// SAME closed deadline permission/selector/time predicate. Identity is checked
/// by the containing caller against its actual source, never an invented actor.
pub(super) fn deadline_grant_scope(m:&mut CoreMeter,g:&Value,slot:&Value,customer:&Value,activity:&str,product:&str,at:i64)->Result<bool>{
    let mut eligible=true;
    for (field,want) in [("operations",json!("outcome.close")),("permissions",json!("close")),("permissions",json!("read"))]{
        let values=shape::array(m,g,field)?;
        eligible&=shape::member(m,values,&want)?;
    }
    let slots=shape::array(m,g,"source_slots")?;eligible&=shape::member(m,slots,slot)?;
    let customers=shape::array(m,g,"customer_scope")?;eligible&=shape::member(m,customers,customer)?;
    eligible&=selector_allows(m,g,"activity_selectors",activity)?;
    eligible&=selector_allows(m,g,"product_selectors",product)?;
    let starts=time(m,g,"starts_at")?;let ends=time(m,g,"ends_at")?;
    eligible&=!meter::cmp(m,&at.into(),&starts.into())?.is_lt();
    eligible&=meter::cmp(m,&at.into(),&ends.into())?.is_lt();
    Ok(eligible)
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
    if program.uses_app_owned_cost_recipe(){
        let slot=shape::text(m,activity,"source_slot")?;
        return super::app_cost::work_operands(m,r,c,p,program,slot);
    }
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
    let resolved = resolve_economic(
        m,
        r,
        c,
        p,
        activity,
        product,
        owner,
        times,
        program,
        mode,
        verified_facts,
    )?;
    let authority = resolved.authority_document(m, None)?;
    Ok(Operands {
        terms: resolved.terms,
        authority,
        introduced: resolved.introduced,
    })
}
pub(super) fn economic_original(
    m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,p:&Value,activity:&Value,product:&Value,
    owner:&InvocationToken,times:&ParsedDraftTimes<'_>,program:&super::super::selected_program::SelectedProgram,
    mode:EconomicMode,verified_facts:&[ExactRef],original_namespace:&str,
)->Result<Operands>{
    let resolved=resolve_economic_original(m,r,c,p,activity,product,owner,times,program,mode,verified_facts,original_namespace)?;
    let authority=resolved.authority_document(m,None)?;
    Ok(Operands{terms:resolved.terms,authority,introduced:resolved.introduced})
}
pub(super) fn resolve_economic(
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
) -> Result<ResolvedOperands> {
    resolve_economic_identity(m,r,c,p,activity,product,owner,times,program,mode,verified_facts,&c.canonical_economic_namespace)
}
/// Historical subject identity is distinct from today's assessor/corrector
/// grant namespace. Its operand comes from the original WorkRecord/claim; the
/// current source/grant namespace and all rights remain independently checked.
pub(super) fn resolve_economic_original(
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
    original_namespace:&str,
) -> Result<ResolvedOperands> {
    resolve_economic_identity(m,r,c,p,activity,product,owner,times,program,mode,verified_facts,original_namespace)
}
fn resolve_economic_identity(
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
    original_namespace:&str,
) -> Result<ResolvedOperands> {
    m.phase(Phase::OriginalAuthority);
    if program.uses_app_owned_cost_recipe(){
        let source_slot=shape::text(m,activity,"source_slot")?;
        return super::app_cost::binding_resolved(m,r,c,p,program,mode,source_slot,verified_facts,original_namespace);
    }
    if !meter::cmp(m, &c.authorities.len().into(), &1usize.into())?.is_eq() {
        return Err(fail("one required nominated work slot"));
    }
    let a = &c.authorities[0];
    let source_slot = invoke!(text, m, activity, "source_slot")?;
    if mode!=EconomicMode::DeadlineClose{invoke!(equal, m, &json!(a.source_slot), &json!(source_slot))?;}
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
        &json!(original_namespace)
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
    // Current state/grant origin and monotonic lineage are audited by the host
    // before these DATA operands can enter a prepared financial transition.
    // Recheck their exact identity/current use here; never replace original
    // accepted grant nominations with today's grant Ref.
    let enrollment_id = invoke!(text, m, &enrollment, "enrollment_id")?;
    let revision_text = invoke!(text, m, &state, "revision")?;
    let revision_u64 = super::super::facts::counter_with_work(revision_text, m)?;
    let revision = num_bigint::BigInt::from(revision_u64);
    if !super::shape::same_text(m, revision_text, &revision_u64.to_string())? {
        return Err(fail("noncanonical current source revision"));
    }
    if !meter::cmp(m, &revision, &0.into())?.is_gt() {
        return Err(fail("positive current source revision"));
    }
    closed(
        m,
        &state,
        &[
            "schema",
            "enrollment_ref",
            "revision",
            "status",
            "current_grant_ref",
            "operation_key",
            "actor",
            "accepted_at",
        ],
        &["prior_state_ref"],
    )?;
    super::inventory::source::current_header(m, c, &state)?;
    let state_at = invoke!(time, m, &state, "accepted_at")?;
    if meter::cmp(m, &state_at, &at)?.is_gt() {
        return Err(fail("current source cannot originate in the future"));
    }
    let state_id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            enrollment_id,
            revision_text
        ]),
    )?;
    invoke!(equal, m, &json!(state_id), &json!(a.current_state_ref.id))?;
    let birth_id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, "1"]),
    )?;
    let (birth_ref, birth_state) = r.by_identity(
        m,
        &c.key,
        "workflow-source-state",
        &birth_id,
        "ledger-workflow-source-state/1",
    )?;
    if meter::cmp(m, &revision, &1.into())?.is_eq() {
        if state.get("prior_state_ref").is_some() || !same(m, &birth_state, &state)? {
            return Err(fail("initial current source must be exact birth"));
        }
    } else {
        let prior_value = invoke!(field, m, &state, "prior_state_ref")?;
        let prior = super::shape::reference(m, prior_value)?;
        let prior_revision = meter::sub(m, &revision, &1.into())?;
        let prior_id = body::semantic_id(
            m,
            "wsource_",
            "workflow-source-state",
            &json!([
                c.key.logical_store_id,
                c.key.scope,
                enrollment_id,
                prior_revision.to_string()
            ]),
        )?;
        if !super::shape::same_text(m, &prior.kind, "workflow-source-state")?
            || !super::shape::same_text(m, &prior.id, &prior_id)?
        {
            return Err(fail("current source predecessor identity"));
        }
    }
    if !meter::cmp(m, &a.original_grant_refs.len().into(), &1.into())?.is_eq() {
        return Err(fail("selected original immutable grant nomination"));
    }
    if !same(m, &binding["authority_refs"], &json!(a.original_grant_refs))? {
        return Err(fail("binding original grant nominations"));
    }
    let original_ref = &a.original_grant_refs[0];
    let original_grant = r.get(
        m,
        &c.key,
        original_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let registered = super::inventory::source::birth(
        m,
        r,
        c,
        owner,
        &enrollment,
        &birth_state,
        &original_grant,
        &p["customer"],
        &a.enrollment_ref,
        &birth_ref,
        original_ref,
    )?;
    let current = r.get(
        m,
        &c.key,
        &a.current_grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
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
        &state["revision"]
    )?;
    invoke!(
        equal,
        m,
        invoke!(field, m, &current, "grant_id")?,
        &enrollment["enrollment_id"]
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
    // Original accepted rights remain an independent intersection. A narrowed
    // current grant cannot silently substitute a different original capability.
    invoke!(
        grant,
        m,
        owner,
        &original_grant,
        ceiling,
        c,
        invoke!(field, m, p, "customer")?,
        invoke!(text, m, p, "activity")?,
        invoke!(text, m, product, "product_id")?,
        source_slot,
        &at,
        mode
    )?;
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
    let slot_terms_ref = ParsedRef::parse(m, owner, &bindings[0]["binding_ref"])?;
    let slot_terms = r.get_parsed_schemas(
        m,
        owner,
        &c.key,
        &slot_terms_ref,
        "workflow-slot-terms",
        &[
            "ledger-workflow-slot-binding-terms/2",
            "ledger-workflow-slot-binding-terms/3",
        ],
    )?;
    let slot_terms_ref = slot_terms_ref.exact().clone();
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
            if let Some(arena) = m.representation_arena() {
                let class = crate::validation_work::RepresentationKind::SourceClosure;
                let bytes = m.copy_owned_representation(&arena, raw, class)?;
                introduced.push(ConstructedEnvelope::from_owned(
                    reference, bytes, &arena, m, class,
                )?);
            } else {
                introduced.push(ConstructedEnvelope::from_bytes(reference, raw.to_vec()));
            }
        }
    }
    let authority_body = json!({"schema":"ledger-workflow-economic-authority/2","operation":mode.operation(),"actor":c.actor,"source":c.key.source,"scope":c.key.scope,"customer":p["customer"],"canonical_economic_namespace":c.canonical_economic_namespace,"installation_mode":c.installation_mode,"enrollment_ref":a.enrollment_ref,"current_state_ref":a.current_state_ref,"current_grant_ref":a.current_grant_ref,"original_binding_ref":a.binding_ref,"original_accepted_agreement_ref":a.accepted_agreement_ref,"original_grant_refs":a.original_grant_refs,"source_slot":source_slot,"rights":mode.rights(),"evidence_refs":p["evidence_refs"],"verified_fact_refs":verified_facts,"observed_at":c.accepted_at,"authority_observation_ref":a.authority_observation_ref});
    let _ = terms_ref;
    Ok(ResolvedOperands {
        terms: Terms {
            binding,
            accepted,
            activation,
            catalog,
            instantiation,
            slot_terms,
            slot_terms_ref,
            role_ref,
        },
        authority_body:super::working_value::WorkingValue::Legacy(authority_body),
        _metadata:None,
        introduced,
    })
}
