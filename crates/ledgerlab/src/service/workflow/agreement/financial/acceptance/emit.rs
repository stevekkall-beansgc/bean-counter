//! SAME AGR03 accepted/head record recipes over actual owning AppAccept input.
//! These records are DATA; no head CAS, completed origin or commit is inferred.
use super::*;
pub(in super::super) struct AcceptanceHeadRecords {
    pub(in super::super) accepted: wire::SourceRecord,
    pub(in super::super) head: wire::SourceRecord,
    prior: Option<wire::SourceRecord>,
}
impl AcceptanceHeadRecords {
    pub(super) fn prior_ref(
        &self,
    ) -> impl Iterator<Item = &ledgerlab_core::policy::workflow::transition_constructor::ExactRef>
    {
        self.prior.iter().map(|r| r.reference())
    }
    pub(super) async fn match_locked(
        &self,
        tx: &mut SqliteTx,
        incoming: &ingress::IncomingRequest,
        binding: &Value,
        expected: u64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let customer = request_wrapper::text(binding, "customer", s)?;
        let agreement = request_wrapper::text(binding, "agreement_id", s)?;
        let current = tx
            .workflow_financial_head(
                &incoming.key().logical_store_id,
                &incoming.key().scope,
                crate::store::sqlite::workflow::financial::HeadKey::AgreementAcceptance {
                    customer,
                    agreement,
                },
                s,
            )
            .await
            .map_err(|e| match e {
                crate::store::sqlite::workflow::financial::FinancialReadError::Store(e) => {
                    crate::service::store_error(e)
                }
                crate::store::sqlite::workflow::financial::FinancialReadError::Resource(e) => {
                    core_error(e)
                }
            })?;
        match (&self.prior, current) {
            (None, None) => Ok(()),
            (Some(prior), Some(current)) => {
                match_locked_head(prior.reference(), &current, expected, s)
            }
            _ => Err(reject("STALE_REVISION")),
        }
    }
}
fn match_locked_head(
    prior: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
    current: &crate::store::sqlite::workflow::financial::HeadLocator,
    expected: u64,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    // The paid SQL locator casts its checked INTEGER revision to raw decimal
    // text. Compare its counter value, not raw text to a quoted JSON string.
    s.semantic_work()
        .scan_bytes(current.revision.bytes().len())
        .map_err(core_error)?;
    let raw = std::str::from_utf8(current.revision.bytes())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let revision = counter(raw, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&revision.into(), &expected.into()])
        .map_err(core_error)?;
    require(revision == expected, "ORIGINAL_TERMS_MISMATCH")?;
    for (actual, wanted) in [
        (current.kind.bytes(), prior.kind.as_bytes()),
        (current.id.bytes(), prior.id.as_bytes()),
        (current.digest.bytes(), prior.digest.as_bytes()),
    ] {
        same_bytes(actual, wanted, s)?;
    }
    Ok(())
}

#[cfg(test)]
mod locked_head_tests {
    use super::*;

    #[test]
    fn raw_sql_revision_matches_only_expected_counter_and_complete_prior_ref() {
        use crate::store::sqlite::workflow::financial::HeadLocator;
        use ledgerlab_core::{
            canonical::audit_arena::AuditArena, policy::workflow::transition_constructor::ExactRef,
        };

        let prior = ExactRef {
            kind: "workflow-head-transition".into(),
            id: "whead_original".into(),
            digest: "sha256:original".into(),
        };
        let check = |revision: &[u8], kind: &[u8], id: &[u8], digest: &[u8], expected| {
            let arena = AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let current = HeadLocator {
                revision: s.copy_representation(revision, Kind::Record).unwrap(),
                kind: s.copy_representation(kind, Kind::Record).unwrap(),
                id: s.copy_representation(id, Kind::Record).unwrap(),
                digest: s.copy_representation(digest, Kind::Record).unwrap(),
            };
            match_locked_head(&prior, &current, expected, &mut s)
        };
        let k = prior.kind.as_bytes();
        let i = prior.id.as_bytes();
        let d = prior.digest.as_bytes();
        // SQL CAST(INTEGER AS TEXT) returns b"1", not JSON's b"\"1\"".
        assert_eq!(check(b"1", k, i, d, 1), Ok(()));
        assert_eq!(check(b"10", k, i, d, 10), Ok(()));
        for result in [
            check(b"2", k, i, d, 1),
            check(b"1", k, i, d, 2),
            check(b"1", b"workflow-other", i, d, 1),
            check(b"1", k, b"whead_other", d, 1),
            check(b"1", k, i, b"sha256:other", 1),
        ] {
            assert_eq!(result, Err(reject("ORIGINAL_TERMS_MISMATCH")));
        }
        assert_eq!(check(b"\"1\"", k, i, d, 1), Err(reject("INVALID_SCALAR")));
        assert_eq!(
            check(&[0xff], k, i, d, 1),
            Err(ServiceError::IntegrityFailure)
        );
    }
}

