//! Exact saved wire relationships for chronological recipe dispatch.
//! A valid frame remains unresolved DATA until full prefix, semantic replay,
//! authorization, dependency/capacity/funding and resource obligations succeed.
use super::request_wrapper::{self as request, SavedRequest};
use super::*;
use crate::store::sqlite::workflow::financial::OriginalFrame;
use ledgerlab_core::{
    canonical::audit_arena::AuditedValue,
    validation_work::{
        ArithmeticOperation as Op, BigInt, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use sha2::{Digest, Sha256};

/// Mechanical grouping only. Every class still needs its original semantic
/// recipe, descriptor and introduction completion; this is not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WireClass {
    Publication,
    Prerequisite,
    Agreement,
    AgreementAlias,
    Financial,
}
pub(super) struct OriginalWrappers {
    class: WireClass,
    // Parsed trees drop before the retained raw storage. Each owns its own
    // reservation in the SAME attempt arena; no borrowed Value escapes it.
    operation: AuditedValue,
    manifest: AuditedValue,
    receipt: AuditedValue,
    request: SavedRequest,
    frame: OriginalFrame,
}
impl OriginalWrappers {
    pub(super) fn class(&self) -> WireClass {
        self.class
    }
    pub(super) fn operation(&self) -> &Value {
        self.operation.value()
    }
    pub(super) fn manifest(&self) -> &Value {
        self.manifest.value()
    }
    pub(super) fn receipt(&self) -> &Value {
        self.receipt.value()
    }
    pub(super) fn original_request(&self) -> &SavedRequest {
        &self.request
    }
    pub(super) fn sequence(&self) -> i64 {
        self.frame.sequence
    }
    pub(super) fn operation_identity(&self) -> (&[u8], &[u8]) {
        (
            self.frame.operation_id.bytes(),
            self.frame.operation_digest.bytes(),
        )
    }
    /// Exact pointer representation from this acquired/checked wrapper frame.
    /// Calling this does not complete its origin; the chronological caller only
    /// stages it after the actual recipe's original semantics have finished.
    pub(super) fn origin_pointer(
        &self,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        fn reference(
            kind: &str,
            id: &[u8],
            digest: &[u8],
            s: &mut OriginalAuditSession,
        ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
            s.semantic_work().scan_bytes(id.len()).map_err(core_error)?;
            let id = std::str::from_utf8(id).map_err(|_| ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .scan_bytes(digest.len())
                .map_err(core_error)?;
            let digest = std::str::from_utf8(digest).map_err(|_| ServiceError::IntegrityFailure)?;
            let kind = super::wire::string(kind, s)?;
            let id = super::wire::string(id, s)?;
            let digest = super::wire::string(digest, s)?;
            super::wire::object(
                &[
                    ("digest", digest.bytes()),
                    ("id", id.bytes()),
                    ("kind", kind.bytes()),
                ],
                4194304,
                s,
            )
        }
        let operation = reference(
            "workflow-operation",
            self.frame.operation_id.bytes(),
            self.frame.operation_digest.bytes(),
            s,
        )?;
        let manifest = reference(
            "workflow-manifest",
            self.frame.manifest_id.bytes(),
            self.frame.manifest_digest.bytes(),
            s,
        )?;
        let receipt = reference(
            "workflow-receipt",
            self.frame.receipt_id.bytes(),
            self.frame.receipt_digest.bytes(),
            s,
        )?;
        let key = super::request_wrapper::field(self.operation(), "key", s)?;
        let key = s
            .canonical_representation(key, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&self.frame.sequence.into(), &0i64.into()])
            .map_err(core_error)?;
        require(self.frame.sequence > 0, "INTEGRITY_FAILURE")?;
        let sequence = super::wire::counter_string(self.frame.sequence as u64, s)?;
        super::wire::object(
            &[
                ("commit_sequence", sequence.bytes()),
                ("manifest_ref", manifest.bytes()),
                ("operation_key", key.bytes()),
                ("operation_ref", operation.bytes()),
                ("receipt_ref", receipt.bytes()),
            ],
            4194304,
            s,
        )
    }
    pub(super) fn retained_lengths(&self) -> [usize; 3] {
        [
            self.frame.operation.bytes().len(),
            self.frame.manifest.bytes().len(),
            self.frame.receipt.bytes().len(),
        ]
    }
}
fn resource(e: crate::store::sqlite::workflow::financial::FinancialReadError) -> ServiceError {
    match e {
        crate::store::sqlite::workflow::financial::FinancialReadError::Store(e) => {
            crate::service::store_error(e)
        }
        crate::store::sqlite::workflow::financial::FinancialReadError::Resource(e) => core_error(e),
    }
}
#[derive(Clone, Copy)]
pub(super) struct Recipe {
    pub(super) operation: &'static str,
    pub(super) manifest: &'static str,
    pub(super) closure: &'static str,
    pub(super) authority: &'static str,
}
fn equal_text(left: &str, right: &str, session: &mut OriginalAuditSession) -> Result<bool> {
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(left.len()).map_err(core_error)?;
    work.scan_bytes(right.len()).map_err(core_error)?;
    Ok(left == right)
}
const DEADLINE_REGISTRY: &str =
    "sha256:8ebed26695ae3594d40f91d733839bddcc31b94dcb688b2aa5890af7775af0da";
const SUPPLIER_REGISTRY: &str =
    "sha256:efd33a606f95b0b8614fe657d13a617620c8c33329f98207cbfc6e70376f9c93";
const WORK_TARGET_REGISTRY: &str =
    "sha256:d3fd78be2a734b4096c60f03869ff4da366a8fbf5f505b61d1188bbf420cd2d2";
const CONSENT_REGISTRY: &str =
    "sha256:b6b4aa2542bb571fe45142e5f1fbc7369aef95ee2a7c181c236496466571245f";
const PARTY_COMMITMENT_REGISTRY: &str =
    "sha256:762b708044ffad44717074a9b3456b91ebbe62d7a12d76cfa80d6a37ecbfe276";
fn party_commitment_recipe(
    kind: &str,
    payload: &str,
    session: &mut OriginalAuditSession,
) -> Result<Option<Recipe>> {
    for (k, p, operation, manifest, closure, authority) in [
        (
            "agreement.accept",
            "ledger-workflow-agreement-accept/9",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation-replace/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.revoke",
            "ledger-workflow-agreement-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/2",
        ),
        (
            "authority.change",
            "ledger-workflow-party-authorize/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "authority.change",
            "ledger-workflow-party-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "cost.correct",
            "ledger-workflow-cost-correct/2",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "cost.finalize",
            "ledger-workflow-cost-finalize/2",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-cost-attest/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-observe/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-fact-attest/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-family-close-attest/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-party-evidence-retain/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "evidence.retain",
            "ledger-workflow-party-evidence-retain/2",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "link.assert",
            "ledger-workflow-link-assert/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.close",
            "ledger-workflow-outcome-close/2",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.correct",
            "ledger-workflow-outcome-correction/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "policy.publish",
            "ledger-workflow-policy-publication/6",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-publication-manifest/1",
            "absent",
            "ledger-workflow-publication-authority/1",
        ),
        (
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "work.accept",
            "ledger-workflow-work/1",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "work.reverse",
            "ledger-workflow-work-reverse/2",
            "ledger-workflow-economic-operation/9",
            "ledger-workflow-economic-manifest/9",
            "ledger-economic-resolved-inputs/9",
            "ledger-workflow-economic-authority/7",
        ),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation,
                manifest,
                closure,
                authority,
            }));
        }
    }
    Ok(None)
}
const APP_CAUSAL_REGISTRY: &str =
    "sha256:42368b84a652527c58389014fefa69140d6a40c2217e193694f162441699cdc2";
