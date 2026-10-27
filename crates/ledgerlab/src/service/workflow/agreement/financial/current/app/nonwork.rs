//! Held, targeted non-work operands. Stable identities come from the same core
//! selectors as execution; projection hits never supply authority or pricing.
use super::*;
use crate::store::sqlite::workflow::financial::{AppClaimLocator, HeadKey, HeadLocator};
use ledgerlab_core::policy::workflow::transition_constructor::{
    self as core, CostHead, PermanentClaimKey,
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
    let mut o = observe_binding(ledger, tx, key, kind, &binding, (None, None), s).await?;
    pool.select_stage_recipe(o.original.publication.program());
    let original_binding = s
        .parse_representation(
            o.original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    o.activation = Some(if kind=="invocation.authorize" {
        o.original.current_initial_activation(tx,key,original_binding.value(),&o.at,o.before,s).await?
    }else{
        o.original.original_initial_activation(tx,key,original_binding.value(),&o.at,o.before,s).await?
    });
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
    work_capture::retain_namespace_birth_inputs(&mut o, tx, key, s).await?;
    let root = work_capture::retained_root(&mut o, tx, key, index, s).await?;
    o.work_root = Some(root);
    let mut locators = work_capture::with_capture(
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
    super::stage::retain_invocation_operation(&mut o,&mut state,&mut pool,tx,key,payload,s).await?;
    super::stage::retain_stage_operation(&mut o,&mut state,&mut pool,tx,key,payload,s).await?;
    if let Some(report) = locators.selector_start_work_heads() {
        let held =
            work_heads::observe_relation_decision(tx, &mut o.original, key, report, &mut state, s)
                .await?;
        o._relation_head = Some(held);
        let seed = s
            .parse_representation(
                o._relation_head
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .seed
                    .bytes(),
                RepresentationKind::Record,
            )
            .map_err(core_error)?;
        pool.payload_refs(seed.value(), s)?;
    }
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
        for input in b.namespace_owner_views() { pool.add(input.reference, s)?; }
    }
    for birth in &o.namespace_source_births {
        pool.add(birth.state.reference(), s)?;
        pool.add(birth.grant.reference(), s)?;
        for input in birth.namespace_owner_views() { pool.add(input.reference, s)?; }
    }
    if let Some(a) = o.activation.as_ref() {
        pool.add(a.head.reference(), s)?;
        pool.add(a.activation.reference(), s)?;
    }

    // Complete the actually selected Claim/Revision bodies before deriving
    // their original retention-proof operands. No current/latest substitute.
    pool.complete(tx, key, o.before, s).await?;
    let payload_schema = check::text(payload, "schema", s)?;
    if matches!(
        payload_schema,
        "ledger-workflow-work-reverse/2" | "ledger-workflow-quantity-correction/2"
    ) {
        // Only the new scalar policy recipe needs one immediate prior root
        // to authenticate its current inverse/replacement prefix. This is an
        // immutable exact-Ref read, never another mutable head or history walk.
        locators
            .retain_current_root_predecessor(
                o.original.publication.program(),
                &ck,
                &state.financial.decision_heads,
                &state.capacities,
                &pool.views(),
                s,
            )
            .map_err(core_error)?;
        if let Some(prior) = locators.current_root_predecessor_ref() {
            pool.add(prior, s)?;
            pool.complete(tx, key, o.before, s).await?;
        }
        locators
            .retain_current_family_outcome_links(
                o.original.publication.program(),
                &ck,
                &state.financial.family_heads,
                &pool.views(),
                s,
            )
            .map_err(core_error)?;
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
    let proof_payload = outcome_proof_payload(payload, payload_schema, s)?;
    let original_app_proofs = matches!(
        payload_schema,
        "ledger-workflow-cost-correct/2"
            | "ledger-workflow-work-reverse/2"
            | "ledger-workflow-quantity-correction/2"
    );
    let app_count = if original_app_proofs {
        s.semantic_work()
            .arithmetic(
                Op::Add,
                &[&extra.claims.len().into(), &extra.costs.len().into()],
            )
            .map_err(core_error)?;
        extra
            .claims
            .len()
            .checked_add(extra.costs.len())
            .ok_or(ServiceError::Unavailable)?
    } else {
        0
    };
    let proof_count = proof_payload
        .get("fact_proofs")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    s.semantic_work()
        .arithmetic(Op::Add, &[&app_count.into(), &proof_count.into()])
        .map_err(core_error)?;
    let count = app_count
        .checked_add(proof_count)
        .ok_or(ServiceError::Unavailable)?;
    let stride = std::mem::size_of::<(ExactRef, ExternalBufferLease)>();
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&count.into(), &stride.into()])
        .map_err(core_error)?;
    let bytes = count.checked_mul(stride).ok_or(ServiceError::Unavailable)?;
    let control = std::mem::size_of::<Vec<(ExactRef, ExternalBufferLease)>>();
    s.semantic_work()
        .arithmetic(Op::Add, &[&bytes.into(), &control.into()])
        .map_err(core_error)?;
    let bytes = bytes
        .checked_add(control)
        .ok_or(ServiceError::Unavailable)?;
    let assertion_storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut assertions = Vec::new();
    assertions
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    // Only app correction and causal root changes consume the original app
    // proof pairs. Outcome declarations must contain their named facts only.
    if original_app_proofs {
        for claim in &extra.claims {
            let body = s
                .parse_representation(pool.body(claim)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let reference =
                source::from_value_local(check::field(body.value(), "observation_ref", s)?, s)?;
            if !super::contains_reference(assertions.iter().map(|(r, _)| r), &reference.0, s)? {
                assertions.push(reference);
            }
        }
        for head in &extra.costs {
            let body = s
                .parse_representation(pool.body(&head.reference)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let reference = source::from_value_local(
                check::field(body.value(), "current_evidence_ref", s)?,
                s,
            )?;
            if !super::contains_reference(assertions.iter().map(|(r, _)| r), &reference.0, s)? {
                assertions.push(reference);
            }
        }
    }
    // Named replacement facts use the same committed receipt association as
    // initial outcome facts. Explicit None carries no new assertions.
    if let Some(proofs) = proof_payload.get("fact_proofs").and_then(Value::as_array) {
        for proof in proofs {
            let reference =
                source::from_value_local(check::field(proof, "attestation_ref", s)?, s)?;
            if !super::contains_reference(assertions.iter().map(|(r, _)| r), &reference.0, s)? {
                assertions.push(reference);
            }
        }
    }
    o._verification_storage = if assertions.is_empty() {
        None
    } else {
        Some(super::verification_storage(
            &mut state.financial.evidence_verifications,
            assertions.len(),
            s,
        )?)
    };
    for assertion in assertions {
        let retained = retained_verification(tx, key, o.before, &assertion.0, s).await?;
        pool.add(&retained.0, s)?;
        if !super::contains_reference(&state.financial.evidence_verifications, &retained.0, s)? {
            o._verification_storage
                .as_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .0
                .push(retained.1);
            state.financial.evidence_verifications.push(retained.0);
        }
    }
    drop(assertion_storage);
    pool.complete(tx, key, o.before, s).await?;
    let program = o.original.publication.program();
    if program.uses_declared_work_link_recipe() || program.uses_policy_semantics_recipe() {
        if let Some(head) = &state.financial.chain_head {
            let chain = s
                .parse_representation(pool.body(head)?, RepresentationKind::Record)
                .map_err(core_error)?;
            let mut links = super::chain_links::acquire(
                tx,
                key,
                super::chain_links::Selection {
                    recipe: if program.uses_stage_lifecycle_recipe() {
                        super::chain_links::ClaimRecipe::Stage
                    } else if program.uses_policy_semantics_recipe() {
                        super::chain_links::ClaimRecipe::Policy
                    } else {
                        super::chain_links::ClaimRecipe::Declared
                    },
                    binding: o.original.acceptance.binding.reference(),
                    customer: check::text(chain.value(), "customer", s)?,
                    chain: check::text(chain.value(), "chain", s)?,
                    before: o.before,
                },
                &mut pool,
                s,
            )
            .await?;
            state.financial.links = std::mem::take(&mut links.references);
            o._chain_storage = Some(links);
        }
    }
    if program.uses_work_lifecycle_recipe() {
        let raw = s
            .canonical_representation(payload, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        let app = core::AppStateData {
            cost_heads: &extra.costs,
            observation_refs: &extra.observations,
            cost_claim_refs: &extra.claims,
            evidence_consumption_refs: &extra.consumed,
        };
        let (correspondences, _correspondence_owner) = work_capture::source_correspondences(&o, s)?;
        let supplier=core::SupplierStateData{invocation_heads:&state.invocations,cost_heads:&[],cost_consumption_heads:&[],cost_owner_heads:&[]};
        let data = core::EconomicStateData {
            retained_assertion: extra.assertion.as_ref(),
            financial: &state.financial,
            capacity_heads: &state.capacities,
            supplier: program.uses_stage_lifecycle_recipe().then_some(&supplier),
            app: Some(&app),
            current_party_authorizations: &state.parties,
            observation: None,
            audit: None,
            initial_source_correspondences: &correspondences,
        };
        let groups = locators
            .select_fact_reference_inputs(program, &ck, raw.bytes(), (&data, &pool.views()), s)
            .map_err(core_error)?;
        o._fact_reference_groups = Some(groups);
        for group in o
            ._fact_reference_groups
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .groups()
        {
            for input in group.inputs().entries() {
                pool.add(input.reference(), s)?;
            }
        }
        pool.complete_lifecycle(program, tx, key, o.before, s)
            .await?;
    }
    work_capture::required_role_registrations(&o, &locators, tx, key, s).await?;
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
pub(super) async fn retained_verification(
    tx: &mut SqliteTx,
    key: &OperationKey,
    before: i64,
    att_ref: &ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<(ExactRef, ExternalBufferLease)> {
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
        let retained = source::from_value_local(r, s)?;
        let record = tx
            .workflow_financial_document(&retained.0, &key.logical_store_id, &key.scope, before, s)
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
            require(selected.replace(retained).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    selected.ok_or(ServiceError::IntegrityFailure)
}

fn outcome_proof_payload<'a>(
    payload: &'a Value,
    schema: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    if matches!(
        schema,
        "ledger-workflow-outcome-correction/1" | "ledger-workflow-outcome-correction/2"
    ) {
        check::field(payload, "replacement", s)
    } else {
        Ok(payload)
    }
}

fn core_operation_kind(payload: &Value, s: &mut OriginalAuditSession) -> Result<&'static str> {
    Ok(match check::text(payload, "schema", s)? {
        "ledger-workflow-invocation-authorize/1" | "ledger-workflow-stage-authorize/1" => "invocation.authorize",
        "ledger-workflow-invocation-close/1" => "invocation.close",
        "ledger-workflow-stage-close/1" => "outcome.submit",
        "ledger-workflow-fact-attest/1"
        | "ledger-workflow-family-close-attest/1"
        | "ledger-workflow-app-observe/1"
        | "ledger-workflow-app-cost-attest/1" => "evidence.retain",
        "ledger-workflow-cost-finalize/2" => "cost.finalize",
        "ledger-workflow-cost-correct/2" => "cost.correct",
        "ledger-workflow-outcome/1" | "ledger-workflow-outcome/2" => "outcome.submit",
        "ledger-workflow-outcome-correction/1" | "ledger-workflow-outcome-correction/2" => {
            "outcome.correct"
        }
        "ledger-workflow-outcome-close/2" => "outcome.close",
        "ledger-workflow-quantity-correction/2" => "quantity.correct",
        "ledger-workflow-work-reverse/2" => "work.reverse",
        _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
    })
}

#[cfg(test)]
mod declared_dispatch_tests {
    use super::*;
    #[test]
    fn exact_outcome_versions_route_and_select_replacement_proofs() {
        for version in ["1", "2"] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            for (prefix, kind) in [
                ("ledger-workflow-outcome/", "outcome.submit"),
                ("ledger-workflow-outcome-correction/", "outcome.correct"),
            ] {
                let schema = format!("{prefix}{version}");
                assert!(super::super::super::super::is_local_financial_schema(
                    &schema
                ));
                let replacement = json!({"schema":format!("ledger-workflow-outcome/{version}"),"fact_proofs":[{"field":"support:code","attestation_ref":{"kind":"workflow-fact-attestation","id":"synthetic-only","digest":"synthetic-only"}}]});
                let payload = if kind == "outcome.correct" {
                    json!({"schema":schema,"replacement":replacement})
                } else {
                    replacement.clone()
                };
                assert_eq!(core_operation_kind(&payload, &mut s).unwrap(), kind);
                let selected = outcome_proof_payload(&payload, &schema, &mut s).unwrap();
                assert_eq!(selected["fact_proofs"], replacement["fact_proofs"]);
                if kind == "outcome.correct" {
                    assert!(std::ptr::eq(selected, &payload["replacement"]));
                    let none = json!({"schema":schema,"replacement":{"kind":"none","evidence_refs":[],"reason":"synthetic"}});
                    let selected = outcome_proof_payload(&none, &schema, &mut s).unwrap();
                    assert_eq!(selected["kind"], "none");
                    assert!(selected.get("fact_proofs").is_none());
                }
            }
        }
        for schema in [
            "ledger-workflow-outcome/3",
            "ledger-workflow-outcome-correction/02",
            "foreign/outcome/2",
        ] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(!super::super::super::super::is_local_financial_schema(
                schema
            ));
            assert!(
                matches!(core_operation_kind(&json!({"schema":schema}), &mut s), Err(ServiceError::Rejection(code)) if code == "UNSUPPORTED_SCHEMA_PROFILE")
            );
        }
    }
}
