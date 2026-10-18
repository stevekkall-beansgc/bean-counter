//! Authority-backlinked active app history is immediate input, even though
//! the separately qualified prior_ref edges are historical. This accounts
//! that growing branch; it is not a whole-I or original-audit certificate.
use super::{original::OriginalReservations,super::{CoreMeter,shape}};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork}};

pub(in crate::policy::workflow::transition_constructor) struct AppHistory {
    pub maximum_versions_per_claim:usize,
    pub maximum_total_versions:usize,
    pub records:usize,
    pub body_bytes:usize,
    pub selected_guard_work:usize,
}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||shape::fail("app history product"))
}
fn ref_floor(m:&mut CoreMeter,kind:&str,prefix:&str)->Result<usize>{
    // Exact canonical {digest,id,kind}:25 punctuation/key bytes, quoted
    // sha256:64, quoted prefix+64, and quoted fixed kind. No invented Ref is
    // materialized or admitted by this byte-floor calculation.
    let id=w::usize_add(m,prefix.len(),64+2)?;
    let kind=w::usize_add(m,kind.len(),2)?;
    let n=w::usize_add(m,25+73,id)?;w::usize_add(m,n,kind)
}
fn cumulative_floor(m:&mut CoreMeter,n:usize,triples:usize)->Result<usize>{
    // Manifest i contains i triples; its NEW FinancialHistoryObservation3
    // preexisting_input_refs contains the i-1 prior triples. The latter is
    // unique per actual pending operation key and its canonical BODY is also
    // charged in WB. Thus sum(i+i-1)=n², without counting any wrapper twice.
    let square=mul(m,n,n)?;mul(m,square,triples)
}
fn versions(m:&mut CoreMeter,claims:usize)->Result<usize>{
    let r=ref_floor(m,"workflow-cost","wdoc_")?;
    let d=ref_floor(m,"workflow-decision","wdrev_")?;
    let a=ref_floor(m,"workflow-economic-authority","wdoc_")?;
    let triples=w::usize_add(m,r,d)?;let triples=w::usize_add(m,triples,a)?;
    // Every successive cost manifest contains its own R/D/A and all earlier
    // triples; its financial header separately lists all preexisting triples.
    // Their cumulative bytes alone consume WB. Ignoring setup, evidence
    // retention, other header fields/bodies and legacy pools only enlarges
    // this upper bound. The original correction duration supplies no count.
    // Histories share the SAME WB: sum(n_i²)>=sum(n_i)²/A. Comparing after
    // multiplication avoids rounding the rational lower bound downward. The
    // existing workflow-operation count bounds the TOTAL, not each claim.
    let available=mul(m,268435456usize,claims)?;
    let(mut low,mut high)=(0usize,100000usize);
    while w::compare(m,low,high)?.is_lt(){
        let sum=w::usize_add(m,low,high)?;let sum=w::usize_add(m,sum,1)?;
        ValidationWork::arithmetic(m,ArithmeticOperation::DivideRemainder,&[&sum.into(),&2usize.into()])?;
        let middle=sum/2;
        let bytes=cumulative_floor(m,middle,triples)?;
        if w::compare(m,bytes,available)?.is_gt(){
            ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&middle.into(),&1usize.into()])?;
            high=middle.checked_sub(1).ok_or_else(||shape::fail("app history search"))?;
        }else{low=middle;}
    }
    Ok(low)
}
pub(in crate::policy::workflow::transition_constructor) fn derive(m:&mut CoreMeter,original:&OriginalReservations)->Result<AppHistory>{
    if w::compare(m,original.app_costs.len(),0usize)?.is_eq(){return Ok(AppHistory{maximum_versions_per_claim:0,maximum_total_versions:0,records:0,body_bytes:0,selected_guard_work:0});}
    let app=original.app_body_bytes.as_ref().ok_or_else(||shape::fail("app history body constructors missing"))?;
    let mut source_bytes=0;let mut source_guards=0;
    for frame in &original.source_bounds.frames{
        m.visit("capacity.history_original_source_frame")?;
        let source=frame.changing_bodies.as_ref().ok_or_else(||shape::fail("app history source bodies missing"))?;
        let bytes=w::usize_add(m,source.current_grant,source.current_state)?;let bytes=w::usize_add(m,bytes,source.observation)?;
        let guards=w::usize_add(m,frame.grant.maximum_decoded_guard_work,source.current_state_guard)?;let guards=w::usize_add(m,guards,source.observation_guard)?;
        if w::compare(m,bytes,source_bytes)?.is_gt(){source_bytes=bytes;}
        if w::compare(m,guards,source_guards)?.is_gt(){source_guards=guards;}
    }
    // Seven monetary bodies (authority/evaluation/decision/revision/
    // consumption/two actions), final evidence+verification and three source
    // bodies. Original claim/observation/program/binding/grants/verifiers and
    // their full closures remain in P; current consent and other implicit
    // operands are separate changing-frame terms. Financial observations are
    // reached by old wrappers, not by the R/D/A path accounted here.
    let mut bytes=source_bytes;let mut guards=source_guards;
    for (body,guard) in [(app.standalone_authority,app.decoded.standalone_authority),(app.evaluation,app.decoded.evaluation),(app.decision,app.decoded.decision),(app.revision,app.decoded.revision),(app.consumption,app.decoded.consumption),(app.action,app.decoded.action),(app.action,app.decoded.action),(app.final_evidence,app.decoded.final_evidence),(app.evidence_verification,app.decoded.evidence_verification)]{
        m.visit("capacity.history_constructor_body")?;bytes=w::usize_add(m,bytes,body)?;guards=w::usize_add(m,guards,guard)?;
    }
    let maximum_versions_per_claim=versions(m,1)?;
    let generations=if w::compare(m,original.app_costs.len(),1usize)?.is_eq(){maximum_versions_per_claim}else{versions(m,original.app_costs.len())?};
    Ok(AppHistory{maximum_versions_per_claim,maximum_total_versions:generations,records:mul(m,12,generations)?,body_bytes:mul(m,bytes,generations)?,selected_guard_work:mul(m,guards,generations)?})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn retained_cost_triple_floor_and_coupled_storage_boundary(){
        let mut meter=CoreMeter::new(65536).unwrap();
        let mut total=0usize;
        for(kind,prefix)in[("workflow-cost","wdoc_"),("workflow-decision","wdrev_"),("workflow-economic-authority","wdoc_")]{
            let value=serde_json::json!({"kind":kind,"id":format!("{prefix}{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
            let actual=crate::canonical::CanonicalBytes::from_value(&value).unwrap();
            let derived=ref_floor(&mut meter,kind,prefix).unwrap();
            assert_eq!(derived,actual.as_slice().len());total+=derived;
        }
        assert_eq!(total,571);
        for claims in [1usize,2,31]{
            let n=versions(&mut meter,claims).unwrap() as u128;
            assert!(571*n*n<=268435456*claims as u128);
            assert!(571*(n+1)*(n+1)>268435456*claims as u128);
            assert!(n<100000);
        }
    }
}
