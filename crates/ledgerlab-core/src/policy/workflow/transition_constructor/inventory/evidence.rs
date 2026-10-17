use super::super::emitted_record::ConstructedEnvelope;
use super::super::record_body::RecordBody;
use super::super::{
    CoreMeter,
    data::*,
    meter,
    records::{self, Records},
    representation::InvocationToken,
    shape,
};
use super::check::*;
use crate::{Result, canonical};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
/// Canonical padded base64 with pre-allocation decoded-byte ceiling. All grammar
/// checks execute through the meter; this is retained DATA, never a signature.
pub(in crate::policy::workflow::transition_constructor) fn decode(
    m: &mut CoreMeter,
    s: &str,
) -> Result<Vec<u8>> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::evidence::decode", |m| decode_data(m, s));
    }
    #[cfg(not(test))]
    decode_data(m, s)
}
pub(in crate::policy::workflow::transition_constructor) fn decode_data(
    m: &mut dyn super::super::intrinsic_record::IntrinsicValidationWork,
    s: &str,
) -> Result<Vec<u8>> {
    use crate::validation_work::{self as w, ArithmeticOperation as Op};
    use std::cmp::Ordering;
    if w::compare(m, s.len(), 0usize)? != Ordering::Greater
        || w::compare(m, s.len(), 5464usize)? == Ordering::Greater
        || invoke!(w::compare, m, w::i64_remainder(m, s.len() as i64, 4)?, 0i64)? != Ordering::Equal
    {
        return Err(shape::fail("evidence base64 bound"));
    }
    let groups = w::i64_operation(m, Op::DivideRemainder, s.len() as i64, 4)?;
    let mut out = Vec::new();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (index, chunk) in s.as_bytes().chunks_exact(4).enumerate() {
        m.visit_named("evidence.base64_group")?;
        let mut digits = [0i64; 4];
        let mut pad = 0usize;
        for (i, b) in chunk.iter().enumerate() {
            m.scan_bytes(1)?;
            m.visit_named("evidence.padding")?;
            if *b == b'=' {
                pad = w::usize_add(m, pad, 1)?;
                let pos = w::usize_add(m, index, 1)?;
                if w::compare(m, i, 2usize)? == Ordering::Less
                    || w::compare(m, pos, groups)? != Ordering::Equal
                {
                    return Err(shape::fail("base64 padding"));
                }
            } else {
                if w::compare(m, pad, 0usize)? != Ordering::Equal {
                    return Err(shape::fail("base64 after padding"));
                }
                let mut found = None;
                for (j, a) in alphabet.iter().enumerate() {
                    m.scan_bytes(1)?;
                    m.visit_named("evidence.base64_digit")?;
                    if a == b {
                        found = Some(j as i64);
                        break;
                    }
                }
                digits[i] = found.ok_or_else(|| shape::fail("base64 digit"))?;
            }
        }
        if w::compare(m, pad, 2usize)? == Ordering::Greater {
            return Err(shape::fail("base64 padding"));
        }
        if w::compare(m, pad, 2usize)? == Ordering::Equal
            && invoke!(w::compare, m, w::i64_remainder(m, digits[1], 16)?, 0i64)? != Ordering::Equal
        {
            return Err(shape::fail("base64 unused bits"));
        }
        if w::compare(m, pad, 1usize)? == Ordering::Equal
            && invoke!(w::compare, m, w::i64_remainder(m, digits[2], 4)?, 0i64)? != Ordering::Equal
        {
            return Err(shape::fail("base64 unused bits"));
        }
        let count = w::usize_sub(m, 3, pad)?;
        let next = w::usize_add(m, out.len(), count)?;
        if w::compare(m, next, 4096usize)? == Ordering::Greater {
            return Err(shape::fail("evidence content bound"));
        }
        let mut value = 0i64;
        for digit in digits {
            let product = w::i64_operation(m, Op::Multiply, value, 64)?;
            value = w::i64_operation(m, Op::Add, product, digit)?;
        }
        let a = w::i64_operation(m, Op::DivideRemainder, value, 65536)?;
        let remainder = w::i64_remainder(m, value, 65536)?;
        let b = w::i64_operation(m, Op::DivideRemainder, remainder, 256)?;
        let d = w::i64_remainder(m, remainder, 256)?;
        m.write_bytes(count)?;
        out.extend_from_slice(&[a as u8, b as u8, d as u8][..count]);
    }
    Ok(out)
}
pub(in crate::policy::workflow::transition_constructor) fn root(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    reference: &Value,
    before_doc: &Value,
    before_key: &str,
    expected_actor: &Value,
    expected_source: Option<&Value>,
) -> Result<RecordBody> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::evidence::root", |m| {
            root_data(
                m,
                r,
                c,
                o,
                reference,
                before_doc,
                before_key,
                expected_actor,
                expected_source,
            )
        });
    }
    #[cfg(not(test))]
    root_data(
        m,
        r,
        c,
        o,
        reference,
        before_doc,
        before_key,
        expected_actor,
        expected_source,
    )
}
fn root_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    reference: &Value,
    before_doc: &Value,
    before_key: &str,
    expected_actor: &Value,
    expected_source: Option<&Value>,
) -> Result<RecordBody> {
    let root = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        reference,
        "workflow-authority",
        "ledger-workflow-local-authority-snapshot/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &root,
        &[
            "schema",
            "basis",
            "basis_digest",
            "verification_profile",
            "verified_at"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        &root,
        "verification_profile",
        &json!("local-private-directory-owner/1")
    )?;
    let basis = invoke!(shape::field, m, &root, "basis")?;
    invoke!(
        shape::closed,
        m,
        basis,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "rights",
            "source_grant_revision"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        basis,
        "schema",
        &json!("ledger-workflow-local-authority-basis/1")
    )?;
    for (k, w) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("principal_id", expected_actor.clone()),
        ("rights", json!(["admin", "read"])),
    ] {
        invoke!(eq, m, basis, k, &w)?;
    }
    invoke!(opaque, m, basis, "source", 256)?;
    invoke!(opaque, m, basis, "canonical_economic_namespace", 256)?;
    if let Some(source) = expected_source {
        invoke!(eq, m, basis, "source", source)?;
    }
    invoke!(number, m, basis, "source_grant_revision", true)?;
    let digest = invoke!(records::hash, m, "local-authority-basis", basis)?;
    invoke!(eq, m, &root, "basis_digest", &json!(digest))?;
    invoke!(before, m, &root, "verified_at", before_doc, before_key)?;
    Ok(root)
}
pub(in crate::policy::workflow::transition_constructor) fn retained(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    e: &Value,
    purpose: &str,
    subject: &Value,
    metadata: &Value,
    before_doc: &Value,
    before_key: &str,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::evidence::retained", |m| {
            retained_data(
                m, r, c, o, e, purpose, subject, metadata, before_doc, before_key,
            )
        });
    }
    #[cfg(not(test))]
    retained_data(
        m, r, c, o, e, purpose, subject, metadata, before_doc, before_key,
    )
}
fn retained_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    e: &Value,
    purpose: &str,
    subject: &Value,
    metadata: &Value,
    before_doc: &Value,
    before_key: &str,
) -> Result<()> {
    // Reuse only the factory-established immutable properties. All subject,
    // terms, mode, time, actor and root associations remain paid at this use.
    if let Some(facts) = r.evidence_facts(m, e)? {
        let actual = facts.retained_at().micros();
        invoke!(eq, m, e, "purpose", &json!(purpose))?;
        invoke!(eq, m, e, "subject", subject)?;
        invoke!(eq, m, e, "metadata", metadata)?;
        invoke!(eq, m, e, "installation_mode", &json!(c.installation_mode))?;
        let limit = invoke!(time, m, before_doc, before_key)?;
        if meter::cmp(m, &actual.into(), &limit)?.is_gt() {
            return Err(shape::fail("captured time order"));
        }
        invoke!(
            root,
            m,
            &mut *r,
            c,
            o,
            invoke!(shape::field, m, e, "authority_ref")?,
            e,
            "retained_at",
            data_field!(m, e, "retained_by"),
            None
        )?;
        return Ok(());
    }
    invoke!(
        shape::closed,
        m,
        e,
        &[
            "schema",
            "authority_ref",
            "content_base64",
            "content_sha256",
            "installation_mode",
            "media_type",
            "metadata",
            "purpose",
            "retained_at",
            "retained_by",
            "subject"
        ],
        &[]
    )?;
    invoke!(eq, m, e, "purpose", &json!(purpose))?;
    invoke!(eq, m, e, "subject", subject)?;
    invoke!(eq, m, e, "metadata", metadata)?;
    invoke!(eq, m, e, "installation_mode", &json!(c.installation_mode))?;
    invoke!(name, m, e, "retained_by")?;
    let media = invoke!(shape::text, m, e, "media_type")?;
    let text = invoke!(shape::same_text, m, media, "text/plain;charset=utf-8")?;
    if !text && !invoke!(shape::same_text, m, media, "application/octet-stream")? {
        return Err(shape::fail("evidence media type"));
    }
    if let Some(arena)=m.representation_arena(){
        use crate::validation_work::RepresentationKind;
        let encoded=shape::text(m,e,"content_base64")?;
        let bytes=m.decode_owned_base64_representation(&arena,encoded,RepresentationKind::EvidenceContent)?;
        if text{m.scan_bytes(bytes.bytes().len())?;m.visit("evidence.utf8_constraint")?;std::str::from_utf8(bytes.bytes()).map_err(|_|shape::fail("evidence text encoding"))?;}
        let digest=m.hash_owned_representation(&arena,bytes.bytes(),RepresentationKind::EvidenceContent)?;
        let _scratch=arena.reserve_external_payload(64,m)?;
        let hash=canonical::hex_with_work(&digest,m)?;
        let expected=shape::text(m,e,"content_sha256")?;
        if !shape::same_text(m,&hash,expected)?{return Err(shape::fail("retained evidence raw content digest"));}
    }else{
        let bytes = invoke!(decode, m, invoke!(shape::text, m, e, "content_base64")?)?;
        if text {
            m.scan_bytes(bytes.len())?;
            m.visit("evidence.utf8_constraint")?;
            std::str::from_utf8(&bytes).map_err(|_| shape::fail("evidence text encoding"))?;
        }
        m.hash_bytes(bytes.len())?;
        let hash = canonical::hex_with_work(&Sha256::digest(&bytes), m)?;
        invoke!(eq, m, e, "content_sha256", &json!(hash))?;
    }
    invoke!(before, m, e, "retained_at", before_doc, before_key)?;
    invoke!(
        root,
        m,
        &mut *r,
        c,
        o,
        invoke!(shape::field, m, e, "authority_ref")?,
        e,
        "retained_at",
        data_field!(m, e, "retained_by"),
        None
    )?;
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn assent(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    a: &Value,
    e: &Value,
    purpose: &str,
    registered: &Value,
    binding: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::evidence::assent", |m| {
            assent_data(m, r, c, o, a, e, purpose, registered, binding)
        });
    }
    #[cfg(not(test))]
    assent_data(m, r, c, o, a, e, purpose, registered, binding)
}
fn assent_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    a: &Value,
    e: &Value,
    purpose: &str,
    registered: &Value,
    binding: &Value,
) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        a,
        &[
            "schema",
            "scope",
            "agreement_id",
            "customer",
            "terms_digest",
            "mode",
            "acceptor",
            "accepted_at",
            "evidence_ref",
            "operator",
            "operator_attestation_ref"
        ],
        &[]
    )?;
    invoke!(copy, m, a, "acceptor", registered, "acceptor")?;
    invoke!(copy, m, a, "operator", binding, "accepted_by")?;
    invoke!(copy, m, e, "retained_by", a, "operator")?;
    invoke!(before, m, a, "accepted_at", binding, "accepted_at")?;
    let metadata = if purpose == "assent" {
        json!({"accepted_at":data_field!(m,&a,"accepted_at"),"acceptor":data_field!(m,&a,"acceptor"),"terms_digest":data_field!(m,&a,"terms_digest")})
    } else {
        json!({"acceptor":data_field!(m,&a,"acceptor"),"assent_evidence_ref":data_field!(m,&a,"evidence_ref"),"statement":"operator_verified_exact_terms_acceptance","terms_digest":data_field!(m,&a,"terms_digest")})
    };
    invoke!(
        retained,
        m,
        &mut *r,
        c,
        o,
        e,
        purpose,
        &json!({"customer":data_field!(m,&a,"customer"),"agreement_id":data_field!(m,&a,"agreement_id")}),
        &metadata,
        binding,
        "accepted_at"
    )
}
