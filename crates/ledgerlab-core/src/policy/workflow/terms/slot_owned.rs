//! Terms7 owning transforms for actual private agreement ingress. This reuses
//! the existing quantities, scalar, Ref/set sort and fixed DTO encoder helpers.
//! Complete original policy/role/catalog/rights validation remains in the SAME
//! slot validator; these bytes alone never establish an accepted right.
use super::owned::{self as o, Encode, Storage};
use super::*;
use crate::{
    canonical::audit_arena::{AuditArena, AuditedBytes, AuditedValue},
    validation_work::{
        self as w, OriginalAuditSession, RepresentationKind as Kind, ValidationVisit as Visit,
        ValidationWork,
    },
};
use std::mem;
fn order(parts: &mut [AuditedBytes], work: &mut dyn ValidationWork) -> Result<()> {
    paid::sort(parts, work, |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(a.bytes().len())?;
        w.scan_bytes(b.bytes().len())?;
        Ok(a.bytes().cmp(b.bytes()))
    })?;
    for pair in parts.windows(2) {
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(pair[0].bytes().len())?;
        work.scan_bytes(pair[1].bytes().len())?;
        if pair[0].bytes() == pair[1].bytes() {
            return Err(fail("duplicate slot set item"));
        }
    }
    Ok(())
}
fn join(
    parts: &[AuditedBytes],
    storage: &mut Storage,
    arena: &AuditArena,
    work: &mut dyn ValidationWork,
) -> Result<AuditedBytes> {
    let mut n = 2usize;
    for (i, part) in parts.iter().enumerate() {
        work.visit(Visit::Element)?;
        n = w::usize_add(work, n, part.bytes().len())?;
        if !w::compare(work, i, 0usize)?.is_eq() {
            n = w::usize_add(work, n, 1)?;
        }
    }
    Encode { storage, arena }.join_list(parts, n, work)
}
fn encoded_set(
    rows: &[Value],
    storage: &mut Storage,
    arena: &AuditArena,
    work: &mut dyn ValidationWork,
) -> Result<AuditedBytes> {
    let mut parts = storage.vector::<AuditedBytes>(rows.len(), arena, work)?;
    for row in rows {
        work.visit(Visit::Element)?;
        let bytes = arena.canonical_representation_bytes(row, work, 262144)?;
        work.write_bytes(mem::size_of::<AuditedBytes>())?;
        parts.push(bytes);
    }
    order(&mut parts, work)?;
    join(&parts, storage, arena, work)
}
fn replace(
    tree: &mut AuditedValue,
    key: &str,
    bytes: AuditedBytes,
    arena: &AuditArena,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    // SAME audited parser and same actual owner; this internal normalization
    // parse is S work, never an arbitrary caller-entered representation scope.
    let value = arena.parse_representation_bounded(bytes.bytes(), 262144, work)?;
    tree.replace_existing_owned(key, value, work)
}
fn counter(
    row: &mut AuditedValue,
    key: &str,
    storage: &mut Storage,
    arena: &AuditArena,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    let raw = o::text(o::field(row.value(), key, work)?, work)?;
    let value = super::super::facts::counter_with_work(raw, work)?;
    let word = w::unsigned_string(value.into(), work)?;
    let bytes = Encode { storage, arena }.text(&word, work)?;
    drop(word);
    replace(row, key, bytes, arena, work)
}
pub(super) fn normalize(raw: &[u8], session: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let result = (|| {
        if w::compare(session.semantic_work(), raw.len(), 262144usize)?.is_gt() {
            return Err(fail("private slot terms byte limit"));
        }
        let mut tree = session.parse_representation(raw, Kind::SourceDefinition)?;
        let arena = session.arena().shared_handle();
        let work: &mut dyn ValidationWork = session.meter_mut();
        super::super::facts::reject_null_with_work(tree.value(), work)?;
        o::closed(
            tree.value(),
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
                "causal_reversal_clauses",
                "exposure_scope",
                "app_cost_terms",
            ],
            &[],
            work,
        )?;
        if !paid::compare_text(
            o::text(o::field(tree.value(), "schema", work)?, work)?,
            "ledger-workflow-slot-binding-terms/7",
            work,
        )?
        .is_eq()
        {
            return Err(fail("owning app terms7 schema"));
        }
        let mut storage = Storage::new();
        let scopes = o::array(o::field(tree.value(), "scope", work)?, 2, 2, work)?;
        for v in scopes {
            paid::text(o::text(v, work)?, 128, work)?;
        }
        for key in ["agreement_id", "customer"] {
            paid::text(
                o::text(o::field(tree.value(), key, work)?, work)?,
                128,
                work,
            )?;
        }
        for key in ["slot", "role_slot", "funding_leg", "correction_source_slot"] {
            super::super::facts::name_with_work(
                o::text(o::field(tree.value(), key, work)?, work)?,
                work,
            )?;
        }
        for (key, literal) in [("book", "retail"), ("exposure_scope", "original_work")] {
            if !paid::compare_text(
                o::text(o::field(tree.value(), key, work)?, work)?,
                literal,
                work,
            )?
            .is_eq()
            {
                return Err(fail("original retail exposure terms"));
            }
        }
        o::reference_scratch(&mut storage, &arena, work)?;
        paid::reference(
            o::field(tree.value(), "role_ref", work)?,
            "workflow-roles",
            work,
        )?;
        let mut quantities = o::quantity_rows(
            o::field(tree.value(), "quantity_ceilings", work)?,
            &mut storage,
            &arena,
            work,
        )?;
        o::scalar_scratch(quantities.len(), &mut storage, &arena, work)?;
        paid::quantities(&mut quantities, work)?;
        let bytes = Encode {
            storage: &mut storage,
            arena: &arena,
        }
        .quantities(&quantities, work)?;
        replace(&mut tree, "quantity_ceilings", bytes, &arena, work)?;
        let exposure = o::amount(
            o::field(tree.value(), "exposure_ceiling", work)?,
            &mut storage,
            &arena,
            work,
        )?;
        paid::amount(&exposure, work)?;
        drop(exposure);
        let activities = o::array(
            o::field(tree.value(), "allowed_activities", work)?,
            1,
            16,
            work,
        )?;
        for v in activities {
            super::super::facts::name_with_work(o::text(v, work)?, work)?;
        }
        for quantity in &quantities {
            work.visit(Visit::Element)?;
            let mut found = false;
            for activity in activities {
                if paid::compare_text(&quantity.activity, o::text(activity, work)?, work)?.is_eq() {
                    found = true;
                    break;
                }
            }
            paid::check(work, found, "quantity ceiling outside allowed activity")?;
        }
        drop(quantities);
        let bytes = encoded_set(activities, &mut storage, &arena, work)?;
        replace(&mut tree, "allowed_activities", bytes, &arena, work)?;
        let grants = o::array(
            o::field(tree.value(), "source_grant_refs", work)?,
            1,
            32,
            work,
        )?;
        for v in grants {
            paid::reference(v, "workflow-source-grant", work)?;
        }
        let bytes = encoded_set(grants, &mut storage, &arena, work)?;
        replace(&mut tree, "source_grant_refs", bytes, &arena, work)?;
        let clauses = o::array(
            o::field(tree.value(), "causal_reversal_clauses", work)?,
            0,
            64,
            work,
        )?;
        let bytes = encoded_set(clauses, &mut storage, &arena, work)?;
        replace(&mut tree, "causal_reversal_clauses", bytes, &arena, work)?;
        let rows = o::array(o::field(tree.value(), "app_cost_terms", work)?, 1, 64, work)?;
        let mut parts = storage.vector::<AuditedBytes>(rows.len(), &arena, work)?;
        for row in rows {
            work.visit(Visit::Element)?;
            let mut row = arena.clone_value(row, work)?;
            counter(&mut row, "finalize_within_us", &mut storage, &arena, work)?;
            counter(&mut row, "correction_within_us", &mut storage, &arena, work)?;
            let amount = o::amount(
                o::field(row.value(), "maximum_final_amount", work)?,
                &mut storage,
                &arena,
                work,
            )?;
            paid::amount(&amount, work)?;
            drop(amount);
            let bytes = arena.canonical_representation_bytes(row.value(), work, 262144)?;
            drop(row);
            work.write_bytes(mem::size_of::<AuditedBytes>())?;
            parts.push(bytes);
        }
        order(&mut parts, work)?;
        let bytes = join(&parts, &mut storage, &arena, work)?;
        replace(&mut tree, "app_cost_terms", bytes, &arena, work)?;
        drop(parts);
        arena.canonical_representation_bytes(tree.value(), work, 262144)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
