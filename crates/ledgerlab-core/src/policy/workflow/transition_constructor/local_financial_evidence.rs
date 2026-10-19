//! Literal local monetary request evidence. No mutation capability or audit
//! claim is produced here; the actual candidate supplies normalized intent.
use super::super::selected_program::SelectedProgram;
use super::{
    candidate::EconomicCandidate,
    data::*,
    shape,
    working_value::{owning_definition, owning_reference},
    CoreMeter,
};
use crate::{
    canonical::{
        self,
        audit_arena::{AuditArena, AuditedBytes, AuditedValue, ExternalBufferLease},
    },
    validation_work::{OriginalAuditSession, RepresentationKind as Kind, ValidationWork},
    Result,
};
use serde_json::Value;
use std::mem;
const REGISTRY: &str = "sha256:d0a21becb08150a781770cfef80b5521eeb93c3b45aa9ebdab23c800ef44ee4a";

/// Owned DATA retained by the local operation framer through the transaction.
pub struct LocalFinancialRequestEvidence {
    value: AuditedValue,
    bytes: AuditedBytes,
}
impl LocalFinancialRequestEvidence {
    pub fn value(&self) -> &Value {
        self.value.value()
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }
    pub fn request_hash(&self) -> &str {
        self.value.value()["request_hash"]
            .as_str()
            .expect("closed producer")
    }
}
fn string(m: &mut CoreMeter, a: &AuditArena, s: &str) -> Result<AuditedValue> {
    a.string_value(s, m)
}
fn object(m: &mut CoreMeter, a: &AuditArena, f: &[(&str, &Value)]) -> Result<AuditedValue> {
    a.object_from_fields(f, m)
}
fn encode(m: &mut CoreMeter, a: &AuditArena, v: &Value, k: Kind) -> Result<AuditedBytes> {
    m.canonical_owned_representation(a, v, k)
}
fn digest(
    m: &mut CoreMeter,
    a: &AuditArena,
    domain: &[u8],
    parts: &[&[u8]],
) -> Result<AuditedValue> {
    let mut size = domain.len();
    for p in parts {
        m.visit("local.evidence_digest_part_size")?;
        size = crate::validation_work::usize_add(m, size, p.len())?;
    }
    let mut b = a.byte_builder(size, m)?;
    b.append(domain, m)?;
    for p in parts {
        m.visit("local.evidence_digest_part")?;
        b.append(p, m)?;
    }
    let bytes = b.finish(m)?;
    let hash = m.hash_owned_representation(a, bytes.bytes(), Kind::OperationFrame)?;
    let hex = canonical::hex_with_work(&hash, m)?;
    let _scratch = a.reserve_external_payload(71 + mem::size_of::<String>(), m)?;
    m.write_bytes(71 + mem::size_of::<String>())?;
    let mut text = String::with_capacity(71);
    text.push_str("sha256:");
    text.push_str(&hex);
    string(m, a, &text)
}
fn key(m: &mut CoreMeter, a: &AuditArena, k: &OperationKey) -> Result<AuditedValue> {
    let store = string(m, a, &k.logical_store_id)?;
    let source = string(m, a, &k.source)?;
    let id = string(m, a, &k.operation_id)?;
    let x = string(m, a, &k.scope[0])?;
    let y = string(m, a, &k.scope[1])?;
    let scope = a.array_from_values(&[x.value(), y.value()], m)?;
    object(
        m,
        a,
        &[
            ("logical_store_id", store.value()),
            ("scope", scope.value()),
            ("source", source.value()),
            ("operation_id", id.value()),
        ],
    )
}
struct Set {
    rows: Vec<AuditedValue>,
    _owner: ExternalBufferLease,
}
impl Set {
    fn new(m: &mut CoreMeter, a: &AuditArena) -> Result<Self> {
        let n = 128 * mem::size_of::<AuditedValue>() + mem::size_of::<Vec<AuditedValue>>();
        let owner = a.reserve_external_payload(n, m)?;
        m.write_bytes(n)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(128)
            .map_err(|_| shape::fail("descriptor set allocation"))?;
        Ok(Self {
            rows,
            _owner: owner,
        })
    }
    fn insert(&mut self, m: &mut CoreMeter, a: &AuditArena, row: AuditedValue) -> Result<()> {
        let bytes = encode(m, a, row.value(), Kind::SourceDefinition)?;
        let mut at = self.rows.len();
        for (i, prior) in self.rows.iter().enumerate() {
            m.visit("local.evidence_set_member")?;
            let p = encode(m, a, prior.value(), Kind::SourceDefinition)?;
            let order = shape::byte_order(m, bytes.bytes(), p.bytes())?;
            if order.is_eq() {
                return Ok(());
            }
            if order.is_lt() {
                at = i;
                break;
            }
        }
        if !crate::validation_work::compare(m, self.rows.len(), 128usize)?.is_lt() {
            return Err(shape::fail("descriptor set limit"));
        }
        for _ in at..self.rows.len() {
            m.visit("local.evidence_set_move")?;
            m.write_bytes(mem::size_of::<AuditedValue>())?;
        }
        m.write_bytes(mem::size_of::<AuditedValue>())?;
        self.rows.insert(at, row);
        Ok(())
    }
    fn array(&self, m: &mut CoreMeter, a: &AuditArena) -> Result<AuditedValue> {
        let n = 128 * mem::size_of::<&Value>() + mem::size_of::<Vec<&Value>>();
        let _o = a.reserve_external_payload(n, m)?;
        m.write_bytes(n)?;
        let mut v = Vec::new();
        v.try_reserve_exact(self.rows.len())
            .map_err(|_| shape::fail("descriptor borrow allocation"))?;
        for r in &self.rows {
            m.visit("local.evidence_set_borrow")?;
            v.push(r.value());
        }
        a.array_from_values(&v, m)
    }
}
struct Anchors {
    locators: Set,
    anchors: Set,
}
impl Anchors {
    fn new(m: &mut CoreMeter, a: &AuditArena) -> Result<Self> {
        Ok(Self {
            locators: Set::new(m, a)?,
            anchors: Set::new(m, a)?,
        })
    }
    fn add(
        &mut self,
        m: &mut CoreMeter,
        a: &AuditArena,
        role: &str,
        relation: &str,
        reference: &Value,
        descriptors: &Value,
    ) -> Result<()> {
        super::shape::reference(m, reference)?;
        let role = string(m, a, role)?;
        let relation = string(m, a, relation)?;
        let locator = object(m, a, &[("ref", reference), ("role", role.value())])?;
        // A role/ref pair has one recipe relation. Repeated semantic use may
        // add original descriptors, never choose a conflicting relation.
        let mut merged = None;
        for (position, prior) in self.anchors.rows.iter().enumerate() {
            if shape::same(m, &prior.value()["role"], role.value())?
                && shape::same(m, &prior.value()["record_ref"], reference)?
            {
                if !shape::same(m, &prior.value()["relation"], relation.value())? {
                    return Err(shape::fail("conflicting descriptor anchor relation"));
                }
                let mut refs = Set::new(m, a)?;
                for value in [
                    shape::array(m, prior.value(), "descriptor_refs")?,
                    descriptors
                        .as_array()
                        .ok_or_else(|| shape::fail("anchor descriptor array"))?
                        .as_slice(),
                ] {
                    for r in value {
                        m.visit("local.anchor_descriptor_union")?;
                        let r = a.clone_value(r, m)?;
                        refs.insert(m, a, r)?;
                    }
                }
                let refs = refs.array(m, a)?;
                let row = object(
                    m,
                    a,
                    &[
                        ("descriptor_refs", refs.value()),
                        ("record_ref", reference),
                        ("relation", relation.value()),
                        ("role", role.value()),
                    ],
                )?;
                merged = Some((position, row));
                break;
            }
        }
        if let Some((position, row)) = merged {
            for _ in position..self.anchors.rows.len() {
                m.visit("local.anchor_merge_move")?;
                m.write_bytes(mem::size_of::<AuditedValue>())?;
            }
            self.anchors.rows.remove(position);
            self.anchors.insert(m, a, row)?;
            return Ok(());
        }
        let anchor = object(
            m,
            a,
            &[
                ("descriptor_refs", descriptors),
                ("record_ref", reference),
                ("relation", relation.value()),
                ("role", role.value()),
            ],
        )?;
        self.locators.insert(m, a, locator)?;
        self.anchors.insert(m, a, anchor)
    }
}

