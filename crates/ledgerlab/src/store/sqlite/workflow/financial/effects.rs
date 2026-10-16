//! Mechanical CAS and permanent indices for a complete core candidate.
//! A scoped committed journal/member association is necessary, never authority.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    DependencyKind, EconomicCandidate, ExactRef, FinancialHeadKey, HeadEffect, PermanentClaimKey,
};

fn key_parts<'a>(
    key: &'a FinancialHeadKey,
    candidate: &'a EconomicCandidate,
    session: &mut OriginalAuditSession,
) -> Result<(&'static str, [&'a str; 4]), FinancialReadError> {
    Ok(match key {
        FinancialHeadKey::Invocation { invocation_id } => {
            ("invocation", [invocation_id, "", "", ""])
        }
        FinancialHeadKey::Cost { cost_claim_ref: r } => ("cost", [&r.kind, &r.id, &r.digest, ""]),
        FinancialHeadKey::CostConsumption { cost_claim_ref: r } => {
            ("cost_consumption", [&r.kind, &r.id, &r.digest, ""])
        }
        FinancialHeadKey::CostOwner { .. } => {
            return Err(StoreError::Integrity("global supplier owner used as scoped key").into());
        }
        FinancialHeadKey::Decision { decision_id } => ("decision", [decision_id, "", "", ""]),
        FinancialHeadKey::Capacity { root_decision_id } => {
            ("capacity", [root_decision_id, "", "", ""])
        }
        FinancialHeadKey::Family {
            target_ref,
            family_slot,
        } => (
            "family",
            [
                &target_ref.kind,
                &target_ref.id,
                &target_ref.digest,
                family_slot,
            ],
        ),
        FinancialHeadKey::Chain {
            logical_store_id,
            scope,
            customer,
            chain,
        } => {
            if !same_text(logical_store_id, &candidate.key().logical_store_id, session)?
                || !same_text(&scope[0], &candidate.key().scope[0], session)?
                || !same_text(&scope[1], &candidate.key().scope[1], session)?
            {
                return Err(StoreError::Integrity("financial chain scope mismatch").into());
            }
            ("chain", [customer, chain, "", ""])
        }
    })
}
/// No numeric formatting allocation: the accepted revision language is the
/// same nonzero canonical u64 decimal, with every executed digit operation paid
/// before it runs under the caller's original-audit owner.
pub(super) fn revision(
    text: &str,
    session: &mut OriginalAuditSession,
) -> Result<u64, FinancialReadError> {
    let work = session.semantic_work();
    work.visit(ValidationVisit::Constraint)?;
    work.arithmetic(
        ArithmeticOperation::Compare,
        &[&text.len().into(), &20usize.into()],
    )?;
    if text.is_empty() || text.len() > 20 {
        return Err(StoreError::Integrity("financial revision").into());
    }
    work.scan_bytes(text.len())?;
    work.visit(ValidationVisit::Constraint)?;
    if text.as_bytes()[0] == b'0' {
        return Err(StoreError::Integrity("noncanonical financial revision").into());
    }
    let mut value = 0u64;
    for byte in text.bytes() {
        work.visit(ValidationVisit::ScalarDigit)?;
        work.visit(ValidationVisit::Constraint)?;
        if !byte.is_ascii_digit() {
            return Err(StoreError::Integrity("financial revision").into());
        }
        work.arithmetic(ArithmeticOperation::Subtract, &[&byte.into(), &b'0'.into()])?;
        let digit = byte - b'0';
        work.arithmetic(
            ArithmeticOperation::Multiply,
            &[&value.into(), &10u64.into()],
        )?;
        value = value
            .checked_mul(10)
            .ok_or_else(|| StoreError::Integrity("financial revision"))?;
        work.arithmetic(ArithmeticOperation::Add, &[&value.into(), &digit.into()])?;
        value = value
            .checked_add(u64::from(digit))
            .ok_or_else(|| StoreError::Integrity("financial revision"))?;
    }
    Ok(value)
}
pub(super) fn same_text(
    left: &str,
    right: &str,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    session
        .semantic_work()
        .visit(ValidationVisit::MemberCompare)?;
    // Conservative whole-pass reservation, not an observed short-read count.
    session.semantic_work().scan_bytes(left.len())?;
    session.semantic_work().scan_bytes(right.len())?;
    Ok(left == right)
}
pub(super) fn same_ref(
    left: &ExactRef,
    right: &ExactRef,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    Ok(same_text(&left.kind, &right.kind, session)?
        && same_text(&left.id, &right.id, session)?
        && same_text(&left.digest, &right.digest, session)?)
}
pub(super) fn next_revision(
    old: &str,
    next: u64,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    let old = revision(old, session)?;
    session
        .semantic_work()
        .arithmetic(ArithmeticOperation::Add, &[&old.into(), &1u64.into()])?;
    let incremented = old.checked_add(1);
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    if let Some(incremented) = incremented {
        session.semantic_work().arithmetic(
            ArithmeticOperation::Compare,
            &[&incremented.into(), &next.into()],
        )?;
        Ok(incremented == next)
    } else {
        Ok(false)
    }
}
pub(super) fn head_text<'a>(
    bytes: &'a AuditedBytes,
    session: &mut OriginalAuditSession,
) -> Result<&'a str, FinancialReadError> {
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    session.semantic_work().scan_bytes(bytes.bytes().len())?;
    std::str::from_utf8(bytes.bytes())
        .map_err(|_| StoreError::Integrity("financial head text").into())
}
impl SqliteTx {
    pub(super) async fn financial_member(
        &mut self,
        identity: &[u8],
        r: &ExactRef,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let bound = [
            identity,
            r.kind.as_bytes(),
            r.id.as_bytes(),
            r.digest.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_documents d ON d.kind=m.kind AND d.id=m.id AND d.digest=m.digest WHERE m.identity_key=? AND m.kind=? AND m.id=? AND m.digest=?)")
            .bind(identity).bind(&r.kind).bind(&r.id).bind(&r.digest).fetch_one(self.conn()).await?;
        self.financial_finish_io(session).await?;
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if !present {
            return Err(StoreError::Integrity(
                "financial projection outside complete journal members",
            )
            .into());
        }
        Ok(())
    }
    async fn advance_financial_head(
        &mut self,
        candidate: &EconomicCandidate,
        effect: &HeadEffect,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if let FinancialHeadKey::CostOwner { line_key } = effect.key() {
            return self
                .advance_supplier_owner(candidate, effect, line_key, session)
                .await;
        }
        let key = candidate.key();
        let (kind, parts) = key_parts(effect.key(), candidate, session)?;
        let next = revision(effect.revision(), session)?;
        let proposed = effect.proposed();
        let expected_kind = match kind {
            "decision" => "workflow-decision",
            "family" => "workflow-family-state",
            "chain" => "workflow-chain-state",
            "capacity" => "workflow-correction-capacity",
            "invocation" => "workflow-invocation",
            "cost" | "cost_consumption" => "workflow-cost",
            _ => return Err(StoreError::Integrity("financial head kind").into()),
        };
        let position = effect.introduced_position();
        session.semantic_work().arithmetic(
            ArithmeticOperation::Compare,
            &[&position.into(), &candidate.introduced().len().into()],
        )?;
        let document = candidate
            .introduced()
            .get(position)
            .ok_or_else(|| StoreError::Integrity("financial head introduced position"))?;
        if !same_text(&proposed.kind, expected_kind, session)?
            || !same_ref(document.reference(), proposed, session)?
        {
            return Err(
                StoreError::Integrity("financial head not an introduced canonical record").into(),
            );
        }
        let locator = match effect.key() {
            FinancialHeadKey::Invocation { invocation_id } => HeadKey::Invocation(invocation_id),
            FinancialHeadKey::Cost { cost_claim_ref } => HeadKey::Cost(cost_claim_ref),
            FinancialHeadKey::CostConsumption { cost_claim_ref } => {
                HeadKey::CostConsumption(cost_claim_ref)
            }
            FinancialHeadKey::CostOwner { .. } => {
                return Err(StoreError::Integrity("global owner routed as scoped").into());
            }
            FinancialHeadKey::Decision { decision_id } => HeadKey::Decision(decision_id),
            FinancialHeadKey::Capacity { root_decision_id } => HeadKey::Capacity(root_decision_id),
            FinancialHeadKey::Family {
                target_ref,
                family_slot,
            } => HeadKey::Family {
                target_kind: &target_ref.kind,
                target_id: &target_ref.id,
                target_digest: &target_ref.digest,
                slot: family_slot,
            },
            FinancialHeadKey::Chain {
                customer, chain, ..
            } => HeadKey::Chain { customer, chain },
        };
        let old = self
            .financial_head_during_append(&key.logical_store_id, &key.scope, locator, session)
            .await?;
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        session
            .semantic_work()
            .arithmetic(ArithmeticOperation::Compare, &[&next.into(), &1u64.into()])?;
        match (old, effect.expected()) {
            (None, None) if next == 1 => {
                let bound = [
                    key.logical_store_id.as_bytes(),
                    key.scope[0].as_bytes(),
                    key.scope[1].as_bytes(),
                    kind.as_bytes(),
                    parts[0].as_bytes(),
                    parts[1].as_bytes(),
                    parts[2].as_bytes(),
                    parts[3].as_bytes(),
                    effect.revision().as_bytes(),
                    proposed.kind.as_bytes(),
                    proposed.id.as_bytes(),
                    proposed.digest.as_bytes(),
                ];
                let payload = io::binding_payload_sets(&bound, &[&bound, &bound], 2, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                sqlx::query("INSERT INTO workflow_financial_heads VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
                    .bind(&key.logical_store_id)
                    .bind(&key.scope[0])
                    .bind(&key.scope[1])
                    .bind(kind)
                    .bind(parts[0])
                    .bind(parts[1])
                    .bind(parts[2])
                    .bind(parts[3])
                    .bind(effect.revision())
                    .bind(&proposed.kind)
                    .bind(&proposed.id)
                    .bind(&proposed.digest)
                    .execute(self.conn())
                    .await?;
                self.financial_finish_io(session).await?;
            }
            (Some(old), Some(expected)) => {
                let old_revision = head_text(&old.revision, session)?;
                let old_kind = head_text(&old.kind, session)?;
                let old_id = head_text(&old.id, session)?;
                let old_digest = head_text(&old.digest, session)?;
                if !same_text(old_kind, &expected.kind, session)?
                    || !same_text(old_id, &expected.id, session)?
                    || !same_text(old_digest, &expected.digest, session)?
                    || !next_revision(old_revision, next, session)?
                {
                    return Err(StoreError::ExpectedCurrent.into());
                }
                let bound = [
                    effect.revision().as_bytes(),
                    proposed.kind.as_bytes(),
                    proposed.id.as_bytes(),
                    proposed.digest.as_bytes(),
                    key.logical_store_id.as_bytes(),
                    key.scope[0].as_bytes(),
                    key.scope[1].as_bytes(),
                    kind.as_bytes(),
                    parts[0].as_bytes(),
                    parts[1].as_bytes(),
                    parts[2].as_bytes(),
                    parts[3].as_bytes(),
                    old_revision.as_bytes(),
                    expected.kind.as_bytes(),
                    expected.id.as_bytes(),
                    expected.digest.as_bytes(),
                ];
                let payload = io::binding_payload_sets(
                    &bound,
                    &[&bound, &bound],
                    2,
                    2 * (8 * 128 + 243),
                    session,
                )?;
                self.financial_reserve_io(session, payload)?;
                let changed=sqlx::query("UPDATE workflow_financial_heads SET revision=?,record_kind=?,record_id=?,record_digest=? WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=? AND revision=? AND record_kind=? AND record_id=? AND record_digest=?")
                    .bind(effect.revision()).bind(&proposed.kind).bind(&proposed.id).bind(&proposed.digest)
                    .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1]).bind(kind)
                    .bind(parts[0]).bind(parts[1]).bind(parts[2]).bind(parts[3])
                    .bind(old_revision).bind(&expected.kind).bind(&expected.id).bind(&expected.digest)
                    .execute(self.conn()).await?.rows_affected();
                self.financial_finish_io(session).await?;
                session.semantic_work().arithmetic(
                    ArithmeticOperation::Compare,
                    &[&changed.into(), &1u64.into()],
                )?;
                if changed != 1 {
                    return Err(StoreError::ExpectedCurrent.into());
                }
            }
            _ => return Err(StoreError::ExpectedCurrent.into()),
        }
        Ok(())
    }
    /// Called only inside the poisoned financial append, after the complete
    /// operation/members have been inserted and before its single commit. It
    /// does not clear the caller's poison bit or provide an independent append.
    pub(super) async fn apply_financial_effects(
        &mut self,
        candidate: &EconomicCandidate,
        identity: &[u8],
        sequence: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if !self.failed {
            return Err(StoreError::Integrity("financial effects outside poisoned append").into());
        }
        let key = candidate.key();
        let bound = [
            identity,
            key.logical_store_id.as_bytes(),
            key.scope[0].as_bytes(),
            key.scope[1].as_bytes(),
            key.source.as_bytes(),
            key.operation_id.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let matched: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=? AND source=? AND operation_id=? AND commit_sequence=? AND operation_kind IN ('work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert','authority.change','evidence.retain','invocation.authorize','invocation.close','cost.finalize','cost.correct'))")
            .bind(identity).bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
            .bind(&key.source).bind(&key.operation_id).bind(sequence).fetch_one(self.conn()).await?;
        self.financial_finish_io(session).await?;
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if !matched {
            return Err(
                StoreError::Integrity("financial effects missing exact journal operation").into(),
            );
        }
        for head in candidate.heads() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            self.financial_member(identity, head.proposed(), session)
                .await?;
            self.advance_financial_head(candidate, head, session)
                .await?;
        }
        for claim in candidate.claims() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let r = claim.record_ref();
            self.financial_member(identity, r, session).await?;
            match claim.key() {
                PermanentClaimKey::CostLine { line_key } => {
                    self.insert_supplier_line_claim(identity, line_key, r, session)
                        .await?
                }
                PermanentClaimKey::CostComponent {
                    scope,
                    original_work_claim_id,
                    supplier_component_slot,
                } => {
                    self.insert_supplier_component_claim(
                        candidate,
                        scope,
                        original_work_claim_id,
                        supplier_component_slot,
                        r,
                        session,
                    )
                    .await?
                }
                PermanentClaimKey::Work {
                    logical_store_id,
                    scope,
                    source_namespace,
                    business_operation_id,
                    completion,
                } => {
                    if !same_text(logical_store_id, &key.logical_store_id, session)?
                        || !same_text(&scope[0], &key.scope[0], session)?
                        || !same_text(&scope[1], &key.scope[1], session)?
                        || !same_text(&r.kind, "workflow-work-claim", session)?
                    {
                        return Err(StoreError::Integrity("financial work claim scope/kind").into());
                    }
                    let bound = [
                        key.logical_store_id.as_bytes(),
                        key.scope[0].as_bytes(),
                        key.scope[1].as_bytes(),
                        source_namespace.as_bytes(),
                        business_operation_id.as_bytes(),
                        completion.as_bytes(),
                        r.kind.as_bytes(),
                        r.id.as_bytes(),
                        r.digest.as_bytes(),
                    ];
                    let payload =
                        io::binding_payload_sets(&bound, &[&bound, &bound, &bound], 3, 0, session)?;
                    self.financial_reserve_io(session, payload)?;
                    sqlx::query(
                        "INSERT INTO workflow_financial_work_claims VALUES(?,?,?,?,?,?,?,?,?)",
                    )
                    .bind(&key.logical_store_id)
                    .bind(&key.scope[0])
                    .bind(&key.scope[1])
                    .bind(source_namespace)
                    .bind(business_operation_id)
                    .bind(completion)
                    .bind(&r.kind)
                    .bind(&r.id)
                    .bind(&r.digest)
                    .execute(self.conn())
                    .await?;
                    self.financial_finish_io(session).await?;
                }
                PermanentClaimKey::Outcome {
                    scope,
                    agreement_id,
                    family_slot,
                    target_id,
                    source_namespace,
                    business_outcome_token,
                } => {
                    if !same_text(&scope[0], &key.scope[0], session)?
                        || !same_text(&scope[1], &key.scope[1], session)?
                        || !same_text(&r.kind, "workflow-outcome-claim", session)?
                    {
                        return Err(
                            StoreError::Integrity("financial outcome claim scope/kind").into()
                        );
                    }
                    let bound = [
                        key.logical_store_id.as_bytes(),
                        key.scope[0].as_bytes(),
                        key.scope[1].as_bytes(),
                        agreement_id.as_bytes(),
                        family_slot.as_bytes(),
                        target_id.as_bytes(),
                        source_namespace.as_bytes(),
                        business_outcome_token.as_bytes(),
                        r.kind.as_bytes(),
                        r.id.as_bytes(),
                        r.digest.as_bytes(),
                    ];
                    let payload = io::binding_payload_sets(
                        &bound,
                        &[&bound, &bound, &bound, &bound],
                        4,
                        0,
                        session,
                    )?;
                    self.financial_reserve_io(session, payload)?;
                    sqlx::query("INSERT INTO workflow_financial_outcome_claims VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                        .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                        .bind(agreement_id).bind(family_slot).bind(target_id).bind(source_namespace).bind(business_outcome_token)
                        .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
                    self.financial_finish_io(session).await?;
                }
            }
        }
        for enrollment in candidate.account_enrollments() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            self.insert_supplier_account_enrollment(identity, enrollment, session)
                .await?;
        }
        for link in candidate.links() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            if !same_text(&link.scope()[0], &key.scope[0], session)?
                || !same_text(&link.scope()[1], &key.scope[1], session)?
            {
                return Err(StoreError::Integrity("financial link scope").into());
            }
            let r = link.record_ref();
            let child = link.child_ref();
            let predecessor = link.predecessor_ref();
            let product = link.product_ref();
            let binding = link.binding_ref();
            for member in [r, child, predecessor, product, binding] {
                self.financial_member(identity, member, session).await?;
            }
            let bound = [
                key.logical_store_id.as_bytes(),
                key.scope[0].as_bytes(),
                key.scope[1].as_bytes(),
                child.kind.as_bytes(),
                child.id.as_bytes(),
                child.digest.as_bytes(),
                link.slot().as_bytes(),
                predecessor.kind.as_bytes(),
                predecessor.id.as_bytes(),
                predecessor.digest.as_bytes(),
                product.kind.as_bytes(),
                product.id.as_bytes(),
                product.digest.as_bytes(),
                binding.kind.as_bytes(),
                binding.id.as_bytes(),
                binding.digest.as_bytes(),
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
            ];
            let payload =
                io::binding_payload_sets(&bound, &[&bound, &bound, &bound], 3, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            sqlx::query("INSERT INTO workflow_financial_links VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                .bind(&child.kind).bind(&child.id).bind(&child.digest).bind(link.slot())
                .bind(&predecessor.kind).bind(&predecessor.id).bind(&predecessor.digest)
                .bind(&product.kind).bind(&product.id).bind(&product.digest)
                .bind(&binding.kind).bind(&binding.id).bind(&binding.digest)
                .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
            self.financial_finish_io(session).await?;
        }
        for dependency in candidate.dependencies() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let r = dependency.record_ref();
            if !same_text(&r.kind, "workflow-dependency", session)? {
                return Err(StoreError::Integrity("financial dependency record kind").into());
            }
            let target = dependency.target_ref();
            let terms = dependency.original_terms_ref();
            let capacity = dependency.capacity_ref();
            for member in [r, target, terms, capacity] {
                self.financial_member(identity, member, session).await?;
            }
            let kind = match dependency.kind() {
                DependencyKind::OutcomeFamily => "outcome_family",
                DependencyKind::WorkBasis => "work_basis",
                DependencyKind::DelayedCost => "delayed_cost",
            };
            let bound = [
                key.logical_store_id.as_bytes(),
                key.scope[0].as_bytes(),
                key.scope[1].as_bytes(),
                dependency.ancestor_decision_id().as_bytes(),
                dependency.dependent_decision_id().as_bytes(),
                kind.as_bytes(),
                target.kind.as_bytes(),
                target.id.as_bytes(),
                target.digest.as_bytes(),
                dependency.family_slot().unwrap_or("").as_bytes(),
                terms.kind.as_bytes(),
                terms.id.as_bytes(),
                terms.digest.as_bytes(),
                capacity.kind.as_bytes(),
                capacity.id.as_bytes(),
                capacity.digest.as_bytes(),
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
            ];
            let payload =
                io::binding_payload_sets(&bound, &[&bound, &bound, &bound], 3, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            sqlx::query("INSERT INTO workflow_financial_dependencies VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                .bind(dependency.ancestor_decision_id()).bind(dependency.dependent_decision_id()).bind(kind)
                .bind(&target.kind).bind(&target.id).bind(&target.digest).bind(dependency.family_slot().unwrap_or(""))
                .bind(&terms.kind).bind(&terms.id).bind(&terms.digest)
                .bind(&capacity.kind).bind(&capacity.id).bind(&capacity.digest)
                .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
            self.financial_finish_io(session).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;

    #[test]
    fn locked_revision_checks_preserve_canonical_u64_and_overflow_refusal() {
        for value in ["1", "9", "10", "18446744073709551615"] {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert_eq!(
                revision(value, &mut session).unwrap(),
                value.parse::<u64>().unwrap()
            );
        }
        for value in [
            "",
            "0",
            "00",
            "+1",
            "01",
            "-1",
            "1\n",
            "18446744073709551616",
            "100000000000000000000",
        ] {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(revision(value, &mut session).is_err(), "{value:?}");
        }
        for (old, next, expected) in [
            ("1", 2, true),
            ("1", 3, false),
            ("18446744073709551614", u64::MAX, true),
            ("18446744073709551615", 1, false),
        ] {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert_eq!(next_revision(old, next, &mut session).unwrap(), expected);
        }
    }
}
