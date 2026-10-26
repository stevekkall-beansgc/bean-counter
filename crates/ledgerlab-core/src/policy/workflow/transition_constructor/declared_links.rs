//! The exact two-hop declared-link recipe. This is not a graph search: the
//! original Product supplies both slots and every endpoint is an exact Ref.
use super::{authority, body, data::*, emitted_record::ConstructedEnvelope, records::Records, shape, CoreMeter};
use super::super::selected_program::SelectedProgram;
use crate::{validation_work as w, Result};
use serde_json::{json, Value};

fn product<'a>(m: &mut CoreMeter, program: &'a SelectedProgram, p: &Value) -> Result<&'a Value> {
    for d in program.source_documents() {
        if shape::same(m, &json!(d.reference()), &p["product_ref"])? { return Ok(d.canonical_value()); }
    }
    Err(shape::fail("declared link original Product"))
}
fn definition<'a>(m: &mut CoreMeter, program: &'a SelectedProgram, p: &Value) -> Result<Option<&'a Value>> {
    let product = product(m, program, p)?;
    let mut result = None;
    for link in shape::array(m, product, "links")? {
        if shape::same(m, &link["child_activity"], &p["activity"])? && result.replace(link).is_some() { return Err(shape::fail("ambiguous declared child slot")); }
    }
    Ok(result)
}
pub(super) fn validate_inputs<'a>(m: &mut CoreMeter, program: &SelectedProgram, p: &'a Value) -> Result<Option<&'a Value>> {
    let inputs = shape::array(m, p, "links")?;
    let Some(def) = definition(m, program, p)? else {
        if !w::compare(m, inputs.len(), 0usize)?.is_eq() { return Err(shape::fail("undeclared inline link")); }
        return Ok(None);
    };
    if !w::compare(m, inputs.len(), 1usize)?.is_eq() { return Err(shape::fail("required single inline predecessor")); }
    let input = &inputs[0];
    shape::closed(m, input, &["slot", "predecessor"], &[])?;
    shape::equal(m, &input["slot"], &def["name"])?;
    shape::reference(m, &input["predecessor"])?;
    Ok(Some(input))
}
fn selected_right(m: &mut CoreMeter, grant: &Value, slot: &Value) -> Result<()> {
    let permissions = shape::array(m, grant, "permissions")?;
    if !shape::member(m, permissions, &json!("link"))? { return Err(shape::fail("source link permission")); }
    let mut allowed = false;
    for selector in shape::array(m, grant, "link_selectors")? {
        let names = shape::array(m, selector, "names")?;
        allowed |= shape::member(m, names, slot)?;
    }
    if !allowed { return Err(shape::fail("source original/current link selector")); }
    Ok(())
}
fn rights(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, authority: &Value, slot: &Value) -> Result<()> {
    for value in std::iter::once(&authority["current_grant_ref"]).chain(shape::array(m, authority, "original_grant_refs")?.iter()) {
        let reference = shape::reference(m, value)?;
        let grant = r.get(m, &c.key, &reference, "workflow-source-grant", "ledger-workflow-source-grant/1")?;
        selected_right(m, &grant, slot)?;
    }
    Ok(())
}
fn endpoint(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, p: &Value, predecessor: &Value) -> Result<()> {
    let def = definition(m, program, p)?.ok_or_else(|| shape::fail("original inline declaration"))?;
    let reference = shape::reference(m, predecessor)?;
    let work = r.get(m, &c.key, &reference, "workflow-work", "ledger-workflow-work-record/1")?;
    let original = shape::field(m, &work, "payload")?;
    for field in ["product_ref", "policy_ref", "binding_ref", "customer", "chain"] { shape::equal(m, &p[field], &original[field])?; }
    shape::equal(m, &original["status"], &json!("succeeded"))?;
    let activities = shape::array(m, def, "predecessor_activities")?;
    if !w::compare(m, activities.len(), 1usize)?.is_eq() { return Err(shape::fail("exact predecessor activity")); }
    shape::equal(m, &original["activity"], &activities[0])?;
    // In the admitted topology A has no outgoing Work link. This proves the
    // concrete acyclic two-hop shape without treating declaration reachability
    // as authorization or walking arbitrary history.
    let prior_links = shape::array(m, original, "links")?;
    if !w::compare(m, prior_links.len(), 0usize)?.is_eq() { return Err(shape::fail("priced root has predecessor")); }
    let claim_id = shape::text(m, &work, "claim_id")?;
    let claim = r.by_identity(m, &c.key, "workflow-work-claim", claim_id, "ledger-workflow-work-claim/1")?;
    super::inventory::entry::historical_work_claim(m, c, &claim.0, &claim.1, &work)?;
    let a = crate::domain::Timestamp::parse_with_work(shape::text(m, &work, "accepted_at")?, m)?.micros();
    let b = crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if w::compare(m, a, b)?.is_gt() { return Err(shape::fail("future predecessor")); }
    Ok(())
}
/// Fresh B uses its own current authenticated authority and exact original A.
pub(super) fn emit_work(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, selected: (&Value, &ExactRef, &ExactRef, &DraftStateData)) -> Result<Option<ConstructedEnvelope>> {
    let (p, child, authority_ref, state) = selected;
    let Some(input) = validate_inputs(m, program, p)? else { return Ok(None); };
    endpoint(m, r, c, program, p, &input["predecessor"])?;
    let predecessor = shape::reference(m, &input["predecessor"])?;
    let prior = r.get(m, &c.key, &predecessor, "workflow-work", "ledger-workflow-work-record/1")?;
    current_work(m, r, c, &predecessor, &prior, state)?;
    let mut inbound = 1usize;
    for reference in &state.links {
        let link = r.get(m, &c.key, reference, "workflow-link", "ledger-workflow-link/1")?;
        if shape::same(m, &link["predecessor"], &input["predecessor"])? { inbound = w::usize_add(m, inbound, 1)?; }
    }
    if w::compare(m, inbound, 128usize)?.is_gt() { return Err(shape::fail("predecessor inbound ceiling")); }
    // The just-constructed authority is supplied by the caller, so its current
    // and original grant Refs are the exact same checked context operands.
    let current = c.authorities.first().ok_or_else(|| shape::fail("inline source authority"))?;
    let grant = r.get(m, &c.key, &current.current_grant_ref, "workflow-source-grant", "ledger-workflow-source-grant/1")?;
    selected_right(m, &grant, &input["slot"])?;
    for reference in &current.original_grant_refs {
        let grant = r.get(m, &c.key, reference, "workflow-source-grant", "ledger-workflow-source-grant/1")?;
        selected_right(m, &grant, &input["slot"])?;
    }
    let arena = m.representation_arena().ok_or_else(|| shape::fail("owning Work link"))?;
    let key = super::cost_postings::operation_key(m, &arena, c)?;
    let child = super::working_value::owning_reference(m, &arena, child)?;
    let authority = super::working_value::owning_reference(m, &arena, authority_ref)?;
    let tuple = arena.array_from_values(&[&key.value()["scope"], child.value(), &p["product_ref"], &input["slot"], &input["predecessor"]], m)?;
    let id = body::semantic_id(m, "wlink_", "workflow-link", tuple.value())?;
    let schema = arena.string_value("ledger-workflow-link/1", m)?;
    let classification = arena.string_value("monetary", m)?;
    let value = arena.object_from_fields(&[("schema", schema.value()), ("scope", &key.value()["scope"]), ("child", child.value()), ("product_ref", &p["product_ref"]), ("binding_ref", &p["binding_ref"]), ("slot", &input["slot"]), ("predecessor", &input["predecessor"]), ("classification", classification.value()), ("authority_ref", authority.value()), ("operation_key", key.value())], m)?;
    Ok(Some(body::record_borrowed(m, &c.key, "workflow-link", id, value.value())?))
}
/// Validate an immutable B edge from its original Work declaration. Current B
/// eligibility is checked separately only for the first outcome claim.
pub(super) fn retained_work(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, work_ref: &ExactRef, work: &Value) -> Result<ExactRef> {
    let p = shape::field(m, work, "payload")?;
    let input = validate_inputs(m, program, p)?.ok_or_else(|| shape::fail("report inline link missing"))?;
    endpoint(m, r, c, program, p, &input["predecessor"])?;
    let id = body::semantic_id(m, "wlink_", "workflow-link", &json!([c.key.scope, work_ref, p["product_ref"], input["slot"], input["predecessor"]]))?;
    let (reference, link) = r.by_identity(m, &c.key, "workflow-link", &id, "ledger-workflow-link/1")?;
    shape::closed(m, &link, &["schema", "scope", "child", "product_ref", "binding_ref", "slot", "predecessor", "classification", "authority_ref", "operation_key"], &[])?;
    let arena = m.representation_arena().ok_or_else(|| shape::fail("owning retained Link association"))?;
    let key = super::cost_postings::operation_key(m, &arena, c)?;
    let child = super::working_value::owning_reference(m, &arena, work_ref)?;
    let classification = arena.string_value("monetary", m)?;
    for (field, expected) in [("scope", &key.value()["scope"]), ("child", child.value()), ("product_ref", &p["product_ref"]), ("binding_ref", &p["binding_ref"]), ("slot", &input["slot"]), ("predecessor", &input["predecessor"]), ("classification", classification.value()), ("operation_key", &work["key"])] { shape::equal(m, &link[field], expected)?; }
    let authority_ref = shape::reference(m, &link["authority_ref"])?;
    let authority = r.get(m, &c.key, &authority_ref, "workflow-economic-authority", "ledger-workflow-economic-authority/8")?;
    authority::historical_namespace(m, r, c, &link["authority_ref"], &work["source_namespace"], Some(&work["key"]["source"]), ("work.accept", &p["binding_ref"], &work["accepted_at"]))?;
    rights(m, r, c, &authority, &input["slot"])?;
    Ok(reference)
}

