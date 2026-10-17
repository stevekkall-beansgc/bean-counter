//! RP01 storage for the SAME derived selected Policy grammar. Source strings
//! move from the separately admitted Value projection; this bound covers the
//! typed Vecs, internally-tagged serde Content buffers and Ref-copy overlap.
//! It is not a whole compiler work/arena certificate.
use super::{model::*,super::{DefinitionRef,transition_constructor::{CoreMeter}}};
use crate::{Result,Error,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,ValidationWork,ArithmeticOperation}};
use serde::Deserialize;
use serde_json::Value;
use std::mem;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReference{kind:String,id:String,digest:String}
fn fail()->Error{Error::new("UNSUPPORTED_SCHEMA_PROFILE","closed selected policy")}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
 ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
 a.checked_mul(b).ok_or_else(fail)
}
// Every possible typed array element in Policy's closed derived grammar.
const CELL:usize={let values=[mem::size_of::<BindingSlot>(),mem::size_of::<Rule>(),mem::size_of::<Family>(),mem::size_of::<Code>(),mem::size_of::<DelayedCost>(),mem::size_of::<AppCostDeclaration>(),mem::size_of::<String>(),mem::size_of::<Value>()];let mut max=0;let mut i=0;while i<values.len(){if values[i]>max{max=values[i];}i+=1;}max};
type Content=serde::__private229::de::Content<'static>;
// The expected-field/variant strings are literals from the same model; this
// superset bounds serde's single failure message, including an escaped input
// field/string (at most six bytes per UTF8 input byte). No input is formatted
// into an error unless this storage has already been admitted.
const EXPECTED:&str=stringify!(schema policy_id version product_ref context_schema_ref slot_template_ref compiler_ref currency scale rounding mode binding_slots rules families stages delayed_costs app_delayed_costs previous_policy_ref name book funding_leg allowed_activities correction_source_slot rule_id activity component_name component_key binding_slot phase when formula dependency op true kind fixed amount unit quantity_field rate discount basis view percent final_cost supplier_slot evidence_slot multiplier numerator denominator markup ceiling supplier_final_cost app_final_cost evidence_source_slot finalize_within_us on_missing resource_funding_leg usage_field usage_unit work_source_slot cost_source_slot correction_within_us correction_anchor causal_reversal maximum_final_amount cost_basis comparable_quantity_rule allocation root_relation standalone_deadline_effect family_name family_key trigger_activity target_link_slot selector source_slot code_field codes window maximum_premium maximum_discount no_success_close close_source_slot path target_activity code effect none premium anchor anchor_path start_offset_us occurrence_us receipt_grace_us acceptance_grace_us activation_deadline_us correction_us atoms kind id digest);
fn dimensions(m:&mut CoreMeter,v:&Value,depth:usize)->Result<(usize,usize)>{
 m.visit("compile.policy_decode_storage_node")?;
 if w::compare(m,depth,64)?.is_gt(){return Err(fail());}
 // Boxed Content options/newtypes, although none is needed by valid selected
 // policies, also bound every malformed derived-deserializer prefix.
 let mut bytes=mul(m,2,mem::size_of::<Content>())?;let mut strings=0;
 match v{
  Value::String(s)=>{m.scan_bytes(s.len())?;strings=s.len();},
  Value::Array(rows)=>{
   // serde Vec's cautious exact size hint is capped at1MiB; later geometric
   // growth has old+new live capacity <3*max(len,4). Content Seq plus the
   // corresponding typed Vec may overlap; account both, not either alone.
   let n=if w::compare(m,rows.len(),0)?.is_eq(){0}else if w::compare(m,rows.len(),4)?.is_lt(){4}else{rows.len()};
   let per=w::usize_add(m,CELL,mem::size_of::<Content>())?;
   let cells=mul(m,n,per)?;let allocation=mul(m,3,cells)?;bytes=w::usize_add(m,bytes,allocation)?;
   for row in rows{let next=w::usize_add(m,depth,1)?;let child=dimensions(m,row,next)?;bytes=w::usize_add(m,bytes,child.0)?;strings=w::usize_add(m,strings,child.1)?;}
  },
  Value::Object(rows)=>{
   let n=if w::compare(m,rows.len(),0)?.is_eq(){0}else if w::compare(m,rows.len(),4)?.is_lt(){4}else{rows.len()};
   let cells=mul(m,n,mem::size_of::<(Content,Content)>())?;let allocation=mul(m,3,cells)?;bytes=w::usize_add(m,bytes,allocation)?;
   for(key,row)in rows{m.visit("compile.policy_decode_storage_field")?;m.scan_bytes(key.len())?;strings=w::usize_add(m,strings,key.len())?;let next=w::usize_add(m,depth,1)?;let child=dimensions(m,row,next)?;bytes=w::usize_add(m,bytes,child.0)?;strings=w::usize_add(m,strings,child.1)?;}
  },_=>{}
 }
 Ok((bytes,strings))
}
pub(super) fn payload_bound(m:&mut CoreMeter,v:&Value)->Result<usize>{
 // All selected recipes already require empty stages. Refuse the unsupported
 // branch before serde could rebuild arbitrary Value/BTreeMap stage bodies.
 // The legacy path retains its original validation order and trace.
 m.visit("compile.policy_stages_dictionary")?;
 let stages=crate::policy::workflow::facts::source::member(v,"stages",m)?;
 m.visit("compile.policy_stages_type")?;
 if let Some(rows)=stages.and_then(Value::as_array){
  if w::compare(m,rows.len(),0usize)?.is_gt(){return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","selected policy profile"));}
 }
 let (mut bytes,strings)=dimensions(m,v,0)?;
 // DefinitionRef creates one new copy of each of the raw Ref's three moved
 // strings. Other typed strings are moved by Value::visit_string/Content.
 for field in ["product_ref","context_schema_ref","slot_template_ref","compiler_ref","previous_policy_ref"]{
  if let Some(row)=v.get(field){for key in ["kind","id","digest"]{if let Some(s)=row.get(key).and_then(Value::as_str){bytes=w::usize_add(m,bytes,s.len())?;}}}
 }
 let expected=mul(m,2,EXPECTED.len())?;let escaped=mul(m,6,strings)?;
 let message=w::usize_add(m,expected,escaped)?;let error=w::usize_add(m,message,256)?;
 // String formatting growth and into_boxed_str shrinking may overlap the
 // old allocation, followed by ErrorImpl; reserve all three simultaneously.
 let capacity=w::usize_add(m,error,8)?;let errors=mul(m,3,capacity)?;bytes=w::usize_add(m,bytes,errors)?;
 let headers=mem::size_of::<Policy<RawReference>>()+mem::size_of::<Policy>()+mem::size_of::<Value>()+2*mem::size_of::<usize>()+mem::size_of::<String>();
 w::usize_add(m,bytes,headers)
}
fn reference(m:&mut CoreMeter,r:RawReference)->Result<DefinitionRef>{
 DefinitionRef::new_with_work(&r.kind,&r.id,&r.digest,m).map_err(|error|match error.code{
  "DEFINITION_KIND"|"IDENTIFIER"|"DEFINITION_DIGEST_MISMATCH"=>fail(),_=>error,
 })
}
pub(super) fn decode(m:&mut CoreMeter,value:Value,owner:ExternalBufferLease)->Result<(Policy,ExternalBufferLease)>{
 // The source-derived storage bound includes the complete geometric Vec
 // relocation/push payload and failure formatting overlap. Reserve one such
 // full possible byte pass before the opaque derived decoder executes; Ref
 // syntax/copies below retain their own semantic callbacks as well.
 let bytes=owner.reserved_payload_bytes();m.scan_bytes(bytes)?;m.write_bytes(bytes)?;
 // Only the Ref representation changes here; all field dictionaries, enum
 // tags, required/default fields and type refusal come from SAME Policy<R>.
 decode_work(&value,m)?;
 let raw:Policy<RawReference>=serde_json::from_value(value).map_err(|_|fail())?;
 let policy=Policy{schema:raw.schema,policy_id:raw.policy_id,version:raw.version,
  product_ref:reference(m,raw.product_ref)?,context_schema_ref:reference(m,raw.context_schema_ref)?,slot_template_ref:reference(m,raw.slot_template_ref)?,compiler_ref:reference(m,raw.compiler_ref)?,
  currency:raw.currency,scale:raw.scale,rounding:raw.rounding,mode:raw.mode,binding_slots:raw.binding_slots,rules:raw.rules,families:raw.families,stages:raw.stages,delayed_costs:raw.delayed_costs,app_delayed_costs:raw.app_delayed_costs,
  previous_policy_ref:raw.previous_policy_ref.map(|r|reference(m,r)).transpose()?};
 Ok((policy,owner))
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::{canonical::audit_arena::AuditArena,validation_work::OriginalAuditSession};
 fn policy()->Value{
  let source:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
  source["documents"].as_array().unwrap().iter().find(|v|v["schema"]=="ledger-workflow-policy/1").unwrap().clone()
 }
 #[test]
 fn owning_policy_decode_reuses_derived_dictionary_and_paid_refs(){
  let mut values=vec![policy()];
  for(edit,value)in [("unknown",Value::String("x".into())),("product_ref",serde_json::json!({"kind":"workflow-product","id":"bad","digest":"bad"})),("scale",Value::String("18".into()))]{let mut raw=policy();raw[edit]=value;values.push(raw);}
  let mut raw=policy();raw["rules"][0]["formula"]["unknown"]=Value::String("x".into());values.push(raw);
  for value in values{
   let legacy=serde_json::from_value::<Policy>(value.clone()).map(|p|serde_json::to_value(p).unwrap());
   let arena=AuditArena::new().unwrap();let before=arena.usage().unwrap().live_bytes;
   let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
   let m=session.meter_mut();let n=payload_bound(m,&value).unwrap();let owner=arena.reserve_external_payload(n,m).unwrap();
   let result=decode(m,value,owner);
   match(legacy,result){
    (Ok(expected),Ok((actual,owner)))=>{assert_eq!(serde_json::to_value(&actual).unwrap(),expected);assert!(arena.usage().unwrap().live_bytes>before);drop(actual);drop(owner);},
    (Err(_),Err(error))=>assert_eq!(error,fail()),_=>panic!("derived policy dictionary diverged"),
   }
   assert_eq!(arena.usage().unwrap().live_bytes,before);
  }
 }
}
// Accounting-only projection of the SAME model dictionaries. Serde remains
// the sole decoder/refusal authority; these calls reserve its actual field,
// tag and type dispatch work before it runs (not a second policy parser).
fn decode_work(value:&Value,m:&mut CoreMeter)->Result<()>{
 use crate::policy::workflow::facts::source as paid;
 fn object(v:&Value,keys:&[&str],m:&mut CoreMeter)->Result<()>{paid::object_decode_work(v,keys,m)}
 fn child<'a>(v:&'a Value,key:&str,m:&mut CoreMeter)->Result<&'a Value>{Ok(paid::member(v,key,m)?.unwrap_or(&Value::Null))}
 fn rows<'a>(v:&'a Value,key:&str,m:&mut CoreMeter)->Result<&'a [Value]>{Ok(child(v,key,m)?.as_array().map(Vec::as_slice).unwrap_or(&[]))}
 fn tag(v:&Value,key:&str,variants:&[&str],m:&mut CoreMeter)->Result<()>{
  let value=child(v,key,m)?;if let Some(text)=value.as_str(){for variant in variants{m.visit("compile.policy_serde_variant")?;m.scan_bytes(text.len())?;m.scan_bytes(variant.len())?;}}Ok(())
 }
 fn types(v:&Value,m:&mut CoreMeter,depth:usize)->Result<()>{
  m.visit("compile.policy_serde_value_type")?;if w::compare(m,depth,64usize)?.is_gt(){return Err(fail());}
  match v{Value::Array(rows)=>{for row in rows{m.visit("compile.policy_serde_element")?;let next=w::usize_add(m,depth,1)?;types(row,m,next)?;}},Value::Object(rows)=>{for row in rows.values(){m.visit("compile.policy_serde_field_type")?;let next=w::usize_add(m,depth,1)?;types(row,m,next)?;}},_=>{m.visit("compile.policy_serde_scalar_type")?;}}Ok(())
 }
 const AMOUNT:&[&str]=&["currency","scale","atoms"];
 const PERCENT:&[&str]=&["kind","percent"];
 const FORMULA:&[&str]=&["kind","amount","quantity_field","unit","rate","basis","mode","evidence_slot","supplier_slot","multiplier","markup","ceiling"];
 types(value,m,0)?;paid::error_work(value,m)?;
 object(value,&["schema","policy_id","version","product_ref","context_schema_ref","slot_template_ref","compiler_ref","currency","scale","rounding","mode","binding_slots","rules","families","stages","delayed_costs","app_delayed_costs","previous_policy_ref"],m)?;
 for field in ["product_ref","context_schema_ref","slot_template_ref","compiler_ref","previous_policy_ref"]{object(child(value,field,m)?,&["kind","id","digest"],m)?;}
 for row in rows(value,"binding_slots",m)?{object(row,&["name","book","funding_leg","allowed_activities","correction_source_slot"],m)?;}
 for rule in rows(value,"rules",m)?{
  object(rule,&["rule_id","activity","component_name","component_key","binding_slot","phase","when","formula","dependency"],m)?;
  let predicate=child(rule,"when",m)?;object(predicate,&["op"],m)?;tag(predicate,"op",&["true"],m)?;
  let formula=child(rule,"formula",m)?;
  // TaggedContentVisitor identifies kind and buffers other keys; the derived
  // variant visitor then dispatches the same fields. Prepay both passes.
  object(formula,FORMULA,m)?;object(formula,FORMULA,m)?;
  tag(formula,"kind",&["fixed","unit","discount","final_cost","supplier_final_cost","app_final_cost"],m)?;
  object(child(formula,"basis",m)?,&["component_key","view"],m)?;
  object(child(formula,"amount",m)?,PERCENT,m)?;
  object(child(formula,"multiplier",m)?,&["numerator","denominator"],m)?;
  object(child(formula,"ceiling",m)?,AMOUNT,m)?;
 }
 for family in rows(value,"families",m)?{
  object(family,&["family_name","family_key","trigger_activity","target_link_slot","selector","source_slot","code_field","codes","window","maximum_premium","maximum_discount","correction_source_slot","no_success_close"],m)?;
  object(child(family,"selector",m)?,&["path","target_activity","binding_slot","component_key","basis"],m)?;
  object(child(family,"window",m)?,&["anchor","anchor_path","start_offset_us","occurrence_us","receipt_grace_us","acceptance_grace_us","activation_deadline_us","correction_us"],m)?;
  for field in ["maximum_premium","maximum_discount"]{object(child(family,field,m)?,AMOUNT,m)?;}
  object(child(family,"no_success_close",m)?,&["close_source_slot"],m)?;
  for code in rows(family,"codes",m)?{
   object(code,&["code","effect"],m)?;let effect=child(code,"effect",m)?;
   object(effect,&["kind","amount"],m)?;object(effect,&["kind","amount"],m)?;
   tag(effect,"kind",&["none","premium","discount"],m)?;object(child(effect,"amount",m)?,PERCENT,m)?;
  }
 }
 for row in rows(value,"delayed_costs",m)?{object(row,&["component_key","evidence_source_slot","finalize_within_us","on_missing"],m)?;}
 for row in rows(value,"app_delayed_costs",m)?{
  object(row,&["component_key","activity","resource_funding_leg","usage_field","usage_unit","work_source_slot","cost_source_slot","correction_source_slot","finalize_within_us","correction_within_us","correction_anchor","causal_reversal","maximum_final_amount","on_missing","cost_basis","comparable_quantity_rule","allocation"],m)?;
  object(child(row,"maximum_final_amount",m)?,AMOUNT,m)?;
  object(child(row,"causal_reversal",m)?,&["root_relation","mode","standalone_deadline_effect"],m)?;
 }
 Ok(())
}