fn frame(
    m: &mut CoreMeter,
    a: &AuditArena,
    raw: &[u8],
    request: &Value,
    payload: &Value,
    anchors: &Anchors,
    descriptors: &Value,
) -> Result<LocalFinancialRequestEvidence> {
    let locators = anchors.locators.array(m, a)?;
    let anchors = anchors.anchors.array(m, a)?;
    let kind = shape::field(m, request, "kind")?;
    let schema = shape::field(m, payload, "schema")?;
    let tuple = a.array_from_values(&[kind, schema, locators.value()], m)?;
    let tuple = encode(m, a, tuple.value(), Kind::SourceClosure)?;
    let locator_digest = digest(
        m,
        a,
        b"bean-counter/descriptor-locators/1\0",
        &[tuple.bytes()],
    )?;
    let tag = string(m, a, "ledger-operation-descriptors/8")?;
    let registry = string(m, a, REGISTRY)?;
    let profile = string(m, a, "ledger-operation-normalization/2")?;
    let fields = [
        ("anchors", anchors.value()),
        ("descriptors", descriptors),
        ("locator_digest", locator_digest.value()),
        ("normalization_profile", profile.value()),
        ("operation_kind", kind),
        ("payload_schema", schema),
        ("registry_digest", registry.value()),
        ("schema", tag.value()),
    ];
    let base = object(m, a, &fields)?;
    let bytes = encode(m, a, base.value(), Kind::SourceDefinition)?;
    let closure = digest(
        m,
        a,
        b"bean-counter/descriptor-closure/1\0",
        &[bytes.bytes()],
    )?;
    let resolution = object(
        m,
        a,
        &[
            ("anchors", anchors.value()),
            ("closure_digest", closure.value()),
            ("descriptors", descriptors),
            ("locator_digest", locator_digest.value()),
            ("normalization_profile", profile.value()),
            ("operation_kind", kind),
            ("payload_schema", schema),
            ("registry_digest", registry.value()),
            ("schema", tag.value()),
        ],
    )?;
    let normalized = object(
        m,
        a,
        &[
            ("contract", &request["contract"]),
            ("key", &request["key"]),
            ("kind", kind),
            ("payload", payload),
            ("schema", &request["schema"]),
        ],
    )?;
    let tuple = a.array_from_values(&[closure.value(), normalized.value()], m)?;
    let bytes = encode(m, a, tuple.value(), Kind::OperationFrame)?;
    let request_hash = digest(
        m,
        a,
        b"bean-counter/operation-request/2\0",
        &[bytes.bytes()],
    )?;
    let bytes = encode(m, a, descriptors, Kind::SourceClosure)?;
    let bundle = digest(m, a, b"bean-counter/schema-bundle/1\0", &[bytes.bytes()])?;
    let raw_base64 = m.encode_owned_base64_representation(a, raw, Kind::OriginalRequest)?;
    let raw_base64 =
        std::str::from_utf8(raw_base64.bytes()).map_err(|_| shape::fail("base64 encoder UTF8"))?;
    let raw_base64 = string(m, a, raw_base64)?;
    let hash = m.hash_owned_representation(a, raw, Kind::OriginalRequest)?;
    let hex = canonical::hex_with_work(&hash, m)?;
    let raw_hash = string(m, a, &hex)?;
    let tag = string(m, a, "ledger-operation-request-evidence/2")?;
    let value = object(
        m,
        a,
        &[
            ("descriptor_resolution", resolution.value()),
            ("normalization_profile", profile.value()),
            ("normalized_request", normalized.value()),
            ("original_request_bytes_base64", raw_base64.value()),
            ("original_request_bytes_sha256", raw_hash.value()),
            ("request_hash", request_hash.value()),
            ("schema", tag.value()),
            ("schema_bundle_digest", bundle.value()),
        ],
    )?;
    let bytes = encode(m, a, value.value(), Kind::Record)?;
    Ok(LocalFinancialRequestEvidence { value, bytes })
}

