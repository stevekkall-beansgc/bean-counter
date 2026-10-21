//! Exact retained supplier operands used by the same finalize/correct executor.
//! These are checked DATA associations. Host completed origins, owner custody,
//! current source authorization and complete capacity remain separate gates.
use super::super::{
    arithmetic, body, data::*, meter, record_body::RecordBody, records::Records, shape, CoreMeter,
};
use super::{conserved, nonnegative, InvocationAmounts};
use crate::{
    canonical,
    canonical::audit_arena::ExternalBufferLease,
    domain::Timestamp,
    validation_work::{self as w, ValidationWork},
    Error, Result,
};
use num_bigint::BigInt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::policy::workflow::transition_constructor) struct InvocationSelection {
    pub original_ref: ExactRef,
    pub original: RecordBody,
    pub current_ref: ExactRef,
    pub current: RecordBody,
    pub work_ref: ExactRef,
    pub work: RecordBody,
    pub binding_ref: ExactRef,
    pub binding: RecordBody,
    pub terms_ref: ExactRef,
    pub terms: RecordBody,
    pub offer: RecordBody,
    pub owner: RecordBody,
    pub amounts: InvocationAmounts,
    // References, counters and scalar scratch are retained under this owner.
    _ownership: ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) struct FinalEvidence {
    pub assertion_ref: ExactRef,
    pub assertion: RecordBody,
    pub verification_ref: ExactRef,
    pub verification: RecordBody,
    pub eligible: BigInt,
    pub quantity: arithmetic::Ratio,
    _ownership: ExternalBufferLease,
}

