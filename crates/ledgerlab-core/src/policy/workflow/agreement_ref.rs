//! References to the reviewed administrative and agreement data families.
//! A checked reference is a locator, never proof of retained membership/auth.
use crate::{Error, Result, canonical};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: String,
    id: String,
    digest: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgreementRef(Reference);
impl AgreementRef {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse(raw)?;
        let r: Reference = canonical::dto(value)?;
        let digest = r
            .digest
            .strip_prefix("sha256:")
            .ok_or_else(|| fail("digest prefix"))?;
        if !hex(digest) {
            return Err(fail("digest"));
        }
        let (prefix, content_id) = match r.kind.as_str() {
            "workflow-product"
            | "workflow-fact-schema"
            | "workflow-slot-template"
            | "workflow-policy"
            | "workflow-compiler-descriptor"
            | "workflow-build-manifest"
            | "workflow-authority"
            | "workflow-evidence"
            | "workflow-customer"
            | "workflow-source-enrollment"
            | "workflow-source-grant"
            | "workflow-definition-fragment"
            | "workflow-binding"
            | "workflow-roles"
            | "workflow-slot-catalog"
            | "workflow-slot-instantiation"
            | "workflow-slot-terms"
            | "workflow-offer"
            | "workflow-acceptance-terms"
            | "workflow-assent"
            | "workflow-agreement-intent" => ("wdoc_", true),
            "workflow-compiled-plan" => ("wplan_", true),
            "workflow-publication" => ("wpub_", false),
            "workflow-source-state" => ("wsource_", false),
            "workflow-accepted-agreement" => ("waccepted_", false),
            "workflow-head-transition" => ("whead_", false),
            "workflow-activation" => ("wactivation_", false),
            "workflow-operation" => ("wop_", false),
            "workflow-manifest" => ("wmanifest_", true),
            "workflow-receipt" => ("wreceipt_", true),
            _ => return Err(fail("unsupported data family")),
        };
        let id = r.id.strip_prefix(prefix).ok_or_else(|| fail("id prefix"))?;
        if !hex(id) || (content_id && id != digest) {
            return Err(fail("id/digest shape"));
        }
        Ok(Self(r))
    }
    pub fn kind(&self) -> &str {
        &self.0.kind
    }
    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn digest(&self) -> &str {
        &self.0.digest
    }
}
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn fail(s: &str) -> Error {
    Error::new("AGREEMENT_REFERENCE", s)
}
