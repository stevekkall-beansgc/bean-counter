//! AUTH03 commitments over the existing chronological scratch stream. This
//! computes DATA; the host separately establishes actual origin completion,
//! earliest introduction, witness membership and the held snapshot. No pointer
//! set or wrapper accumulates in memory, and no work allowance is manufactured.
use super::super::{CoreMeter,data::*,records,shape};
use crate::{Result,canonical::{self,audit_arena::{AuditedBytes,AuditedValue,AuditCancellation,ExternalBufferLease}},validation_work::{self as w,ArithmeticOperation,OriginalAuditSession,RepresentationKind,ValidationWork}};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::mem;
mod pointers;
mod production;
pub use production::{SourcePrefixInput,FinancialPrefixInput,ProducedOriginPrefix,ProducedOriginPrefixes};
pub use pointers::{ValidatedOriginPointers,OriginPointerBlock,OriginPointerEntry,ORIGIN_POINTER_BLOCK,ORIGIN_POINTER_MAX_BYTES,OriginPointerBatch,PreparedOriginBatch,ORIGIN_POINTER_BATCH,ORIGIN_POINTER_BATCH_WORDS};

pub enum OriginWitness { Financial,Source(usize) }
struct ExpectedPrefix<'a>{reference:&'a ExactRef,digest:String,count:u64}
struct Prefix<'a>{hash:Sha256,expected:Option<ExpectedPrefix<'a>>,through:u64,count:u64}
pub struct EconomicAuditStream<'a>{
    context:Option<&'a DraftContextData>,financial_ref:Option<&'a ExactRef>,production_key:Option<&'a OperationKey>,
    sources:Vec<Prefix<'a>>,source_count:usize,previous_source:Option<AuditedBytes>,financial:Option<Prefix<'a>>,
    combined:Option<Sha256>,last_sequence:Option<u64>,union_count:u64,phase:u8,sealed:bool,
    cancellation:AuditCancellation,ownership:Option<ExternalBufferLease>,
}
pub struct RecomputedAuditDigest<'a>{context:&'a DraftContextData,financial_ref:&'a ExactRef,digest:String,cancellation:AuditCancellation,_ownership:ExternalBufferLease}
impl RecomputedAuditDigest<'_>{
    pub fn combined_audit_digest(&self)->&str{&self.digest}
    /// This checks DATA bindings only; origin completion/custody remains the
    /// actual chronological caller's obligation. No asserted digest constructor
    /// or unbound String conversion exists.
    pub(crate) fn bound_digest<'b>(&'b self,c:&DraftContextData,financial:&ExactRef,s:&mut OriginalAuditSession)->Result<&'b str>{
        let result=(||{
            s.check_cancelled()?;
            let arena=s.arena().shared_handle();let m=s.meter_mut();
            m.visit("audit.same_attempt_owner")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()){return Err(fail("audit digest belongs to another attempt"));}
            for (left,right)in[(&self.context.key.logical_store_id,&c.key.logical_store_id),(&self.context.key.scope[0],&c.key.scope[0]),(&self.context.key.scope[1],&c.key.scope[1]),(&self.context.key.source,&c.key.source),(&self.context.key.operation_id,&c.key.operation_id)]{
                if !shape::same_text(m,left,right)?{return Err(fail("audit digest actual operation key"));}
            }
            if !shape::same_ref(m,self.financial_ref,financial)?{return Err(fail("audit digest financial observation association"));}
            for (left,right)in[(&self.context.authorities,&c.authorities),(&c.authorities,&self.context.authorities)]{
                for a in left{m.visit("audit.digest_source_set")?;let mut found=false;for b in right{if shape::same_ref(m,&a.authority_observation_ref,&b.authority_observation_ref)?{found=true;break;}}if !found{return Err(fail("audit digest source observation association"));}}
            }
            Ok(self.digest.as_str())
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
}
impl Drop for EconomicAuditStream<'_>{fn drop(&mut self){if !self.sealed{self.cancellation.cancel();}}}
fn fail(s:&str)->crate::Error{shape::fail(s)}
fn owning_reference(m:&mut CoreMeter,arena:&crate::canonical::audit_arena::AuditArena,r:&ExactRef)->Result<AuditedValue>{
    let kind=arena.string_value(&r.kind,m)?;let id=arena.string_value(&r.id,m)?;let digest=arena.string_value(&r.digest,m)?;
    arena.object_from_fields(&[("kind",kind.value()),("id",id.value()),("digest",digest.value())],m)
}
fn operation_key(m:&mut CoreMeter,arena:&crate::canonical::audit_arena::AuditArena,c:&DraftContextData)->Result<AuditedValue>{operation_key_value(m,arena,&c.key)}
fn operation_key_value(m:&mut CoreMeter,arena:&crate::canonical::audit_arena::AuditArena,key:&OperationKey)->Result<AuditedValue>{
    let store=arena.string_value(&key.logical_store_id,m)?;let source=arena.string_value(&key.source,m)?;let id=arena.string_value(&key.operation_id,m)?;
    let a=arena.string_value(&key.scope[0],m)?;let b=arena.string_value(&key.scope[1],m)?;let scope=arena.array_from_values(&[a.value(),b.value()],m)?;
    arena.object_from_fields(&[("logical_store_id",store.value()),("scope",scope.value()),("source",source.value()),("operation_id",id.value())],m)
}

