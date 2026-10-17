use super::super::transition_constructor::{CoreMeter, arithmetic, codec, meter};
use super::model::{Effect, Formula, Policy};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::cmp::Ordering;
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
    *s = if trim == s.len() {
        "0".to_owned()
    } else {
        s[trim..].to_owned()
    };
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
    *s = format!(
        "{}{}{}{}",
        if negative && !zero { "-" } else { "" },
        w,
        if f.is_empty() { "" } else { "." },
        f
    );
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
pub(super) fn policy(m: &mut CoreMeter, p: &mut Policy) -> Result<()> {
    counter(m, &mut p.version)?;
    for s in &mut p.binding_slots {
        set(m, &mut s.allowed_activities)?;
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
        set(m,rows)?;
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
        set(m, &mut f.codes)?;
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
    *s = if leading == s.len() {
        "0".to_owned()
    } else {
        s[leading..].to_owned()
    };
    Ok(())
}
