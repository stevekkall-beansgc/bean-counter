//! Helper-level historical compatibility only, not a complete original audit.
use super::{body, data::*, meter, shape, CoreMeter};
use crate::{canonical, Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
macro_rules! call {
    ($f:path,$m:expr,$a:expr) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
fn fail(s: &str) -> Error {
    shape::fail(s)
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if shape::same(m, a, b)? {
        Ok(())
    } else {
        Err(fail("original financial DATA mismatch"))
    }
}
// Exact ordinary saved tuples. Both registries explicitly carry the listed
// economic3 entries. Causal4 is NOT accepted merely because its registry is known.
fn ordinary_tuple(m: &mut CoreMeter, kind: &str, payload: &str, registry: &str) -> Result<()> {
    const ROWS: &[(&str, &str, &str)] = &[
        (
            "work.accept",
            "ledger-workflow-work/1",
            "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "sha256:5d9c1c2b3719978693927dd72264aa3c402daaf62800d9e11bb49b0d0868ec1e",
        ),
        (
            "work.accept",
            "ledger-workflow-work/1",
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
        ),
        (
            "outcome.submit",
            "ledger-workflow-outcome/1",
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54",
        ),
    ];
    for (k, p, r) in ROWS {
        m.visit("original.saved_recipe")?;
        if shape::same_text(m, kind, k)?
            && shape::same_text(m, payload, p)?
            && shape::same_text(m, registry, r)?
        {
            return Ok(());
        }
    }
    Err(Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "original financial tuple not implemented",
    ))
}
/// Compare canonical padded base64 against raw bytes without allocating a
/// second encoded request. Every executed numeric operation is paid before use.
fn raw_evidence(m: &mut CoreMeter, e: &Value, raw: &[u8]) -> Result<()> {
    use crate::validation_work::{self as w, ArithmeticOperation as Op};
    shape::closed(
        m,
        e,
        &[
            "schema",
            "normalization_profile",
            "original_request_bytes_base64",
            "original_request_bytes_sha256",
            "normalized_request",
            "request_hash",
            "schema_bundle_digest",
            "descriptor_resolution",
        ],
        &[],
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, e, "schema")?,
        &json!("ledger-operation-request-evidence/2")
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, e, "normalization_profile")?,
        &json!("ledger-operation-normalization/2")
    )?;
    if !meter::cmp(m, &raw.len().into(), &0.into())?.is_gt()
        || meter::cmp(m, &raw.len().into(), &262144.into())?.is_gt()
    {
        return Err(fail("original request byte ceiling"));
    }
    let encoded = shape::text(m, e, "original_request_bytes_base64")?.as_bytes();
    let rounded = w::usize_add(m, raw.len(), 2)?;
    let groups = w::i64_operation(m, Op::DivideRemainder, rounded as i64, 3)?;
    let length = w::i64_operation(m, Op::Multiply, groups, 4)?;
    if !meter::cmp(m, &encoded.len().into(), &length.into())?.is_eq() {
        return Err(fail("original request base64 length"));
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (index, chunk) in raw.chunks(3).enumerate() {
        m.visit("original.base64_group")?;
        m.scan_bytes(chunk.len())?;
        let mut value = 0i64;
        for i in 0..3 {
            let byte = if meter::cmp(m, &i.into(), &chunk.len().into())?.is_lt() {
                chunk[i] as i64
            } else {
                0
            };
            let shifted = w::i64_operation(m, Op::Multiply, value, 256)?;
            value = w::i64_operation(m, Op::Add, shifted, byte)?;
        }
        let mut out = [b'='; 4];
        let mut denominator = 262144i64;
        for (i, digit) in out.iter_mut().enumerate() {
            let count = w::usize_add(m, chunk.len(), 1)?;
            if meter::cmp(m, &i.into(), &count.into())?.is_lt() {
                let q = w::i64_operation(m, Op::DivideRemainder, value, denominator)?;
                let rem = w::i64_remainder(m, q, 64)?;
                *digit = alphabet[rem as usize];
            }
            denominator = w::i64_operation(m, Op::DivideRemainder, denominator, 64)?;
        }
        let start = w::i64_operation(m, Op::Multiply, index as i64, 4)? as usize;
        let end = w::usize_add(m, start, 4)?;
        if !shape::same_bytes(m, &out, &encoded[start..end])? {
            return Err(fail("original request base64 identity"));
        }
    }
    m.hash_bytes(raw.len())?;
    let digest = canonical::hex_with_work(&Sha256::digest(raw), m)?;
    if !call!(
        shape::same_text,
        m,
        &digest,
        call!(shape::text, m, e, "original_request_bytes_sha256")?
    )? {
        return Err(fail("original raw request SHA256"));
    }
    Ok(())
}
fn partition_inputs(
    m: &mut CoreMeter,
    pool: &[RecordInput],
    preexisting: &[ExactRef],
    pending: &[RecordInput],
) -> Result<Vec<RecordInput>> {
    let mut out = vec![];
    for reference in preexisting {
        for introduced in pending {
            if shape::same_ref(m, reference, &introduced.reference)? {
                return Err(fail("pending representation declared preexisting"));
            }
        }
        let mut found = None;
        for input in pool {
            m.visit("original.preexisting_locator")?;
            if shape::same_text(m, &input.reference.kind, &reference.kind)?
                && shape::same_text(m, &input.reference.id, &reference.id)?
            {
                if !shape::same_ref(m, &input.reference, reference)? || found.is_some() {
                    return Err(fail("conflicting original input identity"));
                }
                found = Some(input);
            }
        }
        let input = found.ok_or_else(|| fail("missing exact pre-operation input"))?;
        m.read_bytes(input.canonical_body.len())?;
        m.write_bytes(input.canonical_body.len())?;
        out.push(input.clone());
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn introduced_verified_fact_is_comparison_only_but_reused_fact_is_input() {
        let mut m = CoreMeter::new(65536).unwrap();
        let value = body::document(
            &mut m,
            "workflow-verified-fact",
            json!({"schema":"ledger-workflow-verified-fact/1","synthetic":"partition-only"}),
        )
        .unwrap();
        let row = RecordInput {
            reference: value.reference.clone(),
            canonical_body: value.canonical_body.into_legacy().unwrap(),
        };
        let pool = vec![row.clone()];
        assert!(partition_inputs(&mut m, &pool, &[], &[])
            .unwrap()
            .is_empty());
        let prior =
            partition_inputs(&mut m, &pool, std::slice::from_ref(&row.reference), &[]).unwrap();
        assert_eq!(prior.len(), 1);
        assert_eq!(prior[0].canonical_body, row.canonical_body);
        assert!(partition_inputs(
            &mut m,
            &pool,
            std::slice::from_ref(&row.reference),
            std::slice::from_ref(&row)
        )
        .is_err());
        let mut changed = row.reference.clone();
        changed.digest = format!("sha256:{}", "0".repeat(64));
        assert!(partition_inputs(&mut m, &pool, &[changed], &[]).is_err());
        assert!(partition_inputs(&mut m, &[], &[row.reference], &[]).is_err());
    }
    #[test]
    fn ordinary_saved_tuple_never_widens_to_causal_or_cross_kind() {
        let mut m = CoreMeter::new(65536).unwrap();
        let r = "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54";
        assert!(ordinary_tuple(&mut m, "work.accept", "ledger-workflow-work/1", r).is_ok());
        assert!(ordinary_tuple(&mut m, "outcome.submit", "ledger-workflow-outcome/1", r).is_ok());
        assert!(ordinary_tuple(&mut m, "work.reverse", "ledger-workflow-work/1", r).is_err());
        assert!(ordinary_tuple(&mut m, "work.accept", "ledger-workflow-outcome/1", r).is_err());
        assert!(ordinary_tuple(
            &mut m,
            "quantity.correct",
            "ledger-workflow-quantity-correction/2",
            r
        )
        .is_err());
    }
    #[test]
    fn exact_raw_evidence_rejects_changed_bytes_padding_hash_and_fields() {
        for (raw, encoded, sha) in [
            (
                b"f".as_slice(),
                "Zg==",
                "252f10c83610ebca1a059c0bae8255eba2f95be4d1d7bcfa89d7248a82d9f111",
            ),
            (
                b"fo".as_slice(),
                "Zm8=",
                "9c3aee7110b787f0fb5f81633a36392bd277ea945d44c874a9a23601aefe20cf",
            ),
            (
                b"foo".as_slice(),
                "Zm9v",
                "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae",
            ),
        ] {
            let e = json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":encoded,"original_request_bytes_sha256":sha,"normalized_request":{},"request_hash":"unconsumed-here","schema_bundle_digest":"unconsumed-here","descriptor_resolution":{}});
            let mut m = CoreMeter::new(65536).unwrap();
            assert!(raw_evidence(&mut m, &e, raw).is_ok());
            let mut wrong = e.clone();
            wrong["original_request_bytes_base64"] = json!("AAAA");
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
            let mut wrong = e.clone();
            wrong["original_request_bytes_sha256"] = json!("0".repeat(64));
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
            let mut wrong = e.clone();
            wrong["unexpected"] = json!(true);
            assert!(raw_evidence(&mut m, &wrong, raw).is_err());
        }
    }
}
