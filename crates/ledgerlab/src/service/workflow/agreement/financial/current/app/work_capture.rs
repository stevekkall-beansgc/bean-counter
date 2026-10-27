//! Actual captured root source and completed initial activation, borrowed into
//! the SAME pure work locator. This is source-slot DATA, not complete I.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::{
        select_work_head_locators, select_work_input_seeds, EconomicCapture, EconomicRecordView,
        OperationKey as CoreKey, SourceAuthorityCapture, WorkHeadLocators,
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
pub(super) fn same_reference(
    a: &ExactRef,
    b: &ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
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
struct SecondaryOriginals {
    source_index: usize,
    references: Vec<ExactRef>,
    _payload: ExternalBufferLease,
    _control: ExternalBufferLease,
}
fn secondary_originals(
    observed: &ObservedAppBinding,
    primary: usize,
    s: &mut OriginalAuditSession,
) -> Result<(Vec<SecondaryOriginals>, ExternalBufferLease)> {
    let control = storage(
        observed.sources.len(),
        mem::size_of::<SecondaryOriginals>(),
        s,
    )?;
    let mut groups: Vec<SecondaryOriginals> = Vec::new();
    groups
        .try_reserve_exact(observed.sources.len())
        .map_err(|_| ServiceError::Unavailable)?;
    let primary = observed
        .sources
        .get(primary)
        .ok_or(ServiceError::IntegrityFailure)?;
    for (index, source) in observed.sources.iter().enumerate() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        if source.current_grant_body().is_none()
            || same_reference(source.enrollment_ref(), primary.enrollment_ref(), s)?
        {
            continue;
        }
        let mut duplicate = false;
        for group in &groups {
            if same_reference(
                source.enrollment_ref(),
                observed.sources[group.source_index].enrollment_ref(),
                s,
            )? {
                duplicate = true;
            }
        }
        if duplicate {
            continue;
        }
        let mut bytes = 0usize;
        let mut count = 0usize;
        for original in &observed.sources {
            if same_reference(source.enrollment_ref(), original.enrollment_ref(), s)? {
                count = add(count, 1, s)?;
                let r = original.original_grant_ref();
                for text in [&r.kind, &r.id, &r.digest] {
                    bytes = add(bytes, text.len(), s)?;
                }
            }
        }
        let payload = s.reserve_external_payload(bytes).map_err(core_error)?;
        let controls = storage(count, mem::size_of::<ExactRef>(), s)?;
        let mut references = Vec::new();
        references
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        for original in &observed.sources {
            if same_reference(source.enrollment_ref(), original.enrollment_ref(), s)? {
                let r = original.original_grant_ref();
                s.semantic_work()
                    .write_bytes(mem::size_of::<ExactRef>())
                    .map_err(core_error)?;
                references.push(ExactRef {
                    kind: copy(&r.kind, s)?,
                    id: copy(&r.id, s)?,
                    digest: copy(&r.digest, s)?,
                });
            }
        }
        s.semantic_work()
            .write_bytes(mem::size_of::<SecondaryOriginals>())
            .map_err(core_error)?;
        groups.push(SecondaryOriginals {
            source_index: index,
            references,
            _payload: payload,
            _control: controls,
        });
    }
    Ok((groups, control))
}
pub(super) fn with_capture<T>(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    source_index: usize,
    received_at: &Timestamp,
    s: &mut OriginalAuditSession,
    run: impl FnOnce(
        &EconomicCapture<'_>,
        &InputPool<'_>,
        &[u8],
        &mut OriginalAuditSession,
    ) -> Result<T>,
) -> Result<T> {
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
        if same_reference(other.enrollment_ref(), source.enrollment_ref(), s)? {
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
        if !same_reference(other.enrollment_ref(), source.enrollment_ref(), s)? {
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
    // Exact operation-derived original slots are DATA from the same owning
    // locator. The final interpreter independently rechecks every authority.
    let required_slots = observed
        .local_locators
        .as_ref()
        .map(|l| l.required_authority_slots())
        .filter(|slots| !slots.is_empty());
    let lifecycle_roles = observed
        .original
        .publication
        .program()
        .uses_work_lifecycle_recipe()
        && required_slots.is_some();
    let secondary = if lifecycle_roles {
        Some(secondary_originals(observed, source_index, s)?)
    } else {
        None
    };
    let frame_capacity = if observed
        .original
        .publication
        .program()
        .uses_work_lifecycle_recipe()
    {
        32
    } else {
        slots.len()
    };
    let _frame_owner = storage(
        frame_capacity,
        mem::size_of::<SourceAuthorityCapture<'_>>(),
        s,
    )?;
    let _capture_owner = s
        .reserve_external_payload(mem::size_of::<EconomicCapture<'_>>())
        .map_err(core_error)?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(frame_capacity)
        .map_err(|_| ServiceError::Unavailable)?;
    let selected_slot = observed
        .work_source_slot
        .as_ref()
        .map(|v| {
            s.parse_representation(v.bytes(), RepresentationKind::Record)
                .map_err(core_error)
        })
        .transpose()?;
    let payload_schema = check::text(payload, "schema", s)?;
    let selecting_work = payload_schema == "ledger-workflow-work/1" && selected_slot.is_none();
    let mut fact_slot = None;
    if payload_schema == "ledger-workflow-fact-attest/1" {
        let att = check::field(payload, "attestation", s)?;
        let schema_ref = check::field(att, "subject_schema_ref", s)?;
        for document in observed.original.publication.program().source_documents() {
            let reference = document.reference();
            if reference.kind() != check::text(schema_ref, "kind", s)?
                || reference.id() != check::text(schema_ref, "id", s)?
                || reference.digest() != check::text(schema_ref, "digest", s)?
            {
                continue;
            }
            let fields = check::field(document.canonical_value(), "fields", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            for claim in check::field(att, "claims", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                let name = check::text(claim, "field", s)?;
                let field = fields
                    .iter()
                    .find(|field| field.get("name").and_then(Value::as_str) == Some(name))
                    .ok_or(ServiceError::IntegrityFailure)?;
                let provenance = check::field(field, "provenance", s)?;
                check::same_text(check::text(provenance, "kind", s)?, "attested", s)?;
                let slot = check::text(provenance, "source_slot", s)?;
                if let Some(old) = fact_slot {
                    check::same_text(old, slot, s)?;
                } else {
                    fact_slot = Some(slot);
                }
            }
        }
        require(fact_slot.is_some(), "INVALID_REFERENCE")?;
    }
    for slot in slots {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let name = check::text(slot, "name", s)?;
        if let Some(required) = required_slots {
            let mut selected = false;
            for wanted in required {
                if equal(name, wanted, s)? {
                    selected = true;
                }
            }
            if !selected {
                continue;
            }
        }
        if let Some(fact_slot) = fact_slot {
            if !equal(name, fact_slot, s)? {
                continue;
            }
        }
        if let Some(selected) = selected_slot.as_ref() {
            if name
                != selected
                    .value()
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?
            {
                continue;
            }
        }
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
        if nominated {
            require(frames.len() < frame_capacity, "UNSUPPORTED_COMPOSITION")?;
            s.semantic_work()
                .write_bytes(mem::size_of::<SourceAuthorityCapture<'_>>())
                .map_err(core_error)?;
            if required_slots.is_none() && !selecting_work && !frames.is_empty() {
                continue;
            }
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
        if lifecycle_roles {
            if let Some((groups, _)) = &secondary {
                for group in groups {
                    let other = &observed.sources[group.source_index];
                    let grant = other
                        .current_grant_body()
                        .ok_or(ServiceError::IntegrityFailure)?;
                    let actual_source = check::text(grant, "source", s)?;
                    let mut eligible_nominee = false;
                    for nominee in nominees {
                        if equal(
                            nominee.as_str().ok_or(ServiceError::IntegrityFailure)?,
                            actual_source,
                            s,
                        )? {
                            eligible_nominee = true;
                        }
                    }
                    if !eligible_nominee {
                        continue;
                    }
                    require(frames.len() < frame_capacity, "UNSUPPORTED_COMPOSITION")?;
                    s.semantic_work()
                        .write_bytes(mem::size_of::<SourceAuthorityCapture<'_>>())
                        .map_err(core_error)?;
                    frames.push(SourceAuthorityCapture {
                        source_slot: name,
                        source: actual_source,
                        principal: actor,
                        canonical_economic_namespace: check::text(
                            grant,
                            "canonical_economic_namespace",
                            s,
                        )?,
                        enrollment_ref: other.enrollment_ref(),
                        current_state_ref: other.state_ref(),
                        current_grant_ref: other
                            .current_grant_ref()
                            .ok_or(ServiceError::IntegrityFailure)?,
                        original_grant_refs: &group.references,
                        binding_ref: observed.original.acceptance.binding.reference(),
                        accepted_agreement_ref: observed.original.acceptance.accepted_reference(),
                        activation_ref: activation.activation.reference(),
                    });
                }
            }
        }
    }
    if let Some(required) = required_slots {
        s.semantic_work()
            .arithmetic(Op::Compare, &[&frames.len().into(), &required.len().into()])
            .map_err(core_error)?;
        if lifecycle_roles {
            for wanted in required {
                let mut found = false;
                for frame in &frames {
                    if equal(frame.source_slot, wanted, s)? {
                        found = true;
                    }
                }
                require(found, "AUTHORITY_REQUIRED")?;
            }
        } else {
            require(frames.len() == required.len(), "AUTHORITY_REQUIRED")?;
        }
    }
    if lifecycle_roles {
        // The initiating snapshot remains primary even when a dependent slot
        // sorts before its correction slot. Role selection never uses this
        // order to choose a secondary nominee.
        let mut primary_count = 0usize;
        for index in 0..frames.len() {
            if equal(frames[index].source, &key.source, s)? {
                if index != primary_count {
                    s.semantic_work()
                        .write_bytes(2 * mem::size_of::<SourceAuthorityCapture<'_>>())
                        .map_err(core_error)?;
                    frames.swap(index, primary_count);
                }
                primary_count = add(primary_count, 1, s)?;
            }
        }
        require(primary_count > 0, "AUTHORITY_REQUIRED")?;
    }
    let capture = EconomicCapture {
        key: &core_key,
        actor,
        canonical_economic_namespace: namespace,
        installation_mode: "real_local",
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
    run(&capture, &pool, raw.bytes(), s)
}

pub(super) fn select(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    source_index: usize,
    received_at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<(
    AuditedBytes,
    AuditedBytes,
    WorkHeadLocators,
    Option<WorkHeadLocators>,
)> {
    with_capture(
        observed,
        key,
        payload,
        source_index,
        received_at,
        s,
        |capture, pool, raw, s| {
            let located = select_work_input_seeds(
                observed.original.publication.program(),
                capture,
                raw,
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
            let heads = select_work_head_locators(
                observed.original.publication.program(),
                capture,
                raw,
                &pool.rows,
                s,
            )
            .map_err(core_error)?;
            let predecessor = located.into_predecessor_work_heads();
            Ok((slot, refs, heads, predecessor))
        },
    )
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
                same_reference(old.enrollment_ref(), source.enrollment_ref(), s)?,
                "UNSUPPORTED_COMPOSITION",
            )?;
            require(
                same_reference(old.state_ref(), source.state_ref(), s)?,
                "STALE_REVISION",
            )?;
            require(
                same_reference(
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

/// Current canonical registrations for the operation's actual nominated roles.
/// This point-read stage never selects a nominee or grants its role rights.
pub(super) async fn required_role_registrations(
    observed: &ObservedAppBinding,
    locators: &ledgerlab_core::policy::workflow::transition_constructor::LocalOperationLocators,
    tx: &mut SqliteTx,
    key: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    if !observed.original.publication.program().uses_work_lifecycle_recipe() {
        return Ok(());
    }
    let binding = observed._parsed_binding.value();
    let slots = check::field(binding, "source_slots", s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
    for source in &observed.sources {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        let Some(grant) = source.current_grant_body() else { continue; };
        let name = check::text(grant, "source", s)?;
        let mut relevant = false;
        for required in locators.required_authority_slots() {
            for slot in slots {
                if !equal(check::text(slot, "name", s)?, required, s)? { continue; }
                for nominee in check::field(slot, "sources", s)?.as_array().ok_or(ServiceError::IntegrityFailure)? {
                    if equal(nominee.as_str().ok_or(ServiceError::IntegrityFailure)?, name, s)? { relevant = true; }
                }
            }
        }
        if !relevant { continue; }
        check::same_text(check::text(grant, "principal_id", s)?, &observed.setup.operator, s)?;
        let namespace = check::text(grant, "canonical_economic_namespace", s)?;
        let enrollment = check::text(source.enrollment_record().body(), "enrollment_id", s)?;
        let registered = if observed.original.publication.program().uses_shared_namespace_recipe() {
            tx.workflow_financial_registered_mapping(&key.logical_store_id, &key.scope,
                enrollment, &source.enrollment_ref().id, (name, namespace), s).await
        } else {
            check::same_text(namespace, name, s)?;
            tx.workflow_financial_registered_source(&key.logical_store_id, &key.scope,
                enrollment, &source.enrollment_ref().id, name, s).await
        }.map_err(prefix::read_error)?;
        require(registered, "AUTHORITY_REQUIRED")?;
    }
    Ok(())
}
/// Retain each actual shared registration's canonical owner's original birth.
/// No current owner head/rights or namespace inference replaces its certificate.
pub(super) async fn retain_namespace_birth_inputs(
    observed: &mut ObservedAppBinding, tx: &mut SqliteTx, key: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    if !observed.original.publication.program().uses_shared_namespace_recipe() { return Ok(()); }
    let count = observed.sources.len();
    observed.namespace_source_births_storage = Some(storage(count, mem::size_of::<prefix::SourceBirthInputs>(), s)?);
    observed.namespace_source_births.try_reserve_exact(count).map_err(|_| ServiceError::Unavailable)?;
    for index in 0..count {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        let source = &observed.sources[index];
        let grant = source.original_grant_body();
        if equal(check::text(grant, "source", s)?, check::text(grant, "canonical_economic_namespace", s)?, s)? { continue; }
        if let Some(primary) = &observed.work_source_birth {
            let enrollment = check::field(primary.state.body(), "enrollment_ref", s)?;
            if equal(check::text(enrollment, "id", s)?, &source.enrollment_ref().id, s)? { continue; }
        }
        let mut duplicate = false;
        for birth in &observed.namespace_source_births {
            let enrollment = check::field(birth.state.body(), "enrollment_ref", s)?;
            if equal(check::text(enrollment, "id", s)?, &source.enrollment_ref().id, s)? { duplicate = true; }
        }
        if duplicate { continue; }
        let birth = observed.original.source_birth_inputs(tx, key, source.enrollment_ref(), observed.before, s).await?;
        s.semantic_work().write_bytes(mem::size_of::<prefix::SourceBirthInputs>()).map_err(core_error)?;
        observed.namespace_source_births.push(birth);
    }
    Ok(())
}
pub(super) struct InputPool<'a> {
    pub(super) rows: Vec<EconomicRecordView<'a>>,
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
            .chain(birth.namespace_owner_views())
            .chain(o.namespace_source_births.iter().flat_map(|b| {
                [EconomicRecordView { reference: b.state.reference(), canonical_body: b.state.canonical_body() },
                 EconomicRecordView { reference: b.grant.reference(), canonical_body: b.grant.canonical_body() }]
                    .into_iter().chain(b.namespace_owner_views())
            }))
            .chain(
                o.local_input_bodies
                    .iter()
                    .flat_map(|pool| pool.iter_views()),
            )
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
                if !equal(&previous.reference.digest, &value.reference.digest, s)? {
                    require(o.original.publication.program().uses_stage_lifecycle_recipe() && value.reference.kind == "workflow-invocation", "INTEGRITY_FAILURE")?;
                    continue;
                }
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

/// Select the real already-retained source root for the unchanged local setup.
/// Historical completion is conjunctive with today's held admin/setup check;
/// no fresh snapshot body is inserted into pending observation inputs.
pub(super) async fn retained_root(
    observed: &mut ObservedAppBinding,
    tx: &mut SqliteTx,
    key: &OperationKey,
    index: usize,
    s: &mut OriginalAuditSession,
) -> Result<source::SourceRecord> {
    s.semantic_work()
        .visit(ValidationVisit::RefLookup)
        .map_err(core_error)?;
    let selected = observed
        .sources
        .get(index)
        .ok_or(ServiceError::IntegrityFailure)?;
    let reference = check::field(selected.enrollment_record().body(), "authority_ref", s)?;
    let administration = observed
        .work_source_birth
        .as_mut()
        .and_then(|birth| birth.registered_administration.as_mut());
    let (root, administrative_context) = if let Some(administration) = administration {
        let root = administration
            .root
            .take()
            .ok_or(ServiceError::IntegrityFailure)?;
        let expected = wire::reference(root.reference(), s)?;
        let actual = s
            .canonical_representation(reference, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .visit(ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        require(expected.bytes() == actual.bytes(), "INTEGRITY_FAILURE")?;
        check::same_text(&administration.actor, &observed.setup.operator, s)?;
        (
            root,
            Some((&administration.key, administration.actor.as_str())),
        )
    } else {
        (
            source::retained_record(
                tx,
                key,
                reference,
                "workflow-authority",
                "ledger-workflow-local-authority-snapshot/1",
                observed.before,
                s,
            )
            .await?,
            None,
        )
    };
    let encoded = wire::reference(root.reference(), s)?;
    observed
        .original
        .require_held_reference(encoded.bytes(), "workflow-authority", s)?;
    let (administrative_key, administrative_actor) =
        administrative_context.unwrap_or((key, observed.setup.operator.as_str()));
    super::super::party::produce::validate_original_root(
        root.body(),
        administrative_key,
        administrative_actor,
        &observed.at,
        s,
    )?;
    prefix::source::custody(root.body(), &observed.setup, s)?;
    let basis = check::field(root.body(), "basis", s)?;
    let revision = super::super::super::acceptance::counter(
        check::text(basis, "source_grant_revision", s)?,
        s,
    )?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&revision.into(), &observed.setup.grant_revision.into()],
        )
        .map_err(core_error)?;
    require(
        revision == observed.setup.grant_revision,
        "AUTHORITY_REQUIRED",
    )?;
    Ok(root)
}

pub(super) fn source_correspondences<'a>(
    observed: &'a ObservedAppBinding,
    s: &mut OriginalAuditSession,
) -> Result<(Vec<&'a ledgerlab_core::policy::workflow::transition_constructor::InitialSourceCorrespondence>, ExternalBufferLease)> {
    let count = add(observed.namespace_source_births.len(), usize::from(observed.work_source_birth.is_some()), s)?;
    let owner = storage(count, mem::size_of::<&ledgerlab_core::policy::workflow::transition_constructor::InitialSourceCorrespondence>(), s)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(count).map_err(|_| ServiceError::Unavailable)?;
    for birth in observed.work_source_birth.iter().chain(observed.namespace_source_births.iter()) {
        s.semantic_work().visit(ValidationVisit::Element).map_err(core_error)?;
        if let Some(correspondence) = birth.correspondence() {
            s.semantic_work().write_bytes(mem::size_of::<&ledgerlab_core::policy::workflow::transition_constructor::InitialSourceCorrespondence>()).map_err(core_error)?;
            refs.push(correspondence);
        }
    }
    Ok((refs, owner))
}

pub(super) fn construct_local(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    raw_request: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<(
    ledgerlab_core::policy::workflow::transition_constructor::EconomicCandidate,
    ledgerlab_core::policy::workflow::transition_constructor::LocalFinancialRequestEvidence,
)> {
    use ledgerlab_core::policy::workflow::transition_constructor::{
        construct_local_financial_candidate, produce_local_financial_request_evidence,
        EconomicStateData,
    };
    let index = observed
        .work_source_index
        .ok_or(ServiceError::IntegrityFailure)?;
    let received = observed
        .work_received_at
        .as_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    with_capture(
        observed,
        key,
        payload,
        index,
        received,
        s,
        |capture, _locator, raw, s| {
            let state = observed
                .work_state
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            let inputs = observed
                .local_input_bodies
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .views();
            let extra = observed.local_extra.as_ref();
            let app = extra.map(|e| {
                ledgerlab_core::policy::workflow::transition_constructor::AppStateData {
                    cost_heads: &e.costs,
                    observation_refs: &e.observations,
                    cost_claim_refs: &e.claims,
                    evidence_consumption_refs: &e.consumed,
                }
            });
            let (correspondences, _correspondence_owner) = source_correspondences(observed, s)?;
            let supplier = ledgerlab_core::policy::workflow::transition_constructor::SupplierStateData {
                invocation_heads: &state.invocations, cost_heads: &[], cost_consumption_heads: &[], cost_owner_heads: &[],
            };
            let state = EconomicStateData {
                retained_assertion: extra.and_then(|e| e.assertion.as_ref()),
                financial: &state.financial,
                capacity_heads: &state.capacities,
                supplier: observed.original.publication.program().uses_stage_lifecycle_recipe().then_some(&supplier),
                app: app.as_ref(),
                current_party_authorizations: &state.parties,
                observation: None,
                audit: None,
                initial_source_correspondences: &correspondences,
            };
            let candidate = construct_local_financial_candidate(
                observed.original.publication.program(),
                capture,
                raw,
                &inputs,
                &state,
                s,
            )
            .map_err(local_construction_error)?;
            let evidence = produce_local_financial_request_evidence(
                observed.original.publication.program(),
                raw_request,
                &candidate,
                &inputs,
                &state,
                s,
            )
            .map_err(core_error)?;
            Ok((candidate, evidence))
        },
    )
}

// This factory selects the local recipe, whose registry includes this refusal.
// Preserve the released shared mapper for every other diagnostic and recipe.
const LOCAL_CONSTRUCTION_ERRORS: &[&str] = &[
    "ACTIVATION_SCHEDULE_CONFLICT",
    "AGREEMENT_OWNERSHIP_CONFLICT",
    "ASSENT_TERMS_MISMATCH",
    "AUDIT_CANCELLED",
    "AUDIT_INTEGRITY_OR_PROFILE_FAILURE",
    "AUTHORITY_REVIEW_REQUIRED",
    "CAPACITY_EXCEEDED",
    "CLOCK_REGRESSION",
    "CORRECTION_TERMS_UNSPECIFIED",
    "CORRECTION_WINDOW_CLOSED",
    "COST_COVERAGE_CONFLICT",
    "COST_LINE_OWNED",
    "EXPOSURE_EXCEEDED",
    "HISTORICAL_VERIFIER_UNAVAILABLE",
    "INSTALLATION_MODE_MISMATCH",
    "INVOCATION_CONFLICT",
    "INVOCATION_REQUIRED",
    "LINK_CARDINALITY",
    "LINK_CYCLE",
    "MISSING_DEPENDENCY",
    "MISSING_FINANCIAL_ORIGIN",
    "OCCURRENCE_EVIDENCE_REQUIRED",
    "ORIGINAL_TERMS_MISMATCH",
    "SOURCE_CEILING_EXCEEDED",
    "TARGET_CLOSED",
    "WAITING_FINAL_COST",
];
fn local_construction_error(error: ledgerlab_core::Error) -> ServiceError {
    if error.code == "REVISION_CONFLICT" {
        return ServiceError::Rejection("STALE_REVISION".into());
    }
    if error.code == "EXPOSURE_CEILING_EXCEEDED" {
        return ServiceError::Rejection("EXPOSURE_EXCEEDED".into());
    }
    if LOCAL_CONSTRUCTION_ERRORS.contains(&error.code) {
        ServiceError::Rejection(error.code.into())
    } else {
        // Includes the existing sanitized operator-resource diagnostics.
        core_error(error)
    }
}

#[cfg(test)]
mod local_error_tests {
    use super::*;

    #[test]
    fn local_revision_conflict_uses_declared_stale_revision() {
        let registry: Value = serde_json::from_str(include_str!(
            "../../../../../../../../../contracts/candidates/full-suite-v1/core/local-execution-01/registry.json"
        ))
        .unwrap();
        assert!(registry["errors"]
            .as_array()
            .unwrap()
            .contains(&json!("STALE_REVISION")));
        let error = || ledgerlab_core::Error {
            code: "REVISION_CONFLICT",
            detail: "root expected revision".into(),
        };
        assert_eq!(local_construction_error(error()), reject("STALE_REVISION"));
        assert_eq!(core_error(error()), ServiceError::Unavailable);
    }

    #[test]
    fn local_construction_errors_are_declared_without_widening_legacy_mapping() {
        let registry: Value = serde_json::from_str(include_str!(
            "../../../../../../../../../contracts/candidates/full-suite-v1/core/local-execution-01/registry.json"
        )).unwrap();
        for code in LOCAL_CONSTRUCTION_ERRORS {
            assert!(registry["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some(*code)));
            assert_eq!(
                local_construction_error(ledgerlab_core::Error {
                    code,
                    detail: "fixture".into()
                }),
                ServiceError::Rejection((*code).into())
            );
            assert_eq!(
                core_error(ledgerlab_core::Error {
                    code,
                    detail: "fixture".into()
                }),
                ServiceError::Unavailable
            );
        }
        let ceiling = || ledgerlab_core::Error {
            code: "EXPOSURE_CEILING_EXCEEDED",
            detail: "actual app final amount above original maximum".into(),
        };
        assert_eq!(
            local_construction_error(ceiling()),
            ServiceError::Rejection("EXPOSURE_EXCEEDED".into())
        );
        assert_eq!(core_error(ceiling()), ServiceError::Unavailable);
        let resource = ledgerlab_core::Error {
            code: "PROGRAM_BUDGET_EXCEEDED",
            detail: "resource limit: semantic_work limit=1 observed=2".into(),
        };
        assert_eq!(
            local_construction_error(resource),
            ServiceError::ResourceLimit {
                setting: "semantic_work",
                limit: 1,
                observed: 2
            }
        );
    }

    #[test]
    fn local_outcome_window_error_uses_its_declared_vocabulary() {
        let error = |code| ledgerlab_core::Error {
            code,
            detail: "mapping fixture".into(),
        };
        assert_eq!(
            local_construction_error(error("CORRECTION_WINDOW_CLOSED")),
            ServiceError::Rejection("CORRECTION_WINDOW_CLOSED".into())
        );
        assert_eq!(
            core_error(error("CORRECTION_WINDOW_CLOSED")),
            ServiceError::Unavailable
        );
        assert_eq!(
            local_construction_error(error("FUTURE_INTERNAL_FAILURE")),
            ServiceError::Unavailable
        );
        assert_eq!(
            local_construction_error(error("STALE_REVISION")),
            core_error(error("STALE_REVISION"))
        );
    }
}
