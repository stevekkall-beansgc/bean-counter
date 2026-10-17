//! Capacity construction over original monetary reservations. Structural union
//! is one prerequisite; complete source/resource and currentness proof remains
//! mandatory before producing a candidate or immutable capacity certificate.
mod topology;

pub(super) mod original;

pub(super) mod current;
mod future_records;
mod app_bytes;
mod source;

/// Exact measured I∪N portion of the executed transition. This is deliberately
/// distinct from a prospective capacity proof: O is added by the closed wrapper
/// producer, and future F/E and all-ancestor bounds must still be derived.
pub(super) struct ActualMembers {
    pub without_operation_records:usize,
    pub without_operation_body_bytes:usize,
}
pub(super) fn actual_members(m:&mut super::CoreMeter,t:&super::data::Construction)->crate::Result<ActualMembers>{
    use crate::validation_work as w;
    let mut count=0usize;let mut bytes=0usize;
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
        let body=t.records.consumed_canonical(m,&envelope.reference)?;
        let already_counted=body.is_some();
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
    // Reserve one still-required operation member. The operation's actual body
    // and the full future envelope are checked independently; no zero-byte O
    // or observed-size substitute is written into a CorrectionCapacity record.
    let with_operation=w::usize_add(m,count,1)?;
    if w::compare(m,with_operation,4096usize)?.is_gt()||w::compare(m,bytes,8388608usize)?.is_gt(){
        return Err(crate::Error::new("CORRECTION_CAPACITY_EXCEEDED","actual immediate member union"));
    }
    Ok(ActualMembers{without_operation_records:count,without_operation_body_bytes:bytes})
}
