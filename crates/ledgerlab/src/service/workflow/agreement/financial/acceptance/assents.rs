//! Candidate assent material from literal private input or exact retained Refs.
//! Selection uses subject/fullterms identity only; the SAME actual material
//! verifiers still check custody/mapping/times and normalize the result Ref.
use super::*;
pub(super) struct Inputs<'a> {
    inline: &'a [Value],
    retained: Vec<current::source::SourceRecord>,
    _storage: ExternalBufferLease,
}
pub(super) fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
impl<'a> Inputs<'a> {
    pub(super) async fn load(
        tx: &mut SqliteTx,
        key: &OperationKey,
        incoming: &'a ingress::IncomingRequest,
        input: &AppAcceptanceInput,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let inline = request_wrapper::field(incoming.payload(s)?, "documents", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        size(inline.len(), 0, 128, s)?;
        let refs = input
            .assent_refs()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        size(refs.len(), 1, 16, s)?;
        s.semantic_work()
            .arithmetic(
                Op::Multiply,
                &[
                    &refs.len().into(),
                    &mem::size_of::<current::source::SourceRecord>().into(),
                ],
            )
            .map_err(core_error)?;
        let n = refs
            .len()
            .checked_mul(mem::size_of::<current::source::SourceRecord>())
            .ok_or(ServiceError::Unavailable)?;
        let storage = s.reserve_external_payload(n).map_err(core_error)?;
        let mut out = Self {
            inline,
            retained: Vec::new(),
            _storage: storage,
        };
        out.retained
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for reference in refs {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if let Some(record) =
                current::source::assent_record(tx, key, reference, before, s).await?
            {
                s.semantic_work()
                    .write_bytes(mem::size_of::<current::source::SourceRecord>())
                    .map_err(core_error)?;
                out.retained.push(record);
            }
        }
        Ok(out)
    }
    pub(super) fn matches(
        value: &Value,
        input: &AppAcceptanceInput,
        party: Option<&str>,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        let schema = request_wrapper::text(value, "schema", s)?;
        let expected = if party.is_some() {
            input.party_assent_schema()
        } else {
            "ledger-workflow-assent/1"
        };
        if !equal(schema, expected, s)? {
            return Ok(false);
        }
        let subject = if party.is_some() {
            request_wrapper::field(value, "consent", s)?
        } else {
            value
        };
        for name in ["customer", "agreement_id"] {
            if !equal(
                request_wrapper::text(subject, name, s)?,
                request_wrapper::text(input.terms.canonical_value(), name, s)?,
                s,
            )? {
                return Ok(false);
            }
        }
        let digest = if let Some(party) = party {
            if !equal(request_wrapper::text(subject, "party", s)?, party, s)? {
                return Ok(false);
            }
            let basis = request_wrapper::field(subject, "basis", s)?;
            if !equal(
                request_wrapper::text(basis, "kind", s)?,
                "acceptance_terms",
                s,
            )? {
                return Ok(false);
            }
            request_wrapper::text(basis, "terms_digest", s)?
        } else {
            request_wrapper::text(subject, "terms_digest", s)?
        };
        equal(digest, input.terms.terms_digest(), s)
    }
    pub(super) fn select(
        &self,
        input: &AppAcceptanceInput,
        party: Option<&str>,
        s: &mut OriginalAuditSession,
    ) -> Result<&Value> {
        let mut selected = None;
        // Inline material is tentative: the eventual SAME verifier must produce
        // one of the exact normalized assent_refs. A schema/subject match is
        // never credited as permission or mapping evidence.
        for value in self.inline {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if Self::matches(value, input, party, s)? {
                s.semantic_work()
                    .visit(Visit::Constraint)
                    .map_err(core_error)?;
                require(selected.replace(value).is_none(), "INVALID_SHAPE")?;
            }
        }
        if let Some(value) = selected {
            return Ok(value);
        }
        for record in &self.retained {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if Self::matches(record.body(), input, party, s)? {
                s.semantic_work()
                    .visit(Visit::Constraint)
                    .map_err(core_error)?;
                require(selected.replace(record.body()).is_none(), "INVALID_SHAPE")?;
            }
        }
        selected.ok_or_else(|| reject("ASSENT_TERMS_MISMATCH"))
    }
}
