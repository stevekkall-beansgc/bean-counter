//! Data reconstruction of immutable activation boundaries. Origins are checked
//! by history_before; no mutable head or current clock selects historical terms.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::service::workflow::agreement) struct Transition {
    pub(in crate::service::workflow::agreement) head: Document,
    pub(in crate::service::workflow::agreement) activation: Document,
    pub(in crate::service::workflow::agreement) binding: Document,
    pub(in crate::service::workflow::agreement) revision: i64,
    pub(in crate::service::workflow::agreement) sequence: i64,
    pub(in crate::service::workflow::agreement) at: Timestamp,
    pub(in crate::service::workflow::agreement) effective: Timestamp,
    pub(in crate::service::workflow::agreement) action: ActivationAction,
    pub(in crate::service::workflow::agreement) target: Option<RecordRef>,
    pub(in crate::service::workflow::agreement) prior: Option<RecordRef>,
}
fn checked(d: &Document) -> Result<Value> {
    let v =
        canonical::parse_bounded(&d.bytes, 262144).map_err(|_| ServiceError::IntegrityFailure)?;
    if records::bytes(&v)? != d.bytes || v["schema"] != d.schema {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(v)
}
fn reference(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
pub(in crate::service::workflow::agreement) fn transition(
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    head: Document,
    activation: Document,
    binding: Document,
) -> Result<Transition> {
    let h = checked(&head)?;
    let a = checked(&activation)?;
    let b = canonical_body(&binding)?;
    let saved_key = OperationKey::parse(&records::bytes(&h["operation_key"])?)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let revision = text(&h, "new_revision")?
        .parse::<i64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let sequence = text(&h, "commit_sequence")?
        .parse::<i64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let at =
        Timestamp::parse(text(&h, "accepted_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let effective =
        Timestamp::parse(text(&a, "effective_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    if saved_key.logical_store_id != key.logical_store_id
        || saved_key.scope != key.scope
        || h["scope"] != json!(key.scope)
        || h["head_kind"] != "agreement_activation"
        || h["customer"] != customer
        || h["agreement_id"] != agreement
        || b["customer"] != customer
        || b["agreement_id"] != agreement
        || h["accepted_binding_ref"] != json!(binding.reference)
        || h["resolved_effective_time"] != a["effective_at"]
        || a["binding_ref"] != json!(binding.reference)
        || a["head_transition_ref"] != json!(head.reference)
        || a["accepted_at"] != h["accepted_at"]
        || a["commit_sequence"] != h["commit_sequence"]
        || sequence <= 0
        || revision <= 0
        || record::semantic_document(
            "workflow-head-transition",
            &saved_key,
            customer,
            agreement,
            revision as u64,
            h.clone(),
        )?
        .reference
            != head.reference
        || record::semantic_document(
            "workflow-activation",
            &saved_key,
            customer,
            agreement,
            revision as u64,
            a.clone(),
        )?
        .reference
            != activation.reference
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let action = match text(&a, "action")? {
        "activate" => ActivationAction::Activate,
        "replace" => ActivationAction::Replace,
        "revoke" => ActivationAction::Revoke,
        _ => return Err(ServiceError::IntegrityFailure),
    };
    let target = a.get("target_transition_ref").map(reference).transpose()?;
    let prior = h.get("prior_head_ref").map(reference).transpose()?;
    Ok(Transition {
        head,
        activation,
        binding,
        revision,
        sequence,
        at,
        effective,
        action,
        target,
        prior,
    })
}
fn valid_binding(t: &Transition) -> Result<()> {
    if t.action == ActivationAction::Revoke {
        return Ok(());
    }
    let b = canonical_body(&t.binding)?;
    let starts =
        Timestamp::parse(text(&b, "starts_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let ends = b
        .get("ends_at")
        .map(|v| {
            Timestamp::parse(v.as_str().ok_or(ServiceError::IntegrityFailure)?)
                .map_err(|_| ServiceError::IntegrityFailure)
        })
        .transpose()?;
    binding_interval_with_work(
        &t.effective,
        &starts,
        ends.as_ref(),
        &mut ledgerlab_core::validation_work::NoopValidationWork,
    )
}
/// SAME effective-boundary interval predicate for legacy schedule verification
/// and owning activation production. Parsing/retained custody remain callers'.
pub(in crate::service::workflow::agreement) fn binding_interval_with_work(
    effective: &Timestamp,
    starts: &Timestamp,
    ends: Option<&Timestamp>,
    work: &mut dyn ledgerlab_core::validation_work::ValidationWork,
) -> Result<()> {
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op, ValidationVisit as Visit};
    work.arithmetic(
        Op::Compare,
        &[&effective.micros().into(), &starts.micros().into()],
    )
    .map_err(core_error)?;
    work.visit(Visit::Constraint).map_err(core_error)?;
    if effective.micros() < starts.micros() {
        return Err(ServiceError::IntegrityFailure);
    }
    if let Some(end) = ends {
        work.arithmetic(
            Op::Compare,
            &[&effective.micros().into(), &end.micros().into()],
        )
        .map_err(core_error)?;
        work.visit(Visit::Constraint).map_err(core_error)?;
        if effective.micros() >= end.micros() {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(())
}
/// Latest boundary includes revoke. Callers must then check the selected
/// binding's interval; never search backwards for an older valid binding.
pub(in crate::service::workflow::agreement) fn boundary_at<'a>(
    history: &'a [Transition],
    at: &Timestamp,
) -> Result<Option<&'a Transition>> {
    let superseded: BTreeSet<_> = history
        .iter()
        .filter(|t| t.at.micros() <= at.micros())
        .filter_map(|t| t.target.as_ref())
        .map(|r| r.id.clone())
        .collect();
    Ok(history
        .iter()
        .filter(|t| {
            t.at.micros() <= at.micros()
                && !superseded.contains(&t.activation.reference.id)
                && t.effective.micros() <= at.micros()
        })
        .max_by_key(|t| (t.effective.micros(), t.sequence)))
}
/// Validate the unambiguous activate/replace portion and the whole contiguous
/// identity/time lineage. Revoke selection is finalized by the approved stop
/// verifier; this routine does not invent a pending-cancellation rule.
pub(in crate::service::workflow::agreement) fn check_next(
    history: &[Transition],
    next: &Transition,
) -> Result<()> {
    let prior = history.last();
    if prior.map_or(0, |p| p.revision).checked_add(1) != Some(next.revision)
        || next.prior.as_ref() != prior.map(|p| &p.head.reference)
        || prior.is_some_and(|p| p.sequence >= next.sequence || p.at.micros() > next.at.micros())
        || next.effective.micros() < next.at.micros()
    {
        return Err(ServiceError::IntegrityFailure);
    }
    valid_binding(next)?;
    let superseded: BTreeSet<_> = history
        .iter()
        .filter_map(|t| t.target.as_ref())
        .map(|r| r.id.clone())
        .collect();
    let pending: Vec<_> = history
        .iter()
        .filter(|t| {
            !superseded.contains(&t.activation.reference.id)
                && t.effective.micros() > next.at.micros()
        })
        .collect();
    if pending.len() > 1 {
        return Err(ServiceError::IntegrityFailure);
    }
    match next.action {
        ActivationAction::Activate => {
            if !pending.is_empty() || next.target.is_some() {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Replace => {
            if pending.first().map(|t| &t.activation.reference) != next.target.as_ref() {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Revoke => {
            if next.effective != next.at {
                return Err(ServiceError::IntegrityFailure);
            }
            let selected = if let Some(pending) = pending.first() {
                Some(*pending)
            } else {
                boundary_at(history, &next.at)?
            };
            if selected.is_none_or(|t| {
                t.action == ActivationAction::Revoke
                    || Some(&t.activation.reference) != next.target.as_ref()
            }) {
                return Err(reject("ACTIVATION_SCHEDULE_CONFLICT"));
            }
        }
    }
    Ok(())
}
/// The ordered target index is scratch only and has no origin authority. Its
/// storage and every heap comparison are paid by the existing original owner.
struct PaidTargetIndex<'a> {
    ids: Vec<&'a str>,
    _storage: ledgerlab_core::canonical::audit_arena::ExternalBufferLease,
}
impl<'a> PaidTargetIndex<'a> {
    fn build(
        history: &'a [Transition],
        at: Option<i64>,
        session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
    ) -> Result<Self> {
        use ledgerlab_core::validation_work::{
            ArithmeticOperation as Op, ValidationVisit as Visit,
        };
        let count = history.len();
        session
            .semantic_work()
            .arithmetic(
                Op::Multiply,
                &[&count.into(), &std::mem::size_of::<&str>().into()],
            )
            .map_err(core_error)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or(ServiceError::Unavailable)?;
        let storage = session
            .reserve_external_payload(bytes)
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(bytes)
            .map_err(core_error)?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&ids.capacity().into(), &count.into()])
            .map_err(core_error)?;
        require(ids.capacity() <= count, "PROGRAM_BUDGET_EXCEEDED")?;
        for t in history {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if let Some(at) = at {
                session
                    .semantic_work()
                    .arithmetic(Op::Compare, &[&t.at.micros().into(), &at.into()])
                    .map_err(core_error)?;
                if t.at.micros() > at {
                    continue;
                }
            }
            if let Some(target) = &t.target {
                ids.push(target.id.as_str());
            }
        }
        // In-place heapsort gives deterministic O(n log n) comparisons and no
        // auxiliary allocation; the helper charges actual string bytes.
        fn compare(
            a: &str,
            b: &str,
            s: &mut ledgerlab_core::validation_work::OriginalAuditSession,
        ) -> Result<std::cmp::Ordering> {
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
            s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
            Ok(a.cmp(b))
        }
        fn sift(
            ids: &mut [&str],
            mut root: usize,
            end: usize,
            s: &mut ledgerlab_core::validation_work::OriginalAuditSession,
        ) -> Result<()> {
            loop {
                s.semantic_work()
                    .arithmetic(Op::Multiply, &[&root.into(), &2usize.into()])
                    .map_err(core_error)?;
                let doubled = root.checked_mul(2).ok_or(ServiceError::Unavailable)?;
                s.semantic_work()
                    .arithmetic(Op::Add, &[&doubled.into(), &1usize.into()])
                    .map_err(core_error)?;
                let child = doubled.checked_add(1).ok_or(ServiceError::Unavailable)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&child.into(), &end.into()])
                    .map_err(core_error)?;
                if child >= end {
                    break;
                }
                let mut largest = child;
                s.semantic_work()
                    .arithmetic(Op::Add, &[&child.into(), &1usize.into()])
                    .map_err(core_error)?;
                let sibling = child.checked_add(1).ok_or(ServiceError::Unavailable)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&sibling.into(), &end.into()])
                    .map_err(core_error)?;
                if sibling < end && compare(ids[child], ids[sibling], s)?.is_lt() {
                    largest = sibling;
                }
                if !compare(ids[root], ids[largest], s)?.is_lt() {
                    break;
                }
                s.semantic_work()
                    .visit(Visit::SortMove)
                    .map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(2 * std::mem::size_of::<&str>())
                    .map_err(core_error)?;
                ids.swap(root, largest);
                root = largest;
            }
            Ok(())
        }
        let n = ids.len();
        session
            .semantic_work()
            .arithmetic(Op::DivideRemainder, &[&n.into(), &2usize.into()])
            .map_err(core_error)?;
        for root in (0..n / 2).rev() {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            sift(&mut ids, root, n, session)?;
        }
        for end in (1..n).rev() {
            session
                .semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            session
                .semantic_work()
                .visit(Visit::SortMove)
                .map_err(core_error)?;
            session
                .semantic_work()
                .write_bytes(2 * std::mem::size_of::<&str>())
                .map_err(core_error)?;
            ids.swap(0, end);
            sift(&mut ids, 0, end, session)?;
        }
        Ok(Self {
            ids,
            _storage: storage,
        })
    }
    fn contains(
        &self,
        id: &str,
        session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
    ) -> Result<bool> {
        use ledgerlab_core::validation_work::{
            ArithmeticOperation as Op, ValidationVisit as Visit,
        };
        let (mut low, mut high) = (0usize, self.ids.len());
        loop {
            session
                .semantic_work()
                .arithmetic(Op::Compare, &[&low.into(), &high.into()])
                .map_err(core_error)?;
            if low >= high {
                break;
            }
            session
                .semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            session
                .semantic_work()
                .arithmetic(Op::Subtract, &[&high.into(), &low.into()])
                .map_err(core_error)?;
            let width = high - low;
            session
                .semantic_work()
                .arithmetic(Op::DivideRemainder, &[&width.into(), &2usize.into()])
                .map_err(core_error)?;
            let half = width / 2;
            session
                .semantic_work()
                .arithmetic(Op::Add, &[&low.into(), &half.into()])
                .map_err(core_error)?;
            let middle = low.checked_add(half).ok_or(ServiceError::Unavailable)?;
            let item = self.ids[middle];
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(item.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(id.len())
                .map_err(core_error)?;
            match item.cmp(id) {
                std::cmp::Ordering::Less => {
                    session
                        .semantic_work()
                        .arithmetic(Op::Add, &[&middle.into(), &1usize.into()])
                        .map_err(core_error)?;
                    low = middle.checked_add(1).ok_or(ServiceError::Unavailable)?;
                }
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Ok(true),
            }
        }
        Ok(false)
    }
}

/// Original-session counterpart of the existing check. It uses the same
/// ordered-set membership and pending/stop semantics with paid scratch data.
pub(in crate::service::workflow::agreement) fn check_next_with_session(
    history: &[Transition],
    next: &Transition,
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<()> {
    use ledgerlab_core::validation_work::{
        ArithmeticOperation as Op, RepresentationKind, ValidationVisit as Visit,
    };
    let prior = history.last();
    let revision = prior.map_or(0, |p| p.revision);
    session
        .semantic_work()
        .arithmetic(Op::Add, &[&revision.into(), &1i64.into()])
        .map_err(core_error)?;
    session
        .semantic_work()
        .arithmetic(Op::Compare, &[&revision.into(), &next.revision.into()])
        .map_err(core_error)?;
    if revision.checked_add(1) != Some(next.revision) {
        return Err(ServiceError::IntegrityFailure);
    }
    if !paid_ref_eq(
        next.prior.as_ref(),
        prior.map(|p| &p.head.reference),
        session,
    )? {
        return Err(ServiceError::IntegrityFailure);
    }
    if let Some(p) = prior {
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&p.sequence.into(), &next.sequence.into()])
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&p.at.micros().into(), &next.at.micros().into()],
            )
            .map_err(core_error)?;
        if p.sequence >= next.sequence || p.at.micros() > next.at.micros() {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    session
        .semantic_work()
        .arithmetic(
            Op::Compare,
            &[&next.effective.micros().into(), &next.at.micros().into()],
        )
        .map_err(core_error)?;
    if next.effective.micros() < next.at.micros() {
        return Err(ServiceError::IntegrityFailure);
    }
    if next.action != ActivationAction::Revoke {
        let binding = session
            .parse_representation(&next.binding.bytes, RepresentationKind::Record)
            .map_err(core_error)?;
        let _timestamps = session
            .reserve_external_payload(2 * (4 * 64 + 27))
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(2 * (4 * 64 + 27))
            .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::Field)
            .map_err(core_error)?;
        let starts = Timestamp::parse_with_work(
            text(binding.value(), "starts_at")?,
            session.semantic_work(),
        )
        .map_err(core_error)?;
        session
            .semantic_work()
            .visit(Visit::Field)
            .map_err(core_error)?;
        let ends = binding
            .value()
            .get("ends_at")
            .map(|v| {
                Timestamp::parse_with_work(
                    v.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    session.semantic_work(),
                )
                .map_err(core_error)
            })
            .transpose()?;
        binding_interval_with_work(
            &next.effective,
            &starts,
            ends.as_ref(),
            session.semantic_work(),
        )?;
    }
    let superseded = PaidTargetIndex::build(history, None, session)?;
    let mut pending = None;
    for t in history {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&t.effective.micros().into(), &next.at.micros().into()],
            )
            .map_err(core_error)?;
        if t.effective.micros() > next.at.micros()
            && !superseded.contains(&t.activation.reference.id, session)?
            && pending.replace(t).is_some()
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    match next.action {
        ActivationAction::Activate => {
            if pending.is_some() || next.target.is_some() {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Replace => {
            if !paid_ref_eq(
                pending.map(|t| &t.activation.reference),
                next.target.as_ref(),
                session,
            )? {
                return Err(ServiceError::IntegrityFailure);
            }
        }
        ActivationAction::Revoke => {
            session
                .semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&next.effective.micros().into(), &next.at.micros().into()],
                )
                .map_err(core_error)?;
            if next.effective != next.at {
                return Err(ServiceError::IntegrityFailure);
            }
            let selected = if pending.is_some() {
                pending
            } else {
                paid_boundary_at(history, &next.at, session)?
            };
            if selected.is_none_or(|t| t.action == ActivationAction::Revoke)
                || !paid_ref_eq(
                    selected.map(|t| &t.activation.reference),
                    next.target.as_ref(),
                    session,
                )?
            {
                return Err(reject("ACTIVATION_SCHEDULE_CONFLICT"));
            }
        }
    }
    Ok(())
}
fn paid_ref_eq(
    a: Option<&RecordRef>,
    b: Option<&RecordRef>,
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<bool> {
    use ledgerlab_core::validation_work::ValidationVisit as Visit;
    session
        .semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    match (a, b) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => {
            for (x, y) in [(&a.kind, &b.kind), (&a.id, &b.id), (&a.digest, &b.digest)] {
                session
                    .semantic_work()
                    .scan_bytes(x.len())
                    .map_err(core_error)?;
                session
                    .semantic_work()
                    .scan_bytes(y.len())
                    .map_err(core_error)?;
                if x != y {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
pub(in crate::service::workflow::agreement) fn paid_boundary_at<'a>(
    history: &'a [Transition],
    at: &Timestamp,
    session: &mut ledgerlab_core::validation_work::OriginalAuditSession,
) -> Result<Option<&'a Transition>> {
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op, ValidationVisit as Visit};
    let superseded = PaidTargetIndex::build(history, Some(at.micros()), session)?;
    let mut selected: Option<&Transition> = None;
    for t in history {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(Op::Compare, &[&t.at.micros().into(), &at.micros().into()])
            .map_err(core_error)?;
        session
            .semantic_work()
            .arithmetic(
                Op::Compare,
                &[&t.effective.micros().into(), &at.micros().into()],
            )
            .map_err(core_error)?;
        if t.at.micros() > at.micros()
            || t.effective.micros() > at.micros()
            || superseded.contains(&t.activation.reference.id, session)?
        {
            continue;
        }
        if let Some(old) = selected {
            session
                .semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&t.effective.micros().into(), &old.effective.micros().into()],
                )
                .map_err(core_error)?;
            session
                .semantic_work()
                .arithmetic(Op::Compare, &[&t.sequence.into(), &old.sequence.into()])
                .map_err(core_error)?;
            if (t.effective.micros(), t.sequence) < (old.effective.micros(), old.sequence) {
                continue;
            }
        }
        selected = Some(t);
    }
    Ok(selected)
}

/// Every independent birth identity must resolve. Full committed certificates
/// authenticate these bodies; index copies are compared after, not trusted.
pub(in crate::service::workflow::agreement) async fn history_before(
    tx: &mut SqliteTx,
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    before: i64,
) -> Result<Vec<Transition>> {
    let candidates = tx
        .workflow_agreement_activation_prefix(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
        )
        .await
        .map_err(crate::service::store_error)?;
    let mut history = BTreeMap::new();
    let mut intents = BTreeMap::new();
    for identity in candidates {
        let k = OperationKey::parse(&identity).map_err(|_| ServiceError::IntegrityFailure)?;
        if k.logical_store_id != key.logical_store_id || k.scope != key.scope {
            return Err(ServiceError::IntegrityFailure);
        }
        let o = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let body = canonical::parse_bounded(&o.operation_bytes, 8388608)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let at = Timestamp::parse(text(&body, "accepted_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if body["key"] != json!(k)
            || body["kind"] != o.kind
            || !matches!(o.kind.as_str(), "agreement.activate" | "agreement.revoke")
            || o.sequence <= 0
            || body["commit_sequence"] != o.sequence.to_string().as_str()
            || at.micros() != o.accepted_at_us
            || records::bytes(&body)? != o.operation_bytes
            || records::record_reference("workflow-operation", &key.scope, &body)?
                != o.operation_ref
        {
            return Err(ServiceError::IntegrityFailure);
        }
        if o.sequence >= before {
            continue;
        }
        let verified = Box::pin(verify::retained(tx, &k, &o)).await?;
        if verified["receipt"]["result_kind"] == "semantic_alias" {
            continue;
        }
        if verified["receipt"]["result_kind"] != "administrative" {
            return Err(ServiceError::IntegrityFailure);
        }
        let refs: Vec<RecordRef> =
            serde_json::from_value(verified["receipt"]["result_refs"].clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
        let find = |kind: &str| -> Result<RecordRef> {
            let matches: Vec<_> = refs.iter().filter(|r| r.kind == kind).collect();
            if matches.len() != 1 {
                return Err(ServiceError::IntegrityFailure);
            }
            Ok(matches[0].clone())
        };
        if refs.len() != 3 {
            return Err(ServiceError::IntegrityFailure);
        }
        let intent = find("workflow-agreement-intent")?;
        let head = loaded(tx, key, &find("workflow-head-transition")?).await?;
        let activation = loaded(tx, key, &find("workflow-activation")?).await?;
        let a = checked(&activation)?;
        let binding = loaded(tx, key, &reference(&a["binding_ref"])?).await?;
        let t = transition(key, customer, agreement, head, activation, binding)?;
        if t.sequence != o.sequence
            || t.at != at
            || intents.insert(t.revision, intent).is_some()
            || history.insert(t.revision, t).is_some()
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    let history: Vec<_> = history.into_values().collect();
    for (i, t) in history.iter().enumerate() {
        check_next(&history[..i], t)?;
    }
    let indexed = tx
        .workflow_agreement_activation_history(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
        )
        .await
        .map_err(crate::service::store_error)?;
    let indexed: Vec<_> = indexed.iter().filter(|t| t.sequence < before).collect();
    if indexed.len() != history.len() {
        return Err(ServiceError::IntegrityFailure);
    }
    for (i, t) in history.iter().enumerate() {
        let r = indexed[i];
        if r.head.revision != t.revision
            || r.head.transition != t.head.reference
            || r.activation != t.activation.reference
            || Some(&r.intent) != intents.get(&t.revision)
            || r.binding != t.binding.reference
            || r.action != t.action
            || r.sequence != t.sequence
            || r.accepted_at_us != t.at.micros()
            || r.effective_at_us != t.effective.micros()
            || r.target != t.target
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(history)
}

#[cfg(test)]
/// Evaluate only an already certified schedule prefix. Expiry/revoke yields no
/// selection, never a fallback to a previously valid binding.
pub(in crate::service::workflow::agreement) fn selected_binding(
    history: &[Transition],
    at: &Timestamp,
) -> Result<Option<RecordRef>> {
    let Some(t) = boundary_at(history, at)? else {
        return Ok(None);
    };
    if t.action == ActivationAction::Revoke {
        return Ok(None);
    }
    let b = canonical_body(&t.binding)?;
    let starts =
        Timestamp::parse(text(&b, "starts_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let ends = b
        .get("ends_at")
        .map(|v| {
            Timestamp::parse(v.as_str().ok_or(ServiceError::IntegrityFailure)?)
                .map_err(|_| ServiceError::IntegrityFailure)
        })
        .transpose()?;
    if at.micros() < starts.micros() || ends.is_some_and(|end| at.micros() >= end.micros()) {
        return Ok(None);
    }
    Ok(Some(t.binding.reference.clone()))
}

#[cfg(test)]
mod tests;
