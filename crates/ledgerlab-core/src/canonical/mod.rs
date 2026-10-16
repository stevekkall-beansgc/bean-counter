//! The integer-only ledger-canonical-v1 profile of RFC 8785.
//! Strings are JSON escaped; object keys compare UTF-16 code units, not UTF-8.
pub mod audit_arena;

use crate::validation_work::{
    self as vw, ArithmeticOperation as Op, NoopValidationWork, ValidationVisit as Visit,
    ValidationWork,
};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use std::cmp::Ordering;

const SAFE: i64 = 9_007_199_254_740_991;
pub const CANDIDATE_LIMIT: usize = 262_144;
pub const BUNDLE_LIMIT: usize = 4 * 1024 * 1024;

/// Existing journal framing limits, selected only by the owning resource
/// implementation. These do not widen the public canonical record/body API.
pub(crate) enum CanonicalContainerBound {
    Operation,
    Manifest,
    Receipt,
}
impl CanonicalContainerBound {
    pub(crate) fn maximum(&self) -> usize {
        match self {
            Self::Operation => 8 * 1024 * 1024,
            Self::Manifest => 4 * 1024 * 1024,
            Self::Receipt => 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalBytes(Vec<u8>);
impl CanonicalBytes {
    pub fn from_value(value: &impl Serialize) -> Result<Self> {
        Self::from_value_with_work(value, &mut NoopValidationWork)
    }
    pub fn from_value_with_work(
        value: &impl Serialize,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        let value = serde_json::to_value(value).map_err(|e| Error::new("JSON", e.to_string()))?;
        let mut destination = Destination::legacy();
        emit(&value, &mut destination, 0, work)?;
        if vw::compare(work, destination.bytes.len(), BUNDLE_LIMIT)? == Ordering::Greater {
            return Err(Error::new("LIMIT", "canonical bundle bytes"));
        }
        Ok(Self(destination.bytes))
    }
    /// Selected output uses the same paid emitter once. The supplied value is
    /// already decoded; no count walk or serde clone precedes emission. This
    /// path retains the selected body's null prohibition and cannot widen the
    /// existing bundle ceiling. Destination bounds exclude integer scratch.
    pub fn from_value_with_work_bounded(
        value: &Value,
        work: &mut dyn ValidationWork,
        maximum: usize,
    ) -> Result<Self> {
        let mut destination = Destination::bounded(maximum.min(BUNDLE_LIMIT));
        emit(value, &mut destination, 0, work)?;
        Ok(Self(destination.bytes))
    }
    /// The identical executing emitter for code-owned journal containers.
    /// Ownership/reservation must already have been admitted by the resource
    /// session; this entry alone provides no arena or authentication guarantee.
    pub(crate) fn from_container_with_work(
        value: &Value,
        work: &mut dyn ValidationWork,
        bound: CanonicalContainerBound,
    ) -> Result<Self> {
        let mut destination = Destination::bounded(bound.maximum());
        emit(value, &mut destination, 0, work)?;
        Ok(Self(destination.bytes))
    }
    /// RP01 owning entry only. Existing public paid recipes retain their
    /// previous callback trace; both paths execute the identical emitter.
    pub(crate) fn from_representation_with_work(value: &Value, work: &mut dyn ValidationWork, maximum: usize) -> Result<Self> {
        let mut destination = Destination::representation(maximum.min(BUNDLE_LIMIT));
        emit(value, &mut destination, 0, work)?;
        Ok(Self(destination.bytes))
    }
    pub(crate) fn from_representation_container_with_work(value: &Value, work: &mut dyn ValidationWork, bound: CanonicalContainerBound) -> Result<Self> {
        let mut destination = Destination::representation(bound.maximum());
        emit(value, &mut destination, 0, work)?;
        Ok(Self(destination.bytes))
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Reject ambiguous lexical JSON before constructing any wire DTO.
pub fn parse(bytes: &[u8]) -> Result<Value> {
    parse_bounded(bytes, CANDIDATE_LIMIT)
}
pub fn parse_bounded(bytes: &[u8], limit: usize) -> Result<Value> {
    parse_bounded_impl(
        bytes,
        limit,
        &mut NoopParseAllocationWork(NoopValidationWork),
        false,
        false,
    )
}

/// Allocation requests made before the corresponding library operation. These
/// describe payload capacity, not allocator bookkeeping or a physical RSS cap.
/// An audited caller must account for transient old/new array storage, pinned
/// serde string scratch/copy growth, and the pinned Map implementation's nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseAllocation {
    ArrayCapacity {
        current_capacity: usize,
        requested_capacity: usize,
        element_size: usize,
    },
    ObjectEntry {
        current_len: usize,
        key_bytes: usize,
        value_size: usize,
        string_size: usize,
    },
    StringDecode {
        encoded_bytes: usize,
    },
}

/// Mandatory allocation admission for the audited parser entry. Callback
/// errors propagate unchanged. A successful finish reports retained payload
/// capacity after old array storage or decoder scratch has been released.
/// Object inserts have no observable node capacity: their conservative admitted
/// node reservation stays live until the caller discards the parsed tree.
pub trait ParseAllocationWork {
    /// The SAME resource session used for allocation and semantic parser work.
    /// An audited caller cannot substitute an implicit compatibility counter.
    fn validation_work(&mut self) -> &mut dyn ValidationWork;
    fn before_allocation(&mut self, request: ParseAllocation) -> Result<()>;
    fn finish_allocation(
        &mut self,
        request: ParseAllocation,
        retained_capacity_bytes: usize,
    ) -> Result<()>;
}
struct NoopParseAllocationWork(NoopValidationWork);
impl ParseAllocationWork for NoopParseAllocationWork {
    fn validation_work(&mut self) -> &mut dyn ValidationWork {
        &mut self.0
    }
    fn before_allocation(&mut self, _: ParseAllocation) -> Result<()> {
        Ok(())
    }
    fn finish_allocation(&mut self, _: ParseAllocation, _: usize) -> Result<()> {
        Ok(())
    }
}

/// The same lexical parser with explicit fallible allocation admission. It
/// never selects a Noop adapter internally. Pinned opaque allocation bounds
/// remain the audited caller's proof obligation; this hook alone proves none.
pub fn parse_bounded_with_allocations(
    bytes: &[u8],
    limit: usize,
    allocations: &mut dyn ParseAllocationWork,
) -> Result<Value> {
    parse_bounded_impl(bytes, limit, allocations, true, false)
}
pub(crate) fn parse_bounded_with_allocations_complete(bytes: &[u8], limit: usize, allocations: &mut dyn ParseAllocationWork) -> Result<Value> {
    parse_bounded_impl(bytes, limit, allocations, true, true)
}
// Reserve a source-derived whole byte pass before entering opaque or lexical
// scanning. This is an upper reservation, not measured short-circuit traffic.
fn byte_pass_bound(work: &mut dyn ValidationWork, n: usize, multiplier: usize, fixed: usize) -> Result<usize> {
    work.arithmetic(Op::Multiply, &[&n.into(), &multiplier.into()])?;
    let scaled = n.checked_mul(multiplier).ok_or_else(|| Error::new("LIMIT", "JSON byte accounting"))?;
    work.arithmetic(Op::Add, &[&scaled.into(), &fixed.into()])?;
    scaled.checked_add(fixed).ok_or_else(|| Error::new("LIMIT", "JSON byte accounting"))
}
fn parse_bounded_impl(
    bytes: &[u8],
    limit: usize,
    allocations: &mut dyn ParseAllocationWork,
    audit_duplicates: bool,
    complete_representation: bool,
) -> Result<Value> {
    let work = allocations.validation_work();
    if vw::compare(work, bytes.len(), limit)? == Ordering::Greater {
        return Err(Error::new("LIMIT", "JSON bytes"));
    }
    // Token, UTF-8, escape and delimiter scanning are byte-domain work; do not
    // introduce per-character Value charges. Structural validity below is paid
    // where it executes, including refusals before allocation callbacks.
    work.scan_bytes(bytes.len())?;
    std::str::from_utf8(bytes).map_err(|_| Error::new("JSON", "invalid UTF-8"))?;
    // Monotone position means whitespace and string-body reads total <=N.
    // Space terminators<=10N, take<=4N, value tags<=N, container end tests<=4N,
    // numeric sign/digit/end<=4N and literals<=N: <=26N;32N+16 also covers
    // terminal lookahead. Number UTF8/conversion and serde are separate below.
    if complete_representation {
        let lexical = byte_pass_bound(work, bytes.len(), 32, 16)?;
        work.scan_bytes(lexical)?;
    }
    let mut p = Parser {
        bytes,
        pos: 0,
        allocations,
        audit_duplicates,
        complete_representation,
    };
    let result = p.value(0)?;
    p.space();
    if p.pos != bytes.len() {
        return Err(Error::new("JSON", "trailing input"));
    }
    Ok(result)
}
struct Parser<'a, 'w> {
    bytes: &'a [u8],
    pos: usize,
    allocations: &'w mut dyn ParseAllocationWork,
    audit_duplicates: bool,
    complete_representation: bool,
}
impl Parser<'_, '_> {
    fn space(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|c| b" \t\r\n".contains(c))
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, c: u8) -> Result<()> {
        self.space();
        if self.bytes.get(self.pos) != Some(&c) {
            return Err(Error::new("JSON", "unexpected token"));
        }
        self.pos += 1;
        Ok(())
    }
    fn string(&mut self) -> Result<String> {
        self.space();
        let start = self.pos;
        self.take(b'"')?;
        loop {
            match self.bytes.get(self.pos) {
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 2;
                }
                Some(_) => self.pos += 1,
                None => return Err(Error::new("JSON", "unterminated string")),
            }
        }
        let encoded_bytes = if self.complete_representation {
            vw::usize_sub(self.allocations.validation_work(), self.pos, start)?
        } else { self.pos - start };
        let request = ParseAllocation::StringDecode { encoded_bytes };
        // serde_json1.0.151 owns escape scratch and the returned String. Admit
        // their complete source-derived live bound before entering that decoder;
        // do not replace its Unicode, escape or error semantics with a new one.
        self.allocations.before_allocation(request)?;
        // Pinned serde SliceRead: skip/escape/UTF8/error-position passes,
        // scratch extends + geometric realloc copies, StringVisitor copy.
        // D<=E; summed geometric old capacities<4E. Scan<=16E+64 and
        // destination/scratch/realloc writes<=8E+64, including tiny capacity.
        if self.complete_representation {
            let work = self.allocations.validation_work();
            let scans = byte_pass_bound(work, encoded_bytes, 16, 64)?;
            work.scan_bytes(scans)?;
            let writes = byte_pass_bound(work, encoded_bytes, 8, 64)?;
            work.write_bytes(writes)?;
        }
        let decoded: String = serde_json::from_slice(&self.bytes[start..self.pos])
            .map_err(|e| Error::new("JSON", e.to_string()))?;
        self.allocations
            .finish_allocation(request, decoded.capacity())?;
        Ok(decoded)
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.allocations.validation_work().visit(Visit::Value)?;
        self.space();
        match self.bytes.get(self.pos).copied() {
            Some(b'{') => {
                if vw::compare(self.allocations.validation_work(), depth, 32usize)?
                    != Ordering::Less
                {
                    return Err(Error::new("LIMIT", "JSON depth"));
                }
                self.pos += 1;
                self.space();
                let mut map = Map::new();
                if self.bytes.get(self.pos) == Some(&b'}') {
                    self.pos += 1;
                    return Ok(Value::Object(map));
                }
                loop {
                    self.allocations.validation_work().visit(Visit::Field)?;
                    let key = self.string()?;
                    self.take(b':')?;
                    let child = vw::usize_add(self.allocations.validation_work(), depth, 1)?;
                    let value = self.value(child)?;
                    // Expose the actual duplicate-key constraint. Map's private
                    // balancing/storage comparisons are not semantic proof work.
                    // Preserve the original order: nested value validity precedes
                    // duplicate-key refusal (e.g. a malformed duplicate value).
                    if self.audit_duplicates {
                        for existing in map.keys() {
                            self.allocations
                                .validation_work()
                                .visit(Visit::MemberCompare)?;
                            key_comparison_bytes(existing, &key, self.allocations.validation_work())?;
                            // A second full candidate pass pre-admits the pinned
                            // BTree insertion search: each stored key can occur at
                            // most once on its unique root-to-leaf search path.
                            // No payload copy occurs when owned keys move into Map.
                            if self.complete_representation {
                                key_comparison_bytes(existing, &key, self.allocations.validation_work())?;
                            }
                            if existing == &key {
                                return Err(Error::new(
                                    "JSON_DUPLICATE_KEY",
                                    "duplicate object key",
                                ));
                            }
                        }
                    }
                    self.allocations
                        .before_allocation(ParseAllocation::ObjectEntry {
                            current_len: map.len(),
                            key_bytes: key.len(),
                            value_size: std::mem::size_of::<Value>(),
                            string_size: std::mem::size_of::<String>(),
                        })?;
                    // Compatibility keeps its original logarithmic map insertion;
                    // it is never selected by the audited entry.
                    let old = map.insert(key, value);
                    if !self.audit_duplicates && old.is_some() {
                        return Err(Error::new("JSON_DUPLICATE_KEY", "duplicate object key"));
                    }
                    self.space();
                    if self.bytes.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Object(map))
            }
            Some(b'[') => {
                if vw::compare(self.allocations.validation_work(), depth, 32usize)?
                    != Ordering::Less
                {
                    return Err(Error::new("LIMIT", "JSON depth"));
                }
                self.pos += 1;
                self.space();
                let mut out = Vec::new();
                if self.bytes.get(self.pos) == Some(&b']') {
                    self.pos += 1;
                    return Ok(Value::Array(out));
                }
                loop {
                    self.allocations.validation_work().visit(Visit::Element)?;
                    let child = vw::usize_add(self.allocations.validation_work(), depth, 1)?;
                    let value = self.value(child)?;
                    if vw::compare(
                        self.allocations.validation_work(),
                        out.len(),
                        out.capacity(),
                    )? == Ordering::Equal
                    {
                        let capacity = out.capacity();
                        self.allocations
                            .validation_work()
                            .arithmetic(Op::Multiply, &[&capacity.into(), &2usize.into()])?;
                        let doubled = capacity.saturating_mul(2);
                        let requested =
                            if vw::compare(self.allocations.validation_work(), doubled, 4usize)?
                                == Ordering::Less
                            {
                                4
                            } else {
                                doubled
                            };
                        let request = ParseAllocation::ArrayCapacity {
                            current_capacity: capacity,
                            requested_capacity: requested,
                            element_size: std::mem::size_of::<Value>(),
                        };
                        self.allocations.before_allocation(request)?;
                        self.allocations
                            .validation_work()
                            .arithmetic(Op::Subtract, &[&requested.into(), &out.len().into()])?;
                        out.try_reserve_exact(requested - out.len())
                            .map_err(|_| Error::new("LIMIT", "JSON array allocation"))?;
                        self.allocations.validation_work().arithmetic(
                            Op::Multiply,
                            &[&out.capacity().into(), &std::mem::size_of::<Value>().into()],
                        )?;
                        self.allocations.finish_allocation(
                            request,
                            out.capacity()
                                .checked_mul(std::mem::size_of::<Value>())
                                .ok_or_else(|| Error::new("LIMIT", "JSON array allocation"))?,
                        )?;
                    }
                    // The admitted capacity is already present; push cannot grow.
                    out.push(value);
                    self.space();
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Array(out))
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                if self.bytes[self.pos] == b'-' {
                    self.pos += 1;
                }
                while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                    self.pos += 1;
                }
                if self.complete_representation {
                    let token_bytes = vw::usize_sub(self.allocations.validation_work(), self.pos, start)?;
                    self.allocations.validation_work().scan_bytes(token_bytes)?;
                }
                let token = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| Error::new("JSON", "number"))?;
                let digits = token.strip_prefix('-').unwrap_or(token);
                let work = self.allocations.validation_work();
                work.visit(Visit::Constraint)?;
                let empty = digits.is_empty();
                let leading =
                    if !empty && vw::compare(work, digits.len(), 1usize)? == Ordering::Greater {
                        work.visit(Visit::Constraint)?;
                        digits.starts_with('0')
                    } else {
                        false
                    };
                work.visit(Visit::Constraint)?;
                if empty || leading || token == "-0" {
                    return Err(Error::new("JSON_NUMBER", "noncanonical numeric token"));
                }
                // Same signed-i64 language/error class, but execute each digit's
                // multiply/add (or subtract) through the actual work schedule.
                work.visit(Visit::Constraint)?;
                let negative = token.starts_with('-');
                let mut n = 0i64;
                if self.complete_representation { work.scan_bytes(digits.len())?; }
                for digit in digits.bytes() {
                    work.visit(Visit::ScalarDigit)?;
                    work.arithmetic(Op::Multiply, &[&n.into(), &10i64.into()])?;
                    n = n
                        .checked_mul(10)
                        .ok_or_else(|| Error::new("JSON_NUMBER", "unsafe integer"))?;
                    let digit = i64::from(digit - b'0'); // byte-token decoding, not value arithmetic
                    work.arithmetic(
                        if negative { Op::Subtract } else { Op::Add },
                        &[&n.into(), &digit.into()],
                    )?;
                    n = if negative {
                        n.checked_sub(digit)
                    } else {
                        n.checked_add(digit)
                    }
                    .ok_or_else(|| Error::new("JSON_NUMBER", "unsafe integer"))?;
                }
                let lower = vw::compare(work, n, -SAFE)?;
                let unsafe_range =
                    lower == Ordering::Less || vw::compare(work, n, SAFE)? == Ordering::Greater;
                if unsafe_range || self.bytes.get(self.pos).is_some_and(|b| b".eE".contains(b)) {
                    return Err(Error::new(
                        "JSON_NUMBER",
                        "only safe integer tokens allowed",
                    ));
                }
                Ok(Value::from(n))
            }
            _ => Err(Error::new("JSON", "unexpected token or BOM")),
        }
    }
    fn literal(&mut self, token: &[u8], value: Value) -> Result<Value> {
        if self.bytes.get(self.pos..self.pos + token.len()) != Some(token) {
            return Err(Error::new("JSON", "literal"));
        }
        self.pos += token.len();
        Ok(value)
    }
}
struct Destination {
    bytes: Vec<u8>,
    maximum: Option<usize>,
    complete_representation: bool,
}

