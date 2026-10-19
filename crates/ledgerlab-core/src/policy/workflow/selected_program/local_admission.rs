//! Release admission for the supported local service/app/outcome composition.
//! This narrows NEW promises; saved source/plan decoding and receipt recovery
//! retain their exact original recipe. Names, prices and time windows are data.
use super::{compiler::{same_literal,SelectedDocument},model::{Formula,Policy}};
use super::super::{DefinitionRef,FactSchema,transition_constructor::{CoreMeter,shape}};
use crate::{Error,Result,validation_work as w};
use serde_json::Value;
fn unsupported(detail:&str)->Error{Error::new("UNSUPPORTED_COMPOSITION",detail)}
fn require(m:&mut CoreMeter,condition:bool,detail:&str)->Result<()>{
    m.visit("local.admission_constraint")?;
    if condition{Ok(())}else{Err(unsupported(detail))}
}
macro_rules! require { ($m:expr,$condition:expr,$detail:expr) => {{ let condition=$condition; require($m,condition,$detail) }}; }
fn field_is(m:&mut CoreMeter,value:&Value,field:&str,want:&str)->Result<bool>{let actual=shape::text(m,value,field)?;same_literal(m,actual,want)}
fn count(m:&mut CoreMeter,actual:usize,want:usize,detail:&str)->Result<()>{
    if w::compare(m,actual,want)?.is_eq(){Ok(())}else{Err(unsupported(detail))}
}
fn schema<'a>(m:&mut CoreMeter,schemas:&'a [(DefinitionRef,FactSchema)],id:&str)->Result<&'a Value>{
    for(reference,schema)in schemas{
        if same_literal(m,reference.id(),id)?{return Ok(schema.canonical_value());}
    }
    Err(unsupported("supported local fact schema missing"))
}
pub(super) fn check(m:&mut CoreMeter,p:&Policy,docs:&[SelectedDocument],schemas:&[(DefinitionRef,FactSchema)])->Result<()>{
    count(m,p.binding_slots.len(),1,"supported local recipe requires one retail binding")?;
    count(m,p.rules.len(),2,"supported local recipe requires service and app cost")?;
    count(m,p.families.len(),1,"supported local recipe requires one outcome family")?;
    let declarations=p.app_delayed_costs.as_ref().ok_or_else(||unsupported("supported local app declaration"))?;
    count(m,declarations.len(),1,"supported local recipe requires one app component")?;
    require!(m,declarations[0].causal_reversal.is_some(),"supported local recipe requires declared app causal reversal")?;
    let mut base=None;let mut app=None;
    for rule in &p.rules{
        m.visit("local.admission_rule")?;
        match &rule.formula{
            Formula::Fixed{..}=>{require!(m,base.is_none(),"supported local service producer")?;base=Some(rule);},
            Formula::AppFinalCost{..}=>{require!(m,app.is_none(),"supported local app producer")?;app=Some(rule);},
            _=>return Err(unsupported("unsupported local service/app formula composition")),
        }
    }
    let base=base.ok_or_else(||unsupported("supported local service producer missing"))?;
    let app=app.ok_or_else(||unsupported("supported local app producer missing"))?;
    require!(m,same_literal(m,&base.activity,&app.activity)?,"supported local service and app must share work")?;
    let family=&p.families[0];
    require!(m,family.no_success_close.is_some(),"supported local recipe requires declared no-success close")?;
    count(m,family.selector.path.len(),1,"supported local outcome requires a direct link")?;
    require!(m,same_literal(m,&family.selector.target_activity,&base.activity)?,"supported local family work")?;
    require!(m,same_literal(m,&family.selector.component_key,&base.component_key)?,"supported local family service basis")?;
    let mut product=None;
    for doc in docs{
        if same_literal(m,doc.reference().id(),p.product_ref.id())?{product=Some(doc.canonical_value());}
    }
    let product=product.ok_or_else(||unsupported("supported local product missing"))?;
    let activities=shape::array(m,product,"activities")?;
    count(m,activities.len(),2,"supported local recipe requires work and outcome activities")?;
    let mut work_seen=false;let mut outcome_seen=false;
    for activity in activities{
        let name=shape::text(m,activity,"name")?;
        let primitive=shape::text(m,activity,"primitive")?;
        let schema_id=shape::text(m,&activity["fact_schema"],"id")?;
        let fields=schema(m,schemas,schema_id)?;
        let fields=shape::array(m,fields,"fields")?;
        if same_literal(m,name,&base.activity)?{
            require!(m,!work_seen&&same_literal(m,primitive,"work")?,"supported local work activity")?;work_seen=true;
            for field in fields{
                require!(m,field_is(m,&field["provenance"],"kind","caller")?,"supported local work cannot require attested or derived facts")?;
            }
        }else if same_literal(m,name,&family.trigger_activity)?{
            require!(m,!outcome_seen&&same_literal(m,primitive,"outcome")?,"supported local outcome activity")?;outcome_seen=true;
            count(m,fields.len(),2,"supported local outcome requires code and occurrence facts")?;
            for field in fields{
                let field_name=shape::text(m,field,"name")?;
                let code=same_literal(m,field_name,&family.code_field)?;
                let expected=if code{"outcome"}else{"occurrence"};
                require!(m,field_is(m,&field["provenance"],"evidence_kind",expected)?,"unsupported local outcome fact purpose")?;
            }
        }else{return Err(unsupported("unsupported local product activity"));}
    }
    require!(m,work_seen&&outcome_seen,"supported local work/outcome pair")?;
    let links=shape::array(m,product,"links")?;
    count(m,links.len(),1,"supported local recipe has only its outcome-to-work link")?;
    let link=&links[0];
    require!(m,field_is(m,link,"name",&family.target_link_slot)?,"supported local outcome link")?;
    let predecessors=shape::array(m,link,"predecessor_activities")?;
    count(m,predecessors.len(),1,"supported local link has one work predecessor")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical::{self,audit_arena::AuditArena},policy::workflow::{document_digest,selected_program::{SelectedCompilerSupport,SelectedSourceInput,compile_selected_publication}},validation_work::{OriginalAuditSession,ResourceLimits}};
    #[test]
    fn supported_local_recipe_is_structural_and_refuses_unimplemented_promises(){
        let arena=AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support=SelectedCompilerSupport::local_execution_with_session(&mut session).unwrap();
        let fixture:Value=serde_json::from_slice(include_bytes!("fixtures/local-first-policy-input.json")).unwrap();
        let mut documents=fixture["documents"].as_array().unwrap().clone();
        let policy=documents.iter_mut().find(|v|v["schema"]=="ledger-workflow-policy/4").unwrap();
        policy["compiler_ref"]=serde_json::to_value(support.reference()).unwrap();
        // A price change within the supported structural recipe is data.
        policy["rules"][0]["formula"]["amount"]=serde_json::json!("1.2");
        let digest=document_digest("ledger-workflow-policy/4",policy).unwrap();
        let root=DefinitionRef::new("workflow-policy",&format!("wdoc_{}",&digest[7..]),&digest).unwrap();
        documents.push(support.canonical_value().clone());
        let program=compile_selected_publication(SelectedSourceInput{documents:documents.iter().map(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec()).collect(),root_policy_ref:root},&support,&mut session).unwrap();
        program.require_local_admission_with_session(&mut session).unwrap();
        let mut m=CoreMeter::for_compile(1048576).unwrap();
        let p=program.policy();
        let mut changed=p.clone();changed.families.push(changed.families[0].clone());
        assert_eq!(check(&mut m,&changed,program.source_documents(),program.fact_schemas()).unwrap_err().code,"UNSUPPORTED_COMPOSITION");
        let mut changed=p.clone();changed.families[0].selector.path.push("other:link".into());
        assert_eq!(check(&mut m,&changed,program.source_documents(),program.fact_schemas()).unwrap_err().code,"UNSUPPORTED_COMPOSITION");
        let mut changed=p.clone();changed.rules[0].formula=Formula::Unit{quantity_field:"support:quantity".into(),unit:"support:request".into(),rate:"1".into()};
        assert_eq!(check(&mut m,&changed,program.source_documents(),program.fact_schemas()).unwrap_err().code,"UNSUPPORTED_COMPOSITION");
        let mut changed=p.clone();changed.families[0].no_success_close=None;
        assert_eq!(check(&mut m,&changed,program.source_documents(),program.fact_schemas()).unwrap_err().code,"UNSUPPORTED_COMPOSITION");
        let mut changed=p.clone();changed.app_delayed_costs.as_mut().unwrap()[0].causal_reversal=None;
        assert_eq!(check(&mut m,&changed,program.source_documents(),program.fact_schemas()).unwrap_err().code,"UNSUPPORTED_COMPOSITION");
    }
}
