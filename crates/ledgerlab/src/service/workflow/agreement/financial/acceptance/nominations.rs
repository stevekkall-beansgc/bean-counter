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
    }
    Ok(())
}
