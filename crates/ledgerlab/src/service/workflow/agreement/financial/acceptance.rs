//! The actual app acceptance input bridge. Original publication custody and
//! complete source/assent/current admission remain enclosing host obligations;
//! this owning normalized projection cannot manufacture a prepared commit.
use super::*;
mod admission;
mod assents;
mod binding;
mod consent;
mod customer;
mod nominations;
mod private;
pub(super) use customer::ObservedCustomerConsent;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue, ExternalBufferLease},
    policy::workflow::{AgreementRef, OwnedAcceptanceTerms},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as Kind,
        ValidationVisit as Visit,
    },
};
use std::{cmp::Ordering as Cmp, mem};
const APP_CAUSAL: &str = "sha256:42368b84a652527c58389014fefa69140d6a40c2217e193694f162441699cdc2";

pub(super) struct AppAcceptanceInput {
    terms: OwnedAcceptanceTerms,
    assents: AuditedValue,
    authorities: AuditedValue,
    expected_revision: u64,
}
impl AppAcceptanceInput {
    /// `program` must come from the actual completed original publication. Its
    /// immutable code-owned registry is checked independently of payload schema.
    pub(super) fn normalize(
        incoming: &ingress::IncomingRequest,
        publication: &prefix::RecompiledPublication,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let result = Self::normalize_inner(incoming, publication, session);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    fn normalize_inner(
        incoming: &ingress::IncomingRequest,
        publication: &prefix::RecompiledPublication,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        request_wrapper::same_text(incoming.kind(), "agreement.accept", session)?;
        let registry = publication
            .program()
            .execution_registry()
            .ok_or(ServiceError::Unavailable)?;
        request_wrapper::same_text(registry, APP_CAUSAL, session)?;
        let program = publication.program();
        let payload = incoming.payload(session)?;
        request_wrapper::closed(
            payload,
            &[
                "schema",
                "agreement_id",
                "expected_revision",
                "publication_ref",
                "slot_catalog_ref",
                "slot_instantiation_ref",
                "binding_inputs",
                "assent_refs",
                "authority_refs",
                "documents",
            ],
            session,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(payload, "schema", session)?,
            "ledger-workflow-agreement-accept/8",
            session,
        )?;
        let binding = request_wrapper::field(payload, "binding_inputs", session)?;
        let assents = reference_set(
            request_wrapper::field(payload, "assent_refs", session)?,
            "workflow-assent",
            1,
            16,
            session,
        )?;
        let authorities = reference_set(
            request_wrapper::field(payload, "authority_refs", session)?,
            "workflow-source-grant",
            1,
            32,
            session,
        )?;
        let inner_assents = reference_set(
            request_wrapper::field(binding, "assent_refs", session)?,
            "workflow-assent",
            1,
            16,
            session,
        )?;
        let inner_authorities = reference_set(
            request_wrapper::field(binding, "authority_refs", session)?,
            "workflow-source-grant",
            1,
            32,
            session,
        )?;
        same_bytes(assents.bytes(), inner_assents.bytes(), session)?;
        same_bytes(authorities.bytes(), inner_authorities.bytes(), session)?;
        let projected = project_binding(binding, session)?;
        let agreement = encode(
            request_wrapper::field(payload, "agreement_id", session)?,
            session,
        )?;
        let customer = encode(
            request_wrapper::field(binding, "customer", session)?,
            session,
        )?;
        let publication_bytes = encode(
            request_wrapper::field(payload, "publication_ref", session)?,
            session,
        )?;
        validate_reference(
            request_wrapper::field(payload, "publication_ref", session)?,
            "workflow-publication",
            session,
        )?;
        let original_publication = wire::reference(publication.publication().0, session)?;
        same_bytes(
            publication_bytes.bytes(),
            original_publication.bytes(),
            session,
        )?;
        let scope = encode(
            request_wrapper::field(
                request_wrapper::field(incoming.value(), "key", session)?,
                "scope",
                session,
            )?,
            session,
        )?;
        let schema = wire::string("ledger-workflow-acceptance-terms/2", session)?;
        let version = encode(
            request_wrapper::field(binding, "version", session)?,
            session,
        )?;
        let body = wire::object(
            &[
                ("agreement_id", agreement.bytes()),
                ("binding_inputs", projected.bytes()),
                ("customer", customer.bytes()),
                ("publication_ref", publication_bytes.bytes()),
                ("schema", schema.bytes()),
                ("scope", scope.bytes()),
                ("version", version.bytes()),
            ],
            262144,
            session,
        )?;
        let context = program
            .context_schema_with_work(session.semantic_work())
            .map_err(core_error)?;
        let terms =
            OwnedAcceptanceTerms::parse(body.bytes(), context, session).map_err(core_error)?;
        let normalized =
            request_wrapper::field(terms.canonical_value(), "binding_inputs", session)?;
        // Match duplicated exact direct refs AFTER the SAME terms normalizer.
        for name in ["slot_catalog_ref", "slot_instantiation_ref"] {
            let supplied = encode(request_wrapper::field(payload, name, session)?, session)?;
            let normalized = encode(request_wrapper::field(normalized, name, session)?, session)?;
            same_bytes(supplied.bytes(), normalized.bytes(), session)?;
        }
        let expected_revision = counter(
            request_wrapper::text(payload, "expected_revision", session)?,
            session,
        )?;
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&expected_revision.into(), &1u8.into()])
            .map_err(core_error)?;
        let next = expected_revision
            .checked_add(1)
            .ok_or_else(|| reject("INVALID_SCALAR"))?;
        let actual = counter(
            request_wrapper::text(normalized, "version", session)?,
            session,
        )?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&actual.into(), &next.into()])
            .map_err(core_error)?;
        require(actual == next, "INVALID_SHAPE")?;
        // Actual private document normalization/graph joins and host custody
        // still precede preparation. This codec does not silently admit them.
        let docs = request_wrapper::field(payload, "documents", session)?
            .as_array()
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        size(docs.len(), 0, 128, session)?;
        let assents = session
            .parse_representation(assents.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        let authorities = session
            .parse_representation(authorities.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        Ok(Self {
            terms,
            assents,
            authorities,
            expected_revision,
        })
    }
    /// Assemble the actual new binding from the SAME normalized full terms
    /// and retained compiled publication. Actor/time are captured host operands;
    /// this owning DATA still requires the shared core slot validator and full
    /// chronological/current admission before private preparation.
    pub(super) fn binding(
        &self,
        publication: &prefix::RecompiledPublication,
        actor: &str,
        at: &Timestamp,
        session: &mut OriginalAuditSession,
    ) -> Result<wire::SourceRecord> {
        let result = binding::assemble(self, publication, actor, at, session);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    pub(super) fn terms(&self) -> &OwnedAcceptanceTerms {
        &self.terms
    }
    pub(super) fn assent_refs(&self) -> &Value {
        self.assents.value()
    }
    pub(super) fn authority_refs(&self) -> &Value {
        self.authorities.value()
    }
    pub(super) fn expected_revision(&self) -> u64 {
        self.expected_revision
    }
    pub(super) async fn customer_consent(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        enrollment: &current::source::SourceRecord,
        assent: &Value,
        actor: &str,
        at: &Timestamp,
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<ObservedCustomerConsent> {
        let result = async {
            let registration = current::source::customer_record(
                tx,
                key,
                request_wrapper::text(self.terms.canonical_value(), "customer", session)?,
                before,
                session,
            )
            .await?;
            let observed = customer::observe(
                tx,
                key,
                &self.terms,
                enrollment,
                registration,
                assent,
                actor,
                at,
                before,
                session,
            )
            .await?;
            self.require_assent(observed.assent().reference(), session)?;
            Ok(observed)
        }
        .await;
        terminal(result, tx, session)
    }
    /// Original registration is selected from the completed original closure,
    /// never today's registration index or current party/grant mapping.
    pub(super) async fn original_customer_consent(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        enrollment: &current::source::SourceRecord,
        registration: current::source::SourceRecord,
        assent: &Value,
        actor: &str,
        at: &Timestamp,
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<ObservedCustomerConsent> {
        let result = async {
            let observed = customer::observe(
                tx,
                key,
                &self.terms,
                enrollment,
                registration,
                assent,
                actor,
                at,
                before,
                session,
            )
            .await?;
            self.require_assent(observed.assent().reference(), session)?;
            Ok(observed)
        }
        .await;
        terminal(result, tx, session)
    }
    pub(super) async fn original_platform_consent(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        resource_leg: &str,
        assent: &Value,
        at: &Timestamp,
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<current::party::OriginalPlatformConsent> {
        let result = async {
            let observed = current::party::platform_consent_original(
                tx,
                key,
                &self.terms,
                resource_leg,
                assent,
                at,
                before,
                session,
            )
            .await?;
            let bytes = session
                .copy_representation(observed.assent_bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            let record = wire::source_record(
                "workflow-assent",
                "ledger-workflow-party-assent/1",
                bytes,
                session,
            )?;
            self.require_assent(record.reference(), session)?;
            Ok(observed)
        }
        .await;
        terminal(result, tx, session)
    }
    fn require_assent(
        &self,
        reference: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let expected = wire::reference(reference, session)?;
        let mut found = false;
        for candidate in self
            .assents
            .value()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            let actual = encode(candidate, session)?;
            session
                .semantic_work()
                .scan_bytes(expected.bytes().len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            if expected.bytes() == actual.bytes() {
                found = true;
                break;
            }
        }
        require(found, "INVALID_REFERENCE")
    }
    pub(super) async fn platform_consent(
        &self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        resource_leg: &str,
        assent: &Value,
        at: &Timestamp,
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<current::party::ObservedPlatformConsent> {
        let result = async {
            let observed = current::party::platform_consent(
                tx,
                key,
                &self.terms,
                resource_leg,
                assent,
                at,
                before,
                session,
            )
            .await?;
            let bytes = session
                .copy_representation(observed.assent_bytes(), Kind::SourceDefinition)
                .map_err(core_error)?;
            let record = wire::source_record(
                "workflow-assent",
                "ledger-workflow-party-assent/1",
                bytes,
                session,
            )?;
            self.require_assent(record.reference(), session)?;
            Ok(observed)
        }
        .await;
        terminal(result, tx, session)
    }
}
fn terminal<T>(
    result: Result<T>,
    tx: &mut SqliteTx,
    session: &mut OriginalAuditSession,
) -> Result<T> {
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}

fn encode(value: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    s.canonical_representation(value, Kind::SourceDefinition)
        .map_err(core_error)
}
fn size(n: usize, min: usize, max: usize, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &min.into()])
        .map_err(core_error)?;
    require(n >= min, "INVALID_SHAPE")?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &max.into()])
        .map_err(core_error)?;
    require(n <= max, "INVALID_SHAPE")
}
fn same_bytes(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "ORIGINAL_TERMS_MISMATCH")
}
pub(super) fn counter(raw: &str, s: &mut OriginalAuditSession) -> Result<u64> {
    size(raw.len(), 1, 20, s)?;
    let mut n = 0u64;
    for b in raw.bytes() {
        s.semantic_work()
            .visit(Visit::ScalarDigit)
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(1).map_err(core_error)?;
        require(b.is_ascii_digit(), "INVALID_SCALAR")?;
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&n.into(), &10u8.into()])
            .map_err(core_error)?;
        n = n.checked_mul(10).ok_or_else(|| reject("INVALID_SCALAR"))?;
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&b.into(), &b'0'.into()])
            .map_err(core_error)?;
        let d = b - b'0';
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &d.into()])
            .map_err(core_error)?;
        n = n
            .checked_add(u64::from(d))
            .ok_or_else(|| reject("INVALID_SCALAR"))?;
    }
    Ok(n)
}
fn validate_reference(v: &Value, kind: &str, s: &mut OriginalAuditSession) -> Result<()> {
    request_wrapper::closed(v, &["kind", "id", "digest"], s)?;
    let k = request_wrapper::text(v, "kind", s)?;
    let id = request_wrapper::text(v, "id", s)?;
    let d = request_wrapper::text(v, "digest", s)?;
    request_wrapper::same_text(k, kind, s)?;
    let mut n = k.len();
    for x in [id.len(), d.len()] {
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &x.into()])
            .map_err(core_error)?;
        n = n.checked_add(x).ok_or(ServiceError::Unavailable)?;
    }
    let _lease = s.reserve_external_payload(n).map_err(core_error)?;
    let _data =
        AgreementRef::from_parts_with_work(k, id, d, s.semantic_work()).map_err(core_error)?;
    Ok(())
}
struct References<'a> {
    values: Vec<&'a Value>,
    _lease: ExternalBufferLease,
}
fn ref_cmp(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<Cmp> {
    for name in ["digest", "id", "kind"] {
        let a = request_wrapper::text(a, name, s)?;
        let b = request_wrapper::text(b, name, s)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
        s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
        let order = a.cmp(b);
        if order != Cmp::Equal {
            return Ok(order);
        }
    }
    Ok(Cmp::Equal)
}
fn reference_set(
    v: &Value,
    kind: &str,
    min: usize,
    max: usize,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let rows = v.as_array().ok_or_else(|| reject("INVALID_SHAPE"))?;
    size(rows.len(), min, max, s)?;
    s.semantic_work()
        .arithmetic(
            Op::Multiply,
            &[&rows.len().into(), &mem::size_of::<&Value>().into()],
        )
        .map_err(core_error)?;
    let n = rows
        .len()
        .checked_mul(mem::size_of::<&Value>())
        .ok_or(ServiceError::Unavailable)?;
    let lease = s.reserve_external_payload(n).map_err(core_error)?;
    let mut refs = References {
        values: Vec::new(),
        _lease: lease,
    };
    refs.values
        .try_reserve_exact(rows.len())
        .map_err(|_| ServiceError::Unavailable)?;
    for row in rows {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        validate_reference(row, kind, s)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<&Value>())
            .map_err(core_error)?;
        refs.values.push(row);
    }
    let mut i = 1usize;
    loop {
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &refs.values.len().into()])
            .map_err(core_error)?;
        if i >= refs.values.len() {
            break;
        }
        let mut j = i;
        loop {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&j.into(), &0usize.into()])
                .map_err(core_error)?;
            if j == 0 {
                break;
            }
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&j.into(), &1usize.into()])
                .map_err(core_error)?;
            let prior = j - 1;
            if ref_cmp(refs.values[prior], refs.values[j], s)? != Cmp::Greater {
                break;
            }
            s.semantic_work()
                .visit(Visit::SortMove)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(2 * mem::size_of::<&Value>())
                .map_err(core_error)?;
            refs.values.swap(prior, j);
            j = prior;
        }
        s.semantic_work()
            .arithmetic(Op::Add, &[&i.into(), &1usize.into()])
            .map_err(core_error)?;
        i += 1;
    }
    for pair in refs.values.windows(2) {
        require(ref_cmp(pair[0], pair[1], s)? != Cmp::Equal, "DUPLICATE")?;
    }
    // This existing document ceiling bounds the builder only, not valid count.
    let mut out = {
        let (a, w) = s.parts();
        a.byte_builder(262144, w).map_err(core_error)?
    };
    out.append(b"[", s.semantic_work()).map_err(core_error)?;
    for (i, r) in refs.values.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&i.into(), &0usize.into()])
            .map_err(core_error)?;
        if i != 0 {
            out.append(b",", s.semantic_work()).map_err(core_error)?;
        }
        let r = encode(r, s)?;
        out.append(r.bytes(), s.semantic_work())
            .map_err(core_error)?;
    }
    out.append(b"]", s.semantic_work()).map_err(core_error)?;
    out.finish(s.semantic_work()).map_err(core_error)
}
fn project_binding(binding: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    const KEYS: [&str; 15] = [
        "accepted_context",
        "agreement_id",
        "bindings",
        "customer",
        "ends_at",
        "exposure_ceilings",
        "funding_legs",
        "quantity_ceilings",
        "roles_refs",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "source_namespace",
        "source_slots",
        "starts_at",
        "version",
    ];
    let object = binding.as_object().ok_or_else(|| reject("INVALID_SHAPE"))?;
    size(object.len(), 16, 17, s)?; //14 required projected +two assent/authority sets
    let mut has_end = false;
    for (key, _) in object {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let mut allowed = false;
        for expected in KEYS.iter().chain(["assent_refs", "authority_refs"].iter()) {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(key.len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(expected.len())
                .map_err(core_error)?;
            if key == expected {
                allowed = true;
                break;
            }
        }
        require(allowed, "INVALID_SHAPE")?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(key.len())
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(7).map_err(core_error)?;
        if key == "ends_at" {
            has_end = true;
        }
    }
    let mut bytes: [Option<AuditedBytes>; 15] = std::array::from_fn(|_| None);
    for (i, key) in KEYS.iter().enumerate() {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        if *key == "ends_at" && !has_end {
            continue;
        }
        bytes[i] = Some(encode(request_wrapper::field(binding, key, s)?, s)?);
    }
    let mut fields = [("", &[] as &[u8]); 15];
    let mut count = 0usize;
    for (key, value) in KEYS.iter().zip(bytes.iter()) {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        if let Some(value) = value {
            s.semantic_work()
                .write_bytes(mem::size_of::<(&str, &[u8])>())
                .map_err(core_error)?;
            fields[count] = (key, value.bytes());
            s.semantic_work()
                .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            count += 1;
        }
    }
    wire::object(&fields[..count], 262144, s)
}

/// SAME original acceptance input reconstruction. Retained byte equality does
/// not establish introductions, original grant/assent custody or head prestate;
/// this type remains private unresolved DATA, with no receipt/commit conversion.
pub(super) struct ReconstructedAcceptance {
    pub(super) input: AppAcceptanceInput,
    pub(super) binding: wire::SourceRecord,
    _stored_binding: crate::store::sqlite::workflow::financial::RetainedDocument,
    _stored_terms: crate::store::sqlite::workflow::financial::RetainedDocument,
}
pub(super) async fn reconstruct(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    publication: &prefix::RecompiledPublication,
    session: &mut OriginalAuditSession,
) -> Result<ReconstructedAcceptance> {
    let result = reconstruct_inner(tx, origin, publication, session).await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, std::sync::atomic::Ordering::Release);
    }
    result
}
async fn reconstruct_inner(
    tx: &mut SqliteTx,
    origin: &prefix::LocatedWrappers,
    publication: &prefix::RecompiledPublication,
    session: &mut OriginalAuditSession,
) -> Result<ReconstructedAcceptance> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        origin.wrappers().class() == wrappers::WireClass::Agreement,
        "INTEGRITY_FAILURE",
    )?;
    // Use literal original raw evidence; no current retry/PUB/schema substitutes.
    let incoming =
        ingress::IncomingRequest::parse(origin.wrappers().original_request().raw(), session)?;
    request_wrapper::key(
        request_wrapper::field(incoming.value(), "key", session)?,
        origin.key(),
        session,
    )?;
    let input = AppAcceptanceInput::normalize(&incoming, publication, session)?;
    let r = input.terms().reference();
    let mut n = 0usize;
    for part in [r.kind(), r.id(), r.digest()] {
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &part.len().into()])
            .map_err(core_error)?;
        n = n.checked_add(part.len()).ok_or(ServiceError::Unavailable)?;
    }
    let _ref_lease = session.reserve_external_payload(n).map_err(core_error)?;
    let mut copy = |s: &str| -> Result<String> {
        session
            .semantic_work()
            .scan_bytes(s.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(s.len())
            .map_err(core_error)?;
        let mut out = String::new();
        out.try_reserve_exact(s.len())
            .map_err(|_| ServiceError::Unavailable)?;
        out.push_str(s);
        Ok(out)
    };
    let reference = ledgerlab_core::policy::workflow::transition_constructor::ExactRef {
        kind: copy(r.kind())?,
        id: copy(r.id())?,
        digest: copy(r.digest())?,
    };
    let encoded = wire::reference(&reference, session)?;
    let parsed = session
        .parse_representation(encoded.bytes(), Kind::Record)
        .map_err(core_error)?;
    origin.require_member(parsed.value(), session)?;
    let sequence = origin.wrappers().sequence();
    session
        .semantic_work()
        .arithmetic(Op::Add, &[&sequence.into(), &1i64.into()])
        .map_err(core_error)?;
    let before = sequence
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let retained = tx
        .workflow_financial_document(
            &reference,
            &origin.key().logical_store_id,
            &origin.key().scope,
            before,
            session,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    same_bytes(
        retained.schema.bytes(),
        b"ledger-workflow-acceptance-terms/2",
        session,
    )?;
    same_bytes(
        retained.body.bytes(),
        input.terms().canonical_bytes(),
        session,
    )?;
    // Rebuild the actual accepted Binding2 using captured ORIGINAL actor/time,
    // not today's local principal, setup, publication or policy. These saved
    // observations remain DATA until the chronological authority audit finishes.
    let operation = origin.wrappers().operation();
    let authority = request_wrapper::field(operation, "authority_snapshot", session)?;
    request_wrapper::same_text(
        request_wrapper::text(authority, "schema", session)?,
        "ledger-workflow-agreement-authority/5",
        session,
    )?;
    let actor = request_wrapper::text(authority, "actor", session)?;
    let _time_storage = session
        .reserve_external_payload(2 * (4 * 64 + 27))
        .map_err(core_error)?;
    let at = Timestamp::parse_with_work(
        request_wrapper::text(operation, "accepted_at", session)?,
        session.semantic_work(),
    )
    .map_err(core_error)?;
    let observed = Timestamp::parse_with_work(
        request_wrapper::text(authority, "observed_at", session)?,
        session.semantic_work(),
    )
    .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&at.micros().into(), &observed.micros().into()],
        )
        .map_err(core_error)?;
    require(at.micros() == observed.micros(), "INTEGRITY_FAILURE")?;
    let binding = input.binding(publication, actor, &at, session)?;
    let binding_reference = wire::reference(binding.reference(), session)?;
    let binding_value = session
        .parse_representation(binding_reference.bytes(), Kind::Record)
        .map_err(core_error)?;
    origin.require_member(binding_value.value(), session)?;
    let stored_binding = tx
        .workflow_financial_document(
            binding.reference(),
            &origin.key().logical_store_id,
            &origin.key().scope,
            before,
            session,
        )
        .await
        .map_err(prefix::read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    same_bytes(
        stored_binding.schema.bytes(),
        b"ledger-workflow-binding/2",
        session,
    )?;
    same_bytes(stored_binding.body.bytes(), binding.bytes(), session)?;
    Ok(ReconstructedAcceptance {
        input,
        binding,
        _stored_binding: stored_binding,
        _stored_terms: retained,
    })
}

