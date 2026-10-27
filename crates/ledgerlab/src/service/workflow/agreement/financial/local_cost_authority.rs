//! Optional pre-policy supplier account ownership, using the existing held
//! administrator and the same sealed financial append coordinator.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    policy::workflow::{
        selected_program::SelectedCompilerSupport,
        transition_constructor::{self as core, DraftContextData, EconomicRecordView, ExactRef},
    },
    validation_work::{OriginalAuditSession, RepresentationKind},
};

fn decode(v: &[u8]) -> Result<&str> {
    std::str::from_utf8(v).map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) fn context(
    key: &OperationKey,
    actor: &str,
    namespace: &str,
    mode: &str,
    received: &str,
    accepted: &str,
    s: &mut OriginalAuditSession,
) -> Result<(DraftContextData, ExternalBufferLease, ExternalBufferLease)> {
    let (key, key_owner) = local_inputs::core_key_with_owner(key, s)?;
    let texts = [actor, namespace, mode, received, accepted];
    let bytes = texts
        .iter()
        .try_fold(std::mem::size_of::<DraftContextData>(), |n, v| {
            n.checked_add(v.len()).ok_or(ServiceError::Unavailable)
        })?;
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    Ok((
        DraftContextData {
            key,
            actor: actor.to_owned(),
            canonical_economic_namespace: namespace.to_owned(),
            installation_mode: mode.to_owned(),
            received_at: received.to_owned(),
            accepted_at: accepted.to_owned(),
            authorities: Vec::new(),
            source_observation_inputs: Vec::new(),
        },
        owner,
        key_owner,
    ))
}
fn selected(req: &ingress::IncomingRequest, s: &mut OriginalAuditSession) -> Result<bool> {
    Ok(req.kind() == "authority.change"
        && request_wrapper::text(req.payload(s)?, "schema", s)? == "ledger-cost-authority-enroll/1")
}
fn support_for_evidence(
    payload: &Value,
    pool: &local_inputs::LocalInputs,
    s: &mut OriginalAuditSession,
) -> Result<SelectedCompilerSupport> {
    let refs = request_wrapper::field(payload, "evidence_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require((1..=16).contains(&refs.len()), "INVALID_SHAPE")?;
    let mut platform = None;
    for reference in refs {
        let mut selected = None;
        for row in pool.iter_views() {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::RefLookup)
                .map_err(core_error)?;
            if acceptance::same_recipe_text(
                request_wrapper::text(reference, "kind", s)?,
                &row.reference.kind,
                s,
            )? && acceptance::same_recipe_text(
                request_wrapper::text(reference, "id", s)?,
                &row.reference.id,
                s,
            )? && acceptance::same_recipe_text(
                request_wrapper::text(reference, "digest", s)?,
                &row.reference.digest,
                s,
            )? {
                let body = s
                    .parse_representation(row.canonical_body, RepresentationKind::Record)
                    .map_err(core_error)?;
                let schema = request_wrapper::text(body.value(), "schema", s)?;
                selected = Some(
                    if acceptance::same_recipe_text(
                        schema,
                        "ledger-workflow-retained-evidence/2",
                        s,
                    )? {
                        true
                    } else {
                        request_wrapper::same_text(
                            schema,
                            "ledger-workflow-retained-evidence/1",
                            s,
                        )?;
                        false
                    },
                );
                break;
            }
        }
        let selected = selected.ok_or(ServiceError::IntegrityFailure)?;
        require(
            platform.is_none_or(|old| old == selected),
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        platform = Some(selected);
    }
    if platform == Some(true) {
        SelectedCompilerSupport::local_stage_lifecycle_v2_execution_with_session(s)
    } else {
        SelectedCompilerSupport::local_stage_lifecycle_execution_with_session(s)
    }
    .map_err(core_error)
}

pub(super) async fn prepare_append(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &ingress::IncomingRequest,
    management: &ManagementObservation,
    s: &mut OriginalAuditSession,
) -> Result<WorkflowReply> {
    require(selected(req, s)?, "UNSUPPORTED_SCHEMA_PROFILE")?;
    let clock_owner = s
        .reserve_external_payload(3 * 2 * 27 + 2 * std::mem::size_of::<Timestamp>())
        .map_err(core_error)?;
    let at = crate::local::now_with_work(s.semantic_work()).map_err(|e| match e {
        crate::local::LocalClockError::Unavailable => ServiceError::Unavailable,
        crate::local::LocalClockError::Resource(e) => core_error(e),
    })?;
    let before = tx
        .workflow_management_sequence(s)
        .await
        .map_err(prefix::read_error)?;
    let setup = management.held().setup();
    let (c, _context_owner, _key_owner) = context(
        req.key(),
        &setup.operator,
        &setup.source,
        "real_local",
        at.as_str(),
        at.as_str(),
        s,
    )?;
    let scope = request_wrapper::field(request_wrapper::field(req.value(), "key", s)?, "scope", s)?;
    let root = current::party::produce::root(setup, scope, &at, s)?;
    let payload = req.payload(s)?;
    let accounts = request_wrapper::field(payload, "provider_account_identities", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require((1..=16).contains(&accounts.len()), "INVALID_SHAPE")?;
    // Absence is queried under the same held write transaction. A hit is a
    // permanent full account tuple, irrespective of its authority label/scope.
    let occupied_owner = s
        .reserve_external_payload(
            accounts.len()
                * (std::mem::size_of::<ExactRef>() + std::mem::size_of::<ExternalBufferLease>()),
        )
        .map_err(core_error)?;
    let mut occupied = Vec::with_capacity(accounts.len());
    let mut hit_owners = Vec::with_capacity(accounts.len());
    for account in accounts {
        if let Some(hit) = tx
            .workflow_supplier_account_owner(
                request_wrapper::text(account, "provider_namespace", s)?,
                request_wrapper::text(account, "provider_account_id", s)?,
                request_wrapper::text(account, "environment", s)?,
                s,
            )
            .await
            .map_err(prefix::read_error)?
        {
            let kind = wire::string(decode(hit.authority.kind.bytes())?, s)?;
            let id = wire::string(decode(hit.authority.id.bytes())?, s)?;
            let digest = wire::string(decode(hit.authority.digest.bytes())?, s)?;
            let bytes = wire::object(
                &[
                    ("kind", kind.bytes()),
                    ("id", id.bytes()),
                    ("digest", digest.bytes()),
                ],
                262144,
                s,
            )?;
            let value = s
                .parse_representation(bytes.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let (r, owner) = current::source::from_value_local(value.value(), s)?;
            occupied.push(r);
            hit_owners.push(owner);
        }
    }
    let mut pool = local_inputs::LocalInputs::new(s)?;
    for r in request_wrapper::field(payload, "evidence_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        pool.add_value(r, s)?;
    }
    pool.complete(tx, req.key(), before, s).await?;
    let support = support_for_evidence(payload, &pool, s)?;
    let (mut inputs, _views_owner) = pool.owned_views(1, s)?;
    inputs.push(EconomicRecordView {
        reference: root.reference(),
        canonical_body: root.bytes(),
    });
    let (candidate, evidence) = core::construct_local_cost_authority_enrollment(
        &support,
        &c,
        req.raw(),
        root.reference(),
        &inputs,
        &core::CostAuthorityEnrollmentState {
            occupied_accounts: &occupied,
        },
        s,
    )
    .map_err(core_error)?
    .into_parts();
    let journal = wire::frame_candidate(
        &candidate,
        req.key(),
        req.kind(),
        evidence.value(),
        &at,
        &at,
        before,
        s,
    )?;
    let reply = WorkflowReply::financial(&journal, &evidence, s)?;
    management.recheck(ledger, tx, req.key(), s).await?;
    tx.workflow_financial_append(&journal.append_view(), s)
        .await
        .map_err(prefix::read_error)?;
    let _owners = (clock_owner, occupied_owner, hit_owners);
    Ok(reply)
}

/// A known original proves its original account absence through the immutable
/// committed result/member and matching permanent account row. It never runs
/// fresh account admission against today's occupied projection.
pub(super) async fn verify_original(
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    Ok(reproduce_original(tx, original, incoming, s)
        .await?
        .is_some())
}

pub(super) async fn fold_original(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    inventory: &mut prefix::inventory::Inventory,
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let candidate = reproduce_original(tx, origin, None, s)
        .await?
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut roots = 0usize;
    for record in candidate.introduced() {
        if record.schema() == "ledger-workflow-local-authority-snapshot/1" {
            let body = s
                .parse_representation(record.canonical_body(), RepresentationKind::Authority)
                .map_err(core_error)?;
            prefix::source::custody(body.value(), setup, s)?;
            roots += 1;
        }
    }
    require(roots == 1, "INTEGRITY_FAILURE")?;
    for reference in candidate.consumed_refs() {
        let stored = tx
            .workflow_financial_document(
                reference,
                &origin.key().logical_store_id,
                &origin.key().scope,
                origin.wrappers().sequence(),
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        inventory.compare_record(reference, stored.body.bytes(), s)?;
    }
    for record in candidate.introduced() {
        inventory.introduce(
            record.reference(),
            record.canonical_body(),
            origin.wrappers().sequence(),
            s,
        )?;
    }
    Ok(())
}

async fn reproduce_original(
    tx: &mut SqliteTx,
    original: &prefix::LocatedWrappers,
    incoming: Option<&ingress::IncomingRequest>,
    s: &mut OriginalAuditSession,
) -> Result<Option<core::EconomicCandidate>> {
    let w = original.wrappers();
    if w.class() != wrappers::WireClass::Financial {
        return Ok(None);
    }
    let saved = ingress::IncomingRequest::parse(w.original_request().raw(), s)?;
    if !selected(&saved, s)? {
        return Ok(None);
    }
    let evidence = request_wrapper::field(w.operation(), "request_evidence", s)?;
    if let Some(req) = incoming {
        require(selected(req, s)?, "IDENTITY_CONFLICT")?;
        let normalized = core::normalize_local_cost_authority_enrollment_intent(req.raw(), s)
            .map_err(core_error)?;
        let expected = request_wrapper::field(
            request_wrapper::field(evidence, "normalized_request", s)?,
            "payload",
            s,
        )?;
        let expected = s
            .canonical_representation(expected, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(normalized.bytes().len() + expected.bytes().len())
            .map_err(core_error)?;
        require(normalized.bytes() == expected.bytes(), "IDENTITY_CONFLICT")?;
    }
    let resolution = request_wrapper::field(evidence, "descriptor_resolution", s)?;
    request_wrapper::same_text(
        request_wrapper::text(resolution, "registry_digest", s)?,
        wrappers::STAGE_LIFECYCLE_REGISTRY,
        s,
    )?;
    let snapshot = request_wrapper::field(w.operation(), "authority_snapshot", s)?;
    request_wrapper::same_text(
        request_wrapper::text(snapshot, "schema", s)?,
        "ledger-cost-enrollment-authority/1",
        s,
    )?;
    let root_body = request_wrapper::field(snapshot, "local_admin_snapshot", s)?;
    let basis = request_wrapper::field(root_body, "basis", s)?;
    let actor = request_wrapper::text(basis, "principal_id", s)?;
    let accepted = Timestamp::parse_with_work(
        request_wrapper::text(w.operation(), "accepted_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let received = Timestamp::parse_with_work(
        request_wrapper::text(w.operation(), "received_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    current::party::produce::validate_original_root(
        root_body,
        original.key(),
        actor,
        &accepted,
        s,
    )?;
    let root_bytes = s
        .canonical_representation(root_body, RepresentationKind::Record)
        .map_err(core_error)?;
    let root = wire::source_record(
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1",
        root_bytes,
        s,
    )?;
    let (c, _context_owner, _key_owner) = context(
        original.key(),
        actor,
        request_wrapper::text(basis, "canonical_economic_namespace", s)?,
        request_wrapper::text(snapshot, "installation_mode", s)?,
        received.as_str(),
        accepted.as_str(),
        s,
    )?;
    let before = w
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut pool = local_inputs::LocalInputs::new(s)?;
    for member in request_wrapper::field(w.manifest(), "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if request_wrapper::text(member, "kind", s)? != "workflow-operation" {
            pool.add_value(member, s)?;
        }
    }
    pool.complete(tx, original.key(), before, s).await?;
    require(
        pool.body(root.reference())? == root.bytes(),
        "INTEGRITY_FAILURE",
    )?;
    let (inputs, _views_owner) = pool.owned_views(0, s)?;
    let descriptors = request_wrapper::field(resolution, "descriptors", s)?
        .as_array().ok_or(ServiceError::IntegrityFailure)?;
    require(descriptors.len() == 1, "INTEGRITY_FAILURE")?;
    let support = publication_prepare::local_support(&descriptors[0], s)?;
    let state = core::CostAuthorityEnrollmentState {
        occupied_accounts: &[],
    };
    let (candidate, evidence) = core::construct_local_cost_authority_enrollment(
        &support,
        &c,
        saved.raw(),
        root.reference(),
        &inputs,
        &state,
        s,
    )
    .map_err(core_error)?
    .into_parts();
    for effect in candidate.account_enrollments() {
        let account = effect.account();
        let hit = tx
            .workflow_supplier_account_owner(
                account.provider_namespace(),
                account.provider_account_id(),
                account.environment(),
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        for (actual, want) in [
            (&hit.authority, effect.cost_authority_ref()),
            (&hit.activation, effect.owner_activation_ref()),
        ] {
            for (bytes, text) in [
                (actual.kind.bytes(), want.kind.as_str()),
                (actual.id.bytes(), want.id.as_str()),
                (actual.digest.bytes(), want.digest.as_str()),
            ] {
                s.semantic_work()
                    .scan_bytes(bytes.len() + text.len())
                    .map_err(core_error)?;
                require(bytes == text.as_bytes(), "INTEGRITY_FAILURE")?;
            }
        }
    }
    let journal = wire::frame_candidate(
        &candidate,
        original.key(),
        saved.kind(),
        evidence.value(),
        &received,
        &accepted,
        w.sequence(),
        s,
    )?;
    journal.compare_original(w, s)?;
    Ok(Some(candidate))
}
