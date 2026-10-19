//! Immutable source constraints. Parsing or comparing these values does not
//! authenticate a source, establish enrollment, or grant an operation right.
use super::facts::reject_null;
use crate::validation_work::{
    self as work, NoopValidationWork, ValidationVisit as Visit, ValidationWork,
};
mod owned;
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    Error, Result,
};
pub use owned::OwnedSourceCeiling;
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
        normalize(&mut body, &mut NoopValidationWork, false)?;
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
        subset_with_work(&self.body, &prior.body, &mut NoopValidationWork).unwrap_or(false)
    }

    pub fn contains_time(&self, time: &Timestamp) -> bool {
        self.body.starts_at.micros() <= time.micros() && time.micros() < self.body.ends_at.micros()
    }
}
const CLOSED_OPERATIONS: &[&str] = &[
    "installation.initialize",
    "installation.upgrade",
    "evidence.retain",
    "policy.publish",
    "agreement.accept",
    "agreement.activate",
    "agreement.revoke",
    "authority.change",
    "invocation.authorize",
    "invocation.close",
    "work.accept",
    "link.assert",
    "outcome.submit",
    "outcome.correct",
    "outcome.close",
    "work.reverse",
    "term.set",
    "cumulative_agreement.set",
    "activity.submit",
    "quantity.correct",
    "recurrence.set",
    "recurrence.cancel",
    "occurrence.accept",
    "fiscal.set",
    "period.resolve",
    "period.close",
    "statement.adjustments.issue",
    "settlement.close",
    "cost.finalize",
    "cost.correct",
    "connector.configure",
    "connector.pause",
    "connector.reconcile",
    "connector.resume",
    "dispatch.hold",
    "dispatch.resume",
    "settlement.dispatch",
    "delivery.enqueue",
    "backup.create",
    "backup.retrieve",
    "backup.verify",
    "backup.prune",
    "restore.stage",
    "restore.verify",
    "restore.activate",
    "restore.resume",
    "meter.ingest",
    "meter.finalize",
    "meter.correct",
    "service.rate.accept",
    "deployment.apply",
    "deployment.pause",
    "deployment.resume",
    "deployment.export",
    "deployment.offboard",
    "credential.issue",
    "credential.rotate",
    "credential.revoke",
    "producer.enroll",
    "producer.revoke",
    "provider.observation.record",
    "lifecycle.target.close",
    "recovery.bundle.import",
    "settlement.credit.allocate",
];
fn normalize(body: &mut Ceiling, w: &mut dyn ValidationWork, complete: bool) -> Result<()> {
    id_work(&body.principal_id, 128, w)?;
    id_work(&body.source, 256, w)?;
    id_work(&body.canonical_economic_namespace, 256, w)?;
    strings_work(
        &mut body.customer_scope,
        1,
        64,
        |v, w| id_work(v, 128, w),
        w,
        complete,
    )?;
    strings_work(
        &mut body.operations,
        1,
        64,
        |v, w| {
            for allowed in CLOSED_OPERATIONS {
                if super::terms::paid::compare_text(v, allowed, w)?.is_eq() {
                    return Ok(());
                }
            }
            Err(fail("unknown operation"))
        },
        w,
        complete,
    )?;
    selectors_work(&mut body.product_selectors, 64, w, complete)?;
    selectors_work(&mut body.activity_selectors, 64, w, complete)?;
    selectors_work(&mut body.link_selectors, 32, w, complete)?;
    strings_work(
        &mut body.source_slots,
        0,
        32,
        super::facts::name_with_work,
        w,
        complete,
    )?;
    strings_work(
        &mut body.permissions,
        1,
        8,
        |v, w| {
            for allowed in [
                "read",
                "submit",
                "correct",
                "link",
                "admin",
                "invocation",
                "close",
                "operate",
            ] {
                if super::terms::paid::compare_text(v, allowed, w)?.is_eq() {
                    return Ok(());
                }
            }
            Err(fail("unknown permission"))
        },
        w,
        complete,
    )?;
    if !work::compare(w, body.starts_at.micros(), body.ends_at.micros())?.is_lt() {
        return Err(fail("empty source interval"));
    }
    Ok(())
}
fn id_work(v: &str, max: usize, w: &mut dyn ValidationWork) -> Result<()> {
    super::terms::paid::text(v, max, w).map_err(|e| {
        if e.code == "IDENTIFIER" {
            fail("bounded opaque identity")
        } else {
            e
        }
    })
}
fn strings_work(
    values: &mut Vec<String>,
    min: usize,
    max: usize,
    mut check: impl FnMut(&str, &mut dyn ValidationWork) -> Result<()>,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<()> {
    w.visit(Visit::Constraint)?;
    if work::compare(w, values.len(), min)?.is_lt() || work::compare(w, values.len(), max)?.is_gt()
    {
        return Err(fail("set count"));
    }
    for value in values.iter() {
        w.visit(Visit::Element)?;
        check(value, w)?;
    }
    if complete {
        super::terms::paid::string_set(values, w)
    } else {
        canonical::sort_set(values)
    }
}
fn selector_order(
    a: &Selector,
    b: &Selector,
    w: &mut dyn ValidationWork,
) -> Result<std::cmp::Ordering> {
    use std::cmp::Ordering;
    w.visit(Visit::MemberCompare)?;
    match (a, b) {
        (Selector::Namespace(a), Selector::Namespace(b)) => {
            super::terms::paid::compare_text(&a.namespace, &b.namespace, w)
        }
        (Selector::Names(a), Selector::Names(b)) => {
            for (a, b) in a.names.iter().zip(&b.names) {
                let order = super::terms::paid::compare_text(a, b, w)?;
                if !order.is_eq() {
                    return Ok(order);
                }
            }
            // Equal name prefix: comma precedes close-bracket in exact JCS.
            Ok(work::compare(w, b.names.len(), a.names.len())?)
        }
        (Selector::Names(_), Selector::Namespace(_)) => Ok(Ordering::Less),
        (Selector::Namespace(_), Selector::Names(_)) => Ok(Ordering::Greater),
    }
}
fn selectors_work(
    values: &mut Vec<Selector>,
    max: usize,
    w: &mut dyn ValidationWork,
    complete: bool,
) -> Result<()> {
    w.visit(Visit::Constraint)?;
    if work::compare(w, values.len(), max)?.is_gt() {
        return Err(fail("selector count"));
    }
    for value in values.iter_mut() {
        w.visit(Visit::Element)?;
        match value {
            Selector::Names(v) => strings_work(
                &mut v.names,
                1,
                64,
                super::facts::name_with_work,
                w,
                complete,
            )?,
            Selector::Namespace(v) => {
                w.visit(Visit::Constraint)?;
                w.scan_bytes(1)?;
                let prefix = v
                    .namespace
                    .strip_suffix(':')
                    .ok_or_else(|| fail("namespace syntax"))?;
                let n = work::usize_add(w, prefix.len(), 2)?;
                w.scan_bytes(prefix.len())?;
                w.write_bytes(n)?;
                if complete {
                    let mut qualified = String::new();
                    qualified
                        .try_reserve_exact(n)
                        .map_err(|_| fail("namespace validation scratch"))?;
                    qualified.push_str(prefix);
                    qualified.push_str(":x");
                    super::facts::name_with_work(&qualified, w)?;
                } else {
                    super::facts::name_with_work(&format!("{prefix}:x"), w)?;
                }
            }
        }
    }
    if complete {
        super::terms::paid::sort(values, w, selector_order)?;
        for pair in values.windows(2) {
            if selector_order(&pair[0], &pair[1], w)?.is_eq() {
                return Err(crate::Error::new("DUPLICATE", "set member"));
            }
        }
        Ok(())
    } else {
        canonical::sort_set(values)
    }
}

fn text_equal(a: &str, b: &str, w: &mut dyn ValidationWork) -> Result<bool> {
    Ok(super::terms::paid::compare_text(a, b, w)?.is_eq())
}
fn string_subset(n: &[String], p: &[String], w: &mut dyn ValidationWork) -> Result<bool> {
    for name in n {
        w.visit(Visit::Element)?;
        let mut found = false;
        for prior in p {
            if text_equal(name, prior, w)? {
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}
fn selectors_subset(n: &[Selector], p: &[Selector], w: &mut dyn ValidationWork) -> Result<bool> {
    for selector in n {
        w.visit(Visit::Element)?;
        match selector {
            Selector::Namespace(n) => {
                let mut found = false;
                for selector in p {
                    w.visit(Visit::Element)?;
                    if let Selector::Namespace(p) = selector {
                        if text_equal(&n.namespace, &p.namespace, w)? {
                            found = true;
                            break;
                        }
                    }
                }
                if !found {
                    return Ok(false);
                }
            }
            Selector::Names(n) => {
                for name in &n.names {
                    w.visit(Visit::Element)?;
                    let mut found = false;
                    for selector in p {
                        w.visit(Visit::Element)?;
                        let matches = match selector {
                            Selector::Names(p) => {
                                let mut covered = false;
                                for allowed in &p.names {
                                    if text_equal(name, allowed, w)? {
                                        covered = true;
                                        break;
                                    }
                                }
                                covered
                            }
                            Selector::Namespace(p) => {
                                w.visit(Visit::MemberCompare)?;
                                w.scan_bytes(name.len())?;
                                w.scan_bytes(p.namespace.len())?;
                                name.starts_with(&p.namespace)
                            }
                        };
                        if matches {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}
fn subset_with_work(n: &Ceiling, p: &Ceiling, w: &mut dyn ValidationWork) -> Result<bool> {
    if !text_equal(&n.principal_id, &p.principal_id, w)?
        || !text_equal(&n.source, &p.source, w)?
        || !text_equal(
            &n.canonical_economic_namespace,
            &p.canonical_economic_namespace,
            w,
        )?
    {
        return Ok(false);
    }
    for (n, p) in [
        (&n.customer_scope, &p.customer_scope),
        (&n.operations, &p.operations),
        (&n.source_slots, &p.source_slots),
        (&n.permissions, &p.permissions),
    ] {
        w.visit(Visit::Field)?;
        if !string_subset(n, p, w)? {
            return Ok(false);
        }
    }
    for (n, p) in [
        (&n.product_selectors, &p.product_selectors),
        (&n.activity_selectors, &p.activity_selectors),
        (&n.link_selectors, &p.link_selectors),
    ] {
        w.visit(Visit::Field)?;
        if !selectors_subset(n, p, w)? {
            return Ok(false);
        }
    }
    Ok(
        !work::compare(w, n.starts_at.micros(), p.starts_at.micros())?.is_lt()
            && !work::compare(w, n.ends_at.micros(), p.ends_at.micros())?.is_gt(),
    )
}
fn fail(detail: &str) -> Error {
    Error::new("SOURCE_CEILING", detail)
}

#[cfg(test)]
mod tests;