pub(super) struct ReportPath {
    pub reference: ExactRef,
    pub work: super::record_body::RecordBody,
    pub link: ExactRef,
    pub claim: ExactRef,
    pub decision: ExactRef,
    _owner: crate::canonical::audit_arena::ExternalBufferLease,
}
pub(super) fn report_path(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, p: &Value, selected: (&ExactRef, &Value), current: Option<&DraftStateData>) -> Result<Option<ReportPath>> {
    let schema = shape::text(m, p, "schema")?;
    if !shape::same_text(m, schema, "ledger-workflow-outcome/2")? { return Ok(None); }
    if !program.uses_declared_work_link_recipe() { return Err(shape::fail("original declared link recipe")); }
    let arena = m.representation_arena().ok_or_else(|| shape::fail("owning selector report"))?;
    let bytes = std::mem::size_of::<ReportPath>() + 5 * (std::mem::size_of::<ExactRef>() + 64 + 128 + 71);
    let owner = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let reference = shape::reference(m, &p["selector_start_work_ref"])?;
    let work = r.get(m, &c.key, &reference, "workflow-work", "ledger-workflow-work-record/1")?;
    let (target_work, family) = selected;
    shape::equal(m, &work["payload"]["binding_ref"], &p["binding_ref"])?;
    shape::equal(m, &work["payload"]["policy_ref"], &p["policy_ref"])?;
    shape::equal(m, &work["payload"]["product_ref"], &p["product_ref"])?;
    shape::equal(m, &work["payload"]["status"], &json!("succeeded"))?;
    let input = validate_inputs(m, program, &work["payload"])?.ok_or_else(|| shape::fail("report declared predecessor"))?;
    shape::equal(m, &input["predecessor"], &json!(target_work))?;
    let path = shape::array(m, &family["selector"], "path")?;
    if !w::compare(m, path.len(), 2usize)?.is_eq() { return Err(shape::fail("two-hop original family selector")); }
    shape::equal(m, &family["target_link_slot"], &path[0])?;
    shape::equal(m, &input["slot"], &path[1])?;
    let product = product(m, program, p)?;
    let defs = shape::array(m, product, "links")?;
    let slot = shape::text(m, family, "target_link_slot")?;
    let def = shape::named(m, defs, "name", slot)?;
    shape::equal(m, &def["child_activity"], &p["activity"])?;
    let predecessor_activities = shape::array(m, def, "predecessor_activities")?;
    if !w::compare(m, predecessor_activities.len(), 1usize)?.is_eq() { return Err(shape::fail("single original report activity")); }
    shape::equal(m, &work["payload"]["activity"], &predecessor_activities[0])?;
    let link = retained_work(m, r, c, program, &reference, &work)?;
    let claim_id = shape::text(m, &work, "claim_id")?;
    let (claim, _) = r.by_identity(m, &c.key, "workflow-work-claim", claim_id, "ledger-workflow-work-claim/1")?;
    let decision_id = super::work_keys::decision(m, claim_id)?;
    let id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", &json!([decision_id, "1"]))?;
    let (decision, _) = r.by_identity(m, &c.key, "workflow-decision", &id, "ledger-workflow-decision-revision/1")?;
    if let Some(state) = current { current_work(m, r, c, &reference, &work, state)?; }
    Ok(Some(ReportPath { reference, work, link, claim, decision, _owner: owner }))
}
pub(super) fn current_work(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, reference: &ExactRef, work: &Value, state: &DraftStateData) -> Result<()> {
    let claim = shape::text(m, work, "claim_id")?;
    let id = super::work_keys::decision(m, claim)?;
    let mut found = None;
    for head in &state.decision_heads {
        if shape::same_text(m, &head.decision_id, &id)? && found.replace(head).is_some() { return Err(shape::fail("duplicate predecessor root head")); }
    }
    let head = found.ok_or_else(|| shape::fail("current predecessor root head missing"))?;
    let decision = r.get_schemas(m, &c.key, &head.reference, "workflow-decision", &["ledger-workflow-decision-revision/1", "ledger-workflow-decision-revision/2"])?;
    shape::equal(m, &decision["decision_id"], &json!(id))?;
    shape::equal(m, &decision["revision"], &json!(head.revision))?;
    shape::equal(m, &decision["subject_ref"], &json!(reference))?;
    shape::equal(m, &decision["kind"], &json!("work"))?;
    shape::equal(m, &decision["original_terms_ref"], &work["payload"]["binding_ref"])?;
    let status = shape::text(m, &decision, "status")?;
    if !shape::same_text(m, status, "active")? { return Err(crate::Error::new("TARGET_CLOSED", "declared predecessor root reversed")); }
    Ok(())
}
pub(super) fn verify_report_original(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, report: &ReportPath, terms: &authority::Terms) -> Result<()> {
    let decision = r.get(m, &c.key, &report.decision, "workflow-decision", "ledger-workflow-decision-revision/1")?;
    super::outcome::verify_original_actions(m, r, c, program, terms, &report.reference, (&report.work, &report.claim, &report.decision, &decision))
}
/// Retain the exact original report certificate operands in the existing
/// evaluation's typed Ref set. Monetary posting basis remains A actions only.
pub(super) fn evaluation_basis(m: &mut CoreMeter, basis: &Value, report: &ReportPath, outcome_link: &ExactRef) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let arena = m.representation_arena().ok_or_else(|| shape::fail("owning selector evaluation"))?;
    let work = super::working_value::owning_reference(m, &arena, &report.reference)?;
    let claim = super::working_value::owning_reference(m, &arena, &report.claim)?;
    let decision = super::working_value::owning_reference(m, &arena, &report.decision)?;
    let link = super::working_value::owning_reference(m, &arena, &report.link)?;
    let outcome = super::working_value::owning_reference(m, &arena, outcome_link)?;
    let basis = basis.as_array().ok_or_else(|| shape::fail("original action basis array"))?;
    let count = w::usize_add(m, basis.len(), 5)?;
    let bytes = count * std::mem::size_of::<&Value>();
    let _owner = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(count).map_err(|_| shape::fail("selector evaluation Ref array"))?;
    refs.extend(basis.iter());
    refs.extend([work.value(), claim.value(), decision.value(), link.value(), outcome.value()]);
    let per = std::mem::size_of::<ExactRef>() + std::mem::size_of::<&ExactRef>() + 64 + 128 + 71;
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Multiply, &[&count.into(), &per.into()])?;
    let n = count.checked_mul(per).ok_or_else(|| shape::fail("selector evaluation controls"))?;
    let _selected_owner = arena.reserve_external_payload(n + 2 * std::mem::size_of::<Vec<ExactRef>>(), m)?;
    m.write_bytes(n + 2 * std::mem::size_of::<Vec<ExactRef>>())?;
    let mut selected = Vec::new();
    selected.try_reserve_exact(count).map_err(|_| shape::fail("selector evaluation Ref controls"))?;
    for reference in &refs { selected.push(shape::reference(m, reference)?); }
    let mut pointers = Vec::new();
    pointers.try_reserve_exact(count).map_err(|_| shape::fail("selector evaluation pointers"))?;
    pointers.extend(selected.iter());
    let set = super::app_cost::reference_set(m, &arena, &pointers)?;
    Ok(set)
}