/// Actual fresh public input acquisition. This retains the original compiler
/// operands and SAME normalized binding; it deliberately carries no complete
/// origin/preparation capability. The remaining graph/consent/current checks
/// consume these owners before any journal append may be prepared.
pub(super) struct PendingAppAcceptance {
    pub(super) input: AppAcceptanceInput,
    pub(super) binding: wire::SourceRecord,
    binding_value: AuditedValue,
    pub(super) local_root: wire::SourceRecord,
    pub(super) publication: prefix::RecompiledPublication,
    pub(super) at: Timestamp,
    pub(super) before: i64,
    pub(super) sources: Vec<current::source::ObservedSource>,
    admission_source: usize,
    consents: consent::Coverage,
    authority: wire::SourceRecord,
    pub(super) private_documents: private::InlineDocuments,
    _sources_storage: ExternalBufferLease,
    _time_storage: ExternalBufferLease,
}
pub(super) async fn prepare(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    session: &mut OriginalAuditSession,
) -> Result<PendingAppAcceptance> {
    let result = async {
        session.check_cancelled().map_err(core_error)?;
        let setup = ledger
            .admin
            .fresh(&ledger.store, tx, incoming.key())
            .await?;
        let installation = tx
            .load_installation()
            .await
            .map_err(crate::service::store_error)?;
        require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
        let before = tx
            .workflow_sequence()
            .await
            .map_err(crate::service::store_error)?;
        let time_storage = session
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
        let payload = incoming.payload(session)?;
        let publication = prefix::publication_for(
            tx,
            incoming.key(),
            request_wrapper::field(payload, "publication_ref", session)?,
            before,
            session,
        )
        .await?;
        let input = AppAcceptanceInput::normalize(incoming, &publication, session)?;
        let binding = input.binding(&publication, &setup.operator, &at, session)?;
        // SAME local snapshot construction used by actual party management.
        // The source is the held private owner observation, never request DATA.
        let local_root = current::party::produce::root(
            &setup,
            request_wrapper::field(
                request_wrapper::field(incoming.value(), "key", session)?,
                "scope",
                session,
            )?,
            &at,
            session,
        )?;
        let private_documents = private::normalize_inline(incoming, &input, session)?;
        let private_documents = private::resolve_graph(
            private_documents,
            tx,
            incoming.key(),
            &input,
            before,
            session,
        )
        .await?;
        private::associate_graph(&private_documents, &input, session)?;
        let binding_value = session
            .parse_representation(binding.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        private::validate_slots(
            &private_documents,
            &input,
            &publication,
            binding_value.value(),
            &incoming.key().scope,
            session,
        )?;
        let refs = input
            .authority_refs()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        size(refs.len(), 1, 32, session)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Multiply,
                &[
                    &refs.len().into(),
                    &mem::size_of::<current::source::ObservedSource>().into(),
                ],
            )
            .map_err(core_error)?;
        let payload = refs
            .len()
            .checked_mul(mem::size_of::<current::source::ObservedSource>())
            .ok_or(ServiceError::Unavailable)?;
        let sources_storage = session
            .reserve_external_payload(payload)
            .map_err(core_error)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        // These are the actual original grant nominations from normalized full
        // terms. Today's head/body acquisition is shared with work.accept;
        // original custody and exact operation-right intersection remain gates.
        for reference in refs {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let observed =
                current::source::observe_value(tx, incoming.key(), reference, before, session)
                    .await?;
            // Keep actual revocation DATA too. The exact consuming authority
            // intersection decides which active current grant is mandatory.
            session
                .semantic_work()
                .write_bytes(mem::size_of::<current::source::ObservedSource>())
                .map_err(core_error)?;
            sources.push(observed);
        }
        nominations::validate(
            &sources,
            &private_documents,
            &input,
            &publication,
            binding_value.value(),
            at.as_str(),
            session,
        )?;
        let admission_source =
            admission::select(&sources, &setup.operator, incoming.key(), session)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&admission_source.into(), &sources.len().into()],
            )
            .map_err(core_error)?;
        let source = sources
            .get(admission_source)
            .ok_or(ServiceError::IntegrityFailure)?;
        let consents = consent::observe_all(
            tx,
            incoming,
            &input,
            &publication,
            &private_documents,
            source,
            &local_root,
            &setup.operator,
            &at,
            before,
            session,
        )
        .await?;
        let authority = admission::authority(
            &input,
            source,
            &consents,
            &local_root,
            incoming.key(),
            &setup.operator,
            &at,
            session,
        )?;
        Ok(PendingAppAcceptance {
            input,
            binding,
            binding_value,
            local_root,
            publication,
            at,
            before,
            sources,
            admission_source,
            consents,
            authority,
            private_documents,
            _sources_storage: sources_storage,
            _time_storage: time_storage,
        })
    }
    .await;
    terminal(result, tx, session)
}
