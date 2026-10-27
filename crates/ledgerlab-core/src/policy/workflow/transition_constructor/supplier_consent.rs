//! Source-derived supplier commitment and actual consent verification. These
//! are owning derived records; a supplied equality, Ref or success flag is not
//! an acceptance/current-party/financial capability.
use super::super::selected_program::{
    supplier_projection::{self, Values},
    SelectedProgram,
};
use super::{
    body,
    data::*,
    emitted_record::ConstructedEnvelope,
    inventory,
    records::Records,
    shape,
    working_value::{owning_definition, parse_owned_reference, NormalizedInput, WorkingValue},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Result,
};
use serde_json::Value;
use std::mem;

pub struct LocalSupplierCommitment {
    record: ConstructedEnvelope,
    _ownership: ExternalBufferLease,
}
impl LocalSupplierCommitment {
    pub fn reference(&self) -> &ExactRef {
        &self.record.reference
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.record.canonical_body
    }
    pub fn canonical_value(&self) -> &Value {
        self.record
            .decoded
            .as_ref()
            .expect("owning commitment frame")
    }
}
fn literal(m: &mut CoreMeter, value: &Value, field: &str, expected: &str) -> Result<()> {
    let actual = shape::text(m, value, field)?;
    if !shape::same_text(m, actual, expected)? {
        return Err(shape::fail("supplier projection literal"));
    }
    Ok(())
}
fn named<'a>(
    m: &mut CoreMeter,
    value: &'a Value,
    field: &str,
    name: &str,
    key: &str,
) -> Result<&'a Value> {
    let rows = shape::array(m, value, field)?;
    shape::named(m, rows, key, name)
}
fn load(
    m: &mut CoreMeter,
    records: &mut Records,
    key: &OperationKey,
    value: &Value,
    kind: &str,
    schema: &str,
) -> Result<super::record_body::RecordBody> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier input"))?;
    let reference = parse_owned_reference(m, &a, value)?;
    records.get(m, key, reference.reference(), kind, schema)
}
fn source_slot(
    m: &mut CoreMeter,
    a: &crate::canonical::audit_arena::AuditArena,
    binding: &Value,
    slots: &mut Values,
    name: &Value,
) -> Result<()> {
    let name = name
        .as_str()
        .ok_or_else(|| shape::fail("supplier used source slot"))?;
    let original = named(m, binding, "source_slots", name, "name")?;
    shape::closed(m, original, &["name", "sources", "evidence_kinds"], &[])?;
    let sources = shape::array(m, original, "sources")?;
    if sources.is_empty() || w::compare(m, sources.len(), 16usize)?.is_gt() {
        return Err(shape::fail("supplier nominee dimension"));
    }
    inventory::binding::ordered(m, sources)?;
    slots.unique(m, a, original)
}

