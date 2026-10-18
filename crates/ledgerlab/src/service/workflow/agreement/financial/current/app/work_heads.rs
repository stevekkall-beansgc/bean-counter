//! Actual held projection rows for SAME core-derived work locator keys. They
//! remain DATA; child/context/body/current-head checks and atomic CAS still run.
use super::*;
use crate::store::sqlite::workflow::financial::{
    AppClaimLocator, FamilyIdentityHead, HeadKey, HeadLocator,
};
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::{FinancialHeadKey, WorkHeadLocators},
};
use std::mem;
pub(super) struct ObservedFamilyHead {
    family: Option<FamilyIdentityHead>,
    decision: Option<HeadLocator>,
}
pub(super) struct ObservedWorkHeads {
    claim: Option<AppClaimLocator>,
    root: Option<HeadLocator>,
    capacity: Option<HeadLocator>,
    chain: Option<HeadLocator>,
    families: Vec<ObservedFamilyHead>,
    app_decisions: Vec<Option<HeadLocator>>,
    pub(super) seeds: AuditedBytes,
    pub(super) pins: AuditedBytes,
    _storage: ExternalBufferLease,
    _families: ExternalBufferLease,
    _apps: ExternalBufferLease,
}
fn product(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_mul(b).ok_or(ServiceError::Unavailable)
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn text<'a>(bytes: &'a [u8], s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.semantic_work()
        .scan_bytes(bytes.len())
        .map_err(core_error)?;
    std::str::from_utf8(bytes).map_err(|_| ServiceError::IntegrityFailure)
}
fn reference(
    kind: &[u8],
    id: &[u8],
    digest: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let kind = wire::string(text(kind, s)?, s)?;
    let id = wire::string(text(id, s)?, s)?;
    let digest = wire::string(text(digest, s)?, s)?;
    let _fields = s
        .reserve_external_payload(2 * mem::size_of::<[(&str, &[u8]); 3]>())
        .map_err(core_error)?;
    wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )
}
fn pin(
    head: &HeadLocator,
    kind: &str,
    reference: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let kind = wire::string(kind, s)?;
    let revision = text(head.revision.bytes(), s)?;
    super::super::super::acceptance::counter(revision, s)?;
    let revision = wire::string(revision, s)?;
    let _fields = s
        .reserve_external_payload(2 * mem::size_of::<[(&str, &[u8]); 3]>())
        .map_err(core_error)?;
    wire::object(
        &[
            ("kind", kind.bytes()),
            ("ref", reference),
            ("revision", revision.bytes()),
        ],
        262144,
        s,
    )
}
fn retain_head(
    prefix: &mut prefix::LocatedAppAcceptance,
    head: &Option<HeadLocator>,
    kind: &str,
    rows: &mut Vec<AuditedBytes>,
    pins: &mut Vec<AuditedBytes>,
    pin_kind: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    if let Some(head) = head {
        check::same_text(text(head.kind.bytes(), s)?, kind, s)?;
        let r = reference(head.kind.bytes(), head.id.bytes(), head.digest.bytes(), s)?;
        prefix.require_held_reference(r.bytes(), kind, s)?;
        let encoded_pin = pin(head, pin_kind, r.bytes(), s)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<AuditedBytes>())
            .map_err(core_error)?;
        pins.push(encoded_pin);
        s.semantic_work()
            .write_bytes(mem::size_of::<AuditedBytes>())
            .map_err(core_error)?;
        rows.push(r);
    }
    Ok(())
}
pub(super) async fn observe(
    tx: &mut SqliteTx,
    prefix: &mut prefix::LocatedAppAcceptance,
    key: &OperationKey,
    keys: &WorkHeadLocators,
    parties: &super::super::super::acceptance::CurrentPlatformConsents,
    s: &mut OriginalAuditSession,
) -> Result<ObservedWorkHeads> {
    let storage = s
        .reserve_external_payload(
            mem::size_of::<ObservedWorkHeads>()
                + mem::size_of::<ObservedFamilyHead>()
                + mem::size_of::<Option<HeadLocator>>()
                + 2 * mem::size_of::<Vec<AuditedBytes>>(),
        )
        .map_err(core_error)?;
    let family_count = keys.family_keys().len();
    let app_count = keys.app_decision_ids().len();
    let family_bytes = product(family_count, mem::size_of::<ObservedFamilyHead>(), s)?;
    let family_owner = s
        .reserve_external_payload(family_bytes)
        .map_err(core_error)?;
    let app_bytes = product(app_count, mem::size_of::<Option<HeadLocator>>(), s)?;
    let app_owner = s.reserve_external_payload(app_bytes).map_err(core_error)?;
    let financial_count = add(add(4, product(family_count, 2, s)?, s)?, app_count, s)?;
    let party_count = parties.mappings().len();
    let party_seeds = product(party_count, 3, s)?;
    let seed_count = add(financial_count, party_seeds, s)?;
    let seed_bytes = product(seed_count, mem::size_of::<AuditedBytes>(), s)?;
    let _seed_owner = s.reserve_external_payload(seed_bytes).map_err(core_error)?;
    let _pin_owner = s.reserve_external_payload(seed_bytes).map_err(core_error)?;
    let mut pins = Vec::new();
    pins.try_reserve_exact(seed_count)
        .map_err(|_| ServiceError::Unavailable)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(seed_count)
        .map_err(|_| ServiceError::Unavailable)?;
    let claim = tx
        .workflow_work_claim(keys.work_claim_key(), s)
        .await
        .map_err(prefix::read_error)?;
    if let Some(claim) = &claim {
        check::same_text(text(claim.kind.bytes(), s)?, "workflow-work-claim", s)?;
        let r = reference(
            claim.kind.bytes(),
            claim.id.bytes(),
            claim.digest.bytes(),
            s,
        )?;
        prefix.require_held_reference(r.bytes(), "workflow-work-claim", s)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<AuditedBytes>())
            .map_err(core_error)?;
        rows.push(r);
    }
    let root = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Decision(keys.root_decision_id()),
            s,
        )
        .await
        .map_err(prefix::read_error)?;
    retain_head(
        prefix,
        &root,
        "workflow-decision",
        &mut rows,
        &mut pins,
        "decision",
        s,
    )?;
    let capacity = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Capacity(keys.root_decision_id()),
            s,
        )
        .await
        .map_err(prefix::read_error)?;
    retain_head(
        prefix,
        &capacity,
        "workflow-correction-capacity",
        &mut rows,
        &mut pins,
        "capacity",
        s,
    )?;
    let FinancialHeadKey::Chain {
        logical_store_id,
        scope,
        customer,
        chain,
    } = keys.chain_key()
    else {
        return Err(ServiceError::IntegrityFailure);
    };
    for (actual, expected) in [
        (logical_store_id.as_str(), key.logical_store_id.as_str()),
        (scope[0].as_str(), key.scope[0].as_str()),
        (scope[1].as_str(), key.scope[1].as_str()),
    ] {
        check::same_text(actual, expected, s)?;
    }
    let chain = tx
        .workflow_financial_head(
            &key.logical_store_id,
            &key.scope,
            HeadKey::Chain { customer, chain },
            s,
        )
        .await
        .map_err(prefix::read_error)?;
    retain_head(
        prefix,
        &chain,
        "workflow-chain-state",
        &mut rows,
        &mut pins,
        "chain",
        s,
    )?;
    let mut families = Vec::new();
    families
        .try_reserve_exact(family_count)
        .map_err(|_| ServiceError::Unavailable)?;
    for locator in keys.family_keys() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let family = tx
            .workflow_family_head_by_target_identity(
                &key.logical_store_id,
                &key.scope,
                locator.target_id(),
                locator.family_slot(),
                s,
            )
            .await
            .map_err(prefix::read_error)?;
        if let Some(family) = &family {
            let target = reference(
                family.target_kind.bytes(),
                family.target_id.bytes(),
                family.target_digest.bytes(),
                s,
            )?;
            prefix.require_held_reference(target.bytes(), "workflow-target", s)?;
            check::same_text(
                text(family.head.kind.bytes(), s)?,
                "workflow-family-state",
                s,
            )?;
            let r = reference(
                family.head.kind.bytes(),
                family.head.id.bytes(),
                family.head.digest.bytes(),
                s,
            )?;
            prefix.require_held_reference(r.bytes(), "workflow-family-state", s)?;
            let encoded_pin = pin(&family.head, "family", r.bytes(), s)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<AuditedBytes>())
                .map_err(core_error)?;
            pins.push(encoded_pin);
            s.semantic_work()
                .write_bytes(mem::size_of::<AuditedBytes>())
                .map_err(core_error)?;
            rows.push(r);
        }
        let decision = tx
            .workflow_financial_head(
                &key.logical_store_id,
                &key.scope,
                HeadKey::Decision(locator.decision_id()),
                s,
            )
            .await
            .map_err(prefix::read_error)?;
        retain_head(
            prefix,
            &decision,
            "workflow-decision",
            &mut rows,
            &mut pins,
            "decision",
            s,
        )?;
        s.semantic_work()
            .write_bytes(mem::size_of::<ObservedFamilyHead>())
            .map_err(core_error)?;
        families.push(ObservedFamilyHead { family, decision });
    }
    let mut app_decisions = Vec::new();
    app_decisions
        .try_reserve_exact(app_count)
        .map_err(|_| ServiceError::Unavailable)?;
    for id in keys.app_decision_ids() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        let head = tx
            .workflow_financial_head(&key.logical_store_id, &key.scope, HeadKey::Decision(id), s)
            .await
            .map_err(prefix::read_error)?;
        retain_head(
            prefix,
            &head,
            "workflow-decision",
            &mut rows,
            &mut pins,
            "decision",
            s,
        )?;
        s.semantic_work()
            .write_bytes(mem::size_of::<Option<HeadLocator>>())
            .map_err(core_error)?;
        app_decisions.push(head);
    }
    for mapping in parties.mappings() {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        // Current mapping and its real earlier evidence are independent of the
        // historical verification's saved party-head fields.
        for reference in std::iter::once(mapping.reference()).chain(mapping.evidence_refs()) {
            s.semantic_work()
                .visit(ValidationVisit::Element)
                .map_err(core_error)?;
            let row = wire::reference(reference, s)?;
            prefix.require_held_reference(row.bytes(), &reference.kind, s)?;
            let mut present = false;
            for old in &rows {
                s.semantic_work()
                    .visit(ValidationVisit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(old.bytes().len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(row.bytes().len())
                    .map_err(core_error)?;
                if old.bytes() == row.bytes() {
                    present = true;
                    break;
                }
            }
            if !present {
                s.semantic_work()
                    .write_bytes(mem::size_of::<AuditedBytes>())
                    .map_err(core_error)?;
                rows.push(row);
            }
        }
    }
    let seeds = wire::encoded_set(&mut rows, s)?;
    let pins = wire::encoded_set(&mut pins, s)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<ObservedWorkHeads>())
        .map_err(core_error)?;
    Ok(ObservedWorkHeads {
        claim,
        root,
        capacity,
        chain,
        families,
        app_decisions,
        seeds,
        pins,
        _storage: storage,
        _families: family_owner,
        _apps: app_owner,
    })
}