// Reserve the full possible byte pass before each actually executed key
// comparison. This is a conservative deterministic charge, not a measurement
// of the comparison's short-circuit reads. The comparison algorithm is unchanged.
fn key_comparison_bytes(left: &str, right: &str, work: &mut dyn ValidationWork) -> Result<()> {
    let possible = vw::usize_add(work, left.len(), right.len())?;
    work.scan_bytes(possible)
}
impl Destination {
    fn legacy() -> Self {
        Self {
            bytes: Vec::new(),
            maximum: None,
            complete_representation: false,
        }
    }
    fn bounded(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum: Some(maximum),
            complete_representation: false,
        }
    }
    fn representation(maximum: usize) -> Self {
        Self { bytes: Vec::new(), maximum: Some(maximum), complete_representation: true }
    }
    fn prepare(&mut self, n: usize, work: &mut dyn ValidationWork) -> Result<()> {
        if let Some(maximum) = self.maximum {
            if self.complete_representation {
                work.arithmetic(Op::Add, &[&self.bytes.len().into(), &n.into()])?;
            }
            let next = self
                .bytes
                .len()
                .checked_add(n)
                .ok_or_else(destination_limit)?;
            let oversized = if self.complete_representation { vw::compare(work, next, maximum)?.is_gt() } else { next > maximum };
            if oversized {
                return Err(destination_limit());
            }
            work.write_bytes(n)?;
            // Avoid Vec's geometric growth crossing the requested destination
            // allowance. The requested capacity is checked before any reserve;
            // allocator bookkeeping is not an installation memory-cap claim.
            let growth = if self.complete_representation { vw::compare(work, next, self.bytes.capacity())?.is_gt() } else { next > self.bytes.capacity() };
            if growth {
                self.bytes.reserve_exact(n);
            }
        } else {
            work.write_bytes(n)?;
        }
        Ok(())
    }
}
fn destination_limit() -> Error {
    Error::new("PROGRAM_BUDGET_EXCEEDED", "canonical destination bound")
}
fn append(out: &mut Destination, bytes: &[u8], work: &mut dyn ValidationWork) -> Result<()> {
    out.prepare(bytes.len(), work)?;
    out.bytes.extend(bytes);
    Ok(())
}
fn emit_string(s: &str, out: &mut Destination, work: &mut dyn ValidationWork) -> Result<()> {
    append(out, b"\"", work)?;
    for character in s.chars() {
        work.scan_bytes(character.len_utf8())?;
        // Escaping an already typed string is byte work. The caller pays the
        // string's Value/Field visit; formatting characters are not new values.
        match character {
            '"' => append(out, b"\\\"", work)?,
            '\\' => append(out, b"\\\\", work)?,
            '\u{08}' => append(out, b"\\b", work)?,
            '\u{0c}' => append(out, b"\\f", work)?,
            '\n' => append(out, b"\\n", work)?,
            '\r' => append(out, b"\\r", work)?,
            '\t' => append(out, b"\\t", work)?,
            '\u{00}'..='\u{1f}' => {
                // The escape has exactly six ASCII bytes.
                out.prepare(6, work)?;
                out.bytes
                    .extend(format!("\\u{:04x}", u32::from(character)).as_bytes());
            }
            _ => {
                let mut encoded = [0; 4];
                append(out, character.encode_utf8(&mut encoded).as_bytes(), work)?;
            }
        }
    }
    append(out, b"\"", work)
}
fn emit_integer(i: i64, out: &mut Destination, work: &mut dyn ValidationWork) -> Result<()> {
    let negative = vw::compare(work, i, 0)? == Ordering::Less;
    let mut value = if negative {
        work.arithmetic(Op::Negate, &[&num_bigint::BigInt::from(i)])?;
        -i
    } else {
        i
    };
    let mut digits = Vec::new();
    loop {
        let digit = vw::i64_remainder(work, value, 10)?;
        let digit = vw::i64_operation(work, Op::Add, digit, i64::from(b'0'))?;
        work.write_bytes(1)?;
        digits.push(digit as u8);
        value = vw::i64_operation(work, Op::DivideRemainder, value, 10)?;
        if vw::compare(work, value, 0)? == Ordering::Equal {
            break;
        }
    }
    if negative {
        append(out, b"-", work)?;
    }
    for digit in digits.into_iter().rev() {
        append(out, &[digit], work)?;
    }
    Ok(())
}
fn emit(
    v: &Value,
    out: &mut Destination,
    depth: usize,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    work.visit(Visit::Value)?;
    if vw::compare(work, depth, 32usize)? == Ordering::Greater {
        if out.maximum.is_some() {
            return Err(destination_limit());
        }
        return Err(Error::new("LIMIT", "JSON depth"));
    }
    match v {
        Value::Null => {
            if out.maximum.is_some() {
                return Err(Error::new("INVALID_SHAPE", "null selected body"));
            }
            append(out, b"null", work)?;
        }
        Value::Bool(true) => append(out, b"true", work)?,
        Value::Bool(false) => append(out, b"false", work)?,
        Value::Number(n) => {
            work.visit(Visit::Constraint)?;
            let i = if let Some(i) = n.as_i64() {
                i
            } else {
                if out.maximum.is_some() {
                    work.visit(Visit::Constraint)?;
                    if !n.is_u64() {
                        return Err(Error::new("INVALID_SCALAR", "noninteger JSON"));
                    }
                }
                return Err(Error::new("JSON_NUMBER", "unsafe integer"));
            };
            if vw::compare(work, i, -SAFE)? == Ordering::Less
                || vw::compare(work, i, SAFE)? == Ordering::Greater
            {
                return Err(Error::new("JSON_NUMBER", "unsafe integer"));
            }
            emit_integer(i, out, work)?;
        }
        Value::String(s) => emit_string(s, out, work)?,
        Value::Array(items) => {
            append(out, b"[", work)?;
            for (i, item) in items.iter().enumerate() {
                work.visit(Visit::Element)?;
                if i > 0 {
                    append(out, b",", work)?;
                }
                let next = vw::usize_add(work, depth, 1)?;
                emit(item, out, next, work)?;
            }
            append(out, b"]", work)?;
        }
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            vw::try_sort_by(&mut keys, work, |a, b, w| {
                w.visit(Visit::MemberCompare)?;
                key_comparison_bytes(a, b, w)?;
                Ok(a.encode_utf16().cmp(b.encode_utf16()))
            })?;
            append(out, b"{", work)?;
            for (i, key) in keys.into_iter().enumerate() {
                work.visit(Visit::Field)?;
                if i > 0 {
                    append(out, b",", work)?;
                }
                emit_string(key, out, work)?;
                append(out, b":", work)?;
                work.visit(Visit::RefLookup)?;
                let next = vw::usize_add(work, depth, 1)?;
                emit(&map[key], out, next, work)?;
            }
            append(out, b"}", work)?;
        }
    }
    Ok(())
}