/// The successor Offer and accepted platform leg consume the same exact new
/// evidence contract, retaining original root/subject/owner/time associations.
fn platform_offer_evidence(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    records: &mut Records,
    c: &DraftContextData,
    selected: (&Value, &Value, &Value),
) -> Result<()> {
    if !program.uses_platform_funding_evidence_recipe() {
        return Ok(());
    }
    let (binding, terms, offer) = selected;
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("platform Offer evidence owner"))?;
    let name = shape::text(m, terms, "funding_leg")?;
    let leg = named(m, binding, "funding_legs", name, "name")?;
    literal(m, leg, "mode", "platform")?;
    shape::equal(m, &leg["supplier_binding_slot"], &terms["slot"])?;
    let metadata = a.object_from_fields(
        &[
            ("mode", &leg["mode"]),
            ("resource_owner", &leg["resource_owner"]),
            ("payer", &leg["payer"]),
        ],
        m,
    )?;
    let subject = a.object_from_fields(
        &[
            ("customer", &binding["customer"]),
            ("agreement_id", &binding["agreement_id"]),
        ],
        m,
    )?;
    let funding = shape::array(m, leg, "evidence_refs")?;
    // Offer support is not constrained to the funding leg. Every named record
    // still joins its exact typed original provenance; retained2 additionally
    // repeats this new evidence recipe at the Offer's original use boundary.
    for value in shape::array(m, offer, "evidence_refs")? {
        let reference = parse_owned_reference(m, &a, value)?;
        let body = records.provenance(m, &c.key, reference.reference())?;
        let schema = shape::text(m, &body, "schema")?;
        if shape::same_text(m, schema, "ledger-workflow-retained-evidence/2")? {
            inventory::evidence::retained_platform(
                m,
                records,
                c,
                &super::representation::InvocationToken::new(),
                &body,
                (subject.value(), &body["metadata"], binding, "accepted_at"),
            )?;
        }
    }
    let token = super::representation::InvocationToken::new();
    for reference in funding {
        let evidence = load(
            m,
            records,
            &c.key,
            reference,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/2",
        )?;
        inventory::evidence::retained_platform(
            m,
            records,
            c,
            &token,
            &evidence,
            (subject.value(), metadata.value(), binding, "accepted_at"),
        )?;
    }
    Ok(())
}
/// Preview reads only the actual normalized association inputs used by the
/// commitment. Its primary supplier Terms/Offer refs are deliberately not read:
/// those documents depend on this commitment. Actual acceptance repeats this
/// derivation from complete original inputs and compares every resulting byte.
pub fn derive_local_supplier_commitment(
    program: &SelectedProgram,
    key: &OperationKey,
    binding_inputs: &Value,
    supplier_terms_projection: &Value,
    inputs: &[EconomicRecordView<'_>],
    session: &mut OriginalAuditSession,
) -> Result<LocalSupplierCommitment> {
    let result = (|| {
        session.check_cancelled()?;
        let a = session.arena().shared_handle();
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("supplier successor exact Stage recipe"));
        }
        let raw = session.meter_mut().canonical_owned_representation(
            &a,
            supplier_terms_projection,
            RepresentationKind::SourceDefinition,
        )?;
        let normalized =
            super::super::SlotBindingTerms::normalize_stage_supplier_projection_with_session(
                program,
                raw.bytes(),
                session,
            )?;
        let terms = session
            .parse_representation(normalized.bytes(), RepresentationKind::SourceDefinition)?;
        let m = session.meter_mut();
        let mut records = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        derive(m, &mut records, key, program, binding_inputs, terms.value())
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn derive(
    m: &mut CoreMeter,
    records: &mut Records,
    key: &OperationKey,
    program: &SelectedProgram,
    binding: &Value,
    terms: &Value,
) -> Result<LocalSupplierCommitment> {
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier commitment"))?;
    let slot = shape::text(m, terms, "slot")?;
    let policy = program.original_policy_value();
    let primary = named(m, policy, "binding_slots", slot, "name")?;
    literal(m, primary, "book", "supplier")?;
    literal(m, terms, "book", "supplier")?;
    let scope = super::work_keys::scope(m, key)?;
    shape::equal(m, &terms["scope"], scope.value())?;
    for field in ["customer", "agreement_id"] {
        shape::equal(m, &terms[field], &binding[field])?;
    }
    for field in ["book", "funding_leg", "correction_source_slot"] {
        shape::equal(m, &terms[field], &primary[field])?;
    }
    let product = owning_definition(m, &a, program.product_ref())?;
    let policy_ref = owning_definition(m, &a, program.root_policy_ref())?;
    if let Some(value) = binding.get("product_ref") {
        shape::equal(m, value, product.value())?;
    }
    if let Some(value) = binding.get("policy_ref") {
        shape::equal(m, value, policy_ref.value())?;
    }
    let compiled = owning_definition(m, &a, program.compiled_plan().reference())?;
    if let Some(value) = binding.get("compiled_plan_ref") {
        shape::equal(m, value, compiled.value())?;
    }
    // Preview supplies no primary binding Ref and no accepted-origin record.
    // These are only the exact code-derived source coordinates needed by the
    // shared catalog/slot DATA validator, never an accepted Binding capability.
    let binding_projection = WorkingValue::copy(m, binding)?.with_fields(
        m,
        &[
            ("product_ref", product.value()),
            ("policy_ref", policy_ref.value()),
            ("compiled_plan_ref", compiled.value()),
        ],
    )?;
    let binding: &Value = &binding_projection;
    let projection = supplier_projection::program_projection(m, &a, program, slot)?;
    let catalog = load(
        m,
        records,
        key,
        &binding["slot_catalog_ref"],
        "workflow-slot-catalog",
        "ledger-workflow-slot-catalog/1",
    )?;
    let inst = load(
        m,
        records,
        key,
        &binding["slot_instantiation_ref"],
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let context = shape::field(m, binding, "accepted_context")?;
    let context_ref = owning_definition(m, &a, program.context_schema_ref())?;
    shape::equal(m, &context["fact_schema_ref"], context_ref.value())?;
    let mut schema = None;
    for (reference, definition) in program.fact_schemas() {
        if reference == program.context_schema_ref() {
            schema = Some(definition);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("supplier original context schema"))?;
    let normalized = NormalizedInput::normalize(m, schema, &context["facts"])?;
    shape::equal(m, normalized.canonical_value(), &context["facts"])?;
    let nodes = shape::array(m, projection.value(), "nodes")?;
    let mut additional = Values::new(m, &a, 15)?;
    let mut sources = Values::new(m, &a, 32)?;
    let mut components = Values::new(m, &a, 128)?;
    let mut context_fields = Values::new(m, &a, 64)?;
    for node in nodes {
        for field in ["facts", "binding_facts"] {
            for operand in shape::array(m, node, field)? {
                if field == "binding_facts" {
                    context_fields.unique(m, &a, &operand["field"]["name"])?;
                }
            }
        }
        let kind = shape::text(m, node, "kind")?;
        if shape::same_text(m, kind, "stage")? {
            let stage = &node["stage"];
            for field in ["authorization_source_slot", "closure_source_slot"] {
                source_slot(m, &a, binding, &mut sources, &stage[field])?;
            }
            for expected in shape::array(m, stage, "expected_slots")? {
                source_slot(m, &a, binding, &mut sources, &expected["source_slot"])?;
            }
            continue;
        }
        let rule = if shape::same_text(m, kind, "rule")? {
            &node["rule"]
        } else {
            &node["family"]
        };
        let original_slot = if kind == "rule" {
            &rule["binding_slot"]
        } else {
            &rule["selector"]["binding_slot"]
        };
        let name = original_slot
            .as_str()
            .ok_or_else(|| shape::fail("supplier selected binding slot"))?;
        let definition = named(m, policy, "binding_slots", name, "name")?;
        source_slot(
            m,
            &a,
            binding,
            &mut sources,
            &definition["correction_source_slot"],
        )?;
        if !shape::same_text(m, name, slot)? {
            literal(m, definition, "book", "retail")?;
            let actual = named(m, binding, "bindings", name, "slot")?;
            let retail = load(
                m,
                records,
                key,
                &actual["binding_ref"],
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/3",
            )?;
            let role = load(
                m,
                records,
                key,
                &retail["role_ref"],
                "workflow-roles",
                "ledger-workflow-role-assignment/1",
            )?;
            inventory::slots::validate_original_projection(
                m, program, binding, &catalog, &inst, &retail, &role,
            )?;
            let leg = shape::text(m, &retail, "funding_leg")?;
            let leg = named(m, binding, "funding_legs", leg, "name")?;
            let row = a.object_from_fields(
                &[
                    ("terms", &retail),
                    ("binding_slot", definition),
                    ("funding_leg", leg),
                ],
                m,
            )?;
            additional.unique(m, &a, row.value())?;
        }
    }
    for activity in shape::array(m, projection.value(), "activities")? {
        source_slot(m, &a, binding, &mut sources, &activity["source_slot"])?;
        for (reference, schema) in program.fact_schemas() {
            let reference = owning_definition(m, &a, reference)?;
            if !shape::same(m, reference.value(), &activity["fact_schema_ref"])? {
                continue;
            }
            for field in shape::array(m, schema.canonical_value(), "fields")? {
                if field["provenance"]["kind"].as_str() == Some("attested") {
                    source_slot(
                        m,
                        &a,
                        binding,
                        &mut sources,
                        &field["provenance"]["source_slot"],
                    )?;
                }
            }
        }
    }
    for component in shape::array(m, projection.value(), "components")? {
        let name = shape::text(m, component, "key")?;
        let mapping = named(m, &inst, "component_map", name, "template_key")?;
        let slot_name = shape::text(m, mapping, "slot_id")?;
        let actual = named(m, &catalog, "components", slot_name, "slot_id")?;
        let root_name = shape::text(m, component, "root_key")?;
        let root = named(m, &inst, "component_map", root_name, "template_key")?;
        shape::equal(m, &actual["root_slot"], &root["slot_id"])?;
        shape::equal(m, &actual["role_slot"], &component["role_slot"])?;
        let row = a.object_from_fields(
            &[
                ("component_key", &component["key"]),
                ("component_slot", &mapping["slot_id"]),
                ("root_slot", &actual["root_slot"]),
                ("role_slot", &actual["role_slot"]),
            ],
            m,
        )?;
        components.unique(m, &a, row.value())?;
    }
    let mut fields = a.object_from_fields(&[], m)?;
    for field in context_fields.values() {
        let name = field
            .value()
            .as_str()
            .ok_or_else(|| shape::fail("supplier context field name"))?;
        if let Some(value) = context["facts"].get(name) {
            fields = WorkingValue::Owned(fields)
                .with_fields(m, &[(name, value)])?
                .into_owned()?;
        }
    }
    let context = a.object_from_fields(
        &[
            ("fact_schema_ref", context_ref.value()),
            ("facts", fields.value()),
        ],
        m,
    )?;
    // additional_bindings is keyed by its actual slot, rather than whole-row JCS.
    let additional = additional.into_keyed(m, &a, "binding_slot", "name")?;
    let additional = additional.finish(m, &a, false)?;
    let sources = sources.finish(m, &a, true)?;
    let components = components.finish(m, &a, true)?;
    let funding_name = shape::text(m, terms, "funding_leg")?;
    let funding = named(m, binding, "funding_legs", funding_name, "name")?;
    literal(m, funding, "mode", "platform")?;
    shape::equal(m, &funding["supplier_binding_slot"], &terms["slot"])?;
    let acceptance = a.object_from_fields(
        &[
            ("terms", terms),
            ("binding_slot", primary),
            ("funding_leg", funding),
            ("source_slots", sources.value()),
            ("accepted_context", context.value()),
            ("component_bindings", components.value()),
            ("starts_at", &binding["starts_at"]),
            ("additional_bindings", additional.value()),
        ],
        m,
    )?;
    let acceptance = if let Some(end) = binding.get("ends_at") {
        WorkingValue::Owned(acceptance)
            .with_fields(m, &[("ends_at", end)])?
            .into_owned()?
    } else {
        acceptance
    };
    let constants = m.parse_owned_representation(&a, br#"{"schema":"ledger-workflow-supplier-commitment/2","recipe":"supplier-semantic-projection/2"}"#, RepresentationKind::SourceDefinition)?;
    let value = a.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("scope", scope.value()),
            ("customer", &binding["customer"]),
            ("agreement_id", &binding["agreement_id"]),
            ("supplier_slot", &terms["slot"]),
            ("projection_recipe", &constants.value()["recipe"]),
            ("program", projection.value()),
            ("acceptance", acceptance.value()),
        ],
        m,
    )?;
    let owner =
        a.reserve_external_payload(mem::size_of::<LocalSupplierCommitment>() + 14 + 69 + 71, m)?;
    let record = body::document_borrowed(m, "workflow-offer", value.value())?;
    Ok(LocalSupplierCommitment {
        record,
        _ownership: owner,
    })
}

pub struct LocalSupplierAssent {
    record: ConstructedEnvelope,
    _ownership: ExternalBufferLease,
}
impl LocalSupplierAssent {
    pub fn reference(&self) -> &ExactRef {
        &self.record.reference
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.record.canonical_body
    }
    pub fn canonical_value(&self) -> &Value {
        self.record
            .decoded
            .as_ref()
            .expect("owning supplier assent frame")
    }
}
/// Closed assertion DATA only. Evidence3, actual versioned triple and original/
/// current provider authorization are checked independently by the verifier.
pub fn normalize_local_supplier_assent_with_session(
    program: &SelectedProgram,
    value: &Value,
    session: &mut OriginalAuditSession,
) -> Result<LocalSupplierAssent> {
    let result = (|| {
        session.check_cancelled()?;
        let a = session.arena().shared_handle();
        let m = session.meter_mut();
        program.require_attempt_owner(m)?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("supplier assent exact successor recipe"));
        }
        super::super::facts::reject_null_with_work(value, m)?;
        shape::closed(
            m,
            value,
            &[
                "schema",
                "consent",
                "mode",
                "evidence_ref",
                "operator",
                "operator_attestation_ref",
            ],
            &[],
        )?;
        literal(m, value, "schema", "ledger-workflow-party-assent/3")?;
        let consent = shape::field(m, value, "consent")?;
        shape::closed(
            m,
            consent,
            &[
                "scope",
                "customer",
                "agreement_id",
                "party",
                "acceptor",
                "roles",
                "authorization_ref",
                "basis",
                "accepted_at",
            ],
            &[],
        )?;
        let scope = shape::array(m, consent, "scope")?;
        if !w::compare(m, scope.len(), 2usize)?.is_eq() {
            return Err(shape::fail("supplier assent scope"));
        }
        for value in scope {
            inventory::binding::id(m, value, 128)?;
        }
        for field in ["customer", "agreement_id", "party", "acceptor"] {
            inventory::binding::id(m, &consent[field], 128)?;
        }
        inventory::binding::id(m, &value["operator"], 128)?;
        let mode = shape::text(m, value, "mode")?;
        if !shape::same_text(m, mode, "real")? && !shape::same_text(m, mode, "synthetic")? {
            return Err(shape::fail("supplier assent mode"));
        }
        let roles = shape::array(m, consent, "roles")?;
        if !w::compare(m, roles.len(), 1usize)?.is_eq() || roles[0].as_str() != Some("provider") {
            return Err(shape::fail("supplier assent provider-only role"));
        }
        let basis = shape::field(m, consent, "basis")?;
        shape::closed(
            m,
            basis,
            &["kind", "commitment_ref", "offer_ref", "supplier_terms_ref"],
            &[],
        )?;
        literal(m, basis, "kind", "supplier_commitment")?;
        for (owner, field, kind) in [
            (consent, "authorization_ref", "workflow-authority"),
            (basis, "commitment_ref", "workflow-offer"),
            (basis, "offer_ref", "workflow-offer"),
            (basis, "supplier_terms_ref", "workflow-slot-terms"),
            (value, "evidence_ref", "workflow-evidence"),
            (value, "operator_attestation_ref", "workflow-evidence"),
        ] {
            let reference = parse_owned_reference(m, &a, &owner[field])?;
            if !shape::same_text(m, &reference.reference().kind, kind)? {
                return Err(shape::fail("supplier assent typed Ref"));
            }
        }
        let raw = shape::text(m, consent, "accepted_at")?;
        let at = crate::domain::Timestamp::parse_with_work(raw, m)?;
        let at = a.string_value(at.as_str(), m)?;
        let consent =
            WorkingValue::copy(m, consent)?.with_fields(m, &[("accepted_at", at.value())])?;
        let value = WorkingValue::copy(m, value)?.with_fields(m, &[("consent", &consent)])?;
        let owner =
            a.reserve_external_payload(mem::size_of::<LocalSupplierAssent>() + 15 + 69 + 71, m)?;
        let record = body::document_borrowed(m, "workflow-assent", &value)?;
        Ok(LocalSupplierAssent {
            record,
            _ownership: owner,
        })
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

/// Actual candidate/retained association DATA. Pending normalized provider
/// assents are supplied in inputs along with retained evidence and mappings;
/// the returned reference list does not classify them as historical inputs.
pub struct SupplierConsentInputs<'a> {
    pub publication_ref: &'a ExactRef,
    pub acceptance_terms_ref: &'a ExactRef,
    pub registered_customer_ref: &'a ExactRef,
    pub local_admin_ref: &'a ExactRef,
    pub binding: &'a Value,
    pub inputs: &'a [EconomicRecordView<'a>],
    pub current_party_authorizations: &'a [PartyAuthorizationHead],
}
pub struct LocalSupplierConsentVerifications {
    records: Vec<ConstructedEnvelope>,
    consumed: super::records::OwnedConsumed,
    _ownership: ExternalBufferLease,
}
impl LocalSupplierConsentVerifications {
    pub fn records(&self) -> impl ExactSizeIterator<Item = EconomicRecordView<'_>> {
        self.records.iter().map(|r| EconomicRecordView {
            reference: &r.reference,
            canonical_body: &r.canonical_body,
        })
    }
    pub fn referenced_inputs(&self) -> &[ExactRef] {
        &self.consumed.references
    }
}
/// Derive the complete selected supplier set from the actual program. Each
/// provider asserts the newly derived successor triple; old consent is never
/// upgraded by equality of provider, prices, or an old program projection.
pub fn derive_local_supplier_consent_verifications(
    program: &SelectedProgram,
    context: &DraftContextData,
    input: &SupplierConsentInputs<'_>,
    session: &mut OriginalAuditSession,
) -> Result<LocalSupplierConsentVerifications> {
    let result = (|| {
        session.check_cancelled()?;
        let a = session.arena().shared_handle();
        let m = session.meter_mut();
        program.require_attempt_owner(m)?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("supplier verifier exact successor recipe"));
        }
        let key = &context.key;
        let mut records = Records::borrowed_parent_selected(m, &a, key, input.inputs, program)?;
        let owner = a.reserve_external_payload(
            mem::size_of::<LocalSupplierConsentVerifications>()
                + 16 * mem::size_of::<ConstructedEnvelope>(),
            m,
        )?;
        let mut outputs = Vec::new();
        outputs
            .try_reserve_exact(16)
            .map_err(|_| shape::fail("supplier verification vector allocation"))?;
        let mut slots = Values::new(m, &a, 16)?;
        for rule in shape::array(m, program.original_policy_value(), "rules")? {
            let name = shape::text(m, rule, "binding_slot")?;
            let slot = named(
                m,
                program.original_policy_value(),
                "binding_slots",
                name,
                "name",
            )?;
            let book = shape::text(m, slot, "book")?;
            if shape::same_text(m, book, "supplier")? {
                slots.unique(m, &a, &slot["name"])?;
            }
        }
        if slots.values().is_empty() {
            for value in shape::array(m, input.binding, "assent_refs")? {
                let reference = parse_owned_reference(m, &a, value)?;
                let assent = records.get_schemas(
                    m,
                    key,
                    reference.reference(),
                    "workflow-assent",
                    &[
                        "ledger-workflow-assent/1",
                        "ledger-workflow-party-assent/2",
                        "ledger-workflow-party-assent/3",
                    ],
                )?;
                if assent["schema"].as_str() == Some("ledger-workflow-party-assent/3") {
                    return Err(shape::fail(
                        "supplier assent without any selected supplier obligation",
                    ));
                }
            }
            return Ok(LocalSupplierConsentVerifications {
                records: outputs,
                consumed: records.into_consumed_owned(),
                _ownership: owner,
            });
        }
        let binding = input.binding;
        let accepted_terms = records.get(
            m,
            key,
            input.acceptance_terms_ref,
            "workflow-acceptance-terms",
            "ledger-workflow-acceptance-terms/2",
        )?;
        inventory::binding::validate_stage(program, m, binding, &accepted_terms)?;
        let registered = records.get(
            m,
            key,
            input.registered_customer_ref,
            "workflow-customer",
            "ledger-workflow-customer-registration/1",
        )?;
        let customer = shape::text(m, binding, "customer")?;
        literal(m, &registered, "customer", customer)?;
        let pub_ref = super::working_value::owning_reference(m, &a, input.publication_ref)?;
        let terms_ref = super::working_value::owning_reference(m, &a, input.acceptance_terms_ref)?;
        let original = a.object_from_fields(&[("publication_ref", pub_ref.value())], m)?;
        inventory::publication::validate(
            m,
            &mut records,
            context,
            &super::representation::InvocationToken::new(),
            program,
            binding,
            (original.value(), &accepted_terms),
        )?;
        let terms_bytes = records.canonical(m, input.acceptance_terms_ref)?;
        let digest =
            super::records::hash_parts(m, "workflow-acceptance-terms", &[terms_bytes], false)?;
        let digest = a.string_value(&digest, m)?;
        let accepted = a.object_from_fields(&[("terms_digest", digest.value())], m)?;
        super::app_cost::consent::original(
            program,
            m,
            &mut records,
            context,
            binding,
            accepted.value(),
            (input.acceptance_terms_ref, &registered),
        )?;
        super::app_cost::consent::current_fullterms_heads(
            program,
            m,
            &mut records,
            context,
            binding,
            accepted.value(),
            input.current_party_authorizations,
        )?;
        let actor = a.string_value(&context.actor, m)?;
        let at = a.string_value(&context.accepted_at, m)?;
        let mode = a.string_value(&context.installation_mode, m)?;
        let root_ref = super::working_value::owning_reference(m, &a, input.local_admin_ref)?;
        let snapshot = a.object_from_fields(&[("verified_at", at.value())], m)?;
        super::inventory::evidence::root(
            m,
            &mut records,
            context,
            &super::representation::InvocationToken::new(),
            root_ref.value(),
            snapshot.value(),
            ("verified_at", actor.value(), None),
        )?;
        let scope = super::work_keys::scope(m, key)?;
        let plan = owning_definition(m, &a, program.compiled_plan().reference())?;
        let constants = m.parse_owned_representation(&a, br#"{"schema":"ledger-workflow-supplier-consent-verification/2","recipe":"supplier-semantic-projection/2","kind":"supplier_commitment"}"#, RepresentationKind::SourceDefinition)?;
        let mut checked_provider_assents = Values::new(m, &a, 16)?;
        for slot in slots.values() {
            let slot_name = slot
                .value()
                .as_str()
                .ok_or_else(|| shape::fail("supplier selected slot string"))?;
            let row = named(m, binding, "bindings", slot_name, "slot")?;
            let supplier_ref = parse_owned_reference(m, &a, &row["binding_ref"])?;
            let supplier = records.get(
                m,
                key,
                supplier_ref.reference(),
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/8",
            )?;
            shape::closed(
                m,
                &supplier,
                &[
                    "schema",
                    "scope",
                    "agreement_id",
                    "customer",
                    "slot",
                    "book",
                    "role_slot",
                    "role_ref",
                    "funding_leg",
                    "allowed_activities",
                    "quantity_ceilings",
                    "exposure_ceiling",
                    "source_grant_refs",
                    "correction_source_slot",
                    "offer_ref",
                    "cost_authority_ref",
                    "provider_account_identity",
                    "consumption_basis",
                    "commitment_ref",
                ],
                &[],
            )?;
            literal(m, &supplier, "slot", slot_name)?;
            let primary = named(
                m,
                program.original_policy_value(),
                "binding_slots",
                slot_name,
                "name",
            )?;
            for field in ["book", "funding_leg", "correction_source_slot"] {
                shape::equal(m, &supplier[field], &primary[field])?;
            }
            for field in ["scope", "customer", "agreement_id"] {
                let expected = if field == "scope" {
                    scope.value()
                } else {
                    &binding[field]
                };
                shape::equal(m, &supplier[field], expected)?;
            }
            let projection = a.object_from_fields(
                &[
                    ("scope", &supplier["scope"]),
                    ("agreement_id", &supplier["agreement_id"]),
                    ("customer", &supplier["customer"]),
                    ("slot", &supplier["slot"]),
                    ("book", &supplier["book"]),
                    ("role_slot", &supplier["role_slot"]),
                    ("role_ref", &supplier["role_ref"]),
                    ("funding_leg", &supplier["funding_leg"]),
                    ("allowed_activities", &supplier["allowed_activities"]),
                    ("quantity_ceilings", &supplier["quantity_ceilings"]),
                    ("exposure_ceiling", &supplier["exposure_ceiling"]),
                    ("source_grant_refs", &supplier["source_grant_refs"]),
                    (
                        "correction_source_slot",
                        &supplier["correction_source_slot"],
                    ),
                    ("cost_authority_ref", &supplier["cost_authority_ref"]),
                    (
                        "provider_account_identity",
                        &supplier["provider_account_identity"],
                    ),
                    ("consumption_basis", &supplier["consumption_basis"]),
                ],
                m,
            )?;
            let derived = derive(m, &mut records, key, program, binding, projection.value())?;
            let commitment_ref = parse_owned_reference(m, &a, &supplier["commitment_ref"])?;
            if !shape::same_ref(m, commitment_ref.reference(), derived.reference())? {
                return Err(shape::fail(
                    "supplier commitment differs from actual complete dependency closure",
                ));
            }
            let committed = records.get(
                m,
                key,
                commitment_ref.reference(),
                "workflow-offer",
                "ledger-workflow-supplier-commitment/2",
            )?;
            shape::equal(m, &committed, derived.canonical_value())?;
            let offer = load(
                m,
                &mut records,
                key,
                &supplier["offer_ref"],
                "workflow-offer",
                "ledger-workflow-offer/4",
            )?;
            shape::closed(
                m,
                &offer,
                &[
                    "schema",
                    "agreement_id",
                    "version",
                    "provider",
                    "valuation_mode",
                    "role_refs",
                    "activity_names",
                    "unit_ceilings",
                    "exposure_ceiling",
                    "allowed_component_slots",
                    "source_slots",
                    "starts_at",
                    "ends_at",
                    "evidence_refs",
                    "commitment_ref",
                ],
                &[],
            )?;
            shape::equal(m, &offer["commitment_ref"], &supplier["commitment_ref"])?;
            platform_offer_evidence(
                program,
                m,
                &mut records,
                context,
                (binding, &supplier, &offer),
            )?;
            for field in ["agreement_id", "version"] {
                shape::equal(m, &offer[field], &binding[field])?;
            }
            let role = load(
                m,
                &mut records,
                key,
                &supplier["role_ref"],
                "workflow-roles",
                "ledger-workflow-role-assignment/1",
            )?;
            for field in ["scope", "agreement_id", "customer", "role_slot"] {
                shape::equal(m, &role[field], &supplier[field])?;
            }
            shape::equal(m, &offer["provider"], &role["provider"])?;
            let roles = shape::array(m, binding, "roles_refs")?;
            if !shape::member(m, roles, &supplier["role_ref"])? {
                return Err(shape::fail("supplier original role absent Binding"));
            }
            let roles = shape::array(m, &offer, "role_refs")?;
            if !shape::member(m, roles, &supplier["role_ref"])? {
                return Err(shape::fail("supplier role absent Offer"));
            }
            let basis = a.object_from_fields(
                &[
                    ("kind", &constants.value()["kind"]),
                    ("commitment_ref", &supplier["commitment_ref"]),
                    ("offer_ref", &supplier["offer_ref"]),
                    ("supplier_terms_ref", &row["binding_ref"]),
                ],
                m,
            )?;
            let mut customer = Values::new(m, &a, 16)?;
            let mut parties = Values::new(m, &a, 16)?;
            let mut originals = Values::new(m, &a, 16)?;
            let mut currents = Values::new(m, &a, 16)?;
            let mut provider_count = 0usize;
            for assent_ref in shape::array(m, binding, "assent_refs")? {
                let reference = parse_owned_reference(m, &a, assent_ref)?;
                let assent = records.get_schemas(
                    m,
                    key,
                    reference.reference(),
                    "workflow-assent",
                    &[
                        "ledger-workflow-assent/1",
                        "ledger-workflow-party-assent/2",
                        "ledger-workflow-party-assent/3",
                    ],
                )?;
                let schema = shape::text(m, &assent, "schema")?;
                if shape::same_text(m, schema, "ledger-workflow-assent/1")? {
                    customer.unique(m, &a, assent_ref)?;
                    continue;
                }
                let consent = &assent["consent"];
                let provider = shape::same_text(m, schema, "ledger-workflow-party-assent/3")?;
                if provider && !shape::same(m, &consent["basis"], basis.value())? {
                    continue;
                }
                let original_auth = if provider {
                    shape::equal(m, &consent["party"], &offer["provider"])?;
                    provider_count = w::usize_add(m, provider_count, 1)?;
                    checked_provider_assents.unique(m, &a, assent_ref)?;
                    super::app_cost::consent::supplier_original(
                        m,
                        &mut records,
                        context,
                        binding,
                        accepted.value(),
                        (input.acceptance_terms_ref, &assent, basis.value()),
                    )?
                } else {
                    load(
                        m,
                        &mut records,
                        key,
                        &consent["authorization_ref"],
                        "workflow-authority",
                        "ledger-workflow-party-authorization/1",
                    )?
                };
                let current = super::app_cost::consent::supplier_current(
                    m,
                    &mut records,
                    context,
                    binding,
                    consent,
                    &original_auth,
                    input.current_party_authorizations,
                )?;
                let current = super::working_value::owning_reference(m, &a, &current)?;
                parties.unique(m, &a, assent_ref)?;
                originals.unique(m, &a, &consent["authorization_ref"])?;
                currents.unique(m, &a, current.value())?;
            }
            if !w::compare(m, provider_count, 1usize)?.is_eq() {
                return Err(shape::fail(
                    "exact fresh successor provider assent missing or duplicated",
                ));
            }
            let customer = customer.finish(m, &a, true)?;
            let parties = parties.finish(m, &a, true)?;
            let originals = originals.finish(m, &a, true)?;
            let currents = currents.finish(m, &a, true)?;
            let value = a.object_from_fields(
                &[
                    ("schema", &constants.value()["schema"]),
                    ("scope", scope.value()),
                    ("customer", &binding["customer"]),
                    ("agreement_id", &binding["agreement_id"]),
                    ("publication_ref", pub_ref.value()),
                    ("compiled_plan_ref", plan.value()),
                    ("commitment_ref", &supplier["commitment_ref"]),
                    ("offer_ref", &supplier["offer_ref"]),
                    ("supplier_terms_ref", &row["binding_ref"]),
                    ("acceptance_terms_ref", terms_ref.value()),
                    ("customer_assent_refs", customer.value()),
                    ("party_assent_refs", parties.value()),
                    ("original_party_authorization_refs", originals.value()),
                    ("current_party_authorization_refs", currents.value()),
                    ("projection_recipe", &constants.value()["recipe"]),
                    ("verified_by", actor.value()),
                    ("verified_at", at.value()),
                    ("local_admin_ref", root_ref.value()),
                    ("installation_mode", mode.value()),
                ],
                m,
            )?;
            outputs.push(body::document_borrowed(
                m,
                "workflow-verification",
                value.value(),
            )?);
        }
        for reference in shape::array(m, binding, "assent_refs")? {
            let parsed = parse_owned_reference(m, &a, reference)?;
            let assent = records.get_schemas(
                m,
                key,
                parsed.reference(),
                "workflow-assent",
                &[
                    "ledger-workflow-assent/1",
                    "ledger-workflow-party-assent/2",
                    "ledger-workflow-party-assent/3",
                ],
            )?;
            if assent["schema"].as_str() == Some("ledger-workflow-party-assent/3") {
                let mut found = false;
                for checked in checked_provider_assents.values() {
                    if shape::same(m, checked.value(), reference)? {
                        found = true;
                    }
                }
                if !found {
                    return Err(shape::fail(
                        "unrelated successor provider assent in actual Binding",
                    ));
                }
            }
        }
        Ok(LocalSupplierConsentVerifications {
            records: outputs,
            consumed: records.into_consumed_owned(),
            _ownership: owner,
        })
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

/// Repeat the accepted successor association at actual consumption, with
/// independent held current provider mappings. Old verification current refs
/// remain historical operands and never replace these current heads.
pub(super) fn current(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    binding: &Value,
    accepted: &Value,
    heads: &[PartyAuthorizationHead],
) -> Result<()> {
    validate_retained(program, m, r, c, binding, accepted, Some(heads))
}
pub(super) fn original(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    binding: &Value,
    accepted: &Value,
) -> Result<()> {
    validate_retained(program, m, r, c, binding, accepted, None)
}
fn validate_retained(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    binding: &Value,
    accepted: &Value,
    heads: Option<&[PartyAuthorizationHead]>,
) -> Result<()> {
    if !program.uses_stage_lifecycle_recipe() {
        return Ok(());
    }
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier current consent"))?;
    let authority = load(
        m,
        r,
        &c.key,
        &accepted["authority_observation_ref"],
        "workflow-authority",
        "ledger-workflow-agreement-authority/6",
    )?;
    let verification_refs = shape::array(m, &authority, "supplier_consent_verification_refs")?;
    let terms_ref = parse_owned_reference(m, &a, &accepted["terms_ref"])?;
    let mut suppliers = 0usize;
    let mut checked = Values::new(m, &a, 16)?;
    let policy = program.original_policy_value();
    for slot in shape::array(m, policy, "binding_slots")? {
        let book = shape::text(m, slot, "book")?;
        if !shape::same_text(m, book, "supplier")? {
            continue;
        }
        let name = shape::text(m, slot, "name")?;
        let mut used = false;
        for rule in shape::array(m, policy, "rules")? {
            if shape::same(m, &rule["binding_slot"], &slot["name"])? {
                used = true;
            }
        }
        if !used {
            continue;
        }
        suppliers = w::usize_add(m, suppliers, 1)?;
        let row = named(m, binding, "bindings", name, "slot")?;
        let supplier = load(
            m,
            r,
            &c.key,
            &row["binding_ref"],
            "workflow-slot-terms",
            "ledger-workflow-slot-binding-terms/8",
        )?;
        let projection = a.object_from_fields(
            &[
                ("scope", &supplier["scope"]),
                ("agreement_id", &supplier["agreement_id"]),
                ("customer", &supplier["customer"]),
                ("slot", &supplier["slot"]),
                ("book", &supplier["book"]),
                ("role_slot", &supplier["role_slot"]),
                ("role_ref", &supplier["role_ref"]),
                ("funding_leg", &supplier["funding_leg"]),
                ("allowed_activities", &supplier["allowed_activities"]),
                ("quantity_ceilings", &supplier["quantity_ceilings"]),
                ("exposure_ceiling", &supplier["exposure_ceiling"]),
                ("source_grant_refs", &supplier["source_grant_refs"]),
                (
                    "correction_source_slot",
                    &supplier["correction_source_slot"],
                ),
                ("cost_authority_ref", &supplier["cost_authority_ref"]),
                (
                    "provider_account_identity",
                    &supplier["provider_account_identity"],
                ),
                ("consumption_basis", &supplier["consumption_basis"]),
            ],
            m,
        )?;
        let derived = derive(m, r, &c.key, program, binding, projection.value())?;
        let declared = parse_owned_reference(m, &a, &supplier["commitment_ref"])?;
        if !shape::same_ref(m, declared.reference(), derived.reference())? {
            return Err(shape::fail(
                "current supplier accepted commitment dependency closure",
            ));
        }
        let commitment = r.get(
            m,
            &c.key,
            declared.reference(),
            "workflow-offer",
            "ledger-workflow-supplier-commitment/2",
        )?;
        shape::equal(m, &commitment, derived.canonical_value())?;
        let offer = load(
            m,
            r,
            &c.key,
            &supplier["offer_ref"],
            "workflow-offer",
            "ledger-workflow-offer/4",
        )?;
        shape::equal(m, &offer["commitment_ref"], &supplier["commitment_ref"])?;
        platform_offer_evidence(program, m, r, c, (binding, &supplier, &offer))?;
        let kind = a.string_value("supplier_commitment", m)?;
        let basis = a.object_from_fields(
            &[
                ("kind", kind.value()),
                ("commitment_ref", &supplier["commitment_ref"]),
                ("offer_ref", &supplier["offer_ref"]),
                ("supplier_terms_ref", &row["binding_ref"]),
            ],
            m,
        )?;
        let mut customer = Values::new(m, &a, 16)?;
        let mut parties = Values::new(m, &a, 16)?;
        let mut originals = Values::new(m, &a, 16)?;
        let mut provider_count = 0usize;
        for value in shape::array(m, binding, "assent_refs")? {
            let reference = parse_owned_reference(m, &a, value)?;
            let assent = r.get_schemas(
                m,
                &c.key,
                reference.reference(),
                "workflow-assent",
                &[
                    "ledger-workflow-assent/1",
                    "ledger-workflow-party-assent/2",
                    "ledger-workflow-party-assent/3",
                ],
            )?;
            let schema = shape::text(m, &assent, "schema")?;
            if shape::same_text(m, schema, "ledger-workflow-assent/1")? {
                customer.unique(m, &a, value)?;
                continue;
            }
            if shape::same_text(m, schema, "ledger-workflow-party-assent/3")? {
                if !shape::same(m, &assent["consent"]["basis"], basis.value())? {
                    continue;
                }
                shape::equal(m, &assent["consent"]["party"], &offer["provider"])?;
                let original = super::app_cost::consent::supplier_original(
                    m,
                    r,
                    c,
                    binding,
                    accepted,
                    (terms_ref.reference(), &assent, basis.value()),
                )?;
                if let Some(heads) = heads {
                    super::app_cost::consent::supplier_current(
                        m,
                        r,
                        c,
                        binding,
                        &assent["consent"],
                        &original,
                        heads,
                    )?;
                }
                checked.unique(m, &a, value)?;
                provider_count = w::usize_add(m, provider_count, 1)?;
            }
            parties.unique(m, &a, value)?;
            originals.unique(m, &a, &assent["consent"]["authorization_ref"])?;
        }
        if !w::compare(m, provider_count, 1usize)?.is_eq() {
            return Err(shape::fail(
                "actual current successor provider assent missing or duplicate",
            ));
        }
        let customer = customer.finish(m, &a, true)?;
        let parties = parties.finish(m, &a, true)?;
        let originals = originals.finish(m, &a, true)?;
        let mut found = false;
        for value in verification_refs {
            let verification = load(
                m,
                r,
                &c.key,
                value,
                "workflow-verification",
                "ledger-workflow-supplier-consent-verification/2",
            )?;
            if !shape::same(m, &verification["supplier_terms_ref"], &row["binding_ref"])? {
                continue;
            }
            if found {
                return Err(shape::fail("duplicate original supplier verification"));
            }
            found = true;
            for (field, want) in [
                ("customer", &binding["customer"]),
                ("agreement_id", &binding["agreement_id"]),
                ("publication_ref", &accepted["publication_ref"]),
                ("compiled_plan_ref", &binding["compiled_plan_ref"]),
                ("acceptance_terms_ref", &accepted["terms_ref"]),
                ("commitment_ref", &supplier["commitment_ref"]),
                ("offer_ref", &supplier["offer_ref"]),
                ("customer_assent_refs", customer.value()),
                ("party_assent_refs", parties.value()),
                ("original_party_authorization_refs", originals.value()),
                ("verified_at", &authority["observed_at"]),
                ("verified_by", &authority["actor"]),
                ("local_admin_ref", &authority["local_admin_ref"]),
            ] {
                shape::equal(m, &verification[field], want)?;
            }
            literal(
                m,
                &verification,
                "projection_recipe",
                "supplier-semantic-projection/2",
            )?;
        }
        if !found {
            return Err(shape::fail(
                "actual original supplier consent verification absent",
            ));
        }
    }
    if !w::compare(m, suppliers, verification_refs.len())?.is_eq() {
        return Err(shape::fail(
            "unknown or missing supplier consent verification",
        ));
    }
    for value in shape::array(m, binding, "assent_refs")? {
        let reference = parse_owned_reference(m, &a, value)?;
        let assent = r.get_schemas(
            m,
            &c.key,
            reference.reference(),
            "workflow-assent",
            &[
                "ledger-workflow-assent/1",
                "ledger-workflow-party-assent/2",
                "ledger-workflow-party-assent/3",
            ],
        )?;
        if assent["schema"].as_str() != Some("ledger-workflow-party-assent/3") {
            continue;
        }
        let mut found = false;
        for used in checked.values() {
            if shape::same(m, used.value(), value)? {
                found = true;
            }
        }
        if !found {
            return Err(shape::fail(
                "unrelated provider assent on actual Stage binding",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canonical::{audit_arena::AuditArena, CanonicalBytes},
        policy::workflow::{
            selected_program::{
                compile_selected_publication, load_local_selected_execution,
                SelectedCompilerSupport, SelectedSourceInput,
            },
            SlotBindingTerms,
        },
        validation_work::ResourceLimits,
    };
    use serde_json::json;
    fn session(a: &AuditArena) -> OriginalAuditSession {
        OriginalAuditSession::for_resource_profile_v2(a).unwrap()
    }
    fn source(support: &SelectedCompilerSupport) -> SelectedSourceInput {
        let source: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-stage/source.json"
        ))
        .unwrap();
        let mut documents: Vec<_> = source["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| CanonicalBytes::from_value(d).unwrap().into_vec())
            .collect();
        documents.push(support.canonical_bytes().to_vec());
        SelectedSourceInput {
            documents,
            root_policy_ref: serde_json::from_value(source["root_policy_ref"].clone()).unwrap(),
        }
    }
    #[test]
    fn stage_supplier_commitment_selects_actual_retail_dependencies_and_new_provider_basis() {
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut s = session(&arena);
        let support =
            SelectedCompilerSupport::local_stage_lifecycle_execution_with_session(&mut s).unwrap();
        if let Ok(path) = std::env::var("WAVE_STAGE_AUTHORING_OUTPUT") {
            let raw: Value = serde_json::from_slice(include_bytes!(
                "../../../../../../examples/workflow-stage/source.json"
            ))
            .unwrap();
            let value = crate::policy::workflow::selected_program::stage_fixture_normalized_source(
                &raw, &support, &mut s,
            )
            .unwrap();
            std::fs::write(path, CanonicalBytes::from_value(&value).unwrap().into_vec()).unwrap();
            eprintln!("AUTHORING DATA ONLY: emitted normalized source closure; no semantic vector or execution qualification");
            return;
        }
        let p = compile_selected_publication(source(&support), &support, &mut s).unwrap();
        p.require_local_admission_with_session(&mut s).unwrap();
        let mut reload = session(&arena);
        let restored = load_local_selected_execution(
            source(&support),
            p.compiled_plan().canonical_bytes(),
            p.compiled_plan().reference(),
            &mut reload,
        )
        .unwrap();
        assert_eq!(
            p.compiled_plan().canonical_bytes(),
            restored.compiled_plan().canonical_bytes()
        );
        assert_eq!(
            p.compiled_plan().canonical_value()["typed_instructions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| i["opcode"] == "close_stage")
                .count(),
            1
        );
        let fixture: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../examples/workflow-stage/business.json"
        ))
        .unwrap();
        let key = OperationKey {
            logical_store_id: "store".into(),
            scope: ["example-company".into(), "local".into()],
            source: "urn:example:stage-admin".into(),
            operation_id: "commitment-preview".into(),
        };
        let mut producer = session(&arena);
        let m = producer.meter_mut();
        let role = body::document(m, "workflow-roles", fixture["role"].clone()).unwrap();
        let supplier_role =
            body::document(m, "workflow-roles", fixture["supplier_role"].clone()).unwrap();
        let catalog =
            body::document(m, "workflow-slot-catalog", fixture["catalog"].clone()).unwrap();
        let inst=body::document(m,"workflow-slot-instantiation",json!({"schema":"ledger-workflow-slot-instantiation/1","agreement_id":fixture["agreement"],"customer":fixture["customer"],"revision":"1","template_ref":p.original_policy_value()["slot_template_ref"],"catalog_ref":catalog.reference,"component_map":fixture["component_map"],"family_map":fixture["family_map"]})).unwrap();
        let grant = json!({"kind":"workflow-source-grant","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let mut retail = fixture["terms"].clone();
        retail["role_ref"] = json!(role.reference);
        retail["source_grant_refs"] = json!([grant]);
        let mut normalized = session(&arena);
        let retail = SlotBindingTerms::normalize_stage_private_with_session(
            &p,
            &CanonicalBytes::from_value(&retail).unwrap().into_vec(),
            &mut normalized,
        )
        .unwrap();
        let retail: Value = serde_json::from_slice(retail.bytes()).unwrap();
        let original_retail = body::document(m, "workflow-slot-terms", retail.clone()).unwrap();
        let mut changed = retail.clone();
        changed["exposure_ceiling"]["atoms"] = json!("3900000000000000000");
        let changed_retail = body::document(m, "workflow-slot-terms", changed).unwrap();
        let unrelated = body::document(
            m,
            "workflow-slot-terms",
            json!({"schema":"ledger-workflow-slot-binding-terms/3","unrelated":"not selected"}),
        )
        .unwrap();
        let mut binding = fixture["binding"].clone();
        binding["slot_catalog_ref"] = json!(catalog.reference);
        binding["slot_instantiation_ref"] = json!(inst.reference);
        binding["authority_refs"] = json!([grant]);
        binding["roles_refs"] = json!([role.reference, supplier_role.reference]);
        binding["accepted_context"] = json!({"fact_schema_ref":p.context_schema_ref(),"facts":{}});
        binding["bindings"] =
            json!([{"slot":"support:retail","binding_ref":original_retail.reference}]);
        let supplier = json!({"scope":key.scope,"customer":fixture["customer"],"agreement_id":fixture["agreement"],"slot":"support:supplier","book":"supplier","role_slot":"support:supplier-role","role_ref":supplier_role.reference,"funding_leg":"support:provider","allowed_activities":["support:close","support:prepare"],"quantity_ceilings":fixture["binding"]["quantity_ceilings"],"exposure_ceiling":fixture["supplier_inputs"]["maximum_exposure"],"source_grant_refs":[grant],"correction_source_slot":"support:supplier-authorizer","cost_authority_ref":{"kind":"workflow-authority","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))},"provider_account_identity":fixture["supplier_inputs"]["provider_account_identity"],"consumption_basis":"owned_obligation_high_water"});
        let envelopes = [
            &role,
            &supplier_role,
            &catalog,
            &inst,
            &original_retail,
            &changed_retail,
            &unrelated,
        ];
        let views: Vec<_> = envelopes
            .iter()
            .map(|e| EconomicRecordView {
                reference: &e.reference,
                canonical_body: &e.canonical_body,
            })
            .collect();
        let mut first = session(&arena);
        let original =
            derive_local_supplier_commitment(&p, &key, &binding, &supplier, &views, &mut first)
                .unwrap();
        assert_eq!(
            original.canonical_value()["acceptance"]["additional_bindings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            original.canonical_value()["program"]["stages"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(original.canonical_value()["acceptance"]["source_slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "support:stage-authorizer"));
        let mut with_unrelated = binding.clone();
        with_unrelated["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!({"slot":"support:unrelated","binding_ref":unrelated.reference}));
        let mut second = session(&arena);
        let unchanged = derive_local_supplier_commitment(
            &p,
            &key,
            &with_unrelated,
            &supplier,
            &views,
            &mut second,
        )
        .unwrap();
        assert_eq!(original.reference(), unchanged.reference());
        let mut changed_binding = binding.clone();
        changed_binding["bindings"][0]["binding_ref"] = json!(changed_retail.reference);
        let mut third = session(&arena);
        let changed = derive_local_supplier_commitment(
            &p,
            &key,
            &changed_binding,
            &supplier,
            &views,
            &mut third,
        )
        .unwrap();
        assert_ne!(original.reference(), changed.reference());
        let old = json!({"schema":"ledger-workflow-party-assent/2","consent":{}});
        let mut old_session = session(&arena);
        assert!(normalize_local_supplier_assent_with_session(&p, &old, &mut old_session).is_err());
    }
}
