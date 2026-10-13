//! Checked evidence input data. These bytes and declarations are not proof of
//! customer assent, operator authority, enrollment or committed membership.
use super::facts::reject_null;
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    Error, Result,
};
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
    fn check(&self, kind: &str, prefix: &str, document: bool) -> Result<()> {
        digest(&self.digest)?;
        let hex = self
            .id
            .strip_prefix(prefix)
            .ok_or_else(|| fail("reference prefix"))?;
        if self.kind != kind || !raw_digest(hex) || (document && hex != &self.digest[7..]) {
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
        body.metadata = match body.purpose.as_str() {
            "customer_registration" => {
                let m: Registration = canonical::dto(body.metadata)?;
                id(&m.acceptor, 128)?;
                value(m)?
            }
            "source_enrollment" => {
                let m: Enrollment = canonical::dto(body.metadata)?;
                id(&m.source, 256)?;
                id(&m.principal_id, 128)?;
                value(m)?
            }
            "assent" => {
                let m: Assent = canonical::dto(body.metadata)?;
                digest(&m.terms_digest)?;
                id(&m.acceptor, 128)?;
                value(m)?
            }
            "operator_attestation" => {
                let m: Attestation = canonical::dto(body.metadata)?;
                digest(&m.terms_digest)?;
                id(&m.acceptor, 128)?;
                m.assent_evidence_ref
                    .check("workflow-evidence", "wdoc_", true)?;
                if m.statement != "operator_verified_exact_terms_acceptance" {
                    return Err(fail("statement"));
                }
                value(m)?
            }
            "funding" => {
                let m: Funding = canonical::dto(body.metadata)?;
                id(&m.resource_owner, 128)?;
                id(&m.payer, 128)?;
                if m.mode != "customer_local" {
                    return Err(fail("funding mode"));
                }
                value(m)?
            }
            "admission_stop" => {
                let m: Stop = canonical::dto(body.metadata)?;
                if body.subject.agreement_id.is_none() {
                    return Err(fail("stop agreement subject"));
                }
                m.binding_ref.check("workflow-binding", "wdoc_", true)?;
                m.target_transition_ref
                    .check("workflow-activation", "wactivation_", false)?;
                if m.reason.is_empty() || m.reason.len() > 1024 {
                    return Err(fail("reason"));
                }
                value(m)?
            }
            _ => return Err(fail("unsupported purpose")),
        };
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
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn digest(s: &str) -> Result<()> {
    if s.strip_prefix("sha256:").is_some_and(raw_digest) {
        Ok(())
    } else {
        Err(fail("digest"))
    }
}
fn id(s: &str, max: usize) -> Result<()> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(fail("identity"))
    } else {
        Ok(())
    }
}
fn fail(s: &str) -> Error {
    Error::new("EVIDENCE_INPUT", s)
}
#[cfg(test)]
mod tests;
