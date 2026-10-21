//! App changing-body byte algebra from the SAME closed emitters. This is a
//! canonical BODY and decoded-type-guard contribution, not an arena, whole
//! semantic-work, closure or ProfileB proof.
//! All counts are original-program dimensions; no observed fixture size or
//! registered record maximum is substituted for a constructor contribution.
use super::super::{shape, CoreMeter};
use crate::{
    validation_work::{self as w, ArithmeticOperation, RepresentationKind, ValidationWork},
    Result,
};
use serde_json::Value;
// Frozen scalar grammar bounds INCLUDING JSON quotes. Id excludes C0/DEL;
// at most128 codepoints*4UTF8 bytes plus quotes. SourceId permits controls,
// hence256*6 escaped bytes. Name is ASCII and at most128 bytes.
#[derive(Clone, Copy)]
struct Body {
    bytes: usize,
    guard: usize,
    depth: usize,
}
impl Body {
    const fn scalar(bytes: usize) -> Self {
        Self {
            bytes,
            guard: 5,
            depth: 0,
        }
    }
    const fn reference(bytes: usize) -> Self {
        Self {
            bytes,
            guard: 35,
            depth: 1,
        }
    }
}

const ID: Body = Body::scalar(514);
const SOURCE: Body = Body::scalar(1538);
const NAME: Body = Body::scalar(130);
const REF: Body = Body::reference(102 + 64 + 128);
const TIME: Body = Body::scalar(29);
const VERSION: Body = Body::scalar(21);
// Amount is a closed currency/scale/atoms object, rather than one scalar.
const AMOUNT: Body = Body::reference(71);
const RATIONAL_NUMERATOR: Body = Body::scalar(158);
const RATIONAL_DENOMINATOR: Body = Body::scalar(157);
fn add(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    w::usize_add(m, a, b)
}
fn mul(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("app changing body product"))
}
fn largest(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    Ok(if w::compare(m, a, b)?.is_gt() { a } else { b })
}
fn maximum(m: &mut CoreMeter, a: Body, b: Body) -> Result<Body> {
    Ok(Body {
        bytes: largest(m, a.bytes, b.bytes)?,
        guard: largest(m, a.guard, b.guard)?,
        depth: largest(m, a.depth, b.depth)?,
    })
}
fn sum(m: &mut CoreMeter, a: Body, b: Body) -> Result<Body> {
    Ok(Body {
        bytes: add(m, a.bytes, b.bytes)?,
        guard: add(m, a.guard, b.guard)?,
        depth: largest(m, a.depth, b.depth)?,
    })
}
fn array(m: &mut CoreMeter, n: usize, body: Body) -> Result<Body> {
    let items = mul(m, n, body.bytes)?;
    let commas = if w::compare(m, n, 0usize)?.is_gt() {
        w::usize_sub(m, n, 1)?
    } else {
        0
    };
    let bytes = add(m, items, commas)?;
    let bytes = add(m, 2, bytes)?;
    let per = add(m, 4, body.guard)?;
    let guard = mul(m, n, per)?;
    let guard = add(m, 4, guard)?;
    let depth = if w::compare(m, n, 0usize)?.is_gt() {
        add(m, body.depth, 1)?
    } else {
        0
    };
    Ok(Body {
        bytes,
        guard,
        depth,
    })
}
fn object(m: &mut CoreMeter, fields: &[(&str, Body)]) -> Result<Body> {
    let commas = if w::compare(m, fields.len(), 0usize)?.is_gt() {
        w::usize_sub(m, fields.len(), 1)?
    } else {
        0
    };
    let mut bytes = add(m, 2, commas)?;
    let mut guard = 4;
    let mut depth = 0;
    for (key, body) in fields {
        m.visit("capacity.app_body_closed_field")?;
        let key = add(m, key.len(), 3)?;
        let field = add(m, key, body.bytes)?;
        bytes = add(m, bytes, field)?;
        guard = add(m, guard, 4)?;
        guard = add(m, guard, body.guard)?;
        let child = add(m, body.depth, 1)?;
        depth = largest(m, depth, child)?;
    }
    if w::compare(m, depth, 32usize)?.is_gt() {
        return Err(shape::fail("prospective app decoded depth"));
    }
    Ok(Body {
        bytes,
        guard,
        depth,
    })
}
const fn ascii(s: &str) -> Body {
    Body::scalar(s.len() + 2)
}
// codec::validate_selected_types pays4 per Value,4 per field/element edge,
// and1 more for a JSON integer.5 per scalar is consequently conservative.
// Immutable normalized facts/context may have a different actual shape than
// any literal fixture, so derive these dimensions from their actual bodies.
pub(super) fn immutable_guard(
    m: &mut CoreMeter,
    v: &Value,
    depth: usize,
) -> Result<(usize, usize)> {
    m.visit("capacity.app_immutable_value_dimension")?;
    if w::compare(m, depth, 32usize)?.is_gt() {
        return Err(shape::fail("original app decoded depth"));
    }
    let mut guard = 5;
    let mut deepest = depth;
    match v {
        Value::Array(values) => {
            guard = 4;
            for value in values {
                m.visit("capacity.app_immutable_element")?;
                let next = add(m, depth, 1)?;
                let (g, d) = immutable_guard(m, value, next)?;
                guard = add(m, guard, 4)?;
                guard = add(m, guard, g)?;
                deepest = largest(m, deepest, d)?;
            }
        }
        Value::Object(values) => {
            guard = 4;
            for (_, value) in values {
                m.visit("capacity.app_immutable_field")?;
                let next = add(m, depth, 1)?;
                let (g, d) = immutable_guard(m, value, next)?;
                guard = add(m, guard, 4)?;
                guard = add(m, guard, g)?;
                deepest = largest(m, deepest, d)?;
            }
        }
        Value::Null => return Err(shape::fail("null original app body")),
        _ => {}
    }
    Ok((guard, deepest))
}
fn exact(m: &mut CoreMeter, v: &Value) -> Result<Body> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app byte bound"))?;
    let encoded = m.canonical_owned_representation(&arena, v, RepresentationKind::Record)?;
    let (guard, depth) = immutable_guard(m, v, 0)?;
    Ok(Body {
        bytes: encoded.bytes().len(),
        guard,
        depth,
    })
}
/// Full immutable encoded size, including escaping. Used by original-source
/// dimension derivation, never as a cached semantic validation capability.
pub(super) fn immutable_bytes(m: &mut CoreMeter, v: &Value) -> Result<usize> {
    Ok(exact(m, v)?.bytes)
}
/// All future current grants are subsets of their original ceiling. Exact
/// scalar/set bytes come from that original; selector regrouping uses every
/// permitted nonempty subset, rather than only the originally written rows.
pub(super) fn source_grant_body(
    m: &mut CoreMeter,
    ceiling: &Value,
    selectors: [(usize, usize, usize); 3],
) -> Result<usize> {
    let principal = shape::field(m, ceiling, "principal_id")?;
    let principal = exact(m, principal)?;
    let source = shape::field(m, ceiling, "source")?;
    let source = exact(m, source)?;
    let namespace = shape::field(m, ceiling, "canonical_economic_namespace")?;
    let namespace = exact(m, namespace)?;
    let customer = shape::field(m, ceiling, "customer_scope")?;
    let customer = exact(m, customer)?;
    let operations = shape::field(m, ceiling, "operations")?;
    let operations = exact(m, operations)?;
    let slots = shape::field(m, ceiling, "source_slots")?;
    let slots = exact(m, slots)?;
    let permissions = shape::field(m, ceiling, "permissions")?;
    let permissions = exact(m, permissions)?;
    let mut bodies = [Body::scalar(0); 3];
    for (i, (count, names, name_bytes)) in selectors.into_iter().enumerate() {
        m.visit("capacity.source_regrouped_selector_body")?;
        let names = array(m, names, Body::scalar(name_bytes))?;
        let selector = object(m, &[("names", names)])?;
        bodies[i] = array(m, count, selector)?;
    }
    Ok(object(
        m,
        &[
            ("schema", ascii("ledger-workflow-source-grant/1")),
            ("grant_id", ID),
            ("revision", VERSION),
            ("original_ceiling_ref", REF),
            ("principal_id", principal),
            ("source", source),
            ("canonical_economic_namespace", namespace),
            ("customer_scope", customer),
            ("operations", operations),
            ("product_selectors", bodies[0]),
            ("activity_selectors", bodies[1]),
            ("link_selectors", bodies[2]),
            ("source_slots", slots),
            ("permissions", permissions),
            ("starts_at", TIME),
            ("ends_at", TIME),
        ],
    )?
    .bytes)
}
pub(in crate::policy::workflow::transition_constructor) struct SourceBodyBounds {
    pub current_grant: usize,
    pub current_state: usize,
    pub observation: usize,
    pub current_state_guard: usize,
    pub observation_guard: usize,
}
pub(super) fn source_bodies(
    m: &mut CoreMeter,
    enrollment: &Value,
    store: &str,
    original_grants: usize,
    current_grant: usize,
) -> Result<SourceBodyBounds> {
    let scope_value = shape::field(m, enrollment, "scope")?;
    let scope = exact(m, scope_value)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning source body store"))?;
    let store_value = arena.string_value(store, m)?;
    let store = exact(m, store_value.value())?;
    let key = object(
        m,
        &[
            ("logical_store_id", store),
            ("scope", scope),
            ("source", SOURCE),
            ("operation_id", ID),
        ],
    )?;
    // Both optional references are counted even though a revoked state omits
    // current_grant_ref. Original/successor revisions never exceed i64::MAX.
    let state = object(
        m,
        &[
            ("schema", ascii("ledger-workflow-source-state/1")),
            ("enrollment_ref", REF),
            ("revision", VERSION),
            ("status", ascii("revoked")),
            ("current_grant_ref", REF),
            ("prior_state_ref", REF),
            ("operation_key", key),
            ("actor", ID),
            ("accepted_at", TIME),
        ],
    )?;
    let grants = array(m, original_grants, REF)?;
    // Observation counts are u64, unlike the source revision's i64 grammar.
    let count = Body::scalar(22);
    let observation = object(
        m,
        &[
            (
                "schema",
                ascii("ledger-workflow-authority-prefix-observation/1"),
            ),
            ("logical_store_id", store),
            ("scope", scope),
            ("installation_mode", ascii("isolated_synthetic")),
            ("root_ref", REF),
            ("enrollment_ref", REF),
            ("current_state_ref", REF),
            ("current_grant_ref", REF),
            ("original_grant_refs", grants),
            ("head_revision", VERSION),
            ("verified_through_sequence", count),
            (
                "verification_profile",
                ascii("ledger-authority-origin-audit/1"),
            ),
            ("verifier_ref", REF),
            ("prefix_digest", Body::scalar(73)),
            ("origin_count", count),
            ("document_count", count),
            ("audited_retained_bytes", count),
            ("observed_at", TIME),
        ],
    )?;
    Ok(SourceBodyBounds {
        current_grant,
        current_state: state.bytes,
        observation: observation.bytes,
        current_state_guard: state.guard,
        observation_guard: observation.guard,
    })
}
/// Normalized facts range over the SAME immutable schema, not the original
/// operation's one observed value. Optional fields are all counted, and every
/// declared enum alternative contributes to the scalar maximum. Scalar bounds
/// are the existing fact normalizer's grammar; this is not a new fact ceiling.
fn normalized_facts(m: &mut CoreMeter, schema: &Value) -> Result<Body> {
    let fields = shape::array(m, schema, "fields")?;
    let commas = if w::compare(m, fields.len(), 0usize)?.is_gt() {
        w::usize_sub(m, fields.len(), 1)?
    } else {
        0
    };
    let mut bytes = add(m, 2, commas)?;
    let mut guard = 4usize;
    let mut depth = 0usize;
    for field in fields {
        m.visit("capacity.original_fact_output_range")?;
        let name = shape::field(m, field, "name")?;
        let name = exact(m, name)?;
        let kind = shape::text(m, &field["type"], "kind")?;
        let scalar = if shape::same_text(m, kind, "boolean")? {
            Body::scalar(5)
        } else if shape::same_text(m, kind, "string")? {
            let max = shape::text(m, &field["type"], "max_bytes")?;
            let max = super::super::super::facts::counter_with_work(max, m)?;
            let max = usize::try_from(max).map_err(|_| shape::fail("fact output byte range"))?;
            // Controls are refused by the SAME normalizer. Every remaining
            // UTF8 input byte needs at most two JSON bytes (quote/backslash).
            let escaped = mul(m, 2, max)?;
            Body::scalar(add(m, escaped, 2)?)
        } else if shape::same_text(m, kind, "enum")? {
            let mut maximum_bytes = 0;
            for value in shape::array(m, &field["type"], "values")? {
                m.visit("capacity.original_enum_output_range")?;
                let value = exact(m, value)?;
                maximum_bytes = largest(m, maximum_bytes, value.bytes)?;
            }
            Body::scalar(maximum_bytes)
        } else if shape::same_text(m, kind, "integer")? {
            // SignedScalar's normalized coefficient has at most30 digits.
            Body::scalar(33)
        } else if shape::same_text(m, kind, "decimal")? {
            // At most30 coefficient digits, one decimal point, sign+quotes.
            // A leading zero at scale18 is shorter than this coefficient case.
            Body::scalar(34)
        } else if shape::same_text(m, kind, "timestamp")? {
            TIME
        } else {
            return Err(crate::Error::new(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "prospective fact output kind is not derived",
            ));
        };
        let field_bytes = add(m, name.bytes, 1)?;
        let field_bytes = add(m, field_bytes, scalar.bytes)?;
        bytes = add(m, bytes, field_bytes)?;
        guard = add(m, guard, 4)?;
        guard = add(m, guard, scalar.guard)?;
        depth = largest(m, depth, 1)?;
    }
    Ok(Body {
        bytes,
        guard,
        depth,
    })
}
/// Family evaluations may assert future legal facts under their original
/// trigger schema. This maximum is separately derived from the immutable
/// compiled activity/schema relation, never from fixture facts/current policy.
pub(in crate::policy::workflow::transition_constructor) fn family_facts(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    payload: &Value,
) -> Result<FamilyFactBounds> {
    let mut maximum_facts = Body {
        bytes: 2,
        guard: 4,
        depth: 0,
    };
    let mut family_count = 0;
    for instruction in shape::array(
        m,
        program.compiled_plan().canonical_value(),
        "typed_instructions",
    )? {
        m.visit("capacity.original_family_fact_schema")?;
        let opcode = shape::text(m, instruction, "opcode")?;
        if !shape::same_text(m, opcode, "evaluate_family")? {
            continue;
        }
        let family = &instruction["family"];
        if !shape::same(
            m,
            &family["selector"]["target_activity"],
            &payload["activity"],
        )? {
            continue;
        }
        let activity = shape::text(m, family, "trigger_activity")?;
        let activities = shape::array(
            m,
            program.compiled_plan().canonical_value(),
            "activity_table",
        )?;
        let selected = shape::named(m, activities, "activity", activity)?;
        let reference = shape::field(m, selected, "fact_schema_ref")?;
        let facts = facts_for_reference(m, program, reference)?;
        maximum_facts = maximum(m, maximum_facts, facts)?;
        family_count = add(m, family_count, 1)?;
    }
    Ok(FamilyFactBounds {
        maximum_body_bytes: maximum_facts.bytes,
        maximum_decoded_guard: maximum_facts.guard,
        maximum_depth: maximum_facts.depth,
        family_instructions: family_count,
    })
}
fn facts_for_reference(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    reference: &Value,
) -> Result<Body> {
    let mut schema = None;
    for document in program.source_documents() {
        m.visit("capacity.original_fact_schema_document")?;
        let r = document.reference();
        let kind = shape::text(m, reference, "kind")?;
        if !shape::same_text(m, r.kind(), kind)? {
            continue;
        }
        let id = shape::text(m, reference, "id")?;
        if !shape::same_text(m, r.id(), id)? {
            continue;
        }
        let digest = shape::text(m, reference, "digest")?;
        if shape::same_text(m, r.digest(), digest)? {
            m.visit("capacity.original_fact_schema_unique")?;
            if schema.is_some() {
                return Err(shape::fail("duplicate original fact schema"));
            }
            schema = Some(document.canonical_value());
        }
    }
    normalized_facts(
        m,
        schema.ok_or_else(|| shape::fail("original fact schema body missing"))?,
    )
}
pub(in crate::policy::workflow::transition_constructor) struct FamilyFactBounds {
    pub maximum_body_bytes: usize,
    pub maximum_decoded_guard: usize,
    pub maximum_depth: usize,
    pub family_instructions: usize,
}

