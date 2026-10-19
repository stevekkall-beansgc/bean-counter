//! Recompute saved financial semantics from its literal request and captured
//! original inputs. This is DATA, never a completed origin or a commit token.
//! The coordinator still owns authenticated prefix/pre-state selection, complete
//! descriptor/manifest/receipt/audit witnesses and capacity/funding obligations.
use super::super::selected_program::SelectedProgram;
use super::{
    arithmetic, body, codec, construct,
    data::*,
    meter,
    records::{self, Records},
    shape, CoreMeter,
};
use crate::{canonical, domain::Timestamp, Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

macro_rules! call {
    ($f:path,$m:expr,$a:expr) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
fn fail(s: &str) -> Error {
    shape::fail(s)
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if shape::same(m, a, b)? {
        Ok(())
    } else {
        Err(fail("original financial DATA mismatch"))
    }
}
fn copy(m: &mut CoreMeter, v: &Value, k: &str) -> Result<String> {
    let s = shape::text(m, v, k)?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
/// Explicit raw operands, not a proof of historical selection or custody.
/// `original_request_bytes` is checked against BOTH saved base64 and SHA256.
pub(in crate::policy::workflow) struct SavedFinancialData<'a> {
    pub operation: &'a RecordInput,
    pub original_request_bytes: &'a [u8],
    pub activation_ref: &'a ExactRef,
    pub original_state: &'a DraftStateData,
    pub original_inputs: &'a [RecordInput],
}
/// Private fields prevent this from being mistaken for the host's completed
/// original certificate. It deliberately has no serialization or commit path.
pub(in crate::policy::workflow) struct ReconstructedFinancialData {
    operation_ref: ExactRef,
    commit_sequence: String,
    draft: DraftTransition,
    audit_usage: super::MeterUsage,
    economic_usage: super::MeterUsage,
}
impl ReconstructedFinancialData {
    pub(in crate::policy::workflow) fn operation_ref(&self) -> &ExactRef {
        &self.operation_ref
    }
    pub(in crate::policy::workflow) fn commit_sequence(&self) -> &str {
        &self.commit_sequence
    }
    pub(in crate::policy::workflow) fn draft_data(&self) -> &DraftTransition {
        &self.draft
    }
    pub(in crate::policy::workflow) fn audit_usage(&self) -> &super::MeterUsage {
        &self.audit_usage
    }
    pub(in crate::policy::workflow) fn economic_usage(&self) -> &super::MeterUsage {
        &self.economic_usage
    }

    pub(in crate::policy::workflow) fn unresolved_original_obligations(
        &self,
    ) -> &'static [&'static str] {
        &[
            "authenticated-prefix-and-exact-historical-prestate-selection",
            "full-original-descriptor-extraction-and-request-hash",
            "financial-observation-and-combined-audit-proof",
            "exact-manifest-receipt-and-custody",
            "dependency-capacity-funding-and-composed-resource-proof",
        ]
    }
}
// Exact ordinary saved tuples. Both registries explicitly carry the listed
// economic3 entries. Causal4 is NOT accepted merely because its registry is known.
fn ordinary_tuple(m: &mut CoreMeter, kind: &str, payload: &str, registry: &str) -> Result<()> {
    const ROWS: &[(&str, &str, &str)] = &[
        (
            "work.accept",
            "ledger-workflow-work/1",
            "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
        ),
        (
            "work.accept",
            "ledger-workflow-work/1",
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
        ),
    ];
    for (k, p, r) in ROWS {
        m.visit("original.saved_recipe")?;
        if shape::same_text(m, kind, k)?
            && shape::same_text(m, payload, p)?
            && shape::same_text(m, registry, r)?
        {
            return Ok(());
        }
    }
    Err(Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "original financial tuple not implemented",
    ))
}
/// Compare canonical padded base64 against raw bytes without allocating a
/// second encoded request. Every executed numeric operation is paid before use.
fn raw_evidence(m: &mut CoreMeter, e: &Value, raw: &[u8]) -> Result<()> {
    use crate::validation_work::{self as w, ArithmeticOperation as Op};
    shape::closed(
        m,
        e,
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
        &[],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, e, "schema")?,
        &json!("ledger-operation-request-evidence/2")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, e, "normalization_profile")?,
        &json!("ledger-operation-normalization/2")
    )?;
    if !meter::cmp(m, &raw.len().into(), &0.into())?.is_gt()
        || meter::cmp(m, &raw.len().into(), &262144.into())?.is_gt()
    {
        return Err(fail("original request byte ceiling"));
    }
    let encoded = shape::text(m, e, "original_request_bytes_base64")?.as_bytes();
    let rounded = w::usize_add(m, raw.len(), 2)?;
    let groups = w::i64_operation(m, Op::DivideRemainder, rounded as i64, 3)?;
    let length = w::i64_operation(m, Op::Multiply, groups, 4)?;
    if !meter::cmp(m, &encoded.len().into(), &length.into())?.is_eq() {
        return Err(fail("original request base64 length"));
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (index, chunk) in raw.chunks(3).enumerate() {
        m.visit("original.base64_group")?;
        m.scan_bytes(chunk.len())?;
        let mut value = 0i64;
        for i in 0..3 {
            let byte = if meter::cmp(m, &i.into(), &chunk.len().into())?.is_lt() {
                chunk[i] as i64
            } else {
                0
            };
            let shifted = w::i64_operation(m, Op::Multiply, value, 256)?;
            value = w::i64_operation(m, Op::Add, shifted, byte)?;
        }
        let mut out = [b'='; 4];
        let mut denominator = 262144i64;
        for (i, digit) in out.iter_mut().enumerate() {
            let count = w::usize_add(m, chunk.len(), 1)?;
            if meter::cmp(m, &i.into(), &count.into())?.is_lt() {
                let q = w::i64_operation(m, Op::DivideRemainder, value, denominator)?;
                let rem = w::i64_remainder(m, q, 64)?;
                *digit = alphabet[rem as usize];
            }
            denominator = w::i64_operation(m, Op::DivideRemainder, denominator, 64)?;
        }
        let start = w::i64_operation(m, Op::Multiply, index as i64, 4)? as usize;
        let end = w::usize_add(m, start, 4)?;
        if !shape::same_bytes(m, &out, &encoded[start..end])? {
            return Err(fail("original request base64 identity"));
        }
    }
    m.hash_bytes(raw.len())?;
    let digest = canonical::hex_with_work(&Sha256::digest(raw), m)?;
    if !call!(
        shape::same_text,
        m,
        &digest,
        call!(shape::text, m, e, "original_request_bytes_sha256")?
    )? {
        return Err(fail("original raw request SHA256"));
    }
    Ok(())
}
fn snapshot_context(
    m: &mut CoreMeter,
    op: &Value,
    activation: &ExactRef,
    inputs: &[RecordInput],
) -> Result<DraftContextData> {
    let key_value = shape::field(m, op, "key")?;
    shape::closed(
        m,
        key_value,
        &["logical_store_id", "scope", "source", "operation_id"],
        &[],
    )?;
    let key: OperationKey =
        serde_json::from_value(key_value.clone()).map_err(|_| fail("original operation key"))?;
    let a = shape::field(m, op, "authority_snapshot")?;
    shape::closed(
        m,
        a,
        &[
            "schema",
            "operation",
            "actor",
            "source",
            "scope",
            "customer",
            "canonical_economic_namespace",
            "installation_mode",
            "enrollment_ref",
            "current_state_ref",
            "current_grant_ref",
            "original_binding_ref",
            "original_accepted_agreement_ref",
            "original_grant_refs",
            "source_slot",
            "rights",
            "evidence_refs",
            "verified_fact_refs",
            "observed_at",
            "authority_observation_ref",
        ],
        &[],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, a, "schema")?,
        &json!("ledger-workflow-economic-authority/2")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, a, "operation")?,
        call!(shape::field, m, op, "kind")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, a, "source")?,
        &json!(key.source)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, a, "scope")?,
        &json!(key.scope)
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, a, "observed_at")?,
        call!(shape::field, m, op, "accepted_at")?
    )?;
    let mut grants = vec![];
    for value in shape::array(m, a, "original_grant_refs")? {
        grants.push(shape::reference(m, value)?)
    }
    let observation_ref = call!(
        shape::reference,
        m,
        call!(shape::field, m, a, "authority_observation_ref")?
    )?;
    let source_refs = shape::array(m, op, "source_observation_refs")?;
    call!(equal, m, &json!(source_refs), &json!([observation_ref]))?;
    let mut verifier = None;
    for input in inputs {
        if shape::same_ref(m, &input.reference, &observation_ref)? {
            let observation = codec::parse(m, &input.canonical_body, 4194304)?;
            verifier = Some(call!(
                shape::reference,
                m,
                call!(shape::field, m, &observation, "verifier_ref")?
            )?);
        }
    }
    let verifier = verifier.ok_or_else(|| fail("original source observation missing"))?;
    let mut observations = vec![];
    for input in inputs {
        m.visit("original.observation_input")?;
        let selected = shape::same_ref(m, &input.reference, &observation_ref)?
            || shape::same_ref(m, &input.reference, &verifier)?;
        let pending = call!(
            shape::member,
            m,
            call!(shape::array, m, op, "result_refs")?,
            &json!(input.reference)
        )?;
        if selected && pending {
            m.read_bytes(input.canonical_body.len())?;
            m.write_bytes(input.canonical_body.len())?;
            observations.push(input.clone());
        }
    }
    let c = DraftContextData {
        actor: copy(m, a, "actor")?,
        canonical_economic_namespace: copy(m, a, "canonical_economic_namespace")?,
        installation_mode: copy(m, a, "installation_mode")?,
        received_at: copy(m, op, "received_at")?,
        accepted_at: copy(m, op, "accepted_at")?,
        authorities: vec![AuthorityContextData {
            source_slot: copy(m, a, "source_slot")?,
            enrollment_ref: call!(
                shape::reference,
                m,
                call!(shape::field, m, a, "enrollment_ref")?
            )?,
            current_state_ref: call!(
                shape::reference,
                m,
                call!(shape::field, m, a, "current_state_ref")?
            )?,
            current_grant_ref: call!(
                shape::reference,
                m,
                call!(shape::field, m, a, "current_grant_ref")?
            )?,
            original_grant_refs: grants,
            binding_ref: call!(
                shape::reference,
                m,
                call!(shape::field, m, a, "original_binding_ref")?
            )?,
            accepted_agreement_ref: call!(
                shape::reference,
                m,
                call!(shape::field, m, a, "original_accepted_agreement_ref")?
            )?,
            activation_ref: activation.clone(),
            authority_observation_ref: Some(observation_ref),
        }],
        key,
        source_observation_inputs: observations,
    };
    super::inventory::entry::context(m, &c)?;
    let received = Timestamp::parse_with_work(&c.received_at, m)?.micros();
    let accepted = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if meter::cmp(m, &received.into(), &accepted.into())?.is_gt() {
        return Err(fail("original time order"));
    }
    Ok(c)
}
/// Header/observation partition is untrusted DATA until the host verifies the
/// historical prefix. New result bodies are never supplied as old operands.
fn partition_inputs(
    m: &mut CoreMeter,
    pool: &[RecordInput],
    preexisting: &[ExactRef],
    pending: &[RecordInput],
) -> Result<Vec<RecordInput>> {
    let mut out = vec![];
    for reference in preexisting {
        for introduced in pending {
            if shape::same_ref(m, reference, &introduced.reference)? {
                return Err(fail("pending representation declared preexisting"));
            }
        }
        let mut found = None;
        for input in pool {
            m.visit("original.preexisting_locator")?;
            if shape::same_text(m, &input.reference.kind, &reference.kind)?
                && shape::same_text(m, &input.reference.id, &reference.id)?
            {
                if !shape::same_ref(m, &input.reference, reference)? || found.is_some() {
                    return Err(fail("conflicting original input identity"));
                }
                found = Some(input);
            }
        }
        let input = found.ok_or_else(|| fail("missing exact pre-operation input"))?;
        m.read_bytes(input.canonical_body.len())?;
        m.write_bytes(input.canonical_body.len())?;
        out.push(input.clone());
    }
    Ok(out)
}
/// Reconstruct actual source-derived semantics; saved outputs are comparison
/// operands only. It refuses missing/extra results, including capacity records
/// the current selected constructor cannot yet derive. No prefix/receipt proof
/// or completed-origin capability is returned by this pure function.
pub(in crate::policy::workflow) fn reconstruct_original_financial(
    session: &mut crate::validation_work::OriginalAuditSession,
    program: &SelectedProgram,
    saved: &SavedFinancialData<'_>,
) -> Result<ReconstructedFinancialData> {
    let result = reconstruct_original_inner(session, program, saved);
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn reconstruct_original_inner(
    session: &mut crate::validation_work::OriginalAuditSession,
    program: &SelectedProgram,
    saved: &SavedFinancialData<'_>,
) -> Result<ReconstructedFinancialData> {
    let m = session.meter_mut();
    let preliminary = codec::parse(m, &saved.operation.canonical_body, 4194304)?;
    shape::closed(
        m,
        &preliminary,
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
        &[],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &preliminary, "schema")?,
        &json!("ledger-workflow-economic-operation/3")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &preliminary, "contract")?,
        &json!("workflow/1")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &preliminary, "closure_profile")?,
        &json!("ledger-economic-resolved-inputs/3")
    )?;
    let c = snapshot_context(m, &preliminary, saved.activation_ref, saved.original_inputs)?;
    let mut op_records = Records::new(m, std::slice::from_ref(saved.operation))?;
    let op = op_records.get(
        m,
        &c.key,
        &saved.operation.reference,
        "workflow-operation",
        "ledger-workflow-economic-operation/3",
    )?;
    let id = body::semantic_id(m, "wop_", "workflow-operation", &json!(c.key))?;
    if !shape::same_text(m, &id, &saved.operation.reference.id)? {
        return Err(fail("original operation identity"));
    }
    let sequence = copy(m, &op, "commit_sequence")?;
    if !call!(
        meter::cmp,
        m,
        &arithmetic::integer(m, &sequence, false)?,
        &0.into()
    )?
    .is_gt()
    {
        return Err(fail("original commit sequence"));
    }
    let evidence = shape::field(m, &op, "request_evidence")?;
    raw_evidence(m, evidence, saved.original_request_bytes)?;
    let raw = codec::parse(m, saved.original_request_bytes, 262144)?;
    shape::closed(
        m,
        &raw,
        &["schema", "key", "kind", "contract", "payload"],
        &[],
    )?;
    for (key, want) in [
        ("schema", json!("ledger-operation/1")),
        ("key", json!(c.key)),
        ("kind", op["kind"].clone()),
        ("contract", json!("workflow/1")),
    ] {
        call!(equal, m, call!(shape::field, m, &raw, key)?, &want)?;
    }
    let resolution = shape::field(m, evidence, "descriptor_resolution")?;
    shape::closed(
        m,
        resolution,
        &[
            "schema",
            "registry_digest",
            "operation_kind",
            "payload_schema",
            "locator_digest",
            "anchors",
            "descriptors",
            "normalization_profile",
            "closure_digest",
        ],
        &[],
    )?;
    call!(
        ordinary_tuple,
        m,
        shape::text(m, &op, "kind")?,
        shape::text(m, &raw["payload"], "schema")?,
        shape::text(m, resolution, "registry_digest")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, resolution, "schema")?,
        &json!("ledger-operation-descriptors/1")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, resolution, "operation_kind")?,
        call!(shape::field, m, &op, "kind")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, resolution, "payload_schema")?,
        call!(shape::field, m, &raw["payload"], "schema")?
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, resolution, "normalization_profile")?,
        &json!("ledger-operation-normalization/2")
    )?;
    let payload = codec::encode(m, &raw["payload"], 262144)?;
    let mut selection_records = Records::new(m, saved.original_inputs)?;
    let selection = super::financial_observation::validate(
        m,
        &mut selection_records,
        &c,
        &op,
        saved.original_state,
    )?;
    // No saved amounts or expected DraftTransition are passed to this call.
    let immediate = partition_inputs(
        m,
        saved.original_inputs,
        &selection.preexisting,
        &c.source_observation_inputs,
    )?;
    let mut economic = m.original_economic_meter()?;
    let draft = construct::draft_transition(
        program,
        &c,
        &payload,
        &immediate,
        saved.original_state,
        &mut economic,
    )?;
    let economic_usage = m.finish_original_economic(economic)?;
    let normalized_payload = codec::parse(m, &draft.normalized_payload, 262144)?;
    let normalized = json!({"schema":"ledger-operation/1","key":c.key,"kind":op["kind"],"contract":"workflow/1","payload":normalized_payload});
    call!(
        equal,
        m,
        call!(shape::field, m, evidence, "normalized_request")?,
        &normalized
    )?;
    let mut expected = vec![];
    let mut records = Records::new(m, saved.original_inputs)?;
    for envelope in &draft.new_envelopes {
        m.visit("original.result_compare")?;
        let value = codec::parse(m, &envelope.canonical_body, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        let retained = records.get(
            m,
            &c.key,
            &envelope.reference,
            &envelope.reference.kind,
            schema,
        )?;
        equal(m, &retained, &value)?;
        if shape::same_text(m, &envelope.reference.kind, "workflow-economic-authority")? {
            call!(
                equal,
                m,
                call!(shape::field, m, &op, "authority_snapshot")?,
                &value
            )?;
        }
        expected.push(envelope.reference.clone());
    }
    let expected = body::ref_set(m, expected)?;
    call!(
        equal,
        m,
        call!(shape::field, m, &op, "result_refs")?,
        &json!(expected)
    )?;
    // Preserve the actual full wire input, rather than the constructor's payload
    // projection, as this original request's raw byte-domain measurement.
    m.begin_request(saved.original_request_bytes)?;
    Ok(ReconstructedFinancialData {
        operation_ref: saved.operation.reference.clone(),
        commit_sequence: sequence,
        draft,
        audit_usage: m.snapshot_usage()?,
        economic_usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn introduced_verified_fact_is_comparison_only_but_reused_fact_is_input() {
        let mut m = CoreMeter::new(65536).unwrap();
        let value = body::document(
            &mut m,
            "workflow-verified-fact",
            json!({"schema":"ledger-workflow-verified-fact/1","synthetic":"partition-only"}),
        )
        .unwrap();
        let row = RecordInput {
            reference: value.reference.clone(),
            canonical_body: value.canonical_body.into_legacy().unwrap(),
        };
        let pool = vec![row.clone()];
        assert!(partition_inputs(&mut m, &pool, &[], &[])
            .unwrap()
            .is_empty());
        let prior =
            partition_inputs(&mut m, &pool, std::slice::from_ref(&row.reference), &[]).unwrap();
        assert_eq!(prior.len(), 1);
        assert_eq!(prior[0].canonical_body, row.canonical_body);
        assert!(partition_inputs(
            &mut m,
            &pool,
            std::slice::from_ref(&row.reference),
            std::slice::from_ref(&row)
        )
        .is_err());
        let mut changed = row.reference.clone();
        changed.digest = format!("sha256:{}", "0".repeat(64));
        assert!(partition_inputs(&mut m, &pool, &[changed], &[]).is_err());
        assert!(partition_inputs(&mut m, &[], &[row.reference], &[]).is_err());
    }
    #[test]
    fn ordinary_saved_tuple_never_widens_to_causal_or_cross_kind() {
        let mut m = CoreMeter::new(65536).unwrap();
        let r = "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54";
        assert!(ordinary_tuple(&mut m, "work.accept", "ledger-workflow-work/1", r).is_ok());
        assert!(ordinary_tuple(&mut m, "outcome.submit", "ledger-workflow-outcome/1", r).is_ok());
        assert!(ordinary_tuple(&mut m, "work.reverse", "ledger-workflow-work/1", r).is_err());
        assert!(ordinary_tuple(&mut m, "work.accept", "ledger-workflow-outcome/1", r).is_err());
        assert!(ordinary_tuple(
            &mut m,
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            r
        )
        .is_err());
    }
    #[test]
    fn exact_raw_evidence_rejects_changed_bytes_padding_hash_and_fields() {
        for (raw, encoded, sha) in [
            (
                b"f".as_slice(),
                "Zg==",
                "252f10c83610ebca1a059c0bae8255eba2f95be4d1d7bcfa89d7248a82d9f111",
            ),
            (
                b"fo".as_slice(),
                "Zm8=",
                "9c3aee7110b787f0fb5f81633a36392bd277ea945d44c874a9a23601aefe20cf",
            ),
            (
                b"foo".as_slice(),
                "Zm9v",
                "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
            ),
        ] {
            let e = json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":encoded,"original_request_bytes_sha256":sha,"normalized_request":{},"request_hash":"unconsumed-here","schema_bundle_digest":"unconsumed-here","descriptor_resolution":{}});
            let mut m = CoreMeter::new(65536).unwrap();
            assert!(raw_evidence(&mut m, &e, raw).is_ok());
            let mut wrong = e.clone();
            wrong["original_request_bytes_base64"] = json!("AAAA");
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
            let mut wrong = e.clone();
            wrong["original_request_bytes_sha256"] = json!("0".repeat(64));
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
            let mut wrong = e.clone();
            wrong["unexpected"] = json!(true);
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
        }
    }
}
