//! SAME original source-administration input recipe with owning normalization.
//! Exact normalized intent remains DATA until complete root, result/lineage,
//! earliest-introduction and as-of active-head checks finish chronologically.
use super::super::{acceptance, ingress, wire};
use super::*;
use ledgerlab_core::{canonical::audit_arena::AuditedBytes, policy::workflow::OwnedSourceCeiling};

pub(in super::super) struct SourceRequest {
    payload: AuditedBytes,
    ceiling: Option<OwnedSourceCeiling>,
}
impl SourceRequest {
    pub(in super::super) fn normalized_payload(&self) -> &[u8] {
        self.payload.bytes()
    }
    pub(in super::super) fn ceiling(&self) -> Option<&Value> {
        self.ceiling
            .as_ref()
            .map(OwnedSourceCeiling::canonical_value)
    }
}
fn same(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn encode(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.canonical_representation(v, RepresentationKind::SourceDefinition)
        .map_err(core_error)
}
fn value(v: &Value, name: &str, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    encode(request_wrapper::field(v, name, s)?, s)
}
fn revision(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let raw = request_wrapper::text(v, "expected_revision", s)?;
    let n = acceptance::counter(raw, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &0u64.into()])
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(n > 0, "INVALID_SHAPE")?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &i64::MAX.into()])
        .map_err(core_error)?;
    require(n <= i64::MAX as u64, "INVALID_SHAPE")?;
    s.semantic_work().scan_bytes(1).map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(!raw.starts_with('0'), "INVALID_SHAPE")
}
pub(in super::super) fn normalize(
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<SourceRequest> {
    let result = normalize_inner(payload, s);
    if result.is_err() {
        s.invalidate();
    }
    result
}
fn normalize_inner(p: &Value, s: &mut OriginalAuditSession) -> Result<SourceRequest> {
    let schema = request_wrapper::text(p, "schema", s)?;
    if same(schema, "ledger-workflow-source-enroll/1", s)? {
        request_wrapper::closed(
            p,
            &[
                "schema",
                "action",
                "expected_revision",
                "enrollment_id",
                "customer_refs",
                "ceiling",
                "evidence_ref",
            ],
            s,
        )?;
        request_wrapper::same_text(request_wrapper::text(p, "action", s)?, "source.enroll", s)?;
        request_wrapper::same_text(request_wrapper::text(p, "expected_revision", s)?, "0", s)?;
        ingress::identifier(request_wrapper::text(p, "enrollment_id", s)?, 128, s)?;
        acceptance::validate_reference(
            request_wrapper::field(p, "evidence_ref", s)?,
            "workflow-evidence",
            s,
        )?;
        let customers = acceptance::reference_set(
            request_wrapper::field(p, "customer_refs", s)?,
            "workflow-customer",
            1,
            64,
            s,
        )?;
        let ceiling = OwnedSourceCeiling::normalize(request_wrapper::field(p, "ceiling", s)?, s)
            .map_err(core_error)?;
        let action = value(p, "action", s)?;
        let id = value(p, "enrollment_id", s)?;
        let evidence = value(p, "evidence_ref", s)?;
        let payload = wire::object(
            &[
                ("action", action.bytes()),
                ("ceiling", ceiling.canonical_bytes()),
                ("customer_refs", customers.bytes()),
                ("enrollment_id", id.bytes()),
                ("evidence_ref", evidence.bytes()),
                ("expected_revision", b"\"0\""),
                ("schema", b"\"ledger-workflow-source-enroll/1\""),
            ],
            262144,
            s,
        )?;
        Ok(SourceRequest {
            payload,
            ceiling: Some(ceiling),
        })
    } else if same(schema, "ledger-workflow-source-restrict/1", s)? {
        request_wrapper::closed(
            p,
            &[
                "schema",
                "action",
                "enrollment_ref",
                "expected_revision",
                "replacement_ceiling",
            ],
            s,
        )?;
        request_wrapper::same_text(request_wrapper::text(p, "action", s)?, "source.restrict", s)?;
        revision(p, s)?;
        acceptance::validate_reference(
            request_wrapper::field(p, "enrollment_ref", s)?,
            "workflow-source-enrollment",
            s,
        )?;
        let ceiling =
            OwnedSourceCeiling::normalize(request_wrapper::field(p, "replacement_ceiling", s)?, s)
                .map_err(core_error)?;
        let action = value(p, "action", s)?;
        let enrollment = value(p, "enrollment_ref", s)?;
        let revision = value(p, "expected_revision", s)?;
        let payload = wire::object(
            &[
                ("action", action.bytes()),
                ("enrollment_ref", enrollment.bytes()),
                ("expected_revision", revision.bytes()),
                ("replacement_ceiling", ceiling.canonical_bytes()),
                ("schema", b"\"ledger-workflow-source-restrict/1\""),
            ],
            262144,
            s,
        )?;
        Ok(SourceRequest {
            payload,
            ceiling: Some(ceiling),
        })
    } else {
        request_wrapper::same_text(schema, "ledger-workflow-source-revoke/1", s)?;
        request_wrapper::closed(
            p,
            &[
                "schema",
                "action",
                "enrollment_ref",
                "expected_revision",
                "reason",
            ],
            s,
        )?;
        request_wrapper::same_text(request_wrapper::text(p, "action", s)?, "source.revoke", s)?;
        revision(p, s)?;
        acceptance::validate_reference(
            request_wrapper::field(p, "enrollment_ref", s)?,
            "workflow-source-enrollment",
            s,
        )?;
        let reason = request_wrapper::text(p, "reason", s)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&reason.len().into(), &1024usize.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&reason.len().into(), &0usize.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(!reason.is_empty() && reason.len() <= 1024, "INVALID_SHAPE")?;
        let action = value(p, "action", s)?;
        let enrollment = value(p, "enrollment_ref", s)?;
        let revision = value(p, "expected_revision", s)?;
        let reason = value(p, "reason", s)?;
        let payload = wire::object(
            &[
                ("action", action.bytes()),
                ("enrollment_ref", enrollment.bytes()),
                ("expected_revision", revision.bytes()),
                ("reason", reason.bytes()),
                ("schema", b"\"ledger-workflow-source-revoke/1\""),
            ],
            262144,
            s,
        )?;
        Ok(SourceRequest {
            payload,
            ceiling: None,
        })
    }
}
/// Reproduce from the literal ORIGINAL raw body; saved normalized request is
/// solely the comparison target, never the normalization input.
pub(in super::super) fn original_request(
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<SourceRequest> {
    let result = (|| {
        request_wrapper::same_text(
            request_wrapper::text(origin.wrappers().operation(), "kind", s)?,
            "authority.change",
            s,
        )?;
        let literal =
            request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
        let result = normalize(literal, s)?;
        let evidence =
            request_wrapper::field(origin.wrappers().operation(), "request_evidence", s)?;
        let normalized = request_wrapper::field(evidence, "normalized_request", s)?;
        let expected = encode(request_wrapper::field(normalized, "payload", s)?, s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(result.payload.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        require(
            result.payload.bytes() == expected.bytes(),
            "INTEGRITY_FAILURE",
        )?;
        let _evidence = super::super::evidence::original_administration(
            origin,
            result.normalized_payload(),
            s,
        )?;
        Ok(result)
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
    fn registration_and_source_evidence_preserve_existing_r1_domains_and_literal_raw() {
        let reference = |kind: &str, digit: char| serde_json::json!({"kind":kind,"id":format!("wdoc_{}",digit.to_string().repeat(64)),"digest":format!("sha256:{}",digit.to_string().repeat(64))});
        let registration = serde_json::json!({"schema":"ledger-workflow-customer-register/1","action":"customer.register","expected_revision":"0","customer":"customer-é","acceptor":"acceptor-é","registration_evidence_ref":reference("workflow-evidence", 'c')});
        let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/agreement-ready-fixture-input.json"
        )))
        .unwrap();
        let mut enroll = fixture["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["payload"]["action"] == "source.enroll")
            .unwrap()["payload"]
            .clone();
        enroll["customer_refs"] = serde_json::json!([
            reference("workflow-customer", 'b'),
            reference("workflow-customer", 'a')
        ]);
        enroll["evidence_ref"] = reference("workflow-evidence", 'c');
        let enrollment = reference("workflow-source-enrollment", 'd');
        let restrict = serde_json::json!({"schema":"ledger-workflow-source-restrict/1","action":"source.restrict","enrollment_ref":enrollment,"expected_revision":"1","replacement_ceiling":enroll["ceiling"]});
        let revoke = serde_json::json!({"schema":"ledger-workflow-source-revoke/1","action":"source.revoke","enrollment_ref":enrollment,"expected_revision":"2","reason":"keep original\nreason"});
        for (index, payload) in [registration, enroll, restrict, revoke]
            .into_iter()
            .enumerate()
        {
            let raw = serde_json::to_vec_pretty(&serde_json::json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"authority.change","key":{"logical_store_id":"test-store","scope":["test","local"],"source":"source","operation_id":"saved-original"},"payload":payload})).unwrap();
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let req = ingress::IncomingRequest::parse_original_administration(&raw, &mut session)
                .unwrap();
            let input = req.payload(&mut session).unwrap();
            let normalized = if index == 0 {
                registration_payload(input, &mut session).unwrap()
            } else {
                normalize(input, &mut session).unwrap().payload
            };
            let legacy =
                crate::service::workflow::agreement::request::Request::parse(&raw).unwrap();
            let normalized_value: Value = serde_json::from_slice(normalized.bytes()).unwrap();
            let expected =
                crate::service::workflow::agreement::journal::evidence(&legacy, &normalized_value)
                    .unwrap();
            let actual = super::super::super::evidence::administration(
                &req,
                normalized.bytes(),
                &mut session,
            )
            .unwrap();
            assert_eq!(actual.value(), &expected, "case {index}");
            if index == 0 {
                let root = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
                    kind: "workflow-authority".into(),
                    id: format!("wdoc_{}", "e".repeat(64)),
                    digest: format!("sha256:{}", "e".repeat(64)),
                };
                let at = Timestamp::parse("2026-10-17T00:00:00Z").unwrap();
                let generated = registration_record(
                    input,
                    &serde_json::json!(["test", "local"]),
                    "owner",
                    &at,
                    &root,
                    &mut session,
                )
                .unwrap();
                let expected = crate::service::workflow::records::source_document("workflow-customer", serde_json::json!({"schema":"ledger-workflow-customer-registration/1","scope":["test","local"],"customer":input["customer"],"acceptor":input["acceptor"],"evidence_ref":input["registration_evidence_ref"],"registered_by":"owner","registered_at":at,"authority_ref":root,"installation_mode":"real_local"})).unwrap();
                assert_eq!(generated.bytes(), expected.bytes);
                assert_eq!(generated.reference().kind, expected.reference.kind);
                assert_eq!(generated.reference().id, expected.reference.id);
                assert_eq!(generated.reference().digest, expected.reference.digest);
            }
        }
    }
    #[test]
    fn literal_r1_evidence_reproduces_six_original_records_and_request_commitments() {
        let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/agreement-03-primary-author.json"
        )))
        .unwrap();
        let root = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
            kind: "workflow-authority".into(),
            id: format!("wdoc_{}", "e".repeat(64)),
            digest: format!("sha256:{}", "e".repeat(64)),
        };
        let at = Timestamp::parse("2026-10-17T00:00:00Z").unwrap();
        let mut purposes = std::collections::BTreeSet::new();
        for row in fixture["operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["normalized_request"]["kind"] == "evidence.retain")
        {
            let p = &row["normalized_request"]["payload"];
            let raw = serde_json::to_vec_pretty(&serde_json::json!({"schema":"ledger-operation/1","contract":"workflow/1","kind":"evidence.retain","key":{"logical_store_id":"test-store","scope":["test","local"],"source":"source","operation_id":"saved-retention"},"payload":p})).unwrap();
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let req = ingress::IncomingRequest::parse_original_administration(&raw, &mut session)
                .unwrap();
            let normalized = ledgerlab_core::policy::workflow::OwnedEvidenceInput::normalize(
                req.payload(&mut session).unwrap(),
                &mut session,
            )
            .unwrap();
            let legacy =
                crate::service::workflow::agreement::request::Request::parse(&raw).unwrap();
            let expected = crate::service::workflow::agreement::journal::evidence(
                &legacy,
                normalized.canonical_value(),
            )
            .unwrap();
            let actual = super::super::super::evidence::administration(
                &req,
                normalized.canonical_bytes(),
                &mut session,
            )
            .unwrap();
            assert_eq!(actual.value(), &expected);
            let generated = retention_record(
                normalized.canonical_value(),
                "owner",
                &at,
                &root,
                &mut session,
            )
            .unwrap();
            let mut body = normalized.canonical_value().clone();
            body["schema"] = "ledger-workflow-retained-evidence/1".into();
            body["retained_by"] = "owner".into();
            body["retained_at"] = serde_json::json!(at);
            body["authority_ref"] = serde_json::json!(root);
            body["installation_mode"] = "real_local".into();
            let expected =
                crate::service::workflow::records::source_document("workflow-evidence", body)
                    .unwrap();
            assert_eq!(generated.bytes(), expected.bytes);
            assert_eq!(generated.reference().id, expected.reference.id);
            assert_eq!(generated.reference().digest, expected.reference.digest);
            purposes.insert(p["purpose"].as_str().unwrap());
        }
        assert_eq!(purposes.len(), 6);
    }
    #[test]
    fn original_source_request_normalization_matches_existing_r1_for_all_three_transitions() {
        let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/agreement-ready-fixture-input.json"
        )))
        .unwrap();
        let mut enrollment = fixture["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["payload"]["action"] == "source.enroll")
            .unwrap()["payload"]
            .clone();
        enrollment["customer_refs"] = serde_json::json!([{"kind":"workflow-customer","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))}]);
        enrollment["evidence_ref"] = serde_json::json!({"kind":"workflow-evidence","id":format!("wdoc_{}","c".repeat(64)),"digest":format!("sha256:{}","c".repeat(64))});
        let reference = serde_json::json!({"kind":"workflow-source-enrollment","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let restrict = serde_json::json!({"schema":"ledger-workflow-source-restrict/1","action":"source.restrict","enrollment_ref":reference,"expected_revision":"1","replacement_ceiling":enrollment["ceiling"]});
        let revoke = serde_json::json!({"schema":"ledger-workflow-source-revoke/1","action":"source.revoke","enrollment_ref":reference,"expected_revision":"2","reason":"actual\nreason"});
        let mut invalid = restrict.clone();
        invalid["expected_revision"] = "01".into();
        for (index, p) in [enrollment, restrict, revoke, invalid]
            .into_iter()
            .enumerate()
        {
            let legacy = crate::service::workflow::agreement::source::normalize(&p);
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let baseline = arena.usage().unwrap().live_bytes;
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let current = normalize(&p, &mut session);
            assert_eq!(legacy.is_ok(), index < 3, "legacy case {index}");
            assert_eq!(legacy.is_ok(), current.is_ok(), "owning case {index}");
            if let (Ok(legacy), Ok(current)) = (legacy, current) {
                assert_eq!(
                    serde_json::from_slice::<Value>(current.normalized_payload()).unwrap(),
                    legacy
                );
                drop(current);
            }
            assert_eq!(baseline, arena.usage().unwrap().live_bytes);
        }
    }
}

/// SAME R1 customer-register input. No SourceCeiling or current source is
/// inferred from an administrative registration request.
fn registration_payload(p: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    request_wrapper::closed(
        p,
        &[
            "schema",
            "action",
            "expected_revision",
            "customer",
            "acceptor",
            "registration_evidence_ref",
        ],
        s,
    )?;
    for (field, expected) in [
        ("schema", "ledger-workflow-customer-register/1"),
        ("action", "customer.register"),
        ("expected_revision", "0"),
    ] {
        request_wrapper::same_text(request_wrapper::text(p, field, s)?, expected, s)?;
    }
    for field in ["customer", "acceptor"] {
        ingress::identifier(request_wrapper::text(p, field, s)?, 128, s)?;
    }
    acceptance::validate_reference(
        request_wrapper::field(p, "registration_evidence_ref", s)?,
        "workflow-evidence",
        s,
    )?;
    // The normalized shape contains no decimal/time/set field. SAME JCS over
    // the checked borrowed fields preserves the legacy Registration DTO bytes.
    encode(p, s)
}
/// A recomputed permanent registration is DATA until chronological absence,
/// evidence/root custody and earliest-introduction checks have all completed.
pub(in super::super) struct CustomerRegistration<'a> {
    payload: AuditedBytes,
    management: Management<'a>,
    evidence: current::source::SourceRecord,
    registration: wire::SourceRecord,
}
fn registration_record(
    p: &Value,
    scope: &Value,
    actor: &str,
    at: &Timestamp,
    root: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let acceptor = value(p, "acceptor", s)?;
    let customer = value(p, "customer", s)?;
    let evidence = value(p, "registration_evidence_ref", s)?;
    let actor = wire::string(actor, s)?;
    let at = wire::string(at.as_str(), s)?;
    let root = wire::reference(root, s)?;
    let scope = encode(scope, s)?;
    let body = wire::object(
        &[
            ("acceptor", acceptor.bytes()),
            ("authority_ref", root.bytes()),
            ("customer", customer.bytes()),
            ("evidence_ref", evidence.bytes()),
            ("installation_mode", b"\"real_local\""),
            ("registered_at", at.bytes()),
            ("registered_by", actor.bytes()),
            ("schema", b"\"ledger-workflow-customer-registration/1\""),
            ("scope", scope.bytes()),
        ],
        262144,
        s,
    )?;
    wire::source_record(
        "workflow-customer",
        "ledger-workflow-customer-registration/1",
        body,
        s,
    )
}
pub(in super::super) async fn reconstruct_registration<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<CustomerRegistration<'a>> {
    let result = registration_inner(tx, origin, s).await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}
async fn registration_inner<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<CustomerRegistration<'a>> {
    let operation = origin.wrappers().operation();
    request_wrapper::same_text(
        request_wrapper::text(operation, "kind", s)?,
        "authority.change",
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
        "administrative",
        s,
    )?;
    let literal =
        request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
    let payload = registration_payload(literal, s)?;
    let saved = request_wrapper::field(
        request_wrapper::field(operation, "request_evidence", s)?,
        "normalized_request",
        s,
    )?;
    let saved = value(saved, "payload", s)?;
    same_bytes(payload.bytes(), saved.bytes(), s)?;
    let _evidence = super::super::evidence::original_administration(origin, payload.bytes(), s)?;
    let p = s
        .parse_representation(payload.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let p = p.value();
    let management = management(tx, origin, s).await?;
    let evidence_ref = request_wrapper::field(p, "registration_evidence_ref", s)?;
    origin.require_member(evidence_ref, s)?;
    let evidence = current::source::retained_record(
        tx,
        origin.key(),
        evidence_ref,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1",
        origin.wrappers().sequence(),
        s,
    )
    .await?;
    let proof = evidence.body();
    // Evidence's full literal producer is an earlier chronological obligation;
    // these are the SAME registration-specific consumer checks from R1.
    request_wrapper::same_text(
        request_wrapper::text(proof, "purpose", s)?,
        "customer_registration",
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(proof, "installation_mode", s)?,
        "real_local",
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(request_wrapper::field(proof, "subject", s)?, "customer", s)?,
        request_wrapper::text(p, "customer", s)?,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(request_wrapper::field(proof, "metadata", s)?, "acceptor", s)?,
        request_wrapper::text(p, "acceptor", s)?,
        s,
    )?;
    let scope = request_wrapper::field(request_wrapper::field(operation, "key", s)?, "scope", s)?;
    let registration = registration_record(
        p,
        scope,
        management.actor,
        &management.at,
        management.root.reference(),
        s,
    )?;
    let results =
        wire::RefSet::new([registration.reference()].into_iter(), 1, true, s)?.encode(s)?;
    let expected = value(operation, "result_refs", s)?;
    same_bytes(results.bytes(), expected.bytes(), s)?;
    let reference = wire::reference(registration.reference(), s)?;
    let reference = s
        .parse_representation(reference.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    origin.require_member(reference.value(), s)?;
    let stored = current::source::retained_record(
        tx,
        origin.key(),
        reference.value(),
        "workflow-customer",
        "ledger-workflow-customer-registration/1",
        management.own_before,
        s,
    )
    .await?;
    same_bytes(stored.canonical_body(), registration.bytes(), s)?;
    Ok(CustomerRegistration {
        payload,
        management,
        evidence,
        registration,
    })
}

/// Literal R1 retained evidence and exact result, with its immediate earlier
/// operands still owning. No retention truth, consent or completed origin is
/// inferred from the purpose or from the declared bytes.
pub(in super::super) struct RetainedEvidence<'a> {
    input: ledgerlab_core::policy::workflow::OwnedEvidenceInput,
    management: Management<'a>,
    dependencies: [Option<current::source::SourceRecord>; 2],
    result: wire::SourceRecord,
}
fn retention_record(
    p: &Value,
    actor: &str,
    at: &Timestamp,
    root: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let content = value(p, "content_base64", s)?;
    let hash = value(p, "content_sha256", s)?;
    let media = value(p, "media_type", s)?;
    let metadata = value(p, "metadata", s)?;
    let purpose = value(p, "purpose", s)?;
    let subject = value(p, "subject", s)?;
    let actor = wire::string(actor, s)?;
    let at = wire::string(at.as_str(), s)?;
    let root = wire::reference(root, s)?;
    let body = wire::object(
        &[
            ("authority_ref", root.bytes()),
            ("content_base64", content.bytes()),
            ("content_sha256", hash.bytes()),
            ("installation_mode", b"\"real_local\""),
            ("media_type", media.bytes()),
            ("metadata", metadata.bytes()),
            ("purpose", purpose.bytes()),
            ("retained_at", at.bytes()),
            ("retained_by", actor.bytes()),
            ("schema", b"\"ledger-workflow-retained-evidence/1\""),
            ("subject", subject.bytes()),
        ],
        262144,
        s,
    )?;
    wire::source_record(
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1",
        body,
        s,
    )
}
pub(in super::super) async fn reconstruct_retention<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<RetainedEvidence<'a>> {
    let result = retention_inner(tx, origin, s).await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}
async fn retention_inner<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<RetainedEvidence<'a>> {
    let operation = origin.wrappers().operation();
    request_wrapper::same_text(
        request_wrapper::text(operation, "kind", s)?,
        "evidence.retain",
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
        "administrative",
        s,
    )?;
    let literal =
        request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
    let input = ledgerlab_core::policy::workflow::OwnedEvidenceInput::normalize(literal, s)
        .map_err(core_error)?;
    let p = input.canonical_value();
    let saved = request_wrapper::field(
        request_wrapper::field(operation, "request_evidence", s)?,
        "normalized_request",
        s,
    )?;
    let saved = value(saved, "payload", s)?;
    same_bytes(input.canonical_bytes(), saved.bytes(), s)?;
    let _evidence =
        super::super::evidence::original_administration(origin, input.canonical_bytes(), s)?;
    let management = management(tx, origin, s).await?;
    let mut dependencies = [None, None];
    let purpose = request_wrapper::text(p, "purpose", s)?;
    let metadata = request_wrapper::field(p, "metadata", s)?;
    if same(purpose, "operator_attestation", s)? {
        let reference = request_wrapper::field(metadata, "assent_evidence_ref", s)?;
        origin.require_member(reference, s)?;
        let prior = current::source::retained_record(
            tx,
            origin.key(),
            reference,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
            origin.wrappers().sequence(),
            s,
        )
        .await?;
        let body = prior.body();
        request_wrapper::same_text(request_wrapper::text(body, "purpose", s)?, "assent", s)?;
        request_wrapper::same_text(
            request_wrapper::text(body, "retained_by", s)?,
            management.actor,
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(body, "installation_mode", s)?,
            "real_local",
            s,
        )?;
        let subject = value(p, "subject", s)?;
        let prior_subject = value(body, "subject", s)?;
        same_bytes(subject.bytes(), prior_subject.bytes(), s)?;
        for key in ["terms_digest", "acceptor"] {
            s.semantic_work().visit(Visit::Field).map_err(core_error)?;
            request_wrapper::same_text(
                request_wrapper::text(metadata, key, s)?,
                request_wrapper::text(request_wrapper::field(body, "metadata", s)?, key, s)?,
                s,
            )?;
        }
        s.semantic_work()
            .write_bytes(std::mem::size_of::<Option<current::source::SourceRecord>>())
            .map_err(core_error)?;
        dependencies[0] = Some(prior);
    } else if same(purpose, "admission_stop", s)? {
        let binding_ref = request_wrapper::field(metadata, "binding_ref", s)?;
        let target_ref = request_wrapper::field(metadata, "target_transition_ref", s)?;
        origin.require_member(binding_ref, s)?;
        origin.require_member(target_ref, s)?;
        let binding = current::source::retained_record(
            tx,
            origin.key(),
            binding_ref,
            "workflow-binding",
            "ledger-workflow-binding/1",
            origin.wrappers().sequence(),
            s,
        )
        .await?;
        let target = current::source::retained_record(
            tx,
            origin.key(),
            target_ref,
            "workflow-activation",
            "ledger-workflow-activation-transition/1",
            origin.wrappers().sequence(),
            s,
        )
        .await?;
        let expected = encode(binding_ref, s)?;
        let actual = value(target.body(), "binding_ref", s)?;
        same_bytes(expected.bytes(), actual.bytes(), s)?;
        for key in ["customer", "agreement_id"] {
            s.semantic_work().visit(Visit::Field).map_err(core_error)?;
            request_wrapper::same_text(
                request_wrapper::text(request_wrapper::field(p, "subject", s)?, key, s)?,
                request_wrapper::text(binding.body(), key, s)?,
                s,
            )?;
        }
        // This does not select/stop an activation. The actual earlier target,
        // accepted private closure and predecessor origins must complete first.
        s.semantic_work()
            .write_bytes(2 * std::mem::size_of::<Option<current::source::SourceRecord>>())
            .map_err(core_error)?;
        dependencies = [Some(binding), Some(target)];
    }
    let result = retention_record(
        p,
        management.actor,
        &management.at,
        management.root.reference(),
        s,
    )?;
    compare_source_results(
        tx,
        origin,
        &[(&result, "ledger-workflow-retained-evidence/1")],
        management.own_before,
        s,
    )
    .await?;
    Ok(RetainedEvidence {
        input,
        management,
        dependencies,
        result,
    })
}