/// Closed family changing bodies from the actual submit/correct/close/causal
/// emitters. Complete old-input closures, source/F observations, operation and
/// capacity outputs are separate terms; these cannot be a wire envelope.
pub(in crate::policy::workflow::transition_constructor) struct FamilyBodyBounds {
    pub facts: FamilyFactBounds,
    pub _claim: usize,
    pub _link: usize,
    pub action: usize,
    pub evaluation: usize,
    pub decision: usize,
    pub family_state: usize,
    pub _chain: usize,
    pub authority: usize,
    pub _maximum_decoded_body_guard: usize,
}
pub(in crate::policy::workflow::transition_constructor) fn family_bodies(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    payload: &Value,
    terms: &super::super::authority::Terms,
    logical_store: &Value,
    maximum_original_actions: usize,
    maximum_attested_fields: usize,
) -> Result<FamilyBodyBounds> {
    let facts = family_facts(m, program, payload)?;
    let facts_body = Body {
        bytes: facts.maximum_body_bytes,
        guard: facts.maximum_decoded_guard,
        depth: facts.maximum_depth,
    };
    let context = exact(m, &terms.binding["accepted_context"])?;
    let scope = exact(m, &terms.slot_terms["scope"])?;
    let customer = exact(m, &terms.binding["customer"])?;
    let agreement = exact(m, &terms.binding["agreement_id"])?;
    let store = exact(m, logical_store)?;
    let key = object(
        m,
        &[
            ("logical_store_id", store),
            ("scope", scope),
            ("source", SOURCE),
            ("operation_id", ID),
        ],
    )?;
    let empty = Body {
        bytes: 2,
        guard: 4,
        depth: 0,
    };
    let basis = array(m, maximum_original_actions, REF)?;
    let two = array(m, 2, REF)?;
    let one = array(m, 1, REF)?;
    let evidence = array(m, 16, REF)?;
    let verified = array(m, maximum_attested_fields, REF)?;
    let proof = object(m, &[("field", NAME), ("attestation_ref", REF)])?;
    let proofs = array(m, maximum_attested_fields, proof)?;
    let grant_count = shape::array(m, &terms.slot_terms, "source_grant_refs")?.len();
    let original_grants = array(m, grant_count, REF)?;
    let payload = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-outcome/1") }),
            ("target_ref", REF),
            ("family_slot", ID),
            ("business_outcome_token", ID),
            ("activity", NAME),
            ("product_ref", REF),
            ("policy_ref", REF),
            ("fact_schema_ref", REF),
            ("binding_ref", REF),
            ("facts", facts_body),
            ("fact_proofs", proofs),
            ("evidence_refs", evidence),
            ("occurred_at", TIME),
        ],
    )?;
    let claim = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-outcome-claim/1") }),
            ("scope", scope),
            ("agreement_id", agreement),
            ("family_slot", ID),
            ("target_ref", REF),
            ("source_namespace", SOURCE),
            ("business_outcome_token", ID),
            ("original_payload", payload),
            ("received_at", TIME),
            ("accepted_at", TIME),
        ],
    )?;
    let link = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-link/1") }),
            ("scope", scope),
            ("child", REF),
            ("product_ref", REF),
            ("binding_ref", REF),
            ("slot", NAME),
            ("predecessor", REF),
            ("classification", const { ascii("monetary") }),
            ("authority_ref", REF),
            ("operation_key", key),
        ],
    )?;
    // Include inverse-only optional inverse_of; replacement and original
    // alternatives are shorter. Family basis is the original root action set.
    let action = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-action/1") }),
            ("effect_id", ID),
            ("decision_id", ID),
            ("revision", VERSION),
            ("posting_role", const { ascii("replacement") }),
            ("book", const { ascii("retail") }),
            ("component_slot", ID),
            ("root_component_slot", ID),
            ("role_ref", REF),
            ("slot_terms_ref", REF),
            ("funding_leg", NAME),
            ("amount", AMOUNT),
            ("inverse_of", REF),
            ("basis_refs", basis),
            ("operation_key", key),
        ],
    )?;
    let step = object(
        m,
        &[
            ("instruction", VERSION),
            ("component_slot", ID),
            ("family_slot", ID),
            ("predicate", const { ascii("true") }),
            ("basis_refs", basis),
            ("rational_numerator", RATIONAL_NUMERATOR),
            ("rational_denominator", RATIONAL_DENOMINATOR),
            ("rounded", AMOUNT),
            ("disposition", const { ascii("inverse") }),
            ("action_ref", REF),
        ],
    )?;
    let steps = array(m, 2, step)?;
    let evaluation = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-evaluation-proof/1") },
            ),
            ("subject_ref", REF),
            ("policy_ref", REF),
            ("compiled_plan_ref", REF),
            ("compiler_ref", REF),
            ("binding_ref", REF),
            ("fact_schema_ref", REF),
            ("facts", facts_body),
            ("context", context),
            ("steps", steps),
        ],
    )?;
    // Every family producer starts dependency_refs empty and preserves that
    // original set. No work/stage consumer is silently added to this bound.
    let decision = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-decision-revision/1") },
            ),
            ("decision_id", ID),
            ("revision", VERSION),
            ("subject_ref", REF),
            ("kind", const { ascii("outcome") }),
            ("transition", const { ascii("outcome_correct") }),
            ("status", const { ascii("reversed") }),
            ("original_terms_ref", REF),
            ("authority_ref", REF),
            ("evaluation_ref", REF),
            ("actions_appended", two),
            ("active_action_refs", one),
            ("dependency_refs", empty),
            ("operation_key", key),
            ("accepted_at", TIME),
            ("prior_ref", REF),
        ],
    )?;
    let window = object(
        m,
        &[
            ("anchor_ref", REF),
            ("anchor_time", TIME),
            ("activation_deadline", TIME),
            ("starts_at", TIME),
            ("occurs_before", TIME),
            ("received_by", TIME),
            ("accepted_by", TIME),
            ("correction_until", TIME),
        ],
    )?;
    let closure = object(
        m,
        &[
            ("root_decision_ref", REF),
            ("root_expected_revision", VERSION),
            ("authority_ref", REF),
            ("original_slot_terms_ref", REF),
            ("root_component_slot", ID),
        ],
    )?;
    let family = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-family-state/2") }),
            ("target_ref", REF),
            ("family_slot", ID),
            ("revision", VERSION),
            ("state", const { ascii("closed_no_success") }),
            ("window", window),
            ("anchor_work_ref", REF),
            ("closure_evidence_refs", evidence),
            ("operation_key", key),
            ("accepted_at", TIME),
            ("prior_ref", REF),
            ("claim_ref", REF),
            ("decision_ref", REF),
            ("causal_closure", closure),
        ],
    )?;
    let chain = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-chain-state/1") }),
            ("logical_store_id", store),
            ("scope", scope),
            ("customer", customer),
            ("chain", ID),
            ("binding_ref", REF),
            ("accepted_agreement_ref", REF),
            ("activation_ref", REF),
            ("revision", VERSION),
            ("admitted_events", VERSION),
            ("links_root", Body::scalar(73)),
            ("operation_key", key),
            ("accepted_at", TIME),
            ("prior_ref", REF),
        ],
    )?;
    let rights = array(m, 2, const { ascii("correct") })?;
    let authority = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-economic-authority/7") },
            ),
            ("operation", const { ascii("quantity.correct") }),
            ("actor", ID),
            ("source", SOURCE),
            ("scope", scope),
            ("customer", customer),
            ("canonical_economic_namespace", SOURCE),
            ("installation_mode", const { ascii("isolated_synthetic") }),
            ("enrollment_ref", REF),
            ("current_state_ref", REF),
            ("current_grant_ref", REF),
            ("original_binding_ref", REF),
            ("original_accepted_agreement_ref", REF),
            ("original_grant_refs", original_grants),
            ("source_slot", NAME),
            ("rights", rights),
            ("evidence_refs", evidence),
            ("verified_fact_refs", verified),
            ("observed_at", TIME),
            ("authority_observation_ref", REF),
            ("causal_closure_checks", empty),
            ("app_observation_refs", empty),
            ("app_final_evidence_refs", empty),
            ("app_cost_revision_refs", empty),
            ("app_cost_closure_checks", empty),
            ("app_pending_cost_closure_checks", empty),
        ],
    )?;
    let mut guard = 0;
    for body in [
        claim, link, action, evaluation, decision, family, chain, authority,
    ] {
        guard = largest(m, guard, body.guard)?;
    }
    Ok(FamilyBodyBounds {
        facts,
        _claim: claim.bytes,
        _link: link.bytes,
        action: action.bytes,
        evaluation: evaluation.bytes,
        decision: decision.bytes,
        family_state: family.bytes,
        _chain: chain.bytes,
        authority: authority.bytes,
        _maximum_decoded_body_guard: guard,
    })
}

