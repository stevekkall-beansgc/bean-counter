//! Exact in-memory operands. Origin membership and index completeness are host obligations.
use super::{
    CoreMeter, codec,
    data::{EconomicRecordView, ExactRef, OperationKey, RecordInput},
    record_body::RecordBody,
};
use crate::{
    Error, Result, canonical,
    canonical::audit_arena::{AuditArena, ExternalBufferLease},
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{mem, ops::Deref};
fn invalid(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
fn admit_body(m:&mut CoreMeter,parent_data:bool,selected_bytes:&mut usize,selected_refs:&mut usize,raw:&[u8])->Result<()> {
    if !parent_data{return m.borrowed_body(raw);}
    // Structural bounds on this actual parent DATA set, not economic work or
    // resolved-byte credit. Independent child Records repeats its own work.
    m.visit("resolve.parent_selected_body")?;
    let bytes=w::usize_add(m,*selected_bytes,raw.len())?;
    let refs=w::usize_add(m,*selected_refs,1)?;
    if m.resource_limits().is_some(){m.check_resolved_dimensions(refs,bytes)?;}
    else if w::compare(m,bytes,8388608usize)?.is_gt()||w::compare(m,refs,4096usize)?.is_gt(){return Err(invalid("parent selected input closure"));}
    *selected_bytes=bytes;*selected_refs=refs;Ok(())
}
// Canonical key/part serialization belongs to the named owning representation
// operation under fixed RP01. Digest/domain/Ref association remains semantic.
// The local artifact is destroyed after its actual hash pass; no Vec escapes.
fn hash_value(m: &mut CoreMeter, digest: &mut Sha256, value: &Value) -> Result<()> {
    if let Some(arena) = m.representation_arena() {
        let bytes = m.canonical_owned_representation(&arena, value, RepresentationKind::Record)?;
        m.hash_bytes(bytes.bytes().len())?;
        digest.update(bytes.bytes());
    } else {
        let bytes = codec::encode(m, value, 4194304)?;
        m.hash_bytes(bytes.len())?;
        digest.update(&bytes);
    }
    Ok(())
}
pub(super) fn hash(m: &mut CoreMeter, domain: &str, value: &Value) -> Result<String> {
    let prefix = format!("bean-counter/{domain}/1\0");
    m.visit("record.hash")?;
    m.hash_bytes(prefix.len())?;
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    if let Value::Array(parts) = value {
        // Frame punctuation does not narrow the body bound. Stream the array
        // instead of materializing body+frame behind a second 4MiB gate.
        m.visit("hash.array")?;
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"[");
        for (index, part) in parts.iter().enumerate() {
            m.visit("hash.part")?;
            if index > 0 {
                m.hash_bytes(1)?;
                m.write_bytes(1)?;
                digest.update(b",");
            }
            hash_value(m, &mut digest, part)?;
        }
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"]");
    } else {
        hash_value(m, &mut digest, value)?;
    }
    Ok(format!(
        "sha256:{}",
        canonical::hex_with_work(&digest.finalize(), m)?
    ))
}
// Parts must already be validated canonical JSON, never arbitrary wire bytes.
pub(super) fn hash_parts(
    m: &mut CoreMeter,
    domain: &str,
    parts: &[&[u8]],
    array: bool,
) -> Result<String> {
    let prefix = format!("bean-counter/{domain}/1\0");
    m.visit("record.hash_canonical")?;
    m.hash_bytes(prefix.len())?;
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    if array {
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"[");
    }
    for (i, part) in parts.iter().enumerate() {
        m.visit("hash.canonical_part")?;
        if i > 0 {
            m.hash_bytes(1)?;
            m.write_bytes(1)?;
            digest.update(b",");
        }
        m.hash_bytes(part.len())?;
        digest.update(part);
    }
    if array {
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"]");
    }
    Ok(format!(
        "sha256:{}",
        canonical::hex_with_work(&digest.finalize(), m)?
    ))
}
pub(super) fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<()> {
    reference_parts(m,&r.kind,&r.id,&r.digest)
}
pub(super) fn reference_parts(m:&mut CoreMeter,kind:&str,id:&str,digest:&str)->Result<()> {
    if !super::meter::cmp(m, &digest.len().into(), &71usize.into())?.is_eq()
        || !super::shape::same_text(m, digest.get(..7).unwrap_or(""), "sha256:")?
    {
        return Err(invalid("digest shape"));
    }
    for b in digest[7..].bytes() {
        m.scan_bytes(1)?;
        if !m.check("ref.hex", || {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        })? {
            return Err(invalid("digest hex"));
        }
    }
    if super::meter::cmp(m, &id.len().into(), &128usize.into())?.is_gt()
        || !super::meter::cmp(m, &id.len().into(), &65usize.into())?.is_gt()
    {
        return Err(invalid("reference identity"));
    }
    let prefix =
        id.rsplit_once('_')
            .ok_or_else(|| invalid("reference prefix"))?;
    if !super::meter::cmp(m, &prefix.1.len().into(), &64usize.into())?.is_eq() {
        return Err(invalid("reference suffix"));
    }
    for b in prefix.1.bytes() {
        m.scan_bytes(1)?;
        if !m.check("ref.id_hex", || {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        })? {
            return Err(invalid("reference suffix"));
        }
    }
    if !super::shape::same_text(m, kind.get(..9).unwrap_or(""), "workflow-")?
        || super::meter::cmp(m, &kind.len().into(), &64usize.into())?.is_gt()
    {
        return Err(invalid("reference kind"));
    }
    Ok(())
}
/// Invocation input representation only; neither variant proves host custody.
/// The private financial path borrows actual owning host artifacts, while the
/// legacy draft keeps its original independent raw input behavior.
pub(super) enum SuppliedRecords<'a> {
    Legacy(&'a [RecordInput]),
    BorrowedApp {records:&'a [EconomicRecordView<'a>],arena:AuditArena,sources:&'a [&'a super::source_records::InitialSourceCorrespondence]},
    Borrowed {
        records: &'a [EconomicRecordView<'a>],
        arena: &'a AuditArena,
    },
}
impl<'a> SuppliedRecords<'a> {
    pub(super) fn resolve<'inv, 'short>(
        self,
        m: &mut CoreMeter,
        key: &'short OperationKey,
        pending: &'short [RecordInput],
    ) -> Result<Records<'inv, 'short>>
    where
        'a: 'short,
    {
        match self {
            Self::Legacy(records) => {
                let mut pool = records.to_vec();
                pool.extend_from_slice(pending);
                Records::new(m, &pool)
            }
            Self::BorrowedApp{records,arena,sources}=>{
                let mut result=Records::borrowed_app(m,&arena,key,records,pending)?;
                result.install_initial_source_outputs(m,sources)?;Ok(result)
            },
            Self::Borrowed { records, arena } => Records::borrowed(m, arena, key, records, pending),
        }
    }
}
#[derive(Clone)]
enum RecordBytes<'raw> {
    Legacy(Vec<u8>),
    Borrowed(&'raw [u8]),
}
impl Deref for RecordBytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Legacy(v) => v,
            Self::Borrowed(v) => v,
        }
    }
}
impl AsRef<[u8]> for RecordBytes<'_> {
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}
struct SourceBirthFacts {
    state_position:usize,grant_position:usize,registered:RecordBody,accepted:[u8;32],accepted_len:usize,
}
struct SourceObservationFacts {state:RecordBody,verifier:RecordBody,root:RecordBody,app_catalog:bool}
struct SourceGrantFacts {ceiling_position:usize,at:i64,accepted:[u8;32],accepted_len:usize}
struct SourceOutputFacts {bytes:[u8;285],_ownership:ExternalBufferLease}
pub(super) struct SourceBirthCheck {
    pub registered:Option<RecordBody>,pub outputs:bool,
    positions:Option<(usize,usize,usize)>,owner:usize,
}
// Input partition DATA from the existing separate constructor parameters.
// This does not authenticate introduction; the held chronological owner must
// independently establish that partition. A pending header is never old I.
#[derive(Clone,Copy)]
enum OperandPartition { Preexisting, Pending }
pub(super) enum UsedCanonical<'a>{Preexisting(&'a [u8]),Pending(&'a [u8]),Absent}
struct Operand<'inv, 'raw> {
    reference: ExactRef,
    raw: RecordBytes<'raw>,
    body: Option<RecordBody>,
    intrinsic: Option<&'inv super::intrinsic_record::IntrinsicRecord<'inv>>,
    admitted: bool,
    // ONLY canonicality+exact schema/kind framing/digest+ID FAMILY were checked.
    // No per-kind semantic-ID, subject, authority, rights or currentness proof.
    canonical_frame_digest_checked: bool,
    reference_grammar_checked:bool,
    // Exact operand-position membership, not origin or semantic validation.
    consumed_in_invocation: bool,
    partition:OperandPartition,
    // Pure immutable facts proved earlier in THIS invocation only.
    source_birth:Option<SourceBirthFacts>,
    source_grant:Option<SourceGrantFacts>,
    source_observation:Option<SourceObservationFacts>,
    // Only the SAME producer's exact immutable output correspondence. This
    // does not include historical authority, custody or consumer predicates.
    initial_source_outputs:Option<Box<SourceOutputFacts>>,
}
pub(super) struct Records<'inv, 'raw> {
    operands: Vec<Operand<'inv, 'raw>>,
    index: super::record_index::IdentityIndex,
    consumed: Vec<ExactRef>,
    pending_consumed:usize,
    intrinsic_owner: Option<&'inv super::intrinsic_record::IntrinsicDataScope>,
    arena: Option<AuditArena>,
    _metadata: Option<ExternalBufferLease>,
    owner_key: Option<&'raw OperationKey>,
    app_records:bool,
    // Coordinator DATA lookup, never an economic inventory/semantic proof.
    parent_data:bool,
    parent_selected_bytes:usize,
    parent_selected_refs:usize,
}
pub(super) struct OwnedConsumed {
    pub references: Vec<ExactRef>,
    // The inventory's existing reservation covers these strings/capacity. It
    // remains live after all other table allocations are destroyed.
    _metadata: Option<ExternalBufferLease>,
}
impl<'inv, 'raw> Records<'inv, 'raw> {
    pub(super) fn new(m: &mut CoreMeter, inputs: &[RecordInput]) -> Result<Self> {
        m.visit("records.count")?;
        if super::meter::cmp(m, &inputs.len().into(), &4096usize.into())?.is_gt() {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved count"));
        }
        for input in inputs {
            m.visit("records.metadata_bound")?;
            for (length, maximum) in [
                (input.reference.kind.len(), 64usize),
                (input.reference.id.len(), 128),
                (input.reference.digest.len(), 71),
            ] {
                if super::meter::cmp(m, &length.into(), &maximum.into())?.is_gt() {
                    return Err(invalid("supplied reference metadata bound"));
                }
            }
        }
        let references: Vec<_> = inputs.iter().map(|input| &input.reference).collect();
        let index = super::record_index::IdentityIndex::build(m, &references)?;
        let mut operands: Vec<Operand> = Vec::new();
        for input in inputs {
            // Supplied pool metadata is inspected now; body work and byte
            // admission occur only when this exact operand is consumed.
            operands.push(Operand {
                reference: input.reference.clone(),
                raw: RecordBytes::Legacy(input.canonical_body.clone()),
                body: None,
                intrinsic: None,
                admitted: false,
                canonical_frame_digest_checked: false,reference_grammar_checked:false,
                consumed_in_invocation: false,
                partition:OperandPartition::Preexisting,
                source_birth:None,source_grant:None,source_observation:None,initial_source_outputs:None,
            });
        }
        Ok(Self {
            operands,
            index,
            consumed: vec![],
            pending_consumed:0,
            intrinsic_owner: None,
            arena: None,
            _metadata: None,
            owner_key: None,
            app_records:false,
            parent_data:false,parent_selected_bytes:0,parent_selected_refs:0,
        })
    }
    /// Borrow exact caller-owned bytes for ONE complete financial invocation.
    /// This inventory is DATA; the host still owns origins/snapshot/currentness.
    /// All metadata/index capacities and string copies are admitted before any
    /// producer. Body parse/representation leases are acquired lazily at use.
    pub(super) fn borrowed(
        m: &mut CoreMeter,
        arena: &AuditArena,
        key: &'raw OperationKey,
        inputs: &'raw [EconomicRecordView<'raw>],
        pending: &'raw [RecordInput],
    ) -> Result<Self> {
        Self::borrowed_mode(m,arena,key,inputs,pending,false,false)
    }
    /// Named optional-AP inventory. Only its existing immutable Invocation and
    /// CostLineOwner recipes need stable-ID/new-digest revision lookup. The app
    /// customer path preserves its ordinary locator and revision-specific IDs.
    /// Exact app document vocabulary; no AP stable-ID locator widening and no
    /// change to the raw legacy draft's recognized record domains.
    pub(super) fn borrowed_app(m:&mut CoreMeter,arena:&AuditArena,key:&'raw OperationKey,inputs:&'raw [EconomicRecordView<'raw>],pending:&'raw [RecordInput])->Result<Self>{
        let mut records=Self::borrowed_mode(m,arena,key,inputs,pending,false,false)?;
        records.app_records=true;Ok(records)
    }
    /// SAME paid body/index implementation for conservative parent DATA
    /// preselection. It cannot consume or relax the child's one-table claim.
    pub(super) fn borrowed_parent_app(m:&mut CoreMeter,arena:&AuditArena,key:&'raw OperationKey,inputs:&'raw [EconomicRecordView<'raw>])->Result<Self>{
        let mut records=Self::borrowed_mode(m,arena,key,inputs,&[],false,true)?;
        records.app_records=true;Ok(records)
    }
    pub(super) fn borrowed_ap(
        m:&mut CoreMeter,arena:&AuditArena,key:&'raw OperationKey,
        inputs:&'raw [EconomicRecordView<'raw>],pending:&'raw [RecordInput],
    )->Result<Self>{Self::borrowed_mode(m,arena,key,inputs,pending,true,false)}
    fn borrowed_mode(
        m:&mut CoreMeter,arena:&AuditArena,key:&'raw OperationKey,
        inputs:&'raw [EconomicRecordView<'raw>],pending:&'raw [RecordInput],ap_revisions:bool,parent_data:bool,
    )->Result<Self>{
        if parent_data{m.require_original_data_selection()?;}else{m.claim_borrowed_inventory()?;}
        let n = w::usize_add(m, inputs.len(), pending.len())?;
        if m.resource_limits().is_some() { m.check_resolved_dimensions(n,0)?; }
        else if w::compare(m, n, 4096usize)?.is_gt() {
            return Err(invalid("supplied borrowed count"));
        }
        let values = inputs
            .iter()
            .map(|v| (v.reference, v.canonical_body, OperandPartition::Preexisting))
            .chain(
                pending
                    .iter()
                    .map(|v| (&v.reference, v.canonical_body.as_slice(), OperandPartition::Pending)),
            );
        let mut string_bytes = 0;
        for (r, _, _) in values.clone() {
            m.visit("records.borrowed_metadata_bound")?;
            for (actual, maximum) in [
                (r.kind.len(), 64usize),
                (r.id.len(), 128),
                (r.digest.len(), 71),
            ] {
                if w::compare(m, actual, maximum)?.is_gt() {
                    return Err(invalid("supplied reference metadata bound"));
                }
                string_bytes = w::usize_add(m, string_bytes, actual)?;
            }
        }
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&string_bytes.into(), &2usize.into()],
        )?;
        let strings = string_bytes
            .checked_mul(2)
            .ok_or_else(|| invalid("borrowed string bound"))?;
        let index = 4 * mem::size_of::<usize>(); // ref vector + two merge vectors + retained index
        let item = w::usize_add(m, mem::size_of::<Operand>(), mem::size_of::<ExactRef>())?;
        let item = w::usize_add(m, item, index)?;
        ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&n.into(), &item.into()])?;
        let slots = n
            .checked_mul(item)
            .ok_or_else(|| invalid("borrowed slot bound"))?;
        let bytes = w::usize_add(m, slots, strings)?;
        let bytes = w::usize_add(m, bytes, 6 * mem::size_of::<Vec<usize>>())?;
        // Local construction and owning return controls coexist. Include the
        // actual Records header (also its parent-DATA counters), not only Vec
        // element capacity/string payload. Legacy raw construction is separate.
        let bytes = w::usize_add(m,bytes,2*mem::size_of::<Self>())?;
        // Serial frame-hash scratch: borrowed scope's two<=128-byte strings
        // and64-byte kind + small Value/Vec headers<=480; domain format<=128;
        // hex+digest formatting overlap<=192. Fixed1024 covers their sum.
        // Owning encoded fragments have independent live artifact leases.
        let bytes = w::usize_add(m, bytes, 1024)?;
        let metadata = arena.reserve_external_payload(bytes, m)?;
        let mut references = Vec::new();
        references
            .try_reserve_exact(n)
            .map_err(|_| invalid("borrowed refs allocation"))?;
        for (r, _, _) in values.clone() {
            m.visit("records.borrowed_index_ref")?;
            references.push(r);
        }
        let index = if ap_revisions{super::record_index::IdentityIndex::build_financial(m, &references)?}else{super::record_index::IdentityIndex::build(m,&references)?};
        let mut operands = Vec::new();
        operands
            .try_reserve_exact(n)
            .map_err(|_| invalid("borrowed operands allocation"))?;
        let mut consumed = Vec::new();
        consumed
            .try_reserve_exact(n)
            .map_err(|_| invalid("borrowed consumed allocation"))?;
        fn text(m: &mut CoreMeter, s: &str) -> Result<String> {
            m.scan_bytes(s.len())?;
            m.write_bytes(s.len())?;
            Ok(s.to_owned())
        }
        for (r, raw, partition) in values {
            m.visit("records.borrowed_operand")?;
            operands.push(Operand {
                reference: ExactRef {
                    kind: text(m, &r.kind)?,
                    id: text(m, &r.id)?,
                    digest: text(m, &r.digest)?,
                },
                raw: RecordBytes::Borrowed(raw),
                body: None,
                intrinsic: None,
                admitted: false,
                canonical_frame_digest_checked: false,reference_grammar_checked:false,
                consumed_in_invocation: false,
                partition,
                source_birth:None,source_grant:None,source_observation:None,initial_source_outputs:None,
            });
        }
        m.write_bytes(2*mem::size_of::<Self>())?;
        Ok(Self {
            operands,
            index,
            consumed,
            pending_consumed:0,
            intrinsic_owner: None,
            arena: Some(arena.shared_handle()),
            _metadata: Some(metadata),
            owner_key: Some(key),
            app_records:false,
            parent_data,parent_selected_bytes:0,parent_selected_refs:0,
        })
    }
    fn source_body_position(&self,m:&mut CoreMeter,reference:&ExactRef,value:&Value)->Result<Option<usize>>{
        if self.arena.is_none(){return Ok(None);}
        let Some(position)=self.index.find_exact(m,reference,|i|&self.operands[i].reference)? else{return Ok(None)};
        let operand=&self.operands[position];
        m.visit("source.exact_owning_body")?;
        Ok(if super::shape::same_ref(m,&operand.reference,reference)? && operand.canonical_frame_digest_checked && operand.body.as_ref().is_some_and(|v|v.is_same_owning_value(value)){Some(position)}else{None})
    }
    pub(super) fn install_initial_source_outputs(&mut self,m:&mut CoreMeter,facts:&[&super::source_records::InitialSourceCorrespondence])->Result<()>{
        let Some(arena)=self.arena.as_ref() else{return Err(invalid("source construction facts require owning records"));};
        m.visit("source.output_facts_presence")?;
        if facts.is_empty(){return Ok(());}
        // One bounded lookup Ref/formatting workspace. Only actual selected
        // facts allocate a separately preadmitted immutable per-operand Box.
        let _scratch=arena.reserve_external_payload(mem::size_of::<ExactRef>()+64+128+71+285+4*mem::size_of::<&str>(),m)?;
        for fact in facts{
            m.visit("source.install_actual_output_correspondence")?;
            let parts=fact.checked_parts(arena,m)?;let digest=parts[0];
            m.scan_bytes(71+64)?;m.write_bytes(71+69+26)?;
            let reference=ExactRef{kind:"workflow-source-enrollment".to_owned(),id:format!("wdoc_{}",&digest[7..]),digest:digest.to_owned()};
            let position=self.index.find_exact(m,&reference,|i|&self.operands[i].reference)?.ok_or_else(||invalid("construction fact lacks selected enrollment operand"))?;
            m.visit("source.output_fact_unique")?;
            if self.operands[position].initial_source_outputs.is_some(){return Err(invalid("duplicate selected source construction fact"));}
            let ownership=arena.reserve_external_payload(2*mem::size_of::<SourceOutputFacts>()+mem::size_of::<Box<SourceOutputFacts>>(),m)?;
            m.scan_bytes(285)?;m.write_bytes(2*285+2*mem::size_of::<SourceOutputFacts>())?;
            let mut bytes=[0;285];
            bytes[..71].copy_from_slice(parts[0].as_bytes());bytes[71..142].copy_from_slice(parts[1].as_bytes());
            bytes[142..213].copy_from_slice(parts[2].as_bytes());bytes[213..].copy_from_slice(parts[3].as_bytes());
            self.operands[position].initial_source_outputs=Some(Box::new(SourceOutputFacts{bytes,_ownership:ownership}));
        }
        Ok(())
    }
    fn initial_source_outputs(&self,m:&mut CoreMeter,e:usize,enrollment_ref:&ExactRef,state_ref:&ExactRef,grant_ref:&ExactRef)->Result<bool>{
        m.visit("source.output_correspondence_presence")?;
        let Some(facts)=self.operands[e].initial_source_outputs.as_ref() else{return Ok(false)};
        let bytes=&facts.bytes;
        Ok(super::shape::same_bytes(m,&bytes[..71],enrollment_ref.digest.as_bytes())?
            &&super::shape::same_bytes(m,&bytes[71..142],grant_ref.digest.as_bytes())?
            &&super::shape::same_bytes(m,&bytes[142..213],state_ref.digest.as_bytes())?
            &&super::shape::same_bytes(m,&bytes[213..],state_ref.id.as_bytes())?
            // Document IDs are part of the exact full Ref, even when their
            // spelling is determined by the same body digest. Do not infer
            // the supplied grant ID merely from a matching digest/ID family.
            &&super::shape::same_bytes(m,grant_ref.id.as_bytes().get(..5).unwrap_or(&[]),b"wdoc_")?
            &&super::shape::same_bytes(m,grant_ref.id.as_bytes().get(5..).unwrap_or(&[]),&bytes[78..142])?)
    }
    fn source_context_owner(&self,m:&mut CoreMeter,c:&super::data::DraftContextData)->Result<bool>{
        let Some(owner)=self.owner_key else{return Ok(false)};
        if !super::shape::same_text(m,&owner.logical_store_id,&c.key.logical_store_id)?{return Ok(false);}
        for(a,b)in owner.scope.iter().zip(&c.key.scope){if !super::shape::same_text(m,a,b)?{return Ok(false);}}
        Ok(true)
    }
    pub(super) fn source_birth_facts(&self,m:&mut CoreMeter,c:&super::data::DraftContextData,enrollment:&Value,state:&Value,grant:&Value,customer:&Value,enrollment_ref:&ExactRef,state_ref:&ExactRef,grant_ref:&ExactRef)->Result<SourceBirthCheck>{
        let mut check=SourceBirthCheck{registered:None,outputs:false,positions:None,owner:std::ptr::from_ref(self) as usize};
        if !self.source_context_owner(m,c)?{return Ok(check);}
        let Some(e)=self.source_body_position(m,enrollment_ref,enrollment)?else{return Ok(check)};
        let Some(s)=self.source_body_position(m,state_ref,state)?else{return Ok(check)};
        let Some(g)=self.source_body_position(m,grant_ref,grant)?else{return Ok(check)};
        check.positions=Some((e,s,g));check.outputs=self.initial_source_outputs(m,e,enrollment_ref,state_ref,grant_ref)?;
        let Some(facts)=self.operands[e].source_birth.as_ref()else{return Ok(check)};
        if !w::compare(m,s,facts.state_position)?.is_eq()||!w::compare(m,g,facts.grant_position)?.is_eq(){return Ok(check);}
        if !super::shape::same_bytes(m,c.accepted_at.as_bytes(),&facts.accepted[..facts.accepted_len])?{return Ok(check);}
        let mode=super::shape::text(m,enrollment,"installation_mode")?;
        if !super::shape::same_text(m,mode,&c.installation_mode)?{return Ok(check);}
        let registered_customer=super::shape::field(m,&facts.registered,"customer")?;
        super::shape::equal(m,registered_customer,customer)?;
        m.visit("source.reuse_same_invocation_birth")?;
        check.registered=Some(facts.registered.clone());Ok(check)
    }
    pub(super) fn remember_source_birth(&mut self,m:&mut CoreMeter,c:&super::data::DraftContextData,check:&SourceBirthCheck,registered:&RecordBody)->Result<()> {
        let Some((e,s,g))=check.positions else{return Ok(())};
        if !m.check("source.birth_checked_inventory",||check.owner==std::ptr::from_ref(self) as usize)?{return Err(invalid("birth positions belong to another invocation inventory"));}
        if w::compare(m,c.accepted_at.len(),32usize)?.is_gt(){return Err(invalid("validated source time representation"));}
        m.scan_bytes(c.accepted_at.len())?;m.write_bytes(c.accepted_at.len())?;
        let mut accepted=[0;32];accepted[..c.accepted_at.len()].copy_from_slice(c.accepted_at.as_bytes());
        m.visit("source.retain_same_invocation_birth")?;
        self.operands[e].source_birth=Some(SourceBirthFacts{state_position:s,grant_position:g,registered:registered.clone(),accepted,accepted_len:c.accepted_at.len()});
        Ok(())
    }
    /// Reuse only a fully checked immutable grant intersection from this
    /// inventory. Source/principal/namespace identity and requested operation,
    /// slot/activity/product/rights are STILL checked at every consuming use.
    pub(super) fn source_grant(
        &mut self,m:&mut CoreMeter,owner:&super::representation::InvocationToken,
        reference:&ExactRef,g:&Value,enrollment_ref:&ExactRef,enrollment:&Value,
        c:&super::data::DraftContextData,customer:&Value,activity:&str,product:&str,
        slot:&str,at:&num_bigint::BigInt,mode:super::authority::EconomicMode,
    )->Result<()> {
        m.visit("source.grant_exact_instant")?;
        let instant=i64::try_from(at).map_err(|_|invalid("source grant timestamp integer"))?;
        let ceiling=super::shape::field(m,enrollment,"ceiling")?;
        let positions=if self.source_context_owner(m,c)?{
            match(self.source_body_position(m,reference,g)?,self.source_body_position(m,enrollment_ref,enrollment)?){(Some(g),Some(e))=>Some((g,e)),_=>None}
        }else{None};
        let mut reusable=false;
        if let Some((g,e))=positions{
            if let Some(facts)=self.operands[g].source_grant.as_ref(){
                reusable=w::compare(m,e,facts.ceiling_position)?.is_eq()&&w::compare(m,instant,facts.at)?.is_eq()&&super::shape::same_bytes(m,c.accepted_at.as_bytes(),&facts.accepted[..facts.accepted_len])?;
            }
        }
        if !reusable{super::authority::grant_shape(m,g)?;}
        super::authority::grant_identity(m,g,ceiling,c)?;
        if !reusable{super::authority::grant_intersection(m,owner,g,ceiling,at)?;}
        super::authority::grant_coverage(m,g,customer,activity,product,slot,mode.operation(),mode.rights())?;
        if !reusable{if let Some((g,e))=positions{
            if w::compare(m,c.accepted_at.len(),32usize)?.is_gt(){return Err(invalid("validated grant time representation"));}
            m.scan_bytes(c.accepted_at.len())?;m.write_bytes(c.accepted_at.len())?;
            let mut accepted=[0;32];accepted[..c.accepted_at.len()].copy_from_slice(c.accepted_at.as_bytes());
            m.visit("source.retain_same_invocation_grant_intersection")?;
            self.operands[g].source_grant=Some(SourceGrantFacts{ceiling_position:e,at:instant,accepted,accepted_len:c.accepted_at.len()});
        }}
        Ok(())
    }
    /// SourceAuthority calls this AFTER its actual per-use observation fields
    /// and root-basis joins. The cached portion is bound to the SAME owning
    /// state/observation/verifier/root values in this inventory only. It never
    /// attests the serialized prefix's truth or supplies current-head custody.
    pub(super) fn source_observation_after_joins(&mut self,m:&mut CoreMeter,c:&super::data::DraftContextData,reference:&ExactRef,state:&RecordBody,observation:&RecordBody,verifier:&RecordBody,root:&RecordBody,app_catalog:bool)->Result<()> {
        let position=if self.source_context_owner(m,c)?{self.source_body_position(m,reference,observation)?}else{None};
        if let Some(position)=position{
            if let Some(facts)=self.operands[position].source_observation.as_ref(){
                let same=m.check("source.observation_same_catalog",||facts.app_catalog==app_catalog)?
                    &&m.check("source.observation_same_state",||facts.state.is_same_owning_value(state))?
                    &&m.check("source.observation_same_verifier",||facts.verifier.is_same_owning_value(verifier))?
                    &&m.check("source.observation_same_root",||facts.root.is_same_owning_value(root))?;
                if same{return Ok(());}
            }
        }
        if app_catalog{super::inventory::observation::validate_app_catalog(m,c,state,observation,verifier,root)?;}
        else{super::inventory::observation::validate(m,c,state,observation,verifier,root)?;}
        if let Some(position)=position{
            m.visit("source.retain_same_invocation_observation")?;
            self.operands[position].source_observation=Some(SourceObservationFacts{state:state.clone(),verifier:verifier.clone(),root:root.clone(),app_catalog});
        }
        Ok(())
    }
    /// DATA-only immutable representation reuse. The host must independently
    /// authenticate the original and snapshot; this cannot authorize a commit.
    pub(super) fn with_intrinsic_evidence(
        mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        owner: &'inv super::intrinsic_record::IntrinsicDataScope,
        inputs: &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    ) -> Result<Self> {
        if !super::shape::same_text(m, owner.logical_store_id(), &key.logical_store_id)? {
            return Err(invalid("intrinsic DATA store association"));
        }
        for (actual, expected) in owner.scope().iter().zip(&key.scope) {
            if !super::shape::same_text(m, actual, expected)? {
                return Err(invalid("intrinsic DATA scope association"));
            }
        }
        if super::meter::cmp(m, &inputs.len().into(), &4096usize.into())?.is_gt() {
            return Err(invalid("intrinsic DATA count"));
        }
        for record in inputs {
            m.visit("intrinsic.pool_member")?;
            let reference = record.reference();
            record.consume(m, owner, reference)?;
            let position = self
                .index
                .find_exact(m, reference, |i| {
                    &self.operands[i].reference
                })?
                .ok_or_else(|| invalid("intrinsic DATA absent from supplied operands"))?;
            let operand = &mut self.operands[position];
            if !super::shape::same_ref(m, &operand.reference, reference)?
                || !super::shape::same_bytes(m, &operand.raw, record.canonical_body())?
            {
                return Err(invalid("intrinsic DATA exact operand mismatch"));
            }
            m.visit("intrinsic.pool_duplicate")?;
            if operand.intrinsic.is_some() {
                return Err(invalid("duplicate intrinsic DATA declaration"));
            }
            operand.intrinsic = Some(record);
        }
        self.intrinsic_owner = Some(owner);
        Ok(self)
    }
    /// Match the exact immutable body, then perform paid owner/Ref association.
    /// Only the already-established content/schema properties are reusable.
    pub(super) fn evidence_facts(
        &self,
        m: &mut CoreMeter,
        body: &Value,
    ) -> Result<Option<&super::intrinsic_record::RetainedEvidenceFacts>> {
        let Some(owner) = self.intrinsic_owner else {
            return Ok(None);
        };
        for operand in &self.operands {
            m.visit("intrinsic.evidence_locator")?;
            let Some(record) = operand.intrinsic else {
                continue;
            };
            if super::shape::same(m, record.body(), body)? {
                return Ok(Some(record.consume(m, owner, &operand.reference)?));
            }
        }
        // Ordinary uncommitted/new operands still run their raw validator.
        Ok(None)
    }
    /// Reuse ONLY grammar actually established by an earlier successful Ref
    /// check in THIS table. Construction validates metadata lengths, not full
    /// grammar. First use and misses execute reference() before any body use;
    /// every kind/schema/store/scope/body/authority check still runs.
    fn checked_reference(&self,m:&mut CoreMeter,r:&ExactRef)->Result<Option<usize>>{
        if self.arena.is_some(){
            if let Some(position)=self.index.find_exact(m,r,|i|&self.operands[i].reference)?{
                if super::shape::same_ref(m,&self.operands[position].reference,r)?{
                    if m.check("resolve.reference_grammar_previously_checked",||self.operands[position].reference_grammar_checked)?{
                        m.visit("resolve.same_inventory_reference_grammar")?;
                    }else{reference(m,r)?;}
                    return Ok(Some(position));
                }
            }
        }
        // Misses and raw legacy lookups retain the ordinary grammar validator.
        reference(m,r)?;Ok(None)
    }
    pub(super) fn optional(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        kind: &str,
        schema: &str,
    ) -> Result<Option<RecordBody>> {
        if self.arena.is_some(){
            let position=self.checked_reference(m,r)?;
            m.visit("resolve.optional_identity")?;
            if let Some(position)=position{return self.get_grammatical_schemas(m,key,r,kind,&[schema],false,Some(position)).map(Some);}
            // In once-only identity tables, a valid but different digest at
            // the same (kind,id) is a conflicting required Ref, not absence.
            // Preserve the old optional miss/conflict behavior and ordering.
            return match self.index.find_exact(m,r,|i|&self.operands[i].reference)?{
                None=>Ok(None),Some(_)=>self.get_grammatical_schemas(m,key,r,kind,&[schema],false,None).map(Some),
            };
        }
        reference(m, r)?;
        m.visit("resolve.optional_identity")?;
        match self
            .index
            .find_exact(m, r, |i| &self.operands[i].reference)?
        {
            None => Ok(None),
            Some(_) => self.get_grammatical(m, key, r, kind, schema).map(Some),
        }
    }
    /// Validate a genuinely pending financial-observation output under THIS
    /// existing inventory owner. No second inventory claim or old-I insertion
    /// is permitted. A reused verifier is resolved through the actual table.
    pub(super) fn observation_output(&mut self,m:&mut CoreMeter,key:&OperationKey,input:EconomicRecordView<'_>,kind:&str,schema:&str)->Result<RecordBody>{
        m.visit("resolve.pending_observation_recipe")?;
        if !m.check("resolve.pending_economic_inventory",||!self.parent_data)?{return Err(invalid("pending observation requires economic inventory"));}
        let financial=super::shape::same_text(m,kind,"workflow-financial-history-observation")?;
        let allowed=if financial{super::shape::same_text(m,schema,"ledger-workflow-financial-history-observation/3")?}
            else{super::shape::same_text(m,kind,"workflow-authority-audit-verifier")?&&super::shape::same_text(m,schema,"ledger-authority-audit-verifier/2")?};
        if !allowed{return Err(invalid("pending observation exact recipe"));}
        if let Some(body)=self.optional(m,key,input.reference,kind,schema)?{
            if financial{return Err(invalid("pending financial observation is already preexisting"));}
            let canonical=self.canonical(m,input.reference)?;
            if !super::shape::same_bytes(m,canonical,input.canonical_body)?{return Err(invalid("reused financial verifier body mismatch"));}
            return Ok(body);
        }
        if !super::shape::same_text(m,&input.reference.kind,kind)?{return Err(invalid("pending observation kind"));}
        let arena=self.arena.as_ref().ok_or_else(||invalid("owning pending financial observation"))?;
        // The ONE child inventory owns the total input/pending-body counter.
        // These output bytes are not appended to preexisting consumed I.
        m.borrowed_body(input.canonical_body)?;
        let parsed=m.parse_owned_representation(arena,input.canonical_body,RepresentationKind::Record)?;
        codec::validate_selected_types(m,parsed.value())?;
        let canonical=m.canonical_owned_representation(arena,parsed.value(),RepresentationKind::Record)?;
        if !super::shape::same_bytes(m,canonical.bytes(),input.canonical_body)?{return Err(invalid("noncanonical pending observation body"));}
        let actual=super::shape::text(m,parsed.value(),"schema")?;
        if !super::shape::same_text(m,actual,schema)?{return Err(invalid("pending observation schema"));}
        let schema_value=super::shape::field(m,parsed.value(),"schema")?;
        let schema_bytes=m.canonical_owned_representation(arena,schema_value,RepresentationKind::SourceDefinition)?;
        let digest=hash_parts(m,"workflow-document",&[schema_bytes.bytes(),canonical.bytes()],true)?;
        if !super::shape::same_text(m,&digest,&input.reference.digest)?{return Err(invalid("pending observation body hash mismatch"));}
        m.visit("resolve.pending_id_family")?;
        let id=input.reference.id.strip_prefix("wdoc_").ok_or_else(||invalid("pending observation ID family"))?;
        if !super::shape::same_text(m,id,&digest[7..])?{return Err(invalid("pending observation content ID"));}
        RecordBody::owned(arena,parsed,m)
    }
    pub(super) fn get(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        expected_schema: &str,
    ) -> Result<RecordBody> {
        let position=self.checked_reference(m,r)?;
        self.get_grammatical_schemas(m,key,r,expected_kind,&[expected_schema],false,position)
    }
    pub(super) fn get_parsed(
        &mut self,
        m: &mut CoreMeter,
        owner: &super::representation::InvocationToken,
        key: &OperationKey,
        r: &super::representation::ParsedRef<'_>,
        kind: &str,
        schema: &str,
    ) -> Result<RecordBody> {
        r.require_owner(m, owner)?;
        self.get_grammatical(m, key, r.exact(), kind, schema)
    }
    fn get_grammatical(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        expected_schema: &str,
    ) -> Result<RecordBody> {
        self.get_grammatical_schemas(m, key, r, expected_kind, &[expected_schema], false,None)
    }
    pub(super) fn get_parsed_schemas(
        &mut self,
        m: &mut CoreMeter,
        owner: &super::representation::InvocationToken,
        key: &OperationKey,
        reference: &super::representation::ParsedRef<'_>,
        kind: &str,
        schemas: &[&str],
    ) -> Result<RecordBody> {
        reference.require_owner(m, owner)?;
        self.get_grammatical_schemas(m, key, reference.exact(), kind, schemas, false,None)
    }
    /// Exact closed saved-schema dispatch. The resolved body chooses only among
    /// the caller's code-owned per-kind versions; kind/domain/hash checks are
    /// identical, with the ACTUAL selected schema in the document hash tuple.
    pub(super) fn get_schemas(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        schemas: &[&str],
    ) -> Result<RecordBody> {
        let position=self.checked_reference(m,r)?;
        self.get_grammatical_schemas(m, key, r, expected_kind, schemas, false,position)
    }
    /// Supplemental immutable provenance, not an intrinsic per-kind proof or
    /// an authorization operand. The host must complete its actual origin.
    pub(super) fn provenance(&mut self,m:&mut CoreMeter,key:&OperationKey,r:&ExactRef)->Result<RecordBody>{
        let position=self.checked_reference(m,r)?;
        self.get_grammatical_schemas(m,key,r,&r.kind,&[],true,position)
    }
    fn get_grammatical_schemas(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        schemas: &[&str],
        provenance_only:bool,
        checked_reference_position:Option<usize>,
    ) -> Result<RecordBody> {
        // Framing reuse is owned by this invocation's store/scope. A cached
        // hash whose tuple included the original scope cannot authorize a
        // subsequent lookup under another scope, even with an exact Ref.
        if let Some(owner) = self.owner_key {
            if !super::shape::same_text(m, &owner.logical_store_id, &key.logical_store_id)? {
                return Err(invalid("borrowed inventory store owner"));
            }
            for (a, b) in owner.scope.iter().zip(&key.scope) {
                if !super::shape::same_text(m, a, b)? {
                    return Err(invalid("borrowed inventory scope owner"));
                }
            }
        }
        m.visit("resolve.expected_kind")?;
        if !super::shape::same_text(m, &r.kind, expected_kind)? {
            return Err(invalid("wrong required kind"));
        }
        let mut found = None;
        let mut canonical_bytes = None;
        let mut intrinsic_reused = false;
        let mut checked_position = None;
        let mut resolved_position = None;
        let selected_position=match checked_reference_position{Some(position)=>Some(position),None=>self.index.find_exact(m,r,|i|&self.operands[i].reference)?};
        if let Some(position) = selected_position {
            resolved_position=Some(position);
            let operand = &mut self.operands[position];
            m.visit("resolve.ref_lookup")?;
            if super::shape::same_ref(m, &operand.reference, r)? {
                if self.arena.is_some(){
                    // Every entry to this private function is preceded by
                    // reference(), or by same-invocation ParsedRef::parse +
                    // owner check, or the already-established marker above.
                    // Record only grammar, never schema/type/body semantics.
                    m.write_bytes(mem::size_of::<bool>())?;
                    operand.reference_grammar_checked=true;
                }
                if let Some(record) = operand.intrinsic {
                    let owner = self
                        .intrinsic_owner
                        .ok_or_else(|| invalid("intrinsic DATA owner missing"))?;
                    record.consume(m, owner, r)?;
                    if let Some(arena) = &self.arena {
                        if !operand.admitted {
                            admit_body(m,self.parent_data,&mut self.parent_selected_bytes,&mut self.parent_selected_refs,&operand.raw)?;
                            operand.admitted = true;
                        }
                        if operand.body.is_none() {
                            let value = arena.clone_value(record.body(), m)?;
                            operand.body = Some(RecordBody::owned(arena, value, m)?);
                        }
                        found = operand.body.clone();
                    } else {
                        m.resolved_body(&format!("{}:{}", r.kind, r.id), &operand.raw)?;
                        found = Some(RecordBody::Legacy(record.body().clone()));
                    }
                    intrinsic_reused = true;
                } else if operand.body.is_none() {
                    let maximum = match operand.reference.kind.as_str() {
                        "workflow-authority" | "workflow-economic-authority" => 65536,
                        "workflow-product"
                        | "workflow-policy"
                        | "workflow-fact-schema"
                        | "workflow-slot-template"
                        | "workflow-compiler-descriptor"
                        | "workflow-source-enrollment"
                        | "workflow-source-grant" => 262144,
                        _ => 4194304,
                    };
                    if let Some(arena) = &self.arena {
                        if !operand.admitted {
                            admit_body(m,self.parent_data,&mut self.parent_selected_bytes,&mut self.parent_selected_refs,&operand.raw)?;
                            operand.admitted = true;
                        }
                        let kind = match maximum {
                            65536 => RepresentationKind::Authority,
                            262144 => RepresentationKind::SourceDefinition,
                            _ => RepresentationKind::Record,
                        };
                        let parsed = m.parse_owned_representation(arena, &operand.raw, kind)?;
                        codec::validate_selected_types(m, parsed.value())?;
                        let canonical =
                            m.canonical_owned_representation(arena, parsed.value(), kind)?;
                        if !super::shape::same_bytes(m, canonical.bytes(), &operand.raw)? {
                            return Err(invalid("noncanonical consumed body"));
                        }
                        operand.body = Some(RecordBody::owned(arena, parsed, m)?);
                    } else {
                        m.resolved_body(&format!("{}:{}", r.kind, r.id), &operand.raw)?;
                        let parsed = codec::parse(m, &operand.raw, maximum)?;
                        let canonical = codec::encode(m, &parsed, maximum)?;
                        if !super::shape::same_bytes(m, &canonical, &operand.raw)? {
                            return Err(invalid("noncanonical consumed body"));
                        }
                        operand.body = Some(RecordBody::Legacy(parsed));
                    }
                }
                if !intrinsic_reused {
                    found = operand.body.clone();
                    canonical_bytes = Some(operand.raw.clone());
                    if self.arena.is_some() {
                        intrinsic_reused = m
                            .check("resolve.canonical_frame_digest_checked", || {
                                operand.canonical_frame_digest_checked
                            })?;
                        checked_position = Some(position);
                    }
                }
            }
        }
        let body = found.ok_or_else(|| invalid("missing exact required record"))?;
        m.visit("resolve.expected_schema")?;
        let schema = super::shape::text(m, &body, "schema")?;
        let mut selected = provenance_only;
        for expected in schemas {
            if super::shape::same_text(m, schema, expected)? {
                selected = true;
                break;
            }
        }
        if !selected {
            return Err(invalid("wrong required schema"));
        }
        let prefix = match expected_kind {
            "workflow-cost"|"workflow-party-evidence"|"workflow-party-authorization"|"workflow-party-assent"|"workflow-verification" if self.app_records=>"wdoc_",
            "workflow-operation" => "wop_",
            "workflow-work" => "wevent_",
            "workflow-work-claim" => "wclaim_",
            "workflow-outcome-claim" => "woclaim_",
            "workflow-decision" => "wdrev_",
            "workflow-action" => "waction_",
            "workflow-target" => "wtarget_",
            "workflow-family-state" => "wfamily_",
            "workflow-link" => "wlink_",
            "workflow-chain-state" => "wchain_",
            "workflow-correction-capacity" => "wcapacity_",
            "workflow-dependency" => "wdep_",
            "workflow-capacity-retirement" => "wretire_",
            "workflow-source-state" => "wsource_",
            "workflow-accepted-agreement" => "waccepted_",
            "workflow-head-transition" => "whead_",
            "workflow-activation" => "wactivation_",
            "workflow-publication" => "wpub_",
            "workflow-compiled-plan" => "wplan_",
            "workflow-product"
            | "workflow-policy"
            | "workflow-fact-schema"
            | "workflow-slot-template"
            | "workflow-compiler-descriptor"
            | "workflow-roles"
            | "workflow-slot-catalog"
            | "workflow-slot-instantiation"
            | "workflow-slot-terms"
            | "workflow-acceptance-terms"
            | "workflow-assent"
            | "workflow-binding"
            | "workflow-intent"
            | "workflow-agreement-intent"
            | "workflow-build-manifest"
            | "workflow-evidence"
            | "workflow-customer"
            | "workflow-source-enrollment"
            | "workflow-source-grant"
            | "workflow-authority"
            | "workflow-authority-prefix-observation"
            | "workflow-authority-audit-verifier"
            | "workflow-economic-authority"
            | "workflow-financial-history-observation"
            | "workflow-verified-fact"
            | "workflow-fact-attestation"
            | "workflow-evidence-verification"
            | "workflow-evaluation" => "wdoc_",
            _ => return Err(invalid("unknown required record kind")),
        };
        m.visit("resolve.id_family")?;
        let head =
            r.id.get(..prefix.len())
                .ok_or_else(|| invalid("record identity domain mismatch"))?;
        if !super::shape::same_text(m, head, prefix)? {
            return Err(invalid("record identity domain mismatch"));
        }
        let doc = prefix == "wdoc_";
        if !intrinsic_reused {
            let raw = canonical_bytes.ok_or_else(|| invalid("missing canonical operand"))?;
            let digest = if let Some(arena) = &self.arena {
                if doc {
                    m.scan_bytes(schema.len())?;
                    m.write_bytes(schema.len())?;
                    let schema_value = Value::String(schema.to_owned());
                    let schema = m.canonical_owned_representation(
                        arena,
                        &schema_value,
                        RepresentationKind::SourceDefinition,
                    )?;
                    hash_parts(m, "workflow-document", &[schema.bytes(), &raw], true)?
                } else if prefix == "wplan_" {
                    hash_parts(m, "workflow-plan", &[&raw], false)?
                } else {
                    m.scan_bytes(expected_kind.len())?;
                    m.write_bytes(expected_kind.len())?;
                    let kind_value = Value::String(expected_kind.to_owned());
                    let kind = m.canonical_owned_representation(
                        arena,
                        &kind_value,
                        RepresentationKind::SourceDefinition,
                    )?;
                    for name in &key.scope {
                        m.scan_bytes(name.len())?;
                        m.write_bytes(name.len())?;
                    }
                    let scope_value = json!(key.scope);
                    let scope = m.canonical_owned_representation(
                        arena,
                        &scope_value,
                        RepresentationKind::SourceDefinition,
                    )?;
                    hash_parts(
                        m,
                        "workflow-record",
                        &[kind.bytes(), scope.bytes(), &raw],
                        true,
                    )?
                }
            } else if doc {
                let schema = codec::encode(m, &json!(schema), 262144)?;
                hash_parts(m, "workflow-document", &[&schema, &raw], true)?
            } else if prefix == "wplan_" {
                hash_parts(m, "workflow-plan", &[&raw], false)?
            } else {
                let kind = codec::encode(m, &json!(expected_kind), 262144)?;
                let scope = codec::encode(m, &json!(key.scope), 262144)?;
                hash_parts(m, "workflow-record", &[&kind, &scope, &raw], true)?
            };
            m.visit("resolve.digest_match")?;
            if !super::shape::same_text(m, &digest, &r.digest)?
                || (doc && !super::shape::same_text(m, &r.id[5..], &r.digest[7..])?)
            {
                return Err(invalid("record body hash mismatch"));
            }
        }
        if let Some(position) = checked_position {
            // Exact expected schema/kind/prefix and canonical digest completed;
            // no per-kind semantic assertion is inferred by this cache bit.
            self.operands[position].canonical_frame_digest_checked = true;
        }
        let position=resolved_position.ok_or_else(||invalid("resolved operand position missing"))?;
        let mut seen = false;
        if self.arena.is_some(){
            // This SAME index lookup already established the exact full Ref.
            // Membership can therefore use its immutable operand position.
            // The bit is written only after all framing/schema checks above.
            seen=m.check("resolve.exact_operand_already_consumed",||self.operands[position].consumed_in_invocation)?;
        }else{
            // Preserve the original v1 callback sequence and linear algorithm.
            for prior in &self.consumed {
                m.visit("resolve.consumed_compare")?;
                if super::shape::same_ref(m, prior, r)? {seen=true;break;}
            }
        }
        if !seen {
            let old=if self.arena.is_some(){m.check("resolve.preexisting_input_partition",||matches!(self.operands[position].partition,OperandPartition::Preexisting))?}else{true};
            if old&&self.arena.is_some() {
                for text in [&r.kind, &r.id, &r.digest] {
                    m.scan_bytes(text.len())?;
                    m.write_bytes(text.len())?;
                }
            }
            if old{self.consumed.push(r.clone());}
            else{self.pending_consumed=w::usize_add(m,self.pending_consumed,1)?;}
            self.operands[position].consumed_in_invocation=true;
        }
        Ok(body)
    }
    // Identity is a locator, never a proof: the selected body still passes the
    // complete existing kind/schema/domain/canonical/digest checks below.
    pub(super) fn by_identity(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        kind: &str,
        id: &str,
        schema: &str,
    ) -> Result<(ExactRef, RecordBody)> {
        let mut found = None;
        m.visit("resolve.identity_locator")?;
        if let Some(position) = self
            .index
            .find(m, kind, id, |i| &self.operands[i].reference)?
        {
            found = Some(self.operands[position].reference.clone());
        }
        let reference = found.ok_or_else(|| invalid("missing birth identity"))?;
        let body = self.get(m, key, &reference, kind, schema)?;
        Ok((reference, body))
    }
    pub(super) fn by_raw_sha(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        kind: &str,
        wanted: &str,
        schema: &str,
    ) -> Result<(ExactRef, RecordBody)> {
        let mut found = None;
        for operand in &self.operands {
            m.visit("resolve.raw_digest_locator")?;
            if !super::shape::same_text(m, &operand.reference.kind, kind)? {
                continue;
            }
            if !super::meter::cmp(m, &operand.raw.len().into(), &4194304usize.into())?.is_le() {
                return Err(invalid("locator body bound"));
            }
            m.hash_bytes(operand.raw.len())?;
            let raw_digest = format!(
                "sha256:{}",
                canonical::hex_with_work(&Sha256::digest(&operand.raw), m)?
            );
            if super::shape::same_text(m, &raw_digest, wanted)? {
                if found.is_some() {
                    return Err(invalid("ambiguous raw digest locator"));
                }
                found = Some(operand.reference.clone());
            }
        }
        let reference = found.ok_or_else(|| invalid("missing raw digest locator"))?;
        let body = self.get(m, key, &reference, kind, schema)?;
        Ok((reference, body))
    }
    // Only a consumed, canonicality-checked body can expose its representation.
    /// Exact I-set membership from this invocation's immutable operand index.
    /// A present but unused operand is not a consumed member. This does not
    /// parse or admit anything, and cannot establish original custody.
    pub(super) fn consumed_canonical(&self,m:&mut CoreMeter,r:&ExactRef)->Result<Option<&[u8]>> {
        Ok(match self.used_canonical(m,r)?{UsedCanonical::Preexisting(body)=>Some(body),_=>None})
    }
    pub(super) fn used_pending_count(&self)->usize{self.pending_consumed}
    pub(super) fn used_canonical(&self,m:&mut CoreMeter,r:&ExactRef)->Result<UsedCanonical<'_>> {
        if self.arena.is_none(){return Err(invalid("owning consumed member lookup"));}
        let Some(position)=self.index.find_exact(m,r,|i|&self.operands[i].reference)? else{return Ok(UsedCanonical::Absent)};
        let operand=&self.operands[position];
        if super::shape::same_ref(m,&operand.reference,r)? && m.check("resolve.exact_consumed_member",||operand.consumed_in_invocation)? {
            Ok(match operand.partition{OperandPartition::Preexisting=>UsedCanonical::Preexisting(&operand.raw),OperandPartition::Pending=>UsedCanonical::Pending(&operand.raw)})
        }else{Ok(UsedCanonical::Absent)}
    }
    pub(super) fn canonical(&self, m: &mut CoreMeter, r: &ExactRef) -> Result<&[u8]> {
        if let Some(position) = self
            .index
            .find_exact(m, r, |i| &self.operands[i].reference)?
        {
            let op = &self.operands[position];
            m.visit("resolve.canonical_lookup")?;
            if super::shape::same_ref(m, &op.reference, r)? {
                if !m.check("resolve.canonical_checked", || op.body.is_some())? {
                    return Err(invalid("unconsumed canonical representation"));
                }
                return Ok(&op.raw);
            }
        }
        Err(invalid("canonical operand missing"))
    }
    pub(super) fn into_consumed_owned(mut self) -> OwnedConsumed {
        OwnedConsumed { references: std::mem::take(&mut self.consumed), _metadata: self._metadata.take() }
    }
    /// Only the legacy DATA projection may detach these copies from its table.
    /// Financial construction retains the table (including its lease) through
    /// all derivations and transfers ownership to the final candidate instead.
    pub(super) fn into_legacy_consumed(self) -> Result<Vec<ExactRef>> {
        if self.arena.is_some() || self._metadata.is_some() {
            return Err(invalid(
                "owning inventory cannot detach consumed references",
            ));
        }
        Ok(self.consumed)
    }
    pub(super) fn consumed(&self) -> &[ExactRef] {
        &self.consumed
    }
}

