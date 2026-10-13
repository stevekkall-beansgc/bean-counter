//! Strict raw outer ingress. Payload trust/normalization is a separate typed step.
use super::super::{Result, records, reject, request::OperationKey, require};
use ledgerlab_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Outer {
    schema: String,
    key: OperationKey,
    kind: String,
    contract: String,
    payload: Value,
}
/// Cannot be constructed by deserializing a purported prepared journal. This
/// validates only the closed lexical outer envelope and dispatch discriminator.
/// A payload Value never establishes evidence authenticity or enrollment rights.
pub(super) struct Request {
    outer: Outer,
    raw: Vec<u8>,
}
impl Request {
    pub(super) fn parse(raw: &[u8]) -> Result<Self> {
        let outer = parse_outer(raw)?;
        require(
            matches!(outer.kind.as_str(), "evidence.retain" | "authority.change"),
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        let schema = outer
            .payload
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        let valid = match outer.kind.as_str() {
            "evidence.retain" => matches!(
                schema,
                "ledger-workflow-evidence-retain/1"
                    | "ledger-workflow-definition-retain/1"
                    | "ledger-workflow-definition-fragment/1"
                    | "ledger-workflow-definition-finalize/1"
            ),
            "authority.change" => matches!(
                schema,
                "ledger-workflow-customer-register/1"
                    | "ledger-workflow-source-enroll/1"
                    | "ledger-workflow-source-restrict/1"
                    | "ledger-workflow-source-revoke/1"
            ),
            _ => false,
        };
        require(valid, "UNSUPPORTED_SCHEMA_PROFILE")?;
        Ok(Self {
            outer,
            raw: raw.to_vec(),
        })
    }
    /// Closed raw ingress for the reviewed agreement coordinator. This step
    /// does not resolve private documents, authenticate assent or grant rights.
    /// Existing prerequisite dispatch remains separately restricted above.
    pub(super) fn parse_agreement(raw: &[u8]) -> Result<Self> {
        let outer = parse_outer(raw)?;
        let schema = outer
            .payload
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        let valid = match outer.kind.as_str() {
            "agreement.accept" => schema == "ledger-workflow-agreement-accept/2",
            "agreement.activate" => matches!(
                schema,
                "ledger-workflow-activation/1" | "ledger-workflow-activation-replace/1"
            ),
            "agreement.revoke" => schema == "ledger-workflow-agreement-revoke/1",
            _ => false,
        };
        require(valid, "UNSUPPORTED_SCHEMA_PROFILE")?;
        Ok(Self {
            outer,
            raw: raw.to_vec(),
        })
    }
    pub(super) fn key(&self) -> &OperationKey {
        &self.outer.key
    }
    pub(super) fn kind(&self) -> &str {
        &self.outer.kind
    }
    pub(super) fn payload(&self) -> &Value {
        &self.outer.payload
    }
    pub(super) fn raw(&self) -> &[u8] {
        &self.raw
    }
    pub(super) fn normalized_envelope(&self, payload: &impl Serialize) -> Value {
        json!({"schema":"ledger-operation/1","contract":"workflow/1","key":self.outer.key,"kind":self.outer.kind,"payload":payload})
    }
}
fn parse_outer(raw: &[u8]) -> Result<Outer> {
    let v = canonical::parse_bounded(raw, 262144).map_err(|_| reject("INVALID_SHAPE"))?;
    require(no_null(&v), "INVALID_SHAPE")?;
    let outer: Outer = serde_json::from_value(v).map_err(|_| reject("INVALID_SHAPE"))?;
    require(
        outer.schema == "ledger-operation/1" && outer.contract == "workflow/1",
        "INVALID_SHAPE",
    )?;
    OperationKey::parse(&records::bytes(&outer.key)?)?;
    Ok(outer)
}
fn no_null(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => a.iter().all(no_null),
        Value::Object(o) => o.values().all(no_null),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> Value {
        json!({"schema":"ledger-operation/1","contract":"workflow/1","key":{"logical_store_id":"test-store","scope":["test","local"],"source":"urn:test:owner","operation_id":"explicit-evidence"},"kind":"evidence.retain","payload":{"schema":"ledger-workflow-evidence-retain/1"}})
    }
    #[test]
    fn agreement_closed_raw_family_preserves_evidence_without_advertising_execution() {
        let mut v = valid();
        v["kind"] = json!("agreement.accept");
        v["payload"] = json!({"schema":"ledger-workflow-agreement-accept/2"});
        let raw = serde_json::to_vec_pretty(&v).unwrap();
        let request = Request::parse_agreement(&raw).unwrap();
        assert_eq!(request.raw(), raw);
        assert!(Request::parse(&raw).is_err());
        let duplicate = String::from_utf8(raw.clone()).unwrap().replacen(
            "\"kind\": \"agreement.accept\"",
            "\"kind\": \"agreement.accept\", \"\\u006bind\": \"agreement.accept\"",
            1,
        );
        assert!(Request::parse_agreement(duplicate.as_bytes()).is_err());
        v["owner"] = json!(true);
        assert!(Request::parse_agreement(&records::bytes(&v).unwrap()).is_err());
        v.as_object_mut().unwrap().remove("owner");
        v["payload"]["trusted"] = Value::Null;
        assert!(Request::parse_agreement(&records::bytes(&v).unwrap()).is_err());
    }
    #[test]
    fn agreement_raw_outer_never_drops_duplicate_unknown_or_null_input() {
        let mut v = valid();
        v["owner"] = json!(true);
        assert!(Request::parse(&records::bytes(&v).unwrap()).is_err());
        let mut v = valid();
        v["key"]["admin"] = json!(true);
        assert!(Request::parse(&records::bytes(&v).unwrap()).is_err());
        let mut v = valid();
        v["payload"]["metadata"] = Value::Null;
        assert!(Request::parse(&records::bytes(&v).unwrap()).is_err());
        let raw = records::bytes(&valid()).unwrap();
        let s = String::from_utf8(raw).unwrap();
        let duplicate = s.replacen(
            "\"kind\":\"evidence.retain\"",
            "\"kind\":\"evidence.retain\",\"\\u006bind\":\"evidence.retain\"",
            1,
        );
        assert!(Request::parse(duplicate.as_bytes()).is_err());
    }
    #[test]
    fn agreement_outer_retains_original_raw_and_does_not_claim_payload_validation() {
        let v = valid();
        let canonical = records::bytes(&v).unwrap();
        let pretty = serde_json::to_vec_pretty(&v).unwrap();
        let a = Request::parse(&canonical).unwrap();
        let b = Request::parse(&pretty).unwrap();
        assert_eq!(a.key(), b.key());
        assert_eq!(
            a.normalized_envelope(a.payload()),
            b.normalized_envelope(b.payload())
        );
        assert_ne!(a.raw(), b.raw());
        assert_eq!(a.kind(), "evidence.retain");
        // The deliberately incomplete payload here proves this step is only
        // outer ingress. No prepared/authorized append can be obtained from it.
    }
}
