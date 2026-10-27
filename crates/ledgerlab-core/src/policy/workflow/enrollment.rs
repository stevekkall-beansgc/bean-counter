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
/// Exact saved /3 request DATA. Producer membership and custody are supplied
/// independently by the host, never inferred from this closed decoding.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedEnrollmentInput {
    schema: String,
    action: String,
    expected_revision: String,
    enrollment_id: String,
    customer_refs: Vec<Value>,
    ceiling: Value,
    evidence_ref: Value,
    namespace_owner_enrollment_ref: Value,
}
#[derive(Clone, Debug)]
pub struct SourceEnrollment {
    scope: [String; 2],
    enrollment_id: String,
    customer_refs: Vec<AgreementRef>,
    ceiling: SourceCeiling,
    bytes: CanonicalBytes,
    reference: AgreementRef,
    namespace_owner: Option<AgreementRef>,
}
impl SourceEnrollment {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let input: Input = canonical::dto(value)?;
        Self::normalize(input, None)
    }
    /// Owning shared-mapping DATA reader for the exact authenticated original
    /// SourceEnroll3 route. This does not authenticate that request or owner:
    /// the host retains their original producer/member/root/evidence/custody,
    /// same-store association and permanent no-remap check at its held cutoff.
    /// It neither copies owner grants nor establishes any operation right.
    pub fn parse_shared_namespace(
        raw: &[u8],
        original_enroll3_payload: &Value,
        canonical_owner: &SourceEnrollment,
    ) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let input: Input = canonical::dto(value)?;
        if CanonicalBytes::from_value(original_enroll3_payload)?
            .as_slice()
            .len()
            > 262144
        {
            return Err(fail("shared enrollment request byte limit"));
        }
        reject_null(original_enroll3_payload)?;
        let shared: SharedEnrollmentInput = canonical::dto(original_enroll3_payload.clone())?;
        Self::normalize(input, Some((&shared, canonical_owner)))
    }
    fn normalize(input: Input, shared: Option<(&SharedEnrollmentInput, &Self)>) -> Result<Self> {
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
        let namespace_owner = if let Some((request, owner)) = shared {
            if request.schema != "ledger-workflow-source-enroll/3"
                || request.action != "source.enroll"
                || request.expected_revision != "0"
            {
                return Err(fail("exact shared enrollment recipe"));
            }
            let owner_ref = reference(&request.namespace_owner_enrollment_ref)?;
            if owner_ref.kind() != "workflow-source-enrollment"
                || &owner_ref != owner.reference()
                || owner.namespace_owner.is_some()
                || owner.ceiling().source() != owner.ceiling().canonical_economic_namespace()
                || ceiling.canonical_economic_namespace() != owner.ceiling().source()
                || ceiling.principal_id() != owner.ceiling().principal_id()
                || input.enrolled_by != ceiling.principal_id()
                || input.scope != owner.scope
            {
                return Err(fail("shared canonical owner association"));
            }
            let owner_body = owner.canonical_value()?;
            if owner_body["installation_mode"] != input.installation_mode
                || owner_body["enrolled_by"] != input.enrolled_by
                || customer_refs != owner.customer_refs
            {
                return Err(fail("shared owner principal/customer/mode"));
            }
            let owner_at = owner_body["enrolled_at"]
                .as_str()
                .ok_or_else(|| fail("owner enrollment time"))?;
            let owner_at = Timestamp::parse(owner_at)?;
            if owner_at.micros() > input.enrolled_at.micros() {
                return Err(fail("shared owner enrolled after target"));
            }
            let mut requested_customers = Vec::new();
            for value in &request.customer_refs {
                let value = reference(value)?;
                if value.kind() != "workflow-customer" {
                    return Err(fail("shared request customer family"));
                }
                requested_customers.push(value);
            }
            canonical::sort_set(&mut requested_customers)?;
            let requested_ceiling =
                SourceCeiling::parse(CanonicalBytes::from_value(&request.ceiling)?.as_slice())?;
            let requested_evidence = reference(&request.evidence_ref)?;
            if request.enrollment_id != input.enrollment_id
                || requested_customers != customer_refs
                || requested_ceiling.canonical_bytes() != ceiling.canonical_bytes()
                || requested_evidence != evidence_ref
                || ceiling.canonical_value()?["customer_scope"]
                    != owner.ceiling().canonical_value()?["customer_scope"]
            {
                return Err(fail("shared original request/enrollment correspondence"));
            }
            Some(owner_ref)
        } else {
            // Legacy /1 and /2 readers keep their exact canonical-source guard.
            if ceiling.source() != ceiling.canonical_economic_namespace() {
                return Err(fail("initial canonical source mapping"));
            }
            None
        };
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
            namespace_owner,
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
    /// Exact original /3 owner association DATA, absent on legacy parsing.
    pub fn namespace_owner_enrollment_ref(&self) -> Option<&AgreementRef> {
        self.namespace_owner.as_ref()
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
