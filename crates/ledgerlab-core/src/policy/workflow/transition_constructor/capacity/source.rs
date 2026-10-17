//! Typed successful-grant S bound from the immutable ORIGINAL enrollment.
//! This is one callee of the future executor, not its whole resource proof.
//! Malformed refusal prefixes, source birth/observation, codec bytes/arena and
//! original custody remain separate. No serialized maximum is trusted here.
use super::super::{CoreMeter,shape};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use serde_json::Value;

pub(in crate::policy::workflow::transition_constructor) struct GrantBound {
    pub semantic_work:usize,
    pub timestamp_parse_work_through_original_end:usize,
    pub maximum_product_selectors:usize,
    pub maximum_activity_selectors:usize,
    pub maximum_link_selectors:usize,
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
        let membership=mul(m,11,n)?;let member=add(m,1,membership)?;let subset=mul(m,n,member)?;let subset=add(m,2,subset)?;
        total=add(m,total,current)?;total=add(m,total,subset)?;
    }
    let product=selectors(m,ceiling,"product_selectors",64)?;
    let activity=selectors(m,ceiling,"activity_selectors",64)?;
    let link=selectors(m,ceiling,"link_selectors",32)?;
    for selected in [&product,&activity,&link]{let n=selector_subset(m,selected)?;total=add(m,total,n)?;}
    // All four valid grant/ceiling endpoints are <=this ORIGINAL end.
    // Invalid out-of-interval endpoints still need the separate refusal bound.
    let timestamp_bound=time_bound(m,ceiling)?;
    let times=mul(m,4,timestamp_bound)?;let times=add(m,times,18+5)?;total=add(m,total,times)?;
    let memberships=add(m,lengths[0],lengths[1])?;let rights=mul(m,2,lengths[3])?;
    let memberships=add(m,memberships,rights)?;let memberships=add(m,memberships,lengths[2])?;
    let queries=mul(m,11,memberships)?;total=add(m,total,queries)?;
    for selected in [&activity,&product]{let n=coverage(m,selected)?;total=add(m,total,n)?;}
    Ok(GrantBound{semantic_work:total,timestamp_parse_work_through_original_end:timestamp_bound,maximum_product_selectors:product.maximum,maximum_activity_selectors:activity.maximum,maximum_link_selectors:link.maximum})
}

pub(in crate::policy::workflow::transition_constructor) struct SourceFrameBound {
    pub enrollment_ref:super::super::data::ExactRef,
    pub grant:GrantBound,
    pub original_grant_count:usize,
    pub maximum_uncached_grant_predicate_work:usize,
}
pub(in crate::policy::workflow::transition_constructor) struct OriginalSourceBounds {
    pub frames:Vec<SourceFrameBound>,
    pub terms_grant_count:usize,
    _ownership:crate::canonical::audit_arena::ExternalBufferLease,
}
/// Derive the source dimensions from every actual ORIGINAL Terms grant, not
/// only the current actor's matching grants. The complete future frame loads
/// all of them before choosing its independently authorized principal/source.
pub(super) fn original_frames(
    m:&mut CoreMeter,r:&mut super::super::records::Records,c:&super::super::data::DraftContextData,
    terms:&Value,
)->Result<OriginalSourceBounds>{
    let references=shape::array(m,terms,"source_grant_refs")?;bounded(m,references.len(),1,32)?;
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning original source bounds"))?;
    let per=std::mem::size_of::<SourceFrameBound>()+64+128+71;
    let payload=mul(m,references.len(),per)?;let payload=add(m,payload,std::mem::size_of::<OriginalSourceBounds>())?;
    // One temporary grant Ref plus a duplicate enrollment Ref coexist with
    // every retained row on a repeated enrollment. Both are paid before parse.
    let transient=2*(std::mem::size_of::<super::super::data::ExactRef>()+64+128+71);
    let payload=add(m,payload,transient)?;
    let ownership=arena.reserve_external_payload(payload,m)?;
    let mut frames:Vec<SourceFrameBound>=Vec::new();frames.try_reserve_exact(references.len()).map_err(|_|shape::fail("original source bound allocation"))?;
    for item in references {
        m.visit("capacity.original_source_grant")?;
        let reference=shape::reference(m,item)?;
        let grant=r.get(m,&c.key,&reference,"workflow-source-grant","ledger-workflow-source-grant/1")?;
        let enrollment_ref=shape::reference(m,&grant["original_ceiling_ref"])?;
        let mut selected=None;
        for(i,frame)in frames.iter().enumerate(){m.visit("capacity.source_frame_union")?;if shape::same_ref(m,&frame.enrollment_ref,&enrollment_ref)?{selected=Some(i);break;}}
        if let Some(index)=selected {
            frames[index].original_grant_count=add(m,frames[index].original_grant_count,1)?;
        }else{
            let enrollment=r.get(m,&c.key,&enrollment_ref,"workflow-source-enrollment","ledger-workflow-source-enrollment/1")?;
            let ceiling=shape::field(m,&enrollment,"ceiling")?;let bound=derive(m,ceiling)?;
            frames.push(SourceFrameBound{enrollment_ref,grant:bound,original_grant_count:1,maximum_uncached_grant_predicate_work:0});
        }
    }
    for frame in &mut frames{
        m.visit("capacity.source_frame_grant_multiplicity")?;
        // Current grant once plus every originally nominated matching grant.
        // Grouping by enrollment overcounts if some principals do not match;
        // it never omits a legal current principal under that original ceiling.
        let calls=add(m,frame.original_grant_count,1)?;
        frame.maximum_uncached_grant_predicate_work=mul(m,calls,frame.grant.semantic_work)?;
    }
    Ok(OriginalSourceBounds{frames,terms_grant_count:references.len(),_ownership:ownership})
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
        let ceiling=json!({"principal_id":"principal","source":"source","canonical_economic_namespace":"namespace","customer_scope":["customer"],"operations":["work.accept"],"product_selectors":[{"names":["test:product"]}],"activity_selectors":[{"names":["test:a","test:b"]}],"link_selectors":[],"source_slots":["test:worker"],"permissions":["read","submit"],"starts_at":"2026-10-17T00:00:00.000000Z","ends_at":"2026-10-25T00:00:00.000000Z"});
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
            let result=authority::grant(&mut m,&InvocationToken::new(),&grant,&ceiling,&context,&json!("customer"),"test:a","test:product","test:worker",&at.into(),EconomicMode::Work);
            if mask==2{assert!(result.is_err());}else{result.unwrap();assert!(m.usage().semantic_work<=bound.semantic_work as u64);session.meter_mut().finish_original_economic(m).unwrap();}
        }
    }
}