/// Actual recomputed initial birth. It does not credit source identity, current
/// head, customer custody or an origin; the chronological owner consumes these
/// owning inputs only after all earlier recipes and absence checks complete.
pub(in super::super) struct InitialSource {
    request: SourceRequest,
    root: current::source::SourceRecord,
    evidence: current::source::SourceRecord,
    customers: Vec<current::source::SourceRecord>,
    enrollment: wire::SourceRecord,
    grant: wire::SourceRecord,
    state: wire::SourceRecord,
    _customers: ExternalBufferLease,
}
impl InitialSource {
    pub(in super::super) fn result_refs(
        &self,
    ) -> [&ledgerlab_core::policy::workflow::transition_constructor::ExactRef; 3] {
        [
            self.enrollment.reference(),
            self.grant.reference(),
            self.state.reference(),
        ]
    }
}
fn same_bytes(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "INTEGRITY_FAILURE")
}
async fn initial_inner(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<InitialSource> {
    let request = original_request(origin, s)?;
    let payload = s
        .parse_representation(
            request.normalized_payload(),
            RepresentationKind::OriginalRequest,
        )
        .map_err(core_error)?;
    let p = payload.value();
    request_wrapper::same_text(
        request_wrapper::text(p, "schema", s)?,
        "ledger-workflow-source-enroll/1",
        s,
    )?;
    let management = management(tx, origin, s).await?;
    let root = &management.root;
    let actor = management.actor;
    let at = &management.at;
    let own_before = management.own_before;
    let operation = origin.wrappers().operation();
    let ceiling = request.ceiling().ok_or(ServiceError::IntegrityFailure)?;
    request_wrapper::same_text(request_wrapper::text(ceiling, "principal_id", s)?, actor, s)?;
    for field in ["source", "canonical_economic_namespace"] {
        request_wrapper::same_text(
            request_wrapper::text(ceiling, field, s)?,
            &origin.key().source,
            s,
        )?;
    }
    let refs = request_wrapper::field(p, "customer_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let names = request_wrapper::field(ceiling, "customer_scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&refs.len().into(), &names.len().into()])
        .map_err(core_error)?;
    require(refs.len() == names.len(), "INTEGRITY_FAILURE")?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[
                &refs.len().into(),
                &std::mem::size_of::<current::source::SourceRecord>().into(),
            ],
        )
        .map_err(core_error)?;
    let n = refs
        .len()
        .checked_mul(std::mem::size_of::<current::source::SourceRecord>())
        .ok_or(ServiceError::Unavailable)?;
    let storage = s.reserve_external_payload(n).map_err(core_error)?;
    let mut customers = Vec::new();
    customers
        .try_reserve_exact(refs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    let mut seen = [false; 64];
    s.semantic_work()
        .write_bytes(seen.len())
        .map_err(core_error)?;
    for reference in refs {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        origin.require_member(reference, s)?;
        let customer = current::source::retained_record(
            tx,
            origin.key(),
            reference,
            "workflow-customer",
            "ledger-workflow-customer-registration/1",
            origin.wrappers().sequence(),
            s,
        )
        .await?;
        let body = customer.body();
        request_wrapper::closed(
            body,
            &[
                "schema",
                "scope",
                "customer",
                "acceptor",
                "evidence_ref",
                "registered_by",
                "registered_at",
                "authority_ref",
                "installation_mode",
            ],
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(body, "installation_mode", s)?,
            "real_local",
            s,
        )?;
        let scope = value(body, "scope", s)?;
        let expected_scope = encode(
            request_wrapper::field(request_wrapper::field(operation, "key", s)?, "scope", s)?,
            s,
        )?;
        same_bytes(scope.bytes(), expected_scope.bytes(), s)?;
        let name = request_wrapper::text(body, "customer", s)?;
        ingress::identifier(name, 128, s)?;
        let mut found = false;
        for (index, expected) in names.iter().enumerate() {
            let expected = expected.as_str().ok_or(ServiceError::IntegrityFailure)?;
            if same(name, expected, s)? {
                s.semantic_work()
                    .visit(Visit::Constraint)
                    .map_err(core_error)?;
                require(!seen[index], "INTEGRITY_FAILURE")?;
                s.semantic_work().write_bytes(1).map_err(core_error)?;
                seen[index] = true;
                found = true;
                break;
            }
        }
        require(found, "INTEGRITY_FAILURE")?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<current::source::SourceRecord>())
            .map_err(core_error)?;
        customers.push(customer);
    }
    let proof_ref = request_wrapper::field(p, "evidence_ref", s)?;
    origin.require_member(proof_ref, s)?;
    let evidence = current::source::retained_record(
        tx,
        origin.key(),
        proof_ref,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1",
        origin.wrappers().sequence(),
        s,
    )
    .await?;
    let proof = evidence.body();
    request_wrapper::same_text(
        request_wrapper::text(proof, "purpose", s)?,
        "source_enrollment",
        s,
    )?;
    request_wrapper::same_text(request_wrapper::text(proof, "retained_by", s)?, actor, s)?;
    request_wrapper::same_text(
        request_wrapper::text(proof, "installation_mode", s)?,
        "real_local",
        s,
    )?;
    let metadata = request_wrapper::field(proof, "metadata", s)?;
    request_wrapper::closed(metadata, &["source", "principal_id"], s)?;
    request_wrapper::same_text(
        request_wrapper::text(metadata, "source", s)?,
        &origin.key().source,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(metadata, "principal_id", s)?,
        actor,
        s,
    )?;
    let customer =
        request_wrapper::text(request_wrapper::field(proof, "subject", s)?, "customer", s)?;
    let mut found = false;
    for name in names {
        if same(
            customer,
            name.as_str().ok_or(ServiceError::IntegrityFailure)?,
            s,
        )? {
            found = true;
            break;
        }
    }
    require(found, "AUTHORITY_REQUIRED")?;
    let [enrollment, grant, state] = birth_records(
        &request,
        p,
        origin.key(),
        request_wrapper::field(operation, "key", s)?,
        actor,
        &at,
        root.reference(),
        s,
    )?;
    let recomputed = wire::RefSet::new(
        [enrollment.reference(), grant.reference(), state.reference()].into_iter(),
        3,
        true,
        s,
    )?
    .encode(s)?;
    let expected = value(operation, "result_refs", s)?;
    same_bytes(recomputed.bytes(), expected.bytes(), s)?;
    for (record, schema) in [
        (&enrollment, "ledger-workflow-source-enrollment/1"),
        (&grant, "ledger-workflow-source-grant/1"),
        (&state, "ledger-workflow-source-state/1"),
    ] {
        let reference = wire::reference(record.reference(), s)?;
        let parsed = s
            .parse_representation(reference.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        origin.require_member(parsed.value(), s)?;
        let stored = current::source::retained_record(
            tx,
            origin.key(),
            parsed.value(),
            &record.reference().kind,
            schema,
            own_before,
            s,
        )
        .await?;
        same_bytes(stored.canonical_body(), record.bytes(), s)?;
    }
    Ok(InitialSource {
        request,
        root: management.root,
        evidence,
        customers,
        enrollment,
        grant,
        state,
        _customers: storage,
    })
}
pub(in super::super) async fn reconstruct_initial(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<InitialSource> {
    let result = initial_inner(tx, origin, s).await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn birth_records(
    request: &SourceRequest,
    p: &Value,
    key: &OperationKey,
    key_value: &Value,
    actor: &str,
    at: &Timestamp,
    root: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<[wire::SourceRecord; 3]> {
    let ceiling = request.ceiling().ok_or(ServiceError::IntegrityFailure)?;
    let scope = value(key_value, "scope", s)?;
    let enrollment_id = value(p, "enrollment_id", s)?;
    let customers_bytes = value(p, "customer_refs", s)?;
    let proof_bytes = value(p, "evidence_ref", s)?;
    let actor_bytes = wire::string(actor, s)?;
    let at_bytes = wire::string(at.as_str(), s)?;
    let root_ref = wire::reference(root, s)?;
    let body = wire::object(
        &[
            ("authority_ref", root_ref.bytes()),
            (
                "ceiling",
                request
                    .ceiling
                    .as_ref()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .canonical_bytes(),
            ),
            ("customer_refs", customers_bytes.bytes()),
            ("enrolled_at", at_bytes.bytes()),
            ("enrolled_by", actor_bytes.bytes()),
            ("enrollment_id", enrollment_id.bytes()),
            ("evidence_ref", proof_bytes.bytes()),
            ("installation_mode", b"\"real_local\""),
            ("schema", b"\"ledger-workflow-source-enrollment/1\""),
            ("scope", scope.bytes()),
        ],
        262144,
        s,
    )?;
    let enrollment = wire::source_record(
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
        body,
        s,
    )?;
    let enrollment_ref = wire::reference(enrollment.reference(), s)?;
    let activities = value(ceiling, "activity_selectors", s)?;
    let namespace = value(ceiling, "canonical_economic_namespace", s)?;
    let customers_scope = value(ceiling, "customer_scope", s)?;
    let ends = value(ceiling, "ends_at", s)?;
    let links = value(ceiling, "link_selectors", s)?;
    let operations = value(ceiling, "operations", s)?;
    let permissions = value(ceiling, "permissions", s)?;
    let principal = value(ceiling, "principal_id", s)?;
    let products = value(ceiling, "product_selectors", s)?;
    let source = value(ceiling, "source", s)?;
    let slots = value(ceiling, "source_slots", s)?;
    let starts = value(ceiling, "starts_at", s)?;
    let body = wire::object(
        &[
            ("activity_selectors", activities.bytes()),
            ("canonical_economic_namespace", namespace.bytes()),
            ("customer_scope", customers_scope.bytes()),
            ("ends_at", ends.bytes()),
            ("grant_id", enrollment_id.bytes()),
            ("link_selectors", links.bytes()),
            ("operations", operations.bytes()),
            ("original_ceiling_ref", enrollment_ref.bytes()),
            ("permissions", permissions.bytes()),
            ("principal_id", principal.bytes()),
            ("product_selectors", products.bytes()),
            ("revision", b"\"1\""),
            ("schema", b"\"ledger-workflow-source-grant/1\""),
            ("source", source.bytes()),
            ("source_slots", slots.bytes()),
            ("starts_at", starts.bytes()),
        ],
        262144,
        s,
    )?;
    let grant = wire::source_record(
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
        body,
        s,
    )?;
    let grant_ref = wire::reference(grant.reference(), s)?;
    let key_bytes = encode(key_value, s)?;
    let body = wire::object(
        &[
            ("accepted_at", at_bytes.bytes()),
            ("actor", actor_bytes.bytes()),
            ("current_grant_ref", grant_ref.bytes()),
            ("enrollment_ref", enrollment_ref.bytes()),
            ("operation_key", key_bytes.bytes()),
            ("revision", b"\"1\""),
            ("schema", b"\"ledger-workflow-source-state/1\""),
            ("status", b"\"active\""),
        ],
        262144,
        s,
    )?;
    let state = wire::source_state_record(
        body,
        &key.logical_store_id,
        request_wrapper::field(key_value, "scope", s)?,
        request_wrapper::text(p, "enrollment_id", s)?,
        "1",
        s,
    )?;
    Ok([enrollment, grant, state])
}

#[cfg(test)]
mod birth_tests {
    use super::*;
    use ledgerlab_core::policy::workflow::{SourceEnrollment, SourceGrant, SourceState};
    #[test]
    fn original_source_birth_uses_exact_existing_records_and_scoped_revision_identity() {
        let fixture: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/agreement-ready-fixture-input.json"
        )))
        .unwrap();
        let mut input = fixture["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["payload"]["action"] == "source.enroll")
            .unwrap()["payload"]
            .clone();
        input["customer_refs"] = serde_json::json!([{"kind":"workflow-customer","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))}]);
        input["evidence_ref"] = serde_json::json!({"kind":"workflow-evidence","id":format!("wdoc_{}","c".repeat(64)),"digest":format!("sha256:{}","c".repeat(64))});
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let request = normalize(&input, &mut s).unwrap();
        let payload = s
            .parse_representation(
                request.normalized_payload(),
                RepresentationKind::OriginalRequest,
            )
            .unwrap();
        let key = OperationKey {
            logical_store_id: "source-record-test".into(),
            scope: ["test".into(), "local".into()],
            source: input["ceiling"]["source"].as_str().unwrap().into(),
            operation_id: "source-birth-test".into(),
        };
        let root = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
            kind: "workflow-authority".into(),
            id: format!("wdoc_{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        let at = Timestamp::parse("2026-10-17T00:00:00Z").unwrap();
        let key_value = serde_json::json!(key);
        let [enrollment, grant, state] = birth_records(
            &request,
            payload.value(),
            &key,
            &key_value,
            input["ceiling"]["principal_id"].as_str().unwrap(),
            &at,
            &root,
            &mut s,
        )
        .unwrap();
        let e = SourceEnrollment::parse(enrollment.bytes()).unwrap();
        let g = SourceGrant::parse(grant.bytes()).unwrap();
        let st = SourceState::parse(state.bytes(), &e).unwrap();
        assert_eq!(e.canonical_bytes(), enrollment.bytes());
        assert_eq!(e.reference().id(), enrollment.reference().id);
        assert_eq!(e.reference().digest(), enrollment.reference().digest);
        assert_eq!(g.canonical_bytes(), grant.bytes());
        assert_eq!(g.document_id(), grant.reference().id);
        assert_eq!(g.content_digest(), grant.reference().digest);
        assert_eq!(st.canonical_bytes(), state.bytes());
        assert_eq!(st.reference().id(), state.reference().id);
        assert_eq!(st.reference().digest(), state.reference().digest);
        g.validate_lineage(e.enrollment_id(), e.reference(), e.ceiling(), None)
            .unwrap();
        st.validate_lineage(&e, Some(&g), None).unwrap();
    }
}

struct Management<'a> {
    root: current::source::SourceRecord,
    at: Timestamp,
    actor: &'a str,
    own_before: i64,
    _time_storage: ExternalBufferLease,
}
async fn management<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<Management<'a>> {
    let time_storage = s
        .reserve_external_payload(2 * (4 * 64 + 27))
        .map_err(core_error)?;
    let operation = origin.wrappers().operation();
    let at = Timestamp::parse_with_work(
        request_wrapper::text(operation, "accepted_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let authority = request_wrapper::field(operation, "authority_snapshot", s)?;
    request_wrapper::closed(
        authority,
        &[
            "schema",
            "principal_id",
            "source",
            "authority_refs",
            "rights",
            "validated_at",
        ],
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(authority, "schema", s)?,
        "ledger-workflow-publication-authority/1",
        s,
    )?;
    let actor = request_wrapper::text(authority, "principal_id", s)?;
    ingress::identifier(actor, 128, s)?;
    request_wrapper::same_text(
        request_wrapper::text(authority, "source", s)?,
        &origin.key().source,
        s,
    )?;
    let validated = Timestamp::parse_with_work(
        request_wrapper::text(authority, "validated_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&validated.micros().into(), &at.micros().into()],
        )
        .map_err(core_error)?;
    require(validated.micros() == at.micros(), "INTEGRITY_FAILURE")?;
    let rights = value(authority, "rights", s)?;
    same_bytes(rights.bytes(), b"[\"admin\"]", s)?;
    let roots = request_wrapper::field(authority, "authority_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&roots.len().into(), &1usize.into()])
        .map_err(core_error)?;
    require(roots.len() == 1, "INTEGRITY_FAILURE")?;
    origin.require_member(&roots[0], s)?;
    s.semantic_work()
        .arithmetic(
            Op::Add,
            &[&origin.wrappers().sequence().into(), &1i64.into()],
        )
        .map_err(core_error)?;
    let own_before = origin
        .wrappers()
        .sequence()
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let root = current::source::retained_record(
        tx,
        origin.key(),
        &roots[0],
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1",
        own_before,
        s,
    )
    .await?;
    current::party::produce::validate_original_root(root.body(), origin.key(), actor, &at, s)?;
    Ok(Management {
        root,
        at,
        actor,
        own_before,
        _time_storage: time_storage,
    })
}

const CEILING_FIELDS: [&str; 12] = [
    "activity_selectors",
    "canonical_economic_namespace",
    "customer_scope",
    "ends_at",
    "link_selectors",
    "operations",
    "permissions",
    "principal_id",
    "product_selectors",
    "source",
    "source_slots",
    "starts_at",
];
fn grant_ceiling(grant: &Value, s: &mut OriginalAuditSession) -> Result<OwnedSourceCeiling> {
    request_wrapper::closed(
        grant,
        &[
            "activity_selectors",
            "canonical_economic_namespace",
            "customer_scope",
            "ends_at",
            "grant_id",
            "link_selectors",
            "operations",
            "original_ceiling_ref",
            "permissions",
            "principal_id",
            "product_selectors",
            "revision",
            "schema",
            "source",
            "source_slots",
            "starts_at",
        ],
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(grant, "schema", s)?,
        "ledger-workflow-source-grant/1",
        s,
    )?;
    let mut values: [Option<AuditedBytes>; 12] = std::array::from_fn(|_| None);
    for (index, key) in CEILING_FIELDS.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        values[index] = Some(value(grant, key, s)?);
    }
    let mut fields = [("", b"".as_slice()); 12];
    for (index, key) in CEILING_FIELDS.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[index] = (
            *key,
            values[index]
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .bytes(),
        );
    }
    let bytes = wire::object(&fields, 262144, s)?;
    let parsed = s
        .parse_representation(bytes.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let normalized = OwnedSourceCeiling::normalize(parsed.value(), s).map_err(core_error)?;
    same_bytes(bytes.bytes(), normalized.canonical_bytes(), s)?;
    Ok(normalized)
}
async fn compare_source_results(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    records: &[(&wire::SourceRecord, &'static str)],
    own_before: i64,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let refs = wire::RefSet::new(
        records.iter().map(|(r, _)| r.reference()),
        records.len(),
        true,
        s,
    )?
    .encode(s)?;
    let expected = value(origin.wrappers().operation(), "result_refs", s)?;
    same_bytes(refs.bytes(), expected.bytes(), s)?;
    for (record, schema) in records {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let reference = wire::reference(record.reference(), s)?;
        let reference = s
            .parse_representation(reference.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        origin.require_member(reference.value(), s)?;
        let stored = current::source::retained_record(
            tx,
            origin.key(),
            reference.value(),
            &record.reference().kind,
            schema,
            own_before,
            s,
        )
        .await?;
        same_bytes(stored.canonical_body(), record.bytes(), s)?;
    }
    Ok(())
}
/// A saved prior-state locator and its bodies are still untrusted DATA. The
/// chronological owner must match this state to the actual as-of active head;
/// this carrier cannot confer that match or a completed origin.
pub(in super::super) struct SourceSuccessor<'a> {
    request: SourceRequest,
    management: Management<'a>,
    enrollment: current::source::SourceRecord,
    prior_state: current::source::SourceRecord,
    prior_grant: current::source::SourceRecord,
    grant: Option<wire::SourceRecord>,
    state: wire::SourceRecord,
}
pub(in super::super) async fn reconstruct_successor<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<SourceSuccessor<'a>> {
    let result = successor_inner(tx, origin, s).await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}
async fn successor_inner<'a>(
    tx: &mut SqliteTx,
    origin: &'a LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<SourceSuccessor<'a>> {
    let request = original_request(origin, s)?;
    let payload = s
        .parse_representation(
            request.normalized_payload(),
            RepresentationKind::OriginalRequest,
        )
        .map_err(core_error)?;
    let p = payload.value();
    let restricted = same(
        request_wrapper::text(p, "schema", s)?,
        "ledger-workflow-source-restrict/1",
        s,
    )?;
    if !restricted {
        request_wrapper::same_text(
            request_wrapper::text(p, "schema", s)?,
            "ledger-workflow-source-revoke/1",
            s,
        )?;
    }
    let management = management(tx, origin, s).await?;
    let own_before = management.own_before;
    let enrollment_ref = request_wrapper::field(p, "enrollment_ref", s)?;
    origin.require_member(enrollment_ref, s)?;
    let enrollment = current::source::retained_record(
        tx,
        origin.key(),
        enrollment_ref,
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1",
        origin.wrappers().sequence(),
        s,
    )
    .await?;
    let e = enrollment.body();
    request_wrapper::closed(
        e,
        &[
            "schema",
            "scope",
            "enrollment_id",
            "ceiling",
            "customer_refs",
            "evidence_ref",
            "enrolled_by",
            "enrolled_at",
            "authority_ref",
            "installation_mode",
        ],
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(e, "installation_mode", s)?,
        "real_local",
        s,
    )?;
    let e_scope = value(e, "scope", s)?;
    let op = origin.wrappers().operation();
    let key = request_wrapper::field(op, "key", s)?;
    let scope = value(key, "scope", s)?;
    same_bytes(e_scope.bytes(), scope.bytes(), s)?;
    let original_ceiling =
        OwnedSourceCeiling::normalize(request_wrapper::field(e, "ceiling", s)?, s)
            .map_err(core_error)?;
    let original = request_wrapper::field(e, "ceiling", s)?;
    let encoded = encode(original, s)?;
    same_bytes(encoded.bytes(), original_ceiling.canonical_bytes(), s)?;
    request_wrapper::same_text(
        request_wrapper::text(original, "principal_id", s)?,
        management.actor,
        s,
    )?;
    for name in ["source", "canonical_economic_namespace"] {
        request_wrapper::same_text(
            request_wrapper::text(original, name, s)?,
            &origin.key().source,
            s,
        )?;
    }
    let results = request_wrapper::field(op, "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let mut saved_state = None;
    for reference in results {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        if same(
            request_wrapper::text(reference, "kind", s)?,
            "workflow-source-state",
            s,
        )? {
            s.semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(
                saved_state.replace(reference).is_none(),
                "INTEGRITY_FAILURE",
            )?;
        }
    }
    let saved_state = saved_state.ok_or(ServiceError::IntegrityFailure)?;
    let retained = current::source::retained_record(
        tx,
        origin.key(),
        saved_state,
        "workflow-source-state",
        "ledger-workflow-source-state/1",
        own_before,
        s,
    )
    .await?;
    let prior_ref = request_wrapper::field(retained.body(), "prior_state_ref", s)?;
    origin.require_member(prior_ref, s)?;
    let prior_state = current::source::retained_record(
        tx,
        origin.key(),
        prior_ref,
        "workflow-source-state",
        "ledger-workflow-source-state/1",
        origin.wrappers().sequence(),
        s,
    )
    .await?;
    let prior = prior_state.body();
    request_wrapper::same_text(request_wrapper::text(prior, "status", s)?, "active", s)?;
    request_wrapper::same_text(
        request_wrapper::text(prior, "actor", s)?,
        management.actor,
        s,
    )?;
    let actual_enrollment = value(prior, "enrollment_ref", s)?;
    let expected_enrollment = encode(enrollment_ref, s)?;
    same_bytes(actual_enrollment.bytes(), expected_enrollment.bytes(), s)?;
    let prior_key = request_wrapper::field(prior, "operation_key", s)?;
    request_wrapper::same_text(
        request_wrapper::text(prior_key, "logical_store_id", s)?,
        &origin.key().logical_store_id,
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(prior_key, "source", s)?,
        &origin.key().source,
        s,
    )?;
    let prior_scope = value(prior_key, "scope", s)?;
    same_bytes(prior_scope.bytes(), scope.bytes(), s)?;
    let expected = request_wrapper::text(p, "expected_revision", s)?;
    request_wrapper::same_text(request_wrapper::text(prior, "revision", s)?, expected, s)?;
    let revision = acceptance::counter(expected, s)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&revision.into(), &1u64.into()])
        .map_err(core_error)?;
    let next = revision
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&next.into(), &i64::MAX.into()])
        .map_err(core_error)?;
    require(next <= i64::MAX as u64, "INTEGRITY_FAILURE")?;
    let next_string = counter_string(next, s)?;
    let next_value = s
        .parse_representation(next_string.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)?;
    let next_text = next_value
        .value()
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?;
    let prior_grant_ref = request_wrapper::field(prior, "current_grant_ref", s)?;
    origin.require_member(prior_grant_ref, s)?;
    let prior_grant = current::source::retained_record(
        tx,
        origin.key(),
        prior_grant_ref,
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
        origin.wrappers().sequence(),
        s,
    )
    .await?;
    let previous = prior_grant.body();
    let supplied = value(previous, "original_ceiling_ref", s)?;
    same_bytes(supplied.bytes(), expected_enrollment.bytes(), s)?;
    request_wrapper::same_text(
        request_wrapper::text(previous, "grant_id", s)?,
        request_wrapper::text(e, "enrollment_id", s)?,
        s,
    )?;
    request_wrapper::same_text(request_wrapper::text(previous, "revision", s)?, expected, s)?;
    let prior_ceiling = grant_ceiling(previous, s)?;
    require(
        prior_ceiling
            .is_subset_of_with_work(&original_ceiling, s)
            .map_err(core_error)?,
        "INTEGRITY_FAILURE",
    )?;
    // Body digest and logical revision ID are independently recomputed. This
    // does not establish that this active state was the actual latest prefix.
    let prior_bytes = s
        .copy_representation(
            prior_state.canonical_body(),
            RepresentationKind::SourceDefinition,
        )
        .map_err(core_error)?;
    let recomputed_prior = wire::source_state_record(
        prior_bytes,
        &origin.key().logical_store_id,
        request_wrapper::field(key, "scope", s)?,
        request_wrapper::text(e, "enrollment_id", s)?,
        expected,
        s,
    )?;
    let supplied = wire::reference(prior_state.reference(), s)?;
    let recomputed = wire::reference(recomputed_prior.reference(), s)?;
    same_bytes(supplied.bytes(), recomputed.bytes(), s)?;
    let grant = if restricted {
        let ceiling = request
            .ceiling
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            ceiling
                .is_subset_of_with_work(&original_ceiling, s)
                .map_err(core_error)?
                && ceiling
                    .is_subset_of_with_work(&prior_ceiling, s)
                    .map_err(core_error)?,
            "SOURCE_CEILING_EXCEEDED",
        )?;
        Some(successor_grant(
            ceiling.canonical_value(),
            request_wrapper::text(e, "enrollment_id", s)?,
            enrollment.reference(),
            next_text,
            s,
        )?)
    } else {
        None
    };
    let state = successor_state(
        origin.key(),
        key,
        enrollment.reference(),
        request_wrapper::text(e, "enrollment_id", s)?,
        prior_state.reference(),
        grant.as_ref().map(wire::SourceRecord::reference),
        next_text,
        management.actor,
        &management.at,
        s,
    )?;
    if let Some(grant) = &grant {
        compare_source_results(
            tx,
            origin,
            &[
                (grant, "ledger-workflow-source-grant/1"),
                (&state, "ledger-workflow-source-state/1"),
            ],
            own_before,
            s,
        )
        .await?;
    } else {
        compare_source_results(
            tx,
            origin,
            &[(&state, "ledger-workflow-source-state/1")],
            own_before,
            s,
        )
        .await?;
    }
    Ok(SourceSuccessor {
        request,
        management,
        enrollment,
        prior_state,
        prior_grant,
        grant,
        state,
    })
}
fn counter_string(mut n: u64, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let mut digits = [0u8; 20];
    let mut start = 20usize;
    loop {
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&start.into(), &1usize.into()])
            .map_err(core_error)?;
        start -= 1;
        s.semantic_work()
            .arithmetic(Op::DivideRemainder, &[&n.into(), &10u64.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Add, &[&b'0'.into(), &(n % 10).into()])
            .map_err(core_error)?;
        s.semantic_work().write_bytes(1).map_err(core_error)?;
        digits[start] = b'0' + (n % 10) as u8;
        n /= 10;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&n.into(), &0u64.into()])
            .map_err(core_error)?;
        if n == 0 {
            break;
        }
    }
    s.semantic_work()
        .scan_bytes(digits.len())
        .map_err(core_error)?;
    wire::string(
        std::str::from_utf8(&digits[start..]).map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )
}
fn successor_grant(
    ceiling: &Value,
    enrollment_id: &str,
    enrollment: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    revision: &str,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let mut values: [Option<AuditedBytes>; 12] = std::array::from_fn(|_| None);
    for (i, key) in CEILING_FIELDS.iter().enumerate() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        values[i] = Some(value(ceiling, key, s)?);
    }
    let enrollment = wire::reference(enrollment, s)?;
    let id = wire::string(enrollment_id, s)?;
    let revision = wire::string(revision, s)?;
    let v = |i: usize| -> Result<&[u8]> {
        Ok(values[i]
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .bytes())
    };
    let body = wire::object(
        &[
            ("activity_selectors", v(0)?),
            ("canonical_economic_namespace", v(1)?),
            ("customer_scope", v(2)?),
            ("ends_at", v(3)?),
            ("grant_id", id.bytes()),
            ("link_selectors", v(4)?),
            ("operations", v(5)?),
            ("original_ceiling_ref", enrollment.bytes()),
            ("permissions", v(6)?),
            ("principal_id", v(7)?),
            ("product_selectors", v(8)?),
            ("revision", revision.bytes()),
            ("schema", b"\"ledger-workflow-source-grant/1\""),
            ("source", v(9)?),
            ("source_slots", v(10)?),
            ("starts_at", v(11)?),
        ],
        262144,
        s,
    )?;
    wire::source_record(
        "workflow-source-grant",
        "ledger-workflow-source-grant/1",
        body,
        s,
    )
}

