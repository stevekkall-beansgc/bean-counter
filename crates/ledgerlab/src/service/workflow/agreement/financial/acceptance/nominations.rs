//! Borrow actual owning source observations into the SAME graph-derived core
//! check. This grouping creates no authority, merged grant or completion flag.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    ExactRef, NominatedSourceFrame, NominatedSourceGrant, validate_app_nominated_sources,
};

struct Grants<'a> {
    values: Vec<NominatedSourceGrant<'a>>,
    _lease: ExternalBufferLease,
}
struct Frames<'a> {
    values: Vec<NominatedSourceFrame<'a>>,
    _lease: ExternalBufferLease,
}
fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn same_ref(a: &ExactRef, b: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
    Ok(equal(&a.kind, &b.kind, s)? && equal(&a.id, &b.id, s)? && equal(&a.digest, &b.digest, s)?)
}
fn member(reference: &ExactRef, rows: &[Value], s: &mut OriginalAuditSession) -> Result<bool> {
    for row in rows {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        if equal(&reference.kind, request_wrapper::text(row, "kind", s)?, s)?
            && equal(&reference.id, request_wrapper::text(row, "id", s)?, s)?
            && equal(
                &reference.digest,
                request_wrapper::text(row, "digest", s)?,
                s,
            )?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn storage(n: usize, size: usize, s: &mut OriginalAuditSession) -> Result<ExternalBufferLease> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&n.into(), &size.into()])
        .map_err(core_error)?;
    s.reserve_external_payload(n.checked_mul(size).ok_or(ServiceError::Unavailable)?)
        .map_err(core_error)
}

pub(super) fn validate(
    sources: &[current::source::ObservedSource],
    documents: &private::InlineDocuments,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    binding: &Value,
    accepted_at: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    validate_selected(sources, documents, input, publication, binding, accepted_at, false, s)
}

pub(super) fn validate_local_admission(
    sources: &[current::source::ObservedSource],
    documents: &private::InlineDocuments,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    binding: &Value,
    accepted_at: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    validate_selected(sources, documents, input, publication, binding, accepted_at, true, s)
}

