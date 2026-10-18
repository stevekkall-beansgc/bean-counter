//! Executed successful typed source-frame callbacks, composed at their actual
//! multiplicities. This is S only. Refusal framing, representation C/bytes,
//! transient ownership, original auditing and the containing executor remain
//! separate obligations; this value cannot open candidate eligibility.
use super::{add,mul,ordered,shape,CoreMeter,SourceFrameBound};
use super::super::super::{body,data::DraftContextData,records::Records,record_body::RecordBody,working_value};
use super::super::super::super::selected_program::SelectedProgram;
use crate::{Result,validation_work::{self as w,ValidationWork}};
use serde_json::Value;

pub(in crate::policy::workflow::transition_constructor) struct SourceCallbackBound {
    pub birth:usize,
    pub grants:usize,
    pub records:usize,
    pub observation:usize,
    pub remaining_callbacks:usize,
    pub timestamp_memo_overhead:usize,
    pub timestamp_reuse_saved:usize,
    pub total:usize,
}
fn sum(m:&mut CoreMeter,parts:&[usize])->Result<usize>{
    let mut result=0;
    for part in parts{m.visit("capacity.source_frame_sum")?;result=add(m,result,*part)?;}
    Ok(result)
}
fn bytes(m:&mut CoreMeter,value:&Value)->Result<usize>{
    m.visit("capacity.source_frame_string")?;
    let value=value.as_str().ok_or_else(||shape::fail("source frame typed string"))?;
    m.scan_bytes(value.len())?;Ok(value.len())
}
fn field_bytes(m:&mut CoreMeter,value:&Value,key:&str)->Result<usize>{
    let value=shape::field(m,value,key)?;bytes(m,value)
}
fn qualified(m:&mut CoreMeter,value:&Value)->Result<usize>{
    let text=value.as_str().ok_or_else(||shape::fail("source frame qualified string"))?;
    m.visit("capacity.source_frame_qualified")?;m.scan_bytes(text.len())?;
    let (namespace,local)=text.split_once(':').ok_or_else(||shape::fail("source frame qualified parts"))?;
    let namespace=mul(m,2,namespace.len())?;
    sum(m,&[15,namespace,local.len()])
}
// Actual successful equal typed-tree comparison. Only the string/container
// grammar used by source ceilings and SourceState is admitted here.
fn same(m:&mut CoreMeter,value:&Value,depth:usize)->Result<usize>{
    m.visit("capacity.source_frame_same_node")?;
    if w::compare(m,depth,32usize)?.is_gt(){return Err(shape::fail("source frame comparison depth"));}
    match value {
        Value::String(_)=>Ok(10),
        Value::Bool(_)=>Ok(7),
        Value::Array(values)=>{
            let mut result=9;
            for value in values{
                m.visit("capacity.source_frame_same_element")?;
                let next=add(m,depth,1)?;let child=same(m,value,next)?;
                let child=add(m,6,child)?;result=add(m,result,child)?;
            }
            Ok(result)
        },
        Value::Object(fields)=>{
            let mut result=9;
            for (_,value) in fields{
                m.visit("capacity.source_frame_same_field")?;
                let next=add(m,depth,1)?;let child=same(m,value,next)?;
                let child=add(m,10,child)?;result=add(m,result,child)?;
            }
            Ok(result)
        },
        _=>Err(shape::fail("source frame comparison grammar")),
    }
}
fn enum_prefix(m:&mut CoreMeter,value:&Value,choices:&[&str])->Result<usize>{
    let value=value.as_str().ok_or_else(||shape::fail("source frame enum string"))?;
    let mut prefix=0;
    for choice in choices {
        m.visit("capacity.source_frame_enum")?;prefix=add(m,prefix,1)?;
        if shape::same_text(m,value,choice)?{return mul(m,4,prefix);}
    }
    Err(shape::fail("source frame closed enum"))
}
fn ceiling_predicates(m:&mut CoreMeter,ceiling:&Value)->Result<usize>{
    let mut result=0;
    for field in ["principal_id","source","canonical_economic_namespace","customer_scope","operations","product_selectors","activity_selectors","link_selectors","source_slots","permissions","starts_at","ends_at"]{
        m.visit("capacity.source_frame_ceiling_copy")?;
        let value=shape::field(m,ceiling,field)?;let cost=same(m,value,0)?;result=add(m,result,cost)?;
    }
    for field in ["customer_scope","operations","source_slots","permissions"]{
        let values=shape::array(m,ceiling,field)?;let mut set=ordered(m,values.len())?;
        for value in values{
            m.visit("capacity.source_frame_original_set")?;
            let validation=match field{
                "customer_scope"=>{let n=bytes(m,value)?;add(m,7,n)?},
                "source_slots"=>qualified(m,value)?,
                "operations"=>enum_prefix(m,value,super::super::super::inventory::source::OPERATIONS)?,
                _=>enum_prefix(m,value,&["read","submit","correct","link","admin","invocation","close","operate"])?
            };
            let member=add(m,1,validation)?;set=add(m,set,member)?;
        }
        result=add(m,result,set)?;
    }
    for field in ["product_selectors","activity_selectors","link_selectors"]{
        let selectors=shape::array(m,ceiling,field)?;let mut set=ordered(m,selectors.len())?;
        for selector in selectors{
            m.visit("capacity.source_frame_original_selector")?;
            let names=shape::array(m,selector,"names")?;
            let names_order=ordered(m,names.len())?;let mut cost=add(m,14,names_order)?;
            for name in names{
                m.visit("capacity.source_frame_original_selector_name")?;
                let q=qualified(m,name)?;let q=add(m,1,q)?;cost=add(m,cost,q)?;
            }
            set=add(m,set,cost)?;
        }
        result=add(m,result,set)?;
    }
    Ok(result)
}
// number(field,true) here has no named19-character guard. Original root
// dimensions are read exactly; the actual future Setup producer is u64 and
// therefore requires up to20 decimal bytes, rather than an assumed19.
fn integer(m:&mut CoreMeter,digits:usize)->Result<usize>{
    let per=mul(m,11,digits)?;add(m,40,per)
}
fn root(m:&mut CoreMeter,value:&Value,time:usize,expected_source:bool)->Result<usize>{
    let basis=shape::field(m,value,"basis")?;
    let source=field_bytes(m,basis,"source")?;
    let namespace=field_bytes(m,basis,"canonical_economic_namespace")?;
    let revision=field_bytes(m,basis,"source_grant_revision")?;
    root_dimensions(m,source,namespace,revision,time,expected_source)
}
fn root_dimensions(m:&mut CoreMeter,source:usize,namespace:usize,digits:usize,time:usize,expected_source:bool)->Result<usize>{
    m.visit("capacity.source_frame_expected_source")?;
    let number=integer(m,digits)?;let times=mul(m,2,time)?;
    // Hbasis is one actual owned canonical hash invocation; its C/bytes and
    // Hex32 byte loop remain separate. No legacy codec is reclassified here.
    sum(m,&[378,source,namespace,if expected_source{11}else{0},number,1,times])
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::super::super::{authority::Terms,data::EconomicRecordView,first_work_tests};
    use crate::{canonical::audit_arena::AuditArena,validation_work::OriginalAuditSession};
    #[test]
    fn original_frame_loads_exact_birth_roots_and_composes_all_callback_terms(){
        // Actual record/shape/canonical consumers over the existing conditional
        // prerequisite data. This proves this derivation's joins and component
        // composition only; no history/source authority or eligibility exists.
        let (program,c,_,records)=first_work_tests::test_inputs();
        let views:Vec<_>=records.iter().map(|r|EconomicRecordView{reference:&r.reference,canonical_body:&r.canonical_body}).collect();
        let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=session.meter_mut().original_economic_meter().unwrap();
        let m=&mut child;
        let mut r=Records::borrowed(m,&arena,&c.key,&views,&[]).unwrap();
        let binding=r.get(m,&c.key,&c.authorities[0].binding_ref,"workflow-binding","ledger-workflow-binding/1").unwrap();
        let accepted=r.get(m,&c.key,&c.authorities[0].accepted_agreement_ref,"workflow-accepted-agreement","ledger-workflow-accepted-agreement/1").unwrap();
        let activation=r.get(m,&c.key,&c.authorities[0].activation_ref,"workflow-activation","ledger-workflow-activation-transition/1").unwrap();
        let catalog=load(m,&mut r,&c,&binding,"slot_catalog_ref","workflow-slot-catalog","ledger-workflow-slot-catalog/1").unwrap();
        let instantiation=load(m,&mut r,&c,&binding,"slot_instantiation_ref","workflow-slot-instantiation","ledger-workflow-slot-instantiation/1").unwrap();
        let slot_terms_ref=shape::reference(m,&binding["bindings"][0]["binding_ref"]).unwrap();
        let slot_terms=r.get(m,&c.key,&slot_terms_ref,"workflow-slot-terms","ledger-workflow-slot-binding-terms/2").unwrap();
        let role_ref=shape::reference(m,&slot_terms["role_ref"]).unwrap();
        let terms=Terms{binding,accepted,activation,catalog,instantiation,slot_terms,slot_terms_ref,role_ref};
        let original=super::super::original_frames(m,&mut r,&c,&terms,&program).unwrap();
        assert_eq!(original.frames.len(),1);assert_eq!(original.terms_grant_count,1);
        let frame=&original.frames[0];let callbacks=frame.complete_callback_work.as_ref().unwrap();
        assert_eq!(callbacks.total,callbacks.birth+callbacks.grants+callbacks.records+callbacks.observation+callbacks.remaining_callbacks+callbacks.timestamp_memo_overhead-callbacks.timestamp_reuse_saved);
        assert!(callbacks.birth>18*frame.grant.timestamp_parse_work_through_original_end);
        assert!(callbacks.grants>2*frame.grant.semantic_work);
        assert!(callbacks.records>frame.maximum_grant_records_work);
        assert!(callbacks.observation>4261+258);
    }
}
fn raw_retained(m:&mut CoreMeter,evidence:&Value,root_cost:usize,time:usize,metadata:usize)->Result<usize>{
    let retainer=field_bytes(m,evidence,"retained_by")?;
    let times=mul(m,2,time)?;
    // Raw RP01 text/binary paths: <=two media alternatives+UTF8 constraint;
    // exact customer subject29, metadata29/49; reserve64 costs19S. The actual
    // execute_complete borrowed-input path has no intrinsic_owner/Qe scan.
    sum(m,&[321,9,retainer,29,metadata,19,root_cost,times])
}
fn guard(m:&mut CoreMeter,value:&Value)->Result<usize>{
    Ok(super::super::app_bytes::immutable_guard(m,value,0)?.0)
}
fn load(m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,value:&Value,field:&str,kind:&str,schema:&str)->Result<RecordBody>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("source frame load owner"))?;
    let value=shape::field(m,value,field)?;
    let reference=working_value::parse_owned_reference(m,&arena,value)?;
    r.get(m,&c.key,reference.reference(),kind,schema)
}

