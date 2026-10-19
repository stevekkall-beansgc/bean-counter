//! Select the one literal admission-source frame from actual locked source
//! observations. This is DATA selection, not grant eligibility. Independent
//! nominated workers are not equated with the administering actor.
use super::*;

pub(super) fn select(
    sources: &[current::source::ObservedSource],
    actor: &str,
    key: &OperationKey,
    s: &mut OriginalAuditSession,
) -> Result<usize> {
    let mut selected = None;
    for (index, source) in sources.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let Some(grant) = source.current_grant_body() else {
            continue;
        };
        let principal = request_wrapper::text(grant, "principal_id", s)?;
        let namespace = request_wrapper::text(grant, "source", s)?;
        let equal = |a: &str, b: &str, s: &mut OriginalAuditSession| -> Result<bool> {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
            s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
            Ok(a == b)
        };
        if equal(principal, actor, s)? && equal(namespace, &key.source, s)? {
            s.semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            // The frozen agreement observation has one state and one scalar
            // revision. Never choose a maximum or concatenate independent
            // current states to manufacture that observation.
            require(selected.replace(index).is_none(), "UNSUPPORTED_COMPOSITION")?;
        }
    }
    selected.ok_or_else(|| reject("AUTHORITY_REQUIRED"))
}

/// Mechanical Authority5 body from actual observed operands. This owning
/// document remains DATA until all admission/nomination/chronological checks
/// complete; it is never a permission bit or a private prepared commit.
pub(super) fn authority(
    input: &AppAcceptanceInput,
    source: &current::source::ObservedSource,
    consents: &consent::Coverage,
    root: &wire::SourceRecord,
    key: &OperationKey,
    actor: &str,
    at: &Timestamp,
    operation: &'static str,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let refs = wire::RefSet::new(
        consents.verification_refs(),
        consents.verification_refs().len(),
        true,
        s,
    )?
    .encode(s)?;
    authority_with_refs(
        input,
        source,
        refs.bytes(),
        root,
        key,
        actor,
        at,
        operation,
        s,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn authority_with_refs(
    input: &AppAcceptanceInput,
    source: &current::source::ObservedSource,
    verification_refs: &[u8],
    root: &wire::SourceRecord,
    key: &OperationKey,
    actor: &str,
    at: &Timestamp,
    operation: &'static str,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    require(acceptance_operation(operation, s)?, "INVALID_SHAPE")?;
    let operation = wire::string(operation, s)?;
    let grant = source
        .current_grant_body()
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let grant_ref = source
        .current_grant_ref()
        .ok_or(ServiceError::IntegrityFailure)?;
    let grants = wire::RefSet::new(std::iter::once(grant_ref), 1, true, s)?.encode(s)?;
    let states = wire::RefSet::new(std::iter::once(source.state_ref()), 1, true, s)?.encode(s)?;
    let root = wire::reference(root.reference(), s)?;
    let actor = wire::string(actor, s)?;
    let current_source = request_wrapper::text(grant, "source", s)?;
    request_wrapper::same_text(current_source, &key.source, s)?;
    let source = wire::string(current_source, s)?;
    let namespace = wire::string(
        request_wrapper::text(grant, "canonical_economic_namespace", s)?,
        s,
    )?;
    let revision = wire::string(request_wrapper::text(grant, "revision", s)?, s)?;
    let scope = encode(
        request_wrapper::field(input.terms.canonical_value(), "scope", s)?,
        s,
    )?;
    let customer = encode(
        request_wrapper::field(input.terms.canonical_value(), "customer", s)?,
        s,
    )?;
    let agreement = encode(
        request_wrapper::field(input.terms.canonical_value(), "agreement_id", s)?,
        s,
    )?;
    let assents = encode(input.assent_refs(), s)?;
    let at = wire::string(at.as_str(), s)?;
    let schema = wire::string("ledger-workflow-agreement-authority/5", s)?;
    let bytes = wire::object(
        &[
            ("actor", actor.bytes()),
            ("agreement_id", agreement.bytes()),
            ("app_consent_verification_refs", verification_refs),
            ("assent_refs", assents.bytes()),
            ("canonical_economic_namespace", namespace.bytes()),
            ("customer", customer.bytes()),
            ("local_admin_ref", root.bytes()),
            ("observed_at", at.bytes()),
            ("operation", operation.bytes()),
            ("purpose", br#""admit""#),
            ("schema", schema.bytes()),
            ("scope", scope.bytes()),
            ("source", source.bytes()),
            ("source_grant_refs", grants.bytes()),
            ("source_revision", revision.bytes()),
            ("source_state_refs", states.bytes()),
        ],
        262144,
        s,
    )?;
    wire::source_record(
        "workflow-authority",
        "ledger-workflow-agreement-authority/5",
        bytes,
        s,
    )
}

fn acceptance_operation(operation: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    Ok(same_recipe_text(operation, "agreement.accept", s)?
        || same_recipe_text(operation, "agreement.activate", s)?)
}
