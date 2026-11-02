//! Current agreement heads are selected from completed original transitions,
//! then compared to the held projection. Neither a row nor a missing row alone
//! establishes the answer. This entry retains the existing local-owner rights.
use super::*;
use crate::store::sqlite::workflow::financial::{HeadKey, HeadLocator};
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as Kind,
        ValidationVisit as Visit,
    },
};

#[derive(Clone, Copy)]
enum Head {
    Acceptance,
    Activation,
}
impl Head {
    fn parse(text: &str, s: &mut OriginalAuditSession) -> Result<Self> {
        if acceptance::same_recipe_text(text, "agreement_acceptance", s)? {
            Ok(Self::Acceptance)
        } else if acceptance::same_recipe_text(text, "agreement_activation", s)? {
            Ok(Self::Activation)
        } else {
            Err(reject("INVALID_SHAPE"))
        }
    }
    fn text(self) -> &'static str {
        match self {
            Self::Acceptance => "agreement_acceptance",
            Self::Activation => "agreement_activation",
        }
    }
    fn domain(self) -> &'static str {
        match self {
            Self::Acceptance => "agreement-acceptance-head",
            Self::Activation => "agreement-activation-head",
        }
    }
    fn record(self) -> wire::AgreementRecord {
        match self {
            Self::Acceptance => wire::AgreementRecord::AcceptanceHead,
            Self::Activation => wire::AgreementRecord::ActivationHead,
        }
    }
    fn locator<'a>(self, customer: &'a str, agreement: &'a str) -> HeadKey<'a> {
        match self {
            Self::Acceptance => HeadKey::AgreementAcceptance {
                customer,
                agreement,
            },
            Self::Activation => HeadKey::AgreementActivation {
                customer,
                agreement,
            },
        }
    }
}

fn same_bytes(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "INTEGRITY_FAILURE")
}

// DATA comparison only. The caller must obtain `body` from its completed
// chronological inventory, never by loading the asserted projection's record.
fn checked_head(
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    head: Head,
    body: AuditedBytes,
    held: &HeadLocator,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let parsed = s
        .parse_representation(body.bytes(), Kind::Record)
        .map_err(core_error)?;
    for (field, expected) in [
        ("schema", "ledger-workflow-head-transition/1"),
        ("head_kind", head.text()),
        ("customer", customer),
        ("agreement_id", agreement),
    ] {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(parsed.value(), field, s)?,
            expected,
            s,
        )?;
    }
    let original_key = request_wrapper::field(parsed.value(), "operation_key", s)?;
    request_wrapper::same_text(
        request_wrapper::text(original_key, "logical_store_id", s)?,
        &key.logical_store_id,
        s,
    )?;
    let scope = request_wrapper::field(original_key, "scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&scope.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    for (actual, expected) in scope.iter().zip(&key.scope) {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        request_wrapper::same_text(
            actual.as_str().ok_or(ServiceError::IntegrityFailure)?,
            expected,
            s,
        )?;
    }
    let revision = request_wrapper::text(parsed.value(), "new_revision", s)?;
    let number = acceptance::counter(revision, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&number.into(), &0u8.into()])
        .map_err(core_error)?;
    require(number > 0, "INTEGRITY_FAILURE")?;
    same_bytes(held.revision.bytes(), revision.as_bytes(), s)?;
    let record = wire::agreement_record(
        head.record(),
        body,
        original_key,
        customer,
        agreement,
        revision,
        s,
    )?;
    for (actual, expected) in [
        (held.kind.bytes(), record.reference().kind.as_bytes()),
        (held.id.bytes(), record.reference().id.as_bytes()),
        (held.digest.bytes(), record.reference().digest.as_bytes()),
    ] {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        same_bytes(actual, expected, s)?;
    }
    Ok(record)
}