#[cfg(test)]
mod owning_tests {
    use super::*;
    use crate::validation_work::OriginalAuditSession;
    use serde_json::json;

    #[test]
    fn matching_metadata_does_not_establish_reference_grammar() {
        // A same-table exact metadata match is only a locator. Both required
        // and optional first use must reject grammar before parsing the body.
        let key=OperationKey{logical_store_id:"store".into(),scope:["scope".into(),"test".into()],source:"source".into(),operation_id:"operation".into()};
        let reference=ExactRef{kind:"workflow-evidence".into(),id:format!("wdoc_{}","a".repeat(64)),digest:format!("sha256:{}","A".repeat(64))};
        let views=[EconomicRecordView{reference:&reference,canonical_body:br#"{"schema":"ledger-workflow-retained-evidence/1"}"#}];
        for optional in [false,true]{
            let arena=AuditArena::new().unwrap();let baseline=arena.usage().unwrap().live_bytes;
            let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut m=session.meter_mut().original_economic_meter().unwrap();
            let mut table=Records::borrowed_app(&mut m,&arena,&key,&views,&[]).unwrap();
            assert!(!table.operands[0].reference_grammar_checked);
            let error=if optional{
                table.optional(&mut m,&key,&reference,&reference.kind,"ledger-workflow-retained-evidence/1").unwrap_err()
            }else{
                table.get(&mut m,&key,&reference,&reference.kind,"ledger-workflow-retained-evidence/1").unwrap_err()
            };
            assert_eq!(error.detail,"digest hex");
            assert!(!table.operands[0].reference_grammar_checked);
            assert!(table.operands[0].body.is_none());
            assert!(table.consumed().is_empty());
            session.invalidate();drop(table);drop(m);drop(session);
            assert_eq!(arena.usage().unwrap().live_bytes,baseline);
        }
    }

    #[test]
    fn pending_used_body_never_becomes_a_preexisting_input() {
        // Framing/partition test only. Neither branch supplies origin,
        // retained-evidence semantics, a current head or financial admission.
        let key=OperationKey{logical_store_id:"store".into(),scope:["scope".into(),"test".into()],source:"source".into(),operation_id:"operation".into()};
        let body=json!({"schema":"ledger-workflow-retained-evidence/1","purpose":"literal"});
        let mut build=CoreMeter::new(65536).unwrap();
        let digest=hash(&mut build,"workflow-document",&json!([body["schema"],body])).unwrap();
        let reference=ExactRef{kind:"workflow-evidence".into(),id:format!("wdoc_{}",&digest[7..]),digest};
        let bytes=super::super::codec::encode(&mut build,&body,4194304).unwrap();
        let views=[EconomicRecordView{reference:&reference,canonical_body:&bytes}];
        let pending=[RecordInput{reference:reference.clone(),canonical_body:bytes.clone()}];
        for is_pending in [false,true]{
            let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut m=session.meter_mut().original_economic_meter().unwrap();
            let mut table=Records::borrowed_app(&mut m,&arena,&key,if is_pending{&[]}else{&views},if is_pending{&pending}else{&[]}).unwrap();
            for _ in 0..2{table.get(&mut m,&key,&reference,&reference.kind,"ledger-workflow-retained-evidence/1").unwrap();}
            assert_eq!(table.used_pending_count(),usize::from(is_pending));
            assert_eq!(table.consumed().len(),usize::from(!is_pending));
            assert_eq!(table.consumed_canonical(&mut m,&reference).unwrap().is_some(),!is_pending);
            match table.used_canonical(&mut m,&reference).unwrap(){
                UsedCanonical::Preexisting(raw)=>{assert!(!is_pending);assert_eq!(raw,bytes);},
                UsedCanonical::Pending(raw)=>{assert!(is_pending);assert_eq!(raw,bytes);},
                UsedCanonical::Absent=>panic!("used exact body missing"),
            }
            let mut other=reference.clone();other.digest=format!("sha256:{}","0".repeat(64));
            assert!(matches!(table.used_canonical(&mut m,&other).unwrap(),UsedCanonical::Absent));
            let consumed=table.into_consumed_owned();assert_eq!(consumed.references.len(),usize::from(!is_pending));
            session.meter_mut().finish_original_economic(m).unwrap();
        }
    }

    #[test]
    fn financial_output_framing_reuses_the_one_inventory_claim() {
        // The existing representation owner test, not origin/authority or
        // whole-assembly admission. The pair's semantic validator remains
        // complete::select, and the full capacity boundary stays closed.
        let key=OperationKey{logical_store_id:"store".into(),scope:["scope".into(),"test".into()],source:"source".into(),operation_id:"operation".into()};
        let mut build=CoreMeter::new(65536).unwrap();
        for (kind,schema) in [("workflow-financial-history-observation","ledger-workflow-financial-history-observation/3"),("workflow-authority-audit-verifier","ledger-authority-audit-verifier/2")]{
            let value=json!({"schema":schema});
            let digest=hash(&mut build,"workflow-document",&json!([schema,value])).unwrap();
            let reference=ExactRef{kind:kind.into(),id:format!("wdoc_{}",&digest[7..]),digest};
            let raw=codec::encode(&mut build,&value,4194304).unwrap();
            let view=EconomicRecordView{reference:&reference,canonical_body:&raw};
            for reused in [false,true]{
                let arena=AuditArena::new().unwrap();let baseline=arena.usage().unwrap().live_bytes;
                let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let mut child=session.meter_mut().original_economic_meter().unwrap();
                let views=[view];
                let mut records=Records::borrowed_app(&mut child,&arena,&key,if reused{&views}else{&[]},&[]).unwrap();
                let result=records.observation_output(&mut child,&key,view,kind,schema);
                if reused&&kind=="workflow-financial-history-observation"{
                    assert_eq!(result.unwrap_err().detail,"pending financial observation is already preexisting");
                    session.invalidate();
                }else{
                    let output=result.unwrap();assert_eq!(output,value);
                    assert_eq!(records.consumed().len(),usize::from(reused));
                    assert_eq!(records.used_pending_count(),0);
                    drop(output);session.meter_mut().finish_original_economic(child).unwrap();
                }
                drop(records);drop(session);assert_eq!(arena.usage().unwrap().live_bytes,baseline);
            }
        }
    }

    #[test]
    fn same_invocation_frame_is_owned_once_but_every_use_checks_association() {
        // This intentionally establishes only canonical frame/hash properties,
        // not a retained-evidence purpose, historical origin or admission proof.
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["scope".into(), "test".into()],
            source: "source".into(),
            operation_id: "operation".into(),
        };
        let body = json!({"schema":"ledger-workflow-retained-evidence/1","purpose":"literal"});
        let mut prepare = CoreMeter::new(65536).unwrap();
        let digest = hash(
            &mut prepare,
            "workflow-document",
            &json!([body["schema"], body]),
        )
        .unwrap();
        let reference = ExactRef {
            kind: "workflow-evidence".into(),
            id: format!("wdoc_{}", &digest[7..]),
            digest,
        };
        let bytes = super::super::codec::encode(&mut prepare, &body, 4194304).unwrap();
        let views = [EconomicRecordView {
            reference: &reference,
            canonical_body: &bytes,
        }];
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().original_economic_meter().unwrap();
        m.enter_draft().unwrap();
        let mut table = Records::borrowed(&mut m, &arena, &key, &views, &[]).unwrap();
        assert!(table.consumed_canonical(&mut m,&reference).unwrap().is_none());
        let first = table
            .get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1",
            )
            .unwrap();
        let once = m.resource_vector_usage().unwrap();
        let semantic = m.usage().semantic_work;
        let second = table
            .get(
                &mut m,
                &key,
                &reference,
                &reference.kind,
                "ledger-workflow-retained-evidence/1",
            )
            .unwrap();
        assert!(std::ptr::eq(first.deref(), second.deref()));
        assert_eq!(
            m.resource_vector_usage().unwrap().representation_calls,
            once.representation_calls
        );
        assert!(m.usage().semantic_work > semantic);
        assert_eq!(m.usage().unique_resolved_refs, 1);
        assert_eq!(m.usage().unique_resolved_bytes, bytes.len() as u64);
        assert_eq!(table.consumed(), &[reference.clone()]);
        assert_eq!(table.consumed_canonical(&mut m,&reference).unwrap(),Some(bytes.as_slice()));
        assert!(
            table
                .get(&mut m, &key, &reference, &reference.kind, "wrong-schema")
                .is_err()
        );
        let mut wrong = key.clone();
        wrong.scope[1] = "other".into();
        assert!(
            table
                .get(
                    &mut m,
                    &wrong,
                    &reference,
                    &reference.kind,
                    "ledger-workflow-retained-evidence/1"
                )
                .is_err()
        );
        let mut wrong = reference.clone();
        wrong.digest = format!("sha256:{}", "0".repeat(64));
        assert!(table.consumed_canonical(&mut m,&wrong).unwrap().is_none());
        assert!(table.optional(&mut m,&key,&wrong,&reference.kind,"ledger-workflow-retained-evidence/1").is_err());
        assert!(
            table
                .get(
                    &mut m,
                    &key,
                    &wrong,
                    &reference.kind,
                    "ledger-workflow-retained-evidence/1"
                )
                .is_err()
        );
        drop(table);
        // A borrowed body may outlive its lookup table. Its own parsed graph
        // reservation must remain attached until the last immutable view dies.
        assert!(arena.usage().unwrap().live_bytes > baseline);
        drop(first);
        assert!(arena.usage().unwrap().live_bytes > baseline);
        drop(second);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }

