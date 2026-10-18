//! Same immutable identity recipes used by work construction and parent DATA
//! lookup. No amounts, candidate, authority, absence or completed-origin proof.
use super::{CoreMeter,body,data::OperationKey,shape};
use crate::{Result,canonical::audit_arena::AuditArena};
use serde_json::Value;
fn arena(m:&mut CoreMeter)->Result<AuditArena>{m.representation_arena().ok_or_else(||shape::fail("owning work identity"))}
pub(super) fn scope(m:&mut CoreMeter,key:&OperationKey)->Result<crate::canonical::audit_arena::AuditedValue>{
 let a=arena(m)?;let l=a.string_value(&key.scope[0],m)?;let r=a.string_value(&key.scope[1],m)?;a.array_from_values(&[l.value(),r.value()],m)
}
pub(super) fn claim(m:&mut CoreMeter,key:&OperationKey,namespace:&str,p:&Value)->Result<String>{
 let a=arena(m)?;let store=a.string_value(&key.logical_store_id,m)?;let scope=scope(m,key)?;let ns=a.string_value(namespace,m)?;let completion=a.string_value("completion",m)?;let business=shape::field(m,p,"business_operation_id")?;
 let tuple=a.array_from_values(&[store.value(),scope.value(),ns.value(),business,completion.value()],m)?;body::semantic_id(m,"wclaim_","workflow-work-claim",tuple.value())
}
pub(super) fn event(m:&mut CoreMeter,key:&OperationKey,p:&Value)->Result<String>{
 let a=arena(m)?;let store=a.string_value(&key.logical_store_id,m)?;let scope=scope(m,key)?;let source=a.string_value(&key.source,m)?;let external=shape::field(m,p,"external_event_id")?;
 let tuple=a.array_from_values(&[store.value(),scope.value(),source.value(),external],m)?;body::semantic_id(m,"wevent_","workflow-event",tuple.value())
}
pub(super) fn decision(m:&mut CoreMeter,claim:&str)->Result<String>{
 let a=arena(m)?;let claim=a.string_value(claim,m)?;let discriminator=a.string_value("work",m)?;let tuple=a.array_from_values(&[claim.value(),discriminator.value()],m)?;body::semantic_id(m,"wdecision_","workflow-decision",tuple.value())
}
pub(super) fn target(m:&mut CoreMeter,key:&OperationKey,agreement:&Value,claim:&str,slot:&str)->Result<String>{
 let a=arena(m)?;let scope=scope(m,key)?;let claim=a.string_value(claim,m)?;let slot=a.string_value(slot,m)?;let tuple=a.array_from_values(&[scope.value(),agreement,claim.value(),slot.value()],m)?;body::semantic_id(m,"wtarget_","workflow-target",tuple.value())
}
pub(super) fn app_decision(m:&mut CoreMeter,scope:&Value,work_id:&str,slot:&str)->Result<String>{
 let a=arena(m)?;let work=a.string_value(work_id,m)?;let slot=a.string_value(slot,m)?;let discriminator=a.string_value("app_cost",m)?;let tuple=a.array_from_values(&[scope,work.value(),slot.value(),discriminator.value()],m)?;body::semantic_id(m,"wdecision_","workflow-decision",tuple.value())
}
