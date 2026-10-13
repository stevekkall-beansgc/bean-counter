//! Pure retained source-state assertions. Neither parsing nor lineage checks
//! authenticate records, commit membership, the actor or current authority.
use super::{
    facts::{counter, reject_null},
    AgreementRef, SourceEnrollment, SourceGrant,
};
use crate::{
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationKey {
    logical_store_id: String,
    scope: [String; 2],
    source: String,
    operation_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    enrollment_ref: Value,
    revision: String,
    status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_grant_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prior_state_ref: Option<Value>,
    operation_key: OperationKey,
    actor: String,
    accepted_at: Timestamp,
}
#[derive(Clone, Debug)]
pub struct SourceState {
    enrollment_ref: AgreementRef,
    revision: u64,
    current_grant_ref: Option<AgreementRef>,
    prior_state_ref: Option<AgreementRef>,
    key: OperationKey,
    bytes: CanonicalBytes,
    reference: AgreementRef,
}
impl SourceState {
    /// `enrollment` is data supplied by the host resolver; this method does not
    /// authenticate it. The host also compares the operation key to its store.
    pub fn parse(raw: &[u8], enrollment: &SourceEnrollment) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let input: Input = canonical::dto(value)?;
        if input.schema != "ledger-workflow-source-state/1" {
            return Err(fail("schema"));
        }
        let revision = counter(&input.revision)?;
        let enrollment_ref = reference(&input.enrollment_ref)?;
        if &enrollment_ref != enrollment.reference()
            || &input.operation_key.scope != enrollment.scope()
        {
            return Err(fail("original enrollment/scope"));
        }
        for s in [
            &input.operation_key.logical_store_id,
            &input.operation_key.scope[0],
            &input.operation_key.scope[1],
            &input.operation_key.operation_id,
            &input.actor,
        ] {
            crate::domain::text(s, 128)?;
        }
        crate::domain::text(&input.operation_key.source, 256)?;
        let current_grant_ref = input
            .current_grant_ref
            .as_ref()
            .map(reference)
            .transpose()?;
        let prior_state_ref = input.prior_state_ref.as_ref().map(reference).transpose()?;
        if current_grant_ref
            .as_ref()
            .is_some_and(|r| r.kind() != "workflow-source-grant")
            || prior_state_ref
                .as_ref()
                .is_some_and(|r| r.kind() != "workflow-source-state")
        {
            return Err(fail("state reference family"));
        }
        if revision == 0
            || (revision == 1) != prior_state_ref.is_none()
            || match input.status.as_str() {
                "active" => current_grant_ref.is_none(),
                "revoked" => current_grant_ref.is_some() || revision == 1,
                _ => true,
            }
        {
            return Err(fail("closed active/revoked revision shape"));
        }
        let mut body = json!({"schema":input.schema,"enrollment_ref":enrollment_ref,"revision":revision.to_string(),"status":input.status,"operation_key":input.operation_key,"actor":input.actor,"accepted_at":input.accepted_at});
        if let Some(r) = &current_grant_ref {
            body["current_grant_ref"] =
                serde_json::to_value(r).map_err(|_| fail("grant reference"))?;
        }
        if let Some(r) = &prior_state_ref {
            body["prior_state_ref"] =
                serde_json::to_value(r).map_err(|_| fail("prior reference"))?;
        }
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > canonical::CANDIDATE_LIMIT {
            return Err(fail("normalized record byte limit"));
        }
        let id = hash(
            "workflow-source-state",
            &json!([
                input.operation_key.logical_store_id,
                input.operation_key.scope,
                enrollment.enrollment_id(),
                revision.to_string()
            ]),
        )?;
        let digest = hash(
            "workflow-record",
            &json!(["workflow-source-state", input.operation_key.scope, body]),
        )?;
        let reference = reference(
            &json!({"kind":"workflow-source-state","id":format!("wsource_{}",&id[7..]),"digest":digest}),
        )?;
        Ok(Self {
            enrollment_ref,
            revision,
            current_grant_ref,
            prior_state_ref,
            key: input.operation_key,
            bytes,
            reference,
        })
    }
    /// Inputs must come from the resolver's authenticated original enrollment
    /// and verified current history. Revoked enrollments cannot be reenabled.
    /// The previous state/grant pair must be the retained immediate predecessor.
    pub fn validate_lineage(
        &self,
        enrollment: &SourceEnrollment,
        current_grant: Option<&SourceGrant>,
        prior: Option<(&Self, &SourceGrant)>,
    ) -> Result<()> {
        if &self.enrollment_ref != enrollment.reference() || &self.key.scope != enrollment.scope() {
            return Err(fail("original enrollment/scope"));
        }
        let prior_grant = match prior {
            None => {
                if self.revision != 1 || self.prior_state_ref.is_some() {
                    return Err(fail("initial state"));
                }
                None
            }
            Some((state, grant)) => {
                if state.enrollment_ref != self.enrollment_ref
                    || state.key.logical_store_id != self.key.logical_store_id
                    || state.key.scope != self.key.scope
                    || state.revision.checked_add(1) != Some(self.revision)
                    || self.prior_state_ref.as_ref() != Some(state.reference())
                    || !state.matches_grant(grant)
                    || grant.original_ceiling_ref() != enrollment.reference()
                    || grant.grant_id() != enrollment.enrollment_id()
                    || !grant.ceiling().is_subset_of(enrollment.ceiling())
                {
                    return Err(fail("immediate active predecessor and exact grant"));
                }
                Some(grant)
            }
        };
        match current_grant {
            Some(grant) => {
                if !self.matches_grant(grant) {
                    return Err(fail("current exact grant"));
                }
                grant.validate_lineage(
                    enrollment.enrollment_id(),
                    enrollment.reference(),
                    enrollment.ceiling(),
                    prior_grant,
                )?;
            }
            None => {
                if self.current_grant_ref.is_some() || prior_grant.is_none() {
                    return Err(fail("revoked state requires active predecessor"));
                }
            }
        }
        Ok(())
    }
    fn matches_grant(&self, grant: &SourceGrant) -> bool {
        self.revision == grant.revision()
            && self.current_grant_ref.as_ref().is_some_and(|r| {
                r.id() == grant.document_id() && r.digest() == grant.content_digest()
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
    pub fn enrollment_ref(&self) -> &AgreementRef {
        &self.enrollment_ref
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn status(&self) -> &str {
        if self.current_grant_ref.is_some() {
            "active"
        } else {
            "revoked"
        }
    }
    pub fn current_grant_ref(&self) -> Option<&AgreementRef> {
        self.current_grant_ref.as_ref()
    }
    pub fn prior_state_ref(&self) -> Option<&AgreementRef> {
        self.prior_state_ref.as_ref()
    }
    pub fn logical_store_id(&self) -> &str {
        &self.key.logical_store_id
    }
    pub fn operation_key(&self) -> Result<Value> {
        serde_json::to_value(&self.key).map_err(|_| fail("operation key"))
    }
}
fn hash(domain: &str, value: &Value) -> Result<String> {
    let mut h = Sha256::new();
    h.update(format!("bean-counter/{domain}/1\0").as_bytes());
    h.update(CanonicalBytes::from_value(value)?.as_slice());
    Ok(format!("sha256:{}", canonical::hex(&h.finalize())))
}
fn reference(v: &Value) -> Result<AgreementRef> {
    AgreementRef::parse(CanonicalBytes::from_value(v)?.as_slice())
}
fn fail(s: &str) -> Error {
    Error::new("SOURCE_STATE", s)
}
#[cfg(test)]
mod tests;
