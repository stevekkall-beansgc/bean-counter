//! Exact retained bytes are distinct from trusted source-head authentication.
use super::{
    commands::{ExpectedPrefix, Proof, RetainedObject},
    ensure, raw_sha256,
    types::Digest,
    Validate, SEGMENT_BYTES,
};
use crate::{Error, Result};

/// Hash verification alone deliberately does not produce an authenticated proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedObjectBytes {
    object: RetainedObject,
    decoded: Vec<u8>,
}
impl VerifiedObjectBytes {
    pub fn check(object: RetainedObject) -> Result<Self> {
        object.validate()?;
        let decoded = decode_base64(&object.body, SEGMENT_BYTES)?;
        ensure(
            decoded.len() as u128 == object.bytes.value(),
            "OBJECT_LENGTH",
            "exact decoded length",
        )?;
        ensure(
            raw_sha256(&decoded) == object.body_hash,
            "OBJECT_HASH",
            "exact body hash",
        )?;
        let value = crate::canonical::parse_bounded(&decoded, SEGMENT_BYTES)?;
        ensure(
            super::canonical_bytes(&value, SEGMENT_BYTES)? == decoded,
            "CANONICAL",
            "object body",
        )?;
        Ok(Self { object, decoded })
    }
    pub fn object(&self) -> &RetainedObject {
        &self.object
    }
    pub fn bytes(&self) -> &[u8] {
        &self.decoded
    }
}
/// Check full membership against an independently authenticated historical prefix.
/// The caller must additionally reconstruct the named segment and its source chain.
pub fn check_membership_identity(
    proof: &Proof,
    object: &RetainedObject,
    source: &ExpectedPrefix,
) -> Result<()> {
    ensure(
        proof.store == source.store
            && proof.scope == source.scope
            && proof.registration == source.registration
            && proof.host == source.host
            && proof.ordinal == source.ordinal
            && proof.segment == source.segment
            && proof.root == source.root,
        "PROOF_PREFIX",
        "historical source prefix",
    )?;
    ensure(
        object.origin.store == proof.store
            && object.origin.scope == proof.scope
            && object.origin.registration == proof.registration
            && object.origin.host == proof.host
            && object.origin.ordinal == proof.ordinal
            && object.kind == proof.fact_kind
            && super::canonical_bytes(&object.full_key, SEGMENT_BYTES)?
                == super::canonical_bytes(&proof.full_key, SEGMENT_BYTES)?
            && object.body_hash == proof.body_hash
            && object.bytes == proof.bytes,
        "PROOF_MEMBER",
        "full source-qualified object identity",
    )
}
/// Closed alphabet, complete quartets and zero pad bits. No alternate encoding.
pub fn decode_base64(text: &str, max: usize) -> Result<Vec<u8>> {
    struct LegacySink(Vec<u8>);
    impl Base64Sink for LegacySink {
        fn begin(
            &mut self,
            capacity: usize,
            _: &mut dyn crate::validation_work::ValidationWork,
        ) -> Result<()> {
            self.0.reserve_exact(capacity);
            Ok(())
        }
        fn push(
            &mut self,
            byte: u8,
            work: &mut dyn crate::validation_work::ValidationWork,
        ) -> Result<()> {
            work.write_bytes(1)?;
            self.0.push(byte);
            Ok(())
        }
        fn len(&self) -> usize {
            self.0.len()
        }
    }
    // Preserve the legacy allocation strategy; the audited entry below never
    // creates a Vec or selects this compatibility adapter.
    let mut sink = LegacySink(Vec::new());
    decode_base64_into(
        text,
        max,
        &mut sink,
        &mut crate::validation_work::NoopValidationWork,
    )?;
    Ok(sink.0)
}