fn bytes(m:&mut CoreMeter,h:&mut Sha256,b:&[u8])->Result<()>{m.hash_bytes(b.len())?;h.update(b);Ok(())}
fn syntax(m:&mut CoreMeter,h:&mut Sha256,b:&[u8])->Result<()>{m.write_bytes(b.len())?;bytes(m,h,b)}
fn part(m:&mut CoreMeter,h:&mut Sha256,v:&Value)->Result<()>{
    let arena=m.representation_arena().ok_or_else(||fail("owning audit commitment"))?;
    let encoded=m.canonical_owned_representation(&arena,v,RepresentationKind::Record)?;bytes(m,h,encoded.bytes())
}
const SOURCE_PREFIX_KEYS:&[&str]=&["logical_store_id","scope","root_ref","enrollment_ref","current_state_ref","original_grant_refs","verifier_ref"];
const FINANCIAL_PREFIX_KEYS:&[&str]=&["logical_store_id","scope","root_ref","pending_operation_key","snapshot_through_sequence","preexisting_input_refs","history_edges","head_pins","verifier_ref"];
fn prefix_start(m:&mut CoreMeter,domain:&'static [u8],keys:&[&str],label:&'static str,seed:&Value)->Result<Sha256>{
    let mut hash=start(m,domain)?;
    for(i,key)in keys.iter().enumerate(){m.visit(label)?;if i>0{syntax(m,&mut hash,b",")?;}let value=shape::field(m,seed,key)?;part(m,&mut hash,value)?;}
    syntax(m,&mut hash,b",[")?;Ok(hash)
}
fn start(m:&mut CoreMeter,domain:&'static [u8])->Result<Sha256>{let mut h=Sha256::new();bytes(m,&mut h,domain)?;syntax(m,&mut h,b"[")?;Ok(h)}
fn add(m:&mut CoreMeter,n:u64)->Result<u64>{ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&n.into(),&1u64.into()])?;n.checked_add(1).ok_or_else(||fail("audit count overflow"))}
fn counter(m:&mut CoreMeter,v:&Value,key:&str)->Result<u64>{
    let raw=shape::text(m,v,key)?;
    if w::compare(m,raw.len(),1usize)?.is_lt()||w::compare(m,raw.len(),19usize)?.is_gt(){return Err(fail("audit canonical counter length"));}
    m.scan_bytes(1)?;if !m.check("audit.counter_canonical_zero",||raw.len()==1||!raw.starts_with('0'))?{return Err(fail("audit noncanonical counter"));}
    let mut n=0u64;
    for digit in raw.bytes(){m.scan_bytes(1)?;if !m.check("audit.counter_digit",||digit.is_ascii_digit())?{return Err(fail("audit counter digit"));}
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&10u64.into()])?;n=n.checked_mul(10).ok_or_else(||fail("audit counter overflow"))?;
        ValidationWork::arithmetic(m,ArithmeticOperation::Subtract,&[&digit.into(),&b'0'.into()])?;
        let digit=u64::from(digit-b'0');ValidationWork::arithmetic(m,ArithmeticOperation::Add,&[&n.into(),&digit.into()])?;n=n.checked_add(digit).ok_or_else(||fail("audit counter overflow"))?;
    }Ok(n)
}
fn expected(m:&mut CoreMeter,r:&ExactRef,v:&Value)->Result<String>{let raw=shape::text(m,v,"prefix_digest")?;records::reference_parts(m,&r.kind,&r.id,raw)?;m.scan_bytes(raw.len())?;m.write_bytes(raw.len())?;Ok(raw.to_owned())}
fn exact_header(m:&mut CoreMeter,r:&ExactRef,v:&Value,kind:&str,schema:&str,quoted_schema:&[u8])->Result<()>{
    records::reference(m,r)?;let actual=shape::text(m,v,"schema")?;
    if !shape::same_text(m,&r.kind,kind)?||!shape::same_text(m,actual,schema)?||!shape::same_text(m,r.id.get(..5).unwrap_or(""),"wdoc_")?||!shape::same_text(m,&r.id[5..],&r.digest[7..])?{return Err(fail("audit exact header Ref family"));}
    let arena=m.representation_arena().ok_or_else(||fail("owning audit header"))?;
    let encoded=m.canonical_owned_representation(&arena,v,RepresentationKind::Record)?;
    let digest=records::hash_parts(m,"workflow-document",&[quoted_schema,encoded.bytes()],true)?;
    if !shape::same_text(m,&r.digest,&digest)?{return Err(fail("audit observation body/Ref mismatch"));}Ok(())
}
fn common_header(m:&mut CoreMeter,v:&Value,c:&DraftContextData)->Result<()>{
    let store=shape::text(m,v,"logical_store_id")?;if !shape::same_text(m,store,&c.key.logical_store_id)?{return Err(fail("audit header store"));}
    let scope=shape::array(m,v,"scope")?;if !w::compare(m,scope.len(),2usize)?.is_eq(){return Err(fail("audit header scope length"));}
    for(part,want)in scope.iter().zip(&c.key.scope){m.visit("audit.header_scope_part")?;let part=part.as_str().ok_or_else(||fail("audit header scope"))?;if !shape::same_text(m,part,want)?{return Err(fail("audit header scope association"));}}
    let mode=shape::text(m,v,"installation_mode")?;if !shape::same_text(m,mode,&c.installation_mode)?{return Err(fail("audit header installation mode"));}Ok(())
}
impl<'a> EconomicAuditStream<'a>{
    pub fn new(context:&'a DraftContextData,financial_ref:&'a ExactRef,session:&mut OriginalAuditSession)->Result<Self>{
        let result=(||{
            session.check_cancelled()?;let arena=session.arena().shared_handle();let m=session.meter_mut();
            if m.resource_vector_usage().is_none(){return Err(fail("audit stream requires RP01 owning session"));}
            let mut count=0usize;
            for(i,a)in context.authorities.iter().enumerate(){m.visit("audit.actual_source_witness")?;let mut seen=false;for previous in &context.authorities[..i]{if shape::same_ref(m,&previous.authority_observation_ref,&a.authority_observation_ref)?{seen=true;break;}}if !seen{count=w::usize_add(m,count,1)?;}}
            let per=w::usize_add(m,mem::size_of::<Prefix>(),71usize)?;
            ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&count.into(),&per.into()])?;
            let payload=count.checked_mul(per).ok_or_else(||fail("audit source-state allocation product"))?;
            // Financial expected digest plus simultaneous final hex/String
            // scratch and outputs, in addition to all retained source states.
            let payload=w::usize_add(m,payload,mem::size_of::<Self>()+71+512)?;
            let ownership=arena.reserve_external_payload(payload,m)?;
            let mut sources=Vec::new();sources.try_reserve_exact(count).map_err(|_|fail("audit source-state vector"))?;
            let key=operation_key(m,&arena,context)?;
            let mut combined=start(m,b"bean-counter/economic-origin-audits/1\0")?;part(m,&mut combined,key.value())?;syntax(m,&mut combined,b",[")?;
            Ok(Self{context:Some(context),financial_ref:Some(financial_ref),production_key:None,sources,source_count:count,previous_source:None,financial:None,combined:Some(combined),last_sequence:None,union_count:0,phase:0,sealed:false,cancellation:arena.cancellation_handle(),ownership:Some(ownership)})
        })();if result.is_err(){session.invalidate();}result
    }
    fn check_owner(&self,s:&mut OriginalAuditSession)->Result<()>{
        s.check_cancelled()?;let cancellation=s.arena().cancellation_handle();s.meter_mut().visit("audit.same_attempt_owner")?;
        if !self.cancellation.same_attempt(&cancellation){return Err(fail("audit stream belongs to another attempt"));}Ok(())
    }
    fn terminal<T>(&mut self,s:&mut OriginalAuditSession,result:Result<T>)->Result<T>{if result.is_err(){self.phase=3;self.cancellation.cancel();s.invalidate();}result}
    /// One owning header at a time, in canonical observation-Ref order.
    pub fn add_source_header(&mut self,reference:&ExactRef,observation:&Value,s:&mut OriginalAuditSession)->Result<usize>{
        let result=(||{
            self.check_owner(s)?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            if !w::compare(m,self.phase,0u8)?.is_eq()||!w::compare(m,self.sources.len(),self.source_count)?.is_lt(){return Err(fail("audit source header phase/count"));}
            let context=self.context.ok_or_else(||fail("verification requires original context"))?;
            let combined=self.combined.as_mut().ok_or_else(||fail("verification requires combined header"))?;
            let mut selected=None;for a in &context.authorities{m.visit("audit.source_header_association")?;if shape::same_ref(m,&a.authority_observation_ref,reference)?{selected=Some(&a.authority_observation_ref);break;}}
            let selected=selected.ok_or_else(||fail("audit source header absent from actual source frames"))?;records::reference(m,selected)?;
            let value=owning_reference(m,&arena,selected)?;let order=m.canonical_owned_representation(&arena,value.value(),RepresentationKind::SourceDefinition)?;
            if let Some(previous)=&self.previous_source{if !shape::byte_order(m,previous.bytes(),order.bytes())?.is_lt(){return Err(fail("audit source header order/duplicate"));}}
            common_header(m,observation,context)?;
            exact_header(m,reference,observation,"workflow-authority-prefix-observation","ledger-workflow-authority-prefix-observation/1",br#""ledger-workflow-authority-prefix-observation/1""#)?;
            let hash=prefix_start(m,b"bean-counter/authority-origin-prefix/1\0",SOURCE_PREFIX_KEYS,"audit.source_header_part",observation)?;
            let state=Prefix{hash,expected:Some(ExpectedPrefix{reference:selected,digest:expected(m,selected,observation)?,count:counter(m,observation,"origin_count")?}),through:counter(m,observation,"verified_through_sequence")?,count:0};
            if !self.sources.is_empty(){syntax(m,combined,b",")?;}bytes(m,combined,order.bytes())?;self.previous_source=Some(order);
            let index=self.sources.len();self.sources.push(state);Ok(index)
        })();self.terminal(s,result)
    }
    pub fn set_financial_header(&mut self,reference:&ExactRef,observation:&Value,s:&mut OriginalAuditSession)->Result<()>{
        let result=(||{
            self.check_owner(s)?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            if !w::compare(m,self.phase,0u8)?.is_eq()||!w::compare(m,self.sources.len(),self.source_count)?.is_eq()||self.financial.is_some(){return Err(fail("audit financial header phase"));}
            let context=self.context.ok_or_else(||fail("verification requires original context"))?;
            let financial_ref=self.financial_ref.ok_or_else(||fail("verification requires financial Ref"))?;
            let combined=self.combined.as_mut().ok_or_else(||fail("verification requires combined header"))?;
            if !shape::same_ref(m,reference,financial_ref)?{return Err(fail("audit financial exact observation"));}
            common_header(m,observation,context)?;let key=operation_key(m,&arena,context)?;shape::equal(m,&observation["pending_operation_key"],key.value())?;
            exact_header(m,reference,observation,"workflow-financial-history-observation","ledger-workflow-financial-history-observation/3",br#""ledger-workflow-financial-history-observation/3""#)?;
            let hash=prefix_start(m,b"bean-counter/financial-origin-prefix/1\0",FINANCIAL_PREFIX_KEYS,"audit.financial_header_part",observation)?;
            self.financial=Some(Prefix{hash,expected:Some(ExpectedPrefix{reference:financial_ref,digest:expected(m,reference,observation)?,count:counter(m,observation,"origin_count")?}),through:counter(m,observation,"snapshot_through_sequence")?,count:0});
            syntax(m,combined,b"],")?;let reference=owning_reference(m,&arena,reference)?;part(m,combined,reference.value())?;syntax(m,combined,b",[")?;
            self.phase=1;Ok(())
        })();self.terminal(s,result)
    }
    pub fn source_observation_ref(&self,index:usize)->Option<&ExactRef>{self.sources.get(index).and_then(|s|s.expected.as_ref().map(|e|e.reference))}
    /// The original's SAME owner includes its actual pre/core/post work. One
    /// union pointer is delivered once, with all actual witness memberships.
    pub fn push_origin(&mut self,pointer:&AuditedValue,memberships:&[OriginWitness],s:&mut OriginalAuditSession)->Result<()>{
        let result=(||{
            self.check_owner(s)?;let arena=s.arena().shared_handle();let m=s.meter_mut();
            if !w::compare(m,self.phase,1u8)?.is_eq()||memberships.is_empty(){return Err(fail("audit pointer phase/membership"));}
            m.visit("audit.pointer_attempt_owner")?;if !pointer.belongs_to(&arena){return Err(fail("audit pointer belongs to another attempt"));}
            let v=pointer.value();shape::closed(m,v,&["commit_sequence","operation_key","operation_ref","manifest_ref","receipt_ref"],&[])?;
            let sequence=counter(m,v,"commit_sequence")?;if !w::compare(m,sequence,0u64)?.is_gt(){return Err(fail("audit pointer positive sequence"));}
            if let Some(previous)=self.last_sequence{if !w::compare(m,sequence,previous)?.is_gt(){return Err(fail("audit union order/duplicate"));}}
            pointer_key(m,&v["operation_key"])?;
            for(field,kind,prefix)in[("operation_ref","workflow-operation","wop_"),("manifest_ref","workflow-manifest","wmanifest_"),("receipt_ref","workflow-receipt","wreceipt_")]{let reference=&v[field];shape::closed(m,reference,&["kind","id","digest"],&[])?;let k=shape::text(m,reference,"kind")?;let id=shape::text(m,reference,"id")?;let digest=shape::text(m,reference,"digest")?;records::reference_parts(m,k,id,digest)?;if !shape::same_text(m,k,kind)?||!shape::same_text(m,id.get(..prefix.len()).unwrap_or(""),prefix)?{return Err(fail("audit pointer Ref family"));}}
            let encoded=m.canonical_owned_representation(&arena,v,RepresentationKind::Record)?;
            for(i,member)in memberships.iter().enumerate(){m.visit("audit.origin_membership")?;for previous in &memberships[..i]{m.visit("audit.membership_unique")?;let same=match(previous,member){(OriginWitness::Financial,OriginWitness::Financial)=>true,(OriginWitness::Source(a),OriginWitness::Source(b))=>w::compare(m,*a,*b)?.is_eq(),_=>false};if same{return Err(fail("audit repeated witness membership"));}}
                let state=match member{OriginWitness::Financial=>self.financial.as_mut().ok_or_else(||fail("audit financial header missing"))?,OriginWitness::Source(index)=>{if !w::compare(m,*index,self.sources.len())?.is_lt(){return Err(fail("audit source membership index"));}&mut self.sources[*index]}};
                if w::compare(m,sequence,state.through)?.is_gt(){return Err(fail("audit pointer exceeds snapshot"));}if !w::compare(m,state.count,0u64)?.is_eq(){syntax(m,&mut state.hash,b",")?;}bytes(m,&mut state.hash,encoded.bytes())?;state.count=add(m,state.count)?;
            }
            if let Some(combined)=self.combined.as_mut(){if !w::compare(m,self.union_count,0u64)?.is_eq(){syntax(m,combined,b",")?;}bytes(m,combined,encoded.bytes())?;}self.union_count=add(m,self.union_count)?;self.last_sequence=Some(sequence);Ok(())
        })();self.terminal(s,result)
    }
    pub fn finish(mut self,s:&mut OriginalAuditSession)->Result<RecomputedAuditDigest<'a>>{
        let result=(||{
            self.check_owner(s)?;let m=s.meter_mut();if !w::compare(m,self.phase,1u8)?.is_eq(){return Err(fail("audit unfinished headers"));}
            for state in self.sources.iter_mut().chain(self.financial.iter_mut()){m.visit("audit.witness_finish")?;let expected=state.expected.as_ref().ok_or_else(||fail("verification requires expected prefix"))?;if !w::compare(m,state.count,expected.count)?.is_eq(){return Err(fail("audit witness origin count mismatch"));}syntax(m,&mut state.hash,b"]]")?;let hash=state.hash.clone().finalize();let text=canonical::hex_with_work(&hash,m)?;if !shape::same_text(m,expected.digest.get(7..).unwrap_or(""),&text)?{return Err(fail("audit witness prefix mismatch"));}}
            let combined=self.combined.as_mut().ok_or_else(||fail("verification requires combined header"))?;syntax(m,combined,b"]]")?;let hash=combined.clone().finalize();let text=canonical::hex_with_work(&hash,m)?;m.write_bytes(71)?;let digest=format!("sha256:{text}");
            let ownership=self.ownership.take().ok_or_else(||fail("audit stream owner missing"))?;self.sealed=true;Ok(RecomputedAuditDigest{context:self.context.ok_or_else(||fail("verification requires original context"))?,financial_ref:self.financial_ref.ok_or_else(||fail("verification requires financial Ref"))?,digest,cancellation:self.cancellation.clone(),_ownership:ownership})
        })();if result.is_err(){self.cancellation.cancel();s.invalidate();}result
    }
}
fn pointer_key(m:&mut CoreMeter,key:&Value)->Result<()>{
    shape::closed(m,key,&["logical_store_id","scope","source","operation_id"],&[])?;
    for field in ["logical_store_id","operation_id"]{let raw=shape::text(m,key,field)?;pointer_text(m,raw,128,true)?;}
    let source=shape::text(m,key,"source")?;pointer_text(m,source,256,false)?;
    let scope=shape::array(m,key,"scope")?;if !w::compare(m,scope.len(),2usize)?.is_eq(){return Err(fail("audit pointer scope length"));}
    for part in scope{let raw=part.as_str().ok_or_else(||fail("audit pointer scope string"))?;pointer_text(m,raw,128,true)?;}Ok(())
}
fn pointer_text(m:&mut CoreMeter,raw:&str,maximum:usize,id:bool)->Result<()>{
    m.scan_bytes(raw.len())?;let mut count=0usize;
    for ch in raw.chars(){m.visit("audit.pointer_text_character")?;count=w::usize_add(m,count,1)?;if id&&!m.check("audit.pointer_id_character",||ch>'\u{1f}'&&ch!='\u{7f}')?{return Err(fail("audit pointer Id control"));}if w::compare(m,count,maximum)?.is_gt(){return Err(fail("audit pointer text bound"));}}
    if !w::compare(m,count,0usize)?.is_gt(){return Err(fail("audit pointer empty text"));}Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use serde_json::json;
    fn reference(kind:&str,prefix:&str,digit:&str)->ExactRef{ExactRef{kind:kind.into(),id:format!("{prefix}{}",digit.repeat(64)),digest:format!("sha256:{}",digit.repeat(64))}}
    fn key(id:&str)->OperationKey{OperationKey{logical_store_id:"store".into(),scope:["org".into(),"scope".into()],source:"source".into(),operation_id:id.into()}}
    fn pointer(id:&str,sequence:&str,digit:&str)->Value{json!({"commit_sequence":sequence,"operation_key":key(id),"operation_ref":reference("workflow-operation","wop_",digit),"manifest_ref":reference("workflow-manifest","wmanifest_",digit),"receipt_ref":reference("workflow-receipt","wreceipt_",digit)})}
    fn encoded(value:&Value)->Vec<u8>{canonical::CanonicalBytes::from_value(value).unwrap().into_vec()}
    #[test]
    fn streaming_prefixes_equal_existing_hash_recipe_and_bad_membership_is_terminal(){
        // Synthetic commitment-only inputs. No origin/source/custody proof,
        // monetary constructor, financial admission or receipt is asserted.
        let mut old=CoreMeter::new(65536).unwrap();
        let root=reference("workflow-authority","wdoc_","1");
        let enrollment=reference("workflow-source-enrollment","wdoc_","2");
        let state=reference("workflow-source-state","wsource_","3");
        let grant=reference("workflow-source-grant","wdoc_","4");
        let verifier=reference("workflow-authority-audit-verifier","wdoc_","5");
        let first=pointer("first","1","6");let second=pointer("second","2","7");
        let source_prefix=records::hash(&mut old,"authority-origin-prefix",&json!(["store",["org","scope"],root,enrollment,state,[grant],verifier,[first]])).unwrap();
        let source=json!({"schema":"ledger-workflow-authority-prefix-observation/1","logical_store_id":"store","scope":["org","scope"],"installation_mode":"real_local","root_ref":root,"enrollment_ref":enrollment,"current_state_ref":state,"current_grant_ref":grant,"original_grant_refs":[grant],"verifier_ref":verifier,"verified_through_sequence":"2","origin_count":"1","prefix_digest":source_prefix});
        let source_document=super::super::super::body::document(&mut old,"workflow-authority-prefix-observation",source.clone()).unwrap();
        let financial_prefix=records::hash(&mut old,"financial-origin-prefix",&json!(["store",["org","scope"],root,key("pending"),"2",[],[],[],verifier,[first,second]])).unwrap();
        let financial=json!({"schema":"ledger-workflow-financial-history-observation/3","logical_store_id":"store","scope":["org","scope"],"installation_mode":"real_local","root_ref":root,"pending_operation_key":key("pending"),"snapshot_through_sequence":"2","preexisting_input_refs":[],"history_edges":[],"head_pins":[],"verifier_ref":verifier,"origin_count":"2","prefix_digest":financial_prefix});
        let financial_document=super::super::super::body::document(&mut old,"workflow-financial-history-observation",financial.clone()).unwrap();
        let expected_combined=records::hash(&mut old,"economic-origin-audits",&json!([key("pending"),[source_document.reference],financial_document.reference,[first,second]])).unwrap();
        let context=DraftContextData{key:key("pending"),actor:"principal".into(),canonical_economic_namespace:"namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-17T00:00:00.000000Z".into(),accepted_at:"2026-10-17T00:00:00.000000Z".into(),authorities:vec![AuthorityContextData{source_slot:"test:slot".into(),enrollment_ref:enrollment,current_state_ref:state,current_grant_ref:grant.clone(),original_grant_refs:vec![grant],binding_ref:root.clone(),accepted_agreement_ref:root.clone(),activation_ref:root,authority_observation_ref:source_document.reference.clone()}],source_observation_inputs:vec![]};
        let mut long_pointers=vec![first.clone(),second.clone()];
        for seq in 3..=33{long_pointers.push(pointer(&format!("origin-{seq}"),&seq.to_string(),"8"));}
        let mut long_financial=financial.clone();long_financial["snapshot_through_sequence"]=json!("33");long_financial["origin_count"]=json!("33");
        long_financial["prefix_digest"]=json!(records::hash(&mut old,"financial-origin-prefix",&json!(["store",["org","scope"],financial["root_ref"],key("pending"),"33",[],[],[],financial["verifier_ref"],long_pointers])).unwrap());
        let long_document=super::super::super::body::document(&mut old,"workflow-financial-history-observation",long_financial.clone()).unwrap();
        let long_combined=records::hash(&mut old,"economic-origin-audits",&json!([key("pending"),[source_document.reference],long_document.reference,long_pointers])).unwrap();
        // Actual first-pass producer uses no Observation Ref or expected
        // prefix/count. These are commitment-only synthetic inputs, just as
        // the verifier parity cases below; no origin completion is asserted.
        {
            let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let pending=key("pending");
            let mut source_seed=source.clone();source_seed.as_object_mut().unwrap().remove("prefix_digest");source_seed.as_object_mut().unwrap().remove("origin_count");
            let mut financial_seed=financial.clone();financial_seed.as_object_mut().unwrap().remove("prefix_digest");financial_seed.as_object_mut().unwrap().remove("origin_count");
            let mut table=ValidatedOriginPointers::new(&mut session).unwrap();
            for pointer in [&first,&second]{let owner=session.parse_representation(&encoded(pointer),RepresentationKind::Record).unwrap();table.record(&owner,&mut session).unwrap();}
            let mut producer=EconomicAuditStream::prefix_producer(&pending,1,&mut session).unwrap();
            producer.add_source_prefix(SourcePrefixInput{fields:&source_seed,through:2},&mut session).unwrap();
            producer.set_financial_prefix(FinancialPrefixInput{fields:&financial_seed,through:2},&mut session).unwrap();
            let batch=table.batch(0,&mut session).unwrap();
            let mut prepared=producer.prepare_origin_batch(&batch,&[3],&[&[1]],&mut session).unwrap();
            let a=encoded(&first);let b=encoded(&second);prepared.push_chunk(&[&a,&b],&mut session).unwrap();prepared.finish(&mut session).unwrap();
            let prefixes=producer.finish_prefixes(&mut session).unwrap();
            assert_eq!(prefixes.sources()[0].prefix_digest(),source["prefix_digest"].as_str().unwrap());assert_eq!(prefixes.sources()[0].origin_count(),1);
            assert_eq!(prefixes.financial().prefix_digest(),financial["prefix_digest"].as_str().unwrap());assert_eq!(prefixes.financial().origin_count(),2);
            // First-pass output cannot escape through the verifier capability.
            let mut bad=EconomicAuditStream::prefix_producer(&pending,0,&mut session).unwrap();
            financial_seed["pending_operation_key"]=json!(key("different"));
            assert!(bad.set_financial_prefix(FinancialPrefixInput{fields:&financial_seed,through:2},&mut session).is_err());assert!(session.check_cancelled().is_err());
        }
        for mode in 0..15{
            let financial=if mode>=11{&long_financial}else{&financial};
            let financial_document=if mode>=11{&long_document}else{&financial_document};
            let expected_combined=if mode>=11{&long_combined}else{&expected_combined};
            let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut stream=EconomicAuditStream::new(&context,&financial_document.reference,&mut session).unwrap();
            let source_body=session.parse_representation(&encoded(&source),RepresentationKind::Record).unwrap();
            stream.add_source_header(&source_document.reference,source_body.value(),&mut session).unwrap();drop(source_body);
            let financial_body=session.parse_representation(&encoded(&financial),RepresentationKind::Record).unwrap();
            stream.set_financial_header(&financial_document.reference,financial_body.value(),&mut session).unwrap();drop(financial_body);
            if mode>=4{
                // Intrinsics are minted in the earlier original owners; all
                // witness/membership work stays in this actual consumer owner.
                let mut historical=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let mut table=ValidatedOriginPointers::new(&mut historical).unwrap();
                for (offset,value) in [(17u64,&first),(117,&second)]{let p=historical.parse_representation(&encoded(value),RepresentationKind::Record).unwrap();table.record_at(&p,offset,&mut historical).unwrap();}
                if mode>=11{for value in long_pointers.iter().skip(2){let p=historical.parse_representation(&encoded(value),RepresentationKind::Record).unwrap();table.record_at(&p,217,&mut historical).unwrap();}}
                let block=table.block(0,&mut session).unwrap();
                assert_eq!(block.sequences(&mut session).unwrap()[..3],[Some(1),Some(2),if mode>=11{Some(3)}else{None}]);
                let entries=block.located_entries(&mut session).unwrap();
                assert_eq!(entries[0].sequence(),1);
                assert_eq!((entries[0].scratch_offset(),entries[0].canonical_length()),(17,encoded(&first).len()));
                let mut row=[0;ORIGIN_POINTER_MAX_BYTES];assert_eq!(entries[0].read_buffer(&mut row).len(),encoded(&first).len());
                assert_eq!(entries[0].scratch_location(),Some((17,encoded(&first).len())));
                assert_eq!(entries[1].scratch_location(),Some((117,encoded(&second).len())));
                let a=encoded(&first);let b=encoded(&second);
                if mode>=9{
                    let batch=table.batch(0,&mut session).unwrap();
                    assert_eq!(batch.entries(&mut session).unwrap()[1].sequence(),2);
                    if mode>=11{
                        let masks=if mode==12{[u32::MAX,2]}else{[u32::MAX,1]};
                        let prepared=stream.prepare_origin_batch(&batch,&masks,&[&[1,0]],&mut session);
                        if mode==12{assert!(prepared.is_err());assert!(session.check_cancelled().is_err());continue;}
                        let mut prepared=prepared.unwrap();let mut raw:Vec<Vec<u8>>=long_pointers.iter().map(encoded).collect();
                        assert_eq!(prepared.union_masks(),[u32::MAX,1]);
                        if mode==13{raw[32]=encoded(&pointer("wrong","33","8"));}
                        if mode==14{raw.swap(0,1);}
                        let first_chunk=prepared.push_chunk(&raw[..32].iter().map(Vec::as_slice).collect::<Vec<_>>(),&mut session);
                        if mode==14{assert!(first_chunk.is_err());assert!(session.check_cancelled().is_err());continue;}
                        first_chunk.unwrap();
                        let result=prepared.push_chunk(&[&raw[32]],&mut session);
                        if mode==13{assert!(result.is_err());assert!(session.check_cancelled().is_err());continue;}
                        result.unwrap();prepared.finish(&mut session).unwrap();let result=stream.finish(&mut session).unwrap();
                        assert_eq!(result.bound_digest(&context,&financial_document.reference,&mut session).unwrap(),expected_combined);continue;
                    }
                    let mut prepared=stream.prepare_origin_batch(&batch,&[3],&[&[1]],&mut session).unwrap();
                    if mode==10{assert!(prepared.finish(&mut session).is_err());assert!(session.check_cancelled().is_err());continue;}
                    prepared.push_chunk(&[&a,&b],&mut session).unwrap();prepared.finish(&mut session).unwrap();
                    let result=stream.finish(&mut session).unwrap();assert_eq!(result.bound_digest(&context,&financial_document.reference,&mut session).unwrap(),expected_combined);continue;
                }
                if mode==8{
                    stream.push_origin_block(&block,1,&[1],&[&a],&mut session).unwrap();
                    let second_block=table.block(1,&mut session).unwrap();
                    stream.push_origin_block(&second_block,1,&[0],&[&b],&mut session).unwrap();
                    let result=stream.finish(&mut session).unwrap();
                    assert_eq!(result.bound_digest(&context,&financial_document.reference,&mut session).unwrap(),expected_combined);continue;
                }
                let changed=encoded(&pointer("changed","2","7"));
                let before=session.usage().semantic_work;
                let result=stream.push_origin_block(&block,3,&[if mode==6{4}else{1}],&[&a,if mode==5{&changed}else{&b}],&mut session);
                if mode==5||mode==6{assert!(result.is_err());assert!(session.check_cancelled().is_err());continue;}
                assert!(result.is_ok());
                // Source-derived callback bound, not a measured admission value.
                assert!(session.usage().semantic_work-before<=160+60);
                if mode==7{assert!(stream.push_origin_block(&block,3,&[1],&[&a,&b],&mut session).is_err());assert!(session.check_cancelled().is_err());continue;}
                let result=stream.finish(&mut session).unwrap();
                assert_eq!(result.bound_digest(&context,&financial_document.reference,&mut session).unwrap(),expected_combined);
                continue;
            }
            let foreign_arena=AuditArena::new().unwrap();
            let mut foreign_session=OriginalAuditSession::for_resource_profile_v2(&foreign_arena).unwrap();
            let pointer=if mode==3{foreign_session.parse_representation(&encoded(&first),RepresentationKind::Record).unwrap()}else{session.parse_representation(&encoded(&first),RepresentationKind::Record).unwrap()};
            let members=if mode==1{vec![OriginWitness::Financial]}else{vec![OriginWitness::Financial,OriginWitness::Source(0)]};
            if mode==3{let error=stream.push_origin(&pointer,&members,&mut session).unwrap_err();assert!(error.detail.contains("another attempt"));assert!(session.check_cancelled().is_err());continue;}
            stream.push_origin(&pointer,&members,&mut session).unwrap();
            if mode==2{assert!(stream.push_origin(&pointer,&members,&mut session).is_err());assert!(session.check_cancelled().is_err());continue;}
            drop(pointer);
            let pointer=session.parse_representation(&encoded(&second),RepresentationKind::Record).unwrap();stream.push_origin(&pointer,&[OriginWitness::Financial],&mut session).unwrap();drop(pointer);
            let result=stream.finish(&mut session);
            if mode==1{assert!(result.is_err());assert!(session.check_cancelled().is_err());}
            else{let result=result.unwrap();assert_eq!(result.bound_digest(&context,&financial_document.reference,&mut session).unwrap(),expected_combined);assert!(session.check_cancelled().is_ok());
                let mut foreign=OriginalAuditSession::for_resource_profile_v2(&foreign_arena).unwrap();
                assert!(result.bound_digest(&context,&financial_document.reference,&mut foreign).unwrap_err().detail.contains("another attempt"));assert!(session.check_cancelled().is_err());assert!(foreign.work().visit(w::ValidationVisit::Constraint).is_err());}
        }
    }
    #[test]
    fn pointer_intrinsics_are_attempt_local_ordered_and_use_existing_word_rate(){
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut table=ValidatedOriginPointers::new(&mut s).unwrap();
        let p=s.parse_representation(&encoded(&pointer("first","1","6")),RepresentationKind::Record).unwrap();
        assert_eq!(table.record(&p,&mut s).unwrap(),0);
        let foreign=AuditArena::new().unwrap();let mut other=OriginalAuditSession::for_resource_profile_v2(&foreign).unwrap();
        assert!(table.block(0,&mut other).is_err());assert!(s.check_cancelled().is_err());
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let before=s.usage().semantic_work;
        ValidationWork::arithmetic(s.work(),ArithmeticOperation::PopulationCount,&[&u64::MAX.into()]).unwrap();
        ValidationWork::arithmetic(s.work(),ArithmeticOperation::BitOr,&[&u64::MAX.into(),&1u64.into()]).unwrap();
        ValidationWork::arithmetic(s.work(),ArithmeticOperation::BitAnd,&[&u64::MAX.into(),&1u64.into()]).unwrap();
        assert_eq!(s.usage().semantic_work-before,8);
        let mut table=ValidatedOriginPointers::new(&mut s).unwrap();let p=s.parse_representation(&encoded(&pointer("gap","9","6")),RepresentationKind::Record).unwrap();
        table.record(&p,&mut s).unwrap();assert!(table.record(&p,&mut s).is_err());assert!(s.check_cancelled().is_err());
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut table=ValidatedOriginPointers::new(&mut s).unwrap();let p=s.parse_representation(&encoded(&pointer("unlocated","9","6")),RepresentationKind::Record).unwrap();
        table.record(&p,&mut s).unwrap();let block=table.block(0,&mut s).unwrap();
        assert!(block.entries(&mut s).unwrap()[0].scratch_location().is_none());
        assert!(block.located_entries(&mut s).is_err());assert!(s.check_cancelled().is_err());
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut table=ValidatedOriginPointers::new(&mut s).unwrap();let mut maximum=pointer("max","9223372036854775807","6");
        let id="\u{10ffff}".repeat(128);maximum["operation_key"]=json!({"logical_store_id":id,"scope":[id,id],"source":"\u{1}".repeat(256),"operation_id":id});
        let bytes=encoded(&maximum);assert!(bytes.len()<=ORIGIN_POINTER_MAX_BYTES);
        let p=s.parse_representation(&bytes,RepresentationKind::Record).unwrap();table.record_at(&p,19,&mut s).unwrap();
        let block=table.block(0,&mut s).unwrap();let entry=&block.located_entries(&mut s).unwrap()[0];let mut row=[0;ORIGIN_POINTER_MAX_BYTES];
        assert_eq!(entry.read_buffer(&mut row).len(),bytes.len());
        let arena=AuditArena::new().unwrap();let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();let mut table=ValidatedOriginPointers::new(&mut s).unwrap();
        maximum["operation_ref"]["id"]=json!(format!("wop_extra_{}","6".repeat(64)));
        let p=s.parse_representation(&encoded(&maximum),RepresentationKind::Record).unwrap();assert!(table.record_at(&p,19,&mut s).is_err());assert!(s.check_cancelled().is_err());
    }
}
