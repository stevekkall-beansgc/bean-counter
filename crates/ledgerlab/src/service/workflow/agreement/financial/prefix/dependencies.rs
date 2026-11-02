//! Closed schema Ref edges for the SAME administrative/acceptance closure.
//! The caller supplies normalized or already completed bodies; this traversal
//! creates no custody and never classifies arbitrary business dictionaries as
//! authority. It always runs on the responsible origin's semantic work.
use super::*;
type Emit<'a> = dyn FnMut(&Value, &mut OriginalAuditSession) -> Result<()> + 'a;

/// Edges of the existing activation predecessor walk. Bodies have already
/// been authenticated by the enclosing completed inventory.
pub(super) fn predecessor(
    kind: &str,
    v: &Value,
    emit: &mut Emit<'_>,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(kind.len())
        .map_err(core_error)?;
    match kind {
        "workflow-head-transition" => {
            emit(request_wrapper::field(v, "accepted_binding_ref", s)?, s)?;
            if let Some(prior) = v.get("prior_head_ref") {
                emit(prior, s)?;
            }
        }
        "workflow-binding" => {
            for field in [
                "slot_catalog_ref",
                "slot_instantiation_ref",
                "policy_ref",
                "product_ref",
                "compiled_plan_ref",
            ] {
                emit(request_wrapper::field(v, field, s)?, s)?;
            }
            for field in [
                "roles_refs",
                "assent_refs",
                "authority_refs",
                "fact_schema_refs",
            ] {
                for r in request_wrapper::field(v, field, s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    emit(r, s)?;
                }
            }
            emit(
                request_wrapper::field(
                    request_wrapper::field(v, "accepted_context", s)?,
                    "fact_schema_ref",
                    s,
                )?,
                s,
            )?;
            for item in request_wrapper::field(v, "bindings", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                emit(request_wrapper::field(item, "binding_ref", s)?, s)?;
            }
            for leg in request_wrapper::field(v, "funding_legs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                for r in request_wrapper::field(leg, "evidence_refs", s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    emit(r, s)?;
                }
            }
        }
        "workflow-slot-instantiation" => {
            for field in ["template_ref", "catalog_ref"] {
                emit(request_wrapper::field(v, field, s)?, s)?;
            }
        }
        "workflow-slot-terms" => {
            emit(request_wrapper::field(v, "role_ref", s)?, s)?;
            for r in request_wrapper::field(v, "source_grant_refs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                emit(r, s)?;
            }
        }
        "workflow-assent" => {
            let schema = request_wrapper::text(v, "schema", s)?;
            if equal(schema, "ledger-workflow-assent/1", s)? {
                for field in ["evidence_ref", "operator_attestation_ref"] {
                    emit(request_wrapper::field(v, field, s)?, s)?;
                }
            } else if equal(schema, "ledger-workflow-party-assent/2", s)?
                || equal(schema, "ledger-workflow-party-assent/3", s)?
            {
                body(v, emit, s)?;
            } else {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        "workflow-roles" | "workflow-slot-catalog" => {}
        "workflow-compiled-plan" => {
            emit(request_wrapper::field(v, "compiler_ref", s)?, s)?;
            for r in request_wrapper::field(v, "source_refs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                emit(r, s)?;
            }
        }
        "workflow-acceptance-terms" => body(v, emit, s)?,
        "workflow-publication" => {
            request_wrapper::same_text(
                request_wrapper::text(v, "schema", s)?,
                "ledger-workflow-publication/1",
                s,
            )?;
            body(v, emit, s)?;
        }
        "workflow-policy" => {
            for field in [
                "product_ref",
                "context_schema_ref",
                "slot_template_ref",
                "compiler_ref",
            ] {
                emit(request_wrapper::field(v, field, s)?, s)?;
            }
        }
        "workflow-product" => {
            for r in request_wrapper::field(v, "fact_schemas", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                emit(r, s)?;
            }
        }
        "workflow-slot-template" | "workflow-fact-schema" | "workflow-compiler-descriptor" => {}
        "workflow-authority" => {
            let schema = request_wrapper::text(v, "schema", s)?;
            if equal(schema, "ledger-workflow-party-authorization/1", s)?
                || equal(schema, "ledger-workflow-party-management-authority/1", s)?
            {
                body(v, emit, s)?;
            } else {
                request_wrapper::same_text(
                    schema,
                    "ledger-workflow-local-authority-snapshot/1",
                    s,
                )?;
            }
        }
        "workflow-evidence" => {
            let schema = request_wrapper::text(v, "schema", s)?;
            if equal(schema, "ledger-workflow-party-evidence/1", s)?
                || equal(schema, "ledger-workflow-party-evidence/2", s)?
                || equal(schema, "ledger-workflow-party-evidence/3", s)?
            {
                body(v, emit, s)?;
            } else {
                request_wrapper::same_text(schema, "ledger-workflow-retained-evidence/1", s)?;
                emit(request_wrapper::field(v, "authority_ref", s)?, s)?;
                let purpose = request_wrapper::text(v, "purpose", s)?;
                if equal(purpose, "operator_attestation", s)? {
                    emit(
                        request_wrapper::field(
                            request_wrapper::field(v, "metadata", s)?,
                            "assent_evidence_ref",
                            s,
                        )?,
                        s,
                    )?;
                } else {
                    require(!equal(purpose, "admission_stop", s)?, "INTEGRITY_FAILURE")?;
                }
            }
        }
        "workflow-customer" => {
            for field in ["evidence_ref", "authority_ref"] {
                emit(request_wrapper::field(v, field, s)?, s)?;
            }
        }
        "workflow-source-enrollment" => {
            for r in request_wrapper::field(v, "customer_refs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                emit(r, s)?;
            }
            for field in ["evidence_ref", "authority_ref"] {
                emit(request_wrapper::field(v, field, s)?, s)?;
            }
        }
        "workflow-source-grant" => emit(request_wrapper::field(v, "original_ceiling_ref", s)?, s)?,
        "workflow-source-state" => {
            emit(request_wrapper::field(v, "enrollment_ref", s)?, s)?;
            for field in ["current_grant_ref", "prior_state_ref"] {
                if let Some(r) = v.get(field) {
                    emit(r, s)?;
                }
            }
        }
        _ => return Err(ServiceError::IntegrityFailure),
    }
    Ok(())
}

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
        "ledger-workflow-retained-evidence/2",
        "ledger-workflow-party-management-authority/1",
        "ledger-workflow-party-authorization/1",
        "ledger-workflow-party-evidence/1",
        "ledger-workflow-party-evidence/2",
        "ledger-workflow-party-evidence/3",
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
        "ledger-workflow-compiler-descriptor/8",
        "ledger-workflow-compiled-plan/1",
        "ledger-workflow-compiled-plan/2",
        "ledger-workflow-compiled-plan/3",
        "ledger-workflow-compiled-plan/4",
        "ledger-workflow-compiled-plan/5",
        "ledger-workflow-compiled-plan/6",
        "ledger-workflow-compiled-plan/7",
        "ledger-workflow-compiled-plan/8",
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
        "ledger-workflow-agreement-authority/6",
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
    if equal(schema, "ledger-workflow-party-evidence/2", s)?
        || equal(schema, "ledger-workflow-party-evidence/3", s)?
    {
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

#[cfg(test)]
mod predecessor_tests {
    use super::*;
    #[test]
    fn stop_predecessor_keeps_closed_management_and_publication_edges() {
        let reference =
            |kind: &str, name: &str| serde_json::json!({"kind":kind,"id":name,"digest":name});
        let evidence = reference("workflow-evidence", "evidence");
        let admin = reference("workflow-authority", "admin");
        let compiler = reference("workflow-compiler-descriptor", "compiler");
        let plan = reference("workflow-compiled-plan", "plan");
        let policy = reference("workflow-policy", "policy");
        let management = serde_json::json!({"schema":"ledger-workflow-party-management-authority/1","evidence_refs":[evidence],"local_admin_ref":admin});
        let publication = serde_json::json!({"schema":"ledger-workflow-publication/1","compiled_plan_ref":plan,"compiler_ref":compiler,"root_policy_ref":policy,"source_refs":[compiler,policy],"provenance":{"binary_sha256":"digest","build_manifest_digest":"digest","source_sha":"source"}});
        for (kind, value, expected) in [
            (
                "workflow-authority",
                management.clone(),
                vec![evidence, admin],
            ),
            (
                "workflow-publication",
                publication.clone(),
                vec![plan, compiler.clone(), policy.clone(), compiler, policy],
            ),
            (
                "workflow-authority",
                serde_json::json!({"schema":"ledger-workflow-local-authority-snapshot/1"}),
                vec![],
            ),
        ] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut actual = Vec::new();
            predecessor(
                kind,
                &value,
                &mut |v, _| {
                    actual.push(v.clone());
                    Ok(())
                },
                &mut session,
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
        for (kind, mut value, schema) in [
            (
                "workflow-authority",
                management,
                "ledger-workflow-party-management-authority/2",
            ),
            (
                "workflow-publication",
                publication.clone(),
                "ledger-workflow-publication/2",
            ),
            (
                "workflow-publication",
                publication.clone(),
                "ledger-workflow-party-management-authority/1",
            ),
            ("unknown-kind", publication, "ledger-workflow-publication/1"),
        ] {
            value["schema"] = serde_json::json!(schema);
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut actual = Vec::new();
            assert!(predecessor(
                kind,
                &value,
                &mut |v, _| {
                    actual.push(v.clone());
                    Ok(())
                },
                &mut session
            )
            .is_err());
            assert!(actual.is_empty());
        }
    }
    #[test]
    fn head_walk_uses_only_binding_and_prior_head() {
        let binding =
            serde_json::json!({"kind":"workflow-binding","id":"binding","digest":"digest"});
        let prior =
            serde_json::json!({"kind":"workflow-head-transition","id":"prior","digest":"digest"});
        let ignored =
            serde_json::json!({"kind":"workflow-evidence","id":"ignored","digest":"digest"});
        let body = serde_json::json!({"accepted_binding_ref":binding,"prior_head_ref":prior,"other":ignored});
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut actual = Vec::new();
        predecessor(
            "workflow-head-transition",
            &body,
            &mut |v, _| {
                actual.push(v.clone());
                Ok(())
            },
            &mut session,
        )
        .unwrap();
        assert_eq!(
            actual,
            vec![
                body["accepted_binding_ref"].clone(),
                body["prior_head_ref"].clone()
            ]
        );
    }
    #[test]
    fn selected_policy_predecessor_uses_four_definition_refs() {
        let fixture: Value = serde_json::from_slice(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/candidates/full-suite-v1/core/party-commitment-first-01/fixture-input.json"))).unwrap();
        fn find_policy(v: &Value) -> Option<&Value> {
            if v.get("schema").and_then(Value::as_str) == Some("ledger-workflow-policy/4") {
                return Some(v);
            }
            match v {
                Value::Array(a) => a.iter().find_map(find_policy),
                Value::Object(o) => o.values().find_map(find_policy),
                _ => None,
            }
        }
        let policy = find_policy(&fixture).unwrap();
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut actual = Vec::new();
        predecessor(
            "workflow-policy",
            policy,
            &mut |v, _| {
                actual.push(v.clone());
                Ok(())
            },
            &mut session,
        )
        .unwrap();
        assert_eq!(
            actual,
            [
                "product_ref",
                "context_schema_ref",
                "slot_template_ref",
                "compiler_ref"
            ]
            .map(|name| policy[name].clone())
        );
    }
}
