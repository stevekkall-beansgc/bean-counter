//! Exact supplier-scoped consent projection of this compiler's actual program.
//! No supplied plan, completeness bit or fixture projection enters this module.
//! Its output is owning DATA; origin/current-party custody remains host-owned.
use super::{SelectedProgram, compiler::same_literal};
use super::super::transition_constructor::{CoreMeter,codec,shape};
use crate::{Error,Result,canonical::audit_arena::{AuditArena,AuditedValue,AuditedBytes,ExternalBufferLease},validation_work::{self as w,ArithmeticOperation,ValidationWork,RepresentationKind,OriginalAuditSession}};
use serde_json::Value;
use std::mem;

fn unsupported(reason:&str)->Error{Error::new("UNSUPPORTED_COMPOSITION",reason)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||shape::fail("supplier projection allocation product"))
}
fn named<'a>(m:&mut CoreMeter,rows:&'a [Value],key:&str,name:&str)->Result<&'a Value>{
    let mut found=None;
    for row in rows{m.visit("supplier_projection.named_member")?;let actual=shape::text(m,row,key)?;
        if same_literal(m,actual,name)?{if found.is_some(){return Err(shape::fail("ambiguous supplier projection name"));}found=Some(row);}
    }
    found.ok_or_else(||shape::fail("missing supplier projection name"))
}
fn has_text(m:&mut CoreMeter,rows:&[Value],name:&str)->Result<bool>{
    for row in rows{m.visit("supplier_projection.text_member")?;let actual=row.as_str().ok_or_else(||shape::fail("supplier projection string set"))?;
        if same_literal(m,actual,name)?{return Ok(true);}
    }Ok(false)
}
/// Controlled owning vector. Sorting retains actual canonical key artifacts,
/// pays each comparison/move, and never escapes Values from their leases.
struct Values { values:Vec<AuditedValue>, maximum:usize, _ownership:ExternalBufferLease }
impl Values {
    fn new(m:&mut CoreMeter,arena:&AuditArena,maximum:usize)->Result<Self>{
        let per=w::usize_add(m,mem::size_of::<AuditedValue>(),mem::size_of::<&Value>()+mem::size_of::<(AuditedBytes,usize)>())?;
        let bytes=mul(m,maximum,per)?;let bytes=w::usize_add(m,bytes,3*mem::size_of::<Vec<AuditedValue>>())?;
        let ownership=arena.reserve_external_payload(bytes,m)?;
        let mut values=Vec::new();values.try_reserve_exact(maximum).map_err(|_|shape::fail("supplier projection vector allocation"))?;
        Ok(Self{values,maximum,_ownership:ownership})
    }
    fn push(&mut self,m:&mut CoreMeter,value:AuditedValue)->Result<()> {
        if !w::compare(m,self.values.len(),self.maximum)?.is_lt(){return Err(shape::fail("supplier projection dimension widened"));}
        m.visit("supplier_projection.owned_member")?;self.values.push(value);Ok(())
    }
    fn unique(&mut self,m:&mut CoreMeter,arena:&AuditArena,value:&Value)->Result<()> {
        for old in &self.values {m.visit("supplier_projection.deduplicate")?;if shape::same(m,old.value(),value)?{return Ok(());}}
        let value=arena.clone_value(value,m)?;self.push(m,value)
    }
    fn finish(&self,m:&mut CoreMeter,arena:&AuditArena,set:bool)->Result<AuditedValue>{
        let mut order=Vec::new();order.try_reserve_exact(self.values.len()).map_err(|_|shape::fail("supplier projection sort metadata"))?;
        for (index,value) in self.values.iter().enumerate(){
            m.visit("supplier_projection.canonical_member")?;
            let bytes=m.canonical_owned_representation(arena,value.value(),RepresentationKind::SourceDefinition)?;
            let mut position=order.len();
            if set {for (i,(prior,_)) in order.iter().enumerate(){
                let prior:&AuditedBytes=prior;
                match shape::byte_order(m,bytes.bytes(),prior.bytes())?{
                    std::cmp::Ordering::Equal=>return Err(shape::fail("duplicate supplier projection set member")),
                    std::cmp::Ordering::Less=>{position=i;break;},_=>{},
                }
            }}
            // Vec::insert shifts exactly this suffix, not one abstract move.
            for _ in position..order.len(){m.visit("supplier_projection.sort_move")?;m.write_bytes(mem::size_of::<(AuditedBytes,usize)>())?;}
            order.insert(position,(bytes,index));
        }
        let mut borrowed=Vec::new();borrowed.try_reserve_exact(order.len()).map_err(|_|shape::fail("supplier projection borrowed metadata"))?;
        for (_,index) in &order{m.visit("supplier_projection.sorted_borrow")?;borrowed.push(self.values[*index].value());}
        arena.array_from_values(&borrowed,m)
    }
}

