//! Immutable normalized source-grant data and pure lineage checks. A host
//! resolver must authenticate original enrollment/current state and membership.
use super::{
    facts::{counter, reject_null},
    AgreementRef, SourceCeiling,
};
use crate::{
    canonical::{self, CanonicalBytes},
    Error, Result,
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct SourceGrant {
    grant_id: String,
    revision: u64,
    original_ceiling_ref: AgreementRef,
    ceiling: SourceCeiling,
    bytes: CanonicalBytes,
    digest: String,
    id: String,
}
impl SourceGrant {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let mut value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let map = value.as_object_mut().ok_or_else(|| fail("object"))?;
        let schema = take_string(map, "schema")?;
        if schema != "ledger-workflow-source-grant/1" {
            return Err(fail("schema"));
        }
        let grant_id = take_string(map, "grant_id")?;
        if grant_id.is_empty() || grant_id.len() > 128 || grant_id.chars().any(char::is_control) {
            return Err(fail("grant id"));
        }
        let revision = counter(&take_string(map, "revision")?)?;
        if revision == 0 {
            return Err(fail("positive revision"));
        }
        let reference = map
            .remove("original_ceiling_ref")
            .ok_or_else(|| fail("original ceiling ref"))?;
        let original_ceiling_ref =
            AgreementRef::parse(CanonicalBytes::from_value(&reference)?.as_slice())?;
        if original_ceiling_ref.kind() != "workflow-source-enrollment" {
            return Err(fail("enrollment family"));
        }
        let ceiling = SourceCeiling::parse(CanonicalBytes::from_value(&value)?.as_slice())?;
        let mut body = ceiling.canonical_value()?;
        body["schema"] = Value::String(schema);
        body["grant_id"] = Value::String(grant_id.clone());
        body["revision"] = Value::String(revision.to_string());
        body["original_ceiling_ref"] =
            serde_json::to_value(&original_ceiling_ref).map_err(|_| fail("reference"))?;
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > canonical::CANDIDATE_LIMIT {
            return Err(fail("normalized document byte limit"));
        }
        let digest = super::document_digest("ledger-workflow-source-grant/1", &body)?;
        let id = format!("wdoc_{}", &digest[7..]);
        Ok(Self {
            grant_id,
            revision,
            original_ceiling_ref,
            ceiling,
            bytes,
            digest,
            id,
        })
    }
    /// Supplied data must be the resolver's authenticated retained original
    /// enrollment and current grant. This function creates no trust itself.
    pub fn validate_lineage(
        &self,
        enrollment_id: &str,
        enrollment_ref: &AgreementRef,
        original: &SourceCeiling,
        prior: Option<&Self>,
    ) -> Result<()> {
        if enrollment_ref.kind() != "workflow-source-enrollment"
            || &self.original_ceiling_ref != enrollment_ref
            || self.grant_id != enrollment_id
        {
            return Err(fail("original enrollment identity"));
        }
        match prior {
            None => {
                if self.revision != 1
                    || self.ceiling.canonical_bytes() != original.canonical_bytes()
                {
                    return Err(fail("initial grant equals exact original ceiling"));
                }
            }
            Some(prior) => {
                if prior.original_ceiling_ref != self.original_ceiling_ref
                    || prior.grant_id != self.grant_id
                    || prior.revision.checked_add(1) != Some(self.revision)
                    || !self.ceiling.is_subset_of(original)
                    || !self.ceiling.is_subset_of(&prior.ceiling)
                {
                    return Err(fail("grant successor must narrow original and current"));
                }
            }
        }
        Ok(())
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> Result<Value> {
        canonical::parse(self.canonical_bytes())
    }
    pub fn document_id(&self) -> &str {
        &self.id
    }
    pub fn content_digest(&self) -> &str {
        &self.digest
    }
    pub fn grant_id(&self) -> &str {
        &self.grant_id
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn original_ceiling_ref(&self) -> &AgreementRef {
        &self.original_ceiling_ref
    }
    pub fn ceiling(&self) -> &SourceCeiling {
        &self.ceiling
    }
}
fn take_string(map: &mut serde_json::Map<String, Value>, key: &str) -> Result<String> {
    match map.remove(key) {
        Some(Value::String(v)) => Ok(v),
        _ => Err(fail("required string")),
    }
}
fn fail(s: &str) -> Error {
    Error::new("SOURCE_GRANT", s)
}
#[cfg(test)]
mod tests;
