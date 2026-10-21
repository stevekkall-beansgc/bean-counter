//! Actual management snapshot and canonical party-result construction. Neither
//! this DATA nor a point-read hit completes historical custody or permits append.
use super::super::super::{ingress::IncomingRequest, wire};
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;

pub(in super::super::super) struct PendingPartyOperation {
    pub(in super::super::super) root: wire::SourceRecord,
    pub(in super::super::super) authority: wire::SourceRecord,
    pub(in super::super::super) result: wire::SourceRecord,
    normalized: AuditedBytes,
    dependencies: [Option<PartyRecord>; 2],
    consent_basis: [Option<PartyRecord>; 4],
    prior: Option<PartyRecord>,
    pub(in super::super::super) at: Timestamp,
    pub(in super::super::super) before: i64,
    // Fresh preparation retains the actual completed earlier recipe inventory.
    // This is not a serialized origin/resource completion capability.
    _prefix: Option<prefix::FoldedInventory>,
    local_members: Option<prefix::ResolvedMembers>,
    _clock_owner: Option<ExternalBufferLease>,
}
impl PendingPartyOperation {
    pub(in super::super::super) fn normalized_payload(&self) -> &[u8] {
        self.normalized.bytes()
    }
    pub(in super::super::super) fn resolve_members(
        &mut self,
        session: &mut OriginalAuditSession,
    ) -> Result<prefix::ResolvedMembers> {
        if let Some(members) = self.local_members.take() {
            return Ok(members);
        }
        let mut prefix = self._prefix.take();
        let result = self.resolve_members_in(prefix.as_mut().map(|p| &mut p._inventory), session);
        self._prefix = prefix;
        result
    }
    pub(in super::super::super) fn resolve_members_in(
        &self,
        inventory: Option<&mut prefix::inventory::Inventory>,
        session: &mut OriginalAuditSession,
    ) -> Result<prefix::ResolvedMembers> {
        prefix::ResolvedMembers::resolve_optional(
            inventory,
            &[&self.root, &self.authority, &self.result],
            self.dependency_refs(),
            session,
        )
    }
    pub(in super::super::super) fn prior_authorization_ref(&self) -> Option<&ExactRef> {
        self.prior.as_ref().map(|r| &r.reference)
    }
    pub(in super::super::super) fn dependency_refs(&self) -> impl Iterator<Item = &ExactRef> {
        self.dependencies
            .iter()
            .filter_map(|r| r.as_ref().map(|r| &r.reference))
            .chain(self.prior.iter().map(|r| &r.reference))
            .chain(
                self.consent_basis
                    .iter()
                    .filter_map(|r| r.as_ref().map(|r| &r.reference)),
            )
    }
}
fn evidence_payload(schema: &str, session: &mut OriginalAuditSession) -> Result<bool> {
    Ok(
        equal_text(schema, "ledger-workflow-party-evidence-retain/1", session)?
            || equal_text(schema, "ledger-workflow-party-evidence-retain/2", session)?,
    )
}
fn commitment_payload(payload: &Value, session: &mut OriginalAuditSession) -> Result<bool> {
    equal_text(
        check::text(payload, "schema", session)?,
        "ledger-workflow-party-evidence-retain/2",
        session,
    )
}
fn encode(value: &Value, session: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    session
        .canonical_representation(value, RepresentationKind::SourceDefinition)
        .map_err(core_error)
}
fn counter(mut n: u64, session: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let mut bytes = [0u8; 20];
    let mut start = 20usize;
    loop {
        session
            .semantic_work()
            .arithmetic(Op::DivideRemainder, &[&n.into(), &10u8.into()])
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&start.into(), &1usize.into()])
            .map_err(core_error)?;
        start -= 1;
        session.semantic_work().write_bytes(1).map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&b'0'.into(), &(n % 10).into()])
            .map_err(core_error)?;
        bytes[start] = b'0' + (n % 10) as u8;
        n /= 10;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&n.into(), &0u8.into()])
            .map_err(core_error)?;
        if n == 0 {
            break;
        }
    }
    wire::string(
        std::str::from_utf8(&bytes[start..]).map_err(|_| ServiceError::IntegrityFailure)?,
        session,
    )
}
pub(in super::super::super) fn root(
    setup: &crate::service::billing::Setup,
    scope: &Value,
    at: &Timestamp,
    session: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let store = wire::string(&setup.store_id, session)?;
    let actor = wire::string(&setup.operator, session)?;
    let source = wire::string(&setup.source, session)?;
    let scope = encode(scope, session)?;
    let revision = counter(setup.grant_revision, session)?;
    let schema = wire::string("ledger-workflow-local-authority-basis/1", session)?;
    let basis = wire::object(
        &[
            ("canonical_economic_namespace", source.bytes()),
            ("logical_store_id", store.bytes()),
            ("principal_id", actor.bytes()),
            ("rights", b"[\"admin\",\"read\"]"),
            ("schema", schema.bytes()),
            ("scope", scope.bytes()),
            ("source", source.bytes()),
            ("source_grant_revision", revision.bytes()),
        ],
        65536,
        session,
    )?;
    let digest = wire::domain_digest(
        b"bean-counter/local-authority-basis/1\0",
        &[basis.bytes()],
        session,
    )?;
    let schema = wire::string("ledger-workflow-local-authority-snapshot/1", session)?;
    let profile = wire::string("local-private-directory-owner/1", session)?;
    let at = wire::string(at.as_str(), session)?;
    let body = wire::object(
        &[
            ("basis", basis.bytes()),
            ("basis_digest", digest.bytes()),
            ("schema", schema.bytes()),
            ("verification_profile", profile.bytes()),
            ("verified_at", at.bytes()),
        ],
        65536,
        session,
    )?;
    wire::source_record(
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1",
        body,
        session,
    )
}
fn subject(value: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    check::closed(
        value,
        &["customer", "agreement_id", "party", "acceptor"],
        session,
    )?;
    for name in ["customer", "agreement_id", "party", "acceptor"] {
        id(value, name, 128, session)?;
    }
    Ok(())
}
fn same_subject(subject: &Value, spec: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    for name in ["customer", "agreement_id", "party", "acceptor"] {
        check::same_text(
            check::text(subject, name, session)?,
            check::text(spec, name, session)?,
            session,
        )?;
    }
    Ok(())
}
fn consent(
    value: &Value,
    key: &OperationKey,
    at: &Timestamp,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    check::closed(
        value,
        &[
            "scope",
            "customer",
            "agreement_id",
            "party",
            "acceptor",
            "roles",
            "authorization_ref",
            "basis",
            "accepted_at",
        ],
        session,
    )?;
    scope(check::field(value, "scope", session)?, key, session)?;
    for name in ["customer", "agreement_id", "party", "acceptor"] {
        id(value, name, 128, session)?;
    }
    roles(value, session)?;
    ref_parts(
        check::field(value, "authorization_ref", session)?,
        "workflow-authority",
        session,
    )?;
    let basis = check::field(value, "basis", session)?;
    let kind = check::text(basis, "kind", session)?;
    if equal_text(kind, "acceptance_terms", session)? {
        check::closed(basis, &["kind", "terms_ref", "terms_digest"], session)?;
        ref_parts(
            check::field(basis, "terms_ref", session)?,
            "workflow-acceptance-terms",
            session,
        )?;
        let digest = check::text(basis, "terms_digest", session)?;
        size(digest.len(), 71, 71, session)?;
        session
            .semantic_work()
            .scan_bytes(digest.len())
            .map_err(core_error)?;
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        for byte in hex.bytes() {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
                .map_err(core_error)?;
            require(
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
                "INVALID_SHAPE",
            )?;
        }
    } else {
        check::same_text(kind, "supplier_commitment", session)?;
        check::closed(
            basis,
            &["kind", "commitment_ref", "offer_ref", "supplier_terms_ref"],
            session,
        )?;
        for (name, kind) in [
            ("commitment_ref", "workflow-offer"),
            ("offer_ref", "workflow-offer"),
            ("supplier_terms_ref", "workflow-slot-terms"),
        ] {
            ref_parts(check::field(basis, name, session)?, kind, session)?;
        }
    }
    at_most(&time(value, "accepted_at", session)?, at, session)
}
async fn evidence_inputs(
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    payload: &Value,
    actor: &str,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<[Option<PartyRecord>; 2]> {
    check::closed(
        payload,
        &[
            "schema",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
        ],
        session,
    )?;
    let subject_value = check::field(payload, "subject", session)?;
    subject(subject_value, session)?;
    let encoded = check::text(payload, "content_base64", session)?;
    size(encoded.len(), 1, 5464, session)?;
    let raw = session
        .decode_base64_representation(encoded, RepresentationKind::EvidenceContent)
        .map_err(core_error)?;
    size(raw.bytes().len(), 1, 4096, session)?;
    let hash = session
        .hash_representation(raw.bytes(), RepresentationKind::EvidenceContent)
        .map_err(core_error)?;
    check::digest_text(
        &hash,
        check::text(payload, "content_sha256", session)?,
        false,
        session,
    )?;
    let media = check::text(payload, "media_type", session)?;
    if equal_text(media, "text/plain;charset=utf-8", session)? {
        text(raw.bytes(), session)?;
    } else {
        check::same_text(media, "application/octet-stream", session)?;
    }
    let metadata = check::field(payload, "metadata", session)?;
    let purpose = check::text(metadata, "purpose", session)?;
    let commitment = commitment_payload(payload, session)?;
    if commitment {
        require(
            !equal_text(purpose, "party_authorization", session)?,
            "INVALID_SHAPE",
        )?;
    }
    if equal_text(purpose, "party_authorization", session)? {
        check::closed(metadata, &["purpose", "authorization"], session)?;
        let spec = authorization_spec(
            check::field(metadata, "authorization", session)?,
            req.key(),
            session,
        )?;
        same_subject(subject_value, spec, session)?;
        Ok([None, None])
    } else if equal_text(purpose, "party_consent", session)? {
        check::closed(metadata, &["purpose", "consent"], session)?;
        let spec = check::field(metadata, "consent", session)?;
        consent(spec, req.key(), at, session)?;
        if commitment {
            check::same_text(
                check::text(check::field(spec, "basis", session)?, "kind", session)?,
                "acceptance_terms",
                session,
            )?;
        }
        same_subject(subject_value, spec, session)?;
        Ok([None, None])
    } else {
        check::same_text(purpose, "party_operator_attestation", session)?;
        check::closed(
            metadata,
            &["purpose", "target_evidence_ref", "statement"],
            session,
        )?;
        let target = record(
            tx,
            req.key(),
            check::field(metadata, "target_evidence_ref", session)?,
            "workflow-evidence",
            before,
            session,
        )
        .await?;
        let body = evidence_with_schema(
            &target,
            req.key(),
            actor,
            at,
            if commitment {
                "ledger-workflow-party-evidence/2"
            } else {
                "ledger-workflow-party-evidence/1"
            },
            session,
        )?;
        same_value(
            subject_value,
            check::field(body, "subject", session)?,
            session,
        )?;
        let target_metadata = check::field(body, "metadata", session)?;
        let target_purpose = check::text(target_metadata, "purpose", session)?;
        if commitment {
            check::same_text(target_purpose, "party_consent", session)?;
        }
        let expected = if equal_text(target_purpose, "party_authorization", session)? {
            check::closed(target_metadata, &["purpose", "authorization"], session)?;
            authorization_spec(
                check::field(target_metadata, "authorization", session)?,
                req.key(),
                session,
            )?;
            "operator_verified_party_acceptor_authority"
        } else {
            check::same_text(target_purpose, "party_consent", session)?;
            check::closed(target_metadata, &["purpose", "consent"], session)?;
            consent(
                check::field(target_metadata, "consent", session)?,
                req.key(),
                at,
                session,
            )?;
            "operator_verified_party_exact_consent"
        };
        check::same_text(
            check::text(metadata, "statement", session)?,
            expected,
            session,
        )?;
        Ok([Some(target), None])
    }
}

/// Immediate immutable consent operands. These are distinct from the two
/// evidence-only management-authority inputs and remain owning manifest members.
/// A missing public prerequisite is refused; no purported basis is fabricated.
async fn consent_basis_inputs(
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    dependencies: &[Option<PartyRecord>; 2],
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<[Option<PartyRecord>; 4]> {
    let mut inputs = std::array::from_fn(|_| None);
    let mut metadata = check::field(payload, "metadata", session)?;
    if commitment_payload(payload, session)?
        && equal_text(
            check::text(metadata, "purpose", session)?,
            "party_operator_attestation",
            session,
        )?
    {
        let target = dependencies[0]
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        check::same_text(
            check::text(target.body(), "schema", session)?,
            "ledger-workflow-party-evidence/2",
            session,
        )?;
        metadata = check::field(target.body(), "metadata", session)?;
        check::same_text(
            check::text(metadata, "purpose", session)?,
            "party_consent",
            session,
        )?;
    }
    if !equal_text(
        check::text(metadata, "purpose", session)?,
        "party_consent",
        session,
    )? {
        return Ok(inputs);
    }
    let consent = check::field(metadata, "consent", session)?;
    let authorization = record(
        tx,
        key,
        check::field(consent, "authorization_ref", session)?,
        "workflow-authority",
        before,
        session,
    )
    .await?;
    check::same_text(
        check::text(authorization.body(), "schema", session)?,
        "ledger-workflow-party-authorization/1",
        session,
    )?;
    if commitment_payload(payload, session)? {
        let spec = check::field(authorization.body(), "authorization", session)?;
        same_subject(consent, spec, session)?;
        let selected = roles(consent, session)?;
        let allowed = roles(spec, session)?;
        for role in selected {
            let role = role.as_str().ok_or(ServiceError::IntegrityFailure)?;
            let mut found = false;
            for candidate in allowed {
                if equal_text(
                    role,
                    candidate.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    session,
                )? {
                    found = true;
                    break;
                }
            }
            require(found, "AUTHORITY_REQUIRED")?;
        }
        let accepted = time(consent, "accepted_at", session)?;
        at_most(&time(spec, "starts_at", session)?, &accepted, session)?;
        let end = time(spec, "ends_at", session)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&accepted.micros().into(), &end.micros().into()],
            )
            .map_err(core_error)?;
        require(accepted.micros() < end.micros(), "AUTHORITY_REQUIRED")?;
        // The only deferred economic edge. Authentication material is still
        // real prior custody and included in this operation's exact manifest.
        let (original, evidence, attestation) = original_authorization(
            tx,
            key,
            check::field(consent, "authorization_ref", session)?,
            &accepted,
            before,
            session,
        )
        .await?;
        inputs[0] = Some(original);
        inputs[1] = Some(evidence);
        inputs[2] = Some(attestation);
        return Ok(inputs);
    }
    inputs[0] = Some(authorization);
    let basis = check::field(consent, "basis", session)?;
    if equal_text(
        check::text(basis, "kind", session)?,
        "acceptance_terms",
        session,
    )? {
        let terms = record(
            tx,
            key,
            check::field(basis, "terms_ref", session)?,
            "workflow-acceptance-terms",
            before,
            session,
        )
        .await?;
        let schema = check::text(terms.body(), "schema", session)?;
        require(
            equal_text(schema, "ledger-workflow-acceptance-terms/1", session)?
                || equal_text(schema, "ledger-workflow-acceptance-terms/2", session)?,
            "INVALID_SHAPE",
        )?;
        inputs[1] = Some(terms);
    } else {
        check::same_text(
            check::text(basis, "kind", session)?,
            "supplier_commitment",
            session,
        )?;
        for (index, field, kind, schema) in [
            (
                1,
                "commitment_ref",
                "workflow-offer",
                "ledger-workflow-supplier-commitment/1",
            ),
            (
                2,
                "offer_ref",
                "workflow-offer",
                "ledger-workflow-supplier-offer/3",
            ),
            (
                3,
                "supplier_terms_ref",
                "workflow-slot-terms",
                "ledger-workflow-slot-binding-terms/5",
            ),
        ] {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            let retained = record(
                tx,
                key,
                check::field(basis, field, session)?,
                kind,
                before,
                session,
            )
            .await?;
            check::same_text(
                check::text(retained.body(), "schema", session)?,
                schema,
                session,
            )?;
            session
                .semantic_work()
                .write_bytes(std::mem::size_of::<Option<PartyRecord>>())
                .map_err(core_error)?;
            inputs[index] = Some(retained);
        }
    }
    Ok(inputs)
}
#[allow(clippy::too_many_arguments)]
async fn authorization_evidence(
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    payload: &Value,
    actor: &str,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<[Option<PartyRecord>; 2]> {
    check::closed(
        payload,
        &[
            "schema",
            "expected_revision",
            "authorization",
            "evidence_ref",
            "operator_attestation_ref",
        ],
        session,
    )?;
    check::same_text(
        check::text(payload, "expected_revision", session)?,
        "0",
        session,
    )?;
    let spec = authorization_spec(
        check::field(payload, "authorization", session)?,
        req.key(),
        session,
    )?;
    let retained = record(
        tx,
        req.key(),
        check::field(payload, "evidence_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let attestation = record(
        tx,
        req.key(),
        check::field(payload, "operator_attestation_ref", session)?,
        "workflow-evidence",
        before,
        session,
    )
    .await?;
    let body = evidence(&retained, req.key(), actor, at, session)?;
    let attested = evidence(&attestation, req.key(), actor, at, session)?;
    for body in [body, attested] {
        same_subject(check::field(body, "subject", session)?, spec, session)?;
    }
    let metadata = check::field(body, "metadata", session)?;
    check::closed(metadata, &["purpose", "authorization"], session)?;
    check::same_text(
        check::text(metadata, "purpose", session)?,
        "party_authorization",
        session,
    )?;
    same_value(
        check::field(metadata, "authorization", session)?,
        spec,
        session,
    )?;
    let metadata = check::field(attested, "metadata", session)?;
    check::closed(
        metadata,
        &["purpose", "target_evidence_ref", "statement"],
        session,
    )?;
    check::same_text(
        check::text(metadata, "purpose", session)?,
        "party_operator_attestation",
        session,
    )?;
    check::same_text(
        check::text(metadata, "statement", session)?,
        "operator_verified_party_acceptor_authority",
        session,
    )?;
    same_value(
        check::field(metadata, "target_evidence_ref", session)?,
        check::field(payload, "evidence_ref", session)?,
        session,
    )?;
    at_most(
        &time(body, "retained_at", session)?,
        &time(attested, "retained_at", session)?,
        session,
    )?;
    Ok([Some(retained), Some(attestation)])
}
#[allow(clippy::too_many_arguments)]
async fn authorization_inputs(
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    payload: &Value,
    actor: &str,
    at: &Timestamp,
    before: i64,
    session: &mut OriginalAuditSession,
) -> Result<[Option<PartyRecord>; 2]> {
    let dependencies = authorization_evidence(tx, req, payload, actor, at, before, session).await?;
    let spec = check::field(payload, "authorization", session)?;
    let head = tx
        .workflow_party_authorization_head(
            &PartyHeadKey {
                store: &req.key().logical_store_id,
                scope: &req.key().scope,
                customer: check::text(spec, "customer", session)?,
                agreement: check::text(spec, "agreement_id", session)?,
                authorization_id: check::text(spec, "authorization_id", session)?,
            },
            session,
        )
        .await
        .map_err(prefix::read_error)?;
    require(head.is_none(), "SEMANTIC_CONFLICT")?;
    Ok(dependencies)
}
fn dependency_count(
    dependencies: &[Option<PartyRecord>; 2],
    session: &mut OriginalAuditSession,
) -> Result<usize> {
    let mut count = 0usize;
    for record in dependencies {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        if record.is_some() {
            count = number(count, 1, Op::Add, session)?;
        }
    }
    Ok(count)
}

fn management_authority(
    req: &IncomingRequest,
    root: &wire::SourceRecord,
    actor: &str,
    at: &Timestamp,
    dependencies: &[Option<PartyRecord>; 2],
    subject: &Value,
    session: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let actor = wire::string(actor, session)?;
    let customer = encode(check::field(subject, "customer", session)?, session)?;
    let agreement = encode(check::field(subject, "agreement_id", session)?, session)?;
    let scope = encode(
        check::field(check::field(req.value(), "key", session)?, "scope", session)?,
        session,
    )?;
    let root = wire::reference(root.reference(), session)?;
    let refs = wire::RefSet::new(
        dependencies
            .iter()
            .filter_map(|d| d.as_ref().map(|d| &d.reference)),
        dependency_count(dependencies, session)?,
        true,
        session,
    )?
    .encode(session)?;
    let at = wire::string(at.as_str(), session)?;
    let mode = wire::string("real_local", session)?;
    let operation = wire::string(req.kind(), session)?;
    let schema = wire::string("ledger-workflow-party-management-authority/1", session)?;
    let body = wire::object(
        &[
            ("actor", actor.bytes()),
            ("agreement_id", agreement.bytes()),
            ("customer", customer.bytes()),
            ("evidence_refs", refs.bytes()),
            ("installation_mode", mode.bytes()),
            ("local_admin_ref", root.bytes()),
            ("observed_at", at.bytes()),
            ("operation", operation.bytes()),
            ("schema", schema.bytes()),
            ("scope", scope.bytes()),
        ],
        65536,
        session,
    )?;
    wire::source_record(
        "workflow-authority",
        "ledger-workflow-party-management-authority/1",
        body,
        session,
    )
}
/// Actual locked management input acquisition and canonical result construction.
/// Full chronological origins must complete before these bytes can be prepared.
pub(in super::super::super) async fn prepare(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    let result = async {
        let management = ledger
            .admin
            .fresh_management(&ledger.store, tx, req.key(), session)
            .await?;
        prepare_inner(ledger, tx, req, &management, session).await
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
/// Same producer, borrowing the actual unchanged held management snapshot.
pub(in super::super::super) async fn prepare_observed(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    management: &crate::service::workflow::local::OwnedLocalManagement,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    let result = async {
        ledger
            .admin
            .recheck_management_fresh(management, &ledger.store, tx, req.key(), session)
            .await?;
        prepare_inner(ledger, tx, req, management, session).await
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
/// New local entry only. Its caller selects the exact new journal registry;
/// no old audit tuple is reclassified and no resource/origin flag is returned.
pub(in super::super::super) async fn prepare_local_observed(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    management: &crate::service::workflow::local::OwnedLocalManagement,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    let result = async {
        ledger
            .admin
            .recheck_management_fresh(management, &ledger.store, tx, req.key(), session)
            .await?;
        prepare_selected_inner(
            ledger,
            tx,
            req,
            management,
            PartyPreparation::LocalSelected,
            session,
        )
        .await
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
enum PartyPreparation {
    Chronological,
    LocalSelected,
}
async fn prepare_inner(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    management: &crate::service::workflow::local::OwnedLocalManagement,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    prepare_selected_inner(
        ledger,
        tx,
        req,
        management,
        PartyPreparation::Chronological,
        session,
    )
    .await
}
async fn prepare_selected_inner(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &IncomingRequest,
    management: &crate::service::workflow::local::OwnedLocalManagement,
    preparation: PartyPreparation,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    session.check_cancelled().map_err(core_error)?;
    let setup = management.setup();
    // The two fixed-width calendar renderings and their String growth overlap
    // remain owned through the produced PendingPartyOperation timestamp.
    const CLOCK_BYTES: usize = 3 * 2 * 27
        + 2 * std::mem::size_of::<Timestamp>()
        + std::mem::size_of::<std::time::Duration>();
    let clock_owner = session
        .reserve_external_payload(CLOCK_BYTES)
        .map_err(core_error)?;
    session
        .semantic_work()
        .write_bytes(CLOCK_BYTES)
        .map_err(core_error)?;
    let at = crate::local::now_with_work(session.semantic_work()).map_err(|error| match error {
        crate::local::LocalClockError::Unavailable => ServiceError::Unavailable,
        crate::local::LocalClockError::Resource(error) => core_error(error),
    })?;
    let before = tx
        .workflow_management_sequence(session)
        .await
        .map_err(prefix::read_error)?;
    let normalized_payload = super::normalize::payload(req.payload(session)?, req.key(), session)?;
    let payload = normalized_payload.value();
    let schema = check::text(payload, "schema", session)?;
    let is_evidence = evidence_payload(schema, session)?;
    let is_revoke = equal_text(schema, "ledger-workflow-party-revoke/1", session)?;
    let mut prior = None;
    let dependencies = if is_revoke {
        check::same_text(req.kind(), "authority.change", session)?;
        check::closed(
            payload,
            &["schema", "authorization_ref", "expected_revision", "reason"],
            session,
        )?;
        check::same_text(
            check::text(payload, "expected_revision", session)?,
            "1",
            session,
        )?;
        id(payload, "reason", 1024, session)?;
        let (original, evidence, attestation) = original_authorization(
            tx,
            req.key(),
            check::field(payload, "authorization_ref", session)?,
            &at,
            before,
            session,
        )
        .await?;
        let spec = check::field(original.body(), "authorization", session)?;
        let current = tx
            .workflow_party_authorization_head(
                &PartyHeadKey {
                    store: &req.key().logical_store_id,
                    scope: &req.key().scope,
                    customer: check::text(spec, "customer", session)?,
                    agreement: check::text(spec, "agreement_id", session)?,
                    authorization_id: check::text(spec, "authorization_id", session)?,
                },
                session,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or_else(|| reject("STALE_REVISION"))?;
        for (actual, expected) in [
            (current.revision.bytes(), "1"),
            (current.kind.bytes(), original.reference.kind.as_str()),
            (current.id.bytes(), original.reference.id.as_str()),
            (current.digest.bytes(), original.reference.digest.as_str()),
        ] {
            check::same_text(text(actual, session)?, expected, session)?;
        }
        prior = Some(original);
        [Some(evidence), Some(attestation)]
    } else if is_evidence {
        check::same_text(req.kind(), "evidence.retain", session)?;
        evidence_inputs(tx, req, payload, &setup.operator, &at, before, session).await?
    } else {
        check::same_text(schema, "ledger-workflow-party-authorize/1", session)?;
        check::same_text(req.kind(), "authority.change", session)?;
        authorization_inputs(tx, req, payload, &setup.operator, &at, before, session).await?
    };
    if is_evidence && commitment_payload(payload, session)? {
        let metadata = check::field(payload, "metadata", session)?;
        if equal_text(
            check::text(metadata, "purpose", session)?,
            "party_consent",
            session,
        )? {
            let consent = check::field(metadata, "consent", session)?;
            let current = observe(
                tx,
                req.key(),
                check::field(consent, "authorization_ref", session)?,
                &at,
                before,
                session,
            )
            .await?;
            // Read/validate under this held writer; the original material is
            // retained separately by consent_basis_inputs below.
            drop(current);
        }
    }
    let consent_basis = if is_evidence {
        consent_basis_inputs(tx, req.key(), payload, &dependencies, before, session).await?
    } else {
        std::array::from_fn(|_| None)
    };
    let scope_value = check::field(check::field(req.value(), "key", session)?, "scope", session)?;
    let root = root(&setup, scope_value, &at, session)?;
    let mut pending = construct(
        req,
        payload,
        root,
        &setup.operator,
        at,
        before,
        dependencies,
        consent_basis,
        prior,
        session,
    )?;
    match preparation {
        PartyPreparation::Chronological => {
            let mut earlier =
                prefix::party_before_observed(ledger, tx, req.key(), before, management, session)
                    .await?;
            pending.check_prestate(earlier.as_mut().map(|p| &mut p._inventory), &setup, session)?;
            pending._prefix = earlier;
        }
        PartyPreparation::LocalSelected => {
            // Exact current head/absence and active mapping/interval were read
            // above under this held writer. Required retained dependencies are
            // now checked directly; unrelated history is not an admission cut.
            let root = session
                .parse_representation(pending.root.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            prefix::source::custody(root.value(), setup, session)?;
            if let Some(authorization) = pending.consent_basis[0].as_ref() {
                // Both consent evidence and its later operator attestation
                // require the actual active mapping at THIS retention time.
                let reference = wire::reference(&authorization.reference, session)?;
                let reference = session
                    .parse_representation(reference.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                observe(
                    tx,
                    req.key(),
                    reference.value(),
                    &pending.at,
                    before,
                    session,
                )
                .await?;
            }
            let members = prefix::ResolvedMembers::resolve_local_party(
                tx,
                req.key(),
                before,
                &[&pending.root, &pending.authority, &pending.result],
                pending.dependency_refs(),
                session,
            )
            .await?;
            pending.local_members = Some(members);
        }
    }
    pending._clock_owner = Some(clock_owner);
    Ok(pending)
}

#[allow(clippy::too_many_arguments)]
fn construct(
    req: &IncomingRequest,
    payload: &Value,
    root: wire::SourceRecord,
    actor: &str,
    at: Timestamp,
    before: i64,
    dependencies: [Option<PartyRecord>; 2],
    consent_basis: [Option<PartyRecord>; 4],
    prior: Option<PartyRecord>,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    let is_evidence = evidence_payload(check::text(payload, "schema", session)?, session)?;
    let scope_value = check::field(check::field(req.value(), "key", session)?, "scope", session)?;
    let subject = if let Some(prior) = &prior {
        check::field(prior.body(), "authorization", session)?
    } else if is_evidence {
        check::field(payload, "subject", session)?
    } else {
        check::field(payload, "authorization", session)?
    };
    let authority = management_authority(req, &root, actor, &at, &dependencies, subject, session)?;
    let authority_ref = wire::reference(authority.reference(), session)?;
    let mode = wire::string("real_local", session)?;
    let actor = wire::string(actor, session)?;
    let timestamp = wire::string(at.as_str(), session)?;
    let body = if is_evidence {
        let content = encode(check::field(payload, "content_base64", session)?, session)?;
        let hash = encode(check::field(payload, "content_sha256", session)?, session)?;
        let media = encode(check::field(payload, "media_type", session)?, session)?;
        let metadata = encode(check::field(payload, "metadata", session)?, session)?;
        let subject = encode(check::field(payload, "subject", session)?, session)?;
        let scope = encode(scope_value, session)?;
        let schema = wire::string(
            if commitment_payload(payload, session)? {
                "ledger-workflow-party-evidence/2"
            } else {
                "ledger-workflow-party-evidence/1"
            },
            session,
        )?;
        wire::object(
            &[
                ("authority_ref", authority_ref.bytes()),
                ("content_base64", content.bytes()),
                ("content_sha256", hash.bytes()),
                ("installation_mode", mode.bytes()),
                ("media_type", media.bytes()),
                ("metadata", metadata.bytes()),
                ("retained_at", timestamp.bytes()),
                ("retained_by", actor.bytes()),
                ("schema", schema.bytes()),
                ("scope", scope.bytes()),
                ("subject", subject.bytes()),
            ],
            262144,
            session,
        )?
    } else {
        let original = prior.as_ref().map(|r| r.body()).unwrap_or(payload);
        let authorization = encode(check::field(original, "authorization", session)?, session)?;
        let evidence = encode(check::field(original, "evidence_ref", session)?, session)?;
        let attestation = encode(
            check::field(original, "operator_attestation_ref", session)?,
            session,
        )?;
        let schema = wire::string("ledger-workflow-party-authorization/1", session)?;
        if let Some(prior) = &prior {
            let prior_ref = wire::reference(&prior.reference, session)?;
            wire::object(
                &[
                    ("authority_ref", authority_ref.bytes()),
                    ("authorization", authorization.bytes()),
                    ("evidence_ref", evidence.bytes()),
                    ("installation_mode", mode.bytes()),
                    ("operator_attestation_ref", attestation.bytes()),
                    ("prior_ref", prior_ref.bytes()),
                    ("recorded_at", timestamp.bytes()),
                    ("recorded_by", actor.bytes()),
                    ("revision", b"\"2\""),
                    ("schema", schema.bytes()),
                    ("status", b"\"revoked\""),
                ],
                65536,
                session,
            )?
        } else {
            wire::object(
                &[
                    ("authority_ref", authority_ref.bytes()),
                    ("authorization", authorization.bytes()),
                    ("evidence_ref", evidence.bytes()),
                    ("installation_mode", mode.bytes()),
                    ("operator_attestation_ref", attestation.bytes()),
                    ("recorded_at", timestamp.bytes()),
                    ("recorded_by", actor.bytes()),
                    ("revision", b"\"1\""),
                    ("schema", schema.bytes()),
                    ("status", b"\"active\""),
                ],
                65536,
                session,
            )?
        }
    };
    let result = wire::source_record(
        if is_evidence {
            "workflow-evidence"
        } else {
            "workflow-authority"
        },
        if is_evidence {
            if commitment_payload(payload, session)? {
                "ledger-workflow-party-evidence/2"
            } else {
                "ledger-workflow-party-evidence/1"
            }
        } else {
            "ledger-workflow-party-authorization/1"
        },
        body,
        session,
    )?;
    let normalized = encode(payload, session)?;
    session.check_cancelled().map_err(core_error)?;
    Ok(PendingPartyOperation {
        root,
        authority,
        result,
        normalized,
        dependencies,
        consent_basis,
        prior,
        at,
        before,
        _prefix: None,
        local_members: None,
        _clock_owner: None,
    })
}

/// Saved recipe replay uses captured original material and the SAME constructor.
/// It deliberately has no LocalWorkflowLedger/current mapping argument. Exact
/// historical head/absence and every earlier origin remain driver obligations.
pub(in super::super::super) async fn reconstruct(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    let result = reconstruct_inner(tx, origin, session).await;
    if result.is_err() {
        session.invalidate();
    }
    result
}
async fn reconstruct_inner(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    session: &mut OriginalAuditSession,
) -> Result<PendingPartyOperation> {
    session
        .semantic_work()
        .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
        .map_err(core_error)?;
    require(
        origin.wrappers().class() == wrappers::WireClass::Prerequisite,
        "INTEGRITY_FAILURE",
    )?;
    let request = IncomingRequest::parse(origin.wrappers().original_request().raw(), session)?;
    check::key(
        check::field(request.value(), "key", session)?,
        origin.key(),
        session,
    )?;
    let normalized_payload =
        super::normalize::payload(request.payload(session)?, request.key(), session)?;
    let payload = normalized_payload.value();
    let schema = check::text(payload, "schema", session)?;
    let is_evidence = evidence_payload(schema, session)?;
    let is_revoke = equal_text(schema, "ledger-workflow-party-revoke/1", session)?;
    if is_evidence {
        check::same_text(request.kind(), "evidence.retain", session)?;
    } else {
        check::same_text(request.kind(), "authority.change", session)?;
        if !is_revoke {
            check::same_text(schema, "ledger-workflow-party-authorize/1", session)?;
        }
    }
    let operation = origin.wrappers().operation();
    let at = time(operation, "accepted_at", session)?;
    let before = origin.wrappers().sequence();
    session
        .semantic_work()
        .arithmetic(Op::Add, &[&before.into(), &1i64.into()])
        .map_err(core_error)?;
    let own_before = before
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let management = check::field(operation, "authority_snapshot", session)?;
    check::closed(
        management,
        &[
            "schema",
            "operation",
            "actor",
            "scope",
            "customer",
            "agreement_id",
            "local_admin_ref",
            "evidence_refs",
            "observed_at",
            "installation_mode",
        ],
        session,
    )?;
    check::same_text(
        check::text(management, "schema", session)?,
        "ledger-workflow-party-management-authority/1",
        session,
    )?;
    check::same_text(
        check::text(management, "operation", session)?,
        request.kind(),
        session,
    )?;
    check::same_text(
        check::text(management, "installation_mode", session)?,
        "real_local",
        session,
    )?;
    scope(
        check::field(management, "scope", session)?,
        request.key(),
        session,
    )?;
    let actor = id(management, "actor", 128, session)?;
    same_value(
        check::field(management, "observed_at", session)?,
        check::field(operation, "accepted_at", session)?,
        session,
    )?;
    let root_ref = check::field(management, "local_admin_ref", session)?;
    origin.require_member(root_ref, session)?;
    let retained_root = record(
        tx,
        request.key(),
        root_ref,
        "workflow-authority",
        own_before,
        session,
    )
    .await?;
    let body = retained_root.body();
    validate_original_root(body, request.key(), actor, &at, session)?;
    let root_bytes = session
        .copy_representation(
            retained_root.stored.body.bytes(),
            RepresentationKind::Authority,
        )
        .map_err(core_error)?;
    let root = wire::source_record(
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1",
        root_bytes,
        session,
    )?;
    let mut prior = None;
    let dependencies = if is_evidence {
        evidence_inputs(tx, &request, payload, actor, &at, before, session).await?
    } else if is_revoke {
        check::closed(
            payload,
            &["schema", "authorization_ref", "expected_revision", "reason"],
            session,
        )?;
        check::same_text(
            check::text(payload, "expected_revision", session)?,
            "1",
            session,
        )?;
        id(payload, "reason", 1024, session)?;
        let (original, evidence, attestation) = original_authorization(
            tx,
            request.key(),
            check::field(payload, "authorization_ref", session)?,
            &at,
            before,
            session,
        )
        .await?;
        prior = Some(original);
        [Some(evidence), Some(attestation)]
    } else {
        authorization_evidence(tx, &request, payload, actor, &at, before, session).await?
    };
    let consent_basis = if is_evidence {
        consent_basis_inputs(tx, request.key(), payload, &dependencies, before, session).await?
    } else {
        std::array::from_fn(|_| None)
    };
    let derived = construct(
        &request,
        payload,
        root,
        actor,
        at,
        before,
        dependencies,
        consent_basis,
        prior,
        session,
    )?;
    let authority = wire::reference(derived.authority.reference(), session)?;
    let authority = session
        .parse_representation(authority.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    origin.require_member(authority.value(), session)?;
    let retained_authority = record(
        tx,
        request.key(),
        authority.value(),
        "workflow-authority",
        own_before,
        session,
    )
    .await?;
    let canonical_management = encode(management, session)?;
    same(
        derived.authority.bytes(),
        canonical_management.bytes(),
        session,
    )?;
    same(
        derived.authority.bytes(),
        retained_authority.stored.body.bytes(),
        session,
    )?;
    let results = check::field(operation, "result_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(results.len(), 1, 1, session)?;
    let result_ref = wire::reference(derived.result.reference(), session)?;
    let result_ref = session
        .parse_representation(result_ref.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    same_value(&results[0], result_ref.value(), session)?;
    origin.require_member(&results[0], session)?;
    let retained_result = record(
        tx,
        request.key(),
        &results[0],
        derived.result.reference().kind.as_str(),
        own_before,
        session,
    )
    .await?;
    same(
        derived.result.bytes(),
        retained_result.stored.body.bytes(),
        session,
    )?;
    // Resource data is returned without crediting a party head or earliest
    // introduction. Those become usable only after the enclosing history driver
    // validates exact original custody, all dependencies and historical prestate.
    Ok(derived)
}

/// SAME retained local-root checks, shared by administrative original replay.
/// The caller still establishes original custody and actual held owner trust.
pub(in super::super::super) fn validate_original_root(
    body: &Value,
    key: &OperationKey,
    actor: &str,
    at: &Timestamp,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let _time_storage = session
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    check::closed(
        body,
        &[
            "schema",
            "basis",
            "basis_digest",
            "verification_profile",
            "verified_at",
        ],
        session,
    )?;
    check::same_text(
        check::text(body, "schema", session)?,
        "ledger-workflow-local-authority-snapshot/1",
        session,
    )?;
    check::same_text(
        check::text(body, "verification_profile", session)?,
        "local-private-directory-owner/1",
        session,
    )?;
    at_most(&time(body, "verified_at", session)?, at, session)?;
    let basis = check::field(body, "basis", session)?;
    check::closed(
        basis,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "rights",
            "source_grant_revision",
        ],
        session,
    )?;
    for (name, expected) in [
        ("schema", "ledger-workflow-local-authority-basis/1"),
        ("logical_store_id", key.logical_store_id.as_str()),
        ("principal_id", actor),
        ("source", key.source.as_str()),
        ("canonical_economic_namespace", key.source.as_str()),
    ] {
        check::same_text(check::text(basis, name, session)?, expected, session)?;
    }
    scope(check::field(basis, "scope", session)?, key, session)?;
    let rights = encode(check::field(basis, "rights", session)?, session)?;
    same(rights.bytes(), b"[\"admin\",\"read\"]", session)?;
    let revision = check::text(basis, "source_grant_revision", session)?;
    size(revision.len(), 1, 20, session)?;
    let mut numeric = 0u64;
    for (index, byte) in revision.bytes().enumerate() {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Constraint)
            .map_err(core_error)?;
        session.semantic_work().scan_bytes(1).map_err(core_error)?;
        require(byte.is_ascii_digit(), "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&index.into(), &0usize.into()])
            .map_err(core_error)?;
        if index == 0 {
            require(byte != b'0', "INTEGRITY_FAILURE")?;
        }
        session
            .semantic_work()
            .arithmetic(Op::Subtract, &[&byte.into(), &b'0'.into()])
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&numeric.into(), &10u8.into()])
            .map_err(core_error)?;
        numeric = numeric
            .checked_mul(10)
            .ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&numeric.into(), &(byte - b'0').into()])
            .map_err(core_error)?;
        numeric = numeric
            .checked_add(u64::from(byte - b'0'))
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    let basis_bytes = encode(basis, session)?;
    let basis_digest = wire::domain_digest(
        b"bean-counter/local-authority-basis/1\0",
        &[basis_bytes.bytes()],
        session,
    )?;
    let supplied = encode(check::field(body, "basis_digest", session)?, session)?;
    same(basis_digest.bytes(), supplied.bytes(), session)?;
    Ok(())
}

impl PendingPartyOperation {
    /// SAME exact body associations and historical mapping predicate for fresh
    /// preparation and original replay. No current SQL head substitutes for
    /// this completed-prefix state, and no result is staged here.
    fn check_prestate(
        &self,
        mut inventory: Option<&mut prefix::inventory::Inventory>,
        setup: &crate::service::billing::Setup,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<prefix::inventory::StateKey>> {
        let root = session
            .parse_representation(self.root.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        prefix::source::custody(root.value(), setup, session)?;
        for record in self
            .dependencies
            .iter()
            .flatten()
            .chain(self.consent_basis.iter().flatten())
            .chain(self.prior.iter())
        {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            inventory
                .as_deref_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .compare_record(&record.reference, record.stored.body.bytes(), session)?;
        }
        let result = session
            .parse_representation(self.result.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let body = result.value();
        let schema = check::text(body, "schema", session)?;
        let mut head = None;
        if equal_text(schema, "ledger-workflow-party-authorization/1", session)? {
            let spec = check::field(body, "authorization", session)?;
            let id = encode(check::field(spec, "authorization_id", session)?, session)?;
            let key = prefix::inventory::StateKey::new("party-head", &[id.bytes()], session)?;
            let inventory = inventory
                .as_deref_mut()
                .ok_or(ServiceError::IntegrityFailure)?;
            if let Some(prior) = &self.prior {
                inventory.match_head(&key, prior.stored.body.bytes(), session)?;
            } else {
                inventory.absent(&key, session)?;
            }
            head = Some(key);
        } else if let Some(authorization) = self.consent_basis[0].as_ref() {
            // Acceptance time and custody time are separate. Consent retention
            // (and its exact target attestation) require active mapping NOW at
            // this ORIGINAL retained_at, not today's head nor accepted_at.
            let authorization_body = authorization.body();
            check::same_text(
                check::text(authorization_body, "schema", session)?,
                "ledger-workflow-party-authorization/1",
                session,
            )?;
            let spec = check::field(authorization_body, "authorization", session)?;
            let id = encode(check::field(spec, "authorization_id", session)?, session)?;
            let key = prefix::inventory::StateKey::new("party-head", &[id.bytes()], session)?;
            inventory
                .as_deref_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .match_head(&key, authorization.stored.body.bytes(), session)?;
            check::same_text(
                check::text(authorization_body, "status", session)?,
                "active",
                session,
            )?;
            at_most(&time(spec, "starts_at", session)?, &self.at, session)?;
            let end = time(spec, "ends_at", session)?;
            session
                .semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&self.at.micros().into(), &end.micros().into()],
                )
                .map_err(core_error)?;
            require(self.at.micros() < end.micros(), "AUTHORITY_REQUIRED")?;
        }
        Ok(head)
    }
    /// Bind the SAME literal replay to the actual preceding completed mapping
    /// and retained bodies. No saved prior_ref selects the historical head.
    pub(in super::super::super) fn fold_original(
        &self,
        origin: &prefix::LocatedWrappers,
        inventory: &mut prefix::inventory::Inventory,
        setup: &crate::service::billing::Setup,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let head = self.check_prestate(Some(inventory), setup, session)?;
        for record in [&self.root, &self.authority, &self.result] {
            session
                .semantic_work()
                .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
                .map_err(core_error)?;
            inventory.introduce(
                record.reference(),
                record.bytes(),
                origin.wrappers().sequence(),
                session,
            )?;
        }
        prefix::source::complete_manifest(inventory, &[&self.result], origin, session)?;
        if let Some(key) = head {
            inventory.set_head(&key, self.result.reference(), session)?;
        }
        Ok(())
    }
}
