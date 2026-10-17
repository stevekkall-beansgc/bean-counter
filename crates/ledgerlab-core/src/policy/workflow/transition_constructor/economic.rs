//! Actual fresh/original entry points over ONE complete interpreter and ONE
//! composed origin session. Incomplete eligibility is an explicit failure after
//! real construction/assembly, never a draft promotion or success token.
use super::{candidate::{EconomicCandidate,assembly::Assembly},construct,data::*,shape,working_value::owning_reference};
use super::super::selected_program::SelectedProgram;
use crate::{Result,Error,validation_work::{self as w,OriginalAuditSession,RepresentationKind,ValidationWork}};

fn unfinished(s:&str)->Error{Error::new("UNSUPPORTED_SCHEMA_PROFILE",s)}
fn same_text(m:&mut super::CoreMeter,v:&serde_json::Value,k:&str,want:&str)->Result<()>{let actual=shape::text(m,v,k)?;if !shape::same_text(m,actual,want)?{return Err(shape::fail("original captured field mismatch"));}Ok(())}
fn key(m:&mut super::CoreMeter,c:&DraftContextData)->Result<crate::canonical::audit_arena::AuditedValue>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning operation key"))?;
 let store=arena.string_value(&c.key.logical_store_id,m)?;let source=arena.string_value(&c.key.source,m)?;let id=arena.string_value(&c.key.operation_id,m)?;
 let a=arena.string_value(&c.key.scope[0],m)?;let b=arena.string_value(&c.key.scope[1],m)?;let scope=arena.array_from_values(&[a.value(),b.value()],m)?;
 arena.object_from_fields(&[("logical_store_id",store.value()),("scope",scope.value()),("source",source.value()),("operation_id",id.value())],m)
}
fn audit<'a>(c:&DraftContextData,state:&'a EconomicStateData,s:&mut OriginalAuditSession)->Result<Option<&'a str>>{
 match(state.audit,state.observation){(Some(a),Some(o))=>Ok(Some(a.bound_digest(c,o.observation.reference,s)?)),(None,_)=>Ok(None),_=>Err(shape::fail("audit lacks actual financial observation"))}
}
/// Input views remain borrowed through the actual interpreter; returned output
/// can only be owning. This entry does not accept a DraftTransition or credit.
pub fn construct_economic_candidate<'a>(program:&SelectedProgram,c:&'a DraftContextData,payload:&[u8],inputs:&'a [EconomicRecordView<'a>],state:&EconomicStateData,session:&mut OriginalAuditSession)->Result<EconomicCandidate>{
 let result=(||{
  session.check_cancelled()?;let audit=audit(c,state,session)?;
  let mut child=session.meter_mut().original_economic_meter()?;
  let construction=construct::execute_complete(program,c,payload,inputs,state,&mut child)?;
  let assembly=Assembly::derive(&mut child,c,construction)?;
  assembly.require_complete_capacity_and_resources(&mut child)?;
  let digest=audit.ok_or_else(||unfinished("completed AUTH03 witness stream is missing"))?;
  let candidate=assembly.into_complete(c,digest,&mut child)?;
  session.meter_mut().finish_original_economic(child)?;
  Ok(candidate)
 })();if result.is_err(){session.invalidate();}result
}
/// The historical operand partition is independently selected by the caller's
/// completed prefix. N is comparison-only; it never seeds economic inputs.
pub fn reconstruct_original_candidate<'a>(program:&SelectedProgram,saved:&'a SavedFinancialInput<'a>,session:&mut OriginalAuditSession)->Result<EconomicCandidate>{
 let result=(||{
  session.check_cancelled()?;
  let c=saved.original_context;let arena=session.arena().shared_handle();
  let op=session.parse_representation(saved.operation.canonical_body,RepresentationKind::OperationFrame)?;
  super::codec::validate_selected_types(session.meter_mut(),op.value())?;
  let canonical=session.canonical_representation(op.value(),RepresentationKind::OperationFrame)?;
  if !shape::same_bytes(session.meter_mut(),canonical.bytes(),saved.operation.canonical_body)?{return Err(shape::fail("noncanonical original operation"));}
  let m=session.meter_mut();
  shape::closed(m,op.value(),&["schema","key","kind","contract","request_evidence","authority_snapshot","received_at","accepted_at","commit_sequence","result_refs","closure_profile","source_observation_refs","financial_observation_ref","combined_audit_digest"],&[])?;
  same_text(m,op.value(),"schema","ledger-workflow-economic-operation/9")?;
  same_text(m,op.value(),"closure_profile","ledger-economic-resolved-inputs/9")?;same_text(m,op.value(),"contract","workflow/1")?;
  let expected_key=key(m,c)?;shape::equal(m,&op.value()["key"],expected_key.value())?;
  same_text(m,op.value(),"received_at",&c.received_at)?;same_text(m,op.value(),"accepted_at",&c.accepted_at)?;
  super::records::reference(m,saved.operation.reference)?;
  let id=super::body::semantic_id(m,"wop_","workflow-operation",expected_key.value())?;
  if !shape::same_text(m,&id,&saved.operation.reference.id)?{return Err(shape::fail("original operation identity"));}
  let kind=arena.string_value("workflow-operation",m)?;
  let kind=m.canonical_owned_representation(&arena,kind.value(),RepresentationKind::SourceDefinition)?;
  let scope=m.canonical_owned_representation(&arena,&op.value()["key"]["scope"],RepresentationKind::SourceDefinition)?;
  let digest=super::records::hash_parts(m,"workflow-record",&[kind.bytes(),scope.bytes(),canonical.bytes()],true)?;
  if !shape::same_text(m,&digest,&saved.operation.reference.digest)?{return Err(shape::fail("original operation exact digest"));}
  let sequence=shape::text(m,op.value(),"commit_sequence")?;let sequence=crate::policy::workflow::facts::counter_with_work(sequence,m)?;
  if !w::compare(m,sequence,0u64)?.is_gt(){return Err(shape::fail("original positive commit sequence"));}
  let evidence=shape::field(m,op.value(),"request_evidence")?;
  let encoded=shape::text(m,evidence,"original_request_bytes_base64")?;
  let decoded=session.decode_base64_representation(encoded,RepresentationKind::OriginalRequest)?;
  if !shape::same_bytes(session.meter_mut(),decoded.bytes(),saved.original_request_bytes)?{return Err(shape::fail("original raw request bytes"));}
  let hash=session.hash_representation(saved.original_request_bytes,RepresentationKind::OriginalRequest)?;
  let hash=crate::canonical::hex_with_work(&hash,session.meter_mut())?;
  same_text(session.meter_mut(),evidence,"original_request_bytes_sha256",&hash)?;
  let raw=session.parse_representation(saved.original_request_bytes,RepresentationKind::OriginalRequest)?;
  let m=session.meter_mut();super::codec::validate_selected_types(m,raw.value())?;
  shape::closed(m,raw.value(),&["schema","key","kind","contract","payload"],&[])?;
  same_text(m,raw.value(),"schema","ledger-operation/1")?;same_text(m,raw.value(),"contract","workflow/1")?;
  shape::equal(m,&raw.value()["key"],expected_key.value())?;shape::equal(m,&raw.value()["kind"],&op.value()["kind"])?;
  let operation=super::candidate::assembly::operation(m,&raw.value()["payload"])?;
  same_text(m,op.value(),"kind",operation)?;
  let original=c.authorities.first().ok_or_else(||shape::fail("original captured source frame"))?;
  if !shape::same_ref(m,&original.activation_ref,saved.activation_ref)?{return Err(shape::fail("original captured activation"));}
  let registry=program.execution_registry().ok_or_else(||unfinished("original selected recipe is non-admitting"))?;
  same_text(m,&evidence["descriptor_resolution"],"registry_digest",registry)?;
  // Source request/descriptor/manifest/receipt hashes are also recomputed by
  // the chronological host. Exact pure output and original authority equality
  // below do not turn that still-required custody into a caller boolean.
  let payload=session.canonical_representation(&raw.value()["payload"],RepresentationKind::OriginalRequest)?;
  let audit=audit(c,saved.original_state,session)?;
  if let Some(digest)=audit{same_text(session.meter_mut(),op.value(),"combined_audit_digest",digest)?;}
  let mut child=session.meter_mut().original_economic_meter()?;
  let construction=construct::execute_complete(program,c,payload.bytes(),saved.original_inputs,saved.original_state,&mut child)?;
  let assembly=Assembly::derive(&mut child,c,construction)?;
  assembly.require_complete_capacity_and_resources(&mut child)?;
  let audit=audit.ok_or_else(||unfinished("completed original AUTH03 witness stream is missing"))?;
  let candidate=assembly.into_complete(c,audit,&mut child)?;
  session.meter_mut().finish_original_economic(child)?;
  // Only historical N/wrapper comparison belongs to the original parent.
  // The SAME constructor/capacity/owning assembly stays inside the consuming
  // economic child in both fresh and saved entry points.
  compare_saved(session.meter_mut(),c,&candidate,op.value(),evidence,saved.introduced_results)?;
  Ok(candidate)
 })();if result.is_err(){session.invalidate();}result
}
fn compare_saved(m:&mut super::CoreMeter,c:&DraftContextData,a:&EconomicCandidate,op:&serde_json::Value,evidence:&serde_json::Value,saved:&[EconomicRecordView])->Result<()>{
 let arena=m.representation_arena().ok_or_else(||shape::fail("owning original result comparison"))?;
 if !w::compare(m,a.introduced().len(),saved.len())?.is_eq(){return Err(shape::fail("original introduced partition count"));}
 for row in a.introduced(){
  let mut selected=None;
  for retained in saved{m.visit("original.actual_introduction_compare")?;if shape::same_ref(m,row.reference(),retained.reference)?{if selected.is_some(){return Err(shape::fail("ambiguous original result body"));}selected=Some(retained);}}
  let retained=selected.ok_or_else(||shape::fail("original result missing from independently selected N"))?;
  if !shape::same_bytes(m,row.canonical_body(),retained.canonical_body)?{return Err(shape::fail("original result exact canonical body mismatch"));}
  // The candidate established this exact schema while consuming the owning output.
  let value=m.parse_owned_representation(&arena,row.canonical_body(),RepresentationKind::Record)?;same_text(m,value.value(),"schema",row.schema())?;
 }
 let mut authority=None;
 for row in a.introduced(){m.visit("original.actual_authority_result")?;if shape::same_ref(m,row.reference(),a.result_metadata().authority_ref())?{if authority.is_some(){return Err(shape::fail("original authority result ambiguity"));}authority=Some(row);}}
 let authority=authority.ok_or_else(||shape::fail("original authority result missing"))?;
 let authority=m.parse_owned_representation(&arena,authority.canonical_body(),RepresentationKind::Authority)?;
 shape::equal(m,&op["authority_snapshot"],authority.value())?;
 let payload=m.parse_owned_representation(&arena,a.normalized_payload(),RepresentationKind::OriginalRequest)?;
 let expected_key=key(m,c)?;let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-operation/1","contract":"workflow/1"}"#,RepresentationKind::SourceDefinition)?;
 let normalized=arena.object_from_fields(&[("schema",&constants.value()["schema"]),("key",expected_key.value()),("kind",&op["kind"]),("contract",&constants.value()["contract"]),("payload",payload.value())],m)?;
 shape::equal(m,&evidence["normalized_request"],normalized.value())?;
 let n=a.result_metadata().result_refs().len();ValidationWork::arithmetic(m,w::ArithmeticOperation::Multiply,&[&n.into(),&std::mem::size_of::<&ExactRef>().into()])?;
 let size=n.checked_mul(std::mem::size_of::<&ExactRef>()).ok_or_else(||shape::fail("original result ordering workspace"))?;let _owner=arena.reserve_external_payload(size,m)?;
 let mut borrowed=Vec::new();borrowed.try_reserve_exact(n).map_err(|_|shape::fail("original result reference workspace"))?;
 for reference in a.result_metadata().result_refs(){m.visit("original.actual_result_reference")?;borrowed.push(reference);}
 let result=super::app_cost::reference_set(m,&arena,&borrowed)?;shape::equal(m,&op["result_refs"],result.value())?;
 borrowed.clear();
 for pair in a.result_metadata().source_observations(){m.visit("original.actual_source_observation_reference")?;borrowed.push(pair.observation_ref());}
 let sources=super::app_cost::reference_set(m,&arena,&borrowed)?;shape::equal(m,&op["source_observation_refs"],sources.value())?;
 let financial_ref=owning_reference(m,&arena,a.result_metadata().financial_observation_ref())?;shape::equal(m,&op["financial_observation_ref"],financial_ref.value())?;
 Ok(())
}
