//! Shared allocation DATA for one original-audit attempt. This is not custody,
//! authority, a complete resource proof, or permission to admit financial work.
//! Every parsed/copied artifact owns its lease; dropping a callback beside an
//! escaping naked Value is deliberately not an available public API.
use crate::{
    Error, Result,
    canonical::{self, ParseAllocation, ParseAllocationWork},
    validation_work::{self as w, ValidationVisit, ValidationWork},
};
use serde_json::Value;
use std::{
    mem,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
const LIMIT: usize = 64 * 1024 * 1024;
// Fixed ledger/owner/error propagation reservation. Allocator/RSS overhead is
// distinct from requested payload; stack/I/O/complete evaluator copies remain
// separately required in the composed resource ledger.
const FIXED: usize = 1024;
fn failure(detail: &str) -> Error {
    Error::new("AUDIT_INTEGRITY_OR_PROFILE_FAILURE", detail)
}
fn add(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(w::ArithmeticOperation::Add, &[&a.into(), &b.into()])?;
    a.checked_add(b)
        .ok_or_else(|| failure("arena size overflow"))
}
fn mul(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(w::ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| failure("arena size overflow"))
}
fn sub(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(w::ArithmeticOperation::Subtract, &[&a.into(), &b.into()])?;
    a.checked_sub(b)
        .ok_or_else(|| failure("arena size underflow"))
}
fn greater(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<bool> {
    Ok(w::compare(work, a, b)?.is_gt())
}
fn maximum(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    Ok(if greater(work, a, b)? { a } else { b })
}
fn minimum(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    Ok(if greater(work, a, b)? { b } else { a })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaUsage {
    pub live_bytes: usize,
    pub peak_bytes: usize,
    pub total_reserved_bytes: usize,
}
struct Ledger {
    live: usize,
    peak: usize,
    total: usize,
    poisoned: bool,
}
/// Fixed resource quota only. No caller maximum, serialization, reset, trusted
/// origin state or financial commit conversion exists.
pub struct AuditArena {
    ledger: Arc<Mutex<Ledger>>,
    cancelled: Arc<AtomicBool>,
}
/// Operational cancellation only. Handles may set the attempt's terminal flag;
/// no clear/reset, quota access, or proof completion operation is available.
#[derive(Clone)]
pub struct AuditCancellation {
    cancelled: Arc<AtomicBool>,
}
impl AuditCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}
impl AuditArena {
    pub fn new() -> Result<Self> {
        if FIXED > LIMIT {
            return Err(failure("arena fixed reservation"));
        }
        Ok(Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            ledger: Arc::new(Mutex::new(Ledger {
                live: FIXED,
                peak: FIXED,
                total: FIXED,
                poisoned: false,
            })),
        })
    }
    pub(crate) fn shared_handle(&self) -> Self {
        Self {
            ledger: Arc::clone(&self.ledger),
            cancelled: Arc::clone(&self.cancelled),
        }
    }
    pub fn cancellation_handle(&self) -> AuditCancellation {
        AuditCancellation {
            cancelled: Arc::clone(&self.cancelled),
        }
    }
    pub fn usage(&self) -> Result<ArenaUsage> {
        let s = self
            .ledger
            .lock()
            .map_err(|_| failure("poisoned audit arena"))?;
        if s.poisoned {
            return Err(failure("poisoned audit arena"));
        }
        Ok(ArenaUsage {
            live_bytes: s.live,
            peak_bytes: s.peak,
            total_reserved_bytes: s.total,
        })
    }
    fn reserve(&self, bytes: usize, work: &mut dyn ValidationWork) -> Result<Lease> {
        let mut s = self
            .ledger
            .lock()
            .map_err(|_| failure("poisoned audit arena"))?;
        work.visit(ValidationVisit::Constraint)?;
        if s.poisoned {
            return Err(failure("poisoned audit arena"));
        }
        let next = add(work, s.live, bytes)?;
        if greater(work, next, LIMIT)? {
            return Err(failure("original audit arena exhausted before allocation"));
        }
        let total = add(work, s.total, bytes)?;
        // Each eventual destructor performs one balance comparison and one
        // subtraction on <=64MiB operands. Prepay the exact one-limb schedule
        // BEFORE ownership acquisition; Drop itself cannot fail or call work.
        work.arithmetic(
            w::ArithmeticOperation::Compare,
            &[&LIMIT.into(), &bytes.into()],
        )?;
        work.arithmetic(
            w::ArithmeticOperation::Subtract,
            &[&LIMIT.into(), &bytes.into()],
        )?;
        let peak = maximum(work, s.peak, next)?;
        s.live = next;
        s.peak = peak;
        s.total = total;
        Ok(Lease {
            ledger: self.ledger.clone(),
            bytes,
        })
    }
    /// Exact lexical parser, mandatory preallocation callbacks, and owned retained
    /// reservation. The Value is destroyed before its lease (field drop order).
    pub fn parse_bounded(
        &self,
        raw: &[u8],
        maximum: usize,
        work: &mut dyn ValidationWork,
    ) -> Result<AuditedValue> {
        let mut scope = ParseScope {
            held: Some(self.reserve(0, work)?),
            work,
            pending: None,
        };
        let value = canonical::parse_bounded_with_allocations(raw, maximum, &mut scope)?;
        let reservation = scope
            .held
            .take()
            .ok_or_else(|| failure("parser lease missing"))?;
        Ok(AuditedValue { value, reservation })
    }
    /// Closed RP01 parser accounting; legacy parse_bounded is unchanged.
    pub(crate) fn parse_representation_bounded(
        &self,
        raw: &[u8],
        maximum: usize,
        work: &mut dyn ValidationWork,
    ) -> Result<AuditedValue> {
        let mut scope = ParseScope {
            held: Some(self.reserve(0, work)?),
            work,
            pending: None,
        };
        let value = canonical::parse_bounded_with_allocations_complete(raw, maximum, &mut scope)?;
        let reservation = scope
            .held
            .take()
            .ok_or_else(|| failure("parser lease missing"))?;
        Ok(AuditedValue { value, reservation })
    }
    pub fn copy_bytes(&self, raw: &[u8], work: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        let reservation = self.reserve(raw.len(), work)?;
        work.scan_bytes(raw.len())?;
        work.write_bytes(raw.len())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(raw.len())
            .map_err(|_| failure("audit byte allocation"))?;
        if greater(work, bytes.capacity(), raw.len())? {
            return Err(failure("unsupported byte capacity implementation"));
        }
        bytes.extend_from_slice(raw);
        Ok(AuditedBytes { bytes, reservation })
    }
    /// Reserve an exactly derived external producer's requested payload BEFORE
    /// execution. This is a requested allocation size, never a caller-selected
    /// quota/ceiling: all producers share the fixed64MiB attempt allowance.
    /// Caller source must establish size/copy/lifetime truth and all I/O/grammar
    /// bounds; this resource object cannot prove those obligations. Host SQL
    /// leases live until actual worker completion/reset, even after cancellation.
    /// Size/lease arithmetic executes on ordinary S, never a C callback view.
    pub(crate) fn reserve_external_payload(
        &self,
        bytes: usize,
        work: &mut dyn ValidationWork,
    ) -> Result<ExternalBufferLease> {
        Ok(ExternalBufferLease {
            reservation: self.reserve(bytes, work)?,
        })
    }
    /// Encode a borrowed tree with the SAME paid bounded emitter. The output,
    /// old/new destination growth overlap and all key/small formatting scratch
    /// are admitted before emission; successful bytes keep an owning lease.
    /// This does not account for a caller's ValidationWork trace allocations.
    pub fn canonical_bytes(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
        maximum: usize,
    ) -> Result<AuditedBytes> {
        let maximum = minimum(work, maximum, canonical::BUNDLE_LIMIT)?;
        self.canonical_owned(value, work, maximum, None, false)
    }
    /// Only the resource owner's closed container selector can select these
    /// existing framing bounds. Ordinary canonical records retain their cap.
    pub(crate) fn canonical_container_bytes(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
        bound: canonical::CanonicalContainerBound,
    ) -> Result<AuditedBytes> {
        self.canonical_owned(value, work, bound.maximum(), Some(bound), true)
    }
    /// RP01-only resource walk. Legacy callers retain their exact paid trace.
    pub(crate) fn canonical_representation_bytes(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
        maximum: usize,
    ) -> Result<AuditedBytes> {
        let maximum = minimum(work, maximum, canonical::BUNDLE_LIMIT)?;
        self.canonical_owned(value, work, maximum, None, true)
    }
    fn canonical_owned(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
        maximum: usize,
        container: Option<canonical::CanonicalContainerBound>,
        complete_representation: bool,
    ) -> Result<AuditedBytes> {
        let keys = if complete_representation {
            let mut minimum_output = 0;
            bounded_emission_key_storage(value, work, 0, maximum, &mut minimum_output)?
        } else {
            emission_key_storage(value, work, 0)?
        };
        // Bounded Destination uses reserve_exact, so retained capacity<=maximum
        // and each reallocation's old+new requested payload<=2*maximum.
        // All object key vectors are retained conservatively even after their
        // actual scope ends. Integer scratch is <=16 digits (safe integer),
        // with old8+new16 growth overlap; control escaping uses one fixed
        // six-byte formatted String. See the pinned formatting source ledger.
        let integer_scratch = 8 + 16 + mem::size_of::<Vec<u8>>();
        let escape_scratch = 16 + mem::size_of::<String>();
        let destination = mul(work, 2, maximum)?;
        let peak = add(work, destination, keys)?;
        let peak = add(work, peak, integer_scratch + escape_scratch)?;
        let mut reservation = self.reserve(peak, work)?;
        let bytes = match (complete_representation, container) {
            (true, Some(bound)) => {
                canonical::CanonicalBytes::from_representation_container_with_work(
                    value, work, bound,
                )?
            }
            (true, None) => {
                canonical::CanonicalBytes::from_representation_with_work(value, work, maximum)?
            }
            (false, Some(bound)) => {
                canonical::CanonicalBytes::from_container_with_work(value, work, bound)?
            }
            (false, None) => {
                canonical::CanonicalBytes::from_value_with_work_bounded(value, work, maximum)?
            }
        }
        .into_vec();
        if greater(work, bytes.capacity(), maximum)? {
            return Err(failure("unsupported canonical capacity implementation"));
        }
        let release = sub(work, peak, bytes.capacity())?;
        reservation.shrink(release, work)?;
        Ok(AuditedBytes { bytes, reservation })
    }
    /// Pre-admit an output buffer for an existing strict decoder/borrowed read.
    /// The caller's grammar/domain limit remains required; this cap only limits
    /// storage owner against the SAME arena. Raw framing may exceed4MiB under
    /// its existing caller domain; only canonical emission has a4MiB ceiling.
    pub fn byte_builder(
        &self,
        maximum: usize,
        work: &mut dyn ValidationWork,
    ) -> Result<AuditedByteBuilder> {
        let reservation = self.reserve(maximum, work)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum)
            .map_err(|_| failure("owned output allocation"))?;
        if greater(work, bytes.capacity(), maximum)? {
            return Err(failure("unsupported output capacity implementation"));
        }
        Ok(AuditedByteBuilder {
            artifact: AuditedBytes { bytes, reservation },
            maximum,
            poisoned: false,
        })
    }
    /// No Clone implementation on the owner. Explicit copies execute a paid
    /// structural footprint walk and reserve overlap BEFORE Value::clone.
    pub fn clone_value(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
    ) -> Result<AuditedValue> {
        let bytes = clone_footprint(value, work, 0)?;
        let reservation = self.reserve(bytes, work)?;
        Ok(AuditedValue {
            value: value.clone(),
            reservation,
        })
    }
}
/// A resource reservation, never authority or an indication of SQL completion.
/// No Clone/Serde/reset or ability to extract a payload/allocation is exposed.
pub struct ExternalBufferLease {
    reservation: Lease,
}
impl ExternalBufferLease {
    pub fn reserved_payload_bytes(&self) -> usize {
        self.reservation.bytes
    }
}
struct Lease {
    ledger: Arc<Mutex<Ledger>>,
    bytes: usize,
}
impl Lease {
    fn grow(&mut self, bytes: usize, work: &mut dyn ValidationWork) -> Result<()> {
        let mut s = self
            .ledger
            .lock()
            .map_err(|_| failure("poisoned audit arena"))?;
        work.visit(ValidationVisit::Constraint)?;
        if s.poisoned {
            return Err(failure("poisoned audit arena"));
        }
        let live = add(work, s.live, bytes)?;
        if greater(work, live, LIMIT)? {
            return Err(failure("original audit arena exhausted before allocation"));
        }
        let owned = add(work, self.bytes, bytes)?;
        let total = add(work, s.total, bytes)?;
        let peak = maximum(work, s.peak, live)?;
        s.live = live;
        s.peak = peak;
        s.total = total;
        self.bytes = owned;
        Ok(())
    }
    fn shrink(&mut self, bytes: usize, work: &mut dyn ValidationWork) -> Result<()> {
        if greater(work, bytes, self.bytes)? {
            return Err(failure("arena release ownership"));
        }
        let mut s = self
            .ledger
            .lock()
            .map_err(|_| failure("poisoned audit arena"))?;
        if greater(work, bytes, s.live)? {
            return Err(failure("arena release balance"));
        }
        let owned = sub(work, self.bytes, bytes)?;
        let live = sub(work, s.live, bytes)?;
        self.bytes = owned;
        s.live = live;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = match self.ledger.lock() {
            Ok(s) => s,
            Err(poison) => {
                let mut s = poison.into_inner();
                s.poisoned = true;
                s
            }
        };
        // Both operations were prepaid before this lease acquired ownership.
        // Cleanup remains infallible and never clears a poisoned mutex/state.
        if self.bytes > s.live {
            s.poisoned = true;
            s.live = LIMIT;
        } else {
            s.live -= self.bytes;
        }
    }
}
/// Value MUST precede reservation: no Drop implementation may release it first.
pub struct AuditedValue {
    value: Value,
    reservation: Lease,
}
impl AuditedValue {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn reserved_bytes(&self) -> usize {
        self.reservation.bytes
    }
}
/// Bytes MUST precede reservation. No unchecked into_vec or Clone/Serde escape.
pub struct AuditedBytes {
    bytes: Vec<u8>,
    reservation: Lease,
}
impl AuditedBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn reserved_bytes(&self) -> usize {
        self.reservation.bytes
    }
}
/// A single preallocated output owner, usable as a strict decoder/read sink.
/// Append never grows capacity; no naked Vec or unchecked conversion exists.
pub struct AuditedByteBuilder {
    artifact: AuditedBytes,
    maximum: usize,
    poisoned: bool,
}
impl AuditedByteBuilder {
    pub fn append(&mut self, raw: &[u8], work: &mut dyn ValidationWork) -> Result<()> {
        let result = self.append_inner(raw, work);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn append_inner(&mut self, raw: &[u8], work: &mut dyn ValidationWork) -> Result<()> {
        work.visit(ValidationVisit::Constraint)?;
        if self.poisoned {
            return Err(failure("poisoned owned output"));
        }
        let next = add(work, self.artifact.bytes.len(), raw.len())?;
        if greater(work, next, self.maximum)? {
            return Err(failure("owned output exhausted before append"));
        }
        work.scan_bytes(raw.len())?;
        work.write_bytes(raw.len())?;
        self.artifact.bytes.extend_from_slice(raw);
        Ok(())
    }
    pub fn finish(self, work: &mut dyn ValidationWork) -> Result<AuditedBytes> {
        work.visit(ValidationVisit::Constraint)?;
        if self.poisoned {
            return Err(failure("poisoned owned output"));
        }
        Ok(self.artifact)
    }
}
struct ParseScope<'w> {
    work: &'w mut dyn ValidationWork,
    held: Option<Lease>,
    pending: Option<(ParseAllocation, usize)>,
}
// Exact pinned Rust48a229c alloc BTree: B=6, capacity11, internal edges12;
// Leaf fields: parent pointer, u16 index/len, 11 keys and values. This upper
// bound allows max-alignment padding per field plus internal-node edge/padding.
// A tree has at most n nonempty nodes; insertion can allocate at most height+2
// nodes, height<n, hence <=2*(n+1) simultaneous nodes. Reserving two upper-bound
// nodes per entry covers old nodes AND every split/root transient, even though
// most inserts allocate none. Existing key/value storage is separately owned.
fn map_entry_bound(
    work: &mut dyn ValidationWork,
    value_size: usize,
    string_size: usize,
) -> Result<usize> {
    if !w::compare(work, value_size, mem::size_of::<Value>())?.is_eq()
        || !w::compare(work, string_size, mem::size_of::<String>())?.is_eq()
    {
        return Err(failure("unsupported parser value layout"));
    }
    let align = maximum(work, mem::align_of::<Value>(), mem::align_of::<String>())?;
    let align = maximum(work, align, mem::align_of::<usize>())?;
    let header = add(work, mem::size_of::<usize>(), 4)?;
    let keys = mul(work, 11, string_size)?;
    let values = mul(work, 11, value_size)?;
    let leaf = add(work, header, keys)?;
    let leaf = add(work, leaf, values)?;
    let slack = sub(work, align, 1)?;
    let padding = mul(work, 8, slack)?;
    let edges = mul(work, 12, mem::size_of::<usize>())?;
    let internal = add(work, leaf, edges)?;
    let internal = add(work, internal, padding)?;
    let rounded = add(work, internal, slack)?;
    work.arithmetic(
        w::ArithmeticOperation::DivideRemainder,
        &[&rounded.into(), &align.into()],
    )?;
    let rounded = rounded / align;
    let rounded = mul(work, rounded, align)?;
    mul(work, 2, rounded)
}
fn same_request(
    work: &mut dyn ValidationWork,
    a: ParseAllocation,
    b: ParseAllocation,
) -> Result<bool> {
    work.visit(ValidationVisit::Constraint)?;
    let pairs = match (a, b) {
        (
            ParseAllocation::StringDecode { encoded_bytes: a },
            ParseAllocation::StringDecode { encoded_bytes: b },
        ) => [(a, b), (0, 0), (0, 0), (0, 0)],
        (
            ParseAllocation::ArrayCapacity {
                current_capacity: a,
                requested_capacity: b,
                element_size: c,
            },
            ParseAllocation::ArrayCapacity {
                current_capacity: d,
                requested_capacity: e,
                element_size: f,
            },
        ) => [(a, d), (b, e), (c, f), (0, 0)],
        (
            ParseAllocation::ObjectEntry {
                current_len: a,
                key_bytes: b,
                value_size: c,
                string_size: d,
            },
            ParseAllocation::ObjectEntry {
                current_len: e,
                key_bytes: f,
                value_size: g,
                string_size: h,
            },
        ) => [(a, e), (b, f), (c, g), (d, h)],
        _ => return Ok(false),
    };
    for (a, b) in pairs {
        if !w::compare(work, a, b)?.is_eq() {
            return Ok(false);
        }
    }
    Ok(true)
}
impl ParseAllocationWork for ParseScope<'_> {
    fn validation_work(&mut self) -> &mut dyn ValidationWork {
        self.work
    }

    fn before_allocation(&mut self, request: ParseAllocation) -> Result<()> {
        self.work.visit(ValidationVisit::Constraint)?;
        if self.pending.is_some() {
            return Err(failure("overlapping parser allocation"));
        }
        let work = &mut *self.work;
        let (bytes, pending) = match request {
            ParseAllocation::StringDecode { encoded_bytes } => {
                let payload = mul(work, 3, encoded_bytes)?;
                let payload = maximum(work, payload, 16)?;
                let headers = mul(work, 2, mem::size_of::<String>())?;
                (add(work, payload, headers)?, true)
            }
            ParseAllocation::ArrayCapacity {
                requested_capacity,
                element_size,
                ..
            } => {
                if !w::compare(work, element_size, mem::size_of::<Value>())?.is_eq() {
                    return Err(failure("unsupported parser array layout"));
                }
                (mul(work, requested_capacity, element_size)?, true)
            }
            ParseAllocation::ObjectEntry {
                value_size,
                string_size,
                ..
            } => (map_entry_bound(work, value_size, string_size)?, false),
        };
        self.held
            .as_mut()
            .ok_or_else(|| failure("consumed parser lease"))?
            .grow(bytes, work)?;
        if pending {
            self.pending = Some((request, bytes));
        }
        Ok(())
    }
    fn finish_allocation(&mut self, request: ParseAllocation, retained: usize) -> Result<()> {
        self.work.visit(ValidationVisit::Constraint)?;
        let (expected, reserved) = self
            .pending
            .take()
            .ok_or_else(|| failure("parser finish without reservation"))?;
        let work = &mut *self.work;
        if !same_request(work, expected, request)? || greater(work, retained, reserved)? {
            return Err(failure("unsupported parser retained capacity"));
        }
        let spare = sub(work, reserved, retained)?;
        let release = match request {
            ParseAllocation::StringDecode { .. } => spare,
            ParseAllocation::ArrayCapacity {
                current_capacity,
                element_size,
                ..
            } => {
                let old = mul(work, current_capacity, element_size)?;
                add(work, old, spare)?
            }
            ParseAllocation::ObjectEntry { .. } => return Err(failure("unexpected object finish")),
        };
        self.held
            .as_mut()
            .ok_or_else(|| failure("parser lease missing"))?
            .shrink(release, work)
    }
}
fn clone_footprint(v: &Value, work: &mut dyn ValidationWork, depth: usize) -> Result<usize> {
    work.visit(ValidationVisit::Value)?;
    if w::compare(work, depth, 32usize)? == std::cmp::Ordering::Greater {
        return Err(failure("audit clone depth"));
    }
    let mut total = 0;
    match v {
        Value::String(s) => {
            work.scan_bytes(s.len())?;
            work.write_bytes(s.len())?;
            total = s.capacity();
        }
        Value::Array(a) => {
            total = mul(work, a.capacity(), mem::size_of::<Value>())?;
            for child in a {
                work.visit(ValidationVisit::Element)?;
                let next = w::usize_add(work, depth, 1)?;
                let child = clone_footprint(child, work, next)?;
                total = add(work, total, child)?;
            }
        }
        Value::Object(o) => {
            let entry = map_entry_bound(work, mem::size_of::<Value>(), mem::size_of::<String>())?;
            total = mul(work, o.len(), entry)?;
            for (key, child) in o {
                work.visit(ValidationVisit::Field)?;
                work.scan_bytes(key.len())?;
                work.write_bytes(key.len())?;
                total = add(work, total, key.capacity())?;
                let next = w::usize_add(work, depth, 1)?;
                let child = clone_footprint(child, work, next)?;
                total = add(work, total, child)?;
            }
        }
        _ => {}
    }
    Ok(total)
}

