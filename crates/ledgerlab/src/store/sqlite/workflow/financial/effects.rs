//! Mechanical CAS and permanent indices for a complete core candidate.
//! A scoped committed journal/member association is necessary, never authority.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    DependencyKind, EconomicCandidate, ExactRef, FinancialHeadKey, HeadEffect, PermanentClaimKey,
};

fn key_parts<'a>(
    key: &'a FinancialHeadKey,
    candidate: &'a EconomicCandidate,
) -> Result<(&'static str, [&'a str; 4]), StoreError> {
    Ok(match key {
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
            if logical_store_id != &candidate.key().logical_store_id
                || scope != &candidate.key().scope
            {
                return Err(StoreError::Integrity("financial chain scope mismatch"));
            }
            ("chain", [customer, chain, "", ""])
        }
    })
}
fn revision(text: &str) -> Result<u64, StoreError> {
    let n = text
        .parse::<u64>()
        .map_err(|_| StoreError::Integrity("financial revision"))?;
    if n == 0 || n.to_string() != text {
        return Err(StoreError::Integrity("noncanonical financial revision"));
    }
    Ok(n)
}
impl SqliteTx {
    async fn financial_member(&mut self, identity: &[u8], r: &ExactRef) -> Result<(), StoreError> {
        let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_documents d ON d.kind=m.kind AND d.id=m.id AND d.digest=m.digest WHERE m.identity_key=? AND m.kind=? AND m.id=? AND m.digest=?)")
            .bind(identity).bind(&r.kind).bind(&r.id).bind(&r.digest).fetch_one(self.conn()).await?;
        if !present {
            return Err(StoreError::Integrity(
                "financial projection outside complete journal members",
            ));
        }
        Ok(())
    }
    async fn advance_financial_head(
        &mut self,
        candidate: &EconomicCandidate,
        effect: &HeadEffect,
    ) -> Result<(), StoreError> {
        let key = candidate.key();
        let (kind, parts) = key_parts(effect.key(), candidate)?;
        let next = revision(effect.revision())?;
        let proposed = effect.proposed();
        let expected_kind = match kind {
            "decision" => "workflow-decision",
            "family" => "workflow-family-state",
            "chain" => "workflow-chain-state",
            "capacity" => "workflow-correction-capacity",
            _ => return Err(StoreError::Integrity("financial head kind")),
        };
        if proposed.kind != expected_kind
            || !candidate
                .introduced()
                .iter()
                .any(|d| d.reference() == proposed)
        {
            return Err(StoreError::Integrity(
                "financial head not an introduced canonical record",
            ));
        }
        let old: Option<(String,String,String,String)>=sqlx::query_as("SELECT revision,record_kind,record_id,record_digest FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=?")
            .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1]).bind(kind)
            .bind(parts[0]).bind(parts[1]).bind(parts[2]).bind(parts[3]).fetch_optional(self.conn()).await?;
        match (old, effect.expected()) {
            (None, None) if next == 1 => {
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
            }
            (Some((old_revision, old_kind, old_id, old_digest)), Some(expected)) => {
                if old_kind != expected.kind
                    || old_id != expected.id
                    || old_digest != expected.digest
                    || revision(&old_revision)?.checked_add(1) != Some(next)
                {
                    return Err(StoreError::ExpectedCurrent);
                }
                let changed=sqlx::query("UPDATE workflow_financial_heads SET revision=?,record_kind=?,record_id=?,record_digest=? WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=? AND revision=? AND record_kind=? AND record_id=? AND record_digest=?")
                    .bind(effect.revision()).bind(&proposed.kind).bind(&proposed.id).bind(&proposed.digest)
                    .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1]).bind(kind)
                    .bind(parts[0]).bind(parts[1]).bind(parts[2]).bind(parts[3])
                    .bind(old_revision).bind(&expected.kind).bind(&expected.id).bind(&expected.digest)
                    .execute(self.conn()).await?.rows_affected();
                if changed != 1 {
                    return Err(StoreError::ExpectedCurrent);
                }
            }
            _ => return Err(StoreError::ExpectedCurrent),
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
    ) -> Result<(), StoreError> {
        if !self.failed {
            return Err(StoreError::Integrity(
                "financial effects outside poisoned append",
            ));
        }
        let key = candidate.key();
        let matched: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=? AND source=? AND operation_id=? AND commit_sequence=? AND operation_kind IN ('work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert'))")
            .bind(identity).bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
            .bind(&key.source).bind(&key.operation_id).bind(sequence).fetch_one(self.conn()).await?;
        if !matched {
            return Err(StoreError::Integrity(
                "financial effects missing exact journal operation",
            ));
        }
        for head in candidate.heads() {
            self.financial_member(identity, head.proposed()).await?;
            self.advance_financial_head(candidate, head).await?;
        }
        for claim in candidate.claims() {
            let r = claim.record_ref();
            self.financial_member(identity, r).await?;
            match claim.key() {
                PermanentClaimKey::Work {
                    logical_store_id,
                    scope,
                    source_namespace,
                    business_operation_id,
                    completion,
                } => {
                    if logical_store_id != &key.logical_store_id
                        || scope != &key.scope
                        || r.kind != "workflow-work-claim"
                    {
                        return Err(StoreError::Integrity("financial work claim scope/kind"));
                    }
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
                }
                PermanentClaimKey::Outcome {
                    scope,
                    agreement_id,
                    family_slot,
                    target_id,
                    source_namespace,
                    business_outcome_token,
                } => {
                    if scope != &key.scope || r.kind != "workflow-outcome-claim" {
                        return Err(StoreError::Integrity("financial outcome claim scope/kind"));
                    }
                    sqlx::query("INSERT INTO workflow_financial_outcome_claims VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                        .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                        .bind(agreement_id).bind(family_slot).bind(target_id).bind(source_namespace).bind(business_outcome_token)
                        .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
                }
            }
        }
        for link in candidate.links() {
            if link.scope() != &key.scope {
                return Err(StoreError::Integrity("financial link scope"));
            }
            let r = link.record_ref();
            let child = link.child_ref();
            let predecessor = link.predecessor_ref();
            let product = link.product_ref();
            let binding = link.binding_ref();
            for member in [r, child, predecessor, product, binding] {
                self.financial_member(identity, member).await?;
            }
            sqlx::query("INSERT INTO workflow_financial_links VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                .bind(&child.kind).bind(&child.id).bind(&child.digest).bind(link.slot())
                .bind(&predecessor.kind).bind(&predecessor.id).bind(&predecessor.digest)
                .bind(&product.kind).bind(&product.id).bind(&product.digest)
                .bind(&binding.kind).bind(&binding.id).bind(&binding.digest)
                .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
        }
        for dependency in candidate.dependencies() {
            let r = dependency.record_ref();
            if r.kind != "workflow-dependency" {
                return Err(StoreError::Integrity("financial dependency record kind"));
            }
            let target = dependency.target_ref();
            let terms = dependency.original_terms_ref();
            let capacity = dependency.capacity_ref();
            for member in [r, target, terms, capacity] {
                self.financial_member(identity, member).await?;
            }
            let kind = match dependency.kind() {
                DependencyKind::OutcomeFamily => "outcome_family",
                DependencyKind::WorkBasis => "work_basis",
                DependencyKind::DelayedCost => "delayed_cost",
            };
            sqlx::query("INSERT INTO workflow_financial_dependencies VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
                .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                .bind(dependency.ancestor_decision_id()).bind(dependency.dependent_decision_id()).bind(kind)
                .bind(&target.kind).bind(&target.id).bind(&target.digest).bind(dependency.family_slot().unwrap_or(""))
                .bind(&terms.kind).bind(&terms.id).bind(&terms.digest)
                .bind(&capacity.kind).bind(&capacity.id).bind(&capacity.digest)
                .bind(&r.kind).bind(&r.id).bind(&r.digest).execute(self.conn()).await?;
        }
        Ok(())
    }
}
