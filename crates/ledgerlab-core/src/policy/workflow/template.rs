//! Reusable symbolic template validation. Agreement slot instantiation and
//! financial authority are separate, and cannot be inferred from this type.
mod source;
use super::facts::{
    counter_with_work, map_lookup, member_compare, name_with_work, reject_null_with_work,
};
use crate::canonical::{self, CanonicalBytes};
use crate::validation_work::{
    self as vw, NoopValidationWork, ValidationVisit as Visit, ValidationWork,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
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
        Self::parse_with_work(bytes, previous, &mut NoopValidationWork)
    }
    pub fn parse_with_work(
        bytes: &[u8],
        previous: Option<&Self>,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        work.scan_bytes(bytes.len())?;
        Self::from_value_with_work(canonical::parse(bytes)?, previous, work)
    }
    pub(crate) fn from_value_with_work(
        value: Value,
        previous: Option<&Self>,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, previous, work, false)
    }
    pub(crate) fn normalization_payload_bound_with_work(
        value: &Value,
        previous: Option<&Self>,
        work: &mut dyn ValidationWork,
    ) -> Result<usize> {
        source::bound(value, previous, work)
    }
    pub(crate) fn from_value_complete_with_work(
        value: Value,
        previous: Option<&Self>,
        work: &mut dyn ValidationWork,
    ) -> Result<Self> {
        Self::from_value_mode(value, previous, work, true)
    }
    fn from_value_mode(
        value: Value,
        previous: Option<&Self>,
        work: &mut dyn ValidationWork,
        complete: bool,
    ) -> Result<Self> {
        if complete {
            source::decode(&value, work)?;
        }
        reject_null_with_work(&value, work)?;
        let mut dto: TemplateDto = serde_json::from_value(value).map_err(|error| {
            if complete {
                Error::new("SCHEMA", error.to_string())
            } else {
                fail("SCHEMA", &error.to_string())
            }
        })?;
        work.visit(Visit::Constraint)?;
        if super::facts::source::different(&dto.schema, &SCHEMA, work, complete)? {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "slot-template schema"));
        }
        name_with_work(&dto.template_id, work)?;
        let version = counter_with_work(&dto.version, work)?;
        if vw::compare(work, version, 0u64)? == Ordering::Equal {
            return Err(fail("TEMPLATE_VERSION", "version starts at one"));
        }
        dto.version = vw::unsigned_string(u128::from(version), work)?;
        if vw::compare(work, dto.components.len(), 0usize)? == Ordering::Equal
            || vw::compare(work, dto.components.len(), 128usize)? == Ordering::Greater
            || vw::compare(work, dto.families.len(), 64usize)? == Ordering::Greater
        {
            return Err(fail("TEMPLATE_LIMIT", "component/family cardinality"));
        }
        if complete {
            super::facts::source::sort(&mut dto.components, work, |a, b, w| {
                super::facts::source::text_cmp(&a.key, &b.key, w)
            })?;
            super::facts::source::sort(&mut dto.families, work, |a, b, w| {
                super::facts::source::text_cmp(&a.key, &b.key, w)
            })?;
        } else {
            vw::try_sort_by(&mut dto.components, work, |a, b, w| {
                member_compare(&a.key, &b.key, w)
            })?;
            vw::try_sort_by(&mut dto.families, work, |a, b, w| {
                member_compare(&a.key, &b.key, w)
            })?;
        }
        let mut components: BTreeMap<String, TemplateComponent> = BTreeMap::new();
        let mut families: BTreeMap<String, TemplateFamily> = BTreeMap::new();
        for component in &mut dto.components {
            work.visit(Visit::Element)?;
            name_with_work(&component.key, work)?;
            name_with_work(&component.root_key, work)?;
            name_with_work(&component.role_slot, work)?;
            if complete {
                source::aliases(&mut component.aliases, work)?;
            } else {
                aliases(&mut component.aliases, work)?;
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&components, &component.key, work, complete)?.is_some()
            {
                return Err(fail("TEMPLATE_DUPLICATE_KEY", "component key"));
            }
            if complete {
                super::facts::source::prepare_insert(&components, &component.key, work)?;
                components.insert(
                    super::facts::source::text(&component.key, work)?,
                    source::component(component, work)?,
                );
            } else {
                components.insert(component.key.clone(), component.clone().into());
            }
        }
        for component in components.values() {
            work.visit(Visit::Element)?;
            let root =
                super::facts::source::lookup(&components, &component.root_key, work, complete)?
                    .ok_or_else(|| fail("TEMPLATE_MISSING_ROOT", "component root"))?;
            work.visit(Visit::Constraint)?;
            if super::facts::source::different(&root.root_key, &root.key, work, complete)? {
                return Err(fail("TEMPLATE_ROOT_CHAIN", "root must be direct self-root"));
            }
            work.visit(Visit::Constraint)?;
            if root.book != component.book
                || member_compare(&root.role_slot, &component.role_slot, work)? != Ordering::Equal
            {
                return Err(fail("TEMPLATE_ROOT_SEMANTICS", "root book/role mismatch"));
            }
        }
        for family in &mut dto.families {
            work.visit(Visit::Element)?;
            name_with_work(&family.key, work)?;
            name_with_work(&family.target_component_key, work)?;
            name_with_work(&family.role_slot, work)?;
            if complete {
                source::aliases(&mut family.aliases, work)?;
            } else {
                aliases(&mut family.aliases, work)?;
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::lookup(&families, &family.key, work, complete)?.is_some() {
                return Err(fail("TEMPLATE_DUPLICATE_KEY", "family key"));
            }
            if complete {
                super::facts::source::prepare_insert(&families, &family.key, work)?;
                families.insert(
                    super::facts::source::text(&family.key, work)?,
                    source::family(family, work)?,
                );
            } else {
                families.insert(family.key.clone(), family.clone().into());
            }
            let target = super::facts::source::lookup(
                &components,
                &family.target_component_key,
                work,
                complete,
            )?
            .ok_or_else(|| fail("TEMPLATE_MISSING_ROOT", "family target"))?;
            work.visit(Visit::Constraint)?;
            if target.root_key != target.key {
                return Err(fail(
                    "TEMPLATE_FAMILY_TARGET",
                    "family requires original root component",
                ));
            }
            work.visit(Visit::Constraint)?;
            if super::facts::source::different(
                &target.role_slot,
                &family.role_slot,
                work,
                complete,
            )? {
                return Err(fail("TEMPLATE_FAMILY_TARGET", "family role mismatch"));
            }
        }
        let initial = vw::compare(work, version, 1u64)? == Ordering::Equal;
        match (initial, &dto.previous, previous) {
            (true, None, None) => (),
            (true, _, _) => {
                return Err(fail("TEMPLATE_VERSION", "initial version forbids previous"));
            }
            (false, Some(reference), Some(prior)) => {
                work.visit(Visit::Constraint)?;
                if !source::reference_matches(reference, prior, work, complete)? {
                    return Err(fail(
                        "TEMPLATE_PREVIOUS_MISMATCH",
                        "exact previous reference required",
                    ));
                }
                work.visit(Visit::Constraint)?;
                if super::facts::source::different(
                    &dto.template_id,
                    &prior.template_id,
                    work,
                    complete,
                )? || vw::compare(work, version, prior.version)? != Ordering::Greater
                {
                    return Err(fail(
                        "TEMPLATE_VERSION",
                        "same template and strictly increasing version required",
                    ));
                }
                for old in prior.components.values() {
                    work.visit(Visit::Element)?;
                    let new = super::facts::source::lookup(&components, &old.key, work, complete)?
                        .ok_or_else(|| fail("TEMPLATE_CONTINUITY", "previous component removed"))?;
                    work.visit(Visit::Constraint)?;
                    if new.book != old.book
                        || super::facts::source::different(
                            &new.root_key,
                            &old.root_key,
                            work,
                            complete,
                        )?
                        || super::facts::source::different(
                            &new.role_slot,
                            &old.role_slot,
                            work,
                            complete,
                        )?
                    {
                        return Err(fail(
                            "TEMPLATE_CONTINUITY",
                            "previous component semantics changed",
                        ));
                    }
                }
                for old in prior.families.values() {
                    work.visit(Visit::Element)?;
                    let new = super::facts::source::lookup(&families, &old.key, work, complete)?
                        .ok_or_else(|| fail("TEMPLATE_CONTINUITY", "previous family removed"))?;
                    work.visit(Visit::Constraint)?;
                    if super::facts::source::different(
                        &new.target_component_key,
                        &old.target_component_key,
                        work,
                        complete,
                    )? || super::facts::source::different(
                        &new.role_slot,
                        &old.role_slot,
                        work,
                        complete,
                    )? {
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
                ));
            }
        }
        let body = if complete {
            source::body(&dto, work)?
        } else {
            serde_json::to_value(&dto).map_err(|error| fail("JSON", &error.to_string()))?
        };
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
        self.component_with_work(key, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn family(&self, key: &str) -> Option<&TemplateFamily> {
        self.family_with_work(key, &mut NoopValidationWork)
            .expect("Noop work cannot fail")
    }
    pub fn component_with_work(
        &self,
        key: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&TemplateComponent>> {
        map_lookup(&self.components, key, work)
    }
    pub fn family_with_work(
        &self,
        key: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<Option<&TemplateFamily>> {
        map_lookup(&self.families, key, work)
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
        self.component_named_with_work(key, alias, &mut NoopValidationWork)
    }
    pub fn component_named_with_work(
        &self,
        key: &str,
        alias: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<&TemplateComponent> {
        let component = self
            .component_with_work(key, work)?
            .ok_or_else(|| fail("TEMPLATE_UNKNOWN_KEY", "component key"))?;
        for candidate in &component.aliases {
            if member_compare(candidate, alias, work)? == Ordering::Equal {
                return Ok(component);
            }
        }
        Err(fail("TEMPLATE_UNKNOWN_ALIAS", "component alias"))
    }
    pub fn family_named(&self, key: &str, alias: &str) -> Result<&TemplateFamily> {
        self.family_named_with_work(key, alias, &mut NoopValidationWork)
    }
    pub fn family_named_with_work(
        &self,
        key: &str,
        alias: &str,
        work: &mut dyn ValidationWork,
    ) -> Result<&TemplateFamily> {
        let family = self
            .family_with_work(key, work)?
            .ok_or_else(|| fail("TEMPLATE_UNKNOWN_KEY", "family key"))?;
        for candidate in &family.aliases {
            if member_compare(candidate, alias, work)? == Ordering::Equal {
                return Ok(family);
            }
        }
        Err(fail("TEMPLATE_UNKNOWN_ALIAS", "family alias"))
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
}
fn aliases(values: &mut Vec<String>, work: &mut dyn ValidationWork) -> Result<()> {
    if vw::compare(work, values.len(), 64usize)? == Ordering::Greater {
        return Err(fail("TEMPLATE_LIMIT", "aliases"));
    }
    for alias in values.iter() {
        name_with_work(alias, work)?;
    }
    canonical::sort_set_with_work(values, work)
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}

#[cfg(test)]
mod tests;