fn validate_selected(
    sources: &[current::source::ObservedSource],
    documents: &private::InlineDocuments,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    binding: &Value,
    accepted_at: &str,
    local_admission: bool,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    size(sources.len(), 1, 32, s)?;
    let projected = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let bindings = request_wrapper::field(projected, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    for row in bindings {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let terms = documents.body(request_wrapper::field(row, "binding_ref", s)?, s)?;
        let refs = request_wrapper::field(terms, "source_grant_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        size(refs.len(), 1, 32, s)?;
        let mut grants = Grants {
            values: Vec::new(),
            _lease: storage(sources.len(), mem::size_of::<NominatedSourceGrant<'_>>(), s)?,
        };
        grants
            .values
            .try_reserve_exact(sources.len())
            .map_err(|_| ServiceError::Unavailable)?;
        // Fixed, bounded control state; no copied grant bodies or Ref strings.
        let mut representatives = [0usize; 32];
        let mut starts = [0usize; 32];
        let mut ends = [0usize; 32];
        let mut count = 0usize;
        for (i, source) in sources.iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if !member(source.original_grant_ref(), refs, s)? {
                continue;
            }
            let mut seen = false;
            for previous in &representatives[..count] {
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                if same_ref(
                    sources[*previous].enrollment_ref(),
                    source.enrollment_ref(),
                    s,
                )? {
                    seen = true;
                    break;
                }
            }
            if seen {
                continue;
            }
            s.semantic_work()
                .arithmetic(Op::Compare, &[&count.into(), &32usize.into()])
                .map_err(core_error)?;
            require(count < 32, "INVALID_SHAPE")?;
            s.semantic_work()
                .write_bytes(3 * mem::size_of::<usize>())
                .map_err(core_error)?;
            representatives[count] = i;
            starts[count] = grants.values.len();
            for other in sources {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                if !member(other.original_grant_ref(), refs, s)?
                    || !same_ref(source.enrollment_ref(), other.enrollment_ref(), s)?
                {
                    continue;
                }
                require(
                    same_ref(source.state_ref(), other.state_ref(), s)?,
                    "STALE_REVISION",
                )?;
                let current = source
                    .current_grant_ref()
                    .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
                let other_current = other
                    .current_grant_ref()
                    .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
                require(same_ref(current, other_current, s)?, "STALE_REVISION")?;
                s.semantic_work()
                    .arithmetic(
                        Op::Compare,
                        &[&grants.values.len().into(), &sources.len().into()],
                    )
                    .map_err(core_error)?;
                require(grants.values.len() < sources.len(), "INVALID_SHAPE")?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<NominatedSourceGrant<'_>>())
                    .map_err(core_error)?;
                grants.values.push(NominatedSourceGrant {
                    reference: other.original_grant_ref(),
                    body: other.original_grant_body(),
                });
            }
            ends[count] = grants.values.len();
            s.semantic_work()
                .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            count += 1;
        }
        let mut frames = Frames {
            values: Vec::new(),
            _lease: storage(count, mem::size_of::<NominatedSourceFrame<'_>>(), s)?,
        };
        frames
            .values
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        for i in 0..count {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let source = &sources[representatives[i]];
            let current_grant_ref = source
                .current_grant_ref()
                .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
            let current_grant = source
                .current_grant_body()
                .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
            s.semantic_work()
                .write_bytes(mem::size_of::<NominatedSourceFrame<'_>>())
                .map_err(core_error)?;
            frames.values.push(NominatedSourceFrame {
                enrollment_ref: source.enrollment_ref(),
                enrollment: source.enrollment_record().body(),
                current_state_ref: source.state_ref(),
                current_state: source.state_body(),
                current_grant_ref,
                current_grant,
                original_grants: &grants.values[starts[i]..ends[i]],
            });
        }
        validate_app_nominated_sources(
            publication.program(),
            binding,
            terms,
            &frames.values,
            accepted_at,
            s,
        )
        .map_err(core_error)?;
        if local_admission {
            require_local_source_shape(binding, &frames.values, s)?;
            let catalog=documents.body(request_wrapper::field(binding,"slot_catalog_ref",s)?,s)?;
            let families=request_wrapper::field(catalog,"families",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work().arithmetic(Op::Compare,&[&families.len().into(),&1usize.into()]).map_err(core_error)?;
            require(families.len()==1,"UNSUPPORTED_COMPOSITION")?;
            let mut policy=None;
            for document in publication.program().source_documents(){
                s.semantic_work().visit(Visit::Element).map_err(core_error)?;
                if equal(document.reference().id(),publication.program().root_policy_ref().id(),s)? { policy=Some(document.canonical_value()); }
            }
            let policy=policy.ok_or(ServiceError::IntegrityFailure)?;
            let policy_families=request_wrapper::field(policy,"families",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work().arithmetic(Op::Compare,&[&policy_families.len().into(),&1usize.into()]).map_err(core_error)?;
            require(policy_families.len()==1,"UNSUPPORTED_COMPOSITION")?;
            require_local_causal_clause(terms,&families[0],request_wrapper::text(&policy_families[0],"correction_source_slot",s)?,s)?;
        }
    }
    Ok(())
}

/// The existing work-time future_source_shape restriction, checked before a
/// new local agreement/activation can promise dependent operations. The
/// frames have just passed validate_app_nominated_sources. No accepting actor
/// or request source is used as the nominated future worker.
fn require_local_source_shape(
    binding: &Value,
    frames: &[NominatedSourceFrame<'_>],
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work().arithmetic(Op::Compare, &[&frames.len().into(), &1usize.into()]).map_err(core_error)?;
    require(frames.len() == 1, "UNSUPPORTED_COMPOSITION")?;
    let frame = &frames[0];
    let first = frame.original_grants.first().ok_or(ServiceError::IntegrityFailure)?;
    let source = request_wrapper::text(first.body, "source", s)?;
    let principal = request_wrapper::text(first.body, "principal_id", s)?;
    for nomination in request_wrapper::field(binding, "source_slots", s)?.as_array().ok_or(ServiceError::IntegrityFailure)? {
        s.semantic_work().visit(Visit::Element).map_err(core_error)?;
        for nominee in request_wrapper::field(nomination, "sources", s)?.as_array().ok_or(ServiceError::IntegrityFailure)? {
            s.semantic_work().visit(Visit::Element).map_err(core_error)?;
            require(equal(nominee.as_str().ok_or(ServiceError::IntegrityFailure)?, source, s)?, "UNSUPPORTED_COMPOSITION")?;
        }
    }
    for grant in frame.original_grants {
        s.semantic_work().visit(Visit::Element).map_err(core_error)?;
        require(equal(request_wrapper::text(grant.body, "source", s)?, source, s)?
            && equal(request_wrapper::text(grant.body, "principal_id", s)?, principal, s)?, "UNSUPPORTED_COMPOSITION")?;
        let reference = request_wrapper::field(grant.body, "original_ceiling_ref", s)?;
        require(equal(request_wrapper::text(reference, "kind", s)?, &frame.enrollment_ref.kind, s)?
            && equal(request_wrapper::text(reference, "id", s)?, &frame.enrollment_ref.id, s)?
            && equal(request_wrapper::text(reference, "digest", s)?, &frame.enrollment_ref.digest, s)?, "UNSUPPORTED_COMPOSITION")?;
    }
    Ok(())
}

// Existing slot projection already validates clause shape/mode/nomination and
// exact app terms. NEW admission also requires the original family clause;
// the compiler's policy declaration never supplies missing assent.
fn require_local_causal_clause(terms:&Value,family:&Value,corrector:&str,s:&mut OriginalAuditSession)->Result<()> {
    let clauses=request_wrapper::field(terms,"causal_reversal_clauses",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work().arithmetic(Op::Compare,&[&clauses.len().into(),&1usize.into()]).map_err(core_error)?;
    require(clauses.len()==1,"UNSUPPORTED_COMPOSITION")?;
    let clause=&clauses[0];
    for (field,expected) in [("family_slot",request_wrapper::text(family,"slot_id",s)?),("root_component_slot",request_wrapper::text(family,"target_component_slot",s)?),("correction_source_slot",corrector),("mode","exact_inverse_and_permanent_close"),("standalone_deadline_effect","does_not_expire_preaccepted_causal_reversal")] {
        require(equal(request_wrapper::text(clause,field,s)?,expected,s)?,"UNSUPPORTED_COMPOSITION")?;
    }
    Ok(())
}

#[cfg(test)]
mod local_admission_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn local_admission_requires_original_exact_family_causal_assent() {
        let arena=ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let family=json!({"slot_id":"family","target_component_slot":"service"});
        let terms=json!({"causal_reversal_clauses":[{"family_slot":"family","root_component_slot":"service","correction_source_slot":"assessor","mode":"exact_inverse_and_permanent_close","standalone_deadline_effect":"does_not_expire_preaccepted_causal_reversal"}]});
        require_local_causal_clause(&terms,&family,"assessor",&mut s).unwrap();
        assert_eq!(require_local_causal_clause(&json!({"causal_reversal_clauses":[]}),&family,"assessor",&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
        let mut wrong=terms.clone();wrong["causal_reversal_clauses"][0]["correction_source_slot"]=json!("other");
        assert_eq!(require_local_causal_clause(&wrong,&family,"assessor",&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
        wrong=terms;wrong["causal_reversal_clauses"][0]["root_component_slot"]=json!("app-cost");
        assert_eq!(require_local_causal_clause(&wrong,&family,"assessor",&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
    }

    #[test]
    fn local_admission_uses_nominated_frame_not_accepting_actor() {
        let arena=ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let enrollment=ExactRef{kind:"workflow-source-enrollment".into(),id:format!("wdoc_{}","a".repeat(64)),digest:format!("sha256:{}","a".repeat(64))};
        let grant_ref=ExactRef{kind:"workflow-source-grant".into(),id:format!("wdoc_{}","b".repeat(64)),digest:format!("sha256:{}","b".repeat(64))};
        let empty=json!({});
        // Production calls this support check only after graph/origin validation.
        // A different accepting administrator is intentionally not an operand.
        let grant=json!({"source":"future-worker","principal_id":"worker-principal","original_ceiling_ref":enrollment});
        let grants=[NominatedSourceGrant{reference:&grant_ref,body:&grant}];
        let frame=NominatedSourceFrame{enrollment_ref:&enrollment,enrollment:&empty,current_state_ref:&enrollment,current_state:&empty,current_grant_ref:&grant_ref,current_grant:&grant,original_grants:&grants};
        let binding=json!({"source_slots":[{"name":"worker","sources":["future-worker"]},{"name":"assessor","sources":["future-worker"]}]});
        let mut s=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        require_local_source_shape(&binding,std::slice::from_ref(&frame),&mut s).unwrap();
        let mismatched=json!({"source_slots":[{"name":"assessor","sources":["other-source"]}]});
        assert_eq!(require_local_source_shape(&mismatched,std::slice::from_ref(&frame),&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
        let other_principal=json!({"source":"future-worker","principal_id":"another-principal","original_ceiling_ref":enrollment});
        let mixed=[NominatedSourceGrant{reference:&grant_ref,body:&grant},NominatedSourceGrant{reference:&grant_ref,body:&other_principal}];
        let mixed_frame=NominatedSourceFrame{original_grants:&mixed,..frame};
        assert_eq!(require_local_source_shape(&binding,std::slice::from_ref(&mixed_frame),&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
        let wrong_enrollment=json!({"source":"future-worker","principal_id":"worker-principal","original_ceiling_ref":{"kind":enrollment.kind,"id":enrollment.id,"digest":format!("sha256:{}","c".repeat(64))}});
        let wrong=[NominatedSourceGrant{reference:&grant_ref,body:&wrong_enrollment}];
        let wrong_frame=NominatedSourceFrame{original_grants:&wrong,..mixed_frame};
        assert_eq!(require_local_source_shape(&binding,std::slice::from_ref(&wrong_frame),&mut s),Err(reject("UNSUPPORTED_COMPOSITION")));
    }
}
