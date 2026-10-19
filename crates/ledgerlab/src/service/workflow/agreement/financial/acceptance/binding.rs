//! The existing acceptance binding recipe over owning normalized inputs.
//! No private graph is duplicated here: the SAME core slot validator consumes
//! this body and actual retained catalog/instantiation/role/terms/grants. Neither
//! a matching body nor this source record establishes consent or provenance.
use super::*;
use ledgerlab_core::policy::workflow::{selected_program::SelectedProgram, DefinitionRef};

fn definition(r: &DefinitionRef, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let digest = wire::string(r.digest(), s)?;
    let id = wire::string(r.id(), s)?;
    let kind = wire::string(r.kind(), s)?;
    wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )
}
fn original_product<'a>(
    program: &'a SelectedProgram,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let expected = program.product_ref();
    for source in program.source_documents() {
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        let actual = source.reference();
        // Reserve the complete conservative byte pass before exact equality.
        for (a, b) in [
            (actual.kind(), expected.kind()),
            (actual.id(), expected.id()),
            (actual.digest(), expected.digest()),
        ] {
            s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
            s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
        }
        if actual == expected {
            return Ok(source.canonical_value());
        }
    }
    Err(ServiceError::IntegrityFailure)
}
pub(super) fn assemble(
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    actor: &str,
    at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let program = publication.program();
    request_wrapper::same_text(
        program
            .execution_registry()
            .ok_or(ServiceError::Unavailable)?,
        input.registry(),
        s,
    )?;
    let terms = input.terms.canonical_value();
    let projected = request_wrapper::field(terms, "binding_inputs", s)?;
    // Independently bind this actual publication even when a caller reuses a
    // normalized input object with a different privately retained owner.
    let supplied = encode(request_wrapper::field(terms, "publication_ref", s)?, s)?;
    let actual = wire::reference(publication.publication().0, s)?;
    same_bytes(supplied.bytes(), actual.bytes(), s)?;
    let product = original_product(program, s)?;
    let schemas = reference_set(
        request_wrapper::field(product, "fact_schemas", s)?,
        "workflow-fact-schema",
        1,
        16,
        s,
    )?;
    let policy = definition(program.root_policy_ref(), s)?;
    let product_ref = definition(program.product_ref(), s)?;
    let plan = definition(program.compiled_plan().reference(), s)?;
    let evaluator = encode(
        request_wrapper::field(program.compiled_plan().canonical_value(), "evaluator", s)?,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(program.compiled_plan().canonical_value(), "evaluator", s)?,
        "ledger-workflow-evaluator/1",
        s,
    )?;
    let actor = wire::string(actor, s)?;
    let at = wire::string(at.as_str(), s)?;
    let schema = wire::string("ledger-workflow-binding/2", s)?;
    let assents = encode(input.assent_refs(), s)?;
    let authorities = encode(input.authority_refs(), s)?;
    const KEYS: [&str; 15] = [
        "accepted_context",
        "agreement_id",
        "bindings",
        "customer",
        "ends_at",
        "exposure_ceilings",
        "funding_legs",
        "quantity_ceilings",
        "roles_refs",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "source_namespace",
        "source_slots",
        "starts_at",
        "version",
    ];
    let mut values: [Option<AuditedBytes>; 15] = std::array::from_fn(|_| None);
    for (index, name) in KEYS.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        let value = if *name == "ends_at" {
            // Optional-field lookup itself is executing paid field work.
            let mut found = None;
            for (key, value) in projected
                .as_object()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(key.len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(name.len())
                    .map_err(core_error)?;
                if key == name {
                    found = Some(value);
                    break;
                }
            }
            found
        } else {
            Some(request_wrapper::field(projected, name, s)?)
        };
        if let Some(value) = value {
            values[index] = Some(encode(value, s)?);
        }
    }
    // The fixed recipe keys are in JCS order; business strings and all nested
    // values above use the SAME normalizer/emitter, never a second JSON codec.
    const ALL: [&str; 25] = [
        "accepted_at",
        "accepted_by",
        "accepted_context",
        "agreement_id",
        "assent_refs",
        "authority_refs",
        "bindings",
        "compiled_plan_ref",
        "customer",
        "ends_at",
        "evaluator",
        "exposure_ceilings",
        "fact_schema_refs",
        "funding_legs",
        "policy_ref",
        "product_ref",
        "quantity_ceilings",
        "roles_refs",
        "schema",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "source_namespace",
        "source_slots",
        "starts_at",
        "version",
    ];
    let mut fields = [("", &[] as &[u8]); 25];
    let mut count = 0usize;
    for key in ALL {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let special = match key {
            "accepted_at" => Some(at.bytes()),
            "accepted_by" => Some(actor.bytes()),
            "assent_refs" => Some(assents.bytes()),
            "authority_refs" => Some(authorities.bytes()),
            "compiled_plan_ref" => Some(plan.bytes()),
            "evaluator" => Some(evaluator.bytes()),
            "fact_schema_refs" => Some(schemas.bytes()),
            "policy_ref" => Some(policy.bytes()),
            "product_ref" => Some(product_ref.bytes()),
            "schema" => Some(schema.bytes()),
            _ => None,
        };
        let mut value = special;
        if value.is_none() {
            for (name, encoded) in KEYS.iter().zip(values.iter()) {
                s.semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(key.len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(name.len())
                    .map_err(core_error)?;
                if key == *name {
                    value = encoded.as_ref().map(|v| v.bytes());
                    break;
                }
            }
        }
        if let Some(value) = value {
            s.semantic_work()
                .write_bytes(mem::size_of::<(&str, &[u8])>())
                .map_err(core_error)?;
            fields[count] = (key, value);
            s.semantic_work()
                .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            count = count.checked_add(1).ok_or(ServiceError::Unavailable)?;
        }
    }
    let body = wire::object(&fields[..count], 262144, s)?;
    wire::source_record("workflow-binding", "ledger-workflow-binding/2", body, s)
}
