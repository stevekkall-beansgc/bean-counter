//! Private acquisition metadata for actual already-written required Ref edges.
//! No reachability, future witness or original-completion proof is computed.
use super::*;
use std::mem;
#[derive(Clone, Copy)]
pub(super) struct EdgeLocation {
    pub(super) _offset: u64,
    pub(super) _ordinary: u32,
    pub(super) _roots: u32,
    pub(super) _digest: [u8; 32],
    // Cumulative normative WB/body counts at this completed original cutoff.
    // These are private append-derived DATA, not serialized budget assertions.
    pub(super) _documents: u64,
    pub(super) _charged_bytes: u64,
}
pub(super) struct EdgeLocations {
    rows: Vec<EdgeLocation>,
    _ownership: Option<ExternalBufferLease>,
}
impl EdgeLocations {
    pub(super) fn new() -> Self {
        Self {
            rows: Vec::new(),
            _ownership: None,
        }
    }
    pub(super) fn append(
        &mut self,
        ordinal: u64,
        row: EdgeLocation,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(
            compare(self.rows.len() as u64, ordinal, s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        require(compare(ordinal, 100000, s)?.is_lt(), "INTEGRITY_FAILURE")?;
        if compare(self.rows.len() as u64, self.rows.capacity() as u64, s)?.is_eq() {
            let old = self.rows.capacity() as u64;
            let next = if compare(old, 0, s)?.is_eq() {
                32
            } else {
                let doubled = multiply(old, 2, s)?;
                if compare(doubled, 100000, s)?.is_gt() {
                    100000
                } else {
                    doubled
                }
            };
            let bytes = add(
                multiply(next, mem::size_of::<EdgeLocation>() as u64, s)?,
                mem::size_of::<Self>() as u64,
                s,
            )?;
            let owner = s
                .reserve_external_payload(bytes as usize)
                .map_err(core_error)?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(next as usize)
                .map_err(|_| ServiceError::Unavailable)?;
            let moved = multiply(
                self.rows.len() as u64,
                mem::size_of::<EdgeLocation>() as u64,
                s,
            )?;
            copied(moved as usize, s)?;
            replacement.append(&mut self.rows);
            let old = mem::replace(&mut self.rows, replacement);
            drop(old);
            self._ownership = Some(owner);
        }
        copied(mem::size_of::<EdgeLocation>(), s)?;
        self.rows.push(row);
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn get(&self, ordinal: usize, s: &mut OriginalAuditSession) -> Result<EdgeLocation> {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        copied(mem::size_of::<EdgeLocation>(), s)?;
        self.rows
            .get(ordinal)
            .copied()
            .ok_or(ServiceError::IntegrityFailure)
    }
}
