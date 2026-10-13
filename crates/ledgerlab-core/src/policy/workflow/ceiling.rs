//! Immutable source constraints. Parsing or comparing these values does not
//! authenticate a source, establish enrollment, or grant an operation right.
use super::facts::{name, reject_null};
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Selector {
    Names(Names),
    Namespace(Namespace),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Names {
    names: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Namespace {
    namespace: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ceiling {
    principal_id: String,
    source: String,
    canonical_economic_namespace: String,
    customer_scope: Vec<String>,
    operations: Vec<String>,
    product_selectors: Vec<Selector>,
    activity_selectors: Vec<Selector>,
    link_selectors: Vec<Selector>,
    source_slots: Vec<String>,
    permissions: Vec<String>,
    starts_at: Timestamp,
    ends_at: Timestamp,
}
/// Pure normalized original/current constraint data. A coordinator must supply
/// authenticated retained enrollment and current state separately.
#[derive(Clone, Debug)]
pub struct SourceCeiling {
    body: Ceiling,
    bytes: CanonicalBytes,
}
impl SourceCeiling {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let mut body: Ceiling = canonical::dto(value)?;
        id(&body.principal_id, 128)?;
        id(&body.source, 256)?;
        id(&body.canonical_economic_namespace, 256)?;
        strings(&mut body.customer_scope, 1, 64, |s| id(s, 128))?;
        // These are immutable declared rights, including future operations;
        // recognizing a name does not install or dispatch its handler.
        let schema: Value = canonical::parse(include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/agreement-ready.schema.json"
        ))?;
        let operations = schema["$defs"]["ClosedOperationKind"]["enum"]
            .as_array()
            .ok_or_else(|| fail("closed operation catalog"))?;
        strings(&mut body.operations, 1, 64, |s| {
            if operations.iter().any(|v| v.as_str() == Some(s)) {
                Ok(())
            } else {
                Err(fail("unknown operation"))
            }
        })?;
        selectors(&mut body.product_selectors, 64)?;
        selectors(&mut body.activity_selectors, 64)?;
        selectors(&mut body.link_selectors, 32)?;
        strings(&mut body.source_slots, 0, 32, name)?;
        strings(&mut body.permissions, 1, 8, |s| {
            if [
                "read",
                "submit",
                "correct",
                "link",
                "admin",
                "invocation",
                "close",
                "operate",
            ]
            .contains(&s)
            {
                Ok(())
            } else {
                Err(fail("unknown permission"))
            }
        })?;
        if body.starts_at.micros() >= body.ends_at.micros() {
            return Err(fail("empty source interval"));
        }
        let bytes = CanonicalBytes::from_value(&body)?;
        Ok(Self { body, bytes })
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> Result<Value> {
        canonical::parse(self.canonical_bytes())
    }
    pub fn principal_id(&self) -> &str {
        &self.body.principal_id
    }
    pub fn source(&self) -> &str {
        &self.body.source
    }
    pub fn canonical_economic_namespace(&self) -> &str {
        &self.body.canonical_economic_namespace
    }
    /// Every dimension must narrow. Call this against both the retained
    /// original ceiling and the actual current grant; neither is inferred here.
    pub fn is_subset_of(&self, prior: &Self) -> bool {
        let (n, p) = (&self.body, &prior.body);
        n.principal_id == p.principal_id
            && n.source == p.source
            && n.canonical_economic_namespace == p.canonical_economic_namespace
            && subset(&n.customer_scope, &p.customer_scope)
            && subset(&n.operations, &p.operations)
            && subset(&n.source_slots, &p.source_slots)
            && subset(&n.permissions, &p.permissions)
            && selector_subset(&n.product_selectors, &p.product_selectors)
            && selector_subset(&n.activity_selectors, &p.activity_selectors)
            && selector_subset(&n.link_selectors, &p.link_selectors)
            && n.starts_at.micros() >= p.starts_at.micros()
            && n.ends_at.micros() <= p.ends_at.micros()
    }
    pub fn contains_time(&self, time: &Timestamp) -> bool {
        self.body.starts_at.micros() <= time.micros() && time.micros() < self.body.ends_at.micros()
    }
}
fn subset(n: &[String], p: &[String]) -> bool {
    n.iter().all(|v| p.contains(v))
}
fn selector_subset(n: &[Selector], p: &[Selector]) -> bool {
    n.iter().all(|s| match s {
        Selector::Namespace(n) => p
            .iter()
            .any(|s| matches!(s,Selector::Namespace(p) if n.namespace==p.namespace)),
        Selector::Names(n) => n.names.iter().all(|name| {
            p.iter().any(|s| match s {
                Selector::Namespace(p) => name.starts_with(&p.namespace),
                Selector::Names(p) => p.names.contains(name),
            })
        }),
    })
}
fn selectors(values: &mut Vec<Selector>, max: usize) -> Result<()> {
    if values.len() > max {
        return Err(fail("selector count"));
    }
    for s in values.iter_mut() {
        match s {
            Selector::Names(v) => strings(&mut v.names, 1, 64, name)?,
            Selector::Namespace(v) => {
                let prefix = v
                    .namespace
                    .strip_suffix(':')
                    .ok_or_else(|| fail("namespace syntax"))?;
                // Reuse qualified-name grammar; the selector is a complete prefix,
                // so e.g. a: cannot accidentally match a-long:.
                name(&format!("{prefix}:x"))?;
            }
        }
    }
    canonical::sort_set(values)
}
fn strings(
    values: &mut Vec<String>,
    min: usize,
    max: usize,
    check: impl Fn(&str) -> Result<()>,
) -> Result<()> {
    if !(min..=max).contains(&values.len()) {
        return Err(fail("set count"));
    }
    for v in values.iter() {
        check(v)?;
    }
    canonical::sort_set(values)
}
fn id(s: &str, max: usize) -> Result<()> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(fail("bounded opaque identity"))
    } else {
        Ok(())
    }
}
fn fail(detail: &str) -> Error {
    Error::new("SOURCE_CEILING", detail)
}

#[cfg(test)]
mod tests;
