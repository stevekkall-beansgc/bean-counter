//! Actual pending/retained financial observation DATA for the app recipe.
//! Prefix authentication and earliest introductions are host obligations; this
//! checks exact framing and the COMPLETE held head projection, never a subset.
use super::super::{
    data::*,
    record_body::RecordBody,
    records::Records,
    shape,
    working_value::{owning_reference, parse_owned_reference},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, RepresentationKind, ValidationWork},
    Result,
};
pub(in crate::policy::workflow::transition_constructor) struct CheckedObservation {
    pub observation_ref: ExactRef,
    pub verification_ref: ExactRef,
    pub observation: RecordBody,
    pub verification: RecordBody,
    _ownership: ExternalBufferLease,
    _output_ownership: Option<ExternalBufferLease>,
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn literal(m: &mut CoreMeter, v: &serde_json::Value, k: &str, wanted: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, wanted)? {
        return Err(shape::fail("financial observation closed literal"));
    }
    Ok(())
}
fn ordered(m: &mut CoreMeter, values: &[serde_json::Value]) -> Result<()> {
    if w::compare(m, values.len(), 4096usize)?.is_gt() {
        return Err(shape::fail("financial observation set count"));
    }
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning financial set comparison"))?;
    let mut previous: Option<crate::canonical::audit_arena::AuditedBytes> = None;
    for value in values {
        m.visit("financial.complete_set_order")?;
        let bytes = m.canonical_owned_representation(&arena, value, RepresentationKind::Record)?;
        if let Some(prior) = &previous {
            if !shape::byte_order(m, prior.bytes(), bytes.bytes())?.is_lt() {
                return Err(shape::fail("financial observation set order/duplicate"));
            }
        }
        previous = Some(bytes);
    }
    Ok(())
}
fn reference_grammar(m: &mut CoreMeter, v: &serde_json::Value) -> Result<()> {
    shape::closed(m, v, &["kind", "id", "digest"], &[])?;
    let k = shape::text(m, v, "kind")?;
    let i = shape::text(m, v, "id")?;
    let d = shape::text(m, v, "digest")?;
    super::super::records::reference_parts(m, k, i, d)
}
fn counter(m: &mut CoreMeter, v: &serde_json::Value, k: &str) -> Result<()> {
    let raw = shape::text(m, v, k)?;
    crate::policy::workflow::facts::counter_with_work(raw, m)?;
    Ok(())
}
fn pin(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    value: &serde_json::Value,
    state: &EconomicStateData,
) -> Result<()> {
    shape::closed(m, value, &["kind", "ref", "revision"], &[])?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning financial head pin"))?;
    let kind = shape::text(m, value, "kind")?;
    let parsed = parse_owned_reference(m, &arena, &value["ref"])?;
    let reference = parsed.reference();
    let revision = shape::text(m, value, "revision")?;
    crate::policy::workflow::facts::counter_with_work(revision, m)?;
    let mut matches = 0usize;
    if shape::same_text(m, kind, "decision")? {
        for h in &state.financial.decision_heads {
            m.visit("financial.complete_decision_pin")?;
            if shape::same_ref(m, &h.reference, reference)? {
                if !shape::same_text(m, revision, &h.revision)? {
                    return Err(shape::fail("decision pin revision"));
                }
                let body = r.get_schemas(
                    m,
                    &c.key,
                    reference,
                    "workflow-decision",
                    &[
                        "ledger-workflow-decision-revision/1",
                        "ledger-workflow-decision-revision/3",
                    ],
                )?;
                literal(m, &body, "decision_id", &h.decision_id)?;
                literal(m, &body, "revision", revision)?;
                matches = w::usize_add(m, matches, 1)?;
            }
        }
    } else if shape::same_text(m, kind, "family")? {
        for h in &state.financial.family_heads {
            m.visit("financial.complete_family_pin")?;
            if shape::same_ref(m, &h.reference, reference)? {
                if !shape::same_text(m, revision, &h.revision)? {
                    return Err(shape::fail("family pin revision"));
                }
                let body = r.get_schemas(
                    m,
                    &c.key,
                    reference,
                    "workflow-family-state",
                    &[
                        "ledger-workflow-family-state/1",
                        "ledger-workflow-family-state/2",
                    ],
                )?;
                let arena = m
                    .representation_arena()
                    .ok_or_else(|| shape::fail("family pin owner"))?;
                let target = owning_reference(m, &arena, &h.target_ref)?;
                shape::equal(m, &body["target_ref"], target.value())?;
                literal(m, &body, "family_slot", &h.family_slot)?;
                literal(m, &body, "revision", revision)?;
                matches = w::usize_add(m, matches, 1)?;
            }
        }
    } else if shape::same_text(m, kind, "chain")? {
        if let Some(h) = &state.financial.chain_head {
            if shape::same_ref(m, h, reference)? {
                let body = r.get(
                    m,
                    &c.key,
                    reference,
                    "workflow-chain-state",
                    "ledger-workflow-chain-state/1",
                )?;
                literal(m, &body, "revision", revision)?;
                matches = w::usize_add(m, matches, 1)?;
            }
        }
    } else if shape::same_text(m, kind, "capacity")? {
        for h in state.capacity_heads {
            m.visit("financial.complete_capacity_pin")?;
            if shape::same_ref(m, &h.reference, reference)? {
                if !shape::same_text(m, revision, &h.revision)? {
                    return Err(shape::fail("capacity pin revision"));
                }
                let body = r.get(
                    m,
                    &c.key,
                    reference,
                    "workflow-correction-capacity",
                    "ledger-correction-capacity/1",
                )?;
                literal(m, &body, "revision", revision)?;
                let root_ref = parse_owned_reference(m, &arena, &body["root_ref"])?;
                let root = r.get(
                    m,
                    &c.key,
                    root_ref.reference(),
                    "workflow-decision",
                    "ledger-workflow-decision-revision/1",
                )?;
                literal(m, &root, "decision_id", &h.root_decision_id)?;
                literal(m, &root, "revision", "1")?;
                matches = w::usize_add(m, matches, 1)?;
            }
        }
    } else if shape::same_text(m, kind, "app_cost")? {
        if let Some(app) = state.app {
            for h in app.cost_heads {
                m.visit("financial.complete_app_pin")?;
                if shape::same_ref(m, &h.reference, reference)? {
                    if !shape::same_text(m, revision, &h.revision)? {
                        return Err(shape::fail("app pin revision"));
                    }
                    let body = r.get(
                        m,
                        &c.key,
                        reference,
                        "workflow-cost",
                        "ledger-workflow-app-cost-revision/1",
                    )?;
                    literal(m, &body, "revision", revision)?;
                    let arena = m
                        .representation_arena()
                        .ok_or_else(|| shape::fail("app pin owner"))?;
                    let claim = owning_reference(m, &arena, &h.cost_claim_ref)?;
                    shape::equal(m, &body["cost_claim_ref"], claim.value())?;
                    matches = w::usize_add(m, matches, 1)?;
                }
            }
        }
    } else {
        return Err(shape::fail("financial app head kind"));
    }
    if !w::compare(m, matches, 1usize)?.is_eq() {
        return Err(shape::fail("financial pin lacks unique held head"));
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn select(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    r: &mut Records,
    c: &DraftContextData,
    state: &EconomicStateData,
) -> Result<CheckedObservation> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning financial observation"))?;
    let pair = state
        .observation
        .ok_or_else(|| shape::fail("complete financial observation pair missing"))?;
    let observation = r.observation_output(
        m,
        &c.key,
        pair.observation,
        "workflow-financial-history-observation",
        "ledger-workflow-financial-history-observation/3",
    )?;
    shape::closed(
        m,
        &observation,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "installation_mode",
            "root_ref",
            "pending_operation_key",
            "snapshot_through_sequence",
            "preexisting_input_refs",
            "history_edges",
            "head_pins",
            "verifier_ref",
            "prefix_digest",
            "origin_count",
            "document_count",
            "audited_retained_bytes",
            "observed_at",
        ],
        &[],
    )?;
    let verifier_ref = parse_owned_reference(m, &arena, &observation["verifier_ref"])?;
    if !shape::same_ref(m, verifier_ref.reference(), pair.verification.reference)? {
        return Err(shape::fail("financial observation exact verifier pair"));
    }
    let verification = r.observation_output(
        m,
        &c.key,
        pair.verification,
        "workflow-authority-audit-verifier",
        "ledger-authority-audit-verifier/2",
    )?;
    shape::closed(
        m,
        &verification,
        &[
            "schema",
            "profile",
            "algorithm",
            "resources",
            "artifacts",
            "origin_recipes",
        ],
        &[],
    )?;
    literal(
        m,
        &verification,
        "profile",
        "ledger-financial-origin-audit/1",
    )?;
    literal(
        m,
        &verification,
        "algorithm",
        "verified-introduction-fixed-point/1",
    )?;
    literal(
        m,
        &verification,
        "resources",
        "workflow-profile-b-origin-audit/1",
    )?;
    if super::super::audit_metadata::selected_catalog(m, program)? {
        super::super::audit_metadata::check_financial(m, &verification)?;
    }
    let key = super::super::cost_postings::operation_key(m, &arena, c)?;
    shape::equal(m, &observation["pending_operation_key"], key.value())?;
    for k in ["logical_store_id", "scope"] {
        shape::equal(m, &observation[k], &key.value()[k])?;
    }
    literal(m, &observation, "installation_mode", &c.installation_mode)?;
    literal(m, &observation, "observed_at", &c.accepted_at)?;
    let at = crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?;
    let _ = at;
    let observation_root = parse_owned_reference(m, &arena, &observation["root_ref"])?;
    for authority in &c.authorities {
        m.visit("financial.complete_source_root")?;
        let source = r.get(
            m,
            &c.key,
            authority.required_observation_ref()?,
            "workflow-authority-prefix-observation",
            "ledger-workflow-authority-prefix-observation/1",
        )?;
        let root = parse_owned_reference(m, &arena, &source["root_ref"])?;
        if !shape::same_ref(m, root.reference(), observation_root.reference())? {
            return Err(shape::fail("financial/source root association"));
        }
    }
    for k in [
        "snapshot_through_sequence",
        "origin_count",
        "document_count",
        "audited_retained_bytes",
    ] {
        counter(m, &observation, k)?;
    }
    let prefix = shape::text(m, &observation, "prefix_digest")?;
    super::super::records::reference_parts(
        m,
        &pair.observation.reference.kind,
        &pair.observation.reference.id,
        prefix,
    )?;
    let preexisting = shape::array(m, &observation, "preexisting_input_refs")?;
    ordered(m, preexisting)?;
    for value in preexisting {
        m.visit("financial.complete_preexisting_ref")?;
        reference_grammar(m, value)?;
    }
    let edges = shape::array(m, &observation, "history_edges")?;
    ordered(m, edges)?;
    for edge in edges {
        m.visit("financial.complete_history_edge")?;
        shape::closed(m, edge, &["owner_ref", "field", "target_ref"], &[])?;
        reference_grammar(m, &edge["owner_ref"])?;
        reference_grammar(m, &edge["target_ref"])?;
        shape::text(m, edge, "field")?;
    }
    let pins = shape::array(m, &observation, "head_pins")?;
    ordered(m, pins)?;
    let mut count = w::usize_add(
        m,
        state.financial.decision_heads.len(),
        state.financial.family_heads.len(),
    )?;
    count = w::usize_add(m, count, state.capacity_heads.len())?;
    if state.financial.chain_head.is_some() {
        count = w::usize_add(m, count, 1)?;
    }
    if let Some(app) = state.app {
        count = w::usize_add(m, count, app.cost_heads.len())?;
    }
    if !w::compare(m, pins.len(), count)?.is_eq() {
        return Err(shape::fail(
            "financial observation must pin every held input head",
        ));
    }
    for value in pins {
        pin(m, r, c, value, state)?;
    }
    // This association deliberately does not accept the serialized preexisting
    // list or history cuts as a completed origin. The final assembly compares
    // those sets with the actual consumed input closure, after capacity joins.
    let mut bytes = w::usize_add(m, std::mem::size_of::<CheckedObservation>(), 0)?;
    for reference in [pair.observation.reference, pair.verification.reference] {
        for s in [&reference.kind, &reference.id, &reference.digest] {
            bytes = w::usize_add(m, bytes, s.len())?;
        }
    }
    let ownership = arena.reserve_external_payload(bytes, m)?;
    Ok(CheckedObservation {
        observation_ref: copy_ref(m, pair.observation.reference)?,
        verification_ref: copy_ref(m, pair.verification.reference)?,
        observation,
        verification,
        _ownership: ownership,
        _output_ownership: None,
    })
}