fn checked_body(
    m: &mut CoreMeter,
    a: &AuditArena,
    key: &OperationKey,
    row: EconomicRecordView<'_>,
) -> Result<AuditedValue> {
    super::records::reference(m, row.reference)?;
    let body = m.parse_owned_representation(a, row.canonical_body, Kind::Record)?;
    super::codec::validate_selected_types(m, body.value())?;
    let encoded = encode(m, a, body.value(), Kind::Record)?;
    if !shape::same_bytes(m, encoded.bytes(), row.canonical_body)? {
        return Err(shape::fail("selected evidence operand canonical bytes"));
    }
    let schema = shape::field(m, body.value(), "schema")?;
    let schema = encode(m, a, schema, Kind::SourceDefinition)?;
    let digest = if row.reference.id.starts_with("wdoc_") {
        super::records::hash_parts(
            m,
            "workflow-document",
            &[schema.bytes(), encoded.bytes()],
            true,
        )?
    } else if row.reference.id.starts_with("wplan_") {
        super::records::hash_parts(m, "workflow-plan", &[encoded.bytes()], false)?
    } else {
        let kind = string(m, a, &row.reference.kind)?;
        let kind = encode(m, a, kind.value(), Kind::SourceDefinition)?;
        let k = key_value_scope(m, a, key)?;
        let k = encode(m, a, k.value(), Kind::SourceDefinition)?;
        super::records::hash_parts(
            m,
            "workflow-record",
            &[kind.bytes(), k.bytes(), encoded.bytes()],
            true,
        )?
    };
    if !shape::same_text(m, &digest, &row.reference.digest)? {
        return Err(shape::fail("selected evidence operand exact digest"));
    }
    if row.reference.id.starts_with("wdoc_")
        && !shape::same_text(m, &row.reference.id[5..], &row.reference.digest[7..])?
    {
        return Err(shape::fail("selected document identity"));
    }
    Ok(body)
}
fn key_value_scope(m: &mut CoreMeter, a: &AuditArena, k: &OperationKey) -> Result<AuditedValue> {
    let x = string(m, a, &k.scope[0])?;
    let y = string(m, a, &k.scope[1])?;
    a.array_from_values(&[x.value(), y.value()], m)
}

fn checked_request(
    program: &SelectedProgram,
    raw: &[u8],
    candidate: &EconomicCandidate,
    s: &mut OriginalAuditSession,
) -> Result<(AuditedValue, AuditedValue)> {
    s.check_cancelled()?;
    program.require_attempt_owner(s.meter_mut())?;
    if !program.uses_local_execution_recipe() || !candidate.result_metadata().is_local_execution() {
        return Err(shape::fail(
            "local request evidence requires actual local candidate",
        ));
    }
    let a = s.arena().shared_handle();
    let request = s.parse_representation(raw, Kind::OriginalRequest)?;
    let m = s.meter_mut();
    super::codec::validate_selected_types(m, request.value())?;
    shape::closed(
        m,
        request.value(),
        &["schema", "key", "kind", "contract", "payload"],
        &[],
    )?;
    for (field, want) in [("schema", "ledger-operation/1"), ("contract", "workflow/1")] {
        let got = shape::text(m, request.value(), field)?;
        if !shape::same_text(m, got, want)? {
            return Err(shape::fail("local request envelope"));
        }
    }
    let expected_key = key(m, &a, candidate.key())?;
    shape::equal(m, &request.value()["key"], expected_key.value())?;
    let payload = s.parse_representation(candidate.normalized_payload(), Kind::OriginalRequest)?;
    let operation = super::candidate::assembly::operation(s.meter_mut(), payload.value())?;
    let actual = shape::text(s.meter_mut(), request.value(), "kind")?;
    if !shape::same_text(s.meter_mut(), actual, operation)? {
        return Err(shape::fail("local request kind/payload tuple"));
    }
    let incoming =
        s.canonical_representation(&request.value()["payload"], Kind::OriginalRequest)?;
    let normalized = super::intent::normalize_local_saved_intent(
        program,
        candidate.normalized_payload(),
        incoming.bytes(),
        s,
    )?;
    if !shape::same_bytes(
        s.meter_mut(),
        normalized.bytes(),
        candidate.normalized_payload(),
    )? {
        return Err(shape::fail(
            "raw local request differs from actual candidate intent",
        ));
    }
    Ok((request, payload))
}

