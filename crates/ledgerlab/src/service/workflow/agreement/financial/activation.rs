//! Owning app activation payload projection through the existing activation
//! grammar and scalar normalizers. No schedule or source authority is granted.
use super::*;
mod transition_records;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::ExactRef,
    validation_work::{OriginalAuditSession, RepresentationKind, ValidationVisit as Visit},
};
pub(super) use transition_records::{ActivationRecords, build_records};

fn encode(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.canonical_representation(v, RepresentationKind::SourceDefinition)
        .map_err(core_error)
}
fn same(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "INVALID_REFERENCE")
}
fn references(
    v: &Value,
    kind: &str,
    min: usize,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    acceptance::reference_set(v, kind, min, 16, s).map_err(|e| match e {
        ServiceError::Rejection(code) if code == "DUPLICATE" => reject("INVALID_SHAPE"),
        other => other,
    })
}
fn effective(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let kind = request_wrapper::text(v, "kind", s)?;
    if acceptance::same_recipe_text(kind, "at_commit", s)? {
        request_wrapper::closed(v, &["kind"], s)?;
        return wire::object(&[("kind", b"\"at_commit\"")], 262144, s);
    }
    request_wrapper::same_text(kind, "not_before", s)?;
    request_wrapper::closed(v, &["kind", "at"], s)?;
    let _scratch = s
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    let at = Timestamp::parse_with_work(request_wrapper::text(v, "at", s)?, s.semantic_work())
        .map_err(core_error)?;
    let at = wire::string(at.as_str(), s)?;
    wire::object(
        &[("at", at.bytes()), ("kind", b"\"not_before\"")],
        262144,
        s,
    )
}
pub(super) fn normalize(
    kind: &str,
    payload: &Value,
    binding_ref: &ExactRef,
    binding: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let result = normalize_inner(kind, payload, binding_ref, binding, s);
    if result.is_err() {
        s.invalidate();
    }
    result
}
fn normalize_inner(
    kind: &str,
    payload: &Value,
    binding_ref: &ExactRef,
    binding: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let schema = request_wrapper::text(payload, "schema", s)?;
    let shape = activation_verify::shape::select(kind, schema, |a, b| {
        acceptance::same_recipe_text(a, b, s)
    })?;
    request_wrapper::closed(payload, shape.fields, s)?;
    request_wrapper::same_text(
        request_wrapper::text(binding, "schema", s)?,
        "ledger-workflow-binding/2",
        s,
    )?;
    request_wrapper::same_text(&binding_ref.kind, "workflow-binding", s)?;
    request_wrapper::same_text(
        request_wrapper::text(payload, "agreement_id", s)?,
        request_wrapper::text(binding, "agreement_id", s)?,
        s,
    )?;
    let supplied_ref = request_wrapper::field(payload, "accepted_binding_ref", s)?;
    acceptance::validate_reference(supplied_ref, "workflow-binding", s)?;
    let supplied_ref = encode(supplied_ref, s)?;
    let actual_ref = wire::reference(binding_ref, s)?;
    same(supplied_ref.bytes(), actual_ref.bytes(), s)?;
    let revoking = acceptance::same_recipe_text(shape.action, "revoke", s)?;
    let replacing = acceptance::same_recipe_text(shape.action, "replace", s)?;
    let evidence = references(
        request_wrapper::field(payload, "evidence_refs", s)?,
        "workflow-evidence",
        if revoking { 1 } else { 0 },
        s,
    )?;
    let _scalar = s
        .reserve_external_payload(20 + 4 * 64)
        .map_err(core_error)?;
    let revision = activation_verify::normalize_counter(
        request_wrapper::text(payload, "expected_revision", s)?,
        s.semantic_work(),
    )?;
    let revision = wire::string(&revision, s)?;
    let mut facts = None;
    let mut at = None;
    if revoking {
        acceptance::validate_reference(
            request_wrapper::field(payload, "transition_ref", s)?,
            "workflow-activation",
            s,
        )?;
        let reason = request_wrapper::text(payload, "reason", s)?;
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(reason.len())
            .map_err(core_error)?;
        let count = reason.chars().count();
        s.semantic_work()
            .arithmetic(
                ledgerlab_core::validation_work::ArithmeticOperation::Compare,
                &[&count.into(), &0usize.into()],
            )
            .map_err(core_error)?;
        require(count > 0, "INVALID_SHAPE")?;
        s.semantic_work()
            .arithmetic(
                ledgerlab_core::validation_work::ArithmeticOperation::Compare,
                &[&count.into(), &1024usize.into()],
            )
            .map_err(core_error)?;
        require(count <= 1024, "INVALID_SHAPE")?;
    } else {
        for (field, kind) in [
            ("policy_ref", "workflow-policy"),
            ("product_ref", "workflow-product"),
            ("compiled_plan_ref", "workflow-compiled-plan"),
        ] {
            let supplied = request_wrapper::field(payload, field, s)?;
            acceptance::validate_reference(supplied, kind, s)?;
            let supplied = encode(supplied, s)?;
            let expected = encode(request_wrapper::field(binding, field, s)?, s)?;
            same(supplied.bytes(), expected.bytes(), s)?;
        }
        let normalized = references(
            request_wrapper::field(payload, "fact_schema_refs", s)?,
            "workflow-fact-schema",
            1,
            s,
        )?;
        let expected = encode(request_wrapper::field(binding, "fact_schema_refs", s)?, s)?;
        same(normalized.bytes(), expected.bytes(), s)?;
        facts = Some(normalized);
        at = Some(effective(
            request_wrapper::field(payload, "effective", s)?,
            s,
        )?);
        if replacing {
            acceptance::validate_reference(
                request_wrapper::field(payload, "replaces_transition_ref", s)?,
                "workflow-activation",
                s,
            )?;
        }
    }
    // Maximum closed dictionary is the twelve-field replacement payload.
    let _control = s
        .reserve_external_payload(
            std::mem::size_of::<[Option<AuditedBytes>; 12]>()
                + std::mem::size_of::<[(&str, &[u8]); 12]>()
                + std::mem::size_of::<[Option<&[u8]>; 12]>(),
        )
        .map_err(core_error)?;
    let mut values: [Option<AuditedBytes>; 12] = std::array::from_fn(|_| None);
    let mut selected: [Option<&[u8]>; 12] = [None; 12];
    for (i, field) in shape.fields.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let replacement = if acceptance::same_recipe_text(field, "expected_revision", s)? {
            Some(revision.bytes())
        } else if acceptance::same_recipe_text(field, "evidence_refs", s)? {
            Some(evidence.bytes())
        } else if acceptance::same_recipe_text(field, "fact_schema_refs", s)? {
            facts.as_ref().map(|x| x.bytes())
        } else if acceptance::same_recipe_text(field, "effective", s)? {
            at.as_ref().map(|x| x.bytes())
        } else {
            None
        };
        s.semantic_work()
            .write_bytes(std::mem::size_of::<Option<&[u8]>>())
            .map_err(core_error)?;
        selected[i] = replacement;
        if replacement.is_none() {
            values[i] = Some(encode(request_wrapper::field(payload, field, s)?, s)?);
        }
    }
    let mut fields = [("", b"".as_slice()); 12];
    for (i, name) in shape.fields.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[i] = (
            name,
            match selected[i] {
                Some(bytes) => bytes,
                None => values[i]
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .bytes(),
            },
        );
    }
    ledgerlab_core::validation_work::try_sort_by(
        &mut fields[..shape.fields.len()],
        s.semantic_work(),
        |a, b, w| {
            w.visit(Visit::MemberCompare)?;
            w.scan_bytes(a.0.len())?;
            w.scan_bytes(b.0.len())?;
            Ok(a.0.cmp(b.0))
        },
    )
    .map_err(core_error)?;
    wire::object(&fields[..shape.fields.len()], 262144, s)
}