fn app_causal_recipe(
    kind: &str,
    payload: &str,
    session: &mut OriginalAuditSession,
) -> Result<Option<Recipe>> {
    for (k, p, operation, manifest, closure, authority) in [
        (
            "agreement.accept",
            "ledger-workflow-agreement-accept/8",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation-replace/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.revoke",
            "ledger-workflow-agreement-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/2",
        ),
        (
            "authority.change",
            "ledger-workflow-party-authorize/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "authority.change",
            "ledger-workflow-party-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "cost.correct",
            "ledger-workflow-cost-correct/2",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "cost.finalize",
            "ledger-workflow-cost-finalize/2",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-cost-attest/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-observe/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-fact-attest/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-family-close-attest/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "evidence.retain",
            "ledger-workflow-party-evidence-retain/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "link.assert",
            "ledger-workflow-link-assert/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.close",
            "ledger-workflow-outcome-close/2",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.correct",
            "ledger-workflow-outcome-correction/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "policy.publish",
            "ledger-workflow-policy-publication/6",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-publication-manifest/1",
            "absent",
            "ledger-workflow-publication-authority/1",
        ),
        (
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "work.accept",
            "ledger-workflow-work/1",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
        (
            "work.reverse",
            "ledger-workflow-work-reverse/2",
            "ledger-workflow-economic-operation/8",
            "ledger-workflow-economic-manifest/8",
            "ledger-economic-resolved-inputs/8",
            "ledger-workflow-economic-authority/7",
        ),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation,
                manifest,
                closure,
                authority,
            }));
        }
    }
    Ok(None)
}
const APP_REGISTRY: &str =
    "sha256:9a6379b0f0d20005f1406ca6ed3a5b55c40d275cec5435e1648d9b482332509b";
fn app_recipe(
    kind: &str,
    payload: &str,
    session: &mut OriginalAuditSession,
) -> Result<Option<Recipe>> {
    for (k, p, operation, manifest, closure, authority) in [
        (
            "agreement.accept",
            "ledger-workflow-agreement-accept/7",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation-replace/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/5",
        ),
        (
            "agreement.revoke",
            "ledger-workflow-agreement-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-agreement-authority/2",
        ),
        (
            "authority.change",
            "ledger-workflow-party-authorize/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "authority.change",
            "ledger-workflow-party-revoke/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "cost.correct",
            "ledger-workflow-cost-correct/2",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "cost.finalize",
            "ledger-workflow-cost-finalize/2",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-cost-attest/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "evidence.retain",
            "ledger-workflow-app-observe/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "evidence.retain",
            "ledger-workflow-fact-attest/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "evidence.retain",
            "ledger-workflow-family-close-attest/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "evidence.retain",
            "ledger-workflow-party-evidence-retain/1",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-agreement-manifest/1",
            "absent",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "link.assert",
            "ledger-workflow-link-assert/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "outcome.close",
            "ledger-workflow-outcome-close/2",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "outcome.correct",
            "ledger-workflow-outcome-correction/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "policy.publish",
            "ledger-workflow-policy-publication/5",
            "ledger-workflow-operation-record/1",
            "ledger-workflow-publication-manifest/1",
            "absent",
            "ledger-workflow-publication-authority/1",
        ),
        (
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "work.accept",
            "ledger-workflow-work/1",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
        (
            "work.reverse",
            "ledger-workflow-work-reverse/2",
            "ledger-workflow-economic-operation/7",
            "ledger-workflow-economic-manifest/7",
            "ledger-economic-resolved-inputs/7",
            "ledger-workflow-economic-authority/6",
        ),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation,
                manifest,
                closure,
                authority,
            }));
        }
    }
    Ok(None)
}
/// Exact new administrative tuples. This is a wire recipe, not party custody,
/// authority, assent or a completed original certificate.
fn consent_management_recipe(
    kind: &str,
    payload: &str,
    session: &mut OriginalAuditSession,
) -> Result<Option<Recipe>> {
    for (k, p, manifest, authority) in [
        (
            "policy.publish",
            "ledger-workflow-policy-publication/4",
            "ledger-workflow-publication-manifest/1",
            "ledger-workflow-publication-authority/1",
        ),
        (
            "agreement.accept",
            "ledger-workflow-agreement-accept/6",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-agreement-authority/4",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-agreement-authority/4",
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation-replace/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-agreement-authority/4",
        ),
        (
            "agreement.revoke",
            "ledger-workflow-agreement-revoke/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-agreement-authority/2",
        ),
        (
            "authority.change",
            "ledger-workflow-party-authorize/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "authority.change",
            "ledger-workflow-party-revoke/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-party-management-authority/1",
        ),
        (
            "evidence.retain",
            "ledger-workflow-party-evidence-retain/1",
            "ledger-workflow-agreement-manifest/1",
            "ledger-workflow-party-management-authority/1",
        ),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation: "ledger-workflow-operation-record/1",
                manifest,
                closure: "absent",
                authority,
            }));
        }
    }
    Ok(None)
}
fn supplier_recipe(
    kind: &str,
    payload: &str,
    session: &mut OriginalAuditSession,
) -> Result<Option<Recipe>> {
    // Exact disjoint (kind,payload) rows from the frozen owner-local registry.
    // Ordinary/root rows do not inherit supplier authority4 or old closure3/4.
    for (k, p) in [
        ("evidence.retain", "ledger-workflow-fact-attest/1"),
        ("evidence.retain", "ledger-workflow-family-close-attest/1"),
        ("work.accept", "ledger-workflow-work/1"),
        ("link.assert", "ledger-workflow-link-assert/1"),
        ("outcome.submit", "ledger-workflow-outcome/1"),
        ("outcome.correct", "ledger-workflow-outcome-correction/1"),
        ("outcome.close", "ledger-workflow-outcome-close/2"),
        ("quantity.correct", "ledger-workflow-quantity-correction/2"),
        ("work.reverse", "ledger-workflow-work-reverse/2"),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation: "ledger-workflow-economic-operation/6",
                manifest: "ledger-workflow-economic-manifest/6",
                closure: "ledger-economic-resolved-inputs/6",
                authority: "ledger-workflow-economic-authority/5",
            }));
        }
    }
    for (k, p) in [
        ("evidence.retain", "ledger-workflow-supplier-cost-attest/1"),
        (
            "invocation.authorize",
            "ledger-workflow-invocation-authorize/1",
        ),
        ("invocation.close", "ledger-workflow-invocation-close/1"),
        ("cost.finalize", "ledger-workflow-cost-finalize/1"),
        ("cost.correct", "ledger-workflow-cost-correct/1"),
    ] {
        if equal_text(kind, k, session)? && equal_text(payload, p, session)? {
            return Ok(Some(Recipe {
                operation: "ledger-workflow-economic-operation/5",
                manifest: "ledger-workflow-economic-manifest/5",
                closure: "ledger-economic-resolved-inputs/5",
                authority: "ledger-workflow-economic-authority/4",
            }));
        }
    }
    if equal_text(kind, "authority.change", session)?
        && equal_text(payload, "ledger-cost-authority-enroll/1", session)?
    {
        return Ok(Some(Recipe {
            operation: "ledger-workflow-economic-operation/5",
            manifest: "ledger-workflow-economic-manifest/5",
            closure: "ledger-economic-resolved-inputs/5",
            authority: "ledger-cost-enrollment-authority/1",
        }));
    }
    Ok(None)
}
pub(super) const LOCAL_REGISTRY: &str =
    "sha256:d0a21becb08150a781770cfef80b5521eeb93c3b45aa9ebdab23c800ef44ee4a";
