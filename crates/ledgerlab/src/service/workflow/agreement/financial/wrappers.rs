//! Exact mechanical saved wrappers under their original economic recipe.
//! A valid frame remains unresolved DATA until full prefix, semantic replay,
//! authorization, dependency/capacity/funding and resource obligations succeed.
use super::request_wrapper::{self as request, SavedRequest};
use super::*;
use crate::store::sqlite::workflow::financial::FinancialFrame;
use ledgerlab_core::{
    canonical::audit_arena::AuditedValue,
    validation_work::{
        ArithmeticOperation as Op, BigInt, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use sha2::{Digest, Sha256};

pub(super) struct OriginalWrappers {
    // Parsed trees drop before the retained raw storage. Each owns its own
    // reservation in the SAME attempt arena; no borrowed Value escapes it.
    operation: AuditedValue,
    manifest: AuditedValue,
    receipt: AuditedValue,
    request: SavedRequest,
    frame: FinancialFrame,
}
impl OriginalWrappers {
    pub(super) fn operation(&self) -> &Value {
        self.operation.value()
    }
    pub(super) fn manifest(&self) -> &Value {
        self.manifest.value()
    }
    pub(super) fn receipt(&self) -> &Value {
        self.receipt.value()
    }
    pub(super) fn original_request(&self) -> &SavedRequest {
        &self.request
    }
    pub(super) fn sequence(&self) -> i64 {
        self.frame.sequence
    }
}
fn resource(e: crate::store::sqlite::workflow::financial::FinancialReadError) -> ServiceError {
    match e {
        crate::store::sqlite::workflow::financial::FinancialReadError::Store(e) => {
            crate::service::store_error(e)
        }
        crate::store::sqlite::workflow::financial::FinancialReadError::Resource(e) => core_error(e),
    }
}
#[derive(Clone, Copy)]
struct Recipe {
    operation: &'static str,
    manifest: &'static str,
    closure: &'static str,
    authority: &'static str,
}
fn recipe(
    kind: &str,
    payload: &str,
    registry: &str,
    session: &mut OriginalAuditSession,
) -> Result<Recipe> {
    const ECON: &str = "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e";
    const CAUSAL: &str = "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54";
    const EXPOSURE: &str =
        "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4";
    // Dispatch EACH kind/payload/registry tuple, never 'known registry=>latest'.
    const ORDINARY: &[(&str, &str)] = &[
        ("work.accept", "ledger-workflow-work/1"),
        ("outcome.submit", "ledger-workflow-outcome/1"),
        ("outcome.correct", "ledger-workflow-outcome-correction/1"),
        ("outcome.close", "ledger-workflow-outcome-close/1"),
        ("link.assert", "ledger-workflow-link-assert/1"),
    ];
    for (k, p) in ORDINARY {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if kind == *k
            && payload == *p
            && (registry == ECON || registry == CAUSAL || registry == EXPOSURE)
        {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/3",
                manifest: "ledger-workflow-economic-manifest/3",
                closure: "ledger-economic-resolved-inputs/3",
                authority: "ledger-workflow-economic-authority/2",
            });
        }
    }
    // The old /1 correction tuples exist ONLY in economic-audit03. Neither
    // additive registry inherits them merely by keeping ordinary schemas3.
    for (k, p) in [
        ("quantity.correct", "ledger-workflow-quantity-correction/1"),
        ("work.reverse", "ledger-workflow-work-reverse/1"),
    ] {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if kind == k && payload == p && registry == ECON {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/3",
                manifest: "ledger-workflow-economic-manifest/3",
                closure: "ledger-economic-resolved-inputs/3",
                authority: "ledger-workflow-economic-authority/2",
            });
        }
    }
    for (k, p) in [
        ("quantity.correct", "ledger-workflow-quantity-correction/2"),
        ("work.reverse", "ledger-workflow-work-reverse/2"),
    ] {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if kind == k && payload == p && (registry == CAUSAL || registry == EXPOSURE) {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/4",
                manifest: "ledger-workflow-economic-manifest/4",
                closure: "ledger-economic-resolved-inputs/4",
                authority: "ledger-workflow-economic-authority/3",
            });
        }
    }
    Err(reject("UNSUPPORTED_SCHEMA_PROFILE"))
}
fn same_value(left: &Value, right: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    let a = session
        .canonical_representation(left, RepresentationKind::Record)
        .map_err(core_error)?;
    let b = session
        .canonical_representation(right, RepresentationKind::Record)
        .map_err(core_error)?;
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(a.bytes().len()).map_err(core_error)?;
    work.scan_bytes(b.bytes().len()).map_err(core_error)?;
    require(a.bytes() == b.bytes(), "INTEGRITY_FAILURE")
}
fn checked(
    raw: &[u8],
    kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let parsed = session
        .parse_representation(raw, kind)
        .map_err(core_error)?;
    let encoded = session
        .canonical_representation(parsed.value(), kind)
        .map_err(core_error)?;
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(raw.len()).map_err(core_error)?;
    work.scan_bytes(encoded.bytes().len()).map_err(core_error)?;
    require(raw == encoded.bytes(), "INTEGRITY_FAILURE")?;
    Ok(parsed)
}

