//! Actual captured root source and completed initial activation, borrowed into
//! the SAME pure work locator. This is source-slot DATA, not complete I.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::{
        EconomicCapture, EconomicRecordView, OperationKey as CoreKey, SourceAuthorityCapture,
        select_work_input_seeds,
    },
};
use std::mem;
fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(ValidationVisit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn same_ref(a: &ExactRef, b: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
    Ok(equal(&a.kind, &b.kind, s)? && equal(&a.id, &b.id, s)? && equal(&a.digest, &b.digest, s)?)
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn storage(n: usize, stride: usize, s: &mut OriginalAuditSession) -> Result<ExternalBufferLease> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&n.into(), &stride.into()])
        .map_err(core_error)?;
    s.reserve_external_payload(n.checked_mul(stride).ok_or(ServiceError::Unavailable)?)
        .map_err(core_error)
}
fn copy(v: &str, s: &mut OriginalAuditSession) -> Result<String> {
    s.semantic_work().scan_bytes(v.len()).map_err(core_error)?;
    s.semantic_work().write_bytes(v.len()).map_err(core_error)?;
    Ok(v.to_owned())
}
pub(super) fn select(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    source_index: usize,
    received_at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<(AuditedBytes, AuditedBytes)> {
    let _controls = s
        .reserve_external_payload(
            mem::size_of::<Vec<ExactRef>>()
                + mem::size_of::<ExactRef>()
                + mem::size_of::<Vec<SourceAuthorityCapture<'_>>>()
                + mem::size_of::<SourceAuthorityCapture<'_>>(),
        )
        .map_err(core_error)?;
    let activation = observed
        .activation
        .as_ref()
        .ok_or(ServiceError::Unavailable)?;
    let binding = observed._parsed_binding.value();
    let actor = observed.setup.operator.as_str();
    s.semantic_work()
        .visit(ValidationVisit::RefLookup)
        .map_err(core_error)?;
    let source = observed
        .sources
        .get(source_index)
        .ok_or(ServiceError::IntegrityFailure)?;
    let grant = source
        .current_grant_body()
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let namespace = check::text(grant, "canonical_economic_namespace", s)?;
    // All actual Ref payloads and Vec controls are leased before their copies.
    let mut bytes = 0usize;
    for other in &observed.sources {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        if same_ref(other.enrollment_ref(), source.enrollment_ref(), s)? {
            for text in [
                &other.original_grant_ref().kind,
                &other.original_grant_ref().id,
                &other.original_grant_ref().digest,
            ] {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                bytes = add(bytes, text.len(), s)?;
            }
        }
    }
    let _ref_payload = s.reserve_external_payload(bytes).map_err(core_error)?;
    let _ref_controls = storage(observed.sources.len(), mem::size_of::<ExactRef>(), s)?;
    let mut originals = Vec::new();
    originals
        .try_reserve_exact(observed.sources.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for other in &observed.sources {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        if !same_ref(other.enrollment_ref(), source.enrollment_ref(), s)? {
            continue;
        }
        let r = other.original_grant_ref();
        let r = ExactRef {
            kind: copy(&r.kind, s)?,
            id: copy(&r.id, s)?,
            digest: copy(&r.digest, s)?,
        };
        s.semantic_work()
            .write_bytes(mem::size_of::<ExactRef>())
            .map_err(core_error)?;
        originals.push(r);
    }
    let mut bytes = mem::size_of::<CoreKey>();
    for text in [
        &key.logical_store_id,
        &key.scope[0],
        &key.scope[1],
        &key.source,
        &key.operation_id,
    ] {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        bytes = add(bytes, text.len(), s)?;
    }
    let _key_owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    let core_key = CoreKey {
        logical_store_id: copy(&key.logical_store_id, s)?,
        scope: [copy(&key.scope[0], s)?, copy(&key.scope[1], s)?],
        source: copy(&key.source, s)?,
        operation_id: copy(&key.operation_id, s)?,
    };
    s.semantic_work()
        .write_bytes(mem::size_of::<CoreKey>())
        .map_err(core_error)?;
    let slots = check::field(binding, "source_slots", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&slots.len().into(), &32usize.into()])
        .map_err(core_error)?;
    require(slots.len() <= 32, "INVALID_SHAPE")?;
    let _frame_owner = storage(slots.len(), mem::size_of::<SourceAuthorityCapture<'_>>(), s)?;
    let _capture_owner = s
        .reserve_external_payload(mem::size_of::<EconomicCapture<'_>>())
        .map_err(core_error)?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(slots.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for slot in slots {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let name = check::text(slot, "name", s)?;
        let nominees = check::field(slot, "sources", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut nominated = false;
        for nominee in nominees {
            s.semantic_work()
                .visit(ValidationVisit::Element)
                .map_err(core_error)?;
            if equal(
                nominee.as_str().ok_or(ServiceError::IntegrityFailure)?,
                &key.source,
                s,
            )? {
                nominated = true
            }
        }
        if !nominated {
            continue;
        }
        s.semantic_work()
            .write_bytes(mem::size_of::<SourceAuthorityCapture<'_>>())
            .map_err(core_error)?;
        frames.push(SourceAuthorityCapture {
            source_slot: name,
            source: &key.source,
            principal: actor,
            canonical_economic_namespace: namespace,
            enrollment_ref: source.enrollment_ref(),
            current_state_ref: source.state_ref(),
            current_grant_ref: source
                .current_grant_ref()
                .ok_or(ServiceError::IntegrityFailure)?,
            original_grant_refs: &originals,
            binding_ref: observed.original.acceptance.binding.reference(),
            accepted_agreement_ref: observed.original.acceptance.accepted_reference(),
            activation_ref: activation.activation.reference(),
        });
    }
    let capture = EconomicCapture {
        key: &core_key,
        actor,
        canonical_economic_namespace: namespace,
        installation_mode: "real",
        received_at: received_at.as_str(),
        accepted_at: observed.at.as_str(),
        sources: &frames,
    };
    s.semantic_work()
        .write_bytes(mem::size_of::<EconomicCapture<'_>>())
        .map_err(core_error)?;
    let raw = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let pool = input_pool(observed, s)?;
    let located = select_work_input_seeds(
        observed.original.publication.program(),
        &capture,
        raw.bytes(),
        &pool.rows,
        s,
    )
    .map_err(core_error)?;
    let slot = wire::string(located.source().source_slot(), s)?;
    let refs = wire::RefSet::new(
        located.direct_refs().iter(),
        located.direct_refs().len(),
        true,
        s,
    )?
    .encode(s)?;
    Ok((slot, refs))
}

pub(super) fn source_index(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<usize> {
    let actor = observed.setup.operator.as_str();
    let mut representative: Option<usize> = None;
    for (index, source) in observed.sources.iter().enumerate() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let Some(grant) = source.current_grant_body() else {
            continue;
        };
        if !equal(check::text(grant, "source", s)?, &key.source, s)?
            || !equal(check::text(grant, "principal_id", s)?, actor, s)?
        {
            continue;
        }
        if let Some(previous) = representative {
            s.semantic_work()
                .visit(ValidationVisit::RefLookup)
                .map_err(core_error)?;
            let old = observed
                .sources
                .get(previous)
                .ok_or(ServiceError::IntegrityFailure)?;
            require(
                same_ref(old.enrollment_ref(), source.enrollment_ref(), s)?,
                "UNSUPPORTED_COMPOSITION",
            )?;
            require(
                same_ref(old.state_ref(), source.state_ref(), s)?,
                "STALE_REVISION",
            )?;
            require(
                same_ref(
                    old.current_grant_ref()
                        .ok_or(ServiceError::IntegrityFailure)?,
                    source
                        .current_grant_ref()
                        .ok_or(ServiceError::IntegrityFailure)?,
                    s,
                )?,
                "STALE_REVISION",
            )?;
        } else {
            representative = Some(index)
        }
    }

    representative.ok_or_else(|| reject("AUTHORITY_REQUIRED"))
}
struct InputPool<'a> {
    rows: Vec<EconomicRecordView<'a>>,
    _storage: ExternalBufferLease,
    _controls: ExternalBufferLease,
}
fn input_pool<'a>(
    o: &'a ObservedAppBinding,
    s: &mut OriginalAuditSession,
) -> Result<InputPool<'a>> {
    let activation = o.activation.as_ref().ok_or(ServiceError::Unavailable)?;
    let birth = o
        .work_source_birth
        .as_ref()
        .ok_or(ServiceError::Unavailable)?;
    let controls = s
        .reserve_external_payload(
            mem::size_of::<InputPool<'_>>()
                + 2 * mem::size_of::<[EconomicRecordView<'_>; 4]>()
                + mem::size_of::<EconomicRecordView<'_>>(),
        )
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<[EconomicRecordView<'_>; 4]>())
        .map_err(core_error)?;
    let fixed = [
        EconomicRecordView {
            reference: activation.activation.reference(),
            canonical_body: activation.activation.bytes(),
        },
        EconomicRecordView {
            reference: activation.head.reference(),
            canonical_body: activation.head.bytes(),
        },
        EconomicRecordView {
            reference: birth.state.reference(),
            canonical_body: birth.state.canonical_body(),
        },
        EconomicRecordView {
            reference: birth.grant.reference(),
            canonical_body: birth.grant.canonical_body(),
        },
    ];
    let values = || {
        o.original
            .acceptance
            .work_locator_records()
            .chain(o.sources.iter().flat_map(|source| source.records()))
            .chain(fixed)
    };
    s.semantic_work()
        .scan_bytes(2 * mem::size_of_val(&fixed))
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(2 * mem::size_of_val(&fixed))
        .map_err(core_error)?;
    let mut count = 0usize;
    for _ in values() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        count = add(count, 1, s)?;
    }
    let owner = storage(count, mem::size_of::<EconomicRecordView<'_>>(), s)?;
    let mut rows: Vec<EconomicRecordView<'a>> = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    for value in values() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let mut duplicate = false;
        for previous in &rows {
            s.semantic_work()
                .visit(ValidationVisit::RefLookup)
                .map_err(core_error)?;
            if equal(&previous.reference.kind, &value.reference.kind, s)?
                && equal(&previous.reference.id, &value.reference.id, s)?
            {
                require(
                    equal(&previous.reference.digest, &value.reference.digest, s)?,
                    "INTEGRITY_FAILURE",
                )?;
                s.semantic_work()
                    .visit(ValidationVisit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(previous.canonical_body.len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(value.canonical_body.len())
                    .map_err(core_error)?;
                require(
                    previous.canonical_body == value.canonical_body,
                    "INTEGRITY_FAILURE",
                )?;
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            s.semantic_work()
                .write_bytes(mem::size_of::<EconomicRecordView<'_>>())
                .map_err(core_error)?;
            rows.push(value);
        }
    }
    Ok(InputPool {
        rows,
        _storage: owner,
        _controls: controls,
    })
}
