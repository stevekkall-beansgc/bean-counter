//! Fixed Stage membership and the actual Work-driven declared→started edge.
//! Supplier authority and monetary closure remain separate consumers.
use super::super::selected_program::SelectedProgram;
use super::{
    body,
    data::*,
    emitted_record::ConstructedEnvelope,
    records::Records,
    shape,
    working_value::{owning_reference, parse_owned_reference, OwnedExactRef},
    CoreMeter,
};
use crate::{
    canonical::{
        self,
        audit_arena::{AuditArena, ExternalBufferLease},
    },
    domain::Timestamp,
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Result,
};
use num_traits::ToPrimitive;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::mem;

fn arena(m: &mut CoreMeter) -> Result<AuditArena> {
    m.representation_arena()
        .ok_or_else(|| shape::fail("owning Stage consumer"))
}
fn text(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn same(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("Stage literal association"));
    }
    Ok(())
}
fn id(m: &mut CoreMeter, prefix: &str, domain: &[u8], tuple: &Value) -> Result<String> {
    let a = arena(m)?;
    let bytes = m.canonical_owned_representation(&a, tuple, RepresentationKind::Record)?;
    let count = w::usize_add(m, domain.len(), bytes.bytes().len())?;
    m.hash_bytes(count)?;
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes.bytes());
    let hex = canonical::hex_with_work(&hash.finalize(), m)?;
    let count = w::usize_add(m, prefix.len(), hex.len())?;
    m.write_bytes(count)?;
    Ok(format!("{prefix}{hex}"))
}
fn instance_id(m: &mut CoreMeter, key: &OperationKey, v: &Value) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = super::work_keys::scope(m, key)?;
    let tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            &v["agreement_id"],
            &v["chain"],
            &v["stage_name"],
        ],
        m,
    )?;
    id(
        m,
        "wstage_",
        b"bean-counter/workflow/1/stage-instance\0",
        tuple.value(),
    )
}
pub(super) fn state_id(
    m: &mut CoreMeter,
    key: &OperationKey,
    instance: &ExactRef,
    revision: &Value,
) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = super::work_keys::scope(m, key)?;
    let instance = a.string_value(&instance.id, m)?;
    let tuple = a.array_from_values(
        &[store.value(), scope.value(), instance.value(), revision],
        m,
    )?;
    id(
        m,
        "wstage_state_",
        b"bean-counter/workflow/1/stage-state\0",
        tuple.value(),
    )
}
pub(super) fn definition<'a>(
    m: &mut CoreMeter,
    p: &'a SelectedProgram,
    name: &str,
) -> Result<&'a Value> {
    let mut found = None;
    for d in p.source_documents() {
        m.visit("stage.original_policy")?;
        let r = d.reference();
        let expected = p.root_policy_ref();
        if shape::same_text(m, r.kind(), expected.kind())?
            && shape::same_text(m, r.id(), expected.id())?
            && shape::same_text(m, r.digest(), expected.digest())?
        {
            found = Some(d.canonical_value());
            break;
        }
    }
    let policy = found.ok_or_else(|| shape::fail("Stage original Policy"))?;
    same(m, policy, "mode", "closure_share")?;
    let stages = shape::array(m, policy, "stages")?;
    let definition = shape::named(m, stages, "name", name)?;
    shape::closed(
        m,
        definition,
        &[
            "name",
            "authorization_source_slot",
            "closure_activity",
            "closure_component_key",
            "closure_source_slot",
            "closure_evidence_kind",
            "window",
            "expected_slots",
        ],
        &[],
    )?;
    Ok(definition)
}

fn normalize_declaration(
    m: &mut CoreMeter,
    value: &Value,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let a = arena(m)?;
    let mut declaration = super::working_value::WorkingValue::copy(m, value)?;
    declaration.normalize_set(m, "authority_refs", false)?;
    let rows = shape::array(m, &declaration, "expected")?;
    if rows.is_empty() || w::compare(m, rows.len(), 32usize)?.is_gt() {
        return Err(shape::fail("Stage expected dimension"));
    }
    let per =
        mem::size_of::<crate::canonical::audit_arena::AuditedValue>() + mem::size_of::<&Value>();
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&rows.len().into(), &per.into()],
    )?;
    let bytes = rows
        .len()
        .checked_mul(per)
        .ok_or_else(|| shape::fail("Stage normalization metadata"))?;
    let _owner = a.reserve_external_payload(bytes, m)?;
    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(rows.len())
        .map_err(|_| shape::fail("Stage expected normalization allocation"))?;
    for row in rows {
        let mut row = super::working_value::WorkingValue::copy(m, row)?;
        row.normalize_set(m, "component_slots", false)?;
        ordered.push(row.into_owned()?);
        let mut j = ordered.len() - 1;
        while j > 0 {
            let left = shape::text(m, ordered[j - 1].value(), "template_input_key")?;
            let right = shape::text(m, ordered[j].value(), "template_input_key")?;
            let size = w::usize_add(m, left.len(), right.len())?;
            m.scan_bytes(size)?;
            match left.cmp(right) {
                std::cmp::Ordering::Equal => {
                    return Err(shape::fail("duplicate Stage expected template key"))
                }
                std::cmp::Ordering::Less => break,
                std::cmp::Ordering::Greater => {
                    m.visit("Stage.expected_sort_move")?;
                    ordered.swap(j - 1, j);
                    j -= 1;
                }
            }
        }
    }
    let mut refs = Vec::new();
    refs.try_reserve_exact(ordered.len())
        .map_err(|_| shape::fail("Stage normalized expected views"))?;
    for row in &ordered {
        refs.push(row.value());
    }
    let rows = a.array_from_values(&refs, m)?;
    declaration.replace(m, "expected", rows.value())?;
    declaration.into_owned()
}

fn normalize_declaration_request(
    m: &mut CoreMeter,
    p: &mut super::working_value::WorkingValue,
) -> Result<()> {
    shape::closed(
        m,
        p,
        &[
            "schema",
            "stage_instance",
            "expected_chain_revision",
            "invocation_inputs",
        ],
        &[],
    )?;
    same(m, p, "schema", "ledger-workflow-stage-authorize/1")?;
    let declaration = normalize_declaration(m, &p["stage_instance"])?;
    p.replace(m, "stage_instance", declaration.value())?;
    Ok(())
}

