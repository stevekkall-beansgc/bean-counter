use super::super::transition_constructor::{CoreMeter, arithmetic, codec, meter};
use super::model::{Effect, Formula, Policy, Code, Amount, AppCostDeclaration};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::cmp::Ordering;
use crate::{canonical::audit_arena::{AuditArena,AuditedValue},validation_work::{self as work,RepresentationKind,ValidationWork,ArithmeticOperation}};
use std::mem;
fn invalid() -> Error {
    Error::new("INVALID_SCALAR", "selected scalar normalization")
}
pub(super) fn counter(m: &mut CoreMeter, s: &mut String) -> Result<BigInt> {
    if !m.check("counter.length", || !s.is_empty() && s.len() <= 64)? {
        return Err(invalid());
    }
    for b in s.bytes() {
        if !m.check("counter.digit", || b.is_ascii_digit())? {
            return Err(invalid());
        }
    }
    let mut trim = 0;
    for b in s.bytes() {
        if !m.check("counter.leading_zero", || b == b'0')? {
            break;
        }
        trim += 1;
    }
    m.visit("counter.zero_form")?;
    if m.representation_arena().is_some(){trim_digits_in_place(m,s,trim)?;}else{*s = if trim == s.len() {
        "0".to_owned()
    } else {
        s[trim..].to_owned()
    };}
    let value = arithmetic::integer(m, s, false)?;
    if meter::cmp(m, &value, &BigInt::from(u64::MAX))? == Ordering::Greater {
        return Err(invalid());
    }
    Ok(value)
}
pub(super) fn decimal(m: &mut CoreMeter, s: &mut String) -> Result<()> {
    if !m.check("decimal.length", || !s.is_empty() && s.len() <= 64)? {
        return Err(invalid());
    }
    let negative = m.check("decimal.sign", || s.starts_with('-'))?;
    if negative {
        return Err(invalid());
    }
    let digits = if negative { &s[1..] } else { s.as_str() };
    m.visit("decimal.split")?;
    let mut split = digits.split('.');
    let whole = split.next().ok_or_else(invalid)?;
    let fraction = split.next();
    if m.check("decimal.extra_separator", || split.next().is_some())?
        || !m.check("decimal.whole", || !whole.is_empty())?
    {
        return Err(invalid());
    }
    if let Some(f) = fraction {
        if !m.check("decimal.fraction", || !f.is_empty() && f.len() <= 18)? {
            return Err(invalid());
        }
    }
    for b in whole.bytes().chain(fraction.unwrap_or("").bytes()) {
        if !m.check("decimal.digit", || b.is_ascii_digit())? {
            return Err(invalid());
        }
    }
    let mut leading = 0;
    for b in whole.bytes() {
        if !m.check("decimal.leading_zero", || b == b'0')? {
            break;
        }
        leading += 1;
    }
    let w = if leading == whole.len() {
        "0"
    } else {
        &whole[leading..]
    };
    let f = fraction.unwrap_or("");
    let mut trailing = 0;
    for b in f.bytes().rev() {
        if !m.check("decimal.trailing_zero", || b == b'0')? {
            break;
        }
        trailing += 1;
    }
    let f = &f[..f.len() - trailing];
    let mut coefficient_digits = 0;
    let mut started = false;
    for b in w.bytes().chain(f.bytes()) {
        if !started {
            if m.check("decimal.coefficient_leading_zero", || b == b'0')? {
                continue;
            }
            started = true;
        }
        m.visit("decimal.coefficient_digit")?;
        coefficient_digits += 1;
    }
    if !m.check("decimal.coefficient_bound", || coefficient_digits <= 30)? {
        return Err(invalid());
    }
    let zero = m.check("decimal.zero", || w == "0" && f.is_empty())?;
    if m.representation_arena().is_some(){
        // Every byte has passed the original ASCII decimal guards above.
        // Normalization only removes leading zeroes/trailing fractional data;
        // retain the existing String allocation instead of allocating format!.
        let remove=if leading==whole.len(){work::usize_sub(m,leading,1)?}else{leading};
        let whole_length=work::usize_sub(m,whole.len(),remove)?;
        let fraction_length=if f.is_empty(){0}else{work::usize_add(m,1,f.len())?};
        let final_length=work::usize_add(m,whole_length,fraction_length)?;
        m.scan_bytes(s.len())?;m.write_bytes(s.len())?;
        s.drain(..remove);s.truncate(final_length);
    }else{*s = format!(
        "{}{}{}{}",
        if negative && !zero { "-" } else { "" },
        w,
        if f.is_empty() { "" } else { "." },
        f
    );}
    Ok(())
}
fn set<T: Serialize + DeserializeOwned>(m: &mut CoreMeter, items: &mut Vec<T>) -> Result<()> {
    let mut ordered = Vec::<(Vec<u8>, Value)>::new();
    for item in items.iter() {
        m.visit("source.set_member")?;
        let v = serde_json::to_value(item).map_err(|_| invalid())?;
        let bytes = codec::encode(m, &v, 262144)?;
        let mut position = ordered.len();
        for (i, (old, _)) in ordered.iter().enumerate() {
            m.visit("source.set_compare")?;
            match bytes.cmp(old) {
                Ordering::Equal => return Err(Error::new("INVALID_SHAPE", "duplicate set member")),
                Ordering::Less => {
                    position = i;
                    break;
                }
                Ordering::Greater => {}
            }
        }
        ordered.insert(position, (bytes, v));
    }
    *items = ordered
        .into_iter()
        .map(|(_, v)| serde_json::from_value(v).map_err(|_| invalid()))
        .collect::<Result<_>>()?;
    Ok(())
}
// RP01 sorts the SAME canonical member bytes while moving the existing typed
// members in place. It does not serialize into an unowned Value/byte vector or
// deserialize a second typed/String graph. Legacy `set` remains unchanged.
pub(super) fn owning_set<T>(m:&mut CoreMeter,items:&mut [T],value:fn(&AuditArena,&mut CoreMeter,&T)->Result<AuditedValue>)->Result<()> {
    let arena=m.representation_arena().ok_or_else(invalid)?;
    let per=mem::size_of::<(crate::canonical::audit_arena::AuditedBytes,usize)>()+mem::size_of::<usize>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&items.len().into(),&per.into()])?;
    let size=items.len().checked_mul(per).ok_or_else(invalid)?;
    let _metadata=arena.reserve_external_payload(size,m)?;
    let mut ordered=Vec::new();ordered.try_reserve_exact(items.len()).map_err(|_|invalid())?;
    let mut positions=Vec::new();positions.try_reserve_exact(items.len()).map_err(|_|invalid())?;
    for(index,item)in items.iter().enumerate(){
        m.visit("source.set_member")?;
        let body=value(&arena,m,item)?;
        let bytes=m.canonical_owned_representation(&arena,body.value(),RepresentationKind::SourceDefinition)?;
        let mut position=ordered.len();
        for(i,(old,_))in ordered.iter().enumerate(){
            let old:&crate::canonical::audit_arena::AuditedBytes=old;
            m.visit("source.set_compare")?;
            let read=work::usize_add(m,bytes.bytes().len(),old.bytes().len())?;m.scan_bytes(read)?;
            match bytes.bytes().cmp(old.bytes()){
                Ordering::Equal=>return Err(Error::new("INVALID_SHAPE","duplicate set member")),
                Ordering::Less=>{position=i;break;},Ordering::Greater=>{}
            }
        }
        let moved=work::usize_sub(m,ordered.len(),position)?;
        let moved=work::usize_add(m,moved,1)?;
        let entry=mem::size_of::<(crate::canonical::audit_arena::AuditedBytes,usize)>();
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&moved.into(),&entry.into()])?;
        let moved=moved.checked_mul(entry).ok_or_else(invalid)?;
        m.scan_bytes(moved)?;m.write_bytes(moved)?;
        m.write_bytes(mem::size_of::<usize>())?;
        ordered.insert(position,(bytes,index));positions.push(index);
    }
    // positions[k] is the original member currently at k. Each swap moves T;
    // none of its String/Vec payloads is copied or independently allocated.
    for(i,(_,wanted))in ordered.iter().enumerate(){
        let mut found=None;
        for(j,present)in positions.iter().enumerate().skip(i){
            m.visit("source.set_permutation_compare")?;
            if work::compare(m,*present,*wanted)?.is_eq(){found=Some(j);break;}
        }
        let j=found.ok_or_else(invalid)?;m.visit("source.set_permutation_swap")?;
        let cell=work::usize_add(m,mem::size_of::<T>(),mem::size_of::<usize>())?;
        ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&cell.into(),&3usize.into()])?;
        let movement=cell.checked_mul(3).ok_or_else(invalid)?;
        m.scan_bytes(movement)?;m.write_bytes(movement)?;
        items.swap(i,j);positions.swap(i,j);
    }
    Ok(())
}
fn text_value(arena:&AuditArena,m:&mut CoreMeter,value:&String)->Result<AuditedValue>{arena.string_value(value,m)}
pub(super) fn amount_value(arena:&AuditArena,m:&mut CoreMeter,value:&Amount)->Result<AuditedValue>{
    let currency=arena.string_value(&value.currency,m)?;let atoms=arena.string_value(&value.atoms,m)?;
    let scale=Value::from(value.scale);
    arena.object_from_fields(&[("currency",currency.value()),("scale",&scale),("atoms",atoms.value())],m)
}
pub(super) fn code_value(arena:&AuditArena,m:&mut CoreMeter,value:&Code)->Result<AuditedValue>{
    let code=arena.string_value(&value.code,m)?;
    let effect=match &value.effect{
        Effect::None=>{let kind=arena.string_value("none",m)?;arena.object_from_fields(&[("kind",kind.value())],m)?},
        Effect::Premium{amount}|Effect::Discount{amount}=>{
            let kind=arena.string_value(if matches!(value.effect,Effect::Premium{..}){"premium"}else{"discount"},m)?;
            let amount_kind=arena.string_value(&amount.kind,m)?;let percent=arena.string_value(&amount.percent,m)?;
            let amount=arena.object_from_fields(&[("kind",amount_kind.value()),("percent",percent.value())],m)?;
            arena.object_from_fields(&[("kind",kind.value()),("amount",amount.value())],m)?
        }
    };
    arena.object_from_fields(&[("code",code.value()),("effect",effect.value())],m)
}
pub(super) fn declaration_value(arena:&AuditArena,m:&mut CoreMeter,d:&AppCostDeclaration)->Result<AuditedValue>{
    let strings=[("component_key",d.component_key.as_str()),("activity",&d.activity),("resource_funding_leg",&d.resource_funding_leg),("usage_field",&d.usage_field),("usage_unit",&d.usage_unit),("work_source_slot",&d.work_source_slot),("cost_source_slot",&d.cost_source_slot),("correction_source_slot",&d.correction_source_slot),("finalize_within_us",&d.finalize_within_us),("correction_within_us",&d.correction_within_us),("on_missing",&d.on_missing),("cost_basis",&d.cost_basis),("comparable_quantity_rule",&d.comparable_quantity_rule),("allocation",&d.allocation)];
    let extra=work::usize_add(m,strings.len(),3)?;
    let per=mem::size_of::<AuditedValue>()+mem::size_of::<(&str,&Value)>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&extra.into(),&per.into()])?;
    let bytes=extra.checked_mul(per).ok_or_else(invalid)?;let _metadata=arena.reserve_external_payload(bytes,m)?;
    let mut values=Vec::new();values.try_reserve_exact(strings.len()).map_err(|_|invalid())?;
    for(_,text)in &strings{m.visit("source.app_declaration_string")?;values.push(arena.string_value(text,m)?);}
    let amount=amount_value(arena,m,&d.maximum_final_amount)?;
    let anchor=d.correction_anchor.as_ref().map(|text|arena.string_value(text,m)).transpose()?;
    let causal=if let Some(c)=&d.causal_reversal{
        let root=arena.string_value(&c.root_relation,m)?;let mode=arena.string_value(&c.mode,m)?;let deadline=arena.string_value(&c.standalone_deadline_effect,m)?;
        Some(arena.object_from_fields(&[("root_relation",root.value()),("mode",mode.value()),("standalone_deadline_effect",deadline.value())],m)?)
    }else{None};
    let mut fields=Vec::new();fields.try_reserve_exact(extra).map_err(|_|invalid())?;
    for((key,_),value)in strings.iter().zip(&values){m.visit("source.app_declaration_field")?;fields.push((*key,value.value()));}
    fields.push(("maximum_final_amount",amount.value()));
    if let Some(value)=&anchor{fields.push(("correction_anchor",value.value()));}
    if let Some(value)=&causal{fields.push(("causal_reversal",value.value()));}
    arena.object_from_fields(&fields,m)
}
pub(super) fn policy(m: &mut CoreMeter, p: &mut Policy) -> Result<()> {
    counter(m, &mut p.version)?;
    for s in &mut p.binding_slots {
        if m.representation_arena().is_some(){owning_set(m,&mut s.allowed_activities,text_value)?;}else{set(m, &mut s.allowed_activities)?;}
    }
    for r in &mut p.rules {
        match &mut r.formula {
            Formula::Fixed { amount } => decimal(m, amount)?,
            Formula::Unit { rate, .. } => decimal(m, rate)?,
            Formula::Discount { amount, .. } => decimal(m, &mut amount.percent)?,
            Formula::FinalCost {multiplier,markup,ceiling,..} | Formula::AppFinalCost {multiplier,markup,ceiling,..}=>{
                counter_atoms(m,&mut multiplier.numerator)?;
                counter_atoms(m,&mut multiplier.denominator)?;
                decimal(m,markup)?;
                counter_atoms(m,&mut ceiling.atoms)?;
            },
            Formula::SupplierFinalCost {ceiling,..}=>counter_atoms(m,&mut ceiling.atoms)?,
        }
    }
    for delayed in &mut p.delayed_costs {counter(m,&mut delayed.finalize_within_us)?;}
    if let Some(rows)=&mut p.app_delayed_costs {
        for declaration in rows.iter_mut(){
            counter(m,&mut declaration.finalize_within_us)?;
            counter(m,&mut declaration.correction_within_us)?;
            counter_atoms(m,&mut declaration.maximum_final_amount.atoms)?;
        }
        if m.representation_arena().is_some(){owning_set(m,rows,declaration_value)?;}else{set(m,rows)?;}
    }
    for f in &mut p.families {
        for c in &mut f.codes {
            match &mut c.effect {
                Effect::None => {}
                Effect::Premium { amount } | Effect::Discount { amount } => {
                    decimal(m, &mut amount.percent)?
                }
            }
        }
        if m.representation_arena().is_some(){owning_set(m,&mut f.codes,code_value)?;}else{set(m, &mut f.codes)?;}
        for s in [
            &mut f.window.start_offset_us,
            &mut f.window.occurrence_us,
            &mut f.window.receipt_grace_us,
            &mut f.window.acceptance_grace_us,
            &mut f.window.activation_deadline_us,
            &mut f.window.correction_us,
        ] {
            counter(m, s)?;
        }
        for a in [&mut f.maximum_premium, &mut f.maximum_discount] {
            counter_atoms(m, &mut a.atoms)?;
        }
    }
    Ok(())
}
fn counter_atoms(m: &mut CoreMeter, s: &mut String) -> Result<()> {
    if !m.check("atoms.length", || !s.is_empty() && s.len() <= 64)? {
        return Err(invalid());
    }
    for b in s.bytes() {
        if !m.check("atoms.digit", || b.is_ascii_digit())? {
            return Err(invalid());
        }
    }
    let mut leading = 0;
    for b in s.bytes() {
        if !m.check("atoms.leading_zero", || b == b'0')? {
            break;
        }
        leading += 1;
    }
    if m.representation_arena().is_some(){trim_digits_in_place(m,s,leading)?;}else{*s = if leading == s.len() {
        "0".to_owned()
    } else {
        s[leading..].to_owned()
    };}
    Ok(())
}
fn trim_digits_in_place(m:&mut CoreMeter,s:&mut String,leading:usize)->Result<()> {
    let remove=if work::compare(m,leading,s.len())?.is_eq(){work::usize_sub(m,leading,1)?}else{leading};
    m.scan_bytes(s.len())?;m.write_bytes(s.len())?;s.drain(..remove);Ok(())
}

