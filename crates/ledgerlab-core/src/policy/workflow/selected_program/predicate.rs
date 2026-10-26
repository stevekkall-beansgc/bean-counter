//! The existing closed Predicate AST's typed three-valued meaning. No ambient
//! context, graph discovery or source authority is obtained by this evaluator.
use super::super::super::{facts::FactValue, FactSchema};
use super::super::model::{Operand as TypedOperand, Predicate};
use crate::policy::workflow::transition_constructor::{shape, CoreMeter};
use crate::{validation_work as w, Error, Result};
use serde_json::Value;
use std::cmp::Ordering;
#[path = "predicate_domains.rs"]
mod domains;

pub(super) fn prove_with_funding(
    m: &mut CoreMeter,
    conditions: &[&Predicate],
    implications: &[(usize, usize)],
    activity: &FactSchema,
    context: &FactSchema,
    legs: &[&str],
    quantities: Option<&[Option<&str>]>,
) -> Result<()> {
    domains::prove_scoped(
        m,
        conditions,
        implications,
        activity,
        context,
        Some(legs),
        quantities,
    )
}

/// Typechecking and scalar normalization of the already defined AST. The caller
/// shares `nodes` across the whole policy; no rule-local reset can hide its
/// aggregate AST cost. Source/funding/status require their real operand owner
/// before their separately defined lifecycle semantics may be admitted.
#[cfg(test)]
pub(super) fn normalize(
    m: &mut CoreMeter,
    condition: &mut Predicate,
    activity_schema: &FactSchema,
    context_schema: &FactSchema,
    nodes: &mut usize,
) -> Result<()> {
    normalize_with_operational(m, condition, activity_schema, context_schema, nodes, false)
}
#[derive(Clone, Copy)]
enum ContextSupport {
    None,
    Original,
    Work,
}
impl ContextSupport {
    fn allows(self, name: &str) -> bool {
        match self {
            Self::None => false,
            Self::Original => matches!(name, "source" | "funding_mode"),
            Self::Work => matches!(name, "source" | "funding_mode" | "status"),
        }
    }
}
pub(super) fn normalize_with_operational(
    m: &mut CoreMeter,
    condition: &mut Predicate,
    activity_schema: &FactSchema,
    context_schema: &FactSchema,
    nodes: &mut usize,
    operational: bool,
) -> Result<()> {
    normalize_with_context(
        m,
        condition,
        activity_schema,
        context_schema,
        nodes,
        if operational {
            ContextSupport::Original
        } else {
            ContextSupport::None
        },
    )
}
pub(super) fn normalize_work_lifecycle(
    m: &mut CoreMeter,
    condition: &mut Predicate,
    activity_schema: &FactSchema,
    context_schema: &FactSchema,
    nodes: &mut usize,
) -> Result<()> {
    normalize_with_context(
        m,
        condition,
        activity_schema,
        context_schema,
        nodes,
        ContextSupport::Work,
    )
}
fn normalize_with_context(
    m: &mut CoreMeter,
    condition: &mut Predicate,
    activity_schema: &FactSchema,
    context_schema: &FactSchema,
    nodes: &mut usize,
    operational: ContextSupport,
) -> Result<()> {
    fn field<'a>(
        m: &mut CoreMeter,
        operand: &'a TypedOperand,
        activity: &'a FactSchema,
        context: &'a FactSchema,
    ) -> Result<(&'a FactSchema, &'a str)> {
        let (schema, name) = match operand {
            TypedOperand::Fact { fact } => (activity, fact.as_str()),
            TypedOperand::BindingFact { binding_fact } => (context, binding_fact.as_str()),
            TypedOperand::Context { .. } => {
                return Err(bad("operational predicate context not yet authenticated"))
            }
        };
        let fields = shape::array(m, schema.canonical_value(), "fields")?;
        shape::named(m, fields, "name", name)?;
        Ok((schema, name))
    }
    fn literal(
        m: &mut CoreMeter,
        operand: &TypedOperand,
        value: &mut Value,
        ordered: bool,
        activity: &FactSchema,
        context: &FactSchema,
        operational: ContextSupport,
    ) -> Result<()> {
        if let TypedOperand::Context { context } = operand {
            if !operational.allows(context) {
                return Err(bad("original predicate context capability"));
            }
            operational_literal(m, context, value, ordered)?;
            return Ok(());
        }
        let (schema, name) = field(m, operand, activity, context)?;
        let fields = shape::array(m, schema.canonical_value(), "fields")?;
        let definition = shape::named(m, fields, "name", name)?;
        let kind = shape::text(m, &definition["type"], "kind")?;
        if ordered && !matches!(kind, "integer" | "decimal" | "timestamp") {
            return Err(bad(
                "ordered predicate requires numeric or timestamp domain",
            ));
        }
        *value = schema
            .normalize_field_scalar_complete_with_work(name, value, m)?
            .canonical_value_complete_with_work(m)?
            .ok_or_else(|| bad("predicate literal cannot be Missing"))?;
        Ok(())
    }
    fn visit(
        m: &mut CoreMeter,
        condition: &mut Predicate,
        activity: &FactSchema,
        context: &FactSchema,
        nodes: &mut usize,
        depth: usize,
        operational: ContextSupport,
    ) -> Result<()> {
        m.visit("predicate.compile_node")?;
        *nodes = w::usize_add(m, *nodes, 1)?;
        if w::compare(m, *nodes, 256usize)?.is_gt() || w::compare(m, depth, 8usize)?.is_gt() {
            return Err(bad("predicate AST bounds"));
        }
        match condition {
            Predicate::True | Predicate::False => Ok(()),
            Predicate::Eq { field, value } => {
                literal(m, field, value, false, activity, context, operational)
            }
            Predicate::Lt { field, value }
            | Predicate::Le { field, value }
            | Predicate::Gt { field, value }
            | Predicate::Ge { field, value } => {
                literal(m, field, value, true, activity, context, operational)
            }
            Predicate::In { field, values } => {
                if w::compare(m, values.len(), 1usize)?.is_lt()
                    || w::compare(m, values.len(), 32usize)?.is_gt()
                {
                    return Err(bad("predicate set bounds"));
                }
                for value in values.iter_mut() {
                    literal(m, field, value, false, activity, context, operational)?;
                }
                if m.representation_arena().is_some() {
                    super::super::normalize::owning_set(m, values, |a, m, value| {
                        a.clone_value(value, m)
                    })?;
                } else {
                    super::super::normalize::set(m, values)?;
                }
                Ok(())
            }
            Predicate::All { args } | Predicate::Any { args } => {
                if w::compare(m, args.len(), 1usize)?.is_lt()
                    || w::compare(m, args.len(), 8usize)?.is_gt()
                {
                    return Err(bad("predicate arity"));
                }
                for child in args {
                    let next = w::usize_add(m, depth, 1)?;
                    visit(m, child, activity, context, nodes, next, operational)?;
                }
                Ok(())
            }
            Predicate::Not { arg } => {
                let next = w::usize_add(m, depth, 1)?;
                visit(m, arg, activity, context, nodes, next, operational)
            }
        }
    }
    visit(
        m,
        condition,
        activity_schema,
        context_schema,
        nodes,
        1,
        operational,
    )
}

