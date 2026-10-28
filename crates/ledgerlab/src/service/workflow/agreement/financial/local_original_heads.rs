//! Chronological projection DATA for exact local original replay. It never
//! reads today's mutable heads or promotes result amounts to pricing inputs.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::EconomicRecordView,
    validation_work::{OriginalAuditSession, RepresentationKind},
};
use prefix::inventory::{Inventory, StateKey};

pub(super) struct OriginalHeads {
    inventory: Option<Inventory>,
}
pub(super) fn state_key(
    domain: &'static str,
    values: &[&Value],
    s: &mut OriginalAuditSession,
) -> Result<StateKey> {
    let _controls = s
        .reserve_external_payload(values.len() * std::mem::size_of::<AuditedBytes>())
        .map_err(core_error)?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(values.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for value in values {
        encoded.push(
            s.canonical_representation(value, RepresentationKind::Record)
                .map_err(core_error)?,
        );
    }
    let _parts = s
        .reserve_external_payload(values.len() * std::mem::size_of::<&[u8]>())
        .map_err(core_error)?;
    let parts: Vec<_> = encoded.iter().map(|b| b.bytes()).collect();
    StateKey::new(domain, &parts, s)
}
impl OriginalHeads {
    /// All actual preceding producer results are folded in journal order. A
    /// semantic alias deliberately produces no head transition. The selected
    /// original's introductions never enter this projection.
    pub(super) async fn before(
        tx: &mut SqliteTx,
        key: &OperationKey,
        before: i64,
        parent: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let mut out = Self { inventory: None };
        let through = before
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut after = 0;
        loop {
            let mut s = OriginalAuditSession::for_resource_profile_v2(parent.arena())
                .map_err(core_error)?;
            let Some(origin) = prefix::next(tx, key, after, through, &mut s).await? else {
                break;
            };
            let origin = origin.checked_wrappers(tx, &mut s).await?;
            let sequence = origin.wrappers().sequence();
            require(sequence > after && sequence < before, "INTEGRITY_FAILURE")?;
            match out.inventory.as_mut() {
                Some(inventory) => inventory.next_origin(&origin, &mut s)?,
                None => {
                    out.inventory = Some(Inventory::new(
                        tx.workflow_original_scratch(&mut s)
                            .map_err(prefix::read_error)?,
                        &origin,
                        &mut s,
                    )?)
                }
            }
            let result = request_wrapper::text(origin.wrappers().receipt(), "result_kind", &mut s)?;
            if result != "semantic_alias" {
                if origin.wrappers().class() == wrappers::WireClass::Financial
                    && request_wrapper::text(origin.wrappers().operation(), "kind", &mut s)?
                        != "authority.change"
                {
                    require(
                        local_financial_original::verify(tx, &origin, None, &mut s).await?,
                        "UNSUPPORTED_SCHEMA_PROFILE",
                    )?;
                }
                for reference in
                    request_wrapper::field(origin.wrappers().receipt(), "result_refs", &mut s)?
                        .as_array()
                        .ok_or(ServiceError::IntegrityFailure)?
                {
                    let kind = request_wrapper::text(reference, "kind", &mut s)?;
                    if !matches!(
                        kind,
                        "workflow-decision"
                            | "workflow-chain-state"
                            | "workflow-correction-capacity"
                            | "workflow-stage-state"
                            | "workflow-invocation"
                            | "workflow-family-state"
                            | "workflow-work-claim"
                            | "workflow-authority"
                    ) {
                        continue;
                    }
                    origin.require_member(reference, &mut s)?;
                    let (r, _owner) = current::source::from_value_local(reference, &mut s)?;
                    // SAME native-schema/body/full-Ref and producer certificate
                    // kernel used for ordinary retained financial operands.
                    let mut pool = local_inputs::LocalInputs::new(&mut s)?;
                    let evidence = request_wrapper::field(
                        origin.wrappers().operation(),
                        "request_evidence",
                        &mut s,
                    )?;
                    let resolution =
                        request_wrapper::field(evidence, "descriptor_resolution", &mut s)?;
                    pool.select_saved_registry(
                        request_wrapper::text(resolution, "registry_digest", &mut s)?,
                        &mut s,
                    )?;
                    pool.add(&r, &mut s)?;
                    pool.complete(
                        tx,
                        origin.key(),
                        sequence
                            .checked_add(1)
                            .ok_or(ServiceError::IntegrityFailure)?,
                        &mut s,
                    )
                    .await?;
                    let body = s
                        .parse_representation(pool.body(&r)?, RepresentationKind::Record)
                        .map_err(core_error)?;
                    let v = body.value();
                    let schema = request_wrapper::text(v, "schema", &mut s)?;
                    let selected = match kind {
                        "workflow-decision" => Some(state_key(
                            "local-original-decision",
                            &[request_wrapper::field(v, "decision_id", &mut s)?],
                            &mut s,
                        )?),
                        "workflow-chain-state" => Some(state_key(
                            "local-original-chain",
                            &[
                                request_wrapper::field(v, "customer", &mut s)?,
                                request_wrapper::field(v, "chain", &mut s)?,
                            ],
                            &mut s,
                        )?),
                        "workflow-correction-capacity" => {
                            let root = if schema == "ledger-correction-capacity/2" {
                                let (reference, _lease) = current::source::from_value_local(
                                    request_wrapper::field(v, "root_ref", &mut s)?,
                                    &mut s,
                                )?;
                                request_wrapper::same_text(
                                    &reference.kind,
                                    "workflow-decision",
                                    &mut s,
                                )?;
                                Some(
                                    s.parse_representation(
                                        pool.body(&reference)?,
                                        RepresentationKind::Record,
                                    )
                                    .map_err(core_error)?,
                                )
                            } else {
                                None
                            };
                            let id = if let Some(root) = &root {
                                request_wrapper::field(root.value(), "decision_id", &mut s)?
                            } else {
                                request_wrapper::field(v, "root_decision_id", &mut s)?
                            };
                            Some(state_key("local-original-capacity", &[id], &mut s)?)
                        }
                        "workflow-invocation" => Some(state_key(
                            "local-original-invocation",
                            &[request_wrapper::field(v, "invocation_id", &mut s)?],
                            &mut s,
                        )?),
                        "workflow-family-state" => Some(state_key(
                            "local-original-family",
                            &[
                                request_wrapper::field(v, "target_ref", &mut s)?,
                                request_wrapper::field(v, "family_slot", &mut s)?,
                            ],
                            &mut s,
                        )?),
                        "workflow-stage-state" => Some(state_key(
                            "local-original-stage",
                            &[request_wrapper::field(v, "stage_instance_ref", &mut s)?],
                            &mut s,
                        )?),
                        "workflow-work-claim" => {
                            let id = wire::string(&r.id, &mut s)?;
                            let id = s
                                .parse_representation(id.bytes(), RepresentationKind::Record)
                                .map_err(core_error)?;
                            Some(state_key(
                                "local-original-work-claim",
                                &[id.value()],
                                &mut s,
                            )?)
                        }
                        "workflow-authority"
                            if schema == "ledger-workflow-party-authorization/1" =>
                        {
                            let a = request_wrapper::field(v, "authorization", &mut s)?;
                            Some(state_key(
                                "local-original-party",
                                &[request_wrapper::field(a, "authorization_id", &mut s)?],
                                &mut s,
                            )?)
                        }
                        _ => None,
                    };
                    if let Some(selected) = selected {
                        let inventory = out
                            .inventory
                            .as_mut()
                            .ok_or(ServiceError::IntegrityFailure)?;
                        inventory.introduce(&r, pool.body(&r)?, sequence, &mut s)?;
                        inventory.set_head(&selected, &r, &mut s)?;
                    }
                }
            }
            s.check_cancelled().map_err(core_error)?;
            after = sequence;
        }
        parent.check_cancelled().map_err(core_error)?;
        Ok(out)
    }
    /// Select the prefix head, then require its exact body in original I. No
    /// first/highest revision choice in I and no comparison-only N operand.
    pub(super) fn input<'a>(
        &mut self,
        domain: &'static str,
        values: &[&Value],
        inputs: &'a [EconomicRecordView<'a>],
        s: &mut OriginalAuditSession,
    ) -> Result<Option<EconomicRecordView<'a>>> {
        let Some(inventory) = self.inventory.as_mut() else {
            return Ok(None);
        };
        let key = state_key(domain, values, s)?;
        let Some(body) = inventory.head_body(&key, s)? else {
            return Ok(None);
        };
        let mut found = None;
        for input in inputs {
            s.semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(body.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(input.canonical_body.len())
                .map_err(core_error)?;
            if body.bytes() == input.canonical_body {
                require(found.replace(*input).is_none(), "INTEGRITY_FAILURE")?;
            }
        }
        found.map(Some).ok_or(ServiceError::IntegrityFailure)
    }
}
