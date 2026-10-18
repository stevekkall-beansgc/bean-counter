//! Capacity construction over original monetary reservations. Structural union
//! is one prerequisite; complete source/resource and currentness proof remains
//! mandatory before producing a candidate or immutable capacity certificate.
mod topology;

pub(super) mod original;

pub(super) mod current;
mod future_records;
mod app_bytes;
mod source;
mod production;
mod pricing;
pub(super) mod history;

/// Exact measured I∪N portion of the executed transition. This is deliberately
/// distinct from a prospective capacity proof: O is added by the closed wrapper
/// producer, and future F/E and all-ancestor bounds must still be derived.
pub(super) struct ActualMembers {
    pub without_operation_records:usize,
    pub without_operation_body_bytes:usize,
}
pub(super) fn actual_members(m:&mut super::CoreMeter,t:&super::data::Construction)->crate::Result<ActualMembers>{
    use crate::validation_work as w;
    let mut count=0usize;let mut bytes=0usize;let mut pending=0usize;
    for reference in t.records.consumed(){
        m.visit("capacity.actual_consumed_member")?;
        let body=t.records.canonical(m,reference)?;
        count=w::usize_add(m,count,1)?;bytes=w::usize_add(m,bytes,body.len())?;
    }
    for (i,envelope) in t.new_envelopes.iter().enumerate(){
        m.visit("capacity.actual_new_member")?;
        for previous in &t.new_envelopes[..i]{
            m.visit("capacity.actual_introduction_unique")?;
            if super::shape::same_ref(m,&previous.reference,&envelope.reference)?{
                return Err(super::shape::fail("duplicate authoritative result introduction"));
            }
        }
        // Reuse the SAME immutable full-Ref operand index and its post-check
        // consumed marker. This avoids a new N×I scan while preserving exact
        // I∪N membership: a merely supplied, unused operand does not count.
        let used=t.records.used_canonical(m,&envelope.reference)?;
        let (already_counted,body)=match used{
            super::records::UsedCanonical::Preexisting(body)=>(true,Some(body)),
            super::records::UsedCanonical::Pending(body)=>{pending=w::usize_add(m,pending,1)?;(false,Some(body))},
            super::records::UsedCanonical::Absent=>(false,None),
        };
        if let Some(body)=body{
            // A pending observation may be consumed and introduced together;
            // both its full Ref AND its actual bytes must coincide.
            if !super::shape::same_bytes(m,body,&envelope.canonical_body)?{
                return Err(super::shape::fail("input/result exact Ref body conflict"));
            }
        }
        if !already_counted{
            count=w::usize_add(m,count,1)?;bytes=w::usize_add(m,bytes,envelope.canonical_body.len())?;
        }
    }
    if !w::compare(m,pending,t.records.used_pending_count())?.is_eq(){return Err(super::shape::fail("consumed pending record lacks an authoritative introduction"));}
    // Reserve one still-required operation member. The operation's actual body
    // and the full future envelope are checked independently; no zero-byte O
    // or observed-size substitute is written into a CorrectionCapacity record.
    let with_operation=w::usize_add(m,count,1)?;
    if m.resource_limits().is_some(){
        // The record quota includes the operation allowance. Old/input bytes
        // and newly constructed N bytes have separate configured quotas,
        // already checked by Records and new_body; do not cap their sum again.
        m.check_resolved_dimensions(with_operation,0)?;
    }else if w::compare(m,with_operation,4096usize)?.is_gt()||w::compare(m,bytes,8388608usize)?.is_gt(){
        return Err(crate::Error::new("CORRECTION_CAPACITY_EXCEEDED","actual immediate member union"));
    }
    Ok(ActualMembers{without_operation_records:count,without_operation_body_bytes:bytes})
}

/// Compose the already source-derived successful SOURCE and PRICING portions
/// at the root monetary interpreter's local call multiplicities. This is called
/// by the containing gate in its SAME child; it is deliberately not an
/// EnvelopeBound. Type/Ref/authority/closure/body/DTO/capacity-calculation work,
/// target-maxima/window arithmetic, funding, refusal and original-audit envelopes must
/// still be added before the complete bound can be passed to a writer.
pub(super) fn accounted_root_callbacks(m:&mut super::CoreMeter,original:&original::OriginalReservations)->crate::Result<(usize,usize)>{
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    fn product(m:&mut super::CoreMeter,a:usize,b:usize)->crate::Result<usize>{
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
        a.checked_mul(b).ok_or_else(||super::shape::fail("root callback composition product"))
    }
    let records=original.future_records.as_ref().ok_or_else(||super::shape::fail("root callback dimensions are incomplete"))?;
    let kernels=original.pricing_kernels.as_ref().ok_or_else(||super::shape::fail("original pricing kernels are incomplete"))?;
    if w::compare(m,original.source_bounds.frames.len(),0usize)?.is_eq(){return Err(super::shape::fail("original root source frame is absent"));}
    let mut maximum_source=0usize;
    for frame in &original.source_bounds.frames{
        m.visit("capacity.compose_original_source_frame")?;
        let source=frame.complete_callback_work.as_ref().ok_or_else(||super::shape::fail("original source callback expression is incomplete"))?;
        if w::compare(m,source.total,maximum_source)?.is_gt(){maximum_source=source.total;}
    }
    // Root source plus each distinct affected family/app slot. Reusing a
    // physical source does not remove any independent slot/grant predicates.
    let source=product(m,maximum_source,records.maximum_source_frames)?;
    // root_plan prices current + replacement + original-action reconstruction;
    // each family additionally reconstructs its original work component net.
    let work_calls=w::usize_add(m,3,original.families.len())?;
    let mut pricing=product(m,kernels.work,work_calls)?;
    // Active family verification plus explicit-none exact-inverse producer.
    // Pending/none/closed alternatives do less and keep their vertices.
    let family_calls=product(m,2,original.families.len())?;
    let family=product(m,kernels.maximum_family,family_calls)?;
    pricing=w::usize_add(m,pricing,family)?;
    // A finalized app inverse reconstructs its prior original price once;
    // pending has no initial price and cannot manufacture an evaluation.
    let app=product(m,kernels.maximum_app,original.app_costs.len())?;
    pricing=w::usize_add(m,pricing,app)?;
    Ok((source,pricing))
}

