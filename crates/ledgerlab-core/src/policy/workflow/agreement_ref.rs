//! References to the reviewed administrative and agreement data families.
//! A checked reference is a locator, never proof of retained membership/auth.
use crate::validation_work::{
    self as vw, NoopValidationWork, ValidationVisit as Visit, ValidationWork,
};
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
        validate(&r.kind, &r.id, &r.digest, &mut NoopValidationWork)?;
        Ok(Self(r))
    }
    /// Borrowed closed fields for the new paid agreement path. This is locator
    /// DATA only; the caller must pre-admit and retain these three copied strings.
    pub fn from_parts_with_work(
        kind: &str,
        id: &str,
        digest: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        validate(kind, id, digest, work)?;
        let mut copy = |text: &str| -> Result<String> {
            work.scan_bytes(text.len())?;
            work.write_bytes(text.len())?;
            let mut out = String::new();
            out.try_reserve_exact(text.len())
                .map_err(|_| Error::new("RESOURCE_LIMIT", "reference copy"))?;
            out.push_str(text);
            Ok(out)
        };
        Ok(Self(Reference {
            kind: copy(kind)?,
            id: copy(id)?,
            digest: copy(digest)?,
        }))
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
fn validate(kind: &str, id: &str, digest: &str, work: &mut dyn ValidationWork) -> Result<()> {
    work.visit(Visit::Constraint)?;
    work.scan_bytes(7)?;
    let digest = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| fail("digest prefix"))?;
    if !hex_with_work(digest, work)? {
        return Err(fail("digest"));
    }
    const KINDS: &[(&str, &str, bool)] = &[
        ("workflow-product", "wdoc_", true),
        ("workflow-fact-schema", "wdoc_", true),
        ("workflow-slot-template", "wdoc_", true),
        ("workflow-policy", "wdoc_", true),
        ("workflow-compiler-descriptor", "wdoc_", true),
        ("workflow-build-manifest", "wdoc_", true),
        ("workflow-authority", "wdoc_", true),
        ("workflow-evidence", "wdoc_", true),
        ("workflow-customer", "wdoc_", true),
        ("workflow-source-enrollment", "wdoc_", true),
        ("workflow-source-grant", "wdoc_", true),
        ("workflow-definition-fragment", "wdoc_", true),
        ("workflow-binding", "wdoc_", true),
        ("workflow-roles", "wdoc_", true),
        ("workflow-slot-catalog", "wdoc_", true),
        ("workflow-slot-instantiation", "wdoc_", true),
        ("workflow-slot-terms", "wdoc_", true),
        ("workflow-offer", "wdoc_", true),
        ("workflow-acceptance-terms", "wdoc_", true),
        ("workflow-assent", "wdoc_", true),
        ("workflow-agreement-intent", "wdoc_", true),
        ("workflow-compiled-plan", "wplan_", true),
        ("workflow-publication", "wpub_", false),
        ("workflow-source-state", "wsource_", false),
        ("workflow-accepted-agreement", "waccepted_", false),
        ("workflow-head-transition", "whead_", false),
        ("workflow-activation", "wactivation_", false),
        ("workflow-operation", "wop_", false),
        ("workflow-manifest", "wmanifest_", true),
        ("workflow-receipt", "wreceipt_", true),
    ];
    let mut selected = None;
    for &(candidate, prefix, content_id) in KINDS {
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(kind.len())?;
        work.scan_bytes(candidate.len())?;
        if kind == candidate {
            selected = Some((prefix, content_id));
            break;
        }
    }
    let (prefix, content_id) = selected.ok_or_else(|| fail("unsupported data family"))?;

    work.scan_bytes(prefix.len())?;
    let id = id.strip_prefix(prefix).ok_or_else(|| fail("id prefix"))?;
    if !hex_with_work(id, work)? {
        return Err(fail("id/digest shape"));
    }
    if content_id {
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(id.len())?;
        work.scan_bytes(digest.len())?;
        if id != digest {
            return Err(fail("id/digest shape"));
        }
    }
    Ok(())
}
fn hex_with_work(s: &str, work: &mut dyn ValidationWork) -> Result<bool> {
    if !vw::compare(work, s.len(), 64usize)?.is_eq() {
        return Ok(false);
    }
    for byte in s.bytes() {
        work.scan_bytes(1)?;
        work.visit(Visit::Constraint)?;
        if !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn fail(s: &str) -> Error {
    Error::new("AGREEMENT_REFERENCE", s)
}
