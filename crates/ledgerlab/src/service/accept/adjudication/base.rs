//! The original validated plan and its exact proposed bytes enter the same R3
//! transaction. This constructor cannot accept a previously stored receipt.
use super::*;
use ledgerlab_core::adjudication::{self as r3, Validate};
use ledgerlab_core::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;
fn require(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error {
            code: "ORIGINAL_BASE_BINDING",
            detail: "exact fresh original plan".into(),
        })
    }
}
impl FreshBaseAcceptance {
    pub(crate) fn from_fresh_v2(
        plan: ValidatedOriginalBasePlan,
        terms: &wire::Enroll,
        exact_members: Vec<wire::RetainedObject>,
    ) -> Result<Self> {
        terms.validate()?;
        require(
            plan.resolution().target == terms.target.as_str()
                && plan.resolution().family_key.is_none(),
        )?;
        require(plan.observed_heads().iter().any(|h| {
            h.lock.class == crate::store::outcomes::OutcomeLockClass::Target
                && h.revision.is_none()
                && h.value.is_none()
        }))?;
        require(!exact_members.is_empty() && exact_members.len() <= 128)?;
        let mut bytes = 0usize;
        let mut rows = BTreeMap::new();
        let mut retail = 0i128;
        let mut supplier = 0i128;
        for object in &exact_members {
            object.validate()?;
            require(
                object.kind == wire::FactKind::OriginalBase
                    && object.origin.store == terms.store
                    && object.origin.host == terms.store
                    && object.origin.scope == terms.scope
                    && object.origin.registration == terms.registration,
            )?;
            let verified = r3::proofs::VerifiedObjectBytes::check(object.clone())?;
            bytes += verified.bytes().len();
            require(bytes <= 1_048_576 && plan.records().iter().any(|r| r == verified.bytes()))?;
            let row: Value =
                ledgerlab_core::canonical::parse_bounded(verified.bytes(), r3::COMMAND_BYTES)?;
            let kind = row["kind"].as_str().ok_or_else(|| Error {
                code: "ORIGINAL_BASE_KIND",
                detail: "envelope".into(),
            })?;
            let id = row["id"].as_str().ok_or_else(|| Error {
                code: "ORIGINAL_BASE_ID",
                detail: "envelope".into(),
            })?;
            require(
                rows.insert(
                    (kind.to_owned(), id.to_owned()),
                    (row.clone(), object.body_hash.clone()),
                )
                .is_none(),
            )?;
            if kind == "base-posting" {
                let n = row["body"]["amount"]["atoms"]
                    .as_str()
                    .and_then(|s| s.parse::<i128>().ok())
                    .ok_or_else(|| Error {
                        code: "ORIGINAL_BASE_AMOUNT",
                        detail: "atoms".into(),
                    })?;
                let total = match row["body"]["book"].as_str() {
                    Some("retail") => &mut retail,
                    Some("supplier") => &mut supplier,
                    _ => {
                        return Err(Error {
                            code: "ORIGINAL_BASE_BOOK",
                            detail: "book".into(),
                        })
                    }
                };
                *total = total.checked_add(n).ok_or_else(|| Error {
                    code: "ORIGINAL_BASE_AMOUNT",
                    detail: "overflow".into(),
                })?;
            }
        }
        require(
            retail == terms.base_atoms.value() as i128
                && supplier == terms.supplier_booked.value() as i128,
        )?;
        let acceptances: Vec<_> = rows
            .iter()
            .filter(|((kind, _), _)| kind == "base-acceptance")
            .collect();
        require(acceptances.len() == 1)?;
        let (_, (receipt, hash)) = acceptances[0];
        require(*hash == terms.base_receipt && receipt["body"]["target"] == terms.target.as_str())?;
        let members = receipt["body"]["members"].as_array().ok_or_else(|| Error {
            code: "ORIGINAL_BASE_MEMBERS",
            detail: "closure".into(),
        })?;
        require(members.len() + 1 == rows.len())?;
        for member in members {
            let key = (
                member["kind"].as_str().unwrap_or("").to_owned(),
                member["id"].as_str().unwrap_or("").to_owned(),
            );
            let (row, _) = rows.get(&key).ok_or_else(|| Error {
                code: "ORIGINAL_BASE_MEMBER",
                detail: "missing".into(),
            })?;
            require(row["content_hash"] == member["content_hash"])?;
        }
        let snapshot = &receipt["body"]["target_snapshot"];
        let (_, hash) = rows
            .get(&(
                "target-snapshot".into(),
                snapshot["id"].as_str().unwrap_or("").into(),
            ))
            .ok_or_else(|| Error {
                code: "ORIGINAL_BASE_MANIFEST",
                detail: "missing".into(),
            })?;
        require(*hash == terms.base_manifest)?;
        let saved = plan.receipt();
        require(r3::raw_sha256(saved) == terms.base_receipt)?;
        Ok(Self {
            writes: OriginalBaseWrites::OriginalV2(Box::new(plan)),
            absence: vec![],
            manifest: terms.base_manifest.clone(),
            receipt: terms.base_receipt.clone(),
            exact_members,
        })
    }
}
