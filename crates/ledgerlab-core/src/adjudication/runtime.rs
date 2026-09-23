//! Incremental R3 transition helpers. Inputs are bounded point observations;
//! no function reads a global journal/history catalog.
use super::{canonical_bytes, commands::Command, types::Digest, COMMAND_BYTES, SEGMENT_BYTES};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

pub fn hash(domain: &str, value: &impl Serialize) -> Result<Digest> {
    if ![
        "command",
        "submission",
        "grant",
        "claim",
        "receipt",
        "action",
        "closure",
        "segment",
        "replay",
        "route",
        "namespace",
        "authority",
        "evidence",
        "result",
        "enrollment",
    ]
    .contains(&domain)
    {
        return Err(Error::new("HASH_DOMAIN", "R3 domain"));
    }
    let mut h = Sha256::new();
    h.update(format!("ledgerlab/central-r3/{domain}/1\0"));
    h.update(canonical_bytes(value, SEGMENT_BYTES)?);
    Digest::parse(&crate::canonical::hex(&h.finalize()))
}
pub fn command_value(command: &Command) -> Result<serde_json::Value> {
    serde_json::to_value(command).map_err(|e| Error::new("SHAPE", e.to_string()))
}
pub fn command_digest(command: &Command) -> Result<Digest> {
    let v = command_value(command)?;
    let p = if v["kind"] == "RECEIVE" {
        &v["payload"]["submission"]
    } else {
        &v["payload"]
    };
    hash("command", &serde_json::json!([v["kind"], v["key"], p]))
}
pub fn full_key(value: &impl Serialize) -> Result<Vec<u8>> {
    canonical_bytes(value, COMMAND_BYTES)
}

/// Binary full-key framing used by the accepted index model. Tags are exactly
/// eight bytes; UTF-8 components remain unnormalized and lengths are unambiguous.
pub fn index_key(tag: [u8; 8], components: &[&[u8]]) -> Result<Vec<u8>> {
    if components.len() > u8::MAX as usize {
        return Err(Error::new("INDEX_ARITY", "component count"));
    }
    let mut out = tag.to_vec();
    out.push(components.len() as u8);
    for component in components {
        let n =
            u16::try_from(component.len()).map_err(|_| Error::new("INDEX_COMPONENT", "length"))?;
        out.extend(n.to_be_bytes());
        out.extend(*component);
    }
    if out.len() > super::MAX_KEY_BYTES {
        return Err(Error::new("INDEX_K", "full key bound"));
    }
    Ok(out)
}
pub fn delivery_key(d: &super::commands::Delivery) -> Result<Vec<u8>> {
    index_key(
        *b"DELIVERY",
        &[
            d.0 .0.as_str().as_bytes(),
            d.0 .1.as_str().as_bytes(),
            d.1.as_str().as_bytes(),
            d.2.as_str().as_bytes(),
        ],
    )
}
pub fn family_key(f: &super::commands::Family) -> Result<Vec<u8>> {
    index_key(
        *b"FAMILY__",
        &[
            f.0 .0.as_str().as_bytes(),
            f.0 .1.as_str().as_bytes(),
            f.1.as_str().as_bytes(),
            f.2.as_str().as_bytes(),
            f.3.as_str().as_bytes(),
        ],
    )
}
pub fn case_key(c: &super::commands::Case) -> Result<Vec<u8>> {
    index_key(
        *b"CASE____",
        &[
            c.0 .0 .0.as_str().as_bytes(),
            c.0 .0 .1.as_str().as_bytes(),
            c.0 .1.as_str().as_bytes(),
            c.0 .2.as_str().as_bytes(),
            c.0 .3.as_str().as_bytes(),
            c.1.as_str().as_bytes(),
            c.2.as_str().as_bytes(),
        ],
    )
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryState {
    pub delivery: super::commands::Delivery,
    pub submission: Digest,
    pub receipt: super::commands::Receipt,
    pub token: super::types::Id,
    pub command: Digest,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_command_hash_and_binary_framing_match_frozen_values() {
        let trace:serde_json::Value=serde_json::from_str(include_str!("../../../../contracts/candidates/central-adjudication-r3-candidate1/minimal-trace.json")).unwrap();
        for c in trace["commands"].as_array().unwrap() {
            let command: Command = serde_json::from_value(c.clone()).unwrap();
            assert_eq!(
                command_digest(&command).unwrap().as_str(),
                c["authority"]["command"].as_str().unwrap()
            );
        }
        assert_ne!(
            index_key(*b"DELIVERY", &[b"a", b"bc", b"d", b"e"]).unwrap(),
            index_key(*b"DELIVERY", &[b"ab", b"c", b"d", b"e"]).unwrap()
        );
        let widths = [128, 128, 128, 128, 128, 30, 32, 128, 128, 30, 64, 30];
        let components: Vec<_> = widths.iter().map(|n| vec![b'"'; *n]).collect();
        assert_eq!(
            index_key(
                *b"AUTHDOC_",
                &components.iter().map(Vec::as_slice).collect::<Vec<_>>()
            )
            .unwrap()
            .len(),
            1115
        );
    }
}
