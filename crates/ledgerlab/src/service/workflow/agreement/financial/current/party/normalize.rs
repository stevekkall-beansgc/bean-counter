//! Closed party request projection through the existing Timestamp normalizer.
//! Literal request bytes remain separate evidence; no assent is manufactured.
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;

// Only the fixed declared dictionaries below reach this emitter. Arbitrary
// input keys are rejected before their original fields can be projected away.
fn dictionary<const N: usize>(
    value: &Value,
    keys: [&'static str; N],
    replacements: &[(&str, &[u8])],
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    check::closed(value, &keys, session)?;
    let mut encoded: [Option<AuditedBytes>; N] = std::array::from_fn(|_| None);
    let mut selected_fields: [Option<&[u8]>; N] = [None; N];
    let mut fields = [("", b"".as_slice()); N];
    for (i, key) in keys.iter().enumerate() {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Field)
            .map_err(core_error)?;
        let mut selected = None;
        for (name, replacement) in replacements {
            if equal_text(key, name, session)? {
                selected = Some(*replacement);
                break;
            }
        }
        if selected.is_none() {
            let field = check::field(value, key, session)?;
            encoded[i] = Some(
                session
                    .canonical_representation(field, RepresentationKind::SourceDefinition)
                    .map_err(core_error)?,
            );
        }
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        selected_fields[i] = selected;
    }
    for (i, key) in keys.iter().enumerate() {
        session
            .semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Field)
            .map_err(core_error)?;
        let bytes = match selected_fields[i] {
            Some(bytes) => bytes,
            None => encoded[i]
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .bytes(),
        };
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<(&str, &[u8])>())
            .map_err(core_error)?;
        fields[i] = (*key, bytes);
    }
    wire::object(&fields, 262144, session)
}
fn timestamp(
    value: &Value,
    field: &str,
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    // Existing Timestamp uses fixed Gregorian/decimal scratch and one27-byte
    // output; both remain admitted until its canonical string is encoded.
    let _scratch = session
        .reserve_external_payload(4 * 64 + 27)
        .map_err(core_error)?;
    let time = time(value, field, session)?;
    wire::string(time.as_str(), session)
}
fn authorization(
    value: &Value,
    key: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    authorization_spec(value, key, session)?;
    let starts = timestamp(value, "starts_at", session)?;
    let ends = timestamp(value, "ends_at", session)?;
    dictionary(
        value,
        [
            "acceptor",
            "agreement_id",
            "authorization_id",
            "customer",
            "ends_at",
            "party",
            "roles",
            "scope",
            "starts_at",
        ],
        &[("ends_at", ends.bytes()), ("starts_at", starts.bytes())],
        session,
    )
}
pub(in super::super::super) fn payload(
    value: &Value,
    key: &OperationKey,
    session: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let schema = check::text(value, "schema", session)?;
    let encoded = if equal_text(schema, "ledger-workflow-party-authorize/1", session)? {
        let spec = authorization(check::field(value, "authorization", session)?, key, session)?;
        dictionary(
            value,
            [
                "authorization",
                "evidence_ref",
                "expected_revision",
                "operator_attestation_ref",
                "schema",
            ],
            &[("authorization", spec.bytes())],
            session,
        )?
    } else if equal_text(schema, "ledger-workflow-party-evidence-retain/1", session)?
        || equal_text(schema, "ledger-workflow-party-evidence-retain/2", session)?
    {
        let metadata = check::field(value, "metadata", session)?;
        let purpose = check::text(metadata, "purpose", session)?;
        let metadata = if equal_text(purpose, "party_authorization", session)? {
            let spec = authorization(
                check::field(metadata, "authorization", session)?,
                key,
                session,
            )?;
            dictionary(
                metadata,
                ["authorization", "purpose"],
                &[("authorization", spec.bytes())],
                session,
            )?
        } else if equal_text(purpose, "party_consent", session)? {
            let consent = check::field(metadata, "consent", session)?;
            let accepted = timestamp(consent, "accepted_at", session)?;
            let consent = dictionary(
                consent,
                [
                    "accepted_at",
                    "acceptor",
                    "agreement_id",
                    "authorization_ref",
                    "basis",
                    "customer",
                    "party",
                    "roles",
                    "scope",
                ],
                &[("accepted_at", accepted.bytes())],
                session,
            )?;
            dictionary(
                metadata,
                ["consent", "purpose"],
                &[("consent", consent.bytes())],
                session,
            )?
        } else {
            check::same_text(purpose, "party_operator_attestation", session)?;
            dictionary(
                metadata,
                ["purpose", "statement", "target_evidence_ref"],
                &[],
                session,
            )?
        };
        dictionary(
            value,
            [
                "content_base64",
                "content_sha256",
                "media_type",
                "metadata",
                "schema",
                "subject",
            ],
            &[("metadata", metadata.bytes())],
            session,
        )?
    } else {
        check::same_text(schema, "ledger-workflow-party-revoke/1", session)?;
        dictionary(
            value,
            ["authorization_ref", "expected_revision", "reason", "schema"],
            &[],
            session,
        )?
    };
    session
        .parse_representation(encoded.bytes(), RepresentationKind::SourceDefinition)
        .map_err(core_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn party_time_projection_uses_same_timestamp_and_retains_literal_input() {
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let key = OperationKey {
            logical_store_id: "normalizer-store".into(),
            scope: ["test".into(), "local".into()],
            source: "operator".into(),
            operation_id: "normalizer-only".into(),
        };
        let raw = br#"{"schema":"ledger-workflow-party-evidence-retain/1","content_base64":"eA==","content_sha256":"2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881","media_type":"text/plain;charset=utf-8","subject":{"customer":"c","agreement_id":"a","party":"platform","acceptor":"owner"},"metadata":{"purpose":"party_authorization","authorization":{"authorization_id":"auth","scope":["test","local"],"customer":"c","agreement_id":"a","party":"platform","acceptor":"owner","roles":["bearer","payer"],"starts_at":"2026-10-17T08:00:00-04:00","ends_at":"2026-10-18T12:00:00Z"}}}"#;
        let parsed = session
            .parse_representation(raw, RepresentationKind::SourceDefinition)
            .unwrap();
        let normalized = payload(parsed.value(), &key, &mut session).unwrap();
        let spec = &normalized.value()["metadata"]["authorization"];
        assert_eq!(spec["starts_at"], "2026-10-17T12:00:00.000000Z");
        assert_eq!(spec["ends_at"], "2026-10-18T12:00:00.000000Z");
        assert_eq!(
            parsed.value()["metadata"]["authorization"]["starts_at"],
            "2026-10-17T08:00:00-04:00"
        );
        let canonical = payload(normalized.value(), &key, &mut session).unwrap();
        assert_eq!(canonical.value(), normalized.value());
        drop(canonical);
        drop(normalized);
        drop(parsed);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
