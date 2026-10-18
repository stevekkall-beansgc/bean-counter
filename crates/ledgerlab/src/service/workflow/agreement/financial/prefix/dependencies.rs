//! Closed schema Ref edges for the SAME administrative/acceptance closure.
//! The caller supplies normalized or already completed bodies; this traversal
//! creates no custody and never classifies arbitrary business dictionaries as
//! authority. It always runs on the responsible origin's semantic work.
use super::*;
type Emit<'a> = dyn FnMut(&Value, &mut OriginalAuditSession) -> Result<()> + 'a;
fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn generic(v: &Value, emit: &mut Emit<'_>, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work().visit(Visit::Value).map_err(core_error)?;
    match v {
        Value::Object(map) => {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&map.len().into(), &3usize.into()])
                .map_err(core_error)?;
            if map.len() == 3 {
                let mut names = 0u8;
                for name in map.keys() {
                    s.semantic_work().visit(Visit::Field).map_err(core_error)?;
                    if equal(name, "kind", s)? || equal(name, "id", s)? || equal(name, "digest", s)?
                    {
                        s.semantic_work()
                            .arithmetic(Op::Add, &[&names.into(), &1u8.into()])
                            .map_err(core_error)?;
                        names = names.checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
                    }
                }
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&names.into(), &3u8.into()])
                    .map_err(core_error)?;
                if names == 3 {
                    return emit(v, s);
                }
            }
            for v in map.values() {
                s.semantic_work().visit(Visit::Field).map_err(core_error)?;
                generic(v, emit, s)?;
            }
        }
        Value::Array(values) => {
            for v in values {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                generic(v, emit, s)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn binding(v: &Value, emit: &mut Emit<'_>, s: &mut OriginalAuditSession) -> Result<()> {
    for (name, v) in v.as_object().ok_or(ServiceError::IntegrityFailure)? {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        if equal(name, "accepted_context", s)? {
            // Facts are scalar business values under the actual FactSchema,
            // even if three user field names happen to be kind/id/digest.
            emit(request_wrapper::field(v, "fact_schema_ref", s)?, s)?;
        } else {
            generic(v, emit, s)?;
        }
    }
    Ok(())
}
fn payload(v: &Value, emit: &mut Emit<'_>, s: &mut OriginalAuditSession) -> Result<()> {
    for (name, v) in v.as_object().ok_or(ServiceError::IntegrityFailure)? {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        if equal(name, "binding_inputs", s)? {
            binding(v, emit, s)?;
        } else if equal(name, "documents", s)? {
            for v in v.as_array().ok_or(ServiceError::IntegrityFailure)? {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                body(v, emit, s)?;
            }
        } else {
            generic(v, emit, s)?;
        }
    }
    Ok(())
}
pub(super) fn body(v: &Value, emit: &mut Emit<'_>, s: &mut OriginalAuditSession) -> Result<()> {
    let schema = request_wrapper::text(v, "schema", s)?;
    let mut allowed = false;
    for expected in [
        "ledger-workflow-local-authority-snapshot/1",
        "ledger-workflow-source-enrollment/1",
        "ledger-workflow-source-grant/1",
        "ledger-workflow-source-state/1",
        "ledger-workflow-customer-registration/1",
        "ledger-workflow-retained-evidence/1",
        "ledger-workflow-party-management-authority/1",
        "ledger-workflow-party-authorization/1",
        "ledger-workflow-party-evidence/1",
        "ledger-workflow-party-evidence/2",
        "ledger-workflow-publication/1",
        "ledger-workflow-build-manifest/1",
        "ledger-workflow-policy/1",
        "ledger-workflow-policy/2",
        "ledger-workflow-policy/3",
        "ledger-workflow-policy/4",
        "ledger-workflow-product/1",
        "ledger-workflow-fact-schema/1",
        "ledger-workflow-slot-template/1",
        "ledger-workflow-compiler-descriptor/1",
        "ledger-workflow-compiler-descriptor/2",
        "ledger-workflow-compiler-descriptor/3",
        "ledger-workflow-compiler-descriptor/4",
        "ledger-workflow-compiler-descriptor/5",
        "ledger-workflow-compiler-descriptor/6",
        "ledger-workflow-compiler-descriptor/7",
        "ledger-workflow-compiled-plan/1",
        "ledger-workflow-compiled-plan/2",
        "ledger-workflow-compiled-plan/3",
        "ledger-workflow-compiled-plan/4",
        "ledger-workflow-compiled-plan/5",
        "ledger-workflow-compiled-plan/6",
        "ledger-workflow-compiled-plan/7",
        "ledger-workflow-binding/1",
        "ledger-workflow-binding/2",
        "ledger-workflow-acceptance-terms/1",
        "ledger-workflow-acceptance-terms/2",
        "ledger-workflow-agreement-intent/1",
        "ledger-workflow-accepted-agreement/1",
        "ledger-workflow-head-transition/1",
        "ledger-workflow-role-assignment/1",
        "ledger-workflow-slot-catalog/1",
        "ledger-workflow-slot-instantiation/1",
        "ledger-workflow-slot-binding-terms/1",
        "ledger-workflow-slot-binding-terms/2",
        "ledger-workflow-slot-binding-terms/3",
        "ledger-workflow-slot-binding-terms/7",
        "ledger-workflow-assent/1",
        "ledger-workflow-party-assent/1",
        "ledger-workflow-party-assent/2",
        "ledger-workflow-app-consent-verification/1",
        "ledger-workflow-app-consent-verification/2",
        "ledger-workflow-agreement-authority/2",
        "ledger-workflow-agreement-authority/5",
        "ledger-workflow-activation-transition/1",
    ] {
        if equal(schema, expected, s)? {
            allowed = true;
            break;
        }
    }
    if !allowed {
        return Err(ServiceError::Unavailable);
    }
    if equal(schema, "ledger-workflow-party-evidence/2", s)? {
        emit(request_wrapper::field(v, "authority_ref", s)?, s)?;
        let metadata = request_wrapper::field(v, "metadata", s)?;
        let purpose = request_wrapper::text(metadata, "purpose", s)?;
        if equal(purpose, "party_consent", s)? {
            let consent = request_wrapper::field(metadata, "consent", s)?;
            emit(request_wrapper::field(consent, "authorization_ref", s)?, s)?;
        } else {
            request_wrapper::same_text(purpose, "party_operator_attestation", s)?;
            emit(
                request_wrapper::field(metadata, "target_evidence_ref", s)?,
                s,
            )?;
        }
    } else if equal(schema, "ledger-workflow-binding/1", s)?
        || equal(schema, "ledger-workflow-binding/2", s)?
    {
        binding(v, emit, s)?;
    } else if equal(schema, "ledger-workflow-acceptance-terms/1", s)?
        || equal(schema, "ledger-workflow-acceptance-terms/2", s)?
    {
        for (name, v) in v.as_object().ok_or(ServiceError::IntegrityFailure)? {
            s.semantic_work().visit(Visit::Field).map_err(core_error)?;
            if equal(name, "binding_inputs", s)? {
                binding(v, emit, s)?;
            } else {
                generic(v, emit, s)?;
            }
        }
    } else if equal(schema, "ledger-workflow-agreement-intent/1", s)? {
        for (name, v) in v.as_object().ok_or(ServiceError::IntegrityFailure)? {
            s.semantic_work().visit(Visit::Field).map_err(core_error)?;
            if equal(name, "payload", s)? {
                payload(v, emit, s)?;
            } else {
                generic(v, emit, s)?;
            }
        }
    } else {
        generic(v, emit, s)?;
    }
    Ok(())
}
