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
#[derive(Clone, Copy)]
enum Recipe {
    App,
    StageRetail,
    StageSupplier { projection: bool },
}
pub(super) fn normalize(raw: &[u8], session: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    normalize_recipe(raw, Recipe::App, session)
}
pub(super) fn normalize_stage(
    program: &super::super::selected_program::SelectedProgram,
    raw: &[u8],
    projection: bool,
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_stage_lifecycle_recipe() {
            return Err(fail("Stage private terms recipe"));
        }
        if w::compare(session.semantic_work(), raw.len(), 262144usize)?.is_gt() {
            return Err(fail("private slot terms byte limit"));
        }
        let parsed = session.parse_representation(raw, Kind::SourceDefinition)?;
        let book = o::text(
            o::field(parsed.value(), "book", session.meter_mut())?,
            session.meter_mut(),
        )?;
        let recipe = if paid::compare_text(book, "supplier", session.meter_mut())?.is_eq() {
            Recipe::StageSupplier { projection }
        } else {
            paid::check(session.meter_mut(), !projection, "supplier projection book")?;
            let retail = paid::compare_text(book, "retail", session.meter_mut())?.is_eq();
            paid::check(session.meter_mut(), retail, "Stage private retail book")?;
            Recipe::StageRetail
        };
        drop(parsed);
        normalize_recipe(raw, recipe, session)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn normalize_recipe(
    raw: &[u8],
    recipe: Recipe,
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = (|| {
        if w::compare(session.semantic_work(), raw.len(), 262144usize)?.is_gt() {
            return Err(fail("private slot terms byte limit"));
        }
        let mut tree = session.parse_representation(raw, Kind::SourceDefinition)?;
        let arena = session.arena().shared_handle();
        let work: &mut dyn ValidationWork = session.meter_mut();
        super::super::facts::reject_null_with_work(tree.value(), work)?;
        let retail = [
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
        ];
        match recipe {
            Recipe::App => o::closed(
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
            )?,
            Recipe::StageRetail => o::closed(tree.value(), &retail, &[], work)?,
            Recipe::StageSupplier { projection } => {
                let keys = [
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
                ];
                let preview = [
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
                    "cost_authority_ref",
                    "provider_account_identity",
                    "consumption_basis",
                ];
                o::closed(
                    tree.value(),
                    if projection { &preview } else { &keys },
                    &[],
                    work,
                )?;
            }
        }
        let schema = match recipe {
            Recipe::App => Some("ledger-workflow-slot-binding-terms/7"),
            Recipe::StageRetail => Some("ledger-workflow-slot-binding-terms/3"),
            Recipe::StageSupplier { projection: true } => None,
            Recipe::StageSupplier { projection: false } => {
                Some("ledger-workflow-slot-binding-terms/8")
            }
        };
        if let Some(expected) = schema {
            if !paid::compare_text(
                o::text(o::field(tree.value(), "schema", work)?, work)?,
                expected,
                work,
            )?
            .is_eq()
            {
                return Err(fail("owning selected slot terms schema"));
            }
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
        let supplier = matches!(recipe, Recipe::StageSupplier { .. });
        let expected_book = if supplier { "supplier" } else { "retail" };
        if !paid::compare_text(
            o::text(o::field(tree.value(), "book", work)?, work)?,
            expected_book,
            work,
        )?
        .is_eq()
        {
            return Err(fail("selected terms book"));
        }
        if !supplier
            && !paid::compare_text(
                o::text(o::field(tree.value(), "exposure_scope", work)?, work)?,
                "original_work",
                work,
            )?
            .is_eq()
        {
            return Err(fail("original retail exposure terms"));
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
        if supplier {
            for field in if matches!(recipe, Recipe::StageSupplier { projection: true }) {
                &["cost_authority_ref"][..]
            } else {
                &["offer_ref", "commitment_ref", "cost_authority_ref"][..]
            } {
                paid::reference(
                    o::field(tree.value(), field, work)?,
                    if *field == "cost_authority_ref" {
                        "workflow-authority"
                    } else {
                        "workflow-offer"
                    },
                    work,
                )?;
            }
            let account = o::field(tree.value(), "provider_account_identity", work)?;
            o::closed(
                account,
                &["provider_namespace", "provider_account_id", "environment"],
                &[],
                work,
            )?;
            for (field, max) in [
                ("provider_namespace", 256),
                ("provider_account_id", 128),
                ("environment", 128),
            ] {
                paid::text(o::text(o::field(account, field, work)?, work)?, max, work)?;
            }
            if !paid::compare_text(
                o::text(o::field(tree.value(), "consumption_basis", work)?, work)?,
                "owned_obligation_high_water",
                work,
            )?
            .is_eq()
            {
                return Err(fail("supplier original high-water terms"));
            }
        } else {
            let clauses = o::array(
                o::field(tree.value(), "causal_reversal_clauses", work)?,
                0,
                64,
                work,
            )?;
            for (index, clause) in clauses.iter().enumerate() {
                work.visit(Visit::Element)?;
                let root = o::text(o::field(clause, "root_component_slot", work)?, work)?;
                let family = o::text(o::field(clause, "family_slot", work)?, work)?;
                for prior in &clauses[..index] {
                    work.visit(Visit::MemberCompare)?;
                    if paid::compare_text(
                        root,
                        o::text(o::field(prior, "root_component_slot", work)?, work)?,
                        work,
                    )?
                    .is_eq()
                        && paid::compare_text(
                            family,
                            o::text(o::field(prior, "family_slot", work)?, work)?,
                            work,
                        )?
                        .is_eq()
                    {
                        return Err(fail("duplicate causal root/family clause"));
                    }
                }
            }
            let bytes = encoded_set(clauses, &mut storage, &arena, work)?;
            replace(&mut tree, "causal_reversal_clauses", bytes, &arena, work)?;
        }
        if matches!(recipe, Recipe::App) {
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
        }
        arena.canonical_representation_bytes(tree.value(), work, 262144)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn stage_terms_exact_paid_retail_supplier_and_acyclic_preview() {
        let r = |kind: &str| json!({"kind":kind,"id":format!("wdoc_{}","1".repeat(64)),"digest":format!("sha256:{}","1".repeat(64))});
        let retail = json!({"schema":"ledger-workflow-slot-binding-terms/3","scope":["test","local"],"agreement_id":"agreement","customer":"customer","slot":"support:retail","book":"retail","role_slot":"support:retail","role_ref":r("workflow-roles"),"funding_leg":"support:funding","allowed_activities":["support:work"],"quantity_ceilings":[{"activity":"support:work","field":"support:quantity","unit":"support:item","maximum":"1.00"}],"exposure_ceiling":{"currency":"USD","scale":18,"atoms":"100"},"source_grant_refs":[r("workflow-source-grant")],"correction_source_slot":"support:corrector","causal_reversal_clauses":[],"exposure_scope":"original_work"});
        let mut supplier = retail.clone();
        let o = supplier.as_object_mut().unwrap();
        o.remove("causal_reversal_clauses");
        o.remove("exposure_scope");
        o.insert(
            "schema".into(),
            json!("ledger-workflow-slot-binding-terms/8"),
        );
        o.insert("book".into(), json!("supplier"));
        o.insert("offer_ref".into(), r("workflow-offer"));
        o.insert("commitment_ref".into(), r("workflow-offer"));
        o.insert("cost_authority_ref".into(), r("workflow-authority"));
        o.insert("provider_account_identity".into(),json!({"provider_namespace":"provider","provider_account_id":"account","environment":"test"}));
        o.insert(
            "consumption_basis".into(),
            json!("owned_obligation_high_water"),
        );
        for (body, recipe) in [
            (&retail, Recipe::StageRetail),
            (&supplier, Recipe::StageSupplier { projection: false }),
        ] {
            let arena = AuditArena::new().unwrap();
            let baseline = arena.usage().unwrap().live_bytes;
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let raw = serde_json::to_vec(body).unwrap();
            let normalized = normalize_recipe(&raw, recipe, &mut s).unwrap();
            let actual: Value = serde_json::from_slice(normalized.bytes()).unwrap();
            assert_eq!(actual["quantity_ceilings"][0]["maximum"], "1");
            drop(normalized);
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
            assert!(normalize(&raw, &mut s).is_err());
        }
        let mut preview = supplier.clone();
        let p = preview.as_object_mut().unwrap();
        for key in ["schema", "offer_ref", "commitment_ref"] {
            p.remove(key);
        }
        let arena = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let raw = serde_json::to_vec(&preview).unwrap();
        let normalized =
            normalize_recipe(&raw, Recipe::StageSupplier { projection: true }, &mut s).unwrap();
        let actual: Value = serde_json::from_slice(normalized.bytes()).unwrap();
        assert!(actual.get("commitment_ref").is_none());
        assert!(actual.get("schema").is_none());
        drop(normalized);
        preview["offer_ref"] = r("workflow-offer");
        assert!(normalize_recipe(
            &serde_json::to_vec(&preview).unwrap(),
            Recipe::StageSupplier { projection: true },
            &mut s
        )
        .is_err());
        let arena = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        supplier["source_grant_refs"]
            .as_array_mut()
            .unwrap()
            .push(r("workflow-source-grant"));
        assert!(normalize_recipe(
            &serde_json::to_vec(&supplier).unwrap(),
            Recipe::StageSupplier { projection: false },
            &mut s
        )
        .is_err());
    }
}