#[cfg(test)]
mod lineage_tests {
    use super::super::{records, InvocationAmounts};
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    fn reference(kind: &str, id: &str) -> ExactRef {
        ExactRef {
            kind: kind.into(),
            id: id.into(),
            digest: format!("sha256:{}", "a".repeat(64)),
        }
    }
    fn owned(m: &mut CoreMeter, arena: &AuditArena, value: &Value) -> RecordBody {
        let value = arena.clone_value(value, m).unwrap();
        RecordBody::owned(arena, value, m).unwrap()
    }
    fn value(n: i64) -> Value {
        json!({"currency":"USD","scale":18,"atoms":n.to_string()})
    }
    #[test]
    fn immutable_lineage_keeps_initial_obligation_and_high_water_through_down_up_and_zero() {
        // This exercises actual immutable record construction, not custody,
        // retained-source verification, a complete candidate or financial run.
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let c = DraftContextData {
            key: OperationKey {
                logical_store_id: "store".into(),
                scope: ["scope".into(), "project".into()],
                source: "urn:source".into(),
                operation_id: "op".into(),
            },
            actor: "actor".into(),
            canonical_economic_namespace: "urn:source".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-16T20:00:00.000000Z".into(),
            accepted_at: "2026-10-16T20:00:00.000000Z".into(),
            authorities: vec![],
            source_observation_inputs: vec![],
        };
        let binding_ref = reference("workflow-binding", "binding");
        let work_ref = reference("workflow-work", "work");
        let invocation_ref = reference("workflow-invocation", "invocation");
        let owner_ref = reference("workflow-cost-authority", "authority");
        let terms_ref = reference("workflow-slot-terms", "terms");
        let line = json!({"cost_authority_id":"authority","provider_account_identity":{"provider_namespace":"urn:provider","provider_account_id":"account","environment":"test"},"original_document_id":"document","original_line_id":"line"});
        let mut previous: Option<records::CostLineage> = None;
        let mut original_claim: Option<super::super::super::emitted_record::ConstructedEnvelope> =
            None;
        let mut initial_decision: Option<ExactRef> = None;
        for (index, eligible) in [200i64, 160, 250, 300, 0].into_iter().enumerate() {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut m = session.meter_mut().original_economic_meter().unwrap();
            let current = match &previous {
                None => owned(
                    &mut m,
                    &arena,
                    &json!({"schema":"ledger-workflow-invocation/1","revision":"1","cost_authority_ref":owner_ref,"supplier_terms_ref":terms_ref,"authorized":value(300),"consumed":value(0),"held":value(300),"released":value(0),"state":"authorized","cost_consumption_refs":[],"operation_key":c.key}),
                ),
                Some(old) => old.invocation.decoded(&mut m, 4194304).unwrap(),
            };
            let current_ref = previous
                .as_ref()
                .map(|p| p.invocation.reference.clone())
                .unwrap_or_else(|| invocation_ref.clone());
            let amounts = InvocationAmounts {
                authorized: amount(&mut m, &current["authorized"]).unwrap(),
                consumed: amount(&mut m, &current["consumed"]).unwrap(),
                held: amount(&mut m, &current["held"]).unwrap(),
                released: amount(&mut m, &current["released"]).unwrap(),
            };
            let selected = InvocationSelection {
                original_ref: invocation_ref.clone(),
                original: owned(
                    &mut m,
                    &arena,
                    &json!({"cost_authority_ref":owner_ref,"authorized":value(300)}),
                ),
                current_ref,
                current,
                work_ref: work_ref.clone(),
                work: owned(&mut m, &arena, &json!({})),
                binding_ref: binding_ref.clone(),
                binding: owned(&mut m, &arena, &json!({})),
                terms_ref: terms_ref.clone(),
                terms: owned(&mut m, &arena, &json!({})),
                offer: owned(&mut m, &arena, &json!({})),
                owner: owned(&mut m, &arena, &json!({})),
                amounts,
                _ownership: arena
                    .reserve_external_payload(
                        std::mem::size_of::<InvocationSelection>() + 8192,
                        &mut m,
                    )
                    .unwrap(),
            };
            let assertion_ref = reference("workflow-fact-attestation", &format!("evidence{index}"));
            let mut assertion = json!({"subject":{"line_key":line},"amount":value(eligible)});
            if let Some(old) = &previous {
                let v = old.revision.decoded(&mut m, 4194304).unwrap();
                assertion["prior_final_evidence_ref"] = v["current_evidence_ref"].clone();
            }
            let proof = FinalEvidence {
                assertion_ref,
                assertion: owned(&mut m, &arena, &assertion),
                verification_ref: reference("workflow-evidence-verification", "verification"),
                verification: owned(&mut m, &arena, &json!({})),
                eligible: eligible.into(),
                quantity: arithmetic::Ratio {
                    numerator: 1.into(),
                    denominator: 1.into(),
                },
                _ownership: arena
                    .reserve_external_payload(std::mem::size_of::<FinalEvidence>() + 4096, &mut m)
                    .unwrap(),
            };
            if original_claim.is_none() {
                original_claim = Some(
                    records::claim(&mut m, &c, &selected, &proof, &json!("supplier_component"))
                        .unwrap(),
                );
            }
            let claim = original_claim.as_ref().unwrap();
            let claim_body = claim.decoded(&mut m, 4194304).unwrap();
            let decision=body::record(&mut m,&c.key,"workflow-decision",format!("decision{index}"),json!({"schema":"ledger-workflow-decision-revision/2","kind":"cost","subject_ref":claim.reference,"original_terms_ref":binding_ref,"decision_id":"cost_decision","revision":(index+1).to_string()})).unwrap();
            let actions = if eligible == 0 {
                vec![]
            } else {
                vec![body::record(&mut m,&c.key,"workflow-action",format!("action{index}"),json!({"decision_id":"cost_decision","revision":(index+1).to_string(),"posting_role":if index==0{"original"}else{"replacement"},"book":"supplier","amount":value(eligible),"schema":"ledger-workflow-action/1"})).unwrap()]
            };
            let old_revision = previous
                .as_ref()
                .map(|p| p.revision.decoded(&mut m, 4194304).unwrap());
            let old_owner = previous
                .as_ref()
                .map(|p| p.owner.decoded(&mut m, 4194304).unwrap());
            let old_consumption = previous
                .as_ref()
                .map(|p| p.consumption.decoded(&mut m, 4194304).unwrap());
            let prior = previous.as_ref().map(|p| records::PreviousLineage {
                revision_ref: &p.revision.reference,
                revision: old_revision.as_ref().unwrap(),
                owner_ref: &p.owner.reference,
                owner: old_owner.as_ref().unwrap(),
                consumption_ref: &p.consumption.reference,
                consumption: old_consumption.as_ref().unwrap(),
            });
            let next = records::advance(
                &mut m,
                &c,
                &selected,
                &proof,
                &claim.reference,
                &claim_body,
                &decision,
                &actions,
                prior,
            )
            .unwrap();
            if initial_decision.is_none() {
                initial_decision = Some(decision.reference.clone());
            }
            let owner = next.owner.decoded(&mut m, 4194304).unwrap();
            let consumption = next.consumption.decoded(&mut m, 4194304).unwrap();
            let invocation = next.invocation.decoded(&mut m, 4194304).unwrap();
            let allocation = next.allocation.decoded(&mut m, 4194304).unwrap();
            assert_eq!(
                owner["supplier_obligation_ref"],
                json!(initial_decision.as_ref().unwrap())
            );
            assert_eq!(
                owner["original_evidence_ref"],
                claim_body["original_evidence_ref"]
            );
            let expected_high = [200, 200, 250, 300, 300][index];
            assert_eq!(consumption["high_water"], value(expected_high));
            assert_eq!(consumption["eligible_amount"], value(eligible));
            assert_eq!(invocation["consumed"], value(expected_high));
            assert_eq!(invocation["held"], value(300 - expected_high));
            assert_eq!(
                invocation["cost_consumption_refs"],
                json!([next.consumption.reference])
            );
            assert_eq!(allocation["eligible_supplier_amount"], value(eligible));
            assert_eq!(
                allocation.get("supplier_action_ref").is_some(),
                eligible != 0
            );
            session.meter_mut().finish_original_economic(m).unwrap();
            previous = Some(next);
        }
        drop(previous);
        drop(original_claim);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn copy(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    fn s(m: &mut CoreMeter, s: &str) -> Result<String> {
        m.scan_bytes(s.len())?;
        m.write_bytes(s.len())?;
        Ok(s.to_owned())
    }
    Ok(ExactRef {
        kind: s(m, &r.kind)?,
        id: s(m, &r.id)?,
        digest: s(m, &r.digest)?,
    })
}
pub(super) fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, RecordBody)> {
    let reference = shape::reference(m, v)?;
    let value = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, value))
}
fn revision(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    let raw = shape::text(m, v, key)?;
    let value = super::super::super::facts::counter_with_work(raw, m)?;
    if !meter::cmp(m, &value.into(), &0.into())?.is_gt() {
        return Err(shape::fail("positive supplier revision"));
    }
    Ok(value.into())
}
fn instant(m: &mut CoreMeter, v: &Value, key: &str) -> Result<BigInt> {
    Ok(Timestamp::parse_with_work(shape::text(m, v, key)?, m)?
        .micros()
        .into())
}
pub(super) fn amount(m: &mut CoreMeter, v: &Value) -> Result<BigInt> {
    shape::closed(m, v, &["currency", "scale", "atoms"], &[])?;
    eq(m, &v["currency"], &json!("USD"))?;
    eq(m, &v["scale"], &json!(18))?;
    let raw = shape::text(m, v, "atoms")?;
    let amount = arithmetic::integer(m, raw, false)?;
    nonnegative(m, &amount)?;
    Ok(amount)
}
fn at_most(m: &mut CoreMeter, value: &BigInt, maximum: &Value) -> Result<()> {
    let maximum = amount(m, maximum)?;
    if meter::cmp(m, value, &maximum)?.is_gt() {
        return Err(Error::new("EXPOSURE_EXCEEDED", "original supplier ceiling"));
    }
    Ok(())
}
fn decimal_at_most(m: &mut CoreMeter, value: &arithmetic::Ratio, maximum: &str) -> Result<()> {
    let maximum = arithmetic::decimal(m, maximum)?;
    let lhs = meter::mul(m, &value.numerator, &maximum.denominator)?;
    let rhs = meter::mul(m, &maximum.numerator, &value.denominator)?;
    if meter::cmp(m, &value.numerator, &0.into())?.is_lt() || meter::cmp(m, &lhs, &rhs)?.is_gt() {
        return Err(shape::fail("original supplier quantity ceiling"));
    }
    Ok(())
}
fn invocation_frame(m: &mut CoreMeter, v: &Value) -> Result<()> {
    shape::closed(
        m,
        v,
        &[
            "schema",
            "invocation_id",
            "revision",
            "original_binding_ref",
            "agreement_id",
            "customer",
            "business_operation_id",
            "chain_id",
            "activity",
            "source_namespace",
            "source_slot",
            "source",
            "supplier_slot",
            "supplier_terms_ref",
            "offer_ref",
            "cost_authority_ref",
            "provider_account_identity",
            "role_ref",
            "funding_leg",
            "unit",
            "maximum_quantity",
            "authorized",
            "consumed",
            "held",
            "released",
            "state",
            "original_authority_ref",
            "authorized_at",
            "start_before",
            "operation_key",
            "cost_consumption_refs",
            "other_consumed",
        ],
        &["prior_ref"],
    )?;
    revision(m, v, "revision")?;
    Ok(())
}
/// Select one original pre-work invocation and its exact current locked head.
/// Current consumed is recomputed from every named permanent consumption head;
/// a changed provider line or another invocation cannot provide reusable credit.
pub(in crate::policy::workflow::transition_constructor) fn invocation(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    work_ref: &ExactRef,
    original_ref: &ExactRef,
    state: &EconomicStateData<'_>,
) -> Result<InvocationSelection> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier profile"))?;
    // Six Ref copies (at most 263 bytes each), scalar/format scratch and fixed
    // typed headers. Record bodies retain their own shared owning artifacts.
    let bytes = w::usize_add(m, std::mem::size_of::<InvocationSelection>(), 8192)?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    let work = r.get(
        m,
        &c.key,
        work_ref,
        "workflow-work",
        "ledger-workflow-work-record/1",
    )?;
    let p = shape::field(m, &work, "payload")?;
    eq(m, &p["status"], &json!("succeeded"))?;
    let invocations = shape::array(m, p, "invocation_refs")?;
    if !shape::member(m, invocations, &json!(original_ref))? {
        return Err(shape::fail("supplier invocation absent from original work"));
    }
    let original = r.get(
        m,
        &c.key,
        original_ref,
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    invocation_frame(m, &original)?;
    eq(m, &original["revision"], &json!("1"))?;
    if original.get("prior_ref").is_some() {
        return Err(shape::fail("original supplier invocation predecessor"));
    }
    eq(m, &original["state"], &json!("authorized"))?;
    let (binding_ref, binding) = load(
        m,
        r,
        c,
        &original["original_binding_ref"],
        "workflow-binding",
        "ledger-workflow-binding/1",
    )?;
    eq(m, &p["binding_ref"], &json!(binding_ref))?;
    eq(m, &original["agreement_id"], &binding["agreement_id"])?;
    for (a, b) in [
        ("customer", "customer"),
        ("business_operation_id", "business_operation_id"),
        ("chain_id", "chain"),
        ("activity", "activity"),
    ] {
        eq(m, &original[a], &p[b])?;
    }
    eq(m, &original["source_namespace"], &work["source_namespace"])?;
    eq(m, &original["source"], &work["key"]["source"])?;
    let id = body::semantic_id(
        m,
        "winv_",
        "workflow-invocation",
        &json!([
            c.key.scope,
            original["agreement_id"],
            original["business_operation_id"]
        ]),
    )?;
    if !shape::same_text(m, &id, &original_ref.id)? {
        return Err(shape::fail("original supplier invocation identity"));
    }
    let work_at = instant(m, &work, "accepted_at")?;
    let authorized = instant(m, &original, "authorized_at")?;
    let start_before = instant(m, &original, "start_before")?;
    if meter::cmp(m, &authorized, &work_at)?.is_gt()
        || !meter::cmp(m, &work_at, &start_before)?.is_lt()
    {
        return Err(shape::fail(
            "original work missed invocation start interval",
        ));
    }
    let supplier = state
        .supplier
        .ok_or_else(|| shape::fail("missing complete supplier head DATA"))?;
    let mut head = None;
    for candidate in supplier.invocation_heads {
        m.visit("cost.invocation_head")?;
        if shape::same_text(m, &candidate.invocation_id, &id)? {
            if head.is_some() {
                return Err(shape::fail("duplicate current invocation head"));
            }
            head = Some(candidate);
        }
    }
    let head = head.ok_or_else(|| shape::fail("missing current invocation head"))?;
    let current = r.get(
        m,
        &c.key,
        &head.reference,
        "workflow-invocation",
        "ledger-workflow-invocation/1",
    )?;
    invocation_frame(m, &current)?;
    eq(m, &current["revision"], &json!(head.revision))?;
    if !shape::same_text(m, &head.reference.id, &id)? {
        return Err(shape::fail("current invocation stable identity"));
    }
    // Only amounts/state/current consumption and revision lineage may change.
    for key in [
        "invocation_id",
        "original_binding_ref",
        "agreement_id",
        "customer",
        "business_operation_id",
        "chain_id",
        "activity",
        "source_namespace",
        "source_slot",
        "source",
        "supplier_slot",
        "supplier_terms_ref",
        "offer_ref",
        "cost_authority_ref",
        "provider_account_identity",
        "role_ref",
        "funding_leg",
        "unit",
        "maximum_quantity",
        "authorized",
        "original_authority_ref",
        "authorized_at",
        "start_before",
    ] {
        eq(m, &current[key], &original[key])?;
    }
    let status = shape::text(m, &current, "state")?;
    if !shape::same_text(m, status, "authorized")? && !shape::same_text(m, status, "completed")? {
        return Err(Error::new(
            "INVOCATION_CLOSED",
            "supplier correction/finalization coverage unavailable",
        ));
    }
    let amounts = InvocationAmounts {
        authorized: amount(m, &current["authorized"])?,
        consumed: amount(m, &current["consumed"])?,
        held: amount(m, &current["held"])?,
        released: amount(m, &current["released"])?,
    };
    conserved(m, &amounts)?;
    let mut consumed = amount(m, &current["other_consumed"])?;
    let rows = shape::array(m, &current, "cost_consumption_refs")?;
    if w::compare(m, rows.len(), 128usize)?.is_gt() {
        return Err(shape::fail("invocation consumption set bound"));
    }
    super::super::inventory::binding::ordered(m, rows)?;
    for input in rows {
        m.visit("cost.complete_invocation_consumption")?;
        let (reference, row) = load(
            m,
            r,
            c,
            input,
            "workflow-cost",
            "ledger-cost-obligation-consumption/1",
        )?;
        shape::closed(
            m,
            &row,
            &[
                "schema",
                "cost_claim_ref",
                "original_invocation_ref",
                "revision",
                "eligible_amount",
                "high_water",
                "consumption_delta",
                "operation_key",
            ],
            &["prior_ref"],
        )?;
        eq(m, &row["original_invocation_ref"], &json!(original_ref))?;
        let claim_ref = shape::reference(m, &row["cost_claim_ref"])?;
        let mut selected = None;
        for candidate in supplier.cost_consumption_heads {
            m.visit("cost.consumption_head")?;
            if shape::same_ref(m, &candidate.cost_claim_ref, &claim_ref)? {
                if selected.is_some() {
                    return Err(shape::fail("duplicate cost consumption head"));
                }
                selected = Some(candidate);
            }
        }
        let selected = selected.ok_or_else(|| shape::fail("missing current consumption head"))?;
        if !shape::same_ref(m, &selected.reference, &reference)? {
            return Err(shape::fail("invocation names stale consumption"));
        }
        eq(m, &row["revision"], &json!(selected.revision))?;
        let eligible = amount(m, &row["eligible_amount"])?;
        let high = amount(m, &row["high_water"])?;
        let delta = amount(m, &row["consumption_delta"])?;
        if meter::cmp(m, &high, &eligible)?.is_lt() || meter::cmp(m, &delta, &high)?.is_gt() {
            return Err(shape::fail("consumption high-water relation"));
        }
        consumed = meter::add(m, &consumed, &high)?;
    }
    if !meter::cmp(m, &consumed, &amounts.consumed)?.is_eq() {
        return Err(shape::fail("complete invocation consumption sum"));
    }
    let bindings = shape::array(m, &binding, "bindings")?;
    let slot = shape::text(m, &original, "supplier_slot")?;
    let mapping = shape::named(m, bindings, "slot", slot)?;
    eq(m, &mapping["binding_ref"], &original["supplier_terms_ref"])?;
    let (terms_ref, terms) = load(
        m,
        r,
        c,
        &original["supplier_terms_ref"],
        "workflow-slot-terms",
        "ledger-workflow-slot-binding-terms/4",
    )?;
    for key in ["agreement_id", "customer"] {
        eq(m, &terms[key], &binding[key])?;
    }
    for key in [
        "role_ref",
        "funding_leg",
        "offer_ref",
        "cost_authority_ref",
        "provider_account_identity",
    ] {
        eq(m, &terms[key], &original[key])?;
    }
    eq(m, &terms["scope"], &json!(c.key.scope))?;
    eq(m, &terms["slot"], &original["supplier_slot"])?;
    eq(m, &terms["book"], &json!("supplier"))?;
    eq(
        m,
        &terms["consumption_basis"],
        &json!("owned_obligation_high_water"),
    )?;
    let (_, offer) = load(
        m,
        r,
        c,
        &original["offer_ref"],
        "workflow-offer",
        "ledger-workflow-offer/2",
    )?;
    eq(m, &offer["agreement_id"], &binding["agreement_id"])?;
    eq(m, &offer["policy_ref"], &binding["policy_ref"])?;
    eq(m, &offer["valuation_mode"], &json!("final_evidence"))?;
    let (_, owner) = load(
        m,
        r,
        c,
        &original["cost_authority_ref"],
        "workflow-cost-authority",
        "ledger-cost-authority/1",
    )?;
    eq(
        m,
        &owner["owner_logical_store"],
        &json!(c.key.logical_store_id),
    )?;
    let accounts = shape::array(m, &owner, "provider_account_identities")?;
    if !shape::member(m, accounts, &original["provider_account_identity"])? {
        return Err(shape::fail(
            "supplier account not enrolled by original owner",
        ));
    }
    at_most(m, &amounts.authorized, &terms["exposure_ceiling"])?;
    at_most(m, &amounts.authorized, &offer["exposure_ceiling"])?;
    Ok(InvocationSelection {
        original_ref: copy(m, original_ref)?,
        original,
        current_ref: copy(m, &head.reference)?,
        current,
        work_ref: copy(m, work_ref)?,
        work,
        binding_ref,
        binding,
        terms_ref,
        terms,
        offer,
        owner,
        amounts,
        _ownership: ownership,
    })
}