/// SAME declaration and embedded-Invocation transforms as the forward producer.
/// No state or authority lookup is performed.
pub(super) fn normalize_authorization(
    m: &mut CoreMeter,
    p: &mut super::working_value::WorkingValue,
) -> Result<()> {
    normalize_declaration_request(m, p)?;
    let a = arena(m)?;
    let inputs = shape::array(m, p, "invocation_inputs")?;
    let per =
        mem::size_of::<crate::canonical::audit_arena::AuditedValue>() + mem::size_of::<&Value>();
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&inputs.len().into(), &per.into()],
    )?;
    let bytes = inputs
        .len()
        .checked_mul(per)
        .ok_or_else(|| shape::fail("Stage normalization metadata"))?;
    let bytes = w::usize_add(m, bytes, 2 * mem::size_of::<Vec<&Value>>())?;
    let _owner = a.reserve_external_payload(bytes, m)?;
    m.write_bytes(bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(inputs.len())
        .map_err(|_| shape::fail("Stage intent normalization allocation"))?;
    for input in inputs {
        m.visit("Stage.intent_embedded_Invocation")?;
        let mut input = super::working_value::WorkingValue::copy(m, input)?;
        super::invocation::normalize_authorization(m, &mut input)?;
        values.push(input.into_owned()?);
    }
    let mut views = Vec::new();
    views
        .try_reserve_exact(values.len())
        .map_err(|_| shape::fail("Stage intent views allocation"))?;
    for value in &values {
        m.visit("Stage.intent_embedded_view")?;
        views.push(value.value());
    }
    let array = a.array_from_values(&views, m)?;
    p.replace(m, "invocation_inputs", array.value())?;
    p.normalize_set(m, "invocation_inputs", false)
}

fn validate_declaration_authority(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    instance: &Value,
    state: &Value,
) -> Result<()> {
    let a = arena(m)?;
    let name = shape::text(m, instance, "stage_name")?;
    let def = definition(m, program, name)?;
    let auth_ref = parse_owned_reference(m, &a, &state["authority_ref"])?;
    let authority = r.get(
        m,
        &c.key,
        auth_ref.reference(),
        "workflow-economic-authority",
        "ledger-workflow-stage-authority/1",
    )?;
    shape::equal(
        m,
        &authority["source_slot"],
        &def["authorization_source_slot"],
    )?;
    shape::equal(m, &authority["source"], &state["operation_key"]["source"])?;
    super::authority::historical_namespace(
        m,
        r,
        c,
        &state["authority_ref"],
        &authority["canonical_economic_namespace"],
        Some(&authority["source"]),
        (
            "invocation.authorize",
            &instance["binding_ref"],
            &instance["declared_at"],
        ),
    )?;
    Ok(())
}

pub(super) fn validate_zero_chain_pin(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    chain: &Value,
) -> Result<()> {
    let (state_ref, state) =
        r.stage_declaration_for_operation(m, &c.key, &chain["operation_key"])?;
    shape::closed(
        m,
        &state,
        &[
            "schema",
            "stage_instance_ref",
            "revision",
            "state",
            "operation_key",
            "authority_ref",
            "accepted_at",
        ],
        &[],
    )?;
    same(m, &state, "state", "declared")?;
    same(m, &state, "revision", "1")?;
    shape::equal(m, &state["accepted_at"], &chain["accepted_at"])?;
    let a = arena(m)?;
    let reference = parse_owned_reference(m, &a, &state["stage_instance_ref"])?;
    let instance = r.get(
        m,
        &c.key,
        reference.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    closed_instance(m, &c.key, reference.reference(), &instance)?;
    for field in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &instance[field], &chain[field])?;
    }
    shape::equal(m, &instance["declared_at"], &state["accepted_at"])?;
    let expected = state_id(m, &c.key, reference.reference(), &state["revision"])?;
    if !shape::same_text(m, &expected, &state_ref.reference().id)? {
        return Err(shape::fail("zero-claim Stage declaration state identity"));
    }
    validate_declaration_authority(m, r, c, program, &instance, &state)?;
    Ok(())
}

fn declaration_chain(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    declaration: &Value,
    state: &DraftStateData,
) -> Result<ConstructedEnvelope> {
    transition_chain(m, r, c, program, declaration, state, None)
}
fn transition_chain(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    declaration: &Value,
    state: &DraftStateData,
    new_link: Option<&ExactRef>,
) -> Result<ConstructedEnvelope> {
    let a = arena(m)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let frame = c
        .authorities
        .first()
        .ok_or_else(|| shape::fail("Stage declaring authority capture"))?;
    let accepted = owning_reference(m, &a, &frame.accepted_agreement_ref)?;
    let activation = owning_reference(m, &a, &frame.activation_ref)?;
    let mut revision = num_bigint::BigInt::from(1);
    let mut events = num_bigint::BigInt::from(0);
    let mut previous = None;
    if let Some(reference) = &state.chain_head {
        let old = r.get(
            m,
            &c.key,
            reference,
            "workflow-chain-state",
            "ledger-workflow-chain-state/1",
        )?;
        for field in ["customer", "chain", "binding_ref"] {
            shape::equal(m, &old[field], &declaration[field])?;
        }
        shape::equal(m, &old["accepted_agreement_ref"], accepted.value())?;
        shape::equal(m, &old["activation_ref"], activation.value())?;
        let (prior_revision, prior_events) = super::inventory::entry::chain_head_stage(
            m,
            r,
            c,
            declaration,
            (reference, &old, &state.links),
            program,
        )?;
        revision = super::meter::add(m, &prior_revision, &1.into())?;
        events = prior_events;
        previous = Some(old);
    } else if !state.links.is_empty() {
        return Err(shape::fail("Stage retained links without frozen chain"));
    }
    if new_link.is_some() {
        events = super::meter::add(m, &events, &1.into())?;
    }
    if super::meter::cmp(m, &events, &1000.into())?.is_gt() {
        return Err(crate::Error::new(
            "CAPACITY_EXCEEDED",
            "Stage chain event ceiling",
        ));
    }
    let bytes = state
        .links
        .len()
        .checked_mul(mem::size_of::<&ExactRef>())
        .ok_or_else(|| shape::fail("Stage link views"))?;
    let _owner = a.reserve_external_payload(bytes, m)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(state.links.len())
        .map_err(|_| shape::fail("Stage link views"))?;
    for reference in &state.links {
        let link = r.get(
            m,
            &c.key,
            reference,
            "workflow-link",
            "ledger-workflow-link/1",
        )?;
        shape::equal(m, &link["scope"], &key.value()["scope"])?;
        shape::equal(m, &link["binding_ref"], &declaration["binding_ref"])?;
        refs.push(reference);
    }
    let refs = super::app_cost::reference_set(m, &a, &refs)?;
    let old_digest = super::records::hash(m, "workflow-link-set", refs.value())?;
    let old_digest = a.string_value(&old_digest, m)?;
    if let Some(old) = &previous {
        shape::equal(m, &old["links_root"], old_digest.value())?;
    }
    let refs = if let Some(reference) = new_link {
        let maximum = w::usize_add(m, state.links.len(), 1)?;
        let mut values =
            crate::policy::workflow::selected_program::supplier_projection::Values::new(
                m, &a, maximum,
            )?;
        for value in refs
            .value()
            .as_array()
            .ok_or_else(|| shape::fail("Stage prior Link set"))?
        {
            values.unique(m, &a, value)?;
        }
        let value = owning_reference(m, &a, reference)?;
        values.unique(m, &a, value.value())?;
        values.finish(m, &a, true)?
    } else {
        refs
    };
    let digest = super::records::hash(m, "workflow-link-set", refs.value())?;
    let digest = a.string_value(&digest, m)?;
    let schema = a.string_value("ledger-workflow-chain-state/1", m)?;
    let revision = super::cost_postings::scalar(m, &a, &revision)?;
    let events = super::cost_postings::scalar(m, &a, &events)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let tuple = a.array_from_values(
        &[
            &key.value()["logical_store_id"],
            &key.value()["scope"],
            &declaration["customer"],
            &declaration["chain"],
            revision.value(),
        ],
        m,
    )?;
    let id = body::semantic_id(m, "wchain_", "workflow-chain-state", tuple.value())?;
    let value = a.object_from_fields(
        &[
            ("schema", schema.value()),
            ("logical_store_id", &key.value()["logical_store_id"]),
            ("scope", &key.value()["scope"]),
            ("customer", &declaration["customer"]),
            ("chain", &declaration["chain"]),
            ("binding_ref", &declaration["binding_ref"]),
            ("accepted_agreement_ref", accepted.value()),
            ("activation_ref", activation.value()),
            ("revision", revision.value()),
            ("admitted_events", events.value()),
            ("links_root", digest.value()),
            ("operation_key", key.value()),
            ("accepted_at", at.value()),
        ],
        m,
    )?;
    let value = if let Some(reference) = &state.chain_head {
        let prior = owning_reference(m, &a, reference)?;
        super::working_value::WorkingValue::Owned(value)
            .with_fields(m, &[("prior_ref", prior.value())])?
            .into_owned()?
    } else {
        value
    };
    body::record_borrowed(m, &c.key, "workflow-chain-state", id, value.value())
}