/// Root correction/reverse changing records. This covers the root's own
/// authority/action/evaluation/decision/chain bodies, independently of dependent
/// records. Observations, immediate closures, operation, dependency/capacity
/// bodies and the containing semantic/representation/ownership proof remain.
pub(in crate::policy::workflow::transition_constructor) struct RootBodyBounds {
    pub action: usize,
    pub evaluation: usize,
    pub decision: usize,
    pub authority: usize,
    pub chain: usize,
    pub _maximum_decoded_body_guard: usize,
}
pub(in crate::policy::workflow::transition_constructor) fn root_bodies(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    payload: &Value,
    terms: &super::super::authority::Terms,
    logical_store: &Value,
    maximum_original_actions: usize,
    selected_operands: (usize, usize, Option<&AppBodyBounds>),
) -> Result<RootBodyBounds> {
    let (families, apps, app_bodies) = selected_operands;
    let facts = facts_for_reference(m, program, &payload["fact_schema_ref"])?;
    let context = exact(m, &terms.binding["accepted_context"])?;
    let scope = exact(m, &terms.slot_terms["scope"])?;
    let customer = exact(m, &terms.binding["customer"])?;
    let store = exact(m, logical_store)?;
    let key = object(
        m,
        &[
            ("logical_store_id", store),
            ("scope", scope),
            ("source", SOURCE),
            ("operation_id", ID),
        ],
    )?;
    let empty = Body {
        bytes: 2,
        guard: 4,
        depth: 0,
    };
    let evidence = array(m, 16, REF)?;
    let root_active = array(m, maximum_original_actions, REF)?;
    let appended_count = mul(m, 2, maximum_original_actions)?;
    let appended = array(m, appended_count, REF)?;
    let dependencies = add(m, families, apps)?;
    let dependencies = array(m, dependencies, REF)?;
    let action = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-action/1") }),
            ("effect_id", ID),
            ("decision_id", ID),
            ("revision", VERSION),
            ("posting_role", const { ascii("replacement") }),
            ("book", const { ascii("retail") }),
            ("component_slot", ID),
            ("root_component_slot", ID),
            ("role_ref", REF),
            ("slot_terms_ref", REF),
            ("funding_leg", NAME),
            ("amount", AMOUNT),
            ("inverse_of", REF),
            ("basis_refs", root_active),
            ("operation_key", key),
        ],
    )?;
    let step = object(
        m,
        &[
            ("instruction", VERSION),
            ("component_slot", ID),
            ("predicate", const { ascii("true") }),
            ("basis_refs", root_active),
            ("rational_numerator", RATIONAL_NUMERATOR),
            ("rational_denominator", RATIONAL_DENOMINATOR),
            ("rounded", AMOUNT),
            ("disposition", const { ascii("inverse") }),
            ("action_ref", REF),
        ],
    )?;
    let steps = array(m, appended_count, step)?;
    let evaluation = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-evaluation-proof/1") },
            ),
            ("subject_ref", REF),
            ("policy_ref", REF),
            ("compiled_plan_ref", REF),
            ("compiler_ref", REF),
            ("binding_ref", REF),
            ("fact_schema_ref", REF),
            ("facts", facts),
            ("context", context),
            ("steps", steps),
        ],
    )?;
    let decision = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-decision-revision/1") },
            ),
            ("decision_id", ID),
            ("revision", VERSION),
            ("subject_ref", REF),
            ("kind", const { ascii("work") }),
            ("transition", const { ascii("quantity_correct") }),
            ("status", const { ascii("reversed") }),
            ("original_terms_ref", REF),
            ("authority_ref", REF),
            ("evaluation_ref", REF),
            ("actions_appended", appended),
            ("active_action_refs", root_active),
            ("dependency_refs", dependencies),
            ("operation_key", key),
            ("accepted_at", TIME),
            ("prior_ref", REF),
        ],
    )?;
    let family_check = object(
        m,
        &[
            ("family_state_ref", REF),
            ("expected_revision", VERSION),
            ("original_slot_terms_ref", REF),
            ("root_component_slot", ID),
            ("family_slot", ID),
            ("dependent_authority_ref", REF),
            ("authority_evidence_refs", evidence),
        ],
    )?;
    let family_checks = array(m, families, family_check)?;
    let (app_checks, pending_checks) = if w::compare(m, apps, 0usize)?.is_gt() {
        let app = app_bodies.ok_or_else(|| shape::fail("root app changing-body bound missing"))?;
        let finalized = Body {
            bytes: app.finalized_parent_check,
            guard: app.decoded.finalized_parent_check,
            depth: 3,
        };
        let pending = Body {
            bytes: app.pending_parent_check,
            guard: app.decoded.pending_parent_check,
            depth: 3,
        };
        // Disjoint branches share apps, but summing both arrays is a safe
        // structural overcount without using today's finalized/pending state.
        (array(m, apps, finalized)?, array(m, apps, pending)?)
    } else {
        (empty, empty)
    };
    let grants = shape::array(m, &terms.slot_terms, "source_grant_refs")?.len();
    let grants = array(m, grants, REF)?;
    let rights = array(m, 2, const { ascii("correct") })?;
    let authority = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-economic-authority/7") },
            ),
            ("operation", const { ascii("quantity.correct") }),
            ("actor", ID),
            ("source", SOURCE),
            ("scope", scope),
            ("customer", customer),
            ("canonical_economic_namespace", SOURCE),
            ("installation_mode", const { ascii("isolated_synthetic") }),
            ("enrollment_ref", REF),
            ("current_state_ref", REF),
            ("current_grant_ref", REF),
            ("original_binding_ref", REF),
            ("original_accepted_agreement_ref", REF),
            ("original_grant_refs", grants),
            ("source_slot", NAME),
            ("rights", rights),
            ("evidence_refs", evidence),
            ("verified_fact_refs", empty),
            ("observed_at", TIME),
            ("authority_observation_ref", REF),
            ("causal_closure_checks", family_checks),
            ("app_observation_refs", empty),
            ("app_final_evidence_refs", empty),
            ("app_cost_revision_refs", empty),
            ("app_cost_closure_checks", app_checks),
            ("app_pending_cost_closure_checks", pending_checks),
        ],
    )?;
    let chain = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-chain-state/1") }),
            ("logical_store_id", store),
            ("scope", scope),
            ("customer", customer),
            ("chain", ID),
            ("binding_ref", REF),
            ("accepted_agreement_ref", REF),
            ("activation_ref", REF),
            ("revision", VERSION),
            ("admitted_events", VERSION),
            ("links_root", Body::scalar(73)),
            ("operation_key", key),
            ("accepted_at", TIME),
            ("prior_ref", REF),
        ],
    )?;
    let mut guard = 0;
    for body in [action, evaluation, decision, authority, chain] {
        guard = largest(m, guard, body.guard)?;
    }
    Ok(RootBodyBounds {
        action: action.bytes,
        evaluation: evaluation.bytes,
        decision: decision.bytes,
        authority: authority.bytes,
        chain: chain.bytes,
        _maximum_decoded_body_guard: guard,
    })
}

