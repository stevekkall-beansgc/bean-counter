//! Selected exact pricing, invoked only by the shared draft constructor after
//! typed fact and authority-data validation. It supplies no public calculator.
use super::super::selected_program::SelectedProgram;
use super::{
    CoreMeter, Phase,
    arithmetic::{self, Ratio},
    meter, shape,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use num_traits::One;
use serde_json::Value;
pub(super) struct PricedInstruction {
    pub ordinal: String,
    pub component_key: String,
    pub root_key: String,
    pub role_slot: String,
    pub binding_slot: String,
    pub basis_components: Vec<String>,
    pub rational: Ratio,
    pub atoms: i128,
    // Row payload survives pop/iteration independently of the vector owner.
    pub(super) _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
/// The existing evaluator's actual result vector, with its allocation owner.
/// This is private pricing DATA, not eligibility or a different evaluator.
pub(super) struct PricedInstructions {
    rows:Vec<PricedInstruction>,
    _ownership:Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl std::fmt::Debug for PricedInstruction {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{
        f.debug_struct("PricedInstruction").field("ordinal",&self.ordinal)
            .field("component_key",&self.component_key).field("root_key",&self.root_key)
            .field("role_slot",&self.role_slot).field("binding_slot",&self.binding_slot)
            .field("basis_components",&self.basis_components).field("rational",&self.rational)
            .field("atoms",&self.atoms).finish()
    }
}
impl std::fmt::Debug for PricedInstructions {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{self.rows.fmt(f)}
}
impl PricedInstructions {
    pub(super) fn empty()->Self{Self{rows:Vec::new(),_ownership:None}}
}
impl std::ops::Deref for PricedInstructions {
    type Target=Vec<PricedInstruction>;
    fn deref(&self)->&Self::Target{&self.rows}
}
impl std::ops::DerefMut for PricedInstructions {
    fn deref_mut(&mut self)->&mut Self::Target{&mut self.rows}
}
impl<'a> IntoIterator for &'a PricedInstructions {
    type Item=&'a PricedInstruction;type IntoIter=std::slice::Iter<'a,PricedInstruction>;
    fn into_iter(self)->Self::IntoIter{self.rows.iter()}
}
pub(super) struct PricedIntoIter {
    rows:std::vec::IntoIter<PricedInstruction>,
    _ownership:Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
impl Iterator for PricedIntoIter {
    type Item=PricedInstruction;
    fn next(&mut self)->Option<Self::Item>{self.rows.next()}
}
impl IntoIterator for PricedInstructions {
    type Item=PricedInstruction;type IntoIter=PricedIntoIter;
    fn into_iter(self)->Self::IntoIter{PricedIntoIter{rows:self.rows.into_iter(),_ownership:self._ownership}}
}
fn copied(m:&mut CoreMeter,value:&str)->Result<String>{
    if m.representation_arena().is_some(){m.scan_bytes(value.len())?;m.write_bytes(value.len())?;}
    Ok(value.to_owned())
}
fn output_owner(m:&mut CoreMeter,n:usize)->Result<Option<crate::canonical::audit_arena::ExternalBufferLease>>{
    use crate::validation_work::{self as w,ArithmeticOperation,ValidationWork};
    let Some(arena)=m.representation_arena()else{return Ok(None)};
    let per=std::mem::size_of::<PricedInstruction>();
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&n.into(),&per.into()])?;
    let size=n.checked_mul(per).ok_or_else(||fail("pricing output dimension"))?;
    let size=w::usize_add(m,size,std::mem::size_of::<PricedInstructions>())?;
    let lease=arena.reserve_external_payload(size,m)?;m.write_bytes(size)?;Ok(Some(lease))
}
fn row_owner(m:&mut CoreMeter,instruction:&Value,rule:&Value,component:&Value)->Result<Option<crate::canonical::audit_arena::ExternalBufferLease>>{
    use crate::validation_work as w;
    let Some(arena)=m.representation_arena()else{return Ok(None)};
    let mut size=std::mem::size_of::<PricedInstruction>();
    for(value,key)in[(instruction,"ordinal"),(rule,"component_key"),(component,"root_key"),(component,"role_slot"),(rule,"binding_slot")]{
        m.visit("pricing.output_string_dimension")?;
        let raw=value.get(key).and_then(Value::as_str).ok_or_else(||fail("pricing typed output field"))?;
        size=w::usize_add(m,size,raw.len())?;
    }
    m.visit("pricing.output_basis_variant")?;
    if rule["formula"]["kind"].as_str()==Some("discount"){
        m.visit("pricing.output_basis_dimension")?;
        let raw=rule["formula"]["basis"]["component_key"].as_str().ok_or_else(||fail("pricing typed basis"))?;
        size=w::usize_add(m,size,raw.len())?;
        size=w::usize_add(m,size,std::mem::size_of::<String>())?;
    }
    // Output scalar limbs only. Arithmetic-kernel temporaries/native scratch
    // remain a separate containing proof; this lease never credits them.
    // Pinned num-bigint0.5.1 mul3 allocates x.len()+y.len()+1 digits.
    // Two512-bit operands therefore need <=17u64 digits (136 bytes),
    // including the extra carry digit retained after normalization. The
    // u32-digit build needs <=33*4 bytes and is covered by the same bound.
    size=w::usize_add(m,size,2*(std::mem::size_of::<BigInt>()+136))?;
    let lease=arena.reserve_external_payload(size,m)?;m.write_bytes(size)?;Ok(Some(lease))
}
fn fail(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
pub(super) fn work(
    program: &SelectedProgram,
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    let instructions = program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .ok_or_else(|| fail("compiled instructions"))?;
    let components = program.compiled_plan().canonical_value()["component_slots"]
        .as_array()
        .ok_or_else(|| fail("compiled components"))?;
    if program.uses_app_owned_cost_recipe(){
        price_instructions_selected(instructions,components,activity,normalized_facts,m,Some(CostSelection::AppDeferred))
    } else if program.uses_owner_local_cost_recipe(){
        price_instructions_selected(instructions,components,activity,normalized_facts,m,Some(CostSelection::Deferred))
    }else{price_instructions(instructions, components, activity, normalized_facts, m)}
}
pub(super) fn price_instructions(
    instructions: &[Value],
    components: &[Value],
    activity: &str,
    normalized_facts: &Value,
    m: &mut CoreMeter,
) -> Result<PricedInstructions> {
    price_instructions_selected(instructions,components,activity,normalized_facts,m,None)
}
enum CostSelection<'a>{Deferred,Final{supplier_slot:&'a str,supplier_component:Option<&'a str>,eligible_atoms:&'a BigInt},AppDeferred,AppFinal{component_key:&'a str,eligible_atoms:&'a BigInt}}
/// Only the authenticated cost executor supplies eligible supplier atoms. The
/// original program supplies every formula; saved action amounts are never
/// pricing operands.
pub(super) fn final_cost(program:&SelectedProgram,activity:&str,supplier_slot:&str,supplier_component:&str,eligible_atoms:&BigInt,m:&mut CoreMeter)->Result<PricedInstructions>{
    if !program.uses_owner_local_cost_recipe(){return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","original supplier compiler recipe"));}
    let instructions=shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")?;
    let components=shape::array(m,program.compiled_plan().canonical_value(),"component_slots")?;
    let maximum=arithmetic::integer(m,"999999999999999999999999999999",false)?;
    if meter::cmp(m,eligible_atoms,&0.into())?.is_lt()||meter::cmp(m,eligible_atoms,&maximum)?.is_gt(){return Err(fail("eligible supplier amount"));}
    price_instructions_selected(instructions,components,activity,&Value::Null,m,Some(CostSelection::Final{supplier_slot,supplier_component:Some(supplier_component),eligible_atoms}))
}
/// Actual attributable amount and original WorkRecord facts enter the SAME
/// original instruction evaluator. There is no supplier selection or payable.
/// Evidence/source/lineage eligibility is established by the app executor.
pub(super) fn app_final_cost(program:&SelectedProgram,activity:&str,component_key:&str,original_facts:&Value,eligible_atoms:&BigInt,m:&mut CoreMeter)->Result<PricedInstructions>{
    if !program.uses_app_owned_cost_recipe(){return Err(Error::new("ORIGINAL_VERIFIER_UNAVAILABLE","original app cost compiler recipe"));}
    let instructions=shape::array(m,program.compiled_plan().canonical_value(),"typed_instructions")?;
    let components=shape::array(m,program.compiled_plan().canonical_value(),"component_slots")?;
    let maximum=arithmetic::integer(m,"999999999999999999999999999999",false)?;
    if meter::cmp(m,eligible_atoms,&0.into())?.is_lt()||meter::cmp(m,eligible_atoms,&maximum)?.is_gt(){return Err(fail("eligible attributable amount"));}
    price_instructions_selected(instructions,components,activity,original_facts,m,Some(CostSelection::AppFinal{component_key,eligible_atoms}))
}
fn price_instructions_selected(instructions:&[Value],components:&[Value],activity:&str,normalized_facts:&Value,m:&mut CoreMeter,selection:Option<CostSelection<'_>>)->Result<PricedInstructions>{
    m.phase(Phase::Pricing);
    let scale = arithmetic::integer(m, "1000000000000000000", false)?;
    let atom_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let scale = Ratio {
        numerator: scale,
        denominator: BigInt::one(),
    };
    let output_ownership=output_owner(m,instructions.len())?;
    let mut output: Vec<PricedInstruction> = Vec::new();
    if output_ownership.is_some(){output.try_reserve_exact(instructions.len()).map_err(|_|fail("pricing output allocation"))?;}
    for instruction in instructions {
        if !shape::same(
            m,
            &instruction["opcode"],
            &serde_json::json!("evaluate_rule"),
        )? {
            continue;
        }
        let rule = &instruction["rule"];
        if !shape::same(m, &rule["activity"], &serde_json::json!(activity))? {
            continue;
        }
        if let Some(selection)=&selection {
            let kind=shape::text(m,&rule["formula"],"kind")?;
            let retail=shape::same_text(m,kind,"final_cost")?;
            let supplier=shape::same_text(m,kind,"supplier_final_cost")?;
            match selection {
                CostSelection::AppDeferred=>{
                    if shape::same_text(m,kind,"app_final_cost")?{continue;}
                    if retail||supplier{return Err(fail("AP formula in app-only program"));}
                },
                CostSelection::AppFinal{component_key,..}=>{
                    if !shape::same_text(m,kind,"app_final_cost")?{continue;}
                    let actual=shape::text(m,rule,"component_key")?;
                    if !shape::same_text(m,actual,component_key)?{continue;}
                },
                CostSelection::Deferred=>{if retail||supplier{continue;}},
                CostSelection::Final{supplier_slot,supplier_component,..}=>{
                    if !retail&&!supplier{continue;}
                    if supplier { if let Some(component)=supplier_component { let actual=shape::text(m,rule,"component_key")?; if !shape::same_text(m,actual,component)?{continue;} } }
                    let selected=if retail{shape::text(m,&rule["formula"],"supplier_slot")?}else{shape::text(m,rule,"binding_slot")?};
                    if !shape::same_text(m,selected,supplier_slot)?{continue;}
                },
            }
        }
        m.visit("pricing.instruction")?;
        let key = rule["component_key"]
            .as_str()
            .ok_or_else(|| fail("component key"))?;
        let mut component = None;
        for candidate in components {
            if shape::same(m, &candidate["key"], &serde_json::json!(key))? {
                component = Some(candidate);
                break;
            }
        }
        let component = component.ok_or_else(|| fail("compiled component missing"))?;
        let formula = &rule["formula"];
        let row_ownership=row_owner(m,instruction,rule,component)?;
        let mut basis_components = Vec::new();
        m.visit("pricing.formula_variant")?;
        let rational = match formula["kind"]
            .as_str()
            .ok_or_else(|| fail("formula kind"))?
        {
            "fixed" => {
                let amount = arithmetic::decimal(
                    m,
                    formula["amount"]
                        .as_str()
                        .ok_or_else(|| fail("fixed amount"))?,
                )?;
                arithmetic::multiply(m, &amount, &scale)?
            }
            "unit" => {
                let field = formula["quantity_field"]
                    .as_str()
                    .ok_or_else(|| fail("quantity field"))?;
                m.visit("pricing.fact_lookup")?;
                let quantity = arithmetic::integer(
                    m,
                    normalized_facts[field]
                        .as_str()
                        .ok_or_else(|| fail("normalized integer fact"))?,
                    true,
                )?;
                let rate = arithmetic::decimal(
                    m,
                    formula["rate"].as_str().ok_or_else(|| fail("unit rate"))?,
                )?;
                let quantity = Ratio {
                    numerator: quantity,
                    denominator: BigInt::one(),
                };
                let amount = arithmetic::multiply(m, &quantity, &rate)?;
                arithmetic::multiply(m, &amount, &scale)?
            }
            "discount" => {
                let base = formula["basis"]["component_key"]
                    .as_str()
                    .ok_or_else(|| fail("basis key"))?;
                let binding_slot = rule["binding_slot"]
                    .as_str()
                    .ok_or_else(|| fail("binding slot"))?;
                let mut prior = None;
                for candidate in &output {
                    if shape::same_text(m, &candidate.component_key, base)?
                        && shape::same_text(m, &candidate.binding_slot, binding_slot)?
                    {
                        prior = Some(candidate);
                        break;
                    }
                }
                let prior = prior.ok_or_else(|| fail("same work named basis missing"))?;
                if row_ownership.is_some(){basis_components.try_reserve_exact(1).map_err(|_|fail("pricing basis allocation"))?;}
                basis_components.push(copied(m,base)?);
                let percentage = arithmetic::decimal(
                    m,
                    formula["amount"]["percent"]
                        .as_str()
                        .ok_or_else(|| fail("percentage"))?,
                )?;
                let denominator = meter::mul(m, &percentage.denominator, &BigInt::from(100))?;
                let negative = meter::neg(m, &percentage.numerator)?;
                let percent = arithmetic::rational(m, negative, denominator)?;
                arithmetic::multiply(
                    m,
                    &Ratio {
                        numerator: BigInt::from(prior.atoms),
                        denominator: BigInt::one(),
                    },
                    &percent,
                )?
            }
            "app_final_cost"=>{
                let atoms=match &selection{Some(CostSelection::AppFinal{eligible_atoms,..})=>*eligible_atoms,_=>return Err(Error::new("WAITING_FINAL_COST","actual authenticated app final amount required"))};
                let eligible=Ratio{numerator:atoms.clone(),denominator:BigInt::one()};
                customer_final_cost_rational(m,formula,&eligible,&scale)?
            },
            "supplier_final_cost"|"final_cost"=>{
                let atoms=match &selection{Some(CostSelection::Final{eligible_atoms,..})=>*eligible_atoms,_=>return Err(Error::new("WAITING_FINAL_COST","exact final supplier evidence required"))};
                let supplier=Ratio{numerator:atoms.clone(),denominator:BigInt::one()};
                if shape::same_text(m,formula["kind"].as_str().unwrap_or(""),"supplier_final_cost")?{supplier}else{
                    customer_final_cost_rational(m,formula,&supplier,&scale)?
                }
            },
            _ => return Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE", "selected formula")),
        };
        let atoms = arithmetic::rounded(m, &rational, &atom_limit)?;
        if matches!(selection,Some(CostSelection::Final{..}|CostSelection::AppFinal{..})){
            let ceiling=&formula["ceiling"];
            let currency=shape::text(m,ceiling,"currency")?;
            if !shape::same_text(m,currency,"USD")? || !shape::same(m,&ceiling["scale"],&serde_json::json!(18))?{return Err(fail("cost ceiling units"));}
            let ceiling=shape::text(m,ceiling,"atoms")?;
            let ceiling=arithmetic::integer(m,ceiling,false)?;
            if meter::cmp(m,&BigInt::from(atoms),&ceiling)?.is_gt(){return Err(Error::new("EXPOSURE_CEILING_EXCEEDED","original cost formula ceiling"));}
        }
        output.push(PricedInstruction {
            ordinal:copied(m,instruction["ordinal"].as_str().ok_or_else(||fail("ordinal"))?)?,
            component_key:copied(m,key)?,
            root_key:copied(m,component["root_key"].as_str().ok_or_else(||fail("root key"))?)?,
            role_slot:copied(m,component["role_slot"].as_str().ok_or_else(||fail("role slot"))?)?,
            binding_slot:copied(m,rule["binding_slot"].as_str().ok_or_else(||fail("binding slot"))?)?,
            basis_components,rational,atoms,_ownership:row_ownership,
        });
    }
    Ok(PricedInstructions{rows:output,_ownership:output_ownership})
}

/// SAME original customer cost formula for every eligible cost-basis mode.
/// Its caller supplies an actually validated attributable amount; this helper
/// does not select evidence, grant charge authority or produce an AP payable.
/// Rounding remains at the one original instruction site after this rational.
pub(super) fn customer_final_cost_rational(m:&mut CoreMeter,formula:&Value,eligible:&Ratio,scale:&Ratio)->Result<Ratio>{
    let numerator=shape::text(m,&formula["multiplier"],"numerator")?;
    let denominator=shape::text(m,&formula["multiplier"],"denominator")?;
    let numerator=arithmetic::integer(m,numerator,false)?;
    let denominator=arithmetic::integer(m,denominator,false)?;
    if meter::cmp(m,&numerator,&0.into())?.is_lt()||!meter::cmp(m,&denominator,&0.into())?.is_gt(){return Err(fail("original pass through multiplier"));}
    let multiplier=arithmetic::rational(m,numerator,denominator)?;
    let amount=arithmetic::multiply(m,eligible,&multiplier)?;
    let markup=shape::text(m,formula,"markup")?;
    let markup=arithmetic::decimal(m,markup)?;
    if meter::cmp(m,&markup.numerator,&0.into())?.is_lt(){return Err(fail("negative original cost markup"));}
    let markup=arithmetic::multiply(m,&markup,scale)?;
    add_ratio(m,&amount,&markup)
}

fn add_ratio(m:&mut CoreMeter,a:&Ratio,b:&Ratio)->Result<Ratio>{
    let left=meter::mul(m,&a.numerator,&b.denominator)?;
    let right=meter::mul(m,&b.numerator,&a.denominator)?;
    let n=meter::add(m,&left,&right)?;
    let d=meter::mul(m,&a.denominator,&b.denominator)?;
    arithmetic::rational(m,n,d)
}

/// One original family alternative, evaluated only against its frozen named
/// component net. This is shared by initial and replacement outcome records;
/// no current policy, aggregate invoice, or current statement enters the basis.
pub(super) struct PricedOutcome {
    pub ordinal: String,
    pub root_key: String,
    pub family_key: String,
    pub rational: Ratio,
    pub atoms: i128,
    pub none: bool,
    pub(super) _ownership:Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
fn outcome_owner(m:&mut CoreMeter,instruction:&Value,family:&Value,key:&str)->Result<Option<crate::canonical::audit_arena::ExternalBufferLease>>{
    use crate::validation_work as w;
    let Some(arena)=m.representation_arena()else{return Ok(None)};
    let ordinal=shape::text(m,instruction,"ordinal")?;
    let component=shape::text(m,&family["selector"],"component_key")?;
    let mut size=std::mem::size_of::<PricedOutcome>()+3*std::mem::size_of::<&str>()
        +2*(std::mem::size_of::<BigInt>()+136);
    for raw in [ordinal,component,key]{m.visit("pricing.family_output_dimension")?;size=w::usize_add(m,size,raw.len())?;}
    let lease=arena.reserve_external_payload(size,m)?;m.write_bytes(size)?;Ok(Some(lease))
}
macro_rules! paid_call {
    ($f:path,$m:expr,$a:expr $(,)?) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr $(,)?) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr $(,)?) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
/// Explicit correction with no replacement code or facts. Select only the
/// original compiled family instruction; do not invent a none-code attestation.
pub(super) fn explicit_none(
    program: &SelectedProgram,
    family_key: &str,
    m: &mut CoreMeter,
) -> Result<PricedOutcome> {
    m.phase(Phase::Pricing);
    let mut selected = None;
    for instruction in paid_call!(
        shape::array,
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions"
    )? {
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, instruction, "opcode")?,
            "evaluate_family"
        )? {
            continue;
        }
        let family = paid_call!(shape::field, m, instruction, "family")?;
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, family, "family_key")?,
            family_key
        )? {
            continue;
        }
        m.visit("pricing.unique_family")?;
        if selected.is_some() {
            return Err(fail("ambiguous original family"));
        }
        selected = Some((instruction, family));
    }
    let (instruction, family) = selected.ok_or_else(|| fail("original family missing"))?;
    let output_ownership=outcome_owner(m,instruction,family,family_key)?;
    let ordinal=paid_call!(shape::text,m,instruction,"ordinal")?;
    let ordinal=copied(m,ordinal)?;
    let root=paid_call!(shape::text,m,&family["selector"],"component_key")?;
    let root=copied(m,root)?;
    let key=copied(m,family_key)?;
    Ok(PricedOutcome {
        ordinal,root_key:root,family_key:key,_ownership:output_ownership,
        rational: Ratio {
            numerator: 0.into(),
            denominator: 1.into(),
        },
        atoms: 0,
        none: true,
    })
}
pub(super) fn outcome(
    program: &SelectedProgram,
    family_key: &str,
    code: &str,
    original_net: &BigInt,
    m: &mut CoreMeter,
) -> Result<PricedOutcome> {
    m.phase(Phase::Pricing);
    if meter::cmp(m, original_net, &0.into())?.is_lt() {
        return Err(fail("negative frozen family net"));
    }
    let mut selected = None;
    for instruction in program.compiled_plan().canonical_value()["typed_instructions"]
        .as_array()
        .ok_or_else(|| fail("original instructions"))?
    {
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, instruction, "opcode")?,
            "evaluate_family"
        )? {
            continue;
        }
        let family = paid_call!(shape::field, m, instruction, "family")?;
        if paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, family, "family_key")?,
            family_key
        )? {
            m.visit("pricing.unique_family")?;
            if selected.is_some() {
                return Err(fail("ambiguous original family"));
            }
            selected = Some((instruction, family));
        }
    }
    let (instruction, family) = selected.ok_or_else(|| fail("original family missing"))?;
    let output_ownership=outcome_owner(m,instruction,family,family_key)?;
    let choice = paid_call!(
        shape::named,
        m,
        paid_call!(shape::array, m, family, "codes")?,
        "code",
        code
    )?;
    let effect = paid_call!(shape::field, m, choice, "effect")?;
    let kind = paid_call!(shape::text, m, effect, "kind")?;
    let none = paid_call!(shape::same_text, m, kind, "none")?;
    let mut rational = Ratio {
        numerator: 0.into(),
        denominator: 1.into(),
    };
    if !none {
        let discount = paid_call!(shape::same_text, m, kind, "discount")?;
        if !discount && !paid_call!(shape::same_text, m, kind, "premium")? {
            return Err(fail("original family effect"));
        }
        let formula = paid_call!(shape::field, m, effect, "amount")?;
        if !paid_call!(
            shape::same_text,
            m,
            paid_call!(shape::text, m, formula, "kind")?,
            "percent"
        )? {
            return Err(Error::new(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "selected family percent",
            ));
        }
        let scalar = paid_call!(shape::text, m, formula, "percent")?;
        let percent = arithmetic::decimal(m, scalar)?;
        let hundred = meter::mul(m, &percent.denominator, &100.into())?;
        if meter::cmp(m, &percent.numerator, &0.into())?.is_lt()
            || meter::cmp(m, &percent.numerator, &hundred)?.is_gt()
        {
            return Err(fail("original family percent interval"));
        }
        let numerator = if discount {
            meter::neg(m, &percent.numerator)?
        } else {
            percent.numerator
        };
        let percent = arithmetic::rational(m, numerator, hundred)?;
        rational = arithmetic::multiply(
            m,
            &Ratio {
                numerator: original_net.clone(),
                denominator: 1.into(),
            },
            &percent,
        )?;
    }
    let atom_limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let atoms = arithmetic::rounded(m, &rational, &atom_limit)?;
    if !none {
        let bound_key = if meter::cmp(m, &atoms.into(), &0.into())?.is_lt() {
            "maximum_discount"
        } else {
            "maximum_premium"
        };
        let declaration = paid_call!(shape::field, m, family, bound_key)?;
        let scalar = paid_call!(shape::text, m, declaration, "atoms")?;
        let limit = arithmetic::integer(m, scalar, false)?;
        let magnitude = meter::absolute(m, &atoms.into())?;
        if meter::cmp(m, &magnitude, &limit)?.is_gt() {
            return Err(fail("original family amount ceiling"));
        }
        if paid_call!(shape::same_text, m, bound_key, "maximum_discount")?
            && meter::cmp(m, &magnitude, original_net)?.is_gt()
        {
            return Err(fail("discount exceeds frozen original net"));
        }
    }
    let ordinal=paid_call!(shape::text,m,instruction,"ordinal")?;
    let ordinal=copied(m,ordinal)?;
    let root=paid_call!(shape::text,m,&family["selector"],"component_key")?;
    let root=copied(m,root)?;
    let key=copied(m,family_key)?;
    Ok(PricedOutcome {
        ordinal,root_key:root,family_key:key,_ownership:output_ownership,
        rational,
        atoms,
        none,
    })
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn final_supplier_and_retail_share_original_kernel_without_unit_rate_proxy(){
        let input:Value=serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/owner-local-cost-01/fixture-input.json")).unwrap();
        let policy=&input["records"].as_array().unwrap().iter().find(|r|r["definition"]=="Policy2").unwrap()["body"];
        let instructions:Vec<Value>=policy["rules"].as_array().unwrap().iter().enumerate().map(|(i,r)|serde_json::json!({"opcode":"evaluate_rule","ordinal":i.to_string(),"rule":r})).collect();
        let components:Vec<Value>=policy["rules"].as_array().unwrap().iter().map(|r|serde_json::json!({"key":r["component_key"],"root_key":r["component_key"],"role_slot":"test:caller-selected-role"})).collect();
        let mut m=CoreMeter::new(65536).unwrap();
        let deferred=price_instructions_selected(&instructions,&components,"support:complete",&serde_json::json!({}),&mut m,Some(CostSelection::Deferred)).unwrap();
        assert_eq!(deferred.len(),1);assert_eq!(deferred[0].atoms,1_000_000_000_000_000_000);
        let eligible=BigInt::from(2_000_000_000_000_000_000i128);
        let finalized=price_instructions_selected(&instructions,&components,"support:complete",&Value::Null,&mut m,Some(CostSelection::Final{supplier_slot:"support:supplier",supplier_component:None,eligible_atoms:&eligible})).unwrap();
        assert_eq!(finalized.len(),2);
        for price in &finalized{assert_eq!(price.atoms,2_000_000_000_000_000_000);}
        let mut changed=instructions.clone();
        for instruction in &mut changed{
            if instruction["rule"]["formula"]["kind"]=="final_cost"{
                instruction["rule"]["formula"]["multiplier"]=serde_json::json!({"numerator":"3","denominator":"2"});
                instruction["rule"]["formula"]["markup"]=serde_json::json!("0.1");
            }
        }
        let eligible=BigInt::from(1_600_000_000_000_000_000i128);
        let next=price_instructions_selected(&changed,&components,"support:complete",&Value::Null,&mut m,Some(CostSelection::Final{supplier_slot:"support:supplier",supplier_component:None,eligible_atoms:&eligible})).unwrap();
        let retail=next.iter().find(|p|p.binding_slot=="support:retail").unwrap();
        let supplier=next.iter().find(|p|p.binding_slot=="support:supplier").unwrap();
        assert_eq!(retail.atoms,2_500_000_000_000_000_000);assert_eq!(supplier.atoms,1_600_000_000_000_000_000);
        let eligible=BigInt::from(3_000_000_000_000_000_000i128);
        assert_eq!(price_instructions_selected(&changed,&components,"support:complete",&Value::Null,&mut m,Some(CostSelection::Final{supplier_slot:"support:supplier",supplier_component:None,eligible_atoms:&eligible})).err().unwrap().code,"EXPOSURE_CEILING_EXCEEDED");
    }
    #[test]
    fn explicit_no_replacement_uses_original_ordinal_without_a_code() {
        let (program, _, _, _) = super::super::first_work_tests::test_inputs();
        let mut meter = CoreMeter::new(65536).unwrap();
        let price = explicit_none(&program, "fixture.measure:assessment", &mut meter).unwrap();
        let choice = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &999000000000000000_i128.into(),
            &mut meter,
        )
        .unwrap();
        assert_eq!(price.ordinal, choice.ordinal);
        assert_eq!(price.root_key, choice.root_key);
        assert_eq!(price.rational.numerator, 0.into());
        assert_eq!(price.rational.denominator, 1.into());
        assert!(price.none);
        assert_eq!(price.atoms, 0);
        assert!(explicit_none(&program, "absent:family", &mut meter).is_err());
        // Same selected family evaluator/none branch with independent row
        // owners; no constructor, source-origin or capacity eligibility.
        let arena=crate::canonical::audit_arena::AuditArena::new().unwrap();
        let before=arena.usage().unwrap().live_bytes;
        let mut original=crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child=original.meter_mut().original_economic_meter().unwrap();
        let own_none=explicit_none(&program,"fixture.measure:assessment",&mut child).unwrap();
        let own_choice=outcome(&program,"fixture.measure:assessment","fixture.measure:success",&999000000000000000_i128.into(),&mut child).unwrap();
        assert_eq!(own_none.ordinal,price.ordinal);assert_eq!(own_none.rational,price.rational);
        assert_eq!(own_choice.ordinal,choice.ordinal);assert_eq!(own_choice.rational,choice.rational);assert_eq!(own_choice.atoms,choice.atoms);
        assert!(own_none._ownership.is_some()&&own_choice._ownership.is_some());
        drop(own_none);drop(own_choice);original.meter_mut().finish_original_economic(child).unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes,before);

    }
    #[test]
    fn original_family_none_zero_and_signed_alternatives_remain_distinct() {
        // The production selected compiler supplies the program. No expected
        // economic output is supplied as a constructor/pricing operand.
        let (program, _, _, _) = super::super::first_work_tests::test_inputs();
        let mut m = CoreMeter::new(65536).unwrap();
        let net = arithmetic::integer(&mut m, "999000000000000000", false).unwrap();
        let success = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(success.atoms, 199800000000000000);
        assert!(!success.none);
        let rebate = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:rebate",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(rebate.atoms, -99900000000000000);
        assert!(!rebate.none);
        let none = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:none",
            &net,
            &mut m,
        )
        .unwrap();
        assert_eq!(none.atoms, 0);
        assert!(none.none);
        let zero = outcome(
            &program,
            "fixture.measure:assessment",
            "fixture.measure:success",
            &0.into(),
            &mut m,
        )
        .unwrap();
        assert_eq!(zero.atoms, 0);
        assert!(!zero.none);
        assert!(
            outcome(
                &program,
                "fixture.measure:assessment",
                "fixture.measure:unknown",
                &net,
                &mut m
            )
            .is_err()
        );
        assert!(
            outcome(
                &program,
                "fixture.measure:assessment",
                "fixture.measure:success",
                &(-1).into(),
                &mut m
            )
            .is_err()
        );
    }
}