/// New-versus-reused result partition uses the actual pre-operation inventory.
/// Saved replay supplies the same partition from authenticated introductions;
/// saved comparison results never make a new observation preexisting.
pub(in crate::policy::workflow::transition_constructor) fn select_and_introduce(
    m: &mut CoreMeter,
    program: &super::super::super::selected_program::SelectedProgram,
    t: &mut Construction,
    c: &DraftContextData,
    state: &EconomicStateData,
) -> Result<CheckedObservation> {
    use super::super::emitted_record::{ConstructedEnvelope, EncodedBody};
    use crate::validation_work::ArithmeticOperation;
    use std::mem;
    let mut checked = select(m, program, &mut t.records, c, state)?;
    let pair = state
        .observation
        .ok_or_else(|| shape::fail("financial observation pair missing"))?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning financial results"))?;
    if t.records
        .optional(
            m,
            &c.key,
            &checked.observation_ref,
            "workflow-financial-history-observation",
            "ledger-workflow-financial-history-observation/3",
        )?
        .is_some()
    {
        return Err(shape::fail(
            "pending financial observation is already preexisting",
        ));
    }
    let reused = t.records.optional(
        m,
        &c.key,
        &checked.verification_ref,
        "workflow-authority-audit-verifier",
        "ledger-authority-audit-verifier/2",
    )?;
    if let Some(old) = &reused {
        shape::equal(m, old, &checked.verification)?;
    }
    let count = if reused.is_some() { 1usize } else { 2usize };
    let requested = w::usize_add(m, t.new_envelopes.len(), count)?;
    let grow = w::compare(m, requested, t.new_envelopes.capacity())?.is_gt();
    let mut bytes = 0usize;
    if grow {
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[
                &requested.into(),
                &mem::size_of::<ConstructedEnvelope>().into(),
            ],
        )?;
        bytes = requested
            .checked_mul(mem::size_of::<ConstructedEnvelope>())
            .ok_or_else(|| shape::fail("financial result vector product"))?;
    }
    for (i, row) in [pair.observation, pair.verification].iter().enumerate() {
        if i == 1 && reused.is_some() {
            continue;
        }
        m.visit("financial.new_result_payload")?;
        bytes = w::usize_add(m, bytes, mem::size_of::<ExactRef>())?;
        for value in [
            &row.reference.kind,
            &row.reference.id,
            &row.reference.digest,
        ] {
            bytes = w::usize_add(m, bytes, value.len())?;
        }
        for old in &t.new_envelopes {
            m.visit("financial.new_result_unique")?;
            if shape::same_ref(m, &old.reference, row.reference)? {
                return Err(shape::fail("financial result introduced twice"));
            }
        }
    }
    let owner = arena.reserve_external_payload(bytes, m)?;
    if grow {
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[
                &t.new_envelopes.len().into(),
                &mem::size_of::<ConstructedEnvelope>().into(),
            ],
        )?;
        let copied = t
            .new_envelopes
            .len()
            .checked_mul(mem::size_of::<ConstructedEnvelope>())
            .ok_or_else(|| shape::fail("financial result relocation product"))?;
        m.scan_bytes(copied)?;
        m.write_bytes(copied)?;
        // The old producer lease remains live during this old/new overlap.
        t.new_envelopes
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("financial result vector allocation"))?;
    }
    for (i, row) in [pair.observation, pair.verification].iter().enumerate() {
        if i == 1 && reused.is_some() {
            continue;
        }
        m.visit("financial.new_authoritative_result")?;
        m.new_body(row.canonical_body.len())?;
        let bytes =
            m.copy_owned_representation(&arena, row.canonical_body, RepresentationKind::Record)?;
        let encoding = EncodedBody::owned(&arena, bytes, m)?;
        let frame = if i == 0 {
            checked.observation.clone()
        } else {
            checked.verification.clone()
        };
        t.new_envelopes.push(ConstructedEnvelope {
            reference: copy_ref(m, row.reference)?,
            canonical_body: encoding,
            decoded: Some(frame),
        });
    }
    checked._output_ownership = Some(owner);
    Ok(checked)
}
