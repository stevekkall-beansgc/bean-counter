//! Closed reusable activity/unit/link declarations. These definitions supply
//! no source rights, customer bindings, event links or economic authorization.
mod source;
use super::facts::{
    counter_with_work, has_control, map_lookup, member_compare, name_with_work, reject_null,
    reject_null_with_work,
};
use super::FactSchema;
use crate::canonical::{self, CanonicalBytes};
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

const SCHEMA: &str = "ledger-workflow-product/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    Work,
    Outcome,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingRelation {
    SameBinding,
    SameAgreement,
    DeclaredSupplier,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingDependency {
    Refuse,
    Wait,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}
impl Labels {
    fn validate_with_work(&self, work: &mut dyn ValidationWork) -> Result<()> {
        fn text(s: &str, max: usize, work: &mut dyn ValidationWork) -> Result<()> {
            work.visit(Visit::Constraint)?;
            if s.is_empty()
                || vw::compare(work, s.len(), max)? == Ordering::Greater
                || has_control(s, work)?
            {
                return Err(Error::new(
                    "IDENTIFIER",
                    "invalid UTF-8 byte length or control character",
                ));
            }
            Ok(())
        }
        text(&self.title, 128, work)?;
        if let Some(description) = &self.description {
            text(description, 1024, work)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: String,
    id: String,
    digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitNormalization {
    to: String,
    factor: ExactRatio,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitDto {
    name: String,
    labels: Labels,
    dimension: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normalization: Option<UnitNormalization>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityDto {
    name: String,
    labels: Labels,
    primitive: Primitive,
    fact_schema: Reference,
    source_slot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    funding_leg: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkDto {
    name: String,
    child_activity: String,
    predecessor_activities: Vec<String>,
    child_primitive: Primitive,
    predecessor_primitive: Primitive,
    minimum: String,
    maximum: String,
    binding_relation: BindingRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supplier_slot: Option<String>,
    on_missing: MissingDependency,
    max_depth: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductDto {
    schema: String,
    product_id: String,
    version: String,
    labels: Labels,
    profile: String,
    evaluator: String,
    grammar: String,
    limits_profile: String,
    units: Vec<UnitDto>,
    activities: Vec<ActivityDto>,
    fact_schemas: Vec<Reference>,
    links: Vec<LinkDto>,
}

#[derive(Clone, Debug)]
pub struct UnitDefinition(UnitDto);
impl UnitDefinition {
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn dimension(&self) -> &str {
        &self.0.dimension
    }
    pub fn normalization(&self) -> Option<(&str, &ExactRatio)> {
        self.0
            .normalization
            .as_ref()
            .map(|n| (n.to.as_str(), &n.factor))
    }
}
#[derive(Clone, Debug)]
pub struct Activity(ActivityDto);
impl Activity {
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn primitive(&self) -> Primitive {
        self.0.primitive
    }
    pub fn fact_schema_id(&self) -> &str {
        &self.0.fact_schema.id
    }
    pub fn fact_schema_digest(&self) -> &str {
        &self.0.fact_schema.digest
    }
    pub fn source_slot(&self) -> &str {
        &self.0.source_slot
    }
    pub fn funding_leg(&self) -> Option<&str> {
        self.0.funding_leg.as_deref()
    }
}
#[derive(Clone, Debug)]
pub struct LinkDefinition {
    dto: LinkDto,
    minimum: u8,
    maximum: u8,
    depth: u8,
}
impl LinkDefinition {
    pub fn name(&self) -> &str {
        &self.dto.name
    }
    pub fn child_activity(&self) -> &str {
        &self.dto.child_activity
    }
    pub fn predecessor_activities(&self) -> &[String] {
        &self.dto.predecessor_activities
    }
    pub fn child_primitive(&self) -> Primitive {
        self.dto.child_primitive
    }
    pub fn predecessor_primitive(&self) -> Primitive {
        self.dto.predecessor_primitive
    }
    pub fn minimum(&self) -> u8 {
        self.minimum
    }
    pub fn maximum(&self) -> u8 {
        self.maximum
    }
    pub fn maximum_depth(&self) -> u8 {
        self.depth
    }
    pub fn binding_relation(&self) -> BindingRelation {
        self.dto.binding_relation
    }
    pub fn supplier_slot(&self) -> Option<&str> {
        self.dto.supplier_slot.as_deref()
    }
    pub fn on_missing(&self) -> MissingDependency {
        self.dto.on_missing
    }
}

#[derive(Clone, Debug)]
pub struct Product {
    body: Value,
    product_id: String,
    version: u64,
    units: BTreeMap<String, UnitDefinition>,
    activities: BTreeMap<String, Activity>,
    links: BTreeMap<String, LinkDefinition>,
    schemas: BTreeMap<String, FactSchema>,
}
/// Closed syntax for bounded dependency discovery only. This does not establish
/// a normalized Product or authenticate any referenced schema.
pub(super) fn candidate_fact_schema_refs(raw: &[u8]) -> Result<Vec<super::DefinitionRef>> {
    let value = canonical::parse_bounded(raw, 262144)?;
    reject_null(&value)?;
    let dto: ProductDto = canonical::dto(value)?;
    if dto.schema != SCHEMA || dto.fact_schemas.is_empty() || dto.fact_schemas.len() > 16 {
        return Err(fail("PRODUCT_SCHEMA", "candidate schema/dependency count"));
    }
    let mut refs = Vec::new();
    for r in dto.fact_schemas {
        if r.kind != "workflow-fact-schema" {
            return Err(fail("PRODUCT_SCHEMA", "candidate dependency family"));
        }
        refs.push(super::DefinitionRef::new(&r.kind, &r.id, &r.digest)?);
    }
    canonical::sort_set(&mut refs)?;
    Ok(refs)
}
impl Product {
    /// Resolve definitions only from supplied validated immutable schemas.
    /// Source-closure admission and current authority remain compiler/coordinator
    /// obligations; this operation cannot fetch a URL or select a mutable head.
    pub fn parse(bytes: &[u8], supplied_schemas: &[FactSchema]) -> Result<Self> {
        Self::parse_with_work(bytes, supplied_schemas, &mut NoopValidationWork)
    }
    /// The selected compiler rejects unit conversion before this decoded path:
    /// legacy ratio serde is retained for compatibility, not a metered domain.
    pub fn parse_with_work(
        bytes: &[u8],
        supplied_schemas: &[FactSchema],
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        work.scan_bytes(bytes.len())?;
        Self::from_value_with_work(canonical::parse(bytes)?, supplied_schemas, work)
    }
    pub(crate) fn from_value_with_work(
        value: Value,
        supplied_schemas: &[FactSchema],
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, supplied_schemas, work, false)
    }
    /// SAME selected terminal-unit precondition as the owning compiler. The
    /// general legacy source entry remains unchanged and supports its original
    /// unit-conversion DTO; that ratio serde is not an RP01 execution recipe.
    pub(crate) fn normalization_payload_bound_with_work(
        value: &Value,
        supplied_schemas: &[FactSchema],
        work: &mut dyn ValidationWork,
    ) -> Result<usize> {
        source::bound(value, supplied_schemas, work)
    }
    pub(crate) fn from_value_complete_with_work(
        value: Value,
        supplied_schemas: &[FactSchema],
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, supplied_schemas, work, true)
    }
    fn from_value_mode(
        value: Value,
        supplied_schemas: &[FactSchema],
        work: &mut dyn ValidationWork,
        complete: bool,
    ) -> Result<Self> {
        if complete {
            source::terminal(&value, work)?;
            source::decode(&value, work)?;
        }
        reject_null_with_work(&value, work)?;
        let mut dto: ProductDto = serde_json::from_value(value).map_err(|error| {
            if complete {
                Error::new("SCHEMA", error.to_string())
            } else {
                fail("SCHEMA", &error.to_string())
            }
        })?;
        if source::compare(&dto.schema, SCHEMA, work, complete)? != Ordering::Equal
            || source::compare(&dto.profile, "workflow/1", work, complete)? != Ordering::Equal
            || source::compare(
                &dto.evaluator,
                "ledger-workflow-evaluator/1",
                work,
                complete,
            )? != Ordering::Equal
            || source::compare(&dto.grammar, "ledger-policy-grammar/1", work, complete)?
                != Ordering::Equal
            || source::compare(&dto.limits_profile, "workflow-limits/1", work, complete)?
                != Ordering::Equal
        {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "product profiles"));
        }
        name_with_work(&dto.product_id, work)?;
        dto.labels.validate_with_work(work)?;
        let version = counter_with_work(&dto.version, work)?;
        if vw::compare(work, version, 0u64)? == Ordering::Equal {
            return Err(fail("PRODUCT_VERSION", "version starts at one"));
        }
        dto.version = vw::unsigned_string(u128::from(version), work)?;
        work.visit(Visit::Constraint)?;
        if (vw::compare(work, dto.units.len(), 1usize)? == Ordering::Less
            || vw::compare(work, dto.units.len(), 16usize)? == Ordering::Greater)
            || (vw::compare(work, dto.activities.len(), 1usize)? == Ordering::Less
                || vw::compare(work, dto.activities.len(), 16usize)? == Ordering::Greater)
            || (vw::compare(work, dto.fact_schemas.len(), 1usize)? == Ordering::Less
                || vw::compare(work, dto.fact_schemas.len(), 16usize)? == Ordering::Greater)
            || vw::compare(work, dto.links.len(), 32usize)? == Ordering::Greater
        {
            return Err(fail("PRODUCT_LIMIT", "units/activities/schemas/links"));
        }
        // Shared input closure may include the separate context schema and
        // source ancestors, within the authored closure count/byte ceilings.
        if vw::compare(work, supplied_schemas.len(), 128usize)? == Ordering::Greater {
            return Err(fail("DEFINITION_LIMIT", "supplied schemas"));
        }
        let mut available = BTreeMap::new();
        let mut source_bytes = 0usize;
        for schema in supplied_schemas {
            work.visit(Visit::Element)?;
            let schema_bytes = (if complete {
                schema.canonical_bytes_complete_with_work(work)?
            } else {
                schema.canonical_bytes_with_work(work)?
            })
            .as_slice()
            .len();
            work.arithmetic(
                Op::Add,
                &[&BigInt::from(source_bytes), &BigInt::from(schema_bytes)],
            )?;
            source_bytes = source_bytes
                .checked_add(schema_bytes)
                .ok_or_else(|| fail("DEFINITION_LIMIT", "source bytes"))?;
            if vw::compare(work, source_bytes, 1048576usize)? == Ordering::Greater {
                return Err(fail("DEFINITION_LIMIT", "source bytes"));
            }
            work.visit(Visit::RefLookup)?;
            let id = if complete {
                schema.document_id_complete_with_work(work)?
            } else {
                schema.document_id_with_work(work)?
            };
            if complete {
                super::facts::source::prepare_insert(&available, &id, work)?;
            }
            available.insert(id, schema);
        }
        if complete {
            source::references(&mut dto.fact_schemas, work)?;
        } else {
            canonical::sort_set_with_work(&mut dto.fact_schemas, work)?;
        }
        let mut schemas = BTreeMap::new();
        let mut schema_keys: Vec<(&str, &str)> = if complete {
            Vec::with_capacity(dto.fact_schemas.len())
        } else {
            Vec::new()
        };
        for reference in &dto.fact_schemas {
            work.visit(Visit::Element)?;
            work.visit(Visit::Constraint)?;
            if super::facts::source::different(
                &reference.kind,
                "workflow-fact-schema",
                work,
                complete,
            )? {
                return Err(fail("DEFINITION_KIND", "product fact schema"));
            }
            let schema = super::facts::source::lookup(&available, &reference.id, work, complete)?
                .ok_or_else(|| fail("DEFINITION_MISSING", "product fact schema"))?;
            work.visit(Visit::Constraint)?;
            if source::digest_diff(&reference.digest, schema, work, complete)? {
                return Err(fail("DEFINITION_DIGEST_MISMATCH", "fact schema"));
            }
            let key = (
                schema.canonical_value()["schema_id"]
                    .as_str()
                    .expect("validated schema identity"),
                schema.canonical_value()["version"]
                    .as_str()
                    .expect("validated version"),
            );
            work.visit(Visit::RefLookup)?;
            work.visit(Visit::Constraint)?;
            for prior in &schema_keys {
                if source::compare(prior.0, key.0, work, complete)? == Ordering::Equal
                    && source::compare(prior.1, key.1, work, complete)? == Ordering::Equal
                {
                    return Err(fail("DEFINITION_CONFLICT", "schema identity/version"));
                }
            }
            if complete {
                work.write_bytes(std::mem::size_of::<(&str, &str)>())?;
            }
            schema_keys.push(key);
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&schemas, &reference.id, work, complete)?.is_some() {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "fact schema reference"));
            }
            if complete {
                super::facts::source::prepare_insert(&schemas, &reference.id, work)?;
                schemas.insert(
                    super::facts::source::text(&reference.id, work)?,
                    (*schema).clone_complete_with_work(work)?,
                );
            } else {
                schemas.insert(reference.id.clone(), (*schema).clone());
            }
        }
        if complete {
            super::facts::source::sort(&mut dto.units, work, |a, b, w| {
                super::facts::source::text_cmp(&a.name, &b.name, w)
            })?;
            super::facts::source::sort(&mut dto.activities, work, |a, b, w| {
                super::facts::source::text_cmp(&a.name, &b.name, w)
            })?;
            super::facts::source::sort(&mut dto.links, work, |a, b, w| {
                super::facts::source::text_cmp(&a.name, &b.name, w)
            })?;
        } else {
            vw::try_sort_by(&mut dto.units, work, |a, b, w| {
                member_compare(&a.name, &b.name, w)
            })?;
            vw::try_sort_by(&mut dto.activities, work, |a, b, w| {
                member_compare(&a.name, &b.name, w)
            })?;
            vw::try_sort_by(&mut dto.links, work, |a, b, w| {
                member_compare(&a.name, &b.name, w)
            })?;
        }
        let mut units = BTreeMap::new();
        for unit in &dto.units {
            work.visit(Visit::Element)?;
            name_with_work(&unit.name, work)?;
            name_with_work(&unit.dimension, work)?;
            unit.labels.validate_with_work(work)?;
            work.visit(Visit::Constraint)?;
            if let Some(normalization) = &unit.normalization {
                name_with_work(&normalization.to, work)?;
                work.visit(Visit::Constraint)?;
                if !normalization.factor.is_positive() {
                    return Err(fail("UNIT_NORMALIZATION", "positive factor required"));
                }
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&units, &unit.name, work, complete)?.is_some() {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "unit name"));
            }
            if complete {
                super::facts::source::prepare_insert(&units, &unit.name, work)?;
                units.insert(
                    super::facts::source::text(&unit.name, work)?,
                    source::unit(unit, work)?,
                );
            } else {
                units.insert(unit.name.clone(), UnitDefinition(unit.clone()));
            }
        }
        for unit in units.values() {
            work.visit(Visit::Element)?;
            work.visit(Visit::Constraint)?;
            if let Some((to, _)) = unit.normalization() {
                let target = super::facts::source::lookup(&units, to, work, complete)?
                    .ok_or_else(|| fail("UNIT_MISSING", "normalization target"))?;
                work.visit(Visit::Constraint)?;
                if target.normalization().is_some()
                    || unit.name() == to
                    || unit.dimension() != target.dimension()
                {
                    return Err(fail(
                        "UNIT_NORMALIZATION",
                        "terminal target of same dimension required",
                    ));
                }
            }
        }
        for schema in schemas.values() {
            work.visit(Visit::Element)?;
            for field in schema.canonical_value()["fields"]
                .as_array()
                .expect("validated fields")
            {
                work.visit(Visit::Constraint)?;
                if let Some(unit) = field["type"].get("unit").and_then(Value::as_str) {
                    work.visit(Visit::Constraint)?;
                    if super::facts::source::lookup(&units, unit, work, complete)?.is_none() {
                        return Err(fail("UNIT_MISSING", "fact field unit"));
                    }
                }
            }
        }
        let mut activities = BTreeMap::new();
        for activity in &dto.activities {
            work.visit(Visit::Element)?;
            name_with_work(&activity.name, work)?;
            name_with_work(&activity.source_slot, work)?;
            activity.labels.validate_with_work(work)?;
            work.visit(Visit::Constraint)?;
            if let Some(leg) = &activity.funding_leg {
                name_with_work(leg, work)?;
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::different(
                &activity.fact_schema.kind,
                "workflow-fact-schema",
                work,
                complete,
            )? {
                return Err(fail("DEFINITION_KIND", "activity fact schema"));
            }
            work.visit(Visit::RefLookup)?;
            let schema =
                super::facts::source::lookup(&schemas, &activity.fact_schema.id, work, complete)?
                    .ok_or_else(|| {
                    fail(
                        "DEFINITION_MISSING",
                        "activity schema not declared by product",
                    )
                })?;
            work.visit(Visit::Constraint)?;
            if source::digest_diff(&activity.fact_schema.digest, schema, work, complete)? {
                return Err(fail("DEFINITION_DIGEST_MISMATCH", "activity schema"));
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&activities, &activity.name, work, complete)?.is_some()
            {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "activity name"));
            }
            if complete {
                super::facts::source::prepare_insert(&activities, &activity.name, work)?;
                activities.insert(
                    super::facts::source::text(&activity.name, work)?,
                    source::activity(activity, work)?,
                );
            } else {
                activities.insert(activity.name.clone(), Activity(activity.clone()));
            }
        }
        let mut links = BTreeMap::new();
        for link in &mut dto.links {
            work.visit(Visit::Element)?;
            name_with_work(&link.name, work)?;
            name_with_work(&link.child_activity, work)?;
            work.visit(Visit::Constraint)?;
            if vw::compare(work, link.predecessor_activities.len(), 1usize)? == Ordering::Less
                || vw::compare(work, link.predecessor_activities.len(), 16usize)?
                    == Ordering::Greater
            {
                return Err(fail("PRODUCT_LIMIT", "predecessor activity set"));
            }
            for predecessor in &link.predecessor_activities {
                work.visit(Visit::Element)?;
                name_with_work(predecessor, work)?;
            }
            if complete {
                source::names(&mut link.predecessor_activities, work)?;
            } else {
                canonical::sort_set_with_work(&mut link.predecessor_activities, work)?;
            }
            let minimum = counter_with_work(&link.minimum, work)?;
            let maximum = counter_with_work(&link.maximum, work)?;
            let depth = counter_with_work(&link.max_depth, work)?;
            if vw::compare(work, minimum, 1u64)? == Ordering::Greater
                || vw::compare(work, maximum, 1u64)? == Ordering::Less
                || vw::compare(work, maximum, 8u64)? == Ordering::Greater
                || vw::compare(work, maximum, minimum)? == Ordering::Less
                || vw::compare(work, depth, 16u64)? == Ordering::Greater
            {
                return Err(fail("LINK_LIMIT", "cardinality/depth"));
            }
            link.minimum = vw::unsigned_string(u128::from(minimum), work)?;
            link.maximum = vw::unsigned_string(u128::from(maximum), work)?;
            link.max_depth = vw::unsigned_string(u128::from(depth), work)?;
            work.visit(Visit::Constraint)?;
            match (link.binding_relation, &link.supplier_slot) {
                (BindingRelation::DeclaredSupplier, Some(slot)) => name_with_work(slot, work)?,
                (BindingRelation::DeclaredSupplier, None) => {
                    return Err(fail(
                        "LINK_SUPPLIER_SLOT",
                        "declared supplier needs explicit slot",
                    ));
                }
                (_, Some(_)) => {
                    return Err(fail(
                        "LINK_SUPPLIER_SLOT",
                        "supplier slot forbidden for this relation",
                    ));
                }
                (_, None) => (),
            }
            let child =
                super::facts::source::lookup(&activities, &link.child_activity, work, complete)?
                    .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "child activity"))?;
            work.visit(Visit::Constraint)?;
            if child.primitive() != link.child_primitive {
                return Err(fail("LINK_PRIMITIVE", "child primitive mismatch"));
            }
            for predecessor in &link.predecessor_activities {
                work.visit(Visit::Element)?;
                let activity =
                    super::facts::source::lookup(&activities, predecessor, work, complete)?
                        .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "predecessor activity"))?;
                work.visit(Visit::Constraint)?;
                if activity.primitive() != link.predecessor_primitive {
                    return Err(fail("LINK_PRIMITIVE", "predecessor primitive mismatch"));
                }
            }
            // Cyclic or same-activity *type declarations* are permitted. Actual
            // event identity/monetary graphs must be validated at resolution.
            let definition = LinkDefinition {
                dto: if complete {
                    source::link(link, work)?
                } else {
                    link.clone()
                },
                minimum: minimum as u8,
                maximum: maximum as u8,
                depth: depth as u8,
            };
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&links, &link.name, work, complete)?.is_some() {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "link name"));
            }
            if complete {
                super::facts::source::prepare_insert(&links, &link.name, work)?;
            }
            links.insert(
                if complete {
                    super::facts::source::text(&link.name, work)?
                } else {
                    link.name.clone()
                },
                definition,
            );
        }
        let body = if complete {
            source::body(&dto, work)?
        } else {
            serde_json::to_value(&dto).map_err(|error| fail("JSON", &error.to_string()))?
        };
        Ok(Self {
            body,
            product_id: dto.product_id,
            version,
            units,
            activities,
            links,
            schemas,
        })
    }
    pub fn product_id(&self) -> &str {
        &self.product_id
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn unit(&self, name: &str) -> Option<&UnitDefinition> {
        self.unit_with_work(name, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn unit_with_work(
        &self,
        name: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&UnitDefinition>> {
        map_lookup(&self.units, name, work)
    }
    pub fn activity(&self, name: &str) -> Option<&Activity> {
        self.activity_with_work(name, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn activity_with_work(
        &self,
        name: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&Activity>> {
        map_lookup(&self.activities, name, work)
    }
    pub fn link(&self, name: &str) -> Option<&LinkDefinition> {
        self.link_with_work(name, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn link_with_work(
        &self,
        name: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&LinkDefinition>> {
        map_lookup(&self.links, name, work)
    }
    pub fn fact_schema(&self, id: &str) -> Option<&FactSchema> {
        self.fact_schema_with_work(id, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn fact_schema_with_work(
        &self,
        id: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&FactSchema>> {
        map_lookup(&self.schemas, id, work)
    }
    pub fn activities(&self) -> impl Iterator<Item = &Activity> {
        self.activities.values()
    }
    pub fn units(&self) -> impl Iterator<Item = &UnitDefinition> {
        self.units.values()
    }
    pub fn links(&self) -> impl Iterator<Item = &LinkDefinition> {
        self.links.values()
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
    pub fn content_digest(&self) -> Result<String> {
        self.content_digest_with_work(&mut NoopValidationWork)
    }
    pub fn content_digest_with_work(&self, work: &mut dyn ValidationWork) -> Result<String> {
        super::document_digest_with_work(SCHEMA, &self.body, work)
    }
    pub fn document_id(&self) -> Result<String> {
        self.document_id_with_work(&mut NoopValidationWork)
    }
    pub fn document_id_with_work(&self, work: &mut dyn ValidationWork) -> Result<String> {
        let digest = self.content_digest_with_work(work)?;
        work.write_bytes(69)?;
        Ok(format!("wdoc_{}", &digest[7..]))
    }
    /// Check declaration feasibility of a named monetary path. When an edge
    /// permits alternative activity types, this is an existential type check,
    /// not proof that every actual predecessor can traverse the next slot.
    /// Compiler/runtime must retain the exact path and check every concrete
    /// endpoint, primitive, graph cycle/depth and binding under original terms;
    /// unsupported endpoints refuse/wait rather than selecting a fallback.
    pub fn validate_selector_path(&self, child: &str, path: &[String], target: &str) -> Result<()> {
        self.validate_selector_path_with_work(child, path, target, &mut NoopValidationWork)
    }
    pub fn validate_selector_path_with_work(
        &self,
        child: &str,
        path: &[String],
        target: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<()> {
        work.visit(Visit::Constraint)?;
        if path.is_empty() || vw::compare(work, path.len(), 4usize)? == Ordering::Greater {
            return Err(fail("SELECTOR_PATH", "named predecessor path length"));
        }
        work.visit(Visit::RefLookup)?;
        if self.activity_with_work(target, work)?.is_none() {
            return Err(fail("LINK_ACTIVITY_MISSING", "selector target"));
        }
        let mut possible = BTreeSet::from([child.to_owned()]);
        for slot in path {
            work.visit(Visit::Edge)?;
            work.visit(Visit::RefLookup)?;
            let link = self
                .link_with_work(slot, work)?
                .ok_or_else(|| fail("SELECTOR_PATH", "undeclared link slot"))?;
            work.visit(Visit::RefLookup)?;
            if !super::facts::set_contains(&possible, link.child_activity(), work)?
                || vw::compare(work, link.maximum(), 1u8)? != Ordering::Equal
            {
                return Err(fail("SELECTOR_PATH", "path activity/cardinality mismatch"));
            }
            let mut next = BTreeSet::new();
            for activity in link.predecessor_activities() {
                work.visit(Visit::Element)?;
                work.visit(Visit::RefLookup)?;
                next.insert(activity.clone());
            }
            possible = next;
        }
        work.visit(Visit::RefLookup)?;
        if !super::facts::set_contains(&possible, target, work)? {
            return Err(fail("SELECTOR_PATH", "target activity mismatch"));
        }
        Ok(())
    }
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}

#[cfg(test)]
mod tests;
