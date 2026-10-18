//! Consumer-owned AUTH03 walk over the actual completed introduction inventory.
//! Observation headers remain DATA: current custody/head/header production and
//! complete caller eligibility precede any private prepared commit.
use super::*;
use inventory::ConfiguredRoot;
use ledgerlab_core::policy::workflow::transition_constructor::{
    DraftContextData, EconomicAuditStream, ExactRef, RecomputedAuditDigest,
};
use std::{cmp::Ordering, mem};

pub(in super::super) struct ObservationHeader<'a> {
    pub(in super::super) reference: &'a ExactRef,
    pub(in super::super) body: &'a Value,
}
// These fixed bit constants contain no executed runtime shifts. Every dynamic
// selection and actual word operation below is paid on the consuming parent.
const BITS: [u64; 33] = {
    let mut out = [0; 33];
    let mut i = 0;
    while i < 33 {
        out[i] = 1u64 << i;
        i += 1;
    }
    out
};
const BLOCK_BITS: [u32; 32] = {
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = 1u32 << i;
        i += 1;
    }
    out
};
fn compare(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<Ordering> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    Ok(a.cmp(&b))
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn product(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_mul(b).ok_or(ServiceError::Unavailable)
}
fn bit(index: usize, s: &mut OriginalAuditSession) -> Result<u64> {
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    BITS.get(index)
        .copied()
        .ok_or(ServiceError::IntegrityFailure)
}
fn and(a: u64, b: u64, s: &mut OriginalAuditSession) -> Result<u64> {
    s.semantic_work()
        .arithmetic(Op::BitAnd, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    Ok(a & b)
}
fn union(a: u64, b: u64, s: &mut OriginalAuditSession) -> Result<u64> {
    s.semantic_work()
        .arithmetic(Op::BitOr, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    Ok(a | b)
}
fn nonzero(a: u64, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.into(), &0u64.into()])
        .map_err(core_error)?;
    Ok(a != 0)
}
struct RootGroup {
    root: ConfiguredRoot,
    witnesses: u64,
}
struct Memberships {
    by_sequence: Vec<u64>,
    groups: Vec<RootGroup>,
    _payload: ExternalBufferLease,
}
impl Memberships {
    fn new(through: usize, source_count: usize, s: &mut OriginalAuditSession) -> Result<Self> {
        let count = add(through, 1, s)?;
        let groups = add(source_count, 1, s)?;
        let bytes = add(
            product(count, mem::size_of::<u64>(), s)?,
            product(groups, mem::size_of::<RootGroup>(), s)?,
            s,
        )?;
        let bytes = add(bytes, mem::size_of::<Self>(), s)?;
        let payload = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut by_sequence = Vec::new();
        by_sequence
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        let zeroed = product(count, mem::size_of::<u64>(), s)?;
        s.semantic_work().write_bytes(zeroed).map_err(core_error)?;
        by_sequence.resize(count, 0);
        let mut roots = Vec::new();
        roots
            .try_reserve_exact(groups)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            by_sequence,
            groups: roots,
            _payload: payload,
        })
    }
    fn group(
        &mut self,
        header: &Value,
        witness: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<usize> {
        let root = ConfiguredRoot::from_value(request_wrapper::field(header, "root_ref", s)?, s)?;
        let mask = bit(witness, s)?;
        for (i, group) in self.groups.iter_mut().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if group.root.same(&root, s)? {
                group.witnesses = union(group.witnesses, mask, s)?;
                return Ok(i);
            }
        }
        let index = self.groups.len();
        require(
            compare(index, self.groups.capacity(), s)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        s.semantic_work()
            .write_bytes(mem::size_of::<RootGroup>())
            .map_err(core_error)?;
        self.groups.push(RootGroup {
            root,
            witnesses: mask,
        });
        Ok(index)
    }
    fn seed(
        &mut self,
        inventory: &mut inventory::Inventory,
        group: usize,
        witness: usize,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let origin = inventory.seed_origin(reference, &self.groups[group].root, s)?;
        if let Some(sequence) = origin {
            let mask = bit(witness, s)?;
            mark(&mut self.by_sequence, sequence, mask, s)?;
        }
        Ok(())
    }
    fn close(
        &mut self,
        inventory: &mut inventory::Inventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        // Required edges strictly decrease physical sequence. One descending
        // pass therefore computes the fixed point, including arbitrary gaps.
        // Equal configured roots share this traversal under THIS consumer;
        // different roots retain independent memberships and cuts.
        for sequence in (1..self.by_sequence.len()).rev() {
            s.semantic_work().visit(Visit::Value).map_err(core_error)?;
            let membership = self.by_sequence[sequence];
            if !nonzero(membership, s)? {
                continue;
            }
            for group in &self.groups {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let selected = and(membership, group.witnesses, s)?;
                if !nonzero(selected, s)? {
                    continue;
                }
                let words = &mut self.by_sequence;
                inventory.required_origins(
                    sequence as i64,
                    &group.root,
                    &mut |prior, s| {
                        require(
                            compare(prior as usize, sequence, s)?.is_lt(),
                            "INTEGRITY_FAILURE",
                        )?;
                        mark(words, prior, selected, s)
                    },
                    s,
                )?;
            }
        }
        Ok(())
    }
}
fn mark(words: &mut [u64], sequence: i64, mask: u64, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &0i64.into()])
        .map_err(core_error)?;
    require(sequence > 0, "INTEGRITY_FAILURE")?;
    require(
        compare(sequence as usize, words.len(), s)?.is_lt(),
        "INTEGRITY_FAILURE",
    )?;
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    let word = words
        .get_mut(sequence as usize)
        .ok_or(ServiceError::IntegrityFailure)?;
    *word = union(*word, mask, s)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<u64>())
        .map_err(core_error)
}
fn rows<'a>(body: &'a Value, name: &str, s: &mut OriginalAuditSession) -> Result<&'a [Value]> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    request_wrapper::field(body, name, s)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or(ServiceError::IntegrityFailure)
}
impl FoldedInventory {
    /// Recompute S_i and F separately from actual completed earliest origins,
    /// then stream their exact U under the consuming existing parent. This
    /// returns the core's bound digest DATA, not a custody/current capability.
    pub(super) fn audit_witnesses<'a>(
        &mut self,
        context: &'a DraftContextData,
        sources: &[ObservationHeader<'a>],
        financial: ObservationHeader<'a>,
        s: &mut OriginalAuditSession,
    ) -> Result<RecomputedAuditDigest<'a>> {
        let result = self.audit_witnesses_inner(context, sources, financial, s);
        if result.is_err() {
            s.invalidate();
        }
        result
    }
    fn audit_witnesses_inner<'a>(
        &mut self,
        context: &'a DraftContextData,
        sources: &[ObservationHeader<'a>],
        financial: ObservationHeader<'a>,
        s: &mut OriginalAuditSession,
    ) -> Result<RecomputedAuditDigest<'a>> {
        require(!compare(sources.len(), 32, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        let mut stream =
            EconomicAuditStream::new(context, financial.reference, s).map_err(core_error)?;
        for header in sources {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            stream
                .add_source_header(header.reference, header.body, s)
                .map_err(core_error)?;
        }
        stream
            .set_financial_header(financial.reference, financial.body, s)
            .map_err(core_error)?;
        let count = self.pointers.len();
        let through = if compare(count, 0, s)?.is_gt() {
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&count.into(), &1usize.into()])
                .map_err(core_error)?;
            let last = self.pointers.block(count - 1, s).map_err(core_error)?;
            let sequence = last.sequences(s).map_err(core_error)?;
            sequence[0].ok_or(ServiceError::IntegrityFailure)? as usize
        } else {
            0
        };
        let mut memberships = Memberships::new(through, sources.len(), s)?;
        for (index, header) in sources.iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let group = memberships.group(header.body, index, s)?;
            for name in ["enrollment_ref", "current_state_ref"] {
                memberships.seed(
                    &mut self._inventory,
                    group,
                    index,
                    request_wrapper::field(header.body, name, s)?,
                    s,
                )?;
            }
            for grant in rows(header.body, "original_grant_refs", s)? {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                memberships.seed(&mut self._inventory, group, index, grant, s)?;
            }
        }
        let f = sources.len();
        let group = memberships.group(financial.body, f, s)?;
        for reference in rows(financial.body, "preexisting_input_refs", s)? {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            memberships.seed(&mut self._inventory, group, f, reference, s)?;
        }
        for (name, field) in [("history_edges", "target_ref"), ("head_pins", "ref")] {
            for row in rows(financial.body, name, s)? {
                s.semantic_work().visit(Visit::Edge).map_err(core_error)?;
                memberships.seed(
                    &mut self._inventory,
                    group,
                    f,
                    request_wrapper::field(row, field, s)?,
                    s,
                )?;
            }
        }
        memberships.close(&mut self._inventory, s)?;
        self.stream_memberships(&memberships, sources.len(), &mut stream, s)?;
        stream.finish(s).map_err(core_error)
    }
    fn stream_memberships(
        &mut self,
        memberships: &Memberships,
        source_count: usize,
        stream: &mut EconomicAuditStream<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        // Closed intrinsic pointer grammar proves5355 bytes maximum per row;
        // this is a fixed staging allocation, not a new record/column cap.
        const ROW: usize = 5355;
        let control = mem::size_of::<[[u8; ROW]; 32]>()
            + mem::size_of::<[&[u8]; 32]>()
            + mem::size_of::<[bool; 32]>()
            + mem::size_of::<[u32; 33]>();
        let _control = s.reserve_external_payload(control).map_err(core_error)?;
        let mut buffer = [[0u8; ROW]; 32];
        let witnesses = add(source_count, 1, s)?;
        for first in (0..self.pointers.len()).step_by(32) {
            s.semantic_work().visit(Visit::Value).map_err(core_error)?;
            let block = self.pointers.block(first, s).map_err(core_error)?;
            let entries = block.entries(s).map_err(core_error)?;
            let mut masks = [0u32; 33];
            let mut selected = [false; 32];
            for (position, entry) in entries.iter().enumerate() {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                let membership = *memberships
                    .by_sequence
                    .get(entry.sequence() as usize)
                    .ok_or(ServiceError::IntegrityFailure)?;
                let used = nonzero(membership, s)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<bool>())
                    .map_err(core_error)?;
                selected[position] = used;
                if !used {
                    continue;
                }
                // This scalar transpose is functional but UNQUALIFIED: its
                // complete cost is accounted separately from the core stream.
                for (index, mask) in masks.iter_mut().take(witnesses).enumerate() {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    if nonzero(and(membership, bit(index, s)?, s)?, s)? {
                        s.semantic_work()
                            .visit(Visit::RefLookup)
                            .map_err(core_error)?;
                        let flag = BLOCK_BITS[position];
                        s.semantic_work()
                            .arithmetic(Op::BitOr, &[&(*mask).into(), &flag.into()])
                            .map_err(core_error)?;
                        *mask |= flag;
                    }
                }
            }
            let mut union_mask = masks[source_count];
            for mask in &masks[..source_count] {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                s.semantic_work()
                    .arithmetic(Op::BitOr, &[&union_mask.into(), &(*mask).into()])
                    .map_err(core_error)?;
                union_mask |= *mask;
            }
            s.semantic_work()
                .arithmetic(Op::PopulationCount, &[&union_mask.into()])
                .map_err(core_error)?;
            let count = union_mask.count_ones() as usize;
            if compare(count, 0, s)?.is_eq() {
                continue;
            }
            let mut raw = [b"".as_slice(); 32];
            let mut target = raw.iter_mut();
            for ((row, entry), used) in buffer.iter_mut().zip(entries).zip(selected) {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                if !used {
                    continue;
                }
                s.semantic_work()
                    .visit(Visit::Constraint)
                    .map_err(core_error)?;
                let (offset, length) = entry
                    .scratch_location()
                    .ok_or(ServiceError::IntegrityFailure)?;
                require(!compare(length, ROW, s)?.is_gt(), "INTEGRITY_FAILURE")?;
                let output = &mut row[..length];
                self._inventory.read_pointer_at(offset, output, s)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<&[u8]>())
                    .map_err(core_error)?;
                *target.next().ok_or(ServiceError::IntegrityFailure)? = output;
            }
            stream
                .push_origin_block(
                    &block,
                    masks[source_count],
                    &masks[..source_count],
                    &raw[..count],
                    s,
                )
                .map_err(core_error)?;
        }
        Ok(())
    }
}
