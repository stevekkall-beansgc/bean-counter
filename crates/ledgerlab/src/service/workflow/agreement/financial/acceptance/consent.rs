//! Construct AppConsentVerification1 only from the actual owning customer and
//! locked-current platform observations. This remains DATA until the enclosing
//! chronological/current proof and complete acceptance graph finish.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;

pub(in super::super) struct ObservedAcceptanceConsent {
    customer: ObservedCustomerConsent,
    platform: current::party::ObservedPlatformConsent,
    party_assent: wire::SourceRecord,
    verification: wire::SourceRecord,
}
impl ObservedAcceptanceConsent {
    pub(in super::super) fn verification(&self) -> &wire::SourceRecord {
        &self.verification
    }
    pub(in super::super) fn customer(&self) -> &ObservedCustomerConsent {
        &self.customer
    }
    pub(in super::super) fn platform(&self) -> &current::party::ObservedPlatformConsent {
        &self.platform
    }
    pub(in super::super) fn party_assent(&self) -> &wire::SourceRecord {
        &self.party_assent
    }
}
fn named_reference(
    kind: &str,
    id: &str,
    digest: &str,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let k = wire::string(kind, s)?;
    let i = wire::string(id, s)?;
    let d = wire::string(digest, s)?;
    wire::object(
        &[
            ("digest", d.bytes()),
            ("id", i.bytes()),
            ("kind", k.bytes()),
        ],
        262144,
        s,
    )
}
fn one(reference: &[u8], s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&reference.len().into(), &2usize.into()])
        .map_err(core_error)?;
    let n = reference
        .len()
        .checked_add(2)
        .ok_or(ServiceError::Unavailable)?;
    let (arena, work) = s.parts();
    let mut out = arena.byte_builder(n, work).map_err(core_error)?;
    for part in [b"[" as &[u8], reference, b"]"] {
        out.append(part, s.semantic_work()).map_err(core_error)?;
    }
    out.finish(s.semantic_work()).map_err(core_error)
}
fn head_text<'a>(b: &'a [u8], s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    std::str::from_utf8(b).map_err(|_| ServiceError::IntegrityFailure)
}
impl AppAcceptanceInput {
    /// Terms refs are the exact app slots selected by the SAME core projection
    /// validator, never an acceptance/custody flag. Every ref is independently
    /// required in these normalized binding inputs. The enclosing caller must
    /// cover every declared resource leg and retain all returned owners.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn observe_resource_consent(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        publication: &prefix::RecompiledPublication,
        enrollment: &current::source::SourceRecord,
        app_terms: &[&ExactRef],
        resource_leg: &str,
        customer_assent: &Value,
        party_assent: &Value,
        local_admin: &wire::SourceRecord,
        actor: &str,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<ObservedAcceptanceConsent> {
        let result = async {
            let customer = self
                .customer_consent(tx, key, enrollment, customer_assent, actor, at, before, s)
                .await?;
            let platform = self
                .platform_consent(tx, key, resource_leg, party_assent, at, before, s)
                .await?;
            let party_raw = s
                .copy_representation(platform.assent_bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            let party_assent = wire::source_record(
                "workflow-assent",
                "ledger-workflow-party-assent/1",
                party_raw,
                s,
            )?;
            size(app_terms.len(), 1, 16, s)?;
            let bindings = request_wrapper::field(
                request_wrapper::field(self.terms.canonical_value(), "binding_inputs", s)?,
                "bindings",
                s,
            )?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
            for r in app_terms {
                request_wrapper::same_text(&r.kind, "workflow-slot-terms", s)?;
                let expected = wire::reference(r, s)?;
                let mut found = false;
                for b in bindings {
                    s.semantic_work()
                        .visit(Visit::MemberCompare)
                        .map_err(core_error)?;
                    let actual = encode(request_wrapper::field(b, "binding_ref", s)?, s)?;
                    s.semantic_work()
                        .scan_bytes(expected.bytes().len())
                        .map_err(core_error)?;
                    s.semantic_work()
                        .scan_bytes(actual.bytes().len())
                        .map_err(core_error)?;
                    if expected.bytes() == actual.bytes() {
                        found = true;
                        break;
                    }
                }
                require(found, "INVALID_REFERENCE")?;
            }
            let app_terms =
                wire::RefSet::new(app_terms.iter().copied(), app_terms.len(), true, s)?.encode(s)?;
            let customer_ref = wire::reference(customer.assent().reference(), s)?;
            let customer_refs = one(customer_ref.bytes(), s)?;
            let party_ref = wire::reference(party_assent.reference(), s)?;
            let party_refs = one(party_ref.bytes(), s)?;
            let original_ref = wire::reference(platform.mapping().reference(), s)?;
            let original_refs = one(original_ref.bytes(), s)?;
            let current = platform.mapping().current_head();
            let kind = head_text(current.kind.bytes(), s)?;
            let id = head_text(current.id.bytes(), s)?;
            let digest = head_text(current.digest.bytes(), s)?;
            let current_ref = named_reference(kind, id, digest, s)?;
            let current_refs = one(current_ref.bytes(), s)?;
            // Actual compiled publication/plan, not a schema=>latest switch.
            let declared = encode(
                request_wrapper::field(self.terms.canonical_value(), "publication_ref", s)?,
                s,
            )?;
            let publication_ref = wire::reference(publication.publication().0, s)?;
            same_bytes(declared.bytes(), publication_ref.bytes(), s)?;
            let plan = publication.program().compiled_plan().reference();
            let plan_ref = named_reference(plan.kind(), plan.id(), plan.digest(), s)?;
            let terms = self.terms.reference();
            let terms_ref = named_reference(terms.kind(), terms.id(), terms.digest(), s)?;
            let customer_id = encode(
                request_wrapper::field(self.terms.canonical_value(), "customer", s)?,
                s,
            )?;
            let agreement = encode(
                request_wrapper::field(self.terms.canonical_value(), "agreement_id", s)?,
                s,
            )?;
            let scope = encode(
                request_wrapper::field(self.terms.canonical_value(), "scope", s)?,
                s,
            )?;
            let admin_ref = wire::reference(local_admin.reference(), s)?;
            let actor = wire::string(actor, s)?;
            let at = wire::string(at.as_str(), s)?;
            let schema = wire::string("ledger-workflow-app-consent-verification/1", s)?;
            let body = wire::object(
                &[
                    ("acceptance_terms_ref", terms_ref.bytes()),
                    ("agreement_id", agreement.bytes()),
                    ("app_terms_refs", app_terms.bytes()),
                    ("compiled_plan_ref", plan_ref.bytes()),
                    ("current_party_authorization_refs", current_refs.bytes()),
                    ("customer", customer_id.bytes()),
                    ("customer_assent_refs", customer_refs.bytes()),
                    ("installation_mode", b"\"real_local\""),
                    ("local_admin_ref", admin_ref.bytes()),
                    ("original_party_authorization_refs", original_refs.bytes()),
                    ("party_assent_refs", party_refs.bytes()),
                    ("publication_ref", publication_ref.bytes()),
                    ("schema", schema.bytes()),
                    ("scope", scope.bytes()),
                    ("verified_at", at.bytes()),
                    ("verified_by", actor.bytes()),
                ],
                262144,
                s,
            )?;
            let verification = wire::source_record(
                "workflow-verification",
                "ledger-workflow-app-consent-verification/1",
                body,
                s,
            )?;
            Ok(ObservedAcceptanceConsent {
                customer,
                platform,
                party_assent,
                verification,
            })
        }
        .await;
        terminal(result, tx, s)
    }
}