#[allow(clippy::too_many_arguments)]
fn successor_state(
    key: &OperationKey,
    key_value: &Value,
    enrollment: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    enrollment_id: &str,
    prior: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    grant: Option<&ledgerlab_core::policy::workflow::transition_constructor::ExactRef>,
    revision: &str,
    actor: &str,
    at: &Timestamp,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let actor = wire::string(actor, s)?;
    let at = wire::string(at.as_str(), s)?;
    let enrollment_bytes = wire::reference(enrollment, s)?;
    let prior_bytes = wire::reference(prior, s)?;
    let key_bytes = encode(key_value, s)?;
    let revision_bytes = wire::string(revision, s)?;
    let current_grant = grant.map(|g| wire::reference(g, s)).transpose()?;
    let body = if let Some(g) = &current_grant {
        wire::object(
            &[
                ("accepted_at", at.bytes()),
                ("actor", actor.bytes()),
                ("current_grant_ref", g.bytes()),
                ("enrollment_ref", enrollment_bytes.bytes()),
                ("operation_key", key_bytes.bytes()),
                ("prior_state_ref", prior_bytes.bytes()),
                ("revision", revision_bytes.bytes()),
                ("schema", b"\"ledger-workflow-source-state/1\""),
                ("status", b"\"active\""),
            ],
            262144,
            s,
        )?
    } else {
        wire::object(
            &[
                ("accepted_at", at.bytes()),
                ("actor", actor.bytes()),
                ("enrollment_ref", enrollment_bytes.bytes()),
                ("operation_key", key_bytes.bytes()),
                ("prior_state_ref", prior_bytes.bytes()),
                ("revision", revision_bytes.bytes()),
                ("schema", b"\"ledger-workflow-source-state/1\""),
                ("status", b"\"revoked\""),
            ],
            262144,
            s,
        )?
    };
    wire::source_state_record(
        body,
        &key.logical_store_id,
        request_wrapper::field(key_value, "scope", s)?,
        enrollment_id,
        revision,
        s,
    )
}

