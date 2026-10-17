pub(super) mod source;
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
        let mut canonical = vw::unsigned_string(coefficient as u128, work)?;
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
        Self::parse_with_work(bytes, &mut NoopValidationWork)
    }
    pub fn parse_with_work(bytes: &[u8], work: &mut dyn ValidationWork) -> Result<Self> {
        work.scan_bytes(bytes.len())?;
        Self::from_value_with_work(canonical::parse(bytes)?, work)
    }
    pub(crate) fn from_value_with_work(
        value: Value,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, work, false)
    }
    pub(crate) fn normalization_payload_bound_with_work(
        value: &Value,
        work: &mut dyn ValidationWork,
    ) -> Result<usize> {
        source::bound(value, work)
    }
    pub(crate) fn from_value_complete_with_work(
        value: Value,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, work, true)
    }
    fn from_value_mode(
        value: Value,
        work: &mut dyn ValidationWork,
        complete: bool,
    ) -> Result<Self> {
        if complete {
            source::decode_work(&value, work)?;
        }
        reject_null_with_work(&value, work)?;
        // Serde unit variants can ignore extra fields even with
        // deny_unknown_fields. Enforce their closed object dictionaries first.
        work.visit(Visit::RefLookup)?;
        if let Some(fields) = value.get("fields").and_then(Value::as_array) {
            for field in fields {
                work.visit(Visit::Field)?;
                work.visit(Visit::RefLookup)?;
                closed_variant_with_work(field.get("type"), false, work)?;
                work.visit(Visit::RefLookup)?;
                closed_variant_with_work(field.get("provenance"), true, work)?;
            }
        }
        let mut dto: SchemaDto = serde_json::from_value(value).map_err(|error| {
            if complete {
                Error::new("SCHEMA", error.to_string())
            } else {
                fail("SCHEMA", &error.to_string())
            }
        })?;
        work.visit(Visit::Constraint)?;
        let unsupported = if complete {
            !source::text_cmp(&dto.schema, "ledger-workflow-fact-schema/1", work)?.is_eq()
                || !source::text_cmp(&dto.normalization, "ledger-facts-normalization/1", work)?
                    .is_eq()
        } else {
            dto.schema != "ledger-workflow-fact-schema/1"
                || dto.normalization != "ledger-facts-normalization/1"
        };
        if unsupported {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "fact schema/normalization",
            ));
        }
        name_with_work(&dto.schema_id, work)?;
        let version = counter_with_work(&dto.version, work)?;
        if vw::compare(work, version, 0u64)? == Ordering::Equal {
            return Err(fail("FACT_SCHEMA", "version starts at one"));
        }
        dto.version = vw::unsigned_string(u128::from(version), work)?;
        if vw::compare(work, dto.fields.len(), 64usize)? == Ordering::Greater {
            return Err(fail("FACT_LIMIT", "schema fields"));
        }
        if complete {
            source::sort(&mut dto.fields, work, |a, b, w| {
                source::text_cmp(&a.name, &b.name, w)
            })?;
        } else {
            vw::try_sort_by(&mut dto.fields, work, |a, b, w| {
                member_compare(&a.name, &b.name, w)
            })?;
        }
        let mut names: Vec<String> = if complete {
            Vec::with_capacity(dto.fields.len())
        } else {
            Vec::new()
        };
        let mut fields = Vec::with_capacity(dto.fields.len());
        for field in &mut dto.fields {
            work.visit(Visit::Field)?;
            name_with_work(&field.name, work)?;
            work.visit(Visit::RefLookup)?;
            if let Some(previous) = names.last() {
                let order = if complete {
                    source::text_cmp(previous, &field.name, work)?
                } else {
                    member_compare(previous, &field.name, work)?
                };
                if order == Ordering::Equal {
                    return Err(fail("FACT_SCHEMA", "duplicate field name"));
                }
            }
            if complete {
                work.write_bytes(std::mem::size_of::<String>())?;
            }
            names.push(if complete {
                copy_text(&field.name, work)?
            } else {
                field.name.clone()
            });
            let attested = match &field.provenance {
                ProvenanceDto::Caller => false,
                ProvenanceDto::Attested {
                    source_slot,
                    evidence_kind,
                } => {
                    name_with_work(source_slot, work)?;
                    if !contains_name(
                        &[
                            "occurrence",
                            "outcome",
                            "supplier_final_cost",
                            "meter_quantity",
                            "allocation_input",
                        ],
                        evidence_kind,
                        work,
                    )? {
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
                    let min = counter_with_work(min_bytes, work)?;
                    let max = counter_with_work(max_bytes, work)?;
                    if vw::compare(work, min, max)? == Ordering::Greater
                        || vw::compare(work, max, 1024u64)? == Ordering::Greater
                    {
                        return Err(fail("FACT_SCHEMA", "string byte bounds"));
                    }
                    *min_bytes = vw::unsigned_string(u128::from(min), work)?;
                    *max_bytes = vw::unsigned_string(u128::from(max), work)?;
                    FactType::String {
                        min: min as usize,
                        max: max as usize,
                    }
                }
                TypeDto::Enum { values } => {
                    if vw::compare(work, values.len(), 0usize)? == Ordering::Equal
                        || vw::compare(work, values.len(), 32usize)? == Ordering::Greater
                    {
                        return Err(fail("FACT_LIMIT", "enum values"));
                    }
                    for value in values.iter() {
                        work.visit(Visit::Element)?;
                        name_with_work(value, work)?;
                    }
                    FactType::Enum(if complete {
                        source::enum_set(values, work)?
                    } else {
                        canonical::sort_set_with_work(values, work)?;
                        values.iter().cloned().collect()
                    })
                }
                TypeDto::Integer { min, max, unit } => {
                    number_type_mode(min, max, unit, 0, true, work, complete)?
                }
                TypeDto::Decimal {
                    min,
                    max,
                    max_scale,
                    unit,
                } => {
                    let scale = counter_with_work(max_scale, work)?;
                    if vw::compare(work, scale, 18u64)? == Ordering::Greater {
                        return Err(fail("FACT_SCHEMA", "decimal maximum scale"));
                    }
                    *max_scale = vw::unsigned_string(u128::from(scale), work)?;
                    number_type_mode(min, max, unit, scale as u8, false, work, complete)?
                }
                TypeDto::Timestamp { min, max } => {
                    let low = Timestamp::parse_with_work(min, work)?;
                    let high = Timestamp::parse_with_work(max, work)?;
                    if vw::compare(work, low.micros(), high.micros())? == Ordering::Greater {
                        return Err(fail("FACT_SCHEMA", "timestamp bounds"));
                    }
                    *min = if complete {
                        copy_text(low.as_str(), work)?
                    } else {
                        low.as_str().to_owned()
                    };
                    *max = if complete {
                        copy_text(high.as_str(), work)?
                    } else {
                        high.as_str().to_owned()
                    };
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
                    ));
                }
            };
            if complete {
                work.write_bytes(std::mem::size_of::<Field>())?;
            }
            fields.push(Field {
                name: if complete {
                    copy_text(&field.name, work)?
                } else {
                    field.name.clone()
                },
                required: field.required,
                field_type,
                attested,
            });
        }
        let body = if complete {
            source::body(dto, work)?
        } else {
            serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?
        };
        Ok(Self { body, fields })
    }
    pub(super) fn clone_payload_bound_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<usize> {
        source::clone_payload(self, work)
    }
    pub(super) fn clone_complete_with_work(&self, work: &mut dyn ValidationWork) -> Result<Self> {
        source::clone_schema(self, work)
    }
    pub(super) fn canonical_bytes_complete_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<CanonicalBytes> {
        CanonicalBytes::from_representation_with_work(&self.body, work, canonical::BUNDLE_LIMIT)
    }
    pub(super) fn content_digest_complete_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<String> {
        source::document_digest(self, work)
    }
    pub(super) fn document_id_complete_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<String> {
        let digest = self.content_digest_complete_with_work(work)?;
        work.scan_bytes(64)?;
        work.write_bytes(69)?;
        Ok(format!("wdoc_{}", &digest[7..]))
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    pub fn canonical_bytes(&self) -> Result<CanonicalBytes> {
        self.canonical_bytes_with_work(&mut NoopValidationWork)
    }
    pub fn canonical_bytes_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<CanonicalBytes> {
        CanonicalBytes::from_value_with_work(&self.body, work)
    }
    /// S12 workflow-document framing; legacy domains remain unchanged.
    pub fn content_digest(&self) -> Result<String> {
        self.content_digest_with_work(&mut NoopValidationWork)
    }
    pub fn content_digest_with_work(&self, work: &mut dyn ValidationWork) -> Result<String> {
        super::document_digest_with_work("ledger-workflow-fact-schema/1", &self.body, work)
    }
    pub fn document_id(&self) -> Result<String> {
        self.document_id_with_work(&mut NoopValidationWork)
    }
    pub fn document_id_with_work(&self, work: &mut dyn ValidationWork) -> Result<String> {
        let digest = self.content_digest_with_work(work)?;
        work.write_bytes(69)?;
        Ok(format!("wdoc_{}", &digest[7..]))
    }
    /// Syntax normalization supplies no provenance/authority attestation.
    pub fn normalize(&self, bytes: &[u8]) -> Result<NormalizedFacts> {
        self.normalize_with_work(bytes, &mut NoopValidationWork)
    }
    pub fn normalize_with_work(
        &self,
        bytes: &[u8],
        work: &mut dyn ValidationWork,
    ) -> Result<NormalizedFacts> {
        work.scan_bytes(bytes.len())?;
        self.normalize_value_with_work(&canonical::parse(bytes)?, work)
    }
    pub(crate) fn normalize_value_with_work(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
    ) -> Result<NormalizedFacts> {
        self.normalize_value_mode(value, work, false)
    }
    /// Financial caller pre-admits the exact output/transient arena footprint;
    /// this same normalizer additionally pays executing copies before making
    /// them. This is DATA normalization, not custody or a resource allowance.
    pub(crate) fn normalize_value_complete_with_work(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
    ) -> Result<NormalizedFacts> {
        self.normalize_value_mode(value, work, true)
    }
    /// One claimed attested field uses this SAME immutable field definition.
    /// Unrelated required facts are not invented to normalize a proof subset.
    /// The caller still authenticates source/subject/provenance and pre-admits
    /// the returned scalar/string storage before invoking this DATA function.
    pub(crate) fn normalize_attested_field_complete_with_work(
        &self,
        name: &str,
        value: &Value,
        work: &mut dyn ValidationWork,
    ) -> Result<FactValue> {
        let field = self.field_with_work(name, work)?;
        work.visit(Visit::Constraint)?;
        if !field.attested {
            return Err(fail("FACT_PROVENANCE", "field is not attested"));
        }
        normalize_value_mode(&field.field_type, value, work, true)
    }
    fn normalize_value_mode(
        &self,
        value: &Value,
        work: &mut dyn ValidationWork,
        complete: bool,
    ) -> Result<NormalizedFacts> {
        work.visit(Visit::Value)?;
        let object = value
            .as_object()
            .ok_or_else(|| fail("FACT_TYPE", "facts object required"))?;
        for key in object.keys() {
            work.visit(Visit::Field)?;
            let mut known = false;
            for field in &self.fields {
                if member_compare(&field.name, key, work)? == Ordering::Equal {
                    known = true;
                    break;
                }
            }
            if !known {
                return Err(if complete {
                    Error::new("FACT_UNKNOWN", copy_text(key, work)?)
                } else {
                    fail("FACT_UNKNOWN", key)
                });
            }
        }
        let mut values = BTreeMap::new();
        let mut canonical = serde_json::Map::new();
        let mut required_proofs = Vec::new();
        for field in &self.fields {
            work.visit(Visit::Field)?;
            work.visit(Visit::RefLookup)?;
            let fact = match object.get(&field.name) {
                None if field.required => {
                    return Err(if complete {
                        Error::new("FACT_REQUIRED", copy_text(&field.name, work)?)
                    } else {
                        fail("FACT_REQUIRED", &field.name)
                    });
                }
                None => FactValue::Missing,
                Some(value) => {
                    if field.attested {
                        if complete {
                            grow_proof_fields(&mut required_proofs, work)?;
                            let copied = copy_text(&field.name, work)?;
                            work.write_bytes(std::mem::size_of::<String>())?;
                            required_proofs.push(copied);
                        } else {
                            required_proofs.push(field.name.clone());
                        }
                    }
                    normalize_value_mode(&field.field_type, value, work, complete)?
                }
            };
            let body = if complete {
                fact.canonical_value_complete_with_work(work)?
            } else {
                fact.canonical_value()
            };
            if let Some(value) = body {
                let key = if complete {
                    copy_text(&field.name, work)?
                } else {
                    field.name.clone()
                };
                canonical.insert(key, value);
            }
            let key = if complete {
                copy_text(&field.name, work)?
            } else {
                field.name.clone()
            };
            values.insert(key, fact);
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
        self.normalize_context_with_work(bytes, &mut NoopValidationWork)
    }
    pub fn normalize_context_with_work(
        &self,
        bytes: &[u8],
        work: &mut dyn ValidationWork,
    ) -> Result<NormalizedFacts> {
        self.validate_context_schema_with_work(work)?;
        self.normalize_with_work(bytes, work)
    }
    pub fn validate_context_schema(&self) -> Result<()> {
        self.validate_context_schema_with_work(&mut NoopValidationWork)
    }
    pub fn validate_context_schema_with_work(&self, work: &mut dyn ValidationWork) -> Result<()> {
        for field in &self.fields {
            work.visit(Visit::Field)?;
            work.visit(Visit::Constraint)?;
            if !field.required {
                return Err(fail(
                    "CONTEXT_SCHEMA",
                    "binding context fields must be required caller facts",
                ));
            }
            work.visit(Visit::Constraint)?;
            if field.attested {
                return Err(fail(
                    "CONTEXT_SCHEMA",
                    "binding context fields must be required caller facts",
                ));
            }
        }
        Ok(())
    }
    /// Quantity corrections and formula typechecking use the original schema.
    pub fn quantity_unit(&self, field: &str) -> Result<&str> {
        self.quantity_unit_with_work(field, &mut NoopValidationWork)
    }
    pub fn quantity_unit_with_work(
        &self,
        field: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<&str> {
        let field = self.field_with_work(field, work)?;
        work.visit(Visit::Constraint)?;
        match &field.field_type {
            FactType::Number {
                min,
                unit: Some(unit),
                ..
            } => {
                work.visit(Visit::Constraint)?;
                if field.required && vw::compare(work, min.coefficient, 0)? != Ordering::Less {
                    Ok(unit)
                } else {
                    Err(fail(
                        "QUANTITY_TYPE",
                        "quantity requires required nonnegative numeric field and unit",
                    ))
                }
            }
            _ => Err(fail(
                "QUANTITY_TYPE",
                "quantity requires required nonnegative numeric field and unit",
            )),
        }
    }
    /// Validate a correction against this immutable descriptor, without any
    /// current policy lookup. Runtime authority/lineage checks remain separate.
    pub fn normalize_quantity(&self, field: &str, bytes: &[u8]) -> Result<FactValue> {
        self.normalize_quantity_with_work(field, bytes, &mut NoopValidationWork)
    }
    pub fn normalize_quantity_with_work(
        &self,
        field: &str,
        bytes: &[u8],
        work: &mut dyn ValidationWork,
    ) -> Result<FactValue> {
        self.quantity_unit_with_work(field, work)?;
        let definition = self.field_with_work(field, work)?;
        work.scan_bytes(bytes.len())?;
        normalize_value_with_work(&definition.field_type, &canonical::parse(bytes)?, work)
    }
    fn field_with_work(&self, name: &str, work: &mut dyn ValidationWork) -> Result<&Field> {
        work.visit(Visit::RefLookup)?;
        for field in &self.fields {
            if member_compare(&field.name, name, work)? == Ordering::Equal {
                return Ok(field);
            }
        }
        Err(fail("FACT_UNKNOWN", name))
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
    fn canonical_value_complete_with_work(
        &self,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<Value>> {
        Ok(Some(match self {
            Self::Missing => return Ok(None),
            Self::Boolean(value) => Value::Bool(*value),
            Self::String(value) | Self::Enum(value) => Value::String(copy_text(value, work)?),
            Self::Integer(value) | Self::Decimal(value) => {
                Value::String(copy_text(value.as_str(), work)?)
            }
            Self::Timestamp(value) => Value::String(copy_text(value.as_str(), work)?),
        }))
    }
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
        self.get_with_work(field, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn get_with_work(
        &self,
        field: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&FactValue>> {
        map_lookup(&self.values, field, work)
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    /// Coordinator must authenticate every listed field before evaluation.
    pub fn required_proof_fields(&self) -> &[String] {
        &self.required_proofs
    }
}
fn copy_text(value: &str, work: &mut dyn ValidationWork) -> Result<String> {
    work.scan_bytes(value.len())?;
    work.write_bytes(value.len())?;
    Ok(value.to_owned())
}
fn copy_size(work: &mut dyn ValidationWork, a: usize, b: usize) -> Result<usize> {
    work.arithmetic(Op::Multiply, &[&BigInt::from(a), &BigInt::from(b)])?;
    a.checked_mul(b)
        .ok_or_else(|| fail("FACT_RANGE", "proof vector byte overflow"))
}
fn grow_proof_fields(values: &mut Vec<String>, work: &mut dyn ValidationWork) -> Result<()> {
    if vw::compare(work, values.len(), values.capacity())? != Ordering::Equal {
        return Ok(());
    }
    let next = if vw::compare(work, values.capacity(), 0usize)? == Ordering::Equal {
        4
    } else {
        copy_size(work, values.capacity(), 2)?
    };
    let moved = copy_size(work, values.len(), std::mem::size_of::<String>())?;
    work.scan_bytes(moved)?;
    work.write_bytes(moved)?;
    let additional = vw::usize_sub(work, next, values.len())?;
    values.reserve_exact(additional);
    Ok(())
}
fn normalize_value_with_work(
    kind: &FactType,
    value: &Value,
    work: &mut dyn ValidationWork,
) -> Result<FactValue> {
    normalize_value_mode(kind, value, work, false)
}
fn normalize_value_mode(
    kind: &FactType,
    value: &Value,
    work: &mut dyn ValidationWork,
    complete: bool,
) -> Result<FactValue> {
    work.visit(Visit::Value)?;
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
            if vw::compare(work, s.len(), *min)? == Ordering::Less
                || vw::compare(work, s.len(), *max)? == Ordering::Greater
                || has_control(s, work)?
            {
                return Err(fail("FACT_RANGE", "string bytes/controls"));
            }
            FactValue::String(if complete {
                copy_text(s, work)?
            } else {
                s.to_owned()
            })
        }
        FactType::Enum(values) => {
            let s = text()?;
            work.visit(Visit::RefLookup)?;
            let mut found = false;
            for candidate in values {
                if member_compare(candidate, s, work)? == Ordering::Equal {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(fail("FACT_RANGE", "enum value"));
            }
            FactValue::Enum(if complete {
                copy_text(s, work)?
            } else {
                s.to_owned()
            })
        }
        FactType::Number {
            min,
            max,
            scale,
            integer,
            ..
        } => {
            let raw = text()?;
            work.visit(Visit::Constraint)?;
            if *integer && contains_byte(raw, b'.', work)? {
                return Err(fail("FACT_PRECISION", "integer syntax"));
            }
            let scalar = SignedScalar::parse_with_work(raw, *scale, work)?;
            if scalar.compare_with_work(min, work)? == Ordering::Less
                || scalar.compare_with_work(max, work)? == Ordering::Greater
            {
                return Err(fail("FACT_RANGE", "numeric bounds"));
            }
            if *integer {
                FactValue::Integer(scalar)
            } else {
                FactValue::Decimal(scalar)
            }
        }
        FactType::Time { min, max } => {
            let time = Timestamp::parse_with_work(text()?, work)?;
            if vw::compare(work, time.micros(), min.micros())? == Ordering::Less
                || vw::compare(work, time.micros(), max.micros())? == Ordering::Greater
            {
                return Err(fail("FACT_RANGE", "timestamp bounds"));
            }
            FactValue::Timestamp(time)
        }
    })
}
fn number_type_with_work(
    min: &mut String,
    max: &mut String,
    unit: &Option<String>,
    scale: u8,
    integer: bool,
    work: &mut dyn ValidationWork,
) -> Result<FactType> {
    number_type_mode(min, max, unit, scale, integer, work, false)
}
#[allow(clippy::too_many_arguments)]
fn number_type_mode(
    min: &mut String,
    max: &mut String,
    unit: &Option<String>,
    scale: u8,
    integer: bool,
    work: &mut dyn ValidationWork,
    complete: bool,
) -> Result<FactType> {
    if let Some(unit) = unit {
        name_with_work(unit, work)?;
    }
    work.visit(Visit::Constraint)?;
    if integer && (contains_byte(min, b'.', work)? || contains_byte(max, b'.', work)?) {
        return Err(fail("FACT_SCHEMA", "integer bound syntax"));
    }
    let low = SignedScalar::parse_with_work(min, scale, work)?;
    let high = SignedScalar::parse_with_work(max, scale, work)?;
    if low.compare_with_work(&high, work)? == Ordering::Greater {
        return Err(fail("FACT_SCHEMA", "numeric bounds"));
    }
    *min = if complete {
        copy_text(low.as_str(), work)?
    } else {
        low.as_str().to_owned()
    };
    *max = if complete {
        copy_text(high.as_str(), work)?
    } else {
        high.as_str().to_owned()
    };
    Ok(FactType::Number {
        min: low,
        max: high,
        scale,
        integer,
        unit: if complete {
            unit.as_deref().map(|v| copy_text(v, work)).transpose()?
        } else {
            unit.clone()
        },
    })
}
pub(super) fn counter(raw: &str) -> Result<u64> {
    counter_with_work(raw, &mut NoopValidationWork)
}
pub(super) fn counter_with_work(raw: &str, work: &mut dyn ValidationWork) -> Result<u64> {
    work.visit(Visit::Constraint)?;
    if raw.is_empty() || vw::compare(work, raw.len(), 64usize)? == Ordering::Greater {
        return Err(fail("FACT_SCHEMA", "counter syntax"));
    }
    // Preserve syntax-first errors even if an early numeric prefix overflows.
    for byte in raw.bytes() {
        work.scan_bytes(1)?;
        work.visit(Visit::ScalarDigit)?;
        if !byte.is_ascii_digit() {
            return Err(fail("FACT_SCHEMA", "counter syntax"));
        }
    }
    let mut value = 0u64;
    for byte in raw.bytes() {
        work.scan_bytes(1)?;
        work.visit(Visit::ScalarDigit)?;
        work.arithmetic(Op::Multiply, &[&BigInt::from(value), &BigInt::from(10)])?;
        value = value
            .checked_mul(10)
            .ok_or_else(|| fail("FACT_SCHEMA", "counter bound"))?;
        work.arithmetic(Op::Subtract, &[&BigInt::from(byte), &BigInt::from(b'0')])?;
        let digit = u64::from(byte) - u64::from(b'0');
        work.arithmetic(Op::Add, &[&BigInt::from(value), &BigInt::from(digit)])?;
        value = value
            .checked_add(digit)
            .ok_or_else(|| fail("FACT_SCHEMA", "counter bound"))?;
    }
    if vw::compare(work, value, i64::MAX as u64)? == Ordering::Greater {
        return Err(fail("FACT_SCHEMA", "counter bound"));
    }
    Ok(value)
}
pub(super) fn name(raw: &str) -> Result<()> {
    name_with_work(raw, &mut NoopValidationWork)
}
pub(super) fn name_with_work(raw: &str, work: &mut dyn ValidationWork) -> Result<()> {
    work.visit(Visit::Constraint)?;
    let mut point = None;
    for (i, byte) in raw.bytes().enumerate() {
        work.scan_bytes(1)?;
        work.visit(Visit::MemberCompare)?;
        if byte == b':' {
            point = Some(i);
            break;
        }
    }
    let point = point.ok_or_else(|| fail("FACT_SCHEMA", "qualified name required"))?;
    let after = vw::usize_add(work, point, 1)?;
    let (namespace, local) = (&raw[..point], &raw[after..]);
    fn valid(part: &str, max: usize, w: &mut dyn ValidationWork) -> Result<bool> {
        w.visit(Visit::Constraint)?;
        if part.is_empty() || vw::compare(w, part.len(), max)? == Ordering::Greater {
            return Ok(false);
        }
        w.scan_bytes(1)?;
        w.visit(Visit::Constraint)?;
        if !part.as_bytes()[0].is_ascii_alphabetic() {
            return Ok(false);
        }
        for byte in part.bytes() {
            w.scan_bytes(1)?;
            w.visit(Visit::Constraint)?;
            if !byte.is_ascii_alphanumeric() && !b"_.-".contains(&byte) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    if !valid(namespace, 63, work)? || !valid(local, 64, work)? {
        return Err(fail("FACT_SCHEMA", "qualified name syntax"));
    }
    Ok(())
}
pub(super) fn reject_null(value: &Value) -> Result<()> {
    reject_null_with_work(value, &mut NoopValidationWork)
}
pub(super) fn reject_null_with_work(value: &Value, work: &mut dyn ValidationWork) -> Result<()> {
    work.visit(Visit::Value)?;
    match value {
        Value::Null => return Err(fail("FACT_SCHEMA", "explicit null")),
        Value::Array(values) => {
            for value in values {
                work.visit(Visit::Element)?;
                reject_null_with_work(value, work)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                work.visit(Visit::Field)?;
                reject_null_with_work(value, work)?;
            }
        }
        _ => (),
    }
    Ok(())
}
pub(super) fn member_compare(a: &str, b: &str, work: &mut dyn ValidationWork) -> Result<Ordering> {
    work.visit(Visit::MemberCompare)?;
    Ok(a.cmp(b))
}
pub(super) fn map_lookup<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &str,
    work: &mut dyn ValidationWork,
) -> Result<Option<&'a T>> {
    work.visit(Visit::RefLookup)?;
    for (candidate, value) in map {
        if member_compare(candidate, key, work)? == Ordering::Equal {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn contains_byte(raw: &str, target: u8, work: &mut dyn ValidationWork) -> Result<bool> {
    for byte in raw.bytes() {
        work.scan_bytes(1)?;
        work.visit(Visit::MemberCompare)?;
        if byte == target {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn set_contains(
    names: &BTreeSet<String>,
    key: &str,
    work: &mut dyn ValidationWork,
) -> Result<bool> {
    work.visit(Visit::RefLookup)?;
    for name in names {
        if member_compare(name, key, work)? == Ordering::Equal {
            return Ok(true);
        }
    }
    Ok(false)
}
fn contains_name(names: &[&str], candidate: &str, work: &mut dyn ValidationWork) -> Result<bool> {
    for name in names {
        if member_compare(name, candidate, work)? == Ordering::Equal {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(super) fn has_control(text: &str, work: &mut dyn ValidationWork) -> Result<bool> {
    for character in text.chars() {
        work.scan_bytes(character.len_utf8())?;
        work.visit(Visit::Constraint)?;
        if character.is_control() {
            return Ok(true);
        }
    }
    Ok(false)
}
fn closed_variant_with_work(
    value: Option<&Value>,
    provenance: bool,
    work: &mut dyn ValidationWork,
) -> Result<()> {
    work.visit(Visit::Constraint)?;
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| fail("FACT_SCHEMA", "tagged object required"))?;
    work.visit(Visit::RefLookup)?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("FACT_SCHEMA", "kind required"))?;
    work.visit(Visit::Constraint)?;
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
    for key in object.keys() {
        work.visit(Visit::Field)?;
        if !contains_name(allowed, key, work)? {
            return Err(fail("FACT_SCHEMA", "unknown tagged object field"));
        }
    }
    Ok(())
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}

#[cfg(test)]
mod tests;
