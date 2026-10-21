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
    pub(super) _pins: AuditedBytes,
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
        _pins: pins,
        _storage: storage,
        _families: family_owner,
        _apps: app_owner,
    })
}

/// Owning typed copies of the actual held projections for the unchanged core
/// DATA ABI. This is neither projection completeness nor candidate eligibility.
pub(super) struct OwnedWorkState {
    pub(super) financial: ledgerlab_core::policy::workflow::transition_constructor::DraftStateData,
    pub(super) capacities:
        Vec<ledgerlab_core::policy::workflow::transition_constructor::CapacityHead>,
    pub(super) parties:
        Vec<ledgerlab_core::policy::workflow::transition_constructor::PartyAuthorizationHead>,
    _storage: ExternalBufferLease,
}
fn copied(value: &str, s: &mut OriginalAuditSession) -> Result<String> {
    s.semantic_work()
        .scan_bytes(value.len())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(value.len())
        .map_err(core_error)?;
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| ServiceError::Unavailable)?;
    output.push_str(value);
    Ok(output)
}
fn copied_ref(head: &HeadLocator, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    let kind = copied(text(head.kind.bytes(), s)?, s)?;
    let id = copied(text(head.id.bytes(), s)?, s)?;
    let digest = copied(text(head.digest.bytes(), s)?, s)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<ExactRef>())
        .map_err(core_error)?;
    Ok(ExactRef { kind, id, digest })
}
fn copied_exact(reference: &ExactRef, s: &mut OriginalAuditSession) -> Result<ExactRef> {
    let kind = copied(&reference.kind, s)?;
    let id = copied(&reference.id, s)?;
    let digest = copied(&reference.digest, s)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<ExactRef>())
        .map_err(core_error)?;
    Ok(ExactRef { kind, id, digest })
}
fn output_ref_bytes(
    bytes: &mut usize,
    parts: &[&[u8]],
    s: &mut OriginalAuditSession,
) -> Result<()> {
    for part in parts {
        s.semantic_work()
            .visit(ValidationVisit::Element)
            .map_err(core_error)?;
        *bytes = add(*bytes, part.len(), s)?;
    }
    Ok(())
}
impl ObservedWorkHeads {
    pub(super) fn typed_state(
        &self,
        keys: &WorkHeadLocators,
        parties: &super::super::super::acceptance::CurrentPlatformConsents,
        s: &mut OriginalAuditSession,
    ) -> Result<OwnedWorkState> {
        use ledgerlab_core::policy::workflow::transition_constructor::{
            CapacityHead, DecisionHead, DraftStateData, FamilyHead, PartyAuthorizationHead,
        };
        let result = (|| {
            let family_count = keys.family_keys().len();
            let app_count = keys.app_decision_ids().len();
            s.semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&family_count.into(), &self.families.len().into()],
                )
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&app_count.into(), &self.app_decisions.len().into()],
                )
                .map_err(core_error)?;
            require(
                family_count == self.families.len() && app_count == self.app_decisions.len(),
                "INTEGRITY_FAILURE",
            )?;
            let decisions = add(add(1, family_count, s)?, app_count, s)?;
            let party_count = parties.mappings().len();
            let mut bytes = mem::size_of::<OwnedWorkState>()
                + mem::size_of::<FamilyHead>()
                + mem::size_of::<DecisionHead>()
                + mem::size_of::<CapacityHead>()
                + mem::size_of::<PartyAuthorizationHead>()
                + 2 * mem::size_of::<ExactRef>();
            for (count, stride) in [
                (decisions, mem::size_of::<DecisionHead>()),
                (family_count, mem::size_of::<FamilyHead>()),
                (1, mem::size_of::<CapacityHead>()),
                (1, mem::size_of::<ExactRef>()),
                (party_count, mem::size_of::<PartyAuthorizationHead>()),
            ] {
                bytes = add(bytes, product(count, stride, s)?, s)?;
            }
            for head in [&self.root, &self.capacity, &self.chain]
                .into_iter()
                .chain(self.families.iter().map(|f| &f.decision))
                .chain(self.app_decisions.iter())
            {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                if let Some(head) = head {
                    output_ref_bytes(
                        &mut bytes,
                        &[
                            head.kind.bytes(),
                            head.id.bytes(),
                            head.digest.bytes(),
                            head.revision.bytes(),
                        ],
                        s,
                    )?;
                }
            }
            if let Some(claim) = &self.claim {
                output_ref_bytes(
                    &mut bytes,
                    &[claim.kind.bytes(), claim.id.bytes(), claim.digest.bytes()],
                    s,
                )?;
            }
            for family in &self.families {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                if let Some(family) = &family.family {
                    output_ref_bytes(
                        &mut bytes,
                        &[
                            family.target_kind.bytes(),
                            family.target_id.bytes(),
                            family.target_digest.bytes(),
                            family.head.kind.bytes(),
                            family.head.id.bytes(),
                            family.head.digest.bytes(),
                            family.head.revision.bytes(),
                        ],
                        s,
                    )?;
                }
            }
            // Stable key strings are copied into their typed output slots only
            // for actual hits. Reserve their finite maximum before any copy.
            bytes = add(bytes, product(keys.root_decision_id().len(), 2, s)?, s)?;
            for family in keys.family_keys() {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                output_ref_bytes(
                    &mut bytes,
                    &[
                        family.family_slot().as_bytes(),
                        family.decision_id().as_bytes(),
                    ],
                    s,
                )?;
            }
            for id in keys.app_decision_ids() {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                bytes = add(bytes, id.len(), s)?;
            }
            for mapping in parties.mappings() {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                let r = mapping.reference();
                let authorization = check::field(mapping.body(), "authorization", s)?;
                let id = check::text(authorization, "authorization_id", s)?;
                output_ref_bytes(
                    &mut bytes,
                    &[
                        r.kind.as_bytes(),
                        r.id.as_bytes(),
                        r.digest.as_bytes(),
                        mapping.current_head().revision.bytes(),
                        id.as_bytes(),
                    ],
                    s,
                )?;
            }
            let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
            // Prepay all reserved output/control writes before the first Vec
            // or String producer, including a later refusal prefix.
            s.semantic_work().write_bytes(bytes).map_err(core_error)?;
            let mut financial = DraftStateData::default();
            financial
                .decision_heads
                .try_reserve_exact(decisions)
                .map_err(|_| ServiceError::Unavailable)?;
            financial
                .family_heads
                .try_reserve_exact(family_count)
                .map_err(|_| ServiceError::Unavailable)?;
            financial
                .work_claims
                .try_reserve_exact(1)
                .map_err(|_| ServiceError::Unavailable)?;
            let mut capacities = Vec::new();
            capacities
                .try_reserve_exact(1)
                .map_err(|_| ServiceError::Unavailable)?;
            let mut current_parties: Vec<PartyAuthorizationHead> = Vec::new();
            current_parties
                .try_reserve_exact(party_count)
                .map_err(|_| ServiceError::Unavailable)?;
            if let Some(claim) = &self.claim {
                let reference = ExactRef {
                    kind: copied(text(claim.kind.bytes(), s)?, s)?,
                    id: copied(text(claim.id.bytes(), s)?, s)?,
                    digest: copied(text(claim.digest.bytes(), s)?, s)?,
                };
                s.semantic_work()
                    .write_bytes(mem::size_of::<ExactRef>())
                    .map_err(core_error)?;
                financial.work_claims.push(reference);
            }
            let mut decision = |head: &Option<HeadLocator>,
                                id: &str,
                                s: &mut OriginalAuditSession|
             -> Result<()> {
                s.semantic_work()
                    .visit(ValidationVisit::Constraint)
                    .map_err(core_error)?;
                if let Some(head) = head {
                    let reference = copied_ref(head, s)?;
                    let revision = copied(text(head.revision.bytes(), s)?, s)?;
                    let decision_id = copied(id, s)?;
                    s.semantic_work()
                        .write_bytes(mem::size_of::<DecisionHead>())
                        .map_err(core_error)?;
                    financial.decision_heads.push(DecisionHead {
                        decision_id,
                        revision,
                        reference,
                    });
                }
                Ok(())
            };
            decision(&self.root, keys.root_decision_id(), s)?;
            if let Some(head) = &self.capacity {
                let reference = copied_ref(head, s)?;
                let revision = copied(text(head.revision.bytes(), s)?, s)?;
                let root_decision_id = copied(keys.root_decision_id(), s)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<CapacityHead>())
                    .map_err(core_error)?;
                capacities.push(CapacityHead {
                    root_decision_id,
                    revision,
                    reference,
                });
            }
            if let Some(head) = &self.chain {
                financial.chain_head = Some(copied_ref(head, s)?);
            }
            for (family, key) in self.families.iter().zip(keys.family_keys()) {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                decision(&family.decision, key.decision_id(), s)?;
                if let Some(family) = &family.family {
                    let target_ref = ExactRef {
                        kind: copied(text(family.target_kind.bytes(), s)?, s)?,
                        id: copied(text(family.target_id.bytes(), s)?, s)?,
                        digest: copied(text(family.target_digest.bytes(), s)?, s)?,
                    };
                    let reference = copied_ref(&family.head, s)?;
                    let revision = copied(text(family.head.revision.bytes(), s)?, s)?;
                    let family_slot = copied(key.family_slot(), s)?;
                    s.semantic_work()
                        .write_bytes(mem::size_of::<FamilyHead>())
                        .map_err(core_error)?;
                    financial.family_heads.push(FamilyHead {
                        target_ref,
                        family_slot,
                        revision,
                        reference,
                    });
                }
            }
            for (head, id) in self.app_decisions.iter().zip(keys.app_decision_ids()) {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                decision(head, id, s)?;
            }
            for mapping in parties.mappings() {
                s.semantic_work()
                    .visit(ValidationVisit::Element)
                    .map_err(core_error)?;
                let reference = mapping.reference();
                let mut duplicate = false;
                for old in &current_parties {
                    s.semantic_work()
                        .visit(ValidationVisit::Element)
                        .map_err(core_error)?;
                    duplicate = work_capture::same_reference(&old.reference, reference, s)?;
                    if duplicate {
                        break;
                    }
                }
                if duplicate {
                    continue;
                }
                let reference = copied_exact(reference, s)?;
                let authorization = check::field(mapping.body(), "authorization", s)?;
                let authorization_id =
                    copied(check::text(authorization, "authorization_id", s)?, s)?;
                let revision = copied(text(mapping.current_head().revision.bytes(), s)?, s)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<PartyAuthorizationHead>())
                    .map_err(core_error)?;
                current_parties.push(PartyAuthorizationHead {
                    authorization_id,
                    revision,
                    reference,
                });
            }
            s.semantic_work()
                .write_bytes(mem::size_of::<OwnedWorkState>())
                .map_err(core_error)?;
            Ok(OwnedWorkState {
                financial,
                capacities,
                parties: current_parties,
                _storage: storage,
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}

/// No work exists for a pre-work assertion. Only actual current party heads
/// are represented; absence of financial work/head keys is structural here.
pub(super) fn unattached_state(
    parties: &super::super::super::acceptance::CurrentPlatformConsents,
    s: &mut OriginalAuditSession,
) -> Result<OwnedWorkState> {
    use ledgerlab_core::policy::workflow::transition_constructor::{
        DraftStateData, PartyAuthorizationHead,
    };
    let mut bytes = mem::size_of::<OwnedWorkState>()
        + parties.mappings().len() * mem::size_of::<PartyAuthorizationHead>();
    for m in parties.mappings() {
        let a = check::field(m.body(), "authorization", s)?;
        bytes += check::text(a, "authorization_id", s)?.len()
            + m.current_head().revision.bytes().len()
            + m.reference().kind.len()
            + m.reference().id.len()
            + m.reference().digest.len();
    }
    let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut current = Vec::with_capacity(parties.mappings().len());
    for m in parties.mappings() {
        let a = check::field(m.body(), "authorization", s)?;
        let reference = copied_exact(m.reference(), s)?;
        current.push(PartyAuthorizationHead {
            authorization_id: copied(check::text(a, "authorization_id", s)?, s)?,
            revision: copied(text(m.current_head().revision.bytes(), s)?, s)?,
            reference,
        });
    }
    Ok(OwnedWorkState {
        financial: DraftStateData::default(),
        capacities: Vec::new(),
        parties: current,
        _storage: storage,
    })
}
