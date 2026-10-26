//! Fixed full-chain acquisition. The cursor never chooses a report: all
//! matching immutable edges feed the core's held links_root comparison.
use super::*;
use crate::store::sqlite::workflow::financial::{AppClaimLocator, BindingLinkCursor};
use ledgerlab_core::canonical::audit_arena::AuditedValue;

pub(super) struct ChainLinks {
    pub(super) references: Vec<ExactRef>,
    work_births: Vec<ExactRef>,
    owners: Vec<ExternalBufferLease>,
    _vectors: ExternalBufferLease,
}
impl ChainLinks {
    fn new(s: &mut OriginalAuditSession) -> Result<Self> {
        let vectors = s
            .reserve_external_payload(std::mem::size_of::<Self>())
            .map_err(core_error)?;
        Ok(Self {
            references: Vec::new(),
            work_births: Vec::new(),
            owners: Vec::new(),
            _vectors: vectors,
        })
    }
    fn grow(&mut self, s: &mut OriginalAuditSession) -> Result<()> {
        let capacity = self
            .references
            .capacity()
            .max(self.work_births.capacity())
            .max(1);
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&capacity.into(), &2usize.into()])
            .map_err(core_error)?;
        let next = capacity.checked_mul(2).ok_or(ServiceError::Unavailable)?;
        let stride =
            2 * std::mem::size_of::<ExactRef>() + 2 * std::mem::size_of::<ExternalBufferLease>();
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&next.into(), &stride.into()])
            .map_err(core_error)?;
        let rows = next.checked_mul(stride).ok_or(ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(
                Op::Add,
                &[&rows.into(), &std::mem::size_of::<Self>().into()],
            )
            .map_err(core_error)?;
        let bytes = rows
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(ServiceError::Unavailable)?;
        let lease = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&next.into(), &2usize.into()])
            .map_err(core_error)?;
        let owner_count = next.checked_mul(2).ok_or(ServiceError::Unavailable)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(next)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut births = Vec::new();
        births
            .try_reserve_exact(next)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut owners = Vec::new();
        owners
            .try_reserve_exact(owner_count)
            .map_err(|_| ServiceError::Unavailable)?;
        refs.append(&mut self.references);
        births.append(&mut self.work_births);
        owners.append(&mut self.owners);
        self.references = refs;
        self.work_births = births;
        self.owners = owners;
        self._vectors = lease;
        Ok(())
    }
    fn has_work_birth(&self, reference: &ExactRef, s: &mut OriginalAuditSession) -> Result<bool> {
        super::contains_reference(&self.work_births, reference, s)
    }
    fn remember_work_birth(
        &mut self,
        reference: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if self.work_births.len() == self.work_births.capacity() {
            self.grow(s)?;
        }
        let value = wire::reference(reference, s)?;
        let value = s
            .parse_representation(value.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let (reference, owner) = source::from_value_local(value.value(), s)?;
        self.work_births.push(reference);
        self.owners.push(owner);
        Ok(())
    }
    fn push(&mut self, reference: &Value, s: &mut OriginalAuditSession) -> Result<()> {
        if self.references.len() == self.references.capacity() {
            self.grow(s)?;
        }
        let (r, owner) = source::from_value_local(reference, s)?;
        require(
            !super::contains_reference(&self.references, &r, s)?,
            "INTEGRITY_FAILURE",
        )?;
        self.references.push(r);
        self.owners.push(owner);
        Ok(())
    }
}
fn decode(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| ServiceError::IntegrityFailure)
}
fn value(row: &AppClaimLocator, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let kind = wire::string(decode(row.kind.bytes())?, s)?;
    let id = wire::string(decode(row.id.bytes())?, s)?;
    let digest = wire::string(decode(row.digest.bytes())?, s)?;
    let _fields = s
        .reserve_external_payload(2 * std::mem::size_of::<[(&str, &[u8]); 3]>())
        .map_err(core_error)?;
    let encoded = wire::object(
        &[
            ("digest", digest.bytes()),
            ("id", id.bytes()),
            ("kind", kind.bytes()),
        ],
        262144,
        s,
    )?;
    s.parse_representation(encoded.bytes(), RepresentationKind::Record)
        .map_err(core_error)
}
fn equal(left: &Value, right: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let left = s
        .canonical_representation(left, RepresentationKind::Record)
        .map_err(core_error)?;
    let right = s
        .canonical_representation(right, RepresentationKind::Record)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(left.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(right.bytes().len())
        .map_err(core_error)?;
    require(left.bytes() == right.bytes(), "INTEGRITY_FAILURE")
}
fn result_member(receipt: &Value, reference: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let expected = s
        .canonical_representation(reference, RepresentationKind::Record)
        .map_err(core_error)?;
    let mut count = 0usize;
    for r in check::field(receipt, "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let actual = s
            .canonical_representation(r, RepresentationKind::Record)
            .map_err(core_error)?;
        s.semantic_work()
            .visit(ValidationVisit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(actual.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(expected.bytes().len())
            .map_err(core_error)?;
        if actual.bytes() == expected.bytes() {
            s.semantic_work()
                .arithmetic(Op::Add, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            count = count.checked_add(1).ok_or(ServiceError::Unavailable)?;
        }
    }
    require(count == 1, "INTEGRITY_FAILURE")
}
fn unique_result<'a>(
    receipt: &'a Value,
    kind: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let mut selected = None;
    for reference in check::field(receipt, "result_refs", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        if check::text(reference, "kind", s)? == kind {
            require(selected.replace(reference).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    selected.ok_or(ServiceError::IntegrityFailure)
}
#[derive(Clone, Copy)]
pub(super) enum ClaimRecipe {
    Declared,
    Policy,
}
pub(super) struct Selection<'a> {
    pub(super) recipe: ClaimRecipe,
    pub(super) binding: &'a ExactRef,
    pub(super) customer: &'a str,
    pub(super) chain: &'a str,
    pub(super) before: i64,
}
pub(super) async fn acquire(
    tx: &mut SqliteTx,
    key: &OperationKey,
    selected: Selection<'_>,
    pool: &mut super::super::super::local_inputs::LocalInputs,
    s: &mut OriginalAuditSession,
) -> Result<ChainLinks> {
    let mut out = ChainLinks::new(s)?;
    let mut after: Option<BindingLinkCursor> = None;
    loop {
        s.semantic_work()
            .visit(ValidationVisit::Edge)
            .map_err(core_error)?;
        let Some(row) = tx
            .workflow_local_binding_link_next(
                &key.logical_store_id,
                &key.scope,
                selected.binding,
                selected.before,
                after.as_ref(),
                s,
            )
            .await
            .map_err(prefix::read_error)?
        else {
            break;
        };
        let link_ref = value(&row.record, s)?;
        let link = source::retained_local_record(
            tx,
            key,
            link_ref.value(),
            ("workflow-link", "ledger-workflow-link/1"),
            selected.before,
            s,
        )
        .await?;
        prefix::verify_selected_association(tx, key, link.reference(), selected.before, s).await?;
        let child = value(&row.cursor.child, s)?;
        let predecessor = value(&row.predecessor, s)?;
        equal(check::field(link.body(), "child", s)?, child.value(), s)?;
        equal(
            check::field(link.body(), "predecessor", s)?,
            predecessor.value(),
            s,
        )?;
        check::same_text(
            check::text(link.body(), "slot", s)?,
            std::str::from_utf8(row.cursor.slot.bytes())
                .map_err(|_| ServiceError::IntegrityFailure)?,
            s,
        )?;
        let binding = wire::reference(selected.binding, s)?;
        let binding = s
            .parse_representation(binding.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        equal(
            check::field(link.body(), "binding_ref", s)?,
            binding.value(),
            s,
        )?;
        let predecessor = source::retained_local_record(
            tx,
            key,
            predecessor.value(),
            ("workflow-work", "ledger-workflow-work-record/1"),
            selected.before,
            s,
        )
        .await?;
        prefix::verify_selected_association(tx, key, predecessor.reference(), selected.before, s)
            .await?;
        let payload = check::field(predecessor.body(), "payload", s)?;
        equal(check::field(payload, "binding_ref", s)?, binding.value(), s)?;
        let customer = check::text(payload, "customer", s)?;
        let chain = check::text(payload, "chain", s)?;
        s.semantic_work()
            .scan_bytes(
                customer.len() + selected.customer.len() + chain.len() + selected.chain.len(),
            )
            .map_err(core_error)?;
        if customer == selected.customer && chain == selected.chain {
            if !out.has_work_birth(predecessor.reference(), s)? {
                let producer_key = check::field(predecessor.body(), "key", s)?;
                let identity = s
                    .canonical_representation(producer_key, RepresentationKind::Record)
                    .map_err(core_error)?;
                let producer = prefix::target(tx, key, identity, s)
                    .await?
                    .checked_wrappers(tx, s)
                    .await?;
                super::super::super::local_inputs::retain_work_claim(
                    tx,
                    key,
                    selected.before,
                    &predecessor,
                    &producer,
                    pool,
                    s,
                )
                .await?;
                out.remember_work_birth(predecessor.reference(), s)?;
            }
            let producer_key = check::field(link.body(), "operation_key", s)?;
            let identity = s
                .canonical_representation(producer_key, RepresentationKind::Record)
                .map_err(core_error)?;
            let origin = prefix::target(tx, key, identity, s)
                .await?
                .checked_wrappers(tx, s)
                .await?;
            s.semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[
                        &origin.wrappers().sequence().into(),
                        &selected.before.into(),
                    ],
                )
                .map_err(core_error)?;
            require(
                origin.wrappers().sequence() < selected.before,
                "INTEGRITY_FAILURE",
            )?;
            origin.require_member(link_ref.value(), s)?;
            origin.require_member(child.value(), s)?;
            result_member(origin.wrappers().receipt(), link_ref.value(), s)?;
            result_member(origin.wrappers().receipt(), child.value(), s)?;
            let evidence = check::field(origin.wrappers().operation(), "request_evidence", s)?;
            let normalized = check::field(evidence, "normalized_request", s)?;
            let normalized_payload = check::field(normalized, "payload", s)?;
            let child_kind = check::text(child.value(), "kind", s)?;
            let (schema, operation) = match child_kind {
                "workflow-work" => ("ledger-workflow-work-record/1", "work.accept"),
                "workflow-outcome-claim" => (
                    match (
                        selected.recipe,
                        check::text(normalized_payload, "schema", s)?,
                    ) {
                        (ClaimRecipe::Policy, "ledger-workflow-outcome/1") => {
                            "ledger-workflow-outcome-claim/1"
                        }
                        (_, "ledger-workflow-outcome/2") => "ledger-workflow-outcome-claim/2",
                        _ => return Err(ServiceError::IntegrityFailure),
                    },
                    "outcome.submit",
                ),
                _ => return Err(ServiceError::IntegrityFailure),
            };
            let child_body = source::retained_local_record(
                tx,
                key,
                child.value(),
                (child_kind, schema),
                selected.before,
                s,
            )
            .await?;
            check::same_text(
                check::text(origin.wrappers().operation(), "kind", s)?,
                operation,
                s,
            )?;
            equal(
                check::field(normalized, "key", s)?,
                check::field(link.body(), "operation_key", s)?,
                s,
            )?;
            let child_payload = if child_kind == "workflow-work" {
                equal(
                    check::field(child_body.body(), "key", s)?,
                    check::field(link.body(), "operation_key", s)?,
                    s,
                )?;
                check::field(child_body.body(), "payload", s)?
            } else {
                check::field(child_body.body(), "original_payload", s)?
            };
            equal(check::field(normalized, "payload", s)?, child_payload, s)?;
            let initial = unique_result(origin.wrappers().receipt(), "workflow-decision", s)?;
            let initial_body = source::retained_local_record(
                tx,
                key,
                initial,
                ("workflow-decision", "ledger-workflow-decision-revision/1"),
                selected.before,
                s,
            )
            .await?;
            equal(
                check::field(initial_body.body(), "subject_ref", s)?,
                child.value(),
                s,
            )?;
            equal(
                check::field(initial_body.body(), "authority_ref", s)?,
                check::field(link.body(), "authority_ref", s)?,
                s,
            )?;
            check::same_text(check::text(initial_body.body(), "revision", s)?, "1", s)?;
            pool.add(initial_body.reference(), s)?;
            if child_kind == "workflow-work" {
                if !out.has_work_birth(child_body.reference(), s)? {
                    super::super::super::local_inputs::retain_work_claim(
                        tx,
                        key,
                        selected.before,
                        &child_body,
                        &origin,
                        pool,
                        s,
                    )
                    .await?;
                    out.remember_work_birth(child_body.reference(), s)?;
                }
                let mut matches = 0usize;
                for input in check::field(child_payload, "links", s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    s.semantic_work()
                        .visit(ValidationVisit::Edge)
                        .map_err(core_error)?;
                    if check::text(input, "slot", s)? == check::text(link.body(), "slot", s)? {
                        equal(
                            check::field(input, "predecessor", s)?,
                            check::field(link.body(), "predecessor", s)?,
                            s,
                        )?;
                        s.semantic_work()
                            .arithmetic(Op::Add, &[&matches.into(), &1usize.into()])
                            .map_err(core_error)?;
                        matches = matches.checked_add(1).ok_or(ServiceError::Unavailable)?;
                    }
                }
                require(matches == 1, "INTEGRITY_FAILURE")?;
            } else if schema == "ledger-workflow-outcome-claim/1" {
                let target = source::retained_local_record(
                    tx,
                    key,
                    check::field(child_payload, "target_ref", s)?,
                    ("workflow-target", "ledger-workflow-component-target/1"),
                    selected.before,
                    s,
                )
                .await?;
                prefix::verify_selected_association(
                    tx,
                    key,
                    target.reference(),
                    selected.before,
                    s,
                )
                .await?;
                equal(
                    check::field(target.body(), "work_ref", s)?,
                    check::field(link.body(), "predecessor", s)?,
                    s,
                )?;
                equal(
                    check::field(target.body(), "binding_ref", s)?,
                    binding.value(),
                    s,
                )?;
                pool.add(target.reference(), s)?;
            } else {
                equal(
                    check::field(child_payload, "selector_start_work_ref", s)?,
                    check::field(link.body(), "predecessor", s)?,
                    s,
                )?;
            }
            out.push(link_ref.value(), s)?;
            pool.add(link.reference(), s)?;
        }
        after = Some(row.cursor);
    }
    pool.complete(tx, key, selected.before, s).await?;
    Ok(out)
}

#[cfg(test)]
#[path = "chain_links_tests.rs"]
mod tests;
