//! Exact approved Evaluation codec. Deserialization cannot manufacture an
//! Evaluation: inputs are decoded, replayed through the existing evaluator and
//! compared against every retained output byte and original identity.
use super::*;
use crate::{
    canonical::{self, CanonicalBytes},
    domain, Error, Result,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

fn field<T: DeserializeOwned>(v: &Value, key: &str) -> Result<T> {
    serde_json::from_value(
        v.get(key)
            .cloned()
            .ok_or_else(|| Error::new("RETAINED_FIELD", key))?,
    )
    .map_err(|e| Error::new("RETAINED_FIELD", e.to_string()))
}
pub fn decode_evaluation(bytes: &[u8], history: &[Evaluation]) -> Result<Evaluation> {
    let v = canonical::parse_bounded(bytes, canonical::BUNDLE_LIMIT)?;
    if CanonicalBytes::from_value(&v)?.as_slice() != bytes {
        return Err(Error::new("RETAINED_BYTES", "noncanonical evaluation"));
    }
    let state = &v["event"];
    let scope = field(state, "scope")?;
    let source: String = field(state, "source")?;
    let ingress: String = field(state, "ingress_utf8")?;
    let resolved: String = field(state, "event_utf8")?;
    let event_value = canonical::parse(resolved.as_bytes())?;
    let event = domain::normalize(ingress.as_bytes(), scope, &source)?
        .resolve(event_value["chain"].as_str())?;
    let bundle = &v["bundle"];
    let currency: String = field(bundle, "currency")?;
    let bundle = Bundle::compile(
        &currency,
        field(bundle, "scale")?,
        field(bundle, "policies")?,
    )?;
    let context = field(&v, "context")?;
    let authority = field(&v, "source_authority")?;
    let invocations: Vec<Invocation> = field(&v, "invocations")?;
    let costs: Vec<CostEvidence> = field(&v, "costs")?;
    let received = field(&v, "received_at")?;
    let replay = bundle.evaluate(Input {
        event: &event,
        context: &context,
        history,
        source_authority: &authority,
        invocations: &invocations,
        costs: &costs,
        received_at: &received,
    })?;
    if CanonicalBytes::from_value(&replay)?.as_slice() != bytes {
        return Err(Error::new(
            "RETAINED_REPLAY",
            "complete original evaluation differs",
        ));
    }
    Ok(replay)
}
