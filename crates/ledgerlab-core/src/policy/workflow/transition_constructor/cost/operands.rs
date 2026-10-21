//! Exact retained supplier operands used by the same finalize/correct executor.
//! These are checked DATA associations. Host completed origins, owner custody,
//! current source authorization and complete capacity remain separate gates.
use super::super::{arithmetic, body, data::*, record_body::RecordBody, shape, CoreMeter};
use super::{nonnegative, InvocationAmounts};
use crate::canonical::audit_arena::ExternalBufferLease;
use crate::Result;
use num_bigint::BigInt;
use serde_json::{json, Value};

pub(in crate::policy::workflow::transition_constructor) struct InvocationSelection {
    pub original_ref: ExactRef,
    pub original: RecordBody,
    pub current_ref: ExactRef,
    pub current: RecordBody,
    pub work_ref: ExactRef,
    pub _work: RecordBody,
    pub binding_ref: ExactRef,
    pub _binding: RecordBody,
    pub terms_ref: ExactRef,
    pub _terms: RecordBody,
    pub _offer: RecordBody,
    pub _owner: RecordBody,
    pub amounts: InvocationAmounts,
    // References, counters and scalar scratch are retained under this owner.
    _ownership: ExternalBufferLease,
}
pub(in crate::policy::workflow::transition_constructor) struct FinalEvidence {
    pub assertion_ref: ExactRef,
    pub assertion: RecordBody,
    pub _verification_ref: ExactRef,
    pub _verification: RecordBody,
    pub _eligible: BigInt,
    pub _quantity: arithmetic::Ratio,
    _ownership: ExternalBufferLease,
}

fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
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
                _work: owned(&mut m, &arena, &json!({})),
                binding_ref: binding_ref.clone(),
                _binding: owned(&mut m, &arena, &json!({})),
                terms_ref: terms_ref.clone(),
                _terms: owned(&mut m, &arena, &json!({})),
                _offer: owned(&mut m, &arena, &json!({})),
                _owner: owned(&mut m, &arena, &json!({})),
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
                _verification_ref: reference("workflow-evidence-verification", "verification"),
                _verification: owned(&mut m, &arena, &json!({})),
                _eligible: eligible.into(),
                _quantity: arithmetic::Ratio {
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
                (&*claim_body, &decision, actions.as_slice()),
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
