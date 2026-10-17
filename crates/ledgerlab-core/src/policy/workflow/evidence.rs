//! Checked evidence input data. These bytes and declarations are not proof of
//! customer assent, operator authority, enrollment or committed membership.
use super::facts::reject_null;
use crate::validation_work::{
    self as work, NoopValidationWork, ValidationVisit as Visit, ValidationWork,
};
mod owned;
use crate::{
    Error, Result,
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
};
pub use owned::OwnedEvidenceInput;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    customer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agreement_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    purpose: String,
    subject: Subject,
    media_type: String,
    content_base64: String,
    content_sha256: String,
    metadata: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    acceptor: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    source: String,
    principal_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assent {
    terms_digest: String,
    acceptor: String,
    accepted_at: Timestamp,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attestation {
    terms_digest: String,
    acceptor: String,
    assent_evidence_ref: Reference,
    statement: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Funding {
    resource_owner: String,
    payer: String,
    mode: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    binding_ref: Reference,
    target_transition_ref: Reference,
    reason: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: String,
    id: String,
    digest: String,
}
impl Reference {
    fn check_with_work(
        &self,
        kind: &str,
        prefix: &str,
        document: bool,
        w: &mut dyn ValidationWork,
    ) -> Result<()> {
        digest_with_work(&self.digest, w)?;
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(self.id.len())?;
        w.scan_bytes(prefix.len())?;
        let hex = self
            .id
            .strip_prefix(prefix)
            .ok_or_else(|| fail("reference prefix"))?;
        if !same_text(&self.kind, kind, w)?
            || !raw_digest_with_work(hex, w)?
            || (document && !same_text(hex, &self.digest[7..], w)?)
        {
            return Err(fail("reference shape"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct EvidenceInput {
    body: Input,
    bytes: CanonicalBytes,
    content: Vec<u8>,
}
impl EvidenceInput {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let parsed = canonical::parse_bounded(raw, 262144)?;
        reject_null(&parsed)?;
        let mut body: Input = canonical::dto(parsed)?;
        if body.schema != "ledger-workflow-evidence-retain/1" {
            return Err(fail("schema"));
        }
        id(&body.subject.customer, 128)?;
        if let Some(v) = &body.subject.agreement_id {
            id(v, 128)?;
        }
        if !["text/plain;charset=utf-8", "application/octet-stream"]
            .contains(&body.media_type.as_str())
        {
            return Err(fail("media type"));
        }
        let content = crate::adjudication::proofs::decode_base64(&body.content_base64, 4096)?;
        if content.is_empty() {
            return Err(fail("empty content"));
        }
        if body.media_type == "text/plain;charset=utf-8" {
            std::str::from_utf8(&content).map_err(|_| fail("text encoding"))?;
        }
        if !raw_digest(&body.content_sha256)
            || canonical::hex(&Sha256::digest(&content)) != body.content_sha256
        {
            return Err(fail("content digest"));
        }
        let metadata = match body.purpose.as_str() {
            "customer_registration" => Metadata::Registration(canonical::dto(body.metadata)?),
            "source_enrollment" => Metadata::Enrollment(canonical::dto(body.metadata)?),
            "assent" => Metadata::Assent(canonical::dto(body.metadata)?),
            "operator_attestation" => Metadata::Attestation(canonical::dto(body.metadata)?),
            "funding" => Metadata::Funding(canonical::dto(body.metadata)?),
            "admission_stop" => Metadata::Stop(canonical::dto(body.metadata)?),
            _ => return Err(fail("unsupported purpose")),
        };
        normalize_metadata(
            &metadata,
            body.subject.agreement_id.is_some(),
            &mut NoopValidationWork,
        )?;
        body.metadata = value(metadata)?;
        let bytes = CanonicalBytes::from_value(&body)?;
        Ok(Self {
            body,
            bytes,
            content,
        })
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> Result<Value> {
        canonical::parse(self.canonical_bytes())
    }
    pub fn purpose(&self) -> &str {
        &self.body.purpose
    }
    pub fn customer(&self) -> &str {
        &self.body.subject.customer
    }
    pub fn agreement_id(&self) -> Option<&str> {
        self.body.subject.agreement_id.as_deref()
    }
    pub fn metadata(&self) -> &Value {
        &self.body.metadata
    }
    pub fn content(&self) -> &[u8] {
        &self.content
    }
    pub fn media_type(&self) -> &str {
        &self.body.media_type
    }
    pub fn content_sha256(&self) -> &str {
        &self.body.content_sha256
    }
}
fn value(v: impl Serialize) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| fail("metadata"))
}
fn raw_digest(s: &str) -> bool {
    raw_digest_with_work(s, &mut NoopValidationWork).unwrap_or(false)
}
fn raw_digest_with_work(s: &str, w: &mut dyn ValidationWork) -> Result<bool> {
    if !work::compare(w, s.len(), 64usize)?.is_eq() {
        return Ok(false);
    }
    w.scan_bytes(s.len())?;
    for c in s.bytes() {
        w.visit(Visit::Constraint)?;
        if !c.is_ascii_digit() && !(b'a'..=b'f').contains(&c) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn digest_with_work(s: &str, w: &mut dyn ValidationWork) -> Result<()> {
    w.visit(Visit::MemberCompare)?;
    w.scan_bytes(s.len())?;
    let hex = s.strip_prefix("sha256:").ok_or_else(|| fail("digest"))?;
    if raw_digest_with_work(hex, w)? {
        Ok(())
    } else {
        Err(fail("digest"))
    }
}
fn id(s: &str, max: usize) -> Result<()> {
    id_with_work(s, max, &mut NoopValidationWork)
}
fn id_with_work(s: &str, max: usize, w: &mut dyn ValidationWork) -> Result<()> {
    w.visit(Visit::Constraint)?;
    if work::compare(w, s.len(), 0usize)?.is_eq() || work::compare(w, s.len(), max)?.is_gt() {
        return Err(fail("identity"));
    }
    w.scan_bytes(s.len())?;
    for c in s.chars() {
        w.visit(Visit::Constraint)?;
        if c.is_control() {
            return Err(fail("identity"));
        }
    }
    Ok(())
}
fn same_text(a: &str, b: &str, w: &mut dyn ValidationWork) -> Result<bool> {
    Ok(super::terms::paid::compare_text(a, b, w)?.is_eq())
}
#[derive(Serialize)]
#[serde(untagged)]
enum Metadata {
    Registration(Registration),
    Enrollment(Enrollment),
    Assent(Assent),
    Attestation(Attestation),
    Funding(Funding),
    Stop(Stop),
}
fn normalize_metadata(m: &Metadata, has_agreement: bool, w: &mut dyn ValidationWork) -> Result<()> {
    w.visit(Visit::Constraint)?;
    match m {
        Metadata::Registration(m) => id_with_work(&m.acceptor, 128, w)?,
        Metadata::Enrollment(m) => {
            id_with_work(&m.source, 256, w)?;
            id_with_work(&m.principal_id, 128, w)?;
        }
        Metadata::Assent(m) => {
            digest_with_work(&m.terms_digest, w)?;
            id_with_work(&m.acceptor, 128, w)?;
        }
        Metadata::Attestation(m) => {
            digest_with_work(&m.terms_digest, w)?;
            id_with_work(&m.acceptor, 128, w)?;
            m.assent_evidence_ref
                .check_with_work("workflow-evidence", "wdoc_", true, w)?;
            if !same_text(&m.statement, "operator_verified_exact_terms_acceptance", w)? {
                return Err(fail("statement"));
            }
        }
        Metadata::Funding(m) => {
            id_with_work(&m.resource_owner, 128, w)?;
            id_with_work(&m.payer, 128, w)?;
            if !same_text(&m.mode, "customer_local", w)? {
                return Err(fail("funding mode"));
            }
        }
        Metadata::Stop(m) => {
            if !has_agreement {
                return Err(fail("stop agreement subject"));
            }
            m.binding_ref
                .check_with_work("workflow-binding", "wdoc_", true, w)?;
            m.target_transition_ref.check_with_work(
                "workflow-activation",
                "wactivation_",
                false,
                w,
            )?;
            if work::compare(w, m.reason.len(), 0usize)?.is_eq()
                || work::compare(w, m.reason.len(), 1024usize)?.is_gt()
            {
                return Err(fail("reason"));
            }
        }
    }
    Ok(())
}
fn fail(s: &str) -> Error {
    Error::new("EVIDENCE_INPUT", s)
}
#[cfg(test)]
mod tests;