fn row_for<'a>(
    m: &mut CoreMeter,
    reference: &ExactRef,
    inputs: &'a [EconomicRecordView<'a>],
) -> Result<EconomicRecordView<'a>> {
    for row in inputs {
        m.visit("local.evidence_selected_operand")?;
        if shape::same_ref(m, reference, row.reference)? {
            return Ok(*row);
        }
    }
    Err(shape::fail(
        "actual candidate input missing for request evidence",
    ))
}
fn has_ref(m: &mut CoreMeter, anchors: &Anchors, reference: &Value) -> Result<bool> {
    for row in &anchors.anchors.rows {
        if shape::same(m, &row.value()["record_ref"], reference)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn descriptor_array(m: &mut CoreMeter, a: &AuditArena, values: &[&Value]) -> Result<AuditedValue> {
    let mut set = Set::new(m, a)?;
    for v in values {
        let v = a.clone_value(v, m)?;
        set.insert(m, a, v)?;
    }
    set.array(m, a)
}
fn add_direct(
    m: &mut CoreMeter,
    a: &AuditArena,
    anchors: &mut Anchors,
    value: &Value,
    key: &str,
    subject: &Value,
    compiler: &Value,
    empty: &Value,
    observation: bool,
) -> Result<()> {
    let (role, relation, ds) = match key {
        "product_ref" => ("product", "direct", empty),
        "policy_ref" => ("policy", "direct", compiler),
        "binding_ref" => ("binding", "original_binding", subject),
        "fact_schema_ref" | "subject_schema_ref" => {
            let one = descriptor_array(m, a, &[value])?;
            return anchors.add(m, a, "fact_schema", "direct", value, one.value());
        }
        "work_ref" | "original_work_ref" | "child" => ("work", "original_work", subject),
        "target_ref" => ("target", "original_target", subject),
        "predecessor" => ("target", "direct", subject),
        "original_claim_ref" => ("claim", "original_claim", subject),
        "original_cost_claim_ref" => ("app_cost_claim", "original_app_cost_claim", subject),
        "decision_ref" => ("dependent_decision", "dependent_decision", subject),
        "observation_ref" => ("app_observation", "original_app_observation", subject),
        "attestation_ref" if observation => {
            ("app_observation", "original_app_observation", subject)
        }
        "attestation_ref"
        | "verification_ref"
        | "prior_final_evidence_ref"
        | "evidence_ref"
        | "family_state_ref" => ("evidence", "evidence_input", empty),
        _ => return Err(shape::fail("unmapped direct descriptor field")),
    };
    anchors.add(m, a, role, relation, value, ds)
}
fn direct_anchors(
    m: &mut CoreMeter,
    a: &AuditArena,
    anchors: &mut Anchors,
    value: &Value,
    subject: &Value,
    compiler: &Value,
    empty: &Value,
    observation: bool,
) -> Result<()> {
    let fields = value
        .as_object()
        .ok_or_else(|| shape::fail("typed descriptor payload object"))?;
    for (name, v) in fields {
        m.visit("local.evidence_direct_field")?;
        match name.as_str() {
            "product_ref"
            | "policy_ref"
            | "binding_ref"
            | "fact_schema_ref"
            | "subject_schema_ref"
            | "work_ref"
            | "original_work_ref"
            | "target_ref"
            | "original_claim_ref"
            | "original_cost_claim_ref"
            | "decision_ref"
            | "observation_ref"
            | "attestation_ref"
            | "verification_ref"
            | "prior_final_evidence_ref"
            | "evidence_ref"
            | "family_state_ref"
            | "child"
            | "predecessor" => add_direct(
                m,
                a,
                anchors,
                v,
                name,
                subject,
                compiler,
                empty,
                observation,
            )?,
            "evidence_refs" | "supporting_evidence_refs" | "authority_evidence_refs" => {
                let refs = v
                    .as_array()
                    .ok_or_else(|| shape::fail("direct evidence array"))?;
                for r in refs {
                    m.visit("local.evidence_direct_proof")?;
                    anchors.add(m, a, "evidence", "evidence_input", r, empty)?;
                }
            }
            "attestation"
            | "subject"
            | "replacement"
            | "link"
            | "observation_proof"
            | "final_cost_proof"
            | "replacement_final_cost_proof"
            | "close_proof" => direct_anchors(
                m,
                a,
                anchors,
                v,
                subject,
                compiler,
                empty,
                name == "observation_proof",
            )?,
            "dependent_reversals" | "family_closures" | "fact_proofs" => {
                let rows = v
                    .as_array()
                    .ok_or_else(|| shape::fail("direct descriptor proof rows"))?;
                for row in rows {
                    m.visit("local.evidence_direct_proof_row")?;
                    direct_anchors(m, a, anchors, row, subject, compiler, empty, false)?;
                }
            }
            // Closed payload/body validation already establishes these opaque
            // business fields. Names within facts/claims are never Ref edges.
            _ => {}
        }
    }
    Ok(())
}
fn needed_schema_refs(m: &mut CoreMeter, a: &AuditArena, set: &mut Set, v: &Value) -> Result<()> {
    if let Some(obj) = v.as_object() {
        for (field, value) in obj {
            m.visit("local.evidence_subject_schema_field")?;
            match field.as_str() {
                "fact_schema_ref" | "subject_schema_ref" => {
                    super::shape::reference(m, value)?;
                    let value = a.clone_value(value, m)?;
                    set.insert(m, a, value)?;
                }
                "attestation" | "subject" | "replacement" | "payload" | "original_payload" => {
                    needed_schema_refs(m, a, set, value)?
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Exact normal local producer, after the ONE actual financial child. The
/// candidate's consumed fullRefs select semantic operands; prefetched rows and
/// new results do not become descriptor anchors merely by being available.
pub fn produce_local_financial_request_evidence<'a>(
    program: &SelectedProgram,
    raw_request: &[u8],
    candidate: &EconomicCandidate,
    inputs: &'a [EconomicRecordView<'a>],
    state: &EconomicStateData<'_>,
    s: &mut OriginalAuditSession,
) -> Result<LocalFinancialRequestEvidence> {
    let result = (|| {
        let (request, payload) = checked_request(program, raw_request, candidate, s)?;
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        let empty = a.array_from_values(&[], m)?;
        let compiler_ref = owning_definition(m, &a, program.compiler_descriptor_ref())?;
        let context_ref = owning_definition(m, &a, program.context_schema_ref())?;
        let compiler = descriptor_array(m, &a, &[compiler_ref.value(), context_ref.value()])?;
        let mut needed = Set::new(m, &a)?;
        needed_schema_refs(m, &a, &mut needed, payload.value())?;
        for r in candidate.consumed_refs() {
            let row = row_for(m, r, inputs)?;
            let body = checked_body(m, &a, candidate.key(), row)?;
            if shape::same_text(m, &r.kind, "workflow-work")?
                || shape::same_text(m, &r.kind, "workflow-outcome-claim")?
            {
                needed_schema_refs(m, &a, &mut needed, body.value())?;
            }
        }
        // Only actual original subject schemas, plus this original accepted
        // compiler/context. Unrelated source schemas are not descriptors.
        needed.insert(m, &a, compiler_ref)?;
        needed.insert(m, &a, context_ref)?;
        let subject = needed.array(m, &a)?;
        let mut anchors = Anchors::new(m, &a)?;
        direct_anchors(
            m,
            &a,
            &mut anchors,
            payload.value(),
            subject.value(),
            compiler.value(),
            empty.value(),
            false,
        )?;
        for r in candidate.consumed_refs() {
            let row = row_for(m, r, inputs)?;
            let body = checked_body(m, &a, candidate.key(), row)?;
            let reference = owning_reference(m, &a, r)?;
            selected_anchor(
                m,
                &a,
                &mut anchors,
                r,
                reference.value(),
                body.value(),
                candidate,
                state,
                payload.value(),
                subject.value(),
                compiler.value(),
                empty.value(),
            )?;
        }
        let anchors = subject_anchor_subsets(
            m,
            &a,
            program,
            candidate,
            inputs,
            &anchors,
            subject.value(),
            compiler.value(),
        )?;
        let mut descriptors = Set::new(m, &a)?;
        for row in &anchors.anchors.rows {
            for r in shape::array(m, row.value(), "descriptor_refs")? {
                let r = a.clone_value(r, m)?;
                descriptors.insert(m, &a, r)?;
            }
        }
        let descriptors = descriptors.array(m, &a)?;
        frame(
            m,
            &a,
            raw_request,
            request.value(),
            payload.value(),
            &anchors,
            descriptors.value(),
        )
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

fn selected_anchor(
    m: &mut CoreMeter,
    a: &AuditArena,
    anchors: &mut Anchors,
    r: &ExactRef,
    reference: &Value,
    body: &Value,
    candidate: &EconomicCandidate,
    state: &EconomicStateData<'_>,
    payload: &Value,
    subject: &Value,
    compiler: &Value,
    empty: &Value,
) -> Result<()> {
    let schema = shape::text(m, body, "schema")?;
    m.visit("local.evidence_closed_schema_dispatch")?;
    let (role, relation, descriptor_mode, support) = match (r.kind.as_str(), schema) {
        ("workflow-source-grant", "ledger-workflow-source-grant/1")
        | ("workflow-source-state", "ledger-workflow-source-state/1")
        | (
            "workflow-authority",
            "ledger-workflow-local-authority-snapshot/1"
            | "ledger-workflow-agreement-authority/2"
            | "ledger-workflow-agreement-authority/5"
            | "ledger-workflow-party-management-authority/1"
            | "ledger-workflow-party-authorization/1",
        )
        | (
            "workflow-economic-authority",
            "ledger-workflow-economic-authority/8"
            | "ledger-workflow-app-causal-authority/2"
            | "ledger-workflow-app-pending-causal-authority/2",
        ) => ("authority", "direct", 0, false),
        ("workflow-source-enrollment", "ledger-workflow-source-enrollment/1") => {
            ("source_enrollment", "source_enrollment", 0, false)
        }
        ("workflow-work", "ledger-workflow-work-record/1") => ("work", "original_work", 1, false),
        ("workflow-binding", "ledger-workflow-binding/2") => {
            ("binding", "original_binding", 1, false)
        }
        ("workflow-target", "ledger-workflow-component-target/1") => {
            ("target", "original_target", 1, false)
        }
        ("workflow-work-claim", "ledger-workflow-work-claim/1")
        | ("workflow-outcome-claim", "ledger-workflow-outcome-claim/1") => {
            ("claim", "original_claim", 1, false)
        }
        ("workflow-cost", "ledger-workflow-app-cost-claim/1") => {
            ("app_cost_claim", "original_app_cost_claim", 1, false)
        }
        ("workflow-cost", "ledger-workflow-app-cost-revision/1") => {
            let mut current = false;
            if let Some(app) = state.app {
                for head in app.cost_heads {
                    m.visit("local.evidence_current_app_head")?;
                    if shape::same_ref(m, &head.reference, r)? {
                        current = true;
                    }
                }
            }
            if current {
                ("app_cost_revision", "current_app_cost_revision", 1, false)
            } else {
                ("evidence", "evidence_input", 0, true)
            }
        }
        ("workflow-fact-attestation", "ledger-workflow-app-observation/1") => {
            ("app_observation", "original_app_observation", 1, false)
        }
        ("workflow-slot-terms", "ledger-workflow-slot-binding-terms/7") => {
            ("app_terms", "original_app_terms", 1, false)
        }
        (
            "workflow-family-state",
            "ledger-workflow-family-state/1" | "ledger-workflow-family-state/2",
        ) => ("family_state", "original_family", 1, false),
        ("workflow-publication", "ledger-workflow-publication/1") => {
            ("publication", "publication_source", 2, false)
        }
        ("workflow-policy", "ledger-workflow-policy/4") => ("policy", "direct", 2, false),
        ("workflow-product", "ledger-workflow-product/1") => ("product", "direct", 0, false),
        ("workflow-fact-schema", "ledger-workflow-fact-schema/1") => {
            // Source inventory is not a semantic schema selection. Only the
            // actual subject/context schema union contributes self anchors.
            let mut needed = false;
            for item in subject
                .as_array()
                .ok_or_else(|| shape::fail("subject descriptor array"))?
            {
                if shape::same(m, item, reference)? {
                    needed = true;
                }
            }
            if !needed {
                return Ok(());
            }
            ("fact_schema", "direct", 3, false)
        }
        ("workflow-slot-catalog", "ledger-workflow-slot-catalog/1") => {
            ("slot_catalog", "agreement_input", 0, false)
        }
        ("workflow-slot-instantiation", "ledger-workflow-slot-instantiation/1") => {
            ("slot_instantiation", "agreement_input", 0, false)
        }
        ("workflow-customer", "ledger-workflow-customer-registration/1") => {
            ("customer", "customer_registration", 0, false)
        }
        (
            "workflow-assent",
            "ledger-workflow-assent/1"
            | "ledger-workflow-party-assent/1"
            | "ledger-workflow-party-assent/2",
        ) => ("assent", "agreement_input", 0, false),
        ("workflow-activation", "ledger-workflow-activation-transition/1") => {
            ("activation_target", "activation_target", 0, false)
        }
        (
            "workflow-decision",
            "ledger-workflow-decision-revision/1"
            | "ledger-workflow-decision-revision/2"
            | "ledger-workflow-decision-revision/3",
        ) => {
            let root_op = super::candidate::assembly::operation(m, payload)?;
            let mut dependent = false;
            if root_op == "quantity.correct" || root_op == "work.reverse" {
                for head in candidate.heads() {
                    if let Some(old) = head.expected() {
                        if shape::same_ref(m, old, r)? {
                            for new in candidate.introduced() {
                                if shape::same_ref(m, new.reference(), head.proposed())? {
                                    let v = m.parse_owned_representation(
                                        a,
                                        new.canonical_body(),
                                        Kind::Record,
                                    )?;
                                    let kind = shape::text(m, v.value(), "kind")?;
                                    dependent = !shape::same_text(m, kind, "work")?;
                                }
                            }
                        }
                    }
                }
            }
            if dependent {
                ("dependent_decision", "dependent_decision", 1, false)
            } else {
                ("evidence", "evidence_input", 0, true)
            }
        }
        ("workflow-action", "ledger-workflow-action/1")
        | ("workflow-evaluation", "ledger-workflow-evaluation-proof/1")
        | ("workflow-correction-capacity", "ledger-correction-capacity/2")
        | ("workflow-dependency", "ledger-workflow-dependency/2")
        | ("workflow-capacity-retirement", "ledger-workflow-capacity-retirement/1")
        | ("workflow-cost", "ledger-workflow-app-cost-consumption/1")
        | ("workflow-chain-state", "ledger-workflow-chain-state/1")
        | ("workflow-link", "ledger-workflow-link/1")
        | ("workflow-head-transition", "ledger-workflow-head-transition/1") => {
            ("evidence", "evidence_input", 0, true)
        }
        (
            "workflow-fact-attestation",
            "ledger-workflow-app-final-cost/1"
            | "ledger-workflow-fact-attestation/1"
            | "ledger-workflow-family-close-attestation/1",
        )
        | ("workflow-evidence-verification", "ledger-workflow-evidence-verification/1")
        | ("workflow-verified-fact", "ledger-workflow-verified-fact/1")
        | (
            "workflow-evidence",
            "ledger-workflow-retained-evidence/1"
            | "ledger-workflow-party-evidence/1"
            | "ledger-workflow-party-evidence/2",
        )
        | (
            "workflow-verification",
            "ledger-workflow-app-consent-verification/1"
            | "ledger-workflow-app-consent-verification/2",
        ) => ("evidence", "evidence_input", 0, true),
        ("workflow-accepted-agreement", "ledger-workflow-accepted-agreement/1")
        | ("workflow-acceptance-terms", "ledger-workflow-acceptance-terms/2")
        | ("workflow-agreement-intent", "ledger-workflow-agreement-intent/1")
        | ("workflow-roles", "ledger-workflow-role-assignment/1")
        | (
            "workflow-compiled-plan",
            "ledger-workflow-compiled-plan/6"
            | "ledger-workflow-compiled-plan/7"
            | "ledger-workflow-compiled-plan/8",
        )
        | ("workflow-build-manifest", "ledger-workflow-build-manifest/1")
        | ("workflow-slot-template", "ledger-workflow-slot-template/1") => {
            ("evidence", "evidence_input", 0, true)
        }
        (
            "workflow-compiler-descriptor",
            "ledger-workflow-compiler-descriptor/6"
            | "ledger-workflow-compiler-descriptor/7"
            | "ledger-workflow-compiler-descriptor/8",
        ) => ("evidence", "evidence_input", 3, true),
        _ => {
            return Err(shape::fail(
                "selected local semantic operand has no exact descriptor mapping",
            ));
        }
    };
    if support && has_ref(m, anchors, reference)? {
        return Ok(());
    }
    let one = if descriptor_mode == 3 {
        Some(descriptor_array(m, a, &[reference])?)
    } else {
        None
    };
    let descriptors = match descriptor_mode {
        1 => subject,
        2 => compiler,
        3 => one.as_ref().unwrap().value(),
        _ => empty,
    };
    // Specific direct roles keep their existing relation. A link predecessor,
    // for example, also acquires its distinct original-target semantic role
    // only when that exact pair does not contradict the direct recipe.
    let mut relation = relation;
    for row in &anchors.anchors.rows {
        let existing_role = shape::text(m, row.value(), "role")?;
        if shape::same_text(m, existing_role, role)?
            && shape::same(m, &row.value()["record_ref"], reference)?
        {
            let existing = shape::text(m, row.value(), "relation")?;
            if shape::same_text(m, existing, "direct")? {
                relation = "direct";
            } else if !shape::same_text(m, existing, relation)? {
                return Err(shape::fail(
                    "selected descriptor relation conflicts with direct recipe",
                ));
            }
            break;
        }
    }
    anchors.add(m, a, role, relation, reference, descriptors)
}

fn collect_subject_schema<'a>(
    m: &mut CoreMeter,
    a: &AuditArena,
    program: &SelectedProgram,
    key: &OperationKey,
    inputs: &'a [EconomicRecordView<'a>],
    reference: &Value,
    set: &mut Set,
    depth: usize,
) -> Result<()> {
    // A fixed typed subject chain: decision/revision -> immutable claim ->
    // target/work. This is not the normal request's generic Ref closure.
    if crate::validation_work::compare(m, depth, 16usize)?.is_gt() {
        return Err(shape::fail("descriptor subject association depth"));
    }
    let r = shape::reference(m, reference)?;
    let row = row_for(m, &r, inputs)?;
    let body = checked_body(m, a, key, row)?;
    let schema = shape::text(m, body.value(), "schema")?;
    m.visit("local.descriptor_original_subject_dispatch")?;
    let next = match schema {
        "ledger-workflow-work-record/1" => {
            let original = &body.value()["payload"];
            let policy = owning_definition(m, a, program.root_policy_ref())?;
            if !shape::same(m, &original["policy_ref"], policy.value())? {
                return Err(shape::fail(
                    "descriptor subject requires its own original program",
                ));
            }
            needed_schema_refs(m, a, set, original)?;
            None
        }
        "ledger-workflow-outcome-claim/1" => {
            needed_schema_refs(m, a, set, &body.value()["original_payload"])?;
            None
        }
        "ledger-workflow-work-claim/1" | "ledger-workflow-component-target/1" => {
            Some(&body.value()["work_ref"])
        }
        "ledger-workflow-app-cost-claim/1" | "ledger-workflow-app-observation/1" => {
            Some(&body.value()["subject"]["original_work_ref"])
        }
        "ledger-workflow-app-cost-revision/1" => Some(&body.value()["cost_claim_ref"]),
        "ledger-workflow-decision-revision/1"
        | "ledger-workflow-decision-revision/2"
        | "ledger-workflow-decision-revision/3" => Some(&body.value()["subject_ref"]),
        "ledger-workflow-family-state/1" | "ledger-workflow-family-state/2" => {
            Some(&body.value()["target_ref"])
        }
        _ => {
            return Err(shape::fail(
                "unmapped original descriptor subject association",
            ));
        }
    };
    if let Some(next) = next {
        let depth = crate::validation_work::usize_add(m, depth, 1)?;
        collect_subject_schema(m, a, program, key, inputs, next, set, depth)?;
    }
    Ok(())
}
fn subject_anchor_subsets<'a>(
    m: &mut CoreMeter,
    a: &AuditArena,
    program: &SelectedProgram,
    candidate: &EconomicCandidate,
    inputs: &'a [EconomicRecordView<'a>],
    original: &Anchors,
    binding_subjects: &Value,
    compiler: &Value,
) -> Result<Anchors> {
    let mut result = Anchors::new(m, a)?;
    for entry in &original.anchors.rows {
        m.visit("local.descriptor_subject_anchor")?;
        let row = entry.value();
        let role = shape::text(m, row, "role")?;
        let relation = shape::text(m, row, "relation")?;
        let reference = &row["record_ref"];
        let descriptors = if matches!(
            role,
            "work"
                | "target"
                | "claim"
                | "app_cost_claim"
                | "app_cost_revision"
                | "app_observation"
                | "dependent_decision"
                | "family_state"
        ) {
            let mut set = Set::new(m, a)?;
            for r in compiler
                .as_array()
                .ok_or_else(|| shape::fail("compiler descriptor array"))?
            {
                let r = a.clone_value(r, m)?;
                set.insert(m, a, r)?;
            }
            collect_subject_schema(
                m,
                a,
                program,
                candidate.key(),
                inputs,
                reference,
                &mut set,
                0,
            )?;
            set.array(m, a)?
        } else if matches!(role, "binding" | "app_terms") {
            a.clone_value(binding_subjects, m)?
        } else {
            a.clone_value(&row["descriptor_refs"], m)?
        };
        result.add(m, a, role, relation, reference, descriptors.value())?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    fn domain_hash(domain: &[u8], value: &Value) -> String {
        let mut h = Sha256::new();
        h.update(domain);
        h.update(
            canonical::CanonicalBytes::from_value(value)
                .unwrap()
                .as_slice(),
        );
        format!("sha256:{}", canonical::hex(&h.finalize()))
    }
    #[test]
    fn local_descriptor_and_request_use_frozen_normalization2_domains() {
        let a = AuditArena::new().unwrap();
        let baseline = a.usage().unwrap().live_bytes;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let request = json!({"schema":"ledger-operation/1","key":{"logical_store_id":"store","scope":["scope","test"],"source":"worker","operation_id":"operation"},"kind":"work.accept","contract":"workflow/1","payload":{"schema":"ledger-workflow-work/1","quantity":"00020"}});
        let raw = canonical::CanonicalBytes::from_value(&request).unwrap();
        let payload = json!({"schema":"ledger-workflow-work/1","quantity":"20"});
        let descriptors = json!([]);
        let anchors = Anchors::new(s.meter_mut(), &a).unwrap();
        let evidence = frame(
            s.meter_mut(),
            &a,
            raw.as_slice(),
            &request,
            &payload,
            &anchors,
            &descriptors,
        )
        .unwrap();
        let actual = evidence.value();
        let mut resolution = actual["descriptor_resolution"].clone();
        let closure = resolution
            .as_object_mut()
            .unwrap()
            .remove("closure_digest")
            .unwrap();
        assert_eq!(
            closure,
            domain_hash(b"bean-counter/descriptor-closure/1\0", &resolution)
        );
        assert_eq!(
            resolution["locator_digest"],
            domain_hash(
                b"bean-counter/descriptor-locators/1\0",
                &json!(["work.accept", "ledger-workflow-work/1", []])
            )
        );
        assert_eq!(
            actual["schema_bundle_digest"],
            domain_hash(b"bean-counter/schema-bundle/1\0", &descriptors)
        );
        assert_eq!(
            actual["request_hash"],
            domain_hash(
                b"bean-counter/operation-request/2\0",
                &json!([closure, actual["normalized_request"]])
            )
        );
        assert_eq!(actual["normalized_request"]["payload"], payload);
        assert_eq!(
            actual["original_request_bytes_sha256"],
            canonical::hex(&Sha256::digest(raw.as_slice()))
        );
        assert_eq!(
            evidence.bytes(),
            canonical::CanonicalBytes::from_value(actual)
                .unwrap()
                .as_slice()
        );
        drop(evidence);
        drop(anchors);
        assert_eq!(a.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn repeated_locator_unions_descriptors_and_resorts_exact_anchors() {
        let a = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&a).unwrap();
        let reference = json!({"kind":"workflow-binding","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let first = json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}","b".repeat(64)),"digest":format!("sha256:{}","b".repeat(64))});
        let second = json!({"kind":"workflow-fact-schema","id":format!("wdoc_{}","c".repeat(64)),"digest":format!("sha256:{}","c".repeat(64))});
        let mut anchors = Anchors::new(s.meter_mut(), &a).unwrap();
        anchors
            .add(
                s.meter_mut(),
                &a,
                "binding",
                "original_binding",
                &reference,
                &json!([first]),
            )
            .unwrap();
        anchors
            .add(
                s.meter_mut(),
                &a,
                "binding",
                "original_binding",
                &reference,
                &json!([second]),
            )
            .unwrap();
        let array = anchors.anchors.array(s.meter_mut(), &a).unwrap();
        assert_eq!(array.value().as_array().unwrap().len(), 1);
        assert_eq!(array.value()[0]["descriptor_refs"], json!([first, second]));
        assert_eq!(anchors.locators.rows.len(), 1);
        assert!(anchors
            .add(
                s.meter_mut(),
                &a,
                "binding",
                "direct",
                &reference,
                &json!([])
            )
            .is_err());
    }
}
