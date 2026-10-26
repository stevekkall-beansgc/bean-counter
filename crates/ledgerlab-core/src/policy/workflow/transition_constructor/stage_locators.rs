//! Exact Stage head/member lookup DATA. Host retains producer/member custody,
//! locked currentness/absence, and every returned owner through construction.
use super::super::selected_program::SelectedProgram;
use super::{
    data::*,
    records::Records,
    shape,
    working_value::{parse_owned_reference, OwnedExactRef, WorkingValue},
    CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, OriginalAuditSession, ValidationWork},
    Result,
};
use std::mem;

pub struct StageExpectedHeadLocator {
    claim: String,
    decision: String,
}
impl StageExpectedHeadLocator {
    pub fn work_claim_id(&self) -> &str {
        &self.claim
    }
    pub fn decision_id(&self) -> &str {
        &self.decision
    }
}
pub struct StageOperationLocators {
    agreement: String,
    chain: String,
    stage: String,
    binding: OwnedExactRef,
    instance: Option<OwnedExactRef>,
    members: Vec<StageExpectedHeadLocator>,
    _ownership: ExternalBufferLease,
}
impl StageOperationLocators {
    pub fn agreement(&self) -> &str {
        &self.agreement
    }
    pub fn chain(&self) -> &str {
        &self.chain
    }
    pub fn stage(&self) -> &str {
        &self.stage
    }
    pub fn original_binding_ref(&self) -> &ExactRef {
        self.binding.reference()
    }
    pub fn stage_instance_ref(&self) -> Option<&ExactRef> {
        self.instance.as_ref().map(|v| v.reference())
    }
    pub fn expected_work_heads(&self) -> &[StageExpectedHeadLocator] {
        &self.members
    }
}
fn text(m: &mut CoreMeter, value: &str) -> Result<String> {
    m.scan_bytes(value.len())?;
    m.write_bytes(value.len())?;
    Ok(value.to_owned())
}
pub fn select_stage_operation_locators(
    program: &SelectedProgram,
    key: &OperationKey,
    raw_payload: &[u8],
    inputs: &[EconomicRecordView<'_>],
    session: &mut OriginalAuditSession,
) -> Result<Option<StageOperationLocators>> {
    let result = (|| {
        session.check_cancelled()?;
        let arena = session.arena().shared_handle();
        let m = session.meter_mut();
        program.require_attempt_owner(m)?;
        let payload = WorkingValue::parse(m, raw_payload)?;
        let schema = shape::text(m, &payload, "schema")?;
        let authorize = shape::same_text(m, schema, "ledger-workflow-stage-authorize/1")?;
        let close = shape::same_text(m, schema, "ledger-workflow-stage-close/1")?;
        if !authorize && !close {
            return Ok(None);
        }
        if !program.uses_stage_lifecycle_recipe() {
            return Err(shape::fail("Stage operation lookup recipe"));
        }
        let mut records = Records::borrowed_selected(m, &arena, key, inputs, &[], program)?;
        let instance = if close {
            Some(parse_owned_reference(
                m,
                &arena,
                &payload["stage_instance_ref"],
            )?)
        } else {
            None
        };
        let instance_body = if let Some(reference) = &instance {
            Some(records.get(
                m,
                key,
                reference.reference(),
                "workflow-stage-instance",
                "ledger-workflow-stage-instance/1",
            )?)
        } else {
            None
        };
        let declaration = if let Some(body) = &instance_body {
            &**body
        } else {
            shape::field(m, &payload, "stage_instance")?
        };
        let expected = shape::array(m, declaration, "expected")?;
        if expected.is_empty() || w::compare(m, expected.len(), 32usize)?.is_gt() {
            return Err(shape::fail("Stage expected head dimension"));
        }
        let binding = parse_owned_reference(m, &arena, &declaration["binding_ref"])?;
        let body = records.get(
            m,
            key,
            binding.reference(),
            "workflow-binding",
            "ledger-workflow-binding/2",
        )?;
        for field in ["agreement_id", "customer"] {
            shape::equal(m, &body[field], &declaration[field])?;
        }
        let namespace = shape::text(m, &body, "source_namespace")?;
        let agreement = shape::text(m, declaration, "agreement_id")?;
        let chain = shape::text(m, declaration, "chain")?;
        let stage = shape::text(m, declaration, "stage_name")?;
        let per = mem::size_of::<StageExpectedHeadLocator>() + 2 * 128;
        ValidationWork::arithmetic(
            m,
            w::ArithmeticOperation::Multiply,
            &[&expected.len().into(), &per.into()],
        )?;
        let bytes = expected
            .len()
            .checked_mul(per)
            .ok_or_else(|| shape::fail("Stage head locator storage"))?;
        let bytes = w::usize_add(m, bytes, mem::size_of::<StageOperationLocators>())?;
        let bytes = w::usize_add(m, bytes, agreement.len())?;
        let bytes = w::usize_add(m, bytes, chain.len())?;
        let bytes = w::usize_add(m, bytes, stage.len())?;
        let ownership = arena.reserve_external_payload(bytes, m)?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(expected.len())
            .map_err(|_| shape::fail("Stage head locator allocation"))?;
        for member in expected {
            m.visit("Stage.exact_expected_head_lookup")?;
            super::inventory::binding::id(m, &member["business_operation_id"], 128)?;
            let claim = super::work_keys::claim(m, key, namespace, member)?;
            let decision = super::work_keys::decision(m, &claim)?;
            for old in &members {
                let old: &StageExpectedHeadLocator = old;
                if shape::same_text(m, &old.claim, &claim)? {
                    return Err(shape::fail("duplicate Stage canonical expected completion"));
                }
            }
            members.push(StageExpectedHeadLocator { claim, decision });
        }
        Ok(Some(StageOperationLocators {
            agreement: text(m, agreement)?,
            chain: text(m, chain)?,
            stage: text(m, stage)?,
            binding,
            instance,
            members,
            _ownership: ownership,
        }))
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
