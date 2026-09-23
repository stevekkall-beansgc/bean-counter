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
    fn sextet(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    ensure(
        text.len().is_multiple_of(4) && text.len() <= max.div_ceil(3) * 4,
        "BASE64",
        "encoded length",
    )?;
    let mut out = Vec::with_capacity((text.len() / 4 * 3).min(max));
    for (i, c) in text.as_bytes().as_chunks::<4>().0.iter().enumerate() {
        let bad = || Error::new("BASE64", "alphabet or padding");
        let a = sextet(c[0]).ok_or_else(bad)?;
        let b = sextet(c[1]).ok_or_else(bad)?;
        out.push((a << 2) | (b >> 4));
        let last = (i + 1) * 4 == text.len();
        if c[2] == b'=' {
            ensure(
                last && c[3] == b'=' && b & 15 == 0,
                "BASE64",
                "canonical pad bits",
            )?;
        } else {
            let d = sextet(c[2]).ok_or_else(bad)?;
            out.push((b << 4) | (d >> 2));
            if c[3] == b'=' {
                ensure(last && d & 3 == 0, "BASE64", "canonical pad bits")?;
            } else {
                let e = sextet(c[3]).ok_or_else(bad)?;
                out.push((d << 6) | e);
            }
        }
    }
    ensure(out.len() <= max, "BASE64", "decoded bound")?;
    Ok(out)
}
/// Separate exact inventory digest, useful for adapter CAS assertions.
pub fn object_digest(object: &RetainedObject) -> Result<Digest> {
    Ok(raw_sha256(&super::canonical_bytes(object, SEGMENT_BYTES)?))
}
