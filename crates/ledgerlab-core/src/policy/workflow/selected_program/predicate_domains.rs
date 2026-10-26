//! Finite representable cells for the existing scalar predicate language.
//! Cells are compiler DATA, never actual evidence or an execution authorization.
use super::{bad, compare, FactSchema, FactValue, Predicate, Truth, TypedOperand};
use crate::canonical::audit_arena::ExternalBufferLease;
use crate::domain::Timestamp;
use crate::policy::workflow::transition_constructor::{arithmetic, shape, CoreMeter};
use crate::{validation_work as w, Result};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, Zero};
use serde_json::Value;
use std::cmp::Ordering;

struct Domain<'a> {
    operand: &'a TypedOperand,
    values: Vec<FactValue>,
    _owner: Option<ExternalBufferLease>,
}
fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| bad("predicate metadata product"))
}
fn cmp(m: &mut CoreMeter, a: &BigInt, b: &BigInt) -> Result<Ordering> {
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Compare, &[a, b])?;
    Ok(a.cmp(b))
}
fn operand_same(m: &mut CoreMeter, a: &TypedOperand, b: &TypedOperand) -> Result<bool> {
    match (a, b) {
        (TypedOperand::Fact { fact: a }, TypedOperand::Fact { fact: b })
        | (
            TypedOperand::BindingFact { binding_fact: a },
            TypedOperand::BindingFact { binding_fact: b },
        ) => shape::same_text(m, a, b),
        _ => Ok(false),
    }
}
fn atoms<'a>(
    m: &mut CoreMeter,
    p: &'a Predicate,
    f: &mut impl FnMut(&'a TypedOperand, &'a Value, &mut CoreMeter) -> Result<()>,
) -> Result<()> {
    m.visit("predicate.domain_ast")?;
    match p {
        Predicate::True | Predicate::False => Ok(()),
        Predicate::Eq { field, value }
        | Predicate::Lt { field, value }
        | Predicate::Le { field, value }
        | Predicate::Gt { field, value }
        | Predicate::Ge { field, value } => f(field, value, m),
        Predicate::In { field, values } => {
            for value in values {
                f(field, value, m)?;
            }
            Ok(())
        }
        Predicate::All { args } | Predicate::Any { args } => {
            for arg in args {
                atoms(m, arg, f)?;
            }
            Ok(())
        }
        Predicate::Not { arg } => atoms(m, arg, f),
    }
}
fn literal_count(m: &mut CoreMeter, p: &[&Predicate]) -> Result<usize> {
    let mut literals = 0usize;
    for condition in p {
        atoms(m, condition, &mut |_, _, m| {
            literals = w::usize_add(m, literals, 1)?;
            Ok(())
        })?;
    }
    Ok(literals)
}
fn add(m: &mut CoreMeter, values: &mut Vec<FactValue>, value: FactValue) -> Result<()> {
    for prior in values.iter() {
        m.visit("predicate.domain_duplicate")?;
        let same = match (prior, &value) {
            (FactValue::Missing, FactValue::Missing) => true,
            (FactValue::Missing, _) | (_, FactValue::Missing) => false,
            _ => compare(m, prior, &value, false)?.is_eq(),
        };
        if same {
            return Ok(());
        }
    }
    if w::compare(m, values.len(), 4096usize)?.is_ge() {
        return Err(bad("predicate cells"));
    }
    values.push(value);
    Ok(())
}
fn scalar_text(m: &mut CoreMeter, value: &BigInt) -> Result<String> {
    // Same bounded exact scalar formatting obligation as financial postings.
    m.scan_bytes(156)?;
    m.write_bytes(156)?;
    let text = value.to_string();
    if w::compare(m, text.len(), 156usize)?.is_gt() {
        return Err(bad("predicate scalar encoding"));
    }
    Ok(text)
}
fn scaled(m: &mut CoreMeter, text: &str, scale: usize) -> Result<BigInt> {
    let ratio = arithmetic::decimal(m, text)?;
    let factor = w::power_ten(m, u8::try_from(scale).map_err(|_| bad("predicate scale"))?)?;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&ratio.numerator, &factor],
    )?;
    let numerator = ratio.numerator * factor;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::DivideRemainder,
        &[&numerator, &ratio.denominator],
    )?;
    let (value, remainder) = numerator.div_rem(&ratio.denominator);
    if !cmp(m, &remainder, &BigInt::zero())?.is_eq() {
        return Err(bad("unrepresentable predicate cut"));
    }
    Ok(value)
}
fn numeric_text(m: &mut CoreMeter, value: &BigInt, scale: usize) -> Result<String> {
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Absolute, &[value])?;
    let magnitude = value.abs();
    let mut text = scalar_text(m, &magnitude)?;
    if w::compare(m, scale, 0usize)?.is_gt() {
        let width = w::usize_add(m, scale, 1)?;
        if w::compare(m, text.len(), width)?.is_lt() {
            let count = w::usize_sub(m, width, text.len())?;
            m.write_bytes(width)?;
            let mut padded = String::new();
            padded
                .try_reserve_exact(width)
                .map_err(|_| bad("predicate scalar allocation"))?;
            for _ in 0..count {
                m.visit("predicate.scalar_zero")?;
                padded.push('0');
            }
            padded.push_str(&text);
            text = padded;
        }
        let point = w::usize_sub(m, text.len(), scale)?;
        let bytes = w::usize_add(m, text.len(), 1)?;
        m.write_bytes(bytes)?;
        text.insert(point, '.');
        while text.ends_with('0') {
            m.scan_bytes(1)?;
            text.pop();
        }
        if text.ends_with('.') {
            m.scan_bytes(1)?;
            text.pop();
        }
    }
    if cmp(m, value, &BigInt::zero())?.is_lt() {
        let bytes = w::usize_add(m, text.len(), 1)?;
        m.write_bytes(bytes)?;
        text.insert(0, '-');
    }
    Ok(text)
}
/// Nearest next representable coefficient. A decimal with scale S is not a
/// continuous interval: after thirty significant coefficient digits, the next
/// permitted lattice point may require trailing zeroes. Never invent a midpoint.
fn neighbor(m: &mut CoreMeter, value: &BigInt, direction: i8) -> Result<BigInt> {
    let step = BigInt::from(direction);
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Add, &[value, &step])?;
    let candidate = value + step;
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Absolute, &[&candidate])?;
    let digits = scalar_text(m, &candidate.abs())?.len();
    if !w::compare(m, digits, 30usize)?.is_gt() {
        return Ok(candidate);
    }
    let excess = w::usize_sub(m, digits, 30)?;
    let grid = w::power_ten(m, u8::try_from(excess).map_err(|_| bad("predicate scale"))?)?;
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::DivideRemainder,
        &[&candidate, &grid],
    )?;
    let (mut q, r) = candidate.div_rem(&grid);
    let sign = cmp(m, &r, &BigInt::zero())?;
    if (direction > 0 && sign.is_gt()) || (direction < 0 && sign.is_lt()) {
        w::ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Add,
            &[&q, &BigInt::from(direction)],
        )?;
        q += direction;
    }
    w::ValidationWork::arithmetic(m, w::ArithmeticOperation::Multiply, &[&q, &grid])?;
    Ok(q * grid)
}
fn domain<'a>(
    m: &mut CoreMeter,
    operand: &'a TypedOperand,
    conditions: &[&Predicate],
    activity: &FactSchema,
    context: &FactSchema,
) -> Result<Domain<'a>> {
    let (schema, name) = match operand {
        TypedOperand::Fact { fact } => (activity, fact.as_str()),
        TypedOperand::BindingFact { binding_fact } => (context, binding_fact.as_str()),
        _ => return Err(bad("operational predicate domain not yet authenticated")),
    };
    let fields = shape::array(m, schema.canonical_value(), "fields")?;
    let field = shape::named(m, fields, "name", name)?;
    let ty = shape::field(m, field, "type")?;
    let kind = shape::text(m, ty, "kind")?;
    let literal_count = literal_count(m, conditions)?;
    let triples = mul(m, literal_count, 3)?;
    let mut count = w::usize_add(m, triples, 8)?;
    if kind == "enum" {
        let entries = shape::array(m, ty, "values")?.len();
        let n = w::usize_add(m, entries, 1)?;
        if w::compare(m, n, count)?.is_gt() {
            count = n;
        }
    }
    let size = mul(m, count, std::mem::size_of::<FactValue>() + 160)?;
    let maximum_string = if kind == "string" {
        shape::text(m, ty, "max_bytes")?
            .parse::<usize>()
            .map_err(|_| bad("string bytes"))?
    } else {
        0
    };
    let mut bytes = w::usize_add(m, size, maximum_string)?;
    for condition in conditions {
        atoms(m, condition, &mut |f, v, m| {
            if operand_same(m, f, operand)? {
                if let Some(text) = v.as_str() {
                    bytes = w::usize_add(m, bytes, text.len())?;
                }
            }
            Ok(())
        })?;
    }
    if kind == "enum" {
        for value in shape::array(m, ty, "values")? {
            let text = value.as_str().ok_or_else(|| bad("enum value"))?;
            bytes = w::usize_add(m, bytes, text.len())?;
        }
    }
    let owner = m
        .representation_arena()
        .map(|a| a.reserve_external_payload(bytes, m))
        .transpose()?;
    m.write_bytes(bytes)?;
    // At most three 512-bit neighbor candidates, reduction/scaling limbs and
    // decimal-format scratch coexist with the retained case vector.
    let _scratch = m
        .representation_arena()
        .map(|a| a.reserve_external_payload(4096, m))
        .transpose()?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| bad("predicate domain allocation"))?;
    if field["required"].as_bool() == Some(false) {
        add(m, &mut values, FactValue::Missing)?;
    }
    let literal_bytes = mul(m, literal_count, std::mem::size_of::<&Value>())?;
    let _literal_owner = m
        .representation_arena()
        .map(|a| a.reserve_external_payload(literal_bytes, m))
        .transpose()?;
    m.write_bytes(literal_bytes)?;
    let mut literals = Vec::new();
    literals
        .try_reserve_exact(literal_count)
        .map_err(|_| bad("predicate cut allocation"))?;
    for p in conditions {
        atoms(m, p, &mut |f, v, m| {
            if operand_same(m, f, operand)? {
                literals.push(v);
            }
            Ok(())
        })?;
    }
    match kind {
        "boolean" => {
            add(m, &mut values, FactValue::Boolean(false))?;
            add(m, &mut values, FactValue::Boolean(true))?;
        }
        "enum" => {
            for value in shape::array(m, ty, "values")? {
                let value = schema.normalize_field_scalar_complete_with_work(name, value, m)?;
                add(m, &mut values, value)?;
            }
        }
        "integer" | "decimal" => {
            let scale = if kind == "integer" {
                0
            } else {
                shape::text(m, ty, "max_scale")?
                    .parse::<usize>()
                    .map_err(|_| bad("decimal scale"))?
            };
            let raw_low = shape::text(m, ty, "min")?;
            let low = scaled(m, raw_low, scale)?;
            let raw_high = shape::text(m, ty, "max")?;
            let high = scaled(m, raw_high, scale)?;
            for cut in [shape::field(m, ty, "min")?, shape::field(m, ty, "max")?]
                .into_iter()
                .chain(literals.iter().copied())
            {
                let raw = cut.as_str().ok_or_else(|| bad("numeric cut"))?;
                let base = scaled(m, raw, scale)?;
                for point in [
                    neighbor(m, &base, -1)?,
                    base.clone(),
                    neighbor(m, &base, 1)?,
                ] {
                    if cmp(m, &point, &low)?.is_lt() || cmp(m, &point, &high)?.is_gt() {
                        continue;
                    }
                    let text = numeric_text(m, &point, scale)?;
                    let value = schema.normalize_field_scalar_complete_with_work(
                        name,
                        &Value::String(text),
                        m,
                    )?;
                    add(m, &mut values, value)?;
                }
            }
        }
        "timestamp" => {
            let raw_low = shape::text(m, ty, "min")?;
            let low = Timestamp::parse_with_work(raw_low, m)?.micros();
            let raw_high = shape::text(m, ty, "max")?;
            let high = Timestamp::parse_with_work(raw_high, m)?.micros();
            for cut in [shape::field(m, ty, "min")?, shape::field(m, ty, "max")?]
                .into_iter()
                .chain(literals.iter().copied())
            {
                let base = Timestamp::parse_with_work(
                    cut.as_str().ok_or_else(|| bad("timestamp cut"))?,
                    m,
                )?
                .micros();
                for offset in [-1i64, 0, 1] {
                    w::ValidationWork::arithmetic(
                        m,
                        w::ArithmeticOperation::Add,
                        &[&base.into(), &offset.into()],
                    )?;
                    let point = base
                        .checked_add(offset)
                        .ok_or_else(|| bad("timestamp cell overflow"))?;
                    if w::compare(m, point, low)?.is_lt() || w::compare(m, point, high)?.is_gt() {
                        continue;
                    }
                    let time = Timestamp::from_micros_with_work(point, m)?;
                    add(m, &mut values, FactValue::Timestamp(time))?;
                }
            }
        }
        "string" => {
            for value in &literals {
                let value = schema.normalize_field_scalar_complete_with_work(name, value, m)?;
                add(m, &mut values, value)?;
            }
            let min = shape::text(m, ty, "min_bytes")?
                .parse::<usize>()
                .map_err(|_| bad("string bytes"))?;
            let mut length = min;
            'search: while !w::compare(m, length, maximum_string)?.is_gt() {
                let attempts = w::usize_add(m, literals.len(), 1)?;
                for index in 0..attempts {
                    let mut n = index;
                    m.write_bytes(length)?;
                    let mut text = String::new();
                    text.try_reserve_exact(length)
                        .map_err(|_| bad("predicate string allocation"))?;
                    for _ in 0..length {
                        w::ValidationWork::arithmetic(
                            m,
                            w::ArithmeticOperation::DivideRemainder,
                            &[&n.into(), &95usize.into()],
                        )?;
                        text.push((b' ' + (n % 95) as u8) as char);
                        n /= 95;
                    }
                    if n != 0 {
                        break;
                    }
                    let candidate = FactValue::String(text);
                    let mut found = false;
                    for prior in &values {
                        if !matches!(prior, FactValue::Missing)
                            && compare(m, prior, &candidate, false)?.is_eq()
                        {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        add(m, &mut values, candidate)?;
                        break 'search;
                    }
                }
                length = w::usize_add(m, length, 1)?;
            }
        }
        _ => {
            return Err(bad(
                "predicate relation domain requires real lifecycle operands",
            ))
        }
    }
    if values.is_empty() {
        return Err(bad("empty predicate domain"));
    }
    Ok(Domain {
        operand,
        values,
        _owner: owner,
    })
}