pub(in crate::policy::workflow::transition_constructor) struct AppBodyBounds {
    pub action: usize,
    pub evaluation: usize,
    pub _claim: usize,
    pub consumption: usize,
    pub revision: usize,
    pub decision: usize,
    pub standalone_authority: usize,
    pub _finalized_authority: usize,
    pub _pending_authority: usize,
    pub finalized_parent_check: usize,
    pub pending_parent_check: usize,
    pub _retirement: usize,
    pub _observation: usize,
    pub final_evidence: usize,
    pub evidence_verification: usize,
    pub _finalize_new_bodies: usize,
    pub _correct_new_bodies: usize,
    pub finalized_causal_new_bodies: usize,
    pub pending_causal_new_bodies: usize,
    pub decoded: AppDecodedBounds,
}
/// Named app-body selected-guard contributions. These are neither the
/// economic executor total nor a bound on arbitrary unvalidated JSON.
pub(in crate::policy::workflow::transition_constructor) struct AppDecodedBounds {
    pub _finalize_new_bodies: usize,
    pub _correct_new_bodies: usize,
    pub _finalized_causal_new_bodies: usize,
    pub _pending_causal_new_bodies: usize,
    pub _observation: usize,
    pub final_evidence: usize,
    pub evidence_verification: usize,
    pub action: usize,
    pub evaluation: usize,
    pub _claim: usize,
    pub consumption: usize,
    pub revision: usize,
    pub decision: usize,
    pub standalone_authority: usize,
    pub _finalized_authority: usize,
    pub _pending_authority: usize,
    pub finalized_parent_check: usize,
    pub pending_parent_check: usize,
    pub _retirement: usize,
}
/// Actual immutable original payload/context remain fixed for standalone app
/// pricing and causal inversion. The caller supplies the SAME validated terms,
/// not an independently supplied maximum or saved capacity assertion.
pub(in crate::policy::workflow::transition_constructor) fn derive(
    m: &mut CoreMeter,
    logical_store: &Value,
    payload: &Value,
    terms: &super::super::authority::Terms,
) -> Result<AppBodyBounds> {
    // Scope/store/customer/agreement are original immutable values. Use actual
    // JCS lengths so escaping and UTF8 cannot be confused with character count.
    let scope = exact(m, &terms.slot_terms["scope"])?;
    let customer = exact(m, &terms.binding["customer"])?;
    let agreement = exact(m, &terms.binding["agreement_id"])?;
    let store = exact(m, logical_store)?;
    let key = object(
        m,
        &[
            ("logical_store_id", store),
            ("scope", scope),
            ("source", SOURCE),
            ("operation_id", ID),
        ],
    )?;
    let empty = Body {
        bytes: 2,
        guard: 4,
        depth: 0,
    };
    let one_ref = array(m, 1, REF)?;
    let two_refs = array(m, 2, REF)?;
    let three_refs = array(m, 3, REF)?;
    let grants = shape::array(m, &terms.slot_terms, "source_grant_refs")?;
    let original_grants = array(m, grants.len(), REF)?;
    let call = object(m, &[("source_namespace", SOURCE), ("call_id", ID)])?;
    let subject = object(
        m,
        &[
            ("logical_store_id", store),
            ("scope", scope),
            ("customer", customer),
            ("agreement_id", agreement),
            ("original_work_ref", REF),
            ("original_binding_ref", REF),
            ("component_slot", ID),
            ("component_key", NAME),
            ("app_call", call),
        ],
    )?;
    let provider = object(m, &[("provider", ID), ("account", ID), ("request_id", ID)])?;
    let material = object(
        m,
        &[
            ("media_type", const { ascii("application/octet-stream") }),
            ("content_base64", Body::scalar(5464 + 2)),
            ("content_sha256", Body::scalar(64 + 2)),
        ],
    )?;
    let observation = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-observation/1") },
            ),
            ("attestation_id", ID),
            ("source", SOURCE),
            ("evidence_kind", const { ascii("app_usage") }),
            ("subject", subject),
            ("usage_field", NAME),
            ("quantity", Body::scalar(64 + 2)),
            ("unit", NAME),
            ("observed_at", TIME),
            ("provider_key", provider),
            ("material", material),
        ],
    )?;
    let comparison = object(
        m,
        &[
            ("kind", const { ascii("comparable") }),
            ("quantity", Body::scalar(64 + 2)),
            ("unit", NAME),
        ],
    )?;
    let unavailable = object(
        m,
        &[
            ("kind", const { ascii("unavailable") }),
            ("reason", const { ascii("not_comparable") }),
        ],
    )?;
    let comparison = maximum(m, comparison, unavailable)?;
    let final_evidence = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-final-cost/1") },
            ),
            ("attestation_id", ID),
            ("source", SOURCE),
            ("evidence_kind", const { ascii("app_final_cost") }),
            ("subject", subject),
            ("observation_ref", REF),
            ("cost_revision", VERSION),
            ("prior_final_evidence_ref", REF),
            ("finality", const { ascii("final_attributable_amount") }),
            ("amount", AMOUNT),
            ("quantity_comparison", comparison),
            ("issued_at", TIME),
            ("material", material),
        ],
    )?;
    let evidence_verification = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-evidence-verification/1") },
            ),
            ("attestation_ref", REF),
            ("attestation_raw_sha256", Body::scalar(66)),
            ("attestation_digest", Body::scalar(73)),
            ("verified_source", SOURCE),
            (
                "verification_primitive",
                const { ascii("authenticated-nominated-source-retention/1") },
            ),
            ("source_grant_ref", REF),
            ("source_grant_revision", VERSION),
            ("authenticated_principal_ref", REF),
            ("trusted_received_at", TIME),
            ("subject_binding_refs", three_refs),
            ("verification_evidence_refs", empty),
        ],
    )?;
    let claim = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-cost-claim/1") },
            ),
            ("subject", subject),
            ("observation_ref", REF),
            ("original_final_evidence_ref", REF),
            ("original_terms_ref", REF),
            ("dependency_ref", REF),
            ("operation_key", key),
        ],
    )?;
    let consumption = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-cost-consumption/1") },
            ),
            ("cost_claim_ref", REF),
            ("observation_ref", REF),
            ("final_evidence_ref", REF),
            ("cost_revision", VERSION),
            ("eligible_amount", AMOUNT),
            ("operation_key", key),
        ],
    )?;
    let revision = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-cost-revision/1") },
            ),
            ("cost_claim_ref", REF),
            ("revision", VERSION),
            ("prior_ref", REF),
            ("current_decision_ref", REF),
            ("current_evidence_ref", REF),
            ("consumption_ref", REF),
            ("status", const { ascii("reversed") }),
            ("operation_key", key),
        ],
    )?;
    let decision = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-decision-revision/3") },
            ),
            ("decision_id", ID),
            ("revision", VERSION),
            ("prior_ref", REF),
            ("subject_ref", REF),
            ("kind", const { ascii("cost") }),
            ("transition", const { ascii("dependent_reverse") }),
            ("status", const { ascii("reversed") }),
            ("original_terms_ref", REF),
            ("authority_ref", REF),
            ("evaluation_ref", REF),
            ("actions_appended", two_refs),
            ("active_action_refs", one_ref),
            ("dependency_refs", one_ref),
            ("operation_key", key),
            ("accepted_at", TIME),
        ],
    )?;
    let action = object(
        m,
        &[
            ("schema", const { ascii("ledger-workflow-action/1") }),
            ("effect_id", ID),
            ("decision_id", ID),
            ("revision", VERSION),
            ("posting_role", const { ascii("replacement") }),
            ("book", const { ascii("retail") }),
            ("component_slot", ID),
            ("root_component_slot", ID),
            ("role_ref", REF),
            ("slot_terms_ref", REF),
            ("funding_leg", NAME),
            ("amount", AMOUNT),
            ("inverse_of", REF),
            ("basis_refs", empty),
            ("operation_key", key),
        ],
    )?;
    let step = object(
        m,
        &[
            ("instruction", VERSION),
            ("component_slot", ID),
            ("predicate", const { ascii("true") }),
            ("basis_refs", empty),
            ("rational_numerator", RATIONAL_NUMERATOR),
            ("rational_denominator", RATIONAL_DENOMINATOR),
            ("rounded", AMOUNT),
            ("disposition", const { ascii("inverse") }),
            ("action_ref", REF),
        ],
    )?;
    let steps = array(m, 2, step)?;
    let facts = exact(m, &payload["facts"])?;
    let context = exact(m, &terms.binding["accepted_context"])?;
    let evaluation = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-evaluation-proof/1") },
            ),
            ("subject_ref", REF),
            ("policy_ref", REF),
            ("compiled_plan_ref", REF),
            ("compiler_ref", REF),
            ("binding_ref", REF),
            ("fact_schema_ref", REF),
            ("facts", facts),
            ("context", context),
            ("steps", steps),
        ],
    )?;
    let rights = array(m, 2, const { ascii("submit") })?;
    let standalone_authority = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-economic-authority/7") },
            ),
            ("operation", const { ascii("evidence.retain") }),
            ("actor", ID),
            ("source", SOURCE),
            ("scope", scope),
            ("customer", customer),
            ("canonical_economic_namespace", SOURCE),
            ("installation_mode", const { ascii("isolated_synthetic") }),
            ("enrollment_ref", REF),
            ("current_state_ref", REF),
            ("current_grant_ref", REF),
            ("original_binding_ref", REF),
            ("original_accepted_agreement_ref", REF),
            ("original_grant_refs", original_grants),
            ("source_slot", NAME),
            ("rights", rights),
            ("evidence_refs", empty),
            ("verified_fact_refs", empty),
            ("observed_at", TIME),
            ("authority_observation_ref", REF),
            ("causal_closure_checks", empty),
            ("app_observation_refs", one_ref),
            ("app_final_evidence_refs", two_refs),
            ("app_cost_revision_refs", one_ref),
            ("app_cost_closure_checks", empty),
            ("app_pending_cost_closure_checks", empty),
        ],
    )?;
    let clause = object(
        m,
        &[
            (
                "root_relation",
                const { ascii("original_work_delayed_cost_dependency") },
            ),
            ("mode", const { ascii("exact_inverse_and_permanent_close") }),
            (
                "standalone_deadline_effect",
                const { ascii("does_not_expire_preaccepted_causal_reversal") },
            ),
        ],
    )?;
    // Bounds include the optional clause; their sum need not itself be a valid
    // branch instance. The exact base fields are overcounted when replaced.
    let evidence_refs = array(m, 16, REF)?;
    let extras = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-causal-authority/1") },
            ),
            ("evidence_refs", evidence_refs),
            ("causal_root_ref", REF),
            ("causal_root_operation", const { ascii("quantity.correct") }),
            ("original_work_ref", REF),
            ("dependency_ref", REF),
            ("component_slot", ID),
            ("original_app_terms_ref", REF),
            ("standalone_correction_until", TIME),
            (
                "deadline_basis",
                const { ascii("original_standalone_window") },
            ),
            ("initial_cost_decision_ref", REF),
            ("initial_cost_accepted_at", TIME),
            ("causal_reversal", clause),
        ],
    )?;
    let finalized_authority = sum(m, standalone_authority, extras)?;
    let pending_extras = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-app-pending-causal-authority/1") },
            ),
            ("causal_root_ref", REF),
            ("causal_root_operation", const { ascii("quantity.correct") }),
            ("original_work_ref", REF),
            ("dependency_ref", REF),
            ("component_slot", ID),
            ("original_app_terms_ref", REF),
            ("financial_observation_ref", REF),
            ("absence_kind", const { ascii("no_initial_app_cost_claim") }),
            ("causal_reversal", clause),
        ],
    )?;
    let pending_authority = sum(m, standalone_authority, pending_extras)?;
    let finalized_parent_check = object(
        m,
        &[
            ("original_work_ref", REF),
            ("original_cost_claim_ref", REF),
            ("current_cost_revision_ref", REF),
            ("current_decision_ref", REF),
            ("dependency_ref", REF),
            ("original_binding_ref", REF),
            ("original_terms_ref", REF),
            ("original_source_grant_refs", original_grants),
            ("current_source_grant_ref", REF),
            ("correction_source_slot", NAME),
            ("causal_root_ref", REF),
            ("causal_root_operation", const { ascii("quantity.correct") }),
            ("component_slot", ID),
            ("dependent_authority_ref", REF),
            (
                "deadline_basis",
                const { ascii("original_standalone_window") },
            ),
            ("initial_cost_decision_ref", REF),
        ],
    )?;
    let pending_parent_check = object(
        m,
        &[
            ("original_work_ref", REF),
            ("dependency_ref", REF),
            ("original_binding_ref", REF),
            ("original_terms_ref", REF),
            ("original_source_grant_refs", original_grants),
            ("current_source_grant_ref", REF),
            ("correction_source_slot", NAME),
            ("causal_root_ref", REF),
            ("causal_root_operation", const { ascii("quantity.correct") }),
            ("component_slot", ID),
            ("dependent_authority_ref", REF),
            ("financial_observation_ref", REF),
            ("absence_kind", const { ascii("no_initial_app_cost_claim") }),
        ],
    )?;
    let retirement = object(
        m,
        &[
            (
                "schema",
                const { ascii("ledger-workflow-capacity-retirement/1") },
            ),
            ("dependency_ref", REF),
            ("closure_state_refs", one_ref),
            ("reason", const { ascii("no_future_or_active_dependency") }),
            ("authority_ref", REF),
            ("operation_key", key),
        ],
    )?;
    let mut finalize_new_bodies = Body {
        bytes: 0,
        guard: 0,
        depth: 0,
    };
    for n in [
        claim,
        standalone_authority,
        action,
        evaluation,
        decision,
        consumption,
        revision,
    ] {
        finalize_new_bodies = sum(m, finalize_new_bodies, n)?;
    }
    let mut correct_new_bodies = Body {
        bytes: 0,
        guard: 0,
        depth: 0,
    };
    for n in [
        standalone_authority,
        action,
        action,
        evaluation,
        decision,
        consumption,
        revision,
    ] {
        correct_new_bodies = sum(m, correct_new_bodies, n)?;
    }
    let mut finalized_causal_new_bodies = Body {
        bytes: 0,
        guard: 0,
        depth: 0,
    };
    for n in [finalized_authority, action, evaluation, decision, revision] {
        finalized_causal_new_bodies = sum(m, finalized_causal_new_bodies, n)?;
    }
    let pending_causal_new_bodies = sum(m, pending_authority, retirement)?;
    let decoded = AppDecodedBounds {
        action: action.guard,
        evaluation: evaluation.guard,
        _claim: claim.guard,
        consumption: consumption.guard,
        revision: revision.guard,
        decision: decision.guard,
        standalone_authority: standalone_authority.guard,
        _finalized_authority: finalized_authority.guard,
        _pending_authority: pending_authority.guard,
        finalized_parent_check: finalized_parent_check.guard,
        pending_parent_check: pending_parent_check.guard,
        _retirement: retirement.guard,
        _observation: observation.guard,
        final_evidence: final_evidence.guard,
        evidence_verification: evidence_verification.guard,
        _finalize_new_bodies: finalize_new_bodies.guard,
        _correct_new_bodies: correct_new_bodies.guard,
        _finalized_causal_new_bodies: finalized_causal_new_bodies.guard,
        _pending_causal_new_bodies: pending_causal_new_bodies.guard,
    };
    Ok(AppBodyBounds {
        action: action.bytes,
        evaluation: evaluation.bytes,
        _claim: claim.bytes,
        consumption: consumption.bytes,
        revision: revision.bytes,
        decision: decision.bytes,
        standalone_authority: standalone_authority.bytes,
        _finalized_authority: finalized_authority.bytes,
        _pending_authority: pending_authority.bytes,
        finalized_parent_check: finalized_parent_check.bytes,
        pending_parent_check: pending_parent_check.bytes,
        _retirement: retirement.bytes,
        _observation: observation.bytes,
        final_evidence: final_evidence.bytes,
        evidence_verification: evidence_verification.bytes,
        _finalize_new_bodies: finalize_new_bodies.bytes,
        _correct_new_bodies: correct_new_bodies.bytes,
        finalized_causal_new_bodies: finalized_causal_new_bodies.bytes,
        pending_causal_new_bodies: pending_causal_new_bodies.bytes,
        decoded,
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::codec;
    use super::*;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        validation_work::OriginalAuditSession,
    };
    use serde_json::json;
    #[test]
    fn prospective_closed_body_guard_counts_structure_independently_of_string_bytes() {
        let reference = json!({"kind":"workflow-work","id":"work","digest":"digest"});
        for count in [0, 1, 32] {
            let facts = json!({"units":"100","label":"x".repeat(10000),"flag":true,"scale":18});
            let value = json!({"facts":facts,"refs":vec![reference.clone();count]});
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let m = session.meter_mut();
            let fact_shape = exact(m, &value["facts"]).unwrap();
            let refs = array(m, count, REF).unwrap();
            let bound = object(m, &[("facts", fact_shape), ("refs", refs)]).unwrap();
            let mut actual = CoreMeter::new(65536).unwrap();
            codec::validate_selected_types(&mut actual, &value).unwrap();
            assert!(actual.usage().semantic_work <= bound.guard as u64);
            assert!(
                canonical::CanonicalBytes::from_value(&value)
                    .unwrap()
                    .as_slice()
                    .len()
                    <= bound.bytes
            );
            assert_eq!(bound.depth, if count == 0 { 2 } else { 3 });
        }
        let fields = json!([
            {"name":"example:text","required":true,"type":{"kind":"string","min_bytes":"0","max_bytes":"8"},"provenance":{"kind":"caller"}},
            {"name":"example:choice","required":true,"type":{"kind":"enum","values":["example:a","example:z"]},"provenance":{"kind":"caller"}},
            {"name":"example:integer","required":true,"type":{"kind":"integer","min":"-999999999999999999999999999999","max":"999999999999999999999999999999"},"provenance":{"kind":"caller"}},
            {"name":"example:decimal","required":true,"type":{"kind":"decimal","min":"-999999999999999999999999999999","max":"999999999999999999999999999999","max_scale":"18"},"provenance":{"kind":"caller"}},
            {"name":"example:flag","required":false,"type":{"kind":"boolean"},"provenance":{"kind":"caller"}}
        ]);
        let schema=super::super::super::super::FactSchema::parse(&serde_json::to_vec(&json!({"schema":"ledger-workflow-fact-schema/1","schema_id":"example:range","version":"1","normalization":"ledger-facts-normalization/1","fields":fields})).unwrap()).unwrap();
        let input = json!({"example:text":"\"".repeat(8),"example:choice":"example:z","example:integer":"-999999999999999999999999999999","example:decimal":"-999999999999.999999999999999999","example:flag":false});
        let normalized = schema
            .normalize(&serde_json::to_vec(&input).unwrap())
            .unwrap();
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let bound = normalized_facts(session.meter_mut(), schema.canonical_value()).unwrap();
        let actual = canonical::CanonicalBytes::from_value(normalized.canonical_value()).unwrap();
        assert!(actual.as_slice().len() <= bound.bytes);
        let mut guard = CoreMeter::new(65536).unwrap();
        codec::validate_selected_types(&mut guard, normalized.canonical_value()).unwrap();
        assert!(guard.usage().semantic_work <= bound.guard as u64);
        assert_eq!(bound.depth, 1);
        let mut m = CoreMeter::new(65536).unwrap();
        let too_deep = Body {
            bytes: 2,
            guard: 5,
            depth: 32,
        };
        assert!(object(&mut m, &[("extra", too_deep)]).is_err());
    }
}
