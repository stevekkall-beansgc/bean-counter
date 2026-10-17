//! Owning entry to the SAME source-ceiling normalization rules. Source custody,
//! original actor/current head and full replay remain host obligations.
use super::super::terms::{
    owned::{self as input, Encode, Storage},
    paid,
};
use super::*;
use crate::{
    canonical::audit_arena::{AuditArena, AuditedBytes, AuditedValue},
    validation_work::{OriginalAuditSession, RepresentationKind, ValidationWork},
};
use std::mem;

pub struct OwnedSourceCeiling {
    body: AuditedValue,
    typed: Ceiling,
    bytes: AuditedBytes,
    _storage: Storage,
}
fn strings(
    value: &Value,
    minimum: usize,
    maximum: usize,
    storage: &mut Storage,
    arena: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Vec<String>> {
    let rows = input::array(value, minimum, maximum, w)?;
    let mut result = storage.vector(rows.len(), arena, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        let text = input::text(row, w)?;
        let copied = storage.text(text, arena, w)?;
        w.write_bytes(mem::size_of::<String>())?;
        result.push(copied);
    }
    // Existing string-set implementation retains encoded keys and conversion
    // tuples during sorting; reuse the same source-pinned terms reservation.
    let mut size = 0usize;
    for value in &result {
        w.arithmetic(
            work::ArithmeticOperation::Multiply,
            &[&7usize.into(), &value.len().into()],
        )?;
        let n = value
            .len()
            .checked_mul(7)
            .ok_or_else(|| fail("source set scratch"))?;
        let n = work::usize_add(w, n, 6)?;
        size = work::usize_add(w, size, n)?;
    }
    w.arithmetic(
        work::ArithmeticOperation::Multiply,
        &[&4usize.into(), &rows.len().into()],
    )?;
    let tuples = rows
        .len()
        .checked_mul(4)
        .ok_or_else(|| fail("source set scratch"))?;
    w.arithmetic(
        work::ArithmeticOperation::Multiply,
        &[&tuples.into(), &mem::size_of::<(Vec<u8>, String)>().into()],
    )?;
    let tuples = tuples
        .checked_mul(mem::size_of::<(Vec<u8>, String)>())
        .ok_or_else(|| fail("source set scratch"))?;
    size = work::usize_add(w, size, tuples)?;
    w.arithmetic(
        work::ArithmeticOperation::Multiply,
        &[&rows.len().into(), &mem::size_of::<String>().into()],
    )?;
    let output = rows
        .len()
        .checked_mul(mem::size_of::<String>())
        .ok_or_else(|| fail("source set scratch"))?;
    size = work::usize_add(w, size, output)?;
    storage.reserve(size, arena, w)?;
    Ok(result)
}
fn selectors(
    value: &Value,
    maximum: usize,
    storage: &mut Storage,
    arena: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Vec<Selector>> {
    let rows = input::array(value, 0, maximum, w)?;
    let mut output = storage.vector(rows.len(), arena, w)?;
    for row in rows {
        w.visit(Visit::Element)?;
        let object = row
            .as_object()
            .ok_or_else(|| fail("source selector object"))?;
        if !work::compare(w, object.len(), 1usize)?.is_eq() {
            return Err(fail("source selector field"));
        }
        let (key, value) = object
            .iter()
            .next()
            .ok_or_else(|| fail("source selector field"))?;
        let selector = if paid::compare_text(key, "names", w)?.is_eq() {
            Selector::Names(Names {
                names: strings(value, 1, 64, storage, arena, w)?,
            })
        } else {
            if !paid::compare_text(key, "namespace", w)?.is_eq() {
                return Err(fail("source selector field"));
            }
            let namespace = storage.text(input::text(value, w)?, arena, w)?;
            // Qualified prefix validation creates exactly prefix + ":x".
            let n = work::usize_add(w, namespace.len(), 2)?;
            storage.reserve(n, arena, w)?;
            Selector::Namespace(Namespace { namespace })
        };
        w.write_bytes(mem::size_of::<Selector>())?;
        output.push(selector);
    }
    Ok(output)
}
fn decode(
    value: &Value,
    s: &mut Storage,
    a: &AuditArena,
    w: &mut dyn ValidationWork,
) -> Result<Ceiling> {
    input::closed(
        value,
        &[
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "customer_scope",
            "operations",
            "product_selectors",
            "activity_selectors",
            "link_selectors",
            "source_slots",
            "permissions",
            "starts_at",
            "ends_at",
        ],
        &[],
        w,
    )?;
    let mut time = |name| -> Result<Timestamp> {
        s.reserve(4 * 64 + 27, a, w)?;
        Timestamp::parse_with_work(input::text(input::field(value, name, w)?, w)?, w)
    };
    let starts_at = time("starts_at")?;
    let ends_at = time("ends_at")?;
    Ok(Ceiling {
        principal_id: input::string(value, "principal_id", s, a, w)?,
        source: input::string(value, "source", s, a, w)?,
        canonical_economic_namespace: input::string(
            value,
            "canonical_economic_namespace",
            s,
            a,
            w,
        )?,
        customer_scope: strings(input::field(value, "customer_scope", w)?, 1, 64, s, a, w)?,
        operations: strings(input::field(value, "operations", w)?, 1, 64, s, a, w)?,
        product_selectors: selectors(input::field(value, "product_selectors", w)?, 64, s, a, w)?,
        activity_selectors: selectors(input::field(value, "activity_selectors", w)?, 64, s, a, w)?,
        link_selectors: selectors(input::field(value, "link_selectors", w)?, 32, s, a, w)?,
        source_slots: strings(input::field(value, "source_slots", w)?, 0, 32, s, a, w)?,
        permissions: strings(input::field(value, "permissions", w)?, 1, 8, s, a, w)?,
        starts_at,
        ends_at,
    })
}
fn encode_selectors(
    e: &mut Encode<'_>,
    rows: &[Selector],
    w: &mut dyn ValidationWork,
) -> Result<AuditedBytes> {
    e.list(rows, w, |e, row, w| match row {
        Selector::Namespace(v) => {
            let value = e.text(&v.namespace, w)?;
            e.object(&[("namespace", value.bytes())], w)
        }
        Selector::Names(v) => {
            let value = e.list(&v.names, w, |e, v, w| e.text(v, w))?;
            e.object(&[("names", value.bytes())], w)
        }
    })
}
fn encode(body: &Ceiling, e: &mut Encode<'_>, w: &mut dyn ValidationWork) -> Result<AuditedBytes> {
    let activities = encode_selectors(e, &body.activity_selectors, w)?;
    let namespace = e.text(&body.canonical_economic_namespace, w)?;
    let customers = e.list(&body.customer_scope, w, |e, v, w| e.text(v, w))?;
    let end = e.text(body.ends_at.as_str(), w)?;
    let links = encode_selectors(e, &body.link_selectors, w)?;
    let operations = e.list(&body.operations, w, |e, v, w| e.text(v, w))?;
    let permissions = e.list(&body.permissions, w, |e, v, w| e.text(v, w))?;
    let principal = e.text(&body.principal_id, w)?;
    let products = encode_selectors(e, &body.product_selectors, w)?;
    let source = e.text(&body.source, w)?;
    let slots = e.list(&body.source_slots, w, |e, v, w| e.text(v, w))?;
    let start = e.text(body.starts_at.as_str(), w)?;
    e.object(
        &[
            ("activity_selectors", activities.bytes()),
            ("canonical_economic_namespace", namespace.bytes()),
            ("customer_scope", customers.bytes()),
            ("ends_at", end.bytes()),
            ("link_selectors", links.bytes()),
            ("operations", operations.bytes()),
            ("permissions", permissions.bytes()),
            ("principal_id", principal.bytes()),
            ("product_selectors", products.bytes()),
            ("source", source.bytes()),
            ("source_slots", slots.bytes()),
            ("starts_at", start.bytes()),
        ],
        w,
    )
}
impl OwnedSourceCeiling {
    pub fn normalize(value: &Value, session: &mut OriginalAuditSession) -> Result<Self> {
        let result = Self::inner(value, session);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    fn inner(value: &Value, session: &mut OriginalAuditSession) -> Result<Self> {
        let arena = session.arena().shared_handle();
        let w = session.meter_mut();
        let mut storage = Storage::new();
        let mut value = decode(value, &mut storage, &arena, w)?;
        super::normalize(&mut value, w, true)?;
        let bytes = encode(
            &value,
            &mut Encode {
                storage: &mut storage,
                arena: &arena,
            },
            w,
        )?;
        // Typed payload and parsed/encoded views all precede their reservation
        // owner, including subsequent SAME source-ceiling lineage predicates.
        let body =
            session.parse_representation(bytes.bytes(), RepresentationKind::SourceDefinition)?;
        Ok(Self {
            body,
            typed: value,
            bytes,
            _storage: storage,
        })
    }
    /// SAME immutable ceiling intersection rules with executing work. This is
    /// a DATA predicate; both owners must remain in this one audit attempt.
    pub fn is_subset_of_with_work(
        &self,
        prior: &Self,
        session: &mut OriginalAuditSession,
    ) -> Result<bool> {
        let result = (|| {
            session.check_cancelled()?;
            let arena = session.arena().shared_handle();
            session.semantic_work().visit(Visit::Constraint)?;
            if !self.body.belongs_to(&arena) || !prior.body.belongs_to(&arena) {
                return Err(fail("cross-attempt source ceiling"));
            }
            super::subset_with_work(&self.typed, &prior.typed, session.semantic_work())
        })();
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    pub fn canonical_value(&self) -> &Value {
        self.body.value()
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use serde_json::json;
    #[test]
    fn owning_lineage_matches_legacy_narrowing_and_legal_selector_regrouping() {
        let original = json!({"principal_id":"owner","source":"source","canonical_economic_namespace":"source","customer_scope":["c"],"operations":["work.accept","evidence.retain"],"product_selectors":[{"names":["a:product"]}],"activity_selectors":[{"names":["a:one","a:two"]}],"link_selectors":[],"source_slots":["a:worker"],"permissions":["submit","read"],"starts_at":"2026-10-17T00:00:00Z","ends_at":"2026-10-18T00:00:00Z"});
        let mut cases = vec![(original.clone(), true)];
        let mut grouped = original.clone();
        grouped["activity_selectors"] =
            json!([{"names":["a:one"]},{"names":["a:two"]},{"names":["a:one","a:two"]}]);
        cases.push((grouped, true));
        for (field, value) in [
            ("permissions", json!(["read", "submit", "operate"])),
            ("activity_selectors", json!([{"namespace":"a:"}])),
            ("starts_at", json!("2026-10-16T00:00:00Z")),
            ("source", json!("different")),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            cases.push((changed, false));
        }
        for (value, expected) in cases {
            let old = SourceCeiling::parse(&serde_json::to_vec(&original).unwrap()).unwrap();
            let next = SourceCeiling::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(next.is_subset_of(&old), expected);
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let old = OwnedSourceCeiling::normalize(&original, &mut session).unwrap();
            let next = OwnedSourceCeiling::normalize(&value, &mut session).unwrap();
            assert_eq!(
                next.is_subset_of_with_work(&old, &mut session).unwrap(),
                expected
            );
        }
    }
    #[test]
    fn owning_source_ceiling_reuses_legacy_rules_bytes_and_selector_set_order() {
        let schema: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/agreement-ready.schema.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(super::super::CLOSED_OPERATIONS).unwrap(),
            schema["$defs"]["ClosedOperationKind"]["enum"]
        );
        let base = json!({"principal_id":"owner","source":"source","canonical_economic_namespace":"source","customer_scope":["c"],"operations":["work.accept","evidence.retain"],"product_selectors":[{"names":["a:two","a:one"]},{"namespace":"b:"},{"names":["a:one"]}],"activity_selectors":[],"link_selectors":[],"source_slots":["a:worker"],"permissions":["submit","read"],"starts_at":"2026-10-17T08:00:00-04:00","ends_at":"2026-10-18T12:00:00Z"});
        let mut cases = vec![base.clone()];
        for (field, value) in [
            ("permissions", json!(["read", "read"])),
            (
                "product_selectors",
                json!([{"names":["a:one"]},{"names":["a:one"]}]),
            ),
            ("ends_at", json!("2026-10-17T12:00:00Z")),
            ("operations", json!(["not.known"])),
        ] {
            let mut v = base.clone();
            v[field] = value;
            cases.push(v);
        }
        for value in cases {
            let legacy = SourceCeiling::parse(&serde_json::to_vec(&value).unwrap());
            let arena = AuditArena::new().unwrap();
            let baseline = arena.usage().unwrap().live_bytes;
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let owning = OwnedSourceCeiling::normalize(&value, &mut session);
            assert_eq!(legacy.is_ok(), owning.is_ok());
            if let (Ok(legacy), Ok(owning)) = (legacy, owning) {
                assert_eq!(legacy.canonical_bytes(), owning.canonical_bytes());
                assert_eq!(legacy.canonical_value().unwrap(), *owning.canonical_value());
                drop(owning);
            }
            assert_eq!(baseline, arena.usage().unwrap().live_bytes);
        }
    }
}
