//! Consumer-owned AUTH03 walk over the actual completed introduction inventory.
//! Observation headers remain DATA: current custody/head/header production and
//! complete caller eligibility precede any private prepared commit.
use super::*;
use inventory::ConfiguredRoot;
use ledgerlab_core::policy::workflow::transition_constructor::{
    DraftContextData, EconomicAuditStream, ExactRef, FinancialPrefixInput,
    ORIGIN_POINTER_MAX_BYTES, ProducedOriginPrefixes, RecomputedAuditDigest, SourcePrefixInput,
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
    by_ordinal: Vec<u64>,
    packed: Vec<Vec<u32>>,
    groups: Vec<RootGroup>,
    _payload: ExternalBufferLease,
}
impl Memberships {
    fn new(count: usize, source_count: usize, s: &mut OriginalAuditSession) -> Result<Self> {
        let witnesses = add(source_count, 1, s)?;
        let padded = add(count, 31, s)?;
        s.semantic_work()
            .arithmetic(Op::DivideRemainder, &[&padded.into(), &32usize.into()])
            .map_err(core_error)?;
        let words = padded / 32;
        let mut bytes = product(count, mem::size_of::<u64>(), s)?;
        for extra in [
            product(witnesses, mem::size_of::<RootGroup>(), s)?,
            product(witnesses, mem::size_of::<Vec<u32>>(), s)?,
            product(product(witnesses, words, s)?, mem::size_of::<u32>(), s)?,
            mem::size_of::<Self>(),
        ] {
            bytes = add(bytes, extra, s)?;
        }
        let payload = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut by_ordinal = Vec::new();
        by_ordinal
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        let zeroed = product(count, mem::size_of::<u64>(), s)?;
        s.semantic_work().write_bytes(zeroed).map_err(core_error)?;
        by_ordinal.resize(count, 0);
        let mut packed = Vec::new();
        packed
            .try_reserve_exact(witnesses)
            .map_err(|_| ServiceError::Unavailable)?;
        for _ in 0..witnesses {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let mut row = Vec::new();
            row.try_reserve_exact(words)
                .map_err(|_| ServiceError::Unavailable)?;
            let zeroed = product(words, mem::size_of::<u32>(), s)?;
            s.semantic_work().write_bytes(zeroed).map_err(core_error)?;
            row.resize(words, 0);
            s.semantic_work()
                .write_bytes(mem::size_of::<Vec<u32>>())
                .map_err(core_error)?;
            packed.push(row);
        }
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(witnesses)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            by_ordinal,
            packed,
            groups,
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
        if let Some(position) = inventory.seed_position(reference, &self.groups[group].root, s)? {
            mark(
                &mut self.by_ordinal,
                &mut self.packed,
                position.ordinal as usize,
                bit(witness, s)?,
                s,
            )?;
        }
        Ok(())
    }
    fn close(
        &mut self,
        inventory: &mut inventory::Inventory,
        pointers:&ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        for first in (0..self.by_ordinal.len()).step_by(32).rev() {
            s.semantic_work().visit(Visit::Value).map_err(core_error)?;
            let block = pointers.block(first, s).map_err(core_error)?;
            let entries = block.entries(s).map_err(core_error)?;
            let end = add(first, entries.len(), s)?;
            for (ordinal, entry) in (first..end).rev().zip(entries.iter().rev()) {
                s.semantic_work().visit(Visit::Value).map_err(core_error)?;
                let membership = self.by_ordinal[ordinal];
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
                    let words = &mut self.by_ordinal;
                    let packed = &mut self.packed;
                    inventory.required_positions_at(
                        ordinal,
                        &group.root,
                        &mut |prior, s| {
                            require(
                                compare(prior.ordinal as usize, ordinal, s)?.is_lt(),
                                "INTEGRITY_FAILURE",
                            )?;
                            s.semantic_work()
                                .arithmetic(
                                    Op::Compare,
                                    &[&prior.sequence.into(), &entry.sequence().into()],
                                )
                                .map_err(core_error)?;
                            require(
                                u64::from(prior.sequence) < entry.sequence(),
                                "INTEGRITY_FAILURE",
                            )?;
                            mark(words, packed, prior.ordinal as usize, selected, s)
                        },
                        s,
                    )?;
                }
            }
        }
        Ok(())
    }
}
fn mark(
    words: &mut [u64],
    packed: &mut [Vec<u32>],
    ordinal: usize,
    mask: u64,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    require(
        compare(ordinal, words.len(), s)?.is_lt(),
        "INTEGRITY_FAILURE",
    )?;
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    let word = words
        .get_mut(ordinal)
        .ok_or(ServiceError::IntegrityFailure)?;
    let combined = union(*word, mask, s)?;
    s.semantic_work()
        .arithmetic(Op::Subtract, &[&combined.into(), &(*word).into()])
        .map_err(core_error)?;
    let mut delta = combined - *word;
    s.semantic_work()
        .write_bytes(mem::size_of::<u64>())
        .map_err(core_error)?;
    *word = combined;
    // Update only newly reached witness memberships, directly in the eventual
    // ordinal-packed mask rows. No H-way per-pointer transpose is performed.
    s.semantic_work()
        .arithmetic(Op::DivideRemainder, &[&ordinal.into(), &32usize.into()])
        .map_err(core_error)?;
    let (block, position) = (ordinal / 32, ordinal % 32);
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    let flag = BLOCK_BITS[position];
    while nonzero(delta, s)? {
        s.semantic_work()
            .arithmetic(Op::LeadingZeros, &[&delta.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(
                Op::Subtract,
                &[&63u32.into(), &delta.leading_zeros().into()],
            )
            .map_err(core_error)?;
        let witness = (63 - delta.leading_zeros()) as usize;
        let selected = bit(witness, s)?;
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&delta.into(), &selected.into()])
            .map_err(core_error)?;
        delta -= selected;
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let row = packed
            .get_mut(witness)
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        let value = row.get_mut(block).ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .arithmetic(Op::BitOr, &[&(*value).into(), &flag.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<u32>())
            .map_err(core_error)?;
        *value |= flag;
    }
    Ok(())
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
        let _control = s
            .reserve_external_payload(mem::size_of::<[&Value; 32]>())
            .map_err(core_error)?;
        let mut bodies = [financial.body; 32];
        for (target, header) in bodies.iter_mut().zip(sources) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<&Value>())
                .map_err(core_error)?;
            *target = header.body;
        }
        let memberships = self.witness_memberships(&bodies[..sources.len()], financial.body, s)?;
        self.stream_memberships(&memberships, sources.len(), &mut stream, None, s)?;
        stream.finish(s).map_err(core_error)
    }
    fn witness_memberships(
        &mut self,
        sources: &[&Value],
        financial: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Memberships> {
        let mut memberships = Memberships::new(self.pointers.len(), sources.len(), s)?;
        for (index, header) in sources.iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let group = memberships.group(*header, index, s)?;
            for name in ["enrollment_ref", "current_state_ref"] {
                memberships.seed(
                    &mut self._inventory,
                    group,
                    index,
                    request_wrapper::field(*header, name, s)?,
                    s,
                )?;
            }
            for grant in rows(*header, "original_grant_refs", s)? {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                memberships.seed(&mut self._inventory, group, index, grant, s)?;
            }
        }
        let f = sources.len();
        let group = memberships.group(financial, f, s)?;
        for reference in rows(financial, "preexisting_input_refs", s)? {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            memberships.seed(&mut self._inventory, group, f, reference, s)?;
        }
        for (name, field) in [("history_edges", "target_ref"), ("head_pins", "ref")] {
            for row in rows(financial, name, s)? {
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
        memberships.close(&mut self._inventory, &self.pointers, s)?;
        Ok(memberships)
    }
    /// Real first-pass prefix production uses the same original-completed
    /// inventory, exact root cuts, packed memberships and bounded raw reader.
    /// The actual current seed/header emitter still owns custody and framing.
    pub(super) fn produce_witness_prefixes<'p, 'k>(
        &'p mut self,
        key: &'k ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
        sources: &[SourcePrefixInput<'_>],
        financial: FinancialPrefixInput<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<WitnessPrefixProduction<'p, 'k>> {
        let result = self.produce_witness_prefixes_inner(key, sources, financial, s);
        if result.is_err() {
            s.invalidate();
        }
        result
    }
    fn produce_witness_prefixes_inner<'p, 'k>(
        &'p mut self,
        key: &'k ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
        sources: &[SourcePrefixInput<'_>],
        financial: FinancialPrefixInput<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<WitnessPrefixProduction<'p, 'k>> {
        require(!compare(sources.len(), 32, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        let control = s
            .reserve_external_payload(
                mem::size_of::<[&Value; 32]>() + mem::size_of::<WitnessPrefixProduction<'p, 'k>>(),
            )
            .map_err(core_error)?;
        let mut bodies = [financial.fields; 32];
        let mut producer =
            EconomicAuditStream::prefix_producer(key, sources.len(), s).map_err(core_error)?;
        for (target, input) in bodies.iter_mut().zip(sources) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<&Value>())
                .map_err(core_error)?;
            *target = input.fields;
            producer
                .add_source_prefix(
                    SourcePrefixInput {
                        fields: input.fields,
                        through: input.through,
                    },
                    s,
                )
                .map_err(core_error)?;
        }
        producer
            .set_financial_prefix(
                FinancialPrefixInput {
                    fields: financial.fields,
                    through: financial.through,
                },
                s,
            )
            .map_err(core_error)?;
        let memberships =
            self.witness_memberships(&bodies[..sources.len()], financial.fields, s)?;
        self.stream_memberships(&memberships, sources.len(), &mut producer, None, s)?;
        let produced = producer.finish_prefixes(s).map_err(core_error)?;
        Ok(WitnessPrefixProduction {
            prefix: self,
            key,
            produced,
            memberships,
            _control: control,
        })
    }
    fn stream_memberships(
        &mut self,
        memberships: &Memberships,
        source_count: usize,
        stream: &mut EconomicAuditStream<'_>,
        source_order: Option<&[usize]>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        // I/O remains32 fixed rows even while semantic mask preparation uses
        // a larger immutable table batch. No per-pointer builder or owner.
        const ROW: usize = ORIGIN_POINTER_MAX_BYTES;
        let control = mem::size_of::<[[u8; ROW]; 32]>()
            + mem::size_of::<[&[u8]; 32]>()
            + mem::size_of::<[&[u32]; 32]>()
            + mem::size_of::<[u32; 32]>()
            + mem::size_of::<[bool; 32]>();
        let _control = s.reserve_external_payload(control).map_err(core_error)?;
        let mut buffer = [[0u8; ROW]; 32];
        for first in (0..self.pointers.len()).step_by(1024) {
            s.semantic_work().visit(Visit::Value).map_err(core_error)?;
            let batch = self.pointers.batch(first, s).map_err(core_error)?;
            let entries = batch.located_entries(s).map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&first.into(), &32usize.into()])
                .map_err(core_error)?;
            let first_word = first / 32;
            let padded = add(entries.len(), 31, s)?;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&padded.into(), &32usize.into()])
                .map_err(core_error)?;
            let words = padded / 32;
            let end = add(first_word, words, s)?;
            let financial = &memberships.packed[source_count][first_word..end];
            let mut source_rows: [&[u32]; 32] = [&[]; 32];
            for (index, target) in source_rows.iter_mut().take(source_count).enumerate() {
                let selected = match source_order {
                    Some(order) => {
                        s.semantic_work()
                            .visit(Visit::RefLookup)
                            .map_err(core_error)?;
                        *order.get(index).ok_or(ServiceError::IntegrityFailure)?
                    }
                    None => index,
                };
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                let row = memberships
                    .packed
                    .get(selected)
                    .ok_or(ServiceError::IntegrityFailure)?;
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<&[u32]>())
                    .map_err(core_error)?;
                *target = &row[first_word..end];
            }
            let mut any = false;
            for row in std::iter::once(financial).chain(source_rows[..source_count].iter().copied())
            {
                for mask in row {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    if nonzero(u64::from(*mask), s)? {
                        any = true;
                        break;
                    }
                }
                if any {
                    break;
                }
            }
            if !any {
                continue;
            }
            let mut prepared = stream
                .prepare_origin_batch(&batch, financial, &source_rows[..source_count], s)
                .map_err(core_error)?;
            let mut union = [0u32; 32];
            let copied = product(words, mem::size_of::<u32>(), s)?;
            s.semantic_work().scan_bytes(copied).map_err(core_error)?;
            s.semantic_work().write_bytes(copied).map_err(core_error)?;
            union[..words].copy_from_slice(prepared.union_masks());
            for (entries, mask) in entries.chunks(32).zip(&union[..words]) {
                s.semantic_work().visit(Visit::Value).map_err(core_error)?;
                let selected = flags(*mask, s)?;
                s.semantic_work()
                    .arithmetic(Op::PopulationCount, &[&(*mask).into()])
                    .map_err(core_error)?;
                let count = mask.count_ones() as usize;
                let mut raw = [b"".as_slice(); 32];
                let mut target = raw.iter_mut();
                for ((row, entry), used) in buffer.iter_mut().zip(entries).zip(selected) {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    if !used {
                        continue;
                    }
                    let output = entry.read_buffer(row);
                    self._inventory
                        .read_pointer_at(entry.scratch_offset(), output, s)?;
                    s.semantic_work()
                        .write_bytes(mem::size_of::<&[u8]>())
                        .map_err(core_error)?;
                    *target.next().ok_or(ServiceError::IntegrityFailure)? = output;
                }
                prepared.push_chunk(&raw[..count], s).map_err(core_error)?;
            }
            prepared.finish(s).map_err(core_error)?;
        }
        Ok(())
    }
}