// This is an actual resource-validation traversal, not an uncharged count
// pass. It does not clone, sort or construct a second Value. Charging remains
// separate from the emitter, which executes all its own semantic/byte work.
fn emission_key_storage(
    value: &Value,
    work: &mut dyn ValidationWork,
    depth: usize,
) -> Result<usize> {
    work.visit(ValidationVisit::Value)?;
    if w::compare(work, depth, 32usize)? == std::cmp::Ordering::Greater {
        return Err(Error::new(
            "PROGRAM_BUDGET_EXCEEDED",
            "canonical resource depth",
        ));
    }
    let mut total = 0;
    match value {
        Value::Array(values) => {
            for child in values {
                work.visit(ValidationVisit::Element)?;
                let next = w::usize_add(work, depth, 1)?;
                let child = emission_key_storage(child, work, next)?;
                total = add(work, total, child)?;
            }
        }
        Value::Object(values) => {
            let count = if w::compare(work, values.len(), 0usize)?.is_eq() {
                0
            } else {
                maximum(work, values.len(), 4)?
            };
            let payload = mul(work, count, mem::size_of::<&String>())?;
            total = add(work, payload, mem::size_of::<Vec<&String>>())?;
            for child in values.values() {
                work.visit(ValidationVisit::Field)?;
                let next = w::usize_add(work, depth, 1)?;
                let child = emission_key_storage(child, work, next)?;
                total = add(work, total, child)?;
            }
        }
        _ => {}
    }
    Ok(total)
}

