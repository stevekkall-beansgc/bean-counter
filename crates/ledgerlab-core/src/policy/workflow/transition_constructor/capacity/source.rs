//! Typed successful-grant S bound from the immutable ORIGINAL enrollment.
//! This is one callee of the future executor, not its whole resource proof.
//! Malformed refusal prefixes, source birth/observation, codec bytes/arena and
//! original custody remain separate. No serialized maximum is trusted here.
use super::super::{CoreMeter,shape};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use serde_json::Value;
mod frame;
pub(in crate::policy::workflow::transition_constructor) use frame::SourceCallbackBound;

pub(in crate::policy::workflow::transition_constructor) struct GrantBound {
    pub semantic_work:usize,
    pub timestamp_parse_work_through_original_end:usize,
    pub maximum_product_selectors:usize,
    pub maximum_activity_selectors:usize,
    pub maximum_link_selectors:usize,
    pub maximum_decoded_guard_work:usize,
}
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||shape::fail("source bound product"))
}
fn bounded(m:&mut CoreMeter,n:usize,min:usize,max:usize)->Result<()> {
    if w::compare(m,n,min)?.is_lt()||w::compare(m,n,max)?.is_gt(){return Err(shape::fail("original source bound dimension"));}Ok(())
}
// RP01 strict canonical ordering executes its encoder in C and at most4S for
// each adjacent byte_order. The per-member semantic visit remains S. The
// redundant all-prior structural duplicate pass is absent in this exact path.
fn ordered(m:&mut CoreMeter,n:usize)->Result<usize>{mul(m,5,n)}
struct Selectors {maximum:usize,names:usize,qualified:usize,original_search:usize}
fn selectors(m:&mut CoreMeter,ceiling:&Value,key:&str,limit:usize)->Result<Selectors>{
    let original=shape::array(m,ceiling,key)?;bounded(m,original.len(),0,limit)?;
    let mut total_names=0usize;let mut qualified=0usize;let mut search=0usize;
    for selector in original {
        m.visit("capacity.source_original_selector")?;
        shape::closed(m,selector,&["names"],&[])?;
        let names=shape::array(m,selector,"names")?;bounded(m,names.len(),1,64)?;
        total_names=add(m,total_names,names.len())?;
        let prefix=mul(m,11,names.len())?;let original=add(m,8,prefix)?;search=add(m,search,original)?;
        for name in names {
            m.visit("capacity.source_original_name")?;
            let raw=name.as_str().ok_or_else(||shape::fail("original selector name"))?;
            m.scan_bytes(raw.len())?;
            let (namespace,local)=raw.split_once(':').ok_or_else(||shape::fail("original qualified selector name"))?;
            bounded(m,namespace.len(),1,63)?;bounded(m,local.len(),1,64)?;
            let ns=mul(m,2,namespace.len())?;let q=add(m,15,ns)?;let q=add(m,q,local.len())?;
            if w::compare(m,q,qualified)?.is_gt(){qualified=q;}
        }
    }
    // A future explicit names selector is a NONEMPTY subset of the original
    // union. Sum of name counts overcounts that union safely. Count distinct
    // subsets algebraically, saturating only at the EXISTING grammar bound;
    // neither current grant nor a chosen fixture narrows the accepted rights.
    let mut distinct=0usize;
    for _ in 0..total_names {
        m.visit("capacity.source_subset_dimension")?;
        if !w::compare(m,distinct,limit)?.is_lt(){break;}
        let doubled=mul(m,distinct,2)?;let next=add(m,doubled,1)?;
        distinct=if w::compare(m,next,limit)?.is_gt(){limit}else{next};
    }
    let names=if w::compare(m,total_names,64usize)?.is_gt(){64}else{total_names};
    Ok(Selectors{maximum:distinct,names,qualified,original_search:search})
}
fn selector_subset(m:&mut CoreMeter,s:&Selectors)->Result<usize>{
    let names_order=ordered(m,s.names)?;
    let each_name=add(m,s.qualified,s.original_search)?;
    let all_names=mul(m,s.names,each_name)?;
    let selector=add(m,14,names_order)?;let selector=add(m,selector,all_names)?;
    let all=mul(m,s.maximum,selector)?;
    let outer=ordered(m,s.maximum)?;let all=add(m,outer,all)?;add(m,8,all) //6 bounds +2 caller fields
}
fn coverage(m:&mut CoreMeter,s:&Selectors)->Result<usize>{
    let names=mul(m,11,s.names)?;let one=add(m,7,names)?;
    let all=mul(m,s.maximum,one)?;add(m,1,all)
}
// SAME codec::validate_selected_types:4 per Value,4 per child edge,
// at most5 per scalar. Grant counters/endpoints are JSON strings. These
// dimensions cover all future legal regroupings of the original names;
// current grant syntax is not used to narrow a promised source frame.
fn decoded_array(m:&mut CoreMeter,n:usize,child:usize)->Result<usize>{
    let per=add(m,4,child)?;let children=mul(m,n,per)?;add(m,4,children)
}
fn decoded_grant(m:&mut CoreMeter,lengths:&[usize;4],selectors:[&Selectors;3])->Result<usize>{
    // Sixteen closed fields:12 original ceiling fields and the existing
    // schema/grant_id/revision/original_ceiling_ref header. Eight string
    // scalars, one exact three-string Ref, four scalar arrays, three selectors.
    let mut bound=4+16*4+8*5+35;
    for n in lengths {let array=decoded_array(m,*n,5)?;bound=add(m,bound,array)?;}
    for selector in selectors {
        let names=decoded_array(m,selector.names,5)?;
        let object=add(m,8,names)?; // {names:array}
        let selected=decoded_array(m,selector.maximum,object)?;
        bound=add(m,bound,selected)?;
    }
    Ok(bound)
}
fn distinct_string_subset(m:&mut CoreMeter,n:usize)->Result<usize>{
    // Both the immutable original set and the future selected set are
    // already proved distinct. Each selected string therefore consumes a
    // DIFFERENT original membership prefix. Across every permitted subset,
    // the sum is <=1+...+n, not n independent worst-position matches.
    // This changes no executing validator or paid callback classification.
    let successor=add(m,n,1)?;let twice=mul(m,n,successor)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::DivideRemainder,&[&twice.into(),&2usize.into()])?;
    let prefixes=twice/2;
    let comparisons=mul(m,11,prefixes)?;let visits=add(m,n,comparisons)?;add(m,2,visits)
}
// The Gregorian renderer starts at floor(day/366)+1. For UTC year Y its
// lag is <=ceil((Y-1)/366), because day>=365*(Y-1). This bounds the SAME
// parser's loop from an actual immutable interval, without changing callbacks
// or assuming the proof happens in a particular calendar year.
// The universal valid parser bound is L+1802 here: in each monotone month
// loop February appears at most once. This replaces render month539 by179
// and input month accumulation311 by131 in the prior L+2342 ledger.
fn time_bound(m:&mut CoreMeter,ceiling:&Value)->Result<usize>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning source time bound"))?;
    let _time=arena.reserve_external_payload(std::mem::size_of::<crate::domain::Timestamp>()+27,m)?;
    let raw=shape::text(m,ceiling,"ends_at")?;
    let end=crate::domain::Timestamp::parse_with_work(raw,m)?;
    let mut year=0usize;
    for digit in end.as_str().as_bytes().iter().take(4){
        m.scan_bytes(1)?;year=mul(m,year,10)?;
        ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&(*digit).into(),&b'0'.into()])?;
        year=add(m,year,usize::from(*digit-b'0'))?;
    }
    ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&year.into(),&1usize.into()])?;
    let prior=year.checked_sub(1).ok_or_else(||shape::fail("source time bound year"))?;
    let numerator=add(m,prior,365)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::DivideRemainder,&[&numerator.into(),&366usize.into()])?;
    let lag=numerator/366;
    ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&28usize.into(),&lag.into()])?;
    let saved_loops=28usize.checked_sub(lag).ok_or_else(||shape::fail("source time bound calendar"))?;
    let saved=mul(m,saved_loops,33)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&(32usize+1802).into(),&saved.into()])?;
    (32usize+1802).checked_sub(saved).ok_or_else(||shape::fail("source time bound subtraction"))
}
/// Caller already validates the completed original ceiling. This derivation
/// additionally checks the dimensions it uses; it is not another grant parser
/// or an origin/authentication capability.
pub(super) fn derive(m:&mut CoreMeter,ceiling:&Value)->Result<GrantBound>{
    if m.resource_vector_usage().is_none(){return Err(shape::fail("source bound requires exact RP01 ordering path"));}
    let mut total=627usize;
    let mut lengths=[0usize;4];
    for(i,(name,min,max))in[("customer_scope",1,64),("operations",1,64),("source_slots",0,32),("permissions",1,8)].into_iter().enumerate(){
        let values=shape::array(m,ceiling,name)?;bounded(m,values.len(),min,max)?;let n=values.len();lengths[i]=n;
        let order=ordered(m,n)?;let current=add(m,7,order)?;
        let subset=distinct_string_subset(m,n)?;
        total=add(m,total,current)?;total=add(m,total,subset)?;
    }
    let product=selectors(m,ceiling,"product_selectors",64)?;
    let activity=selectors(m,ceiling,"activity_selectors",64)?;
    let link=selectors(m,ceiling,"link_selectors",32)?;
    for selected in [&product,&activity,&link]{let n=selector_subset(m,selected)?;total=add(m,total,n)?;}
    // All four valid grant/ceiling endpoints are <=this ORIGINAL end.
    // Invalid out-of-interval endpoints still need the separate refusal bound.
    let timestamp_bound=time_bound(m,ceiling)?;
    // Four memo lookups: <=72S each plus the one19S reservation;
    //320 conservatively includes that setup even if already initialized.
    let times=mul(m,4,timestamp_bound)?;let times=add(m,times,18+5+320)?;total=add(m,total,times)?;
    let memberships=add(m,lengths[0],lengths[1])?;let rights=mul(m,2,lengths[3])?;
    let memberships=add(m,memberships,rights)?;let memberships=add(m,memberships,lengths[2])?;
    let queries=mul(m,11,memberships)?;total=add(m,total,queries)?;
    for selected in [&activity,&product]{let n=coverage(m,selected)?;total=add(m,total,n)?;}
    let maximum_decoded_guard_work=decoded_grant(m,&lengths,[&product,&activity,&link])?;
    Ok(GrantBound{semantic_work:total,timestamp_parse_work_through_original_end:timestamp_bound,maximum_product_selectors:product.maximum,maximum_activity_selectors:activity.maximum,maximum_link_selectors:link.maximum,maximum_decoded_guard_work})
}