#[cfg(test)]
mod successor_tests {
    use super::*;
    use ledgerlab_core::policy::workflow::{SourceEnrollment, SourceGrant, SourceState};
    #[test]
    fn restrict_then_terminal_revoke_matches_same_existing_r1_lineage_and_record_bytes() {
        let input = serde_json::json!({"schema":"ledger-workflow-source-enroll/1","action":"source.enroll","expected_revision":"0","enrollment_id":"source-unit","customer_refs":[{"kind":"workflow-customer","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))}],"evidence_ref":{"kind":"workflow-evidence","id":format!("wdoc_{}","c".repeat(64)),"digest":format!("sha256:{}","c".repeat(64))},"ceiling":{"principal_id":"owner","source":"source","canonical_economic_namespace":"source","customer_scope":["c"],"operations":["work.accept","evidence.retain"],"product_selectors":[{"names":["a:product"]}],"activity_selectors":[{"names":["a:one","a:two"]}],"link_selectors":[],"source_slots":["a:worker"],"permissions":["submit","read"],"starts_at":"2026-10-17T00:00:00Z","ends_at":"2026-10-18T00:00:00Z"}});
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let request = normalize(&input, &mut s).unwrap();
        let payload = s
            .parse_representation(
                request.normalized_payload(),
                RepresentationKind::OriginalRequest,
            )
            .unwrap();
        let mut key = OperationKey {
            logical_store_id: "test-store".into(),
            scope: ["test".into(), "local".into()],
            source: "source".into(),
            operation_id: "birth".into(),
        };
        let key_value = serde_json::json!(key);
        let root = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
            kind: "workflow-authority".into(),
            id: format!("wdoc_{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        let at = Timestamp::parse("2026-10-17T00:00:00Z").unwrap();
        let [enrollment, original_grant, original_state] = birth_records(
            &request,
            payload.value(),
            &key,
            &key_value,
            "owner",
            &at,
            &root,
            &mut s,
        )
        .unwrap();
        let e = SourceEnrollment::parse(enrollment.bytes()).unwrap();
        let old_grant = SourceGrant::parse(original_grant.bytes()).unwrap();
        let old_state = SourceState::parse(original_state.bytes(), &e).unwrap();
        let mut narrowed = input["ceiling"].clone();
        narrowed["activity_selectors"] =
            serde_json::json!([{"names":["a:one"]},{"names":["a:two"]}]);
        narrowed["permissions"] = serde_json::json!(["read"]);
        let normalized = OwnedSourceCeiling::normalize(&narrowed, &mut s).unwrap();
        assert!(
            normalized
                .is_subset_of_with_work(request.ceiling.as_ref().unwrap(), &mut s)
                .unwrap()
        );
        let grant = successor_grant(
            normalized.canonical_value(),
            e.enrollment_id(),
            enrollment.reference(),
            "2",
            &mut s,
        )
        .unwrap();
        key.operation_id = "restrict".into();
        let key_value = serde_json::json!(key);
        let state = successor_state(
            &key,
            &key_value,
            enrollment.reference(),
            e.enrollment_id(),
            original_state.reference(),
            Some(grant.reference()),
            "2",
            "owner",
            &at,
            &mut s,
        )
        .unwrap();
        let g = SourceGrant::parse(grant.bytes()).unwrap();
        let st = SourceState::parse(state.bytes(), &e).unwrap();
        g.validate_lineage(
            e.enrollment_id(),
            e.reference(),
            e.ceiling(),
            Some(&old_grant),
        )
        .unwrap();
        st.validate_lineage(&e, Some(&g), Some((&old_state, &old_grant)))
            .unwrap();
        assert_eq!(g.document_id(), grant.reference().id);
        assert_eq!(g.content_digest(), grant.reference().digest);
        assert_eq!(st.canonical_bytes(), state.bytes());
        assert_eq!(st.reference().id(), state.reference().id);
        assert_eq!(st.reference().digest(), state.reference().digest);
        key.operation_id = "revoke".into();
        let key_value = serde_json::json!(key);
        let terminal = successor_state(
            &key,
            &key_value,
            enrollment.reference(),
            e.enrollment_id(),
            state.reference(),
            None,
            "3",
            "owner",
            &at,
            &mut s,
        )
        .unwrap();
        let closed = SourceState::parse(terminal.bytes(), &e).unwrap();
        closed.validate_lineage(&e, None, Some((&st, &g))).unwrap();
        assert_eq!(closed.status(), "revoked");
        assert_eq!(closed.canonical_bytes(), terminal.bytes());
        assert_eq!(closed.reference().id(), terminal.reference().id);
        assert_eq!(closed.reference().digest(), terminal.reference().digest);
    }
}

/// Consume the actual replay into the invocation-local chronological inventory.
/// No SQL projection, asserted prior-state locator or later body can provide
/// the permanent-key/head facts checked here.
pub(super) async fn fold(
    tx: &mut SqliteTx,
    origin: &LocatedWrappers,
    inventory: &mut inventory::Inventory,
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<Option<RecompiledPublication>> {
    if same(
        request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
        "semantic_alias",
        s,
    )? {
        return Err(ServiceError::Unavailable);
    }
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if origin.wrappers().class() == wrappers::WireClass::Publication {
        let program_owner = super::publication::fold(tx, origin, inventory, setup, s).await?;
        // Original ancestor selection retains the appropriate owning PUB in
        // its enclosing traversal, without a current compiler fallback.
        return Ok(Some(program_owner));
    }
    let literal =
        request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
    let schema = request_wrapper::text(literal, "schema", s)?;
    for party in [
        "ledger-workflow-party-authorize/1",
        "ledger-workflow-party-revoke/1",
        "ledger-workflow-party-evidence-retain/1",
        "ledger-workflow-party-evidence-retain/2",
    ] {
        if same(schema, party, s)? {
            let result = origin.reconstruct_party(tx, inventory, s).await?;
            result.fold_original(origin, inventory, setup, s)?;
            return Ok(None);
        }
    }
    if same(schema, "ledger-workflow-customer-register/1", s)? {
        let r = reconstruct_registration(tx, origin, s).await?;
        custody(r.management.root.body(), setup, s)?;
        inventory.compare_record(r.evidence.reference(), r.evidence.canonical_body(), s)?;
        let p = s
            .parse_representation(r.payload.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let customer = value(p.value(), "customer", s)?;
        let key = inventory::StateKey::new("customer", &[customer.bytes()], s)?;
        inventory.absent(&key, s)?;
        stage(inventory, &r.management.root, &[&r.registration], origin, s)?;
        inventory.set_head(&key, r.registration.reference(), s)?;
    } else if same(schema, "ledger-workflow-source-enroll/1", s)? {
        let r = reconstruct_initial(tx, origin, s).await?;
        custody(r.root.body(), setup, s)?;
        inventory.compare_record(r.evidence.reference(), r.evidence.canonical_body(), s)?;
        let p = s
            .parse_representation(
                r.request.normalized_payload(),
                RepresentationKind::SourceDefinition,
            )
            .map_err(core_error)?;
        let id = value(p.value(), "enrollment_id", s)?;
        let permanent = inventory::StateKey::new("enrollment", &[id.bytes()], s)?;
        let head = inventory::StateKey::new("source-head", &[id.bytes()], s)?;
        inventory.absent(&permanent, s)?;
        for customer in &r.customers {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let name = value(customer.body(), "customer", s)?;
            let key = inventory::StateKey::new("customer", &[name.bytes()], s)?;
            inventory.match_head(&key, customer.canonical_body(), s)?;
            inventory.compare_record(customer.reference(), customer.canonical_body(), s)?;
        }
        // Original normalization requires namespace == source; bind that same
        // source to the real immutable local owner, rather than a saved field.
        request_wrapper::same_text(
            request_wrapper::text(
                r.request.ceiling().ok_or(ServiceError::IntegrityFailure)?,
                "canonical_economic_namespace",
                s,
            )?,
            &setup.source,
            s,
        )?;
        stage(
            inventory,
            &r.root,
            &[&r.enrollment, &r.grant, &r.state],
            origin,
            s,
        )?;
        inventory.set_head(&permanent, r.enrollment.reference(), s)?;
        inventory.set_head(&head, r.state.reference(), s)?;
    } else if same(schema, "ledger-workflow-source-restrict/1", s)?
        || same(schema, "ledger-workflow-source-revoke/1", s)?
    {
        let r = reconstruct_successor(tx, origin, s).await?;
        custody(r.management.root.body(), setup, s)?;
        inventory.compare_record(r.enrollment.reference(), r.enrollment.canonical_body(), s)?;
        inventory.compare_record(r.prior_grant.reference(), r.prior_grant.canonical_body(), s)?;
        let id = value(r.enrollment.body(), "enrollment_id", s)?;
        let permanent = inventory::StateKey::new("enrollment", &[id.bytes()], s)?;
        inventory.match_head(&permanent, r.enrollment.canonical_body(), s)?;
        let head = inventory::StateKey::new("source-head", &[id.bytes()], s)?;
        inventory.match_head(&head, r.prior_state.canonical_body(), s)?;
        if let Some(grant) = &r.grant {
            stage(inventory, &r.management.root, &[grant, &r.state], origin, s)?;
        } else {
            stage(inventory, &r.management.root, &[&r.state], origin, s)?;
        }
        inventory.set_head(&head, r.state.reference(), s)?;
    } else if same(schema, "ledger-workflow-evidence-retain/1", s)? {
        let r = reconstruct_retention(tx, origin, s).await?;
        custody(r.management.root.body(), setup, s)?;
        for record in r.dependencies.iter().flatten() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            inventory.compare_record(record.reference(), record.canonical_body(), s)?;
        }
        stage(inventory, &r.management.root, &[&r.result], origin, s)?;
    } else {
        return Err(ServiceError::Unavailable);
    }
    Ok(None)
}
pub(in super::super) fn custody(
    root: &Value,
    setup: &crate::service::billing::Setup,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let basis = request_wrapper::field(root, "basis", s)?;
    for (name, actual) in [
        ("logical_store_id", setup.store_id.as_str()),
        ("principal_id", setup.operator.as_str()),
        ("source", setup.source.as_str()),
        ("canonical_economic_namespace", setup.source.as_str()),
    ] {
        request_wrapper::same_text(request_wrapper::text(basis, name, s)?, actual, s)?;
    }
    let scope = request_wrapper::field(basis, "scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&scope.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    for (v, actual) in scope
        .iter()
        .zip([setup.scope.tenant(), setup.scope.environment()])
    {
        request_wrapper::same_text(v.as_str().ok_or(ServiceError::IntegrityFailure)?, actual, s)?;
    }
    let revision =
        acceptance::counter(request_wrapper::text(basis, "source_grant_revision", s)?, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[&revision.into(), &setup.grant_revision.into()],
        )
        .map_err(core_error)?;
    require(revision <= setup.grant_revision, "INTEGRITY_FAILURE")
}
fn stage(
    inventory: &mut inventory::Inventory,
    root: &current::source::SourceRecord,
    records: &[&wire::SourceRecord],
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    inventory.introduce(
        root.reference(),
        root.canonical_body(),
        origin.wrappers().sequence(),
        s,
    )?;
    for record in records {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        inventory.introduce(
            record.reference(),
            record.bytes(),
            origin.wrappers().sequence(),
            s,
        )?;
    }
    complete_manifest(inventory, records, origin, s)
}
/// Only the closed R1 schemas below have the all-immediate Ref traversal used
/// here. New financial/history/commitment recipes must use their own cuts.
pub(in super::super) fn complete_manifest(
    inventory: &mut inventory::Inventory,
    records: &[&wire::SourceRecord],
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    complete_manifest_references(inventory, records.iter().map(|r| r.reference()), origin, s)
}
pub(super) fn complete_manifest_references<'a>(
    inventory: &mut inventory::Inventory,
    records: impl Iterator<
        Item = &'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    >,
    origin: &LocatedWrappers,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let manifest = origin.wrappers().manifest();
    let members = request_wrapper::field(manifest, "members", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&members.len().into(), &4096usize.into()])
        .map_err(core_error)?;
    require(members.len() <= 4096, "INTEGRITY_FAILURE")?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&members.len().into(), &std::mem::size_of::<usize>().into()],
        )
        .map_err(core_error)?;
    let bytes = members
        .len()
        .checked_mul(std::mem::size_of::<usize>())
        .ok_or(ServiceError::Unavailable)?;
    let _queue_lease = s.reserve_external_payload(bytes).map_err(core_error)?;
    let _seen_lease = s
        .reserve_external_payload(members.len())
        .map_err(core_error)?;
    let mut queue = Vec::new();
    queue
        .try_reserve_exact(members.len())
        .map_err(|_| ServiceError::Unavailable)?;
    s.semantic_work()
        .write_bytes(members.len())
        .map_err(core_error)?;
    let mut seen = vec![false; members.len()];
    for r in records {
        let reference = wire::reference(r, s)?;
        let parsed = s
            .parse_representation(reference.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        enqueue(parsed.value(), manifest, &mut queue, &mut seen, s)?;
    }
    let mut position = 0usize;
    while {
        s.semantic_work()
            .arithmetic(Op::Compare, &[&position.into(), &queue.len().into()])
            .map_err(core_error)?;
        position < queue.len()
    } {
        let index = queue[position];
        s.semantic_work()
            .arithmetic(Op::Add, &[&position.into(), &1usize.into()])
            .map_err(core_error)?;
        position = position
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let body = inventory.body(&members[index], s)?;
        let body = s
            .parse_representation(body.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        super::dependencies::body(
            body.value(),
            &mut |reference, s| enqueue(reference, manifest, &mut queue, &mut seen, s),
            s,
        )?;
    }
    let (id, digest) = origin.wrappers().operation_identity();
    let id = wire::string(
        std::str::from_utf8(id).map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let digest = wire::string(
        std::str::from_utf8(digest).map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let operation = wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", b"\"workflow-operation\""),
        ],
        262144,
        s,
    )?;
    let operation = s
        .parse_representation(operation.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let own = super::publication::member_index(operation.value(), manifest, s)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(!seen[own], "INTEGRITY_FAILURE")?;
    s.semantic_work().write_bytes(1).map_err(core_error)?;
    seen[own] = true;
    for reached in seen {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        require(reached, "INTEGRITY_FAILURE")?;
    }
    Ok(())
}
fn enqueue(
    reference: &Value,
    manifest: &Value,
    queue: &mut Vec<usize>,
    seen: &mut [bool],
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let i = super::publication::member_index(reference, manifest, s)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if !seen[i] {
        s.semantic_work().write_bytes(1).map_err(core_error)?;
        seen[i] = true;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<usize>())
            .map_err(core_error)?;
        queue.push(i);
    }
    Ok(())
}