fn bounded_emission_key_storage(
    value: &Value,
    work: &mut dyn ValidationWork,
    depth: usize,
    output_maximum: usize,
    minimum_output: &mut usize,
) -> Result<usize> {
    work.visit(ValidationVisit::Value)?;
    if w::compare(work, depth, 32usize)? == std::cmp::Ordering::Greater {
        return Err(Error::new(
            "PROGRAM_BUDGET_EXCEEDED",
            "canonical resource depth",
        ));
    }
    // Every JSON value emits at least one byte. Raw UTF-8 string/key bytes
    // are a lower bound even when escaping expands them. Stop this paid walk
    // before an oversized borrowed tree can consume an unbounded refusal path.
    minimum_emission(work, minimum_output, 1, output_maximum)?;
    let mut total = 0;
    match value {
        Value::Array(values) => {
            for child in values {
                work.visit(ValidationVisit::Element)?;
                let next = w::usize_add(work, depth, 1)?;
                let child = bounded_emission_key_storage(
                    child,
                    work,
                    next,
                    output_maximum,
                    minimum_output,
                )?;
                total = add(work, total, child)?;
            }
        }
        Value::Object(values) => {
            let count = if w::compare(work, values.len(), 0usize)?.is_eq() {
                0
            } else {
                maximum(work, values.len(), 4)?
            };
            let payload = mul(work, count, mem::size_of::<&String>())?;
            total = add(work, payload, mem::size_of::<Vec<&String>>())?;
            for (key, child) in values {
                minimum_emission(work, minimum_output, key.len(), output_maximum)?;
                work.visit(ValidationVisit::Field)?;
                let next = w::usize_add(work, depth, 1)?;
                let child = bounded_emission_key_storage(
                    child,
                    work,
                    next,
                    output_maximum,
                    minimum_output,
                )?;
                total = add(work, total, child)?;
            }
        }
        Value::String(text) => minimum_emission(work, minimum_output, text.len(), output_maximum)?,
        _ => {}
    }
    Ok(total)
}

