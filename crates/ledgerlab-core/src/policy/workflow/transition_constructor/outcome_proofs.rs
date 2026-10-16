//! Outcome field proof DATA relationships. Actual original introduction,
//! nominated-source authentication and retained custody are host obligations.
use super::{CoreMeter, body, codec, data::*, meter, records::Records, shape};
use crate::{
    Error, Result, canonical,
    domain::Timestamp,
    policy::workflow::{FactSchema, NormalizedFacts},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
macro_rules! call {
    ($f:path,$m:expr,$a:expr $(,)?) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr $(,)?) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path,$m:expr,$a:expr,$b:expr,$c:expr $(,)?) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    if shape::same(m, a, b)? {
        Ok(())
    } else {
        Err(shape::fail("outcome proof relationship"))
    }
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, Value)> {
    let reference = shape::reference(m, v)?;
    let body = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, body))
}
fn time(m: &mut CoreMeter, v: &Value, k: &str) -> Result<i64> {
    let s = shape::text(m, v, k)?;
    Ok(Timestamp::parse_with_work(s, m)?.micros())
}
pub(super) struct Proofs {
    pub references: Vec<ExactRef>,
    pub introduced: Vec<DraftEnvelope>,
}
pub(super) fn resolve(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &Value,
    schema: &FactSchema,
    normalized: &NormalizedFacts,
    state: &DraftStateData,
) -> Result<Proofs> {
    if !meter::cmp(m, &c.authorities.len().into(), &1.into())?.is_eq() {
        return Err(shape::fail("selected outcome source frame"));
    }
    let authority = &c.authorities[0];
    let expected_subject = json!({"kind":"outcome","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"target_ref":p["target_ref"],"family_slot":p["family_slot"],"business_outcome_token":p["business_outcome_token"],"activity":p["activity"],"binding_ref":p["binding_ref"]});
    let required = normalized.required_proof_fields();
    let proofs = shape::array(m, p, "fact_proofs")?;
    if !meter::cmp(m, &proofs.len().into(), &required.len().into())?.is_eq() {
        return Err(shape::fail("one proof per attested field"));
    }
    let mut used = Vec::<String>::new();
    let mut used_verifications = vec![];
    let mut out = Proofs {
        references: vec![],
        introduced: vec![],
    };
    for proof in proofs {
        m.visit("outcome.proof")?;
        shape::closed(m, proof, &["field", "attestation_ref"], &[])?;
        let name = shape::text(m, proof, "field")?;
        let mut present = false;
        for want in required {
            if shape::same_text(m, name, want)? {
                present = true;
                break;
            }
        }
        if !present {
            return Err(shape::fail("unmatched outcome field proof"));
        }
        for prior in &used {
            if shape::same_text(m, name, prior)? {
                return Err(shape::fail("duplicate outcome field proof"));
            }
        }
        m.write_bytes(name.len())?;
        used.push(name.to_owned());
        let definition = call!(
            shape::named,
            m,
            call!(shape::array, m, schema.canonical_value(), "fields")?,
            "name",
            name
        )?;
        let provenance = shape::field(m, definition, "provenance")?;
        call!(
            equal,
            m,
            call!(shape::field, m, provenance, "kind")?,
            &json!("attested")
        )?;
        call!(
            equal,
            m,
            call!(shape::field, m, provenance, "source_slot")?,
            &json!(authority.source_slot)
        )?;
        let attestation_ref = shape::field(m, proof, "attestation_ref")?;
        let (att_ref, att) = load(
            m,
            r,
            c,
            attestation_ref,
            "workflow-fact-attestation",
            "ledger-workflow-fact-attestation/1",
        )?;
        shape::closed(
            m,
            &att,
            &[
                "schema",
                "attestation_id",
                "source",
                "evidence_kind",
                "subject",
                "subject_schema_ref",
                "claims",
                "issued_at",
                "supporting_evidence_refs",
            ],
            &[],
        )?;
        let identifier = shape::text(m, &att, "attestation_id")?;
        if !meter::cmp(m, &identifier.len().into(), &0.into())?.is_gt()
            || meter::cmp(m, &identifier.len().into(), &128.into())?.is_gt()
        {
            return Err(shape::fail("attestation identifier bound"));
        }
        for ch in identifier.chars() {
            m.scan_bytes(ch.len_utf8())?;
            if m.check("outcome.attestation_identifier", || ch.is_control())? {
                return Err(shape::fail("attestation identifier control"));
            }
        }
        call!(
            equal,
            m,
            call!(shape::field, m, &att, "subject")?,
            &expected_subject
        )?;
        call!(
            equal,
            m,
            call!(shape::field, m, &att, "subject_schema_ref")?,
            call!(shape::field, m, p, "fact_schema_ref")?
        )?;
        call!(
            equal,
            m,
            call!(shape::field, m, &att, "source")?,
            &json!(c.key.source)
        )?;
        call!(
            equal,
            m,
            call!(shape::field, m, &att, "evidence_kind")?,
            call!(shape::field, m, provenance, "evidence_kind")?
        )?;
        let supporting = shape::array(m, &att, "supporting_evidence_refs")?;
        if !meter::cmp(m, &supporting.len().into(), &0.into())?.is_eq() {
            return Err(Error::new(
                "UNSUPPORTED_COMPOSITION",
                "selected supporting fact evidence",
            ));
        }
        let claims = shape::array(m, &att, "claims")?;
        if !meter::cmp(m, &claims.len().into(), &0.into())?.is_gt()
            || meter::cmp(m, &claims.len().into(), &64.into())?.is_gt()
        {
            return Err(shape::fail("attestation claim count"));
        }
        let mut values = p["facts"]
            .as_object()
            .ok_or_else(|| shape::fail("normalized outcome facts"))?
            .clone();
        let mut seen = Vec::<String>::new();
        let mut selected = None;
        for claim in claims {
            m.visit("outcome.attested_claim")?;
            shape::closed(m, claim, &["field", "value"], &[])?;
            let field = shape::text(m, claim, "field")?;
            for prior in &seen {
                if shape::same_text(m, field, prior)? {
                    return Err(shape::fail("duplicate attested field"));
                }
            }
            m.write_bytes(field.len())?;
            seen.push(field.to_owned());
            let value = shape::field(m, claim, "value")?;
            values.insert(field.to_owned(), value.clone());
            if shape::same_text(m, field, name)? {
                selected = Some(value);
            }
        }
        // Validate every declared claim under the original schema, even when
        // this operation consumes only one of its fields. No current schema.
        let normalized_claims = schema.normalize_value_with_work(&Value::Object(values), m)?;
        for claim in claims {
            let field = shape::text(m, claim, "field")?;
            let normalized = shape::field(m, normalized_claims.canonical_value(), field)?;
            call!(
                equal,
                m,
                call!(shape::field, m, claim, "value")?,
                normalized
            )?;
        }
        let selected =
            selected.ok_or_else(|| shape::fail("attestation does not prove selected field"))?;
        call!(
            equal,
            m,
            selected,
            call!(shape::field, m, normalized.canonical_value(), name)?
        )?;
        let mut verification = None;
        for candidate in &state.evidence_verifications {
            m.visit("outcome.verification_locator")?;
            let v = r.get(
                m,
                &c.key,
                candidate,
                "workflow-evidence-verification",
                "ledger-workflow-evidence-verification/1",
            )?;
            if call!(
                shape::same,
                m,
                call!(shape::field, m, &v, "attestation_ref")?,
                &json!(att_ref)
            )? {
                m.visit("outcome.unique_verification")?;
                if verification.is_some() {
                    return Err(shape::fail("ambiguous attestation verification"));
                }
                verification = Some((candidate.clone(), v));
            }
        }
        let (verification_ref, verification) =
            verification.ok_or_else(|| shape::fail("retained attestation verification missing"))?;
        used_verifications.push(verification_ref.clone());
        shape::closed(
            m,
            &verification,
            &[
                "schema",
                "attestation_ref",
                "attestation_raw_sha256",
                "attestation_digest",
                "verified_source",
                "verification_primitive",
                "source_grant_ref",
                "source_grant_revision",
                "authenticated_principal_ref",
                "trusted_received_at",
                "subject_binding_refs",
                "verification_evidence_refs",
            ],
            &[],
        )?;
        for (key, want) in [
            ("attestation_ref", json!(att_ref)),
            ("attestation_digest", json!(att_ref.digest)),
            ("verified_source", json!(c.key.source)),
            (
                "verification_primitive",
                json!("authenticated-nominated-source-retention/1"),
            ),
            ("source_grant_ref", json!(authority.current_grant_ref)),
            (
                "authenticated_principal_ref",
                json!(authority.enrollment_ref),
            ),
            ("subject_binding_refs", json!([p["binding_ref"].clone()])),
            ("verification_evidence_refs", json!([])),
        ] {
            call!(equal, m, call!(shape::field, m, &verification, key)?, &want)?;
        }
        let raw = r.canonical(m, &att_ref)?;
        m.hash_bytes(raw.len())?;
        let actual = canonical::hex_with_work(&Sha256::digest(raw), m)?;
        call!(
            equal,
            m,
            call!(shape::field, m, &verification, "attestation_raw_sha256")?,
            &json!(actual)
        )?;
        let grant = r.get(
            m,
            &c.key,
            &authority.current_grant_ref,
            "workflow-source-grant",
            "ledger-workflow-source-grant/1",
        )?;
        call!(
            equal,
            m,
            call!(shape::field, m, &verification, "source_grant_revision")?,
            call!(shape::field, m, &grant, "revision")?
        )?;
        let issued = time(m, &att, "issued_at")?;
        let trusted = time(m, &verification, "trusted_received_at")?;
        let received = Timestamp::parse_with_work(&c.received_at, m)?.micros();
        let starts = time(m, &grant, "starts_at")?;
        let ends = time(m, &grant, "ends_at")?;
        if meter::cmp(m, &issued.into(), &trusted.into())?.is_gt()
            || meter::cmp(m, &trusted.into(), &received.into())?.is_gt()
            || meter::cmp(m, &trusted.into(), &starts.into())?.is_lt()
            || !meter::cmp(m, &trusted.into(), &ends.into())?.is_lt()
        {
            return Err(shape::fail("attestation historical time relationship"));
        }
        let value = normalized
            .canonical_value()
            .get(name)
            .ok_or_else(|| shape::fail("normalized proof field"))?;
        let b = json!({"schema":"ledger-workflow-verified-fact/1","schema_ref":p["fact_schema_ref"],"field":name,"value":value,"evidence_ref":att_ref,"evidence_digest":att_ref.digest,"source":att["source"],"verification_ref":verification_ref,"scope":c.key.scope,"binding_or_invocation_ref":p["binding_ref"],"subject":expected_subject});
        // Compute immutable identity before deciding whether it is genuinely
        // new. A reused occurrence proof must not be returned as a new result.
        let bytes = codec::encode(m, &b, 4194304)?;
        let schema_bytes = codec::encode(m, &json!("ledger-workflow-verified-fact/1"), 262144)?;
        let digest =
            super::records::hash_parts(m, "workflow-document", &[&schema_bytes, &bytes], true)?;
        let reference = ExactRef {
            kind: "workflow-verified-fact".into(),
            id: format!("wdoc_{}", &digest[7..]),
            digest,
        };
        if let Some(old) = r.optional(
            m,
            &c.key,
            &reference,
            "workflow-verified-fact",
            "ledger-workflow-verified-fact/1",
        )? {
            equal(m, &old, &b)?;
        } else {
            m.new_body(bytes.len())?;
            out.introduced.push(DraftEnvelope {
                reference: reference.clone(),
                canonical_body: bytes,
            });
        }
        out.references.push(reference);
        if call!(
            shape::same,
            m,
            call!(shape::field, m, provenance, "evidence_kind")?,
            &json!("occurrence")
        )? {
            call!(equal, m, value, call!(shape::field, m, p, "occurred_at")?)?;
        }
    }
    let used_verifications = body::ref_set(m, used_verifications)?;
    let declared_verifications = body::ref_set(m, state.evidence_verifications.clone())?;
    if !shape::same(
        m,
        &json!(used_verifications),
        &json!(declared_verifications),
    )? {
        return Err(Error::new(
            "UNSUPPORTED_COMPOSITION",
            "unmatched verification declaration",
        ));
    }
    out.references = body::ref_set(m, out.references)?;
    Ok(out)
}
