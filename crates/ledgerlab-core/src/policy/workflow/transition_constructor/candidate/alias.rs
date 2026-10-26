//! Fresh-key semantic aliases consume an actual sealed original candidate.
//! No caller-selected no-effects flag or saved result is promoted to money.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    alias_authority::construct_local_work_alias_authority, data::*, emitted_record::EncodedBody,
    records, shape, source_records::InitialSourceCorrespondence, CoreMeter,
};
use super::{
    CandidateEnvelope, CandidateExecutionMetadata, CandidateResultKind, CandidateResultMetadata,
    EconomicCandidate,
};
use crate::{
    canonical::{self, audit_arena::AuditArena},
    validation_work::{self as w, OriginalAuditSession, RepresentationKind},
    Result,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::mem;
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
fn literal(m: &mut CoreMeter, v: &Value, k: &str, want: &str) -> Result<()> {
    let actual = shape::text(m, v, k)?;
    if !shape::same_text(m, actual, want)? {
        return Err(shape::fail("alias original literal"));
    }
    Ok(())
}
fn exact_bytes(m: &mut CoreMeter, a: &[u8], b: &[u8]) -> Result<()> {
    if !shape::same_bytes(m, a, b)? {
        return Err(shape::fail("alias original exact bytes"));
    }
    Ok(())
}
fn canonical_view(
    view: EconomicRecordView<'_>,
    s: &mut OriginalAuditSession,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let a = s.arena().shared_handle();
    canonical_meter(view, &a, s.meter_mut())
}
fn canonical_meter(
    view: EconomicRecordView<'_>,
    a: &AuditArena,
    m: &mut CoreMeter,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    records::reference(m, view.reference)?;
    let value = m.parse_owned_representation(a, view.canonical_body, RepresentationKind::Record)?;
    let canonical =
        m.canonical_owned_representation(a, value.value(), RepresentationKind::Record)?;
    exact_bytes(m, canonical.bytes(), view.canonical_body)?;
    Ok(value)
}
fn receipt_checked(
    view: EconomicRecordView<'_>,
    op: &Value,
    results: &Value,
    a: &AuditArena,
    m: &mut CoreMeter,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let value = canonical_meter(view, a, m)?;
    let v = value.value();
    shape::closed(
        m,
        v,
        &[
            "schema",
            "key",
            "accepted_at",
            "commit_sequence",
            "request_hash",
            "request_hash_profile",
            "original_request_bytes_sha256",
            "schema_bundle_digest",
            "descriptor_closure_digest",
            "normalization_profile",
            "manifest_hash",
            "result_kind",
            "result_refs",
            "receipt_hash",
        ],
        &[],
    )?;
    literal(m, v, "schema", "ledger-operation-receipt/2")?;
    literal(m, v, "result_kind", "economic")?;
    literal(
        m,
        v,
        "request_hash_profile",
        "bean-counter/operation-request/2",
    )?;
    literal(
        m,
        v,
        "normalization_profile",
        "ledger-operation-normalization/2",
    )?;
    for k in ["key", "accepted_at", "commit_sequence"] {
        shape::equal(m, &v[k], &op[k])?;
    }
    shape::equal(m, &v["result_refs"], results)?;
    let evidence = shape::field(m, op, "request_evidence")?;
    for k in [
        "request_hash",
        "original_request_bytes_sha256",
        "schema_bundle_digest",
    ] {
        shape::equal(m, &v[k], &evidence[k])?;
    }
    shape::equal(
        m,
        &v["descriptor_closure_digest"],
        &evidence["descriptor_resolution"]["closure_digest"],
    )?;
    let unhashed = a.object_from_fields(
        &[
            ("schema", &v["schema"]),
            ("key", &v["key"]),
            ("accepted_at", &v["accepted_at"]),
            ("commit_sequence", &v["commit_sequence"]),
            ("request_hash", &v["request_hash"]),
            ("request_hash_profile", &v["request_hash_profile"]),
            (
                "original_request_bytes_sha256",
                &v["original_request_bytes_sha256"],
            ),
            ("schema_bundle_digest", &v["schema_bundle_digest"]),
            ("descriptor_closure_digest", &v["descriptor_closure_digest"]),
            ("normalization_profile", &v["normalization_profile"]),
            ("manifest_hash", &v["manifest_hash"]),
            ("result_kind", &v["result_kind"]),
            ("result_refs", &v["result_refs"]),
        ],
        m,
    )?;
    let bytes =
        m.canonical_owned_representation(a, unhashed.value(), RepresentationKind::Record)?;
    let domain = b"bean-counter/operation-receipt/2\0";
    let n = w::usize_add(m, domain.len(), bytes.bytes().len())?;
    m.hash_bytes(n)?;
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes.bytes());
    let hex = canonical::hex_with_work(&h.finalize(), m)?;
    let digest = a.string_value(&format!("sha256:{hex}"), m)?;
    shape::equal(m, &v["receipt_hash"], digest.value())?;
    if !shape::same_text(m, &view.reference.kind, "workflow-receipt")?
        || !shape::same_text(
            m,
            &view.reference.digest,
            digest
                .value()
                .as_str()
                .ok_or_else(|| shape::fail("alias receipt digest"))?,
        )?
    {
        return Err(shape::fail("alias original receipt Ref"));
    }
    let expected = format!("wreceipt_{hex}");
    if !shape::same_text(m, &view.reference.id, &expected)? {
        return Err(shape::fail("alias original receipt identity"));
    }
    Ok(value)
}
/// Original must come from the SAME original interpreter (including exact
/// local10 reconstruction), never a boolean or reconstructed public effects.
/// Host original custody/member/cutoff and current held source/registration
/// remain mandatory. The returned candidate authorizes only a new wrapper /
/// evidence / receipt reusing the original results; all economic effects empty.
pub fn construct_local_work_alias_candidate(
    program: &SelectedProgram,
    capture: &super::super::input_selection::EconomicCapture<'_>,
    incoming_raw: &[u8],
    original: EconomicCandidate,
    origin: (
        EconomicRecordView<'_>,
        EconomicRecordView<'_>,
        EconomicRecordView<'_>,
    ),
    selected: (&[EconomicRecordView<'_>], &[&InitialSourceCorrespondence]),
    s: &mut OriginalAuditSession,
) -> Result<EconomicCandidate> {
    let result = (|| {
        let (work_view, operation_view, receipt_view) = origin;
        s.check_cancelled()?;
        let captured = super::super::economic::local_capture_context(capture, s)?;
        let c = &captured.context;
        program.require_attempt_owner(s.meter_mut())?;
        if !program.uses_shared_namespace_recipe()
            || !original.result_metadata().is_local_execution()
            || original.result_metadata().result_kind() != "economic"
        {
            return Err(shape::fail(
                "exact original local economic candidate required for alias",
            ));
        }
        let a = s.arena().shared_handle();
        let work = canonical_view(work_view, s)?;
        literal(
            s.meter_mut(),
            work.value(),
            "schema",
            "ledger-workflow-work-record/1",
        )?;
        let op = canonical_view(operation_view, s)?;
        shape::closed(
            s.meter_mut(),
            op.value(),
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
            ],
            &[],
        )?;
        literal(
            s.meter_mut(),
            op.value(),
            "schema",
            "ledger-workflow-economic-operation/10",
        )?;
        literal(s.meter_mut(), op.value(), "kind", "work.accept")?;
        literal(s.meter_mut(), op.value(), "contract", "workflow/1")?;
        literal(
            s.meter_mut(),
            op.value(),
            "closure_profile",
            "ledger-local-selected-inputs/1",
        )?;
        let original_key = &original.key;
        for (k, v) in [
            ("logical_store_id", original_key.logical_store_id.as_str()),
            ("source", original_key.source.as_str()),
            ("operation_id", original_key.operation_id.as_str()),
        ] {
            literal(s.meter_mut(), &work.value()["key"], k, v)?;
        }
        let scope = super::super::work_keys::scope(s.meter_mut(), original_key)?;
        shape::equal(s.meter_mut(), &work.value()["key"]["scope"], scope.value())?;
        shape::equal(s.meter_mut(), &op.value()["key"], &work.value()["key"])?;
        for k in ["received_at", "accepted_at"] {
            shape::equal(s.meter_mut(), &op.value()[k], &work.value()[k])?;
        }
        if !shape::same_text(
            s.meter_mut(),
            &original_key.logical_store_id,
            &c.key.logical_store_id,
        )? || original_key.scope != c.key.scope
        {
            return Err(shape::fail("alias original store/scope"));
        }
        if shape::same_text(s.meter_mut(), &original_key.source, &c.key.source)?
            && shape::same_text(
                s.meter_mut(),
                &original_key.operation_id,
                &c.key.operation_id,
            )?
        {
            return Err(shape::fail(
                "same key requires original retry, not alias append",
            ));
        }
        let original_payload = s.canonical_representation(
            &work.value()["payload"],
            RepresentationKind::OriginalRequest,
        )?;
        exact_bytes(
            s.meter_mut(),
            original_payload.bytes(),
            original.normalized_payload(),
        )?;
        let mut original_authority = None;
        for row in original.introduced() {
            s.meter_mut()
                .visit("alias.original_authority_introduction")?;
            if shape::same_ref(
                s.meter_mut(),
                row.reference(),
                original.result_metadata().authority_ref(),
            )? {
                if original_authority.is_some() {
                    return Err(shape::fail("ambiguous original alias authority"));
                }
                original_authority = Some(row);
            }
        }
        let original_authority =
            original_authority.ok_or_else(|| shape::fail("missing original alias authority"))?;
        let original_authority = s.parse_representation(
            original_authority.canonical_body(),
            RepresentationKind::Authority,
        )?;
        shape::equal(
            s.meter_mut(),
            &op.value()["authority_snapshot"],
            original_authority.value(),
        )?;
        let constants = s.parse_representation(
            br#"{"schema":"ledger-operation/1","kind":"work.accept","contract":"workflow/1"}"#,
            RepresentationKind::SourceDefinition,
        )?;
        let normalized = a.object_from_fields(
            &[
                ("schema", &constants.value()["schema"]),
                ("key", &work.value()["key"]),
                ("kind", &constants.value()["kind"]),
                ("contract", &constants.value()["contract"]),
                ("payload", &work.value()["payload"]),
            ],
            s.meter_mut(),
        )?;
        shape::equal(
            s.meter_mut(),
            &op.value()["request_evidence"]["normalized_request"],
            normalized.value(),
        )?;
        let mut matched = false;
        for row in original.introduced() {
            s.meter_mut().visit("alias.original_work_introduction")?;
            if shape::same_ref(s.meter_mut(), row.reference(), work_view.reference)? {
                exact_bytes(
                    s.meter_mut(),
                    row.canonical_body(),
                    work_view.canonical_body,
                )?;
                if matched {
                    return Err(shape::fail("duplicate original Work introduction"));
                }
                matched = true;
            }
        }
        if !matched {
            return Err(shape::fail("alias Work is not original candidate result"));
        }
        let mut n =
            2 * mem::size_of::<EconomicCandidate>() + mem::size_of::<Box<EconomicCandidate>>();
        let count = w::usize_add(
            s.meter_mut(),
            selected.0.len(),
            original.consumed_refs.len(),
        )?;
        let count = w::usize_add(s.meter_mut(), count, original.introduced.len())?;
        let count = w::usize_add(s.meter_mut(), count, 2)?;
        for view in selected.0 {
            s.meter_mut().visit("alias.input_dimension")?;
            for text in [
                &view.reference.kind,
                &view.reference.id,
                &view.reference.digest,
            ] {
                n = w::usize_add(s.meter_mut(), n, text.len())?;
            }
        }
        for r in original
            .consumed_refs
            .iter()
            .chain(original.result.result_refs.iter())
            .chain([operation_view.reference, receipt_view.reference])
        {
            s.meter_mut().visit("alias.original_dimension")?;
            for text in [&r.kind, &r.id, &r.digest] {
                n = w::usize_add(s.meter_mut(), n, text.len())?;
            }
        }
        for _ in 0..count {
            n = w::usize_add(
                s.meter_mut(),
                n,
                mem::size_of::<ExactRef>() + mem::size_of::<&ExactRef>(),
            )?;
        }
        for r in &original.result.result_refs {
            n = w::usize_add(s.meter_mut(), n, mem::size_of::<ExactRef>())?;
            for text in [&r.kind, &r.id, &r.digest] {
                n = w::usize_add(s.meter_mut(), n, text.len())?;
            }
        }
        // Current key, authority Ref/schema and retained result Ref copies.
        for text in [
            &c.key.logical_store_id,
            &c.key.scope[0],
            &c.key.scope[1],
            &c.key.source,
            &c.key.operation_id,
        ] {
            n = w::usize_add(s.meter_mut(), n, text.len())?;
        }
        n = w::usize_add(s.meter_mut(), n, 4096)?;
        let ownership = a.reserve_external_payload(n, s.meter_mut())?;
        s.meter_mut().write_bytes(n)?;
        let mut borrowed = Vec::new();
        borrowed
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("alias input metadata allocation"))?;
        for r in &original.result.result_refs {
            borrowed.push(r);
        }
        let results = super::super::app_cost::reference_set(s.meter_mut(), &a, &borrowed)?;
        shape::equal(s.meter_mut(), &op.value()["result_refs"], results.value())?;
        receipt_checked(receipt_view, op.value(), results.value(), &a, s.meter_mut())?;
        let kind = a.string_value("workflow-operation", s.meter_mut())?;
        let kind = s.canonical_representation(kind.value(), RepresentationKind::Record)?;
        let scoped = s.canonical_representation(scope.value(), RepresentationKind::Record)?;
        let digest = records::hash_parts(
            s.meter_mut(),
            "workflow-record",
            &[kind.bytes(), scoped.bytes(), operation_view.canonical_body],
            true,
        )?;
        if !shape::same_text(s.meter_mut(), &digest, &operation_view.reference.digest)? {
            return Err(shape::fail("alias original operation digest"));
        }
        let opid = super::super::body::semantic_id(
            s.meter_mut(),
            "wop_",
            "workflow-operation",
            &work.value()["key"],
        )?;
        if !shape::same_text(
            s.meter_mut(),
            &operation_view.reference.kind,
            "workflow-operation",
        )? || !shape::same_text(s.meter_mut(), &operation_view.reference.id, &opid)?
        {
            return Err(shape::fail("alias original operation identity"));
        }
        let current =
            construct_local_work_alias_authority(program, c, work_view, incoming_raw, selected, s)?;
        let (snapshot, normalized, consumed) = current.into_parts();
        literal(
            s.meter_mut(),
            snapshot.value(),
            "schema",
            "ledger-workflow-economic-authority/8",
        )?;
        let schema = s.canonical_representation(
            &snapshot.value()["schema"],
            RepresentationKind::SourceDefinition,
        )?;
        let authority =
            s.canonical_representation(snapshot.value(), RepresentationKind::Authority)?;
        let digest = records::hash_parts(
            s.meter_mut(),
            "workflow-document",
            &[schema.bytes(), authority.bytes()],
            true,
        )?;
        let id = format!("wdoc_{}", &digest[7..]);
        let authority_ref = ExactRef {
            kind: copy(s.meter_mut(), "workflow-economic-authority")?,
            id,
            digest,
        };
        let inline = CandidateEnvelope {
            reference: reference(s.meter_mut(), &authority_ref)?,
            schema: copy(s.meter_mut(), "ledger-workflow-economic-authority/8")?,
            body: EncodedBody::owned(&a, authority, s.meter_mut())?,
        };
        borrowed.clear();
        for r in consumed
            .references
            .iter()
            .chain(original.consumed_refs.iter())
            .chain(original.result.result_refs.iter())
            .chain([operation_view.reference, receipt_view.reference])
        {
            borrowed.push(r);
        }
        // Copy the exact sorted union; the owning JSON array is DATA, not a
        // generic Ref/body traversal or a current-head selector.
        let used = super::super::app_cost::reference_set(s.meter_mut(), &a, &borrowed)?;
        let rows = used
            .value()
            .as_array()
            .ok_or_else(|| shape::fail("alias input Ref set"))?;
        s.meter_mut().check_resolved_dimensions(rows.len(), 0)?;
        let mut consumed_refs = Vec::new();
        consumed_refs
            .try_reserve_exact(rows.len())
            .map_err(|_| shape::fail("alias consumed allocation"))?;
        for row in rows {
            s.meter_mut().visit("alias.consumed_reference")?;
            consumed_refs.push(shape::reference(s.meter_mut(), row)?);
        }
        let mut result_refs = Vec::new();
        result_refs
            .try_reserve_exact(original.result.result_refs.len())
            .map_err(|_| shape::fail("alias reused result allocation"))?;
        for r in &original.result.result_refs {
            result_refs.push(reference(s.meter_mut(), r)?);
        }
        let key = OperationKey {
            logical_store_id: copy(s.meter_mut(), &c.key.logical_store_id)?,
            scope: [
                copy(s.meter_mut(), &c.key.scope[0])?,
                copy(s.meter_mut(), &c.key.scope[1])?,
            ],
            source: copy(s.meter_mut(), &c.key.source)?,
            operation_id: copy(s.meter_mut(), &c.key.operation_id)?,
        };
        let operation_body =
            s.copy_representation(operation_view.canonical_body, RepresentationKind::Record)?;
        let receipt_body =
            s.copy_representation(receipt_view.canonical_body, RepresentationKind::Record)?;
        let original_operation = CandidateEnvelope {
            reference: reference(s.meter_mut(), operation_view.reference)?,
            schema: copy(s.meter_mut(), "ledger-workflow-economic-operation/10")?,
            body: EncodedBody::owned(&a, operation_body, s.meter_mut())?,
        };
        let original_receipt = CandidateEnvelope {
            reference: reference(s.meter_mut(), receipt_view.reference)?,
            schema: copy(s.meter_mut(), "ledger-operation-receipt/2")?,
            body: EncodedBody::owned(&a, receipt_body, s.meter_mut())?,
        };
        Ok(EconomicCandidate {
            key,
            normalized_payload: EncodedBody::owned(&a, normalized, s.meter_mut())?,
            introduced: Vec::new(),
            consumed_refs,
            heads: Vec::new(),
            claims: Vec::new(),
            links: Vec::new(),
            dependencies: Vec::new(),
            funding: Vec::new(),
            account_enrollments: Vec::new(),
            result: CandidateResultMetadata {
                kind: CandidateResultKind::SemanticAlias,
                result_refs,
                authority_ref,
                execution: CandidateExecutionMetadata::LocalSelected1,
            },
            _ownership: None,
            _alias_origin: Some(Box::new(original)),
            _alias_ownership: Some(ownership),
            inline_authority: Some(inline),
            alias_origin_records: Some((original_operation, original_receipt)),
        })
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

impl EconomicCandidate {
    /// Only the exact private alias origin has a receipt-domain exception.
    /// Every consumer repeats the same canonical receipt2 hash and joins;
    /// unrelated/non-alias rows keep the ordinary immutable-record kernel.
    pub(in crate::policy::workflow::transition_constructor) fn checked_alias_origin_body(
        &self,
        m: &mut CoreMeter,
        a: &AuditArena,
        row: EconomicRecordView<'_>,
    ) -> Result<Option<crate::canonical::audit_arena::AuditedValue>> {
        let Some((operation, receipt)) = &self.alias_origin_records else {
            return Ok(None);
        };
        let is_operation = shape::same_ref(m, row.reference, operation.reference())?;
        if !is_operation && !shape::same_ref(m, row.reference, receipt.reference())? {
            return Ok(None);
        }
        if self.result_metadata().result_kind() != "semantic_alias" {
            return Err(shape::fail("alias receipt on non-alias candidate"));
        }
        if is_operation {
            exact_bytes(m, row.canonical_body, operation.canonical_body())?;
            return canonical_meter(row, a, m).map(Some);
        }
        exact_bytes(m, row.canonical_body, receipt.canonical_body())?;
        let op = canonical_meter(
            EconomicRecordView {
                reference: operation.reference(),
                canonical_body: operation.canonical_body(),
            },
            a,
            m,
        )?;
        let body = receipt_checked(row, op.value(), &op.value()["result_refs"], a, m)?;
        Ok(Some(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn receipt_ref(value: &mut Value) -> ExactRef {
        let mut h = Sha256::new();
        h.update(b"bean-counter/operation-receipt/2\0");
        h.update(
            canonical::CanonicalBytes::from_value(value)
                .unwrap()
                .as_slice(),
        );
        let hex = canonical::hex(&h.finalize());
        value["receipt_hash"] = json!(format!("sha256:{hex}"));
        ExactRef {
            kind: "workflow-receipt".into(),
            id: format!("wreceipt_{hex}"),
            digest: format!("sha256:{hex}"),
        }
    }
    #[test]
    fn alias_origin_receipt_uses_its_exact_domain_and_operation_joins() {
        let a = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let key = json!({"logical_store_id":"store","scope":["scope","test"],"source":"worker","operation_id":"original"});
        let hash = format!("sha256:{}", "0".repeat(64));
        let at = "2026-10-26T12:00:00Z";
        let results = json!([]);
        let op = json!({"key":key,"accepted_at":at,"commit_sequence":"12","result_refs":results,"request_evidence":{"request_hash":hash,"original_request_bytes_sha256":hash,"schema_bundle_digest":hash,"descriptor_resolution":{"closure_digest":hash}}});
        let mut v = json!({"schema":"ledger-operation-receipt/2","key":key,"accepted_at":at,"commit_sequence":"12","request_hash":hash,"request_hash_profile":"bean-counter/operation-request/2","original_request_bytes_sha256":hash,"schema_bundle_digest":hash,"descriptor_closure_digest":hash,"normalization_profile":"ledger-operation-normalization/2","manifest_hash":hash,"result_kind":"economic","result_refs":results});
        let r = receipt_ref(&mut v);
        let raw = canonical::CanonicalBytes::from_value(&v).unwrap();
        receipt_checked(
            EconomicRecordView {
                reference: &r,
                canonical_body: raw.as_slice(),
            },
            &op,
            &results,
            &a,
            s.meter_mut(),
        )
        .unwrap();
        // A validly rehashed alternate receipt cannot change its producer.
        v.as_object_mut().unwrap().remove("receipt_hash");
        v["accepted_at"] = json!("2026-10-26T12:00:01Z");
        let other = receipt_ref(&mut v);
        let raw = canonical::CanonicalBytes::from_value(&v).unwrap();
        assert!(receipt_checked(
            EconomicRecordView {
                reference: &other,
                canonical_body: raw.as_slice()
            },
            &op,
            &results,
            &a,
            s.meter_mut()
        )
        .is_err());
        // Ordinary record-domain hashes do not authenticate receipts.
        v["accepted_at"] = json!(at);
        v.as_object_mut().unwrap().remove("receipt_hash");
        let mut r = receipt_ref(&mut v);
        r.digest = hash;
        let raw = canonical::CanonicalBytes::from_value(&v).unwrap();
        assert!(receipt_checked(
            EconomicRecordView {
                reference: &r,
                canonical_body: raw.as_slice()
            },
            &op,
            &results,
            &a,
            s.meter_mut()
        )
        .is_err());
    }
}
