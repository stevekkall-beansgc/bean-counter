//! Existing supplier /1 saved-intent normalization only. The unconnected
//! monetary producer is preserved outside compilation in docs/archive.
use super::super::{body, shape, working_value::WorkingValue, CoreMeter};
use crate::{validation_work as w, Result};
pub(in crate::policy::workflow::transition_constructor) fn normalize_request(
    m: &mut CoreMeter,
    p: &mut WorkingValue,
) -> Result<bool> {
    let schema = shape::text(m, p, "schema")?;
    let correct = shape::same_text(m, schema, "ledger-workflow-cost-correct/1")?;
    if correct {
        shape::closed(
            m,
            p,
            &[
                "schema",
                "original_cost_claim_ref",
                "expected_revision",
                "replacement_final_cost_proof",
                "dependent_reversals",
                "evidence_refs",
            ],
            &[],
        )?;
        let raw = shape::text(m, p, "expected_revision")?;
        let revision = super::super::super::facts::counter_with_work(raw, m)?;
        if !w::compare(m, revision, 0u64)?.is_gt() {
            return Err(shape::fail("positive cost expected revision"));
        }
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning supplier normalization"))?;
        let value = super::super::close::counter_value(m, &arena, revision)?;
        p.replace(m, "expected_revision", value.value())?;
        shape::reference(m, &p["original_cost_claim_ref"])?;
        let dependents = shape::array(m, p, "dependent_reversals")?;
        if w::compare(m, dependents.len(), 31usize)?.is_gt() {
            return Err(shape::fail("cost dependent count"));
        }
        // Each complete closure object is a named semantic set. Normalize the
        // nested Ref set before the containing object set and semantic matching.
        for index in 0..dependents.len() {
            let item = &p["dependent_reversals"][index];
            shape::closed(
                m,
                item,
                &[
                    "decision_ref",
                    "expected_revision",
                    "authority_evidence_refs",
                ],
                &[],
            )?;
            shape::reference(m, &item["decision_ref"])?;
            let raw = shape::text(m, item, "expected_revision")?;
            let value = super::super::super::facts::counter_with_work(raw, m)?;
            if !w::compare(m, value, 0u64)?.is_gt() {
                return Err(shape::fail("positive cost dependent revision"));
            }
            let values = shape::array(m, item, "authority_evidence_refs")?;
            if w::compare(m, values.len(), 16usize)?.is_gt() {
                return Err(shape::fail("cost dependent authority evidence bound"));
            }
            let mut refs = Vec::new();
            for reference in values {
                m.visit("cost.normalize_dependent_evidence")?;
                refs.push(shape::reference(m, reference)?);
            }
            let refs = body::ref_set(m, refs)?;
            p.replace_reference_array_field(
                m,
                "dependent_reversals",
                index,
                "authority_evidence_refs",
                &refs,
            )?;
            let value = super::super::close::counter_value(m, &arena, value)?;
            match p {
                WorkingValue::Owned(p) => p.replace_existing_array_field_owned(
                    "dependent_reversals",
                    index,
                    "expected_revision",
                    value,
                    m,
                )?,
                WorkingValue::Legacy(_) => {
                    return Err(shape::fail("cost normalization requires owning profile"));
                }
            }
        }
        p.normalize_set(m, "dependent_reversals", false)?;
    } else {
        if !shape::same_text(m, schema, "ledger-workflow-cost-finalize/1")? {
            return Err(shape::fail("cost request tuple"));
        }
        shape::closed(
            m,
            p,
            &[
                "schema",
                "original_work_ref",
                "original_invocation_ref",
                "supplier_component_slot",
                "final_cost_proof",
                "evidence_refs",
            ],
            &[],
        )?;
        shape::reference(m, &p["original_work_ref"])?;
        shape::reference(m, &p["original_invocation_ref"])?;
        super::super::inventory::binding::id(m, &p["supplier_component_slot"], 128)?;
    }
    let proof = if correct {
        &p["replacement_final_cost_proof"]
    } else {
        &p["final_cost_proof"]
    };
    shape::closed(m, proof, &["attestation_ref", "verification_ref"], &[])?;
    shape::reference(m, &proof["attestation_ref"])?;
    shape::reference(m, &proof["verification_ref"])?;
    let evidence = shape::array(m, p, "evidence_refs")?;
    if w::compare(m, evidence.len(), 16usize)?.is_gt() {
        return Err(shape::fail("cost evidence bound"));
    }
    for reference in evidence {
        shape::reference(m, reference)?;
    }
    p.normalize_set(m, "evidence_refs", false)?;
    Ok(correct)
}
