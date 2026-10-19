//! Actual captured root source and completed initial activation, borrowed into
//! the SAME pure work locator. This is source-slot DATA, not complete I.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::{
        EconomicCapture, EconomicRecordView, OperationKey as CoreKey, SourceAuthorityCapture,
        WorkHeadLocators, select_work_head_locators, select_work_input_seeds,
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
pub(super) fn with_capture<T>(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    source_index: usize,
    received_at: &Timestamp,
    s: &mut OriginalAuditSession,
    run:impl FnOnce(&EconomicCapture<'_>,&InputPool<'_>,&[u8],&mut OriginalAuditSession)->Result<T>,
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
    let _frame_owner = storage(slots.len(), mem::size_of::<SourceAuthorityCapture<'_>>(), s)?;
    let _capture_owner = s
        .reserve_external_payload(mem::size_of::<EconomicCapture<'_>>())
        .map_err(core_error)?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(slots.len())
        .map_err(|_| ServiceError::Unavailable)?;
    let selected_slot=observed.work_source_slot.as_ref().map(|v|s.parse_representation(v.bytes(),RepresentationKind::Record).map_err(core_error)).transpose()?;
    let payload_schema=check::text(payload,"schema",s)?;
    let selecting_work=payload_schema=="ledger-workflow-work/1"&&selected_slot.is_none();
    // Exact operation-derived original slots are DATA from the same owning
    // locator. The final interpreter independently rechecks every authority.
    let required_slots=observed.local_locators.as_ref().map(|l|l.required_authority_slots()).filter(|slots|!slots.is_empty());
    let mut fact_slot=None;
    if payload_schema=="ledger-workflow-fact-attest/1" {
        let att=check::field(payload,"attestation",s)?;
        let schema_ref=check::field(att,"subject_schema_ref",s)?;
        for document in observed.original.publication.program().source_documents(){
            let reference=document.reference();
            if reference.kind()!=check::text(schema_ref,"kind",s)?||reference.id()!=check::text(schema_ref,"id",s)?||reference.digest()!=check::text(schema_ref,"digest",s)?{continue;}
            let fields=check::field(document.canonical_value(),"fields",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
            for claim in check::field(att,"claims",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?{
                let name=check::text(claim,"field",s)?;
                let field=fields.iter().find(|field|field.get("name").and_then(Value::as_str)==Some(name)).ok_or(ServiceError::IntegrityFailure)?;
                let provenance=check::field(field,"provenance",s)?;
                check::same_text(check::text(provenance,"kind",s)?,"attested",s)?;
                let slot=check::text(provenance,"source_slot",s)?;
                if let Some(old)=fact_slot{check::same_text(old,slot,s)?;}else{fact_slot=Some(slot);}
            }
        }
        require(fact_slot.is_some(),"INVALID_REFERENCE")?;
    }
    for slot in slots {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let name = check::text(slot, "name", s)?;
        if let Some(required)=required_slots {
            let mut selected=false;
            for wanted in required {if equal(name,wanted,s)?{selected=true;}}
            if !selected{continue;}
        }
        if let Some(fact_slot)=fact_slot {if !equal(name,fact_slot,s)?{continue;}}
        if let Some(selected)=selected_slot.as_ref(){if name!=selected.value().as_str().ok_or(ServiceError::IntegrityFailure)?{continue;}}
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
        if required_slots.is_none()&&!selecting_work&&!frames.is_empty(){continue;}
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
    if let Some(required)=required_slots {
        s.semantic_work().arithmetic(Op::Compare,&[&frames.len().into(),&required.len().into()]).map_err(core_error)?;
        require(frames.len()==required.len(),"AUTHORITY_REQUIRED")?;
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
    run(&capture,&pool,raw.bytes(),s)
}

pub(super) fn select(
    observed: &ObservedAppBinding,
    key: &OperationKey,
    payload: &Value,
    source_index: usize,
    received_at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<(AuditedBytes, AuditedBytes, WorkHeadLocators)> {
    with_capture(observed,key,payload,source_index,received_at,s,|capture,pool,raw,s|{
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
    Ok((slot, refs, heads))
    })
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
    let root = source::retained_record(
        tx,
        key,
        reference,
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1",
        observed.before,
        s,
    )
    .await?;
    let encoded = wire::reference(root.reference(), s)?;
    observed
        .original
        .require_held_reference(encoded.bytes(), "workflow-authority", s)?;
    super::super::party::produce::validate_original_root(
        root.body(),
        key,
        &observed.setup.operator,
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

pub(super) fn construct_local(observed:&ObservedAppBinding,key:&OperationKey,payload:&Value,raw_request:&[u8],s:&mut OriginalAuditSession)->Result<(ledgerlab_core::policy::workflow::transition_constructor::EconomicCandidate,ledgerlab_core::policy::workflow::transition_constructor::LocalFinancialRequestEvidence)>{
 use ledgerlab_core::policy::workflow::transition_constructor::{EconomicStateData,construct_local_financial_candidate,produce_local_financial_request_evidence};
 let index=observed.work_source_index.ok_or(ServiceError::IntegrityFailure)?;
 let received=observed.work_received_at.as_ref().ok_or(ServiceError::IntegrityFailure)?;
 with_capture(observed,key,payload,index,received,s,|capture,_locator,raw,s|{
  let state=observed.work_state.as_ref().ok_or(ServiceError::IntegrityFailure)?;
  let inputs=observed.local_input_bodies.as_ref().ok_or(ServiceError::IntegrityFailure)?.views();
  let extra=observed.local_extra.as_ref();
  let app=extra.map(|e|ledgerlab_core::policy::workflow::transition_constructor::AppStateData{cost_heads:&e.costs,observation_refs:&e.observations,cost_claim_refs:&e.claims,evidence_consumption_refs:&e.consumed});
  let state=EconomicStateData{retained_assertion:extra.and_then(|e|e.assertion.as_ref()),financial:&state.financial,capacity_heads:&state.capacities,supplier:None,app:app.as_ref(),current_party_authorizations:&state.parties,observation:None,audit:None,initial_source_correspondences:&[]};
  let candidate=construct_local_financial_candidate(observed.original.publication.program(),capture,raw,&inputs,&state,s).map_err(local_construction_error)?;
  let evidence=produce_local_financial_request_evidence(observed.original.publication.program(),raw_request,&candidate,&inputs,&state,s).map_err(core_error)?;
  Ok((candidate,evidence))
 })
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
    fn local_construction_errors_are_declared_without_widening_legacy_mapping() {
        let registry: Value = serde_json::from_str(include_str!(
            "../../../../../../../../../contracts/candidates/full-suite-v1/core/local-execution-01/registry.json"
        )).unwrap();
        for code in LOCAL_CONSTRUCTION_ERRORS {
            assert!(registry["errors"].as_array().unwrap().iter().any(|v|v.as_str()==Some(*code)));
            assert_eq!(local_construction_error(ledgerlab_core::Error { code, detail: "fixture".into() }), ServiceError::Rejection((*code).into()));
            assert_eq!(core_error(ledgerlab_core::Error { code, detail: "fixture".into() }), ServiceError::Unavailable);
        }
        let resource = ledgerlab_core::Error {code:"PROGRAM_BUDGET_EXCEEDED",detail:"resource limit: semantic_work limit=1 observed=2".into()};
        assert_eq!(local_construction_error(resource), ServiceError::ResourceLimit {setting:"semantic_work",limit:1,observed:2});
    }

    #[test]
    fn local_outcome_window_error_uses_its_declared_vocabulary() {
        let error = |code| ledgerlab_core::Error { code, detail: "mapping fixture".into() };
        assert_eq!(
            local_construction_error(error("CORRECTION_WINDOW_CLOSED")),
            ServiceError::Rejection("CORRECTION_WINDOW_CLOSED".into())
        );
        assert_eq!(core_error(error("CORRECTION_WINDOW_CLOSED")), ServiceError::Unavailable);
        assert_eq!(local_construction_error(error("FUTURE_INTERNAL_FAILURE")), ServiceError::Unavailable);
        assert_eq!(local_construction_error(error("STALE_REVISION")), core_error(error("STALE_REVISION")));
    }
}
