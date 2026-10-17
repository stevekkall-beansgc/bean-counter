//! Closed financial-observation DATA and exact immediate/history partition.
//! This validates serialized relationships, NOT authenticated prefix custody,
//! pointer completeness, earliest introductions or the claimed audit digest.
use super::{CoreMeter, arithmetic, body, codec, data::*, meter, records::Records, shape};
use crate::{Result, canonical};
use serde_json::{Value, json};
pub(super) mod audit_stream;
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
fn equal(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn counter(m: &mut CoreMeter, v: &Value, k: &str, max: u64) -> Result<()> {
    let raw = shape::text(m, v, k)?;
    if meter::cmp(m, &raw.len().into(), &19.into())?.is_gt() {
        return Err(shape::fail("audit counter grammar"));
    }
    let n = arithmetic::integer(m, raw, false)?;
    if meter::cmp(m, &n, &0.into())?.is_lt() || meter::cmp(m, &n, &max.into())?.is_gt() {
        return Err(shape::fail("audit counter bound"));
    }
    Ok(())
}
fn refs(m: &mut CoreMeter, v: &Value) -> Result<Vec<ExactRef>> {
    let values = v
        .as_array()
        .ok_or_else(|| shape::fail("financial Ref set"))?;
    if meter::cmp(m, &values.len().into(), &4096.into())?.is_gt() {
        return Err(shape::fail("financial Ref set count"));
    }
    let mut out = vec![];
    for item in values {
        out.push(shape::reference(m, item)?);
    }
    let ordered = body::ref_set(m, out)?;
    equal(m, v, &json!(ordered))?; // also refuses duplicate set entries
    Ok(ordered)
}
fn sorted(m: &mut CoreMeter, v: &Value) -> Result<()> {
    let values = v
        .as_array()
        .ok_or_else(|| shape::fail("financial object set"))?;
    if meter::cmp(m, &values.len().into(), &4096.into())?.is_gt() {
        return Err(shape::fail("financial object set bound"));
    }
    let mut previous: Option<Vec<u8>> = None;
    for item in values {
        let bytes = codec::encode(m, item, 4194304)?;
        if let Some(prior) = &previous {
            if shape::byte_order(m, prior, &bytes)? != std::cmp::Ordering::Less {
                return Err(shape::fail("financial object set order/duplicate"));
            }
        }
        previous = Some(bytes);
    }
    Ok(())
}
/// Not serializable and not a host VerifiedFinancialHistory. Every getter is
/// only DATA; its source/header claims still require the real original audit.
pub(super) struct FinancialInputSelection {
    pub reference: ExactRef,
    pub canonical_body: Vec<u8>,
    pub preexisting: Vec<ExactRef>,
    pub history_edges: Vec<Value>,
}
pub(super) fn validate(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    op: &Value,
    state: &DraftStateData,
) -> Result<FinancialInputSelection> {
    let reference = call!(
        shape::reference,
        m,
        call!(shape::field, m, op, "financial_observation_ref")?
    )?;
    let observation = r.get(
        m,
        &c.key,
        &reference,
        "workflow-financial-history-observation",
        "ledger-workflow-financial-history-observation/1",
    )?;
    shape::closed(
        m,
        &observation,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "installation_mode",
            "root_ref",
            "pending_operation_key",
            "snapshot_through_sequence",
            "preexisting_input_refs",
            "history_edges",
            "head_pins",
            "verifier_ref",
            "prefix_digest",
            "origin_count",
            "document_count",
            "audited_retained_bytes",
            "observed_at",
        ],
        &[],
    )?;
    for (k, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("installation_mode", json!(c.installation_mode)),
        ("pending_operation_key", json!(c.key)),
        ("observed_at", json!(c.accepted_at)),
    ] {
        call!(equal, m, call!(shape::field, m, &observation, k)?, &want)?;
    }
    let root = call!(
        shape::reference,
        m,
        call!(shape::field, m, &observation, "root_ref")?
    )?;
    if !shape::same_text(m, &root.kind, "workflow-authority")? {
        return Err(shape::fail("financial root kind"));
    }
    let source = r.get(
        m,
        &c.key,
        &c.authorities[0].authority_observation_ref,
        "workflow-authority-prefix-observation",
        "ledger-workflow-authority-prefix-observation/1",
    )?;
    call!(
        equal,
        m,
        call!(shape::field, m, &source, "root_ref")?,
        &json!(root)
    )?;
    for k in [
        "logical_store_id",
        "scope",
        "installation_mode",
        "observed_at",
    ] {
        call!(
            equal,
            m,
            call!(shape::field, m, &source, k)?,
            call!(shape::field, m, &observation, k)?
        )?;
    }
    counter(
        m,
        &observation,
        "snapshot_through_sequence",
        9_999_999_999_999_999_999,
    )?;
    let through = call!(
        arithmetic::integer,
        m,
        call!(shape::text, m, &observation, "snapshot_through_sequence")?,
        false
    )?;
    let original_sequence = call!(
        arithmetic::integer,
        m,
        call!(shape::text, m, op, "commit_sequence")?,
        false
    )?;
    if !meter::cmp(m, &through, &original_sequence)?.is_lt() {
        return Err(shape::fail("financial snapshot sequence"));
    }
    for (field, maximum) in [
        ("origin_count", 100000),
        ("document_count", 9_999_999_999_999_999_999),
        ("audited_retained_bytes", 268435456),
    ] {
        counter(m, &observation, field, maximum)?;
    }
    let prefix = shape::text(m, &observation, "prefix_digest")?;
    let digest = ExactRef {
        kind: "workflow-financial-history-observation".into(),
        id: reference.id.clone(),
        digest: prefix.into(),
    };
    super::records::reference(m, &digest)?;
    let verifier = call!(
        shape::reference,
        m,
        call!(shape::field, m, &observation, "verifier_ref")?
    )?;
    if !shape::same_text(m, &verifier.kind, "workflow-authority-audit-verifier")? {
        return Err(shape::fail("financial verifier kind"));
    }
    // Recipe/artifact support and cryptographic prefix rederivation deliberately
    // remain the coordinator original audit's obligations, not a caller flag here.
    let preexisting = call!(
        refs,
        m,
        call!(shape::field, m, &observation, "preexisting_input_refs")?
    )?;
    let results = call!(refs, m, call!(shape::field, m, op, "result_refs")?)?;
    for prior in &preexisting {
        for result in &results {
            if shape::same_text(m, &prior.kind, &result.kind)?
                && shape::same_text(m, &prior.id, &result.id)?
            {
                return Err(shape::fail("new result also declared preexisting"));
            }
        }
    }
    let edges = shape::array(m, &observation, "history_edges")?;
    call!(
        sorted,
        m,
        call!(shape::field, m, &observation, "history_edges")?
    )?;
    let mut expected_edges = vec![];
    for prior in &preexisting {
        m.visit("financial.history_owner")?;
        // Only the three exact AUTH03 financial prior fields are immediate cuts.
        let mut expected_schema = None;
        for (kind, s) in [
            ("workflow-decision", "ledger-workflow-decision-revision/1"),
            ("workflow-family-state", "ledger-workflow-family-state/1"),
            ("workflow-chain-state", "ledger-workflow-chain-state/1"),
        ] {
            if shape::same_text(m, &prior.kind, kind)? {
                expected_schema = Some(s);
                break;
            }
        }
        if let Some(s) = expected_schema {
            let owner = r.get(m, &c.key, prior, &prior.kind, s)?;
            m.visit("financial.prior_presence")?;
            if let Some(target) = owner.get("prior_ref") {
                shape::reference(m, target)?;
                expected_edges
                    .push(json!({"owner_ref":prior,"field":"prior_ref","target_ref":target}));
            }
        }
    }
    canonical::sort_set_with_work(&mut expected_edges, m)?;
    for edge in edges {
        shape::closed(m, edge, &["owner_ref", "field", "target_ref"], &[])?;
        let owner = call!(
            shape::reference,
            m,
            call!(shape::field, m, edge, "owner_ref")?
        )?;
        let target = call!(
            shape::reference,
            m,
            call!(shape::field, m, edge, "target_ref")?
        )?;
        let mut financial_owner = false;
        for kind in [
            "workflow-decision",
            "workflow-family-state",
            "workflow-chain-state",
        ] {
            if shape::same_text(m, &owner.kind, kind)? {
                financial_owner = true;
                break;
            }
        }
        if !financial_owner {
            return Err(shape::fail("nonfinancial history cut owner"));
        }
        if !shape::same_text(m, &owner.kind, &target.kind)? {
            return Err(shape::fail("financial cut target kind"));
        }
        call!(
            equal,
            m,
            call!(shape::field, m, edge, "field")?,
            &json!("prior_ref")
        )?;
    }
    call!(
        equal,
        m,
        call!(shape::field, m, &observation, "history_edges")?,
        &json!(expected_edges)
    )?;
    let mut expected_pins = vec![];
    for head in &state.decision_heads {
        expected_pins
            .push(json!({"kind":"decision","ref":head.reference,"revision":head.revision}));
    }
    for head in &state.family_heads {
        expected_pins.push(json!({"kind":"family","ref":head.reference,"revision":head.revision}));
    }
    if let Some(head) = &state.chain_head {
        let chain = r.get(
            m,
            &c.key,
            head,
            "workflow-chain-state",
            "ledger-workflow-chain-state/1",
        )?;
        let revision = shape::field(m, &chain, "revision")?;
        expected_pins.push(json!({"kind":"chain","ref":head,"revision":revision}));
    }
    // Capacity heads are a remaining constructor/state API obligation. This
    // slice cannot claim a complete capacity witness by accepting extra pins.
    canonical::sort_set_with_work(&mut expected_pins, m)?;
    call!(
        equal,
        m,
        call!(shape::field, m, &observation, "head_pins")?,
        &json!(expected_pins)
    )?;
    let pins = shape::array(m, &observation, "head_pins")?;
    call!(
        sorted,
        m,
        call!(shape::field, m, &observation, "head_pins")?
    )?;
    for pin in pins {
        shape::closed(m, pin, &["kind", "ref", "revision"], &[])?;
        let reference = call!(shape::reference, m, call!(shape::field, m, pin, "ref")?)?;
        let kind = shape::text(m, pin, "kind")?;
        let mut found = None;
        for (k, rk, s) in [
            (
                "decision",
                "workflow-decision",
                "ledger-workflow-decision-revision/1",
            ),
            (
                "family",
                "workflow-family-state",
                "ledger-workflow-family-state/1",
            ),
            (
                "chain",
                "workflow-chain-state",
                "ledger-workflow-chain-state/1",
            ),
            (
                "capacity",
                "workflow-correction-capacity",
                "ledger-correction-capacity/1",
            ),
        ] {
            if shape::same_text(m, kind, k)? {
                found = Some((rk, s));
                break;
            }
        }
        let (rk, s) = found.ok_or_else(|| shape::fail("financial head kind"))?;
        if !shape::same_text(m, &reference.kind, rk)?
            || !shape::member(
                m,
                &json!(preexisting).as_array().expect("Ref Vec").clone(),
                &json!(reference),
            )?
        {
            return Err(shape::fail("financial head not an immediate input"));
        }
        let record = r.get(m, &c.key, &reference, rk, s)?;
        call!(
            equal,
            m,
            call!(shape::field, m, pin, "revision")?,
            call!(shape::field, m, &record, "revision")?
        )?;
        let revision = call!(
            arithmetic::integer,
            m,
            call!(shape::text, m, pin, "revision")?,
            false
        )?;
        if !meter::cmp(m, &revision, &0.into())?.is_gt() {
            return Err(shape::fail("financial head revision"));
        }
    }
    let raw = r.canonical(m, &reference)?;
    m.read_bytes(raw.len())?;
    m.write_bytes(raw.len())?;
    Ok(FinancialInputSelection {
        canonical_body: raw.to_vec(),
        reference,
        preexisting,
        history_edges: expected_edges,
    })
}
