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
    pub(super) fn resolve<'a>(
        prefix: &mut FoldedInventory,
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
            if prefix._inventory.has(record.reference(), s)? {
                prefix
                    ._inventory
                    .compare_record(record.reference(), record.bytes(), s)?;
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
                prefix._inventory.body(parsed.value(), s)?
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
