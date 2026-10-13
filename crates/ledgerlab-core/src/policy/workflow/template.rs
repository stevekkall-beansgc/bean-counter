//! Reusable symbolic template validation. Agreement slot instantiation and
//! financial authority are separate, and cannot be inferred from this type.
use super::facts::{counter, name, reject_null};
use crate::canonical::{self, CanonicalBytes};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const SCHEMA: &str = "ledger-workflow-slot-template/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateBook {
    Retail,
    Supplier,
    CostObservation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TemplateComponent {
    key: String,
    book: TemplateBook,
    root_key: String,
    role_slot: String,
    aliases: Vec<String>,
}
impl TemplateComponent {
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn book(&self) -> TemplateBook {
        self.book
    }
    pub fn root_key(&self) -> &str {
        &self.root_key
    }
    pub fn role_slot(&self) -> &str {
        &self.role_slot
    }
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TemplateFamily {
    key: String,
    target_component_key: String,
    role_slot: String,
    aliases: Vec<String>,
}
impl TemplateFamily {
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn target_component_key(&self) -> &str {
        &self.target_component_key
    }
    pub fn role_slot(&self) -> &str {
        &self.role_slot
    }
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateReference {
    kind: String,
    id: String,
    digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentDto {
    key: String,
    book: TemplateBook,
    root_key: String,
    role_slot: String,
    aliases: Vec<String>,
}
impl From<ComponentDto> for TemplateComponent {
    fn from(value: ComponentDto) -> Self {
        Self {
            key: value.key,
            book: value.book,
            root_key: value.root_key,
            role_slot: value.role_slot,
            aliases: value.aliases,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilyDto {
    key: String,
    target_component_key: String,
    role_slot: String,
    aliases: Vec<String>,
}
impl From<FamilyDto> for TemplateFamily {
    fn from(value: FamilyDto) -> Self {
        Self {
            key: value.key,
            target_component_key: value.target_component_key,
            role_slot: value.role_slot,
            aliases: value.aliases,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateDto {
    schema: String,
    template_id: String,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<TemplateReference>,
    components: Vec<ComponentDto>,
    families: Vec<FamilyDto>,
}

/// A closed template whose ancestry and immutable symbolic slots are checked.
/// Parsing never chooses a previous version from a current head or cache.
#[derive(Clone, Debug)]
pub struct SlotTemplate {
    body: Value,
    template_id: String,
    version: u64,
    components: BTreeMap<String, TemplateComponent>,
    families: BTreeMap<String, TemplateFamily>,
}
impl SlotTemplate {
    pub fn parse(bytes: &[u8], previous: Option<&Self>) -> Result<Self> {
        let value = canonical::parse(bytes)?;
        reject_null(&value)?;
        let mut dto: TemplateDto = canonical::dto(value)?;
        if dto.schema != SCHEMA {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "slot-template schema"));
        }
        name(&dto.template_id)?;
        let version = counter(&dto.version)?;
        if version == 0 {
            return Err(fail("TEMPLATE_VERSION", "version starts at one"));
        }
        dto.version = version.to_string();
        if dto.components.is_empty() || dto.components.len() > 128 || dto.families.len() > 64 {
            return Err(fail("TEMPLATE_LIMIT", "component/family cardinality"));
        }
        dto.components.sort_by(|a, b| a.key.cmp(&b.key));
        dto.families.sort_by(|a, b| a.key.cmp(&b.key));
        let mut components: BTreeMap<String, TemplateComponent> = BTreeMap::new();
        let mut families: BTreeMap<String, TemplateFamily> = BTreeMap::new();
        for component in &mut dto.components {
            name(&component.key)?;
            name(&component.root_key)?;
            name(&component.role_slot)?;
            aliases(&mut component.aliases)?;
            if components
                .insert(component.key.clone(), component.clone().into())
                .is_some()
            {
                return Err(fail("TEMPLATE_DUPLICATE_KEY", "component key"));
            }
        }
        for component in components.values() {
            let root = components
                .get(&component.root_key)
                .ok_or_else(|| fail("TEMPLATE_MISSING_ROOT", "component root"))?;
            if root.root_key != root.key {
                return Err(fail("TEMPLATE_ROOT_CHAIN", "root must be direct self-root"));
            }
            if root.book != component.book || root.role_slot != component.role_slot {
                return Err(fail("TEMPLATE_ROOT_SEMANTICS", "root book/role mismatch"));
            }
        }
        for family in &mut dto.families {
            name(&family.key)?;
            name(&family.target_component_key)?;
            name(&family.role_slot)?;
            aliases(&mut family.aliases)?;
            if families
                .insert(family.key.clone(), family.clone().into())
                .is_some()
            {
                return Err(fail("TEMPLATE_DUPLICATE_KEY", "family key"));
            }
            let target = components
                .get(&family.target_component_key)
                .ok_or_else(|| fail("TEMPLATE_MISSING_ROOT", "family target"))?;
            if target.root_key != target.key {
                return Err(fail(
                    "TEMPLATE_FAMILY_TARGET",
                    "family requires original root component",
                ));
            }
            if target.role_slot != family.role_slot {
                return Err(fail("TEMPLATE_FAMILY_TARGET", "family role mismatch"));
            }
        }
        match (version, &dto.previous, previous) {
            (1, None, None) => (),
            (1, _, _) => return Err(fail("TEMPLATE_VERSION", "initial version forbids previous")),
            (_, Some(reference), Some(prior)) => {
                if reference.kind != "workflow-slot-template"
                    || reference.id != prior.document_id()?
                    || reference.digest != prior.content_digest()?
                {
                    return Err(fail(
                        "TEMPLATE_PREVIOUS_MISMATCH",
                        "exact previous reference required",
                    ));
                }
                if dto.template_id != prior.template_id || version <= prior.version {
                    return Err(fail(
                        "TEMPLATE_VERSION",
                        "same template and strictly increasing version required",
                    ));
                }
                for old in prior.components.values() {
                    let new = components
                        .get(&old.key)
                        .ok_or_else(|| fail("TEMPLATE_CONTINUITY", "previous component removed"))?;
                    if new.book != old.book
                        || new.root_key != old.root_key
                        || new.role_slot != old.role_slot
                    {
                        return Err(fail(
                            "TEMPLATE_CONTINUITY",
                            "previous component semantics changed",
                        ));
                    }
                }
                for old in prior.families.values() {
                    let new = families
                        .get(&old.key)
                        .ok_or_else(|| fail("TEMPLATE_CONTINUITY", "previous family removed"))?;
                    if new.target_component_key != old.target_component_key
                        || new.role_slot != old.role_slot
                    {
                        return Err(fail(
                            "TEMPLATE_CONTINUITY",
                            "previous family semantics changed",
                        ));
                    }
                }
            }
            _ => {
                return Err(fail(
                    "TEMPLATE_DEPENDENCY_MISSING",
                    "successor requires exact retained previous template",
                ))
            }
        }
        let body = serde_json::to_value(&dto).map_err(|error| fail("JSON", &error.to_string()))?;
        Ok(Self {
            body,
            template_id: dto.template_id,
            version,
            components,
            families,
        })
    }
    pub fn template_id(&self) -> &str {
        &self.template_id
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn component(&self, key: &str) -> Option<&TemplateComponent> {
        self.components.get(key)
    }
    pub fn family(&self, key: &str) -> Option<&TemplateFamily> {
        self.families.get(key)
    }
    pub fn components(&self) -> impl Iterator<Item = &TemplateComponent> {
        self.components.values()
    }
    pub fn families(&self) -> impl Iterator<Item = &TemplateFamily> {
        self.families.values()
    }
    /// Friendly aliases are checked only inside an explicitly supplied key.
    /// They never select or merge stable slots across template keys.
    pub fn component_named(&self, key: &str, alias: &str) -> Result<&TemplateComponent> {
        let component = self
            .components
            .get(key)
            .ok_or_else(|| fail("TEMPLATE_UNKNOWN_KEY", "component key"))?;
        if !component.aliases.iter().any(|candidate| candidate == alias) {
            return Err(fail("TEMPLATE_UNKNOWN_ALIAS", "component alias"));
        }
        Ok(component)
    }
    pub fn family_named(&self, key: &str, alias: &str) -> Result<&TemplateFamily> {
        let family = self
            .families
            .get(key)
            .ok_or_else(|| fail("TEMPLATE_UNKNOWN_KEY", "family key"))?;
        if !family.aliases.iter().any(|candidate| candidate == alias) {
            return Err(fail("TEMPLATE_UNKNOWN_ALIAS", "family alias"));
        }
        Ok(family)
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
}
fn aliases(values: &mut Vec<String>) -> Result<()> {
    if values.len() > 64 {
        return Err(fail("TEMPLATE_LIMIT", "aliases"));
    }
    for alias in values.iter() {
        name(alias)?;
    }
    canonical::sort_set(values)
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}

#[cfg(test)]
mod tests;
