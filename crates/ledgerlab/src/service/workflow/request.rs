use super::*;
use crate::store::sqlite::workflow::RecordRef;
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationKey {
    pub logical_store_id: String,
    pub scope: [String; 2],
    pub source: String,
    pub operation_id: String,
}
impl OperationKey {
    pub(crate) fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 65536).map_err(|_| reject("INVALID_SHAPE"))?;
        let key: Self = serde_json::from_value(value).map_err(|_| reject("INVALID_SHAPE"))?;
        key.validate()?;
        Ok(key)
    }
    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        CanonicalBytes::from_value(self)
            .map(|b| b.into_vec())
            .map_err(|_| reject("INVALID_SHAPE"))
    }
    fn validate(&self) -> Result<()> {
        for s in [
            &self.logical_store_id,
            &self.operation_id,
            &self.scope[0],
            &self.scope[1],
        ] {
            require(valid_id(s, 128), "INVALID_SHAPE")?;
        }
        require(valid_id(&self.source, 256), "INVALID_SHAPE")
    }
}
fn valid_id(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outer {
    schema: String,
    key: OperationKey,
    kind: String,
    contract: String,
    payload: Payload,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", deny_unknown_fields)]
enum Payload {
    #[serde(rename = "inline")]
    Inline {
        schema: String,
        documents: Vec<Value>,
        root_policy_ref: RecordRef,
        expected_plan_digest: String,
    },
    #[serde(rename = "retained")]
    Retained {
        schema: String,
        document_refs: Vec<RecordRef>,
        root_policy_ref: RecordRef,
        expected_plan_digest: String,
    },
}
pub(crate) struct PublicationRequest {
    outer: Outer,
    raw: Vec<u8>,
}
impl PublicationRequest {
    /// Strict lexical parsing precedes Value/DTO conversion, so duplicate decoded
    /// keys and prohibited number spellings cannot be silently discarded.
    pub(crate) fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262_144).map_err(|_| reject("INVALID_SHAPE"))?;
        require(no_null(&value), "INVALID_SHAPE")?;
        let outer: Outer = serde_json::from_value(value).map_err(|_| reject("INVALID_SHAPE"))?;
        require(
            outer.schema == "ledger-operation/1"
                && outer.kind == "policy.publish"
                && outer.contract == "workflow/1",
            "INVALID_SHAPE",
        )?;
        outer.key.validate()?;
        Self::validate_payload(&outer.payload)?;
        Ok(Self {
            outer,
            raw: raw.to_vec(),
        })
    }
    /// Internal read projection of an already strictly parsed read payload.
    /// It carries no original write bytes, authority or append permission.
    /// The enclosing read ingress owns its 256KiB bound; do not charge a
    /// manufactured write envelope against that bound.
    pub(crate) fn from_read_candidate(key: &OperationKey, value: &Value) -> Result<Self> {
        key.validate()?;
        require(no_null(value), "INVALID_SHAPE")?;
        let payload: Payload =
            serde_json::from_value(value.clone()).map_err(|_| reject("INVALID_SHAPE"))?;
        Self::validate_payload(&payload)?;
        Ok(Self {
            outer: Outer {
                schema: "ledger-operation/1".into(),
                key: key.clone(),
                kind: "policy.publish".into(),
                contract: "workflow/1".into(),
                payload,
            },
            raw: Vec::new(),
        })
    }
    fn validate_payload(payload: &Payload) -> Result<()> {
        let (schema, count, root, digest) = match payload {
            Payload::Inline {
                schema,
                documents,
                root_policy_ref,
                expected_plan_digest,
            } => (
                schema,
                documents.len(),
                root_policy_ref,
                expected_plan_digest,
            ),
            Payload::Retained {
                schema,
                document_refs,
                root_policy_ref,
                expected_plan_digest,
            } => (
                schema,
                document_refs.len(),
                root_policy_ref,
                expected_plan_digest,
            ),
        };
        require(
            matches!(
                schema.as_str(),
                "ledger-workflow-policy-publication/1"
                    | "ledger-workflow-policy-publication/2"
                    | "ledger-workflow-policy-publication/3"
            ) && (1..=128).contains(&count)
                && root.kind == "workflow-policy"
                && valid_digest(digest)
                && valid_reference(root),
            "INVALID_SHAPE",
        )?;
        if schema == "ledger-workflow-policy-publication/2" {
            if let Payload::Inline { documents, .. } = payload {
                require(
                    documents.iter().all(|document| {
                        matches!(
                            document["schema"].as_str(),
                            Some(
                                "ledger-workflow-fact-schema/1"
                                    | "ledger-workflow-product/1"
                                    | "ledger-workflow-slot-template/1"
                                    | "ledger-workflow-policy/2"
                                    | "ledger-workflow-compiler-descriptor/2"
                            )
                        )
                    }),
                    "INVALID_SHAPE",
                )?;
            }
        }
        if schema == "ledger-workflow-policy-publication/3" {
            if let Payload::Inline { documents, .. } = payload {
                require(
                    documents.iter().all(|d| {
                        matches!(
                            d["schema"].as_str(),
                            Some(
                                "ledger-workflow-fact-schema/1"
                                    | "ledger-workflow-product/1"
                                    | "ledger-workflow-slot-template/1"
                                    | "ledger-workflow-policy/2"
                                    | "ledger-workflow-compiler-descriptor/3"
                            )
                        )
                    }),
                    "INVALID_SHAPE",
                )?;
            }
        }
        if let Payload::Retained { document_refs, .. } = payload {
            require(document_refs.iter().all(valid_reference), "INVALID_SHAPE")?;
        }
        Ok(())
    }
    pub(crate) fn key(&self) -> &OperationKey {
        &self.outer.key
    }
    pub(crate) fn raw(&self) -> &[u8] {
        &self.raw
    }
    pub(crate) fn payload_schema(&self) -> &str {
        match &self.outer.payload {
            Payload::Inline { schema, .. } | Payload::Retained { schema, .. } => schema,
        }
    }
    pub(crate) fn value(&self) -> Result<Value> {
        serde_json::to_value(&self.outer).map_err(|_| reject("INVALID_SHAPE"))
    }
    pub(crate) fn inline_documents(&self) -> Option<&[Value]> {
        match &self.outer.payload {
            Payload::Inline { documents, .. } => Some(documents),
            _ => None,
        }
    }
    pub(crate) fn retained_refs(&self) -> Option<&[RecordRef]> {
        match &self.outer.payload {
            Payload::Retained { document_refs, .. } => Some(document_refs),
            _ => None,
        }
    }
    pub(crate) fn root(&self) -> &RecordRef {
        match &self.outer.payload {
            Payload::Inline {
                root_policy_ref, ..
            }
            | Payload::Retained {
                root_policy_ref, ..
            } => root_policy_ref,
        }
    }
    pub(crate) fn expected_digest(&self) -> &str {
        match &self.outer.payload {
            Payload::Inline {
                expected_plan_digest,
                ..
            }
            | Payload::Retained {
                expected_plan_digest,
                ..
            } => expected_plan_digest,
        }
    }
}
pub(super) fn valid_digest(s: &str) -> bool {
    s.len() == 71
        && s.starts_with("sha256:")
        && s.as_bytes()[7..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
pub(super) fn valid_reference(r: &RecordRef) -> bool {
    valid_digest(&r.digest)
        && r.id == format!("wdoc_{}", &r.digest[7..])
        && matches!(
            r.kind.as_str(),
            "workflow-fact-schema"
                | "workflow-product"
                | "workflow-policy"
                | "workflow-slot-template"
                | "workflow-compiler-descriptor"
        )
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
    fn original() -> Vec<u8> {
        let v:Value=serde_json::from_str(include_str!("../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
        CanonicalBytes::from_value(
            &v["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "operation")
                .unwrap()["body"]["request_evidence"]["normalized_request"],
        )
        .unwrap()
        .into_vec()
    }
    #[test]
    fn raw_ingress_refuses_duplicate_escaped_keys_and_prohibited_lexemes() {
        let raw = original();
        assert!(PublicationRequest::parse(&raw).is_ok());
        let text = String::from_utf8(raw.clone()).unwrap();
        let duplicate = text.replacen("{", "{\"\\u006bey\":{},", 1);
        assert!(PublicationRequest::parse(duplicate.as_bytes()).is_err());
        let unknown = text.replacen("{", "{\"unknown\":true,", 1);
        assert!(PublicationRequest::parse(unknown.as_bytes()).is_err());
        let null = text.replacen("\"documents\":[", "\"documents\":[null,", 1);
        assert!(PublicationRequest::parse(null.as_bytes()).is_err());
        for number in ["18.0", "18e0", "-0", "9007199254740992"] {
            assert!(
                PublicationRequest::parse(
                    text.replace("\"scale\":18", &format!("\"scale\":{number}"))
                        .as_bytes()
                )
                .is_err()
            );
        }
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(&raw);
        assert!(PublicationRequest::parse(&bom).is_err());
        let mut invalid = raw;
        invalid[3] = 0xff;
        assert!(PublicationRequest::parse(&invalid).is_err());
    }
    #[test]
    fn request_ceiling_is_256_kib_not_resolved_closure_ceiling() {
        let mut raw = original();
        raw.resize(262_144, b' ');
        assert!(PublicationRequest::parse(&raw).is_ok());
        raw.push(b' ');
        assert!(PublicationRequest::parse(&raw).is_err());
    }
    #[test]
    fn publication_two_ingress_preserves_versions_and_retained_locators() {
        let mut value: Value = serde_json::from_slice(&original()).unwrap();
        value["payload"]["schema"] = serde_json::json!("ledger-workflow-policy-publication/2");
        // Publication2 never smuggles a Policy1/Descriptor1 through its new
        // inline union. This is ingress only, not executable support.
        assert!(PublicationRequest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        for document in value["payload"]["documents"].as_array_mut().unwrap() {
            if document["schema"] == "ledger-workflow-policy/1" {
                document["schema"] = serde_json::json!("ledger-workflow-policy/2");
            } else if document["schema"] == "ledger-workflow-compiler-descriptor/1" {
                document["schema"] = serde_json::json!("ledger-workflow-compiler-descriptor/2");
            }
        }
        let parsed = PublicationRequest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            parsed.payload_schema(),
            "ledger-workflow-policy-publication/2"
        );
        assert!(super::super::publication::normalized(&parsed, &[]).is_err());
        value["payload"]["mode"] = serde_json::json!("retained");
        value["payload"]
            .as_object_mut()
            .unwrap()
            .remove("documents");
        value["payload"]["document_refs"] =
            serde_json::json!([value["payload"]["root_policy_ref"].clone()]);
        assert!(PublicationRequest::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
        value["payload"]["unknown"] = Value::Bool(true);
        assert!(PublicationRequest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    #[test]
    fn read_candidate_checks_closed_payload_without_charging_write_wrapper() {
        let mut value: Value = serde_json::from_slice(&original()).unwrap();
        let key: OperationKey = serde_json::from_value(value["key"].clone()).unwrap();
        // This exercises the request-syntax boundary only; typed document
        // validation still belongs to the pure normalizer after this helper.
        value["payload"]["documents"][0]["syntax_test_padding"] = Value::String(String::new());
        let read_base = serde_json::json!({"schema":"ledger-read/1","kind":"operation.descriptors","key":key,"payload":value["payload"]});
        let padding = 262_144 - serde_json::to_vec(&read_base).unwrap().len();
        value["payload"]["documents"][0]["syntax_test_padding"] =
            Value::String("x".repeat(padding));
        let read = serde_json::json!({"schema":"ledger-read/1","kind":"operation.descriptors","key":key,"payload":value["payload"]});
        assert_eq!(serde_json::to_vec(&read).unwrap().len(), 262_144);
        assert!(serde_json::to_vec(&value).unwrap().len() > 262_144);
        assert!(PublicationRequest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        let candidate = PublicationRequest::from_read_candidate(&key, &value["payload"]).unwrap();
        assert_eq!(candidate.key(), &key);
        assert!(candidate.raw().is_empty());
        for (field, bad) in [
            ("unexpected", Value::Bool(true)),
            ("expected_plan_digest", Value::Null),
            ("schema", Value::String("wrong".into())),
            ("documents", serde_json::json!([])),
            (
                "root_policy_ref",
                serde_json::json!({"kind":"workflow-product","id":"wrong","digest":"wrong"}),
            ),
        ] {
            let mut payload = value["payload"].clone();
            payload[field] = bad;
            assert!(PublicationRequest::from_read_candidate(&key, &payload).is_err());
        }
    }
    #[test]
    fn equivalent_lexical_forms_retain_their_own_original_raw_bytes() {
        let raw = original();
        let value: Value = serde_json::from_slice(&raw).unwrap();
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        assert_ne!(raw, pretty);
        let a = PublicationRequest::parse(&raw).unwrap();
        let b = PublicationRequest::parse(&pretty).unwrap();
        assert_eq!(a.value().unwrap(), b.value().unwrap());
        assert_eq!(a.raw(), raw);
        assert_eq!(b.raw(), pretty);
    }
}
