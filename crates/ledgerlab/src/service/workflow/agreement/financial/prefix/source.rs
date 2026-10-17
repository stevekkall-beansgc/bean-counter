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
    // Two simultaneous normalized timestamp results plus their fixed parser
    // scratch, admitted before either original time is parsed.
    let _time_storage = s
        .reserve_external_payload(2 * (4 * 64 + 27))
        .map_err(core_error)?;
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
        root,
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