pub(super) async fn read(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    s: &mut OriginalAuditSession,
) -> Result<Value> {
    request_wrapper::closed(payload, &["customer", "agreement_id", "head_kind"], s)?;
    let customer = request_wrapper::text(payload, "customer", s)?;
    let agreement = request_wrapper::text(payload, "agreement_id", s)?;
    ingress::identifier(customer, 128, s)?;
    ingress::identifier(agreement, 128, s)?;
    let head = Head::parse(request_wrapper::text(payload, "head_kind", s)?, s)?;

    // This authenticates the actual local owner before looking up existence.
    // A caller-supplied customer is a selector, never a reader credential.
    ledger.admin.read(&ledger.store, tx, key).await?;
    let before = tx
        .workflow_management_sequence(s)
        .await
        .map_err(prefix::read_error)?;
    let (mut completed, _) = prefix::before_inputs(ledger, tx, key, None, before, None, s).await?;
    let customer_key = wire::string(customer, s)?;
    let agreement_key = wire::string(agreement, s)?;
    let state_key = prefix::inventory::StateKey::new(
        head.domain(),
        &[customer_key.bytes(), agreement_key.bytes()],
        s,
    )?;
    let body = match &mut completed {
        Some(completed) => completed._inventory.head_body(&state_key, s)?,
        None => None,
    };
    let held = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            head.locator(customer, agreement),
            s,
        )
        .await
        .map_err(prefix::read_error)?;
    let record = match (body, held) {
        (None, None) => None,
        (Some(body), Some(held)) => {
            let record = checked_head(key, customer, agreement, head, body, &held, s)?;
            let reference = wire::reference(record.reference(), s)?;
            let reference = s
                .parse_representation(reference.bytes(), Kind::Record)
                .map_err(core_error)?;
            let sequence = completed
                .as_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .introduced_at(reference.value(), record.bytes(), s)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&sequence.into(), &before.into()])
                .map_err(core_error)?;
            require(sequence < before, "INTEGRITY_FAILURE")?;
            Some(record)
        }
        _ => return Err(ServiceError::IntegrityFailure),
    };
    let kind = wire::string(head.text(), s)?;
    let output = if let Some(record) = &record {
        let parsed = s
            .parse_representation(record.bytes(), Kind::Record)
            .map_err(core_error)?;
        let revision = wire::string(request_wrapper::text(parsed.value(), "new_revision", s)?, s)?;
        let reference = wire::reference(record.reference(), s)?;
        wire::object(
            &[
                ("head_kind", kind.bytes()),
                ("head_ref", reference.bytes()),
                ("revision", revision.bytes()),
                ("schema", b"\"ledger-workflow-head-read/1\""),
            ],
            4096,
            s,
        )?
    } else {
        wire::object(
            &[
                ("head_kind", kind.bytes()),
                ("revision", b"\"0\""),
                ("schema", b"\"ledger-workflow-head-read/1\""),
            ],
            4096,
            s,
        )?
    };
    let parsed = s
        .parse_representation(output.bytes(), Kind::Record)
        .map_err(core_error)?;
    // The released read API transfers a Value to its caller. Pay its exact
    // owned copy before that final transfer, as with the other local reads.
    let (arena, work) = s.parts();
    let bytes = arena
        .value_copy_payload_bound(parsed.value(), work)
        .map_err(core_error)?;
    let _output_owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    s.check_cancelled().map_err(core_error)?;
    Ok(parsed.value().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;

    #[tokio::test]
    async fn agreement_head_read_preserves_exact_projection_and_authorized_absence() {
        // These are comparison inputs only, not a fabricated origin proof.
        let arena = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let key = OperationKey {
            logical_store_id: "head-read-test".into(),
            scope: ["example-company".into(), "local".into()],
            source: "actual-reader".into(),
            operation_id: "read".into(),
        };
        for head in [Head::Acceptance, Head::Activation] {
            let body = json!({
                "schema":"ledger-workflow-head-transition/1", "head_kind":head.text(),
                "customer":"customer-A", "agreement_id":"agreement-A",
                "new_revision":"2", "operation_key":key,
            });
            let raw = s.canonical_representation(&body, Kind::Record).unwrap();
            let expected = wire::agreement_record(
                head.record(),
                raw,
                &body["operation_key"],
                "customer-A",
                "agreement-A",
                "2",
                &mut s,
            )
            .unwrap();
            let locator =
                |revision: &str, digest: &str, s: &mut OriginalAuditSession| HeadLocator {
                    revision: s
                        .copy_representation(revision.as_bytes(), Kind::Record)
                        .unwrap(),
                    kind: s
                        .copy_representation(expected.reference().kind.as_bytes(), Kind::Record)
                        .unwrap(),
                    id: s
                        .copy_representation(expected.reference().id.as_bytes(), Kind::Record)
                        .unwrap(),
                    digest: s
                        .copy_representation(digest.as_bytes(), Kind::Record)
                        .unwrap(),
                };
            let held = locator("2", &expected.reference().digest, &mut s);
            let actual = checked_head(
                &key,
                "customer-A",
                "agreement-A",
                head,
                s.canonical_representation(&body, Kind::Record).unwrap(),
                &held,
                &mut s,
            )
            .unwrap();
            assert_eq!(actual.reference(), expected.reference());
            for held in [
                locator("1", &expected.reference().digest, &mut s),
                locator("2", &format!("sha256:{}", "0".repeat(64)), &mut s),
            ] {
                assert!(
                    checked_head(
                        &key,
                        "customer-A",
                        "agreement-A",
                        head,
                        s.canonical_representation(&body, Kind::Record).unwrap(),
                        &held,
                        &mut s
                    )
                    .is_err()
                );
            }
            assert!(
                checked_head(
                    &key,
                    "customer-B",
                    "agreement-A",
                    head,
                    s.canonical_representation(&body, Kind::Record).unwrap(),
                    &held,
                    &mut s
                )
                .is_err()
            );
            let mut wrong_scope = key.clone();
            wrong_scope.scope[1] = "other".into();
            assert!(
                checked_head(
                    &wrong_scope,
                    "customer-A",
                    "agreement-A",
                    head,
                    s.canonical_representation(&body, Kind::Record).unwrap(),
                    &held,
                    &mut s
                )
                .is_err()
            );
        }

        // Actual empty installation: the facade authenticates the owner and
        // exhausts the held original prefix before reporting absence.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        let setup: Value = serde_json::from_slice(include_bytes!(
            "../../../../../../../examples/billing/setup.json"
        ))
        .unwrap();
        crate::billing::BillingLedger::init(&path, &records::bytes(&setup).unwrap())
            .await
            .unwrap();
        let ledger = LocalWorkflowLedger::open_local(&path).await.unwrap();
        ledger.enable_profile_b().await.unwrap();
        for head in [Head::Acceptance, Head::Activation] {
            let request = json!({"schema":"ledger-read/1","kind":"agreement.head",
                "payload":{"customer":"customer-A","agreement_id":"absent","head_kind":head.text()}});
            let result = ledger
                .read(&records::bytes(&request).unwrap())
                .await
                .unwrap();
            assert_eq!(
                result,
                json!({"schema":"ledger-workflow-head-read/1",
                "head_kind":head.text(),"revision":"0"})
            );
            let mut invalid = request.clone();
            invalid["payload"]["head_kind"] = json!("latest");
            assert!(
                ledger
                    .read(&records::bytes(&invalid).unwrap())
                    .await
                    .is_err()
            );
            let mut invalid = request;
            invalid["payload"]["source"] = json!("pretend-reader");
            assert!(
                ledger
                    .read(&records::bytes(&invalid).unwrap())
                    .await
                    .is_err()
            );
        }
    }
}