#[cfg(test)]
mod owning_tests{
 use super::*;
 use crate::validation_work::OriginalAuditSession;
 use serde_json::json;
 #[test]
 fn owning_typed_sets_preserve_legacy_members_order_and_duplicates(){
  let arena=AuditArena::new().unwrap();let before=arena.usage().unwrap().live_bytes;
  let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
  let mut legacy=CoreMeter::for_compile(1048576).unwrap();
  let original=vec!["z".to_owned(),"\\".to_owned(),"😀".to_owned(),"A".to_owned(),"\n".to_owned()];
  let(mut old,mut new)=(original.clone(),original);set(&mut legacy,&mut old).unwrap();owning_set(session.meter_mut(),&mut new,text_value).unwrap();assert_eq!(old,new);
  let codes:Vec<Code>=serde_json::from_value(json!([{"code":"z","effect":{"kind":"none"}},{"code":"a","effect":{"kind":"premium","amount":{"kind":"percent","percent":"25"}}},{"code":"b","effect":{"kind":"discount","amount":{"kind":"percent","percent":"20"}}}])).unwrap();
  let(mut old,mut new)=(codes.clone(),codes);set(&mut legacy,&mut old).unwrap();owning_set(session.meter_mut(),&mut new,code_value).unwrap();assert_eq!(serde_json::to_value(old).unwrap(),serde_json::to_value(new).unwrap());
  let declaration=json!({"component_key":"model","activity":"complete","resource_funding_leg":"platform","usage_field":"tokens","usage_unit":"token","work_source_slot":"worker","cost_source_slot":"cost","correction_source_slot":"corrector","finalize_within_us":"10","correction_within_us":"20","maximum_final_amount":{"currency":"USD","scale":18,"atoms":"200"},"on_missing":"block","cost_basis":"attributable_final_amount","comparable_quantity_rule":"when_present_lte_original","allocation":"one_call_one_component"});
  let first:AppCostDeclaration=serde_json::from_value(declaration.clone()).unwrap();
  let mut second=declaration;second["component_key"]=json!("other");second["correction_anchor"]=json!("initial_cost_accepted_at");second["causal_reversal"]=json!({"root_relation":"original_work","mode":"full_inverse","standalone_deadline_effect":"waive"});
  let rows=vec![serde_json::from_value(second).unwrap(),first];
  let(mut old,mut new)=(rows.clone(),rows);set(&mut legacy,&mut old).unwrap();owning_set(session.meter_mut(),&mut new,declaration_value).unwrap();assert_eq!(serde_json::to_value(old).unwrap(),serde_json::to_value(new).unwrap());
  assert_eq!(arena.usage().unwrap().live_bytes,before);
  let mut duplicates=vec!["same".to_owned(),"same".to_owned()];let unchanged=duplicates.clone();
  let error=owning_set(session.meter_mut(),&mut duplicates,text_value).unwrap_err();assert_eq!(error.code,"INVALID_SHAPE");assert_eq!(duplicates,unchanged);assert_eq!(arena.usage().unwrap().live_bytes,before);
 }
 #[test]
 fn owning_scalar_normalization_reuses_storage_with_original_results(){
  let arena=AuditArena::new().unwrap();let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
  let mut legacy=CoreMeter::for_compile(1048576).unwrap();
  for text in ["0","0000","000123","000.0000","000.0012300","100.000","12.30","1.000000000000000001","0.000000000000000000","-0","1.",".1","1..2"]{
   let(mut a,mut b)=(text.to_owned(),text.to_owned());let address=b.as_ptr();let capacity=b.capacity();
   let expected=decimal(&mut legacy,&mut a);let actual=decimal(session.meter_mut(),&mut b);
   assert_eq!(expected.as_ref().map(|_|()),actual.as_ref().map(|_|()),"{text}");
   assert_eq!(a,b,"{text}");assert_eq!(b.as_ptr(),address);assert_eq!(b.capacity(),capacity);
  }
  for text in ["0","000","00123","123","","01x","18446744073709551615"]{
   let(mut a,mut b)=(text.to_owned(),text.to_owned());let address=b.as_ptr();let capacity=b.capacity();
   let expected=counter(&mut legacy,&mut a);let actual=counter(session.meter_mut(),&mut b);
   assert_eq!(expected,actual,"{text}");assert_eq!(a,b,"{text}");assert_eq!(b.as_ptr(),address);assert_eq!(b.capacity(),capacity);
   let(mut a,mut b)=(text.to_owned(),text.to_owned());let address=b.as_ptr();
   let expected=counter_atoms(&mut legacy,&mut a);let actual=counter_atoms(session.meter_mut(),&mut b);
   assert_eq!(expected,actual,"{text}");assert_eq!(a,b,"{text}");assert_eq!(b.as_ptr(),address);
  }
 }
}