/// The first pass's actual membership owner stays with its exact immutable
/// completed inventory while the host emits headers. No fresh graph walk,
/// serialized flag, replacement snapshot or new original meter is introduced.
pub(super) struct WitnessPrefixProduction<'p, 'k> {
    prefix: &'p mut FoldedInventory,
    key: &'k ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
    produced: ProducedOriginPrefixes,
    memberships: Memberships,
    _control: ExternalBufferLease,
}
impl WitnessPrefixProduction<'_, '_> {
    pub(super) fn prefixes(&self) -> &ProducedOriginPrefixes {
        &self.produced
    }
    pub(super) fn verify<'a>(
        self,
        context: &'a DraftContextData,
        sources: &[ObservationHeader<'a>],
        financial: ObservationHeader<'a>,
        s: &mut OriginalAuditSession,
    ) -> Result<RecomputedAuditDigest<'a>> {
        let result = (|| {
            let _control = s
                .reserve_external_payload(
                    mem::size_of::<[usize; 32]>() + mem::size_of::<[bool; 32]>(),
                )
                .map_err(core_error)?;
            require(
                compare(sources.len(), self.produced.sources().len(), s)?.is_eq(),
                "INTEGRITY_FAILURE",
            )?;
            for (original, current) in [
                (&self.key.logical_store_id, &context.key.logical_store_id),
                (&self.key.scope[0], &context.key.scope[0]),
                (&self.key.scope[1], &context.key.scope[1]),
                (&self.key.source, &context.key.source),
                (&self.key.operation_id, &context.key.operation_id),
            ] {
                request_wrapper::same_text(original, current, s)?;
            }
            request_wrapper::same_text(
                request_wrapper::text(financial.body, "prefix_digest", s)?,
                self.produced.financial().prefix_digest(),
                s,
            )?;
            // Headers are now in canonical Ref order. Match each to one actual
            // first-pass output; identical commitments have identical masks
            // absent a SHA collision. Core rehashes the actual seeds/pointers.
            let mut used = [false; 32];
            let mut order = [0usize; 32];
            let mut stream =
                EconomicAuditStream::new(context, financial.reference, s).map_err(core_error)?;
            for (slot, header) in order.iter_mut().zip(sources) {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let digest = request_wrapper::text(header.body, "prefix_digest", s)?;
                let mut selected = None;
                for (index, prefix) in self.produced.sources().iter().enumerate() {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    s.semantic_work()
                        .visit(Visit::RefLookup)
                        .map_err(core_error)?;
                    if used[index] {
                        continue;
                    }
                    if super::super::acceptance::same_recipe_text(
                        digest,
                        prefix.prefix_digest(),
                        s,
                    )? {
                        selected = Some(index);
                        break;
                    }
                }
                let index = selected.ok_or(ServiceError::IntegrityFailure)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<usize>() + 1)
                    .map_err(core_error)?;
                used[index] = true;
                *slot = index;
                stream
                    .add_source_header(header.reference, header.body, s)
                    .map_err(core_error)?;
            }
            stream
                .set_financial_header(financial.reference, financial.body, s)
                .map_err(core_error)?;
            self.prefix.stream_memberships(
                &self.memberships,
                sources.len(),
                &mut stream,
                Some(&order[..sources.len()]),
                s,
            )?;
            stream.finish(s).map_err(core_error)
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}
const FLAGS: [[bool; 8]; 256] = {
    let mut out = [[false; 8]; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut bit = 0;
        while bit < 8 {
            out[byte][bit] = (byte & (1 << bit)) != 0;
            bit += 1;
        }
        byte += 1;
    }
    out
};
fn flags(mask: u32, s: &mut OriginalAuditSession) -> Result<[bool; 32]> {
    s.semantic_work()
        .scan_bytes(mem::size_of::<u32>())
        .map_err(core_error)?;
    let mut out = [false; 32];
    s.semantic_work()
        .write_bytes(mem::size_of_val(&out))
        .map_err(core_error)?;
    for (target, byte) in out.chunks_exact_mut(8).zip(mask.to_le_bytes()) {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        s.semantic_work().scan_bytes(8).map_err(core_error)?;
        s.semantic_work().write_bytes(8).map_err(core_error)?;
        target.copy_from_slice(&FLAGS[usize::from(byte)]);
    }
    Ok(out)
}