fn counter(value: &str, session: &mut OriginalAuditSession) -> Result<i64> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        !value.is_empty() && !value.starts_with('0'),
        "INTEGRITY_FAILURE",
    )?;
    let mut n = 0i64;
    for byte in value.bytes() {
        session
            .semantic_work()
            .visit(Visit::ScalarDigit)
            .map_err(core_error)?;
        require(byte.is_ascii_digit(), "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&n.into(), &10i64.into()])
            .map_err(core_error)?;
        n = n.checked_mul(10).ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &i64::from(byte - b'0').into()])
            .map_err(core_error)?;
        n = n
            .checked_add(i64::from(byte - b'0'))
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    Ok(n)
}
fn update(h: &mut Sha256, bytes: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .hash_bytes(bytes.len())
        .map_err(core_error)?;
    h.update(bytes);
    Ok(())
}
fn update_value(
    h: &mut Sha256,
    value: &Value,
    kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let encoded = session
        .canonical_representation(value, kind)
        .map_err(core_error)?;
    update(h, encoded.bytes(), session)
}
fn value_hash(
    domain: &[u8],
    value: &Value,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    update(&mut h, domain, session)?;
    update_value(&mut h, value, RepresentationKind::Record, session)?;
    Ok(h.finalize().into())
}
fn operation_hash(
    scope: &Value,
    op: &Value,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    update(
        &mut h,
        b"bean-counter/workflow-record/1\0[\"workflow-operation\",",
        session,
    )?;
    update_value(&mut h, scope, RepresentationKind::Record, session)?;
    update(&mut h, b",", session)?;
    update_value(&mut h, op, RepresentationKind::OperationFrame, session)?;
    update(&mut h, b"]", session)?;
    Ok(h.finalize().into())
}
fn without_hash(
    domain: &[u8],
    value: &Value,
    excluded: &str,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let object = value.as_object().ok_or(ServiceError::IntegrityFailure)?;
    let mut h = Sha256::new();
    update(&mut h, domain, session)?;
    update(&mut h, b"{", session)?;
    let mut first = true;
    // All keys are exact closed ASCII dictionaries, so their BTree order equals
    // JCS UTF-16 order. Values use the same canonical emitter, without clones.
    for (name, v) in object {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if name == excluded {
            continue;
        }
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(
            name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "INTEGRITY_FAILURE",
        )?;
        if !first {
            update(&mut h, b",", session)?;
        }
        first = false;
        update(&mut h, b"\"", session)?;
        update(&mut h, name.as_bytes(), session)?;
        update(&mut h, b"\":", session)?;
        update_value(&mut h, v, RepresentationKind::ReceiptFrame, session)?;
    }
    update(&mut h, b"}", session)?;
    Ok(h.finalize().into())
}
fn id(
    digest: &[u8; 32],
    actual: &[u8],
    prefix: &str,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(actual.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let actual = std::str::from_utf8(actual).map_err(|_| ServiceError::IntegrityFailure)?;
    let suffix = actual
        .strip_prefix(prefix)
        .ok_or(ServiceError::IntegrityFailure)?;
    request::digest_text(digest, suffix, false, session)
}
fn digest(digest: &[u8; 32], actual: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(actual.len())
        .map_err(core_error)?;
    let actual = std::str::from_utf8(actual).map_err(|_| ServiceError::IntegrityFailure)?;
    request::digest_text(digest, actual, true, session)
}

fn reference<'a>(
    value: &'a Value,
    session: &mut OriginalAuditSession,
) -> Result<(&'a str, &'a str, &'a str)> {
    request::closed(value, &["kind", "id", "digest"], session)?;
    let kind = request::text(value, "kind", session)?;
    let id = request::text(value, "id", session)?;
    let digest = request::text(value, "digest", session)?;
    for text in [kind, id, digest] {
        session
            .semantic_work()
            .scan_bytes(text.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(text.is_ascii(), "INTEGRITY_FAILURE")?;
    }
    Ok((digest, id, kind))
}
async fn indexed_sets(
    tx: &mut SqliteTx,
    identity: &[u8],
    frame: &FinancialFrame,
    op: &Value,
    manifest: &Value,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let members = request::field(manifest, "members", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&BigInt::from(members.len()), &frame.member_count.into()],
        )
        .map_err(core_error)?;
    require(
        members.len() as i64 == frame.member_count,
        "INTEGRITY_FAILURE",
    )?;
    let mut previous = None;
    let mut operation_found = false;
    for member in members {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let tuple = reference(member, session)?;
        if let Some(prior) = previous {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            require(prior < tuple, "INTEGRITY_FAILURE")?;
        }
        previous = Some(tuple);
        require(
            tx.workflow_financial_member_present(identity, tuple.2, tuple.1, tuple.0, session)
                .await
                .map_err(resource)?,
            "INTEGRITY_FAILURE",
        )?;
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if tuple.2 == "workflow-operation"
            && tuple.1.as_bytes() == frame.operation_id.bytes()
            && tuple.0.as_bytes() == frame.operation_digest.bytes()
        {
            operation_found = true;
        }
    }
    require(operation_found, "INTEGRITY_FAILURE")?;
    let results = request::field(op, "result_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        !results.is_empty() && results.len() <= 4096,
        "INTEGRITY_FAILURE",
    )?;
    let mut previous = None;
    for result in results {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let tuple = reference(result, session)?;
        if let Some(prior) = previous {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            require(prior < tuple, "INTEGRITY_FAILURE")?;
        }
        previous = Some(tuple);
        require(
            tx.workflow_financial_member_present(identity, tuple.2, tuple.1, tuple.0, session)
                .await
                .map_err(resource)?,
            "INTEGRITY_FAILURE",
        )?;
    }
    Ok(())
}
pub(super) async fn load(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    session: &mut OriginalAuditSession,
) -> Result<OriginalWrappers> {
    let frame = tx
        .workflow_financial_frame(identity, &key.logical_store_id, &key.scope, session)
        .await
        .map_err(resource)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let operation = checked(
        frame.operation.bytes(),
        RepresentationKind::OperationFrame,
        session,
    )?;
    let manifest = checked(
        frame.manifest.bytes(),
        RepresentationKind::ManifestFrame,
        session,
    )?;
    let receipt = checked(
        frame.receipt.bytes(),
        RepresentationKind::ReceiptFrame,
        session,
    )?;
    let op = operation.value();
    let man = manifest.value();
    let rec = receipt.value();
    request::closed(
        op,
        &[
            "schema",
            "key",
            "kind",
            "contract",
            "request_evidence",
            "authority_snapshot",
            "received_at",
            "accepted_at",
            "commit_sequence",
            "result_refs",
            "closure_profile",
            "source_observation_refs",
            "financial_observation_ref",
            "combined_audit_digest",
        ],
        session,
    )?;
    request::closed(
        man,
        &[
            "schema",
            "key",
            "members",
            "closure_profile",
            "source_observation_refs",
            "financial_observation_ref",
            "combined_audit_digest",
        ],
        session,
    )?;
    request::closed(
        rec,
        &[
            "schema",
            "key",
            "request_hash",
            "request_hash_profile",
            "normalization_profile",
            "schema_bundle_digest",
            "descriptor_closure_digest",
            "original_request_bytes_sha256",
            "result_kind",
            "result_refs",
            "accepted_at",
            "commit_sequence",
            "manifest_hash",
            "receipt_hash",
        ],
        session,
    )?;
    for body in [op, man, rec] {
        request::key(request::field(body, "key", session)?, key, session)?;
    }
    request::same_text(request::text(op, "kind", session)?, frame.kind, session)?;
    request::same_text(
        request::text(op, "contract", session)?,
        "workflow/1",
        session,
    )?;
    let evidence = request::field(op, "request_evidence", session)?;
    let request = request::parse_saved(evidence, frame.kind, key, session)?;
    let payload = request::field(request.value(), "payload", session)?;
    let descriptors = request::field(evidence, "descriptor_resolution", session)?;
    let selected = recipe(
        frame.kind,
        request::text(payload, "schema", session)?,
        request::text(descriptors, "registry_digest", session)?,
        session,
    )?;
    request::same_text(
        request::text(op, "schema", session)?,
        selected.operation,
        session,
    )?;
    request::same_text(
        request::text(man, "schema", session)?,
        selected.manifest,
        session,
    )?;
    for body in [op, man] {
        request::same_text(
            request::text(body, "closure_profile", session)?,
            selected.closure,
            session,
        )?;
    }
    request::same_text(
        request::text(
            request::field(op, "authority_snapshot", session)?,
            "schema",
            session,
        )?,
        selected.authority,
        session,
    )?;
    for name in [
        "source_observation_refs",
        "financial_observation_ref",
        "combined_audit_digest",
    ] {
        same_value(
            request::field(op, name, session)?,
            request::field(man, name, session)?,
            session,
        )?;
    }
    request::same_text(
        request::text(rec, "schema", session)?,
        "ledger-operation-receipt/2",
        session,
    )?;
    // Dedicated semantic-alias verification is required; never certify one as
    // an original by accepting a generic economic wrapper.
    request::same_text(
        request::text(rec, "result_kind", session)?,
        "economic",
        session,
    )?;
    request::same_text(
        request::text(rec, "request_hash_profile", session)?,
        "bean-counter/operation-request/2",
        session,
    )?;
    request::same_text(
        request::text(rec, "normalization_profile", session)?,
        "ledger-operation-normalization/2",
        session,
    )?;
    for name in [
        "request_hash",
        "schema_bundle_digest",
        "original_request_bytes_sha256",
    ] {
        same_value(
            request::field(rec, name, session)?,
            request::field(evidence, name, session)?,
            session,
        )?;
    }
    same_value(
        request::field(rec, "descriptor_closure_digest", session)?,
        request::field(descriptors, "closure_digest", session)?,
        session,
    )?;
    same_value(
        request::field(rec, "result_refs", session)?,
        request::field(op, "result_refs", session)?,
        session,
    )?;
    same_value(
        request::field(rec, "accepted_at", session)?,
        request::field(op, "accepted_at", session)?,
        session,
    )?;
    let op_sequence = counter(request::text(op, "commit_sequence", session)?, session)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&op_sequence.into(), &frame.sequence.into()])
        .map_err(core_error)?;
    require(op_sequence == frame.sequence, "INTEGRITY_FAILURE")?;
    same_value(
        request::field(rec, "commit_sequence", session)?,
        request::field(op, "commit_sequence", session)?,
        session,
    )?;
    let key_hash = value_hash(
        b"bean-counter/workflow-operation/1\0",
        request::field(op, "key", session)?,
        session,
    )?;
    id(&key_hash, frame.operation_id.bytes(), "wop_", session)?;
    let op_hash = operation_hash(
        request::field(request::field(op, "key", session)?, "scope", session)?,
        op,
        session,
    )?;
    digest(&op_hash, frame.operation_digest.bytes(), session)?;
    let manifest_hash = value_hash(b"bean-counter/workflow-manifest/1\0", man, session)?;
    id(
        &manifest_hash,
        frame.manifest_id.bytes(),
        "wmanifest_",
        session,
    )?;
    digest(&manifest_hash, frame.manifest_digest.bytes(), session)?;
    request::digest_text(
        &manifest_hash,
        request::text(rec, "manifest_hash", session)?,
        true,
        session,
    )?;
    let receipt_hash = without_hash(
        b"bean-counter/operation-receipt/2\0",
        rec,
        "receipt_hash",
        session,
    )?;
    id(
        &receipt_hash,
        frame.receipt_id.bytes(),
        "wreceipt_",
        session,
    )?;
    digest(&receipt_hash, frame.receipt_digest.bytes(), session)?;
    request::digest_text(
        &receipt_hash,
        request::text(rec, "receipt_hash", session)?,
        true,
        session,
    )?;
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(frame.request_hash.bytes().len())
        .map_err(core_error)?;
    require(
        frame.request_hash.bytes() == request::text(evidence, "request_hash", session)?.as_bytes(),
        "INTEGRITY_FAILURE",
    )?;
    indexed_sets(tx, identity, &frame, op, man, session).await?;
    // Accepted-time epoch, freshly extracted descriptors, normalized intent,
    // exact complete result/member sets and all economic/auth/capacity claims
    // remain mandatory in the full original reconstruction; this is NOT proof.
    Ok(OriginalWrappers {
        operation,
        manifest,
        receipt,
        request,
        frame,
    })
}

