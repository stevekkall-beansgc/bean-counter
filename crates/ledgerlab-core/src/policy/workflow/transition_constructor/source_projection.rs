//! Borrowed nominated-source DATA checks for actual app acceptance. Host owns
//! authenticated original births, held current heads and completed origins;
//! this produces no admission/commit/origin capability or substitute actor.
use super::super::selected_program::SelectedProgram;
use super::{
    authority, data::ExactRef, inventory, meter, representation::InvocationToken, shape, CoreMeter,
};
use crate::{
    domain::Timestamp,
    validation_work::{self as w, OriginalAuditSession},
    Result,
};
use serde_json::Value;

pub struct NominatedSourceGrant<'a> {
    pub reference: &'a ExactRef,
    pub body: &'a Value,
}
pub struct NominatedSourceFrame<'a> {
    pub enrollment_ref: &'a ExactRef,
    pub enrollment: &'a Value,
    pub current_state_ref: &'a ExactRef,
    pub current_state: &'a Value,
    pub current_grant_ref: &'a ExactRef,
    pub current_grant: &'a Value,
    pub original_grants: &'a [NominatedSourceGrant<'a>],
}
fn literal(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("nominated source literal"));
    }
    Ok(())
}
fn text_member(m: &mut CoreMeter, rows: &[Value], want: &str) -> Result<bool> {
    for row in rows {
        m.visit("accept.source_text_member")?;
        let value = row
            .as_str()
            .ok_or_else(|| shape::fail("nominated source string set"))?;
        if shape::same_text(m, value, want)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn reference_parts<'a>(m: &mut CoreMeter, value: &'a Value) -> Result<(&'a str, &'a str, &'a str)> {
    shape::closed(m, value, &["kind", "id", "digest"], &[])?;
    let kind = shape::text(m, value, "kind")?;
    let id = shape::text(m, value, "id")?;
    let digest = shape::text(m, value, "digest")?;
    super::records::reference_parts(m, kind, id, digest)?;
    Ok((kind, id, digest))
}
fn ref_value(m: &mut CoreMeter, value: &Value, reference: &ExactRef) -> Result<()> {
    let (kind, id, digest) = reference_parts(m, value)?;
    if !shape::same_text(m, kind, &reference.kind)?
        || !shape::same_text(m, id, &reference.id)?
        || !shape::same_text(m, digest, &reference.digest)?
    {
        return Err(shape::fail("nominated source exact Ref association"));
    }
    Ok(())
}
fn ref_member(m: &mut CoreMeter, rows: &[Value], reference: &ExactRef) -> Result<bool> {
    for row in rows {
        m.visit("accept.source_ref_member")?;
        let (kind, id, digest) = reference_parts(m, row)?;
        if shape::same_text(m, kind, &reference.kind)?
            && shape::same_text(m, id, &reference.id)?
            && shape::same_text(m, digest, &reference.digest)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn source<'a>(m: &mut CoreMeter, frame: &NominatedSourceFrame<'a>) -> Result<&'a str> {
    shape::text(m, &frame.enrollment["ceiling"], "source")
}
fn nominated(m: &mut CoreMeter, binding: &Value, slot: &str, name: &str) -> Result<bool> {
    let rows = shape::array(m, binding, "source_slots")?;
    let row = shape::named(m, rows, "name", slot)?;
    {
        let sources = shape::array(m, row, "sources")?;
        text_member(m, sources, name)
    }
}
fn grant_data(
    m: &mut CoreMeter,
    g: &Value,
    frame: &NominatedSourceFrame<'_>,
    at: &num_bigint::BigInt,
    token: &InvocationToken,
) -> Result<()> {
    authority::grant_shape(m, g)?;
    literal(m, g, "schema", "ledger-workflow-source-grant/1")?;
    let ceiling = shape::field(m, frame.enrollment, "ceiling")?;
    for key in ["principal_id", "source", "canonical_economic_namespace"] {
        shape::equal(m, &g[key], &ceiling[key])?;
    }
    ref_value(m, &g["original_ceiling_ref"], frame.enrollment_ref)?;
    shape::equal(m, &g["grant_id"], &frame.enrollment["enrollment_id"])?;
    authority::grant_intersection(m, token, g, ceiling, at)
}
fn check_frame(
    m: &mut CoreMeter,
    binding: &Value,
    terms: &Value,
    frame: &NominatedSourceFrame<'_>,
    at: &num_bigint::BigInt,
    token: &InvocationToken,
) -> Result<()> {
    literal(
        m,
        frame.enrollment,
        "schema",
        "ledger-workflow-source-enrollment/1",
    )?;
    literal(
        m,
        frame.current_state,
        "schema",
        "ledger-workflow-source-state/1",
    )?;
    literal(m, frame.current_state, "status", "active")?;
    ref_value(
        m,
        &frame.current_state["enrollment_ref"],
        frame.enrollment_ref,
    )?;
    ref_value(
        m,
        &frame.current_state["current_grant_ref"],
        frame.current_grant_ref,
    )?;
    shape::equal(
        m,
        &frame.current_state["revision"],
        &frame.current_grant["revision"],
    )?;
    shape::equal(m, &frame.enrollment["scope"], &terms["scope"])?;
    let state_at =
        Timestamp::parse_with_work(shape::text(m, frame.current_state, "accepted_at")?, m)?
            .micros();
    if meter::cmp(m, &state_at.into(), at)?.is_gt() {
        return Err(shape::fail("future nominated source state"));
    }
    let ceiling = shape::field(m, frame.enrollment, "ceiling")?;
    let start = Timestamp::parse_with_work(shape::text(m, ceiling, "starts_at")?, m)?.micros();
    let end = Timestamp::parse_with_work(shape::text(m, ceiling, "ends_at")?, m)?.micros();
    if !w::compare(m, start, end)?.is_lt() {
        return Err(shape::fail("nominated original ceiling interval"));
    }
    // The birth/ceiling full grammar is checked by its actual original origin.
    // Re-execute current intrinsic set/subset/interval rules at acceptance.
    grant_data(m, frame.current_grant, frame, at, token)?;
    if !w::compare(m, frame.original_grants.len(), 0usize)?.is_gt() {
        return Err(shape::fail("missing nominated original grants"));
    }
    for (i, grant) in frame.original_grants.iter().enumerate() {
        m.visit("accept.original_nominated_grant")?;
        if !shape::same_text(m, &grant.reference.kind, "workflow-source-grant")? {
            return Err(shape::fail("nominated original grant kind"));
        }
        for previous in &frame.original_grants[..i] {
            if shape::same_ref(m, previous.reference, grant.reference)? {
                return Err(shape::fail("repeated nominated original grant"));
            }
        }
        let terms_refs = shape::array(m, terms, "source_grant_refs")?;
        let binding_refs = shape::array(m, binding, "authority_refs")?;
        if !ref_member(m, terms_refs, grant.reference)?
            || !ref_member(m, binding_refs, grant.reference)?
        {
            return Err(shape::fail("grant absent original terms/binding"));
        }
        grant_data(m, grant.body, frame, at, token)?;
    }
    Ok(())
}
fn requirement(
    m: &mut CoreMeter,
    binding: &Value,
    frames: &[NominatedSourceFrame<'_>],
    activity: &str,
    product: &str,
    slot: &str,
    operation: &str,
    rights: &[&str],
) -> Result<()> {
    let nominations = shape::array(m, binding, "source_slots")?;
    let row = shape::named(m, nominations, "name", slot)?;
    let nominees = shape::array(m, row, "sources")?;
    inventory::binding::ordered(m, nominees)?;
    if !w::compare(m, nominees.len(), 0usize)?.is_gt() {
        return Err(shape::fail("empty required source nomination"));
    }
    for nominee in nominees {
        m.visit("accept.required_nominated_source")?;
        let name = nominee
            .as_str()
            .ok_or_else(|| shape::fail("nominee string"))?;
        let mut selected = None;
        for frame in frames {
            m.visit("accept.source_frame_association")?;
            let actual_source = source(m, frame)?;
            if shape::same_text(m, actual_source, name)? {
                if selected.is_some() {
                    return Err(shape::fail("ambiguous nominated source enrollment"));
                }
                selected = Some(frame);
            }
        }
        let frame =
            selected.ok_or_else(|| shape::fail("missing actual nominated source enrollment"))?;
        authority::grant_coverage(
            m,
            frame.current_grant,
            &binding["customer"],
            activity,
            product,
            slot,
            operation,
            rights,
        )?;
        // Every originally nominated grant for this source remains independently
        // checked; no union of narrower grants is manufactured.
        for grant in frame.original_grants {
            authority::grant_coverage(
                m,
                grant.body,
                &binding["customer"],
                activity,
                product,
                slot,
                operation,
                rights,
            )?;
        }
    }
    Ok(())
}
/// Derive required intersections from the SAME actual program and exact accepted
/// binding/slot terms. No caller requirement slice, selected source index,
/// completion boolean, current-grant union or monetary operation is fabricated.
/// The structural slot validator and singular admission-source observation are
/// separate required checks; this function alone cannot authorize acceptance.
pub fn validate_app_nominated_sources(
    program: &SelectedProgram,
    binding: &Value,
    slot_terms: &Value,
    frames: &[NominatedSourceFrame<'_>],
    accepted_at: &str,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let result = (|| {
        if !program.uses_app_owned_cost_recipe() {
            return Err(shape::fail("actual app program required"));
        }
        let arena = session.arena().shared_handle();
        let m = session.meter_mut();
        let token = InvocationToken::new();
        if w::compare(m, frames.len(), 1usize)?.is_lt()
            || w::compare(m, frames.len(), 32usize)?.is_gt()
        {
            return Err(shape::fail("nominated source frame count"));
        }
        // Ownership of borrowed bodies remains with the caller. These leases
        // pre-admit this function's timestamp/scalar JSON temporaries using the
        // actual source graph footprint; they do not claim a whole memory proof.
        let mut scratch = 0usize;
        for frame in frames {
            let n =
                crate::canonical::audit_arena::value_copy_payload_bound(frame.current_grant, m)?;
            scratch = w::usize_add(m, scratch, n)?;
            let n = crate::canonical::audit_arena::value_copy_payload_bound(
                &frame.enrollment["ceiling"],
                m,
            )?;
            scratch = w::usize_add(m, scratch, n)?;
        }
        let _scratch = arena.reserve_external_payload(scratch, m)?;
        let at_micros = Timestamp::parse_with_work(accepted_at, m)?.micros();
        let at = at_micros.into();
        for (i, frame) in frames.iter().enumerate() {
            m.visit("accept.nominated_source_frame")?;
            for previous in &frames[..i] {
                if shape::same_ref(m, previous.enrollment_ref, frame.enrollment_ref)? {
                    return Err(shape::fail("duplicate nominated enrollment frame"));
                }
            }
            check_frame(m, binding, slot_terms, frame, &at, &token)?;
        }
        // Close the original grant partition: no accepted original nomination
        // disappears merely because a narrower current grant was supplied.
        for reference in shape::array(m, slot_terms, "source_grant_refs")? {
            let mut count = 0usize;
            for frame in frames {
                for grant in frame.original_grants {
                    let (k, id, d) = reference_parts(m, reference)?;
                    if shape::same_text(m, k, &grant.reference.kind)?
                        && shape::same_text(m, id, &grant.reference.id)?
                        && shape::same_text(m, d, &grant.reference.digest)?
                    {
                        count = w::usize_add(m, count, 1)?;
                    }
                }
            }
            if !w::compare(m, count, 1usize)?.is_eq() {
                return Err(shape::fail("incomplete original nominated grant partition"));
            }
        }
        let policy = program.original_policy_value();
        // Exact policy/product association is also checked by slot projection;
        // it is repeated here before deriving any source requirements.
        shape::equal(m, &binding["product_ref"], &policy["product_ref"])?;
        let mut product = None;
        for document in program.source_documents() {
            m.visit("accept.original_source_product")?;
            let reference = document.reference();
            let (k, id, d) = reference_parts(m, &binding["product_ref"])?;
            if shape::same_text(m, k, reference.kind())?
                && shape::same_text(m, id, reference.id())?
                && shape::same_text(m, d, reference.digest())?
            {
                product = Some(document.canonical_value());
            }
        }
        let product = product.ok_or_else(|| shape::fail("original accepted product"))?;
        let product_name = shape::text(m, product, "product_id")?;
        let plan = program.compiled_plan().canonical_value();
        let activities = shape::array(m, plan, "activity_table")?;
        let allowed = shape::array(m, slot_terms, "allowed_activities")?;
        for activity in allowed {
            m.visit("accept.bound_activity_source_requirements")?;
            let name = activity
                .as_str()
                .ok_or_else(|| shape::fail("bound activity name"))?;
            let row = shape::named(m, activities, "activity", name)?;
            // A field's retained proof source is a real selected fact
            // declaration, not an inferred member of arbitrary evidence_refs.
            for document in program.source_documents() {
                m.visit("accept.original_attested_schema")?;
                let reference = document.reference();
                let (k, id, d) = reference_parts(m, &row["fact_schema_ref"])?;
                if !shape::same_text(m, k, reference.kind())?
                    || !shape::same_text(m, id, reference.id())?
                    || !shape::same_text(m, d, reference.digest())?
                {
                    continue;
                }
                for field in shape::array(m, document.canonical_value(), "fields")? {
                    let provenance = shape::field(m, field, "provenance")?;
                    let kind = shape::text(m, provenance, "kind")?;
                    if shape::same_text(m, kind, "attested")? {
                        let slot = shape::text(m, provenance, "source_slot")?;
                        requirement(
                            m,
                            binding,
                            frames,
                            name,
                            product_name,
                            slot,
                            "evidence.retain",
                            &["read", "submit"],
                        )?;
                    }
                }
            }
            let primitive = shape::text(m, row, "primitive")?;
            if !shape::same_text(m, primitive, "work")? {
                continue;
            }
            let work_slot = shape::text(m, row, "source_slot")?;
            requirement(
                m,
                binding,
                frames,
                name,
                product_name,
                work_slot,
                "work.accept",
                &["read", "submit"],
            )?;
            let correction_slot = shape::text(m, slot_terms, "correction_source_slot")?;
            // This original retail slot carries the existing quantity/reversal
            // promise; no source actor is substituted for its nomination.
            let mut quantity = false;
            for ceiling in shape::array(m, slot_terms, "quantity_ceilings")? {
                if shape::same(m, &ceiling["activity"], activity)? {
                    quantity = true;
                }
            }
            if quantity {
                requirement(
                    m,
                    binding,
                    frames,
                    name,
                    product_name,
                    correction_slot,
                    "quantity.correct",
                    &["correct", "read"],
                )?;
            }
            requirement(
                m,
                binding,
                frames,
                name,
                product_name,
                correction_slot,
                "work.reverse",
                &["correct", "read"],
            )?;
        }
        for family in shape::array(m, policy, "families")? {
            m.visit("accept.original_family_source_requirements")?;
            let selector = shape::field(m, family, "selector")?;
            if !shape::same(m, &selector["binding_slot"], &slot_terms["slot"])? {
                continue;
            }
            let activity = shape::text(m, family, "trigger_activity")?;
            // Deadline closing selects an independently sufficient originally
            // accepted slot, not the submitter or no-success slot by default.
            let mut deadline_covered = false;
            for nomination in shape::array(m, binding, "source_slots")? {
                let slot = shape::field(m, nomination, "name")?;
                let names = shape::array(m, nomination, "sources")?;
                for frame in frames {
                    let source = source(m, frame)?;
                    if !text_member(m, names, source)? {
                        continue;
                    }
                    let mut eligible = authority::deadline_grant_scope(
                        m,
                        frame.current_grant,
                        slot,
                        &binding["customer"],
                        activity,
                        product_name,
                        at_micros,
                    )?;
                    eligible &= authority::deadline_grant_scope(
                        m,
                        &frame.enrollment["ceiling"],
                        slot,
                        &binding["customer"],
                        activity,
                        product_name,
                        at_micros,
                    )?;
                    for original in frame.original_grants {
                        eligible &= authority::deadline_grant_scope(
                            m,
                            original.body,
                            slot,
                            &binding["customer"],
                            activity,
                            product_name,
                            at_micros,
                        )?;
                    }
                    deadline_covered |= eligible;
                }
            }
            if !deadline_covered {
                return Err(shape::fail("no sufficient original deadline source slot"));
            }
            let slot = shape::text(m, family, "source_slot")?;
            requirement(
                m,
                binding,
                frames,
                activity,
                product_name,
                slot,
                "outcome.submit",
                &["read", "submit"],
            )?;
            let slot = shape::text(m, family, "correction_source_slot")?;
            requirement(
                m,
                binding,
                frames,
                activity,
                product_name,
                slot,
                "outcome.correct",
                &["correct", "read"],
            )?;
            if let Some(close) = family.get("no_success_close") {
                let slot = shape::text(m, close, "close_source_slot")?;
                requirement(
                    m,
                    binding,
                    frames,
                    activity,
                    product_name,
                    slot,
                    "outcome.close",
                    &["read", "submit"],
                )?;
                requirement(
                    m,
                    binding,
                    frames,
                    activity,
                    product_name,
                    slot,
                    "evidence.retain",
                    &["read", "submit"],
                )?;
            }
        }
        for declaration in shape::array(m, slot_terms, "app_cost_terms")? {
            m.visit("accept.original_app_source_requirements")?;
            let activity = shape::text(m, declaration, "activity")?;
            for (field, operation, rights) in [
                (
                    "work_source_slot",
                    "evidence.retain",
                    &["read", "submit"][..],
                ),
                (
                    "cost_source_slot",
                    "evidence.retain",
                    &["read", "submit"][..],
                ),
                ("cost_source_slot", "cost.finalize", &["read", "submit"][..]),
                (
                    "correction_source_slot",
                    "cost.correct",
                    &["correct", "read"][..],
                ),
                // Revision>1 final evidence is retained by its original
                // correction nomination (retention is a separate executor).
                (
                    "correction_source_slot",
                    "evidence.retain",
                    &["read", "submit"][..],
                ),
            ] {
                let slot = shape::text(m, declaration, field)?;
                requirement(
                    m,
                    binding,
                    frames,
                    activity,
                    product_name,
                    slot,
                    operation,
                    rights,
                )?;
            }
        }
        // No unrelated source frame is admitted as a proxy for a missing one.
        for frame in frames {
            let name = source(m, frame)?;
            let mut used = false;
            for row in shape::array(m, binding, "source_slots")? {
                let slot = shape::text(m, row, "name")?;
                if nominated(m, binding, slot, name)? {
                    used = true;
                }
            }
            if !used {
                return Err(shape::fail("unassociated nominated source frame"));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
