//! RP01 ownership for the selected compiler's existing plan derivation.
//! Rules/families come from the SAME validated typed policy and its normalized
//! source body. This does not qualify publication, economics, or future work.
use super::super::{
    transition_constructor::{shape, CoreMeter},
    DefinitionRef, Product, SlotTemplate,
};
use super::{
    model::{Formula, Policy, Rule},
    SelectedDocument,
};
use crate::{
    canonical::audit_arena::{AuditArena, AuditedValue, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
    Error, Result,
};
use serde_json::Value;
use std::{cmp::Ordering, mem};
fn fail(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
pub(super) struct CompilerRows<T> {
    rows: Vec<T>,
    _ownership: ExternalBufferLease,
}
impl<T> std::ops::Deref for CompilerRows<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.rows
    }
}
impl<T> std::ops::DerefMut for CompilerRows<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.rows
    }
}
impl<'a, T> IntoIterator for &'a CompilerRows<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}
pub(super) fn vector<T>(m: &mut CoreMeter, a: &AuditArena, n: usize) -> Result<CompilerRows<T>> {
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Multiply,
        &[&n.into(), &mem::size_of::<T>().into()],
    )?;
    let bytes = n
        .checked_mul(mem::size_of::<T>())
        .ok_or_else(|| fail("plan vector product"))?;
    let owner = a.reserve_external_payload(bytes, m)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(n)
        .map_err(|_| fail("plan vector allocation"))?;
    Ok(CompilerRows {
        rows,
        _ownership: owner,
    })
}
pub(super) fn reference(
    m: &mut CoreMeter,
    a: &AuditArena,
    r: &DefinitionRef,
) -> Result<AuditedValue> {
    let kind = a.string_value(r.kind(), m)?;
    let id = a.string_value(r.id(), m)?;
    let digest = a.string_value(r.digest(), m)?;
    a.object_from_fields(
        &[
            ("kind", kind.value()),
            ("id", id.value()),
            ("digest", digest.value()),
        ],
        m,
    )
}
pub(super) fn array(
    m: &mut CoreMeter,
    a: &AuditArena,
    rows: &[AuditedValue],
) -> Result<AuditedValue> {
    let mut refs = vector(m, a, rows.len())?;
    for row in rows {
        m.visit("plan.owning_array_member")?;
        refs.push(row.value());
    }
    a.array_from_values(&refs, m)
}
fn value_copy(a: &AuditArena, m: &mut CoreMeter, value: &AuditedValue) -> Result<AuditedValue> {
    a.clone_value(value.value(), m)
}
fn set(m: &mut CoreMeter, a: &AuditArena, rows: &mut [AuditedValue]) -> Result<AuditedValue> {
    super::normalize::owning_set(m, rows, value_copy)?;
    array(m, a, rows)
}
fn order(m: &mut CoreMeter, left: &str, right: &str) -> Result<Ordering> {
    m.visit("plan.ordered_text_compare")?;
    let n = w::usize_add(m, left.len(), right.len())?;
    m.scan_bytes(n)?;
    Ok(left.cmp(right))
}
fn field_same(m: &mut CoreMeter, value: &Value, key: &str, expected: &Value) -> Result<bool> {
    let field = shape::field(m, value, key)?;
    shape::same(m, field, expected)
}
fn text_same(m: &mut CoreMeter, value: &Value, key: &str, expected: &str) -> Result<bool> {
    let field = shape::text(m, value, key)?;
    shape::same_text(m, field, expected)
}
fn ordinal(m: &mut CoreMeter, a: &AuditArena, n: usize) -> Result<AuditedValue> {
    // unsigned_string's bounded native integer output is at most39 digits.
    let _owner = a.reserve_external_payload(39 + mem::size_of::<String>(), m)?;
    let text = w::unsigned_string(n as u128, m)?;
    a.string_value(&text, m)
}
fn source<'a>(
    m: &mut CoreMeter,
    docs: &'a [SelectedDocument],
    r: &Value,
) -> Result<&'a SelectedDocument> {
    let kind = shape::text(m, r, "kind")?;
    let id = shape::text(m, r, "id")?;
    let digest = shape::text(m, r, "digest")?;
    for doc in docs {
        m.visit("plan.exact_source_document")?;
        let actual = doc.reference();
        if shape::same_text(m, actual.kind(), kind)?
            && shape::same_text(m, actual.id(), id)?
            && shape::same_text(m, actual.digest(), digest)?
        {
            return Ok(doc);
        }
    }
    Err(fail("missing exact plan source"))
}
pub(super) struct Output {
    pub body: AuditedValue,
    pub fields: usize,
}
pub(super) fn build(
    m: &mut CoreMeter,
    docs: &[SelectedDocument],
    root: &DefinitionRef,
    p: &Policy,
    product: &Product,
    template: &SlotTemplate,
    selected_operands: (&str, &str),
) -> Result<Output> {
    let (plan_schema, semantics) = selected_operands;
    let a = m
        .representation_arena()
        .ok_or_else(|| fail("owning plan arena"))?;
    let root_value = reference(m, &a, root)?;
    let policy = source(m, docs, root_value.value())?.canonical_value();
    let product_schemas = shape::array(m, product.canonical_value(), "fact_schemas")?;
    let reachable_count = w::usize_add(m, 5, product_schemas.len())?;
    let mut reachable = vector(m, &a, reachable_count)?;
    for r in [
        root,
        &p.product_ref,
        &p.slot_template_ref,
        &p.compiler_ref,
        &p.context_schema_ref,
    ] {
        reachable.push(reference(m, &a, r)?);
    }
    for r in product_schemas {
        let mut found = false;
        for prior in &reachable {
            if shape::same(m, prior.value(), r)? {
                found = true;
                break;
            }
        }
        if !found {
            reachable.push(a.clone_value(r, m)?);
        }
    }
    if !w::compare(m, reachable.len(), docs.len())?.is_eq() {
        return Err(fail("unused source document"));
    }
    let mut source_refs = vector(m, &a, docs.len())?;
    for doc in docs {
        let value = reference(m, &a, doc.reference())?;
        let mut found = false;
        for r in &reachable {
            if shape::same(m, r.value(), value.value())? {
                found = true;
                break;
            }
        }
        if !found {
            return Err(fail("unreachable source"));
        }
        source_refs.push(value);
    }
    let source_refs = set(m, &a, &mut source_refs)?;
    let schemas_count = w::usize_add(m, product_schemas.len(), 1)?;
    let mut schema_refs = vector(m, &a, schemas_count)?;
    for r in product_schemas {
        schema_refs.push(r);
    }
    let context = shape::field(m, policy, "context_schema_ref")?;
    let mut found = false;
    for r in &schema_refs {
        if shape::same(m, r, context)? {
            found = true;
            break;
        }
    }
    if !found {
        schema_refs.push(context);
    }
    for i in 1..schema_refs.len() {
        let mut j = i;
        while j > 0 {
            let left = shape::text(m, schema_refs[j - 1], "digest")?;
            let right = shape::text(m, schema_refs[j], "digest")?;
            if order(m, left, right)? != Ordering::Greater {
                break;
            }
            m.visit("plan.schema_sort_move")?;
            schema_refs.swap(j - 1, j);
            j -= 1;
        }
    }
    let mut field_count = 0;
    for r in &schema_refs {
        let doc = source(m, docs, r)?;
        let n = shape::array(m, doc.canonical_value(), "fields")?.len();
        field_count = w::usize_add(m, field_count, n)?;
    }
    let mut slots = vector(m, &a, field_count)?;
    for r in &schema_refs {
        let doc = source(m, docs, r)?;
        let fields = shape::array(m, doc.canonical_value(), "fields")?;
        let mut sorted = vector(m, &a, fields.len())?;
        for field in fields {
            sorted.push(field);
        }
        for i in 1..sorted.len() {
            let mut j = i;
            while j > 0 {
                let left = shape::text(m, sorted[j - 1], "name")?;
                let right = shape::text(m, sorted[j], "name")?;
                if order(m, left, right)? != Ordering::Greater {
                    break;
                }
                m.visit("plan.field_sort_move")?;
                sorted.swap(j - 1, j);
                j -= 1;
            }
        }
        for &field in sorted.iter() {
            m.visit("plan.fact_slot")?;
            let index = ordinal(m, &a, slots.len())?;
            slots.push(a.object_from_fields(
                &[
                    ("index", index.value()),
                    ("schema_ref", r),
                    ("field", field),
                ],
                m,
            )?);
        }
    }
    let instruction_count = w::usize_add(m, p.rules.len(), p.families.len())?;
    let mut instructions = vector::<AuditedValue>(m, &a, instruction_count)?;
    let mut explanations = vector(m, &a, instruction_count)?;
    let mut edges = vector(m, &a, p.rules.len())?;
    let mut rule_order = vector(m, &a, p.rules.len())?;
    let mut producers = vector::<(&Rule, usize)>(m, &a, p.rules.len())?;
    let mut rules = vector(m, &a, p.rules.len())?;
    for row in p.rules.iter().enumerate() {
        rules.push(row);
    }
    for i in 1..rules.len() {
        let mut j = i;
        while j > 0 {
            m.visit("rule.phase_sort")?;
            let left = rules[j - 1];
            let right = rules[j];
            let lp = if shape::same_text(m, &left.1.phase, "base")? {
                0u8
            } else {
                1
            };
            let rp = if shape::same_text(m, &right.1.phase, "base")? {
                0u8
            } else {
                1
            };
            let phase = w::compare(m, lp, rp)?;
            let compare = if phase.is_eq() {
                let slot = order(m, &left.1.binding_slot, &right.1.binding_slot)?;
                if slot.is_eq() {
                    w::compare(m, left.0, right.0)?
                } else {
                    slot
                }
            } else {
                phase
            };
            if !compare.is_gt() {
                break;
            }
            m.visit("plan.rule_sort_move")?;
            rules.swap(j - 1, j);
            j -= 1;
        }
    }
    let constants_raw: &[u8] = if plan_schema == "ledger-workflow-compiled-plan/8" {
        br#"{"rule":"evaluate_rule","family":"evaluate_family","family_primitive":"family","occurrence":"occurrence","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits":"workflow-local-operator-limits/1","worst":{"fields":"1088","nodes":"256","hops":"16","actions":"128","explanations":"256"}}"#
    } else if plan_schema == "ledger-workflow-compiled-plan/7" {
        br#"{"rule":"evaluate_rule","family":"evaluate_family","family_primitive":"family","occurrence":"occurrence","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits":"workflow-local-limits/1","worst":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576"}}"#
    } else {
        br#"{"rule":"evaluate_rule","family":"evaluate_family","family_primitive":"family","occurrence":"occurrence","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits":"workflow-limits/1","worst":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"}}"#
    };
    let constants =
        m.parse_owned_representation(&a, constants_raw, RepresentationKind::SourceDefinition)?;
    let empty = a.array_from_values(&[], m)?;
    let activities = shape::array(m, product.canonical_value(), "activities")?;
    for &(index, rule) in rules.iter() {
        m.visit("compile.rule_instruction")?;
        let ord = ordinal(m, &a, instructions.len())?;
        let mut activity = None;
        for candidate in activities {
            if text_same(m, candidate, "name", &rule.activity)? {
                activity = Some(candidate);
                break;
            }
        }
        let sr = shape::field(m, activity.ok_or_else(|| fail("activity"))?, "fact_schema")?;
        let mut field_names = vector(m, &a, 1)?;
        let mut basis = vector(m, &a, 1)?;
        let primitive = match &rule.formula {
            Formula::Fixed { .. } => "fixed",
            Formula::Unit { quantity_field, .. } => {
                field_names.push(a.string_value(quantity_field, m)?);
                "unit"
            }
            Formula::Discount { basis: target, .. } => {
                let mut previous = None;
                for (old, position) in producers.iter().rev() {
                    m.visit("plan.prior_basis_producer")?;
                    if shape::same_text(m, &old.activity, &rule.activity)?
                        && shape::same_text(m, &old.binding_slot, &rule.binding_slot)?
                        && shape::same_text(m, &old.component_key, &target.component_key)?
                    {
                        previous = Some(*position);
                        break;
                    }
                }
                let prior = previous.ok_or_else(|| fail("missing prior basis producer"))?;
                basis.push(
                    a.clone_value(shape::field(m, instructions[prior].value(), "ordinal")?, m)?,
                );
                "discount"
            }
            Formula::AppFinalCost { .. } => {
                let mut declaration = None;
                for row in p
                    .app_delayed_costs
                    .as_ref()
                    .ok_or_else(|| fail("app declarations"))?
                {
                    if shape::same_text(m, &row.component_key, &rule.component_key)?
                        && shape::same_text(m, &row.activity, &rule.activity)?
                    {
                        declaration = Some(row);
                        break;
                    }
                }
                field_names.push(
                    a.string_value(
                        &declaration
                            .ok_or_else(|| {
                                fail("app cost declaration missing from compiled facts")
                            })?
                            .usage_field,
                        m,
                    )?,
                );
                "app_final_cost"
            }
            Formula::FinalCost { .. } => "final_cost",
            Formula::SupplierFinalCost { .. } => "supplier_final_cost",
        };
        let mut indices = vector(m, &a, slots.len())?;
        for slot in &slots {
            if !field_same(m, slot.value(), "schema_ref", sr)? {
                continue;
            }
            for name in &field_names {
                if field_same(m, &slot.value()["field"], "name", name.value())? {
                    indices.push(shape::field(m, slot.value(), "index")?);
                    break;
                }
            }
        }
        let indices = a.array_from_values(&indices, m)?;
        let binding_indices = if let super::model::Predicate::Eq { field, .. } = &rule.when {
            let mut indices = vector(m, &a, 1)?;
            let context = reference(m, &a, &p.context_schema_ref)?;
            for slot in &slots {
                if field_same(m, slot.value(), "schema_ref", context.value())?
                    && text_same(m, &slot.value()["field"], "name", &field.binding_fact)?
                {
                    indices.push(shape::field(m, slot.value(), "index")?);
                }
            }
            Some(a.array_from_values(&indices, m)?)
        } else {
            None
        };
        let bases = array(m, &a, &basis)?;
        let typed_rules = shape::array(m, policy, "rules")?;
        let rule_body = typed_rules
            .get(index)
            .ok_or_else(|| fail("normalized rule association"))?;
        let instruction = a.object_from_fields(
            &[
                ("opcode", &constants.value()["rule"]),
                ("ordinal", ord.value()),
                ("policy_ref", root_value.value()),
                ("rule", rule_body),
                ("fact_slots", indices.value()),
                (
                    "binding_fact_slots",
                    binding_indices
                        .as_ref()
                        .map_or(empty.value(), |v| v.value()),
                ),
                ("basis_instructions", bases.value()),
            ],
            m,
        )?;
        let position = instructions.len();
        instructions.push(instruction);
        for b in &basis {
            edges.push(
                a.object_from_fields(&[("producer", b.value()), ("consumer", ord.value())], m)?,
            );
        }
        producers.push((rule, position));
        let rule_id = a.string_value(&rule.rule_id, m)?;
        rule_order.push(a.object_from_fields(
            &[
                ("policy_ref", root_value.value()),
                ("rule_id", rule_id.value()),
            ],
            m,
        )?);
        let names = array(m, &a, &field_names)?;
        let primitive = a.string_value(primitive, m)?;
        explanations.push(a.object_from_fields(
            &[
                ("ordinal", ord.value()),
                ("policy_ref", root_value.value()),
                ("rule_or_family", rule_id.value()),
                ("fields", names.value()),
                ("primitive", primitive.value()),
            ],
            m,
        )?);
    }
    let mut paths = vector(m, &a, p.families.len())?;
    for (index, family) in p.families.iter().enumerate() {
        m.visit("compile.family_instruction")?;
        let ord = ordinal(m, &a, instructions.len())?;
        let mut activity = None;
        for candidate in activities {
            if text_same(m, candidate, "name", &family.trigger_activity)? {
                activity = Some(candidate);
                break;
            }
        }
        let sr = shape::field(
            m,
            activity.ok_or_else(|| fail("family activity"))?,
            "fact_schema",
        )?;
        let mut names = vector(m, &a, slots.len())?;
        for slot in &slots {
            if !field_same(m, slot.value(), "schema_ref", sr)? {
                continue;
            }
            let field = shape::field(m, slot.value(), "field")?;
            let code = text_same(m, field, "name", &family.code_field)?;
            let occurrence = if code {
                false
            } else {
                shape::same(
                    m,
                    &field["provenance"]["evidence_kind"],
                    &constants.value()["occurrence"],
                )?
            };
            if code || occurrence {
                names.push(shape::field(m, field, "name")?);
            }
        }
        let mut indices = vector(m, &a, slots.len())?;
        for slot in &slots {
            if field_same(m, slot.value(), "schema_ref", sr)? {
                for name in &names {
                    if shape::same(m, name, &slot.value()["field"]["name"])? {
                        indices.push(shape::field(m, slot.value(), "index")?);
                        break;
                    }
                }
            }
        }
        let indices = a.array_from_values(&indices, m)?;
        let names = a.array_from_values(&names, m)?;
        let typed = shape::array(m, policy, "families")?;
        let body = typed
            .get(index)
            .ok_or_else(|| fail("normalized family association"))?;
        let target = a.string_value(&family.selector.component_key, m)?;
        instructions.push(a.object_from_fields(
            &[
                ("opcode", &constants.value()["family"]),
                ("ordinal", ord.value()),
                ("policy_ref", root_value.value()),
                ("family", body),
                ("fact_slots", indices.value()),
                ("target_component_key", target.value()),
            ],
            m,
        )?);
        let family_id = a.string_value(&family.family_key, m)?;
        explanations.push(a.object_from_fields(
            &[
                ("ordinal", ord.value()),
                ("policy_ref", root_value.value()),
                ("rule_or_family", family_id.value()),
                ("fields", names.value()),
                ("primitive", &constants.value()["family_primitive"]),
            ],
            m,
        )?);
        paths.push(a.clone_value(shape::field(m, body, "selector")?, m)?);
    }
    let mut activity_table = vector(m, &a, activities.len())?;
    for activity in activities {
        let fields = [
            ("activity", shape::field(m, activity, "name")?),
            ("primitive", shape::field(m, activity, "primitive")?),
            ("fact_schema_ref", shape::field(m, activity, "fact_schema")?),
            ("source_slot", shape::field(m, activity, "source_slot")?),
        ];
        let value = if let Some(funding) = activity.get("funding_leg") {
            a.object_from_fields(
                &[
                    ("activity", fields[0].1),
                    ("primitive", fields[1].1),
                    ("fact_schema_ref", fields[2].1),
                    ("source_slot", fields[3].1),
                    ("funding_leg", funding),
                ],
                m,
            )?
        } else {
            a.object_from_fields(&fields, m)?
        };
        activity_table.push(value);
    }
    let activity_table = array(m, &a, &activity_table)?;
    let fact_slots = array(m, &a, &slots)?;
    let paths = set(m, &a, &mut paths)?;
    let order = array(m, &a, &rule_order)?;
    let instructions = array(m, &a, &instructions)?;
    let edges = array(m, &a, &edges)?;
    let explanations = array(m, &a, &explanations)?;
    let schema = a.string_value(plan_schema, m)?;
    let semantics = a.string_value(semantics, m)?;
    let body = a.object_from_fields(
        &[
            ("schema", schema.value()),
            ("compiler_ref", shape::field(m, policy, "compiler_ref")?),
            ("compiler_semantics", semantics.value()),
            ("evaluator", &constants.value()["evaluator"]),
            ("grammar", &constants.value()["grammar"]),
            ("limits_profile", &constants.value()["limits"]),
            ("source_refs", source_refs.value()),
            ("activity_table", activity_table.value()),
            ("fact_slots", fact_slots.value()),
            ("link_paths", paths.value()),
            (
                "component_slots",
                shape::field(m, template.canonical_value(), "components")?,
            ),
            ("rule_order", order.value()),
            ("typed_instructions", instructions.value()),
            ("dependency_edges", edges.value()),
            ("worst_case", &constants.value()["worst"]),
            ("explanation_map", explanations.value()),
        ],
        m,
    )?;
    Ok(Output {
        body,
        fields: field_count,
    })
}
