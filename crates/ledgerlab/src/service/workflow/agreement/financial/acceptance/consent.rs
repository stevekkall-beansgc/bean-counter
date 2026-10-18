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
            self.finish_resource_consent(
                customer,
                platform,
                publication,
                app_terms,
                local_admin,
                actor,
                at,
                s,
            )
        }
        .await;
        terminal(result, tx, s)
    }
    #[allow(clippy::too_many_arguments)]
    fn finish_resource_consent(
        &self,
        customer: ObservedCustomerConsent,
        platform: current::party::ObservedPlatformConsent,
        publication: &prefix::RecompiledPublication,
        app_terms: &[&ExactRef],
        local_admin: &wire::SourceRecord,
        actor: &str,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<ObservedAcceptanceConsent> {
        let mapping_ref=wire::reference(platform.mapping().reference(),s)?;
        let authorities=self.authority_refs().as_array().ok_or(ServiceError::IntegrityFailure)?;
        let mut nominated=false;
        for r in authorities {let r=encode(r,s)?;if r.bytes()==mapping_ref.bytes(){nominated=true;}}
        require(nominated,"AUTHORITY_REQUIRED")?;
        let party_raw = s
            .copy_representation(platform.assent_bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        let party_assent =
            wire::source_record("workflow-assent", self.party_assent_schema(), party_raw, s)?;
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
        let schema = wire::string(self.consent_verification_schema(), s)?;
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
            self.consent_verification_schema(),
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
    #[allow(clippy::too_many_arguments)]
    async fn original_resource_consent(
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
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<ObservedAcceptanceConsent> {
        let result = async {
            let customer_id = request_wrapper::field(self.terms.canonical_value(), "customer", s)?;
            let registration = prefix
                .original_customer_at(tx, key, customer_id, before, s)
                .await?;
            let customer = self
                .original_customer_consent(
                    tx,
                    key,
                    enrollment,
                    registration,
                    customer_assent,
                    actor,
                    at,
                    before,
                    s,
                )
                .await?;
            let platform = self
                .original_platform_consent(tx, key, resource_leg, party_assent, at, before, s)
                .await?;
            let platform = prefix.original_platform_at(platform, s)?;
            self.finish_resource_consent(
                customer,
                platform,
                publication,
                app_terms,
                local_admin,
                actor,
                at,
                s,
            )
        }
        .await;
        terminal(result, tx, s)
    }
}

/// Actual coverage for every independently declared platform resource leg.
/// Owners retain all material; a coverage vector is still not origin custody.
pub(super) struct Coverage {
    observations: Vec<ObservedAcceptanceConsent>,
    _storage: ExternalBufferLease,
}
struct TermRefs<'a> {
    refs: Vec<&'a ExactRef>,
    _storage: ExternalBufferLease,
}
pub(super) async fn observe_all(
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    documents: &private::InlineDocuments,
    admission: &current::source::ObservedSource,
    root: &wire::SourceRecord,
    actor: &str,
    at: &Timestamp,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<Coverage> {
    observe_all_at(
        tx,
        incoming,
        input,
        publication,
        documents,
        admission,
        root,
        actor,
        at,
        before,
        None,
        s,
    )
    .await
}
pub(super) async fn reconstruct_all(
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    documents: &private::InlineDocuments,
    admission: &current::source::ObservedSource,
    root: &wire::SourceRecord,
    actor: &str,
    at: &Timestamp,
    before: i64,
    prefix: &mut prefix::FoldedInventory,
    s: &mut OriginalAuditSession,
) -> Result<Coverage> {
    observe_all_at(
        tx,
        incoming,
        input,
        publication,
        documents,
        admission,
        root,
        actor,
        at,
        before,
        Some(prefix),
        s,
    )
    .await
}
async fn observe_all_at(
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    input: &AppAcceptanceInput,
    publication: &prefix::RecompiledPublication,
    documents: &private::InlineDocuments,
    admission: &current::source::ObservedSource,
    root: &wire::SourceRecord,
    actor: &str,
    at: &Timestamp,
    before: i64,
    mut historical: Option<&mut prefix::FoldedInventory>,
    s: &mut OriginalAuditSession,
) -> Result<Coverage> {
    let binding = request_wrapper::field(input.terms.canonical_value(), "binding_inputs", s)?;
    let rows = request_wrapper::field(binding, "bindings", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(rows.len(), 1, 16, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&rows.len().into(), &mem::size_of::<&ExactRef>().into()],
        )
        .map_err(core_error)?;
    let n = rows
        .len()
        .checked_mul(mem::size_of::<&ExactRef>())
        .ok_or(ServiceError::Unavailable)?;
    let storage = s.reserve_external_payload(n).map_err(core_error)?;
    let mut terms = TermRefs {
        refs: Vec::new(),
        _storage: storage,
    };
    terms
        .refs
        .try_reserve_exact(rows.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for row in rows {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let reference = documents.reference(request_wrapper::field(row, "binding_ref", s)?, s)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<&ExactRef>())
            .map_err(core_error)?;
        terms.refs.push(reference);
    }
    let legs = request_wrapper::field(binding, "funding_legs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    size(legs.len(), 1, 16, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[
                &legs.len().into(),
                &mem::size_of::<ObservedAcceptanceConsent>().into(),
            ],
        )
        .map_err(core_error)?;
    let n = legs
        .len()
        .checked_mul(mem::size_of::<ObservedAcceptanceConsent>())
        .ok_or(ServiceError::Unavailable)?;
    let storage = s.reserve_external_payload(n).map_err(core_error)?;
    let mut out = Coverage {
        observations: Vec::new(),
        _storage: storage,
    };
    out.observations
        .try_reserve_exact(legs.len())
        .map_err(|_| ServiceError::Unavailable)?;
    let inputs = assents::Inputs::load(tx, incoming.key(), incoming, input, before, s).await?;
    let customer = inputs.select(input, None, s)?;
    for leg in legs {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let mode = request_wrapper::text(leg, "mode", s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(mode.len())
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(8).map_err(core_error)?;
        if mode != "platform" {
            continue;
        }
        let party = request_wrapper::text(leg, "payer", s)?;
        let party_assent = inputs.select(input, Some(party), s)?;
        let observed = if let Some(prefix) = historical.as_deref_mut() {
            input
                .original_resource_consent(
                    tx,
                    incoming.key(),
                    publication,
                    admission.enrollment_record(),
                    &terms.refs,
                    request_wrapper::text(leg, "name", s)?,
                    customer,
                    party_assent,
                    root,
                    actor,
                    at,
                    before,
                    prefix,
                    s,
                )
                .await?
        } else {
            input
                .observe_resource_consent(
                    tx,
                    incoming.key(),
                    publication,
                    admission.enrollment_record(),
                    &terms.refs,
                    request_wrapper::text(leg, "name", s)?,
                    customer,
                    party_assent,
                    root,
                    actor,
                    at,
                    before,
                    s,
                )
                .await?
        };
        let reference = wire::reference(observed.verification().reference(), s)?;
        let mut duplicate = false;
        for prior in &out.observations {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            let existing = wire::reference(prior.verification().reference(), s)?;
            s.semantic_work()
                .scan_bytes(reference.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(existing.bytes().len())
                .map_err(core_error)?;
            if reference.bytes() == existing.bytes() {
                same_bytes(
                    observed.verification().bytes(),
                    prior.verification().bytes(),
                    s,
                )?;
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            s.semantic_work()
                .write_bytes(mem::size_of::<ObservedAcceptanceConsent>())
                .map_err(core_error)?;
            out.observations.push(observed);
        }
    }
    size(out.observations.len(), 1, 16, s)?;
    Ok(out)
}

impl Coverage {
    pub(super) fn selected_refs(&self)->impl Iterator<Item=&ExactRef> {
        self.observations.iter().flat_map(|o|o.customer().records().into_iter().map(|r|r.reference())
            .chain(std::iter::once(o.platform().mapping().reference()))
            .chain(o.platform().mapping().evidence_refs())
            .chain(o.platform().evidence_refs()))
    }
    pub(super) fn generated_records(&self) -> impl Iterator<Item = &wire::SourceRecord> {
        self.observations
            .iter()
            .flat_map(|o| [o.customer().assent(), o.party_assent(), o.verification()])
    }

    /// Select the already normalized, fully checked material for one literal
    /// inline assent. Retained-only material does not enter inline documents.
    pub(super) fn normalized_inline<'a>(
        &'a self,
        raw: &Value,
        input: &AppAcceptanceInput,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<&'a wire::SourceRecord>> {
        let schema = request_wrapper::text(raw, "schema", s)?;
        let customer = assents::equal(schema, "ledger-workflow-assent/1", s)?;
        let platform = assents::equal(schema, input.party_assent_schema(), s)?;
        if !customer && !platform {
            return Ok(None);
        }
        let party = if platform {
            Some(request_wrapper::text(
                request_wrapper::field(raw, "consent", s)?,
                "party",
                s,
            )?)
        } else {
            None
        };
        require(
            assents::Inputs::matches(raw, input, party, s)?,
            "ASSENT_TERMS_MISMATCH",
        )?;
        let mut found: Option<&wire::SourceRecord> = None;
        for observation in &self.observations {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let candidate = if customer {
                observation.customer().assent()
            } else {
                observation.party_assent()
            };
            let body = s
                .parse_representation(candidate.bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            if assents::Inputs::matches(body.value(), input, party, s)? {
                if let Some(prior) = found {
                    same_bytes(prior.bytes(), candidate.bytes(), s)?;
                }
                found = Some(candidate);
            }
        }
        found
            .map(Some)
            .ok_or_else(|| reject("ASSENT_TERMS_MISMATCH"))
    }
    pub(super) fn verification_refs(&self) -> impl ExactSizeIterator<Item = &ExactRef> {
        self.observations
            .iter()
            .map(|o| o.verification().reference())
    }
}

impl Coverage {
    /// Bind every actually observed leg to completed earlier custody and the
    /// same as-of mapping, without conflating inline assents with prior rows.
    pub(super) fn match_prefix(
        &self,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        for observed in &self.observations {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let records = observed.customer().records();
            prefix.match_customer(records[0], s)?;
            for record in records {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                prefix.require_record(record.reference(), s)?;
            }
            prefix.match_party(observed.platform().mapping(), s)?;
            for reference in observed.platform().evidence_refs() {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                prefix.require_record(reference, s)?;
            }
        }
        Ok(())
    }
}

/// Owning current observations from the original accepted fullterms assents.
/// This is a locked mapping view, not a replacement acceptance verification.
pub(in super::super) struct CurrentPlatformConsents {
    observations: Vec<current::party::ObservedPlatformConsent>,
    _storage: ExternalBufferLease,
}
impl CurrentPlatformConsents {
    pub(in super::super) fn mappings(
        &self,
    ) -> impl ExactSizeIterator<Item = &current::party::ObservedPartyAuthorization> {
        self.observations.iter().map(|o| o.mapping())
    }
}
impl Coverage {
    fn accepted_party_assent<'a>(
        &'a self,
        input: &AppAcceptanceInput,
        party: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<&'a wire::SourceRecord> {
        let mut found: Option<&wire::SourceRecord> = None;
        for observation in &self.observations {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let candidate = observation.party_assent();
            let body = s
                .parse_representation(candidate.bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            if assents::Inputs::matches(body.value(), input, Some(party), s)? {
                if let Some(prior) = found {
                    same_bytes(prior.bytes(), candidate.bytes(), s)?;
                }
                found = Some(candidate);
            }
        }
        found.ok_or_else(|| reject("ASSENT_TERMS_MISMATCH"))
    }
    #[allow(clippy::too_many_arguments)]
    async fn observe_platforms_at(
        &self,
        input: &AppAcceptanceInput,
        binding: &Value,
        tx: &mut SqliteTx,
        key: &OperationKey,
        at: &Timestamp,
        before: i64,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
        snapshot: PlatformSnapshot,
    ) -> Result<CurrentPlatformConsents> {
        let result = async {
            let legs = request_wrapper::field(binding, "funding_legs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            size(legs.len(), 1, 16, s)?;
            s.semantic_work()
                .arithmetic(
                    Op::Multiply,
                    &[
                        &legs.len().into(),
                        &mem::size_of::<current::party::ObservedPlatformConsent>().into(),
                    ],
                )
                .map_err(core_error)?;
            let bytes = legs
                .len()
                .checked_mul(mem::size_of::<current::party::ObservedPlatformConsent>())
                .ok_or(ServiceError::Unavailable)?;
            s.semantic_work()
                .arithmetic(
                    Op::Add,
                    &[
                        &bytes.into(),
                        &(2 * mem::size_of::<CurrentPlatformConsents>()).into(),
                    ],
                )
                .map_err(core_error)?;
            let bytes = bytes
                .checked_add(2 * mem::size_of::<CurrentPlatformConsents>())
                .ok_or(ServiceError::Unavailable)?;
            let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
            let mut observations = Vec::new();
            observations
                .try_reserve_exact(legs.len())
                .map_err(|_| ServiceError::Unavailable)?;
            for leg in legs {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                if !assents::equal(request_wrapper::text(leg, "mode", s)?, "platform", s)? {
                    continue;
                }
                let party = request_wrapper::text(leg, "payer", s)?;
                let assent = self.accepted_party_assent(input, party, s)?;
                let parsed = s
                    .parse_representation(assent.bytes(), Kind::SourceDefinition)
                    .map_err(core_error)?;
                // SAME original fullterms verifier; only its held current map
                // and interval are observed at today's captured time.
                let resource_leg = request_wrapper::text(leg, "name", s)?;
                let observed = match snapshot {
                    PlatformSnapshot::Held => {
                        input
                            .platform_consent(tx, key, resource_leg, parsed.value(), at, before, s)
                            .await?
                    }
                    PlatformSnapshot::Original => {
                        let material = input
                            .original_platform_consent(
                                tx,
                                key,
                                resource_leg,
                                parsed.value(),
                                at,
                                before,
                                s,
                            )
                            .await?;
                        prefix.original_platform_at(material, s)?
                    }
                };
                prefix.match_party(observed.mapping(), s)?;
                for reference in observed
                    .mapping()
                    .evidence_refs()
                    .into_iter()
                    .chain(observed.evidence_refs())
                {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    prefix.require_record(reference, s)?;
                }
                s.semantic_work()
                    .write_bytes(mem::size_of::<current::party::ObservedPlatformConsent>())
                    .map_err(core_error)?;
                observations.push(observed);
            }
            size(observations.len(), 1, 16, s)?;
            Ok(CurrentPlatformConsents {
                observations,
                _storage: storage,
            })
        }
        .await;
        terminal(result, tx, s)
    }
}

#[derive(Clone, Copy)]
enum PlatformSnapshot {
    Held,
    Original,
}
impl Coverage {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn observe_current_platforms(
        &self,
        input: &AppAcceptanceInput,
        binding: &Value,
        tx: &mut SqliteTx,
        key: &OperationKey,
        at: &Timestamp,
        before: i64,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<CurrentPlatformConsents> {
        self.observe_platforms_at(
            input,
            binding,
            tx,
            key,
            at,
            before,
            prefix,
            s,
            PlatformSnapshot::Held,
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn reconstruct_activation_platforms(
        &self,
        input: &AppAcceptanceInput,
        binding: &Value,
        tx: &mut SqliteTx,
        key: &OperationKey,
        at: &Timestamp,
        before: i64,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<CurrentPlatformConsents> {
        self.observe_platforms_at(
            input,
            binding,
            tx,
            key,
            at,
            before,
            prefix,
            s,
            PlatformSnapshot::Original,
        )
        .await
    }
}

/// Held local maps are acquired from the exact committed original assent set;
/// each actual map/material is still checked at the captured current time.
pub(super) async fn observe_local_platforms(input:&AppAcceptanceInput,incoming:&ingress::IncomingRequest,
 binding:&Value,tx:&mut SqliteTx,key:&OperationKey,at:&Timestamp,before:i64,s:&mut OriginalAuditSession)->Result<CurrentPlatformConsents>{
 let legs=request_wrapper::field(binding,"funding_legs",s)?.as_array().ok_or(ServiceError::IntegrityFailure)?;
 size(legs.len(),1,16,s)?;
 let storage=s.reserve_external_payload(legs.len()*mem::size_of::<current::party::ObservedPlatformConsent>()).map_err(core_error)?;
 let mut observations=Vec::with_capacity(legs.len());
 let assents=assents::Inputs::load(tx,key,incoming,input,before,s).await?;
 for leg in legs {
  if request_wrapper::text(leg,"mode",s)?!="platform" {continue;}
  let name=request_wrapper::text(leg,"name",s)?;
  let party=request_wrapper::text(leg,"payer",s)?;
  let assent=assents.select(input,Some(party),s)?;
  let observed=input.platform_consent(tx,key,name,assent,at,before,s).await?;
  for r in std::iter::once(observed.mapping().reference()).chain(observed.mapping().evidence_refs()).chain(observed.evidence_refs()) {
   prefix::verify_selected_association(tx,key,r,before,s).await?;
  }
  observations.push(observed);
 }
 size(observations.len(),1,16,s)?;
 Ok(CurrentPlatformConsents{observations,_storage:storage})
}
impl CurrentPlatformConsents{
 pub(in super::super) fn selected_refs(&self)->impl Iterator<Item=&ExactRef>{self.observations.iter().flat_map(|o|std::iter::once(o.mapping().reference()).chain(o.mapping().evidence_refs()).chain(o.evidence_refs()))}
}
