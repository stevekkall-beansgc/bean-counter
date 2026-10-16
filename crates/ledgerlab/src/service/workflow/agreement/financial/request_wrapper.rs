//! Literal saved-request validation. Its output is unresolved retained DATA;
//! it neither selects original descriptors nor authenticates a financial origin.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{
        ArithmeticOperation as Op, BigInt, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};

pub(super) struct SavedRequest {
    raw: AuditedBytes,
    parsed: AuditedValue,
}
impl SavedRequest {
    pub(super) fn raw(&self) -> &[u8] {
        self.raw.bytes()
    }
    pub(super) fn value(&self) -> &Value {
        self.parsed.value()
    }
}
pub(super) fn field<'a>(
    value: &'a Value,
    name: &str,
    session: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let object = value.as_object().ok_or(ServiceError::IntegrityFailure)?;
    // Expose each actual candidate comparison instead of attributing an
    // opaque BTreeMap lookup to one paid visit. No Value/key clone is made.
    for (candidate, field) in object {
        session
            .semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(candidate.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(name.len())
            .map_err(core_error)?;
        if candidate == name {
            return Ok(field);
        }
    }
    Err(ServiceError::IntegrityFailure)
}
pub(super) fn text<'a>(
    value: &'a Value,
    name: &str,
    session: &mut OriginalAuditSession,
) -> Result<&'a str> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    field(value, name, session)?
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)
}
pub(super) fn closed(
    value: &Value,
    fields: &[&str],
    session: &mut OriginalAuditSession,
) -> Result<()> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let object = value.as_object().ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&BigInt::from(object.len()), &BigInt::from(fields.len())],
        )
        .map_err(core_error)?;
    require(object.len() == fields.len(), "INTEGRITY_FAILURE")?;
    for name in fields {
        let _ = field(value, name, session)?;
    }
    Ok(())
}
pub(super) fn same_text(left: &str, right: &str, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(left.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(right.len())
        .map_err(core_error)?;
    require(left == right, "INTEGRITY_FAILURE")
}
pub(super) fn key(
    value: &Value,
    expected: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    closed(
        value,
        &["logical_store_id", "scope", "source", "operation_id"],
        session,
    )?;
    same_text(
        text(value, "logical_store_id", session)?,
        &expected.logical_store_id,
        session,
    )?;
    same_text(text(value, "source", session)?, &expected.source, session)?;
    same_text(
        text(value, "operation_id", session)?,
        &expected.operation_id,
        session,
    )?;
    let scope = field(value, "scope", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    for (actual, expected) in scope.iter().zip(&expected.scope) {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        same_text(
            actual.as_str().ok_or(ServiceError::IntegrityFailure)?,
            expected,
            session,
        )?;
    }
    Ok(())
}
/// Compare a digest without allocating a second hexadecimal string.
pub(super) fn digest_text(
    digest: &[u8; 32],
    text: &str,
    prefixed: bool,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let bytes = if prefixed {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        text.strip_prefix("sha256:")
            .ok_or(ServiceError::IntegrityFailure)?
            .as_bytes()
    } else {
        text.as_bytes()
    };
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(bytes.len() == 64, "INTEGRITY_FAILURE")?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (expected, pair) in digest.iter().zip(bytes.chunks_exact(2)) {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session.semantic_work().scan_bytes(2).map_err(core_error)?;
        require(
            pair[0] == HEX[(expected >> 4) as usize] && pair[1] == HEX[(expected & 15) as usize],
            "INTEGRITY_FAILURE",
        )?;
    }
    Ok(())
}
pub(super) fn parse_saved(
    evidence: &Value,
    kind: &str,
    expected: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<SavedRequest> {
    closed(
        evidence,
        &[
            "schema",
            "normalization_profile",
            "original_request_bytes_base64",
            "original_request_bytes_sha256",
            "normalized_request",
            "request_hash",
            "schema_bundle_digest",
            "descriptor_resolution",
        ],
        session,
    )?;
    same_text(
        text(evidence, "schema", session)?,
        "ledger-operation-request-evidence/2",
        session,
    )?;
    same_text(
        text(evidence, "normalization_profile", session)?,
        "ledger-operation-normalization/2",
        session,
    )?;
    let encoded = text(evidence, "original_request_bytes_base64", session)?;
    let raw = session
        .decode_base64_representation(encoded, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let hash = session
        .hash_representation(raw.bytes(), RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    digest_text(
        &hash,
        text(evidence, "original_request_bytes_sha256", session)?,
        false,
        session,
    )?;
    let parsed = session
        .parse_representation(raw.bytes(), RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let envelope = parsed.value();
    closed(
        envelope,
        &["schema", "key", "kind", "contract", "payload"],
        session,
    )?;
    same_text(
        text(envelope, "schema", session)?,
        "ledger-operation/1",
        session,
    )?;
    same_text(text(envelope, "contract", session)?, "workflow/1", session)?;
    same_text(text(envelope, "kind", session)?, kind, session)?;
    key(field(envelope, "key", session)?, expected, session)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        field(envelope, "payload", session)?.is_object(),
        "INTEGRITY_FAILURE",
    )?;
    Ok(SavedRequest { raw, parsed })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> OperationKey {
        OperationKey {
            logical_store_id: "store".into(),
            scope: ["tenant".into(), "environment".into()],
            source: "source".into(),
            operation_id: "operation".into(),
        }
    }
    fn evidence(raw: &[u8]) -> Value {
        json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2",
            "original_request_bytes_base64":super::super::super::super::publication::base64(raw),
            "original_request_bytes_sha256":records::raw_hash(raw),"normalized_request":{},"request_hash":"unresolved",
            "schema_bundle_digest":"unresolved","descriptor_resolution":{}})
    }
    fn attempt(e: &Value) -> Result<SavedRequest> {
        let arena =
            ledgerlab_core::canonical::audit_arena::AuditArena::new().map_err(core_error)?;
        let mut session =
            OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
        parse_saved(e, "work.accept", &key(), &mut session)
    }
    #[test]
    fn literal_original_wire_is_owned_and_duplicate_hash_key_or_padding_mismatch_refuses() {
        // Raw-envelope DATA only; neither empty descriptor evidence nor this
        // helper's successful result can become a completed origin.
        let envelope = json!({"schema":"ledger-operation/1","key":key(),"kind":"work.accept","contract":"workflow/1","payload":{"schema":"ledger-workflow-work/1"}});
        let raw = serde_json::to_vec_pretty(&envelope).unwrap();
        let saved = attempt(&evidence(&raw)).unwrap();
        assert_eq!(saved.raw(), raw);
        assert_eq!(saved.value(), &envelope);
        for bad_contract in [None, Some("workflow/other")] {
            let mut wrong = envelope.clone();
            if let Some(contract) = bad_contract {
                wrong["contract"] = json!(contract);
            } else {
                wrong.as_object_mut().unwrap().remove("contract");
            }
            assert!(attempt(&evidence(&serde_json::to_vec(&wrong).unwrap())).is_err());
        }
        let mut wrong = evidence(&raw);
        wrong["original_request_bytes_sha256"] = json!("0".repeat(64));
        assert!(attempt(&wrong).is_err());
        let mut changed = envelope.clone();
        changed["key"]["operation_id"] = json!("other");
        assert!(attempt(&evidence(&records::bytes(&changed).unwrap())).is_err());
        let mut malformed = evidence(&raw);
        malformed["original_request_bytes_base64"] = json!("AB==");
        assert!(attempt(&malformed).is_err());
        let duplicate = String::from_utf8(records::bytes(&envelope).unwrap())
            .unwrap()
            .replace(
                "\"kind\":\"work.accept\"",
                "\"kind\":\"work.accept\",\"k\\u0069nd\":\"work.accept\"",
            );
        assert!(attempt(&evidence(duplicate.as_bytes())).is_err());
    }
}