pub(crate) fn no_null(v: &Value) -> Result<()> {
    match v {
        Value::Null => return Err(Error::new("SCHEMA", "null is forbidden")),
        Value::Array(a) => {
            for v in a {
                no_null(v)?;
            }
        }
        Value::Object(o) => {
            for v in o.values() {
                no_null(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn dto<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    no_null(&v)?;
    serde_json::from_value(v).map_err(|e| Error::new("SCHEMA", e.to_string()))
}
pub fn sort_set<T: Serialize>(values: &mut Vec<T>) -> Result<()> {
    sort_set_with_work(values, &mut NoopValidationWork)
}
pub fn sort_set_with_work<T: Serialize>(
    values: &mut Vec<T>,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    let mut keyed = Vec::new();
    for value in values.drain(..) {
        work.visit(Visit::Element)?;
        let key = CanonicalBytes::from_value_with_work(&value, work)?.into_vec();
        keyed.push((key, value));
    }
    vw::try_sort_by(&mut keyed, work, |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        Ok(a.0.cmp(&b.0))
    })?;
    for pair in keyed.windows(2) {
        work.visit(Visit::MemberCompare)?;
        if pair[0].0 == pair[1].0 {
            return Err(Error::new("DUPLICATE", "set member"));
        }
    }
    *values = keyed.into_iter().map(|(_, v)| v).collect();
    Ok(())
}

/// Fixed domains; callers cannot invent a new hash domain from untrusted strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Domain {
    Event,
    Ingress,
    EventContent,
    Document,
    Claim,
    ClaimFacts,
    Link,
    Decision,
    Effect,
    EffectFacts,
    Action,
    Obligation,
    Intention,
    Receipt,
    DecisionContent,
    RecordContent,
    Chain,
    SnapshotRef,
    Explanation,
    ControlTransition,
    IntentionPayload,
    ReversalEffect,
}
impl Domain {
    pub fn name(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Ingress => "ingress",
            Self::EventContent => "event-content",
            Self::Document => "document",
            Self::Claim => "claim",
            Self::ClaimFacts => "claim-facts",
            Self::Link => "link",
            Self::Decision => "decision",
            Self::Effect => "effect",
            Self::EffectFacts => "effect-facts",
            Self::Action => "action",
            Self::Obligation => "obligation",
            Self::Intention => "intention",
            Self::Receipt => "receipt",
            Self::DecisionContent => "decision-content",
            Self::RecordContent => "record-content",
            Self::Chain => "chain",
            Self::SnapshotRef => "snapshot-ref",
            Self::Explanation => "explanation",
            Self::ControlTransition => "control-transition",
            Self::IntentionPayload => "intention-payload",
            Self::ReversalEffect => "reversal-effect",
        }
    }
    pub fn parse(name: &str) -> Result<Self> {
        [
            Self::Event,
            Self::Ingress,
            Self::EventContent,
            Self::Document,
            Self::Claim,
            Self::ClaimFacts,
            Self::Link,
            Self::Decision,
            Self::Effect,
            Self::EffectFacts,
            Self::Action,
            Self::Obligation,
            Self::Intention,
            Self::Receipt,
            Self::DecisionContent,
            Self::RecordContent,
            Self::Chain,
            Self::SnapshotRef,
            Self::Explanation,
            Self::ControlTransition,
            Self::IntentionPayload,
            Self::ReversalEffect,
        ]
        .into_iter()
        .find(|d| d.name() == name)
        .ok_or_else(|| Error::new("HASH_DOMAIN", name))
    }
    pub fn prefix(self) -> Result<&'static str> {
        Ok(match self {
            Self::Event => "ev",
            Self::Document => "doc",
            Self::Claim => "cl",
            Self::Link => "ln",
            Self::Decision => "dc",
            Self::Effect | Self::ReversalEffect => "ef",
            Self::Action => "ac",
            Self::Obligation => "ob",
            Self::Intention => "in",
            Self::Receipt => "rc",
            Self::SnapshotRef => "sr",
            Self::Explanation => "xp",
            Self::ControlTransition => "ct",
            _ => return Err(Error::new("HASH_DOMAIN", "domain has no ID")),
        })
    }
}
pub fn hash_input(domain: Domain, value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = format!("ledgerlab/{}/1\0", domain.name()).into_bytes();
    bytes.extend(CanonicalBytes::from_value(value)?.as_slice());
    Ok(bytes)
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn hex_with_work(bytes: &[u8], work: &mut dyn ValidationWork) -> Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::new();
    for byte in bytes {
        work.scan_bytes(1)?;
        work.write_bytes(2)?;
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 15)]);
    }
    Ok(String::from_utf8(out).expect("ASCII hexadecimal"))
}
pub fn hash(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(hex(&Sha256::digest(hash_input(domain, value)?)))
}
pub fn digest(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(format!("sha256:{}", hash(domain, value)?))
}
pub fn identity(domain: Domain, value: &impl Serialize) -> Result<String> {
    Ok(format!("{}_{}", domain.prefix()?, hash(domain, value)?))
}

