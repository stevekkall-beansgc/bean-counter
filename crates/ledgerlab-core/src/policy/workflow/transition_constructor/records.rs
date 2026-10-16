//! Exact in-memory operands. Origin membership and index completeness are host obligations.
use super::{
    CoreMeter, codec,
    data::{ExactRef, OperationKey, RecordInput},
};
use crate::{Error, Result, canonical};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn invalid(s: &str) -> Error {
    Error::new("INTEGRITY_FAILURE", s)
}
pub(super) fn hash(m: &mut CoreMeter, domain: &str, value: &Value) -> Result<String> {
    let prefix = format!("bean-counter/{domain}/1\0");
    m.visit("record.hash")?;
    m.hash_bytes(prefix.len())?;
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    if let Value::Array(parts) = value {
        // Frame punctuation does not narrow the body bound. Stream the array
        // instead of materializing body+frame behind a second 4MiB gate.
        m.visit("hash.array")?;
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"[");
        for (index, part) in parts.iter().enumerate() {
            m.visit("hash.part")?;
            if index > 0 {
                m.hash_bytes(1)?;
                m.write_bytes(1)?;
                digest.update(b",");
            }
            let bytes = codec::encode(m, part, 4194304)?;
            m.hash_bytes(bytes.len())?;
            digest.update(&bytes);
        }
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"]");
    } else {
        let bytes = codec::encode(m, value, 4194304)?;
        m.hash_bytes(bytes.len())?;
        digest.update(&bytes);
    }
    Ok(format!(
        "sha256:{}",
        canonical::hex_with_work(&digest.finalize(), m)?
    ))
}
// Parts must already be validated canonical JSON, never arbitrary wire bytes.
pub(super) fn hash_parts(
    m: &mut CoreMeter,
    domain: &str,
    parts: &[&[u8]],
    array: bool,
) -> Result<String> {
    let prefix = format!("bean-counter/{domain}/1\0");
    m.visit("record.hash_canonical")?;
    m.hash_bytes(prefix.len())?;
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    if array {
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"[");
    }
    for (i, part) in parts.iter().enumerate() {
        m.visit("hash.canonical_part")?;
        if i > 0 {
            m.hash_bytes(1)?;
            m.write_bytes(1)?;
            digest.update(b",");
        }
        m.hash_bytes(part.len())?;
        digest.update(part);
    }
    if array {
        m.hash_bytes(1)?;
        m.write_bytes(1)?;
        digest.update(b"]");
    }
    Ok(format!(
        "sha256:{}",
        canonical::hex_with_work(&digest.finalize(), m)?
    ))
}
pub(super) fn reference(m: &mut CoreMeter, r: &ExactRef) -> Result<()> {
    if !super::meter::cmp(m, &r.digest.len().into(), &71usize.into())?.is_eq()
        || !super::shape::same_text(m, r.digest.get(..7).unwrap_or(""), "sha256:")?
    {
        return Err(invalid("digest shape"));
    }
    for b in r.digest[7..].bytes() {
        m.scan_bytes(1)?;
        if !m.check("ref.hex", || {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        })? {
            return Err(invalid("digest hex"));
        }
    }
    if super::meter::cmp(m, &r.id.len().into(), &128usize.into())?.is_gt()
        || !super::meter::cmp(m, &r.id.len().into(), &65usize.into())?.is_gt()
    {
        return Err(invalid("reference identity"));
    }
    let prefix =
        r.id.rsplit_once('_')
            .ok_or_else(|| invalid("reference prefix"))?;
    if !super::meter::cmp(m, &prefix.1.len().into(), &64usize.into())?.is_eq() {
        return Err(invalid("reference suffix"));
    }
    for b in prefix.1.bytes() {
        m.scan_bytes(1)?;
        if !m.check("ref.id_hex", || {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        })? {
            return Err(invalid("reference suffix"));
        }
    }
    if !super::shape::same_text(m, r.kind.get(..9).unwrap_or(""), "workflow-")?
        || super::meter::cmp(m, &r.kind.len().into(), &64usize.into())?.is_gt()
    {
        return Err(invalid("reference kind"));
    }
    Ok(())
}
struct Operand<'inv> {
    reference: ExactRef,
    raw: Vec<u8>,
    body: Option<Value>,
    intrinsic: Option<&'inv super::intrinsic_record::IntrinsicRecord<'inv>>,
}
pub(super) struct Records<'inv> {
    operands: Vec<Operand<'inv>>,
    index: super::record_index::IdentityIndex,
    consumed: Vec<ExactRef>,
    intrinsic_owner: Option<&'inv super::intrinsic_record::IntrinsicDataScope>,
}
impl<'inv> Records<'inv> {
    pub(super) fn new(m: &mut CoreMeter, inputs: &[RecordInput]) -> Result<Self> {
        m.visit("records.count")?;
        if super::meter::cmp(m, &inputs.len().into(), &4096usize.into())?.is_gt() {
            return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "resolved count"));
        }
        for input in inputs {
            m.visit("records.metadata_bound")?;
            for (length, maximum) in [
                (input.reference.kind.len(), 64usize),
                (input.reference.id.len(), 128),
                (input.reference.digest.len(), 71),
            ] {
                if super::meter::cmp(m, &length.into(), &maximum.into())?.is_gt() {
                    return Err(invalid("supplied reference metadata bound"));
                }
            }
        }
        let references: Vec<_> = inputs.iter().map(|input| &input.reference).collect();
        let index = super::record_index::IdentityIndex::build(m, &references)?;
        let mut operands: Vec<Operand> = Vec::new();
        for input in inputs {
            // Supplied pool metadata is inspected now; body work and byte
            // admission occur only when this exact operand is consumed.
            operands.push(Operand {
                reference: input.reference.clone(),
                raw: input.canonical_body.clone(),
                body: None,
                intrinsic: None,
            });
        }
        Ok(Self {
            operands,
            index,
            consumed: vec![],
            intrinsic_owner: None,
        })
    }
    /// DATA-only immutable representation reuse. The host must independently
    /// authenticate the original and snapshot; this cannot authorize a commit.
    pub(super) fn with_intrinsic_evidence(
        mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        owner: &'inv super::intrinsic_record::IntrinsicDataScope,
        inputs: &[&'inv super::intrinsic_record::IntrinsicRecord<'inv>],
    ) -> Result<Self> {
        if !super::shape::same_text(m, owner.logical_store_id(), &key.logical_store_id)? {
            return Err(invalid("intrinsic DATA store association"));
        }
        for (actual, expected) in owner.scope().iter().zip(&key.scope) {
            if !super::shape::same_text(m, actual, expected)? {
                return Err(invalid("intrinsic DATA scope association"));
            }
        }
        if super::meter::cmp(m, &inputs.len().into(), &4096usize.into())?.is_gt() {
            return Err(invalid("intrinsic DATA count"));
        }
        for record in inputs {
            m.visit("intrinsic.pool_member")?;
            let reference = record.reference();
            record.consume(m, owner, reference)?;
            let position = self
                .index
                .find(m, &reference.kind, &reference.id, |i| {
                    &self.operands[i].reference
                })?
                .ok_or_else(|| invalid("intrinsic DATA absent from supplied operands"))?;
            let operand = &mut self.operands[position];
            if !super::shape::same_ref(m, &operand.reference, reference)?
                || !super::shape::same_bytes(m, &operand.raw, record.canonical_body())?
            {
                return Err(invalid("intrinsic DATA exact operand mismatch"));
            }
            m.visit("intrinsic.pool_duplicate")?;
            if operand.intrinsic.is_some() {
                return Err(invalid("duplicate intrinsic DATA declaration"));
            }
            operand.intrinsic = Some(record);
        }
        self.intrinsic_owner = Some(owner);
        Ok(self)
    }
    /// Match the exact immutable body, then perform paid owner/Ref association.
    /// Only the already-established content/schema properties are reusable.
    pub(super) fn evidence_facts(
        &self,
        m: &mut CoreMeter,
        body: &Value,
    ) -> Result<Option<&super::intrinsic_record::RetainedEvidenceFacts>> {
        let Some(owner) = self.intrinsic_owner else {
            return Ok(None);
        };
        for operand in &self.operands {
            m.visit("intrinsic.evidence_locator")?;
            let Some(record) = operand.intrinsic else {
                continue;
            };
            if super::shape::same(m, record.body(), body)? {
                return Ok(Some(record.consume(m, owner, &operand.reference)?));
            }
        }
        // Ordinary uncommitted/new operands still run their raw validator.
        Ok(None)
    }
    pub(super) fn optional(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        kind: &str,
        schema: &str,
    ) -> Result<Option<Value>> {
        reference(m, r)?;
        m.visit("resolve.optional_identity")?;
        match self
            .index
            .find(m, &r.kind, &r.id, |i| &self.operands[i].reference)?
        {
            None => Ok(None),
            Some(_) => self.get_grammatical(m, key, r, kind, schema).map(Some),
        }
    }
    pub(super) fn get(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        expected_schema: &str,
    ) -> Result<Value> {
        reference(m, r)?;
        self.get_grammatical(m, key, r, expected_kind, expected_schema)
    }
    pub(super) fn get_parsed(
        &mut self,
        m: &mut CoreMeter,
        owner: &super::representation::InvocationToken,
        key: &OperationKey,
        r: &super::representation::ParsedRef<'_>,
        kind: &str,
        schema: &str,
    ) -> Result<Value> {
        r.require_owner(m, owner)?;
        self.get_grammatical(m, key, r.exact(), kind, schema)
    }
    fn get_grammatical(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        expected_schema: &str,
    ) -> Result<Value> {
        self.get_grammatical_schemas(m, key, r, expected_kind, &[expected_schema])
    }
    pub(super) fn get_parsed_schemas(
        &mut self,
        m: &mut CoreMeter,
        owner: &super::representation::InvocationToken,
        key: &OperationKey,
        reference: &super::representation::ParsedRef<'_>,
        kind: &str,
        schemas: &[&str],
    ) -> Result<Value> {
        reference.require_owner(m, owner)?;
        self.get_grammatical_schemas(m, key, reference.exact(), kind, schemas)
    }
    /// Exact closed saved-schema dispatch. The resolved body chooses only among
    /// the caller's code-owned per-kind versions; kind/domain/hash checks are
    /// identical, with the ACTUAL selected schema in the document hash tuple.
    pub(super) fn get_schemas(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        schemas: &[&str],
    ) -> Result<Value> {
        reference(m, r)?;
        self.get_grammatical_schemas(m, key, r, expected_kind, schemas)
    }
    fn get_grammatical_schemas(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        r: &ExactRef,
        expected_kind: &str,
        schemas: &[&str],
    ) -> Result<Value> {
        m.visit("resolve.expected_kind")?;
        if !super::shape::same_text(m, &r.kind, expected_kind)? {
            return Err(invalid("wrong required kind"));
        }
        let mut found = None;
        let mut canonical_bytes = None;
        let mut intrinsic_reused = false;
        if let Some(position) = self
            .index
            .find(m, &r.kind, &r.id, |i| &self.operands[i].reference)?
        {
            let operand = &mut self.operands[position];
            m.visit("resolve.ref_lookup")?;
            if super::shape::same_ref(m, &operand.reference, r)? {
                if let Some(record) = operand.intrinsic {
                    let owner = self
                        .intrinsic_owner
                        .ok_or_else(|| invalid("intrinsic DATA owner missing"))?;
                    record.consume(m, owner, r)?;
                    m.resolved_body(&format!("{}:{}", r.kind, r.id), &operand.raw)?;
                    found = Some(record.body().clone());
                    intrinsic_reused = true;
                } else if operand.body.is_none() {
                    let maximum = match operand.reference.kind.as_str() {
                        "workflow-authority" | "workflow-economic-authority" => 65536,
                        "workflow-product"
                        | "workflow-policy"
                        | "workflow-fact-schema"
                        | "workflow-slot-template"
                        | "workflow-compiler-descriptor"
                        | "workflow-source-enrollment"
                        | "workflow-source-grant" => 262144,
                        _ => 4194304,
                    };
                    m.resolved_body(&format!("{}:{}", r.kind, r.id), &operand.raw)?;
                    let parsed = codec::parse(m, &operand.raw, maximum)?;
                    let canonical = codec::encode(m, &parsed, maximum)?;
                    if !super::shape::same_bytes(m, &canonical, &operand.raw)? {
                        return Err(invalid("noncanonical consumed body"));
                    }
                    operand.body = Some(parsed);
                }
                if !intrinsic_reused {
                    found = operand.body.clone();
                    canonical_bytes = Some(operand.raw.clone());
                }
            }
        }
        let body = found.ok_or_else(|| invalid("missing exact required record"))?;
        m.visit("resolve.expected_schema")?;
        let schema = super::shape::text(m, &body, "schema")?;
        let mut selected = false;
        for expected in schemas {
            if super::shape::same_text(m, schema, expected)? {
                selected = true;
                break;
            }
        }
        if !selected {
            return Err(invalid("wrong required schema"));
        }
        let prefix = match expected_kind {
            "workflow-operation" => "wop_",
            "workflow-work" => "wevent_",
            "workflow-work-claim" => "wclaim_",
            "workflow-outcome-claim" => "woclaim_",
            "workflow-decision" => "wdrev_",
            "workflow-action" => "waction_",
            "workflow-target" => "wtarget_",
            "workflow-family-state" => "wfamily_",
            "workflow-link" => "wlink_",
            "workflow-chain-state" => "wchain_",
            "workflow-correction-capacity" => "wcapacity_",
            "workflow-dependency" => "wdep_",
            "workflow-capacity-retirement" => "wretire_",
            "workflow-source-state" => "wsource_",
            "workflow-accepted-agreement" => "waccepted_",
            "workflow-head-transition" => "whead_",
            "workflow-activation" => "wactivation_",
            "workflow-publication" => "wpub_",
            "workflow-compiled-plan" => "wplan_",
            "workflow-product"
            | "workflow-policy"
            | "workflow-fact-schema"
            | "workflow-slot-template"
            | "workflow-compiler-descriptor"
            | "workflow-roles"
            | "workflow-slot-catalog"
            | "workflow-slot-instantiation"
            | "workflow-slot-terms"
            | "workflow-acceptance-terms"
            | "workflow-assent"
            | "workflow-binding"
            | "workflow-intent"
            | "workflow-agreement-intent"
            | "workflow-build-manifest"
            | "workflow-evidence"
            | "workflow-customer"
            | "workflow-source-enrollment"
            | "workflow-source-grant"
            | "workflow-authority"
            | "workflow-authority-prefix-observation"
            | "workflow-authority-audit-verifier"
            | "workflow-economic-authority"
            | "workflow-financial-history-observation"
            | "workflow-verified-fact"
            | "workflow-fact-attestation"
            | "workflow-evidence-verification"
            | "workflow-evaluation" => "wdoc_",
            _ => return Err(invalid("unknown required record kind")),
        };
        m.visit("resolve.id_family")?;
        let head =
            r.id.get(..prefix.len())
                .ok_or_else(|| invalid("record identity domain mismatch"))?;
        if !super::shape::same_text(m, head, prefix)? {
            return Err(invalid("record identity domain mismatch"));
        }
        let doc = prefix == "wdoc_";
        if !intrinsic_reused {
            let raw = canonical_bytes.ok_or_else(|| invalid("missing canonical operand"))?;
            let digest = if doc {
                let schema = codec::encode(m, &json!(schema), 262144)?;
                hash_parts(m, "workflow-document", &[&schema, &raw], true)?
            } else if prefix == "wplan_" {
                hash_parts(m, "workflow-plan", &[&raw], false)?
            } else {
                let kind = codec::encode(m, &json!(expected_kind), 262144)?;
                let scope = codec::encode(m, &json!(key.scope), 262144)?;
                hash_parts(m, "workflow-record", &[&kind, &scope, &raw], true)?
            };
            m.visit("resolve.digest_match")?;
            if !super::shape::same_text(m, &digest, &r.digest)?
                || (doc && !super::shape::same_text(m, &r.id[5..], &r.digest[7..])?)
            {
                return Err(invalid("record body hash mismatch"));
            }
        }
        let mut seen = false;
        for prior in &self.consumed {
            m.visit("resolve.consumed_compare")?;
            if super::shape::same_ref(m, prior, r)? {
                seen = true;
                break;
            }
        }
        if !seen {
            self.consumed.push(r.clone());
        }
        Ok(body)
    }
    // Identity is a locator, never a proof: the selected body still passes the
    // complete existing kind/schema/domain/canonical/digest checks below.
    pub(super) fn by_identity(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        kind: &str,
        id: &str,
        schema: &str,
    ) -> Result<(ExactRef, Value)> {
        let mut found = None;
        m.visit("resolve.identity_locator")?;
        if let Some(position) = self
            .index
            .find(m, kind, id, |i| &self.operands[i].reference)?
        {
            found = Some(self.operands[position].reference.clone());
        }
        let reference = found.ok_or_else(|| invalid("missing birth identity"))?;
        let body = self.get(m, key, &reference, kind, schema)?;
        Ok((reference, body))
    }
    pub(super) fn by_raw_sha(
        &mut self,
        m: &mut CoreMeter,
        key: &OperationKey,
        kind: &str,
        wanted: &str,
        schema: &str,
    ) -> Result<(ExactRef, Value)> {
        let mut found = None;
        for operand in &self.operands {
            m.visit("resolve.raw_digest_locator")?;
            if !super::shape::same_text(m, &operand.reference.kind, kind)? {
                continue;
            }
            if !super::meter::cmp(m, &operand.raw.len().into(), &4194304usize.into())?.is_le() {
                return Err(invalid("locator body bound"));
            }
            m.hash_bytes(operand.raw.len())?;
            let raw_digest = format!(
                "sha256:{}",
                canonical::hex_with_work(&Sha256::digest(&operand.raw), m)?
            );
            if super::shape::same_text(m, &raw_digest, wanted)? {
                if found.is_some() {
                    return Err(invalid("ambiguous raw digest locator"));
                }
                found = Some(operand.reference.clone());
            }
        }
        let reference = found.ok_or_else(|| invalid("missing raw digest locator"))?;
        let body = self.get(m, key, &reference, kind, schema)?;
        Ok((reference, body))
    }
    // Only a consumed, canonicality-checked body can expose its representation.
    pub(super) fn canonical(&self, m: &mut CoreMeter, r: &ExactRef) -> Result<&[u8]> {
        if let Some(position) = self
            .index
            .find(m, &r.kind, &r.id, |i| &self.operands[i].reference)?
        {
            let op = &self.operands[position];
            m.visit("resolve.canonical_lookup")?;
            if super::shape::same_ref(m, &op.reference, r)? {
                if !m.check("resolve.canonical_checked", || op.body.is_some())? {
                    return Err(invalid("unconsumed canonical representation"));
                }
                return Ok(&op.raw);
            }
        }
        Err(invalid("canonical operand missing"))
    }
    pub(super) fn consumed(self) -> Vec<ExactRef> {
        self.consumed
    }
}
