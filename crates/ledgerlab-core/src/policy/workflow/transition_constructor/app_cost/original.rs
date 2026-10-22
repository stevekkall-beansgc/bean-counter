//! Exact original work/component/declaration selection shared by observation,
//! final-evidence retention and finalization/correction. Pure DATA association;
//! completed original publication/assent/prefix custody stays a host obligation.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    arithmetic,
    authority::{self, EconomicMode},
    body,
    data::*,
    inventory::entry,
    meter, outcome,
    record_body::RecordBody,
    records::Records,
    shape,
    source_authority::SourceSelection,
    working_value::{owning_reference, NormalizedInput},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    domain::Timestamp,
    validation_work::{self as w, ValidationWork},
    Error, Result,
};
use serde_json::Value;

macro_rules! call {
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr,$d:expr) => {{
        let (a, b, c, d) = ($a, $b, $c, $d);
        $f($m, a, b, c, d)
    }};
}
pub(super) struct OriginalApp<'p> {
    pub work_ref: ExactRef,
    pub work: RecordBody,
    pub claim_ref: ExactRef,
    pub _claim: RecordBody,
    pub binding_ref: ExactRef,
    pub terms: authority::Terms,
    pub declaration: &'p Value,
    pub _instruction: &'p Value,
    pub component_slot: String,
    pub original_decision_ref: ExactRef,
    pub(super) _ownership: ExternalBufferLease,
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn exact_definition(
    m: &mut CoreMeter,
    v: &Value,
    r: &crate::policy::workflow::DefinitionRef,
) -> Result<()> {
    shape::closed(m, v, &["kind", "id", "digest"], &[])?;
    for (key, expected) in [("kind", r.kind()), ("id", r.id()), ("digest", r.digest())] {
        let actual = shape::text(m, v, key)?;
        if !shape::same_text(m, actual, expected)? {
            return Err(shape::fail("app original definition substitution"));
        }
    }
    Ok(())
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
pub(super) fn select<'p>(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &'p SelectedProgram,
    work_ref: &ExactRef,
    component_slot: &str,
    state: &EconomicStateData,
) -> Result<OriginalApp<'p>> {
    select_mode(m, r, c, program, work_ref, component_slot, (state, true))
}
pub(super) fn select_for_root<'p>(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &'p SelectedProgram,
    work_ref: &ExactRef,
    component_slot: &str,
    state: &EconomicStateData,
) -> Result<OriginalApp<'p>> {
    select_mode(m, r, c, program, work_ref, component_slot, (state, false))
}
fn select_mode<'p>(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &'p SelectedProgram,
    work_ref: &ExactRef,
    component_slot: &str,
    selected_operands: (&EconomicStateData, bool),
) -> Result<OriginalApp<'p>> {
    let (state, require_initial_root) = selected_operands;
    if !program.uses_app_owned_cost_recipe() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "original app cost recipe",
        ));
    }
    entry::context_selected(m, c, program)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app original selection"))?;
    // All copied references below have the existing (64,128,71) grammar; one
    // slot<=128. Six Ref payloads plus typed headers precede their producers.
    let refs = w::usize_add(m, 64, 128)?;
    let refs = w::usize_add(m, refs, 71)?;
    ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&refs.into(), &6usize.into()],
    )?;
    let refs = refs
        .checked_mul(6)
        .ok_or_else(|| shape::fail("app original reference metadata"))?;
    let bytes = w::usize_add(m, std::mem::size_of::<OriginalApp>(), refs)?;
    let bytes = w::usize_add(m, bytes, 128)?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    super::super::records::reference(m, work_ref)?;
    let work = r.get(
        m,
        &c.key,
        work_ref,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::closed(
        m,
        &work,
        &[
            "schema",
            "key",
            "payload",
            "claim_id",
            "received_at",
            "accepted_at",
            "source_namespace",
        ],
        &[],
    )?;
    let p = shape::field(m, &work, "payload")?;
    if !call!(
        shape::same_text,
        m,
        shape::text(m, p, "status")?,
        "succeeded"
    )? {
        return Err(Error::new(
            "TARGET_CLOSED",
            "app observation requires successful original work",
        ));
    }
    exact_definition(m, &p["policy_ref"], program.root_policy_ref())?;
    exact_definition(m, &p["product_ref"], program.product_ref())?;
    let owner = super::super::cost_postings::operation_key(m, &arena, c)?;
    eq(
        m,
        &work["key"]["logical_store_id"],
        &owner.value()["logical_store_id"],
    )?;
    eq(m, &work["key"]["scope"], &owner.value()["scope"])?;
    let tuple = arena.array_from_values(
        &[
            &work["key"]["logical_store_id"],
            &work["key"]["scope"],
            &work["key"]["source"],
            &p["external_event_id"],
        ],
        m,
    )?;
    let expected = body::semantic_id(m, "wevent_", "workflow-event", tuple.value())?;
    if !shape::same_text(m, &expected, &work_ref.id)? {
        return Err(shape::fail("app original work identity"));
    }
    let claim_id = shape::text(m, &work, "claim_id")?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    entry::historical_work_claim(m, c, &claim_ref, &claim, &work)?;
    let exact_work = owning_reference(m, &arena, work_ref)?;
    eq(m, &claim["work_ref"], exact_work.value())?;
    outcome::original_claim_association(m, &claim, &work)?;
    let (binding_ref, binding) = load(
        m,
        r,
        c,
        &p["binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    exact_definition(
        m,
        &binding["compiled_plan_ref"],
        program.compiled_plan().reference(),
    )?;
    eq(m, &binding["source_namespace"], &work["source_namespace"])?;
    for key in ["customer", "policy_ref", "product_ref"] {
        eq(m, &binding[key], &p[key])?;
    }
    let (instantiation_ref, instantiation) = load(
        m,
        r,
        c,
        &binding["slot_instantiation_ref"],
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let _ = instantiation_ref;
    let maps = shape::array(m, &instantiation, "component_map")?;
    let mapping = shape::named(m, maps, "slot_id", component_slot)?;
    let component_key = shape::text(m, mapping, "template_key")?;
    let (catalog_ref, catalog) = load(
        m,
        r,
        c,
        &binding["slot_catalog_ref"],
        "workflow-slot-catalog",
        "ledger-workflow-slot-catalog/1",
    )?;
    let _ = catalog_ref;
    let component = call!(
        shape::named,
        m,
        shape::array(m, &catalog, "components")?,
        "slot_id",
        component_slot
    )?;
    if !call!(
        shape::same_text,
        m,
        shape::text(m, component, "book")?,
        "retail"
    )? {
        return Err(shape::fail("app customer component book"));
    }
    let mut instruction = None;
    for candidate in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("app.original_cost_instruction")?;
        if !call!(
            shape::same_text,
            m,
            shape::text(m, candidate, "opcode")?,
            "evaluate_rule"
        )? {
            continue;
        }
        let rule = &candidate["rule"];
        if !call!(
            shape::same_text,
            m,
            shape::text(m, rule, "component_key")?,
            component_key
        )? || !shape::same(m, &rule["activity"], &p["activity"])?
        {
            continue;
        }
        if instruction.is_some() {
            return Err(shape::fail("ambiguous app original instruction"));
        }
        if !call!(
            shape::same_text,
            m,
            shape::text(m, &rule["formula"], "kind")?,
            "app_final_cost"
        )? {
            return Err(shape::fail("app target requires original app formula"));
        }
        instruction = Some(candidate);
    }
    let instruction = instruction.ok_or_else(|| shape::fail("app original instruction missing"))?;
    let slot = shape::text(m, &instruction["rule"], "binding_slot")?;
    let selected = call!(
        shape::named,
        m,
        shape::array(m, &binding, "bindings")?,
        "slot",
        slot
    )?;
    let schema = if program.uses_app_cost_causal_recipe() {
        "ledger-workflow-slot-binding-terms/7"
    } else {
        "ledger-workflow-slot-binding-terms/6"
    };
    let (terms_ref, terms) = load(
        m,
        r,
        c,
        &selected["binding_ref"],
        "workflow-slot-terms",
        schema,
    )?;
    shape::closed(
        m,
        &terms,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "slot",
            "book",
            "role_slot",
            "role_ref",
            "funding_leg",
            "allowed_activities",
            "quantity_ceilings",
            "exposure_ceiling",
            "source_grant_refs",
            "correction_source_slot",
            "causal_reversal_clauses",
            "exposure_scope",
            "app_cost_terms",
        ],
        &[],
    )?;
    eq(m, &terms["scope"], &owner.value()["scope"])?;
    for key in ["agreement_id", "customer"] {
        eq(m, &terms[key], &binding[key])?;
    }
    if !call!(
        shape::same_text,
        m,
        shape::text(m, &terms, "exposure_scope")?,
        "original_work"
    )? || !call!(
        shape::same_text,
        m,
        shape::text(m, &terms, "book")?,
        "retail"
    )? {
        return Err(shape::fail("app original exposure/book"));
    }
    let mut declaration = None;
    for row in shape::array(m, program.original_policy_value(), "app_delayed_costs")? {
        m.visit("app.original_cost_declaration")?;
        if !call!(
            shape::same_text,
            m,
            shape::text(m, row, "component_key")?,
            component_key
        )? || !shape::same(m, &row["activity"], &p["activity"])?
        {
            continue;
        }
        if declaration.is_some() {
            return Err(shape::fail("ambiguous app original declaration"));
        }
        declaration = Some(row);
    }
    let declaration = declaration.ok_or_else(|| shape::fail("app original declaration missing"))?;
    let mut accepted_declaration = None;
    for row in shape::array(m, &terms, "app_cost_terms")? {
        m.visit("app.accepted_original_declaration")?;
        if call!(
            shape::same_text,
            m,
            shape::text(m, row, "component_key")?,
            component_key
        )? && shape::same(m, &row["activity"], &p["activity"])?
        {
            if accepted_declaration.is_some() {
                return Err(shape::fail("duplicate app accepted declaration"));
            }
            accepted_declaration = Some(row);
        }
    }
    eq(
        m,
        accepted_declaration.ok_or_else(|| shape::fail("app declaration was not accepted"))?,
        declaration,
    )?;
    eq(
        m,
        &instruction["rule"]["formula"]["evidence_slot"],
        &declaration["cost_source_slot"],
    )?;
    let frame = &c.authorities[0];
    if !shape::same_ref(m, &binding_ref, &frame.binding_ref)? {
        return Err(shape::fail("app original binding selection"));
    }
    let accepted = r.get(
        m,
        &c.key,
        &frame.accepted_agreement_ref,
        "workflow-accepted-agreement",
        "ledger-workflow-accepted-agreement/1",
    )?;
    eq(m, &accepted["binding_ref"], &p["binding_ref"])?;
    eq(m, &accepted["scope"], &owner.value()["scope"])?;
    for key in ["customer", "agreement_id", "version"] {
        eq(m, &accepted[key], &binding[key])?;
    }
    let activation = r.get(
        m,
        &c.key,
        &frame.activation_ref,
        "workflow-activation",
        "ledger-workflow-activation-transition/1",
    )?;
    let constants = m.parse_owned_representation(
        &arena,
        br#"["1","work"]"#,
        w::RepresentationKind::SourceDefinition,
    )?;
    let claim_id = arena.string_value(&claim_ref.id, m)?;
    let decision_tuple = arena.array_from_values(&[claim_id.value(), &constants.value()[1]], m)?;
    let decision_id =
        body::semantic_id(m, "wdecision_", "workflow-decision", decision_tuple.value())?;
    let decision_id_value = arena.string_value(&decision_id, m)?;
    let revision_tuple =
        arena.array_from_values(&[decision_id_value.value(), &constants.value()[0]], m)?;
    let original_id = body::semantic_id(
        m,
        "wdrev_",
        "workflow-decision-revision",
        revision_tuple.value(),
    )?;
    let (original_decision_ref, original_decision) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &original_id,
        "ledger-workflow-decision-revision/1",
    )?;
    eq(m, &original_decision["subject_ref"], exact_work.value())?;
    eq(
        m,
        &original_decision["original_terms_ref"],
        &p["binding_ref"],
    )?;
    if program.uses_local_execution_recipe() {
        // App observation/finalization has its own original Work selector; it
        // does not pass through the family's original-action verifier.
        let namespace = shape::text(m, &work, "source_namespace")?;
        let received = shape::text(m, &work, "received_at")?;
        super::super::work_proofs::retained(
            m,
            r,
            c,
            program,
            &super::super::work_proofs::WorkProofRequest {
                payload: p,
                facts: &p["facts"],
                namespace,
                received_at: received,
                only_field: None,
                worker_authority: Some(&original_decision["authority_ref"]),
            },
            &original_decision["authority_ref"],
            true,
        )?;
    }
    let mut root = None;
    for head in &state.financial.decision_heads {
        m.visit("app.current_original_root")?;
        if shape::same_text(m, &head.decision_id, &decision_id)? {
            if root.is_some() {
                return Err(shape::fail("duplicate app root head"));
            }
            root = Some(head);
        }
    }
    let root = root.ok_or_else(|| shape::fail("app current root head missing"))?;
    if require_initial_root
        && (!shape::same_ref(m, &root.reference, &original_decision_ref)?
            || !shape::same_text(m, &root.revision, "1")?)
    {
        return Err(Error::new(
            "TARGET_CLOSED",
            "app original work has been corrected or reversed",
        ));
    }
    let current_root = r.get(
        m,
        &c.key,
        &root.reference,
        "workflow-decision",
        "ledger-workflow-decision-revision/1",
    )?;
    eq(m, &current_root["decision_id"], decision_id_value.value())?;
    eq(m, &current_root["subject_ref"], exact_work.value())?;
    let revision = shape::text(m, &current_root, "revision")?;
    if !shape::same_text(m, revision, &root.revision)? {
        return Err(shape::fail("current app root head revision"));
    }
    let role_ref = shape::reference(m, &terms["role_ref"])?;
    let slot_copy = copy(m, component_slot)?;
    Ok(OriginalApp {
        work_ref: reference(m, work_ref)?,
        work,
        claim_ref,
        _claim: claim,
        binding_ref,
        terms: authority::Terms {
            binding,
            accepted,
            _activation: activation,
            catalog,
            instantiation,
            slot_terms: terms,
            slot_terms_ref: terms_ref,
            role_ref,
        },
        declaration,
        _instruction: instruction,
        component_slot: slot_copy,
        original_decision_ref,
        _ownership: ownership,
    })
}
impl OriginalApp<'_> {
    pub(super) fn source(
        &self,
        m: &mut CoreMeter,
        r: &mut Records,
        c: &DraftContextData,
        program: &SelectedProgram,
        slot: &str,
        mode: EconomicMode,
    ) -> Result<SourceSelection> {
        let (binding, terms, source) =
            super::binding::resolve(m, r, c, &self.work["payload"], program, mode, slot)?;
        if !shape::same_ref(m, &binding, &self.binding_ref)?
            || !shape::same_ref(m, &terms.slot_terms_ref, &self.terms.slot_terms_ref)?
        {
            return Err(shape::fail(
                "app original binding/slot cannot change at current source selection",
            ));
        }
        shape::equal(m, &terms.binding, &self.terms.binding)?;
        Ok(source)
    }
    pub(super) fn observed_quantity(
        &self,
        m: &mut CoreMeter,
        program: &SelectedProgram,
    ) -> Result<arithmetic::Ratio> {
        let p = &self.work["payload"];
        let mut schema = None;
        for (reference, definition) in program.fact_schemas() {
            m.visit("app.original_usage_schema")?;
            let actual = shape::text(m, &p["fact_schema_ref"], "digest")?;
            if shape::same_text(m, reference.digest(), actual)? {
                exact_definition(m, &p["fact_schema_ref"], reference)?;
                if schema.is_some() {
                    return Err(shape::fail("ambiguous app original fact schema"));
                }
                schema = Some(definition);
            }
        }
        let schema = schema.ok_or_else(|| shape::fail("app original usage schema"))?;
        let field = shape::text(m, self.declaration, "usage_field")?;
        let unit = schema.quantity_unit_with_work(field, m)?;
        if !call!(
            shape::same_text,
            m,
            unit,
            shape::text(m, self.declaration, "usage_unit")?
        )? {
            return Err(shape::fail("app original usage unit"));
        }
        let normalized = NormalizedInput::normalize(m, schema, &p["facts"])?;
        eq(m, normalized.canonical_value(), &p["facts"])?;
        let value = shape::text(m, &p["facts"], field)?;
        let value = arithmetic::decimal(m, value)?;
        if meter::cmp(m, &value.numerator, &0.into())?.is_lt() {
            return Err(shape::fail("negative app original usage"));
        }
        call!(
            entry::quantities,
            m,
            &self.terms.binding,
            &self.terms.slot_terms,
            shape::text(m, p, "activity")?,
            &p["facts"]
        )?;
        Ok(value)
    }
    pub(super) fn occurrence(&self, m: &mut CoreMeter) -> Result<i64> {
        m.visit("app.original_work_occurrence_presence")?;
        let actual = match self.work["payload"].get("occurred_at") {
            Some(value) => value
                .as_str()
                .ok_or_else(|| shape::fail("app original occurrence scalar"))?,
            None => shape::text(m, &self.work, "received_at")?,
        };
        Ok(Timestamp::parse_with_work(actual, m)?.micros())
    }
}
