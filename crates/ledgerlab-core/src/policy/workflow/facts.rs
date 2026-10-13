use crate::canonical::{self, CanonicalBytes};
use crate::domain::Timestamp;
use crate::money::ExactRatio;
use crate::validation_work::{
    self as vw, ArithmeticOperation as Op, NoopValidationWork, ValidationVisit as Visit,
    ValidationWork,
};
use crate::{Error, Result};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub fn parse(raw: &str, max_scale: u8) -> Result<Self> {
        Self::parse_with_work(raw, max_scale, &mut NoopValidationWork)
    }
    pub fn parse_with_work(
        raw: &str,
        max_scale: u8,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        work.visit(Visit::Constraint)?;
        if raw.is_empty() || vw::compare(work, raw.len(), 64usize)? == Ordering::Greater {
            return Err(fail("FACT_PRECISION", "scalar byte bound"));
        }
        work.visit(Visit::Constraint)?;
        work.scan_bytes(1)?;
        let negative = raw.starts_with('-');
        let magnitude = raw.strip_prefix('-').unwrap_or(raw);
        let mut point = None;
        for (i, byte) in magnitude.bytes().enumerate() {
            work.scan_bytes(1)?;
            work.visit(Visit::MemberCompare)?;
            if byte == b'.' {
                point = Some(i);
                break;
            }
        }
        let (whole, fraction) = if let Some(i) = point {
            let after = vw::usize_add(work, i, 1)?;
            (&magnitude[..i], &magnitude[after..])
        } else {
            (magnitude, "")
        };
        work.visit(Visit::Constraint)?;
        if whole.is_empty() {
            return Err(fail("FACT_PRECISION", "signed scalar syntax/scale"));
        }
        for part in [whole, fraction] {
            for digit in part.bytes() {
                work.scan_bytes(1)?;
                work.visit(Visit::ScalarDigit)?;
                if !digit.is_ascii_digit() {
                    return Err(fail("FACT_PRECISION", "signed scalar syntax/scale"));
                }
            }
        }
        if vw::compare(work, fraction.len(), usize::from(max_scale))? == Ordering::Greater {
            return Err(fail("FACT_PRECISION", "signed scalar syntax/scale"));
        }
        work.visit(Visit::Constraint)?;
        if point.is_some() && fraction.is_empty() {
            return Err(fail("FACT_PRECISION", "signed scalar syntax/scale"));
        }
        if vw::compare(work, fraction.len(), 18usize)? == Ordering::Greater {
            return Err(fail("FACT_PRECISION", "nonnegative decimal required"));
        }
        // Apply the same normalized-coefficient convention as Decimal::parse,
        // directly on this decoded scalar, without a second opaque parse.
        let mut fraction_end = fraction.len();
        while vw::compare(work, fraction_end, 0usize)? == Ordering::Greater {
            work.scan_bytes(1)?;
            work.visit(Visit::MemberCompare)?;
            let prior = vw::usize_sub(work, fraction_end, 1)?;
            if fraction.as_bytes()[prior] != b'0' {
                break;
            }
            fraction_end = prior;
        }
        let fraction = &fraction[..fraction_end];
        let digit_bytes = vw::usize_add(work, whole.len(), fraction.len())?;
        work.write_bytes(digit_bytes)?;
        let digits = format!("{whole}{fraction}");
        let mut first = 0;
        for byte in digits.bytes() {
            work.scan_bytes(1)?;
            work.visit(Visit::MemberCompare)?;
            if byte != b'0' {
                break;
            }
            first = vw::usize_add(work, first, 1)?;
        }
        let digits = &digits[first..];
        if vw::compare(work, digits.len(), 30usize)? == Ordering::Greater {
            return Err(fail("FACT_PRECISION", "decimal coefficient"));
        }
        let mut coefficient = 0i128;
        for byte in digits.bytes() {
            work.scan_bytes(1)?;
            work.visit(Visit::ScalarDigit)?;
            work.arithmetic(
                Op::Multiply,
                &[&BigInt::from(coefficient), &BigInt::from(10)],
            )?;
            coefficient *= 10;
            work.arithmetic(Op::Subtract, &[&BigInt::from(byte), &BigInt::from(b'0')])?;
            let digit = i128::from(byte) - i128::from(b'0');
            work.arithmetic(Op::Add, &[&BigInt::from(coefficient), &BigInt::from(digit)])?;
            coefficient += digit;
        }
        let zero = vw::compare(work, coefficient, 0)? == Ordering::Equal;
        if negative && zero {
            return Err(fail("FACT_PRECISION", "negative zero"));
        }
        let scale = if zero { 0 } else { fraction.len() as u8 };
        work.write_bytes(if zero { 1 } else { digits.len() })?;
        let mut canonical = coefficient.to_string();
        if vw::compare(work, scale, 0u8)? == Ordering::Greater {
            let scale = usize::from(scale);
            if vw::compare(work, canonical.len(), scale)? != Ordering::Greater {
                let width = vw::usize_add(work, scale, 1)?;
                let padding = vw::usize_sub(work, width, canonical.len())?;
                work.write_bytes(padding)?;
                let zeros = "0".repeat(padding);
                work.write_bytes(width)?;
                canonical = format!("{zeros}{canonical}");
            }
            let point = vw::usize_sub(work, canonical.len(), scale)?;
            let width = vw::usize_add(work, canonical.len(), 1)?;
            work.write_bytes(width)?;
            canonical.insert(point, '.');
        }
        if negative {
            work.arithmetic(Op::Negate, &[&BigInt::from(coefficient)])?;
            coefficient = -coefficient;
            let width = vw::usize_add(work, canonical.len(), 1)?;
            work.write_bytes(width)?;
            canonical.insert(0, '-');
        }
        Ok(Self {
            coefficient,
            scale,
            canonical,
        })
    }
    pub fn compare_with_work(
        &self,
        other: &Self,
        work: &mut dyn ValidationWork,
    ) -> Result<Ordering> {
        let left_factor = vw::power_ten(work, other.scale)?;
        let right_factor = vw::power_ten(work, self.scale)?;
        let left_coefficient = BigInt::from(self.coefficient);
        let right_coefficient = BigInt::from(other.coefficient);
        work.arithmetic(Op::Multiply, &[&left_coefficient, &left_factor])?;
        let left = left_coefficient * left_factor;
        work.arithmetic(Op::Multiply, &[&right_coefficient, &right_factor])?;
        let right = right_coefficient * right_factor;
        work.arithmetic(Op::Compare, &[&left, &right])?;
        Ok(left.cmp(&right))
    }
    pub fn as_str(&self) -> &str {
        &self.canonical
    }
    pub fn ratio(&self) -> ExactRatio {
        self.ratio_with_work(&mut NoopValidationWork)
            .expect("validated bounded scalar; Noop work cannot fail")
    }
    pub fn ratio_with_work(&self, work: &mut dyn ValidationWork) -> Result<ExactRatio> {
        let denominator = vw::power_ten(work, self.scale)?;
        ExactRatio::new_with_work(BigInt::from(self.coefficient), denominator, work)
    }
}
impl Ord for SignedScalar {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_with_work(other, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
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
        super::document_digest("ledger-workflow-fact-schema/1", &self.body)
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
    /// Binding context syntax uses only required caller facts. This does not
    /// establish assent, bind a customer or select an accepted policy.
    pub fn normalize_context(&self, bytes: &[u8]) -> Result<NormalizedFacts> {
        self.validate_context_schema()?;
        self.normalize(bytes)
    }
    pub fn validate_context_schema(&self) -> Result<()> {
        if self
            .fields
            .iter()
            .any(|field| !field.required || field.attested)
        {
            return Err(fail(
                "CONTEXT_SCHEMA",
                "binding context fields must be required caller facts",
            ));
        }
        Ok(())
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
pub(super) fn counter(raw: &str) -> Result<u64> {
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
pub(super) fn name(raw: &str) -> Result<()> {
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
pub(super) fn reject_null(value: &Value) -> Result<()> {
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