/// Output ownership belongs to the caller. An audited sink must admit growth
/// before it happens and pay the actual output write before append. It must
/// discard partial output on any error; no naked decoded Vec is returned.
pub trait Base64Sink {
    fn begin(
        &mut self,
        _capacity: usize,
        _work: &mut dyn crate::validation_work::ValidationWork,
    ) -> Result<()> {
        Ok(())
    }
    fn push(
        &mut self,
        byte: u8,
        work: &mut dyn crate::validation_work::ValidationWork,
    ) -> Result<()>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Same strict grammar and byte packing as decode_base64. Every executed
/// grammar decision and numeric operation is fallible work; no count prewalk
/// or opaque decoder runs after a synthetic charge. Small-byte shift/OR
/// packing is expressed as its exact bounded arithmetic equivalents.
pub fn decode_base64_into(
    text: &str,
    max: usize,
    out: &mut dyn Base64Sink,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> Result<()> {
    use crate::validation_work::{self as w, ArithmeticOperation as Op, ValidationVisit as Visit};
    use std::cmp::Ordering;
    fn sextet(b: u8, work: &mut dyn w::ValidationWork) -> Result<i64> {
        work.visit(Visit::ScalarDigit)?;
        for (low, high, base) in [(b'A', b'Z', 0), (b'a', b'z', 26), (b'0', b'9', 52)] {
            if w::compare(work, b, low)? != Ordering::Less
                && w::compare(work, b, high)? != Ordering::Greater
            {
                let value = w::i64_operation(work, Op::Subtract, i64::from(b), i64::from(low))?;
                return w::i64_operation(work, Op::Add, value, base);
            }
        }
        if w::compare(work, b, b'+')? == Ordering::Equal {
            return Ok(62);
        }
        if w::compare(work, b, b'/')? == Ordering::Equal {
            return Ok(63);
        }
        Err(Error::new("BASE64", "alphabet or padding"))
    }
    ensure(
        w::compare(work, out.len(), 0usize)? == Ordering::Equal,
        "BASE64",
        "nonempty output sink",
    )?;
    work.arithmetic(
        Op::DivideRemainder,
        &[
            &num_bigint::BigInt::from(text.len()),
            &num_bigint::BigInt::from(4),
        ],
    )?;
    let remainder = text.len() % 4;
    let quartets = w::compare(work, remainder, 0usize)? == Ordering::Equal;
    work.arithmetic(
        Op::DivideRemainder,
        &[&num_bigint::BigInt::from(max), &num_bigint::BigInt::from(3)],
    )?;
    let mut groups = max / 3;
    work.arithmetic(
        Op::DivideRemainder,
        &[&num_bigint::BigInt::from(max), &num_bigint::BigInt::from(3)],
    )?;
    let remainder = max % 3;
    if w::compare(work, remainder, 0usize)? != Ordering::Equal {
        groups = w::usize_add(work, groups, 1)?;
    }
    work.arithmetic(
        Op::Multiply,
        &[
            &num_bigint::BigInt::from(groups),
            &num_bigint::BigInt::from(4),
        ],
    )?;
    let bound = groups
        .checked_mul(4)
        .ok_or_else(|| Error::new("BASE64", "encoded length"))?;
    ensure(
        quartets && w::compare(work, text.len(), bound)? != Ordering::Greater,
        "BASE64",
        "encoded length",
    )?;
    work.arithmetic(
        Op::DivideRemainder,
        &[
            &num_bigint::BigInt::from(text.len()),
            &num_bigint::BigInt::from(4),
        ],
    )?;
    let count = text.len() / 4;
    work.arithmetic(
        Op::Multiply,
        &[
            &num_bigint::BigInt::from(count),
            &num_bigint::BigInt::from(3),
        ],
    )?;
    let capacity = count * 3;
    let capacity = if w::compare(work, capacity, max)? == Ordering::Less {
        capacity
    } else {
        max
    };
    out.begin(capacity, work)?;
    for (i, c) in text.as_bytes().as_chunks::<4>().0.iter().enumerate() {
        work.visit(Visit::Element)?;
        work.scan_bytes(4)?;
        let a = sextet(c[0], work)?;
        let b = sextet(c[1], work)?;
        let first = w::i64_operation(work, Op::Multiply, a, 4)?;
        let rest = w::i64_operation(work, Op::DivideRemainder, b, 16)?;
        let byte = w::i64_operation(work, Op::Add, first, rest)?;
        out.push(byte as u8, work)?;
        let next = w::usize_add(work, i, 1)?;
        work.arithmetic(
            Op::Multiply,
            &[
                &num_bigint::BigInt::from(next),
                &num_bigint::BigInt::from(4),
            ],
        )?;
        let last = w::compare(work, next * 4, text.len())? == Ordering::Equal;
        if w::compare(work, c[2], b'=')? == Ordering::Equal {
            let padding = if last && w::compare(work, c[3], b'=')? == Ordering::Equal {
                let rem = w::i64_remainder(work, b, 16)?;
                w::compare(work, rem, 0i64)? == Ordering::Equal
            } else {
                false
            };
            ensure(padding, "BASE64", "canonical pad bits")?;
        } else {
            let d = sextet(c[2], work)?;
            let first = w::i64_remainder(work, b, 16)?;
            let first = w::i64_operation(work, Op::Multiply, first, 16)?;
            let rest = w::i64_operation(work, Op::DivideRemainder, d, 4)?;
            let byte = w::i64_operation(work, Op::Add, first, rest)?;
            out.push(byte as u8, work)?;
            if w::compare(work, c[3], b'=')? == Ordering::Equal {
                let padding = if last {
                    let rem = w::i64_remainder(work, d, 4)?;
                    w::compare(work, rem, 0i64)? == Ordering::Equal
                } else {
                    false
                };
                ensure(padding, "BASE64", "canonical pad bits")?;
            } else {
                let e = sextet(c[3], work)?;
                let first = w::i64_remainder(work, d, 4)?;
                let first = w::i64_operation(work, Op::Multiply, first, 64)?;
                let byte = w::i64_operation(work, Op::Add, first, e)?;
                out.push(byte as u8, work)?;
            }
        }
    }
    ensure(
        w::compare(work, out.len(), max)? != Ordering::Greater,
        "BASE64",
        "decoded bound",
    )
}
/// Separate exact inventory digest, useful for adapter CAS assertions.
pub fn object_digest(object: &RetainedObject) -> Result<Digest> {
    Ok(raw_sha256(&super::canonical_bytes(object, SEGMENT_BYTES)?))
}

#[cfg(test)]
mod sink_tests {
    use super::*;
    use crate::validation_work::{tests::CountingWork, ValidationWork};

    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        deny: bool,
    }
    impl Base64Sink for Sink {
        fn push(&mut self, byte: u8, work: &mut dyn ValidationWork) -> Result<()> {
            if self.deny {
                return Err(Error::new("SINK_DENIED", "caller output refusal"));
            }
            work.write_bytes(1)?;
            self.bytes.push(byte);
            Ok(())
        }
        fn len(&self) -> usize {
            self.bytes.len()
        }
    }
    #[test]
    fn strict_base64_sink_matches_literal_bytes_and_legacy_refusals() {
        for (encoded, expected) in [
            ("", b"".as_slice()),
            ("Zg==", b"f"),
            ("Zm8=", b"fo"),
            ("Zm9v", b"foo"),
            ("AA==", b"\0"),
            ("+/8=", b"\xfb\xff"),
        ] {
            assert_eq!(decode_base64(encoded, 16).unwrap(), expected);
            let mut sink = Sink::default();
            decode_base64_into(encoded, 16, &mut sink, &mut CountingWork::new(usize::MAX)).unwrap();
            assert_eq!(sink.bytes, expected);
        }
        for encoded in [
            "Zh==", "Zm9=", "Zg=", "Zg===", "=AAA", "Zg==AAAA", "Zg\n=", "????",
        ] {
            let expected = decode_base64(encoded, 16).unwrap_err();
            let mut sink = Sink::default();
            assert_eq!(
                decode_base64_into(encoded, 16, &mut sink, &mut CountingWork::new(usize::MAX))
                    .unwrap_err(),
                expected
            );
        }
        let expected = decode_base64("Zm9v", 2).unwrap_err();
        assert_eq!(
            decode_base64_into(
                "Zm9v",
                2,
                &mut Sink::default(),
                &mut CountingWork::new(usize::MAX)
            )
            .unwrap_err(),
            expected
        );
    }
    #[test]
    fn strict_base64_sink_propagates_exact_work_and_output_refusal() {
        let mut work = CountingWork::new(usize::MAX);
        let mut sink = Sink::default();
        decode_base64_into("AAECA/8=", 5, &mut sink, &mut work).unwrap();
        assert_eq!(sink.bytes, [0, 1, 2, 3, 255]);
        assert_eq!(work.written, 5);
        let n = work.used;
        decode_base64_into(
            "AAECA/8=",
            5,
            &mut Sink::default(),
            &mut CountingWork::new(n),
        )
        .unwrap();
        assert_eq!(
            decode_base64_into(
                "AAECA/8=",
                5,
                &mut Sink::default(),
                &mut CountingWork::new(n - 1)
            )
            .unwrap_err()
            .code,
            "TEST_EXHAUSTED"
        );
        let mut denied = Sink {
            deny: true,
            ..Sink::default()
        };
        assert_eq!(
            decode_base64_into("Zg==", 1, &mut denied, &mut CountingWork::new(usize::MAX))
                .unwrap_err(),
            Error::new("SINK_DENIED", "caller output refusal")
        );
        assert!(denied.bytes.is_empty());
    }
}
