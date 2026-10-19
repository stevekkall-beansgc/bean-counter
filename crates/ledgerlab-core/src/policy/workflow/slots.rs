//! Initial private agreement structures only. These values establish no
//! enrollment, assent, authority, accepted binding, activation or monetary right.
use super::{
    facts::{counter, name, reject_null},
    SlotTemplate, TemplateBook,
};
use crate::{
    canonical::{self, CanonicalBytes},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
mod private;

#[derive(Clone, Debug)]
struct Document {
    body: Value,
    bytes: CanonicalBytes,
    digest: String,
    id: String,
}
impl Document {
    fn new(body: Value) -> Result<Self> {
        let schema = body["schema"].as_str().expect("validated schema");
        let digest = super::document_digest(schema, &body)?;
        Ok(Self {
            id: format!("wdoc_{}", &digest[7..]),
            bytes: CanonicalBytes::from_value(&body)?,
            body,
            digest,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: String,
    id: String,
    digest: String,
}
impl Reference {
    fn matches(&self, kind: &str, id: &str, digest: &str) -> Result<()> {
        if self.kind != kind || self.id != id || self.digest != digest {
            return Err(fail(
                "SLOT_REFERENCE_MISMATCH",
                "exact immutable structural reference",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleDto {
    schema: String,
    scope: [String; 2],
    agreement_id: String,
    customer: String,
    role_slot: String,
    provider: String,
    cost_originator: String,
    bearer: String,
    payer: String,
    beneficiary: String,
    recipient: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payer_delegation_ref: Option<Reference>,
}
#[derive(Clone, Debug)]
pub struct RoleAssignment {
    document: Document,
}
fn role_equal(
    a: &str,
    b: &str,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> Result<bool> {
    work.visit(crate::validation_work::ValidationVisit::MemberCompare)?;
    work.scan_bytes(a.len())?;
    work.scan_bytes(b.len())?;
    Ok(a == b)
}
fn role_string<'a>(
    v: &'a Value,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> Result<&'a str> {
    work.visit(crate::validation_work::ValidationVisit::Constraint)?;
    v.as_str().ok_or_else(|| fail("JSON", "role string"))
}
fn role_fields(
    schema: &str,
    delegated: bool,
    ids: [&str; 10],
    slot: &str,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> Result<()> {
    use crate::validation_work::{compare, ValidationVisit as Visit};
    if !role_equal(schema, "ledger-workflow-role-assignment/1", work)? {
        return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "role assignment schema"));
    }
    work.visit(Visit::Constraint)?;
    if delegated {
        return Err(fail(
            "UNSUPPORTED_SCHEMA_PROFILE",
            "delegation is not implemented by structural parser",
        ));
    }
    for value in ids {
        work.visit(Visit::Constraint)?;
        if value.is_empty() || compare(work, value.len(), 128usize)?.is_gt() {
            return Err(fail("SLOT_ID", "bounded opaque ID"));
        }
        work.scan_bytes(value.len())?;
        // Preserve the exact existing opaque-ID grammar, including C1 text.
        for c in value.chars() {
            work.visit(Visit::Constraint)?;
            if c <= '\u{1f}' || c == '\u{7f}' {
                return Err(fail("SLOT_ID", "bounded opaque ID"));
            }
        }
    }
    super::facts::name_with_work(slot, work)
}
impl RoleAssignment {
    /// Pure closed shape/identity only; six asserted IDs are not authenticated.
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse(raw)?;
        reject_null(&value)?;
        let dto: RoleDto = canonical::dto(value)?;
        role_fields(
            &dto.schema,
            dto.payer_delegation_ref.is_some(),
            [
                &dto.scope[0],
                &dto.scope[1],
                &dto.agreement_id,
                &dto.customer,
                &dto.provider,
                &dto.cost_originator,
                &dto.bearer,
                &dto.payer,
                &dto.beneficiary,
                &dto.recipient,
            ],
            &dto.role_slot,
            &mut crate::validation_work::NoopValidationWork,
        )?;
        Ok(Self {
            document: Document::new(serde_json::to_value(dto).map_err(json_error)?)?,
        })
    }
    /// SAME role normalization for the owning private-document acceptance path.
    /// This is DATA only: no party/delegation/source or accepted-right proof.
    /// Old public parse/serialization and its resource interpretation remain.
    pub fn normalize_private_with_session(
        raw: &[u8],
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<crate::canonical::audit_arena::AuditedBytes> {
        let result = (|| {
            use crate::validation_work::{
                compare, RepresentationKind as Kind, ValidationVisit as Visit,
            };
            if compare(session.semantic_work(), raw.len(), 262144usize)?.is_gt() {
                return Err(fail("SLOT_LIMIT", "private role byte limit"));
            }
            let parsed = session.parse_representation(raw, Kind::SourceDefinition)?;
            let value = parsed.value();
            let work = session.semantic_work();
            super::facts::reject_null_with_work(value, work)?;
            let keys = [
                "schema",
                "scope",
                "agreement_id",
                "customer",
                "role_slot",
                "provider",
                "cost_originator",
                "bearer",
                "payer",
                "beneficiary",
                "recipient",
            ];
            work.visit(Visit::Constraint)?;
            let object = value
                .as_object()
                .ok_or_else(|| fail("JSON", "role object"))?;
            for (key, _) in object {
                work.visit(Visit::Field)?;
                let mut known = false;
                for allowed in keys.iter().chain(["payer_delegation_ref"].iter()) {
                    if role_equal(key, allowed, work)? {
                        known = true;
                        break;
                    }
                }
                if !known {
                    return Err(fail("JSON", "unknown role field"));
                }
            }
            let mut get = |name| -> Result<&Value> {
                for (key, value) in object {
                    work.visit(Visit::RefLookup)?;
                    if role_equal(key, name, work)? {
                        return Ok(value);
                    }
                }
                Err(fail("JSON", "missing role field"))
            };
            let mut values: [Option<&Value>; 11] = [None; 11];
            for (i, name) in keys.iter().enumerate() {
                values[i] = Some(get(name)?);
            }
            // Borrow the actual parsed fields; there is no intermediary DTO,
            // serde clone or a temporary unaccounted normalized Value tree.
            work.visit(Visit::Constraint)?;
            let scope = values[1]
                .ok_or_else(|| fail("JSON", "role scope"))?
                .as_array()
                .ok_or_else(|| fail("JSON", "role scope"))?;
            if !compare(work, scope.len(), 2usize)?.is_eq() {
                return Err(fail("JSON", "role scope"));
            }
            let delegated = object
                .keys()
                .try_fold(false, |found, key| -> Result<bool> {
                    Ok(role_equal(key, "payer_delegation_ref", work)? || found)
                })?;
            role_fields(
                role_string(values[0].unwrap(), work)?,
                delegated,
                [
                    role_string(&scope[0], work)?,
                    role_string(&scope[1], work)?,
                    role_string(values[2].unwrap(), work)?,
                    role_string(values[3].unwrap(), work)?,
                    role_string(values[5].unwrap(), work)?,
                    role_string(values[6].unwrap(), work)?,
                    role_string(values[7].unwrap(), work)?,
                    role_string(values[8].unwrap(), work)?,
                    role_string(values[9].unwrap(), work)?,
                    role_string(values[10].unwrap(), work)?,
                ],
                role_string(values[4].unwrap(), work)?,
                work,
            )?;
            session.canonical_representation(value, Kind::SourceDefinition)
        })();
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn document_id(&self) -> &str {
        &self.document.id
    }
    pub fn content_digest(&self) -> &str {
        &self.document.digest
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    slot_id: String,
    book: TemplateBook,
    root_slot: String,
    role_slot: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Family {
    slot_id: String,
    target_component_slot: String,
    role_slot: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogDto {
    schema: String,
    agreement_id: String,
    customer: String,
    revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Reference>,
    components: Vec<Component>,
    families: Vec<Family>,
}
#[derive(Clone, Debug)]
pub struct SlotCatalog {
    document: Document,
    dto: CatalogDto,
}
impl SlotCatalog {
    /// Normalize original structural counters and row order using the SAME
    /// shared transforms. This is not catalog/template/ownership validation;
    /// the complete borrowed slot validator must consume the returned bytes.
    pub fn normalize_private_with_session(
        raw: &[u8],
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<crate::canonical::audit_arena::AuditedBytes> {
        private::normalize(raw, private::Family::Catalog, session)
    }

    /// Initial revision only. Successors require a separate reviewed history/head
    /// resolver; this parser deliberately does not claim that capability.
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse(raw)?;
        reject_null(&value)?;
        let mut dto: CatalogDto = canonical::dto(value)?;
        if dto.schema != "ledger-workflow-slot-catalog/1" {
            return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "slot catalog schema"));
        }
        initial(&mut dto.revision, dto.previous.as_ref())?;
        id(&dto.agreement_id)?;
        id(&dto.customer)?;
        if dto.components.is_empty() || dto.components.len() > 128 || dto.families.len() > 64 {
            return Err(fail("SLOT_LIMIT", "catalog collections"));
        }
        dto.components
            .sort_by(|a, b| catalog_key_order(&a.slot_id, &b.slot_id));
        dto.families
            .sort_by(|a, b| catalog_key_order(&a.slot_id, &b.slot_id));
        let mut components = BTreeMap::new();
        for c in &dto.components {
            id(&c.slot_id)?;
            id(&c.root_slot)?;
            name(&c.role_slot)?;
            if components.insert(&c.slot_id, c).is_some() {
                return Err(fail("SLOT_DUPLICATE_ID", "component catalog ID"));
            }
        }
        for c in &dto.components {
            let root = components
                .get(&c.root_slot)
                .ok_or_else(|| fail("SLOT_MISSING_ROOT", "catalog root"))?;
            if root.root_slot != root.slot_id
                || root.book != c.book
                || root.role_slot != c.role_slot
            {
                return Err(fail("SLOT_SEMANTICS_CHANGED", "direct root/book/role"));
            }
        }
        let mut families = BTreeSet::new();
        for f in &dto.families {
            id(&f.slot_id)?;
            id(&f.target_component_slot)?;
            name(&f.role_slot)?;
            if !families.insert(&f.slot_id) {
                return Err(fail("SLOT_DUPLICATE_ID", "family catalog ID"));
            }
            let root = components
                .get(&f.target_component_slot)
                .ok_or_else(|| fail("SLOT_MISSING_ROOT", "family root"))?;
            if root.root_slot != root.slot_id || root.role_slot != f.role_slot {
                return Err(fail("SLOT_SEMANTICS_CHANGED", "family direct root/role"));
            }
        }
        Ok(Self {
            document: Document::new(serde_json::to_value(&dto).map_err(json_error)?)?,
            dto,
        })
    }
    pub fn agreement_id(&self) -> &str {
        &self.dto.agreement_id
    }
    pub fn customer(&self) -> &str {
        &self.dto.customer
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn document_id(&self) -> &str {
        &self.document.id
    }
    pub fn content_digest(&self) -> &str {
        &self.document.digest
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    template_key: String,
    slot_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstantiationDto {
    schema: String,
    agreement_id: String,
    customer: String,
    revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Reference>,
    template_ref: Reference,
    catalog_ref: Reference,
    component_map: Vec<Mapping>,
    family_map: Vec<Mapping>,
}
#[derive(Clone, Debug)]
pub struct SlotInstantiation {
    document: Document,
}
impl SlotInstantiation {
    /// Normalization DATA only; exact template/catalog mapping and subject
    /// coverage still run through the SAME complete slot validator.
    pub fn normalize_private_with_session(
        raw: &[u8],
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<crate::canonical::audit_arena::AuditedBytes> {
        private::normalize(raw, private::Family::Instantiation, session)
    }

    /// Initial exact symbolic mapping only, with no assent/binding authority.
    pub fn parse(raw: &[u8], template: &SlotTemplate, catalog: &SlotCatalog) -> Result<Self> {
        let value = canonical::parse(raw)?;
        reject_null(&value)?;
        let mut dto: InstantiationDto = canonical::dto(value)?;
        if dto.schema != "ledger-workflow-slot-instantiation/1" {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "slot instantiation schema",
            ));
        }
        initial(&mut dto.revision, dto.previous.as_ref())?;
        id(&dto.agreement_id)?;
        id(&dto.customer)?;
        if dto.agreement_id != catalog.agreement_id() || dto.customer != catalog.customer() {
            return Err(fail(
                "SLOT_OWNERSHIP_MISMATCH",
                "catalog/instantiation ownership",
            ));
        }
        dto.template_ref.matches(
            "workflow-slot-template",
            &template.document_id()?,
            &template.content_digest()?,
        )?;
        dto.catalog_ref.matches(
            "workflow-slot-catalog",
            catalog.document_id(),
            catalog.content_digest(),
        )?;
        let components = mappings(&mut dto.component_map, 128)?;
        let families = mappings(&mut dto.family_map, 64)?;
        if components.len() != template.components().count()
            || families.len() != template.families().count()
        {
            return Err(fail(
                "SLOT_MAPPING_INCOMPLETE",
                "complete template key maps",
            ));
        }
        for t in template.components() {
            let slot = components
                .get(t.key())
                .ok_or_else(|| fail("SLOT_MAPPING_INCOMPLETE", "component mapping"))?;
            let c = catalog
                .dto
                .components
                .iter()
                .find(|c| c.slot_id == *slot)
                .ok_or_else(|| fail("SLOT_MAPPING_MISSING_ID", "component catalog ID"))?;
            if c.book != t.book()
                || c.role_slot != t.role_slot()
                || components
                    .get(t.root_key())
                    .is_none_or(|r| *r != c.root_slot)
            {
                return Err(fail(
                    "SLOT_SEMANTICS_CHANGED",
                    "template/catalog component book/root/role",
                ));
            }
        }
        for t in template.families() {
            let slot = families
                .get(t.key())
                .ok_or_else(|| fail("SLOT_MAPPING_INCOMPLETE", "family mapping"))?;
            let f = catalog
                .dto
                .families
                .iter()
                .find(|f| f.slot_id == *slot)
                .ok_or_else(|| fail("SLOT_MAPPING_MISSING_ID", "family catalog ID"))?;
            if f.role_slot != t.role_slot()
                || components
                    .get(t.target_component_key())
                    .is_none_or(|r| *r != f.target_component_slot)
            {
                return Err(fail(
                    "SLOT_SEMANTICS_CHANGED",
                    "template/catalog family root/role",
                ));
            }
        }
        Ok(Self {
            document: Document::new(serde_json::to_value(dto).map_err(json_error)?)?,
        })
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn document_id(&self) -> &str {
        &self.document.id
    }
    pub fn content_digest(&self) -> &str {
        &self.document.digest
    }
}
fn mappings(values: &mut [Mapping], limit: usize) -> Result<BTreeMap<String, String>> {
    if values.len() > limit {
        return Err(fail("SLOT_LIMIT", "mapping collection"));
    }
    values.sort_by(|a, b| mapping_key_order(&a.template_key, &b.template_key));
    let mut keys = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for m in values {
        name(&m.template_key)?;
        id(&m.slot_id)?;
        if keys
            .insert(m.template_key.clone(), m.slot_id.clone())
            .is_some()
            || !ids.insert(m.slot_id.clone())
        {
            return Err(fail("SLOT_MAPPING_DUPLICATE", "injective explicit maps"));
        }
    }
    Ok(keys)
}
fn catalog_key_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}
fn mapping_key_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.cmp(b)
}
fn initial_value(
    raw: &str,
    previous: bool,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> Result<()> {
    let revision = super::facts::counter_with_work(raw, work)?;
    if !crate::validation_work::compare(work, revision, 1u64)?.is_eq() || previous {
        return Err(fail(
            "UNSUPPORTED_SCHEMA_PROFILE",
            "only initial structural revision supported",
        ));
    }
    Ok(())
}
fn initial(revision: &mut String, previous: Option<&Reference>) -> Result<()> {
    initial_value(
        revision,
        previous.is_some(),
        &mut crate::validation_work::NoopValidationWork,
    )?;
    *revision = "1".into();
    Ok(())
}
fn id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
    {
        return Err(fail("SLOT_ID", "bounded opaque ID"));
    }
    Ok(())
}
fn json_error(error: serde_json::Error) -> Error {
    fail("JSON", &error.to_string())
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}
#[cfg(test)]
mod tests;
