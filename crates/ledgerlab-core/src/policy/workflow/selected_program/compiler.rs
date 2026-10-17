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
    root_position:usize,
    sources: Vec<SelectedDocument>,
    plan: SelectedDocument,
    policy: Policy,
    schemas: Vec<(DefinitionRef, FactSchema)>,
    analysis: StaticAnalysis,
    family_close_recipe: bool,
    owner_local_cost_recipe:bool,
    app_owned_cost_recipe:bool,
    app_cost_causal_recipe:bool,
    supplier_scoped_consent_recipe:bool,
    execution_registry:Option<&'static str>,
}
impl SelectedProgram {
    /// Binary recipe identity, not current-support freshness or authority.
    pub fn uses_family_close_recipe(&self)->bool {self.family_close_recipe}
    pub fn uses_owner_local_cost_recipe(&self)->bool {self.owner_local_cost_recipe}
    pub fn uses_app_owned_cost_recipe(&self)->bool {self.app_owned_cost_recipe}
    pub fn uses_app_cost_causal_recipe(&self)->bool {self.app_cost_causal_recipe}
    pub fn uses_supplier_scoped_consent_recipe(&self)->bool {self.supplier_scoped_consent_recipe}
    pub fn execution_registry(&self)->Option<&'static str>{self.execution_registry}
    pub fn root_policy_ref(&self) -> &DefinitionRef {
        &self.root
    }
    pub fn original_policy_value(&self)->&Value {self.sources[self.root_position].canonical_value()}
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
    /// Borrow the actual compiled context schema under the caller's SAME
    /// semantic work owner. No schema is recompiled or cloned, and this view
    /// does not establish publication/origin custody.
    pub fn context_schema_with_work(&self, work:&mut dyn crate::validation_work::ValidationWork)->Result<&FactSchema> {
        let target=&self.policy.context_schema_ref;
        for (reference,schema) in &self.schemas {
            work.visit(crate::validation_work::ValidationVisit::MemberCompare)?;
            for (left,right) in [(reference.kind(),target.kind()),(reference.id(),target.id()),(reference.digest(),target.digest())]{
                let bytes=crate::validation_work::usize_add(work,left.len(),right.len())?;
                work.scan_bytes(bytes)?;
            }
            if reference==target{return Ok(schema);}
        }
        Err(err("compiled context schema missing"))
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
        .and_then(|s| s.strip_suffix("/1").or_else(||s.strip_suffix("/2")).or_else(||s.strip_suffix("/3")).or_else(||s.strip_suffix("/4")).or_else(||s.strip_suffix("/5")).or_else(||s.strip_suffix("/6")))
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
    let recipe=require_selected_recipe(support,m)?;
    compile_selected_recipe(input,support,m,recipe)
}
#[derive(Clone,Copy)]
enum SelectedRecipe { RetailDraft1,FamilyClose1,OwnerLocalCost1,SupplierScopedConsent1,AppOwnedCost1,AppCostCausal1 }
impl SelectedRecipe {
    fn policy_schema(self)->&'static str {match self{Self::RetailDraft1=>"ledger-workflow-policy/1",Self::AppCostCausal1=>"ledger-workflow-policy/4",Self::AppOwnedCost1=>"ledger-workflow-policy/3",Self::FamilyClose1|Self::OwnerLocalCost1|Self::SupplierScopedConsent1=>"ledger-workflow-policy/2"}}
    fn plan_schema(self)->&'static str {match self{Self::RetailDraft1=>"ledger-workflow-compiled-plan/1",Self::FamilyClose1=>"ledger-workflow-compiled-plan/2",Self::OwnerLocalCost1=>"ledger-workflow-compiled-plan/3",Self::AppCostCausal1=>"ledger-workflow-compiled-plan/6",Self::AppOwnedCost1=>"ledger-workflow-compiled-plan/5",Self::SupplierScopedConsent1=>"ledger-workflow-compiled-plan/4"}}
    fn compiler_semantics(self)->&'static str {match self{Self::RetailDraft1=>"ledger-policy-compiler/1",Self::FamilyClose1=>"ledger-policy-compiler/family-close-1",Self::OwnerLocalCost1=>"ledger-policy-compiler/owner-local-cost-1",Self::AppCostCausal1=>"ledger-policy-compiler/app-cost-causal-1",Self::AppOwnedCost1=>"ledger-policy-compiler/app-owned-cost-1",Self::SupplierScopedConsent1=>"ledger-policy-compiler/supplier-scoped-consent-1"}}
}
pub(super) fn same_literal(m:&mut CoreMeter,a:&str,b:&str)->Result<bool>{
    m.visit("compile.recipe_text_identity")?;
    if !crate::validation_work::compare(m,a.len(),b.len())?.is_eq(){return Ok(false);}
    for (a,b) in a.bytes().zip(b.bytes()){m.scan_bytes(2)?;if a!=b{return Ok(false);}}
    Ok(true)
}
fn require_selected_recipe(support:&SelectedCompilerSupport,m:&mut CoreMeter)->Result<SelectedRecipe>{
    if support.canonical_value()["schema"]=="ledger-workflow-compiler-descriptor/6" {
      m.visit("compile.app_cost_causal_recipe_dispatch")?;
      if !same_literal(m,support.canonical_value()["compiler_semantics"].as_str().unwrap_or(""),"ledger-policy-compiler/app-cost-causal-1")? || !same_literal(m,support.canonical_value()["plan_schema"].as_str().unwrap_or(""),"ledger-workflow-compiled-plan/6")? {return Err(err("closed app cost causal compiler tuple"));}
      return Ok(SelectedRecipe::AppCostCausal1);
    }
    if support.canonical_value()["schema"]=="ledger-workflow-compiler-descriptor/5" {
        m.visit("compile.app_owned_cost_recipe_dispatch")?;
        if !same_literal(m,support.canonical_value()["compiler_semantics"].as_str().unwrap_or(""),"ledger-policy-compiler/app-owned-cost-1")?
            || !same_literal(m,support.canonical_value()["plan_schema"].as_str().unwrap_or(""),"ledger-workflow-compiled-plan/5")?{return Err(err("closed app-owned cost compiler tuple"));}
        return Ok(SelectedRecipe::AppOwnedCost1);
    }

    if support.canonical_value()["schema"]=="ledger-workflow-compiler-descriptor/4" {
        m.visit("compile.supplier_scoped_consent_recipe_dispatch")?;
        if !same_literal(m,support.canonical_value()["compiler_semantics"].as_str().unwrap_or(""),"ledger-policy-compiler/supplier-scoped-consent-1")?
            || !same_literal(m,support.canonical_value()["plan_schema"].as_str().unwrap_or(""),"ledger-workflow-compiled-plan/4")?{return Err(err("closed supplier-scoped consent compiler tuple"));}
        return Ok(SelectedRecipe::SupplierScopedConsent1);
    }
    // SelectedCompilerSupport has private fields and no arbitrary-document
    // constructor. New-recipe support can only come from its code-owned
    // catalog factory/exact saved verifier, not descriptor schema alone.
    if support.canonical_value()["schema"]=="ledger-workflow-compiler-descriptor/3" {
        m.visit("compile.owner_local_cost_recipe_dispatch")?;
        if !same_literal(m,support.canonical_value()["compiler_semantics"].as_str().unwrap_or(""),"ledger-policy-compiler/owner-local-cost-1")?
            || !same_literal(m,support.canonical_value()["plan_schema"].as_str().unwrap_or(""),"ledger-workflow-compiled-plan/3")?{return Err(err("closed owner-local cost compiler tuple"));}
        return Ok(SelectedRecipe::OwnerLocalCost1);
    }
    if support.canonical_value()["schema"]=="ledger-workflow-compiler-descriptor/2" {
        m.visit("compile.family_close_recipe_dispatch")?;
        if !same_literal(m,support.canonical_value()["compiler_semantics"].as_str().unwrap_or(""),"ledger-policy-compiler/family-close-1")?
            || !same_literal(m,support.canonical_value()["plan_schema"].as_str().unwrap_or(""),"ledger-workflow-compiled-plan/2")? {
            return Err(err("closed family-close compiler tuple"));
        }
        return Ok(SelectedRecipe::FamilyClose1);
    }
    require_retail_selected_v1(support,m)?;
    Ok(SelectedRecipe::RetailDraft1)
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
/// Fresh publication compilation under the SAME per-origin work owner.
/// No caller allowance, saved-plan substitution, or independent child reset.
pub fn compile_selected_publication(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let result = (|| {
        let mut compiler = session.meter_mut().original_compile_meter()?;
        let program = compile_selected_program(input, support, &mut compiler)?;
        session.meter_mut().finish_original_compile(compiler)?;
        Ok(program)
    })();
    if result.is_err() { session.invalidate(); }
    result
}
pub fn restore_selected_program(
    input: SelectedSourceInput,
    saved_descriptor: &[u8],
    saved_plan: &[u8],
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let result = restore_selected_inner(input, saved_descriptor, saved_plan, session);
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn restore_selected_inner(
    input: SelectedSourceInput,
    saved_descriptor: &[u8],
    saved_plan: &[u8],
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let mut compiler = session.meter_mut().original_compile_meter()?;
    compiler.enter_compile()?;
    let support = SelectedCompilerSupport::restore_saved(saved_descriptor, &mut compiler)?;
    let recipe=require_selected_recipe(&support,&mut compiler)?;
    let program=compile_selected_recipe(input,&support,&mut compiler,recipe)?;
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
fn compile_selected_recipe(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
    recipe:SelectedRecipe,
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
            "ledger-workflow-policy/1" | "ledger-workflow-policy/2" | "ledger-workflow-policy/3" | "ledger-workflow-policy/4" => {
                if schema!=recipe.policy_schema(){return Err(err("policy/compiler recipe version"));}
                if !matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1) && body.get("app_delayed_costs").is_some(){return Err(err("old policy dictionary has no app declarations"));}
                if matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1){
                    for declaration in body["app_delayed_costs"].as_array().ok_or_else(||err("app declaration array"))?{
                        m.visit("compile.original_app_declaration_version")?;
                        if matches!(recipe,SelectedRecipe::AppOwnedCost1){
                            if declaration.get("correction_anchor").is_some()||declaration.get("causal_reversal").is_some(){return Err(err("old app declaration cannot carry new timing"));}
                        }else if declaration.get("causal_reversal").is_some_and(Value::is_null){return Err(err("null app causal clause"));}
                    }
                }
                if matches!(recipe,SelectedRecipe::RetailDraft1) {
                    // Preserve the old closed serde dictionary and paid trace.
                    // Policy1 never gains the optional Policy2 declaration.
                    for family in body["families"].as_array().ok_or_else(||err("family array"))? {
                        if family.get("no_success_close").is_some(){return Err(err("Policy1 family-close field"));}
                    }
                } else {
                    for family in body["families"].as_array().ok_or_else(||err("family array"))? {
                        m.visit("compile.family_close_declaration")?;
                        if let Some(declaration)=family.get("no_success_close") {
                            if declaration.is_null(){return Err(err("null original close declaration"));}
                        }
                    }
                }
                if policy_raw.replace(body).is_some() {
                    return Err(err("multiple policies"));
                }
            }
            "ledger-workflow-compiler-descriptor/1" | "ledger-workflow-compiler-descriptor/2" | "ledger-workflow-compiler-descriptor/3" | "ledger-workflow-compiler-descriptor/4" | "ledger-workflow-compiler-descriptor/5" | "ledger-workflow-compiler-descriptor/6" => {
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
        policy.schema == recipe.policy_schema()
            && policy.currency == "USD"
            && policy.scale == 18
            && policy.rounding == "nearest_ties_away"
            && policy.mode == "component_workflow"
            && policy.previous_policy_ref.is_none()
            && policy.stages.is_empty()
            && (if matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1){policy.app_delayed_costs.as_ref().is_some_and(|rows|!rows.is_empty()&&rows.len()<=64)}else{policy.app_delayed_costs.is_none()})
            && (matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1)||policy.delayed_costs.is_empty())
    })? {
        return Err(err("selected policy profile"));
    }
    super::normalize::policy(m, &mut policy)?;
    arithmetic::integer(m, &policy.version, false)?;
    super::validate::policy(m, &policy, &product, &template, &schemas,matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1))?;
    for rule in &mut policy.rules {
        m.visit("rule.validate")?;
        let activity = product
            .activity_with_work(&rule.activity, m)?
            .ok_or_else(|| err("rule activity"))?;
        template.component_named_with_work(&rule.component_key, &rule.component_name, m)?;
        if !m.check("rule.slot", || {
            policy.binding_slots.iter().any(|s| {
                s.name == rule.binding_slot
                    && (s.book == "retail" || matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1)&&s.book=="supplier")
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
            Formula::FinalCost{markup,multiplier,ceiling,..}=>{
                if !matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1){return Err(err("final cost compiler recipe"));}
                arithmetic::decimal(m,markup)?;
                let n=arithmetic::integer(m,&multiplier.numerator,false)?;
                let d=arithmetic::integer(m,&multiplier.denominator,false)?;
                arithmetic::rational(m,n,d)?;
                arithmetic::integer(m,&ceiling.atoms,false)?;
            },
            Formula::AppFinalCost{markup,multiplier,ceiling,..}=>{
                if !matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1){return Err(err("app cost compiler recipe"));}
                arithmetic::decimal(m,markup)?;
                let n=arithmetic::integer(m,&multiplier.numerator,false)?;
                let d=arithmetic::integer(m,&multiplier.denominator,false)?;
                arithmetic::rational(m,n,d)?;
                arithmetic::integer(m,&ceiling.atoms,false)?;
            },
            Formula::SupplierFinalCost{ceiling,..}=>{
                if !matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1){return Err(err("supplier cost compiler recipe"));}
                arithmetic::integer(m,&ceiling.atoms,false)?;
            },
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
    let root_position=docs.len();
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
            Formula::AppFinalCost{..}=>{
                let declaration=policy.app_delayed_costs.as_ref().and_then(|rows|rows.iter().find(|d|d.component_key==rule.component_key&&d.activity==rule.activity)).ok_or_else(||err("app cost declaration missing from compiled facts"))?;
                (vec![declaration.usage_field.clone()],"app_final_cost",vec![])
            },
            Formula::FinalCost{..}=>(vec![],"final_cost",vec![]),
            Formula::SupplierFinalCost{..}=>(vec![],"supplier_final_cost",vec![]),
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
    let plan = json!({"schema":recipe.plan_schema(),"compiler_ref":policy.compiler_ref,"compiler_semantics":recipe.compiler_semantics(),"evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits_profile":"workflow-limits/1","source_refs":source_refs,"activity_table":activities,"fact_slots":slots,"link_paths":set(m,policy.families.iter().map(|f|json!(f.selector)).collect())?,"component_slots":template.canonical_value()["components"],"rule_order":order,"typed_instructions":instructions,"dependency_edges":edges,"worst_case":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"},"explanation_map":explanations});
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
        root_position,
        family_close_recipe:matches!(recipe,SelectedRecipe::FamilyClose1|SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1|SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1),
        app_cost_causal_recipe:matches!(recipe,SelectedRecipe::AppCostCausal1),
        app_owned_cost_recipe:matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1),
        supplier_scoped_consent_recipe:matches!(recipe,SelectedRecipe::SupplierScopedConsent1),
        owner_local_cost_recipe:matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1),
        execution_registry:support.execution_registry(),
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
