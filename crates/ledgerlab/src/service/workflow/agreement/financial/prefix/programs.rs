//! Live compilation correspondence keys for this ONE chronological attempt.
//! A numeric slot/sequence is only a locator. The enclosing fold registers it
//! after actual original PUB completion and checks every immutable body again
//! when loading an execution view. Keys never leave the held snapshot.
use super::*;
use ledgerlab_core::policy::workflow::selected_program::{
    SelectedExecutionReloadKey, SelectedProgram,
};
use std::mem;

struct Entry {
    sequence: i64,
    key: Box<SelectedExecutionReloadKey>,
}
pub(super) struct CompletedPrograms {
    entries: Vec<Entry>,
    // Entry payload drops before its reservation; individual Box payloads are
    // owned by the core's opaque key, not charged a second time here.
    _control: Option<ExternalBufferLease>,
}
impl CompletedPrograms {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            _control: None,
        }
    }
    pub(super) fn register(
        &mut self,
        sequence: i64,
        program: &SelectedProgram,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let work = s.semantic_work();
        work.visit(Visit::Constraint).map_err(core_error)?;
        work.arithmetic(Op::Compare, &[&sequence.into(), &0i64.into()])
            .map_err(core_error)?;
        require(sequence > 0, "INTEGRITY_FAILURE")?;
        if let Some(last) = self.entries.last() {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&sequence.into(), &last.sequence.into()])
                .map_err(core_error)?;
            require(sequence > last.sequence, "INTEGRITY_FAILURE")?;
        }
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&self.entries.len().into(), &100000usize.into()],
            )
            .map_err(core_error)?;
        require(self.entries.len() < 100000, "INTEGRITY_FAILURE")?;
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&self.entries.len().into(), &self.entries.capacity().into()],
            )
            .map_err(core_error)?;
        if self.entries.len() == self.entries.capacity() {
            let old = self.entries.capacity();
            s.semantic_work()
                .arithmetic(Op::Compare, &[&old.into(), &0usize.into()])
                .map_err(core_error)?;
            let capacity = if old == 0 {
                1
            } else {
                s.semantic_work()
                    .arithmetic(Op::Multiply, &[&old.into(), &2usize.into()])
                    .map_err(core_error)?;
                let doubled = old.checked_mul(2).ok_or(ServiceError::Unavailable)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&doubled.into(), &100000usize.into()])
                    .map_err(core_error)?;
                doubled.min(100000)
            };
            s.semantic_work()
                .arithmetic(
                    Op::Multiply,
                    &[&capacity.into(), &mem::size_of::<Entry>().into()],
                )
                .map_err(core_error)?;
            let bytes = capacity
                .checked_mul(mem::size_of::<Entry>())
                .ok_or(ServiceError::Unavailable)?;
            // New payload is admitted while the old vector/lease still lives.
            let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
            let mut next = Vec::new();
            next.try_reserve_exact(capacity)
                .map_err(|_| ServiceError::Unavailable)?;
            s.semantic_work()
                .arithmetic(
                    Op::Multiply,
                    &[&self.entries.len().into(), &mem::size_of::<Entry>().into()],
                )
                .map_err(core_error)?;
            let moved = self
                .entries
                .len()
                .checked_mul(mem::size_of::<Entry>())
                .ok_or(ServiceError::Unavailable)?;
            s.semantic_work().scan_bytes(moved).map_err(core_error)?;
            s.semantic_work().write_bytes(moved).map_err(core_error)?;
            next.extend(self.entries.drain(..));
            let old = mem::replace(&mut self.entries, next);
            drop(old);
            self._control = Some(owner);
        }
        // The core pre-admits the actual Box, checks same-attempt owning compile
        // provenance, and refuses a key minted from any reloaded execution view.
        let key = program
            .execution_reload_key_with_work(s)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<Entry>())
            .map_err(core_error)?;
        self.entries.push(Entry { sequence, key });
        Ok(())
    }
    pub(super) fn key(
        &self,
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<&SelectedExecutionReloadKey> {
        let (mut low, mut high) = (0usize, self.entries.len());
        loop {
            s.semantic_work()
                .arithmetic(Op::Compare, &[&low.into(), &high.into()])
                .map_err(core_error)?;
            require(low < high, "INTEGRITY_FAILURE")?;
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&high.into(), &low.into()])
                .map_err(core_error)?;
            let width = high - low;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&width.into(), &2usize.into()])
                .map_err(core_error)?;
            let half = width / 2;
            s.semantic_work()
                .arithmetic(Op::Add, &[&low.into(), &half.into()])
                .map_err(core_error)?;
            let middle = low + half;
            let entry = &self.entries[middle];
            s.semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&entry.sequence.into(), &sequence.into()])
                .map_err(core_error)?;
            match entry.sequence.cmp(&sequence) {
                std::cmp::Ordering::Equal => return Ok(&entry.key),
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Less => {
                    s.semantic_work()
                        .arithmetic(Op::Add, &[&middle.into(), &1usize.into()])
                        .map_err(core_error)?;
                    low = middle + 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::selected_program::{
            SelectedCompilerSupport, SelectedSourceInput, compile_selected_publication,
            rehydrate_selected_execution,
        },
    };
    fn source() -> SelectedSourceInput {
        let value: Value = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json"
        )))
        .unwrap();
        SelectedSourceInput {
            documents: value["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| canonical::CanonicalBytes::from_value(v).unwrap().into_vec())
                .collect(),
            root_policy_ref: serde_json::from_value(value["root_policy_ref"].clone()).unwrap(),
        }
    }
    #[test]
    fn live_completed_pub_table_keeps_grown_keys_for_original_reload() {
        // Actual draft compiler correspondence only. These test sequence
        // locators do not claim PUB custody or authorize financial admission.
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut original = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let support = SelectedCompilerSupport::builtin().unwrap();
        let program = compile_selected_publication(source(), &support, &mut original).unwrap();
        let plan = program.compiled_plan().canonical_bytes().to_vec();
        let mut table = CompletedPrograms::new();
        for sequence in [2, 5, 9] {
            table.register(sequence, &program, &mut original).unwrap();
        }
        assert!(table.register(9, &program, &mut original).is_err());
        drop(program);
        drop(original);
        let mut consumer = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        for sequence in [9, 2, 5] {
            let key = table.key(sequence, &mut consumer).unwrap();
            let loaded = rehydrate_selected_execution(source(), &plan, key, &mut consumer).unwrap();
            assert_eq!(loaded.compiled_plan().canonical_bytes(), plan);
            loaded
                .context_schema_with_work(consumer.semantic_work())
                .unwrap();
            assert_eq!(loaded.execution_registry(), None);
        }
        assert!(table.key(7, &mut consumer).is_err());
        drop(table);
        drop(consumer);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