pub(super) fn derive(
    m:&mut CoreMeter,r:&mut Records,c:&DraftContextData,program:&SelectedProgram,
    terms:&super::super::super::authority::Terms,frame:&SourceFrameBound,
    original_grants:usize,original_grant_guards:usize,original_grant_refs:&[super::super::super::data::ExactRef],
)->Result<SourceCallbackBound>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("source frame bound owner"))?;
    // The one identity locator's cloned full Ref and semantic ID coexist.
    // Other temporary Refs below have their own precise owning wrapper.
    let _locator=arena.reserve_external_payload(std::mem::size_of::<super::super::super::data::ExactRef>()+64+128+71+128,m)?;
    let enrollment=r.get(m,&c.key,&frame.enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
    let ceiling=shape::field(m,&enrollment,"ceiling")?;
    let store=arena.string_value(&c.key.logical_store_id,m)?;
    let left=arena.string_value(&c.key.scope[0],m)?;let right=arena.string_value(&c.key.scope[1],m)?;
    let scope=arena.array_from_values(&[left.value(),right.value()],m)?;
    let enrollment_id=arena.clone_value(shape::field(m,&enrollment,"enrollment_id")?,m)?;
    let one=arena.string_value("1",m)?;
    let key=arena.array_from_values(&[store.value(),scope.value(),enrollment_id.value(),one.value()],m)?;
    let id=body::semantic_id(m,"wsource_","workflow-source-state",key.value())?;
    let (_,birth_state)=r.by_identity(m,&c.key,"workflow-source-state",&id,"ledger-workflow-source-state/1")?;
    let birth_grant=load(m,r,c,&birth_state,"current_grant_ref","workflow-source-grant","ledger-workflow-source-grant/1")?;
    let customers=shape::array(m,&enrollment,"customer_refs")?;
    if !w::compare(m,customers.len(),1usize)?.is_eq(){return Err(shape::fail("source frame original customer"));}
    let customer_ref=working_value::parse_owned_reference(m,&arena,&customers[0])?;
    let registered=r.get(m,&c.key,customer_ref.reference(),"workflow-customer","ledger-workflow-customer-registration/1")?;
    let customer_evidence=load(m,r,c,&registered,"evidence_ref","workflow-evidence","ledger-workflow-retained-evidence/1")?;
    let source_evidence=load(m,r,c,&enrollment,"evidence_ref","workflow-evidence","ledger-workflow-retained-evidence/1")?;
    let customer_root=load(m,r,c,&customer_evidence,"authority_ref","workflow-authority","ledger-workflow-local-authority-snapshot/1")?;
    let source_root=load(m,r,c,&source_evidence,"authority_ref","workflow-authority","ledger-workflow-local-authority-snapshot/1")?;
    let registration_root=load(m,r,c,&registered,"authority_ref","workflow-authority","ledger-workflow-local-authority-snapshot/1")?;
    let enrollment_root=load(m,r,c,&enrollment,"authority_ref","workflow-authority","ledger-workflow-local-authority-snapshot/1")?;
    let time=frame.grant.timestamp_parse_work_through_original_end;
    let customer_root_cost=root(m,&customer_root,time,false)?;
    let source_root_cost=root(m,&source_root,time,false)?;
    let registration_root_cost=root(m,&registration_root,time,false)?;
    let enrollment_root_cost=root(m,&enrollment_root,time,false)?;
    let retained_customer=raw_retained(m,&customer_evidence,customer_root_cost,time,29)?;
    let retained_source=raw_retained(m,&source_evidence,source_root_cost,time,49)?;
    let predicates=ceiling_predicates(m,ceiling)?;
    let principal=field_bytes(m,ceiling,"principal_id")?;
    let source=field_bytes(m,ceiling,"source")?;
    let namespace=field_bytes(m,ceiling,"canonical_economic_namespace")?;
    let enrollment_length=field_bytes(m,&enrollment,"enrollment_id")?;
    let acceptor=field_bytes(m,&registered,"acceptor")?;
    let birth_key=shape::field(m,&birth_state,"operation_key")?;
    let key_source=field_bytes(m,birth_key,"source")?;
    let key_operation=field_bytes(m,birth_key,"operation_id")?;
    let birth_key_cost=sum(m,&[112,key_source,key_operation])?;
    let birth_times=mul(m,6,time)?;
    let birth=sum(m,&[2030,predicates,principal,source,namespace,enrollment_length,acceptor,birth_key_cost,birth_times,retained_customer,retained_source,registration_root_cost,enrollment_root_cost,6])?;

    let g=frame.original_grant_count;let t=original_grants;
    let grant_calls=add(m,g,1)?;
    // Exact same Records owner wrappers still execute all association lookups
    // even when shape/intersection facts have already been retained.
    let grant_unit=add(m,frame.grant.semantic_work,814)?;
    let grants=mul(m,grant_calls,grant_unit)?;
    let g258=mul(m,258,g)?;let counters=mul(m,5,255)?;
    let original_order=ordered(m,g)?;
    let mut artifact_paths=0;
    for (path,_) in super::super::super::inventory::observation::SUPPORTED_ARTIFACTS{
        m.visit("capacity.source_frame_artifact_path")?;m.scan_bytes(path.len())?;
        artifact_paths=add(m,artifact_paths,path.len())?;
    }
    let mut observation=sum(m,&[4261,8,g258,counters,original_order,390,1036,15,35,artifact_paths,409])?;
    // The new762 source branch keeps the common source/root/observation joins
    // and replaces the old synthetic3/7 catalog traversal with exact selected
    // bytes. Keep the old common+catalog sum as a conservative overcount,
    // then add the ENTIRE new type guard/comparison. Likewise retain the old
    // first-record guard below and add the entire new guard, rather than
    // claiming the old424-node term describes this different metadata body.
    let selected_catalog_guard=if super::super::super::audit_metadata::selected_catalog(m,program)?{
        let raw=super::super::super::audit_metadata::source_template();
        let value=m.parse_owned_representation(&arena,raw,crate::validation_work::RepresentationKind::SourceDefinition)?;
        let guard=guard(m,value.value())?;
        //4S byte equality;4S registry identity;1S memo catalog identity.
        let selected=add(m,guard,9)?;observation=add(m,observation,selected)?;guard
    }else{0};

    // All old grant bodies are inspected, not just this enrollment's G.
    // Every first body admission runs the actual selected decoded-type guard.
    let mut guards=original_grant_guards;
    for value in [&*enrollment,&*birth_state,&*birth_grant,&*registered,&*customer_evidence,&*source_evidence,&*customer_root,&*source_root,&*registration_root,&*enrollment_root]{
        m.visit("capacity.source_frame_immutable_guard")?;
        let cost=guard(m,value)?;guards=add(m,guards,cost)?;
    }
    let observation_refs=mul(m,39,g)?;let observation_guard=add(m,332,observation_refs)?;
    guards=sum(m,&[guards,227,frame.grant.maximum_decoded_guard_work,observation_guard,424,154,selected_catalog_guard])?;
    let loads=add(m,t,15)?;let loads=mul(m,478,loads)?;
    // No blanket constructor grammar credit: first typed use pays exact
    // match12 + marker check1 + ordinary grammar151 =164S. Already-checked
    // hits cost14S; overcount every call as first use until the containing
    // executor proves its particular repeated-reference multiplicities.
    // Index/body/schema/owner work remains in loads; marker writes add bytes.
    let typed=add(m,t,7)?;let typed=mul(m,164,typed)?;
    let parsed=mul(m,183,8)?;
    let source_parsed=add(m,t,3)?;let source_parsed=mul(m,182,source_parsed)?;
    let records=sum(m,&[loads,typed,parsed,source_parsed,381,guards])?;

    let birth_same=same(m,&birth_state,0)?;let initial=add(m,153,birth_same)?;
    let later=189+11;
    let state_branch=if w::compare(m,initial,later)?.is_gt(){initial}else{later};
    let products=mul(m,70,program.source_documents().len())?;
    let nominations=shape::array(m,&terms.binding,"source_slots")?;
    let nomination_cost=mul(m,12,nominations.len())?;
    let mut maximum_sources=0;
    for nomination in nominations{
        m.visit("capacity.source_frame_nomination")?;
        let sources=shape::array(m,nomination,"sources")?;
        if w::compare(m,sources.len(),maximum_sources)?.is_gt(){maximum_sources=sources.len();}
    }
    let source_membership=mul(m,11,maximum_sources)?;
    let binding_authorities=shape::array(m,&terms.binding,"authority_refs")?;
    let all_authority_prefixes=mul(m,t,binding_authorities.len())?;
    let authority_membership=mul(m,70,all_authority_prefixes)?;
    let grant_matching=mul(m,90,t)?; //11T+10Ts+69Tsp, Ts,Tsp<=T
    let grant_equality=mul(m,150,g)?;
    let direct=sum(m,&[705,state_branch,products,nomination_cost,source_membership,authority_membership,grant_matching,grant_equality])?;
    let slot=205; // existing qualified-name grammar63 namespace/64 local
    let context_grants=mul(m,188,g)?;
    let context=sum(m,&[1358,c.key.logical_store_id.len(),128,principal,c.key.scope[0].len(),c.key.scope[1].len(),slot,context_grants])?;
    let current_root=root_dimensions(m,source,namespace,20,time,true)?;
    let direct_times=mul(m,2,time)?;
    // Actual ref_set still uses legacy codec::encode on this passed meter.
    // Per three-string Ref tree: <=1008S; sort comparisons/moves additional.
    let ref_members=mul(m,1009,g)?;let prior=add(m,g,1)?;
    // g(g-1)/2 <=g(g+1)/2; this conservative variant avoids subtraction and
    // division in the proof itself and safely overcounts all pair visits.
    let ref_pairs=mul(m,g,prior)?;let ref_pairs=mul(m,3,ref_pairs)?;
    let reference_set=add(m,ref_members,ref_pairs)?;
    let remaining_callbacks=sum(m,&[direct,context,632,216,18,19,182,current_root,direct_times,reference_set,1600])?;
    // The token starts empty immediately before source birth. The successful
    // source recipe makes18 birth parses, four per grant and four later parses.
    // Its immutable raw timestamps are inspected here; up to six independent
    // future strings remain (consumer time, current grant endpoints, state,
    // observation and current root). No equality of different future instants
    // is assumed. More than32 distinct possibilities conservatively gets no
    // reuse credit, preserving every legal original composition.
    let stamps=[
        shape::text(m,ceiling,"starts_at")?,shape::text(m,ceiling,"ends_at")?,
        shape::text(m,&birth_state,"accepted_at")?,shape::text(m,&enrollment,"enrolled_at")?,
        shape::text(m,&registered,"registered_at")?,shape::text(m,&customer_evidence,"retained_at")?,
        shape::text(m,&source_evidence,"retained_at")?,shape::text(m,&customer_root,"verified_at")?,
        shape::text(m,&source_root,"verified_at")?,shape::text(m,&registration_root,"verified_at")?,
        shape::text(m,&enrollment_root,"verified_at")?,
    ];
    let mut distinct=0;
    for (i,stamp) in stamps.iter().enumerate(){
        m.visit("capacity.source_timestamp")?;let mut seen=false;
        for prior in &stamps[..i]{m.visit("capacity.source_timestamp_prior")?;if shape::same_text(m,stamp,prior)?{seen=true;break;}}
        if !seen{distinct=add(m,distinct,1)?;}
    }
    let birth_reference_value=shape::field(m,&birth_state,"current_grant_ref")?;
    let birth_reference=working_value::parse_owned_reference(m,&arena,birth_reference_value)?;
    let mut originals_are_birth=true;
    for reference in original_grant_refs {
        m.visit("capacity.source_original_timestamp_identity")?;
        let grant=r.get(m,&c.key,reference,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let original_ceiling_value=shape::field(m,&grant,"original_ceiling_ref")?;
        let original_ceiling=working_value::parse_owned_reference(m,&arena,original_ceiling_value)?;
        if shape::same_ref(m,original_ceiling.reference(),&frame.enrollment_ref)?
            && !shape::same_ref(m,reference,birth_reference.reference())? {originals_are_birth=false;}
    }
    let original_extra=if originals_are_birth{0}else{mul(m,2,g)?};
    let unique=sum(m,&[distinct,6,original_extra])?;
    let calls=mul(m,4,grant_calls)?;let calls=add(m,22,calls)?;
    // GrantBound already prepays its own four lookups. Pay all remaining22
    // lookups here, including the token's actual first reservation.
    let memo_lookups=mul(m,22,72)?;let timestamp_memo_overhead=add(m,memo_lookups,20)?;
    let timestamp_reuse_saved=if w::compare(m,unique,32usize)?.is_le(){
        let reused=w::usize_sub(m,calls,unique)?;mul(m,reused,time)?
    }else{0};
    let total=sum(m,&[birth,grants,records,observation,remaining_callbacks,timestamp_memo_overhead])?;
    let total=w::usize_sub(m,total,timestamp_reuse_saved)?;
    Ok(SourceCallbackBound{birth,grants,records,observation,remaining_callbacks,timestamp_memo_overhead,timestamp_reuse_saved,total})
}
