//! Original-formula pricing callbacks in the prospective S expression.
//! The SAME pricing entry's plan/selection walks compose with its original
//! scalar/rational/rounding kernels. Record construction, output ownership,
//! representation work and the containing executor are separate obligations.
//! No saved capacity, observed callback total, or caller allowance is an input.
use super::super::{CoreMeter,shape};
use crate::{Result,validation_work::{self as w,ArithmeticOperation,ValidationWork}};
use serde_json::Value;

// Evaluate nested dimensions before borrowing the shared meter for the
// enclosing checked operation; no temporary meter or phase credit is used.
macro_rules! bound_call {
    ($f:ident,$m:expr,$a:expr) => {{let a=$a;$f($m,a)}};
    ($f:ident,$m:expr,$a:expr,$b:expr) => {{let(a,b)=($a,$b);$f($m,a,b)}};
}

pub(super) struct PricingKernels {
    pub work:usize,
    pub maximum_family:usize,
    pub maximum_app:usize,
}
#[derive(Clone,Copy)]
struct RatioBits { n:usize,d:usize }
fn add(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{w::usize_add(m,a,b)}
fn mul(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{
    ValidationWork::arithmetic(m,ArithmeticOperation::Multiply,&[&a.into(),&b.into()])?;
    a.checked_mul(b).ok_or_else(||shape::fail("prospective arithmetic work product"))
}
fn maximum(m:&mut CoreMeter,a:usize,b:usize)->Result<usize>{Ok(if w::compare(m,a,b)?.is_gt(){a}else{b})}
fn limbs(m:&mut CoreMeter,bits:usize)->Result<usize>{
    let numerator=bound_call!(add,m,bits,63)?;
    ValidationWork::arithmetic(m,ArithmeticOperation::DivideRemainder,&[&numerator.into(),&64usize.into()])?;
    bound_call!(maximum,m,numerator/64,1)
}
fn digits(m:&mut CoreMeter,raw:&str)->Result<(usize,usize)>{
    if w::compare(m,raw.len(),64usize)?.is_gt(){return Err(shape::fail("original scalar resource dimension"));}
    let mut count=0;let mut significant=0;
    for ch in raw.bytes(){
        m.visit("capacity.original_numeric_dimension")?;m.scan_bytes(1)?;
        if ch.is_ascii_digit(){
            count=bound_call!(add,m,count,1)?;m.visit("capacity.original_significant_digit")?;
            let nonzero=if ch!=b'0'{true}else{!w::compare(m,significant,0usize)?.is_eq()};
            if nonzero{significant=bound_call!(add,m,significant,1)?;}
        }
    }
    Ok((count,significant))
}
fn integer(m:&mut CoreMeter,raw:&str)->Result<(usize,usize)>{
    let(d,significant)=bound_call!(digits,m,raw)?;
    // At d digits every accumulator/product is <10^d<2^(4d). The ten and
    // digit operands have one limb. Each digit therefore pays (2+L) multiply,
    // 3 subtract, (2+L) add, and two visits:9+2L.35 covers fixed checks/sign.
    let accumulator_bits=bound_call!(mul,m,4,d)?;let l=bound_call!(limbs,m,accumulator_bits)?;
    let per=bound_call!(add,m,9,bound_call!(mul,m,2,l)?)?;
    let variable=bound_call!(mul,m,per,d)?;let s=bound_call!(add,m,35,variable)?;
    let bits=bound_call!(mul,m,4,significant)?;Ok((s,bound_call!(maximum,m,bits,1)?))
}
fn decimal(m:&mut CoreMeter,raw:&str)->Result<(usize,RatioBits)>{
    let(d,significant)=bound_call!(digits,m,raw)?;
    m.scan_bytes(raw.len())?;
    let fraction=raw.split_once('.').map(|(_,v)|v.len()).unwrap_or(0);
    if w::compare(m,fraction,18usize)?.is_gt(){return Err(shape::fail("original decimal scale dimension"));}
    // decimal(): multiply/subtract/add/zero-compare/coefficient-increment and
    // two visits cost at most14+3L per digit. The coefficient counter has one
    // limb. The existing <=18-place decimal scale has at most two limbs even
    // under the coarse4f+1 bound, so scale visit/multiply costs at most5.
    //80 covers all fixed checks, final comparisons and optional negation.
    let accumulator_bits=bound_call!(mul,m,4,d)?;let l=bound_call!(limbs,m,accumulator_bits)?;
    let per=bound_call!(add,m,14,bound_call!(mul,m,3,l)?)?;
    let digit_work=bound_call!(mul,m,per,d)?;let scale_work=bound_call!(mul,m,5,fraction)?;
    let s=bound_call!(add,m,80,digit_work)?;let s=bound_call!(add,m,s,scale_work)?;
    let n=bound_call!(maximum,m,bound_call!(mul,m,4,significant)?,1)?;
    let d=bound_call!(add,m,bound_call!(mul,m,4,fraction)?,1)?;
    Ok((s,RatioBits{n,d}))
}
fn costs(m:&mut CoreMeter,r:RatioBits)->Result<(usize,usize)>{
    let bits=bound_call!(maximum,m,r.n,r.d)?;
    if w::compare(m,bits,512usize)?.is_gt(){return Err(crate::Error::new("UNSUPPORTED_SCHEMA_PROFILE","original future arithmetic bound exceeds the existing operand domain"));}
    let limbs=bound_call!(limbs,m,bits)?;
    let twice=bound_call!(mul,m,2,limbs)?;Ok((bound_call!(add,m,1,twice)?,bound_call!(add,m,1,limbs)?))
}
fn rational(m:&mut CoreMeter,r:RatioBits)->Result<usize>{
    let(binary,unary)=bound_call!(costs,m,r)?;
    // Euclid's first remainder is below the positive denominator. Every two
    // subsequent divisions at least halve it, hence <=2*bits(d)+2 iterations,
    // including the first and terminal cases. Each iteration pays condition,
    // divmod, and its zero-divisor comparison. Eight further binary charges
    // cover initial/final comparisons and the two final divmods/remainders.
    let loops=bound_call!(add,m,bound_call!(mul,m,2,r.d)?,2)?;
    let each=bound_call!(mul,m,3,binary)?;let loop_work=bound_call!(mul,m,loops,each)?;
    let fixed=bound_call!(add,m,bound_call!(mul,m,8,binary)?,unary)?;bound_call!(add,m,fixed,loop_work)
}
fn product(m:&mut CoreMeter,a:RatioBits,b:RatioBits)->Result<(usize,RatioBits)>{
    let r=RatioBits{n:bound_call!(add,m,a.n,b.n)?,d:bound_call!(add,m,a.d,b.d)?};
    let(c,_)=bound_call!(costs,m,r)?;let arithmetic=bound_call!(mul,m,2,c)?;
    Ok((bound_call!(add,m,arithmetic,bound_call!(rational,m,r)?)?,r))
}
fn rounded(m:&mut CoreMeter,r:RatioBits)->Result<usize>{
    let _=bound_call!(costs,m,r)?;
    // A rounding carry can cross a limb boundary even when the incoming
    // numerator/denominator did not. Use the existing512-bit per-operation
    // maximum here, including overflow refusal, rather than incoming limbs.
    Ok(8*17+3*9)
}
fn parse_field<'a>(m:&mut CoreMeter,value:&'a Value,name:&str)->Result<&'a str>{shape::text(m,value,name)}
fn customer_cost(m:&mut CoreMeter,f:&Value)->Result<usize>{
    let(n_parse,n)=bound_call!(integer,m,bound_call!(parse_field,m,&f["multiplier"],"numerator")?)?;
    let(d_parse,d)=bound_call!(integer,m,bound_call!(parse_field,m,&f["multiplier"],"denominator")?)?;
    let ratio=RatioBits{n,d};let(c,_)=bound_call!(costs,m,ratio)?;
    let mut s=bound_call!(add,m,n_parse,d_parse)?;let checks=bound_call!(mul,m,2,c)?;s=bound_call!(add,m,s,checks)?;
    s=bound_call!(add,m,s,bound_call!(rational,m,ratio)?)?;
    // The eligible attributable amount is in the existing30-digit atom
    // domain; no supplier invocation or AP consumption is assumed here.
    let(amount_work,amount)=bound_call!(product,m,RatioBits{n:120,d:1},ratio)?;s=bound_call!(add,m,s,amount_work)?;
    let(markup_parse,markup)=bound_call!(decimal,m,bound_call!(parse_field,m,f,"markup")?)?;s=bound_call!(add,m,s,markup_parse)?;
    let(c,_)=bound_call!(costs,m,markup)?;s=bound_call!(add,m,s,c)?;
    let(markup_work,markup)=bound_call!(product,m,markup,RatioBits{n:73,d:1})?;s=bound_call!(add,m,s,markup_work)?;
    // add_ratio(): three cross-products and one addition, then reduction.
    let left=bound_call!(add,m,amount.n,markup.d)?;let right=bound_call!(add,m,markup.n,amount.d)?;
    let n=bound_call!(add,m,bound_call!(maximum,m,left,right)?,1)?;let d=bound_call!(add,m,amount.d,markup.d)?;
    let sum=RatioBits{n,d};let(c,_)=bound_call!(costs,m,sum)?;
    let operations=bound_call!(mul,m,4,c)?;s=bound_call!(add,m,s,operations)?;
    s=bound_call!(add,m,s,bound_call!(rational,m,sum)?)?;s=bound_call!(add,m,s,bound_call!(rounded,m,sum)?)?;
    // Actual ceiling parse and rounded-vs-ceiling comparison remain executed.
    let(ceiling,_)=bound_call!(integer,m,bound_call!(parse_field,m,&f["ceiling"],"atoms")?)?;
    // Three original multiplier/markup text reads, and17 callbacks for the
    // actual post-round ceiling currency/scale/atom-field predicates.
    s=bound_call!(add,m,s,ceiling)?;bound_call!(add,m,s,17+20)
}

pub(super) fn derive(m:&mut CoreMeter,plan:&Value,activity:&Value)->Result<PricingKernels>{
    let instructions=shape::array(m,plan,"typed_instructions")?;
    let components=shape::array(m,plan,"component_slots")?;
    // SAME work pricing parses scale and atom limit once per invocation.
    let(scale,_)=bound_call!(integer,m,"1000000000000000000")?;
    let(limit,_)=bound_call!(integer,m,"999999999999999999999999999999")?;
    let mut work=bound_call!(add,m,scale,limit)?;let mut maximum_app=0;let mut maximum_family=0;
    let mut work_rules=0;let mut family_instructions=0;let mut maximum_codes=0;
    for instruction in instructions{
        m.visit("capacity.original_pricing_kernel")?;
        let opcode=shape::text(m,instruction,"opcode")?;
        if shape::same_text(m,opcode,"evaluate_family")?{
            family_instructions=bound_call!(add,m,family_instructions,1)?;
            let family=&instruction["family"];
            if !shape::same(m,&family["selector"]["target_activity"],activity)?{continue;}
            let codes=shape::array(m,family,"codes")?;
            maximum_codes=bound_call!(maximum,m,maximum_codes,codes.len())?;
            // A none alternative still parses the atom limit and executes
            // rounding. Explicit-none replacement uses less arithmetic.
            let none=bound_call!(add,m,limit,bound_call!(rounded,m,RatioBits{n:1,d:1})?)?;
            maximum_family=bound_call!(maximum,m,maximum_family,bound_call!(add,m,none,17)?)?;
            for code in codes{
                m.visit("capacity.original_family_arithmetic_alternative")?;
                let effect=&code["effect"];let kind=shape::text(m,effect,"kind")?;
                if shape::same_text(m,kind,"none")?{continue;}
                let raw=shape::text(m,&effect["amount"],"percent")?;
                let(parse,p)=bound_call!(decimal,m,raw)?;
                let p=RatioBits{n:p.n,d:bound_call!(add,m,p.d,7)?};let(c,u)=bound_call!(costs,m,p)?;
                let mut s=bound_call!(add,m,parse,c)?;s=bound_call!(add,m,s,u)?;s=bound_call!(add,m,s,bound_call!(rational,m,p)?)?;
                // A frozen component net sums at most128 original actions,
                // each in the30-digit atom domain: <128*2^120. It need not
                // itself have the same digit count as an individual action.
                let(product_work,r)=bound_call!(product,m,RatioBits{n:128,d:1},p)?;
                s=bound_call!(add,m,s,product_work)?;s=bound_call!(add,m,s,bound_call!(rounded,m,r)?)?;
                s=bound_call!(add,m,s,limit)?;
                let(discount,_)=bound_call!(integer,m,bound_call!(parse_field,m,&family["maximum_discount"],"atoms")?)?;
                let(premium,_)=bound_call!(integer,m,bound_call!(parse_field,m,&family["maximum_premium"],"atoms")?)?;
                let ceiling=bound_call!(maximum,m,discount,premium)?;s=bound_call!(add,m,s,ceiling)?;
                // Initial net sign, two percent interval comparisons, result
                // sign, magnitude, ceiling and discount-vs-net checks.
                s=bound_call!(add,m,s,7*17+9)?;
                maximum_family=bound_call!(maximum,m,maximum_family,s)?;
            }
            continue;
        }
        if !shape::same_text(m,opcode,"evaluate_rule")?{return Err(shape::fail("original pricing opcode"));}
        let rule=&instruction["rule"];if !shape::same(m,&rule["activity"],activity)?{continue;}
        let f=&rule["formula"];let kind=shape::text(m,f,"kind")?;
        if shape::same_text(m,kind,"app_final_cost")?{
            let kernel=bound_call!(customer_cost,m,f)?;let common=bound_call!(add,m,scale,limit)?;
            let kernel=bound_call!(add,m,kernel,common)?;maximum_app=bound_call!(maximum,m,maximum_app,kernel)?;continue;
        }
        let kernel=if shape::same_text(m,kind,"fixed")?{
            let(parse,r)=bound_call!(decimal,m,bound_call!(parse_field,m,f,"amount")?)?;
            let(product_work,r)=bound_call!(product,m,r,RatioBits{n:73,d:1})?;
            let s=bound_call!(add,m,parse,product_work)?;bound_call!(add,m,s,bound_call!(rounded,m,r)?)?
        }else if shape::same_text(m,kind,"unit")?{
            // Every valid normalized integer fact has <=64 digits. Commercial
            // quantity ceilings may tighten this term, but cannot widen it.
            let quantity=bound_call!(add,m,35,64*17)?;
            let(parse,rate)=bound_call!(decimal,m,bound_call!(parse_field,m,f,"rate")?)?;
            let(first,r)=bound_call!(product,m,RatioBits{n:256,d:1},rate)?;
            let(second,r)=bound_call!(product,m,r,RatioBits{n:73,d:1})?;
            let s=bound_call!(add,m,quantity,parse)?;let s=bound_call!(add,m,s,first)?;let s=bound_call!(add,m,s,second)?;bound_call!(add,m,s,bound_call!(rounded,m,r)?)?
        }else if shape::same_text(m,kind,"discount")?{
            let(parse,p)=bound_call!(decimal,m,bound_call!(parse_field,m,&f["amount"],"percent")?)?;
            let p=RatioBits{n:p.n,d:bound_call!(add,m,p.d,7)?};let(c,u)=bound_call!(costs,m,p)?;
            let mut s=bound_call!(add,m,parse,c)?;s=bound_call!(add,m,s,u)?;s=bound_call!(add,m,s,bound_call!(rational,m,p)?)?;
            let(product_work,r)=bound_call!(product,m,RatioBits{n:120,d:1},p)?;
            s=bound_call!(add,m,s,product_work)?;bound_call!(add,m,s,bound_call!(rounded,m,r)?)?
        }else{return Err(crate::Error::new("UNSUPPORTED_SCHEMA_PROFILE","non-app future pricing arithmetic is not derived"));};
        work_rules=bound_call!(add,m,work_rules,1)?;
        work=bound_call!(add,m,work,kernel)?;
    }
    // price_instructions_selected: <=10 for each opcode equality,10 for
    // each activity equality and13 for the AppDeferred formula selection.
    // Every ordinary result then performs its actual component-prefix scan,
    // instruction/formula/fact visits, and at most one discount-basis scan of
    // prior results (two four-S string equalities per examined result).
    let walks=bound_call!(mul,m,33,instructions.len())?;
    let component_search=bound_call!(mul,m,10,components.len())?;
    let prior_search=bound_call!(mul,m,8,work_rules)?;
    let per=bound_call!(add,m,3,component_search)?;let per=bound_call!(add,m,per,prior_search)?;
    let selected=bound_call!(mul,m,work_rules,per)?;
    work=bound_call!(add,m,work,walks)?;work=bound_call!(add,m,work,selected)?;
    if w::compare(m,maximum_family,0usize)?.is_gt(){
        // outcome and explicit_none scan ALL original family instructions,
        // not merely this target's matching instruction. The code lookup
        // scans all code names to detect duplicate matches.31 also covers
        // the amount/effect predicates and output-field reads omitted by the
        // arithmetic kernel. Explicit-none's walk is strictly smaller.
        let all=bound_call!(mul,m,5,instructions.len())?;
        let families=bound_call!(mul,m,6,family_instructions)?;
        let codes=bound_call!(mul,m,12,maximum_codes)?;
        let control=bound_call!(add,m,all,families)?;let control=bound_call!(add,m,control,codes)?;
        let control=bound_call!(add,m,control,31)?;
        maximum_family=bound_call!(add,m,maximum_family,control)?;
    }
    if w::compare(m,maximum_app,0usize)?.is_gt(){
        // AppFinal selection adds exact app component selection after the
        // formula predicates. The compiled unique component relation selects
        // one arithmetic kernel; every other instruction is still inspected.
        let walks=bound_call!(mul,m,38,instructions.len())?;
        let component=bound_call!(mul,m,10,components.len())?;
        let control=bound_call!(add,m,walks,component)?;
        // Two producing visits, two plan-array reads, and the app wrapper's
        // additional atom-limit parse and two eligible-amount comparisons.
        let control=bound_call!(add,m,control,4+34)?;
        let control=bound_call!(add,m,control,limit)?;
        maximum_app=bound_call!(add,m,maximum_app,control)?;
    }
    Ok(PricingKernels{work,maximum_family,maximum_app})
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::super::{arithmetic,pricing};
    use num_bigint::BigInt;
    use serde_json::json;

    // Functional regression of the written source inequality. Neither an
    // observed callback total nor any value below is used by the derivation.
    // There is no budget search, raised limit, financial fixture or admission.
    #[test]
    fn prospective_app_arithmetic_bound_covers_zero_rounding_and_large_basis(){
        for f in [
            json!({"multiplier":{"numerator":"1","denominator":"1"},"markup":"0","ceiling":{"atoms":"999999999999999999999999999999"}}),
            json!({"multiplier":{"numerator":"13","denominator":"89"},"markup":"0.125","ceiling":{"atoms":"999999999999999999999999999999"}}),
        ]{
            let mut bound_meter=CoreMeter::new(65536).unwrap();
            let bound=customer_cost(&mut bound_meter,&f).unwrap();
            for raw in ["0","1","999999999999999999999999999999"]{
                let mut actual=CoreMeter::new(65536).unwrap();
                let eligible=arithmetic::Ratio{numerator:raw.parse::<BigInt>().unwrap(),denominator:1.into()};
                let scale=arithmetic::Ratio{numerator:1000000000000000000u64.into(),denominator:1.into()};
                let result=pricing::customer_final_cost_rational(&mut actual,&f,&eligible,&scale).unwrap();
                let limit=arithmetic::integer(&mut actual,"999999999999999999999999999999",false).unwrap();
                arithmetic::rounded(&mut actual,&result,&limit).unwrap();
                assert!(actual.usage().semantic_work<=bound as u64);
            }
        }
    }
    #[test]
    fn prospective_kernel_partition_preserves_none_and_deferred_app(){
        let app=json!({"kind":"app_final_cost","multiplier":{"numerator":"1","denominator":"1"},"markup":"0","ceiling":{"atoms":"999999999999999999999999999999"}});
        let plan=json!({"component_slots":[{"key":"a:service"},{"key":"a:app"}],"typed_instructions":[
            {"opcode":"evaluate_rule","rule":{"activity":"a:work","formula":{"kind":"fixed","amount":"1"}}},
            {"opcode":"evaluate_rule","rule":{"activity":"a:work","formula":app}},
            {"opcode":"evaluate_family","family":{"selector":{"target_activity":"a:work"},"codes":[{"effect":{"kind":"none"}}]}}
        ]});
        let mut m=CoreMeter::new(65536).unwrap();let result=derive(&mut m,&plan,&json!("a:work")).unwrap();
        assert!(result.work>0&&result.maximum_family>0&&result.maximum_app>0);
        // An unrelated activity does not acquire this original work's price
        // branches merely because the source contains their instructions.
        let unrelated=derive(&mut m,&plan,&json!("a:other")).unwrap();
        assert!(result.work>unrelated.work);assert_eq!(unrelated.maximum_family,0);assert_eq!(unrelated.maximum_app,0);
    }
}
