//! Exact held stage projection selected from the immutable submitted instance.
use super::*;
use crate::store::sqlite::workflow::financial::HeadKey;
use ledgerlab_core::policy::workflow::transition_constructor::select_stage_work_head_locator;

/// A declared Stage is an original promise. Authenticate its exact Binding
/// before asking for an activation, so today's replacement cannot reselect it.
pub(super) async fn retain_original_work_activation(
    observed: &mut ObservedAppBinding,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    if !observed
        .original
        .publication
        .program()
        .uses_stage_lifecycle_recipe()
    {
        return Ok(false);
    }
    let Some(reference) = payload.get("stage_instance_ref") else {
        return Ok(false);
    };
    let instance = source::retained_local_record(
        tx,
        key,
        reference,
        (
            "workflow-stage-instance",
            "ledger-workflow-stage-instance/1",
        ),
        observed.before,
        s,
    )
    .await?;
    let expected = wire::reference(observed.original.acceptance.binding.reference(), s)?;
    let supplied = check::field(instance.body(), "binding_ref", s)?;
    let supplied = s
        .canonical_representation(supplied, RepresentationKind::Record)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(expected.bytes().len() + supplied.bytes().len())
        .map_err(core_error)?;
    require(
        expected.bytes() == supplied.bytes(),
        "ORIGINAL_TERMS_MISMATCH",
    )?;
    for field in ["customer", "chain"] {
        check::same_text(
            check::text(instance.body(), field, s)?,
            check::text(payload, field, s)?,
            s,
        )?;
    }
    let binding = s
        .parse_representation(
            observed.original.acceptance.binding.bytes(),
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    observed.activation = Some(
        observed
            .original
            .original_initial_activation(tx, key, binding.value(), &observed.at, observed.before, s)
            .await?,
    );
    Ok(true)
}

pub(super) async fn retain_work_head(
    observed: &mut ObservedAppBinding,
    state: &mut work_heads::OwnedWorkState,
    pool: &mut super::super::super::local_inputs::LocalInputs,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    if !observed
        .original
        .publication
        .program()
        .uses_stage_lifecycle_recipe()
    {
        return Ok(());
    }
    pool.select_stage_recipe(observed.original.publication.program());
    let raw = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let (core_key, _key_owner) = super::super::super::local_inputs::core_key_with_owner(key, s)?;
    let selected = select_stage_work_head_locator(
        observed.original.publication.program(),
        &core_key,
        raw.bytes(),
        &pool.views(),
        s,
    )
    .map_err(core_error)?;
    observed.stage_work_locator = selected;
    let invocations = ledgerlab_core::policy::workflow::transition_constructor::select_work_invocation_head_locators(
        observed.original.publication.program(), &core_key, raw.bytes(), &pool.views(), s
    ).map_err(core_error)?;
    observed.work_invocation_locators = Some(invocations);
    let invocations = observed
        .work_invocation_locators
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    work_heads::prepare_invocation_heads(state, invocations.heads().len(), s)?;
    for invocation in invocations.heads() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        pool.add(invocation.original_ref(), s)?;
        if let Some(head) = tx
            .workflow_financial_head(
                &key.logical_store_id,
                &key.scope,
                HeadKey::Invocation(invocation.invocation_id()),
                s,
            )
            .await
            .map_err(prefix::read_error)?
        {
            let k = wire::string(head_text(head.kind.bytes(), s)?, s)?;
            let i = wire::string(head_text(head.id.bytes(), s)?, s)?;
            let d = wire::string(head_text(head.digest.bytes(), s)?, s)?;
            let encoded = wire::object(
                &[
                    ("digest", d.bytes()),
                    ("id", i.bytes()),
                    ("kind", k.bytes()),
                ],
                262144,
                s,
            )?;
            let parsed = s
                .parse_representation(encoded.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let (reference, _owner) = source::from_value_local(parsed.value(), s)?;
            check::same_text(&reference.kind, "workflow-invocation", s)?;
            work_heads::retain_invocation_head(
                state,
                invocation.invocation_id(),
                &head,
                &reference,
                s,
            )?;
            pool.add(&reference, s)?;
        }
    }
    let Some(locator) = observed.stage_work_locator.as_ref() else {
        return pool.complete(tx, key, observed.before, s).await;
    };
    pool.add(locator.instance_ref(), s)?;
    if let Some(head) = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Stage {
                agreement: locator.agreement(),
                chain: locator.chain(),
                stage: locator.stage(),
            },
            s,
        )
        .await
        .map_err(prefix::read_error)?
    {
        let k = wire::string(head_text(head.kind.bytes(), s)?, s)?;
        let i = wire::string(head_text(head.id.bytes(), s)?, s)?;
        let d = wire::string(head_text(head.digest.bytes(), s)?, s)?;
        let encoded = wire::object(
            &[
                ("digest", d.bytes()),
                ("id", i.bytes()),
                ("kind", k.bytes()),
            ],
            262144,
            s,
        )?;
        let parsed = s
            .parse_representation(encoded.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (reference, _owner) = source::from_value_local(parsed.value(), s)?;
        check::same_text(&reference.kind, "workflow-stage-state", s)?;
        work_heads::retain_stage_head(state, locator, &head, &reference, s)?;
        pool.add(&reference, s)?;
    }
    pool.complete(tx, key, observed.before, s).await
}

fn head_text<'a>(bytes: &'a [u8], s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    std::str::from_utf8(bytes).map_err(|_| ServiceError::IntegrityFailure)
}