/// Consume the explicit final-line assertion and its unchanged retained
/// verification. The historical verifier's source/grant is not substituted by
/// today's finalizer/corrector. Its actual completed retention origin and the
/// current consumer's independent authority must both be checked by the caller.
pub(in crate::policy::workflow::transition_constructor) fn final_evidence(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    selected: &InvocationSelection,
    proof: &Value,
    prior: Option<&RecordBody>,
) -> Result<FinalEvidence> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning supplier proof"))?;
    let bytes = w::usize_add(m, std::mem::size_of::<FinalEvidence>(), 4096)?;
    let ownership = arena.reserve_external_payload(bytes, m)?;
    shape::closed(m, proof, &["attestation_ref", "verification_ref"], &[])?;
    let (assertion_ref, assertion) = load(
        m,
        r,
        c,
        &proof["attestation_ref"],
        "workflow-fact-attestation",
        "ledger-workflow-supplier-final-cost/1",
    )?;
    shape::closed(
        m,
        &assertion,
        &[
            "schema",
            "attestation_id",
            "source",
            "evidence_kind",
            "purpose",
            "subject",
            "provider_document_id",
            "provider_line_id",
            "line_revision",
            "final",
            "quantity",
            "unit",
            "amount",
            "issued_at",
            "supporting_evidence_refs",
        ],
        &["prior_final_evidence_ref"],
    )?;
    eq(
        m,
        &assertion["evidence_kind"],
        &json!("supplier_final_cost"),
    )?;
    eq(m, &assertion["purpose"], &json!("supplier_line_final"))?;
    eq(m, &assertion["final"], &json!(true))?;
    for key in ["attestation_id", "provider_document_id", "provider_line_id"] {
        super::super::inventory::binding::id(m, &assertion[key], 128)?;
    }
    let source = shape::text(m, &assertion, "source")?;
    super::super::inventory::binding::string(m, &assertion["source"], 256)?;
    let subject = shape::field(m, &assertion, "subject")?;
    shape::closed(
        m,
        subject,
        &[
            "logical_store_id",
            "scope",
            "original_binding_ref",
            "original_work_ref",
            "original_invocation_ref",
            "cost_authority_ref",
            "line_key",
        ],
        &[],
    )?;
    for (key, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("original_binding_ref", json!(selected.binding_ref)),
        ("original_work_ref", json!(selected.work_ref)),
        ("original_invocation_ref", json!(selected.original_ref)),
        (
            "cost_authority_ref",
            selected.original["cost_authority_ref"].clone(),
        ),
    ] {
        eq(m, &subject[key], &want)?;
    }
    let line = shape::field(m, subject, "line_key")?;
    shape::closed(
        m,
        line,
        &[
            "cost_authority_id",
            "provider_account_identity",
            "original_document_id",
            "original_line_id",
        ],
        &[],
    )?;
    eq(
        m,
        &line["cost_authority_id"],
        &selected.owner["authority_id"],
    )?;
    eq(
        m,
        &line["provider_account_identity"],
        &selected.original["provider_account_identity"],
    )?;
    let account = shape::field(m, line, "provider_account_identity")?;
    shape::closed(
        m,
        account,
        &["provider_namespace", "provider_account_id", "environment"],
        &[],
    )?;
    for key in ["provider_account_id", "environment"] {
        super::super::inventory::binding::id(m, &account[key], 128)?;
    }
    super::super::inventory::binding::id(m, &account["provider_namespace"], 256)?;
    for key in ["original_document_id", "original_line_id"] {
        super::super::inventory::binding::id(m, &line[key], 128)?;
    }
    let line_revision = revision(m, &assertion, "line_revision")?;
    match prior {
        // First financial finalization need not be the provider's first
        // delivery/version. Authenticated retention verifies any original-line
        // linkage; absence of a CostRevision cannot authorize or invent it.
        None => {}
        Some(prior) => {
            // The caller selected this exact prior via the current CostRevision;
            // no old or alternative line can be nominated by the replacement.
            eq(m, &prior["subject"], subject)?;
            let old_revision = revision(m, prior, "line_revision")?;
            let next = meter::add(m, &old_revision, &1.into())?;
            if !meter::cmp(m, &line_revision, &next)?.is_eq() {
                return Err(shape::fail("next original provider line revision"));
            }
            let prior_ref = shape::reference(m, &assertion["prior_final_evidence_ref"])?;
            let actual = r.get(
                m,
                &c.key,
                &prior_ref,
                "workflow-fact-attestation",
                "ledger-workflow-supplier-final-cost/1",
            )?;
            eq(m, &actual, prior)?;
        }
    }
    eq(m, &assertion["unit"], &selected.original["unit"])?;
    let raw = shape::text(m, &assertion, "quantity")?;
    let quantity = arithmetic::decimal(m, raw)?;
    let maximum = shape::text(m, &selected.original, "maximum_quantity")?;
    decimal_at_most(m, &quantity, maximum)?;
    for limits in [
        &selected.terms["quantity_ceilings"],
        &selected.offer["unit_ceilings"],
    ] {
        for limit in limits
            .as_array()
            .ok_or_else(|| shape::fail("supplier quantity declarations"))?
        {
            m.visit("cost.original_quantity_limit")?;
            if shape::same(m, &limit["activity"], &selected.original["activity"])?
                && shape::same(m, &limit["unit"], &selected.original["unit"])?
            {
                let maximum = shape::text(m, limit, "maximum")?;
                decimal_at_most(m, &quantity, maximum)?;
            }
        }
    }
    let eligible = amount(m, &assertion["amount"])?;
    at_most(m, &eligible, &selected.terms["exposure_ceiling"])?;
    at_most(m, &eligible, &selected.offer["exposure_ceiling"])?;
    at_most(m, &eligible, &selected.original["authorized"])?;
    let (verification_ref, verification) = load(
        m,
        r,
        c,
        &proof["verification_ref"],
        "workflow-evidence-verification",
        "ledger-workflow-evidence-verification/1",
    )?;
    shape::closed(
        m,
        &verification,
        &[
            "schema",
            "attestation_ref",
            "attestation_raw_sha256",
            "attestation_digest",
            "verified_source",
            "verification_primitive",
            "source_grant_ref",
            "source_grant_revision",
            "authenticated_principal_ref",
            "trusted_received_at",
            "subject_binding_refs",
            "verification_evidence_refs",
        ],
        &[],
    )?;
    eq(
        m,
        &verification["attestation_ref"],
        &proof["attestation_ref"],
    )?;
    eq(
        m,
        &verification["attestation_digest"],
        &json!(assertion_ref.digest),
    )?;
    eq(m, &verification["verified_source"], &assertion["source"])?;
    eq(
        m,
        &verification["verification_primitive"],
        &json!("authenticated-nominated-source-retention/1"),
    )?;
    let expected = vec![
        copy(m, &selected.binding_ref)?,
        copy(m, &selected.work_ref)?,
        copy(m, &selected.original_ref)?,
        shape::reference(m, &selected.original["cost_authority_ref"])?,
    ];
    let expected = body::ref_set(m, expected)?;
    eq(m, &verification["subject_binding_refs"], &json!(expected))?;
    let raw = r.canonical(m, &assertion_ref)?;
    m.hash_bytes(raw.len())?;
    let digest = canonical::hex_with_work(&Sha256::digest(raw), m)?;
    eq(m, &verification["attestation_raw_sha256"], &json!(digest))?;
    let issued = instant(m, &assertion, "issued_at")?;
    let received = instant(m, &verification, "trusted_received_at")?;
    let accepted = Timestamp::parse_with_work(&c.accepted_at, m)?
        .micros()
        .into();
    if meter::cmp(m, &issued, &received)?.is_gt() || meter::cmp(m, &received, &accepted)?.is_gt() {
        return Err(shape::fail("supplier final proof trusted time"));
    }
    let (_, enrollment) = load(
        m,
        r,
        c,
        &verification["authenticated_principal_ref"],
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
    )?;
    let (_, grant) = load(
        m,
        r,
        c,
        &verification["source_grant_ref"],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
    )?;
    eq(m, &grant["grant_id"], &enrollment["enrollment_id"])?;
    eq(
        m,
        &grant["revision"],
        &verification["source_grant_revision"],
    )?;
    eq(
        m,
        &grant["original_ceiling_ref"],
        &verification["authenticated_principal_ref"],
    )?;
    eq(m, &grant["source"], &json!(source))?;
    for field in ["source", "principal_id", "canonical_economic_namespace"] {
        eq(m, &grant[field], &enrollment["ceiling"][field])?;
    }
    let starts = instant(m, &grant, "starts_at")?;
    let ends = instant(m, &grant, "ends_at")?;
    if meter::cmp(m, &received, &starts)?.is_lt() || !meter::cmp(m, &received, &ends)?.is_lt() {
        return Err(shape::fail("retained supplier source grant at receipt"));
    }
    let operations = shape::array(m, &grant, "operations")?;
    let permissions = shape::array(m, &grant, "permissions")?;
    if !shape::member(m, operations, &json!("evidence.retain"))?
        || !shape::member(m, permissions, &json!("submit"))?
        || !shape::member(m, permissions, &json!("read"))?
    {
        return Err(shape::fail("retained supplier source rights"));
    }
    let supporting = shape::array(m, &assertion, "supporting_evidence_refs")?;
    if w::compare(m, supporting.len(), 1usize)?.is_lt()
        || w::compare(m, supporting.len(), 16usize)?.is_gt()
    {
        return Err(shape::fail("supplier provider evidence set"));
    }
    super::super::inventory::binding::ordered(m, supporting)?;
    for evidence in supporting {
        let evidence = shape::reference(m, evidence)?;
        r.provenance(m, &c.key, &evidence)?;
    }
    let supporting = shape::array(m, &verification, "verification_evidence_refs")?;
    if w::compare(m, supporting.len(), 16usize)?.is_gt() {
        return Err(shape::fail("supplier verification evidence set"));
    }
    super::super::inventory::binding::ordered(m, supporting)?;
    for evidence in supporting {
        let evidence = shape::reference(m, evidence)?;
        r.provenance(m, &c.key, &evidence)?;
    }
    Ok(FinalEvidence {
        assertion_ref,
        assertion,
        verification_ref,
        verification,
        eligible,
        quantity,
        _ownership: ownership,
    })
}
