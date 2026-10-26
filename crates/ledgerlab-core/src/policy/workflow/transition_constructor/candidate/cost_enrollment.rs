//! Pre-policy installation-local supplier account enrollment. The explicit
//! selected support fixes this management recipe; no Binding is fabricated.
use super::super::{
    body,
    data::*,
    inventory,
    records::Records,
    representation::InvocationToken,
    shape,
    working_value::{owning_reference, WorkingValue},
    CoreMeter,
};
use super::*;
use crate::{
    policy::workflow::selected_program::{
        local_stage_lifecycle_compiler_descriptor_with_session, SelectedCompilerSupport,
    },
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Error, Result,
};
use std::mem;

/// Exact locked lookup hits. Empty hits mean absence only under the held host
/// account/authority locks; this DATA does not certify completeness or custody.
pub struct CostAuthorityEnrollmentState<'a> {
    pub occupied_accounts: &'a [ExactRef],
}
pub struct LocalCostAuthorityEnrollment {
    candidate: EconomicCandidate,
    evidence: super::super::local_financial_evidence::LocalFinancialRequestEvidence,
}
impl LocalCostAuthorityEnrollment {
    pub fn into_parts(
        self,
    ) -> (
        EconomicCandidate,
        super::super::local_financial_evidence::LocalFinancialRequestEvidence,
    ) {
        (self.candidate, self.evidence)
    }
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn literal(m: &mut CoreMeter, v: &serde_json::Value, k: &str, want: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("cost enrollment literal"));
    }
    Ok(())
}
fn produce(
    c: &DraftContextData,
    raw: &[u8],
    root: &ExactRef,
    inputs: &[EconomicRecordView<'_>],
    state: &CostAuthorityEnrollmentState,
    m: &mut CoreMeter,
) -> Result<EconomicCandidate> {
    if !c.authorities.is_empty() || !c.source_observation_inputs.is_empty() {
        return Err(shape::fail(
            "pre-policy cost enrollment has no economic Binding capture",
        ));
    }
    if !state.occupied_accounts.is_empty() {
        return Err(Error::new(
            "IDENTITY_CONFLICT",
            "supplier authority/account already enrolled",
        ));
    }
    let a = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning account enrollment"))?;
    for text in [
        &c.key.logical_store_id,
        &c.key.scope[0],
        &c.key.scope[1],
        &c.key.operation_id,
        &c.actor,
    ] {
        let value = a.string_value(text, m)?;
        inventory::binding::id(m, value.value(), 128)?;
    }
    for text in [&c.key.source, &c.canonical_economic_namespace] {
        let value = a.string_value(text, m)?;
        inventory::binding::id(m, value.value(), 256)?;
    }
    if !shape::same_text(m, &c.installation_mode, "real_local")?
        && !shape::same_text(m, &c.installation_mode, "isolated_synthetic")?
    {
        return Err(shape::fail("cost enrollment installation mode"));
    }
    let received = crate::domain::Timestamp::parse_with_work(&c.received_at, m)?.micros();
    let accepted = crate::domain::Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if w::compare(m, received, accepted)?.is_gt() {
        return Err(shape::fail("cost enrollment trusted time order"));
    }
    let request = WorkingValue::parse(m, raw)?;
    shape::closed(
        m,
        &request,
        &["schema", "key", "kind", "contract", "payload"],
        &[],
    )?;
    literal(m, &request, "schema", "ledger-operation/1")?;
    literal(m, &request, "contract", "workflow/1")?;
    literal(m, &request, "kind", "authority.change")?;
    let key = super::super::cost_postings::operation_key(m, &a, c)?;
    shape::equal(m, &request["key"], key.value())?;
    let mut p = WorkingValue::copy(m, &request["payload"])?;
    shape::closed(
        m,
        &p,
        &[
            "schema",
            "authority_id",
            "provider_account_identities",
            "evidence_refs",
        ],
        &[],
    )?;
    literal(m, &p, "schema", "ledger-cost-authority-enroll/1")?;
    inventory::binding::id(m, &p["authority_id"], 128)?;
    p.normalize_set(m, "provider_account_identities", false)?;
    p.normalize_set(m, "evidence_refs", false)?;
    let accounts = shape::array(m, &p, "provider_account_identities")?;
    if w::compare(m, accounts.len(), 1usize)?.is_lt()
        || w::compare(m, accounts.len(), 16usize)?.is_gt()
    {
        return Err(shape::fail("supplier account enrollment bound"));
    }
    for v in accounts {
        m.visit("cost.enrollment_account")?;
        shape::closed(
            m,
            v,
            &["provider_namespace", "provider_account_id", "environment"],
            &[],
        )?;
        for k in ["provider_namespace", "provider_account_id", "environment"] {
            inventory::binding::id(m, &v[k], if k == "provider_namespace" { 256 } else { 128 })?;
        }
    }
    let evidence = shape::array(m, &p, "evidence_refs")?;
    if w::compare(m, evidence.len(), 1usize)?.is_lt()
        || w::compare(m, evidence.len(), 16usize)?.is_gt()
    {
        return Err(shape::fail("account enrollment evidence bound"));
    }
    for value in evidence {
        shape::reference(m, value)?;
    }
    let mut records = Records::borrowed_app(m, &a, &c.key, inputs, &[])?;
    let root_value = owning_reference(m, &a, root)?;
    let actor = a.string_value(&c.actor, m)?;
    let source = a.string_value(&c.key.source, m)?;
    let at = a.string_value(&c.accepted_at, m)?;
    let before = a.object_from_fields(&[("accepted_at", at.value())], m)?;
    let root_body = inventory::evidence::root(
        m,
        &mut records,
        c,
        &InvocationToken::new(),
        root_value.value(),
        before.value(),
        ("accepted_at", actor.value(), Some(source.value())),
    )?;
    literal(
        m,
        &root_body["basis"],
        "canonical_economic_namespace",
        &c.canonical_economic_namespace,
    )?;
    // The normalized explicit assertion and the actual retained account evidence
    // share this authenticated local owner. These are supporting records, not
    // remotely verified ownership or a source grant substituted for custody.
    for v in shape::array(m, &p, "evidence_refs")? {
        let r = shape::reference(m, v)?;
        let evidence = records.get(
            m,
            &c.key,
            &r,
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1",
        )?;
        literal(m, &evidence, "purpose", "funding")?;
        shape::closed(
            m,
            &evidence["metadata"],
            &["resource_owner", "payer", "mode"],
            &[],
        )?;
        shape::closed(m, &evidence["subject"], &["customer", "agreement_id"], &[])?;
        literal(m, &evidence["metadata"], "mode", "platform")?;
        shape::equal(
            m,
            &evidence["metadata"]["resource_owner"],
            &evidence["metadata"]["payer"],
        )?;
        literal(m, &evidence, "media_type", "text/plain;charset=utf-8")?;
        let encoded = shape::text(m, &evidence, "content_base64")?;
        let content =
            m.decode_owned_base64_representation(&a, encoded, RepresentationKind::EvidenceContent)?;
        let assertion =
            m.parse_owned_representation(&a, content.bytes(), RepresentationKind::EvidenceContent)?;
        shape::closed(
            m,
            assertion.value(),
            &[
                "statement",
                "principal_id",
                "logical_store_id",
                "provider_account_identities",
            ],
            &[],
        )?;
        literal(
            m,
            assertion.value(),
            "statement",
            "local_owner_controls_provider_accounts",
        )?;
        literal(m, assertion.value(), "principal_id", &c.actor)?;
        literal(
            m,
            assertion.value(),
            "logical_store_id",
            &c.key.logical_store_id,
        )?;
        let mut assertion = WorkingValue::Owned(assertion);
        assertion.normalize_set(m, "provider_account_identities", false)?;
        shape::equal(
            m,
            &assertion["provider_account_identities"],
            &p["provider_account_identities"],
        )?;
        inventory::evidence::retained(
            m,
            &mut records,
            c,
            &InvocationToken::new(),
            &evidence,
            "funding",
            (
                &evidence["subject"],
                &evidence["metadata"],
                before.value(),
                "accepted_at",
            ),
        )?;
    }
    let mut size = mem::size_of::<EconomicCandidate>()
        + 4 * mem::size_of::<CandidateEnvelope>()
        + 16 * mem::size_of::<AccountEnrollmentEffect>()
        + 8192;
    for v in accounts {
        for k in ["provider_namespace", "provider_account_id", "environment"] {
            let text = shape::text(m, v, k)?;
            size = w::usize_add(m, size, text.len())?;
        }
    }
    for r in records.consumed() {
        size = w::usize_add(m, size, mem::size_of::<ExactRef>())?;
        for text in [&r.kind, &r.id, &r.digest] {
            size = w::usize_add(m, size, text.len())?;
        }
    }
    for s in [
        &c.key.logical_store_id,
        &c.key.scope[0],
        &c.key.scope[1],
        &c.key.source,
        &c.key.operation_id,
    ] {
        size = w::usize_add(m, size, s.len())?;
    }
    let ownership = a.reserve_external_payload(size, m)?;
    let constants=m.parse_owned_representation(&a,br#"{"authority":"ledger-cost-authority/1","activation":"ledger-cost-owner-activation/1","snapshot":"ledger-cost-enrollment-authority/1","operation":"authority.change","one":"1"}"#,RepresentationKind::SourceDefinition)?;
    let store = a.string_value(&c.key.logical_store_id, m)?;
    let mode = a.string_value(&c.installation_mode, m)?;
    let activation = a.object_from_fields(
        &[
            ("schema", &constants.value()["activation"]),
            ("authority_id", &p["authority_id"]),
            ("owner_logical_store", store.value()),
            ("installation_mode", mode.value()),
            ("local_authority_ref", root_value.value()),
            ("operation_key", key.value()),
            ("activated_at", at.value()),
        ],
        m,
    )?;
    let activation = body::document_borrowed(m, "workflow-authority", activation.value())?;
    let activation_ref = owning_reference(m, &a, &activation.reference)?;
    let administrators = a.array_from_values(&[root_value.value()], m)?;
    let authority = a.object_from_fields(
        &[
            ("schema", &constants.value()["authority"]),
            ("authority_id", &p["authority_id"]),
            ("revision", &constants.value()["one"]),
            (
                "provider_account_identities",
                &p["provider_account_identities"],
            ),
            ("owner_logical_store", store.value()),
            ("owner_activation_ref", activation_ref.value()),
            ("administrator_refs", administrators.value()),
            ("starts_at", at.value()),
        ],
        m,
    )?;
    let authority = body::document_borrowed(m, "workflow-authority", authority.value())?;
    let snapshot = a.object_from_fields(
        &[
            ("schema", &constants.value()["snapshot"]),
            ("operation", &constants.value()["operation"]),
            ("local_admin_snapshot", &root_body),
            ("installation_mode", mode.value()),
            ("evidence_refs", &p["evidence_refs"]),
            ("observed_at", at.value()),
        ],
        m,
    )?;
    let snapshot = body::document_borrowed(m, "workflow-economic-authority", snapshot.value())?;
    let mut account_enrollments = Vec::new();
    account_enrollments
        .try_reserve_exact(accounts.len())
        .map_err(|_| shape::fail("account effect allocation"))?;
    for v in accounts {
        let namespace = shape::text(m, v, "provider_namespace")?;
        let account = shape::text(m, v, "provider_account_id")?;
        let environment = shape::text(m, v, "environment")?;
        account_enrollments.push(AccountEnrollmentEffect {
            account: ProviderAccountIdentity {
                provider_namespace: copy(m, namespace)?,
                provider_account_id: copy(m, account)?,
                environment: copy(m, environment)?,
            },
            cost_authority_ref: reference(m, &authority.reference)?,
            owner_activation_ref: reference(m, &activation.reference)?,
        });
    }
    let mut introduced = Vec::new();
    introduced
        .try_reserve_exact(3)
        .map_err(|_| shape::fail("account result allocation"))?;
    for e in [activation, authority, snapshot] {
        let value = e.decoded(m, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        let schema = copy(m, schema)?;
        introduced.push(CandidateEnvelope {
            reference: e.reference,
            schema,
            body: e.canonical_body,
        });
    }
    let mut result_refs = Vec::new();
    result_refs
        .try_reserve_exact(3)
        .map_err(|_| shape::fail("cost result refs"))?;
    for e in &introduced {
        result_refs.push(reference(m, e.reference())?);
    }
    let result_refs = body::ref_set(m, result_refs)?;
    let result = CandidateResultMetadata {
        kind: CandidateResultKind::Administrative,
        result_refs,
        authority_ref: reference(m, introduced[2].reference())?,
        execution: CandidateExecutionMetadata::LocalSelected1,
    };
    let consumed = records.consumed();
    let mut consumed_refs = Vec::new();
    consumed_refs
        .try_reserve_exact(consumed.len())
        .map_err(|_| shape::fail("account consumed allocation"))?;
    for v in consumed {
        consumed_refs.push(reference(m, v)?);
    }
    let key = OperationKey {
        logical_store_id: copy(m, &c.key.logical_store_id)?,
        scope: [copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?],
        source: copy(m, &c.key.source)?,
        operation_id: copy(m, &c.key.operation_id)?,
    };
    Ok(EconomicCandidate {
        key,
        normalized_payload: p.bytes(m)?,
        introduced,
        consumed_refs,
        heads: Vec::new(),
        claims: Vec::new(),
        links: Vec::new(),
        dependencies: Vec::new(),
        funding: Vec::new(),
        account_enrollments,
        result,
        _ownership: None,
        _alias_origin: None,
        _alias_ownership: Some(ownership),
        inline_authority: None,
        alias_origin_records: None,
    })
}
/// Actual current private-directory administration is independently held by the
/// host; the pure child repeats its root/key/time/evidence joins and produces
/// the account absence effects for the same atomic append coordinator.
pub fn construct_local_cost_authority_enrollment(
    support: &SelectedCompilerSupport,
    c: &DraftContextData,
    raw: &[u8],
    root: &ExactRef,
    inputs: &[EconomicRecordView<'_>],
    state: &CostAuthorityEnrollmentState,
    s: &mut OriginalAuditSession,
) -> Result<LocalCostAuthorityEnrollment> {
    let result = (|| {
        let exact = local_stage_lifecycle_compiler_descriptor_with_session(s)?;
        if !shape::same_bytes(
            s.meter_mut(),
            support.canonical_bytes(),
            exact.canonical_bytes(),
        )? || support.execution_registry().is_none()
        {
            return Err(shape::fail(
                "exact code-owned Stage management support required",
            ));
        }
        let mut child = s.meter_mut().local_economic_meter()?;
        let candidate = produce(c, raw, root, inputs, state, &mut child)?;
        s.meter_mut().finish_local_economic(child)?;
        let evidence = super::super::local_financial_evidence::cost_enrollment(
            support, raw, &candidate, inputs, s,
        )?;
        Ok(LocalCostAuthorityEnrollment {
            candidate,
            evidence,
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use crate::canonical::{self, CanonicalBytes};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    fn hash(domain: &str, value: &Value) -> String {
        let mut h = Sha256::new();
        h.update(format!("bean-counter/{domain}/1\0"));
        h.update(CanonicalBytes::from_value(value).unwrap().as_slice());
        format!("sha256:{}", canonical::hex(&h.finalize()))
    }
    fn document(kind: &str, value: &Value) -> (ExactRef, CanonicalBytes) {
        let digest = hash("workflow-document", &json!([value["schema"], value]));
        (
            ExactRef {
                kind: kind.into(),
                id: format!("wdoc_{}", &digest[7..]),
                digest,
            },
            CanonicalBytes::from_value(value).unwrap(),
        )
    }
    #[test]
    fn account_enrollment_produces_exact_owner_and_global_absence_effect() {
        let a = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let support =
            SelectedCompilerSupport::local_stage_lifecycle_execution_with_session(&mut s).unwrap();
        let at = "2026-10-26T12:00:00Z";
        let c = DraftContextData {
            key: OperationKey {
                logical_store_id: "store".into(),
                scope: ["scope".into(), "test".into()],
                source: "urn:admin".into(),
                operation_id: "enroll-account".into(),
            },
            actor: "owner".into(),
            canonical_economic_namespace: "urn:admin".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: at.into(),
            accepted_at: at.into(),
            authorities: Vec::new(),
            source_observation_inputs: Vec::new(),
        };
        let basis = json!({"schema":"ledger-workflow-local-authority-basis/1","logical_store_id":"store","scope":["scope","test"],"principal_id":"owner","source":"urn:admin","canonical_economic_namespace":"urn:admin","rights":["admin","read"],"source_grant_revision":"1"});
        let root = json!({"schema":"ledger-workflow-local-authority-snapshot/1","basis":basis,"basis_digest":hash("local-authority-basis",&basis),"verification_profile":"local-private-directory-owner/1","verified_at":at});
        let (root_ref, root) = document("workflow-authority", &root);
        let account = json!({"provider_namespace":"urn:provider","provider_account_id":"account","environment":"test"});
        let statement = json!({"statement":"local_owner_controls_provider_accounts","principal_id":"owner","logical_store_id":"store","provider_account_identities":[account]});
        let content = CanonicalBytes::from_value(&statement).unwrap();
        let encoded = s
            .encode_base64_representation(content.as_slice(), RepresentationKind::EvidenceContent)
            .unwrap();
        let text = std::str::from_utf8(encoded.bytes()).unwrap();
        let evidence = json!({"schema":"ledger-workflow-retained-evidence/1","authority_ref":root_ref,"content_base64":text,"content_sha256":canonical::hex(&Sha256::digest(content.as_slice())),"installation_mode":"isolated_synthetic","media_type":"text/plain;charset=utf-8","metadata":{"resource_owner":"platform","payer":"platform","mode":"platform"},"purpose":"funding","retained_at":at,"retained_by":"owner","subject":{"customer":"customer","agreement_id":"agreement"}});
        let (evidence_ref, evidence) = document("workflow-evidence", &evidence);
        let inputs = [
            EconomicRecordView {
                reference: &root_ref,
                canonical_body: root.as_slice(),
            },
            EconomicRecordView {
                reference: &evidence_ref,
                canonical_body: evidence.as_slice(),
            },
        ];
        let raw = json!({"schema":"ledger-operation/1","key":c.key,"kind":"authority.change","contract":"workflow/1","payload":{"schema":"ledger-cost-authority-enroll/1","authority_id":"supplier-owner","provider_account_identities":[account],"evidence_refs":[evidence_ref]}});
        let raw = CanonicalBytes::from_value(&raw).unwrap();
        let output = construct_local_cost_authority_enrollment(
            &support,
            &c,
            raw.as_slice(),
            &root_ref,
            &inputs,
            &CostAuthorityEnrollmentState {
                occupied_accounts: &[],
            },
            &mut s,
        )
        .unwrap();
        let (candidate, evidence) = output.into_parts();
        assert_eq!(candidate.result_metadata().result_kind(), "administrative");
        assert_eq!(candidate.introduced().len(), 3);
        assert_eq!(candidate.account_enrollments().len(), 1);
        assert!(candidate.heads().is_empty());
        assert_eq!(
            candidate.account_enrollments()[0].cost_authority_ref().kind,
            "workflow-authority"
        );
        assert_eq!(
            evidence.value()["normalized_request"]["payload"]["provider_account_identities"][0],
            account
        );
        let mut retry = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        assert!(construct_local_cost_authority_enrollment(
            &support,
            &c,
            raw.as_slice(),
            &root_ref,
            &inputs,
            &CostAuthorityEnrollmentState {
                occupied_accounts: &[candidate.account_enrollments()[0]
                    .cost_authority_ref()
                    .clone()]
            },
            &mut retry
        )
        .is_err());
        let mut changed: Value = serde_json::from_slice(raw.as_slice()).unwrap();
        changed["payload"]["provider_account_identities"][0]["provider_account_id"] =
            json!("another-account");
        let changed = CanonicalBytes::from_value(&changed).unwrap();
        let mut mismatch = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        assert!(construct_local_cost_authority_enrollment(
            &support,
            &c,
            changed.as_slice(),
            &root_ref,
            &inputs,
            &CostAuthorityEnrollmentState {
                occupied_accounts: &[]
            },
            &mut mismatch
        )
        .is_err());
        let mut wrong = c;
        wrong.actor = "other-owner".into();
        let mut retry = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        assert!(construct_local_cost_authority_enrollment(
            &support,
            &wrong,
            raw.as_slice(),
            &root_ref,
            &inputs,
            &CostAuthorityEnrollmentState {
                occupied_accounts: &[]
            },
            &mut retry
        )
        .is_err());
    }
}