#[cfg(test)]
mod recipe_tests {
    use super::*;
    #[test]
    fn exposure_dispatch_retains_exact_original_wrapper_versions() {
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let registry = "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4";
        for registry in [
            registry,
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
        ] {
            for (kind, old, new) in [
                (
                    "quantity.correct",
                    "ledger-workflow-quantity-correction/1",
                    "ledger-workflow-quantity-correction/2",
                ),
                (
                    "work.reverse",
                    "ledger-workflow-work-reverse/1",
                    "ledger-workflow-work-reverse/2",
                ),
            ] {
                assert!(recipe(kind, old, registry, &mut session).is_err());
                let correct = recipe(kind, new, registry, &mut session).unwrap();
                assert_eq!(correct.operation, "ledger-workflow-economic-operation/4");
                let legacy = recipe(
                    kind,
                    old,
                    "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
                    &mut session,
                )
                .unwrap();
                assert_eq!(legacy.operation, "ledger-workflow-economic-operation/3");
            }
            for (kind, payload) in [
                ("work.accept", "ledger-workflow-work/1"),
                ("outcome.submit", "ledger-workflow-outcome/1"),
                ("outcome.correct", "ledger-workflow-outcome-correction/1"),
                ("outcome.close", "ledger-workflow-outcome-close/1"),
                ("link.assert", "ledger-workflow-link-assert/1"),
            ] {
                assert_eq!(
                    recipe(kind, payload, registry, &mut session)
                        .unwrap()
                        .operation,
                    "ledger-workflow-economic-operation/3"
                );
            }
        }
        let ordinary = recipe(
            "work.accept",
            "ledger-workflow-work/1",
            registry,
            &mut session,
        )
        .unwrap();
        assert_eq!(ordinary.operation, "ledger-workflow-economic-operation/3");
        assert_eq!(ordinary.authority, "ledger-workflow-economic-authority/2");
        let causal = recipe(
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            registry,
            &mut session,
        )
        .unwrap();
        assert_eq!(causal.operation, "ledger-workflow-economic-operation/4");
        assert_eq!(causal.authority, "ledger-workflow-economic-authority/3");
        assert!(
            recipe(
                "work.accept",
                "ledger-workflow-quantity-correction/2",
                registry,
                &mut session
            )
            .is_err()
        );
        assert!(
            recipe(
                "agreement.accept",
                "ledger-workflow-agreement-accept/4",
                registry,
                &mut session
            )
            .is_err()
        );
        assert!(
            recipe(
                "quantity.correct",
                "ledger-workflow-quantity-correction/2",
                "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
                &mut session
            )
            .is_err()
        );
    }
}
