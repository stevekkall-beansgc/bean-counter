//! Immutable customer app-cost obligation lineage. Source/evidence eligibility
//! is selected by the same executor; no supplier payable/high-water is emitted.
use super::{original::OriginalApp,evidence::{AppObservation,AppFinalAmount}};
use super::super::{CoreMeter,body,data::*,emitted_record::ConstructedEnvelope,shape,working_value::owning_reference};
use crate::{Result,canonical::audit_arena::ExternalBufferLease,validation_work::{self as w,RepresentationKind}};
use serde_json::Value;
use num_bigint::BigInt;
pub(super) struct AppLineage {
    pub consumption:ConstructedEnvelope,
    pub revision:ConstructedEnvelope,
    _ownership:ExternalBufferLease,
}
pub(super) fn claim(m:&mut CoreMeter,c:&DraftContextData,original:&OriginalApp,observed:&AppObservation,final_cost:&AppFinalAmount,dependency:&ExactRef)->Result<ConstructedEnvelope>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning app cost claim"))?;
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-app-cost-claim/1"}"#,RepresentationKind::SourceDefinition)?;
    let observation=owning_reference(m,&arena,&observed.pair.attestation_ref)?;
    let evidence=owning_reference(m,&arena,&final_cost.pair.attestation_ref)?;
    let binding=owning_reference(m,&arena,&original.binding_ref)?;
    let dependency=owning_reference(m,&arena,dependency)?;
    let key=super::super::cost_postings::operation_key(m,&arena,c)?;
    let value=arena.object_from_fields(&[("schema",&constants.value()["schema"]),("subject",&observed.pair.attestation["subject"]),("observation_ref",observation.value()),("original_final_evidence_ref",evidence.value()),("original_terms_ref",binding.value()),("dependency_ref",dependency.value()),("operation_key",key.value())],m)?;
    body::document_borrowed(m,"workflow-cost",value.value())
}
/// Only actually validated old/current record references are passed by the
/// private plan. Revision/decision/evidence equality is checked by that plan;
/// this emitter never derives a fresh entitlement from saved monetary output.
pub(super) fn advance(m:&mut CoreMeter,c:&DraftContextData,claim_ref:&ExactRef,observation_ref:&ExactRef,final_cost:&AppFinalAmount,decision_ref:&ExactRef,revision:&BigInt,prior:Option<&ExactRef>)->Result<AppLineage>{
    let arena=m.representation_arena().ok_or_else(||shape::fail("owning app cost lineage"))?;
    let bytes=w::usize_add(m,std::mem::size_of::<AppLineage>(),2*(64+128+71))?;
    let ownership=arena.reserve_external_payload(bytes,m)?;
    let constants=m.parse_owned_representation(&arena,br#"{"consumption":"ledger-workflow-app-cost-consumption/1","revision":"ledger-workflow-app-cost-revision/1","active":"active"}"#,RepresentationKind::SourceDefinition)?;
    let claim=owning_reference(m,&arena,claim_ref)?;let observation=owning_reference(m,&arena,observation_ref)?;
    let evidence=owning_reference(m,&arena,&final_cost.pair.attestation_ref)?;
    let key=super::super::cost_postings::operation_key(m,&arena,c)?;
    let amount=super::super::cost_postings::amount(m,&arena,&final_cost.eligible)?;
    let counter=super::super::cost_postings::scalar(m,&arena,revision)?;
    let value=arena.object_from_fields(&[("schema",&constants.value()["consumption"]),("cost_claim_ref",claim.value()),("observation_ref",observation.value()),("final_evidence_ref",evidence.value()),("cost_revision",&final_cost.pair.attestation["cost_revision"]),("eligible_amount",amount.value()),("operation_key",key.value())],m)?;
    let consumption=body::document_borrowed(m,"workflow-cost",value.value())?;
    let decision=owning_reference(m,&arena,decision_ref)?;let consumed=owning_reference(m,&arena,&consumption.reference)?;
    let revision_value=arena.object_from_fields(&[("schema",&constants.value()["revision"]),("cost_claim_ref",claim.value()),("revision",counter.value()),("current_decision_ref",decision.value()),("current_evidence_ref",evidence.value()),("consumption_ref",consumed.value()),("status",&constants.value()["active"]),("operation_key",key.value())],m)?;
    let revision_value=if let Some(prior)=prior {
        let prior=owning_reference(m,&arena,prior)?;
        super::super::working_value::WorkingValue::Owned(revision_value).with_fields(m,&[("prior_ref",prior.value())])?.into_owned()?
    }else{revision_value};
    let revision=body::document_borrowed(m,"workflow-cost",revision_value.value())?;
    Ok(AppLineage{consumption,revision,_ownership:ownership})
}