/// Frozen outcome profiles use explicit domains; this does not change v1 hashes.
pub fn outcome_digest(profile: &str, domain: &str, value: &impl Serialize) -> Result<String> {
    if !["2-candidate.4", "reservation-settlement/1"].contains(&profile)
        || domain.is_empty()
        || !domain.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
    {
        return Err(Error::new("HASH_DOMAIN", "unsupported outcome domain"));
    }
    let mut h = Sha256::new();
    h.update(format!("ledgerlab/{domain}/{profile}\0").as_bytes());
    h.update(CanonicalBytes::from_value(value)?.as_slice());
    Ok(format!("sha256:{}", hex(&h.finalize())))
}

pub mod outcome;

#[cfg(test)]
mod parser_allocation_tests;

#[cfg(test)]
mod executing_work_tests {
    use super::*;
    use crate::validation_work::tests::CountingWork;
    use num_bigint::BigInt;

    #[test]
    fn key_comparison_reservation_is_full_pass_before_unchanged_comparison() {
        struct Scans { used: usize, maximum: usize }
        impl ValidationWork for Scans {
            fn visit(&mut self, _: Visit) -> Result<()> { Ok(()) }
            fn arithmetic(&mut self, _: Op, _: &[&BigInt]) -> Result<()> { Ok(()) }
            fn write_bytes(&mut self, _: usize) -> Result<()> { Ok(()) }
            fn hash_bytes(&mut self, _: usize) -> Result<()> { Ok(()) }
            fn scan_bytes(&mut self, n: usize) -> Result<()> {
                if n > self.maximum - self.used {
                    return Err(Error::new("TEST_SCAN", "comparison bytes"));
                }
                self.used += n;
                Ok(())
            }
        }
        let mut enough = Scans { used: 0, maximum: 16 };
        key_comparison_bytes("a-secret", "b-secret", &mut enough).unwrap();
        assert_eq!(enough.used,16); // full reserved pass, despite first-byte mismatch
        let mut short = Scans { used: 0, maximum: 15 };
        assert_eq!(key_comparison_bytes("a-secret", "b-secret", &mut short).unwrap_err(), Error::new("TEST_SCAN","comparison bytes"));
        assert_eq!(short.used,0);
        let value = serde_json::json!({"\u{e000}":1,"😀":2});
        let mut work = CountingWork::new(usize::MAX);
        assert_eq!(CanonicalBytes::from_value_with_work(&value, &mut work).unwrap().as_slice(), "{\"😀\":2,\"\u{e000}\":1}".as_bytes());
    }

