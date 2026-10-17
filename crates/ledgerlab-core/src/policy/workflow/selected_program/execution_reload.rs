//! Attempt-local reuse of an ACTUAL completed compilation correspondence.
//! This key has no PUB origin/current-support/admission meaning. The host must
//! retain it only with its independently completed PUB association in the same
//! held snapshot. No serialization, Clone, eviction/remint or digest factory.
use super::*;
use crate::validation_work::{OriginalAuditSession,ValidationWork,ValidationVisit,ArithmeticOperation};

pub struct SelectedExecutionReloadKey{
    root:[u8;32],
    plan:[u8;32],
    recipe:SelectedRecipe,
    registry:Option<&'static str>,
    compile_work:u64,
    // Covers this actual Box payload and binds its private attempt identity.
    // The host's bounded key-table Vec/control growth is separately required.
    _ownership:ExternalBufferLease,
}
fn fail(detail:&str)->Error{Error::new("INTEGRITY_FAILURE",detail)}
fn digest_bits(m:&mut CoreMeter,text:&str)->Result<[u8;32]>{
    m.visit("execution_key.digest_shape")?;
    if !work::compare(m,text.len(),71usize)?.is_eq(){return Err(fail("execution key digest length"));}
    m.scan_bytes(7)?;if !text.starts_with("sha256:"){return Err(fail("execution key digest prefix"));}
    fn nibble(m:&mut CoreMeter,b:u8)->Result<u8>{
        m.visit("execution_key.hex_nibble")?;m.scan_bytes(1)?;
        let (base,extra)=match b{b'0'..=b'9'=>(b'0',0u8),b'a'..=b'f'=>(b'a',10u8),_=>return Err(fail("execution key lowercase digest"))};
        ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&b.into(),&base.into()])?;
        let difference=b-base;
        ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&difference.into(),&extra.into()])?;
        Ok(difference+extra)
    }
    let mut result=[0u8;32];
    for (out,pair)in result.iter_mut().zip(text.as_bytes()[7..].chunks_exact(2)){
        let high=nibble(m,pair[0])?;let low=nibble(m,pair[1])?;
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&high.into(),&16u8.into()])?;
        let product=high*16;
        ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&product.into(),&low.into()])?;
        *out=product+low;
    }
    Ok(result)
}
impl SelectedProgram{
    /// Mint compile-correspondence DATA from an actual owning compiler result.
    /// Historical restore exposes that result only AFTER exact saved-plan
    /// equality. The host still completes PUB custody/authority independently.
    pub fn execution_reload_key_with_work(&self,session:&mut OriginalAuditSession)->Result<Box<SelectedExecutionReloadKey>>{
        let result=(||{
            let m=session.meter_mut();m.visit("execution_key.owning_compilation")?;
            if self._control_ownership.is_none(){return Err(fail("execution key cannot adopt a legacy unowned program"));}
            self.require_attempt_owner(m)?;
            m.visit("execution_key.actual_compilation")?;
            if !matches!(self.provenance,ProgramProvenance::Compiled){return Err(fail("execution key cannot be reminted by reload"));}
            let arena=m.representation_arena().ok_or_else(||fail("execution key requires original attempt"))?;
            let ownership=arena.reserve_external_payload(mem::size_of::<SelectedExecutionReloadKey>(),m)?;
            let root=digest_bits(m,self.root_policy_ref().digest())?;
            let plan=digest_bits(m,self.compiled_plan().reference().digest())?;
            m.write_bytes(mem::size_of::<SelectedExecutionReloadKey>())?;
            Ok(Box::new(SelectedExecutionReloadKey{root,plan,recipe:self.recipe,registry:self.execution_registry,compile_work:self.analysis.compile_work,_ownership:ownership}))
        })();
        if result.is_err(){session.invalidate();}result
    }
}
fn same_bits(m:&mut CoreMeter,left:&[u8;32],right:&[u8;32])->Result<bool>{
    m.visit("execution_reload.exact_digest")?;m.scan_bytes(64)?;Ok(left==right)
}
fn same_reference(m:&mut CoreMeter,value:&Value,r:&DefinitionRef)->Result<bool>{
    use crate::policy::workflow::transition_constructor::shape;
    shape::closed(m,value,&["kind","id","digest"],&[])?;
    for(field,expected)in[("kind",r.kind()),("id",r.id()),("digest",r.digest())]{
        let actual=shape::text(m,value,field)?;if !same_literal(m,actual,expected)?{return Ok(false);}
    }Ok(true)
}
/// Rebuild an execution DATA representation from the exact bodies whose
/// source→plan correspondence this live key already established. No compiler,
/// fresh support selector, saved-output assertion or additional meter is used.
/// The host separately supplies its actually completed PUB association.
pub fn rehydrate_selected_execution(input:SelectedSourceInput,saved_plan:&[u8],key:&SelectedExecutionReloadKey,session:&mut OriginalAuditSession)->Result<SelectedProgram>{
    let result=rehydrate_inner(input,saved_plan,key,session.meter_mut());
    if result.is_err(){session.invalidate();}result
}
fn rehydrate_inner(input:SelectedSourceInput,saved_plan:&[u8],key:&SelectedExecutionReloadKey,m:&mut CoreMeter)->Result<SelectedProgram>{
    use crate::policy::workflow::transition_constructor::shape;
    let arena=m.representation_arena().ok_or_else(||fail("execution reload requires original attempt"))?;
    arena.require_external_owner(&key._ownership,m)?;
    let count=input.documents.len();
    if work::compare(m,count,1usize)?.is_lt()||work::compare(m,count,128usize)?.is_gt(){return Err(fail("execution reload source count"));}
    let raw_plan=m.parse_owned_representation(&arena,saved_plan,RepresentationKind::Record)?;
    codec::validate_selected_types(m,raw_plan.value())?;
    let schema=shape::text(m,raw_plan.value(),"schema")?;
    let semantics=shape::text(m,raw_plan.value(),"compiler_semantics")?;
    if !same_literal(m,schema,key.recipe.plan_schema())?||!same_literal(m,semantics,key.recipe.compiler_semantics())?{return Err(fail("execution reload original plan recipe"));}
    let plan=finish_owned_document(m,raw_plan,true)?;
    if !super::super::support::same_bytes(m,plan.canonical_bytes(),saved_plan)?{return Err(fail("execution reload noncanonical plan"));}
    let actual=digest_bits(m,plan.reference().digest())?;
    if !same_bits(m,&actual,&key.plan)?{return Err(fail("execution reload different compiled plan"));}
    let actual_root=digest_bits(m,input.root_policy_ref.digest())?;
    if !same_bits(m,&actual_root,&key.root)?||!same_literal(m,input.root_policy_ref.kind(),"workflow-policy")?{return Err(fail("execution reload different original root"));}
    // Requested payload only; no quota. Source Value clones and each SAME
    // normalizer's private DTO/maps/body bound are separately pre-admitted.
    let per=mem::size_of::<SelectedDocument>()+mem::size_of::<(DefinitionRef,FactSchema)>()+mem::size_of::<ExternalBufferLease>()+"workflow-compiler-descriptor".len()+70+71;
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
    let controls=count.checked_mul(per).ok_or_else(||fail("execution reload control product"))?;
    let controls=work::usize_add(m,controls,"workflow-policy".len()+69+71)?;
    let ownership=arena.reserve_external_payload(controls,m)?;
    let mut docs=Vec::new();docs.try_reserve_exact(count).map_err(|_|fail("execution reload source slots"))?;
    let mut schemas=Vec::new();schemas.try_reserve_exact(count).map_err(|_|fail("execution reload schema slots"))?;
    let mut projection_ownership=Vec::new();projection_ownership.try_reserve_exact(count).map_err(|_|fail("execution reload normalizer leases"))?;
    let mut bytes=0;
    for raw in input.documents{
        m.visit("execution_reload.source_body")?;bytes=work::usize_add(m,bytes,raw.len())?;
        if work::compare(m,bytes,1048576usize)?.is_gt(){return Err(fail("execution reload source closure bytes"));}
        let document=SourceValue::parse(m,&raw)?.document(m)?;
        if !super::super::support::same_bytes(m,document.canonical_bytes(),&raw)?{return Err(fail("execution reload noncanonical source body"));}
        docs.push(document);
    }
    let expected=shape::array(m,plan.canonical_value(),"source_refs")?;
    if !work::compare(m,expected.len(),docs.len())?.is_eq(){return Err(fail("execution reload exact source closure count"));}
    for reference in expected{
        m.visit("execution_reload.expected_source")?;let mut hits=0;
        for document in &docs{if same_reference(m,reference,document.reference())?{hits=work::usize_add(m,hits,1)?;}}
        if !work::compare(m,hits,1usize)?.is_eq(){return Err(fail("execution reload missing or repeated exact source"));}
    }
    let mut root_position=None;
    for(position,document)in docs.iter().enumerate(){
        m.visit("execution_reload.root_source")?;
        let r=document.reference();
        if same_literal(m,r.kind(),input.root_policy_ref.kind())?&&same_literal(m,r.id(),input.root_policy_ref.id())?&&same_literal(m,r.digest(),input.root_policy_ref.digest())?{
            if root_position.is_some(){return Err(fail("execution reload duplicate root"));}root_position=Some(position);
        }
        if !same_literal(m,r.kind(),"workflow-fact-schema")?{continue;}
        let bound=FactSchema::normalization_payload_bound_with_work(document.canonical_value(),m)?;
        let value=normalizer_projection(m,document.canonical_value(),&mut projection_ownership,bound)?;
        let schema=FactSchema::from_value_complete_with_work(value,m)?;
        if !shape::same(m,schema.canonical_value(),document.canonical_value())?{return Err(fail("execution reload altered schema normalization"));}
        let reference=DefinitionRef::new_with_work(r.kind(),r.id(),r.digest(),m)?;
        schemas.push((reference,schema));
    }
    let root_position=root_position.ok_or_else(||fail("execution reload missing original policy"))?;
    let root_body=docs[root_position].canonical_value();
    let policy_schema=shape::text(m,root_body,"schema")?;
    if !same_literal(m,policy_schema,key.recipe.policy_schema())?{return Err(fail("execution reload original policy recipe"));}
    let bound=super::super::policy_decode::payload_bound(m,root_body)?;
    let policy_owner=arena.reserve_external_payload(bound,m)?;
    let value=normalizer_projection(m,root_body,&mut projection_ownership,0)?;
    let(mut policy,policy_owner)=super::super::policy_decode::decode(m,value,policy_owner)?;
    super::super::normalize::policy(m,&mut policy)?;
    let normalized=super::super::owning_policy::build(m,&policy)?;
    if !shape::same(m,normalized.value(),root_body)?{return Err(fail("execution reload altered policy normalization"));}drop(normalized);
    for reference in [&policy.product_ref,&policy.slot_template_ref,&policy.context_schema_ref,&policy.compiler_ref]{lookup(m,&docs,reference)?;}
    if !same_reference(m,&plan.canonical_value()["compiler_ref"],&policy.compiler_ref)?{return Err(fail("execution reload compiler association"));}
    let root=DefinitionRef::new_with_work(input.root_policy_ref.kind(),input.root_policy_ref.id(),input.root_policy_ref.digest(),m)?;
    let mut maximum_path=0;
    for family in &policy.families{m.visit("execution_reload.analysis_path")?;if work::compare(m,family.selector.path.len(),maximum_path)?.is_gt(){maximum_path=family.selector.path.len();}}
    let analysis=StaticAnalysis{fields:shape::array(m,plan.canonical_value(),"fact_slots")?.len(),rules:policy.rules.len(),families:policy.families.len(),maximum_path,compile_work:key.compile_work};
    let recipe=key.recipe;
    Ok(SelectedProgram{provenance:ProgramProvenance::Reloaded,recipe,root,root_position,sources:docs,plan,policy,schemas,analysis,
        family_close_recipe:!matches!(recipe,SelectedRecipe::RetailDraft1),
        owner_local_cost_recipe:matches!(recipe,SelectedRecipe::OwnerLocalCost1|SelectedRecipe::SupplierScopedConsent1),
        supplier_scoped_consent_recipe:matches!(recipe,SelectedRecipe::SupplierScopedConsent1),
        app_owned_cost_recipe:matches!(recipe,SelectedRecipe::AppOwnedCost1|SelectedRecipe::AppCostCausal1),
        app_cost_causal_recipe:matches!(recipe,SelectedRecipe::AppCostCausal1),execution_registry:key.registry,
        _policy_decode_ownership:Some(policy_owner),_source_projection_ownership:projection_ownership,_control_ownership:Some(ownership)})
}
