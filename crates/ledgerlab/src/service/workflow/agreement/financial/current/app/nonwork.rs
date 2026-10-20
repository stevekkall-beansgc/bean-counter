//! Held, targeted non-work operands. Stable identities come from the same core
//! selectors as execution; projection hits never supply authority or pricing.
use super::*;
use crate::store::sqlite::workflow::financial::{AppClaimLocator, HeadKey, HeadLocator};
use ledgerlab_core::policy::workflow::transition_constructor::{
    self as core, CostHead, LocalOperationLocators, PermanentClaimKey,
};

pub(super) struct ExtraState {
    pub(super) assertion: Option<ExactRef>,
    pub(super) costs: Vec<CostHead>,
    pub(super) observations: Vec<ExactRef>,
    pub(super) claims: Vec<ExactRef>,
    pub(super) consumed: Vec<ExactRef>,
    _storage: ExternalBufferLease,
}
fn decoded(b: &[u8]) -> Result<&str> {
    std::str::from_utf8(b).map_err(|_| ServiceError::IntegrityFailure)
}
fn claim_ref(row: &AppClaimLocator, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    let k = wire::string(decoded(row.kind.bytes())?, s)?;
    let i = wire::string(decoded(row.id.bytes())?, s)?;
    let d = wire::string(decoded(row.digest.bytes())?, s)?;
    let r = wire::object(
        &[
            ("digest", d.bytes()),
            ("id", i.bytes()),
            ("kind", k.bytes()),
        ],
        262144,
        s,
    )?;
    let v = s
        .parse_representation(r.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (reference, _) = source::from_value_local(v.value(), s)?;
    Ok(reference)
}
fn head_ref(row: &HeadLocator, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    let v = serde_json::json!({"kind":decoded(row.kind.bytes())?,"id":decoded(row.id.bytes())?,"digest":decoded(row.digest.bytes())?});
    source::from_value_local(&v, s).map(|v| v.0)
}
pub(in super::super::super) async fn observe(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<ObservedAppBinding> {
    let received = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let before = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let mut pool = super::super::super::local_inputs::LocalInputs::new(s)?;
    pool.payload_refs(payload, s)?;
    pool.complete(tx, key, before, s).await?;
    let ck = core::OperationKey {
        logical_store_id: key.logical_store_id.clone(),
        scope: key.scope.clone(),
        source: key.source.clone(),
        operation_id: key.operation_id.clone(),
    };
    let raw = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let subject = core::locate_local_subject_binding(&ck, raw.bytes(), &pool.views(), s)
        .map_err(core_error)?;
    let binding = serde_json::to_value(subject.original_binding_ref())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let kind = core_operation_kind(payload, s)?;
    let mut o = observe_binding(ledger, tx, key, kind, &binding, None, None, s).await?;
    let original_binding = s
        .parse_representation(
            o.original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    o.activation = Some(
        o.original
            .original_initial_activation(tx, key, original_binding.value(), &o.at, o.before, s)
            .await?,
    );
    require(
        o.original
            .publication
            .program()
            .uses_local_execution_recipe(),
        "UNSUPPORTED_SCHEMA_PROFILE",
    )?;
    let index = work_capture::source_index(&o, key, s)?;
    let birth = o
        .original
        .source_birth_inputs(tx, key, o.sources[index].enrollment_ref(), o.before, s)
        .await?;
    o.work_source_birth = Some(birth);
    let root = work_capture::retained_root(&mut o, tx, key, index, s).await?;
    o.work_root = Some(root);
    let locators = work_capture::with_capture(
        &o,
        key,
        payload,
        index,
        &received,
        s,
        |capture, _, raw, s| {
            core::select_local_operation_locators(
                o.original.publication.program(),
                capture,
                raw,
                &pool.views(),
                s,
            )
            .map_err(core_error)
        },
    )?;
    let mut state = if let Some(heads) = locators.work_heads() {
        let held = work_heads::observe(
            tx,
            &mut o.original,
            key,
            heads,
            o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?,
            s,
        )
        .await?;
        let state = held.typed_state(
            heads,
            o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?,
            s,
        )?;
        let seeds = s
            .parse_representation(held.seeds.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        pool.payload_refs(seeds.value(), s)?;
        state
    } else {
        work_heads::unattached_state(o.parties.as_ref().ok_or(ServiceError::IntegrityFailure)?, s)?
    };
    state.financial.work_claims.clear();
    for identity in locators.record_identities() {
        if let Some(row) = tx
            .workflow_local_record_identity(
                &key.logical_store_id,
                &key.scope,
                identity.kind(),
                identity.id(),
                identity.schema(),
                s,
            )
            .await
            .map_err(prefix::read_error)?
        {
            pool.add(&claim_ref(&row, s)?, s)?;
        }
    }
    // Concrete limits are retained. This owns the bounded Ref/head copies until
    // construction and journal framing finish under this same attempt.
    let owner = s
        .reserve_external_payload(4096 * 1024)
        .map_err(core_error)?;
    let mut extra = ExtraState {
        assertion: None,
        costs: Vec::new(),
        observations: Vec::new(),
        claims: Vec::new(),
        consumed: Vec::new(),
        _storage: owner,
    };
    for claim in locators.claim_keys() {
        let row = match claim {
            PermanentClaimKey::Assertion { .. } => tx.workflow_assertion_claim(claim, s).await,
            PermanentClaimKey::Work { .. } => tx.workflow_work_claim(claim, s).await,
            PermanentClaimKey::Outcome { .. } => {
                tx.workflow_outcome_claim(&key.logical_store_id, claim, s)
                    .await
            }
            PermanentClaimKey::AppCall { .. }
            | PermanentClaimKey::AppComponentObservation { .. }
            | PermanentClaimKey::AppProviderCall { .. }
            | PermanentClaimKey::AppCostComponent { .. }
            | PermanentClaimKey::AppFinalEvidence { .. }
            | PermanentClaimKey::AppObservationRevision { .. } => {
                tx.workflow_app_claim(claim, s).await
            }
            _ => return Err(reject("UNSUPPORTED_COMPOSITION")),
        }
        .map_err(prefix::read_error)?;
        if let Some(row) = row {
            let r = claim_ref(&row, s)?;
            pool.add(&r, s)?;
            match claim {
                PermanentClaimKey::Assertion { .. } => extra.assertion = Some(r),
                PermanentClaimKey::Work { .. } => {
                    if !state.financial.work_claims.contains(&r) {
                        state.financial.work_claims.push(r);
                    }
                }
                PermanentClaimKey::Outcome { .. } => state.financial.outcome_claims.push(r),
                PermanentClaimKey::AppCostComponent { .. } => {
                    extra.claims.push(r);
                }
                PermanentClaimKey::AppCall { .. }
                | PermanentClaimKey::AppComponentObservation { .. }
                | PermanentClaimKey::AppProviderCall { .. } => extra.observations.push(r),
                PermanentClaimKey::AppFinalEvidence { .. }
                | PermanentClaimKey::AppObservationRevision { .. } => extra.consumed.push(r),
                _ => unreachable!(),
            }
        }
    }
    // Outcome correction and causal root reversal consume the original
    // immutable claim's fixed link for each actually selected family.
    // The exact held projection is only a locator; pool.complete below verifies
    // its committed body, and the child repeats every semantic association.
    for (child, slot) in locators.original_outcome_links() {
        if let Some(row) = tx
            .workflow_local_outcome_link(&key.logical_store_id, &key.scope, child, slot, s)
            .await
            .map_err(prefix::read_error)?
        {
            let r = claim_ref(&row, s)?;
            pool.add(&r, s)?;
            s.semantic_work()
                .write_bytes(std::mem::size_of::<ExactRef>())
                .map_err(core_error)?;
            state.financial.links.push(r);
        }
    }
    for r in &extra.claims {
        if let Some(h) = tx
            .workflow_financial_head(&key.logical_store_id, &key.scope, HeadKey::AppCost(r), s)
            .await
            .map_err(prefix::read_error)?
        {
            let reference = head_ref(&h, s)?;
            pool.add(&reference, s)?;
            extra.costs.push(CostHead {
                cost_claim_ref: r.clone(),
                revision: decoded(h.revision.bytes())?.to_owned(),
                reference,
            });
        }
    }
    for r in o
        .original
        .publication
        .required_refs()
        .chain(
            o.original
                .acceptance
                .work_locator_records()
                .map(|r| r.reference),
        )
        .chain(
            o.sources
                .iter()
                .flat_map(source::ObservedSource::record_refs),
        )
        .chain(o.parties.iter().flat_map(|p| p.selected_refs()))
    {
        pool.add(r, s)?;
    }
    if let Some(r) = o.work_root.as_ref() {
        pool.add(r.reference(), s)?;
    }
    if let Some(b) = o.work_source_birth.as_ref() {
        pool.add(b.state.reference(), s)?;
        pool.add(b.grant.reference(), s)?;
    }
    if let Some(a) = o.activation.as_ref() {
        pool.add(a.head.reference(), s)?;
        pool.add(a.activation.reference(), s)?;
    }

    // Complete the actually selected Claim/Revision bodies before deriving
    // their original retention-proof operands. No current/latest substitute.
    pool.complete(tx, key, o.before, s).await?;
    let payload_schema = check::text(payload, "schema", s)?;
    let mut assertions = Vec::new();
    // Only app correction and causal root changes consume the original app
    // proof pairs. Outcome declarations must contain their named facts only.
    if matches!(
        payload_schema,
        "ledger-workflow-cost-correct/2"
            | "ledger-workflow-work-reverse/2"
            | "ledger-workflow-quantity-correction/2"
    ) {
        for claim in &extra.claims {
            let body = s
                .parse_representation(pool.body(claim)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let (reference, _) =
                source::from_value_local(check::field(body.value(), "observation_ref", s)?, s)?;
            if !assertions.contains(&reference) {
                assertions.push(reference);
            }
        }
        for head in &extra.costs {
            let body = s
                .parse_representation(pool.body(&head.reference)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let (reference, _) = source::from_value_local(
                check::field(body.value(), "current_evidence_ref", s)?,
                s,
            )?;
            if !assertions.contains(&reference) {
                assertions.push(reference);
            }
        }
    }
    // Named replacement facts use the same committed receipt association as
    // initial outcome facts. Explicit None carries no new assertions.
    let proof_payload = if payload_schema == "ledger-workflow-outcome-correction/1" {
        check::field(payload, "replacement", s)?
    } else {
        payload
    };
    if let Some(proofs) = proof_payload.get("fact_proofs").and_then(Value::as_array) {
        for proof in proofs {
            let (reference, _) =
                source::from_value_local(check::field(proof, "attestation_ref", s)?, s)?;
            if !assertions.contains(&reference) {
                assertions.push(reference);
            }
        }
    }
    for assertion in assertions {
        let reference = retained_verification(tx, key, o.before, &assertion, s).await?;
        pool.add(&reference, s)?;
        if !state.financial.evidence_verifications.contains(&reference) {
            state.financial.evidence_verifications.push(reference);
        }
    }
    pool.complete(tx, key, o.before, s).await?;
    o.work_state = Some(state);
    o.local_input_bodies = Some(pool);
    o.work_source_index = Some(index);
    o.work_received_at = Some(received);
    o.local_extra = Some(extra);
    o.local_locators = Some(locators);
    Ok(o)
}

/// Resolve one named assertion's verification only from its checked original
/// retention receipt; downstream evidence consumers still verify every join.
async fn retained_verification(
    tx: &mut SqliteTx,
    key: &OperationKey,
    before: i64,
    att_ref: &ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<ExactRef> {
    require(
        att_ref.kind == "workflow-fact-attestation",
        "INTEGRITY_FAILURE",
    )?;
    let seq = tx
        .workflow_selected_association_sequence(
            att_ref,
            &key.logical_store_id,
            &key.scope,
            before,
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let origin = prefix::next(tx, key, seq - 1, seq, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let mut selected = None;
    for r in check::field(origin.wrappers().receipt(), "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if check::text(r, "kind", s)? != "workflow-evidence-verification" {
            continue;
        }
        let (reference, _) = source::from_value_local(r, s)?;
        let record = tx
            .workflow_financial_document(&reference, &key.logical_store_id, &key.scope, before, s)
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            record.schema.bytes() == b"ledger-workflow-evidence-verification/1",
            "INTEGRITY_FAILURE",
        )?;
        let parsed = s
            .parse_representation(record.body.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let expected = wire::reference(att_ref, s)?;
        let a = check::field(parsed.value(), "attestation_ref", s)?;
        let a = s
            .canonical_representation(a, RepresentationKind::Record)
            .map_err(core_error)?;
        if a.bytes() == expected.bytes() {
            require(selected.replace(reference).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    selected.ok_or(ServiceError::IntegrityFailure)
}

fn core_operation_kind(payload: &Value, s: &mut OriginalAuditSession) -> Result<&'static str> {
    Ok(match check::text(payload, "schema", s)? {
        "ledger-workflow-fact-attest/1"
        | "ledger-workflow-family-close-attest/1"
        | "ledger-workflow-app-observe/1"
        | "ledger-workflow-app-cost-attest/1" => "evidence.retain",
        "ledger-workflow-cost-finalize/2" => "cost.finalize",
        "ledger-workflow-cost-correct/2" => "cost.correct",
        "ledger-workflow-outcome/1" => "outcome.submit",
        "ledger-workflow-outcome-correction/1" => "outcome.correct",
        "ledger-workflow-outcome-close/2" => "outcome.close",
        "ledger-workflow-quantity-correction/2" => "quantity.correct",
        "ledger-workflow-work-reverse/2" => "work.reverse",
        _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
    })
}
