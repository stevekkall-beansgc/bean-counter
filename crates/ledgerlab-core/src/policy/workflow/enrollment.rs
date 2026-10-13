//! Normalized immutable enrollment assertions. Actual evidence, registration,
//! actor, installation mode and membership are verified by the host resolver.
use super::{facts::reject_null, AgreementRef, SourceCeiling};
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    scope: [String; 2],
    enrollment_id: String,
    customer_refs: Vec<Value>,
    ceiling: Value,
    evidence_ref: Value,
    enrolled_by: String,
    enrolled_at: Timestamp,
    authority_ref: Value,
    installation_mode: String,
}
#[derive(Clone, Debug)]
pub struct SourceEnrollment {
    scope: [String; 2],
    enrollment_id: String,
    customer_refs: Vec<AgreementRef>,
    ceiling: SourceCeiling,
    bytes: CanonicalBytes,
    reference: AgreementRef,
}
impl SourceEnrollment {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let input: Input = canonical::dto(value)?;
        if input.schema != "ledger-workflow-source-enrollment/1" {
            return Err(fail("schema"));
        }
        for s in [
            &input.scope[0],
            &input.scope[1],
            &input.enrollment_id,
            &input.enrolled_by,
        ] {
            crate::domain::text(s, 128)?;
        }
        if !["real_local", "isolated_synthetic"].contains(&input.installation_mode.as_str()) {
            return Err(fail("installation mode"));
        }
        if !(1..=64).contains(&input.customer_refs.len()) {
            return Err(fail("registered customer count"));
        }
        let mut customer_refs = Vec::new();
        for r in &input.customer_refs {
            let r = reference(r)?;
            if r.kind() != "workflow-customer" {
                return Err(fail("registered customer family"));
            }
            customer_refs.push(r);
        }
        canonical::sort_set(&mut customer_refs)?;
        let evidence_ref = reference(&input.evidence_ref)?;
        let authority_ref = reference(&input.authority_ref)?;
        if evidence_ref.kind() != "workflow-evidence"
            || authority_ref.kind() != "workflow-authority"
        {
            return Err(fail("enrollment evidence/root family"));
        }
        let ceiling = SourceCeiling::parse(CanonicalBytes::from_value(&input.ceiling)?.as_slice())?;
        // Credential-alias enrollment is not implemented by this initial local
        // profile. It may not silently manufacture a new economic namespace.
        if ceiling.source() != ceiling.canonical_economic_namespace() {
            return Err(fail("initial canonical source mapping"));
        }
        let body = json!({"schema":input.schema,"scope":input.scope,"enrollment_id":input.enrollment_id,"customer_refs":customer_refs,"ceiling":ceiling.canonical_value()?,"evidence_ref":evidence_ref,"enrolled_by":input.enrolled_by,"enrolled_at":input.enrolled_at,"authority_ref":authority_ref,"installation_mode":input.installation_mode});
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > canonical::CANDIDATE_LIMIT {
            return Err(fail("normalized document byte limit"));
        }
        let digest = super::document_digest("ledger-workflow-source-enrollment/1", &body)?;
        let reference = reference(
            &json!({"kind":"workflow-source-enrollment","id":format!("wdoc_{}",&digest[7..]),"digest":digest}),
        )?;
        Ok(Self {
            scope: input.scope,
            enrollment_id: input.enrollment_id,
            customer_refs,
            ceiling,
            bytes,
            reference,
        })
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> Result<Value> {
        canonical::parse(self.canonical_bytes())
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.reference
    }
    pub fn scope(&self) -> &[String; 2] {
        &self.scope
    }
    pub fn enrollment_id(&self) -> &str {
        &self.enrollment_id
    }
    pub fn customer_refs(&self) -> &[AgreementRef] {
        &self.customer_refs
    }
    pub fn ceiling(&self) -> &SourceCeiling {
        &self.ceiling
    }
}
fn reference(v: &Value) -> Result<AgreementRef> {
    AgreementRef::parse(CanonicalBytes::from_value(v)?.as_slice())
}
fn fail(s: &str) -> Error {
    Error::new("SOURCE_ENROLLMENT", s)
}
#[cfg(test)]
mod tests;