/// The declaration has its own accepted authorizer purpose. Worker, closer
/// and supplier purposes never substitute for this explicit StageDef slot.
pub(super) fn authorize<'a>(
    program: &SelectedProgram,
    c: &'a DraftContextData,
    mut p: super::working_value::WorkingValue,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData<'a>,
    m: &mut CoreMeter,
) -> Result<Construction<'a, 'a>> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("exact Stage authorization recipe required"));
    }
    let a = arena(m)?;
    super::inventory::entry::context_selected(m, c, program)?;
    normalize_declaration_request(m, &mut p)?;
    let declaration = shape::field(m, &p, "stage_instance")?;
    shape::closed(
        m,
        declaration,
        &[
            "customer",
            "chain",
            "agreement_id",
            "binding_ref",
            "stage_name",
            "expected",
            "authority_refs",
        ],
        &[],
    )?;
    let name = shape::text(m, declaration, "stage_name")?;
    let def = definition(m, program, name)?;
    let source_slot = shape::text(m, def, "authorization_source_slot")?;
    let binding_ref = parse_owned_reference(m, &a, &declaration["binding_ref"])?;
    let mut records =
        Records::borrowed_selected(m, &a, &c.key, inputs, &c.source_observation_inputs, program)?;
    records.install_initial_source_outputs(m, state.initial_source_correspondences)?;
    let binding = records.get(
        m,
        &c.key,
        binding_ref.reference(),
        "workflow-binding",
        "ledger-workflow-binding/2",
    )?;
    for field in ["customer", "agreement_id"] {
        shape::equal(m, &declaration[field], &binding[field])?;
    }
    for h in &state.financial.stage_heads {
        m.visit("stage.declaration_permanent_head")?;
        let agreement = shape::text(m, declaration, "agreement_id")?;
        let chain = shape::text(m, declaration, "chain")?;
        if shape::same_text(m, &h.agreement, agreement)?
            && shape::same_text(m, &h.chain, chain)?
            && shape::same_text(m, &h.stage, name)?
        {
            return Err(crate::Error::new(
                "IDENTITY_CONFLICT",
                "Stage declaration is permanent",
            ));
        }
    }
    // This operation has no Work payload. These source-purpose fields are all
    // taken from its actual accepted StageDef/Binding/declaration, not a fake
    // completion, supplier token, current policy or graph-derived authority.
    let purpose = a.object_from_fields(
        &[
            ("binding_ref", &declaration["binding_ref"]),
            ("customer", &declaration["customer"]),
            ("product_ref", &binding["product_ref"]),
            ("policy_ref", &binding["policy_ref"]),
            ("activity", &def["closure_activity"]),
            ("evidence_refs", &declaration["authority_refs"]),
        ],
        m,
    )?;
    let namespace = shape::text(m, &binding, "source_namespace")?;
    let resolved = super::app_cost::binding_resolved(
        m,
        &mut records,
        c,
        purpose.value(),
        program,
        super::authority::EconomicMode::InvocationAuthorize,
        (source_slot, &[], namespace),
    )?;
    let expected = shape::array(m, declaration, "expected")?;
    let templates = shape::array(m, def, "expected_slots")?;
    if !w::compare(m, expected.len(), templates.len())?.is_eq()
        || w::compare(m, expected.len(), 1usize)?.is_lt()
        || w::compare(m, expected.len(), 32usize)?.is_gt()
    {
        return Err(shape::fail("Stage declaration exact expected membership"));
    }
    super::inventory::slots::keyed_order(m, expected, "template_input_key", false)?;
    let maps = shape::array(m, &resolved.instantiation, "component_map")?;
    for (index, row) in expected.iter().enumerate() {
        m.visit("stage.declaration_expected_member")?;
        shape::closed(
            m,
            row,
            &[
                "template_input_key",
                "source",
                "business_operation_id",
                "activity",
                "component_slots",
            ],
            &[],
        )?;
        let key = shape::text(m, row, "template_input_key")?;
        let template = shape::named(m, templates, "key", key)?;
        shape::equal(m, &row["activity"], &template["activity"])?;
        let nominated_slot = shape::text(m, template, "source_slot")?;
        let nominations = shape::array(m, &binding, "source_slots")?;
        let nomination = shape::named(m, nominations, "name", nominated_slot)?;
        let sources = shape::array(m, nomination, "sources")?;
        if !shape::member(m, sources, &row["source"])? {
            return Err(shape::fail("Stage declared original worker nomination"));
        }
        for old in &expected[..index] {
            m.visit("stage.declaration_permanent_completion_unique")?;
            // Binding namespace equality is retained for every actual member.
            // Different transport sources cannot allocate two completions for
            // one canonical business operation inside this fixed Stage.
            if shape::same(
                m,
                &old["business_operation_id"],
                &row["business_operation_id"],
            )? {
                return Err(shape::fail("duplicate canonical Stage completion"));
            }
        }
        let keys = shape::array(m, template, "component_keys")?;
        let slots = shape::array(m, row, "component_slots")?;
        if !w::compare(m, keys.len(), slots.len())?.is_eq() {
            return Err(shape::fail("Stage exact component slots"));
        }
        for key in keys {
            m.visit("stage.declaration_component_mapping")?;
            let key = key
                .as_str()
                .ok_or_else(|| shape::fail("Stage component key"))?;
            let mapping = shape::named(m, maps, "template_key", key)?;
            if !shape::member(m, slots, &mapping["slot_id"])? {
                return Err(shape::fail("Stage declared component mapping"));
            }
        }
    }
    let invocations = shape::array(m, &p, "invocation_inputs")?;
    if w::compare(m, invocations.len(), 512usize)?.is_gt() {
        return Err(shape::fail("Stage Invocation input dimension"));
    }
    let invocation_count = w::usize_add(m, invocations.len(), invocations.len())?;
    let output_count = w::usize_add(m, 4, invocation_count)?;
    let per = mem::size_of::<ConstructedEnvelope>()
        + mem::size_of::<crate::canonical::audit_arena::AuditedValue>()
        + 2 * (64 + 128 + 71);
    w::ValidationWork::arithmetic(
        m,
        w::ArithmeticOperation::Multiply,
        &[&output_count.into(), &per.into()],
    )?;
    let bytes = output_count
        .checked_mul(per)
        .ok_or_else(|| shape::fail("Stage Invocation output storage"))?;
    let invocation_owner = a.reserve_external_payload(bytes, m)?;
    let mut invocation_outputs = Vec::new();
    invocation_outputs
        .try_reserve_exact(output_count)
        .map_err(|_| shape::fail("Stage Invocation output allocation"))?;
    let mut normalized_invocations = Vec::new();
    normalized_invocations
        .try_reserve_exact(invocations.len())
        .map_err(|_| shape::fail("Stage Invocation intent allocation"))?;
    for input in invocations {
        m.visit("Stage.embedded_Invocation")?;
        shape::equal(m, &input["binding_ref"], &declaration["binding_ref"])?;
        shape::equal(m, &input["agreement_id"], &declaration["agreement_id"])?;
        shape::equal(m, &input["chain_id"], &declaration["chain"])?;
        let mut member = None;
        for row in expected {
            let key = shape::text(m, row, "template_input_key")?;
            let template = shape::named(m, templates, "key", key)?;
            if shape::same(
                m,
                &row["business_operation_id"],
                &input["business_operation_id"],
            )? && shape::same(m, &row["activity"], &input["activity"])?
                && shape::same(m, &template["source_slot"], &input["source_slot"])?
                && member.replace(row).is_some()
            {
                return Err(shape::fail("ambiguous Stage Invocation member"));
            }
        }
        let member =
            member.ok_or_else(|| shape::fail("Stage Invocation has no expected member"))?;
        let mut count = 0usize;
        for old in invocations {
            if shape::same(
                m,
                &old["business_operation_id"],
                &input["business_operation_id"],
            )? {
                count = w::usize_add(m, count, 1)?;
            }
        }
        if w::compare(m, count, 16usize)?.is_gt() {
            return Err(shape::fail("Stage per-member Invocation dimension"));
        }
        let (outputs, normalized) =
            super::invocation::embedded_authorize(m, &mut records, c, program, input, state)?;
        let actual_invocation = outputs[1].decoded(m, 262144)?;
        shape::equal(m, &actual_invocation["source"], &member["source"])?;
        for previous in &invocation_outputs {
            let previous: &ConstructedEnvelope = previous;
            if shape::same_text(m, &previous.reference.kind, "workflow-invocation")?
                && shape::same_text(m, &previous.reference.id, &outputs[1].reference.id)?
            {
                return Err(shape::fail("duplicate Stage Invocation identity"));
            }
        }
        invocation_outputs.extend(outputs);
        normalized_invocations.push(normalized.into_owned()?);
    }
    for instruction in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("Stage.original_supplier_share_coverage")?;
        if instruction["rule"]["formula"]["kind"].as_str() != Some("closure_share") {
            continue;
        }
        for member in expected {
            let mut covered = false;
            for output in &invocation_outputs {
                if !shape::same_text(m, &output.reference.kind, "workflow-invocation")? {
                    continue;
                }
                let value = output.decoded(m, 262144)?;
                if shape::same(
                    m,
                    &value["supplier_slot"],
                    &instruction["rule"]["binding_slot"],
                )? && shape::same(
                    m,
                    &value["business_operation_id"],
                    &member["business_operation_id"],
                )? {
                    covered = true;
                }
            }
            if !covered {
                return Err(crate::Error::new(
                    "INVOCATION_REQUIRED",
                    "Stage share lacks original member supplier authorization",
                ));
            }
        }
    }
    let requested = shape::text(m, &p, "expected_chain_revision")?;
    let requested = super::arithmetic::integer(m, requested, false)?;
    let current = if let Some(reference) = &state.financial.chain_head {
        let old = records.get(
            m,
            &c.key,
            reference,
            "workflow-chain-state",
            "ledger-workflow-chain-state/1",
        )?;
        for field in ["customer", "chain", "binding_ref"] {
            shape::equal(m, &old[field], &declaration[field])?;
        }
        {
            let revision = shape::text(m, &old, "revision")?;
            super::arithmetic::integer(m, revision, false)?
        }
    } else {
        0.into()
    };
    if !super::meter::cmp(m, &requested, &current)?.is_eq() {
        return Err(crate::Error::new(
            "STALE_REVISION",
            "Stage expected chain revision",
        ));
    }
    let owner = a.reserve_external_payload(
        mem::size_of::<Construction>()
            + 4 * mem::size_of::<ConstructedEnvelope>()
            + 4 * (mem::size_of::<ExactRef>() + 64 + 128 + 71)
            + 4096,
        m,
    )?;
    let (terms, authority_value, terms_owner) = resolved.into_local_parts()?;
    super::app_cost::check_current_consent(
        program,
        m,
        &mut records,
        c,
        &terms.binding,
        &terms.accepted,
        state,
    )?;
    super::supplier_consent::current(
        program,
        m,
        &mut records,
        c,
        &terms.binding,
        &terms.accepted,
        state.current_party_authorizations,
    )?;
    let authority =
        body::document_borrowed(m, "workflow-economic-authority", authority_value.value())?;
    let id = instance_id(m, &c.key, declaration)?;
    let id_value = a.string_value(&id, m)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let constants=m.parse_owned_representation(&a,br#"{"instance":"ledger-workflow-stage-instance/1","state":"ledger-workflow-stage-state/1","declared":"declared","one":"1"}"#,RepresentationKind::SourceDefinition)?;
    let instance = super::working_value::WorkingValue::copy(m, declaration)?.with_fields(
        m,
        &[
            ("schema", &constants.value()["instance"]),
            ("instance_id", id_value.value()),
            ("declared_at", at.value()),
        ],
    )?;
    let instance = body::record_borrowed(m, &c.key, "workflow-stage-instance", id, &instance)?;
    let instance_body = instance.decoded(m, 262144)?;
    closed_instance(m, &c.key, &instance.reference, &instance_body)?;
    let instance_ref = owning_reference(m, &a, &instance.reference)?;
    let authority_ref = owning_reference(m, &a, &authority.reference)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let value = a.object_from_fields(
        &[
            ("schema", &constants.value()["state"]),
            ("stage_instance_ref", instance_ref.value()),
            ("revision", &constants.value()["one"]),
            ("state", &constants.value()["declared"]),
            ("operation_key", key.value()),
            ("authority_ref", authority_ref.value()),
            ("accepted_at", at.value()),
        ],
        m,
    )?;
    let id = state_id(m, &c.key, &instance.reference, &constants.value()["one"])?;
    let stage = body::record_borrowed(m, &c.key, "workflow-stage-state", id, value.value())?;
    let mut normalized_views = Vec::new();
    normalized_views
        .try_reserve_exact(normalized_invocations.len())
        .map_err(|_| shape::fail("Stage normalized Invocation views"))?;
    for value in &normalized_invocations {
        normalized_views.push(value.value());
    }
    let normalized_inputs = a.array_from_values(&normalized_views, m)?;
    p.replace(m, "invocation_inputs", normalized_inputs.value())?;
    p.normalize_set(m, "invocation_inputs", false)?;
    let normalized = p.bytes(m)?;
    let chain = declaration_chain(
        m,
        &mut records,
        c,
        program,
        &p["stage_instance"],
        state.financial,
    )?;
    let chain_ref = super::working_value::owning_reference(m, &a, &chain.reference)?;
    let chain_ref = parse_owned_reference(m, &a, chain_ref.value())?;
    let mut envelopes = Vec::new();
    envelopes
        .try_reserve_exact(output_count)
        .map_err(|_| shape::fail("Stage authorization output allocation"))?;
    envelopes.push(instance);
    envelopes.push(authority);
    envelopes.push(stage);
    envelopes.push(chain);
    envelopes.extend(invocation_outputs);
    Ok(Construction {
        economic: EconomicConstruction {
            terms,
            original_work: OriginalWorkPayload::Unattached,
        },
        original_reservations: None,
        financial_reservations: None,
        normalized_payload: normalized,
        new_envelopes: envelopes,
        reused_result: None,
        evaluation_steps: Vec::new(),
        proposed_heads: ProposedHeads {
            decision: Vec::new(),
            family: Vec::new(),
            chain: Some(ExactRef {
                kind: text(m, &chain_ref.reference().kind)?,
                id: text(m, &chain_ref.reference().id)?,
                digest: text(m, &chain_ref.reference().digest)?,
            }),
        },
        claim_introductions: Vec::new(),
        link_introductions: Vec::new(),
        prospective_obligations: Vec::new(),
        records,
        history_edges: Vec::new(),
        app_effects: None,
        _app_causal: None,
        current_capacity: None,
        candidate_heads: None,
        candidate_introductions: None,
        financial_observation: None,
        funding_effects: None,
        control_owners: [
            Some(owner),
            terms_owner,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(invocation_owner),
        ],
    })
}
pub(super) fn closed_instance(
    m: &mut CoreMeter,
    key: &OperationKey,
    r: &ExactRef,
    v: &Value,
) -> Result<()> {
    shape::closed(
        m,
        v,
        &[
            "schema",
            "instance_id",
            "customer",
            "chain",
            "agreement_id",
            "binding_ref",
            "stage_name",
            "expected",
            "authority_refs",
            "declared_at",
        ],
        &[],
    )?;
    same(m, v, "schema", "ledger-workflow-stage-instance/1")?;
    if !shape::same_text(m, &r.kind, "workflow-stage-instance")? {
        return Err(shape::fail("Stage instance Ref kind"));
    }
    let expected = instance_id(m, key, v)?;
    if !shape::same_text(m, &r.id, &expected)? {
        return Err(shape::fail("Stage permanent instance identity"));
    }
    same(m, v, "instance_id", &expected)?;
    Timestamp::parse_with_work(shape::text(m, v, "declared_at")?, m)?;
    let expected = shape::array(m, v, "expected")?;
    if w::compare(m, expected.len(), 1usize)?.is_lt()
        || w::compare(m, expected.len(), 32usize)?.is_gt()
    {
        return Err(shape::fail("Stage fixed membership bound"));
    }
    super::inventory::slots::keyed_order(m, expected, "template_input_key", false)?;
    for member in expected {
        m.visit("stage.fixed_expected_shape")?;
        shape::closed(
            m,
            member,
            &[
                "template_input_key",
                "source",
                "business_operation_id",
                "activity",
                "component_slots",
            ],
            &[],
        )?;
        let slots = shape::array(m, member, "component_slots")?;
        if w::compare(m, slots.len(), 1usize)?.is_lt()
            || w::compare(m, slots.len(), 128usize)?.is_gt()
        {
            return Err(shape::fail("Stage expected components bound"));
        }
        let mut previous: Option<&str> = None;
        for slot in slots {
            m.visit("stage.fixed_component_order")?;
            let raw = slot
                .as_str()
                .ok_or_else(|| shape::fail("Stage component Name"))?;
            if let Some(old) = previous {
                let count = w::usize_add(m, old.len(), raw.len())?;
                m.scan_bytes(count)?;
                if old >= raw {
                    return Err(shape::fail("Stage component order/duplicate"));
                }
            }
            previous = Some(raw);
        }
    }
    Ok(())
}
/// Lookup DATA for the one actual Stage head. The host must read the projection
/// under the held transaction and retain its owning instance/body/cutoff.
pub struct StageWorkHeadLocator {
    agreement: String,
    chain: String,
    stage: String,
    instance: OwnedExactRef,
    _ownership: ExternalBufferLease,
}
impl StageWorkHeadLocator {
    pub fn agreement(&self) -> &str {
        &self.agreement
    }
    pub fn chain(&self) -> &str {
        &self.chain
    }
    pub fn stage(&self) -> &str {
        &self.stage
    }
    pub fn instance_ref(&self) -> &ExactRef {
        self.instance.reference()
    }
}
pub struct WorkInvocationHeadLocator {
    original: OwnedExactRef,
    id: String,
}
impl WorkInvocationHeadLocator {
    pub fn original_ref(&self) -> &ExactRef {
        self.original.reference()
    }
    pub fn invocation_id(&self) -> &str {
        &self.id
    }
}
pub struct WorkInvocationHeadLocators {
    rows: Vec<WorkInvocationHeadLocator>,
    _ownership: ExternalBufferLease,
}
impl WorkInvocationHeadLocators {
    pub fn heads(&self) -> &[WorkInvocationHeadLocator] {
        &self.rows
    }
}
/// Actual Work's named original Invocation DATA only. The host reads the exact
/// held mutable heads and original supplier producer/authority inputs; neither
/// this list nor a current head certifies original authorization or exposure.
pub fn select_work_invocation_head_locators(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<WorkInvocationHeadLocators> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("exact Invocation lifecycle recipe required"));
        }
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        same(m, p.value(), "schema", "ledger-workflow-work/1")?;
        let refs = shape::array(m, p.value(), "invocation_refs")?;
        if w::compare(m, refs.len(), 16usize)?.is_gt() {
            return Err(shape::fail("Work Invocation bound"));
        }
        let mut size = mem::size_of::<WorkInvocationHeadLocators>();
        for _ in refs {
            m.visit("work.invocation_locator_dimension")?;
            size = w::usize_add(m, size, mem::size_of::<WorkInvocationHeadLocator>() + 128)?;
        }
        let ownership = a.reserve_external_payload(size, m)?;
        m.write_bytes(size)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(refs.len())
            .map_err(|_| shape::fail("Work Invocation locator allocation"))?;
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        for reference in refs {
            m.visit("work.named_original_invocation")?;
            let original = parse_owned_reference(m, &a, reference)?;
            let invocation = r.get(
                m,
                key,
                original.reference(),
                "workflow-invocation",
                "ledger-workflow-invocation/1",
            )?;
            same(m, &invocation, "revision", "1")?;
            same(m, &invocation, "state", "authorized")?;
            if invocation.get("prior_ref").is_some() {
                return Err(shape::fail("original Invocation has predecessor"));
            }
            for (field, work_field) in [
                ("original_binding_ref", "binding_ref"),
                ("customer", "customer"),
                ("business_operation_id", "business_operation_id"),
                ("chain_id", "chain"),
                ("activity", "activity"),
            ] {
                shape::equal(m, &invocation[field], &p.value()[work_field])?;
            }
            let invocation_id = shape::text(m, &invocation, "invocation_id")?;
            if !shape::same_text(m, invocation_id, &original.reference().id)? {
                return Err(shape::fail("original Invocation full identity"));
            }
            let scope = super::work_keys::scope(m, key)?;
            let tuple = a.array_from_values(
                &[
                    scope.value(),
                    &invocation["agreement_id"],
                    &invocation["business_operation_id"],
                ],
                m,
            )?;
            let id = body::semantic_id(m, "winv_", "workflow-invocation", tuple.value())?;
            if !shape::same_text(m, &id, invocation_id)? {
                return Err(shape::fail("Invocation permanent semantic identity"));
            }
            for old in &rows {
                m.visit("work.invocation_locator_unique")?;
                let old: &WorkInvocationHeadLocator = old;
                if shape::same_text(m, &old.id, &id)? {
                    return Err(shape::fail("duplicate original Work Invocation"));
                }
            }
            rows.push(WorkInvocationHeadLocator { original, id });
        }
        Ok(WorkInvocationHeadLocators {
            rows,
            _ownership: ownership,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
pub fn select_stage_work_head_locator(
    program: &SelectedProgram,
    key: &OperationKey,
    raw: &[u8],
    inputs: &[EconomicRecordView<'_>],
    s: &mut OriginalAuditSession,
) -> Result<Option<StageWorkHeadLocator>> {
    let result = (|| {
        s.check_cancelled()?;
        program.require_attempt_owner(s.meter_mut())?;
        let p = s.parse_representation(raw, RepresentationKind::OriginalRequest)?;
        let Some(reference) = p.value().get("stage_instance_ref") else {
            return Ok(None);
        };
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("exact Stage recipe required"));
        }
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        let mut r = Records::borrowed_parent_selected(m, &a, key, inputs, program)?;
        let owned = parse_owned_reference(m, &a, reference)?;
        let instance = r.get(
            m,
            key,
            owned.reference(),
            "workflow-stage-instance",
            "ledger-workflow-stage-instance/1",
        )?;
        closed_instance(m, key, owned.reference(), &instance)?;
        for k in ["customer", "chain", "binding_ref"] {
            shape::equal(m, &instance[k], &p.value()[k])?;
        }
        let agreement = shape::text(m, &instance, "agreement_id")?;
        let chain = shape::text(m, &instance, "chain")?;
        let stage = shape::text(m, &instance, "stage_name")?;
        definition(m, program, stage)?;
        let mut n = w::usize_add(m, agreement.len(), chain.len())?;
        n = w::usize_add(m, n, stage.len())?;
        n = w::usize_add(m, n, mem::size_of::<StageWorkHeadLocator>())?;
        let owner = a.reserve_external_payload(n, m)?;
        Ok(Some(StageWorkHeadLocator {
            agreement: text(m, agreement)?,
            chain: text(m, chain)?,
            stage: text(m, stage)?,
            instance: owned,
            _ownership: owner,
        }))
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

fn work_membership(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    binding: &Value,
    event: (&str, &str),
) -> Result<(OwnedExactRef, super::record_body::RecordBody)> {
    let (source, accepted_at) = event;
    let raw = shape::field(m, p, "stage_instance_ref")?;
    let a = arena(m)?;
    let inst_ref = shape::reference(m, &binding["slot_instantiation_ref"])?;
    let inst = r.get(
        m,
        &c.key,
        &inst_ref,
        "workflow-slot-instantiation",
        "ledger-workflow-slot-instantiation/1",
    )?;
    let reference = parse_owned_reference(m, &a, raw)?;
    let instance = r.get(
        m,
        &c.key,
        reference.reference(),
        "workflow-stage-instance",
        "ledger-workflow-stage-instance/1",
    )?;
    closed_instance(m, &c.key, reference.reference(), &instance)?;
    for k in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &instance[k], &p[k])?;
    }
    shape::equal(m, &instance["agreement_id"], &binding["agreement_id"])?;
    let name = shape::text(m, &instance, "stage_name")?;
    let def = definition(m, program, name)?;
    same(
        m,
        &def["window"],
        "anchor",
        "verified_predecessor_occurred_at",
    )?;
    let declared = Timestamp::parse_with_work(shape::text(m, &instance, "declared_at")?, m)?;
    let now = Timestamp::parse_with_work(accepted_at, m)?;
    if w::compare(m, declared.micros(), now.micros())?.is_gt() {
        return Err(shape::fail("Work before Stage declaration"));
    }
    let expected = shape::array(m, &instance, "expected")?;
    let templates = shape::array(m, def, "expected_slots")?;
    if !w::compare(m, expected.len(), templates.len())?.is_eq() {
        return Err(shape::fail("Stage exact fixed template membership"));
    }
    let mut member = None;
    for row in expected {
        m.visit("stage.expected_work_association")?;
        let key = shape::text(m, row, "template_input_key")?;
        let template = shape::named(m, templates, "key", key)?;
        shape::equal(m, &row["activity"], &template["activity"])?;
        let actual = shape::array(m, row, "component_slots")?;
        let keys = shape::array(m, template, "component_keys")?;
        if !w::compare(m, actual.len(), keys.len())?.is_eq() {
            return Err(shape::fail("Stage exact component inventory"));
        }
        let maps = shape::array(m, &inst, "component_map")?;
        for key in keys {
            m.visit("stage.original_instantiated_component")?;
            let key = key
                .as_str()
                .ok_or_else(|| shape::fail("Stage template component Name"))?;
            let map = shape::named(m, maps, "template_key", key)?;
            if !shape::member(m, actual, &map["slot_id"])? {
                return Err(shape::fail("Stage expected component mapping"));
            }
        }
        let slot = shape::text(m, template, "source_slot")?;
        let nominations = shape::array(m, binding, "source_slots")?;
        let nomination = shape::named(m, nominations, "name", slot)?;
        let mut nominated = false;
        for nominee in shape::array(m, nomination, "sources")? {
            m.visit("stage.expected_source_nominee")?;
            if shape::same(m, nominee, &row["source"])? {
                nominated = true;
            }
        }
        if !nominated {
            return Err(shape::fail("Stage expected accepted source nomination"));
        }
        let row_source = shape::text(m, row, "source")?;
        if shape::same(
            m,
            &row["business_operation_id"],
            &p["business_operation_id"],
        )? && shape::same_text(m, row_source, source)?
        {
            if member.is_some() {
                return Err(shape::fail("duplicate Stage business completion"));
            }
            shape::equal(m, &row["activity"], &p["activity"])?;
            member = Some(row);
        }
    }
    if member.is_none() {
        return Err(shape::fail("Work is not an expected Stage member"));
    }
    Ok((reference, instance))
}

pub(super) fn validate_started(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    instance: (&ExactRef, &Value),
    state: (&ExactRef, &Value),
    accepted_at: &str,
) -> Result<()> {
    let (instance_ref, instance) = instance;
    let (state_ref, started) = state;
    let a = arena(m)?;
    let instance_value = owning_reference(m, &a, instance_ref)?;
    let name = shape::text(m, instance, "stage_name")?;
    let def = definition(m, program, name)?;
    let expected = shape::array(m, instance, "expected")?;
    let declared = Timestamp::parse_with_work(shape::text(m, instance, "declared_at")?, m)?;
    let now = Timestamp::parse_with_work(accepted_at, m)?;
    same(m, started, "state", "started")?;
    let identity = state_id(m, &c.key, instance_ref, &started["revision"])?;
    if !shape::same_text(m, &identity, &state_ref.id)? {
        return Err(shape::fail("Stage started state identity"));
    }
    shape::equal(m, &started["stage_instance_ref"], instance_value.value())?;
    shape::closed(
        m,
        started,
        &[
            "schema",
            "stage_instance_ref",
            "revision",
            "state",
            "operation_key",
            "authority_ref",
            "accepted_at",
            "prior_ref",
            "first_work_ref",
            "first_work_accepted_at",
            "activation_deadline",
        ],
        &[],
    )?;
    same(m, started, "revision", "2")?;
    // Preserve this immutable clock; later members do not emit a new state.
    let first = shape::reference(m, &started["first_work_ref"])?;
    let first = r.get(
        m,
        &c.key,
        &first,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    shape::equal(
        m,
        &first["payload"]["stage_instance_ref"],
        instance_value.value(),
    )?;
    shape::equal(m, &first["accepted_at"], &started["first_work_accepted_at"])?;
    shape::equal(m, &first["key"], &started["operation_key"])?;
    shape::equal(m, &first["accepted_at"], &started["accepted_at"])?;
    for field in ["customer", "chain", "binding_ref"] {
        shape::equal(m, &first["payload"][field], &instance[field])?;
    }
    let first_source = shape::text(m, &first["key"], "source")?;
    let mut matched = false;
    for member in expected {
        m.visit("stage.first_expected_work")?;
        let source = shape::text(m, member, "source")?;
        if shape::same_text(m, source, first_source)?
            && shape::same(
                m,
                &member["business_operation_id"],
                &first["payload"]["business_operation_id"],
            )?
        {
            shape::equal(m, &member["activity"], &first["payload"]["activity"])?;
            if matched {
                return Err(shape::fail("ambiguous first Stage member"));
            }
            matched = true;
        }
    }
    if !matched {
        return Err(shape::fail("first Work outside fixed Stage membership"));
    }
    let claim_id = shape::text(m, &first, "claim_id")?;
    let (claim_ref, claim) = r.by_identity(
        m,
        &c.key,
        "workflow-work-claim",
        claim_id,
        "ledger-workflow-work-claim/1",
    )?;
    super::inventory::entry::historical_work_claim(m, c, &claim_ref, &claim, &first)?;
    let permanent = super::work_keys::decision(m, claim_id)?;
    let permanent = a.string_value(&permanent, m)?;
    let revision = a.string_value("1", m)?;
    let tuple = a.array_from_values(&[permanent.value(), revision.value()], m)?;
    let decision_id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
    let (_, decision) = r.by_identity(
        m,
        &c.key,
        "workflow-decision",
        &decision_id,
        "ledger-workflow-decision-revision/1",
    )?;
    same(m, &decision, "kind", "work")?;
    same(m, &decision, "transition", "initial")?;
    same(m, &decision, "status", "active")?;
    shape::equal(m, &decision["decision_id"], permanent.value())?;
    shape::equal(m, &decision["original_terms_ref"], &instance["binding_ref"])?;
    shape::equal(m, &decision["operation_key"], &first["key"])?;
    shape::equal(m, &decision["subject_ref"], &started["first_work_ref"])?;
    shape::equal(m, &decision["authority_ref"], &started["authority_ref"])?;
    shape::equal(m, &decision["accepted_at"], &first["accepted_at"])?;
    same(m, &decision, "revision", "1")?;
    let initial_ref = shape::reference(m, &started["prior_ref"])?;
    let initial = r.get(
        m,
        &c.key,
        &initial_ref,
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    shape::closed(
        m,
        &initial,
        &[
            "schema",
            "stage_instance_ref",
            "revision",
            "state",
            "operation_key",
            "authority_ref",
            "accepted_at",
        ],
        &[],
    )?;
    same(m, &initial, "revision", "1")?;
    same(m, &initial, "state", "declared")?;
    shape::equal(m, &initial["stage_instance_ref"], instance_value.value())?;
    shape::equal(m, &initial["accepted_at"], &instance["declared_at"])?;
    let expected_initial = state_id(m, &c.key, instance_ref, &initial["revision"])?;
    if !shape::same_text(m, &initial_ref.id, &expected_initial)? {
        return Err(shape::fail("Stage original declared state identity"));
    }
    let first_at = Timestamp::parse_with_work(shape::text(m, &first, "accepted_at")?, m)?;
    if w::compare(m, first_at.micros(), declared.micros())?.is_lt()
        || w::compare(m, first_at.micros(), now.micros())?.is_gt()
    {
        return Err(shape::fail("Stage first Work chronological bound"));
    }
    let duration = shape::text(m, &def["window"], "activation_deadline_us")?;
    let duration = super::arithmetic::integer(m, duration, false)?;
    if super::meter::cmp(m, &duration, &0.into())?.is_lt() {
        return Err(shape::fail("Stage activation duration"));
    }
    let deadline = super::meter::add(m, &first_at.micros().into(), &duration)?;
    let raw = shape::text(m, started, "activation_deadline")?;
    let actual = Timestamp::parse_with_work(raw, m)?;
    if !super::meter::cmp(m, &deadline, &actual.micros().into())?.is_eq() {
        return Err(shape::fail("Stage immutable original activation deadline"));
    }
    validate_declaration_authority(m, r, c, program, instance, &initial)?;
    super::authority::historical_namespace(
        m,
        r,
        c,
        &decision["authority_ref"],
        &first["source_namespace"],
        Some(&first["key"]["source"]),
        (
            "work.accept",
            &first["payload"]["binding_ref"],
            &first["accepted_at"],
        ),
    )?;
    Ok(())
}
/// Historical membership and immutable first-Work clock, independent of today's head.
pub(super) fn retained_work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    work: &Value,
    binding: &Value,
) -> Result<()> {
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("retained staged Work recipe"));
    }
    let p = shape::field(m, work, "payload")?;
    let at = shape::text(m, work, "accepted_at")?;
    let source = shape::text(m, &work["key"], "source")?;
    let (reference, instance) = work_membership(m, r, c, program, p, binding, (source, at))?;
    let a = arena(m)?;
    let two = a.string_value("2", m)?;
    let identity = state_id(m, &c.key, reference.reference(), two.value())?;
    let (state_ref, state) = r.by_identity(
        m,
        &c.key,
        "workflow-stage-state",
        &identity,
        "ledger-workflow-stage-state/1",
    )?;
    validate_started(
        m,
        r,
        c,
        program,
        (reference.reference(), &instance),
        (&state_ref, &state),
        at,
    )
}

pub(super) struct WorkTransition {
    pub record: Option<ConstructedEnvelope>,
    pub ownership: ExternalBufferLease,
}
/// Called only after the SAME Work interpreter has normalized facts, checked
/// original/current Work authority and constructed its permanent Work/claim.
pub(super) fn work(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    program: &SelectedProgram,
    p: &Value,
    selected: (&ExactRef, &ExactRef, &Value, &DraftStateData),
) -> Result<Option<WorkTransition>> {
    let Some(_) = p.get("stage_instance_ref") else {
        return Ok(None);
    };
    if !program.uses_stage_lifecycle_recipe() {
        return Err(shape::fail("staged Work requires exact Stage recipe"));
    }
    let (work, authority, binding, state) = selected;
    let a = arena(m)?;
    let ownership = a.reserve_external_payload(
        2 * mem::size_of::<WorkTransition>() + mem::size_of::<ExactRef>() + 64 + 128 + 71 + 1024,
        m,
    )?;
    let (reference, instance) = work_membership(
        m,
        r,
        c,
        program,
        p,
        binding,
        (&c.key.source, &c.accepted_at),
    )?;
    let name = shape::text(m, &instance, "stage_name")?;
    let def = definition(m, program, name)?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?;
    let mut head = None;
    let agreement = shape::text(m, &instance, "agreement_id")?;
    let chain = shape::text(m, &instance, "chain")?;
    for h in &state.stage_heads {
        m.visit("stage.actual_held_head")?;
        if shape::same_text(m, &h.agreement, agreement)?
            && shape::same_text(m, &h.chain, chain)?
            && shape::same_text(m, &h.stage, name)?
        {
            if head.is_some() {
                return Err(shape::fail("ambiguous Stage head"));
            }
            head = Some(h);
        }
    }
    let head = head.ok_or_else(|| shape::fail("missing declared Stage head"))?;
    let old = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-stage-state",
        "ledger-workflow-stage-state/1",
    )?;
    let instance_value = owning_reference(m, &a, reference.reference())?;
    shape::equal(m, &old["stage_instance_ref"], instance_value.value())?;
    same(m, &old, "revision", &head.revision)?;
    let identity = state_id(m, &c.key, reference.reference(), &old["revision"])?;
    if !shape::same_text(m, &identity, &head.reference.id)? {
        return Err(shape::fail("Stage state identity"));
    }
    let status = shape::text(m, &old, "state")?;
    if shape::same_text(m, status, "started")? {
        validate_started(
            m,
            r,
            c,
            program,
            (reference.reference(), &instance),
            (&head.reference, &old),
            &c.accepted_at,
        )?;
        return Ok(None);
    }
    if !shape::same_text(m, status, "declared")? {
        return Err(crate::Error::new("TARGET_CLOSED", "Stage is terminal"));
    }
    shape::closed(
        m,
        &old,
        &[
            "schema",
            "stage_instance_ref",
            "revision",
            "state",
            "operation_key",
            "authority_ref",
            "accepted_at",
        ],
        &[],
    )?;
    same(m, &old, "revision", "1")?;
    shape::equal(m, &old["accepted_at"], &instance["declared_at"])?;
    let duration = shape::text(m, &def["window"], "activation_deadline_us")?;
    let duration = super::arithmetic::integer(m, duration, false)?;
    if super::meter::cmp(m, &duration, &0.into())?.is_lt() {
        return Err(shape::fail("Stage activation duration"));
    }
    let deadline = super::meter::add(m, &now.micros().into(), &duration)?
        .to_i64()
        .ok_or_else(|| shape::fail("Stage deadline overflow"))?;
    let deadline = Timestamp::from_micros_with_work(deadline, m)?;
    let deadline = a.string_value(deadline.as_str(), m)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let key = super::cost_postings::operation_key(m, &a, c)?;
    let authority = owning_reference(m, &a, authority)?;
    let work = owning_reference(m, &a, work)?;
    let prior = owning_reference(m, &a, &head.reference)?;
    let constants = m.parse_owned_representation(
        &a,
        br#"{"schema":"ledger-workflow-stage-state/1","revision":"2","state":"started"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let value = a.object_from_fields(
        &[
            ("schema", &constants.value()["schema"]),
            ("stage_instance_ref", instance_value.value()),
            ("revision", &constants.value()["revision"]),
            ("state", &constants.value()["state"]),
            ("operation_key", key.value()),
            ("authority_ref", authority.value()),
            ("accepted_at", at.value()),
            ("prior_ref", prior.value()),
            ("first_work_ref", work.value()),
            ("first_work_accepted_at", at.value()),
            ("activation_deadline", deadline.value()),
        ],
        m,
    )?;
    let id = state_id(
        m,
        &c.key,
        reference.reference(),
        &constants.value()["revision"],
    )?;
    let record = body::record_borrowed(m, &c.key, "workflow-stage-state", id, value.value())?;
    Ok(Some(WorkTransition {
        record: Some(record),
        ownership,
    }))
}

