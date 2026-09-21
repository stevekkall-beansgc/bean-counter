//! Closed frozen outcome envelope validation. Schemas are compiled-in reference
//! bytes; validation performs no filesystem, environment, network or database IO.
use super::{outcome_digest, parse_bounded, CanonicalBytes, BUNDLE_LIMIT};
use crate::{domain, money, Error, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;
pub const ECONOMIC: &str = "2-candidate.4";
pub const SETTLEMENT: &str = "reservation-settlement/1";
fn invalid() -> Error {
    Error::new("OUTCOME_RECORD", "frozen record validation failed")
}
fn need(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid())
    }
}
pub fn bytes(v: &Value) -> Result<Vec<u8>> {
    Ok(CanonicalBytes::from_value(v)?.into_vec())
}
pub fn reference(v: &Value) -> Value {
    json!({"kind":v["kind"],"id":v["id"],"content_hash":v["content_hash"]})
}
pub fn ordered(mut v: Vec<Value>) -> Result<Vec<Value>> {
    let mut keyed = v
        .drain(..)
        .map(|v| Ok((bytes(&v)?, v)))
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keyed.into_iter().map(|(_, v)| v).collect())
}
fn schema(profile: &str) -> Result<&'static Value> {
    static ECON: OnceLock<Value> = OnceLock::new();
    static SETTLE: OnceLock<Value> = OnceLock::new();
    match profile {
        ECONOMIC => Ok(ECON.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../contracts/candidates/v2/schemas/canonical-records.schema.json"
            ))
            .expect("frozen schema")
        })),
        SETTLEMENT => Ok(SETTLE.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../../contracts/candidates/reservation-settlement-v1/records.schema.json"
            ))
            .expect("frozen schema")
        })),
        _ => Err(invalid()),
    }
}
fn spelling(p: &str, s: &str) -> bool {
    if let Some(prefix) = p
        .strip_prefix('^')
        .and_then(|v| v.strip_suffix("[0-9a-f]{64}(?![\\s\\S])"))
    {
        let candidates: Vec<String> = if prefix.starts_with('(') {
            prefix
                .trim_end_matches('_')
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split('|')
                .map(|v| format!("{v}_"))
                .collect()
        } else {
            vec![prefix.into()]
        };
        return candidates.iter().any(|p| {
            s.strip_prefix(p).is_some_and(|x| {
                x.len() == 64
                    && x.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
        });
    }
    if p.starts_with("^[A-Z]") {
        return s.len() == 3 && s.bytes().all(|c| c.is_ascii_uppercase());
    }
    if p.starts_with("^[a-z]") {
        return domain::slug(s).is_ok();
    }
    if p.starts_with("^[^\\u0000") {
        return !s.is_empty() && !s.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}');
    }
    if p.starts_with("^[0-9]{4}") {
        return domain::Timestamp::parse(s).is_ok();
    }
    if p.contains("\\.") {
        return money::Decimal::parse(s).is_ok();
    }
    if p.starts_with("^(0|") || p.starts_with("^[1-9]") {
        let signed = p.contains("-?");
        let digits = s.strip_prefix('-').unwrap_or(s);
        let max = if p.contains("{0,18}") {
            19
        } else if p.contains("{0,29}") {
            30
        } else if p.contains("{0,154}") {
            155
        } else {
            return false;
        };
        return (signed || !s.starts_with('-'))
            && digits.len() <= max
            && !digits.is_empty()
            && digits.bytes().all(|c| c.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
            && s != "-0"
            && (!p.starts_with("^[1-9]") || s != "0");
    }
    false
}
fn scalar(k: &str, v: &Value) -> Result<()> {
    let s = v.as_str().unwrap_or("");
    match k {
        "text" => domain::text(s, usize::MAX),
        "source" => domain::validate_source(s),
        "slug" => domain::slug(s),
        "time" => domain::Timestamp::parse(s).map(|_| ()),
        "uint" => domain::Revision::parse(s).map(|_| ()),
        "atoms" => money::parse_atoms(s).map(|_| ()),
        "nonnegative-atoms" => need(money::parse_atoms(s)? >= 0),
        "nonnegative-money" => {
            need(money::parse_atoms(v["atoms"].as_str().ok_or_else(invalid)?)? >= 0)
        }
        "decimal" => money::Decimal::parse(s).map(|_| ()),
        "positive-decimal" => need(!money::Decimal::parse(s)?.is_zero()),
        "decimal-percent" => money::Decimal::parse(s)?.percent().map(|_| ()),
        "ratio" => money::ExactRatio::from_canonical(
            v["numerator"].as_str().ok_or_else(invalid)?,
            v["denominator"].as_str().ok_or_else(invalid)?,
        )
        .map(|_| ()),
        _ => Err(invalid()),
    }
}
fn validate(root: &Value, s: &Value, v: &Value, depth: usize) -> Result<()> {
    need(depth < 96)?;
    if let Some(b) = s.as_bool() {
        return need(b);
    }
    if let Some(r) = s["$ref"].as_str() {
        validate(
            root,
            root.pointer(r.strip_prefix('#').ok_or_else(invalid)?)
                .ok_or_else(invalid)?,
            v,
            depth + 1,
        )?;
    }
    if let Some(t) = s["type"].as_str() {
        need(match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "integer" => v.is_i64() || v.is_u64(),
            "boolean" => v.is_boolean(),
            _ => false,
        })?;
    }
    if let Some(c) = s.get("const") {
        need(v == c)?
    }
    if let Some(e) = s["enum"].as_array() {
        need(e.contains(v))?
    }
    if let Some(a) = s["oneOf"].as_array() {
        need(
            a.iter()
                .filter(|x| validate(root, x, v, depth + 1).is_ok())
                .count()
                == 1,
        )?
    }
    if let Some(a) = s["allOf"].as_array() {
        for x in a {
            validate(root, x, v, depth + 1)?
        }
    }
    if let Some(x) = s.get("not") {
        need(validate(root, x, v, depth + 1).is_err())?
    }
    if let Some(x) = s.get("if") {
        let key = if validate(root, x, v, depth + 1).is_ok() {
            "then"
        } else {
            "else"
        };
        if let Some(x) = s.get(key) {
            validate(root, x, v, depth + 1)?
        }
    }
    if let Some(o) = v.as_object() {
        if let Some(n) = s["maxProperties"].as_u64() {
            need(o.len() <= n as usize)?
        }
        if let Some(r) = s["required"].as_array() {
            for k in r {
                need(o.contains_key(k.as_str().ok_or_else(invalid)?))?
            }
        }
        for (k, v) in o {
            if let Some(p) = s["properties"].get(k) {
                validate(root, p, v, depth + 1)?
            } else if let Some(p) = s.get("additionalProperties") {
                validate(root, p, v, depth + 1)?
            }
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(n) = s["minItems"].as_u64() {
            need(a.len() >= n as usize)?
        }
        if let Some(n) = s["maxItems"].as_u64() {
            need(a.len() <= n as usize)?
        }
        if s["uniqueItems"] == true {
            let mut set = BTreeSet::new();
            for v in a {
                need(set.insert(bytes(v)?))?
            }
        }
        let prefix = s["prefixItems"].as_array();
        for (i, v) in a.iter().enumerate() {
            if let Some(p) = prefix.and_then(|p| p.get(i)) {
                validate(root, p, v, depth + 1)?
            } else if let Some(p) = s.get("items") {
                validate(root, p, v, depth + 1)?
            }
        }
    }
    if let Some(x) = v.as_str() {
        let n = x.chars().count();
        if let Some(min) = s["minLength"].as_u64() {
            need(n >= min as usize)?
        }
        if let Some(max) = s["maxLength"].as_u64() {
            need(n <= max as usize)?
        }
        if let Some(max) = s["x-utf8-maxBytes"].as_u64() {
            need(x.len() <= max as usize)?
        }
        if let Some(p) = s["pattern"].as_str() {
            need(spelling(p, x))?
        }
        if let Some(max) = s["x-maximum"].as_str() {
            need(
                x.parse::<u64>().map_err(|_| invalid())?
                    <= max.parse::<u64>().map_err(|_| invalid())?,
            )?
        }
        if let Some(k) = s["x-canonicalSchema"].as_str() {
            let inner = parse_bounded(x.as_bytes(), BUNDLE_LIMIT)?;
            need(bytes(&inner)? == x.as_bytes())?;
            validate(root, &root["$defs"][k], &inner, depth + 1)?
        }
    }
    if let Some(n) = v.as_i64() {
        if let Some(min) = s["minimum"].as_i64() {
            need(n >= min)?
        }
        if let Some(max) = s["maximum"].as_i64() {
            need(n <= max)?
        }
    }
    if let Some(k) = s["x-scalar"].as_str() {
        scalar(k, v)?
    }
    if let Some(max) = s["x-canonical-maxBytes"].as_u64() {
        need(bytes(v)?.len() <= max as usize)?
    }
    Ok(())
}
pub fn key(profile: &str, k: &str, s: &Value, b: &Value) -> Result<Value> {
    let (prefix, input) = if profile == SETTLEMENT {
        match k {
            "reservation-observation" => (
                "rso1_",
                json!([s, b["command"]["source"], b["command"]["external_id"]]),
            ),
            "reservation-transition" => (
                "rst1_",
                json!([s, b["invocation_id"], b["after"]["revision"]]),
            ),
            "reservation-receipt" => ("rsr1_", json!([s, b["source"], b["external_id"]])),
            _ => return Err(invalid()),
        }
    } else {
        match k {
            "evidence" => ("ed2_", json!([s, b])),
            "policy-snapshot" => ("po2_", json!([s, b])),
            "target-basis" => ("tb2_", json!([s, b])),
            "replay-input" => ("rp2_", json!([s, b])),
            "binding-snapshot" => ("bs2_", json!([s, b])),
            "base-evaluation" => ("be2_", json!([s, b])),
            "target-snapshot" => ("ts2_", json!([s, b])),
            "base-identity" => (
                "bi2_",
                json!([s, b["target"], b["original_kind"], b["original_id"]]),
            ),
            "event" => (
                "ev2_",
                json!([s, b["data"]["source"], b["data"]["external_id"]]),
            ),
            "base-posting" => (
                "bp2_",
                json!([s, b["event_id"], b["agreement_id"], b["book"], b["ordinal"]]),
            ),
            "claim" => (
                "cl2_",
                json!([s, b["agreement_id"], b["family_id"], b["target"]]),
            ),
            "claim-revision" => ("rv2_", json!([b["claim_id"], b["number"]])),
            "effect" => ("ef2_", json!([b["claim_id"], b["revision_id"], b["slot"]])),
            "action" => ("ac2_", json!([b["effect_id"]])),
            "obligation" => (
                "ob2_",
                json!([
                    s,
                    b["agreement_id"],
                    b["book"],
                    b["currency"],
                    b["scale"],
                    b["roles"]
                ]),
            ),
            "base-acceptance" => ("ba2_", json!([b["target"]])),
            "authority-decision" => ("au2_", json!([b["event_id"]])),
            "admission" => ("ad2_", json!([b["event_id"]])),
            "decision-manifest" => ("dc2_", json!([b["event_id"]])),
            "receipt" => ("rc2_", json!([b["event_id"]])),
            "limit-evidence" => ("li2_", json!([b["event_id"], 0])),
            "explanation" => ("xp2_", json!([b["event_id"], b["ordinal"]])),
            "intention" => (
                "in2_",
                json!([s, b["destination"], b["obligation_id"], b["action_ids"]]),
            ),
            "link" => return Ok(json!([s, "outcome_of", b["event_id"], b["target"]])),
            "dependency" => {
                return Ok(json!([
                    s,
                    b["dependent"],
                    b["input"]["kind"],
                    b["input"]["id"]
                ]))
            }
            "delivery-key" => return Ok(json!([s, b["source"], b["external_id"]])),
            "chain-revision" => return Ok(json!([s, b["chain_id"], b["number"]])),
            _ => return Err(invalid()),
        }
    };
    Ok(json!(format!(
        "{}{}",
        prefix,
        &outcome_digest(profile, k, &input)?[7..]
    )))
}
pub fn envelope(profile: &str, kind: &str, scope: &Value, mut body: Value) -> Result<Value> {
    body.as_object_mut()
        .ok_or_else(invalid)?
        .insert("schema".into(), json!(format!("ledger-{kind}/{profile}")));
    let hash = if kind == "decision-manifest" {
        outcome_digest(profile, "decision-content", &body)?
    } else {
        outcome_digest(
            profile,
            "record-content",
            &json!([kind, if profile == ECONOMIC { 2 } else { 1 }, body]),
        )?
    };
    Ok(
        json!({"kind":kind,"scope":scope,"id":key(profile,kind,scope,&body)?,"body":body,"content_hash":hash}),
    )
}
fn set_order(v: &Value, name: &str) -> Result<()> {
    const SETS: &[&str] = &[
        "rules",
        "evidence",
        "postings",
        "live_action_ids",
        "inverse_action_ids",
        "current_before",
        "action_ids",
        "inputs",
        "depends_on",
        "actions",
        "intention_ids",
        "families",
        "bindings",
        "limits",
        "replacement_codes",
        "sources",
        "correction_sources",
        "predecessors",
        "verified_assents",
        "verified_offers",
        "verified_delegations",
        "verified_evidence",
        "event_types",
        "allowed_modifiers",
        "policy_evidence",
        "identity_mappings",
        "replay",
        "permissions",
    ];
    if let Some(o) = v.as_object() {
        for (k, v) in o {
            set_order(v, k)?;
        }
    }
    if let Some(a) = v.as_array() {
        if SETS.contains(&name) {
            let keys = a.iter().map(bytes).collect::<Result<Vec<_>>>()?;
            need(keys.windows(2).all(|w| w[0] < w[1]))?;
        }
        for v in a {
            set_order(v, "")?;
        }
    }
    Ok(())
}
pub fn decode(raw: &[u8]) -> Result<Value> {
    let v = parse_bounded(raw, BUNDLE_LIMIT)?;
    need(bytes(&v)? == raw)?;
    let kind = v["kind"].as_str().ok_or_else(invalid)?;
    let profile = if kind.starts_with("reservation-") {
        SETTLEMENT
    } else {
        ECONOMIC
    };
    let schema = schema(profile)?;
    validate(schema, schema, &v, 0)?;
    set_order(&v, "")?;
    for part in v["scope"].as_array().ok_or_else(invalid)? {
        domain::text(part.as_str().ok_or_else(invalid)?, 128)?;
    }
    if profile == SETTLEMENT {
        let b = &v["body"];
        if kind == "reservation-observation" {
            let c = &b["command"];
            domain::validate_source(c["source"].as_str().ok_or_else(invalid)?)?;
            for k in ["external_id", "invocation_id"] {
                domain::text(c[k].as_str().ok_or_else(invalid)?, 128)?;
            }
            domain::text(
                b["authority"]["principal"].as_str().ok_or_else(invalid)?,
                128,
            )?;
            domain::Revision::parse(
                b["authority"]["grant_revision"]
                    .as_str()
                    .ok_or_else(invalid)?,
            )?;
            if let Some(r) = c.get("expected_revision") {
                domain::Revision::parse(r.as_str().ok_or_else(invalid)?)?;
            }
        }
        for name in ["before", "after", "result"] {
            if let Some(state) = b.get(name) {
                domain::Revision::parse(state["revision"].as_str().ok_or_else(invalid)?)?;
                let mut families = BTreeSet::new();
                for f in state["families"].as_array().ok_or_else(invalid)? {
                    need(families.insert(bytes(&f["key"])?))?;
                    for k in ["agreement_id", "family_id"] {
                        domain::text(f["key"][k].as_str().ok_or_else(invalid)?, 128)?;
                    }
                }
            }
        }
    }
    need(bytes(&v["body"])?.len() <= 262144)?;
    need(envelope(profile, kind, &v["scope"], v["body"].clone())? == v)?;
    Ok(v)
}