fn minimum_emission(
    work: &mut dyn ValidationWork,
    running: &mut usize,
    additional: usize,
    maximum: usize,
) -> Result<()> {
    let next = add(work, *running, additional)?;
    if greater(work, next, maximum)? {
        return Err(Error::new(
            "PROGRAM_BUDGET_EXCEEDED",
            "canonical output lower bound",
        ));
    }
    *running = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation_work::NoopValidationWork;
    #[test]
    fn oversized_borrowed_tree_refuses_during_paid_preallocation_walk() {
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let tree = serde_json::json!([null, null, null, null]);
        // Old paid path still reaches its original null refusal; only the
        // separately selected RP01 path has the bounded pre-footprint guard.
        let legacy=arena.canonical_bytes(&tree,&mut NoopValidationWork,2).err().unwrap();
        assert_eq!(legacy.code,"INVALID_SHAPE");
        let error = arena
            .canonical_representation_bytes(&tree, &mut NoopValidationWork, 2)
            .err()
            .unwrap();
        assert_eq!(error.code, "PROGRAM_BUDGET_EXCEEDED");
        assert_eq!(error.detail, "canonical output lower bound");
        assert_eq!(arena.usage().unwrap().live_bytes, before);
        let error = arena
            .canonical_representation_bytes(
                &serde_json::json!({"long-key": 0}),
                &mut NoopValidationWork,
                4,
            )
            .err()
            .unwrap();
        assert_eq!(error.detail, "canonical output lower bound");
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
    #[test]
    fn parsed_artifact_and_explicit_copy_own_shared_reservations_until_drop() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let value = arena
            .parse_bounded(
                br#"{"escaped":"\uD834\uDD1E","values":[1,true,"abc"]}"#,
                4096,
                &mut NoopValidationWork,
            )
            .unwrap();
        assert_eq!(value.value()["escaped"], "𝄞");
        let first = arena.usage().unwrap().live_bytes;
        assert!(first > baseline);
        let copy = arena
            .clone_value(value.value(), &mut NoopValidationWork)
            .unwrap();
        assert_eq!(copy.value(), value.value());
        assert!(arena.usage().unwrap().live_bytes > first);
        drop(value);
        assert!(arena.usage().unwrap().live_bytes > baseline);
        drop(copy);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let bytes = arena
            .copy_bytes(b"retained-copy", &mut NoopValidationWork)
            .unwrap();
        assert_eq!(bytes.bytes(), b"retained-copy");
        drop(bytes);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn allocation_refusal_is_before_growth_and_existing_artifacts_survive() {
        let arena = AuditArena::new().unwrap();
        let existing = arena
            .parse_bounded(br#"["retained"]"#, 4096, &mut NoopValidationWork)
            .unwrap();
        let before = arena.usage().unwrap().live_bytes;
        // Counter-only synthetic occupancy: no large input/workload is run.
        let occupied = arena
            .reserve(LIMIT - before - 1, &mut NoopValidationWork)
            .unwrap();
        let error = arena
            .parse_bounded(br#""new""#, 4096, &mut NoopValidationWork)
            .err()
            .unwrap();
        assert_eq!(error.code, "AUDIT_INTEGRITY_OR_PROFILE_FAILURE");
        assert_eq!(existing.value()[0], "retained");
        assert_eq!(arena.usage().unwrap().live_bytes, LIMIT - 1);
        drop(occupied);
        assert_eq!(arena.usage().unwrap().live_bytes, before);
        drop(existing);
        assert_eq!(arena.usage().unwrap().live_bytes, FIXED);
    }
    #[test]
    fn parser_errors_release_only_after_partial_values_are_destroyed() {
        let arena = AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        for raw in [
            br#"{"a":["value"],"a":true}"#.as_slice(),
            br#"{"a":"\uD800"}"#.as_slice(),
            br#"{"a":[1,2,}"#.as_slice(),
        ] {
            assert!(
                arena
                    .parse_bounded(raw, 4096, &mut NoopValidationWork)
                    .is_err()
            );
            assert_eq!(arena.usage().unwrap().live_bytes, before);
        }
    }
    #[test]
    fn paid_copy_callback_failure_returns_no_new_owner_or_reservation() {
        struct Deny;
        impl ValidationWork for Deny {
            fn visit(&mut self, _: ValidationVisit) -> Result<()> {
                Err(Error::new("DENIED", "copy work"))
            }
            fn arithmetic(
                &mut self,
                _: w::ArithmeticOperation,
                _: &[&num_bigint::BigInt],
            ) -> Result<()> {
                Err(Error::new("DENIED", "copy arithmetic"))
            }
            fn scan_bytes(&mut self, _: usize) -> Result<()> {
                Err(Error::new("DENIED", "copy scan"))
            }
            fn write_bytes(&mut self, _: usize) -> Result<()> {
                Err(Error::new("DENIED", "copy write"))
            }
            fn hash_bytes(&mut self, _: usize) -> Result<()> {
                Err(Error::new("DENIED", "copy hash"))
            }
        }
        let arena = AuditArena::new().unwrap();
        let source = arena
            .parse_bounded(br#"{"key":"value"}"#, 4096, &mut NoopValidationWork)
            .unwrap();
        let before = arena.usage().unwrap().live_bytes;
        assert_eq!(
            arena
                .clone_value(source.value(), &mut Deny)
                .err()
                .unwrap()
                .code,
            "DENIED"
        );
        assert_eq!(
            arena.copy_bytes(b"bytes", &mut Deny).err().unwrap().code,
            "DENIED"
        );
        assert_eq!(arena.usage().unwrap().live_bytes, before);
    }
    #[test]
    fn canonical_output_is_owned_bounded_and_paid_without_clone() {
        use crate::validation_work::tests::CountingWork;
        let arena = AuditArena::new().unwrap();
        let source = arena
            .parse_bounded(
                r#"{"z":[1,"\u0001"],"a":"𝄞"}"#.as_bytes(),
                4096,
                &mut NoopValidationWork,
            )
            .unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut exact = CountingWork::new(usize::MAX);
        let encoded = arena
            .canonical_bytes(source.value(), &mut exact, 4096)
            .unwrap();
        assert_eq!(
            encoded.bytes(),
            "{\"a\":\"𝄞\",\"z\":[1,\"\\u0001\"]}".as_bytes()
        );
        let n = exact.used;
        drop(encoded);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let good = arena
            .canonical_bytes(source.value(), &mut CountingWork::new(n), 4096)
            .unwrap();
        drop(good);
        assert_eq!(
            arena
                .canonical_bytes(source.value(), &mut CountingWork::new(n - 1), 4096)
                .err()
                .unwrap()
                .code,
            "TEST_EXHAUSTED"
        );
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        assert_eq!(
            arena
                .canonical_bytes(source.value(), &mut NoopValidationWork, 1)
                .err()
                .unwrap()
                .code,
            "PROGRAM_BUDGET_EXCEEDED"
        );
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let occupied = arena
            .reserve(LIMIT - baseline - 1, &mut NoopValidationWork)
            .unwrap();
        let mut untouched = CountingWork::new(usize::MAX);
        assert!(
            arena
                .canonical_bytes(source.value(), &mut untouched, 4096)
                .is_err()
        );
        assert_eq!(untouched.written, 0);
        drop(occupied);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn owned_sink_never_grows_after_admission_and_refusal_cannot_finish() {
        use crate::validation_work::tests::CountingWork;
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut work = CountingWork::new(usize::MAX);
        let mut sink = arena.byte_builder(3, &mut work).unwrap();
        sink.append(b"a", &mut work).unwrap();
        sink.append(b"bc", &mut work).unwrap();
        let bytes = sink.finish(&mut work).unwrap();
        assert_eq!(bytes.bytes(), b"abc");
        assert_eq!(bytes.reserved_bytes(), 3);
        drop(bytes);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let mut sink = arena.byte_builder(2, &mut NoopValidationWork).unwrap();
        sink.append(b"a", &mut NoopValidationWork).unwrap();
        assert!(sink.append(b"bc", &mut NoopValidationWork).is_err());
        assert!(sink.finish(&mut NoopValidationWork).is_err());
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let mut sink = arena.byte_builder(2, &mut NoopValidationWork).unwrap();
        assert!(sink.append(b"a", &mut CountingWork::new(0)).is_err());
        assert!(sink.finish(&mut NoopValidationWork).is_err());
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn paid_parser_and_cleanup_refusal_leave_no_owner_or_balance_change() {
        use crate::validation_work::tests::CountingWork;
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let raw = br#"{"a":["escaped\n",1]}"#;
        let mut work = CountingWork::new(usize::MAX);
        let value = arena.parse_bounded(raw, 4096, &mut work).unwrap();
        let n = work.used;
        drop(value);
        for cap in [0, 1, 3, n - 1] {
            assert!(
                arena
                    .parse_bounded(raw, 4096, &mut CountingWork::new(cap))
                    .is_err()
            );
            assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        }
        let value = arena
            .parse_bounded(raw, 4096, &mut CountingWork::new(n))
            .unwrap();
        drop(value);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
    #[test]
    fn owning_artifacts_are_send_and_poisoned_admission_stays_closed() {
        fn must_send<T: Send>() {}
        must_send::<AuditArena>();
        must_send::<AuditedValue>();
        must_send::<AuditedBytes>();
        must_send::<AuditedByteBuilder>();
        let arena = AuditArena::new().unwrap();
        let value = arena
            .parse_bounded(br#"["owned"]"#, 4096, &mut NoopValidationWork)
            .unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = arena.ledger.lock().unwrap();
            panic!("synthetic lock poison");
        }));
        assert!(arena.usage().is_err());
        assert!(
            arena
                .copy_bytes(b"closed", &mut NoopValidationWork)
                .is_err()
        );
        drop(value);
        assert!(
            arena
                .parse_bounded(br#"[]"#, 4096, &mut NoopValidationWork)
                .is_err()
        );
    }
}
