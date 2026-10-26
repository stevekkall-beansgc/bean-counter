//! Typed reference-fact acquisition DATA. The owning selector follows only
//! actual fields in the exact original schema. It does not authenticate a
//! record, authorize its relation, or make a parsed Ref an authority token.
use super::super::{selected_program::SelectedProgram, FactSchema};
use super::{data::ExactRef, shape, working_value::NormalizedInput, CoreMeter};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, OriginalAuditSession},
    Result,
};
use serde_json::Value;
use std::mem;

pub struct FactReferenceInput {
    field: String,
    relation: &'static str,
    reference: ExactRef,
}
impl FactReferenceInput {
    pub fn field(&self) -> &str {
        &self.field
    }
    pub fn relation(&self) -> &'static str {
        self.relation
    }
    pub fn reference(&self) -> &ExactRef {
        &self.reference
    }
}
pub struct FactReferenceInputs {
    entries: Vec<FactReferenceInput>,
    _ownership: ExternalBufferLease,
}
impl FactReferenceInputs {
    pub fn entries(&self) -> &[FactReferenceInput] {
        &self.entries
    }
}

fn original_schema<'a>(
    m: &mut CoreMeter,
    program: &'a SelectedProgram,
    reference: &Value,
) -> Result<&'a FactSchema> {
    shape::closed(m, reference, &["kind", "id", "digest"], &[])?;
    let kind = shape::text(m, reference, "kind")?;
    let id = shape::text(m, reference, "id")?;
    let digest = shape::text(m, reference, "digest")?;
    let mut selected = None;
    for (r, schema) in program.fact_schemas() {
        m.visit("fact_reference.original_schema")?;
        if shape::same_text(m, r.kind(), kind)?
            && shape::same_text(m, r.id(), id)?
            && shape::same_text(m, r.digest(), digest)?
        {
            if selected.is_some() {
                return Err(shape::fail("ambiguous original reference schema"));
            }
            selected = Some(schema);
        }
    }
    selected.ok_or_else(|| shape::fail("reference facts require exact original schema"))
}
fn copy(m: &mut CoreMeter, text: &str) -> Result<String> {
    m.scan_bytes(text.len())?;
    m.write_bytes(text.len())?;
    Ok(text.to_owned())
}
fn select(m: &mut CoreMeter, schema: &FactSchema, facts: &Value) -> Result<FactReferenceInputs> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning reference selector"))?;
    let normalized = NormalizedInput::normalize(m, schema, facts)?;
    let facts = normalized.canonical_value();
    let fields = shape::array(m, schema.canonical_value(), "fields")?;
    let mut count = 0usize;
    let mut bytes = mem::size_of::<FactReferenceInputs>();
    for field in fields {
        m.visit("fact_reference.count_declared_field")?;
        let name = shape::text(m, field, "name")?;
        if schema.reference_requirement_with_work(name, m)?.is_none() {
            continue;
        }
        let Some(value) = facts.get(name) else {
            continue;
        };
        count = w::usize_add(m, count, 1)?;
        bytes = w::usize_add(m, bytes, mem::size_of::<FactReferenceInput>())?;
        bytes = w::usize_add(m, bytes, name.len())?;
        for key in ["kind", "id", "digest"] {
            let text = shape::text(m, value, key)?;
            bytes = w::usize_add(m, bytes, text.len())?;
        }
    }
    // All copied field/Ref strings and vector control/payload remain owned
    // after the temporary normalized fact graph is dropped.
    let ownership = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("reference input vector"))?;
    for field in fields {
        m.visit("fact_reference.emit_declared_field")?;
        let name = shape::text(m, field, "name")?;
        let Some((_, relation)) = schema.reference_requirement_with_work(name, m)? else {
            continue;
        };
        let Some(value) = facts.get(name) else {
            continue;
        };
        let name = copy(m, name)?;
        let kind = shape::text(m, value, "kind")?;
        let kind = copy(m, kind)?;
        let id = shape::text(m, value, "id")?;
        let id = copy(m, id)?;
        let digest = shape::text(m, value, "digest")?;
        let digest = copy(m, digest)?;
        entries.push(FactReferenceInput {
            field: name,
            relation,
            reference: ExactRef { kind, id, digest },
        });
    }
    Ok(FactReferenceInputs {
        entries,
        _ownership: ownership,
    })
}

