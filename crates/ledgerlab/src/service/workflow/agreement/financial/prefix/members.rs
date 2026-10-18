//! Attempt-owned exact member union. Append-only owning Ref slots keep the
//! traversal queue stable; a sorted index uses paid binary lookup and bounded
//! pointer moves. No origin, state or eligibility is inferred from membership.
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;
use std::{cmp::Ordering, mem};
const LIMIT: usize = 4096;
struct OwnedRef {
    reference: ExactRef,
    _payload: ExternalBufferLease,
}
pub(in super::super) struct ResolvedMembers {
    entries: Vec<OwnedRef>,
    order: Vec<usize>,
    _storage: ExternalBufferLease,
}
fn arithmetic(a: usize, b: usize, op: Op, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(op, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    match op {
        Op::Add => a.checked_add(b),
        Op::Subtract => a.checked_sub(b),
        Op::Multiply => a.checked_mul(b),
        Op::DivideRemainder => a.checked_div(b),
        _ => None,
    }
    .ok_or(ServiceError::IntegrityFailure)
}
fn compare(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<Ordering> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    Ok(a.cmp(&b))
}
fn parts<'a>(v: &'a Value, s: &mut OriginalAuditSession) -> Result<[&'a str; 3]> {
    request_wrapper::closed(v, &["kind", "id", "digest"], s)?;
    Ok([
        request_wrapper::text(v, "digest", s)?,
        request_wrapper::text(v, "id", s)?,
        request_wrapper::text(v, "kind", s)?,
    ])
}
fn cmp(reference: &ExactRef, v: [&str; 3], s: &mut OriginalAuditSession) -> Result<Ordering> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    for (a, b) in [
        reference.digest.as_str(),
        reference.id.as_str(),
        reference.kind.as_str(),
    ]
    .into_iter()
    .zip(v)
    {
        s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
        s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
        let order = a.cmp(b);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(Ordering::Equal)
}
impl ResolvedMembers {
    fn new(s: &mut OriginalAuditSession) -> Result<Self> {
        let stride = arithmetic(
            mem::size_of::<OwnedRef>(),
            mem::size_of::<usize>(),
            Op::Add,
            s,
        )?;
        let payload = arithmetic(LIMIT, stride, Op::Multiply, s)?;
        let storage = s.reserve_external_payload(payload).map_err(core_error)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(LIMIT)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut order = Vec::new();
        order
            .try_reserve_exact(LIMIT)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            entries,
            order,
            _storage: storage,
        })
    }
    fn add(&mut self, v: &Value, s: &mut OriginalAuditSession) -> Result<usize> {
        let p = parts(v, s)?;
        let mut lo = 0usize;
        let mut hi = self.order.len();
        while compare(lo, hi, s)?.is_lt() {
            let distance = arithmetic(hi, lo, Op::Subtract, s)?;
            let half = arithmetic(distance, 2, Op::DivideRemainder, s)?;
            let mid = arithmetic(lo, half, Op::Add, s)?;
            let index = self.order[mid];
            compare(index, self.entries.len(), s)?;
            let reference = &self
                .entries
                .get(index)
                .ok_or(ServiceError::IntegrityFailure)?
                .reference;
            match cmp(reference, p, s)? {
                Ordering::Equal => return Ok(index),
                Ordering::Less => lo = arithmetic(mid, 1, Op::Add, s)?,
                Ordering::Greater => hi = mid,
            }
        }
        require(
            compare(self.entries.len(), LIMIT, s)?.is_lt(),
            "REFERENCE_LIMIT",
        )?;
        let (reference, payload) = current::source::from_value(v, p[2], s)?;
        let index = self.entries.len();
        let shifted = arithmetic(self.order.len(), lo, Op::Subtract, s)?;
        let copied = arithmetic(shifted, mem::size_of::<usize>(), Op::Multiply, s)?;
        s.semantic_work().scan_bytes(copied).map_err(core_error)?;
        s.semantic_work().write_bytes(copied).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<usize>())
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<OwnedRef>())
            .map_err(core_error)?;
        // Both Vec capacities were pre-admitted once. insert performs the paid
        // contiguous pointer move; it cannot allocate in this guarded branch.
        compare(self.order.len(), self.order.capacity(), s)?;
        compare(self.entries.len(), self.entries.capacity(), s)?;
        arithmetic(self.order.len(), 1, Op::Add, s)?;
        arithmetic(self.entries.len(), 1, Op::Add, s)?;
        self.order.insert(lo, index);
        self.entries.push(OwnedRef {
            reference,
            _payload: payload,
        });
        Ok(index)
    }
    pub(in super::super) fn add_record_ref(
        &mut self,
        r: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let raw = wire::reference(r, s)?;
        let parsed = s
            .parse_representation(raw.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        self.add(parsed.value(), s).map(|_| ())
    }
    pub(in super::super) fn encode(&self, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
        let count = self.order.len();
        let payload = arithmetic(count, mem::size_of::<&ExactRef>(), Op::Multiply, s)?;
        let _storage = s.reserve_external_payload(payload).map_err(core_error)?;
        let mut refs = Vec::new();
        refs.try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        for index in &self.order {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            require(
                compare(*index, self.entries.len(), s)?.is_lt(),
                "INTEGRITY_FAILURE",
            )?;
            s.semantic_work()
                .write_bytes(mem::size_of::<&ExactRef>())
                .map_err(core_error)?;
            refs.push(&self.entries[*index].reference);
        }
        wire::RefSet::from_sorted(refs.iter().copied(), count, s)?.encode(s)
    }
    pub(in super::super) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(in super::super) fn position(
        &self,
        r: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        let (mut low, mut high) = (0usize, self.order.len());
        while compare(low, high, s)?.is_lt() {
            let width = arithmetic(high, low, Op::Subtract, s)?;
            let half = arithmetic(width, 2, Op::DivideRemainder, s)?;
            let middle = arithmetic(low, half, Op::Add, s)?;
            let index = self.order[middle];
            require(
                compare(index, self.entries.len(), s)?.is_lt(),
                "INTEGRITY_FAILURE",
            )?;
            match cmp(
                &self.entries[index].reference,
                [&r.digest, &r.id, &r.kind],
                s,
            )? {
                Ordering::Equal => return Ok(index),
                Ordering::Less => low = arithmetic(middle, 1, Op::Add, s)?,
                Ordering::Greater => high = middle,
            }
        }
        Err(ServiceError::IntegrityFailure)
    }
    pub(in super::super) fn at(
        &self,
        index: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<&ExactRef> {
        require(
            compare(index, self.entries.len(), s)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        Ok(&self.entries[index].reference)
    }
    pub(super) fn resolve<'a>(
        prefix: &mut FoldedInventory,
        records: &[&wire::SourceRecord],
        old: impl Iterator<Item = &'a ExactRef>,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        Self::resolve_optional(Some(&mut prefix._inventory), records, old, s)
    }
    pub(in super::super) fn resolve_optional<'a>(
        mut inventory: Option<&mut inventory::Inventory>,
        records: &[&wire::SourceRecord],
        old: impl Iterator<Item = &'a ExactRef>,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let mut out = Self::new(s)?;
        let size = arithmetic(
            LIMIT,
            mem::size_of::<Option<&wire::SourceRecord>>(),
            Op::Multiply,
            s,
        )?;
        let _pending = s.reserve_external_payload(size).map_err(core_error)?;
        let mut pending: Vec<Option<&wire::SourceRecord>> = Vec::new();
        pending
            .try_reserve_exact(LIMIT)
            .map_err(|_| ServiceError::Unavailable)?;
        s.semantic_work().write_bytes(size).map_err(core_error)?;
        pending.resize(LIMIT, None);
        for record in records {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let raw = wire::reference(record.reference(), s)?;
            let parsed = s
                .parse_representation(raw.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let index = out.add(parsed.value(), s)?;
            if let Some(inventory) = inventory.as_deref_mut() {
                if inventory.has(record.reference(), s)? {
                    inventory.compare_record(record.reference(), record.bytes(), s)?;
                }
            }
            if let Some(prior) = pending[index] {
                s.semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(prior.bytes().len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(record.bytes().len())
                    .map_err(core_error)?;
                require(prior.bytes() == record.bytes(), "INTEGRITY_FAILURE")?;
            }
            s.semantic_work()
                .write_bytes(mem::size_of::<Option<&wire::SourceRecord>>())
                .map_err(core_error)?;
            pending[index] = Some(*record);
        }
        for reference in old {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            out.add_record_ref(reference, s)?;
        }
        let mut position = 0usize;
        while compare(position, out.entries.len(), s)?.is_lt() {
            let body = if let Some(record) = pending[position] {
                s.copy_representation(record.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?
            } else {
                let raw = wire::reference(&out.entries[position].reference, s)?;
                let parsed = s
                    .parse_representation(raw.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                inventory
                    .as_deref_mut()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .body(parsed.value(), s)?
            };
            let body = s
                .parse_representation(body.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            dependencies::body(body.value(), &mut |v, s| out.add(v, s).map(|_| ()), s)?;
            position = arithmetic(position, 1, Op::Add, s)?;
        }
        // The operation itself remains a mandatory additional member. This is
        // the existing4096 union bound, not a different Ref/input domain.
        require(
            compare(out.entries.len(), LIMIT, s)?.is_lt(),
            "REFERENCE_LIMIT",
        )?;
        Ok(out)
    }
}

/// Explicit /9 fixed point only. Per-operation implicit operands, held heads
/// and absence checks must still join this DATA before making a real F header.
/// History rows retain exact owner/field/target; they are not ordinary I edges.
pub(in super::super) struct ResolvedEconomicInputs {
    pub(in super::super) members: ResolvedMembers,
    pub(in super::super) history_rows: AuditedBytes,
    _storage: ExternalBufferLease,
}
impl ResolvedEconomicInputs {
    pub(in super::super) fn select(
        prefix: &mut FoldedInventory,
        key: &ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
        seeds: &Value,
        head_seeds: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        use ledgerlab_core::policy::workflow::transition_constructor::{
            EconomicRecordView, select_resolved_inputs9_edges,
        };
        let result = (|| {
            let rows = seeds.as_array().ok_or(ServiceError::IntegrityFailure)?;
            let mut members = ResolvedMembers::new(s)?;
            let heads = head_seeds
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            for row in rows.iter().chain(heads) {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                members.add(row, s)?;
            }
            let bytes = arithmetic(LIMIT, mem::size_of::<AuditedBytes>(), Op::Multiply, s)?;
            let bytes = arithmetic(bytes, mem::size_of::<ResolvedEconomicInputs>(), Op::Add, s)?;
            let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
            let mut history_rows = Vec::new();
            history_rows
                .try_reserve_exact(LIMIT)
                .map_err(|_| ServiceError::Unavailable)?;
            let mut position = 0usize;
            while compare(position, members.entries.len(), s)?.is_lt() {
                let owner = wire::reference(&members.entries[position].reference, s)?;
                let owner_value = s
                    .parse_representation(owner.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let body = prefix._inventory.body(owner_value.value(), s)?;
                let edges = select_resolved_inputs9_edges(
                    key,
                    EconomicRecordView {
                        reference: &members.entries[position].reference,
                        canonical_body: body.bytes(),
                    },
                    s,
                )
                .map_err(core_error)?;
                for reference in edges.direct_refs() {
                    s.semantic_work().visit(Visit::Edge).map_err(core_error)?;
                    members.add_record_ref(reference, s)?;
                }
                for (field, target) in edges.history_edges() {
                    s.semantic_work().visit(Visit::Edge).map_err(core_error)?;
                    require(
                        compare(history_rows.len(), LIMIT, s)?.is_lt(),
                        "REFERENCE_LIMIT",
                    )?;
                    let target = wire::reference(target, s)?;
                    let target_value = s
                        .parse_representation(target.bytes(), RepresentationKind::Record)
                        .map_err(core_error)?;
                    prefix._inventory.require_value(target_value.value(), s)?;
                    let field = wire::string(field, s)?;
                    let _fields = s
                        .reserve_external_payload(2 * mem::size_of::<[(&str, &[u8]); 3]>())
                        .map_err(core_error)?;
                    let row = wire::object(
                        &[
                            ("field", field.bytes()),
                            ("owner_ref", owner.bytes()),
                            ("target_ref", target.bytes()),
                        ],
                        262144,
                        s,
                    )?;
                    s.semantic_work()
                        .write_bytes(mem::size_of::<AuditedBytes>())
                        .map_err(core_error)?;
                    history_rows.push(row);
                }
                position = arithmetic(position, 1, Op::Add, s)?;
            }
            let history_rows = wire::encoded_set(&mut history_rows, s)?;
            // Reserve the eventual operation member in the unchanged M limit.
            require(
                compare(members.entries.len(), LIMIT, s)?.is_lt(),
                "REFERENCE_LIMIT",
            )?;
            s.semantic_work()
                .write_bytes(mem::size_of::<ResolvedEconomicInputs>())
                .map_err(core_error)?;
            Ok(Self {
                members,
                history_rows,
                _storage: storage,
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}

impl LocatedAppAcceptance {
    pub(in super::super) fn work_explicit_inputs(
        &mut self,
        key: &OperationKey,
        seeds: &Value,
        head_seeds: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<ResolvedEconomicInputs> {
        let result = (|| {
            use ledgerlab_core::policy::workflow::transition_constructor::OperationKey as CoreKey;
            // /9 is selected by this exact original code-owned registry, not
            // by the newest descriptor or the incoming body's schema.
            let registry = self
                .publication
                .program()
                .execution_registry()
                .ok_or(ServiceError::Unavailable)?;
            if !super::super::acceptance::same_recipe_text(
                registry,
                super::super::acceptance::APP_COMMITMENT,
                s,
            )? {
                return Err(ServiceError::Unavailable);
            }
            let mut size = mem::size_of::<CoreKey>();
            for value in [
                &key.logical_store_id,
                &key.scope[0],
                &key.scope[1],
                &key.source,
                &key.operation_id,
            ] {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                size = arithmetic(size, value.len(), Op::Add, s)?;
            }
            let _owner = s.reserve_external_payload(size).map_err(core_error)?;
            fn text(v: &str, s: &mut OriginalAuditSession) -> Result<String> {
                s.semantic_work().scan_bytes(v.len()).map_err(core_error)?;
                s.semantic_work().write_bytes(v.len()).map_err(core_error)?;
                Ok(v.to_owned())
            }
            let key = CoreKey {
                logical_store_id: text(&key.logical_store_id, s)?,
                scope: [text(&key.scope[0], s)?, text(&key.scope[1], s)?],
                source: text(&key.source, s)?,
                operation_id: text(&key.operation_id, s)?,
            };
            s.semantic_work()
                .write_bytes(mem::size_of::<CoreKey>())
                .map_err(core_error)?;
            ResolvedEconomicInputs::select(&mut self._prefix, &key, seeds, head_seeds, s)
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}

impl LocatedAppAcceptance {
    pub(in super::super) fn require_held_reference(
        &mut self,
        bytes: &[u8],
        kind: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let result = (|| {
            let value = s
                .parse_representation(bytes, RepresentationKind::Record)
                .map_err(core_error)?;
            super::super::acceptance::validate_reference(value.value(), kind, s)?;
            self._prefix._inventory.require_value(value.value(), s)
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}

/// Only the selected immediate operand set is retained for the child. This is
/// bounded by existing M/M-bytes, not a resident collection of history bodies.
pub(in super::super) struct RetainedEconomicBodies {
    bodies: Vec<AuditedBytes>,
    selected: ResolvedEconomicInputs,
    _storage: ExternalBufferLease,
}
pub(in super::super) struct EconomicInputViews<'a> {
    pub(in super::super) rows:
        Vec<ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView<'a>>,
    _storage: ExternalBufferLease,
}
impl LocatedAppAcceptance {
    pub(in super::super) fn retain_economic_inputs(
        &mut self,
        selected: ResolvedEconomicInputs,
        s: &mut OriginalAuditSession,
    ) -> Result<RetainedEconomicBodies> {
        self._prefix.retain_economic_inputs(selected, s)
    }
}
impl FoldedInventory {
    pub(in super::super) fn retain_economic_inputs(
        &mut self,
        selected: ResolvedEconomicInputs,
        s: &mut OriginalAuditSession,
    ) -> Result<RetainedEconomicBodies> {
        let result = (|| {
            let count = selected.members.len();
            let bytes = arithmetic(count, mem::size_of::<AuditedBytes>(), Op::Multiply, s)?;
            let bytes = arithmetic(bytes, mem::size_of::<RetainedEconomicBodies>(), Op::Add, s)?;
            let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
            let mut bodies = Vec::new();
            bodies
                .try_reserve_exact(count)
                .map_err(|_| ServiceError::Unavailable)?;
            let mut total = 0usize;
            for member in &selected.members.entries {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let reference = wire::reference(&member.reference, s)?;
                let parsed = s
                    .parse_representation(reference.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                // The SAME completed prefix provides both the exact body and
                // earliest introduction. The child independently hashes and
                // selects every operand; no DATA-to-origin conversion occurs.
                let body = self._inventory.body(parsed.value(), s)?;
                total = arithmetic(total, body.bytes().len(), Op::Add, s)?;
                require(
                    !compare(total, 8388608, s)?.is_gt(),
                    "CORRECTION_CAPACITY_EXCEEDED",
                )?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<AuditedBytes>())
                    .map_err(core_error)?;
                bodies.push(body);
            }
            s.semantic_work()
                .write_bytes(mem::size_of::<RetainedEconomicBodies>())
                .map_err(core_error)?;
            Ok(RetainedEconomicBodies {
                bodies,
                selected,
                _storage: storage,
            })
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}
impl RetainedEconomicBodies {
    pub(in super::super) fn selected(&self) -> &ResolvedEconomicInputs {
        &self.selected
    }
    pub(in super::super) fn views<'a>(
        &'a self,
        s: &mut OriginalAuditSession,
    ) -> Result<EconomicInputViews<'a>> {
        use ledgerlab_core::policy::workflow::transition_constructor::EconomicRecordView;
        require(
            compare(self.bodies.len(), self.selected.members.entries.len(), s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        let count = self.bodies.len();
        let bytes = arithmetic(
            count,
            mem::size_of::<EconomicRecordView<'_>>(),
            Op::Multiply,
            s,
        )?;
        let bytes = arithmetic(bytes, mem::size_of::<EconomicInputViews<'_>>(), Op::Add, s)?;
        let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        for (body, member) in self.bodies.iter().zip(&self.selected.members.entries) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<EconomicRecordView<'_>>())
                .map_err(core_error)?;
            rows.push(EconomicRecordView {
                reference: &member.reference,
                canonical_body: body.bytes(),
            });
        }
        s.semantic_work()
            .write_bytes(mem::size_of::<EconomicInputViews<'_>>())
            .map_err(core_error)?;
        Ok(EconomicInputViews {
            rows,
            _storage: storage,
        })
    }
}