#[path = "stage_close.rs"]
mod close;
pub(super) use close::normalize_request as normalize_close;

pub(super) fn closure_claim_id(
    m: &mut CoreMeter,
    key: &OperationKey,
    instance: &Value,
) -> Result<String> {
    let a = arena(m)?;
    let store = a.string_value(&key.logical_store_id, m)?;
    let scope = super::work_keys::scope(m, key)?;
    let closure = a.string_value("closure", m)?;
    let tuple = a.array_from_values(
        &[
            store.value(),
            scope.value(),
            &instance["agreement_id"],
            &instance["chain"],
            &instance["stage_name"],
            closure.value(),
        ],
        m,
    )?;
    id(
        m,
        "wstage_claim_",
        b"bean-counter/workflow/1/stage-closure\0",
        tuple.value(),
    )
}
pub(super) fn closure_decision_id(
    m: &mut CoreMeter,
    key: &OperationKey,
    instance: &Value,
) -> Result<String> {
    let a = arena(m)?;
    let claim = closure_claim_id(m, key, instance)?;
    let claim = a.string_value(&claim, m)?;
    let tag = a.string_value("stage", m)?;
    let tuple = a.array_from_values(&[claim.value(), tag.value()], m)?;
    body::semantic_id(m, "wdecision_", "workflow-decision", tuple.value())
}
pub(in crate::policy::workflow::transition_constructor) use close::produce as close_stage;

#[path = "stage_retained.rs"]
mod retained;
pub(super) use retained::link as retained_link;

pub(super) use close::prior_net as retained_prior_net;
