//! A permanent Work collision selects its actual original before fresh
//! activation/pricing. Current registered delivery rights remain mandatory.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor as core;

pub(super) struct AliasWork {
    work: ExactRef,
    sequence: i64,
    _owner: ExternalBufferLease,
}
fn decoded<'a>(bytes: &'a [u8], s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    std::str::from_utf8(bytes).map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) async fn select(
    o: &mut ObservedAppBinding,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    index: usize,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    if !o
        .original
        .publication
        .program()
        .uses_shared_namespace_recipe()
    {
        return Ok(false);
    }
    let source = o.sources.get(index).ok_or(ServiceError::IntegrityFailure)?;
    let grant = source
        .current_grant_body()
        .ok_or(ServiceError::IntegrityFailure)?;
    let namespace = check::text(grant, "canonical_economic_namespace", s)?;
    check::same_text(
        namespace,
        check::text(o._parsed_binding.value(), "source_namespace", s)?,
        s,
    )?;
    let enrollment = check::text(source.enrollment_record().body(), "enrollment_id", s)?;
    require(
        tx.workflow_financial_registered_mapping(
            &key.logical_store_id,
            &key.scope,
            enrollment,
            &source.enrollment_ref().id,
            &key.source,
            namespace,
            s,
        )
        .await
        .map_err(prefix::read_error)?,
        "AUTHORITY_REQUIRED",
    )?;
    let business = check::text(payload, "business_operation_id", s)?;
    let bytes = std::mem::size_of::<core::PermanentClaimKey>()
        + key.logical_store_id.len()
        + key.scope[0].len()
        + key.scope[1].len()
        + namespace.len()
        + business.len()
        + "completion".len();
    let _claim_owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let claim = core::PermanentClaimKey::Work {
        logical_store_id: key.logical_store_id.clone(),
        scope: key.scope.clone(),
        source_namespace: namespace.to_owned(),
        business_operation_id: business.to_owned(),
        completion: "completion".into(),
    };
    let Some(claim) = tx
        .workflow_work_claim(&claim, s)
        .await
        .map_err(prefix::read_error)?
    else {
        return Ok(false);
    };
    let kind = wire::string(decoded(claim.kind.bytes(), s)?, s)?;
    let id = wire::string(decoded(claim.id.bytes(), s)?, s)?;
    let digest = wire::string(decoded(claim.digest.bytes(), s)?, s)?;
    let encoded = wire::object(
        &[
            ("kind", kind.bytes()),
            ("id", id.bytes()),
            ("digest", digest.bytes()),
        ],
        262144,
        s,
    )?;
    let parsed = s
        .parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (claim, _owner) = source::from_value_local(parsed.value(), s)?;
    check::same_text(&claim.kind, "workflow-work-claim", s)?;
    let mut pool = super::super::super::local_inputs::LocalInputs::new(s)?;
    pool.select_stage_recipe(o.original.publication.program());
    pool.add(&claim, s)?;
    pool.complete(tx, key, o.before, s).await?;
    let body = s
        .parse_representation(pool.body(&claim)?, RepresentationKind::Record)
        .map_err(core_error)?;
    let (work, owner) = source::from_value_local(check::field(body.value(), "work_ref", s)?, s)?;
    let sequence = tx
        .workflow_selected_association_sequence(
            &work,
            &key.logical_store_id,
            &key.scope,
            o.before,
            s,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let body = s
        .parse_representation(pool.body(&work)?, RepresentationKind::Record)
        .map_err(core_error)?;
    let at = Timestamp::parse_with_work(
        check::text(body.value(), "accepted_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let binding = s
        .parse_representation(
            o.original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    o.activation = Some(
        o.original
            .original_initial_activation(tx, key, binding.value(), &at, sequence, s)
            .await?,
    );
    o.alias_work = Some(AliasWork {
        work,
        sequence,
        _owner: owner,
    });
    o.local_input_bodies = Some(pool);
    Ok(true)
}

pub(in super::super::super) async fn construct(
    observed: &mut ObservedAppBinding,
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &super::super::super::ingress::IncomingRequest,
    s: &mut OriginalAuditSession,
) -> Result<(core::EconomicCandidate, core::LocalFinancialRequestEvidence)> {
    let alias = observed
        .alias_work
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let prior = alias
        .sequence
        .checked_sub(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let original = prefix::next(tx, req.key(), prior, alias.sequence, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?
        .checked_wrappers(tx, s)
        .await?;
    let reconstructed =
        super::super::super::local_original_work::reconstruct(ledger, tx, &original, req, s)
            .await?;
    let pointer = original.wrappers().origin_pointer(s)?;
    let pointer = s
        .parse_representation(pointer.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let (operation_ref, _operation_owner) =
        source::from_value_local(check::field(pointer.value(), "operation_ref", s)?, s)?;
    let (receipt_ref, _receipt_owner) =
        source::from_value_local(check::field(pointer.value(), "receipt_ref", s)?, s)?;
    let operation = s
        .canonical_representation(
            original.wrappers().operation(),
            RepresentationKind::OperationFrame,
        )
        .map_err(core_error)?;
    let receipt = s
        .canonical_representation(original.wrappers().receipt(), RepresentationKind::Record)
        .map_err(core_error)?;
    let pool = observed
        .local_input_bodies
        .as_mut()
        .ok_or(ServiceError::IntegrityFailure)?;
    // Reacquire original I and authoritative results under their own exact
    // producers, while retaining current D source data in the same pool.
    for r in check::field(original.wrappers().manifest(), "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if check::text(r, "kind", s)? != "workflow-operation" {
            pool.add_value(r, s)?;
        }
    }
    pool.add(&operation_ref, s)?;
    pool.add(&receipt_ref, s)?;
    pool.complete(tx, req.key(), observed.before, s).await?;
    let payload = req.payload(s)?;
    let index = observed
        .work_source_index
        .ok_or(ServiceError::IntegrityFailure)?;
    let received = observed
        .work_received_at
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    work_capture::with_capture(
        observed,
        req.key(),
        payload,
        index,
        received,
        s,
        |capture, current_inputs, raw, s| {
            let inputs = &current_inputs.rows;
            let alias = observed
                .alias_work
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            let original_work = core::EconomicRecordView {
                reference: &alias.work,
                canonical_body: observed
                    .local_input_bodies
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .body(&alias.work)?,
            };
            let (correspondences, _owners) = work_capture::source_correspondences(observed, s)?;
            let candidate = core::construct_local_work_alias_candidate(
                observed.original.publication.program(),
                capture,
                raw,
                reconstructed,
                (
                    original_work,
                    core::EconomicRecordView {
                        reference: &operation_ref,
                        canonical_body: operation.bytes(),
                    },
                    core::EconomicRecordView {
                        reference: &receipt_ref,
                        canonical_body: receipt.bytes(),
                    },
                ),
                (inputs, &correspondences),
                s,
            )
            .map_err(core_error)?;
            let financial = core::DraftStateData::default();
            let data = core::EconomicStateData {
                retained_assertion: None,
                financial: &financial,
                capacity_heads: &[],
                supplier: None,
                app: None,
                current_party_authorizations: &[],
                observation: None,
                audit: None,
                initial_source_correspondences: &correspondences,
            };
            let evidence = core::produce_local_financial_request_evidence(
                observed.original.publication.program(),
                req.raw(),
                &candidate,
                inputs,
                &data,
                s,
            )
            .map_err(core_error)?;
            Ok((candidate, evidence))
        },
    )
}