// Exact adopted local-execution-01 dispatch. Historical registries retain
// their original tuples; recognition here does not certify an executor.
fn local_recipe(kind: &str, payload: &str, s: &mut OriginalAuditSession)
    -> Result<Option<(Recipe, &'static str)>> {
    const ROWS: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
        ("agreement.accept", "ledger-workflow-agreement-accept/9", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-agreement-authority/5", "ledger-operation-descriptors/7"),
        ("agreement.activate", "ledger-workflow-activation-replace/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-agreement-authority/5", "ledger-operation-descriptors/7"),
        ("agreement.activate", "ledger-workflow-activation/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-agreement-authority/5", "ledger-operation-descriptors/7"),
        ("agreement.revoke", "ledger-workflow-agreement-revoke/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-agreement-authority/2", "ledger-operation-descriptors/3"),
        ("authority.change", "ledger-workflow-party-authorize/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-party-management-authority/1", "ledger-operation-descriptors/3"),
        ("authority.change", "ledger-workflow-party-revoke/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-party-management-authority/1", "ledger-operation-descriptors/3"),
        ("cost.correct", "ledger-workflow-cost-correct/2", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("cost.finalize", "ledger-workflow-cost-finalize/2", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("evidence.retain", "ledger-workflow-app-cost-attest/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("evidence.retain", "ledger-workflow-app-observe/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("evidence.retain", "ledger-workflow-fact-attest/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("evidence.retain", "ledger-workflow-family-close-attest/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("evidence.retain", "ledger-workflow-party-evidence-retain/1", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-party-management-authority/1", "ledger-operation-descriptors/3"),
        ("evidence.retain", "ledger-workflow-party-evidence-retain/2", "ledger-workflow-operation-record/1", "ledger-workflow-agreement-manifest/1", "absent", "ledger-workflow-party-management-authority/1", "ledger-operation-descriptors/7"),
        ("link.assert", "ledger-workflow-link-assert/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("outcome.close", "ledger-workflow-outcome-close/2", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("outcome.correct", "ledger-workflow-outcome-correction/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("outcome.submit", "ledger-workflow-outcome/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("policy.publish", "ledger-workflow-policy-publication/6", "ledger-workflow-operation-record/1", "ledger-workflow-publication-manifest/1", "absent", "ledger-workflow-publication-authority/1", "ledger-operation-descriptors/1"),
        ("quantity.correct", "ledger-workflow-quantity-correction/2", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("work.accept", "ledger-workflow-work/1", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
        ("work.reverse", "ledger-workflow-work-reverse/2", "ledger-workflow-economic-operation/10", "ledger-workflow-economic-manifest/10", "ledger-local-selected-inputs/1", "ledger-workflow-economic-authority/8", "ledger-operation-descriptors/8"),
    ];
    for &(k,p,operation,manifest,closure,authority,descriptor) in ROWS {
        if equal_text(kind,k,s)? && equal_text(payload,p,s)? {
            return Ok(Some((Recipe{operation,manifest,closure,authority},descriptor)));
        }
    }
    Ok(None)
}
pub(super) fn descriptor_schema(
    kind: &str,
    payload: &str,
    registry: &str,
    session: &mut OriginalAuditSession,
) -> Result<&'static str> {
    if equal_text(registry, LOCAL_REGISTRY, session)? {
        return local_recipe(kind,payload,session)?.map(|(_,descriptor)|descriptor)
            .ok_or_else(||reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, PARTY_COMMITMENT_REGISTRY, session)? {
        let selected = party_commitment_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?;
        return Ok(if equal_text(selected.closure, "absent", session)? {
            if equal_text(kind, "policy.publish", session)? {
                "ledger-operation-descriptors/1"
            } else if equal_text(kind, "agreement.accept", session)?
                || equal_text(kind, "agreement.activate", session)?
                || equal_text(payload, "ledger-workflow-party-evidence-retain/2", session)?
            {
                "ledger-operation-descriptors/7"
            } else {
                "ledger-operation-descriptors/3"
            }
        } else {
            "ledger-operation-descriptors/6"
        });
    }
    if equal_text(registry, APP_CAUSAL_REGISTRY, session)? {
        let selected = app_causal_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?;
        return Ok(if equal_text(selected.closure, "absent", session)? {
            if equal_text(kind, "policy.publish", session)? {
                "ledger-operation-descriptors/1"
            } else {
                "ledger-operation-descriptors/3"
            }
        } else {
            "ledger-operation-descriptors/5"
        });
    }
    if equal_text(registry, APP_REGISTRY, session)? {
        let selected = app_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?;
        return Ok(if equal_text(selected.closure, "absent", session)? {
            if equal_text(kind, "policy.publish", session)? {
                "ledger-operation-descriptors/1"
            } else {
                "ledger-operation-descriptors/3"
            }
        } else {
            "ledger-operation-descriptors/4"
        });
    }
    if equal_text(registry, CONSENT_REGISTRY, session)? {
        if supplier_recipe(kind, payload, session)?.is_some() {
            return Ok("ledger-operation-descriptors/2");
        }
        if consent_management_recipe(kind, payload, session)?.is_some() {
            return Ok(if equal_text(kind, "policy.publish", session)? {
                "ledger-operation-descriptors/1"
            } else {
                "ledger-operation-descriptors/3"
            });
        }
        return Err(reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, SUPPLIER_REGISTRY, session)?
        || equal_text(registry, DEADLINE_REGISTRY, session)?
        || equal_text(registry, WORK_TARGET_REGISTRY, session)?
    {
        if supplier_recipe(kind, payload, session)?.is_some() {
            return Ok("ledger-operation-descriptors/2");
        }
        if (equal_text(kind, "policy.publish", session)?
            && equal_text(payload, "ledger-workflow-policy-publication/3", session)?)
            || (equal_text(kind, "agreement.accept", session)?
                && equal_text(payload, "ledger-workflow-agreement-accept/5", session)?)
        {
            return Ok("ledger-operation-descriptors/1");
        }
        return Err(reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    Ok("ledger-operation-descriptors/1")
}
pub(super) fn recipe(
    kind: &str,
    payload: &str,
    registry: &str,
    session: &mut OriginalAuditSession,
) -> Result<Recipe> {
    if equal_text(registry, LOCAL_REGISTRY, session)? {
        return local_recipe(kind,payload,session)?.map(|(recipe,_)|recipe)
            .ok_or_else(||reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, PARTY_COMMITMENT_REGISTRY, session)? {
        return party_commitment_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, APP_CAUSAL_REGISTRY, session)? {
        return app_causal_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, APP_REGISTRY, session)? {
        return app_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, CONSENT_REGISTRY, session)? {
        if let Some(selected) = supplier_recipe(kind, payload, session)? {
            return Ok(selected);
        }
        return consent_management_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    if equal_text(registry, SUPPLIER_REGISTRY, session)?
        || equal_text(registry, DEADLINE_REGISTRY, session)?
        || equal_text(registry, WORK_TARGET_REGISTRY, session)?
    {
        return supplier_recipe(kind, payload, session)?
            .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    const ECON: &str = "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e";
    const CAUSAL: &str = "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54";
    const EXPOSURE: &str =
        "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4";
    const FAMILY_CLOSE: &str =
        "sha256:3aae5efefe5a28f8e1bd5382609652e67d9cf710250c8d29874715399f9449dc";
    // Dispatch EACH kind/payload/registry tuple, never 'known registry=>latest'.
    const ORDINARY: &[(&str, &str)] = &[
        ("work.accept", "ledger-workflow-work/1"),
        ("outcome.submit", "ledger-workflow-outcome/1"),
        ("outcome.correct", "ledger-workflow-outcome-correction/1"),
        ("outcome.close", "ledger-workflow-outcome-close/1"),
        ("link.assert", "ledger-workflow-link-assert/1"),
    ];
    for (k, p) in ORDINARY {
        if equal_text(kind, k, session)?
            && equal_text(payload, p, session)?
            && (equal_text(registry, ECON, session)?
                || equal_text(registry, CAUSAL, session)?
                || equal_text(registry, EXPOSURE, session)?
                || (equal_text(registry, FAMILY_CLOSE, session)?
                    && !equal_text(kind, "outcome.close", session)?))
        {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/3",
                manifest: "ledger-workflow-economic-manifest/3",
                closure: "ledger-economic-resolved-inputs/3",
                authority: "ledger-workflow-economic-authority/2",
            });
        }
    }
    // The old /1 correction tuples exist ONLY in economic-audit03. Neither
    // additive registry inherits them merely by keeping ordinary schemas3.
    for (k, p) in [
        ("quantity.correct", "ledger-workflow-quantity-correction/1"),
        ("work.reverse", "ledger-workflow-work-reverse/1"),
    ] {
        if equal_text(kind, k, session)?
            && equal_text(payload, p, session)?
            && equal_text(registry, ECON, session)?
        {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/3",
                manifest: "ledger-workflow-economic-manifest/3",
                closure: "ledger-economic-resolved-inputs/3",
                authority: "ledger-workflow-economic-authority/2",
            });
        }
    }
    for (k, p) in [
        ("quantity.correct", "ledger-workflow-quantity-correction/2"),
        ("work.reverse", "ledger-workflow-work-reverse/2"),
    ] {
        if equal_text(kind, k, session)?
            && equal_text(payload, p, session)?
            && (equal_text(registry, CAUSAL, session)?
                || equal_text(registry, EXPOSURE, session)?
                || equal_text(registry, FAMILY_CLOSE, session)?)
        {
            return Ok(Recipe {
                operation: "ledger-workflow-economic-operation/4",
                manifest: "ledger-workflow-economic-manifest/4",
                closure: "ledger-economic-resolved-inputs/4",
                authority: "ledger-workflow-economic-authority/3",
            });
        }
    }
    if equal_text(kind, "outcome.close", session)?
        && equal_text(payload, "ledger-workflow-outcome-close/2", session)?
        && equal_text(registry, FAMILY_CLOSE, session)?
    {
        return Ok(Recipe {
            operation: "ledger-workflow-economic-operation/3",
            manifest: "ledger-workflow-economic-manifest/3",
            closure: "ledger-economic-resolved-inputs/3",
            authority: "ledger-workflow-economic-authority/2",
        });
    }
    Err(reject("UNSUPPORTED_SCHEMA_PROFILE"))
}
fn same_value(left: &Value, right: &Value, session: &mut OriginalAuditSession) -> Result<()> {
    let a = session
        .canonical_representation(left, RepresentationKind::Record)
        .map_err(core_error)?;
    let b = session
        .canonical_representation(right, RepresentationKind::Record)
        .map_err(core_error)?;
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(a.bytes().len()).map_err(core_error)?;
    work.scan_bytes(b.bytes().len()).map_err(core_error)?;
    require(a.bytes() == b.bytes(), "INTEGRITY_FAILURE")
}
fn checked(
    raw: &[u8],
    kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let parsed = session
        .parse_representation(raw, kind)
        .map_err(core_error)?;
    let encoded = session
        .canonical_representation(parsed.value(), kind)
        .map_err(core_error)?;
    let work = session.semantic_work();
    work.visit(Visit::MemberCompare).map_err(core_error)?;
    work.scan_bytes(raw.len()).map_err(core_error)?;
    work.scan_bytes(encoded.bytes().len()).map_err(core_error)?;
    require(raw == encoded.bytes(), "INTEGRITY_FAILURE")?;
    Ok(parsed)
}

fn counter(value: &str, session: &mut OriginalAuditSession) -> Result<i64> {
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        !value.is_empty() && !value.starts_with('0'),
        "INTEGRITY_FAILURE",
    )?;
    let mut n = 0i64;
    for byte in value.bytes() {
        session
            .semantic_work()
            .visit(Visit::ScalarDigit)
            .map_err(core_error)?;
        require(byte.is_ascii_digit(), "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&n.into(), &10i64.into()])
            .map_err(core_error)?;
        n = n.checked_mul(10).ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &i64::from(byte - b'0').into()])
            .map_err(core_error)?;
        n = n
            .checked_add(i64::from(byte - b'0'))
            .ok_or(ServiceError::IntegrityFailure)?;
    }
    Ok(n)
}
fn update(h: &mut Sha256, bytes: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .hash_bytes(bytes.len())
        .map_err(core_error)?;
    h.update(bytes);
    Ok(())
}
fn update_value(
    h: &mut Sha256,
    value: &Value,
    kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let encoded = session
        .canonical_representation(value, kind)
        .map_err(core_error)?;
    update(h, encoded.bytes(), session)
}
fn value_hash(
    domain: &[u8],
    value: &Value,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    update(&mut h, domain, session)?;
    update_value(&mut h, value, RepresentationKind::Record, session)?;
    Ok(h.finalize().into())
}
fn operation_hash(
    scope: &Value,
    op: &Value,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let mut h = Sha256::new();
    update(
        &mut h,
        b"bean-counter/workflow-record/1\0[\"workflow-operation\",",
        session,
    )?;
    update_value(&mut h, scope, RepresentationKind::Record, session)?;
    update(&mut h, b",", session)?;
    update_value(&mut h, op, RepresentationKind::OperationFrame, session)?;
    update(&mut h, b"]", session)?;
    Ok(h.finalize().into())
}
fn without_hash(
    domain: &[u8],
    value: &Value,
    excluded: &str,
    value_kind: RepresentationKind,
    session: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let object = value.as_object().ok_or(ServiceError::IntegrityFailure)?;
    let mut h = Sha256::new();
    update(&mut h, domain, session)?;
    update(&mut h, b"{", session)?;
    let mut first = true;
    // All keys are exact closed ASCII dictionaries, so their BTree order equals
    // JCS UTF-16 order. Values use the same canonical emitter, without clones.
    for (name, v) in object {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if name == excluded {
            continue;
        }
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(
            name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "INTEGRITY_FAILURE",
        )?;
        if !first {
            update(&mut h, b",", session)?;
        }
        first = false;
        update(&mut h, b"\"", session)?;
        update(&mut h, name.as_bytes(), session)?;
        update(&mut h, b"\":", session)?;
        update_value(&mut h, v, value_kind, session)?;
    }
    update(&mut h, b"}", session)?;
    Ok(h.finalize().into())
}
fn id(
    digest: &[u8; 32],
    actual: &[u8],
    prefix: &str,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(actual.len())
        .map_err(core_error)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    let actual = std::str::from_utf8(actual).map_err(|_| ServiceError::IntegrityFailure)?;
    let suffix = actual
        .strip_prefix(prefix)
        .ok_or(ServiceError::IntegrityFailure)?;
    request::digest_text(digest, suffix, false, session)
}
fn digest(digest: &[u8; 32], actual: &[u8], session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .scan_bytes(actual.len())
        .map_err(core_error)?;
    let actual = std::str::from_utf8(actual).map_err(|_| ServiceError::IntegrityFailure)?;
    request::digest_text(digest, actual, true, session)
}

fn reference<'a>(
    value: &'a Value,
    session: &mut OriginalAuditSession,
) -> Result<(&'a str, &'a str, &'a str)> {
    request::closed(value, &["kind", "id", "digest"], session)?;
    let kind = request::text(value, "kind", session)?;
    let id = request::text(value, "id", session)?;
    let digest = request::text(value, "digest", session)?;
    for text in [kind, id, digest] {
        session
            .semantic_work()
            .scan_bytes(text.len())
            .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(text.is_ascii(), "INTEGRITY_FAILURE")?;
    }
    Ok((digest, id, kind))
}
/// Recompute captured framing commitments without treating saved anchors as
/// their own provenance. The semantic handler must separately extract anchors
/// from authenticated original inputs and reproduce the normalized payload.
fn descriptor_hashes(
    evidence: &Value,
    key: &OperationKey,
    kind: &str,
    original: &SavedRequest,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let normalized = request::field(evidence, "normalized_request", session)?;
    request::closed(
        normalized,
        &["schema", "key", "kind", "contract", "payload"],
        session,
    )?;
    request::same_text(
        request::text(normalized, "schema", session)?,
        "ledger-operation/1",
        session,
    )?;
    request::same_text(
        request::text(normalized, "contract", session)?,
        "workflow/1",
        session,
    )?;
    request::same_text(request::text(normalized, "kind", session)?, kind, session)?;
    request::key(request::field(normalized, "key", session)?, key, session)?;
    let payload = request::field(normalized, "payload", session)?;
    let payload_schema = request::text(payload, "schema", session)?;
    let raw_payload = request::field(original.value(), "payload", session)?;
    request::same_text(
        payload_schema,
        request::text(raw_payload, "schema", session)?,
        session,
    )?;
    let resolution = request::field(evidence, "descriptor_resolution", session)?;
    request::closed(
        resolution,
        &[
            "schema",
            "registry_digest",
            "operation_kind",
            "payload_schema",
            "locator_digest",
            "anchors",
            "descriptors",
            "normalization_profile",
            "closure_digest",
        ],
        session,
    )?;
    let expected_descriptor = descriptor_schema(
        kind,
        payload_schema,
        request::text(resolution, "registry_digest", session)?,
        session,
    )?;
    request::same_text(
        request::text(resolution, "schema", session)?,
        expected_descriptor,
        session,
    )?;
    request::same_text(
        request::text(resolution, "operation_kind", session)?,
        kind,
        session,
    )?;
    request::same_text(
        request::text(resolution, "payload_schema", session)?,
        payload_schema,
        session,
    )?;
    request::same_text(
        request::text(resolution, "normalization_profile", session)?,
        "ledger-operation-normalization/2",
        session,
    )?;
    // The enclosing operation remains subject to its existing8MiB container
    // bound. These nested values are not new independently admitted records.
    let closure = without_hash(
        b"bean-counter/descriptor-closure/1\0",
        resolution,
        "closure_digest",
        RepresentationKind::OperationFrame,
        session,
    )?;
    request::digest_text(
        &closure,
        request::text(resolution, "closure_digest", session)?,
        true,
        session,
    )?;
    let mut schema_hash = Sha256::new();
    update(&mut schema_hash, b"bean-counter/schema-bundle/1\0", session)?;
    update_value(
        &mut schema_hash,
        request::field(resolution, "descriptors", session)?,
        RepresentationKind::OperationFrame,
        session,
    )?;
    request::digest_text(
        &schema_hash.finalize().into(),
        request::text(evidence, "schema_bundle_digest", session)?,
        true,
        session,
    )?;
    let mut intent_hash = Sha256::new();
    update(
        &mut intent_hash,
        b"bean-counter/operation-request/2\0[",
        session,
    )?;
    update_value(
        &mut intent_hash,
        request::field(resolution, "closure_digest", session)?,
        RepresentationKind::OperationFrame,
        session,
    )?;
    update(&mut intent_hash, b",", session)?;
    update_value(
        &mut intent_hash,
        normalized,
        RepresentationKind::OperationFrame,
        session,
    )?;
    update(&mut intent_hash, b"]", session)?;
    request::digest_text(
        &intent_hash.finalize().into(),
        request::text(evidence, "request_hash", session)?,
        true,
        session,
    )
}
async fn indexed_sets(
    tx: &mut SqliteTx,
    identity: &[u8],
    frame: &OriginalFrame,
    op: &Value,
    manifest: &Value,
    session: &mut OriginalAuditSession,
) -> Result<()> {
    let members = request::field(manifest, "members", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&BigInt::from(members.len()), &frame.member_count.into()],
        )
        .map_err(core_error)?;
    require(
        members.len() as i64 == frame.member_count,
        "INTEGRITY_FAILURE",
    )?;
    let mut previous = None;
    let mut operation_found = false;
    for member in members {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let tuple = reference(member, session)?;
        if let Some(prior) = previous {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            require(prior < tuple, "INTEGRITY_FAILURE")?;
        }
        previous = Some(tuple);
        require(
            tx.workflow_financial_member_present(identity, tuple.2, tuple.1, tuple.0, session)
                .await
                .map_err(resource)?,
            "INTEGRITY_FAILURE",
        )?;
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        if tuple.2 == "workflow-operation"
            && tuple.1.as_bytes() == frame.operation_id.bytes()
            && tuple.0.as_bytes() == frame.operation_digest.bytes()
        {
            operation_found = true;
        }
    }
    require(operation_found, "INTEGRITY_FAILURE")?;
    let results = request::field(op, "result_refs", session)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    require(
        !results.is_empty() && results.len() <= 4096,
        "INTEGRITY_FAILURE",
    )?;
    let mut previous = None;
    for result in results {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let tuple = reference(result, session)?;
        if let Some(prior) = previous {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            require(prior < tuple, "INTEGRITY_FAILURE")?;
        }
        previous = Some(tuple);
        require(
            tx.workflow_financial_member_present(identity, tuple.2, tuple.1, tuple.0, session)
                .await
                .map_err(resource)?,
            "INTEGRITY_FAILURE",
        )?;
    }
    Ok(())
}
pub(super) async fn load(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    session: &mut OriginalAuditSession,
) -> Result<OriginalWrappers> {
    let frame = tx
        .workflow_original_frame(identity, &key.logical_store_id, &key.scope, session)
        .await
        .map_err(resource)?
        .ok_or(ServiceError::IntegrityFailure)?;
    validate_frame(tx, key, identity, frame, session).await
}

/// Consume the exact chronological frame without loading a second copy or
/// selecting it through another hint. This remains unresolved mechanical DATA.
pub(super) async fn validate_frame(
    tx: &mut SqliteTx,
    key: &OperationKey,
    identity: &[u8],
    frame: OriginalFrame,
    session: &mut OriginalAuditSession,
) -> Result<OriginalWrappers> {
    let operation = checked(
        frame.operation.bytes(),
        RepresentationKind::OperationFrame,
        session,
    )?;
    let manifest = checked(
        frame.manifest.bytes(),
        RepresentationKind::ManifestFrame,
        session,
    )?;
    let receipt = checked(
        frame.receipt.bytes(),
        RepresentationKind::ReceiptFrame,
        session,
    )?;
    let op = operation.value();
    let man = manifest.value();
    let rec = receipt.value();
    let operation_schema = request::text(op, "schema", session)?;
    let mut economic = false;
    for schema in [
        "ledger-workflow-economic-operation/3",
        "ledger-workflow-economic-operation/4",
        "ledger-workflow-economic-operation/5",
        "ledger-workflow-economic-operation/6",
        "ledger-workflow-economic-operation/7",
        "ledger-workflow-economic-operation/8",
        "ledger-workflow-economic-operation/9",
    ] {
        if equal_text(operation_schema, schema, session)? {
            economic = true;
            break;
        }
    }
    let class = if economic {
        WireClass::Financial
    } else {
        match frame.kind {
            "policy.publish" => WireClass::Publication,
            "evidence.retain" | "authority.change" => WireClass::Prerequisite,
            "agreement.accept" | "agreement.activate" | "agreement.revoke" => {
                session
                    .semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                if request::text(op, "schema", session)?
                    == "ledger-workflow-agreement-alias-operation/1"
                {
                    WireClass::AgreementAlias
                } else {
                    WireClass::Agreement
                }
            }
            kind if is_kind(kind) => WireClass::Financial,
            _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
        }
    };
    if class == WireClass::Financial {
        request::closed(
            op,
            &[
                "schema",
                "key",
                "kind",
                "contract",
                "request_evidence",
                "authority_snapshot",
                "received_at",
                "accepted_at",
                "commit_sequence",
                "result_refs",
                "closure_profile",
                "source_observation_refs",
                "financial_observation_ref",
                "combined_audit_digest",
            ],
            session,
        )?;
        request::closed(
            man,
            &[
                "schema",
                "key",
                "members",
                "closure_profile",
                "source_observation_refs",
                "financial_observation_ref",
                "combined_audit_digest",
            ],
            session,
        )?;
    } else {
        request::closed(
            op,
            &[
                "schema",
                "key",
                "kind",
                "contract",
                "request_evidence",
                "authority_snapshot",
                "accepted_at",
                "commit_sequence",
                "result_refs",
            ],
            session,
        )?;
        request::closed(man, &["schema", "key", "members"], session)?;
    }
    request::closed(
        rec,
        &[
            "schema",
            "key",
            "request_hash",
            "request_hash_profile",
            "normalization_profile",
            "schema_bundle_digest",
            "descriptor_closure_digest",
            "original_request_bytes_sha256",
            "result_kind",
            "result_refs",
            "accepted_at",
            "commit_sequence",
            "manifest_hash",
            "receipt_hash",
        ],
        session,
    )?;
    for body in [op, man, rec] {
        request::key(request::field(body, "key", session)?, key, session)?;
    }
    request::same_text(request::text(op, "kind", session)?, frame.kind, session)?;
    request::same_text(
        request::text(op, "contract", session)?,
        "workflow/1",
        session,
    )?;
    let evidence = request::field(op, "request_evidence", session)?;
    let request = request::parse_saved(evidence, frame.kind, key, session)?;
    descriptor_hashes(evidence, key, frame.kind, &request, session)?;
    let payload = request::field(request.value(), "payload", session)?;
    let descriptors = request::field(evidence, "descriptor_resolution", session)?;
    if class == WireClass::Financial {
        let selected = recipe(
            frame.kind,
            request::text(payload, "schema", session)?,
            request::text(descriptors, "registry_digest", session)?,
            session,
        )?;
        request::same_text(
            request::text(op, "schema", session)?,
            selected.operation,
            session,
        )?;
        request::same_text(
            request::text(man, "schema", session)?,
            selected.manifest,
            session,
        )?;
        for body in [op, man] {
            request::same_text(
                request::text(body, "closure_profile", session)?,
                selected.closure,
                session,
            )?;
        }
        request::same_text(
            request::text(
                request::field(op, "authority_snapshot", session)?,
                "schema",
                session,
            )?,
            selected.authority,
            session,
        )?;
        for name in [
            "source_observation_refs",
            "financial_observation_ref",
            "combined_audit_digest",
        ] {
            same_value(
                request::field(op, name, session)?,
                request::field(man, name, session)?,
                session,
            )?;
        }
    } else {
        // These are the already-existing administrative WIRE shapes, not a
        // registry-support grant. Each dedicated semantic handler must still
        // reproduce its own original descriptor/normalization/alias recipe.
        let consent = if equal_text(
            request::text(descriptors,"registry_digest",session)?,LOCAL_REGISTRY,session,
        )? {
            Some(local_recipe(frame.kind,request::text(payload,"schema",session)?,session)?
                .ok_or_else(||reject("UNSUPPORTED_SCHEMA_PROFILE"))?.0)
        } else if equal_text(
            request::text(descriptors, "registry_digest", session)?,
            PARTY_COMMITMENT_REGISTRY,
            session,
        )? {
            Some(
                party_commitment_recipe(
                    frame.kind,
                    request::text(payload, "schema", session)?,
                    session,
                )?
                .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?,
            )
        } else if equal_text(
            request::text(descriptors, "registry_digest", session)?,
            CONSENT_REGISTRY,
            session,
        )? {
            Some(
                consent_management_recipe(
                    frame.kind,
                    request::text(payload, "schema", session)?,
                    session,
                )?
                .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?,
            )
        } else if equal_text(
            request::text(descriptors, "registry_digest", session)?,
            APP_REGISTRY,
            session,
        )? {
            Some(
                app_recipe(
                    frame.kind,
                    request::text(payload, "schema", session)?,
                    session,
                )?
                .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?,
            )
        } else if equal_text(
            request::text(descriptors, "registry_digest", session)?,
            APP_CAUSAL_REGISTRY,
            session,
        )? {
            Some(
                app_causal_recipe(
                    frame.kind,
                    request::text(payload, "schema", session)?,
                    session,
                )?
                .ok_or_else(|| reject("UNSUPPORTED_SCHEMA_PROFILE"))?,
            )
        } else {
            None
        };
        let (operation_schema, manifest_schema, authority_schema) = if let Some(selected) = consent
        {
            // An exact new registry never widens the old kind-only authority
            // recipe. Alias has its own schema and is not invented here.
            require(
                class != WireClass::AgreementAlias,
                "UNSUPPORTED_SCHEMA_PROFILE",
            )?;
            (selected.operation, selected.manifest, selected.authority)
        } else {
            match class {
                WireClass::Publication => (
                    "ledger-workflow-operation-record/1",
                    "ledger-workflow-publication-manifest/1",
                    "ledger-workflow-publication-authority/1",
                ),
                WireClass::Prerequisite => (
                    "ledger-workflow-operation-record/1",
                    "ledger-workflow-agreement-manifest/1",
                    "ledger-workflow-publication-authority/1",
                ),
                WireClass::Agreement => (
                    "ledger-workflow-operation-record/1",
                    "ledger-workflow-agreement-manifest/1",
                    "ledger-workflow-agreement-authority/2",
                ),
                WireClass::AgreementAlias => (
                    "ledger-workflow-agreement-alias-operation/1",
                    "ledger-workflow-agreement-alias-manifest/1",
                    "ledger-workflow-agreement-authority/3",
                ),
                WireClass::Financial => unreachable!(),
            }
        };
        request::same_text(
            request::text(op, "schema", session)?,
            operation_schema,
            session,
        )?;
        request::same_text(
            request::text(man, "schema", session)?,
            manifest_schema,
            session,
        )?;
        request::same_text(
            request::text(
                request::field(op, "authority_snapshot", session)?,
                "schema",
                session,
            )?,
            authority_schema,
            session,
        )?;
    }
    request::same_text(
        request::text(rec, "schema", session)?,
        "ledger-operation-receipt/2",
        session,
    )?;
    let result_kind = request::text(rec, "result_kind", session)?;
    session
        .semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    match class {
        WireClass::Financial => request::same_text(result_kind, "economic", session)?,
        WireClass::AgreementAlias => request::same_text(result_kind, "semantic_alias", session)?,
        _ => require(
            matches!(result_kind, "administrative" | "semantic_alias"),
            "INTEGRITY_FAILURE",
        )?,
    }
    // Aliasness is unresolved until its dedicated original relationship and
    // semantic intent check succeeds. This mechanical layer never completes it.
    request::same_text(
        request::text(rec, "request_hash_profile", session)?,
        "bean-counter/operation-request/2",
        session,
    )?;
    request::same_text(
        request::text(rec, "normalization_profile", session)?,
        "ledger-operation-normalization/2",
        session,
    )?;
    for name in [
        "request_hash",
        "schema_bundle_digest",
        "original_request_bytes_sha256",
    ] {
        same_value(
            request::field(rec, name, session)?,
            request::field(evidence, name, session)?,
            session,
        )?;
    }
    same_value(
        request::field(rec, "descriptor_closure_digest", session)?,
        request::field(descriptors, "closure_digest", session)?,
        session,
    )?;
    same_value(
        request::field(rec, "result_refs", session)?,
        request::field(op, "result_refs", session)?,
        session,
    )?;
    same_value(
        request::field(rec, "accepted_at", session)?,
        request::field(op, "accepted_at", session)?,
        session,
    )?;
    let accepted_at = ledgerlab_core::domain::Timestamp::parse_with_work(
        request::text(op, "accepted_at", session)?,
        session.semantic_work(),
    )
    .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&accepted_at.micros().into(), &frame.accepted_at_us.into()],
        )
        .map_err(core_error)?;
    require(
        accepted_at.micros() == frame.accepted_at_us,
        "INTEGRITY_FAILURE",
    )?;
    let op_sequence = counter(request::text(op, "commit_sequence", session)?, session)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&op_sequence.into(), &frame.sequence.into()])
        .map_err(core_error)?;
    require(op_sequence == frame.sequence, "INTEGRITY_FAILURE")?;
    same_value(
        request::field(rec, "commit_sequence", session)?,
        request::field(op, "commit_sequence", session)?,
        session,
    )?;
    let key_hash = value_hash(
        b"bean-counter/workflow-operation/1\0",
        request::field(op, "key", session)?,
        session,
    )?;
    id(&key_hash, frame.operation_id.bytes(), "wop_", session)?;
    let op_hash = operation_hash(
        request::field(request::field(op, "key", session)?, "scope", session)?,
        op,
        session,
    )?;
    digest(&op_hash, frame.operation_digest.bytes(), session)?;
    let manifest_hash = value_hash(b"bean-counter/workflow-manifest/1\0", man, session)?;
    id(
        &manifest_hash,
        frame.manifest_id.bytes(),
        "wmanifest_",
        session,
    )?;
    digest(&manifest_hash, frame.manifest_digest.bytes(), session)?;
    request::digest_text(
        &manifest_hash,
        request::text(rec, "manifest_hash", session)?,
        true,
        session,
    )?;
    let receipt_hash = without_hash(
        b"bean-counter/operation-receipt/2\0",
        rec,
        "receipt_hash",
        RepresentationKind::ReceiptFrame,
        session,
    )?;
    id(
        &receipt_hash,
        frame.receipt_id.bytes(),
        "wreceipt_",
        session,
    )?;
    digest(&receipt_hash, frame.receipt_digest.bytes(), session)?;
    request::digest_text(
        &receipt_hash,
        request::text(rec, "receipt_hash", session)?,
        true,
        session,
    )?;
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    session
        .semantic_work()
        .scan_bytes(frame.request_hash.bytes().len())
        .map_err(core_error)?;
    require(
        frame.request_hash.bytes() == request::text(evidence, "request_hash", session)?.as_bytes(),
        "INTEGRITY_FAILURE",
    )?;
    indexed_sets(tx, identity, &frame, op, man, session).await?;
    // Freshly extracted descriptors, normalized intent,
    // exact complete result/member sets and all economic/auth/capacity claims
    // remain mandatory in the full original reconstruction; this is NOT proof.
    Ok(OriginalWrappers {
        class,
        operation,
        manifest,
        receipt,
        request,
        frame,
    })
}

#[cfg(test)]
mod recipe_tests {
    #[test]
    fn commitment_dispatch_matches_frozen_rows_and_keeps_old_versions_separate() {
        let table: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/party-commitment-first-01/dispatch.json"
        )))
        .unwrap();
        for row in table["rows"].as_array().unwrap() {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let k = row["operation_kind"].as_str().unwrap();
            let p = row["payload_schema"].as_str().unwrap();
            let r = recipe(k, p, PARTY_COMMITMENT_REGISTRY, &mut s).unwrap();
            let saved = &row["saved_recipe"];
            assert_eq!(r.operation, saved["operation_schema"]);
            assert_eq!(r.manifest, saved["manifest_schema"]);
            assert_eq!(r.authority, saved["authority_schema"]);
            assert_eq!(r.closure, saved["closure_profile"]);
            assert_eq!(
                descriptor_schema(k, p, PARTY_COMMITMENT_REGISTRY, &mut s).unwrap(),
                row["descriptor_schema"]
            );
        }
        for (k, p, r) in [
            (
                "agreement.accept",
                "ledger-workflow-agreement-accept/8",
                PARTY_COMMITMENT_REGISTRY,
            ),
            (
                "agreement.accept",
                "ledger-workflow-agreement-accept/9",
                APP_CAUSAL_REGISTRY,
            ),
            (
                "evidence.retain",
                "ledger-workflow-party-evidence-retain/2",
                APP_CAUSAL_REGISTRY,
            ),
            (
                "quantity.correct",
                "ledger-workflow-quantity-correction/1",
                PARTY_COMMITMENT_REGISTRY,
            ),
            (
                "agreement.accept",
                "ledger-workflow-agreement-accept/6",
                PARTY_COMMITMENT_REGISTRY,
            ),
        ] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(recipe(k, p, r, &mut s).is_err());
        }
    }

    #[test]
    fn app_causal_registry_has_exact_new_rows_without_promoting_previous_payloads() {
        let registry: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/app-cost-causal-01/registry.json"
        )))
        .unwrap();
        for entry in registry["entries"].as_array().unwrap() {
            for payload in entry["payload_schemas"].as_array().unwrap() {
                let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
                let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let kind = entry["kind"].as_str().unwrap();
                let payload = payload.as_str().unwrap();
                let selected = recipe(kind, payload, APP_CAUSAL_REGISTRY, &mut session).unwrap();
                let saved = &entry["saved_recipe"];
                assert_eq!(selected.operation, saved["operation_schema"]);
                assert_eq!(selected.authority, saved["authority_schema"]);
                assert_eq!(selected.manifest, saved["manifest_schema"]);
                assert_eq!(selected.closure, saved["closure_profile"]);
                let descriptor = if selected.closure != "absent" {
                    "ledger-operation-descriptors/5"
                } else if kind == "policy.publish" {
                    "ledger-operation-descriptors/1"
                } else {
                    "ledger-operation-descriptors/3"
                };
                assert_eq!(
                    descriptor_schema(kind, payload, APP_CAUSAL_REGISTRY, &mut session).unwrap(),
                    descriptor
                );
            }
        }
        for (kind, payload) in [
            ("policy.publish", "ledger-workflow-policy-publication/5"),
            ("agreement.accept", "ledger-workflow-agreement-accept/7"),
            ("cost.finalize", "ledger-workflow-cost-finalize/1"),
            ("quantity.correct", "ledger-workflow-quantity-correction/1"),
        ] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(recipe(kind, payload, APP_CAUSAL_REGISTRY, &mut session).is_err());
        }
    }

    #[test]
    fn app_registry_dispatches_exact_frozen_rows_and_refuses_old_ap_payloads() {
        let registry: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/app-owned-cost-01/registry.json"
        )))
        .unwrap();
        for entry in registry["entries"].as_array().unwrap() {
            for payload in entry["payload_schemas"].as_array().unwrap() {
                let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
                let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                let kind = entry["kind"].as_str().unwrap();
                let payload = payload.as_str().unwrap();
                let selected = recipe(kind, payload, APP_REGISTRY, &mut session).unwrap();
                let saved = &entry["saved_recipe"];
                assert_eq!(selected.operation, saved["operation_schema"]);
                assert_eq!(selected.authority, saved["authority_schema"]);
                assert_eq!(selected.manifest, saved["manifest_schema"]);
                assert_eq!(selected.closure, saved["closure_profile"]);
                let expected = if selected.closure != "absent" {
                    "ledger-operation-descriptors/4"
                } else if kind == "policy.publish" {
                    "ledger-operation-descriptors/1"
                } else {
                    "ledger-operation-descriptors/3"
                };
                assert_eq!(
                    descriptor_schema(kind, payload, APP_REGISTRY, &mut session).unwrap(),
                    expected
                );
            }
        }
        for (kind, payload) in [
            ("quantity.correct", "ledger-workflow-quantity-correction/1"),
            ("work.reverse", "ledger-workflow-work-reverse/1"),
            ("cost.finalize", "ledger-workflow-cost-finalize/1"),
            ("agreement.accept", "ledger-workflow-agreement-accept/6"),
            (
                "invocation.authorize",
                "ledger-workflow-invocation-authorize/1",
            ),
            ("evidence.retain", "ledger-workflow-supplier-cost-attest/1"),
        ] {
            let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(recipe(kind, payload, APP_REGISTRY, &mut session).is_err());
        }
    }

    use super::*;
    #[test]
    fn exposure_dispatch_retains_exact_original_wrapper_versions() {
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let registry = "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4";
        for registry in [
            registry,
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
            "sha256:3aae5efefe5a28f8e1bd5382609652e67d9cf710250c8d29874715399f9449dc",
        ] {
            for (kind, old, new) in [
                (
                    "quantity.correct",
                    "ledger-workflow-quantity-correction/1",
                    "ledger-workflow-quantity-correction/2",
                ),
                (
                    "work.reverse",
                    "ledger-workflow-work-reverse/1",
                    "ledger-workflow-work-reverse/2",
                ),
            ] {
                assert!(recipe(kind, old, registry, &mut session).is_err());
                let correct = recipe(kind, new, registry, &mut session).unwrap();
                assert_eq!(correct.operation, "ledger-workflow-economic-operation/4");
                let legacy = recipe(
                    kind,
                    old,
                    "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
                    &mut session,
                )
                .unwrap();
                assert_eq!(legacy.operation, "ledger-workflow-economic-operation/3");
            }
            for (kind, payload) in [
                ("work.accept", "ledger-workflow-work/1"),
                ("outcome.submit", "ledger-workflow-outcome/1"),
                ("outcome.correct", "ledger-workflow-outcome-correction/1"),
                ("outcome.close", "ledger-workflow-outcome-close/1"),
                ("link.assert", "ledger-workflow-link-assert/1"),
            ] {
                if registry
                    == "sha256:3aae5efefe5a28f8e1bd5382609652e67d9cf710250c8d29874715399f9449dc"
                    && kind == "outcome.close"
                {
                    assert!(recipe(kind, payload, registry, &mut session).is_err());
                    assert_eq!(
                        recipe(
                            kind,
                            "ledger-workflow-outcome-close/2",
                            registry,
                            &mut session
                        )
                        .unwrap()
                        .operation,
                        "ledger-workflow-economic-operation/3"
                    );
                    continue;
                }
                assert_eq!(
                    recipe(kind, payload, registry, &mut session)
                        .unwrap()
                        .operation,
                    "ledger-workflow-economic-operation/3"
                );
            }
        }
        assert!(
            recipe(
                "outcome.close",
                "ledger-workflow-outcome-close/2",
                registry,
                &mut session
            )
            .is_err()
        );
        let ordinary = recipe(
            "work.accept",
            "ledger-workflow-work/1",
            registry,
            &mut session,
        )
        .unwrap();
        assert_eq!(ordinary.operation, "ledger-workflow-economic-operation/3");
        assert_eq!(ordinary.authority, "ledger-workflow-economic-authority/2");
        let causal = recipe(
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            registry,
            &mut session,
        )
        .unwrap();
        assert_eq!(causal.operation, "ledger-workflow-economic-operation/4");
        assert_eq!(causal.authority, "ledger-workflow-economic-authority/3");
        assert!(
            recipe(
                "work.accept",
                "ledger-workflow-quantity-correction/2",
                registry,
                &mut session
            )
            .is_err()
        );
        assert!(
            recipe(
                "agreement.accept",
                "ledger-workflow-agreement-accept/4",
                registry,
                &mut session
            )
            .is_err()
        );
        assert!(
            recipe(
                "quantity.correct",
                "ledger-workflow-quantity-correction/2",
                "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
                &mut session
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod supplier_dispatch_tests {
    use super::*;
    #[test]
    fn scoped_consent_registry_uses_exact_payload_authority_and_descriptor_versions() {
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let frozen: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/candidates/full-suite-v1/core/supplier-scoped-consent-01/registry.json"))).unwrap();
        for row in frozen["entries"].as_array().unwrap() {
            let kind = row["kind"].as_str().unwrap();
            let saved = &row["saved_recipe"];
            for payload in row["payload_schemas"].as_array().unwrap() {
                let payload = payload.as_str().unwrap();
                let selected = recipe(kind, payload, CONSENT_REGISTRY, &mut session).unwrap();
                assert_eq!(selected.operation, saved["operation_schema"]);
                assert_eq!(selected.manifest, saved["manifest_schema"]);
                assert_eq!(selected.authority, saved["authority_schema"]);
                assert_eq!(selected.closure, saved["closure_profile"]);
                let expected = if kind == "policy.publish" {
                    "ledger-operation-descriptors/1"
                } else if selected.closure == "absent" {
                    "ledger-operation-descriptors/3"
                } else {
                    "ledger-operation-descriptors/2"
                };
                assert_eq!(
                    descriptor_schema(kind, payload, CONSENT_REGISTRY, &mut session).unwrap(),
                    expected
                );
            }
        }
        for (kind, payload) in [
            ("policy.publish", "ledger-workflow-policy-publication/3"),
            ("agreement.accept", "ledger-workflow-agreement-accept/5"),
            ("authority.change", "ledger-workflow-customer-register/1"),
            ("evidence.retain", "ledger-workflow-evidence-retain/1"),
            ("evidence.retain", "ledger-workflow-party-authorize/1"),
            (
                "authority.change",
                "ledger-workflow-party-evidence-retain/1",
            ),
            ("quantity.correct", "ledger-workflow-quantity-correction/1"),
        ] {
            assert!(recipe(kind, payload, CONSENT_REGISTRY, &mut session).is_err());
            assert!(descriptor_schema(kind, payload, CONSENT_REGISTRY, &mut session).is_err());
        }
        assert!(
            recipe(
                "authority.change",
                "ledger-workflow-party-authorize/1",
                WORK_TARGET_REGISTRY,
                &mut session
            )
            .is_err()
        );
    }
    #[test]
    fn frozen_supplier_and_deadline_rows_dispatch_exact_saved_tuples() {
        let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        for (registry, raw) in [
            (
                SUPPLIER_REGISTRY,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../contracts/candidates/full-suite-v1/core/owner-local-cost-01/registry.json"
                )),
            ),
            (
                DEADLINE_REGISTRY,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../contracts/candidates/full-suite-v1/core/deadline-close-01/registry.json"
                )),
            ),
            (
                WORK_TARGET_REGISTRY,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../contracts/candidates/full-suite-v1/core/delayed-cost-work-target-01/registry.json"
                )),
            ),
        ] {
            let frozen: Value = serde_json::from_str(raw).unwrap();
            let mut rows = 0;
            for entry in frozen["entries"].as_array().unwrap() {
                let saved = &entry["saved_recipe"];
                let kind = entry["kind"].as_str().unwrap();
                if !saved["operation_schema"]
                    .as_str()
                    .unwrap()
                    .starts_with("ledger-workflow-economic-operation/")
                {
                    continue;
                }
                for payload in entry["payload_schemas"].as_array().unwrap() {
                    let payload = payload.as_str().unwrap();
                    let actual = recipe(kind, payload, registry, &mut session).unwrap();
                    assert_eq!(
                        actual.operation,
                        saved["operation_schema"].as_str().unwrap()
                    );
                    assert_eq!(actual.manifest, saved["manifest_schema"].as_str().unwrap());
                    assert_eq!(
                        actual.authority,
                        saved["authority_schema"].as_str().unwrap()
                    );
                    assert_eq!(actual.closure, saved["closure_profile"].as_str().unwrap());
                    assert_eq!(
                        descriptor_schema(kind, payload, registry, &mut session).unwrap(),
                        "ledger-operation-descriptors/2"
                    );
                    rows += 1;
                }
            }
            assert_eq!(rows, 15);
            for (kind, payload) in [
                ("policy.publish", "ledger-workflow-policy-publication/3"),
                ("agreement.accept", "ledger-workflow-agreement-accept/5"),
            ] {
                assert_eq!(
                    descriptor_schema(kind, payload, registry, &mut session).unwrap(),
                    "ledger-operation-descriptors/1"
                );
            }

            for (kind, payload) in [
                ("quantity.correct", "ledger-workflow-quantity-correction/1"),
                ("work.reverse", "ledger-workflow-work-reverse/1"),
                ("outcome.close", "ledger-workflow-outcome-close/1"),
                ("evidence.retain", "ledger-workflow-evidence-retain/1"),
                ("authority.change", "ledger-workflow-source-enroll/1"),
                ("cost.finalize", "ledger-workflow-cost-correct/1"),
            ] {
                assert!(recipe(kind, payload, registry, &mut session).is_err());
            }
        }
    }
}