/// Validate every immutable chain edge's concrete producer association, even
/// when it is not on the currently selected family path. Origin custody and
/// complete-set/current-head locking are independent host obligations.
pub(super) fn validate_chain(m: &mut CoreMeter, r: &mut Records, c: &DraftContextData, program: &SelectedProgram, current: &Value, links: &[ExactRef]) -> Result<()> {
    let arena = m.representation_arena().ok_or_else(|| shape::fail("owning chain associations"))?;
    let key = super::cost_postings::operation_key(m, &arena, c)?;
    for (i, reference) in links.iter().enumerate() {
        for prior in &links[..i] { if shape::same_ref(m, prior, reference)? { return Err(shape::fail("duplicate chain Link")); } }
        let link = r.get(m, &c.key, reference, "workflow-link", "ledger-workflow-link/1")?;
        shape::closed(m, &link, &["schema", "scope", "child", "product_ref", "binding_ref", "slot", "predecessor", "classification", "authority_ref", "operation_key"], &[])?;
        shape::equal(m, &link["scope"], &key.value()["scope"])?;
        shape::equal(m, &link["binding_ref"], &current["binding_ref"])?;
        shape::equal(m, &link["product_ref"], &current["product_ref"])?;
        shape::equal(m, &link["classification"], &json!("monetary"))?;
        let child = shape::reference(m, &link["child"])?;
        let child_body = if shape::same_text(m, &child.kind, "workflow-work")? {
            r.get(m, &c.key, &child, "workflow-work", "ledger-workflow-work-record/1")?
        } else if shape::same_text(m, &child.kind, "workflow-outcome-claim")? {
            r.get(m, &c.key, &child, "workflow-outcome-claim", "ledger-workflow-outcome-claim/2")?
        } else { return Err(shape::fail("declared chain child kind")); };
        let work_child = shape::same_text(m, &child.kind, "workflow-work")?;
        let payload = shape::field(m, &child_body, if work_child { "payload" } else { "original_payload" })?;
        for field in ["product_ref", "policy_ref", "binding_ref"] { shape::equal(m, &payload[field], &current[field])?; }
        let def = definition(m, program, payload)?.ok_or_else(|| shape::fail("declared Link producer activity"))?;
        shape::equal(m, &link["slot"], &def["name"])?;
        if work_child {
            let input = validate_inputs(m, program, payload)?.ok_or_else(|| shape::fail("chain Work link nomination"))?;
            shape::equal(m, &link["predecessor"], &input["predecessor"])?;
            shape::equal(m, &link["operation_key"], &child_body["key"])?;
            shape::equal(m, &payload["customer"], &current["customer"])?;
            shape::equal(m, &payload["chain"], &current["chain"])?;
        } else {
            super::correction::dictionary_outcome(m, payload)?;
            shape::equal(m, &payload["schema"], &json!("ledger-workflow-outcome/2"))?;
            shape::equal(m, &link["predecessor"], &payload["selector_start_work_ref"])?;
            shape::equal(m, &child_body["scope"], &key.value()["scope"])?;
            for field in ["target_ref", "family_slot", "business_outcome_token"] { shape::equal(m, &child_body[field], &payload[field])?; }
            let target = shape::reference(m, &payload["target_ref"])?;
            let target = r.get(m, &c.key, &target, "workflow-target", "ledger-workflow-component-target/1")?;
            shape::equal(m, &child_body["agreement_id"], &target["agreement_id"])?;
            shape::equal(m, &target["binding_ref"], &current["binding_ref"])?;
        }
        let predecessor = shape::reference(m, &link["predecessor"])?;
        let previous = r.get(m, &c.key, &predecessor, "workflow-work", "ledger-workflow-work-record/1")?;
        let previous_payload = shape::field(m, &previous, "payload")?;
        for field in ["product_ref", "policy_ref", "binding_ref", "customer", "chain"] { shape::equal(m, &previous_payload[field], &current[field])?; }
        shape::equal(m, &previous_payload["status"], &json!("succeeded"))?;
        let activities = shape::array(m, def, "predecessor_activities")?;
        if !w::compare(m, activities.len(), 1usize)?.is_eq() { return Err(shape::fail("declared chain predecessor cardinality")); }
        shape::equal(m, &previous_payload["activity"], &activities[0])?;
        let id = body::semantic_id(m, "wlink_", "workflow-link", &json!([c.key.scope, child, link["product_ref"], link["slot"], predecessor]))?;
        if !shape::same_text(m, &id, &reference.id)? { return Err(shape::fail("chain Link semantic identity")); }
        authority::historical_namespace(m, r, c, &link["authority_ref"], &child_body["source_namespace"], Some(&link["operation_key"]["source"]), (if work_child { "work.accept" } else { "outcome.submit" }, &payload["binding_ref"], &child_body["accepted_at"]))?;
        let authority_ref = shape::reference(m, &link["authority_ref"])?;
        let authority = r.get(m, &c.key, &authority_ref, "workflow-economic-authority", "ledger-workflow-economic-authority/8")?;
        rights(m, r, c, &authority, &link["slot"])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::{self, audit_arena::AuditArena}, policy::workflow::selected_program::{compile_selected_publication, load_local_selected_execution, SelectedCompilerSupport, SelectedSourceInput}, validation_work::{OriginalAuditSession, ResourceLimits}};
    fn input(support: &SelectedCompilerSupport) -> SelectedSourceInput {
        let v: Value = serde_json::from_slice(include_bytes!("../../../../../../examples/workflow-declared-link/source.json")).unwrap();
        let mut docs = v["documents"].as_array().unwrap().clone();
        docs.push(support.canonical_value().clone());
        SelectedSourceInput { documents: docs.iter().map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(), root_policy_ref: serde_json::from_value(v["root_policy_ref"].clone()).unwrap() }
    }
    #[test]
    fn declared_link_actual_source_compiles_and_reloads_exact_recipe_and_plan() {
        let a = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let support = SelectedCompilerSupport::local_declared_link_execution_with_session(&mut s).unwrap();
        let p = compile_selected_publication(input(&support), &support, &mut s).unwrap();
        assert!(p.uses_declared_work_link_recipe());
        p.require_local_admission_with_session(&mut s).unwrap();
        let plan = canonical::CanonicalBytes::from_value(p.compiled_plan().canonical_value()).unwrap().into_vec();
        let reference = p.compiled_plan().reference().clone();
        let (_b, mut reload_session) = { let b = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap(); let s = OriginalAuditSession::for_resource_profile_v2(&b).unwrap(); (b,s) };
        let restored = load_local_selected_execution(input(&support), &plan, &reference, &mut reload_session).unwrap();
        assert!(restored.uses_declared_work_link_recipe());
        assert_eq!(canonical::CanonicalBytes::from_value(restored.compiled_plan().canonical_value()).unwrap().as_slice(), plan);
        let mut m = CoreMeter::new(65_536).unwrap();
        let prices = super::super::pricing::work(&restored, "support:report", (&json!({}), &json!({})), &mut m).unwrap();
        assert!(prices.is_empty(), "unpriced report is not a rounded-zero or failed fee");
        assert!(validate_inputs(&mut m, &restored, &json!({"activity":"support:report","product_ref":restored.compiled_plan().canonical_value()["product_ref"],"links":[]})).is_err());
    }
    #[test]
    fn closed_outcome2_requires_exact_selector_and_old_dictionary_stays_closed() {
        let mut m = CoreMeter::new(65_536).unwrap();
        let reference = json!({"kind":"workflow-work","id":format!("wevent_{}", "a".repeat(64)),"digest":format!("sha256:{}", "b".repeat(64))});
        let mut p = json!({"schema":"ledger-workflow-outcome/2","target_ref":reference,"family_slot":"test:f","business_outcome_token":"token","activity":"test:o","product_ref":reference,"policy_ref":reference,"fact_schema_ref":reference,"binding_ref":reference,"facts":{},"fact_proofs":[],"evidence_refs":[],"occurred_at":"2026-10-25T00:00:00.000000Z","selector_start_work_ref":reference});
        super::super::correction::dictionary_outcome(&mut m, &p).unwrap();
        p.as_object_mut().unwrap().remove("selector_start_work_ref");
        assert!(super::super::correction::dictionary_outcome(&mut m, &p).is_err());
        p["schema"] = json!("ledger-workflow-outcome/1");
        super::super::correction::dictionary_outcome(&mut m, &p).unwrap();
        p["selector_start_work_ref"] = reference;
        assert!(super::super::correction::dictionary_outcome(&mut m, &p).is_err());
    }
}