/// Actual assertion claims are a field subset, not a complete future Work.
/// Inspect only declared Reference fields; retention separately proves all
/// fields, issuer purpose, subject and original/current authority.
fn select_claims(
    m: &mut CoreMeter,
    schema: &FactSchema,
    claims: &Value,
) -> Result<FactReferenceInputs> {
    use super::super::FactValue;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning reference claims selector"))?;
    let values = claims
        .as_array()
        .ok_or_else(|| shape::fail("reference assertion claims array"))?;
    let scratch = crate::canonical::audit_arena::value_copy_payload_bound(claims, m)?;
    // Each temporary typed Ref owns at most the actual three-string input
    // graph's payload. It is dropped before the next scalar is normalized.
    let _scratch = arena.reserve_external_payload(scratch, m)?;
    let walk = |m: &mut CoreMeter,
                emit: &mut dyn FnMut(
        &str,
        &'static str,
        &crate::policy::workflow::facts::FactReference,
        &mut CoreMeter,
    ) -> Result<()>|
     -> Result<()> {
        for (index, claim) in values.iter().enumerate() {
            shape::closed(m, claim, &["field", "value"], &[])?;
            let name = shape::text(m, claim, "field")?;
            for prior in &values[..index] {
                let previous = shape::text(m, prior, "field")?;
                if shape::same_text(m, name, previous)? {
                    return Err(shape::fail("duplicate reference assertion field"));
                }
            }
            let Some((_, relation)) = schema.reference_requirement_with_work(name, m)? else {
                continue;
            };
            let value = schema.normalize_attested_field_complete_with_work(
                name,
                shape::field(m, claim, "value")?,
                m,
            )?;
            let FactValue::Reference(reference) = value else {
                return Err(shape::fail("typed reference assertion value"));
            };
            emit(name, relation, &reference, m)?;
        }
        Ok(())
    };
    let mut count = 0usize;
    let mut bytes = mem::size_of::<FactReferenceInputs>();
    walk(m, &mut |name, _, reference, m| {
        count = w::usize_add(m, count, 1)?;
        bytes = w::usize_add(m, bytes, mem::size_of::<FactReferenceInput>())?;
        for text in [name, reference.kind(), reference.id(), reference.digest()] {
            bytes = w::usize_add(m, bytes, text.len())?;
        }
        Ok(())
    })?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| shape::fail("reference claim input vector"))?;
    walk(m, &mut |name, relation, reference, m| {
        entries.push(FactReferenceInput {
            field: copy(m, name)?,
            relation,
            reference: ExactRef {
                kind: copy(m, reference.kind())?,
                id: copy(m, reference.id())?,
                digest: copy(m, reference.digest())?,
            },
        });
        Ok(())
    })?;
    Ok(FactReferenceInputs {
        entries,
        _ownership: ownership,
    })
}
pub(super) fn select_group(
    m: &mut CoreMeter,
    program: &SelectedProgram,
    schema: &Value,
    values: &Value,
    claims: bool,
) -> Result<FactReferenceInputs> {
    let schema = original_schema(m, program, schema)?;
    if claims {
        select_claims(m, schema, values)
    } else {
        select(m, schema, values)
    }
}

