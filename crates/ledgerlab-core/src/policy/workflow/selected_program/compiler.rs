//! Source-derived selected instructions. This module never consumes oracle output.
use super::super::{
    DefinitionRef, FactSchema, Product, SlotTemplate,
    transition_constructor::{CoreMeter, arithmetic, codec},
};
use super::{
    SelectedCompilerSupport,
    model::{Effect, Formula, Policy},
};
use crate::{Error, Result, canonical};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn err(detail: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", detail)
}
pub struct SelectedSourceInput {
    pub documents: Vec<Vec<u8>>,
    pub root_policy_ref: DefinitionRef,
}
#[derive(Clone, Debug)]
pub struct SelectedDocument {
    reference: DefinitionRef,
    body: Value,
    bytes: Vec<u8>,
}
impl SelectedDocument {
    pub fn reference(&self) -> &DefinitionRef {
        &self.reference
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
}
#[derive(Clone, Debug)]
pub struct StaticAnalysis {
    pub fields: usize,
    pub rules: usize,
    pub families: usize,
    pub maximum_path: usize,
    pub compile_work: u64,
}
pub struct SelectedProgram {
    root: DefinitionRef,
    sources: Vec<SelectedDocument>,
    plan: SelectedDocument,
    policy: Policy,
    schemas: Vec<(DefinitionRef, FactSchema)>,
    analysis: StaticAnalysis,
}
impl SelectedProgram {
    pub fn root_policy_ref(&self) -> &DefinitionRef {
        &self.root
    }
    pub fn compiler_descriptor_ref(&self) -> &DefinitionRef {
        &self.policy.compiler_ref
    }
    pub fn product_ref(&self) -> &DefinitionRef {
        &self.policy.product_ref
    }
    pub fn template_ref(&self) -> &DefinitionRef {
        &self.policy.slot_template_ref
    }
    pub fn context_schema_ref(&self) -> &DefinitionRef {
        &self.policy.context_schema_ref
    }
    pub fn source_documents(&self) -> &[SelectedDocument] {
        &self.sources
    }
    pub fn compiled_plan(&self) -> &SelectedDocument {
        &self.plan
    }
    pub fn static_analysis(&self) -> &StaticAnalysis {
        &self.analysis
    }
    pub(in crate::policy::workflow) fn fact_schemas(&self) -> &[(DefinitionRef, FactSchema)] {
        &self.schemas
    }
    pub(super) fn policy(&self) -> &Policy {
        &self.policy
    }
}
fn doc(m: &mut CoreMeter, body: Value) -> Result<SelectedDocument> {
    let schema = body["schema"]
        .as_str()
        .ok_or_else(|| err("source schema"))?;
    let kind = schema
        .strip_prefix("ledger-")
        .and_then(|s| s.strip_suffix("/1"))
        .ok_or_else(|| err("source schema version"))?;
    let bytes = codec::encode(m, &body, 262144)?;
    m.visit("source.hash")?;
    m.read_bytes(bytes.len())?;
    let framed = codec::encode(m, &json!([schema, body]), 4194304)?;
    m.hash_bytes(b"bean-counter/workflow-document/1\0".len() + framed.len())?;
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/workflow-document/1\0");
    hash.update(&framed);
    let digest = format!("sha256:{}", canonical::hex(&hash.finalize()));
    let reference = DefinitionRef::new(kind, &format!("wdoc_{}", &digest[7..]), &digest)?;
    Ok(SelectedDocument {
        reference,
        body,
        bytes,
    })
}
fn lookup<'a>(
    m: &mut CoreMeter,
    docs: &'a [SelectedDocument],
    r: &DefinitionRef,
) -> Result<&'a SelectedDocument> {
    for d in docs {
        if m.check("source.ref_compare", || d.reference == *r)? {
            return Ok(d);
        }
    }
    Err(err("missing exact source"))
}
fn set(m: &mut CoreMeter, values: Vec<Value>) -> Result<Vec<Value>> {
    let mut out = Vec::<(Vec<u8>, Value)>::new();
    for v in values {
        let b = codec::encode(m, &v, 262144)?;
        let mut position = out.len();
        for (i, (old, _)) in out.iter().enumerate() {
            m.visit("set.compare")?;
            match b.cmp(old) {
                std::cmp::Ordering::Equal => return Err(err("duplicate set member")),
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                _ => {}
            }
        }
        out.insert(position, (b, v));
    }
    Ok(out.into_iter().map(|(_, v)| v).collect())
}
/// Fresh compilation dispatches an exact binary-owned recipe, never arbitrary
/// descriptor contents. Historical restore below dispatches the same retained
/// v1 implementation directly and compares its complete source-derived plan.
pub fn compile_selected_program(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
) -> Result<SelectedProgram> {
    m.enter_compile()?;
    require_retail_selected_v1(support, m)?;
    compile_retail_selected_v1(input, support, m)
}
fn require_retail_selected_v1(support: &SelectedCompilerSupport, m: &mut CoreMeter) -> Result<()> {
    let actual = support.reference().id().as_bytes();
    let expected = b"wdoc_f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b";
    m.visit("compile.recipe_dispatch")?;
    if !crate::validation_work::compare(m, actual.len(), expected.len())?.is_eq() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "selected compiler recipe unavailable",
        ));
    }
    for (a, b) in actual.iter().zip(expected) {
        m.scan_bytes(2)?;
        if a != b {
            return Err(Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "selected compiler recipe unavailable",
            ));
        }
    }
    Ok(())
}
pub fn restore_selected_program(
    input: SelectedSourceInput,
    saved_descriptor: &[u8],
    saved_plan: &[u8],
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let mut compiler = session.meter_mut().original_compile_meter()?;
    compiler.enter_compile()?;
    let support = SelectedCompilerSupport::restore_saved(saved_descriptor, &mut compiler)?;
    require_retail_selected_v1(&support, &mut compiler)?;
    let program = compile_retail_selected_v1(input, &support, &mut compiler)?;
    let _compiler_usage = session.meter_mut().finish_original_compile(compiler)?;
    let work = session.work();
    let actual = program.compiled_plan().canonical_bytes();
    work.visit(crate::validation_work::ValidationVisit::MemberCompare)?;
    if !crate::validation_work::compare(work, actual.len(), saved_plan.len())?.is_eq() {
        return Err(Error::new(
            "INTEGRITY_FAILURE",
            "saved selected plan length",
        ));
    }
    for (a, b) in actual.iter().zip(saved_plan) {
        work.scan_bytes(2)?;
        if a != b {
            return Err(Error::new(
                "INTEGRITY_FAILURE",
                "saved selected plan differs from original source computation",
            ));
        }
    }
    Ok(program)
}
fn compile_retail_selected_v1(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
) -> Result<SelectedProgram> {
    m.visit("compile.input")?;
    if !m.check("compile.count", || {
        !input.documents.is_empty() && input.documents.len() <= 128
    })? {
        return Err(err("source count"));
    }
    let mut docs = Vec::new();
    let mut identities = BTreeSet::new();
    let mut semantic = BTreeSet::new();
    let mut schemas = Vec::new();
    let mut product_raw = None;
    let mut template_raw = None;
    let mut policy_raw = None;
    for raw in input.documents {
        let body = codec::parse(m, &raw, 262144)?;
        let schema = body["schema"]
            .as_str()
            .ok_or_else(|| err("source schema"))?;
        m.visit("source.kind")?;
        match schema {
            "ledger-workflow-fact-schema/1" => {
                for field in body["fields"]
                    .as_array()
                    .ok_or_else(|| err("fact fields"))?
                {
                    if !m.check("source.fact_type", || {
                        ["enum", "integer", "timestamp"]
                            .contains(&field["type"]["kind"].as_str().unwrap_or(""))
                    })? {
                        return Err(err("selected fact type"));
                    }
                }

                let parsed = FactSchema::from_value_with_work(body, m)?;
                let normalized = parsed.canonical_value().clone();
                schemas.push(parsed);
                docs.push(doc(m, normalized)?);
            }
            "ledger-workflow-product/1" => {
                for unit in body["units"].as_array().ok_or_else(|| err("unit array"))? {
                    if !m.check("source.unit_terminal", || {
                        unit.get("normalization").is_none()
                    })? {
                        return Err(err("selected terminal unit"));
                    }
                }

                if product_raw.replace(body).is_some() {
                    return Err(err("multiple products"));
                }
            }
            "ledger-workflow-slot-template/1" => {
                if template_raw.replace(body).is_some() {
                    return Err(err("multiple templates"));
                }
            }
            "ledger-workflow-policy/1" => {
                if policy_raw.replace(body).is_some() {
                    return Err(err("multiple policies"));
                }
            }
            "ledger-workflow-compiler-descriptor/1" => {
                if !m.check("source.support", || body == *support.canonical_value())? {
                    return Err(Error::new(
                        "UNSUPPORTED_COMPILER_DESCRIPTOR",
                        "code owned selected support",
                    ));
                }
                docs.push(doc(m, body)?);
            }
            _ => return Err(err("unsupported source family")),
        }
    }
    let product = Product::from_value_with_work(
        product_raw.ok_or_else(|| err("product missing"))?,
        &schemas,
        m,
    )?;
    let template = SlotTemplate::from_value_with_work(
        template_raw.ok_or_else(|| err("template missing"))?,
        None,
        m,
    )?;
    docs.push(doc(m, product.canonical_value().clone())?);
    docs.push(doc(m, template.canonical_value().clone())?);
    let body = policy_raw.ok_or_else(|| err("policy missing"))?;
    for rule in body["rules"].as_array().ok_or_else(|| err("rule array"))? {
        m.visit("predicate.closed")?;
        let predicate = rule["when"]
            .as_object()
            .ok_or_else(|| err("predicate object"))?;
        if !m.check("predicate.unit_keys", || {
            predicate.len() == 1 && predicate.contains_key("op")
        })? {
            return Err(err("closed true predicate"));
        }
    }
    for family in body["families"]
        .as_array()
        .ok_or_else(|| err("family array"))?
    {
        for code in family["codes"]
            .as_array()
            .ok_or_else(|| err("family codes"))?
        {
            m.visit("effect.closed")?;
            let effect = code["effect"]
                .as_object()
                .ok_or_else(|| err("effect object"))?;
            if m.check("effect.none", || {
                effect.get("kind").is_some_and(|v| v == "none")
            })? && !m.check("effect.none_keys", || effect.len() == 1)?
            {
                return Err(err("closed none effect"));
            }
        }
    }
    let mut policy: Policy =
        serde_json::from_value(body).map_err(|_| err("closed selected policy"))?;
    if !m.check("policy.profile", || {
        policy.schema == "ledger-workflow-policy/1"
            && policy.currency == "USD"
            && policy.scale == 18
            && policy.rounding == "nearest_ties_away"
            && policy.mode == "component_workflow"
            && policy.previous_policy_ref.is_none()
            && policy.stages.is_empty()
            && policy.delayed_costs.is_empty()
    })? {
        return Err(err("selected policy profile"));
    }
    super::normalize::policy(m, &mut policy)?;
    arithmetic::integer(m, &policy.version, false)?;
    super::validate::policy(m, &policy, &product, &template, &schemas)?;
    for rule in &mut policy.rules {
        m.visit("rule.validate")?;
        let activity = product
            .activity_with_work(&rule.activity, m)?
            .ok_or_else(|| err("rule activity"))?;
        template.component_named_with_work(&rule.component_key, &rule.component_name, m)?;
        if !m.check("rule.slot", || {
            policy.binding_slots.iter().any(|s| {
                s.name == rule.binding_slot
                    && s.book == "retail"
                    && s.allowed_activities.contains(&rule.activity)
            })
        })? {
            return Err(err("rule binding slot"));
        }
        match &mut rule.formula {
            Formula::Fixed { amount } => {
                arithmetic::decimal(m, amount)?;
            }
            Formula::Unit {
                quantity_field,
                unit,
                rate,
            } => {
                arithmetic::decimal(m, rate)?;
                let mut found_schema = None;
                for schema in &schemas {
                    let id = schema.document_id_with_work(m)?;
                    if m.check("unit.schema_compare", || id == activity.fact_schema_id())? {
                        found_schema = Some(schema);
                        break;
                    }
                }
                let sr = found_schema.ok_or_else(|| err("quantity schema"))?;
                if sr.quantity_unit_with_work(quantity_field, m)? != unit {
                    return Err(err("quantity unit"));
                }
                if product
                    .unit_with_work(unit, m)?
                    .ok_or_else(|| err("unit missing"))?
                    .normalization()
                    .is_some()
                {
                    return Err(err("terminal unit only"));
                }
            }
            Formula::Discount {
                basis,
                mode,
                amount,
            } => {
                if !m.check("discount.profile", || {
                    mode == "additive" && basis.view == "booked_base" && amount.kind == "percent"
                })? {
                    return Err(err("selected discount"));
                }
                arithmetic::decimal(m, &amount.percent)?;
            }
        }
    }
    for family in &policy.families {
        m.visit("family.validate")?;
        template.family_named_with_work(&family.family_key, &family.family_name, m)?;
        product.validate_selector_path_with_work(
            &family.trigger_activity,
            &family.selector.path,
            &family.selector.target_activity,
            m,
        )?;
        if !m.check("family.path_bound", || {
            !family.selector.path.is_empty() && family.selector.path.len() <= 4
        })? {
            return Err(err("selected monetary path"));
        }
        if !m.check("family.window_anchor", || {
            family.window.anchor == "work_accepted_at" && family.window.anchor_path.is_empty()
        })? {
            return Err(err("selected window anchor"));
        }
        for value in [
            &family.window.start_offset_us,
            &family.window.occurrence_us,
            &family.window.receipt_grace_us,
            &family.window.acceptance_grace_us,
            &family.window.activation_deadline_us,
            &family.window.correction_us,
        ] {
            arithmetic::integer(m, value, false)?;
        }
        for code in &family.codes {
            m.visit("family.code")?;
            match &code.effect {
                Effect::None => {}
                Effect::Premium { amount } | Effect::Discount { amount } => {
                    if !m.check("family.percent", || amount.kind == "percent")? {
                        return Err(err("family percent"));
                    }
                    arithmetic::decimal(m, &amount.percent)?;
                }
            }
        }
    }
    for i in 1..policy.families.len() {
        let mut j = i;
        while j > 0 {
            m.visit("family.sort_compare")?;
            if policy.families[j - 1].family_key <= policy.families[j].family_key {
                break;
            }
            policy.families.swap(j - 1, j);
            j -= 1;
        }
    }
    for i in 1..policy.binding_slots.len() {
        let mut j = i;
        while j > 0 {
            m.visit("binding.sort_compare")?;
            if policy.binding_slots[j - 1].name <= policy.binding_slots[j].name {
                break;
            }
            policy.binding_slots.swap(j - 1, j);
            j -= 1;
        }
    }
    let policy_body = serde_json::to_value(&policy).map_err(|_| err("policy encoding"))?;
    docs.push(doc(m, policy_body)?);
    let root = lookup(m, &docs, &input.root_policy_ref)?.reference.clone();
    if !m.check("policy.root", || root.kind() == "workflow-policy")? {
        return Err(err("root policy kind"));
    }
    lookup(m, &docs, &policy.product_ref)?;
    lookup(m, &docs, &policy.slot_template_ref)?;
    lookup(m, &docs, &policy.context_schema_ref)?;
    lookup(m, &docs, &policy.compiler_ref)?;
    for d in &docs {
        m.visit("source.identity")?;
        if !identities.insert(d.reference.id().to_owned()) {
            return Err(err("duplicate source identity"));
        }
        let key = (
            d.reference.kind().to_owned(),
            d.body
                .get("schema_id")
                .or_else(|| d.body.get("product_id"))
                .or_else(|| d.body.get("template_id"))
                .or_else(|| d.body.get("policy_id"))
                .cloned(),
            d.body.get("version").cloned(),
        );
        if !semantic.insert(format!("{:?}", key)) {
            return Err(err("duplicate semantic definition"));
        }
    }
    let total = docs.iter().try_fold(0usize, |n, d| {
        n.checked_add(d.bytes.len())
            .ok_or_else(|| err("source bytes"))
    })?;
    if total > 1048576 {
        return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "source closure"));
    }
    let mut reachable = vec![
        json!(root),
        json!(policy.product_ref),
        json!(policy.slot_template_ref),
        json!(policy.compiler_ref),
        json!(policy.context_schema_ref),
    ];
    for r in product.canonical_value()["fact_schemas"]
        .as_array()
        .ok_or_else(|| err("product schema refs"))?
    {
        m.visit("closure.ref")?;
        let mut exists = false;
        for prior in &reachable {
            if m.check("closure.ref_compare", || prior == r)? {
                exists = true;
                break;
            }
        }
        if !exists {
            reachable.push(r.clone());
        }
    }
    if !m.check("closure.count", || reachable.len() == docs.len())? {
        return Err(err("unused source document"));
    }
    for d in &docs {
        let value = json!(d.reference);
        let mut exists = false;
        for r in &reachable {
            if m.check("closure.member", || r == &value)? {
                exists = true;
                break;
            }
        }
        if !exists {
            return Err(err("unreachable source"));
        }
    }
    let source_refs = set(m, docs.iter().map(|d| json!(d.reference)).collect())?;
    let mut schema_refs = product.canonical_value()["fact_schemas"]
        .as_array()
        .ok_or_else(|| err("fact refs"))?
        .clone();
    let cr = json!(policy.context_schema_ref);
    if !schema_refs.contains(&cr) {
        schema_refs.push(cr);
    }
    schema_refs.sort_by(|a, b| a["digest"].as_str().cmp(&b["digest"].as_str()));
    let mut slots = Vec::new();
    for r in schema_refs {
        let reference: DefinitionRef =
            serde_json::from_value(r.clone()).map_err(|_| err("schema ref"))?;
        let d = lookup(m, &docs, &reference)?;
        let mut fields = d.body["fields"]
            .as_array()
            .ok_or_else(|| err("fields"))?
            .clone();
        fields.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        for field in fields {
            m.visit("plan.fact_slot")?;
            slots.push(json!({"index":slots.len().to_string(),"schema_ref":r,"field":field}));
        }
    }
    let mut instructions = Vec::new();
    let mut edges = Vec::new();
    let mut explanations = Vec::new();
    let mut order = Vec::new();
    let mut producers = BTreeMap::<(String, String, String), String>::new();
    let mut ordered_rules: Vec<(usize, &super::model::Rule)> =
        policy.rules.iter().enumerate().collect();
    for i in 1..ordered_rules.len() {
        let mut j = i;
        while j > 0 {
            m.visit("rule.phase_sort")?;
            let key = |x: &(usize, &super::model::Rule)| {
                (
                    if x.1.phase == "base" { 0 } else { 1 },
                    x.1.binding_slot.clone(),
                    x.0,
                )
            };
            if key(&ordered_rules[j - 1]) <= key(&ordered_rules[j]) {
                break;
            }
            ordered_rules.swap(j - 1, j);
            j -= 1;
        }
    }
    for (_, rule) in ordered_rules {
        m.visit("compile.rule_instruction")?;
        let ordinal = instructions.len().to_string();
        let sr = product.canonical_value()["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == rule.activity)
            .ok_or_else(|| err("activity"))?["fact_schema"]
            .clone();
        let (fields, kind, basis) = match &rule.formula {
            Formula::Fixed { .. } => (vec![], "fixed", vec![]),
            Formula::Unit { quantity_field, .. } => (vec![quantity_field.clone()], "unit", vec![]),
            Formula::Discount { basis, .. } => (
                vec![],
                "discount",
                vec![
                    producers
                        .get(&(
                            rule.activity.clone(),
                            rule.binding_slot.clone(),
                            basis.component_key.clone(),
                        ))
                        .ok_or_else(|| err("missing prior basis producer"))?
                        .clone(),
                ],
            ),
        };
        let indices: Vec<_> = slots
            .iter()
            .filter(|s| s["schema_ref"] == sr && fields.iter().any(|f| s["field"]["name"] == *f))
            .map(|s| s["index"].clone())
            .collect();
        let body = serde_json::to_value(rule).map_err(|_| err("rule encoding"))?;
        instructions.push(json!({"opcode":"evaluate_rule","ordinal":ordinal,"policy_ref":root,"rule":body,"fact_slots":indices,"binding_fact_slots":[],"basis_instructions":basis}));
        for b in basis {
            edges.push(json!({"producer":b,"consumer":ordinal}));
        }
        producers.insert(
            (
                rule.activity.clone(),
                rule.binding_slot.clone(),
                rule.component_key.clone(),
            ),
            ordinal.clone(),
        );
        order.push(json!({"policy_ref":root,"rule_id":rule.rule_id}));
        explanations.push(json!({"ordinal":ordinal,"policy_ref":root,"rule_or_family":rule.rule_id,"fields":fields,"primitive":kind}));
    }
    for family in &policy.families {
        m.visit("compile.family_instruction")?;
        let ordinal = instructions.len().to_string();
        let sr = product.canonical_value()["activities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == family.trigger_activity)
            .ok_or_else(|| err("family activity"))?["fact_schema"]
            .clone();
        let fields: Vec<_> = slots
            .iter()
            .filter(|s| {
                s["schema_ref"] == sr
                    && (s["field"]["name"] == family.code_field
                        || s["field"]["provenance"]["evidence_kind"] == "occurrence")
            })
            .map(|s| s["field"]["name"].clone())
            .collect();
        let indices: Vec<_> = slots
            .iter()
            .filter(|s| s["schema_ref"] == sr && fields.contains(&s["field"]["name"]))
            .map(|s| s["index"].clone())
            .collect();
        instructions.push(json!({"opcode":"evaluate_family","ordinal":ordinal,"policy_ref":root,"family":family,"fact_slots":indices,"target_component_key":family.selector.component_key}));
        explanations.push(json!({"ordinal":ordinal,"policy_ref":root,"rule_or_family":family.family_key,"fields":fields,"primitive":"family"}));
    }
    let activities:Vec<_>=product.canonical_value()["activities"].as_array().unwrap().iter().map(|a|{let mut v=json!({"activity":a["name"],"primitive":a["primitive"],"fact_schema_ref":a["fact_schema"],"source_slot":a["source_slot"]});if let Some(f)=a.get("funding_leg"){v["funding_leg"]=f.clone();}v}).collect();
    let plan = json!({"schema":"ledger-workflow-compiled-plan/1","compiler_ref":policy.compiler_ref,"compiler_semantics":"ledger-policy-compiler/1","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits_profile":"workflow-limits/1","source_refs":source_refs,"activity_table":activities,"fact_slots":slots,"link_paths":set(m,policy.families.iter().map(|f|json!(f.selector)).collect())?,"component_slots":template.canonical_value()["components"],"rule_order":order,"typed_instructions":instructions,"dependency_edges":edges,"worst_case":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"},"explanation_map":explanations});
    let bytes = codec::encode(m, &plan, 4194304)?;
    m.visit("plan.hash")?;
    m.read_bytes(bytes.len())?;
    let mut hash = Sha256::new();
    m.hash_bytes(b"bean-counter/workflow-plan/1\0".len() + bytes.len())?;
    hash.update(b"bean-counter/workflow-plan/1\0");
    hash.update(&bytes);
    let digest = format!("sha256:{}", canonical::hex(&hash.finalize()));
    let reference = DefinitionRef::new(
        "workflow-compiled-plan",
        &format!("wplan_{}", &digest[7..]),
        &digest,
    )?;
    let mut analysis = StaticAnalysis {
        fields: slots.len(),
        rules: policy.rules.len(),
        families: policy.families.len(),
        maximum_path: policy
            .families
            .iter()
            .map(|f| f.selector.path.len())
            .max()
            .unwrap_or(0),
        compile_work: m.usage().semantic_work,
    };
    let mut prepared_schemas = Vec::new();
    for schema in schemas {
        let mut reference = None;
        for d in &docs {
            if m.check("compile.prepared_schema", || {
                d.body == *schema.canonical_value()
            })? {
                reference = Some(d.reference.clone());
                break;
            }
        }
        prepared_schemas.push((
            reference.ok_or_else(|| err("prepared schema missing"))?,
            schema,
        ));
    }
    analysis.compile_work = m.usage().semantic_work;
    Ok(SelectedProgram {
        root,
        sources: docs,
        plan: SelectedDocument {
            reference,
            body: plan,
            bytes,
        },
        policy,
        schemas: prepared_schemas,
        analysis,
    })
}