fn instruction_key(m:&mut CoreMeter,arena:&AuditArena,instruction:&Value)->Result<AuditedValue>{
    let opcode=shape::text(m,instruction,"opcode")?;
    if same_literal(m,opcode,"evaluate_rule")? {
        let kind=arena.string_value("rule",m)?;
        return arena.array_from_values(&[kind.value(),&instruction["rule"]["binding_slot"],&instruction["rule"]["activity"],&instruction["rule"]["rule_id"]],m);
    }
    if same_literal(m,opcode,"evaluate_family")? {
        let kind=arena.string_value("family",m)?;
        return arena.array_from_values(&[kind.value(),&instruction["family"]["selector"]["binding_slot"],&instruction["family"]["family_key"]],m);
    }
    // The selected compiler does not currently compile stages or arbitrary
    // closure opcodes. They may not silently disappear from provider consent.
    Err(unsupported("unimplemented supplier projection instruction semantics"))
}
fn ordinal_position(m:&mut CoreMeter,instructions:&[Value],ordinal:&Value)->Result<usize>{
    let mut result=None;
    for (i,instruction) in instructions.iter().enumerate(){
        m.visit("supplier_projection.ordinal_locator")?;
        if shape::same(m,&instruction["ordinal"],ordinal)?{
            if result.is_some(){return Err(shape::fail("duplicate compiled instruction ordinal"));}result=Some(i);
        }
    }
    result.ok_or_else(||shape::fail("missing supplier producer ordinal"))
}
fn component<'a>(m:&mut CoreMeter,plan:&'a Value,key:&str)->Result<&'a Value>{let rows=shape::array(m,plan,"component_slots")?;named(m,rows,"key",key)}
fn root<'a>(m:&mut CoreMeter,plan:&'a Value,instruction:&'a Value)->Result<&'a str>{
    let opcode=shape::text(m,instruction,"opcode")?;
    let key=if same_literal(m,opcode,"evaluate_rule")?{shape::text(m,&instruction["rule"],"component_key")?}
        else if same_literal(m,opcode,"evaluate_family")?{shape::text(m,instruction,"target_component_key")?}
        else{return Err(unsupported("supplier stage/root interpretation unavailable"));};
    let component=component(m,plan,key)?;shape::text(m,component,"root_key")
}
fn slots<'a>(m:&mut CoreMeter,plan:&'a Value,instruction:&Value,name:&str,arena:&AuditArena)->Result<AuditedValue>{
    let empty=[];
    let indices=match instruction.get(name){None=>&empty[..],Some(v)=>v.as_array().map(Vec::as_slice).ok_or_else(||shape::fail("supplier fact-slot operands"))?};
    let all=shape::array(m,plan,"fact_slots")?;let mut output=Values::new(m,arena,indices.len())?;
    for index in indices {
        m.visit("supplier_projection.exact_fact_operand")?;
        let mut found=None;
        for field in all{m.visit("supplier_projection.fact_slot_locator")?;if shape::same(m,&field["index"],index)?{if found.is_some(){return Err(shape::fail("ambiguous supplier fact slot"));}found=Some(field);}}
        let field=found.ok_or_else(||shape::fail("missing supplier fact slot"))?;
        let value=arena.object_from_fields(&[("schema_ref",&field["schema_ref"]),("field",&field["field"])],m)?;output.push(m,value)?;
    }
    output.finish(m,arena,true)
}

