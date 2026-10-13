use crate::canonical::{self, CanonicalBytes};
use crate::domain::Timestamp;
use crate::money::{Decimal, ExactRatio};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Exact signed fact value. It cannot be used as a nonnegative price implicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedScalar {
    coefficient: i128,
    scale: u8,
    canonical: String,
}
impl SignedScalar {
    fn parse(raw: &str, max_scale: u8) -> Result<Self> {
        if raw.is_empty() || raw.len() > 64 {
            return Err(fail("FACT_PRECISION", "scalar byte bound"));
        }
        let negative = raw.starts_with('-');
        let magnitude = raw.strip_prefix('-').unwrap_or(raw);
        let (whole, fraction) = magnitude.split_once('.').unwrap_or((magnitude, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > usize::from(max_scale)
            || (magnitude.contains('.') && fraction.is_empty())
        {
            return Err(fail("FACT_PRECISION", "signed scalar syntax/scale"));
        }
        // The declared raw fractional scale is checked before normalization;
        // the 30-digit coefficient bound applies to the normalized coefficient.
        // This preserves the legacy exact scalar magnitude convention.
        let decimal =
            Decimal::parse(magnitude).map_err(|error| fail("FACT_PRECISION", &error.detail))?;
        if negative && decimal.is_zero() {
            return Err(fail("FACT_PRECISION", "negative zero"));
        }
        let canonical = format!("{}{}", if negative { "-" } else { "" }, decimal);
        let (whole, fraction) = canonical.split_once('.').unwrap_or((&canonical, ""));
        let coefficient = format!("{whole}{fraction}")
            .parse()
            .map_err(|_| fail("FACT_PRECISION", "scalar coefficient"))?;
        Ok(Self {
            coefficient,
            scale: fraction.len() as u8,
            canonical,
        })
    }
    pub fn as_str(&self) -> &str {
        &self.canonical
    }
    pub fn ratio(&self) -> ExactRatio {
        // A fact has at most 30 coefficient digits and 18 fractional digits.
        ExactRatio::new(
            BigInt::from(self.coefficient),
            BigInt::from(10u128.pow(u32::from(self.scale))),
        )
        .expect("validated bounded scalar")
    }
}
impl Ord for SignedScalar {
    fn cmp(&self, other: &Self) -> Ordering {
        let left =
            BigInt::from(self.coefficient) * BigInt::from(10u128.pow(u32::from(other.scale)));
        let right =
            BigInt::from(other.coefficient) * BigInt::from(10u128.pow(u32::from(self.scale)));
        left.cmp(&right)
    }
}
impl PartialOrd for SignedScalar {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Serialize for SignedScalar {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.canonical)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaDto {
    schema: String,
    schema_id: String,
    version: String,
    normalization: String,
    fields: Vec<FieldDto>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDto {
    name: String,
    required: bool,
    #[serde(rename = "type")]
    field_type: TypeDto,
    provenance: ProvenanceDto,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TypeDto {
    Boolean,
    String {
        min_bytes: String,
        max_bytes: String,
    },
    Enum {
        values: Vec<String>,
    },
    Integer {
        min: String,
        max: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Decimal {
        min: String,
        max: String,
        max_scale: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
    Timestamp {
        min: String,
        max: String,
    },
    Reference {
        record_kind: String,
        relation: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ProvenanceDto {
    Caller,
    Attested {
        source_slot: String,
        evidence_kind: String,
    },
}
#[derive(Clone, Debug)]
enum FactType {
    Boolean,
    String {
        min: usize,
        max: usize,
    },
    Enum(BTreeSet<String>),
    Number {
        min: SignedScalar,
        max: SignedScalar,
        scale: u8,
        integer: bool,
        unit: Option<String>,
    },
    Time {
        min: Timestamp,
        max: Timestamp,
    },
}
#[derive(Clone, Debug)]
struct Field {
    name: String,
    required: bool,
    field_type: FactType,
    attested: bool,
}

/// Schema constructed only after closed-shape and semantic validation.
#[derive(Clone, Debug)]
pub struct FactSchema {
    body: Value,
    fields: Vec<Field>,
}
impl FactSchema {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = canonical::parse(bytes)?;
        reject_null(&value)?;
        // Serde unit variants can ignore extra fields even with
        // deny_unknown_fields. Enforce their closed object dictionaries first.
        if let Some(fields) = value.get("fields").and_then(Value::as_array) {
            for field in fields {
                closed_variant(field.get("type"), false)?;
                closed_variant(field.get("provenance"), true)?;
            }
        }
        let mut dto: SchemaDto = canonical::dto(value)?;
        if dto.schema != "ledger-workflow-fact-schema/1"
            || dto.normalization != "ledger-facts-normalization/1"
        {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "fact schema/normalization",
            ));
        }
        name(&dto.schema_id)?;
        let version = counter(&dto.version)?;
        if version == 0 {
            return Err(fail("FACT_SCHEMA", "version starts at one"));
        }
        dto.version = version.to_string();
        if dto.fields.len() > 64 {
            return Err(fail("FACT_LIMIT", "schema fields"));
        }
        dto.fields.sort_by(|a, b| a.name.cmp(&b.name));
        let mut names = BTreeSet::new();
        let mut fields = Vec::with_capacity(dto.fields.len());
        for field in &mut dto.fields {
            name(&field.name)?;
            if !names.insert(field.name.clone()) {
                return Err(fail("FACT_SCHEMA", "duplicate field name"));
            }
            let attested = match &field.provenance {
                ProvenanceDto::Caller => false,
                ProvenanceDto::Attested {
                    source_slot,
                    evidence_kind,
                } => {
                    name(source_slot)?;
                    if ![
                        "occurrence",
                        "outcome",
                        "supplier_final_cost",
                        "meter_quantity",
                        "allocation_input",
                    ]
                    .contains(&evidence_kind.as_str())
                    {
                        return Err(fail("FACT_SCHEMA", "unsupported evidence kind"));
                    }
                    true
                }
            };
            let field_type = match &mut field.field_type {
                TypeDto::Boolean => FactType::Boolean,
                TypeDto::String {
                    min_bytes,
                    max_bytes,
                } => {
                    let min = counter(min_bytes)?;
                    let max = counter(max_bytes)?;
                    if min > max || max > 1024 {
                        return Err(fail("FACT_SCHEMA", "string byte bounds"));
                    }
                    *min_bytes = min.to_string();
                    *max_bytes = max.to_string();
                    FactType::String {
                        min: min as usize,
                        max: max as usize,
                    }
                }
                TypeDto::Enum { values } => {
                    if values.is_empty() || values.len() > 32 {
                        return Err(fail("FACT_LIMIT", "enum values"));
                    }
                    for value in values.iter() {
                        name(value)?;
                    }
                    canonical::sort_set(values)?;
                    FactType::Enum(values.iter().cloned().collect())
                }
                TypeDto::Integer { min, max, unit } => number_type(min, max, unit, 0, true)?,
                TypeDto::Decimal {
                    min,
                    max,
                    max_scale,
                    unit,
                } => {
                    let scale = counter(max_scale)?;
                    if scale > 18 {
                        return Err(fail("FACT_SCHEMA", "decimal maximum scale"));
                    }
                    *max_scale = scale.to_string();
                    number_type(min, max, unit, scale as u8, false)?
                }
                TypeDto::Timestamp { min, max } => {
                    let low = Timestamp::parse(min)?;
                    let high = Timestamp::parse(max)?;
                    if low.micros() > high.micros() {
                        return Err(fail("FACT_SCHEMA", "timestamp bounds"));
                    }
                    *min = low.as_str().to_owned();
                    *max = high.as_str().to_owned();
                    FactType::Time {
                        min: low,
                        max: high,
                    }
                }
                // Closed record-kind registry and trusted proof resolution are
                // separate dependencies; shape alone must not authenticate refs.
                TypeDto::Reference { .. } => {
                    return Err(fail(
                        "FACT_UNSUPPORTED_TYPE",
                        "reference descriptors not implemented",
                    ))
                }
            };
            fields.push(Field {
                name: field.name.clone(),
                required: field.required,
                field_type,
                attested,
            });
        }
        let body = serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?;
        Ok(Self { body, fields })
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    pub fn canonical_bytes(&self) -> Result<CanonicalBytes> {
        CanonicalBytes::from_value(&self.body)
    }
    /// S12 workflow-document framing; legacy domains remain unchanged.
    pub fn content_digest(&self) -> Result<String> {
        let mut hash = Sha256::new();
        hash.update(b"bean-counter/workflow-document/1\0");
        hash.update(
            CanonicalBytes::from_value(&serde_json::json!([
                "ledger-workflow-fact-schema/1",
                self.body
            ]))?
            .as_slice(),
        );
        Ok(format!("sha256:{}", canonical::hex(&hash.finalize())))
    }
    pub fn document_id(&self) -> Result<String> {
        Ok(format!("wdoc_{}", &self.content_digest()?[7..]))
    }
    /// Syntax normalization supplies no provenance/authority attestation.
    pub fn normalize(&self, bytes: &[u8]) -> Result<NormalizedFacts> {
        let value = canonical::parse(bytes)?;
        let object = value
            .as_object()
            .ok_or_else(|| fail("FACT_TYPE", "facts object required"))?;
        for key in object.keys() {
            if !self.fields.iter().any(|field| &field.name == key) {
                return Err(fail("FACT_UNKNOWN", key));
            }
        }
        let mut values = BTreeMap::new();
        let mut canonical = serde_json::Map::new();
        let mut required_proofs = Vec::new();
        for field in &self.fields {
            let fact = match object.get(&field.name) {
                None if field.required => return Err(fail("FACT_REQUIRED", &field.name)),
                None => FactValue::Missing,
                Some(value) => {
                    if field.attested {
                        required_proofs.push(field.name.clone());
                    }
                    normalize_value(&field.field_type, value)?
                }
            };
            if let Some(value) = fact.canonical_value() {
                canonical.insert(field.name.clone(), value);
            }
            values.insert(field.name.clone(), fact);
        }
        Ok(NormalizedFacts {
            values,
            body: Value::Object(canonical),
            required_proofs,
        })
    }
    /// Quantity corrections and formula typechecking use the original schema.
    pub fn quantity_unit(&self, field: &str) -> Result<&str> {
        let field = self
            .fields
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| fail("FACT_UNKNOWN", field))?;
        match &field.field_type {
            FactType::Number {
                min,
                unit: Some(unit),
                ..
            } if field.required && min.coefficient >= 0 => Ok(unit),
            _ => Err(fail(
                "QUANTITY_TYPE",
                "quantity requires required nonnegative numeric field and unit",
            )),
        }
    }
    /// Validate a correction against this immutable descriptor, without any
    /// current policy lookup. Runtime authority/lineage checks remain separate.
    pub fn normalize_quantity(&self, field: &str, bytes: &[u8]) -> Result<FactValue> {
        self.quantity_unit(field)?;
        let definition = self
            .fields
            .iter()
            .find(|f| f.name == field)
            .expect("validated field");
        normalize_value(&definition.field_type, &canonical::parse(bytes)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactValue {
    Missing,
    Boolean(bool),
    String(String),
    Enum(String),
    Integer(SignedScalar),
    Decimal(SignedScalar),
    Timestamp(Timestamp),
}
impl FactValue {
    fn canonical_value(&self) -> Option<Value> {
        Some(match self {
            Self::Missing => return None,
            Self::Boolean(value) => Value::Bool(*value),
            Self::String(value) | Self::Enum(value) => Value::String(value.clone()),
            Self::Integer(value) | Self::Decimal(value) => Value::String(value.as_str().to_owned()),
            Self::Timestamp(value) => Value::String(value.as_str().to_owned()),
        })
    }
}
#[derive(Clone, Debug)]
pub struct NormalizedFacts {
    values: BTreeMap<String, FactValue>,
    body: Value,
    required_proofs: Vec<String>,
}
impl NormalizedFacts {
    pub fn get(&self, field: &str) -> Option<&FactValue> {
        self.values.get(field)
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    /// Coordinator must authenticate every listed field before evaluation.
    pub fn required_proof_fields(&self) -> &[String] {
        &self.required_proofs
    }
}
fn normalize_value(kind: &FactType, value: &Value) -> Result<FactValue> {
    let text = || {
        value
            .as_str()
            .ok_or_else(|| fail("FACT_TYPE", "string scalar required"))
    };
    Ok(match kind {
        FactType::Boolean => FactValue::Boolean(
            value
                .as_bool()
                .ok_or_else(|| fail("FACT_TYPE", "boolean required"))?,
        ),
        FactType::String { min, max } => {
            let s = text()?;
            if s.len() < *min || s.len() > *max || s.chars().any(char::is_control) {
                return Err(fail("FACT_RANGE", "string bytes/controls"));
            }
            FactValue::String(s.to_owned())
        }
        FactType::Enum(values) => {
            let s = text()?;
            if !values.contains(s) {
                return Err(fail("FACT_RANGE", "enum value"));
            }
            FactValue::Enum(s.to_owned())
        }
        FactType::Number {
            min,
            max,
            scale,
            integer,
            ..
        } => {
            let raw = text()?;
            if *integer && raw.contains('.') {
                return Err(fail("FACT_PRECISION", "integer syntax"));
            }
            let scalar = SignedScalar::parse(raw, *scale)?;
            if &scalar < min || &scalar > max {
                return Err(fail("FACT_RANGE", "numeric bounds"));
            }
            if *integer {
                FactValue::Integer(scalar)
            } else {
                FactValue::Decimal(scalar)
            }
        }
        FactType::Time { min, max } => {
            let time = Timestamp::parse(text()?)?;
            if time.micros() < min.micros() || time.micros() > max.micros() {
                return Err(fail("FACT_RANGE", "timestamp bounds"));
            }
            FactValue::Timestamp(time)
        }
    })
}
fn number_type(
    min: &mut String,
    max: &mut String,
    unit: &Option<String>,
    scale: u8,
    integer: bool,
) -> Result<FactType> {
    if let Some(unit) = unit {
        name(unit)?;
    }
    if integer && (min.contains('.') || max.contains('.')) {
        return Err(fail("FACT_SCHEMA", "integer bound syntax"));
    }
    let low = SignedScalar::parse(min, scale)?;
    let high = SignedScalar::parse(max, scale)?;
    if low > high {
        return Err(fail("FACT_SCHEMA", "numeric bounds"));
    }
    *min = low.as_str().to_owned();
    *max = high.as_str().to_owned();
    Ok(FactType::Number {
        min: low,
        max: high,
        scale,
        integer,
        unit: unit.clone(),
    })
}
fn counter(raw: &str) -> Result<u64> {
    if raw.is_empty() || raw.len() > 64 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fail("FACT_SCHEMA", "counter syntax"));
    }
    let canonical = raw.trim_start_matches('0');
    let value: u64 = if canonical.is_empty() {
        0
    } else {
        canonical
            .parse()
            .map_err(|_| fail("FACT_SCHEMA", "counter bound"))?
    };
    if value > i64::MAX as u64 {
        return Err(fail("FACT_SCHEMA", "counter bound"));
    }
    Ok(value)
}
fn name(raw: &str) -> Result<()> {
    let (namespace, local) = raw
        .split_once(':')
        .ok_or_else(|| fail("FACT_SCHEMA", "qualified name required"))?;
    let valid = |part: &str, max: usize| {
        !part.is_empty()
            && part.len() <= max
            && part.as_bytes()[0].is_ascii_alphabetic()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    };
    if !valid(namespace, 63) || !valid(local, 64) {
        return Err(fail("FACT_SCHEMA", "qualified name syntax"));
    }
    Ok(())
}
fn reject_null(value: &Value) -> Result<()> {
    match value {
        Value::Null => return Err(fail("FACT_SCHEMA", "explicit null")),
        Value::Array(values) => {
            for value in values {
                reject_null(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                reject_null(value)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn closed_variant(value: Option<&Value>, provenance: bool) -> Result<()> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| fail("FACT_SCHEMA", "tagged object required"))?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("FACT_SCHEMA", "kind required"))?;
    let allowed: &[&str] = if provenance {
        match kind {
            "caller" => &["kind"],
            "attested" => &["kind", "source_slot", "evidence_kind"],
            _ => return Err(fail("FACT_SCHEMA", "unsupported provenance")),
        }
    } else {
        match kind {
            "boolean" => &["kind"],
            "string" => &["kind", "min_bytes", "max_bytes"],
            "enum" => &["kind", "values"],
            "integer" => &["kind", "min", "max", "unit"],
            "decimal" => &["kind", "min", "max", "max_scale", "unit"],
            "timestamp" => &["kind", "min", "max"],
            "reference" => &["kind", "record_kind", "relation"],
            _ => return Err(fail("FACT_UNSUPPORTED_TYPE", "unsupported scalar type")),
        }
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(fail("FACT_SCHEMA", "unknown tagged object field"));
    }
    Ok(())
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}

#[cfg(test)]
mod tests;