fn agreement_ref(r: &AgreementRef, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let digest = wire::string(r.digest(), s)?;
    let id = wire::string(r.id(), s)?;
    let kind = wire::string(r.kind(), s)?;
    wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    input: &AppAcceptanceInput,
    binding: &wire::SourceRecord,
    authority: &wire::SourceRecord,
    incoming: &ingress::IncomingRequest,
    actor: &str,
    at: &Timestamp,
    sequence: i64,
    prior: Option<&AuditedBytes>,
    s: &mut OriginalAuditSession,
) -> Result<AcceptanceHeadRecords> {
    let key = request_wrapper::field(incoming.value(), "key", s)?;
    let scope = request_wrapper::field(key, "scope", s)?;
    let context = s
        .parse_representation(binding.bytes(), Kind::SourceDefinition)
        .map_err(core_error)?;
    let customer = request_wrapper::text(context.value(), "customer", s)?;
    let agreement = request_wrapper::text(context.value(), "agreement_id", s)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&input.expected_revision.into(), &1u8.into()])
        .map_err(core_error)?;
    let next = input
        .expected_revision
        .checked_add(1)
        .ok_or_else(|| reject("INVALID_SCALAR"))?;
    let old = wire::counter_string(input.expected_revision, s)?;
    let new = wire::counter_string(next, s)?;
    let revision = request_wrapper::text(context.value(), "version", s)?;
    let actual_revision = counter(revision, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&actual_revision.into(), &next.into()])
        .map_err(core_error)?;
    require(actual_revision == next, "INVALID_SHAPE")?;
    let prior = if let Some(raw) = prior {
        let p = s
            .parse_representation(raw.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(p.value(), "schema", s)?,
            "ledger-workflow-head-transition/1",
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(p.value(), "head_kind", s)?,
            "agreement_acceptance",
            s,
        )?;
        for (name, actual) in [("customer", customer), ("agreement_id", agreement)] {
            request_wrapper::same_text(request_wrapper::text(p.value(), name, s)?, actual, s)?;
        }
        let prior_revision = request_wrapper::text(p.value(), "new_revision", s)?;
        let n = counter(prior_revision, s)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&n.into(), &input.expected_revision.into()])
            .map_err(core_error)?;
        require(n == input.expected_revision, "STALE_REVISION")?;
        let body = s
            .copy_representation(raw.bytes(), Kind::SourceDefinition)
            .map_err(core_error)?;
        Some(wire::agreement_record(
            wire::AgreementRecord::AcceptanceHead,
            body,
            key,
            customer,
            agreement,
            prior_revision,
            s,
        )?)
    } else {
        s.semantic_work()
            .arithmetic(Op::Compare, &[&input.expected_revision.into(), &0u8.into()])
            .map_err(core_error)?;
        require(input.expected_revision == 0, "STALE_REVISION")?;
        None
    };
    let key_bytes = encode(key, s)?;
    let scope_bytes = encode(scope, s)?;
    let customer_bytes = wire::string(customer, s)?;
    let agreement_bytes = wire::string(agreement, s)?;
    let actor = wire::string(actor, s)?;
    let at = wire::string(at.as_str(), s)?;
    let sequence = wire::counter_string(
        sequence
            .try_into()
            .map_err(|_| ServiceError::IntegrityFailure)?,
        s,
    )?;
    let binding_ref = wire::reference(binding.reference(), s)?;
    let authority_ref = wire::reference(authority.reference(), s)?;
    let terms_ref = agreement_ref(input.terms.reference(), s)?;
    let terms_digest = wire::string(input.terms.terms_digest(), s)?;
    let publication = encode(
        request_wrapper::field(input.terms.canonical_value(), "publication_ref", s)?,
        s,
    )?;
    let accepted = wire::object(
        &[
            ("accepted_at", at.bytes()),
            ("agreement_id", agreement_bytes.bytes()),
            ("authority_observation_ref", authority_ref.bytes()),
            ("binding_ref", binding_ref.bytes()),
            ("commit_sequence", sequence.bytes()),
            ("customer", customer_bytes.bytes()),
            ("operation_key", key_bytes.bytes()),
            ("publication_ref", publication.bytes()),
            ("schema", b"\"ledger-workflow-accepted-agreement/1\""),
            ("scope", scope_bytes.bytes()),
            ("terms_digest", terms_digest.bytes()),
            ("terms_ref", terms_ref.bytes()),
            ("version", new.bytes()),
        ],
        262144,
        s,
    )?;
    let accepted = wire::agreement_record(
        wire::AgreementRecord::Accepted,
        accepted,
        key,
        customer,
        agreement,
        revision,
        s,
    )?;
    let prior_ref = prior
        .as_ref()
        .map(|p| wire::reference(p.reference(), s))
        .transpose()?;
    let mut fields = [("", b"".as_slice()); 14];
    s.semantic_work()
        .write_bytes(mem::size_of_val(&fields))
        .map_err(core_error)?;
    let base = [
        ("accepted_at", at.bytes()),
        ("accepted_binding_ref", binding_ref.bytes()),
        ("actor", actor.bytes()),
        ("agreement_id", agreement_bytes.bytes()),
        ("commit_sequence", sequence.bytes()),
        ("customer", customer_bytes.bytes()),
        ("head_kind", b"\"agreement_acceptance\"".as_slice()),
        ("new_revision", new.bytes()),
        ("old_revision", old.bytes()),
        ("operation_key", key_bytes.bytes()),
    ];
    let mut n = 0usize;
    for part in base {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[n] = part;
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &1usize.into()])
            .map_err(core_error)?;
        n = n.checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
    }
    if let Some(prior) = &prior_ref {
        fields[n] = ("prior_head_ref", prior.bytes());
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &1usize.into()])
            .map_err(core_error)?;
        n = n.checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
    }
    for part in [
        (
            "schema",
            b"\"ledger-workflow-head-transition/1\"".as_slice(),
        ),
        ("scope", scope_bytes.bytes()),
    ] {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[n] = part;
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &1usize.into()])
            .map_err(core_error)?;
        n = n.checked_add(1).ok_or(ServiceError::IntegrityFailure)?;
    }
    let head = wire::object(&fields[..n], 262144, s)?;
    let head = wire::agreement_record(
        wire::AgreementRecord::AcceptanceHead,
        head,
        key,
        customer,
        agreement,
        revision,
        s,
    )?;
    Ok(AcceptanceHeadRecords {
        accepted,
        head,
        prior,
    })
}
