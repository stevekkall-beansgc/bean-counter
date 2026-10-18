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
mod headers;

pub(in super::super) struct ObservationHeader<'a> {
    pub(in super::super) reference: &'a ExactRef,
    pub(in super::super) body: &'a Value,
}
// These fixed bit constants contain no executed runtime shifts. Every dynamic
// selection and actual word operation below is paid on the consuming parent.
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
fn nonzero(a: u64, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.into(), &0u64.into()])
        .map_err(core_error)?;
    Ok(a != 0)
}
struct RootGroup {
    root: ConfiguredRoot,
}
/// Derived minimal-witness counters, not an assertion that custody, authority
/// or the full resource envelope has completed.
#[derive(Clone, Copy, Default)]
pub(super) struct WitnessAccounting {
    pub(super) documents: u64,
    pub(super) retained_bytes: u64,
}
fn add_counter(a: u64, b: u64, s: &mut OriginalAuditSession) -> Result<u64> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::IntegrityFailure)
}
impl WitnessAccounting {
    fn include(&mut self, counts: (u64, u64), s: &mut OriginalAuditSession) -> Result<()> {
        self.documents = add_counter(self.documents, counts.0, s)?;
        self.retained_bytes = add_counter(self.retained_bytes, counts.1, s)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<Self>())
            .map_err(core_error)
    }
    fn compare_header(&self, body: &Value, s: &mut OriginalAuditSession) -> Result<()> {
        for (name, expected) in [
            ("document_count", self.documents),
            ("audited_retained_bytes", self.retained_bytes),
        ] {
            let raw = request_wrapper::text(body, name, s)?;
            let actual = super::super::acceptance::counter(raw, s)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&actual.into(), &expected.into()])
                .map_err(core_error)?;
            require(actual == expected, "INTEGRITY_FAILURE")?;
        }
        Ok(())
    }
}
struct Memberships {
    count: usize,
    witness_roots: Vec<usize>,
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
        let mut bytes = product(witnesses, mem::size_of::<usize>(), s)?;
        for extra in [
            product(witnesses, mem::size_of::<RootGroup>(), s)?,
            product(witnesses, mem::size_of::<Vec<u32>>(), s)?,
            product(product(witnesses, words, s)?, mem::size_of::<u32>(), s)?,
            mem::size_of::<Self>(),
        ] {
            bytes = add(bytes, extra, s)?;
        }
        let payload = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut witness_roots = Vec::new();
        witness_roots
            .try_reserve_exact(witnesses)
            .map_err(|_| ServiceError::Unavailable)?;
        let zeroed = product(witnesses, mem::size_of::<usize>(), s)?;
        s.semantic_work().write_bytes(zeroed).map_err(core_error)?;
        witness_roots.resize(witnesses, 0);
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
            count,
            witness_roots,
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