/// Monetary NEW-body portion of a containing root correction. This covers the
/// root and every original family/app branch (including zero/pending states).
/// It deliberately excludes source/F witness bodies, O, complete old closure,
/// capacity/dependency outputs and independently required resource dimensions.
/// Consequently this number is not an EnvelopeBound or an admitting maximum.
pub(super) fn accounted_root_new_bodies(m:&mut super::CoreMeter,original:&original::OriginalReservations)->crate::Result<usize>{
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    fn product(m:&mut super::CoreMeter,a:usize,b:usize)->crate::Result<usize>{
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
        a.checked_mul(b).ok_or_else(||super::shape::fail("root changing-body composition"))
    }
    let root=original.root_body_bytes.as_ref().ok_or_else(||super::shape::fail("root changing-body derivation missing"))?;
    let mut bytes=0;
    for body in [root.authority,root.evaluation,root.decision,root.chain]{m.visit("capacity.root_common_new_body")?;bytes=w::usize_add(m,bytes,body)?;}
    let actions=product(m,original.maximum_root_actions,root.action)?;bytes=w::usize_add(m,bytes,actions)?;
    for reservation in &original.families{
        m.visit("capacity.root_original_family_new_bodies")?;
        let family=original.family_body_bytes.as_ref().ok_or_else(||super::shape::fail("family changing-body derivation missing"))?;
        for body in [family.authority,family.evaluation,family.decision,family.family_state]{bytes=w::usize_add(m,bytes,body)?;}
        let actions=product(m,reservation.maximum_inverse_actions,family.action)?;bytes=w::usize_add(m,bytes,actions)?;
    }
    for _ in &original.app_costs{
        m.visit("capacity.root_original_app_new_bodies")?;
        let app=original.app_body_bytes.as_ref().ok_or_else(||super::shape::fail("app changing-body derivation missing"))?;
        let branch=if w::compare(m,app.finalized_causal_new_bodies,app.pending_causal_new_bodies)?.is_gt(){app.finalized_causal_new_bodies}else{app.pending_causal_new_bodies};
        bytes=w::usize_add(m,bytes,branch)?;
    }
    if !w::compare(m,original.delayed_costs.len(),0usize)?.is_eq(){return Err(crate::Error::new("UNSUPPORTED_SCHEMA_PROFILE","supplier changing-body composition is not derived"));}
    Ok(bytes)
}

/// Current grant/state I and introduced source-observation N contributions.
/// Each promised independently authorized source frame is counted, even when
/// some physical sources coincide. Immutable enrollment/original grants and
/// all transitive dependencies remain in the separate complete original I.
pub(super) fn accounted_root_source_bodies(m:&mut super::CoreMeter,original:&original::OriginalReservations)->crate::Result<(usize,usize)>{
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    let records=original.future_records.as_ref().ok_or_else(||super::shape::fail("source changing-body dimensions missing"))?;
    let mut inputs=0;let mut observation=0;
    for frame in &original.source_bounds.frames{
        m.visit("capacity.root_source_changing_bodies")?;
        let bodies=frame.changing_bodies.as_ref().ok_or_else(||super::shape::fail("source changing-body range missing"))?;
        let current=w::usize_add(m,bodies.current_grant,bodies.current_state)?;
        if w::compare(m,current,inputs)?.is_gt(){inputs=current;}
        if w::compare(m,bodies.observation,observation)?.is_gt(){observation=bodies.observation;}
    }
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&inputs.into(),&records.maximum_source_frames.into()])?;
    let inputs=inputs.checked_mul(records.maximum_source_frames).ok_or_else(||super::shape::fail("source changing-input product"))?;
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&observation.into(),&records.maximum_source_frames.into()])?;
    let observations=observation.checked_mul(records.maximum_source_frames).ok_or_else(||super::shape::fail("source observation product"))?;
    Ok((inputs,observations))
}

pub(super) mod local;
