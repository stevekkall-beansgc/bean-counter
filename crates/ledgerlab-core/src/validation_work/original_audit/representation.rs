//! RP01 closed representation operations. No caller can obtain this module's
//! ValidationWork adapter, supply a budget, or run business validation in C.
use super::OriginalAuditSession;
use crate::{
    Error, Result,
    adjudication::proofs::{self, Base64Sink},
    canonical::audit_arena::{AuditArena, AuditedByteBuilder, AuditedBytes, AuditedValue},
    validation_work::{
        self as w, ArithmeticOperation, ValidationVisit, ValidationWork, economic_meter::Meter,
    },
};
use num_bigint::BigInt;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepresentationKind {
    OriginalRequest,
    EvidenceContent,
    SourceDefinition,
    SourceClosure,
    Authority,
    Record,
    OperationFrame,
    ManifestFrame,
    ReceiptFrame,
}
impl RepresentationKind {
    pub(crate) fn maximum(self) -> usize {
        match self {
            Self::OriginalRequest | Self::SourceDefinition => 262144,
            Self::EvidenceContent => 4096,
            Self::SourceClosure | Self::ReceiptFrame => 1048576,
            Self::Authority => 65536,
            Self::Record | Self::ManifestFrame => 4194304,
            Self::OperationFrame => 8388608,
        }
    }
    fn decoded_maximum(self) -> Result<usize> {
        match self {
            Self::OriginalRequest | Self::EvidenceContent => Ok(self.maximum()),
            _ => Err(failure("kind has no registered base64 wire recipe")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceVectorUsage {
    pub semantic_work: u64,
    pub representation_work: u64,
    pub semantic_callbacks: u64,
    pub representation_callbacks: u64,
    pub representation_calls: u64,
    pub scanned_bytes: u64,
    pub written_bytes: u64,
    pub hashed_bytes: u64,
    pub trace_metadata_bytes: u64,
    pub trace_digest: [u8; 32],
}
/// Fixed-size streaming diagnostics. No per-event Vec/operand/site clone in v2.
/// This is a deterministic accounting trace, not custody or a receipt hash.
pub(in crate::validation_work) struct ResourceLedger {
    semantic: u64,
    representation: u64,
    semantic_callbacks: u64,
    representation_callbacks: u64,
    calls: u64,
    scanned: u64,
    written: u64,
    hashed: u64,
    trace_metadata: u64,
    trace: Sha256,
}
fn failure(s: &str) -> Error {
    Error::new("AUDIT_INTEGRITY_OR_PROFILE_FAILURE", s)
}
fn sum(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| failure("resource counter overflow"))
}
fn product(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or_else(|| failure("resource envelope overflow"))
}
impl ResourceLedger {
    pub(in crate::validation_work) fn new() -> Self {
        Self {
            semantic: 0,
            representation: 0,
            semantic_callbacks: 0,
            representation_callbacks: 0,
            calls: 0,
            scanned: 0,
            written: 0,
            hashed: 0,
            trace_metadata: 0,
            trace: Sha256::new(),
        }
    }
    pub(in crate::validation_work) fn event(
        &mut self,
        representation: bool,
        kind: &'static str,
        cost: u64,
        operands: impl Iterator<Item = u64>,
    ) -> Result<()> {
        let next = sum(
            if representation {
                self.representation
            } else {
                self.semantic
            },
            cost,
        )?;
        if !representation && next > 1048576 {
            return Err(failure("composed original semantic work"));
        }
        let callbacks = sum(
            if representation {
                self.representation_callbacks
            } else {
                self.semantic_callbacks
            },
            1,
        )?;
        // Prefix, length framing and integer operands use fixed stack bytes.
        // The metadata hashing pass is counted separately, never recursively
        // treated as another economic/codec instruction.
        let head = 1 + 8 + kind.len() + 8;
        let mut metadata = sum(self.trace_metadata, head as u64)?;
        self.trace.update([u8::from(representation)]);
        self.trace.update((kind.len() as u64).to_be_bytes());
        self.trace.update(kind.as_bytes());
        self.trace.update(cost.to_be_bytes());
        for operand in operands {
            metadata = sum(metadata, 8)?;
            self.trace.update(operand.to_be_bytes());
        }
        self.trace.update([0xff]);
        metadata = sum(metadata, 1)?;
        self.trace_metadata = metadata;
        if representation {
            self.representation = next;
            self.representation_callbacks = callbacks;
        } else {
            self.semantic = next;
            self.semantic_callbacks = callbacks;
        }
        Ok(())
    }
    pub(in crate::validation_work) fn bytes(
        &mut self,
        scan: u64,
        write: u64,
        hash: u64,
    ) -> Result<()> {
        let scanned = sum(self.scanned, scan)?;
        let written = sum(self.written, write)?;
        let hashed = sum(self.hashed, hash)?;
        self.scanned = scanned;
        self.written = written;
        self.hashed = hashed;
        Ok(())
    }
    pub(in crate::validation_work) fn begin(&mut self) -> Result<()> {
        self.calls = sum(self.calls, 1)?;
        Ok(())
    }
    pub(in crate::validation_work) fn usage(&self) -> ResourceVectorUsage {
        ResourceVectorUsage {
            semantic_work: self.semantic,
            representation_work: self.representation,
            semantic_callbacks: self.semantic_callbacks,
            representation_callbacks: self.representation_callbacks,
            representation_calls: self.calls,
            scanned_bytes: self.scanned,
            written_bytes: self.written,
            hashed_bytes: self.hashed,
            trace_metadata_bytes: self.trace_metadata,
            trace_digest: self.trace.clone().finalize().into(),
        }
    }
}
/// One finite source-derived kernel bound; complete caller multiplicity and
/// simultaneous artifact/stack bounds remain required by candidate admission.
struct OperationEnvelope {
    work: u64,
}
impl OperationEnvelope {
    fn copy() -> Self {
        Self { work: 25 }
    } // kind Compare3 +reserve19 +capacity Compare3
    fn hash() -> Self {
        Self { work: 3 }
    } // existing-kind byte-bound comparison
    fn decode(n: usize, m: usize) -> Result<Self> {
        if !n.is_multiple_of(4) || n > m.div_ceil(3) * 4 {
            return Ok(Self { work: 62 });
        }
        let q = n as u64 / 4;
        Ok(Self {
            work: sum(
                62,
                sum(product(146, q)?, product(10, (3 * q).min(m as u64 + 1))?)?,
            )?,
        })
    }
    fn parse(n: usize, m: usize) -> Result<Self> {
        if n > m {
            return Ok(Self { work: 32 });
        }
        // Syntax/digit32, each string allocation96, map insertion128,
        // array growth/finish128 per token-byte upper; duplicate comparisons
        // include the paid length arithmetic and MemberCompare at most8/pair.
        // Every token/key/child count is <=N, sum-of-object-pairs <=N².
        let n = n as u64;
        Ok(Self {
            work: sum(
                32,
                sum(
                    product(32 + 96 + 128 + 128, n)?,
                    product(8, product(n, n)?)?,
                )?,
            )?,
        })
    }
    fn canonical(m: usize) -> Result<Self> {
        // Legal output has at most M values, key/string bytes and sort entries.
        // Two paid traversals plus allocation/formatting arithmetic fit 512M;
        // insertion-sort comparisons/moves fit 8M². An oversized borrowed tree
        // must stop at this same finite prefix, never acquire another allowance.
        let m = m as u64;
        Ok(Self {
            work: sum(128, sum(product(512, m)?, product(8, product(m, m)?)?)?)?,
        })
    }
    fn encode(n: usize, m: usize) -> Result<Self> {
        if n > m {
            return Ok(Self { work: 3 });
        }
        // Bound check3; ceil-length arithmetic9; builder acquire22/finish1;
        // each group: Element1, three (compare3+mul3+add3), four
        // (add3+compare3+div3+rem3+div3), append7 =>95 <= 96.
        Ok(Self {
            work: sum(35, product(96, (n as u64).div_ceil(3))?)?,
        })
    }
}
struct RepresentationCall<'a> {
    meter: &'a mut Meter,
    remaining: u64,
    completed: bool,
}
impl RepresentationCall<'_> {
    fn debit(&mut self, kind: &'static str, operands: &[&BigInt], cost: u64) -> Result<()> {
        if cost > self.remaining {
            self.meter.fail_original();
            return Err(failure("closed representation source envelope exhausted"));
        }
        self.meter.representation_event(kind, operands, cost)?;
        self.remaining -= cost;
        Ok(())
    }
}
impl Drop for RepresentationCall<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.meter.fail_original();
        }
    }
}
impl ValidationWork for RepresentationCall<'_> {
    fn visit(&mut self, kind: ValidationVisit) -> Result<()> {
        let name = match kind {
            ValidationVisit::Value => "repr.value",
            ValidationVisit::Field => "repr.field",
            ValidationVisit::Element => "repr.element",
            ValidationVisit::Constraint => "repr.constraint",
            ValidationVisit::ScalarDigit => "repr.digit",
            ValidationVisit::MemberCompare => "repr.member",
            ValidationVisit::RefLookup => "repr.lookup",
            ValidationVisit::Edge => "repr.edge",
            ValidationVisit::SortMove => "repr.move",
        };
        self.debit(name, &[], 1)
    }
    fn arithmetic(&mut self, kind: ArithmeticOperation, operands: &[&BigInt]) -> Result<()> {
        let mut cost = 1u64;
        for operand in operands {
            if operand.bits() > 512 {
                self.meter.fail_original();
                return Err(Error::new(
                    "ARITHMETIC_OVERFLOW",
                    "512-bit representation operand",
                ));
            }
            cost = sum(cost, operand.bits().div_ceil(64).max(1))?;
        }
        let name = match kind {
            ArithmeticOperation::Add => "repr.add",
            ArithmeticOperation::Subtract => "repr.subtract",
            ArithmeticOperation::Multiply => "repr.multiply",
            ArithmeticOperation::DivideRemainder => "repr.divrem",
            ArithmeticOperation::Compare => "repr.compare",
            ArithmeticOperation::Negate => "repr.negate",
            ArithmeticOperation::Absolute => "repr.absolute",
        };
        self.debit(name, operands, cost)
    }
    fn scan_bytes(&mut self, n: usize) -> Result<()> {
        self.meter.scan_bytes(n)
    }
    fn write_bytes(&mut self, n: usize) -> Result<()> {
        self.meter.write_bytes(n)
    }
    fn hash_bytes(&mut self, n: usize) -> Result<()> {
        self.meter.hash_bytes(n)
    }
}
fn run<T>(
    arena: &AuditArena,
    meter: &mut Meter,
    envelope: OperationEnvelope,
    operation: impl FnOnce(&AuditArena, &mut dyn ValidationWork) -> Result<T>,
) -> Result<T> {
    meter.begin_representation()?;
    let mut call = RepresentationCall {
        meter,
        remaining: envelope.work,
        completed: false,
    };
    let result = operation(arena, &mut call);
    if result.is_ok() {
        call.completed = true;
    }
    result
}
impl OriginalAuditSession {
    pub fn for_resource_profile_v2(attempt_arena: &AuditArena) -> Result<Self> {
        let mut session = Self::new(attempt_arena)?;
        session.meter.enable_resource_profile_v2()?;
        Ok(session)
    }
    pub fn semantic_work(&mut self) -> &mut dyn ValidationWork {
        &mut self.meter
    }
    pub fn resource_usage(&self) -> Option<ResourceVectorUsage> {
        self.meter.resource_vector_usage()
    }
    pub fn copy_representation(
        &mut self,
        bytes: &[u8],
        kind: RepresentationKind,
    ) -> Result<AuditedBytes> {
        run(
            &self.arena,
            &mut self.meter,
            OperationEnvelope::copy(),
            |arena, w| {
                if w::compare(w, bytes.len(), kind.maximum())?.is_gt() {
                    return Err(failure("representation kind byte limit"));
                }
                arena.copy_bytes(bytes, w)
            },
        )
    }
    pub fn hash_representation(
        &mut self,
        bytes: &[u8],
        kind: RepresentationKind,
    ) -> Result<[u8; 32]> {
        run(
            &self.arena,
            &mut self.meter,
            OperationEnvelope::hash(),
            |_, w| {
                if w::compare(w, bytes.len(), kind.maximum())?.is_gt() {
                    return Err(failure("representation kind byte limit"));
                }
                w.hash_bytes(bytes.len())?;
                Ok(Sha256::digest(bytes).into())
            },
        )
    }
    pub fn parse_representation(
        &mut self,
        raw: &[u8],
        kind: RepresentationKind,
    ) -> Result<AuditedValue> {
        let envelope = OperationEnvelope::parse(raw.len(), kind.maximum())?;
        run(&self.arena, &mut self.meter, envelope, |arena, w| {
            arena.parse_bounded(raw, kind.maximum(), w)
        })
    }
    pub fn canonical_representation(
        &mut self,
        value: &Value,
        kind: RepresentationKind,
    ) -> Result<AuditedBytes> {
        let envelope = OperationEnvelope::canonical(kind.maximum())?;
        run(&self.arena, &mut self.meter, envelope, |arena, w| {
            use crate::canonical::CanonicalContainerBound as Bound;
            match kind {
                RepresentationKind::OperationFrame => {
                    arena.canonical_container_bytes(value, w, Bound::Operation)
                }
                RepresentationKind::ManifestFrame => {
                    arena.canonical_container_bytes(value, w, Bound::Manifest)
                }
                RepresentationKind::ReceiptFrame => {
                    arena.canonical_container_bytes(value, w, Bound::Receipt)
                }
                _ => arena.canonical_bytes(value, w, kind.maximum()),
            }
        })
    }
    pub fn encode_base64_representation(
        &mut self,
        raw: &[u8],
        kind: RepresentationKind,
    ) -> Result<AuditedBytes> {
        let maximum = kind
            .decoded_maximum()
            .inspect_err(|_| self.meter.fail_original())?;
        let envelope = OperationEnvelope::encode(raw.len(), maximum)?;
        run(&self.arena, &mut self.meter, envelope, |arena, work| {
            use ArithmeticOperation as Op;
            if w::compare(work, raw.len(), maximum)?.is_gt() {
                return Err(failure("base64 decoded kind byte limit"));
            }
            let rounded = w::usize_add(work, raw.len(), 2)?;
            let groups = w::i64_operation(work, Op::DivideRemainder, rounded as i64, 3)?;
            let length = w::i64_operation(work, Op::Multiply, groups, 4)? as usize;
            let mut output = arena.byte_builder(length, work)?;
            let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            for chunk in raw.chunks(3) {
                work.visit(ValidationVisit::Element)?;
                work.scan_bytes(chunk.len())?;
                let mut value = 0i64;
                for i in 0..3usize {
                    let byte = if w::compare(work, i, chunk.len())?.is_lt() {
                        i64::from(chunk[i])
                    } else {
                        0
                    };
                    let shifted = w::i64_operation(work, Op::Multiply, value, 256)?;
                    value = w::i64_operation(work, Op::Add, shifted, byte)?;
                }
                let mut encoded = [b'='; 4];
                let mut denominator = 262144i64;
                for (i, digit) in encoded.iter_mut().enumerate() {
                    let count = w::usize_add(work, chunk.len(), 1)?;
                    if w::compare(work, i, count)?.is_lt() {
                        let quotient =
                            w::i64_operation(work, Op::DivideRemainder, value, denominator)?;
                        let index = w::i64_remainder(work, quotient, 64)?;
                        *digit = alphabet[index as usize];
                    }
                    denominator = w::i64_operation(work, Op::DivideRemainder, denominator, 64)?;
                }
                output.append(&encoded, work)?;
            }
            output.finish(work)
        })
    }
    pub fn decode_base64_representation(
        &mut self,
        text: &str,
        kind: RepresentationKind,
    ) -> Result<AuditedBytes> {
        let maximum = kind
            .decoded_maximum()
            .inspect_err(|_| self.meter.fail_original())?;
        let envelope = OperationEnvelope::decode(text.len(), maximum)?;
        run(&self.arena, &mut self.meter, envelope, |arena, w| {
            struct Sink {
                output: AuditedByteBuilder,
                written: usize,
            }
            impl Base64Sink for Sink {
                fn push(&mut self, b: u8, w: &mut dyn ValidationWork) -> Result<()> {
                    self.written = w::usize_add(w, self.written, 1)?;
                    self.output.append(&[b], w)
                }
                fn len(&self) -> usize {
                    self.written
                }
            }
            let mut sink = Sink {
                output: arena.byte_builder(maximum, w)?,
                written: 0,
            };
            proofs::decode_base64_into(text, maximum, &mut sink, w)?;
            sink.output.finish(w)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v2_representation_is_owning_and_separate_from_semantics() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        session
            .semantic_work()
            .visit(ValidationVisit::Constraint)
            .unwrap();
        let parsed = session
            .parse_representation(
                br#"{ "b": [1,"x"], "a": true }"#,
                RepresentationKind::Record,
            )
            .unwrap();
        let canonical = session
            .canonical_representation(parsed.value(), RepresentationKind::Record)
            .unwrap();
        assert_eq!(canonical.bytes(), br#"{"a":true,"b":[1,"x"]}"#);
        let copied = session
            .copy_representation(canonical.bytes(), RepresentationKind::Record)
            .unwrap();
        assert_eq!(copied.bytes(), canonical.bytes());
        let digest = session
            .hash_representation(canonical.bytes(), RepresentationKind::Record)
            .unwrap();
        assert_eq!(
            digest.as_slice(),
            Sha256::digest(canonical.bytes()).as_slice()
        );
        let usage = session.resource_usage().unwrap();
        assert_eq!(usage.semantic_work, 1);
        assert!(usage.representation_work > 0);
        assert_eq!(usage.representation_calls, 4);
        assert!(session.meter.usage().trace.is_empty());
        assert!(arena.usage().unwrap().live_bytes > 1024);
        drop((parsed, canonical, copied));
        assert_eq!(arena.usage().unwrap().live_bytes, 1024);
    }
    #[test]
    fn v1_trace_and_v2_composed_child_use_their_original_owner() {
        let arena = AuditArena::new().unwrap();
        let mut old = OriginalAuditSession::new(&arena).unwrap();
        old.work().visit(ValidationVisit::Constraint).unwrap();
        assert_eq!(old.meter.usage().trace.len(), 1);
        assert!(old.resource_usage().is_none());
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        session
            .semantic_work()
            .visit(ValidationVisit::Constraint)
            .unwrap();
        session
            .copy_representation(b"parent", RepresentationKind::Record)
            .unwrap();
        let mut child = session.meter.original_economic_meter().unwrap();
        child.visit("child.actual_semantic").unwrap();
        let usage = session.meter.finish_original_economic(child).unwrap();
        assert!(usage.trace.is_empty());
        session
            .semantic_work()
            .visit(ValidationVisit::Constraint)
            .unwrap();
        assert_eq!(session.usage().semantic_work, 3);
        assert_eq!(session.resource_usage().unwrap().semantic_work, 3);
        assert_eq!(session.resource_usage().unwrap().representation_calls, 1);
    }
    #[test]
    fn strict_representation_padding_and_caught_refusal_poison() {
        let arena = AuditArena::new().unwrap();
        for (raw, encoded) in [
            (b"".as_slice(), b"".as_slice()),
            (b"f", b"Zg=="),
            (b"fo", b"Zm8="),
            (b"foo", b"Zm9v"),
        ] {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let bytes = session
                .encode_base64_representation(raw, RepresentationKind::EvidenceContent)
                .unwrap();
            assert_eq!(bytes.bytes(), encoded);
            let decoded = session
                .decode_base64_representation(
                    std::str::from_utf8(bytes.bytes()).unwrap(),
                    RepresentationKind::EvidenceContent,
                )
                .unwrap();
            assert_eq!(decoded.bytes(), raw);
            assert_eq!(session.resource_usage().unwrap().semantic_work, 0);
        }
        for text in ["Zg=", "Zh==", "Zm9=", "Zg==AAAA", "☃==="] {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(
                session
                    .decode_base64_representation(text, RepresentationKind::EvidenceContent)
                    .is_err()
            );
            assert!(
                session
                    .semantic_work()
                    .visit(ValidationVisit::Constraint)
                    .is_err()
            );
        }
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(
            session
                .parse_representation(br#"{"same":1,"same":2}"#, RepresentationKind::Record)
                .is_err()
        );
        assert!(
            session
                .copy_representation(b"caught", RepresentationKind::Record)
                .is_err()
        );
    }
    #[test]
    fn original_raw_maximum_roundtrips_without_semantic_or_trace_growth() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let raw: Vec<u8> = (0..262144usize).map(|i| (i % 256) as u8).collect();
        let encoded = session
            .encode_base64_representation(&raw, RepresentationKind::OriginalRequest)
            .unwrap();
        let decoded = session
            .decode_base64_representation(
                std::str::from_utf8(encoded.bytes()).unwrap(),
                RepresentationKind::OriginalRequest,
            )
            .unwrap();
        assert_eq!(decoded.bytes(), raw);
        assert_eq!(session.resource_usage().unwrap().semantic_work, 0);
        assert!(session.meter.usage().trace.is_empty());
        assert_eq!(session.resource_usage().unwrap().representation_calls, 2);
        let mut refused = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let oversized = [0; 262145];
        assert!(
            refused
                .encode_base64_representation(&oversized, RepresentationKind::OriginalRequest)
                .is_err()
        );
        assert!(
            refused
                .semantic_work()
                .visit(ValidationVisit::Constraint)
                .is_err()
        );
    }
    #[test]
    fn closed_envelope_refuses_before_unpaid_operation_and_no_artifact_escapes() {
        let arena = AuditArena::new().unwrap();
        for budget in [22, 21] {
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let before = arena.usage().unwrap().live_bytes;
            let copied = run(
                &arena,
                &mut session.meter,
                OperationEnvelope { work: budget },
                |a, w| a.copy_bytes(b"one", w),
            );
            if budget == 22 {
                assert_eq!(copied.as_ref().unwrap().bytes(), b"one");
                assert_eq!(session.resource_usage().unwrap().representation_work, 22);
            } else {
                assert!(copied.is_err());
                assert_eq!(arena.usage().unwrap().live_bytes, before);
                assert!(
                    session
                        .semantic_work()
                        .visit(ValidationVisit::Constraint)
                        .is_err()
                );
            }
        }
    }
    #[test]
    fn attempt_cancellation_is_send_terminal_and_shared_by_origins_and_child() {
        fn send<T: Send>() {}
        send::<crate::canonical::audit_arena::AuditCancellation>();
        send::<crate::canonical::audit_arena::ExternalBufferLease>();
        let arena = AuditArena::new().unwrap();
        let handle = arena.cancellation_handle();
        let mut first = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut second = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let external = first.reserve_external_payload(4096).unwrap();
        assert_eq!(external.reserved_payload_bytes(), 4096);
        assert_eq!(first.resource_usage().unwrap().semantic_work, 19);
        let mut child = first.meter.original_economic_meter().unwrap();
        child.visit("before_cancel").unwrap();
        handle.cancel();
        assert_eq!(
            child.visit("after_cancel").unwrap_err().code,
            "AUDIT_CANCELLED"
        );
        assert!(first.meter.finish_original_economic(child).is_err());
        assert_eq!(
            second
                .copy_representation(b"no allocation", RepresentationKind::Record)
                .err()
                .unwrap()
                .code,
            "AUDIT_CANCELLED"
        );
        assert_eq!(
            second.check_cancelled().unwrap_err().code,
            "AUDIT_CANCELLED"
        );
        assert!(second.semantic_work().scan_bytes(1).is_err());
        // Request cancellation cannot prematurely release worker-owned payload.
        assert_eq!(arena.usage().unwrap().live_bytes, 1024 + 4096);
        drop(external);
        assert_eq!(arena.usage().unwrap().live_bytes, 1024);
    }
}