        for (i, group) in self.groups.iter_mut().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if group.root.same(&root, s)? {
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<usize>())
                    .map_err(core_error)?;
                *self
                    .witness_roots
                    .get_mut(witness)
                    .ok_or(ServiceError::IntegrityFailure)? = i;
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
        self.groups.push(RootGroup { root });
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<usize>())
            .map_err(core_error)?;
        *self
            .witness_roots
            .get_mut(witness)
            .ok_or(ServiceError::IntegrityFailure)? = index;
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
            require(
                compare(position.ordinal as usize, self.count, s)?.is_lt(),
                "INTEGRITY_FAILURE",
            )?;
            s.semantic_work()
                .arithmetic(
                    Op::DivideRemainder,
                    &[&(position.ordinal as usize).into(), &32usize.into()],
                )
                .map_err(core_error)?;
            let word = position.ordinal as usize / 32;
            let bit = position.ordinal as usize % 32;
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let flag = BLOCK_BITS[bit];
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let row = self
                .packed
                .get_mut(witness)
                .ok_or(ServiceError::IntegrityFailure)?;
            merge_word(row, word, flag, s)?;
        }
        Ok(())
    }
    fn close(
        &mut self,
        inventory: &mut inventory::Inventory,
        _pointers: &ledgerlab_core::policy::workflow::transition_constructor::ValidatedOriginPointers,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let _reader = s
            .reserve_external_payload(inventory::REQUIRED_EDGE_READER_PAYLOAD)
            .map_err(core_error)?;
        // The actual output masks ARE the reachability representation. An
        // earlier dependency is ORed as one ordinal word; there is no per-new
        // witness-bit transpose or secondary by-origin membership array.
        for (row, group) in self.packed.iter_mut().zip(&self.witness_roots) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let root = &self
                .groups
                .get(*group)
                .ok_or(ServiceError::IntegrityFailure)?
                .root;
            for word in (0..row.len()).rev() {
                s.semantic_work().visit(Visit::Value).map_err(core_error)?;
                let base = product(word, 32, s)?;
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                let pending = row[word];
                if compare(pending as usize, u32::MAX as usize, s)?.is_eq() {
                    // All32 origins already selected: no scalar bit discovery,
                    // test, or membership write is performed for these rows.
                    let end = add(base, 32, s)?;
                    for ordinal in (base..end).rev() {
                        s.semantic_work().visit(Visit::Value).map_err(core_error)?;
                        inventory.required_word_masks_at(
                            ordinal,
                            root,
                            &mut |prior_word, mask, s| merge_word(row, prior_word, mask, s),
                            s,
                        )?;
                    }
                    continue;
                }
                if !nonzero(u64::from(pending), s)? {
                    continue;
                }
                let proposed = add(base, 32, s)?;
                let end = if compare(proposed, self.count, s)?.is_gt() {
                    self.count
                } else {
                    proposed
                };
                s.semantic_work()
                    .arithmetic(Op::Subtract, &[&end.into(), &base.into()])
                    .map_err(core_error)?;
                let length = end - base;
                // One checked split retains the actual current-word cell.
                // Every selector reads it afresh: newly reached earlier bits
                // are observed without a second frontier or pending-bit OR.
                let (earlier, current_and_later) = row.split_at_mut(word);
                let (current, _) = current_and_later
                    .split_first_mut()
                    .ok_or(ServiceError::IntegrityFailure)?;
                for (ordinal, selected) in
                    (base..end).rev().zip(WORD_SELECTORS[..length].iter().rev())
                {
                    s.semantic_work().visit(Visit::Value).map_err(core_error)?;
                    if !selected(*current, s)? {
                        continue;
                    }
                    inventory.required_word_masks_at(
                        ordinal,
                        root,
                        &mut |prior_word, mask, s| {
                            if compare(prior_word, word, s)?.is_eq() {
                                s.semantic_work()
                                    .arithmetic(Op::BitOr, &[&(*current).into(), &mask.into()])
                                    .map_err(core_error)?;
                                s.semantic_work()
                                    .write_bytes(mem::size_of::<u32>())
                                    .map_err(core_error)?;
                                *current |= mask;
                                Ok(())
                            } else {
                                merge_word(earlier, prior_word, mask, s)
                            }
                        },
                        s,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn accounting(
        &self,
        inventory: &mut inventory::Inventory,
        roots: &[&super::super::wire::SourceRecord],
        s: &mut OriginalAuditSession,
    ) -> Result<[WitnessAccounting; 33]> {
        s.semantic_work()
            .write_bytes(mem::size_of::<[WitnessAccounting; 33]>())
            .map_err(core_error)?;
        let mut out = [WitnessAccounting::default(); 33];
        for ((row, group), total) in self.packed.iter().zip(&self.witness_roots).zip(&mut out) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let root = &self
                .groups
                .get(*group)
                .ok_or(ServiceError::IntegrityFailure)?
                .root;
            let mut root_body = None;
            for record in roots {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let reference = super::super::wire::reference(record.reference(), s)?;
                let parsed = s
                    .parse_representation(reference.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let candidate = ConfiguredRoot::from_value(parsed.value(), s)?;
                if root.same(&candidate, s)? {
                    require(root_body.is_none(), "INTEGRITY_FAILURE")?;
                    root_body = Some(record.bytes());
                }
            }
            let root_body = root_body.ok_or(ServiceError::IntegrityFailure)?;
            let root_position = inventory.root_position(root, s)?;
            let mut root_selected = false;
            if let Some(position) = root_position {
                s.semantic_work()
                    .arithmetic(
                        Op::DivideRemainder,
                        &[&position.ordinal.into(), &32u32.into()],
                    )
                    .map_err(core_error)?;
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                let mask = *row
                    .get((position.ordinal / 32) as usize)
                    .ok_or(ServiceError::IntegrityFailure)?;
                s.semantic_work()
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                let bit = BLOCK_BITS[(position.ordinal % 32) as usize];
                s.semantic_work()
                    .arithmetic(Op::BitAnd, &[&mask.into(), &bit.into()])
                    .map_err(core_error)?;
                root_selected = nonzero(u64::from(mask & bit), s)?;
            }
            // The configured root is exported even when its origin is cut or
            // when its fresh authenticated snapshot has no earlier origin.
            if !root_selected {
                total.include((1, root_body.len() as u64), s)?;
            }
            for (word, mask) in row.iter().enumerate() {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                if !nonzero(u64::from(*mask), s)? {
                    continue;
                }
                let base = product(word, 32, s)?;
                let end = add(base, 32, s)?;
                let end = if compare(end, self.count, s)?.is_gt() {
                    self.count
                } else {
                    end
                };
                if compare(*mask as usize, u32::MAX as usize, s)?.is_eq() {
                    total.include(inventory.introduced_accounting(base, end, s)?, s)?;
                } else {
                    let selected = flags(*mask, s)?;
                    for (ordinal, used) in (base..end).zip(selected) {
                        s.semantic_work()
                            .visit(Visit::Element)
                            .map_err(core_error)?;
                        if used {
                            let end = add(ordinal, 1, s)?;
                            total.include(inventory.introduced_accounting(ordinal, end, s)?, s)?;
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}
fn merge_word(row: &mut [u32], word: usize, mask: u32, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    let target = row.get_mut(word).ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::BitOr, &[&(*target).into(), &mask.into()])
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<u32>())
        .map_err(core_error)?;
    *target |= mask;
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
    pub(super) fn produce_witness_prefixes<'p, 'k, 's>(
        &'p mut self,
        key: &'k ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
        sources: &[SourcePrefixInput<'_>],
        financial: FinancialPrefixInput<'_>,
        s: &'s mut OriginalAuditSession,
    ) -> Result<WitnessPrefixProduction<'p, 'k, 's>> {
        let result = self.produce_witness_prefixes_inner(key, sources, financial, s);
        let (produced, memberships, control) = match result {
            Ok(value) => value,
            Err(error) => {
                s.invalidate();
                return Err(error);
            }
        };
        Ok(WitnessPrefixProduction {
            prefix: self,
            key,
            produced,
            memberships,
            accounting: None,
            session: s,
            _control: control,
        })
    }
    fn produce_witness_prefixes_inner(
        &mut self,
        key: &ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
        sources: &[SourcePrefixInput<'_>],
        financial: FinancialPrefixInput<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<(ProducedOriginPrefixes, Memberships, ExternalBufferLease)> {
        require(!compare(sources.len(), 32, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        let control = s
            .reserve_external_payload(
                mem::size_of::<[&Value; 32]>()
                    + mem::size_of::<WitnessPrefixProduction<'_, '_, '_>>(),
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
        Ok((produced, memberships, control))
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
pub(super) struct WitnessPrefixProduction<'p, 'k, 's> {
    prefix: &'p mut FoldedInventory,
    key: &'k ledgerlab_core::policy::workflow::transition_constructor::OperationKey,
    produced: ProducedOriginPrefixes,
    memberships: Memberships,
    accounting: Option<[WitnessAccounting; 33]>,
    session: &'s mut OriginalAuditSession,
    _control: ExternalBufferLease,
}
impl WitnessPrefixProduction<'_, '_, '_> {
    /// Bind actual owning root bodies and derive counts over this first pass's
    /// exact memberships. The borrowed result cannot outlive this same parent
    /// and inventory. No installation WB total or serialized count is accepted.
    pub(super) fn derive_accounting(
        &mut self,
        roots: &[&super::super::wire::SourceRecord],
    ) -> Result<()> {
        let result = (|| {
            let _temporary = self
                .session
                .reserve_external_payload(2 * mem::size_of::<[WitnessAccounting; 33]>())
                .map_err(core_error)?;
            let accounting =
                self.memberships
                    .accounting(&mut self.prefix._inventory, roots, self.session)?;
            self.session
                .semantic_work()
                .scan_bytes(mem::size_of_val(&accounting))
                .map_err(core_error)?;
            self.session
                .semantic_work()
                .write_bytes(mem::size_of_val(&accounting))
                .map_err(core_error)?;
            self.accounting = Some(accounting);
            Ok(())
        })();
        if result.is_err() {
            self.session.invalidate();
        }
        result
    }
    pub(super) fn accounting_parts(
        &mut self,
    ) -> Result<(&[WitnessAccounting], &mut OriginalAuditSession)> {
        let accounting = self
            .accounting
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        Ok((&accounting[..self.memberships.packed.len()], self.session))
    }
    /// Header emission borrows this SAME retained consumer, never new credit.
    pub(super) fn session(&mut self) -> &mut OriginalAuditSession {
        self.session
    }

    pub(super) fn prefixes(&self) -> &ProducedOriginPrefixes {
        &self.produced
    }
    pub(super) fn verify<'a>(
        self,
        context: &'a DraftContextData,
        sources: &[ObservationHeader<'a>],
        financial: ObservationHeader<'a>,
    ) -> Result<RecomputedAuditDigest<'a>> {
        let WitnessPrefixProduction {
            prefix,
            key,
            produced,
            memberships,
            accounting,
            session: s,
            _control,
        } = self;
        let result = (|| {
            let accounting = accounting.ok_or(ServiceError::IntegrityFailure)?;
            let _control = s
                .reserve_external_payload(
                    mem::size_of::<[usize; 32]>() + mem::size_of::<[bool; 32]>(),
                )
                .map_err(core_error)?;
            require(
                compare(sources.len(), produced.sources().len(), s)?.is_eq(),
                "INTEGRITY_FAILURE",
            )?;
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            accounting
                .get(produced.sources().len())
                .ok_or(ServiceError::IntegrityFailure)?
                .compare_header(financial.body, s)?;
            for (original, current) in [
                (&key.logical_store_id, &context.key.logical_store_id),
                (&key.scope[0], &context.key.scope[0]),
                (&key.scope[1], &context.key.scope[1]),
                (&key.source, &context.key.source),
                (&key.operation_id, &context.key.operation_id),
            ] {
                request_wrapper::same_text(original, current, s)?;
            }
            request_wrapper::same_text(
                request_wrapper::text(financial.body, "prefix_digest", s)?,
                produced.financial().prefix_digest(),
                s,
            )?;
            // Headers are now in canonical Ref order. Match each to one actual
            // first-pass output; identical commitments have identical masks
            // absent a SHA collision. Core binds these actual header seeds to
            // the produced facts and rechecks the exact ordered raw union.
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
                for (index, prefix) in produced.sources().iter().enumerate() {
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
                    .visit(Visit::RefLookup)
                    .map_err(core_error)?;
                accounting
                    .get(index)
                    .ok_or(ServiceError::IntegrityFailure)?
                    .compare_header(header.body, s)?;
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
            stream
                .reuse_produced_prefixes(produced, &order[..sources.len()], s)
                .map_err(core_error)?;
            prefix.stream_memberships(
                &memberships,
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
fn word_flag<const BYTE: usize, const BIT: usize>(
    mask: u32,
    s: &mut OriginalAuditSession,
) -> Result<bool> {
    s.semantic_work()
        .scan_bytes(mem::size_of::<u32>())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<u32>())
        .map_err(core_error)?;
    let bytes = mask.to_le_bytes();
    s.semantic_work()
        .visit(Visit::RefLookup)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(mem::size_of::<bool>())
        .map_err(core_error)?;
    Ok(FLAGS[usize::from(bytes[BYTE])][BIT])
}
// Constant byte/bit offsets, not runtime arithmetic or a semantic transpose.
// Each function is an existing FLAGS lookup against the live packed word.
const WORD_SELECTORS: [fn(u32, &mut OriginalAuditSession) -> Result<bool>; 32] = [
    word_flag::<0, 0>,
    word_flag::<0, 1>,
    word_flag::<0, 2>,
    word_flag::<0, 3>,
    word_flag::<0, 4>,
    word_flag::<0, 5>,
    word_flag::<0, 6>,
    word_flag::<0, 7>,
    word_flag::<1, 0>,
    word_flag::<1, 1>,
    word_flag::<1, 2>,
    word_flag::<1, 3>,
    word_flag::<1, 4>,
    word_flag::<1, 5>,
    word_flag::<1, 6>,
    word_flag::<1, 7>,
    word_flag::<2, 0>,
    word_flag::<2, 1>,
    word_flag::<2, 2>,
    word_flag::<2, 3>,
    word_flag::<2, 4>,
    word_flag::<2, 5>,
    word_flag::<2, 6>,
    word_flag::<2, 7>,
    word_flag::<3, 0>,
    word_flag::<3, 1>,
    word_flag::<3, 2>,
    word_flag::<3, 3>,
    word_flag::<3, 4>,
    word_flag::<3, 5>,
    word_flag::<3, 6>,
    word_flag::<3, 7>,
];
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

#[cfg(test)]
impl FoldedInventory {
    pub(in super::super) fn assert_packed_accounting(
        &mut self,
        root: &super::super::wire::SourceRecord,
        packed: &[u32],
        expected: (u64, u64),
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let mut masks = Memberships::new(self.pointers.len(), 0, s)?;
        let reference = super::super::wire::reference(root.reference(), s)?;
        let parsed = s
            .parse_representation(reference.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        masks.group(&serde_json::json!({"root_ref":parsed.value()}), 0, s)?;
        masks.packed[0].copy_from_slice(packed);
        let _temporary = s
            .reserve_external_payload(2 * mem::size_of::<[WitnessAccounting; 33]>())
            .map_err(core_error)?;
        let counts = masks.accounting(&mut self._inventory, &[root], s)?;
        assert_eq!((counts[0].documents, counts[0].retained_bytes), expected);
        Ok(())
    }
    pub(in super::super) fn assert_packed_reachable(
        &mut self,
        root: &Value,
        seeds: &[&Value],
        expected: &[u32],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let mut masks = Memberships::new(self.pointers.len(), 0, s)?;
        let group = masks.group(&serde_json::json!({"root_ref":root}), 0, s)?;
        for reference in seeds {
            masks.seed(&mut self._inventory, group, 0, reference, s)?;
        }
        masks.close(&mut self._inventory, &self.pointers, s)?;
        assert_eq!(masks.packed[0], expected);
        Ok(())
    }
}
