//! Owning closed ingress for the actual RP01 host attempt. Legacy ingress is
//! unchanged; no DTO/String/Value copy is made before the shared arena admits it.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};

// Closed parser selection belongs to the real saved recipe, not to an
// untrusted schema's request for admission or an arbitrary validation bypass.
#[derive(Clone, Copy)]
enum IngressRecipe {
    Current,
    OriginalAdministration,
}

pub(super) struct IncomingRequest {
    parsed: AuditedValue,
    raw: AuditedBytes,
    identity: AuditedBytes,
    key: OperationKey,
    kind: &'static str,
    _key_lease: ExternalBufferLease,
}
impl IncomingRequest {
    pub(super) fn parse(raw: &[u8], session: &mut OriginalAuditSession) -> Result<Self> {
        let result = Self::parse_inner(raw, session, IngressRecipe::Current);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    pub(super) fn parse_original_administration(
        raw: &[u8],
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let result = Self::parse_inner(raw, session, IngressRecipe::OriginalAdministration);
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    fn parse_inner(
        raw: &[u8],
        session: &mut OriginalAuditSession,
        recipe: IngressRecipe,
    ) -> Result<Self> {
        let raw = session
            .copy_representation(raw, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        let parsed = session
            .parse_representation(raw.bytes(), RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        let value = parsed.value();
        request_wrapper::closed(
            value,
            &["schema", "key", "kind", "contract", "payload"],
            session,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(value, "schema", session)?,
            "ledger-operation/1",
            session,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(value, "contract", session)?,
            "workflow/1",
            session,
        )?;
        no_null(value, session)?;
        let key_body = request_wrapper::field(value, "key", session)?;
        request_wrapper::closed(
            key_body,
            &["logical_store_id", "scope", "source", "operation_id"],
            session,
        )?;
        let store = request_wrapper::text(key_body, "logical_store_id", session)?;
        let source = request_wrapper::text(key_body, "source", session)?;
        let operation = request_wrapper::text(key_body, "operation_id", session)?;
        let scope = request_wrapper::field(key_body, "scope", session)?
            .as_array()
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        size(scope.len(), 2, 2, session)?;
        let tenant = scope[0].as_str().ok_or_else(|| reject("INVALID_SHAPE"))?;
        let environment = scope[1].as_str().ok_or_else(|| reject("INVALID_SHAPE"))?;
        for (text, maximum) in [
            (store, 128),
            (source, 256),
            (operation, 128),
            (tenant, 128),
            (environment, 128),
        ] {
            identifier(text, maximum, session)?;
        }
        let mut bytes = std::mem::size_of::<OperationKey>();
        for text in [store, source, operation, tenant, environment] {
            session
                .semantic_work()
                .arithmetic(Op::Add, &[&bytes.into(), &text.len().into()])
                .map_err(core_error)?;
            bytes = bytes
                .checked_add(text.len())
                .ok_or(ServiceError::Unavailable)?;
        }
        let key_lease = session
            .reserve_external_payload(bytes)
            .map_err(core_error)?;
        let key = OperationKey {
            logical_store_id: copy(store, session)?,
            source: copy(source, session)?,
            operation_id: copy(operation, session)?,
            scope: [copy(tenant, session)?, copy(environment, session)?],
        };
        let identity = session
            .canonical_representation(key_body, RepresentationKind::Record)
            .map_err(core_error)?;
        let supplied_kind = request_wrapper::text(value, "kind", session)?;
        let mut kind = None;
        for candidate in [
            "work.accept",
            "agreement.accept",
            "work.reverse",
            "quantity.correct",
            "outcome.submit",
            "outcome.correct",
            "outcome.close",
            "link.assert",
            "invocation.authorize",
            "invocation.close",
            "cost.finalize",
            "cost.correct",
            "evidence.retain",
            "authority.change",
        ] {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(supplied_kind.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(candidate.len())
                .map_err(core_error)?;
            if supplied_kind == candidate {
                kind = Some(candidate);
                break;
            }
        }
        let kind = kind.ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?;
        if matches!(recipe, IngressRecipe::OriginalAdministration) {
            request_wrapper::same_text(kind, "authority.change", session)?;
        }
        let payload = request_wrapper::field(value, "payload", session)?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(payload.is_object(), "INVALID_SHAPE")?;
        if kind == "agreement.accept" {
            let schema = request_wrapper::text(payload, "schema", session)?;
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(schema.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes("ledger-workflow-agreement-accept/9".len())
                .map_err(core_error)?;
            if schema != "ledger-workflow-agreement-accept/9" {
                request_wrapper::same_text(schema, "ledger-workflow-agreement-accept/8", session)?;
            }
        }
        if kind == "evidence.retain" || kind == "authority.change" {
            let schema = request_wrapper::text(payload, "schema", session)?;
            let allowed: &[&str] = if kind == "evidence.retain" {
                &[
                    "ledger-workflow-fact-attest/1",
                    "ledger-workflow-family-close-attest/1",
                    "ledger-workflow-supplier-cost-attest/1",
                    "ledger-workflow-app-observe/1",
                    "ledger-workflow-app-cost-attest/1",
                    "ledger-workflow-party-evidence-retain/1",
                    "ledger-workflow-party-evidence-retain/2",
                ]
            } else if matches!(recipe, IngressRecipe::OriginalAdministration) {
                &[
                    "ledger-workflow-customer-register/1",
                    "ledger-workflow-source-enroll/1",
                    "ledger-workflow-source-restrict/1",
                    "ledger-workflow-source-revoke/1",
                ]
            } else {
                &[
                    "ledger-cost-authority-enroll/1",
                    "ledger-workflow-party-authorize/1",
                    "ledger-workflow-party-revoke/1",
                ]
            };
            let mut found = false;
            for candidate in allowed {
                session
                    .semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                session
                    .semantic_work()
                    .scan_bytes(schema.len())
                    .map_err(core_error)?;
                session
                    .semantic_work()
                    .scan_bytes(candidate.len())
                    .map_err(core_error)?;
                if schema == *candidate {
                    found = true;
                    break;
                }
            }
            require(found, "UNSUPPORTED_SCHEMA_PROFILE")?;
        }
        Ok(Self {
            parsed,
            raw,
            identity,
            key,
            kind,
            _key_lease: key_lease,
        })
    }
    pub(super) fn key(&self) -> &OperationKey {
        &self.key
    }
    pub(super) fn kind(&self) -> &'static str {
        self.kind
    }
    pub(super) fn raw(&self) -> &[u8] {
        self.raw.bytes()
    }
    pub(super) fn identity(&self) -> &[u8] {
        self.identity.bytes()
    }
    pub(super) fn value(&self) -> &Value {
        self.parsed.value()
    }
    pub(super) fn payload<'a>(&'a self, session: &mut OriginalAuditSession) -> Result<&'a Value> {
        request_wrapper::field(self.value(), "payload", session)
    }
}
fn size(n: usize, min: usize, max: usize, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &min.into()])
        .map_err(core_error)?;
    require(n >= min, "INVALID_SHAPE")?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &max.into()])
        .map_err(core_error)?;
    require(n <= max, "INVALID_SHAPE")
}
pub(super) fn identifier(
    text: &str,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    size(text.len(), 1, maximum, session)?;
    session
        .semantic_work()
        .scan_bytes(text.len())
        .map_err(core_error)?;
    for character in text.chars() {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(!character.is_control(), "INVALID_SHAPE")?;
    }
    Ok(())
}
fn copy(text: &str, session: &mut OriginalAuditSession) -> Result<String> {
    session
        .semantic_work()
        .scan_bytes(text.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .write_bytes(text.len())
        .map_err(core_error)?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| ServiceError::Unavailable)?;
    copy.push_str(text);
    Ok(copy)
}
fn no_null(value: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    match value {
        Value::Null => Err(reject("INVALID_SHAPE")),
        Value::Array(values) => {
            for value in values {
                session
                    .semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                no_null(value, session)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            for (_, value) in values {
                session
                    .semantic_work()
                    .visit(Visit::Field)
                    .map_err(core_error)?;
                no_null(value, session)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;
    fn raw() -> Vec<u8> {
        br#"{ "schema":"ledger-operation/1", "contract":"workflow/1", "key":{"logical_store_id":"store","scope":["tenant","local"],"source":"urn:test:source","operation_id":"original"}, "kind":"work.accept", "payload":{"schema":"ledger-workflow-work/1","opaque":"\ud83d\ude80"} }"#.to_vec()
    }
    #[test]
    fn owning_ingress_preserves_raw_dictionary_and_releases_all_request_copies() {
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let source = raw();
            let incoming = IncomingRequest::parse(&source, &mut session).unwrap();
            let legacy = request::Request::parse_financial(&source).unwrap();
            assert_eq!(incoming.raw(), source);
            assert_eq!(incoming.key(), legacy.key());
            assert_eq!(incoming.kind(), legacy.kind());
            assert_eq!(incoming.payload(&mut session).unwrap(), legacy.payload());
            assert_eq!(incoming.identity(), incoming.key().bytes().unwrap());
            assert!(arena.usage().unwrap().live_bytes > before);
        }
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
    #[test]
    fn owning_ingress_refuses_decoded_duplicate_null_unknown_and_raw_overflow() {
        let original = String::from_utf8(raw()).unwrap();
        for source in [
            original.replace(
                "\"kind\":\"work.accept\"",
                "\"kind\":\"work.accept\",\"\\u006bind\":\"work.accept\"",
            ),
            original.replace("\"opaque\":\"\\ud83d\\ude80\"", "\"opaque\":null"),
            original.replace(
                "\"contract\":\"workflow/1\"",
                "\"contract\":\"workflow/1\",\"owner\":true",
            ),
        ] {
            let arena = AuditArena::new().unwrap();
            let before = arena.usage().unwrap().live_bytes;
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(IncomingRequest::parse(source.as_bytes(), &mut session).is_err());
            assert!(session.semantic_work().visit(Visit::Constraint).is_err());
            drop(session);
            assert_eq!(arena.usage().unwrap().live_bytes, before);
        }
        let mut source = raw();
        source.resize(262144, b' ');
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert_eq!(
            IncomingRequest::parse(&source, &mut session)
                .unwrap()
                .raw()
                .len(),
            262144
        );
        source.push(b' ');
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(IncomingRequest::parse(&source, &mut session).is_err());
    }
}