    #[test]
    fn owning_inventory_cannot_detach_references_into_legacy_draft() {
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["a".into(), "b".into()],
            source: "s".into(),
            operation_id: "o".into(),
        };
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().original_economic_meter().unwrap();
        m.enter_draft().unwrap();
        let table = Records::borrowed(&mut m, &arena, &key, &[], &[]).unwrap();
        assert!(table.into_legacy_consumed().is_err());
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        assert!(Records::borrowed(&mut m, &arena, &key, &[], &[]).is_err());
        assert!(session.meter_mut().finish_original_economic(m).is_err());
    }
}

#[cfg(test)]
mod source_facts_tests{
 use super::*;
 use super::super::{authority::EconomicMode,data::DraftContextData,representation::InvocationToken};
 use serde_json::json;
 #[test]
 fn owning_grant_reuse_keeps_requested_rights_and_time_checks(){
  // Intrinsic intersection-only unit. No original source enrollment or host
  // authority/custody/origin is claimed by these synthetic framed documents.
  let c=DraftContextData{key:OperationKey{logical_store_id:"store".into(),scope:["org".into(),"scope".into()],source:"source".into(),operation_id:"work".into()},actor:"principal".into(),canonical_economic_namespace:"namespace".into(),installation_mode:"real_local".into(),received_at:"2026-10-18T00:00:00.000000Z".into(),accepted_at:"2026-10-18T00:00:00.000000Z".into(),authorities:vec![],source_observation_inputs:vec![]};
  let ceiling=json!({"principal_id":"principal","source":"source","canonical_economic_namespace":"namespace","customer_scope":["customer"],"operations":["work.accept"],"product_selectors":[{"names":["test:product"]}],"activity_selectors":[{"names":["test:activity"]}],"link_selectors":[],"source_slots":["test:worker"],"permissions":["read","submit"],"starts_at":"2026-10-17T00:00:00.000000Z","ends_at":"2026-10-25T00:00:00.000000Z"});
  let mut prepare=CoreMeter::new(65536).unwrap();
  let enrollment=super::super::body::document(&mut prepare,"workflow-source-enrollment",json!({"schema":"ledger-workflow-source-enrollment/1","ceiling":ceiling})).unwrap();
  let mut grant=ceiling.clone();grant["schema"]=json!("ledger-workflow-source-grant/1");grant["grant_id"]=json!("enrollment");grant["revision"]=json!("1");grant["original_ceiling_ref"]=json!(enrollment.reference);
  let grant=super::super::body::document(&mut prepare,"workflow-source-grant",grant).unwrap();
  let views=[EconomicRecordView{reference:&enrollment.reference,canonical_body:&enrollment.canonical_body},EconomicRecordView{reference:&grant.reference,canonical_body:&grant.canonical_body}];
  let arena=AuditArena::new().unwrap();let mut session=crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
  let mut m=session.meter_mut().original_economic_meter().unwrap();let mut records=Records::borrowed_app(&mut m,&arena,&c.key,&views,&[]).unwrap();
  let e=records.get(&mut m,&c.key,&enrollment.reference,"workflow-source-enrollment","ledger-workflow-source-enrollment/1").unwrap();
  let g=records.get(&mut m,&c.key,&grant.reference,"workflow-source-grant","ledger-workflow-source-grant/1").unwrap();
  let at=crate::domain::Timestamp::parse(&c.accepted_at).unwrap().micros();let owner=InvocationToken::new();
  records.source_grant(&mut m,&owner,&grant.reference,&g,&enrollment.reference,&e,&c,&json!("customer"),"test:activity","test:product","test:worker",&at.into(),EconomicMode::Work).unwrap();
  let before=m.usage().semantic_work;
  records.source_grant(&mut m,&owner,&grant.reference,&g,&enrollment.reference,&e,&c,&json!("customer"),"test:activity","test:product","test:worker",&at.into(),EconomicMode::Work).unwrap();
  assert!(m.usage().semantic_work>before);
  assert!(records.source_grant(&mut m,&owner,&grant.reference,&g,&enrollment.reference,&e,&c,&json!("customer"),"test:activity","test:product","test:worker",&at.into(),EconomicMode::CostCorrect).is_err());
  let invalid_at=crate::domain::Timestamp::parse("2026-10-26T00:00:00.000000Z").unwrap().micros();
  assert!(records.source_grant(&mut m,&owner,&grant.reference,&g,&enrollment.reference,&e,&c,&json!("customer"),"test:activity","test:product","test:worker",&invalid_at.into(),EconomicMode::Work).is_err());
  let mut later=c.clone();later.accepted_at="2026-10-26T00:00:00.000000Z".into();let at=crate::domain::Timestamp::parse(&later.accepted_at).unwrap().micros();
  assert!(records.source_grant(&mut m,&owner,&grant.reference,&g,&enrollment.reference,&e,&later,&json!("customer"),"test:activity","test:product","test:worker",&at.into(),EconomicMode::Work).is_err());
  assert_eq!(records.consumed(),&[enrollment.reference.clone(),grant.reference.clone()]);
 }
}