pub(in crate::policy::workflow::transition_constructor) struct SourceFrameBound {
    pub enrollment_ref:super::super::data::ExactRef,
    pub grant:GrantBound,
    pub original_grant_count:usize,
    pub maximum_uncached_grant_predicate_work:usize,
    pub immutable_decoded_guard_work:usize,
    pub original_enrollment_decoded_guard_work:usize,
    pub maximum_grant_records_work:usize,
    pub complete_callback_work:Option<SourceCallbackBound>,
}
pub(in crate::policy::workflow::transition_constructor) struct OriginalSourceBounds {
    pub frames:Vec<SourceFrameBound>,
    pub terms_grant_count:usize,
    pub distinct_original_grants:usize,
    _ownership:crate::canonical::audit_arena::ExternalBufferLease,
}
/// Derive the source dimensions from every actual ORIGINAL Terms grant, not
/// only the current actor's matching grants. The complete future frame loads
/// all of them before choosing its independently authorized principal/source.
pub(super) fn original_frames(
    m:&mut CoreMeter,r:&mut super::super::records::Records,c:&super::super::data::DraftContextData,
    terms:&super::super::authority::Terms,program:&super::super::super::selected_program::SelectedProgram,
)->Result<OriginalSourceBounds>{
    // Every original accepted binding slot can supply a promised original
    // family/root/app executor. Do not assume the currently priced retail
    // slot's grant list represents those independent original compositions.
    // Taking the full accepted Binding is conservative for unrelated slots.
    let binding_slots=shape::array(m,&terms.binding,"bindings")?;bounded(m,binding_slots.len(),1,64)?;
    let maximum_refs=mul(m,binding_slots.len(),32)?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning original source bounds"))?;
    let per=std::mem::size_of::<SourceFrameBound>()+64+128+71;
    let frame_payload=mul(m,maximum_refs,per)?;
    let ref_per=std::mem::size_of::<super::super::data::ExactRef>()+64+128+71;
    let references_payload=mul(m,maximum_refs,ref_per)?;
    let payload=add(m,frame_payload,references_payload)?;
    let payload=add(m,payload,std::mem::size_of::<OriginalSourceBounds>()+std::mem::size_of::<Vec<super::super::data::ExactRef>>())?;
    // One binding-slot Ref, one grant Ref, and one enrollment Ref coexist
    // with the retained union. Each copy is admitted before shape::reference.
    let transient=3*(std::mem::size_of::<super::super::data::ExactRef>()+64+128+71);
    let payload=add(m,payload,transient)?;
    let ownership=arena.reserve_external_payload(payload,m)?;
    let mut references:Vec<super::super::data::ExactRef>=Vec::new();
    references.try_reserve_exact(maximum_refs).map_err(|_|shape::fail("original grant union allocation"))?;
    let mut maximum_terms_grants=0;let mut maximum_terms_grant_guards=0;let mut selected_slot_present=false;
    for binding_slot in binding_slots{
        m.visit("capacity.original_binding_source_slot")?;
        let reference=shape::reference(m,&binding_slot["binding_ref"])?;
        if shape::same_ref(m,&reference,&terms.slot_terms_ref)?{selected_slot_present=true;}
        let slot=r.get_schemas(m,&c.key,&reference,"workflow-slot-terms",&[
            "ledger-workflow-slot-binding-terms/1","ledger-workflow-slot-binding-terms/2",
            "ledger-workflow-slot-binding-terms/3","ledger-workflow-slot-binding-terms/4",
            "ledger-workflow-slot-binding-terms/5","ledger-workflow-slot-binding-terms/6",
            "ledger-workflow-slot-binding-terms/7",
        ])?;
        shape::equal(m,&slot["slot"],&binding_slot["slot"])?;
        for field in ["customer","agreement_id"]{m.visit("capacity.original_terms_subject")?;shape::equal(m,&slot[field],&terms.binding[field])?;}
        let scope=shape::array(m,&slot,"scope")?;bounded(m,scope.len(),2,2)?;
        for (value,expected) in scope.iter().zip(&c.key.scope){
            m.visit("capacity.original_terms_scope")?;
            let value=value.as_str().ok_or_else(||shape::fail("original source terms scope"))?;
            if !shape::same_text(m,value,expected)?{return Err(shape::fail("original source terms scope association"));}
        }
        let grants=shape::array(m,&slot,"source_grant_refs")?;bounded(m,grants.len(),1,32)?;
        if w::compare(m,grants.len(),maximum_terms_grants)?.is_gt(){maximum_terms_grants=grants.len();}
        let mut term_guards=0;
        for value in grants{
            m.visit("capacity.original_binding_grant_union")?;
            let reference=shape::reference(m,value)?;let mut present=false;
            let grant=r.get(m,&c.key,&reference,"workflow-source-grant","ledger-workflow-source-grant/1")?;
            let (decoded_guard,_)=super::app_bytes::immutable_guard(m,&grant,0)?;
            term_guards=add(m,term_guards,decoded_guard)?;
            for old in &references{m.visit("capacity.original_binding_grant_identity")?;if shape::same_ref(m,old,&reference)?{present=true;break;}}
            if !present{references.push(reference);}
        }
        if w::compare(m,term_guards,maximum_terms_grant_guards)?.is_gt(){maximum_terms_grant_guards=term_guards;}
    }
    if !selected_slot_present{return Err(shape::fail("priced original terms absent from accepted Binding"));}
    let mut frames:Vec<SourceFrameBound>=Vec::new();frames.try_reserve_exact(references.len()).map_err(|_|shape::fail("original source bound allocation"))?;
    for item in &references {
        m.visit("capacity.original_source_grant")?;
        let grant=r.get(m,&c.key,item,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let (original_guard,_)=super::app_bytes::immutable_guard(m,&grant,0)?;
        let enrollment_ref=shape::reference(m,&grant["original_ceiling_ref"])?;
        let mut selected=None;
        for(i,frame)in frames.iter().enumerate(){m.visit("capacity.source_frame_union")?;if shape::same_ref(m,&frame.enrollment_ref,&enrollment_ref)?{selected=Some(i);break;}}
        if let Some(index)=selected {
            frames[index].original_grant_count=add(m,frames[index].original_grant_count,1)?;
            frames[index].immutable_decoded_guard_work=add(m,frames[index].immutable_decoded_guard_work,original_guard)?;
        }else{
            let enrollment=r.get(m,&c.key,&enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
            let ceiling=shape::field(m,&enrollment,"ceiling")?;let bound=derive(m,ceiling)?;
            let (enrollment_guard,_)=super::app_bytes::immutable_guard(m,&enrollment,0)?;
            let immutable_decoded_guard_work=add(m,original_guard,enrollment_guard)?;
            frames.push(SourceFrameBound{enrollment_ref,grant:bound,original_grant_count:1,maximum_uncached_grant_predicate_work:0,immutable_decoded_guard_work,original_enrollment_decoded_guard_work:enrollment_guard,maximum_grant_records_work:0,complete_callback_work:None});
        }
    }
    for frame in &mut frames{
        m.visit("capacity.source_frame_grant_multiplicity")?;
        // Current grant once plus every originally nominated matching grant.
        // Grouping by enrollment overcounts if some principals do not match;
        // it never omits a legal current principal under that original ceiling.
        // One executing Terms list is at most32 even when several accepted
        // slots refer to distinct grants under this same enrollment.
        if w::compare(m,frame.original_grant_count,maximum_terms_grants)?.is_gt(){frame.original_grant_count=maximum_terms_grants;}
        let calls=add(m,frame.original_grant_count,1)?;
        frame.maximum_uncached_grant_predicate_work=mul(m,calls,frame.grant.semantic_work)?;
        // Source-authority loads ALL T original grants, not only this
        // enrollment's matching subset. Add selected enrollment/current grant.
        // First typed use pays164S: exact match12 + marker check1 + paid
        // grammar151. No constructor grammar is assumed. Already-established
        // same-table hits pay14S, but all calls are conservatively first-use
        // here. Get_grammatical<=98+guard+index; index<=13*29+3=380.
        // 642=164+98+380. Marker byte writes and first decoded guards remain
        // separate; no parent semantic cache or authority work is reused.
        let loads=add(m,maximum_terms_grants,2)?;
        let record_controls=mul(m,loads,642)?;
        let guards=add(m,maximum_terms_grant_guards,frame.original_enrollment_decoded_guard_work)?;
        let guards=add(m,guards,frame.grant.maximum_decoded_guard_work)?;
        frame.maximum_grant_records_work=add(m,record_controls,guards)?;
        frame.complete_callback_work=Some(frame::derive(m,r,c,program,terms,frame,maximum_terms_grants,maximum_terms_grant_guards,&references)?);
    }
    Ok(OriginalSourceBounds{frames,terms_grant_count:maximum_terms_grants,distinct_original_grants:references.len(),_ownership:ownership})
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::super::{authority::{self,EconomicMode},data::*,representation::InvocationToken};
    use crate::{canonical::{self,audit_arena::AuditArena},validation_work::OriginalAuditSession,domain::Timestamp};
    use serde_json::json;
    #[test]
    fn original_two_names_cover_all_legal_overlapping_current_regroupings(){
        // Predicate/bound-only test: no source origin, custody, monetary
        // admission, capacity certificate or financial workload is asserted.
        let ceiling=json!({"principal_id":"principal","source":"source","canonical_economic_namespace":"namespace","customer_scope":["customer"],"operations":["outcome.correct","work.accept"],"product_selectors":[{"names":["test:product"]}],"activity_selectors":[{"names":["test:a","test:b"]}],"link_selectors":[],"source_slots":["test:worker"],"permissions":["read","submit"],"starts_at":"2026-10-17T00:00:00.000000Z","ends_at":"2026-10-25T00:00:00.000000Z"});
        let arena=AuditArena::new().unwrap();let mut derive_session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let bound=derive(derive_session.meter_mut(),&ceiling).unwrap();
        assert_eq!(bound.maximum_activity_selectors,3);assert_eq!(bound.maximum_product_selectors,1);assert_eq!(bound.maximum_link_selectors,0);
        let selectors=[json!({"names":["test:a"]}),json!({"names":["test:b"]}),json!({"names":["test:a","test:b"]})];
        let context=DraftContextData{key:OperationKey{logical_store_id:"store".into(),scope:["org".into(),"scope".into()],source:"source".into(),operation_id:"work".into()},actor:"principal".into(),canonical_economic_namespace:"namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-18T00:00:00.000000Z".into(),accepted_at:"2026-10-18T00:00:00.000000Z".into(),authorities:vec![],source_observation_inputs:vec![]};
        let at=Timestamp::parse(&context.accepted_at).unwrap().micros();
        for mask in 1u8..8{
            let mut chosen=Vec::new();for(i,v)in selectors.iter().enumerate(){if mask&(1<<i)!=0{chosen.push(v.clone());}}
            chosen.sort_by_key(|v|canonical::CanonicalBytes::from_value(v).unwrap().into_vec());
            let mut grant=ceiling.clone();grant["activity_selectors"]=json!(chosen);
            grant["schema"]=json!("ledger-workflow-source-grant/1");grant["grant_id"]=json!("enrollment");grant["revision"]=json!("2");grant["original_ceiling_ref"]=json!({"kind":"workflow-source-enrollment","id":"enrollment","digest":format!("sha256:{}","1".repeat(64))});
            let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();let mut m=session.meter_mut().original_economic_meter().unwrap();
            let mut guard=CoreMeter::new(65536).unwrap();super::super::super::codec::validate_selected_types(&mut guard,&grant).unwrap();
            assert!(guard.usage().semantic_work<=bound.maximum_decoded_guard_work as u64);
            let result=authority::grant(&mut m,&InvocationToken::new(),&grant,&ceiling,&context,&json!("customer"),"test:a","test:product","test:worker",&at.into(),EconomicMode::Work);
            if mask==2{assert!(result.is_err());}else{result.unwrap();assert!(m.usage().semantic_work<=bound.semantic_work as u64);session.meter_mut().finish_original_economic(m).unwrap();}
        }
    }
}
