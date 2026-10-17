//! Invocation-local introduction inventory. This is ephemeral DATA storage;
//! only the chronological semantic driver may credit an introduction. No file,
//! offset, index row or matching digest is an origin capability.
use super::*;
use std::{
    cmp::Ordering as Cmp,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

const ENTRY_BYTES: usize = 256;
const KEY_BYTES: usize = 216;

/// The closed saved recipes generate ASCII digest-derived IDs. This key is
/// their exact (digest,id,kind) JCS set order, with zero padding after strings.
/// A lookup never degrades to digest-only identity.
#[derive(Clone, Copy)]
struct Entry([u8; ENTRY_BYTES]);
/// Untrusted lookup key. This is representation only: an exact byte match can
/// select DATA from the same attempt, never finish an originating certificate.
struct Locator([u8; KEY_BYTES]);
trait Key {
    fn key(&self) -> &[u8];
}
impl Key for Entry {
    fn key(&self) -> &[u8] {
        &self.0[..KEY_BYTES]
    }
}
impl Key for Locator {
    fn key(&self) -> &[u8] {
        &self.0
    }
}
impl Entry {
    fn compare(&self, other: &impl Key, session: &mut OriginalAuditSession) -> Result<Cmp> {
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        Ok(self.key().cmp(other.key()))
    }
}
impl Locator {
    fn from_ref(
        kind: &str,
        id: &str,
        digest: &str,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        // These checks only make the fixed representation injective. The
        // digest's hex grammar is NOT authenticated here. A malformed locator
        // cannot equal the fully validated key of a completed introduction.
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(digest.len())
            .map_err(core_error)?;
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            compare(hex.len() as u64, 64, session)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        for (text, maximum) in [(id, 104usize), (kind, 48usize)] {
            session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(text.len())
                .map_err(core_error)?;
            require(
                !text.is_empty()
                    && !compare(text.len() as u64, maximum as u64, session)?.is_gt()
                    && text.bytes().all(|b| b.is_ascii() && b != 0),
                "INTEGRITY_FAILURE",
            )?;
        }
        session
            .semantic_work()
            .write_bytes(KEY_BYTES)
            .map_err(core_error)?;
        let mut output = Self([0; KEY_BYTES]);
        for (position, bytes) in [
            (0, hex.as_bytes()),
            (64, id.as_bytes()),
            (168, kind.as_bytes()),
        ] {
            let end = add(position as u64, bytes.len() as u64, session)? as usize;
            copied(bytes.len(), session)?;
            output.0[position..end].copy_from_slice(bytes);
        }
        Ok(output)
    }
}
// A degree-eight B-tree fits an entire node in one sub-4 KiB disk page.
// Root splits and child splits move only these pages, never the prior history.
const DEGREE: usize = 8;
const MAX_KEYS: usize = DEGREE * 2 - 1;
const HEADER: usize = 16;
const CHILDREN: usize = HEADER + MAX_KEYS * ENTRY_BYTES;
const PAGE_BYTES: usize = CHILDREN + (MAX_KEYS + 1) * 8;

fn io(error: std::io::Error) -> ServiceError {
    crate::service::store_error(crate::store::errors::StoreError::Io(error))
}
fn arithmetic(kind: Op, left: u64, right: u64, session: &mut OriginalAuditSession) -> Result<()> {
    session
        .semantic_work()
        .arithmetic(kind, &[&left.into(), &right.into()])
        .map_err(core_error)
}
fn add(left: u64, right: u64, session: &mut OriginalAuditSession) -> Result<u64> {
    arithmetic(Op::Add, left, right, session)?;
    left.checked_add(right)
        .ok_or(ServiceError::IntegrityFailure)
}
fn subtract(left: u64, right: u64, session: &mut OriginalAuditSession) -> Result<u64> {
    arithmetic(Op::Subtract, left, right, session)?;
    left.checked_sub(right)
        .ok_or(ServiceError::IntegrityFailure)
}
fn multiply(left: u64, right: u64, session: &mut OriginalAuditSession) -> Result<u64> {
    arithmetic(Op::Multiply, left, right, session)?;
    left.checked_mul(right)
        .ok_or(ServiceError::IntegrityFailure)
}
fn compare(left: u64, right: u64, session: &mut OriginalAuditSession) -> Result<Cmp> {
    arithmetic(Op::Compare, left, right, session)?;
    Ok(left.cmp(&right))
}
fn copied(bytes: usize, session: &mut OriginalAuditSession) -> Result<()> {
    let work = session.semantic_work();
    work.scan_bytes(bytes).map_err(core_error)?;
    work.write_bytes(bytes).map_err(core_error)
}

// Each payload has one same-attempt arena reservation, acquired before Vec
// allocation. Declaration order drops the payload before its reservation.
struct Page {
    bytes: Vec<u8>,
    _lease: ExternalBufferLease,
}
impl Page {
    fn new(session: &mut OriginalAuditSession) -> Result<Self> {
        let lease = session
            .reserve_external_payload(PAGE_BYTES + std::mem::size_of::<Vec<u8>>())
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(PAGE_BYTES)
            .map_err(core_error)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PAGE_BYTES)
            .map_err(|_| ServiceError::Unavailable)?;
        bytes.resize(PAGE_BYTES, 0);
        Ok(Self {
            bytes,
            _lease: lease,
        })
    }
    fn count(&self) -> usize {
        u16::from_le_bytes([self.bytes[0], self.bytes[1]]) as usize
    }
    fn leaf(&self) -> bool {
        self.bytes[2] == 1
    }
    fn initialize(&mut self, leaf: bool, session: &mut OriginalAuditSession) -> Result<()> {
        session
            .semantic_work()
            .write_bytes(PAGE_BYTES)
            .map_err(core_error)?;
        self.bytes.fill(0);
        session
            .semantic_work()
            .write_bytes(CHILDREN - HEADER)
            .map_err(core_error)?;
        self.bytes[HEADER..CHILDREN].fill(255);
        self.bytes[2] = u8::from(leaf);
        Ok(())
    }
    fn set_count(&mut self, n: usize, session: &mut OriginalAuditSession) -> Result<()> {
        require(
            !compare(n as u64, MAX_KEYS as u64, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        session.semantic_work().write_bytes(2).map_err(core_error)?;
        self.bytes[..2].copy_from_slice(&(n as u16).to_le_bytes());
        let start = Self::key_offset(n, session)?;
        let length = subtract(CHILDREN as u64, start as u64, session)? as usize;
        session
            .semantic_work()
            .write_bytes(length)
            .map_err(core_error)?;
        self.bytes[start..CHILDREN].fill(255);
        Ok(())
    }
    fn key_offset(index: usize, session: &mut OriginalAuditSession) -> Result<usize> {
        Ok(add(
            HEADER as u64,
            multiply(index as u64, ENTRY_BYTES as u64, session)?,
            session,
        )? as usize)
    }
    fn child_offset(index: usize, session: &mut OriginalAuditSession) -> Result<usize> {
        Ok(add(
            CHILDREN as u64,
            multiply(index as u64, 8, session)?,
            session,
        )? as usize)
    }
    fn entry(&self, index: usize, session: &mut OriginalAuditSession) -> Result<Entry> {
        require(
            compare(index as u64, self.count() as u64, session)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        let start = Self::key_offset(index, session)?;
        let end = add(start as u64, ENTRY_BYTES as u64, session)? as usize;
        copied(ENTRY_BYTES, session)?;
        let mut entry = Entry([0; ENTRY_BYTES]);
        entry.0.copy_from_slice(&self.bytes[start..end]);
        Ok(entry)
    }
    fn child(&self, index: usize, session: &mut OriginalAuditSession) -> Result<u64> {
        require(
            !compare(index as u64, self.count() as u64, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let start = Self::child_offset(index, session)?;
        let end = add(start as u64, 8, session)? as usize;
        session.semantic_work().scan_bytes(8).map_err(core_error)?;
        let mut encoded = [0; 8];
        encoded.copy_from_slice(&self.bytes[start..end]);
        Ok(u64::from_le_bytes(encoded))
    }
    fn set_child(
        &mut self,
        index: usize,
        page: u64,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(
            !compare(index as u64, MAX_KEYS as u64, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let start = Self::child_offset(index, session)?;
        let end = add(start as u64, 8, session)? as usize;
        copied(8, session)?;
        self.bytes[start..end].copy_from_slice(&page.to_le_bytes());
        Ok(())
    }
    fn compare_at<const POSITION: usize>(
        &self,
        key: &impl Key,
        session: &mut OriginalAuditSession,
    ) -> Result<Cmp> {
        // Both offsets are compile-time constants, not an uncharged runtime
        // search-index multiplication. Unused slots are private +infinity
        // sentinels; all constructed keys begin with an ASCII hex digit.
        let start = const { HEADER + POSITION * ENTRY_BYTES };
        let end = const { HEADER + POSITION * ENTRY_BYTES + KEY_BYTES };
        session
            .semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        Ok(self.bytes[start..end].cmp(key.key()))
    }
    fn placement<const POSITION: usize>(
        &self,
        found: bool,
        session: &mut OriginalAuditSession,
    ) -> Result<(usize, bool, u64)> {
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        let child = if found || self.leaf() {
            0
        } else {
            let start = const { CHILDREN + POSITION * 8 };
            let end = const { CHILDREN + POSITION * 8 + 8 };
            copied(8, session)?;
            let mut bytes = [0; 8];
            bytes.copy_from_slice(&self.bytes[start..end]);
            u64::from_le_bytes(bytes)
        };
        Ok((POSITION, found, child))
    }
    fn position(
        &self,
        key: &impl Key,
        session: &mut OriginalAuditSession,
    ) -> Result<(usize, bool, u64)> {
        // Exact ordinary binary-search decision tree over 15 slots. Offsets
        // are constants, including the selected child pointer. Runtime index
        // arithmetic is not executed then waived from accounting.
        macro_rules! last {
            ($p:literal,$next:literal) => {
                match self.compare_at::<$p>(key, session)? {
                    Cmp::Equal => self.placement::<$p>(true, session),
                    Cmp::Less => self.placement::<$next>(false, session),
                    Cmp::Greater => self.placement::<$p>(false, session),
                }
            };
        }
        macro_rules! branch {
            ($p:literal,$left:expr,$right:expr) => {
                match self.compare_at::<$p>(key, session)? {
                    Cmp::Equal => self.placement::<$p>(true, session),
                    Cmp::Greater => $left,
                    Cmp::Less => $right,
                }
            };
        }
        branch!(
            7,
            branch!(
                3,
                branch!(1, last!(0, 1), last!(2, 3)),
                branch!(5, last!(4, 5), last!(6, 7))
            ),
            branch!(
                11,
                branch!(9, last!(8, 9), last!(10, 11)),
                branch!(13, last!(12, 13), last!(14, 15))
            )
        )
    }
    fn insert(
        &mut self,
        index: usize,
        key: &Entry,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let count = self.count();
        require(
            compare(count as u64, MAX_KEYS as u64, session)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        require(
            !compare(index as u64, count as u64, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let start = Self::key_offset(index, session)?;
        let end = Self::key_offset(count, session)?;
        let destination = add(start as u64, ENTRY_BYTES as u64, session)? as usize;
        let shifted = subtract(end as u64, start as u64, session)? as usize;
        copied(shifted, session)?;
        self.bytes.copy_within(start..end, destination);
        let key_end = add(start as u64, ENTRY_BYTES as u64, session)? as usize;
        copied(ENTRY_BYTES, session)?;
        self.bytes[start..key_end].copy_from_slice(&key.0);
        self.set_count(add(count as u64, 1, session)? as usize, session)
    }
    fn insert_child(
        &mut self,
        index: usize,
        page: u64,
        old_count: usize,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let start = Self::child_offset(index, session)?;
        let end = Self::child_offset(add(old_count as u64, 1, session)? as usize, session)?;
        let destination = add(start as u64, 8, session)? as usize;
        copied(
            subtract(end as u64, start as u64, session)? as usize,
            session,
        )?;
        self.bytes.copy_within(start..end, destination);
        self.set_child(index, page, session)
    }
}

// The shared-path request stack contains only page/range locators. It never
// retains a transitive document graph or an authenticated/completed flag.
#[derive(Clone, Copy)]
struct DemandRange {
    page: u64,
    first: usize,
    end: usize,
    remaining: u64,
}
struct DemandStack {
    ranges: Vec<DemandRange>,
    maximum: usize,
    _lease: ExternalBufferLease,
}
impl DemandStack {
    fn new(maximum: usize, session: &mut OriginalAuditSession) -> Result<Self> {
        let payload = multiply(
            maximum as u64,
            std::mem::size_of::<DemandRange>() as u64,
            session,
        )?;
        let bytes = add(
            payload,
            std::mem::size_of::<Vec<DemandRange>>() as u64,
            session,
        )?;
        let lease = session
            .reserve_external_payload(bytes as usize)
            .map_err(core_error)?;
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(maximum)
            .map_err(|_| ServiceError::Unavailable)?;
        Ok(Self {
            ranges,
            maximum,
            _lease: lease,
        })
    }
    fn push(&mut self, range: DemandRange, session: &mut OriginalAuditSession) -> Result<()> {
        require(
            compare(self.ranges.len() as u64, self.maximum as u64, session)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        session
            .semantic_work()
            .visit(Visit::Edge)
            .map_err(core_error)?;
        session
            .semantic_work()
            .write_bytes(std::mem::size_of::<DemandRange>())
            .map_err(core_error)?;
        self.ranges.push(range);
        Ok(())
    }
    fn pop(&mut self, session: &mut OriginalAuditSession) -> Result<Option<DemandRange>> {
        session
            .semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        session
            .semantic_work()
            .scan_bytes(std::mem::size_of::<DemandRange>())
            .map_err(core_error)?;
        Ok(self.ranges.pop())
    }
}

/// Private, non-restartable B-tree. The caller supplies a newly created empty
/// private file; no offset or index content is accepted from an incoming cursor.
/// A repeated key preserves the FIRST completed introduction, never replaces it.
struct IntroductionIndex {
    file: File,
    root: u64,
    pages: u64,
    height: u64,
    entries: u64,
    comparisons_reserved: u64,
    buffers: [Page; 3],
    scratch: ScratchSpace,
    invalid: bool,
}
impl IntroductionIndex {
    fn new(
        file: File,
        operations: u64,
        payload_bytes: u64,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        session.check_cancelled().map_err(core_error)?;
        require(file.metadata().map_err(io)?.len() == 0, "INTEGRITY_FAILURE")?;
        let buffers = [
            Page::new(session)?,
            Page::new(session)?,
            Page::new(session)?,
        ];
        let mut output = Self {
            file,
            root: 0,
            pages: 0,
            height: 1,
            entries: 0,
            comparisons_reserved: 0,
            buffers,
            scratch: ScratchSpace::for_profile(operations, payload_bytes, session)?,
            invalid: false,
        };
        let root = output.allocate(session)?;
        output.buffers[0].initialize(true, session)?;
        output.write(root, 0, session)?;
        Ok(output)
    }
    fn reserve_comparisons(
        &mut self,
        levels: u64,
        per_level: u64,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        let next = add(
            self.comparisons_reserved,
            multiply(levels, per_level, session)?,
            session,
        )?;
        if compare(next, self.scratch.comparison_limit, session)?.is_gt() {
            return Err(ServiceError::Unavailable);
        }
        self.comparisons_reserved = next;
        Ok(())
    }
    fn allocate(&mut self, session: &mut OriginalAuditSession) -> Result<u64> {
        self.scratch.admit(PAGE_BYTES as u64, session)?;
        let page = self.pages;
        self.pages = add(self.pages, 1, session)?;
        Ok(page)
    }
    fn read(&mut self, page: u64, buffer: usize, session: &mut OriginalAuditSession) -> Result<()> {
        require(
            compare(page, self.pages, session)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        let offset = multiply(page, PAGE_BYTES as u64, session)?;
        session.check_cancelled().map_err(core_error)?;
        self.file.seek(SeekFrom::Start(offset)).map_err(io)?;
        session
            .semantic_work()
            .write_bytes(PAGE_BYTES)
            .map_err(core_error)?;
        self.file
            .read_exact(&mut self.buffers[buffer].bytes)
            .map_err(io)?;
        session.check_cancelled().map_err(core_error)?;
        let p = &self.buffers[buffer];
        require(
            !compare(p.count() as u64, MAX_KEYS as u64, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(p.bytes[2] <= 1, "INTEGRITY_FAILURE")
    }
    fn write(
        &mut self,
        page: u64,
        buffer: usize,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(
            compare(page, self.pages, session)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        let offset = multiply(page, PAGE_BYTES as u64, session)?;
        session.check_cancelled().map_err(core_error)?;
        self.file.seek(SeekFrom::Start(offset)).map_err(io)?;
        copied(PAGE_BYTES, session)?;
        self.file
            .write_all(&self.buffers[buffer].bytes)
            .map_err(io)?;
        session.check_cancelled().map_err(core_error)
    }
    fn find(&mut self, key: &Locator, session: &mut OriginalAuditSession) -> Result<Option<Entry>> {
        let result = self.find_inner(key, session);
        if result.is_err() {
            self.invalid = true;
            session.invalidate();
        }
        result
    }
    fn find_inner(
        &mut self,
        key: &Locator,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<Entry>> {
        require(!self.invalid, "INTEGRITY_FAILURE")?;
        // Four comparisons per level is an admission bound, not a measurement
        // of short-circuit comparisons; the executing comparisons still pay S.
        self.reserve_comparisons(self.height, 4, session)?;
        let (mut page, mut remaining) = (self.root, self.height);
        loop {
            require(compare(remaining, 0, session)?.is_gt(), "INTEGRITY_FAILURE")?;
            self.read(page, 0, session)?;
            let (position, found, child) = self.buffers[0].position(key, session)?;
            if found {
                return self.buffers[0].entry(position, session).map(Some);
            }
            if self.buffers[0].leaf() {
                return Ok(None);
            }
            page = child;
            remaining = subtract(remaining, 1, session)?;
        }
    }
    /// Resolve a sorted manifest/recipe demand set by its shared tree paths.
    /// Each visited page is read once for this batch, rather than once for
    /// every member. Results identify the original demand POSITION; callback
    /// order is intentionally unspecified. Neither a hit nor the callback
    /// proves an origin. Only the driver may consume an already-completed
    /// introduction and every absent entry still needs actual local semantics.
    fn find_sorted(
        &mut self,
        keys: &[Locator],
        session: &mut OriginalAuditSession,
        mut consume: impl FnMut(usize, Option<Entry>, &mut OriginalAuditSession) -> Result<()>,
    ) -> Result<()> {
        let result = self.find_sorted_inner(keys, session, &mut consume);
        if result.is_err() {
            self.invalid = true;
            session.invalidate();
        }
        result
    }
    fn find_sorted_inner(
        &mut self,
        keys: &[Locator],
        session: &mut OriginalAuditSession,
        consume: &mut impl FnMut(usize, Option<Entry>, &mut OriginalAuditSession) -> Result<()>,
    ) -> Result<()> {
        require(!self.invalid, "INTEGRITY_FAILURE")?;
        // Sorted-demand validation executes key comparisons as well. Reserve
        // them in the SAME aggregate index ledger before executing them;
        // the per-level binary-search allowance below covers searches only.
        let adjacent = if compare(keys.len() as u64, 0, session)?.is_gt() {
            subtract(keys.len() as u64, 1, session)?
        } else {
            0
        };
        self.reserve_comparisons(adjacent, 1, session)?;
        for adjacent in keys.windows(2) {
            session
                .semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(KEY_BYTES)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(KEY_BYTES)
                .map_err(core_error)?;
            require(adjacent[0].key() < adjacent[1].key(), "INTEGRITY_FAILURE")?;
        }
        if keys.is_empty() {
            return Ok(());
        }
        // Same aggregate comparison ledger as point reads, with no refund or
        // extra S allowance. Executing comparisons below still pay actual S.
        self.reserve_comparisons(
            multiply(self.height, keys.len() as u64, session)?,
            4,
            session,
        )?;
        // At most15 pending siblings per active tree level, and no more
        // nonempty ranges than input demands. No arbitrary semantic depth cap.
        let path_max = add(multiply(self.height, MAX_KEYS as u64, session)?, 1, session)?;
        let maximum = if compare(path_max, keys.len() as u64, session)?.is_lt() {
            path_max as usize
        } else {
            keys.len()
        };
        let mut pending = DemandStack::new(maximum, session)?;
        pending.push(
            DemandRange {
                page: self.root,
                first: 0,
                end: keys.len(),
                remaining: self.height,
            },
            session,
        )?;
        while let Some(range) = pending.pop(session)? {
            require(
                compare(range.remaining, 0, session)?.is_gt(),
                "INTEGRITY_FAILURE",
            )?;
            self.read(range.page, 0, session)?;
            let remaining = subtract(range.remaining, 1, session)?;
            let mut child_group: Option<(usize, usize, u64)> = None;
            let mut last_end = range.first;
            for (relative, key) in keys[range.first..range.end].iter().enumerate() {
                session
                    .semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let index = add(range.first as u64, relative as u64, session)? as usize;
                let (position, found, child) = self.buffers[0].position(key, session)?;
                let leaf = self.buffers[0].leaf();
                let new_group = match child_group {
                    Some((old_position, _, _)) => {
                        !compare(old_position as u64, position as u64, session)?.is_eq() || found
                    }
                    None => false,
                };
                if new_group {
                    let (_, first, old_child) = child_group.take().expect("present range");
                    pending.push(
                        DemandRange {
                            page: old_child,
                            first,
                            end: index,
                            remaining,
                        },
                        session,
                    )?;
                }
                if found {
                    let entry = self.buffers[0].entry(position, session)?;
                    consume(index, Some(entry), session)?;
                } else if leaf {
                    consume(index, None, session)?;
                } else if child_group.is_none() {
                    child_group = Some((position, index, child));
                }
                last_end = add(index as u64, 1, session)? as usize;
            }
            if let Some((_, first, child)) = child_group {
                pending.push(
                    DemandRange {
                        page: child,
                        first,
                        end: last_end,
                        remaining,
                    },
                    session,
                )?;
            }
        }
        Ok(())
    }
    fn insert(&mut self, key: &Entry, session: &mut OriginalAuditSession) -> Result<Option<Entry>> {
        let result = self.insert_inner(key, session);
        if result.is_err() {
            self.invalid = true;
            session.invalidate();
        }
        result
    }
    fn insert_inner(
        &mut self,
        key: &Entry,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<Entry>> {
        require(!self.invalid, "INTEGRITY_FAILURE")?;
        // At most four binary-search comparisons and one split comparison per
        // descended level, including the possible additional root level; unused reservations
        // are not refunded or described as observed comparisons.
        let maximum_levels = add(self.height, 1, session)?;
        self.reserve_comparisons(maximum_levels, 5, session)?;
        self.read(self.root, 0, session)?;
        if compare(self.buffers[0].count() as u64, MAX_KEYS as u64, session)?.is_eq() {
            let old_root = self.root;
            let root = self.allocate(session)?;
            self.buffers[1].initialize(false, session)?;
            self.buffers.swap(0, 1);
            self.buffers[0].set_child(0, old_root, session)?;
            self.split(root, 0, old_root, session)?;
            self.root = root;
            self.height = add(self.height, 1, session)?;
        }
        let (mut page, mut remaining) = (self.root, self.height);
        loop {
            require(compare(remaining, 0, session)?.is_gt(), "INTEGRITY_FAILURE")?;
            let (mut position, found, mut child) = self.buffers[0].position(key, session)?;
            if found {
                return self.buffers[0].entry(position, session).map(Some);
            }
            if self.buffers[0].leaf() {
                self.buffers[0].insert(position, key, session)?;
                self.write(page, 0, session)?;
                self.entries = add(self.entries, 1, session)?;
                return Ok(None);
            }
            self.read(child, 1, session)?;
            if compare(self.buffers[1].count() as u64, MAX_KEYS as u64, session)?.is_eq() {
                let right = self.split(page, position, child, session)?;
                let middle = self.buffers[0].entry(position, session)?;
                match middle.compare(key, session)? {
                    Cmp::Equal => return Ok(Some(middle)),
                    Cmp::Less => {
                        child = right;
                        self.buffers.swap(1, 2);
                        position = add(position as u64, 1, session)? as usize;
                    }
                    Cmp::Greater => {}
                }
            }
            // The child buffer is already loaded (or is the split output).
            let _selected_position = position;
            self.buffers.swap(0, 1);
            page = child;
            remaining = subtract(remaining, 1, session)?;
        }
    }
    // buffer0=nonfull parent, buffer1=full child; buffer2=new right sibling.
    fn split(
        &mut self,
        parent: u64,
        position: usize,
        left: u64,
        session: &mut OriginalAuditSession,
    ) -> Result<u64> {
        let median = self.buffers[1].entry(DEGREE - 1, session)?;
        let leaf = self.buffers[1].leaf();
        self.buffers[2].initialize(leaf, session)?;
        let right_start = Page::key_offset(DEGREE, session)?;
        let right_end = Page::key_offset(MAX_KEYS, session)?;
        let length = subtract(right_end as u64, right_start as u64, session)? as usize;
        let target_end = add(HEADER as u64, length as u64, session)? as usize;
        copied(length, session)?;
        let [_, left_buffer, right_buffer] = &mut self.buffers;
        right_buffer.bytes[HEADER..target_end]
            .copy_from_slice(&left_buffer.bytes[right_start..right_end]);
        if !leaf {
            let source = Page::child_offset(DEGREE, session)?;
            let length = multiply(DEGREE as u64, 8, session)? as usize;
            let end = add(source as u64, length as u64, session)? as usize;
            let target_end = add(CHILDREN as u64, length as u64, session)? as usize;
            copied(length, session)?;
            right_buffer.bytes[CHILDREN..target_end]
                .copy_from_slice(&left_buffer.bytes[source..end]);
        }
        self.buffers[1].set_count(DEGREE - 1, session)?;
        self.buffers[2].set_count(DEGREE - 1, session)?;
        let right = self.allocate(session)?;
        let count = self.buffers[0].count();
        self.buffers[0].insert(position, &median, session)?;
        let child_position = add(position as u64, 1, session)? as usize;
        self.buffers[0].insert_child(child_position, right, count, session)?;
        self.write(left, 1, session)?;
        self.write(right, 2, session)?;
        self.write(parent, 0, session)?;
        Ok(right)
    }
}

impl Entry {
    fn from_ref(
        kind: &str,
        id: &str,
        digest: &str,
        offset: u64,
        length: u32,
        sequence: u32,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(hex.len() == 64, "INTEGRITY_FAILURE")?;
        session
            .semantic_work()
            .scan_bytes(hex.len())
            .map_err(core_error)?;
        for byte in hex.bytes() {
            session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
                "INTEGRITY_FAILURE",
            )?;
        }
        // These are representation bounds of the closed digest-derived record
        // recipes, not limits on policy/business identifiers inside their body.
        for (text, maximum) in [(id, 104usize), (kind, 48usize)] {
            session
                .semantic_work()
                .arithmetic(Op::Compare, &[&text.len().into(), &maximum.into()])
                .map_err(core_error)?;
            session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            session
                .semantic_work()
                .scan_bytes(text.len())
                .map_err(core_error)?;
            require(
                !text.is_empty()
                    && text.len() <= maximum
                    && text.bytes().all(|b| b.is_ascii() && b != 0),
                "INTEGRITY_FAILURE",
            )?;
        }
        session
            .semantic_work()
            .write_bytes(ENTRY_BYTES)
            .map_err(core_error)?;
        let mut output = Self([0; ENTRY_BYTES]);
        for (position, bytes) in [
            (0, hex.as_bytes()),
            (64, id.as_bytes()),
            (168, kind.as_bytes()),
            (216, &offset.to_le_bytes()),
            (224, &length.to_le_bytes()),
            (228, &sequence.to_le_bytes()),
        ] {
            session
                .semantic_work()
                .scan_bytes(bytes.len())
                .map_err(core_error)?;
            session
                .semantic_work()
                .write_bytes(bytes.len())
                .map_err(core_error)?;
            let end = add(position as u64, bytes.len() as u64, session)? as usize;
            output.0[position..end].copy_from_slice(bytes);
        }
        Ok(output)
    }
}

/// Exact logical scratch-byte admission. This is a private disk reservation,
/// independent of (and never credit to) the unchanged semantic/arena allowance.
/// Its limit is computed from actual checked W/WB, not supplied in an ingress.
struct ScratchSpace {
    limit: u64,
    live: u64,
    peak: u64,
    comparison_limit: u64,
}
impl ScratchSpace {
    fn for_profile(
        operations: u64,
        payload_bytes: u64,
        session: &mut OriginalAuditSession,
    ) -> Result<Self> {
        require(
            !compare(operations, 100000, session)?.is_gt()
                && !compare(payload_bytes, 268435456, session)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&payload_bytes.into(), &16u64.into()])
            .map_err(core_error)?;
        let bodies = payload_bytes
            .checked_mul(16)
            .ok_or(ServiceError::IntegrityFailure)?;
        session
            .semantic_work()
            .arithmetic(Op::Multiply, &[&operations.into(), &4096u64.into()])
            .map_err(core_error)?;
        let wrappers = operations
            .checked_mul(4096)
            .ok_or(ServiceError::IntegrityFailure)?;
        Ok(Self {
            limit: add(bodies, wrappers, session)?,
            comparison_limit: multiply(add(payload_bytes, operations, session)?, 32, session)?,
            live: 0,
            peak: 0,
        })
    }
    fn admit(&mut self, bytes: u64, session: &mut OriginalAuditSession) -> Result<()> {
        let next = add(self.live, bytes, session)?;
        if compare(next, self.limit, session)?.is_gt() {
            return Err(ServiceError::Unavailable);
        }
        let peak = if compare(next, self.peak, session)?.is_gt() {
            next
        } else {
            self.peak
        };
        self.live = next;
        self.peak = peak;
        Ok(())
    }
    fn release(&mut self, bytes: u64, session: &mut OriginalAuditSession) -> Result<()> {
        self.live = subtract(self.live, bytes, session)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledgerlab_core::canonical::audit_arena::AuditArena;

    fn session(arena: &AuditArena) -> OriginalAuditSession {
        OriginalAuditSession::for_resource_profile_v2(arena).unwrap()
    }
    fn entry(n: u32, sequence: u32, work: &mut OriginalAuditSession) -> Entry {
        Entry::from_ref(
            "workflow-evidence",
            &format!("evidence-{n:064x}"),
            &format!("sha256:{n:064x}"),
            n as u64,
            120,
            sequence,
            work,
        )
        .unwrap()
    }
    #[test]
    fn paged_split_lookup_first_introduction_and_terminal_failure() {
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let mut work = session(&arena);
        let mut index =
            IntroductionIndex::new(tempfile::tempfile().unwrap(), 1000, 1_000_000, &mut work)
                .unwrap();
        // A coprime permutation forces both left/right splits and internal
        // descent. Each small group is a distinct original operation, not a
        // maintenance session funded outside that operation's allowance.
        for group in 0..16 {
            let mut work = session(&arena);
            for ordinal in group * 16..((group + 1) * 16).min(251) {
                let n = (ordinal * 37) % 251;
                let value = entry(n, n + 1, &mut work);
                assert!(index.insert(&value, &mut work).unwrap().is_none());
            }
        }
        assert!(index.height >= 3);
        assert_eq!(index.entries, 251);
        for group in 0..16 {
            let mut work = session(&arena);
            for n in group * 16..((group + 1) * 16).min(251) {
                let value = entry(n, 999, &mut work);
                let locator = Locator::from_ref(
                    "workflow-evidence",
                    &format!("evidence-{n:064x}"),
                    &format!("sha256:{n:064x}"),
                    &mut work,
                )
                .unwrap();
                let found = index.find(&locator, &mut work).unwrap().unwrap();
                assert_eq!(found.0, entry(n, n + 1, &mut work).0);
                let repeated = index.insert(&value, &mut work).unwrap().unwrap();
                assert_eq!(repeated.0, found.0);
            }
        }
        assert_eq!(index.entries, 251);
        let mut work = session(&arena);
        let absent = Locator::from_ref(
            "workflow-evidence",
            &format!("evidence-{:064x}", 1000),
            &format!("sha256:{:064x}", 1000),
            &mut work,
        )
        .unwrap();
        assert!(index.find(&absent, &mut work).unwrap().is_none());
        let file_length = index.file.metadata().unwrap().len();
        assert_eq!(file_length, index.pages * PAGE_BYTES as u64);
        assert_eq!(index.scratch.live, file_length);
        // A failed operation cannot be caught and followed by trusted use of
        // this partially updated index under another origin's fresh allowance.
        index.scratch.comparison_limit = index.comparisons_reserved;
        assert!(index.find(&absent, &mut work).is_err());
        let mut later = session(&arena);
        assert!(index.find(&absent, &mut later).is_err());
        assert_eq!(index.file.metadata().unwrap().len(), file_length);
        drop(index);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
}

/// Existing paged index plus raw canonical-body spool for one real held audit.
/// Only the chronological caller inserts its validated introduction group.
/// Logical head entries share this SAME tree/quota; they are not stored Refs.
pub(in super::super) struct Inventory {
    tree: IntroductionIndex,
    bodies: File,
    operations: u64,
    payload_bytes: u64,
    body_bytes: u64,
    active_origin: Option<OriginLinks>,
}
/// Only one original is active. Retain exact required Ref-edge provenance
/// before projecting to origin numbers: each witness has its own exact root
/// cut, and an origin can own both a root and another required document.
/// Control/head I/O alone does not add an edge. Stable Entry slots plus a sorted
/// index avoid repeatedly moving full identities during insertion.
struct OriginLinks {
    sequence: u32,
    entries: Vec<Entry>,
    order: Vec<usize>,
    _storage: ExternalBufferLease,
}
pub(in super::super) struct StateKey(Locator);
/// Exact configured Ref DATA, prepared once per witness. This does not
/// authenticate a root body or turn any other local snapshot into a cut.
pub(in super::super) struct ConfiguredRoot(Locator);
impl ConfiguredRoot {
    pub(in super::super) fn from_value(
        value: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        super::super::acceptance::validate_reference(value, "workflow-authority", s)?;
        Ok(Self(Locator::from_ref(
            request_wrapper::text(value, "kind", s)?,
            request_wrapper::text(value, "id", s)?,
            request_wrapper::text(value, "digest", s)?,
            s,
        )?))
    }
}

impl StateKey {
    pub(in super::super) fn new(
        domain: &'static str,
        parts: &[&[u8]],
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let domain = wire::string(domain, s)?;
        let mut bytes = [b"".as_slice(); 12];
        require(
            !compare(parts.len() as u64, 4, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        bytes[0] = b"[";
        bytes[1] = domain.bytes();
        let mut n = 2usize;
        for part in parts {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let end = add(n as u64, 2, s)? as usize;
            s.semantic_work()
                .write_bytes(2 * std::mem::size_of::<&[u8]>())
                .map_err(core_error)?;
            let second = add(n as u64, 1, s)? as usize;
            bytes[n] = b",";
            bytes[second] = part;
            n = end;
        }
        bytes[n] = b"]";
        n = add(n as u64, 1, s)? as usize;
        let hash = wire::domain_digest(b"bean-counter/audit-local-state/1\0", &bytes[..n], s)?;
        let hash = s
            .parse_representation(hash.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let hash = hash
            .value()
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        Ok(Self(Locator::from_ref(
            "audit-local-state",
            "audit",
            hash,
            s,
        )?))
    }
}
impl Inventory {
    pub(in super::super) fn new(
        files: [File; 2],
        first: &LocatedWrappers,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        let [index, bodies] = files;
        let mut payload = first.identity().len() as u64;
        for n in first.wrappers().retained_lengths() {
            payload = add(payload, n as u64, s)?;
        }
        Ok(Self {
            tree: IntroductionIndex::new(index, 1, payload, s)?,
            bodies,
            operations: 1,
            payload_bytes: payload,
            body_bytes: 0,
            active_origin: None,
        })
    }
    fn extend(&mut self, operations: u64, bytes: u64, s: &mut OriginalAuditSession) -> Result<()> {
        let operations = add(self.operations, operations, s)?;
        let bytes = add(self.payload_bytes, bytes, s)?;
        let limits = ScratchSpace::for_profile(operations, bytes, s)?;
        require(
            !compare(self.tree.scratch.live, limits.limit, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        self.tree.scratch.limit = limits.limit;
        self.tree.scratch.comparison_limit = limits.comparison_limit;
        self.operations = operations;
        self.payload_bytes = bytes;
        Ok(())
    }
    pub(in super::super) fn next_origin(
        &mut self,
        origin: &LocatedWrappers,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let mut bytes = origin.identity().len() as u64;
        for n in origin.wrappers().retained_lengths() {
            bytes = add(bytes, n as u64, s)?;
        }
        self.extend(1, bytes, s)
    }
    pub(in super::super) fn has(
        &mut self,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        let key = Locator::from_ref(&r.kind, &r.id, &r.digest, s)?;
        let Some(entry) = self.tree.find(&key, s)? else {
            return Ok(false);
        };
        Ok(true)
    }
    /// Exact earliest introduction in this completed-prefix inventory. This
    /// locator is not itself an origin or compilation proof.
    pub(super) fn introduction_sequence(
        &mut self,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<i64> {
        let key = Locator::from_ref(
            request_wrapper::text(reference, "kind", s)?,
            request_wrapper::text(reference, "id", s)?,
            request_wrapper::text(reference, "digest", s)?,
            s,
        )?;
        let entry = self
            .tree
            .find(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        self.observe_origin_entry(&entry, s)?;
        copied(8, s)?;
        let mut sequence = [0u8; 8];
        sequence.copy_from_slice(&entry.0[228..236]);
        let sequence = u64::from_le_bytes(sequence);
        require(compare(sequence, 0, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        require(!compare(sequence, 100000, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        Ok(sequence as i64)
    }
    pub(in super::super) fn require_value(
        &mut self,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let kind = request_wrapper::text(reference, "kind", s)?;
        let (reference, _lease) = current::source::from_value(reference, kind, s)?;
        let key = Locator::from_ref(&reference.kind, &reference.id, &reference.digest, s)?;
        let entry = self
            .tree
            .find(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        self.observe_origin_entry(&entry, s)
    }
    pub(in super::super) fn compare_record(
        &mut self,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        expected: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let key = Locator::from_ref(&r.kind, &r.id, &r.digest, s)?;
        let entry = self
            .tree
            .find(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        self.observe_origin_entry(&entry, s)?;
        self.compare_body(&entry, expected, s)
    }
    fn compare_body(
        &mut self,
        entry: &Entry,
        expected: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        copied(12, s)?;
        let mut offset = [0; 8];
        offset.copy_from_slice(&entry.0[216..224]);
        let mut length = [0; 4];
        length.copy_from_slice(&entry.0[224..228]);
        let offset = u64::from_le_bytes(offset);
        let length = u32::from_le_bytes(length) as u64;
        require(
            compare(length, expected.len() as u64, s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        let end = add(offset, length, s)?;
        require(
            !compare(end, self.body_bytes, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        s.check_cancelled().map_err(core_error)?;
        self.bodies.seek(SeekFrom::Start(offset)).map_err(io)?;
        // Fixed audit I/O buffer, independent of the metered arena. No full
        // prior body/Value copy or generic mutable payload escapes this read.
        let mut buffer = [0u8; 262144];
        let mut done = 0u64;
        while compare(done, length, s)?.is_lt() {
            let left = subtract(length, done, s)?;
            let n = if compare(left, 262144, s)?.is_gt() {
                262144
            } else {
                left
            };
            let n = n as usize;
            s.check_cancelled().map_err(core_error)?;
            s.semantic_work().write_bytes(n).map_err(core_error)?;
            self.bodies.read_exact(&mut buffer[..n]).map_err(io)?;
            let next = add(done, n as u64, s)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work().scan_bytes(n).map_err(core_error)?;
            s.semantic_work().scan_bytes(n).map_err(core_error)?;
            require(
                buffer[..n] == expected[done as usize..next as usize],
                "INTEGRITY_FAILURE",
            )?;
            done = next;
        }
        s.check_cancelled().map_err(core_error)
    }
    pub(in super::super) fn introduce(
        &mut self,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        body: &[u8],
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let key = Locator::from_ref(&r.kind, &r.id, &r.digest, s)?;
        if let Some(old) = self.tree.find(&key, s)? {
            self.observe_origin_entry(&old, s)?;
            return self.compare_body(&old, body, s);
        }
        require(
            !compare(body.len() as u64, 16, s)?.is_lt()
                && !compare(body.len() as u64, 4194304, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, 100000, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        self.extend(0, body.len() as u64, s)?;
        self.tree.scratch.admit(body.len() as u64, s)?;
        let offset = self.body_bytes;
        let next = add(offset, body.len() as u64, s)?;
        require(
            // Auxiliary origin pointers share this already fixed scratch
            // allowance; they never inflate the normative payload/WB counter.
            !compare(next, self.tree.scratch.limit, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        s.check_cancelled().map_err(core_error)?;
        self.bodies.seek(SeekFrom::Start(offset)).map_err(io)?;
        for chunk in body.chunks(262144) {
            s.check_cancelled().map_err(core_error)?;
            copied(chunk.len(), s)?;
            self.bodies.write_all(chunk).map_err(io)?;
        }
        self.body_bytes = next;
        let entry = Entry::from_ref(
            &r.kind,
            &r.id,
            &r.digest,
            offset,
            body.len() as u32,
            sequence as u32,
            s,
        )?;
        require(self.tree.insert(&entry, s)?.is_none(), "INTEGRITY_FAILURE")?;
        s.check_cancelled().map_err(core_error)
    }
    pub(in super::super) fn absent(
        &mut self,
        key: &StateKey,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(self.tree.find(&key.0, s)?.is_none(), "INTEGRITY_FAILURE")
    }
    pub(in super::super) fn match_head(
        &mut self,
        key: &StateKey,
        expected: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let entry = self
            .tree
            .find(&key.0, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        self.compare_body(&entry, expected, s)
    }
    pub(in super::super) fn set_head(
        &mut self,
        key: &StateKey,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let record = Locator::from_ref(&r.kind, &r.id, &r.digest, s)?;
        let record = self
            .tree
            .find(&record, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        copied(ENTRY_BYTES, s)?;
        let mut entry = record;
        copied(KEY_BYTES, s)?;
        entry.0[..KEY_BYTES].copy_from_slice(&key.0.0);
        self.tree.replace_control(&entry, s)
    }
}
impl IntroductionIndex {
    fn replace_control(&mut self, entry: &Entry, s: &mut OriginalAuditSession) -> Result<()> {
        require(!self.invalid, "INTEGRITY_FAILURE")?;
        self.reserve_comparisons(self.height, 4, s)?;
        let mut page = self.root;
        let mut remaining = self.height;
        loop {
            require(compare(remaining, 0, s)?.is_gt(), "INTEGRITY_FAILURE")?;
            self.read(page, 0, s)?;
            let (position, found, child) = self.buffers[0].position(entry, s)?;
            if found {
                let start = Page::key_offset(position, s)?;
                let end = add(start as u64, ENTRY_BYTES as u64, s)? as usize;
                copied(ENTRY_BYTES, s)?;
                self.buffers[0].bytes[start..end].copy_from_slice(&entry.0);
                return self.write(page, 0, s);
            }
            if self.buffers[0].leaf() {
                require(self.insert(entry, s)?.is_none(), "INTEGRITY_FAILURE")?;
                return Ok(());
            }
            page = child;
            remaining = subtract(remaining, 1, s)?;
        }
    }
}

impl Inventory {
    pub(in super::super) fn body(
        &mut self,
        reference: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        let kind = request_wrapper::text(reference, "kind", s)?;
        let key = Locator::from_ref(
            kind,
            request_wrapper::text(reference, "id", s)?,
            request_wrapper::text(reference, "digest", s)?,
            s,
        )?;
        let entry = self
            .tree
            .find(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        self.observe_origin_entry(&entry, s)?;
        self.entry_body(&entry, s)
    }
    pub(in super::super) fn head_body(
        &mut self,
        key: &StateKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ledgerlab_core::canonical::audit_arena::AuditedBytes>> {
        let Some(entry) = self.tree.find(&key.0, s)? else {
            return Ok(None);
        };
        self.entry_body(&entry, s).map(Some)
    }
    fn entry_body(
        &mut self,
        entry: &Entry,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        copied(12, s)?;
        let mut offset = [0u8; 8];
        offset.copy_from_slice(&entry.0[216..224]);
        let mut length = [0u8; 4];
        length.copy_from_slice(&entry.0[224..228]);
        let offset = u64::from_le_bytes(offset);
        let length = u32::from_le_bytes(length) as u64;
        require(!compare(length, 4194304, s)?.is_gt(), "INTEGRITY_FAILURE")?;
        require(
            !compare(add(offset, length, s)?, self.body_bytes, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let (arena, work) = s.parts();
        let mut out = arena
            .byte_builder(length as usize, work)
            .map_err(core_error)?;
        self.bodies.seek(SeekFrom::Start(offset)).map_err(io)?;
        let mut buffer = [0u8; 262144];
        let mut done = 0;
        while compare(done, length, s)?.is_lt() {
            let left = subtract(length, done, s)?;
            let n = if compare(left, 262144, s)?.is_gt() {
                262144
            } else {
                left
            } as usize;
            s.check_cancelled().map_err(core_error)?;
            s.semantic_work().write_bytes(n).map_err(core_error)?;
            self.bodies.read_exact(&mut buffer[..n]).map_err(io)?;
            out.append(&buffer[..n], s.semantic_work())
                .map_err(core_error)?;
            done = add(done, n as u64, s)?;
        }
        s.check_cancelled().map_err(core_error)?;
        out.finish(s.semantic_work()).map_err(core_error)
    }
}

impl Inventory {
    /// Permanent source registry entries may reuse precisely the same immutable
    /// document. A same semantic identity/version with different bytes refuses.
    pub(in super::super) fn bind_permanent(
        &mut self,
        key: &StateKey,
        r: &ledgerlab_core::policy::workflow::transition_constructor::ExactRef,
        body: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if let Some(prior) = self.tree.find(&key.0, s)? {
            self.compare_body(&prior, body, s)?;
        }
        self.set_head(key, r, s)
    }
}

impl Inventory {
    /// Store ONE actual completed-origin pointer under a private numeric
    /// locator in the existing paged tree/body spool. It is auxiliary DATA,
    /// never a canonical financial record or a serialized completion flag.
    pub(super) fn remember_origin_pointer(
        &mut self,
        sequence: i64,
        body: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, 100000, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        require(
            !compare(body.len() as u64, 4194304, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let encoded = wire::counter_string(sequence as u64, s)?;
        let key = StateKey::new("origin-pointer", &[encoded.bytes()], s)?;
        self.remember_auxiliary(&key, sequence, body, s)
    }
    fn remember_auxiliary(
        &mut self,
        key: &StateKey,
        sequence: i64,
        body: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.absent(key, s)?;
        // Charge existing scratch only. Unlike actual introduced documents,
        // this derived pointer is NOT added to the W/WB profile dimension.
        self.tree.scratch.admit(body.len() as u64, s)?;
        let offset = self.body_bytes;
        let next = add(offset, body.len() as u64, s)?;
        require(
            !compare(next, self.tree.scratch.limit, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        s.check_cancelled().map_err(core_error)?;
        self.bodies.seek(SeekFrom::Start(offset)).map_err(io)?;
        for chunk in body.chunks(262144) {
            s.check_cancelled().map_err(core_error)?;
            copied(chunk.len(), s)?;
            self.bodies.write_all(chunk).map_err(io)?;
        }
        s.semantic_work()
            .write_bytes(ENTRY_BYTES)
            .map_err(core_error)?;
        let mut entry = Entry([0; ENTRY_BYTES]);
        copied(KEY_BYTES, s)?;
        entry.0[..KEY_BYTES].copy_from_slice(&key.0.0);
        for (position, bytes) in [
            (216, offset.to_le_bytes().as_slice()),
            (224, (body.len() as u32).to_le_bytes().as_slice()),
            (228, (sequence as u32).to_le_bytes().as_slice()),
        ] {
            let end = add(position as u64, bytes.len() as u64, s)? as usize;
            copied(bytes.len(), s)?;
            entry.0[position..end].copy_from_slice(bytes);
        }
        self.body_bytes = next;
        require(self.tree.insert(&entry, s)?.is_none(), "INTEGRITY_FAILURE")?;
        s.check_cancelled().map_err(core_error)
    }
    pub(in super::super) fn origin_pointer(
        &mut self,
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, 100000, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let encoded = wire::counter_string(sequence as u64, s)?;
        let key = StateKey::new("origin-pointer", &[encoded.bytes()], s)?;
        self.head_body(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)
    }
}

impl Inventory {
    pub(super) fn begin_origin_links(
        &mut self,
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        require(self.active_origin.is_none(), "INTEGRITY_FAILURE")?;
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, 100000, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let stride = add(ENTRY_BYTES as u64, std::mem::size_of::<usize>() as u64, s)?;
        let bytes = add(
            multiply(4096, stride, s)?,
            std::mem::size_of::<OriginLinks>() as u64,
            s,
        )? as usize;
        let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(4096)
            .map_err(|_| ServiceError::Unavailable)?;
        let mut order = Vec::new();
        order
            .try_reserve_exact(4096)
            .map_err(|_| ServiceError::Unavailable)?;
        self.active_origin = Some(OriginLinks {
            sequence: sequence as u32,
            entries,
            order,
            _storage: storage,
        });
        Ok(())
    }
    fn observe_origin_entry(&mut self, entry: &Entry, s: &mut OriginalAuditSession) -> Result<()> {
        let Some(active) = self.active_origin.as_mut() else {
            return Ok(());
        };
        copied(4, s)?;
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&entry.0[228..232]);
        let sequence = u32::from_le_bytes(bytes);
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, active.sequence as u64, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        if compare(sequence as u64, active.sequence as u64, s)?.is_eq() {
            return Ok(());
        }
        let (mut low, mut high) = (0usize, active.order.len());
        while compare(low as u64, high as u64, s)?.is_lt() {
            let width = subtract(high as u64, low as u64, s)?;
            s.semantic_work()
                .arithmetic(Op::DivideRemainder, &[&width.into(), &2u64.into()])
                .map_err(core_error)?;
            let middle = add(low as u64, width / 2, s)? as usize;
            match active.entries[active.order[middle]].compare(entry, s)? {
                Cmp::Equal => return Ok(()),
                Cmp::Greater => high = middle,
                Cmp::Less => low = add(middle as u64, 1, s)? as usize,
            }
        }
        require(
            compare(active.entries.len() as u64, 4096, s)?.is_lt(),
            "REFERENCE_LIMIT",
        )?;
        let shift = subtract(active.order.len() as u64, low as u64, s)?;
        let moved = multiply(shift, std::mem::size_of::<usize>() as u64, s)? as usize;
        copied(moved, s)?;
        s.semantic_work()
            .visit(Visit::SortMove)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<usize>())
            .map_err(core_error)?;
        copied(ENTRY_BYTES, s)?;
        add(active.order.len() as u64, 1, s)?;
        add(active.entries.len() as u64, 1, s)?;
        active.order.insert(low, active.entries.len());
        active.entries.push(*entry);
        Ok(())
    }
    pub(super) fn finish_origin_links(
        &mut self,
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let mut active = self
            .active_origin
            .take()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(
            compare(active.sequence as u64, sequence as u64, s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        // Configured roots in this closed local caller have kind workflow-authority.
        // Preserve EVERY such exact identity as a candidate, not a trusted-root
        // assertion. Other kinds cannot equal that configured full Ref.
        const AUTHORITY: [u8; 48] = {
            let mut out = [0; 48];
            let text = b"workflow-authority";
            let mut i = 0;
            while i < text.len() {
                out[i] = text[i];
                i += 1;
            }
            out
        };
        let ordinary_storage = s
            .reserve_external_payload(
                4096 * std::mem::size_of::<u32>() + std::mem::size_of::<Vec<u32>>(),
            )
            .map_err(core_error)?;
        let mut ordinary = Vec::<u32>::new();
        ordinary
            .try_reserve_exact(4096)
            .map_err(|_| ServiceError::Unavailable)?;
        s.semantic_work()
            .write_bytes(std::mem::size_of::<usize>())
            .map_err(core_error)?;
        active.order.clear();
        for (index, entry) in active.entries.iter().enumerate() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            s.semantic_work().scan_bytes(96).map_err(core_error)?;
            if entry.0[168..216] == AUTHORITY {
                add(active.order.len() as u64, 1, s)?;
                s.semantic_work()
                    .write_bytes(std::mem::size_of::<usize>())
                    .map_err(core_error)?;
                active.order.push(index);
                continue;
            }
            copied(4, s)?;
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&entry.0[228..232]);
            let prior = u32::from_le_bytes(bytes);
            let (mut low, mut high) = (0usize, ordinary.len());
            let mut seen = false;
            while compare(low as u64, high as u64, s)?.is_lt() {
                let width = subtract(high as u64, low as u64, s)?;
                s.semantic_work()
                    .arithmetic(Op::DivideRemainder, &[&width.into(), &2u64.into()])
                    .map_err(core_error)?;
                let middle = add(low as u64, width / 2, s)? as usize;
                match compare(ordinary[middle] as u64, prior as u64, s)? {
                    Cmp::Equal => {
                        seen = true;
                        break;
                    }
                    Cmp::Greater => high = middle,
                    Cmp::Less => low = add(middle as u64, 1, s)? as usize,
                }
            }
            if !seen {
                let moved =
                    multiply(subtract(ordinary.len() as u64, low as u64, s)?, 4, s)? as usize;
                copied(moved, s)?;
                s.semantic_work()
                    .visit(Visit::SortMove)
                    .map_err(core_error)?;
                s.semantic_work().write_bytes(4).map_err(core_error)?;
                add(ordinary.len() as u64, 1, s)?;
                ordinary.insert(low, prior);
            }
        }
        let length = add(
            add(8, multiply(ordinary.len() as u64, 4, s)?, s)?,
            multiply(active.order.len() as u64, ENTRY_BYTES as u64, s)?,
            s,
        )? as usize;
        let (arena, work) = s.parts();
        let mut encoded = arena.byte_builder(length, work).map_err(core_error)?;
        for count in [ordinary.len(), active.order.len()] {
            encoded
                .append(&(count as u32).to_le_bytes(), s.semantic_work())
                .map_err(core_error)?;
        }
        for prior in &ordinary {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            encoded
                .append(&prior.to_le_bytes(), s.semantic_work())
                .map_err(core_error)?;
        }
        for index in &active.order {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            encoded
                .append(&active.entries[*index].0, s.semantic_work())
                .map_err(core_error)?;
        }
        let encoded = encoded.finish(s.semantic_work()).map_err(core_error)?;
        drop(ordinary);
        drop(ordinary_storage);
        let sequence_value = wire::counter_string(sequence as u64, s)?;
        let key = StateKey::new("origin-links", &[sequence_value.bytes()], s)?;
        self.remember_auxiliary(&key, sequence, encoded.bytes(), s)
    }
    pub(in super::super) fn origin_links(
        &mut self,
        sequence: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::canonical::audit_arena::AuditedBytes> {
        require(
            compare(sequence as u64, 0, s)?.is_gt()
                && !compare(sequence as u64, 100000, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let encoded = wire::counter_string(sequence as u64, s)?;
        let key = StateKey::new("origin-links", &[encoded.bytes()], s)?;
        self.head_body(&key, s)?
            .ok_or(ServiceError::IntegrityFailure)
    }
    pub(in super::super) fn seed_origin(
        &mut self,
        reference: &Value,
        root: &ConfiguredRoot,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<i64>> {
        let key = Locator::from_ref(
            request_wrapper::text(reference, "kind", s)?,
            request_wrapper::text(reference, "id", s)?,
            request_wrapper::text(reference, "digest", s)?,
            s,
        )?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(KEY_BYTES)
            .map_err(core_error)?;
        if key.key() == root.0.key() {
            return Ok(None);
        }
        self.introduction_sequence(reference, s).map(Some)
    }
    pub(in super::super) fn required_origins(
        &mut self,
        sequence: i64,
        configured_root: &ConfiguredRoot,
        emit: &mut dyn FnMut(i64, &mut OriginalAuditSession) -> Result<()>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let raw = self.origin_links(sequence, s)?;
        require(
            !compare(raw.bytes().len() as u64, 8, s)?.is_lt(),
            "INTEGRITY_FAILURE",
        )?;
        copied(8, s)?;
        let mut first = [0; 4];
        first.copy_from_slice(&raw.bytes()[..4]);
        let ordinary = u32::from_le_bytes(first) as u64;
        first.copy_from_slice(&raw.bytes()[4..8]);
        let roots = u32::from_le_bytes(first) as u64;
        require(
            !compare(ordinary, 4096, s)?.is_gt() && !compare(roots, 4096, s)?.is_gt(),
            "INTEGRITY_FAILURE",
        )?;
        let middle = add(8, multiply(ordinary, 4, s)?, s)? as usize;
        let expected = add(middle as u64, multiply(roots, ENTRY_BYTES as u64, s)?, s)?;
        require(
            compare(expected, raw.bytes().len() as u64, s)?.is_eq(),
            "INTEGRITY_FAILURE",
        )?;
        for bytes in raw.bytes()[8..middle].chunks_exact(4) {
            s.semantic_work().visit(Visit::Edge).map_err(core_error)?;
            copied(4, s)?;
            let mut number = [0; 4];
            number.copy_from_slice(bytes);
            emit(i64::from(u32::from_le_bytes(number)), s)?;
        }
        for bytes in raw.bytes()[middle..].chunks_exact(ENTRY_BYTES) {
            s.semantic_work().visit(Visit::Edge).map_err(core_error)?;
            copied(ENTRY_BYTES, s)?;
            let mut entry = Entry([0; ENTRY_BYTES]);
            entry.0.copy_from_slice(bytes);
            if entry.compare(&configured_root.0, s)?.is_eq() {
                continue;
            }
            copied(4, s)?;
            let mut number = [0; 4];
            number.copy_from_slice(&entry.0[228..232]);
            emit(i64::from(u32::from_le_bytes(number)), s)?;
        }
        Ok(())
    }
}