/// Check the actual accepted Binding interval before framing an admission.
/// This is one schedule predicate; predecessor/target/pending completeness is
/// still required by the enclosing private preparation.
pub(super) fn binding_interval(
    payload: &Value,
    binding: &Value,
    at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let result = (|| {
        let _time = s
            .reserve_external_payload(3 * (4 * 64 + 27))
            .map_err(core_error)?;
        let starts = Timestamp::parse_with_work(
            request_wrapper::text(binding, "starts_at", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        let ends = request_wrapper::optional_field(binding, "ends_at", s)?
            .map(|v| {
                s.semantic_work()
                    .visit(Visit::Constraint)
                    .map_err(core_error)?;
                Timestamp::parse_with_work(
                    v.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    s.semantic_work(),
                )
                .map_err(core_error)
            })
            .transpose()?;
        let effective = request_wrapper::field(payload, "effective", s)?;
        let at_commit = acceptance::same_recipe_text(
            request_wrapper::text(effective, "kind", s)?,
            "at_commit",
            s,
        )?;
        let deferred = if at_commit {
            None
        } else {
            Some(
                Timestamp::parse_with_work(
                    request_wrapper::text(effective, "at", s)?,
                    s.semantic_work(),
                )
                .map_err(core_error)?,
            )
        };
        let effective = deferred.as_ref().unwrap_or(at);
        activation_verify::schedule::binding_interval_with_work(
            effective,
            &starts,
            ends.as_ref(),
            s.semantic_work(),
        )
        .map_err(|error| match error {
            ServiceError::IntegrityFailure => reject("ACTIVATION_SCHEDULE_CONFLICT"),
            other => other,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owning_activation_projection_keeps_same_schedule_counter_and_ref_normalization() {
        let vectors: Value =
            serde_json::from_slice(include_bytes!("../testdata/descriptor-independent.json"))
                .unwrap();
        for name in ["A.activate", "B.activate"] {
            let op = vectors["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .unwrap();
            let mut envelope = op["request_evidence"]["normalized_request"].clone();
            envelope["payload"]["expected_revision"] = json!("000");
            envelope["payload"]["effective"] =
                json!({"kind":"not_before","at":"2026-10-13T00:00:20Z"});
            let p = &envelope["payload"];
            let body = json!({"schema":"ledger-workflow-binding/1","agreement_id":p["agreement_id"],"policy_ref":p["policy_ref"],"product_ref":p["product_ref"],"fact_schema_refs":p["fact_schema_refs"],"compiled_plan_ref":p["compiled_plan_ref"]});
            let old_binding = Document {
                reference: serde_json::from_value(p["accepted_binding_ref"].clone()).unwrap(),
                schema: "ledger-workflow-binding/1".into(),
                bytes: records::bytes(&body).unwrap(),
            };
            let req =
                request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
            let expected = activation_verify::normalize(&req, &old_binding).unwrap();
            let mut app_binding = body;
            app_binding["schema"] = json!("ledger-workflow-binding/2");
            let reference: ExactRef =
                serde_json::from_value(p["accepted_binding_ref"].clone()).unwrap();
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let actual =
                normalize("agreement.activate", p, &reference, &app_binding, &mut s).unwrap();
            assert_eq!(actual.bytes(), records::bytes(&expected).unwrap());
            drop(actual);
            for failure in 0..3 {
                let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
                let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let mut bad = p.clone();
                match failure {
                    0 => bad["effective"]["ignored"] = json!(true),
                    1 => {
                        bad["fact_schema_refs"] =
                            json!([bad["fact_schema_refs"][0], bad["fact_schema_refs"][0]])
                    }
                    _ => bad["expected_revision"] = json!("9223372036854775808"),
                }
                assert!(
                    normalize("agreement.activate", &bad, &reference, &app_binding, &mut s)
                        .is_err()
                );
            }
        }
    }
}