/// Exact typed seeds for the coordinator's original-cutoff body/producer/member
/// acquisition. Neither this selection nor caller-supplied completeness can
/// replace the child's fixed record-kind and declared-relation validation.
pub fn select_fact_reference_inputs(
    program: &SelectedProgram,
    fact_schema_ref: &Value,
    actual_facts: &Value,
    session: &mut OriginalAuditSession,
) -> Result<FactReferenceInputs> {
    let result = (|| {
        session.check_cancelled()?;
        let m = session.meter_mut();
        program.require_attempt_owner(m)?;
        let schema = original_schema(m, program, fact_schema_ref)?;
        select(m, schema, actual_facts)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use crate::validation_work::ResourceLimits;
    use serde_json::json;

    #[test]
    fn typed_reference_selector_owns_only_declared_values_and_preserves_relation_data() {
        // Actual selector/normalizer DATA, not completed origin/relation proof.
        let schema = FactSchema::parse(&serde_json::to_vec(&json!({
            "schema":"ledger-workflow-fact-schema/1", "schema_id":"example:references", "version":"1", "normalization":"ledger-facts-normalization/1",
            "fields":[
                {"name":"example:ancestor","required":true,"provenance":{"kind":"caller"},"type":{"kind":"reference","record_kind":"workflow-work","relation":"same_binding"}},
                {"name":"example:optional","required":false,"provenance":{"kind":"caller"},"type":{"kind":"reference","record_kind":"workflow-action","relation":"same_scope_customer"}},
                {"name":"example:text","required":true,"provenance":{"kind":"caller"},"type":{"kind":"string","min_bytes":"0","max_bytes":"512"}}
            ]
        })).unwrap()).unwrap();
        let reference = json!({"kind":"workflow-work","id":format!("wevent_{}","a".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))});
        let facts = json!({"example:ancestor":reference,"example:text":reference.to_string()});
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let selected = select(session.meter_mut(), &schema, &facts).unwrap();
        drop(facts);
        assert_eq!(selected.entries().len(), 1);
        assert_eq!(selected.entries()[0].field(), "example:ancestor");
        assert_eq!(selected.entries()[0].relation(), "same_binding");
        assert_eq!(selected.entries()[0].reference().kind, "workflow-work");
        assert_eq!(
            selected.entries()[0].reference().id,
            reference["id"].as_str().unwrap()
        );
        // Optional absence creates no seed, a Ref-looking string creates no
        // seed, and normalization refuses a typed-kind substitution.
        let mut invalid = json!({"example:ancestor":reference,"example:text":"ordinary"});
        invalid["example:ancestor"]["kind"] = json!("workflow-action");
        assert!(select(session.meter_mut(), &schema, &invalid).is_err());
    }
    #[test]
    fn reference_assertion_subset_does_not_fabricate_unclaimed_required_facts() {
        let schema = FactSchema::parse(&serde_json::to_vec(&json!({
            "schema":"ledger-workflow-fact-schema/1", "schema_id":"example:claims", "version":"1", "normalization":"ledger-facts-normalization/1",
            "fields":[
                {"name":"example:definition","required":true,"provenance":{"kind":"attested","source_slot":"example:issuer","evidence_kind":"meter_quantity"},"type":{"kind":"reference","record_kind":"workflow-product","relation":"same_binding"}},
                {"name":"example:quantity","required":true,"provenance":{"kind":"caller"},"type":{"kind":"integer","min":"0","max":"10","unit":"example:unit"}}
            ]
        })).unwrap()).unwrap();
        let reference = json!({"kind":"workflow-product","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let claims = json!([{"field":"example:definition","value":reference}]);
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let selected = select_claims(s.meter_mut(), &schema, &claims).unwrap();
        assert_eq!(selected.entries().len(), 1);
        assert_eq!(selected.entries()[0].field(), "example:definition");
        assert_eq!(selected.entries()[0].relation(), "same_binding");
        assert_eq!(selected.entries()[0].reference().kind, "workflow-product");
        let wrong = json!([{"field":"example:definition","value":{"kind":"workflow-work","id":format!("wevent_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))}}]);
        assert!(select_claims(s.meter_mut(), &schema, &wrong).is_err());
        assert!(select_claims(s.meter_mut(), &schema, &json!([claims[0], claims[0]])).is_err());
    }
}