pub(super) async fn retain_invocation_operation(
    observed: &mut ObservedAppBinding,
    state: &mut work_heads::OwnedWorkState,
    pool: &mut super::super::super::local_inputs::LocalInputs,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let program = observed.original.publication.program();
    if !program.uses_stage_lifecycle_recipe() {
        return Ok(());
    }
    let raw = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let (ck, _key_owner) = super::super::super::local_inputs::core_key_with_owner(key, s)?;
    observed.invocation_operation_locators=ledgerlab_core::policy::workflow::transition_constructor::select_invocation_operation_locators(program,&ck,raw.bytes(),&pool.views(),s).map_err(core_error)?;
    let Some(locator) = observed.invocation_operation_locators.as_ref() else {
        return Ok(());
    };
    work_heads::prepare_invocation_heads(state, 1, s)?;
    if let Some(reference) = locator.named_invocation_ref() {
        pool.add(reference, s)?;
    }
    if let Some(head) = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Invocation(locator.invocation_id()),
            s,
        )
        .await
        .map_err(prefix::read_error)?
    {
        let kind = wire::string(head_text(head.kind.bytes(), s)?, s)?;
        let id = wire::string(head_text(head.id.bytes(), s)?, s)?;
        let digest = wire::string(head_text(head.digest.bytes(), s)?, s)?;
        let value = wire::object(
            &[
                ("kind", kind.bytes()),
                ("id", id.bytes()),
                ("digest", digest.bytes()),
            ],
            262144,
            s,
        )?;
        let value = s
            .parse_representation(value.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (reference, _owner) = source::from_value_local(value.value(), s)?;
        check::same_text(&reference.kind, "workflow-invocation", s)?;
        work_heads::retain_invocation_head(state, locator.invocation_id(), &head, &reference, s)?;
        pool.add(&reference, s)?;
    }
    if let Some(claim_id) = locator.work_claim_id() {
        if let Some(row) = tx
            .workflow_local_record_identity(
                &key.logical_store_id,
                &key.scope,
                "workflow-work-claim",
                claim_id,
                "ledger-workflow-work-claim/1",
                s,
            )
            .await
            .map_err(prefix::read_error)?
        {
            let kind = wire::string(head_text(row.kind.bytes(), s)?, s)?;
            let id = wire::string(head_text(row.id.bytes(), s)?, s)?;
            let digest = wire::string(head_text(row.digest.bytes(), s)?, s)?;
            let value = wire::object(
                &[
                    ("kind", kind.bytes()),
                    ("id", id.bytes()),
                    ("digest", digest.bytes()),
                ],
                262144,
                s,
            )?;
            let value = s
                .parse_representation(value.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let (reference, _owner) = source::from_value_local(value.value(), s)?;
            pool.add(&reference, s)?;
            state.financial.work_claims.push(reference);
        }
    }
    pool.complete_lifecycle(program, tx, key, observed.before, s)
        .await
}
