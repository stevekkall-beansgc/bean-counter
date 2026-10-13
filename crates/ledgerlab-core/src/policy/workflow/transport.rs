//! Fixed source transport data. Parsed candidates and fragment hashes do not
//! establish supported definitions, retained membership or administrator rights.
use super::{
    facts::{counter, reject_null},
    AgreementRef,
};
use crate::{
    canonical::{self, CanonicalBytes},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const CHUNK: usize = 131072;
const MAX_DOCUMENT: usize = 262144;
const SCHEMAS: [&str; 5] = [
    "ledger-workflow-fact-schema/1",
    "ledger-workflow-product/1",
    "ledger-workflow-slot-template/1",
    "ledger-workflow-policy/1",
    "ledger-workflow-compiler-descriptor/1",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Direct {
    schema: String,
    document: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fragment {
    schema: String,
    upload_id: String,
    document_schema: String,
    canonical_document_sha256: String,
    document_bytes: String,
    part: String,
    bytes_base64: String,
}
#[derive(Clone, Debug)]
pub struct DefinitionFragmentInput {
    body: Fragment,
    decoded: Vec<u8>,
    total: usize,
    part: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Finalize {
    schema: String,
    upload_id: String,
    fragment_refs: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct DefinitionFinalizeInput {
    upload_id: String,
    fragment_refs: Vec<AgreementRef>,
}
#[derive(Clone, Debug)]
enum Payload {
    Direct(Vec<u8>),
    Fragment(DefinitionFragmentInput),
    Finalize(DefinitionFinalizeInput),
}
#[derive(Clone, Debug)]
pub struct SourceRetentionInput(Payload);
impl SourceRetentionInput {
    /// Checks fixed transport syntax only. A direct document remains an
    /// unvalidated candidate until the typed definition normalizer resolves its
    /// exact dependencies. The host also enforces the full outer request bound.
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, MAX_DOCUMENT)?;
        reject_null(&value)?;
        let payload = match value.get("schema").and_then(Value::as_str) {
            Some("ledger-workflow-definition-retain/1") => {
                let input: Direct = canonical::dto(value)?;
                if input.schema != "ledger-workflow-definition-retain/1"
                    || !SCHEMAS.contains(&input.document["schema"].as_str().unwrap_or(""))
                {
                    return Err(fail("source candidate schema"));
                }
                let bytes = CanonicalBytes::from_value(&input.document)?
                    .as_slice()
                    .to_vec();
                if bytes.len() > MAX_DOCUMENT {
                    return Err(fail("candidate document byte limit"));
                }
                Payload::Direct(bytes)
            }
            Some("ledger-workflow-definition-fragment/1") => {
                Payload::Fragment(DefinitionFragmentInput::from_body(canonical::dto(value)?)?)
            }
            Some("ledger-workflow-definition-finalize/1") => {
                let input: Finalize = canonical::dto(value)?;
                crate::domain::text(&input.upload_id, 128)?;
                if input.schema != "ledger-workflow-definition-finalize/1"
                    || !(1..=2).contains(&input.fragment_refs.len())
                {
                    return Err(fail("finalize shape/count"));
                }
                let mut fragment_refs = Vec::new();
                for r in input.fragment_refs {
                    let r = reference(&r)?;
                    if r.kind() != "workflow-definition-fragment" {
                        return Err(fail("fragment reference family"));
                    }
                    fragment_refs.push(r);
                }
                canonical::sort_set(&mut fragment_refs)?;
                Payload::Finalize(DefinitionFinalizeInput {
                    upload_id: input.upload_id,
                    fragment_refs,
                })
            }
            _ => return Err(fail("unsupported source transport")),
        };
        Ok(Self(payload))
    }
    pub fn kind(&self) -> &str {
        match self.0 {
            Payload::Direct(_) => "direct",
            Payload::Fragment(_) => "fragment",
            Payload::Finalize(_) => "finalize",
        }
    }
    pub fn candidate_document_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            Payload::Direct(b) => Some(b),
            _ => None,
        }
    }
    pub fn fragment(&self) -> Option<&DefinitionFragmentInput> {
        match &self.0 {
            Payload::Fragment(v) => Some(v),
            _ => None,
        }
    }
    pub fn finalization(&self) -> Option<&DefinitionFinalizeInput> {
        match &self.0 {
            Payload::Finalize(v) => Some(v),
            _ => None,
        }
    }
}
impl DefinitionFragmentInput {
    fn from_body(body: Fragment) -> Result<Self> {
        if body.schema != "ledger-workflow-definition-fragment/1"
            || !SCHEMAS.contains(&body.document_schema.as_str())
        {
            return Err(fail("fragment source schema"));
        }
        crate::domain::text(&body.upload_id, 128)?;
        if !hex(&body.canonical_document_sha256) {
            return Err(fail("raw document digest"));
        }
        let total = counter(&body.document_bytes)?;
        if total == 0 || total > MAX_DOCUMENT as u64 {
            return Err(fail("document byte count"));
        }
        let total = total as usize;
        let part = match body.part.as_str() {
            "0" => 0,
            "1" if total > CHUNK => 1,
            _ => return Err(fail("part coverage")),
        };
        let decoded = crate::adjudication::proofs::decode_base64(&body.bytes_base64, CHUNK)?;
        let expected = if part == 0 {
            total.min(CHUNK)
        } else {
            total - CHUNK
        };
        if decoded.len() != expected {
            return Err(fail("exact part length"));
        }
        Ok(Self {
            body,
            decoded,
            total,
            part,
        })
    }
    pub fn canonical_value(&self) -> Result<Value> {
        serde_json::to_value(&self.body).map_err(|_| fail("fragment input"))
    }
    pub fn upload_id(&self) -> &str {
        &self.body.upload_id
    }
    pub fn document_schema(&self) -> &str {
        &self.body.document_schema
    }
    pub fn document_sha256(&self) -> &str {
        &self.body.canonical_document_sha256
    }
    pub fn document_bytes(&self) -> usize {
        self.total
    }
    pub fn part(&self) -> usize {
        self.part
    }
    pub fn decoded_bytes(&self) -> &[u8] {
        &self.decoded
    }
    /// Creates data assertions using host-supplied context; not authentication.
    pub fn record_value(&self, store: &str, scope: &[String; 2], source: &str) -> Result<Value> {
        for s in [store, &scope[0], &scope[1]] {
            crate::domain::text(s, 128)?;
        }
        crate::domain::text(source, 256)?;
        let mut value = self.canonical_value()?;
        value["schema"] = json!("ledger-workflow-definition-fragment-record/1");
        value["logical_store_id"] = json!(store);
        value["scope"] = json!(scope);
        value["source"] = json!(source);
        Ok(value)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    logical_store_id: String,
    scope: [String; 2],
    source: String,
    upload_id: String,
    document_schema: String,
    canonical_document_sha256: String,
    document_bytes: String,
    part: String,
    bytes_base64: String,
}
#[derive(Clone, Debug)]
pub struct DefinitionFragment {
    input: DefinitionFragmentInput,
    store: String,
    scope: [String; 2],
    source: String,
    bytes: CanonicalBytes,
    reference: AgreementRef,
}
impl DefinitionFragment {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, MAX_DOCUMENT)?;
        reject_null(&value)?;
        let record: Record = canonical::dto(value)?;
        if record.schema != "ledger-workflow-definition-fragment-record/1" {
            return Err(fail("fragment record schema"));
        }
        let input = DefinitionFragmentInput::from_body(Fragment {
            schema: "ledger-workflow-definition-fragment/1".into(),
            upload_id: record.upload_id,
            document_schema: record.document_schema,
            canonical_document_sha256: record.canonical_document_sha256,
            document_bytes: record.document_bytes,
            part: record.part,
            bytes_base64: record.bytes_base64,
        })?;
        let body = input.record_value(&record.logical_store_id, &record.scope, &record.source)?;
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > MAX_DOCUMENT {
            return Err(fail("normalized fragment byte limit"));
        }
        let digest = super::document_digest("ledger-workflow-definition-fragment-record/1", &body)?;
        let reference = reference(
            &json!({"kind":"workflow-definition-fragment","id":format!("wdoc_{}",&digest[7..]),"digest":digest}),
        )?;
        Ok(Self {
            input,
            store: record.logical_store_id,
            scope: record.scope,
            source: record.source,
            bytes,
            reference,
        })
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.reference
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn input(&self) -> &DefinitionFragmentInput {
        &self.input
    }
}
impl DefinitionFinalizeInput {
    pub fn upload_id(&self) -> &str {
        &self.upload_id
    }
    pub fn fragment_refs(&self) -> &[AgreementRef] {
        &self.fragment_refs
    }
    pub fn canonical_value(&self) -> Value {
        json!({"schema":"ledger-workflow-definition-finalize/1","upload_id":self.upload_id,"fragment_refs":self.fragment_refs})
    }
    /// The coordinator must authenticate each supplied fragment's origin first.
    /// Returned bytes still require typed definition normalization and equality
    /// against that normalized result before they can become a valid source.
    pub fn reassemble(
        &self,
        fragments: &[DefinitionFragment],
        store: &str,
        scope: &[String; 2],
        source: &str,
    ) -> Result<Vec<u8>> {
        if fragments.len() != self.fragment_refs.len() {
            return Err(fail("exact fragment set"));
        }
        let mut refs: Vec<_> = fragments.iter().map(|f| f.reference.clone()).collect();
        canonical::sort_set(&mut refs)?;
        if refs != self.fragment_refs {
            return Err(fail("exact fragment refs"));
        }
        let mut ordered: Vec<_> = fragments.iter().collect();
        ordered.sort_by_key(|f| f.input.part());
        let first = ordered.first().ok_or_else(|| fail("empty fragments"))?;
        let metadata = &first.input;
        if ordered.len() != if metadata.total > CHUNK { 2 } else { 1 } {
            return Err(fail("complete part coverage"));
        }
        let mut bytes = Vec::with_capacity(metadata.total);
        for (part, f) in ordered.iter().enumerate() {
            if f.store != store
                || &f.scope != scope
                || f.source != source
                || f.input.upload_id() != self.upload_id
                || f.input.document_schema() != metadata.document_schema()
                || f.input.document_sha256() != metadata.document_sha256()
                || f.input.total != metadata.total
                || f.input.part() != part
            {
                return Err(fail("upload identity/metadata/coverage"));
            }
            bytes.extend_from_slice(f.input.decoded_bytes());
        }
        if bytes.len() != metadata.total
            || canonical::hex(&Sha256::digest(&bytes)) != metadata.document_sha256()
        {
            return Err(fail("reassembled document bytes/hash"));
        }
        let value = canonical::parse_bounded(&bytes, MAX_DOCUMENT)?;
        reject_null(&value)?;
        if value["schema"].as_str() != Some(metadata.document_schema())
            || CanonicalBytes::from_value(&value)?.as_slice() != bytes
        {
            return Err(fail("canonical source JSON"));
        }
        Ok(bytes)
    }
}
fn reference(v: &Value) -> Result<AgreementRef> {
    AgreementRef::parse(CanonicalBytes::from_value(v)?.as_slice())
}
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn fail(s: &str) -> Error {
    Error::new("DEFINITION_TRANSPORT", s)
}
#[cfg(test)]
mod tests;