fn evaluate(
    m: &mut CoreMeter,
    condition: &Predicate,
    domains: &[Domain<'_>],
    cell: &[usize],
    activity: &FactSchema,
    context: &FactSchema,
) -> Result<Truth> {
    fn atom(
        m: &mut CoreMeter,
        field: &TypedOperand,
        literal: &Value,
        ordered: bool,
        domains: &[Domain<'_>],
        cell: &[usize],
        activity: &FactSchema,
        context: &FactSchema,
    ) -> Result<Option<Ordering>> {
        let mut found = None;
        for (index, domain) in domains.iter().enumerate() {
            if operand_same(m, field, domain.operand)? {
                found = Some(index);
                break;
            }
        }
        let index = found.ok_or_else(|| bad("unbound predicate cell"))?;
        let actual = &domains[index].values[cell[index]];
        let (schema, name) = match field {
            TypedOperand::Fact { fact } => (activity, fact.as_str()),
            TypedOperand::BindingFact { binding_fact } => (context, binding_fact.as_str()),
            _ => return Err(bad("operational predicate cell")),
        };
        let expected = schema.normalize_field_scalar_complete_with_work(name, literal, m)?;
        if matches!(actual, FactValue::Missing) {
            Ok(None)
        } else {
            Ok(Some(compare(m, actual, &expected, ordered)?))
        }
    }
    m.visit("predicate.compile_cell_node")?;
    match condition {
        Predicate::True => Ok(Truth::True),
        Predicate::False => Ok(Truth::False),
        Predicate::All { args } | Predicate::Any { args } => {
            let all = matches!(condition, Predicate::All { .. });
            let mut result = if all { Truth::True } else { Truth::False };
            for arg in args {
                let next = evaluate(m, arg, domains, cell, activity, context)?;
                result = if all {
                    result.all(next)
                } else {
                    result.any(next)
                };
            }
            Ok(result)
        }
        Predicate::Not { arg } => Ok(evaluate(m, arg, domains, cell, activity, context)?.not()),
        Predicate::In { field, values } => {
            let mut result = Truth::False;
            for literal in values {
                let next = atom(m, field, literal, false, domains, cell, activity, context)?
                    .map(|o| Truth::of(o.is_eq()))
                    .unwrap_or(Truth::Unknown);
                result = result.any(next);
            }
            Ok(result)
        }
        Predicate::Eq { field, value }
        | Predicate::Lt { field, value }
        | Predicate::Le { field, value }
        | Predicate::Gt { field, value }
        | Predicate::Ge { field, value } => {
            let ordered = !matches!(condition, Predicate::Eq { .. });
            let Some(order) = atom(m, field, value, ordered, domains, cell, activity, context)?
            else {
                return Ok(Truth::Unknown);
            };
            Ok(Truth::of(match condition {
                Predicate::Eq { .. } => order.is_eq(),
                Predicate::Lt { .. } => order.is_lt(),
                Predicate::Le { .. } => order.is_le(),
                Predicate::Gt { .. } => order.is_gt(),
                _ => order.is_ge(),
            }))
        }
    }
}

/// Exhaust the bounded equivalence cells, preserving discrete numeric/time
/// representability and Missing. An applicable consumer requires its producer
/// True; a monetary rule needs at least one reachable True cell.
pub(super) fn prove(
    m: &mut CoreMeter,
    conditions: &[&Predicate],
    implications: &[(usize, usize)],
    activity: &FactSchema,
    context: &FactSchema,
) -> Result<()> {
    if w::compare(m, conditions.len(), 64usize)?.is_gt() {
        return Err(bad("predicate rule count"));
    }
    let size = mul(
        m,
        256,
        std::mem::size_of::<Domain<'_>>() + std::mem::size_of::<usize>(),
    )?;
    let rows = mul(
        m,
        conditions.len(),
        std::mem::size_of::<bool>() + std::mem::size_of::<Truth>(),
    )?;
    let size = w::usize_add(m, size, rows)?;
    let _owner = m
        .representation_arena()
        .map(|a| a.reserve_external_payload(size, m))
        .transpose()?;
    m.write_bytes(size)?;
    let mut domains = Vec::new();
    domains
        .try_reserve_exact(256)
        .map_err(|_| bad("predicate domain allocation"))?;
    let mut cells = 1usize;
    for condition in conditions {
        atoms(m, condition, &mut |field, _, m| {
            for prior in &domains {
                let prior: &Domain<'_> = prior;
                if operand_same(m, field, prior.operand)? {
                    return Ok(());
                }
            }
            if w::compare(m, domains.len(), 256usize)?.is_ge() {
                return Err(bad("predicate operand count"));
            }
            let next = domain(m, field, conditions, activity, context)?;
            cells = mul(m, cells, next.values.len())?;
            if w::compare(m, cells, 4096usize)?.is_gt() {
                return Err(bad("predicate cells"));
            }
            domains.push(next);
            Ok(())
        })?;
    }
    let mut cell = Vec::new();
    cell.try_reserve_exact(domains.len())
        .map_err(|_| bad("predicate cell allocation"))?;
    cell.resize(domains.len(), 0);
    let mut reachable = Vec::new();
    reachable
        .try_reserve_exact(conditions.len())
        .map_err(|_| bad("predicate reachability allocation"))?;
    reachable.resize(conditions.len(), false);
    let mut truth = Vec::new();
    truth
        .try_reserve_exact(conditions.len())
        .map_err(|_| bad("predicate result allocation"))?;
    for _ in 0..cells {
        m.visit("predicate.compile_cell")?;
        truth.clear();
        for (index, condition) in conditions.iter().enumerate() {
            let result = evaluate(m, condition, &domains, &cell, activity, context)?;
            m.write_bytes(std::mem::size_of::<bool>() + std::mem::size_of::<Truth>())?;
            reachable[index] |= result == Truth::True;
            truth.push(result);
        }
        for &(producer, consumer) in implications {
            if w::compare(m, producer, truth.len())?.is_ge()
                || w::compare(m, consumer, truth.len())?.is_ge()
            {
                return Err(bad("predicate dependency index"));
            }
            if truth[consumer] == Truth::True && truth[producer] != Truth::True {
                return Err(bad("dependent predicate does not imply producer"));
            }
        }
        for (index, domain) in domains.iter().enumerate() {
            m.write_bytes(std::mem::size_of::<usize>())?;
            cell[index] = w::usize_add(m, cell[index], 1)?;
            if w::compare(m, cell[index], domain.values.len())?.is_lt() {
                break;
            }
            cell[index] = 0;
        }
    }
    for value in reachable {
        if !value {
            return Err(bad("unreachable monetary rule"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalBytes;
    use serde_json::json;
    fn fact_schema(scale: usize) -> FactSchema {
        let raw = json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"test:domain","version":"1","normalization":"ledger-facts-normalization/1","fields":[
            {"name":"test:q","required":false,"provenance":{"kind":"caller"},"type":{"kind":"integer","min":"0","max":"10"}},
            {"name":"test:d","required":true,"provenance":{"kind":"caller"},"type":{"kind":"decimal","min":"0","max":"0.2","max_scale":scale.to_string()}},
            {"name":"test:flag","required":true,"provenance":{"kind":"caller"},"type":{"kind":"boolean"}},
            {"name":"test:text","required":true,"provenance":{"kind":"caller"},"type":{"kind":"string","min_bytes":"1","max_bytes":"1"}},
            {"name":"test:t","required":true,"provenance":{"kind":"caller"},"type":{"kind":"timestamp","min":"2026-10-25T00:00:00.000000Z","max":"2026-10-25T00:00:00.000001Z"}}
        ]});
        FactSchema::parse(CanonicalBytes::from_value(&raw).unwrap().as_slice()).unwrap()
    }
    fn condition(value: Value) -> Predicate {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn representable_cells_prove_implication_and_refuse_impossible_money() {
        let schema = fact_schema(1);
        let mut m = CoreMeter::for_compile(1_048_576).unwrap();
        let producer = condition(json!({"op":"ge","field":{"fact":"test:q"},"value":"2"}));
        let consumer = condition(json!({"op":"ge","field":{"fact":"test:q"},"value":"3"}));
        prove(&mut m, &[&producer, &consumer], &[(0, 1)], &schema, &schema).unwrap();
        assert!(prove(&mut m, &[&consumer, &producer], &[(0, 1)], &schema, &schema).is_err());
        let impossible = condition(json!({"op":"lt","field":{"fact":"test:q"},"value":"0"}));
        assert!(prove(&mut m, &[&impossible], &[], &schema, &schema).is_err());
        let between = condition(
            json!({"op":"all","args":[{"op":"gt","field":{"fact":"test:d"},"value":"0.1"},{"op":"lt","field":{"fact":"test:d"},"value":"0.2"}]}),
        );
        assert!(prove(&mut m, &[&between], &[], &schema, &schema).is_err());
        let fine = fact_schema(2);
        prove(&mut m, &[&between], &[], &fine, &fine).unwrap();
        let time_between = condition(
            json!({"op":"all","args":[{"op":"gt","field":{"fact":"test:t"},"value":"2026-10-25T00:00:00.000000Z"},{"op":"lt","field":{"fact":"test:t"},"value":"2026-10-25T00:00:00.000001Z"}]}),
        );
        assert!(prove(&mut m, &[&time_between], &[], &schema, &schema).is_err());
        let equality = condition(
            json!({"op":"all","args":[{"op":"eq","field":{"fact":"test:flag"},"value":true},{"op":"not","arg":{"op":"eq","field":{"fact":"test:text"},"value":"x"}}]}),
        );
        prove(&mut m, &[&equality], &[], &schema, &schema).unwrap();
        let mut sparse = schema.canonical_value().clone();
        sparse["fields"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["name"] == "test:d")
            .unwrap()["type"] = json!({"kind":"decimal","min":"99999999999999999999999999999.8","max":"99999999999999999999999999999.9","max_scale":"18"});
        let sparse =
            FactSchema::parse(CanonicalBytes::from_value(&sparse).unwrap().as_slice()).unwrap();
        let sparse_between = condition(
            json!({"op":"all","args":[{"op":"gt","field":{"fact":"test:d"},"value":"99999999999999999999999999999.8"},{"op":"lt","field":{"fact":"test:d"},"value":"99999999999999999999999999999.9"}]}),
        );
        assert!(prove(&mut m, &[&sparse_between], &[], &sparse, &sparse).is_err());
    }
}
