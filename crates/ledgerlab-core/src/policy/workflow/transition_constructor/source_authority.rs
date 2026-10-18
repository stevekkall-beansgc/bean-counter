//! Actual original/current nominated source intersection shared by app and AP
//! consumers. The operands are original Binding/slot terms/work DATA, never a
//! synthetic invocation or caller authentication flag. Completed origin and
//! current snapshot custody remain the held host's independent obligations.
use super::{CoreMeter,authority::{self,EconomicMode},body,data::*,inventory,meter,record_body::RecordBody,records::Records,representation::InvocationToken,shape};
use super::super::selected_program::SelectedProgram;
use crate::{Error,Result,canonical::audit_arena::ExternalBufferLease,validation_work as w};
use serde_json::{Value,json};
pub(super) struct OriginalSourceSubject<'a>{
    pub binding_ref:&'a ExactRef,
    pub binding:&'a Value,
    pub terms:&'a Value,
    pub payload:&'a Value,
}
pub(super) struct SourceSelection {
    pub enrollment: RecordBody,
    pub registered:RecordBody,
    pub source_slot:String,
    pub original_grant_refs:Vec<ExactRef>,
    pub current: RecordBody,
    pub state: RecordBody,
    pub observation: Option<RecordBody>,
    pub verifier_ref: Option<ExactRef>,
    pub root: Option<RecordBody>,
    _ownership: ExternalBufferLease,
}
impl SourceSelection {
    pub(super) fn required_verifier_ref(&self)->Result<&ExactRef>{self.verifier_ref.as_ref().ok_or_else(||shape::fail("experimental verifier is required"))}
    pub(super) fn required_observation(&self)->Result<&RecordBody>{self.observation.as_ref().ok_or_else(||shape::fail("experimental observation body is required"))}
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, RecordBody)> {
    let reference = shape::reference(m, v)?;
    let body = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, body))
}
/// Exact current source grants are independent of historical worker namespace.
/// The original accepted supplier grants are selected by actual source/principal
/// association and compared in full with the supplied observation nominations.
pub(super) fn source(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    source_slot: &str,
    mode: EconomicMode,
) -> Result<SourceSelection> {
    source_data(m,r,c,program,selected,source_slot,mode,!program.uses_local_execution_recipe())
}
pub(super) fn source_local(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    source_slot: &str,
    mode: EconomicMode,
) -> Result<SourceSelection> {
    source_data(m,r,c,program,selected,source_slot,mode,false)
}
pub(super) fn source_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    selected: OriginalSourceSubject<'_>,
    source_slot: &str,
    mode: EconomicMode,
    audit_observation:bool,
) -> Result<SourceSelection> {
    if audit_observation{inventory::entry::context(m,c)?;}else{inventory::entry::local_context(m,c)?;}
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier authority"))?;
    let size = w::usize_add(m, std::mem::size_of::<SourceSelection>(), 16384)?;
    let ownership = arena.reserve_external_payload(size, m)?;
    let a = &c.authorities[0];
    if !shape::same_ref(m, &a.binding_ref, selected.binding_ref)?
        || (!program.uses_app_owned_cost_recipe() && !shape::same_text(m, &a.source_slot, source_slot)?)
    {
        return Err(shape::fail("supplier original authority selection"));
    }
    let enrollment = r.get(
        m,
        &c.key,
        &a.enrollment_ref,
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    eq(m, &enrollment["scope"], &json!(c.key.scope))?;
    eq(
        m,
        &enrollment["installation_mode"],
        &json!(c.installation_mode),
    )?;
    let state = r.get(
        m,
        &c.key,
        &a.current_state_ref,
        "workflow-source-state",
        "ledger-workflow-source-state/1",
    )?;
    shape::closed(
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
    eq(m, &state["enrollment_ref"], &json!(a.enrollment_ref))?;
    eq(m, &state["status"], &json!("active"))?;
    eq(m, &state["current_grant_ref"], &json!(a.current_grant_ref))?;
    inventory::source::current_header(m, c, &state)?;
    let enrollment_id = shape::text(m, &enrollment, "enrollment_id")?;
    let revision = shape::text(m, &state, "revision")?;
    let revision_value = super::super::facts::counter_with_work(revision, m)?;
    if !w::compare(m, revision_value, 0u64)?.is_gt() {
        return Err(shape::fail("positive current supplier source revision"));
    }
    let id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, revision]),
    )?;
    if !shape::same_text(m, &a.current_state_ref.id, &id)? {
        return Err(shape::fail("supplier current source state identity"));
    }
    let birth_id = body::semantic_id(
        m,
        "wsource_",
        "workflow-source-state",
        &json!([c.key.logical_store_id, c.key.scope, enrollment_id, "1"]),
    )?;
    let (birth_ref, birth) = r.by_identity(
        m,
        &c.key,
        "workflow-source-state",
        &birth_id,
        "ledger-workflow-source-state/1",
    )?;
    let (birth_grant_ref, birth_grant) = load(
        m,
        r,
        c,
        &birth["current_grant_ref"],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    let token = InvocationToken::new();
    let registered=inventory::source::birth(
        m,
        r,
        c,
        &token,
        &enrollment,
        &birth,
        &birth_grant,
        &selected.binding["customer"],
        &a.enrollment_ref,
        &birth_ref,
        &birth_grant_ref,
    )?;
    if w::compare(m, revision_value, 1u64)?.is_eq() {
        if state.get("prior_state_ref").is_some() {
            return Err(shape::fail("initial supplier source predecessor"));
        }
        eq(m, &state, &birth)?;
    } else {
        let prior = shape::reference(m, &state["prior_state_ref"])?;
        let previous = meter::sub(m, &revision_value.into(), &1.into())?;
        let expected = body::semantic_id(
            m,
            "wsource_",
            "workflow-source-state",
            &json!([
                c.key.logical_store_id,
                c.key.scope,
                enrollment_id,
                previous.to_string()
            ]),
        )?;
        if !shape::same_text(m, &prior.id, &expected)?
            || !shape::same_text(m, &prior.kind, "workflow-source-state")?
        {
            return Err(shape::fail("supplier current source predecessor"));
        }
    }
    let current = r.get(
        m,
        &c.key,
        &a.current_grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    eq(m, &current["grant_id"], &enrollment["enrollment_id"])?;
    eq(m, &current["revision"], &state["revision"])?;
    eq(
        m,
        &current["original_ceiling_ref"],
        &json!(a.enrollment_ref),
    )?;
    let at = token.timestamp(m,&c.accepted_at,"Timestamp::parse_with_work.accepted_at")?.micros();
    let state_raw=shape::text(m,&state,"accepted_at")?;
    let state_at = token.timestamp(m,state_raw,"Timestamp::parse_with_work.accepted_at")?.micros();
    if meter::cmp(m, &state_at.into(), &at.into())?.is_gt() {
        return Err(shape::fail("supplier source originates in future"));
    }
    let mut product = None;
    for document in program.source_documents() {
        m.visit("cost.authority_original_product")?;
        if shape::same(
            m,
            &json!(document.reference()),
            &selected.payload["product_ref"],
        )? {
            product = Some(document.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("supplier original product"))?;
    let product_id = shape::text(m, product, "product_id")?;
    let activity = shape::text(m, &selected.payload, "activity")?;
    r.source_grant(
        m,
        &token,
        &a.current_grant_ref,
        &current,
        &a.enrollment_ref,
        &enrollment,
        c,
        &selected.binding["customer"],
        activity,
        product_id,
        source_slot,
        &at.into(),
        mode,
    )?;
    let nominations = shape::array(m, &selected.binding, "source_slots")?;
    let nomination = shape::named(m, nominations, "name", source_slot)?;
    let sources = shape::array(m, nomination, "sources")?;
    if !shape::member(m, sources, &json!(c.key.source))? {
        return Err(shape::fail("supplier source absent original nomination"));
    }
    let terms_grants = shape::array(m, &selected.terms, "source_grant_refs")?;
    let binding_grants = shape::array(m, &selected.binding, "authority_refs")?;
    let mut original_refs = Vec::new();
    original_refs
        .try_reserve_exact(terms_grants.len())
        .map_err(|_| shape::fail("supplier original grant vector"))?;
    for value in terms_grants {
        m.visit("cost.original_supplier_grant")?;
        let (reference, grant) = load(
            m,
            r,
            c,
            value,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        if !shape::member(m, binding_grants, value)? {
            return Err(shape::fail("supplier grant absent original Binding"));
        }
        let matches = if m.representation_arena().is_some(){
            super::input_selection::source_grant_matches(m,&grant,&c.key.source,&c.actor,&a.enrollment_ref)?
        }else{
            // The legacy DATA recipe retains its original comparisons/trace.
            shape::same(m, &grant["source"], &json!(c.key.source))?
                && shape::same(m, &grant["principal_id"], &json!(c.actor))?
                && shape::same(m, &grant["original_ceiling_ref"], &json!(a.enrollment_ref))?
        };
        if !matches {
            continue;
        }
        r.source_grant(
            m,
            &token,
            &reference,
            &grant,
            &a.enrollment_ref,
            &enrollment,
            c,
            &selected.binding["customer"],
            activity,
            product_id,
            source_slot,
            &at.into(),
            mode,
        )?;
        original_refs.push(reference);
    }
    if !w::compare(m, original_refs.len(), 0usize)?.is_gt() {
        return Err(Error::new(
            "SOURCE_AUTHORITY_DENIED",
            "missing originally accepted supplier source grant",
        ));
    }
    let originals = body::ref_set(m, original_refs)?;
    eq(m, &json!(originals), &json!(a.original_grant_refs))?;
    if !audit_observation{
        m.scan_bytes(source_slot.len())?;m.write_bytes(source_slot.len())?;
        return Ok(SourceSelection{enrollment,registered,source_slot:source_slot.to_owned(),original_grant_refs:originals,current,state,observation:None,verifier_ref:None,root:None,_ownership:ownership});
    }
    let observation = r.get(
        m,
        &c.key,
        a.required_observation_ref()?,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    for (key, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("installation_mode", json!(c.installation_mode)),
        ("enrollment_ref", json!(a.enrollment_ref)),
        ("current_state_ref", json!(a.current_state_ref)),
        ("current_grant_ref", json!(a.current_grant_ref)),
        ("original_grant_refs", json!(a.original_grant_refs)),
        ("observed_at", json!(c.accepted_at)),
    ] {
        eq(m, &observation[key], &want)?;
    }
    let (verifier_ref, verifier) = load(
        m,
        r,
        c,
        &observation["verifier_ref"],
        "workflow-authority-audit-verifier",
        "ledger-authority-audit-verifier/2",
    )?;
    let root = inventory::evidence::root(
        m,
        r,
        c,
        &token,
        &observation["root_ref"],
        &json!({"at":c.accepted_at}),
        "at",
        &json!(c.actor),
        Some(&json!(c.key.source)),
    )?;
    for (key, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("principal_id", json!(c.actor)),
        ("source", json!(c.key.source)),
        (
            "canonical_economic_namespace",
            json!(c.canonical_economic_namespace),
        ),
    ] {
        eq(m, &root["basis"][key], &want)?;
    }
    let app_catalog=super::audit_metadata::selected_catalog(m,program)?;
    r.source_observation_after_joins(m,c,a.required_observation_ref()?,&state,&observation,&verifier,&root,app_catalog)?;
    m.scan_bytes(source_slot.len())?;m.write_bytes(source_slot.len())?;
    let source_slot=source_slot.to_owned();
    Ok(SourceSelection {
        enrollment,
        registered,
        source_slot,
        original_grant_refs:originals,
        current,
        state,
        observation:Some(observation),
        verifier_ref:Some(verifier_ref),
        root:Some(root),
        _ownership: ownership,
    })
}

