//! One semantic mask batch, consumed in bounded raw-pointer I/O chunks. Counts
//! and snapshot bounds are checked once per batch; raw bytes are still rebound
//! to each actual immutable table entry before they enter any commitment.
use super::*;

pub const ORIGIN_POINTER_BATCH_WORDS: usize = 32;
pub const ORIGIN_POINTER_BATCH: usize = ORIGIN_POINTER_BLOCK * ORIGIN_POINTER_BATCH_WORDS;
pub struct OriginPointerBatch<'a> {
    entries: &'a [Intrinsic],
    cancellation: &'a AuditCancellation,
    all_locations: bool,
}
pub struct PreparedOriginBatch<'s, 'c, 'p> {
    stream: &'s mut EconomicAuditStream<'c>,
    chunks: std::slice::Chunks<'p, Intrinsic>,
    masks: [[u32; ORIGIN_POINTER_BATCH_WORDS]; 33],
    union: [u32; ORIGIN_POINTER_BATCH_WORDS],
    started: [bool; 33],
    union_started: bool,
    next_word: usize,
    words: usize,
    actual_hashes: [u8; 32 * ORIGIN_POINTER_BLOCK],
    expected_hashes: [u8; 32 * ORIGIN_POINTER_BLOCK],
    last_sequence: u64,
    sealed: bool,
    _ownership: ExternalBufferLease,
}
impl Drop for PreparedOriginBatch<'_, '_, '_> {
    fn drop(&mut self) {
        if !self.sealed {
            self.stream.cancellation.cancel();
        }
    }
}
impl ValidatedOriginPointers {
    pub fn batch<'a>(
        &'a self,
        first: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<OriginPointerBatch<'a>> {
        let result = (|| {
            s.check_cancelled()?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            m.visit("audit.pointer_batch_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()) {
                return Err(fail("pointer batch attempt owner"));
            }
            if !w::compare(m, first, self.entries.len())?.is_lt() {
                return Err(fail("pointer batch ordinal"));
            }
            let proposed = w::usize_add(m, first, ORIGIN_POINTER_BATCH)?;
            let end = if w::compare(m, proposed, self.entries.len())?.is_gt() {
                self.entries.len()
            } else {
                proposed
            };
            Ok(OriginPointerBatch {
                entries: &self.entries[first..end],
                cancellation: &self.cancellation,
                all_locations: self.all_locations,
            })
        })();
        if result.is_err() {
            self.cancellation.cancel();
            s.invalidate();
        }
        result
    }
}
impl OriginPointerBatch<'_> {
    pub fn located_entries<'a>(
        &'a self,
        s: &mut OriginalAuditSession,
    ) -> Result<&'a [OriginPointerEntry]> {
        let result = (|| {
            let entries = self.entries(s)?;
            s.meter_mut().visit("audit.batch_locations_complete")?;
            if !self.all_locations {
                return Err(fail("pointer batch includes unlocated entries"));
            }
            Ok(entries)
        })();
        if result.is_err() {
            self.cancellation.cancel();
            s.invalidate();
        }
        result
    }
    /// A borrowed DATA view, with no extra per-entry validation/copy traversal.
    /// Actual selection and acquisition remain paid at their executing sites.
    pub fn entries<'a>(&'a self, s: &mut OriginalAuditSession) -> Result<&'a [OriginPointerEntry]> {
        let result = (|| {
            s.check_cancelled()?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            m.visit("audit.batch_entries_attempt")?;
            if !self.cancellation.same_attempt(&arena.cancellation_handle()) {
                return Err(fail("batch entry attempt owner"));
            }
            Ok(self.entries)
        })();
        if result.is_err() {
            self.cancellation.cancel();
            s.invalidate();
        }
        result
    }
}
fn last_selected(m: &mut CoreMeter, entries: &[Intrinsic], masks: &[u32]) -> Result<u64> {
    for (chunk, mask) in entries.chunks(ORIGIN_POINTER_BLOCK).zip(masks).rev() {
        m.visit("audit.batch_last_word")?;
        if !w::compare(m, *mask, 0u32)?.is_eq() {
            word(m, ArithmeticOperation::LeadingZeros, *mask, None)?;
            let last = w::usize_sub(m, 31, mask.leading_zeros() as usize)?;
            if !w::compare(m, last, chunk.len())?.is_lt() {
                return Err(fail("audit batch mask exceeds table"));
            }
            ValidationWork::visit(m, w::ValidationVisit::RefLookup)?;
            return Ok(chunk[last].sequence);
        }
    }
    Err(fail("audit batch empty mask"))
}
impl<'c> EconomicAuditStream<'c> {
    /// Source masks are actual fixed-point DATA, not verified flags. This
    /// preparation proves their exact shape/count/through associations, while
    /// the real chronological caller still establishes their membership.
    pub fn prepare_origin_batch<'s, 'p>(
        &'s mut self,
        batch: &OriginPointerBatch<'p>,
        financial_masks: &[u32],
        source_masks: &[&[u32]],
        s: &mut OriginalAuditSession,
    ) -> Result<PreparedOriginBatch<'s, 'c, 'p>> {
        let result = (|| {
            self.check_owner(s)?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            if !w::compare(m, self.phase, 1u8)?.is_eq()
                || !w::compare(m, source_masks.len(), self.sources.len())?.is_eq()
            {
                return Err(fail("audit batch phase/source count"));
            }
            m.visit("audit.batch_table_attempt")?;
            if !batch.cancellation.same_attempt(&self.cancellation) {
                return Err(fail("audit batch foreign table"));
            }
            if !w::compare(m, self.sources.len(), 32usize)?.is_le() {
                return Err(fail("audit batch source bound"));
            }
            let padded = w::usize_add(m, batch.entries.len(), ORIGIN_POINTER_BLOCK - 1)?;
            ValidationWork::arithmetic(
                m,
                ArithmeticOperation::DivideRemainder,
                &[&padded.into(), &ORIGIN_POINTER_BLOCK.into()],
            )?;
            let words = padded / ORIGIN_POINTER_BLOCK;
            if !w::compare(m, financial_masks.len(), words)?.is_eq() {
                return Err(fail("audit financial mask words"));
            }
            if let Some(previous) = self.last_sequence {
                if !w::compare(m, batch.entries[0].sequence, previous)?.is_gt() {
                    return Err(fail("audit batch order/duplicate"));
                }
            }
            let ownership = arena
                .reserve_external_payload(mem::size_of::<PreparedOriginBatch<'s, 'c, 'p>>(), m)?;
            m.write_bytes(
                mem::size_of::<[[u32; ORIGIN_POINTER_BATCH_WORDS]; 33]>()
                    + mem::size_of::<[u32; ORIGIN_POINTER_BATCH_WORDS]>()
                    + mem::size_of::<[bool; 33]>()
                    + 2 * 32 * ORIGIN_POINTER_BLOCK,
            )?;
            let mut masks = [[0u32; ORIGIN_POINTER_BATCH_WORDS]; 33];
            let mut union = [0u32; ORIGIN_POINTER_BATCH_WORDS];
            let mut started = [false; 33];
            if self.reused_prefixes {
                for input in source_masks
                    .iter()
                    .copied()
                    .chain(std::iter::once(financial_masks))
                {
                    m.visit("audit.produced_union_witness")?;
                    if !w::compare(m, input.len(), words)?.is_eq() {
                        return Err(fail("produced union mask words"));
                    }
                    for (combined, mask) in union.iter_mut().zip(input) {
                        m.visit("audit.produced_union_word")?;
                        word(m, ArithmeticOperation::BitOr, *combined, Some(*mask))?;
                        *combined |= *mask;
                    }
                }
            } else {
                for (index, (state, input)) in self
                    .sources
                    .iter_mut()
                    .zip(source_masks.iter().copied())
                    .chain(
                        self.financial
                            .iter_mut()
                            .map(|state| (state, financial_masks)),
                    )
                    .enumerate()
                {
                    m.visit("audit.batch_witness")?;
                    if !w::compare(m, input.len(), words)?.is_eq() {
                        return Err(fail("audit source mask words"));
                    }
                    let mut count = 0u64;
                    for ((dst, combined), mask) in
                        masks[index].iter_mut().zip(union.iter_mut()).zip(input)
                    {
                        m.visit("audit.batch_mask_word")?;
                        word(m, ArithmeticOperation::BitOr, *combined, Some(*mask))?;
                        *combined |= *mask;
                        word(m, ArithmeticOperation::PopulationCount, *mask, None)?;
                        ValidationWork::arithmetic(
                            m,
                            ArithmeticOperation::Add,
                            &[&count.into(), &u64::from(mask.count_ones()).into()],
                        )?;
                        count = count
                            .checked_add(u64::from(mask.count_ones()))
                            .ok_or_else(|| fail("audit batch mask count overflow"))?;
                        m.scan_bytes(mem::size_of::<u32>())?;
                        m.write_bytes(mem::size_of::<u32>())?;
                        *dst = *mask;
                    }
                    started[index] = !w::compare(m, state.count, 0u64)?.is_eq();
                    if !w::compare(m, count, 0u64)?.is_eq() {
                        let last = last_selected(m, batch.entries, input)?;
                        if w::compare(m, last, state.through)?.is_gt() {
                            return Err(fail("audit batch exceeds witness snapshot"));
                        }
                        ValidationWork::arithmetic(
                            m,
                            ArithmeticOperation::Add,
                            &[&state.count.into(), &count.into()],
                        )?;
                        state.count = state
                            .count
                            .checked_add(count)
                            .ok_or_else(|| fail("audit batch witness count overflow"))?;
                    }
                }
            }
            let last_sequence = last_selected(m, batch.entries, &union[..words])?;
            let union_started = !w::compare(m, self.union_count, 0u64)?.is_eq();
            Ok((
                ownership,
                masks,
                union,
                started,
                words,
                last_sequence,
                union_started,
            ))
        })();
        match result {
            Ok((ownership, masks, union, started, words, last_sequence, union_started)) => {
                Ok(PreparedOriginBatch {
                    stream: self,
                    chunks: batch.entries.chunks(ORIGIN_POINTER_BLOCK),
                    masks,
                    union,
                    started,
                    union_started,
                    next_word: 0,
                    words,
                    actual_hashes: [0; 32 * ORIGIN_POINTER_BLOCK],
                    expected_hashes: [0; 32 * ORIGIN_POINTER_BLOCK],
                    last_sequence,
                    sealed: false,
                    _ownership: ownership,
                })
            }
            Err(error) => {
                self.phase = 3;
                self.cancellation.cancel();
                s.invalidate();
                Err(error)
            }
        }
    }
}
impl PreparedOriginBatch<'_, '_, '_> {
    /// Already computed exact union DATA for the host's bounded raw reader.
    /// Borrowing this view creates no new membership or completion authority.
    pub fn union_masks(&self) -> &[u32] {
        &self.union[..self.words]
    }
    /// Consume exactly the next <=32 physical table entries. Only selected
    /// canonical bodies are supplied, in numeric entry order. An empty mask
    /// word consumes no body but must still advance this exact chunk.
    pub fn push_chunk(
        &mut self,
        canonical_pointers: &[&[u8]],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let result = (|| {
            self.stream.check_owner(s)?;
            let arena = s.arena().shared_handle();
            let m = s.meter_mut();
            m.visit("audit.batch_next_chunk")?;
            let entries = self
                .chunks
                .next()
                .ok_or_else(|| fail("audit batch extra chunk"))?;
            let union = self.union[self.next_word];
            word(m, ArithmeticOperation::PopulationCount, union, None)?;
            let count = union.count_ones();
            if !w::compare(m, canonical_pointers.len(), count as usize)?.is_eq() {
                return Err(fail("audit batch chunk raw count"));
            }
            let selected = flags(m, union)?;
            // A full-union witness needs no per-pointer membership test. Its
            // unconditional byte fanout executes only representation hashing;
            // partial masks retain their actual semantic membership decision.
            m.write_bytes(
                2 * mem::size_of::<[Option<(usize, &mut Prefix<'_>)>; 33]>()
                    + mem::size_of::<[[bool; ORIGIN_POINTER_BLOCK]; 33]>(),
            )?;
            let mut uniform: [Option<(usize, &mut Prefix<'_>)>; 33] = std::array::from_fn(|_| None);
            let mut partial: [Option<(usize, &mut Prefix<'_>)>; 33] = std::array::from_fn(|_| None);
            let mut memberships = [[false; ORIGIN_POINTER_BLOCK]; 33];
            let mut uniform_slots = uniform.iter_mut();
            let mut partial_slots = partial.iter_mut();
            if !self.stream.reused_prefixes {
                for (index, state) in self
                    .stream
                    .sources
                    .iter_mut()
                    .chain(self.stream.financial.iter_mut())
                    .enumerate()
                {
                    m.visit("audit.batch_chunk_witness")?;
                    let mask = self.masks[index][self.next_word];
                    if !w::compare(m, mask, 0u32)?.is_eq() {
                        if w::compare(m, mask, union)?.is_eq() {
                            *uniform_slots
                                .next()
                                .ok_or_else(|| fail("audit uniform witness capacity"))? =
                                Some((index, state));
                        } else {
                            memberships[index] = flags(m, mask)?;
                            *partial_slots
                                .next()
                                .ok_or_else(|| fail("audit partial witness capacity"))? =
                                Some((index, state));
                        }
                    }
                }
            }
            let mut raw = canonical_pointers.iter();
            // Fixed-width physical ordinal slots, including paired zero
            // sentinels for unselected/absent entries, make this one ACTUAL
            // byte-array equality. No per-digest equality executes below.
            m.write_bytes(
                mem::size_of_val(&self.actual_hashes) + mem::size_of_val(&self.expected_hashes),
            )?;
            self.actual_hashes.fill(0);
            self.expected_hashes.fill(0);
            for (position, ((entry, selected), (actual, expected))) in entries
                .iter()
                .zip(selected)
                .zip(
                    self.actual_hashes
                        .chunks_exact_mut(32)
                        .zip(self.expected_hashes.chunks_exact_mut(32)),
                )
                .enumerate()
            {
                m.visit("audit.batch_chunk_slot")?;
                if !selected {
                    continue;
                }
                let raw = raw
                    .next()
                    .ok_or_else(|| fail("audit batch missing canonical pointer"))?;
                let digest =
                    m.hash_owned_representation(&arena, raw, RepresentationKind::Record)?;
                m.scan_bytes(64)?;
                m.write_bytes(64)?;
                actual.copy_from_slice(&digest);
                expected.copy_from_slice(&entry.digest);
                for slot in uniform.iter_mut().take_while(|slot| slot.is_some()) {
                    let (index, state) = slot
                        .as_mut()
                        .ok_or_else(|| fail("audit uniform witness gap"))?;
                    if self.started[*index] {
                        syntax(m, &mut state.hash, b",")?;
                    }
                    bytes(m, &mut state.hash, raw)?;
                    self.started[*index] = true;
                }
                for slot in partial.iter_mut().take_while(|slot| slot.is_some()) {
                    let (index, state) = slot
                        .as_mut()
                        .ok_or_else(|| fail("audit partial witness gap"))?;
                    m.visit("audit.batch_partial_membership")?;
                    if memberships[*index][position] {
                        if self.started[*index] {
                            syntax(m, &mut state.hash, b",")?;
                        }
                        bytes(m, &mut state.hash, raw)?;
                        self.started[*index] = true;
                    }
                }
                if let Some(union) = self.stream.union_digest.as_mut() {
                    bytes(m, union, &digest)?;
                }
                if let Some(combined) = self.stream.combined.as_mut() {
                    if self.union_started {
                        syntax(m, combined, b",")?;
                    }
                    bytes(m, combined, raw)?;
                }
                self.union_started = true;
            }
            ValidationWork::visit(m, w::ValidationVisit::MemberCompare)?;
            m.scan_bytes(mem::size_of_val(&self.actual_hashes))?;
            m.scan_bytes(mem::size_of_val(&self.expected_hashes))?;
            if self.actual_hashes != self.expected_hashes {
                return Err(fail("audit batch exact ordered pointer bytes"));
            }
            ValidationWork::arithmetic(
                m,
                ArithmeticOperation::Add,
                &[&self.stream.union_count.into(), &u64::from(count).into()],
            )?;
            self.stream.union_count = self
                .stream
                .union_count
                .checked_add(u64::from(count))
                .ok_or_else(|| fail("audit batch union count overflow"))?;
            self.next_word = w::usize_add(m, self.next_word, 1)?;
            Ok(())
        })();
        if result.is_err() {
            self.stream.phase = 3;
            self.stream.cancellation.cancel();
            s.invalidate();
        }
        result
    }
    pub fn finish(mut self, s: &mut OriginalAuditSession) -> Result<()> {
        let result = (|| {
            self.stream.check_owner(s)?;
            let m = s.meter_mut();
            if !w::compare(m, self.next_word, self.words)?.is_eq() {
                return Err(fail("audit batch incomplete raw chunks"));
            }
            self.stream.last_sequence = Some(self.last_sequence);
            self.sealed = true;
            Ok(())
        })();
        if result.is_err() {
            self.stream.phase = 3;
            self.stream.cancellation.cancel();
            s.invalidate();
        }
        result
    }
}
