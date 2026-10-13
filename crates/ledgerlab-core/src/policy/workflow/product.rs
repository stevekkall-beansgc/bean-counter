//! Closed reusable activity/unit/link declarations. These definitions supply
//! no source rights, customer bindings, event links or economic authorization.
use super::facts::{counter, name, reject_null};
use super::FactSchema;
use crate::canonical::{self, CanonicalBytes};
use crate::money::ExactRatio;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    fn validate(&self) -> Result<()> {
        crate::domain::text(&self.title, 128)?;
        if let Some(description) = &self.description {
            crate::domain::text(description, 1024)?;
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
        let value = canonical::parse(bytes)?;
        reject_null(&value)?;
        let mut dto: ProductDto = canonical::dto(value)?;
        if dto.schema != SCHEMA
            || dto.profile != "workflow/1"
            || dto.evaluator != "ledger-workflow-evaluator/1"
            || dto.grammar != "ledger-policy-grammar/1"
            || dto.limits_profile != "workflow-limits/1"
        {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "product profiles"));
        }
        name(&dto.product_id)?;
        dto.labels.validate()?;
        let version = counter(&dto.version)?;
        if version == 0 {
            return Err(fail("PRODUCT_VERSION", "version starts at one"));
        }
        dto.version = version.to_string();
        if !(1..=16).contains(&dto.units.len())
            || !(1..=16).contains(&dto.activities.len())
            || !(1..=16).contains(&dto.fact_schemas.len())
            || dto.links.len() > 32
        {
            return Err(fail("PRODUCT_LIMIT", "units/activities/schemas/links"));
        }
        // Shared input closure may include the separate context schema and
        // source ancestors, within the authored closure count/byte ceilings.
        if supplied_schemas.len() > 128 {
            return Err(fail("DEFINITION_LIMIT", "supplied schemas"));
        }
        let mut available = BTreeMap::new();
        let mut source_bytes = 0usize;
        for schema in supplied_schemas {
            source_bytes = source_bytes
                .checked_add(schema.canonical_bytes()?.as_slice().len())
                .ok_or_else(|| fail("DEFINITION_LIMIT", "source bytes"))?;
            if source_bytes > 1024 * 1024 {
                return Err(fail("DEFINITION_LIMIT", "source bytes"));
            }
            available.insert(schema.document_id()?, schema);
        }
        canonical::sort_set(&mut dto.fact_schemas)?;
        let mut schemas = BTreeMap::new();
        let mut schema_keys = BTreeSet::new();
        for reference in &dto.fact_schemas {
            if reference.kind != "workflow-fact-schema" {
                return Err(fail("DEFINITION_KIND", "product fact schema"));
            }
            let schema = available
                .get(&reference.id)
                .ok_or_else(|| fail("DEFINITION_MISSING", "product fact schema"))?;
            if reference.digest != schema.content_digest()? {
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
            if !schema_keys.insert(key) {
                return Err(fail("DEFINITION_CONFLICT", "schema identity/version"));
            }
            if schemas
                .insert(reference.id.clone(), (*schema).clone())
                .is_some()
            {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "fact schema reference"));
            }
        }
        dto.units.sort_by(|a, b| a.name.cmp(&b.name));
        dto.activities.sort_by(|a, b| a.name.cmp(&b.name));
        dto.links.sort_by(|a, b| a.name.cmp(&b.name));
        let mut units = BTreeMap::new();
        for unit in &dto.units {
            name(&unit.name)?;
            name(&unit.dimension)?;
            unit.labels.validate()?;
            if let Some(normalization) = &unit.normalization {
                name(&normalization.to)?;
                if !normalization.factor.is_positive() {
                    return Err(fail("UNIT_NORMALIZATION", "positive factor required"));
                }
            }
            if units
                .insert(unit.name.clone(), UnitDefinition(unit.clone()))
                .is_some()
            {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "unit name"));
            }
        }
        for unit in units.values() {
            if let Some((to, _)) = unit.normalization() {
                let target = units
                    .get(to)
                    .ok_or_else(|| fail("UNIT_MISSING", "normalization target"))?;
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
            for field in schema.canonical_value()["fields"]
                .as_array()
                .expect("validated fields")
            {
                if let Some(unit) = field["type"].get("unit").and_then(Value::as_str) {
                    if !units.contains_key(unit) {
                        return Err(fail("UNIT_MISSING", "fact field unit"));
                    }
                }
            }
        }
        let mut activities = BTreeMap::new();
        for activity in &dto.activities {
            name(&activity.name)?;
            name(&activity.source_slot)?;
            activity.labels.validate()?;
            if let Some(leg) = &activity.funding_leg {
                name(leg)?;
            }
            if activity.fact_schema.kind != "workflow-fact-schema" {
                return Err(fail("DEFINITION_KIND", "activity fact schema"));
            }
            let schema = schemas.get(&activity.fact_schema.id).ok_or_else(|| {
                fail(
                    "DEFINITION_MISSING",
                    "activity schema not declared by product",
                )
            })?;
            if activity.fact_schema.digest != schema.content_digest()? {
                return Err(fail("DEFINITION_DIGEST_MISMATCH", "activity schema"));
            }
            if activities
                .insert(activity.name.clone(), Activity(activity.clone()))
                .is_some()
            {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "activity name"));
            }
        }
        let mut links = BTreeMap::new();
        for link in &mut dto.links {
            name(&link.name)?;
            name(&link.child_activity)?;
            if !(1..=16).contains(&link.predecessor_activities.len()) {
                return Err(fail("PRODUCT_LIMIT", "predecessor activity set"));
            }
            for predecessor in &link.predecessor_activities {
                name(predecessor)?;
            }
            canonical::sort_set(&mut link.predecessor_activities)?;
            let minimum = counter(&link.minimum)?;
            let maximum = counter(&link.maximum)?;
            let depth = counter(&link.max_depth)?;
            if minimum > 1 || !(1..=8).contains(&maximum) || maximum < minimum || depth > 16 {
                return Err(fail("LINK_LIMIT", "cardinality/depth"));
            }
            link.minimum = minimum.to_string();
            link.maximum = maximum.to_string();
            link.max_depth = depth.to_string();
            match (link.binding_relation, &link.supplier_slot) {
                (BindingRelation::DeclaredSupplier, Some(slot)) => name(slot)?,
                (BindingRelation::DeclaredSupplier, None) => {
                    return Err(fail(
                        "LINK_SUPPLIER_SLOT",
                        "declared supplier needs explicit slot",
                    ))
                }
                (_, Some(_)) => {
                    return Err(fail(
                        "LINK_SUPPLIER_SLOT",
                        "supplier slot forbidden for this relation",
                    ))
                }
                (_, None) => (),
            }
            let child = activities
                .get(&link.child_activity)
                .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "child activity"))?;
            if child.primitive() != link.child_primitive {
                return Err(fail("LINK_PRIMITIVE", "child primitive mismatch"));
            }
            for predecessor in &link.predecessor_activities {
                let activity = activities
                    .get(predecessor)
                    .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "predecessor activity"))?;
                if activity.primitive() != link.predecessor_primitive {
                    return Err(fail("LINK_PRIMITIVE", "predecessor primitive mismatch"));
                }
            }
            // Cyclic or same-activity *type declarations* are permitted. Actual
            // event identity/monetary graphs must be validated at resolution.
            let definition = LinkDefinition {
                dto: link.clone(),
                minimum: minimum as u8,
                maximum: maximum as u8,
                depth: depth as u8,
            };
            if links.insert(link.name.clone(), definition).is_some() {
                return Err(fail("PRODUCT_DUPLICATE_KEY", "link name"));
            }
        }
        let body = serde_json::to_value(&dto).map_err(|error| fail("JSON", &error.to_string()))?;
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
        self.units.get(name)
    }
    pub fn activity(&self, name: &str) -> Option<&Activity> {
        self.activities.get(name)
    }
    pub fn link(&self, name: &str) -> Option<&LinkDefinition> {
        self.links.get(name)
    }
    pub fn fact_schema(&self, id: &str) -> Option<&FactSchema> {
        self.schemas.get(id)
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
        CanonicalBytes::from_value(&self.body)
    }
    pub fn content_digest(&self) -> Result<String> {
        super::document_digest(SCHEMA, &self.body)
    }
    pub fn document_id(&self) -> Result<String> {
        Ok(format!("wdoc_{}", &self.content_digest()?[7..]))
    }
    /// Check declaration feasibility of a named monetary path. When an edge
    /// permits alternative activity types, this is an existential type check,
    /// not proof that every actual predecessor can traverse the next slot.
    /// Compiler/runtime must retain the exact path and check every concrete
    /// endpoint, primitive, graph cycle/depth and binding under original terms;
    /// unsupported endpoints refuse/wait rather than selecting a fallback.
    pub fn validate_selector_path(&self, child: &str, path: &[String], target: &str) -> Result<()> {
        if path.is_empty() || path.len() > 4 {
            return Err(fail("SELECTOR_PATH", "named predecessor path length"));
        }
        if !self.activities.contains_key(target) {
            return Err(fail("LINK_ACTIVITY_MISSING", "selector target"));
        }
        let mut possible = BTreeSet::from([child.to_owned()]);
        for slot in path {
            let link = self
                .links
                .get(slot)
                .ok_or_else(|| fail("SELECTOR_PATH", "undeclared link slot"))?;
            if !possible.contains(link.child_activity()) || link.maximum() != 1 {
                return Err(fail("SELECTOR_PATH", "path activity/cardinality mismatch"));
            }
            possible = link.predecessor_activities().iter().cloned().collect();
        }
        if !possible.contains(target) {
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