struct ProgramProjection {
    value:AuditedValue,
    selected:Vec<u8>,
    _ownership:ExternalBufferLease,
}
fn program_projection(m:&mut CoreMeter,arena:&AuditArena,program:&SelectedProgram,supplier_slot:&str)->Result<ProgramProjection>{
    let plan=program.compiled_plan().canonical_value();let policy=program.original_policy_value();
    if !program.uses_supplier_scoped_consent_recipe(){return Err(unsupported("original compiler lacks supplier-scoped consent recipe"));}
    let instructions=shape::array(m,plan,"typed_instructions")?;
    if w::compare(m,instructions.len(),256usize)?.is_gt(){return Err(shape::fail("supplier instruction dimension"));}
    let stages=shape::array(m,policy,"stages")?;
    if !w::compare(m,stages.len(),0usize)?.is_eq(){return Err(unsupported("supplier stages require actual executable projection semantics"));}
    let ownership=arena.reserve_external_payload(instructions.len(),m)?;
    let mut selected=Vec::new();selected.try_reserve_exact(instructions.len()).map_err(|_|shape::fail("supplier selection vector"))?;
    m.write_bytes(instructions.len())?;selected.resize(instructions.len(),0u8);
    let mut keys=Values::new(m,arena,instructions.len())?;
    for (i,instruction) in instructions.iter().enumerate(){
        m.visit("supplier_projection.seed_instruction")?;
        let key=instruction_key(m,arena,instruction)?;
        for prior in &keys.values{if shape::same(m,prior.value(),key.value())?{return Err(shape::fail("duplicate stable supplier instruction key"));}}
        keys.push(m,key)?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if same_literal(m,opcode,"evaluate_rule")? {
            let rule=&instruction["rule"];
            let slot=shape::text(m,rule,"binding_slot")?;
            if same_literal(m,slot,supplier_slot)?{
                let key=shape::text(m,rule,"component_key")?;let c=component(m,plan,key)?;
                let book=shape::text(m,c,"book")?;if same_literal(m,book,"supplier")?{selected[i]=1;}
            }
        }
    }
    let mut seeds=0usize;for flag in &selected{m.visit("supplier_projection.seed_count")?;if *flag!=0{seeds=w::usize_add(m,seeds,1)?;}}
    if !w::compare(m,seeds,0usize)?.is_gt(){return Err(unsupported("supplier binding has no original supplier producer"));}
    let edges=shape::array(m,plan,"dependency_edges")?;
    let mut rounds=0usize;
    loop {
        rounds=w::usize_add(m,rounds,1)?;
        let maximum=w::usize_add(m,instructions.len(),1)?;
        if w::compare(m,rounds,maximum)?.is_gt(){return Err(shape::fail("supplier dependency fixed point did not converge"));}
        let mut changed=false;
        for (i,instruction) in instructions.iter().enumerate(){
            m.visit("supplier_projection.selected_root_closure")?;
            if selected[i]==0{continue;}
            let original_root=root(m,plan,instruction)?;
            for (j,candidate) in instructions.iter().enumerate(){
                m.visit("supplier_projection.shared_root_modifier")?;
                let candidate_root=root(m,plan,candidate)?;
                if same_literal(m,candidate_root,original_root)? && selected[j]==0{selected[j]=1;changed=true;}
            }
            if let Some(bases)=instruction.get("basis_instructions"){
                for basis in bases.as_array().ok_or_else(||shape::fail("supplier basis array"))?{
                    let producer=ordinal_position(m,instructions,basis)?;
                    if selected[producer]==0{selected[producer]=1;changed=true;}
                }
            }
        }
        for edge in edges{
            m.visit("supplier_projection.producer_closure")?;
            let producer=ordinal_position(m,instructions,&edge["producer"])?;
            let consumer=ordinal_position(m,instructions,&edge["consumer"])?;
            if selected[consumer]!=0 && selected[producer]==0{selected[producer]=1;changed=true;}
        }
        if !changed{break;}
    }
    let mut nodes=Values::new(m,arena,instructions.len())?;
    let mut order=Values::new(m,arena,instructions.len())?;
    let component_count=shape::array(m,plan,"component_slots")?.len();let mut components=Values::new(m,arena,component_count)?;
    let activity_rows=shape::array(m,plan,"activity_table")?;let mut activities=Values::new(m,arena,activity_rows.len())?;
    let mut contracts=Values::new(m,arena,128)?;
    let delayed_rows=shape::array(m,policy,"delayed_costs")?;let mut delayed=Values::new(m,arena,delayed_rows.len())?;
    contracts.unique(m,arena,&policy["product_ref"])?;contracts.unique(m,arena,&policy["context_schema_ref"])?;
    for (i,instruction) in instructions.iter().enumerate(){
        m.visit("supplier_projection.emit_selected_instruction")?;
        if selected[i]==0{continue;}
        let opcode=shape::text(m,instruction,"opcode")?;
        let is_rule=same_literal(m,opcode,"evaluate_rule")?;
        let body=if is_rule{&instruction["rule"]}else{&instruction["family"]};
        let binding=if is_rule{shape::text(m,body,"binding_slot")?}else{shape::text(m,&body["selector"],"binding_slot")?};
        // Detect the complete producer/shared-root closure BEFORE this check.
        // A name match cannot erase another binding's authority/funding terms.
        if !same_literal(m,binding,supplier_slot)?{return Err(unsupported("supplier closure requires another binding's authority projection"));}
        let facts=slots(m,plan,instruction,"fact_slots",arena)?;
        let binding_facts=slots(m,plan,instruction,"binding_fact_slots",arena)?;
        for values in [facts.value(),binding_facts.value()]{
            for value in values.as_array().ok_or_else(||shape::fail("supplier fact projection"))?{contracts.unique(m,arena,&value["schema_ref"])?;}
        }
        let mut bases=Values::new(m,arena,instructions.len())?;
        if let Some(values)=instruction.get("basis_instructions"){
            for ordinal in values.as_array().ok_or_else(||shape::fail("supplier basis instructions"))?{
                let index=ordinal_position(m,instructions,ordinal)?;
                if selected[index]==0{return Err(shape::fail("supplier projection producer omitted"));}
                bases.unique(m,arena,keys.values[index].value())?;
            }
        }
        let bases=bases.finish(m,arena,true)?;
        let kind=arena.string_value(if is_rule{"rule"}else{"family"},m)?;
        let fields=[("key",keys.values[i].value()),("facts",facts.value()),("binding_facts",binding_facts.value()),("basis_keys",bases.value()),("kind",kind.value()),(if is_rule{"rule"}else{"family"},body),("target_component_key",&instruction["target_component_key"])];
        let node=arena.object_from_fields(if is_rule{&fields[..6]}else{&fields},m)?;nodes.push(m,node)?;
        order.unique(m,arena,keys.values[i].value())?;
        let activity=if is_rule{shape::text(m,body,"activity")?}else{shape::text(m,body,"trigger_activity")?};
        let a=named(m,activity_rows,"activity",activity)?;
        activities.unique(m,arena,a)?;contracts.unique(m,arena,&a["fact_schema_ref"])?;
        let component_key=if is_rule{shape::text(m,body,"component_key")?}else{shape::text(m,instruction,"target_component_key")?};
        let component_value=component(m,plan,component_key)?;components.unique(m,arena,component_value)?;
        let root_key=shape::text(m,component_value,"root_key")?;let root=component(m,plan,root_key)?;components.unique(m,arena,root)?;
        for declaration in shape::array(m,policy,"delayed_costs")?{
            m.visit("supplier_projection.delayed_constraint")?;
            let actual=shape::text(m,declaration,"component_key")?;if same_literal(m,actual,component_key)?{delayed.unique(m,arena,declaration)?;}
        }
    }
    let mut projected_edges=Values::new(m,arena,edges.len())?;
    for edge in edges{
        m.visit("supplier_projection.emit_dependency")?;
        let producer=ordinal_position(m,instructions,&edge["producer"])?;let consumer=ordinal_position(m,instructions,&edge["consumer"])?;
        if selected[consumer]==0{continue;}
        if selected[producer]==0{return Err(shape::fail("supplier dependency endpoint omitted"));}
        let value=arena.object_from_fields(&[("producer",keys.values[producer].value()),("consumer",keys.values[consumer].value())],m)?;projected_edges.push(m,value)?;
    }
    let nodes=nodes.finish(m,arena,true)?;let order=order.finish(m,arena,false)?;
    let edges=projected_edges.finish(m,arena,true)?;let activities=activities.finish(m,arena,true)?;
    let components=components.finish(m,arena,true)?;let delayed=delayed.finish(m,arena,true)?;let contracts=contracts.finish(m,arena,true)?;
    let stages=arena.array_from_values(&[],m)?;
    let value=arena.object_from_fields(&[("evaluator",&plan["evaluator"]),("grammar",&plan["grammar"]),("currency",&policy["currency"]),("scale",&policy["scale"]),("rounding",&policy["rounding"]),("mode",&policy["mode"]),("execution_order",order.value()),("nodes",nodes.value()),("dependency_edges",edges.value()),("activities",activities.value()),("components",components.value()),("delayed_costs",delayed.value()),("source_contract_refs",contracts.value()),("stages",stages.value())],m)?;
    Ok(ProgramProjection{value,selected,_ownership:ownership})
}