pub(super) const FUNDING_MODES: [&str; 4] = [
    "customer_api",
    "customer_subscription",
    "customer_local",
    "platform",
];
pub(super) fn operational_literal<'a>(
    m: &mut CoreMeter,
    name: &str,
    value: &'a Value,
    ordered: bool,
) -> Result<&'a str> {
    if ordered {
        return Err(bad("operational context is an equality domain"));
    }
    let value = value
        .as_str()
        .ok_or_else(|| bad("operational context literal string"))?;
    m.scan_bytes(value.len())?;
    match name {
        "source" => {
            if w::compare(m, value.len(), 1usize)?.is_lt()
                || w::compare(m, value.len(), 256usize)?.is_gt()
            {
                return Err(bad("source predicate literal bound"));
            }
        }
        "funding_mode" => {
            let mut found = false;
            for mode in FUNDING_MODES {
                found |= shape::same_text(m, value, mode)?;
            }
            if !found {
                return Err(bad("funding predicate literal mode"));
            }
        }
        "status" => {
            if !matches!(value, "succeeded" | "failed") {
                return Err(bad("work predicate status literal"));
            }
        }
        _ => return Err(bad("operational predicate context not yet authenticated")),
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Truth {
    False,
    True,
    Unknown,
}
impl Truth {
    pub(super) fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
    pub(super) fn all(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }
    pub(super) fn any(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::False => "false",
            Self::True => "true",
            Self::Unknown => "unknown",
        }
    }
    fn of(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}
pub(super) struct Inputs<'a> {
    pub activity_schema: Option<&'a FactSchema>,
    pub context_schema: &'a FactSchema,
    pub facts: &'a Value,
    pub accepted_context: &'a Value,
    /// Authenticated subject source and exact original evaluated-leg mode.
    pub operation_context: Option<(&'a str, &'a str)>,
    /// Immutable actual original Work status; no current-corrector substitute.
    pub work_status: Option<&'a str>,
}
fn bad(detail: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", detail)
}
struct Operand<'a> {
    schema: &'a FactSchema,
    name: &'a str,
    value: Option<&'a Value>,
    definition: &'a Value,
}
fn operand<'a>(m: &mut CoreMeter, field: &'a Value, inputs: &'a Inputs<'a>) -> Result<Operand<'a>> {
    let object = field
        .as_object()
        .ok_or_else(|| bad("predicate operand dictionary"))?;
    if !w::compare(m, object.len(), 1usize)?.is_eq() {
        return Err(bad("one predicate operand"));
    }
    let (schema, name, values) = if let Some(name) = field.get("fact") {
        (
            inputs
                .activity_schema
                .ok_or_else(|| bad("predicate activity schema"))?,
            name,
            inputs.facts,
        )
    } else if let Some(name) = field.get("binding_fact") {
        (
            inputs.context_schema,
            name,
            shape::field(m, inputs.accepted_context, "facts")?,
        )
    } else {
        // These operands remain gated until their exact trusted identities are
        // approved and supplied by the real operation/lifecycle consumers.
        return Err(bad("operational predicate context not yet authenticated"));
    };
    let name = name.as_str().ok_or_else(|| bad("predicate field name"))?;
    let fields = shape::array(m, schema.canonical_value(), "fields")?;
    let definition = shape::named(m, fields, "name", name)?;
    let values = values
        .as_object()
        .ok_or_else(|| bad("predicate facts dictionary"))?;
    m.visit("predicate.operand_presence")?;
    let value = values.get(name);
    if value.is_none() && definition["required"].as_bool() != Some(false) {
        return Err(Error::new("FACT_REQUIRED", "required predicate field"));
    }
    Ok(Operand {
        schema,
        name,
        value,
        definition,
    })
}
fn compare(m: &mut CoreMeter, a: &FactValue, b: &FactValue, ordered: bool) -> Result<Ordering> {
    m.visit("predicate.typed_compare")?;
    match (a, b) {
        (FactValue::Integer(a), FactValue::Integer(b))
        | (FactValue::Decimal(a), FactValue::Decimal(b)) => a.compare_with_work(b, m),
        (FactValue::Timestamp(a), FactValue::Timestamp(b)) => w::compare(m, a.micros(), b.micros()),
        (FactValue::Boolean(a), FactValue::Boolean(b)) if !ordered => Ok(if a == b {
            Ordering::Equal
        } else {
            Ordering::Less
        }),
        (FactValue::String(a), FactValue::String(b)) | (FactValue::Enum(a), FactValue::Enum(b))
            if !ordered =>
        {
            let same = shape::same_text(m, a, b)?;
            Ok(if same {
                Ordering::Equal
            } else {
                Ordering::Less
            })
        }
        (FactValue::Reference(a), FactValue::Reference(b)) if !ordered => {
            let same = shape::same_text(m, a.kind(), b.kind())?
                && shape::same_text(m, a.id(), b.id())?
                && shape::same_text(m, a.digest(), b.digest())?;
            Ok(if same {
                Ordering::Equal
            } else {
                Ordering::Less
            })
        }
        _ => Err(bad("predicate type/order mismatch")),
    }
}
fn checked_literal(
    m: &mut CoreMeter,
    operand: &Operand<'_>,
    value: &Value,
    ordered: bool,
) -> Result<FactValue> {
    let kind = shape::text(m, &operand.definition["type"], "kind")?;
    if ordered && !matches!(kind, "integer" | "decimal" | "timestamp") {
        return Err(bad(
            "ordered predicate requires numeric or timestamp domain",
        ));
    }
    operand
        .schema
        .normalize_field_scalar_complete_with_work(operand.name, value, m)
}
pub(super) fn evaluate(m: &mut CoreMeter, condition: &Value, inputs: &Inputs<'_>) -> Result<Truth> {
    evaluate_inner(m, condition, inputs, 1)
}
fn evaluate_inner(
    m: &mut CoreMeter,
    condition: &Value,
    inputs: &Inputs<'_>,
    depth: usize,
) -> Result<Truth> {
    m.visit("predicate.node")?;
    if w::compare(m, depth, 8usize)?.is_gt() {
        return Err(bad("predicate depth"));
    }
    let op = shape::text(m, condition, "op")?;
    match op {
        "true" | "false" => {
            shape::closed(m, condition, &["op"], &[])?;
            Ok(Truth::of(op == "true"))
        }
        "all" | "any" => {
            shape::closed(m, condition, &["op", "args"], &[])?;
            let args = shape::array(m, condition, "args")?;
            if w::compare(m, args.len(), 1usize)?.is_lt()
                || w::compare(m, args.len(), 8usize)?.is_gt()
            {
                return Err(bad("predicate arity"));
            }
            let mut result = if op == "all" {
                Truth::True
            } else {
                Truth::False
            };
            for arg in args {
                let next = w::usize_add(m, depth, 1)?;
                let child = evaluate_inner(m, arg, inputs, next)?;
                result = if op == "all" {
                    result.all(child)
                } else {
                    result.any(child)
                };
            }
            Ok(result)
        }
        "not" => {
            shape::closed(m, condition, &["op", "arg"], &[])?;
            let next = w::usize_add(m, depth, 1)?;
            Ok(evaluate_inner(m, &condition["arg"], inputs, next)?.not())
        }
        "eq" | "lt" | "le" | "gt" | "ge" | "in" => {
            shape::closed(
                m,
                condition,
                if op == "in" {
                    &["op", "field", "values"]
                } else {
                    &["op", "field", "value"]
                },
                &[],
            )?;
            let field = shape::field(m, condition, "field")?;
            let ordered = !matches!(op, "eq" | "in");
            if let Some(name) = field.get("context") {
                shape::closed(m, field, &["context"], &[])?;
                let name = name
                    .as_str()
                    .ok_or_else(|| bad("operational context name"))?;
                let actual = match name {
                    "source" => {
                        inputs
                            .operation_context
                            .ok_or_else(|| bad("operational context owner missing"))?
                            .0
                    }
                    "funding_mode" => {
                        inputs
                            .operation_context
                            .ok_or_else(|| bad("operational context owner missing"))?
                            .1
                    }
                    "status" => inputs
                        .work_status
                        .ok_or_else(|| bad("original Work status missing"))?,
                    _ => return Err(bad("operational predicate context not yet authenticated")),
                };
                if op == "in" {
                    let values = shape::array(m, condition, "values")?;
                    if w::compare(m, values.len(), 1usize)?.is_lt()
                        || w::compare(m, values.len(), 32usize)?.is_gt()
                    {
                        return Err(bad("predicate set size"));
                    }
                    let mut found = false;
                    for value in values {
                        let expected = operational_literal(m, name, value, false)?;
                        found |= shape::same_text(m, actual, expected)?;
                    }
                    return Ok(Truth::of(found));
                }
                let expected = operational_literal(m, name, &condition["value"], ordered)?;
                return Ok(Truth::of(shape::same_text(m, actual, expected)?));
            }
            let operand = operand(m, field, inputs)?;
            let actual = operand
                .value
                .map(|value| {
                    operand
                        .schema
                        .normalize_field_scalar_complete_with_work(operand.name, value, m)
                })
                .transpose()?;
            if op == "in" {
                let values = shape::array(m, condition, "values")?;
                if w::compare(m, values.len(), 1usize)?.is_lt()
                    || w::compare(m, values.len(), 32usize)?.is_gt()
                {
                    return Err(bad("predicate set size"));
                }
                let mut found = false;
                for value in values {
                    let value = checked_literal(m, &operand, value, false)?;
                    if let Some(actual) = &actual {
                        found |= compare(m, actual, &value, false)?.is_eq();
                    }
                }
                Ok(if actual.is_none() {
                    Truth::Unknown
                } else {
                    Truth::of(found)
                })
            } else {
                let value = checked_literal(m, &operand, &condition["value"], ordered)?;
                let Some(actual) = actual else {
                    return Ok(Truth::Unknown);
                };
                let ordering = compare(m, &actual, &value, ordered)?;
                Ok(Truth::of(match op {
                    "eq" => ordering.is_eq(),
                    "lt" => ordering.is_lt(),
                    "le" => ordering.is_le(),
                    "gt" => ordering.is_gt(),
                    _ => ordering.is_ge(),
                }))
            }
        }
        _ => Err(bad("predicate operator")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalBytes;
    use serde_json::json;
    #[test]
    fn closed_three_valued_algebra_matches_independent_finite_table() {
        use Truth::{False as F, True as T, Unknown as U};
        for (a, b, all, any) in [
            (T, T, T, T),
            (T, F, F, T),
            (T, U, U, T),
            (F, T, F, T),
            (F, F, F, F),
            (F, U, F, U),
            (U, T, U, T),
            (U, F, F, U),
            (U, U, U, U),
        ] {
            assert_eq!(a.all(b), all);
            assert_eq!(a.any(b), any);
        }
        for (a, not) in [(T, F), (F, T), (U, U)] {
            assert_eq!(a.not(), not);
        }
    }
    fn schemas() -> (FactSchema, FactSchema) {
        let field = |name: &str, kind: Value| json!({"name":name,"required":false,"provenance":{"kind":"caller"},"type":kind});
        let activity = json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"test:activity","version":"1","normalization":"ledger-facts-normalization/1","fields":[
            field("test:n",json!({"kind":"integer","min":"0","max":"10"})),
            field("test:d",json!({"kind":"decimal","min":"0","max":"2","max_scale":"2"})),
            field("test:at",json!({"kind":"timestamp","min":"2026-10-25T00:00:00.000000Z","max":"2026-10-25T00:00:00.000001Z"})),
            field("test:flag",json!({"kind":"boolean"})),
            field("test:text",json!({"kind":"string","min_bytes":"0","max_bytes":"8"}))]});
        let context = json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"test:context","version":"1","normalization":"ledger-facts-normalization/1","fields":[{"name":"test:eligible","required":true,"provenance":{"kind":"caller"},"type":{"kind":"enum","values":["test:no","test:yes"]}}]});
        (
            FactSchema::parse(CanonicalBytes::from_value(&activity).unwrap().as_slice()).unwrap(),
            FactSchema::parse(CanonicalBytes::from_value(&context).unwrap().as_slice()).unwrap(),
        )
    }
    #[test]
    fn typed_values_missing_and_actual_context_wrapper_have_one_predicate_meaning() {
        let (activity, context) = schemas();
        let mut ast: Predicate = serde_json::from_value(json!({"op":"all","args":[
            {"op":"eq","field":{"fact":"test:d"},"value":"1.20"},
            {"op":"not","arg":{"op":"false"}}]}))
        .unwrap();
        let mut compiler_meter = CoreMeter::new(65_536).unwrap();
        let mut nodes = 0;
        normalize(
            &mut compiler_meter,
            &mut ast,
            &activity,
            &context,
            &mut nodes,
        )
        .unwrap();
        assert_eq!(nodes, 4);
        assert_eq!(
            serde_json::to_value(&ast).unwrap()["args"][0]["value"],
            "1.2"
        );
        let mut exhausted_nodes = 253;
        assert!(normalize(
            &mut compiler_meter,
            &mut ast,
            &activity,
            &context,
            &mut exhausted_nodes
        )
        .is_err());
        let mut unordered: Predicate =
            serde_json::from_value(json!({"op":"lt","field":{"fact":"test:flag"},"value":true}))
                .unwrap();
        assert!(normalize(
            &mut compiler_meter,
            &mut unordered,
            &activity,
            &context,
            &mut 0
        )
        .is_err());
        let mut duplicate: Predicate = serde_json::from_value(
            json!({"op":"in","field":{"fact":"test:d"},"values":["1.20","1.2"]}),
        )
        .unwrap();
        assert_eq!(
            normalize(
                &mut compiler_meter,
                &mut duplicate,
                &activity,
                &context,
                &mut 0
            )
            .unwrap_err()
            .code,
            "INVALID_SHAPE"
        );
        let digest = context.content_digest().unwrap();
        let accepted = json!({"fact_schema_ref":{"kind":"workflow-fact-schema","id":format!("wdoc_{}",&digest[7..]),"digest":digest},"facts":{"test:eligible":"test:yes"}});
        let facts = json!({"test:n":"2","test:d":"1.20","test:at":"2026-10-25T00:00:00.000000Z","test:flag":true,"test:text":"ok"});
        let inputs = Inputs {
            activity_schema: Some(&activity),
            context_schema: &context,
            facts: &facts,
            accepted_context: &accepted,
            operation_context: None,
            work_status: None,
        };
        let mut m = CoreMeter::new(65_536).unwrap();
        for condition in [
            json!({"op":"lt","field":{"fact":"test:n"},"value":"10"}),
            json!({"op":"eq","field":{"fact":"test:d"},"value":"1.2"}),
            json!({"op":"lt","field":{"fact":"test:at"},"value":"2026-10-25T00:00:00.000001Z"}),
            json!({"op":"eq","field":{"fact":"test:flag"},"value":true}),
            json!({"op":"in","field":{"fact":"test:text"},"values":["other","ok"]}),
            json!({"op":"eq","field":{"binding_fact":"test:eligible"},"value":"test:yes"}),
        ] {
            assert_eq!(evaluate(&mut m, &condition, &inputs).unwrap(), Truth::True);
        }
        let absent = json!({});
        let missing = Inputs {
            activity_schema: Some(&activity),
            context_schema: &context,
            facts: &absent,
            accepted_context: &accepted,
            operation_context: None,
            work_status: None,
        };
        let condition = json!({"op":"gt","field":{"fact":"test:n"},"value":"0"});
        assert_eq!(
            evaluate(&mut m, &condition, &missing).unwrap(),
            Truth::Unknown
        );
        assert_eq!(
            evaluate(&mut m, &json!({"op":"not","arg":condition}), &missing).unwrap(),
            Truth::Unknown
        );
        assert_eq!(
            evaluate(
                &mut m,
                &json!({"op":"all","args":[{"op":"false"},condition]}),
                &missing
            )
            .unwrap(),
            Truth::False
        );
        assert_eq!(
            evaluate(
                &mut m,
                &json!({"op":"any","args":[{"op":"true"},condition]}),
                &missing
            )
            .unwrap(),
            Truth::True
        );
        assert!(evaluate(
            &mut m,
            &json!({"op":"lt","field":{"fact":"test:flag"},"value":true}),
            &inputs
        )
        .is_err());
        assert!(evaluate(
            &mut m,
            &json!({"op":"eq","field":{"fact":"test:n"},"value":"1.0"}),
            &inputs
        )
        .is_err());
        assert!(evaluate(
            &mut m,
            &json!({"op":"eq","field":{"fact":"test:d"},"value":"1.200"}),
            &inputs
        )
        .is_err());
        let missing_context = json!({"fact_schema_ref":accepted["fact_schema_ref"],"facts":{}});
        let bad = Inputs {
            activity_schema: Some(&activity),
            context_schema: &context,
            facts: &facts,
            accepted_context: &missing_context,
            operation_context: None,
            work_status: None,
        };
        assert_eq!(
            evaluate(
                &mut m,
                &json!({"op":"eq","field":{"binding_fact":"test:eligible"},"value":"test:yes"}),
                &bad
            )
            .unwrap_err()
            .code,
            "FACT_REQUIRED"
        );
    }
}
