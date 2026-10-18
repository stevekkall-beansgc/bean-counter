//! Invocation-local immutable pointer intrinsics, never origin completion.
//! The host associates an entry with a completed original only after its real
//! chronological recipe succeeds. No body, authority, membership or snapshot
//! proof is manufactured by this table.
use super::*;
mod batch;
pub use batch::{OriginPointerBatch,PreparedOriginBatch,ORIGIN_POINTER_BATCH,ORIGIN_POINTER_BATCH_WORDS};

pub const ORIGIN_POINTER_BLOCK:usize=32;
/// Conservative closed-pointer grammar bound:747 fixed bytes plus4608
/// escaped key bytes, with exact operation/manifest/receipt family IDs.
pub const ORIGIN_POINTER_MAX_BYTES:usize=5355;
fn multiply(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||fail("pointer payload multiplication overflow"))
}
/// Borrowed immutable pointer DATA. A scratch location is only an acquisition
/// hint from the actual append; the consumer must still rebind the raw digest.
pub struct OriginPointerEntry {sequence:u64,digest:[u8;32],offset:u64,length:u16,located:bool}
impl OriginPointerEntry {
    pub fn sequence(&self)->u64{self.sequence}
    pub fn scratch_location(&self)->Option<(u64,usize)>{if self.located{Some((self.offset,self.length as usize))}else{None}}
    pub fn scratch_offset(&self)->u64{self.offset}
    pub fn canonical_length(&self)->usize{self.length as usize}
    /// The actual mint established this private length invariant. This only
    /// borrows a physical I/O destination; it performs no new semantic length
    /// validation or authority check. Normal Rust slice safety is retained.
    pub fn read_buffer<'a>(&self,row:&'a mut [u8;ORIGIN_POINTER_MAX_BYTES])->&'a mut [u8]{&mut row[..usize::from(self.length)]}
}
type Intrinsic=OriginPointerEntry;
pub struct ValidatedOriginPointers {
    entries:Vec<Intrinsic>,cancellation:AuditCancellation,all_locations:bool,
    ownership:ExternalBufferLease,_control:ExternalBufferLease,
}
pub struct OriginPointerBlock<'a>{entries:&'a [Intrinsic],cancellation:&'a AuditCancellation,all_locations:bool}
impl ValidatedOriginPointers {
    pub fn len(&self)->usize{self.entries.len()}
    pub fn is_empty(&self)->bool{self.entries.is_empty()}
    pub fn new(s:&mut OriginalAuditSession)->Result<Self>{
        let result=(||{s.check_cancelled()?;
            let control=s.reserve_external_payload(mem::size_of::<Self>())?;
            let ownership=s.reserve_external_payload(0)?;
            Ok(Self{entries:Vec::new(),cancellation:s.arena().cancellation_handle(),all_locations:true,ownership,_control:control})
        })();if result.is_err(){s.invalidate();}result
    }
    /// Validate once under the actual original's existing owner. The returned
    /// ordinal is DATA, not a completed-origin token or a persistent checkpoint.
    pub fn record(&mut self,pointer:&AuditedValue,s:&mut OriginalAuditSession)->Result<usize>{
        self.record_inner(pointer,None,s)
    }
    /// The offset is untrusted lookup DATA from the same scratch append owner,
    /// not custody or completion evidence. Length comes from this SAME emitter.
    pub fn record_at(&mut self,pointer:&AuditedValue,scratch_offset:u64,s:&mut OriginalAuditSession)->Result<usize>{
        self.record_inner(pointer,Some(scratch_offset),s)
    }
    fn record_inner(&mut self,pointer:&AuditedValue,location:Option<u64>,s:&mut OriginalAuditSession)->Result<usize>{
        let result=(||{
            s.check_cancelled()?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            m.visit("audit.pointer_table_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle())||!pointer.belongs_to(&arena){return Err(fail("pointer table attempt owner"));}
            let sequence=validate_pointer(m,pointer.value())?;
            if let Some(previous)=self.entries.last(){if !w::compare(m,sequence,previous.sequence)?.is_gt(){return Err(fail("pointer table numeric order/duplicate"));}}
            // Profile B's existing workflow-origin bound, not a new history cap.
            if !w::compare(m,self.entries.len(),100000usize)?.is_lt(){return Err(fail("pointer table workflow-origin limit"));}
            let encoded=m.canonical_owned_representation(&arena,pointer.value(),RepresentationKind::Record)?;
            if w::compare(m,encoded.bytes().len(),ORIGIN_POINTER_MAX_BYTES)?.is_gt(){return Err(fail("pointer canonical grammar bound"));}
            let digest=m.hash_owned_representation(&arena,encoded.bytes(),RepresentationKind::Record)?;
            if w::compare(m,self.entries.len(),self.entries.capacity())?.is_eq(){
                let old=self.entries.capacity();
                let next=if w::compare(m,old,0usize)?.is_eq(){32}else{let doubled=multiply(m,old,2)?;if w::compare(m,doubled,100000usize)?.is_gt(){100000}else{doubled}};
                let payload=multiply(m,next,mem::size_of::<Intrinsic>())?;
                let lease=arena.reserve_external_payload(payload,m)?;
                let additional=w::usize_sub(m,next,self.entries.len())?;
                let old_payload=multiply(m,self.entries.len(),mem::size_of::<Intrinsic>())?;m.scan_bytes(old_payload)?;
                m.write_bytes(payload)?;
                self.entries.try_reserve_exact(additional).map_err(|_|fail("pointer table vector allocation"))?;
                self.ownership=lease;
            }
            let ordinal=self.entries.len();m.write_bytes(mem::size_of::<Intrinsic>())?;
            m.visit("audit.pointer_append_location_presence")?;
            if location.is_none(){self.all_locations=false;}
            // The SAME producer established<=5355, strictly below u16::MAX.
            self.entries.push(Intrinsic{sequence,digest,offset:location.unwrap_or(0),length:encoded.bytes().len() as u16,located:location.is_some()});Ok(ordinal)
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
    pub fn block<'a>(&'a self,first:usize,s:&mut OriginalAuditSession)->Result<OriginPointerBlock<'a>>{
        let result=(||{
            s.check_cancelled()?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            m.visit("audit.pointer_block_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()){return Err(fail("pointer block attempt owner"));}
            if !w::compare(m,first,self.entries.len())?.is_lt(){return Err(fail("pointer block ordinal"));}
            let proposed=w::usize_add(m,first,ORIGIN_POINTER_BLOCK)?;
            let end=if w::compare(m,proposed,self.entries.len())?.is_gt(){self.entries.len()}else{proposed};
            Ok(OriginPointerBlock{entries:&self.entries[first..end],cancellation:&self.cancellation,all_locations:self.all_locations})
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
}
impl OriginPointerBlock<'_>{
    /// All location-presence checks were performed by the actual producer.
    /// This single aggregate check replaces repeating Option tests at every
    /// consumer entry. Offsets remain DATA and require exact raw digest rebind.
    pub fn located_entries<'a>(&'a self,s:&mut OriginalAuditSession)->Result<&'a [OriginPointerEntry]>{
        let result=(||{let entries=self.entries(s)?;s.meter_mut().visit("audit.pointer_locations_complete")?;
            if !self.all_locations{return Err(fail("pointer table includes unlocated entries"));}Ok(entries)
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
    /// A paid borrowed acquisition view; no second sequence/location vector is
    /// allocated. Getters only expose DATA, never the private digest or a token
    /// establishing that an origin actually completed.
    pub fn entries<'a>(&'a self,s:&mut OriginalAuditSession)->Result<&'a [OriginPointerEntry]>{
        let result=(||{
            s.check_cancelled()?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            m.visit("audit.pointer_entries_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()){return Err(fail("pointer entry attempt owner"));}
            Ok(self.entries)
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
    /// Actual sequence keys for bounded scratch acquisition, including gaps in
    /// the numeric domain. No resident second ordinal map is required.
    pub fn sequences(&self,s:&mut OriginalAuditSession)->Result<[Option<u64>;ORIGIN_POINTER_BLOCK]>{
        let result=(||{
            s.check_cancelled()?;let arena=s.arena().shared_handle();let m=s.meter_mut();m.visit("audit.pointer_sequences_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()){return Err(fail("pointer sequence attempt owner"));}
            let mut out=[None;ORIGIN_POINTER_BLOCK];m.write_bytes(mem::size_of_val(&out))?;
            for(slot,entry)in out.iter_mut().zip(self.entries){m.visit("audit.pointer_sequence")?;*slot=Some(entry.sequence);}Ok(out)
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
}

fn validate_pointer(m:&mut CoreMeter,v:&Value)->Result<u64>{
    shape::closed(m,v,&["commit_sequence","operation_key","operation_ref","manifest_ref","receipt_ref"],&[])?;
    let sequence=counter(m,v,"commit_sequence")?;
    if !w::compare(m,sequence,0u64)?.is_gt(){return Err(fail("audit pointer positive sequence"));}
    pointer_key(m,&v["operation_key"])?;
    for(field,kind,prefix)in[("operation_ref","workflow-operation","wop_"),("manifest_ref","workflow-manifest","wmanifest_"),("receipt_ref","workflow-receipt","wreceipt_")]{
        let reference=&v[field];shape::closed(m,reference,&["kind","id","digest"],&[])?;
        let k=shape::text(m,reference,"kind")?;let id=shape::text(m,reference,"id")?;let digest=shape::text(m,reference,"digest")?;
        records::reference_parts(m,k,id,digest)?;
        if !shape::same_text(m,k,kind)?||!shape::same_text(m,id.get(..prefix.len()).unwrap_or(""),prefix)?{return Err(fail("audit pointer Ref family"));}
        let exact=w::usize_add(m,prefix.len(),64)?;
        if !w::compare(m,id.len(),exact)?.is_eq(){return Err(fail("audit pointer exact family identity"));}
    }Ok(sequence)
}

// A fixed finite byte-to-membership representation table. Every runtime lookup
// is a paid semantic RefLookup; no runtime shift/AND/popcount is hidden here.
// Byte conversion/copy passes are separately charged. Native word OR/popcount/
// leading-zero operations below use the unchanged operand-limb event rate.
const BITS:[[bool;8];256]={let mut table=[[false;8];256];let mut byte=0;while byte<256{let mut bit=0;while bit<8{table[byte][bit]=(byte&(1<<bit))!=0;bit+=1;}byte+=1;}table};
fn flags(m:&mut CoreMeter,mask:u32)->Result<[bool;ORIGIN_POINTER_BLOCK]>{
    m.scan_bytes(mem::size_of::<u32>())?;m.write_bytes(ORIGIN_POINTER_BLOCK)?;
    let mut out=[false;ORIGIN_POINTER_BLOCK];
    for(dst,byte)in out.chunks_exact_mut(8).zip(mask.to_le_bytes()){
        ValidationWork::visit(m,w::ValidationVisit::RefLookup)?;m.scan_bytes(8)?;m.write_bytes(8)?;dst.copy_from_slice(&BITS[usize::from(byte)]);
    }Ok(out)
}
fn word(m:&mut CoreMeter,operation:ArithmeticOperation,a:u32,b:Option<u32>)->Result<()>{
    let a=BigInt::from(a);if let Some(b)=b{ValidationWork::arithmetic(m,operation,&[&a,&BigInt::from(b)])}else{ValidationWork::arithmetic(m,operation,&[&a])}
}
use crate::validation_work::BigInt;
impl EconomicAuditStream<'_>{
    /// Consume a bounded actual table block. The caller's masks are DATA from
    /// its real fixed-point walk; expected prefix/union commitments, snapshot,
    /// table order and exact raw-byte association are all still checked here.
    /// `canonical_pointers` contains only selected union members, in block order.
    /// Source S bound for a successful block with G source witnesses, L<=32
    /// actual table slots and U selected union members:
    ///   44 + 8G + 20K + L + U(K+1) <= 160 + 60G,
    /// where K is the actual nonempty witness-mask count (including financial).
    /// This includes every fixed-word event/lookup and all executed membership
    /// decisions, not the separately charged table.block (10), sequences
    /// (1+L), headers, host traversal/I/O or original semantic verification.
    pub fn push_origin_block(&mut self,block:&OriginPointerBlock<'_>,financial_mask:u32,source_masks:&[u32],canonical_pointers:&[&[u8]],s:&mut OriginalAuditSession)->Result<()>{
        let result=(||{
            self.check_owner(s)?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            if !w::compare(m,self.phase,1u8)?.is_eq()||!w::compare(m,source_masks.len(),self.sources.len())?.is_eq(){return Err(fail("audit block phase/source count"));}
            m.visit("audit.block_table_attempt")?;
            if !block.cancellation.same_attempt(&self.cancellation){return Err(fail("audit block foreign table"));}
            let mut union=financial_mask;
            for mask in source_masks{m.visit("audit.block_union_member")?;word(m,ArithmeticOperation::BitOr,union,Some(*mask))?;union|=*mask;}
            if !w::compare(m,union,0u32)?.is_gt(){return Err(fail("audit block empty union"));}
            word(m,ArithmeticOperation::LeadingZeros,union,None)?;
            let highest=w::usize_sub(m,31,union.leading_zeros() as usize)?;
            if !w::compare(m,highest,block.entries.len())?.is_lt(){return Err(fail("audit block mask exceeds table slice"));}
            if let Some(previous)=self.last_sequence{if !w::compare(m,block.entries[0].sequence,previous)?.is_gt(){return Err(fail("audit block order/duplicate"));}}
            word(m,ArithmeticOperation::PopulationCount,union,None)?;
            let union_count=union.count_ones();
            if !w::compare(m,canonical_pointers.len(),union_count as usize)?.is_eq(){return Err(fail("audit block raw pointer count"));}
            // At most32 actual source observations plus financial, per frozen
            // observation grammar. Fixed stack state is included in source bounds.
            if !w::compare(m,self.sources.len(),32usize)?.is_le(){return Err(fail("audit source observation bound"));}
            let mut memberships=[[false;ORIGIN_POINTER_BLOCK];33];
            let mut started=[false;33];m.write_bytes(mem::size_of_val(&memberships)+mem::size_of_val(&started))?;
            let mut active:[Option<(usize,&mut Prefix<'_>)>;33]=std::array::from_fn(|_|None);
            m.write_bytes(mem::size_of_val(&active))?;
            let mut active_slots=active.iter_mut();
            for(index,(state,mask))in self.sources.iter_mut().zip(source_masks).chain(self.financial.iter_mut().map(|s|(s,&financial_mask))).enumerate(){
                m.visit("audit.block_witness")?;
                if !w::compare(m,*mask,0u32)?.is_eq(){
                    memberships[index]=flags(m,*mask)?;
                    started[index]=!w::compare(m,state.count,0u64)?.is_eq();
                    word(m,ArithmeticOperation::LeadingZeros,*mask,None)?;
                    let last=w::usize_sub(m,31,mask.leading_zeros() as usize)?;
                    if w::compare(m,block.entries[last].sequence,state.through)?.is_gt(){return Err(fail("audit block exceeds witness snapshot"));}
                    word(m,ArithmeticOperation::PopulationCount,*mask,None)?;
                    let count=u64::from(mask.count_ones());
                    ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&state.count.into(),&count.into()])?;
                    state.count=state.count.checked_add(count).ok_or_else(||fail("audit block witness count overflow"))?;
                    *active_slots.next().ok_or_else(||fail("audit active witness capacity"))?=Some((index,state));
                }
            }
            let union_flags=flags(m,union)?;let mut raw=canonical_pointers.iter();
            let mut union_started=!w::compare(m,self.union_count,0u64)?.is_eq();
            for(position,(entry,selected))in block.entries.iter().zip(union_flags).enumerate(){
                m.visit("audit.block_pointer_slot")?;if !selected{continue;}
                let raw=raw.next().ok_or_else(||fail("audit block missing canonical pointer"))?;
                let digest=m.hash_owned_representation(&arena,raw,RepresentationKind::Record)?;
                ValidationWork::visit(m,w::ValidationVisit::MemberCompare)?;m.scan_bytes(64)?;
                if digest!=entry.digest{return Err(fail("audit block exact pointer bytes"));}
                for slot in active.iter_mut().take_while(|slot|slot.is_some()){
                    let(index,state)=slot.as_mut().ok_or_else(||fail("audit active witness gap"))?;
                    m.visit("audit.block_pointer_membership")?;
                    if memberships[*index][position]{if started[*index]{syntax(m,&mut state.hash,b",")?;}bytes(m,&mut state.hash,raw)?;started[*index]=true;}
                }
                if union_started{syntax(m,&mut self.combined,b",")?;}bytes(m,&mut self.combined,raw)?;union_started=true;
            }
            ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&self.union_count.into(),&u64::from(union_count).into()])?;
            self.union_count=self.union_count.checked_add(u64::from(union_count)).ok_or_else(||fail("audit block union count overflow"))?;
            self.last_sequence=Some(block.entries[highest].sequence);Ok(())
        })();self.terminal(s,result)
    }
}