    #[test]
    fn journal_container_emitter_preserves_closed_bounds_and_body_limit() {
        assert_eq!(CanonicalContainerBound::Operation.maximum(), 8 * 1024 * 1024);
        assert_eq!(CanonicalContainerBound::Manifest.maximum(), BUNDLE_LIMIT);
        assert_eq!(CanonicalContainerBound::Receipt.maximum(), 1024 * 1024);
        let value = Value::String("x".repeat(BUNDLE_LIMIT));
        let mut paid = CountingWork::new(usize::MAX);
        let encoded = CanonicalBytes::from_container_with_work(
            &value,
            &mut paid,
            CanonicalContainerBound::Operation,
        )
        .unwrap();
        assert_eq!(encoded.as_slice().len(), BUNDLE_LIMIT + 2);
        assert_eq!(encoded.as_slice().first(), Some(&b'"'));
        assert_eq!(encoded.as_slice().last(), Some(&b'"'));
        let mut public_work = CountingWork::new(usize::MAX);
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(
                &value,
                &mut public_work,
                usize::MAX,
            )
            .unwrap_err(),
            Error::new("PROGRAM_BUDGET_EXCEEDED", "canonical destination bound"),
        );
    }

    #[test]
    fn journal_container_emitter_keeps_exact_work_refusal_and_bytes() {
        let value = serde_json::json!({"escaped":"\n\u{1f600}","integer":-9});
        let mut paid = CountingWork::new(usize::MAX);
        let bytes = CanonicalBytes::from_container_with_work(
            &value,
            &mut paid,
            CanonicalContainerBound::Receipt,
        )
        .unwrap();
        assert_eq!(bytes, CanonicalBytes::from_value(&value).unwrap());
        let required = paid.used;
        let mut exact = CountingWork::new(required);
        assert_eq!(
            CanonicalBytes::from_container_with_work(
                &value,
                &mut exact,
                CanonicalContainerBound::Receipt,
            )
            .unwrap(),
            bytes,
        );
        let mut short = CountingWork::new(required - 1);
        assert!(CanonicalBytes::from_container_with_work(
            &value,
            &mut short,
            CanonicalContainerBound::Receipt,
        ).is_err());
    }

    #[test]
    fn string_formatting_pays_one_typed_value_and_every_scanned_written_byte() {
        let value = Value::String("a\n\u{000b}é😀\"\\".into());
        let expected = "\"a\\n\\u000bé😀\\\"\\\\\"";
        let mut work = CountingWork::new(usize::MAX);
        let bytes = CanonicalBytes::from_value_with_work_bounded(&value, &mut work, expected.len())
            .unwrap();
        assert_eq!(bytes.as_slice(), expected.as_bytes());
        assert_eq!(bytes, CanonicalBytes::from_value(&value).unwrap());
        assert_eq!(work.visits, [Visit::Value]);
        assert_eq!(work.scanned, value.as_str().unwrap().len());
        assert_eq!(work.written, expected.len());
        assert_eq!(work.hashed, 0);
        assert_eq!(work.operations.len(), 1); // Executed depth comparison.

        let mut longer = CountingWork::new(usize::MAX);
        CanonicalBytes::from_value_with_work_bounded(
            &Value::String("é".repeat(1024)),
            &mut longer,
            2050,
        )
        .unwrap();
        assert_eq!(longer.visits, work.visits);
        assert_eq!(longer.used, work.used);
        assert_eq!(longer.scanned, 2048);
        assert_eq!(longer.written, 2050);

        let mut exact = CountingWork::new(work.used);
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(&value, &mut exact, expected.len())
                .unwrap(),
            bytes
        );
        let mut short = CountingWork::new(work.used - 1);
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(&value, &mut short, expected.len())
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
        assert_eq!(short.scanned, 0);
        assert_eq!(short.written, 0);
    }

    #[test]
    fn old_paid_bounded_entry_keeps_its_original_trace() {
        let mut legacy = CountingWork::new(usize::MAX);
        assert_eq!(CanonicalBytes::from_value_with_work_bounded(&Value::String("a".into()), &mut legacy, 3).unwrap().as_slice(), br#""a""#);
        assert_eq!(legacy.visits, vec![Visit::Value]);
        assert!(legacy.operations.is_empty());
        assert_eq!((legacy.used,legacy.scanned,legacy.written),(1,1,3));
    }
    #[test]
    fn destination_arithmetic_refusal_precedes_reserve() {
        let mut full = CountingWork::new(usize::MAX);
        let mut destination = Destination::representation(1);
        destination.prepare(1, &mut full).unwrap();
        assert_eq!(full.operations, vec![
            (Op::Add, vec![0.into(), 1.into()]),
            (Op::Compare, vec![1.into(), 1.into()]),
            (Op::Compare, vec![1.into(), 0.into()]),
        ]);
        let mut short = CountingWork::new(full.used - 1);
        let mut refused = Destination::representation(1);
        assert_eq!(refused.prepare(1, &mut short).unwrap_err().code, "TEST_EXHAUSTED");
        assert_eq!(refused.bytes.capacity(), 0);
        assert!(refused.bytes.is_empty());
    }

    #[test]
    fn capped_single_pass_preserves_bytes_callbacks_and_exact_budgets() {
        let value = serde_json::json!({"\u{e000}":["\u{000b}","\"","\\","é"],"😀":-123});
        let expected = r#"{"😀":-123,"":["\u000b","\"","\\","é"]}"#;
        let n = expected.len();
        let mut legacy = CountingWork::new(usize::MAX);
        let old = CanonicalBytes::from_value_with_work(&value, &mut legacy).unwrap();
        let mut paid = CountingWork::new(usize::MAX);
        let bytes = CanonicalBytes::from_representation_with_work(&value, &mut paid, n).unwrap();
        assert_eq!(bytes.as_slice(), expected.as_bytes());
        assert_eq!(bytes, old);
        // Bounded destination guards now pay their own Add/Compare operations;
        // the original semantic arithmetic remains in the same relative order.
        let mut bounded_operations = paid.operations.iter();
        for original in &legacy.operations[..legacy.operations.len() - 1] {
            assert!(bounded_operations.any(|operation| operation == original));
        }
        assert!(paid.operations.len() > legacy.operations.len());
        assert_eq!(paid.visits, legacy.visits);
        assert_eq!(paid.scanned, legacy.scanned);
        assert_eq!(paid.written, legacy.written);
        let mut exact = CountingWork::new(paid.used);
        assert_eq!(
            CanonicalBytes::from_representation_with_work(&value, &mut exact, n).unwrap(),
            bytes
        );
        let mut short_work = CountingWork::new(paid.used - 1);
        assert_eq!(
            CanonicalBytes::from_representation_with_work(&value, &mut short_work, n)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
        let mut short_bytes = CountingWork::new(usize::MAX);
        assert_eq!(
            CanonicalBytes::from_representation_with_work(&value, &mut short_bytes, n - 1)
                .unwrap_err(),
            destination_limit()
        );
    }

    #[test]
    fn destination_refuses_before_reserve_escape_and_final_integer_append() {
        let value = Value::String("\u{000b}".repeat(1_048_576));
        let mut destination = Destination::bounded(2);
        let mut work = CountingWork::new(usize::MAX);
        assert_eq!(
            emit(&value, &mut destination, 0, &mut work).unwrap_err(),
            destination_limit()
        );
        assert_eq!(destination.bytes, b"\"");
        assert!(destination.bytes.capacity() <= 2);
        assert_eq!(work.written, 1); // No six-byte escape allocation/write was attempted.
        assert_eq!(work.scanned, 1); // No sizing traversal over the large tail.
        let mut destination = Destination::bounded(0);
        let mut work = CountingWork::new(usize::MAX);
        assert_eq!(
            emit(&Value::from(-123), &mut destination, 0, &mut work).unwrap_err(),
            destination_limit()
        );
        assert_eq!(destination.bytes.capacity(), 0);
        assert!(destination.bytes.is_empty());
        assert_eq!(work.written, 3); // Actual scratch digits are paid, not destination bytes.
        let mut destination = Destination::bounded(4);
        emit(&Value::from(-123), &mut destination, 0, &mut work).unwrap();
        assert_eq!(destination.bytes, b"-123");
        assert!(destination.bytes.capacity() <= 4);
        let mut destination = Destination::bounded(3);
        assert_eq!(
            emit(&Value::from(-123), &mut destination, 0, &mut work).unwrap_err(),
            destination_limit()
        );
        assert_eq!(destination.bytes, b"-12");
        assert!(destination.bytes.capacity() <= 3);
    }

    #[test]
    fn selected_refusals_and_callback_error_leave_legacy_policy_unchanged() {
        let mut work = NoopValidationWork;
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(&Value::Null, &mut work, 4).unwrap_err(),
            Error::new("INVALID_SHAPE", "null selected body")
        );
        assert_eq!(
            CanonicalBytes::from_value(&Value::Null).unwrap().as_slice(),
            b"null"
        );
        let float = Value::from(0.5);
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(&float, &mut work, 32).unwrap_err(),
            Error::new("INVALID_SCALAR", "noninteger JSON")
        );
        assert_eq!(
            CanonicalBytes::from_value(&float).unwrap_err().code,
            "JSON_NUMBER"
        );
        let mut deep = Value::Bool(true);
        for _ in 0..33 {
            deep = Value::Array(vec![deep]);
        }
        assert_eq!(
            CanonicalBytes::from_value_with_work_bounded(&deep, &mut work, 1024).unwrap_err(),
            destination_limit()
        );
        assert_eq!(
            CanonicalBytes::from_value(&deep).unwrap_err(),
            Error::new("LIMIT", "JSON depth")
        );
        let mut bytes = Destination::bounded(1);
        let callback = Error::new("INVALID_SHAPE", "distinct callback failure");
        struct FailedWrite(Error);
        impl ValidationWork for FailedWrite {
            fn visit(&mut self, _: Visit) -> Result<()> {
                Ok(())
            }
            fn arithmetic(&mut self, _: Op, _: &[&BigInt]) -> Result<()> {
                Ok(())
            }
            fn scan_bytes(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn hash_bytes(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn write_bytes(&mut self, _: usize) -> Result<()> {
                Err(self.0.clone())
            }
        }
        assert_eq!(
            emit(
                &Value::String("x".into()),
                &mut bytes,
                0,
                &mut FailedWrite(callback.clone())
            )
            .unwrap_err(),
            callback
        );
        assert_eq!(bytes.bytes.capacity(), 0);
        assert!(bytes.bytes.is_empty());
    }

    #[test]
    fn executing_jcs_preserves_utf16_escapes_integer_and_set_order() {
        let value = serde_json::json!({"\u{e000}":["\u{000b}","\u{0008}","\t","\n","\u{000c}","\r","\"","\\","é"],"😀":-42});
        let expected = r#"{"😀":-42,"":["\u000b","\b","\t","\n","\f","\r","\"","\\","é"]}"#;
        let mut work = CountingWork::new(usize::MAX);
        let bytes = CanonicalBytes::from_value_with_work(&value, &mut work).unwrap();
        assert_eq!(bytes.as_slice(), expected.as_bytes());
        assert_eq!(bytes, CanonicalBytes::from_value(&value).unwrap());
        assert!(work.visits.contains(&Visit::SortMove));
        let mut exact = CountingWork::new(work.used);
        assert_eq!(
            CanonicalBytes::from_value_with_work(&value, &mut exact).unwrap(),
            bytes
        );
        let mut short = CountingWork::new(work.used - 1);
        assert_eq!(
            CanonicalBytes::from_value_with_work(&value, &mut short)
                .unwrap_err()
                .code,
            "TEST_EXHAUSTED"
        );
        let mut members = vec![
            serde_json::json!({"b":"a","a":"z"}),
            serde_json::json!({"b":"z","a":"a"}),
        ];
        sort_set_with_work(&mut members, &mut exact).unwrap_err();
        let mut enough = CountingWork::new(usize::MAX);
        // Failed sort_set drains its local work buffer, as its legacy API did.
        // The meaningful successful sort uses a fresh input.
        let mut members = vec![
            serde_json::json!({"b":"a","a":"z"}),
            serde_json::json!({"b":"z","a":"a"}),
        ];
        sort_set_with_work(&mut members, &mut enough).unwrap();
        assert_eq!(members[0], serde_json::json!({"a":"a","b":"z"}));
    }

    #[test]
    fn callback_byte_error_is_returned_unchanged_before_emission() {
        struct ByteLimit {
            written: usize,
        }
        impl ValidationWork for ByteLimit {
            fn visit(&mut self, _: Visit) -> Result<()> {
                Ok(())
            }
            fn arithmetic(&mut self, _: Op, _: &[&BigInt]) -> Result<()> {
                Ok(())
            }
            fn scan_bytes(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn hash_bytes(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn write_bytes(&mut self, n: usize) -> Result<()> {
                if n > 2 - self.written {
                    return Err(Error::new("JSON", "distinct callback exhaustion"));
                }
                self.written += n;
                Ok(())
            }
        }
        let mut work = ByteLimit { written: 0 };
        let error = CanonicalBytes::from_value_with_work(&"\n", &mut work).unwrap_err();
        assert_eq!(error, Error::new("JSON", "distinct callback exhaustion"));
        assert_eq!(work.written, 1);
    }
}
