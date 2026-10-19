//! Selected integer/unit/true compiler. The caller must supply its verified
//! binary-owned compiler descriptor as a separate trusted input. Source/DB
//! descriptors cannot acquire executable support by passing shape validation.
//! This pure module creates no authority, operation receipt or durable write.
use super::facts::{counter, name, reject_null};
use super::{FactSchema, Primitive, Product, SlotTemplate, TemplateBook};
use crate::canonical::{self, CanonicalBytes};
use crate::money::Decimal;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RefDto")]
pub struct DefinitionRef {
    kind: String,
    id: String,
    digest: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefDto {
    kind: String,
    id: String,
    digest: String,
}
impl TryFrom<RefDto> for DefinitionRef {
    type Error = Error;
    fn try_from(value: RefDto) -> Result<Self> {
        Self::new(&value.kind, &value.id, &value.digest)
    }
}
impl DefinitionRef {
    /// Syntax only; no retention, ACL or executable-support claim.
    pub fn new(kind: &str, id: &str, digest: &str) -> Result<Self> {
        Self::new_with_work(
            kind,
            id,
            digest,
            &mut crate::validation_work::NoopValidationWork,
        )
    }
    /// The same DATA constructor with executing semantic/copy callbacks. The
    /// caller owns pre-admission of these three strings; this is not provenance.
    pub fn new_with_work(
        kind: &str,
        id: &str,
        digest: &str,
        work: &mut dyn crate::validation_work::ValidationWork,
    ) -> Result<Self> {
        use crate::validation_work::ValidationVisit as Visit;
        let mut prefix = None;
        for (candidate, expected) in [
            ("workflow-product", "wdoc_"),
            ("workflow-fact-schema", "wdoc_"),
            ("workflow-slot-template", "wdoc_"),
            ("workflow-policy", "wdoc_"),
            ("workflow-compiler-descriptor", "wdoc_"),
            ("workflow-compiled-plan", "wplan_"),
        ] {
            work.visit(Visit::MemberCompare)?;
            work.scan_bytes(kind.len())?;
            work.scan_bytes(candidate.len())?;
            if kind == candidate {
                prefix = Some(expected);
                break;
            }
        }
        let prefix =
            prefix.ok_or_else(|| fail("DEFINITION_KIND", "unsupported definition family"))?;
        fn prefixed<'a>(
            value: &'a str,
            prefix: &str,
            work: &mut dyn crate::validation_work::ValidationWork,
        ) -> Result<&'a str> {
            use crate::validation_work::{
                ArithmeticOperation as Op, BigInt, ValidationVisit as Visit,
            };
            work.visit(Visit::Constraint)?;
            work.scan_bytes(prefix.len())?;
            let value = value
                .strip_prefix(prefix)
                .ok_or_else(|| Error::new("IDENTIFIER", "wrong internal ID prefix"))?;
            work.arithmetic(
                Op::Compare,
                &[&BigInt::from(value.len()), &BigInt::from(64u8)],
            )?;
            if value.len() != 64 {
                return Err(Error::new("IDENTIFIER", "expected lowercase SHA-256"));
            }
            for byte in value.bytes() {
                work.visit(Visit::Constraint)?;
                work.scan_bytes(1)?;
                if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte) {
                    return Err(Error::new("IDENTIFIER", "expected lowercase SHA-256"));
                }
            }
            Ok(value)
        }
        let digest_hex = prefixed(digest, "sha256:", work)?;
        let id_hex = prefixed(id, prefix, work)?;
        work.visit(Visit::MemberCompare)?;
        work.scan_bytes(id_hex.len())?;
        work.scan_bytes(digest_hex.len())?;
        if id_hex != digest_hex {
            return Err(fail(
                "DEFINITION_DIGEST_MISMATCH",
                "content-addressed reference",
            ));
        }
        // Charge before each actual owned copy. There is no temporary DTO or
        // opaque serde clone between this borrowed input and its three strings.
        let mut copy = |value: &str| -> Result<String> {
            work.scan_bytes(value.len())?;
            work.write_bytes(value.len())?;
            Ok(value.to_owned())
        };
        Ok(Self {
            kind: copy(kind)?,
            id: copy(id)?,
            digest: copy(digest)?,
        })
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        canonical::dto(canonical::parse(bytes)?)
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Clone, Debug)]
pub struct DefinitionDocument {
    reference: DefinitionRef,
    body: Value,
    bytes: CanonicalBytes,
}
impl DefinitionDocument {
    fn document(kind: &str, body: Value) -> Result<Self> {
        let schema = body["schema"]
            .as_str()
            .ok_or_else(|| fail("INVALID_SHAPE", "document schema"))?;
        let digest = super::document_digest(schema, &body)?;
        Self::with_digest(kind, body, digest, "wdoc_")
    }
    fn with_digest(kind: &str, body: Value, digest: String, prefix: &str) -> Result<Self> {
        let id = format!("{prefix}{}", &digest[7..]);
        let reference = DefinitionRef::new(kind, &id, &digest)?;
        let bytes = CanonicalBytes::from_value(&body)?;
        Ok(Self {
            reference,
            body,
            bytes,
        })
    }
    pub fn reference(&self) -> &DefinitionRef {
        &self.reference
    }
    pub fn kind(&self) -> &str {
        self.reference.kind()
    }
    pub fn id(&self) -> &str {
        self.reference.id()
    }
    pub fn digest(&self) -> &str {
        self.reference.digest()
    }
    pub fn canonical_value(&self) -> &Value {
        &self.body
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

/// One normalized source and its exact typed dependency closure. The host must
/// authenticate dependency origin/readability; this type creates no retention,
/// publication, compiled plan or executable capability.
#[derive(Clone, Debug)]
pub struct NormalizedDefinition {
    document: DefinitionDocument,
    dependencies: Vec<DefinitionDocument>,
}
impl NormalizedDefinition {
    pub fn document(&self) -> &DefinitionDocument {
        &self.document
    }
    pub fn dependencies(&self) -> &[DefinitionDocument] {
        &self.dependencies
    }
    pub fn direct_dependency_refs(&self) -> Result<Vec<DefinitionRef>> {
        source_dependencies(&self.document)
    }
    pub fn descriptor_refs_for(&self, root: &DefinitionRef) -> Result<Vec<DefinitionRef>> {
        let mut stack = vec![root.clone()];
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        while let Some(reference) = stack.pop() {
            if !seen.insert(reference.id.clone()) {
                continue;
            }
            let document = std::iter::once(&self.document)
                .chain(self.dependencies.iter())
                .find(|d| d.reference() == &reference)
                .ok_or_else(|| fail("DEFINITION_MISSING", "descriptor source dependency"))?;
            if matches!(
                document.kind(),
                "workflow-fact-schema" | "workflow-compiler-descriptor"
            ) {
                out.push(reference);
            }
            stack.extend(source_dependencies(document)?);
        }
        canonical::sort_set(&mut out)?;
        Ok(out)
    }
}

/// Normalize a single initial source using exact typed dependency data. Broader
/// policy modes and source ancestry remain explicitly unsupported by this
/// selected implementation. The support token is code-owned, never supplied by
/// a retained/request descriptor. This does not compute a compiled plan.
pub fn normalize_definition(
    raw: &[u8],
    dependencies: &[DefinitionDocument],
    supported: &VerifiedCompilerSupport,
) -> Result<NormalizedDefinition> {
    let normalized = normalize_retained_definition(raw, dependencies)?;
    let compiler = match normalized.document.kind() {
        "workflow-compiler-descriptor" => Some(&normalized.document),
        "workflow-policy" => {
            let reference: DefinitionRef =
                canonical::dto(normalized.document.body["compiler_ref"].clone())?;
            Some(
                normalized
                    .dependencies
                    .iter()
                    .find(|d| d.reference() == &reference)
                    .ok_or_else(|| fail("DEFINITION_MISSING", "compiler dependency"))?,
            )
        }
        _ => None,
    };
    if compiler.is_some_and(|d| {
        d.reference() != supported.descriptor.reference()
            || d.canonical_bytes() != supported.descriptor.canonical_bytes()
    }) {
        return Err(fail(
            "UNSUPPORTED_COMPILER_DESCRIPTOR",
            "binary-owned descriptor mismatch",
        ));
    }
    Ok(normalized)
}

/// Reconstruct selected saved definition data using its typed exact dependency
/// closure, without current compiler support/build or fresh authority. The host
/// must authenticate the original saved recipe and each origin, then compare
/// returned bytes/ref with the retained record. This never creates support.
pub fn normalize_retained_definition(
    raw: &[u8],
    dependencies: &[DefinitionDocument],
) -> Result<NormalizedDefinition> {
    let value = canonical::parse_bounded(raw, 262144)?;
    reject_null(&value)?;
    if dependencies.len() > 127 {
        return Err(fail("DEFINITION_LIMIT", "definition dependencies"));
    }
    // Raw root has its own wire bound. The retained dependency budget sums
    // canonical bodies; equivalent root whitespace must not change that sum.
    let mut total = 0usize;
    let mut ids = BTreeSet::new();
    for dependency in dependencies {
        if dependency.kind() == "workflow-compiled-plan"
            || dependency.canonical_bytes().len() > 262144
            || !ids.insert(dependency.id())
        {
            return Err(fail(
                "DEFINITION_CONFLICT",
                "invalid/duplicate source dependency",
            ));
        }
        total = total
            .checked_add(dependency.canonical_bytes().len())
            .ok_or_else(|| fail("DEFINITION_LIMIT", "definition closure bytes"))?;
        if total > 1048576 {
            return Err(fail("DEFINITION_LIMIT", "definition closure bytes"));
        }
    }
    let document = match value["schema"].as_str() {
        Some("ledger-workflow-fact-schema/1") => DefinitionDocument::document(
            "workflow-fact-schema",
            FactSchema::parse(raw)?.canonical_value().clone(),
        )?,
        Some("ledger-workflow-compiler-descriptor/1") => {
            let descriptor = CompilerDescriptor::parse(raw)?;
            descriptor.document
        }
        Some("ledger-workflow-slot-template/1") => {
            if value.get("previous").is_some() {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "definition template ancestry not implemented",
                ));
            }
            DefinitionDocument::document(
                "workflow-slot-template",
                SlotTemplate::parse(raw, None)?.canonical_value().clone(),
            )?
        }
        Some("ledger-workflow-product/1") => {
            let facts = dependencies
                .iter()
                .filter(|d| d.kind() == "workflow-fact-schema")
                .map(|d| FactSchema::parse(d.canonical_bytes()))
                .collect::<Result<Vec<_>>>()?;
            DefinitionDocument::document(
                "workflow-product",
                Product::parse(raw, &facts)?.canonical_value().clone(),
            )?
        }
        Some("ledger-workflow-policy/1") => {
            let root = DefinitionDocument::document("workflow-policy", normalize_policy(value)?)?;
            let mut closure: Vec<_> = dependencies
                .iter()
                .map(|d| d.canonical_bytes().to_vec())
                .collect();
            closure.push(root.canonical_bytes().to_vec());
            let sources = normalize_publication_sources(&closure, root.reference())?;
            validate_policy_definition(&sources)?;
            root
        }
        _ => {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "source definition family/private data",
            ));
        }
    };
    if document.canonical_bytes().len() > 262144 {
        return Err(fail("DEFINITION_LIMIT", "normalized source bytes"));
    }
    if dependencies
        .iter()
        .map(|d| d.canonical_bytes().len())
        .sum::<usize>()
        + document.canonical_bytes().len()
        > 1048576
    {
        return Err(fail(
            "DEFINITION_LIMIT",
            "normalized definition closure bytes",
        ));
    }
    let mut pending = source_dependencies(&document)?;
    let mut required = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if !required.insert(reference.id.clone()) {
            continue;
        }
        let dependency = dependencies
            .iter()
            .find(|d| d.reference() == &reference)
            .ok_or_else(|| fail("DEFINITION_MISSING", "exact source dependency"))?;
        pending.extend(source_dependencies(dependency)?);
    }
    if required != ids.into_iter().map(str::to_owned).collect() {
        return Err(fail(
            "DEFINITION_EXTRA",
            "exact definition dependency closure",
        ));
    }
    let mut dependencies = dependencies.to_vec();
    dependencies.sort_by(|a, b| a.canonical_bytes().cmp(b.canonical_bytes()));
    Ok(NormalizedDefinition {
        document,
        dependencies,
    })
}
fn source_dependencies(document: &DefinitionDocument) -> Result<Vec<DefinitionRef>> {
    let mut refs = Vec::new();
    let body = document.canonical_value();
    match document.kind() {
        "workflow-product" => {
            for r in body["fact_schemas"]
                .as_array()
                .ok_or_else(|| fail("INVALID_SHAPE", "product schema refs"))?
            {
                refs.push(canonical::dto(r.clone())?);
            }
        }
        "workflow-policy" => {
            for key in [
                "product_ref",
                "context_schema_ref",
                "slot_template_ref",
                "compiler_ref",
            ] {
                refs.push(canonical::dto(body[key].clone())?);
            }
        }
        "workflow-fact-schema" | "workflow-slot-template" | "workflow-compiler-descriptor" => (),
        _ => return Err(fail("DEFINITION_KIND", "source dependency family")),
    }
    if body.get("previous").is_some() {
        return Err(fail(
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
            "source ancestry not implemented",
        ));
    }
    canonical::sort_set(&mut refs)?;
    Ok(refs)
}
fn validate_policy_definition(sources: &NormalizedPublicationSources) -> Result<()> {
    let product = &sources.product;
    let policy = &sources.policy;
    let slot = &policy.binding_slots[0];
    if product.links().next().is_some()
        || product.units().any(|u| u.normalization().is_some())
        || product
            .activities()
            .any(|a| a.primitive() != Primitive::Work)
    {
        return Err(fail(
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
            "selected policy source profile",
        ));
    }
    let schemas = product
        .activities()
        .map(|a| {
            product
                .fact_schema(a.fact_schema_id())
                .expect("validated schema")
        })
        .chain(std::iter::once(&sources.context));
    for schema in schemas {
        for field in schema.canonical_value()["fields"]
            .as_array()
            .expect("validated fields")
        {
            if field["type"]["kind"] != "integer"
                || field["required"] != true
                || field["provenance"]["kind"] != "caller"
                || field["type"]["min"]
                    .as_str()
                    .is_some_and(|s| s.starts_with('-'))
            {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "selected integer source profile",
                ));
            }
        }
    }
    for activity in &slot.allowed_activities {
        if product.activity(activity).is_none() {
            return Err(fail("LINK_ACTIVITY_MISSING", "binding slot activity"));
        }
    }
    let mut producers = BTreeSet::new();
    for rule in &policy.rules {
        let activity = product
            .activity(&rule.activity)
            .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "rule activity"))?;
        if rule.binding_slot != slot.name
            || !slot.allowed_activities.contains(&rule.activity)
            || activity
                .funding_leg()
                .is_some_and(|leg| leg != slot.funding_leg)
        {
            return Err(fail(
                "PROGRAM_BINDING",
                "rule binding/work funding mismatch",
            ));
        }
        let component = sources
            .template
            .component_named(&rule.component_key, &rule.component_name)?;
        if component.book() != slot.book || component.root_key() != component.key() {
            return Err(fail("PROGRAM_COMPONENT", "unit root/book mismatch"));
        }
        if !producers.insert((&rule.activity, &rule.component_key)) {
            return Err(fail(
                "PROGRAM_DUPLICATE_RULE",
                "activity/component producer",
            ));
        }
        let schema = product
            .fact_schema(activity.fact_schema_id())
            .expect("validated schema");
        if schema.quantity_unit(&rule.formula.quantity_field)? != rule.formula.unit
            || rule.formula.normalize_to.is_some()
        {
            return Err(fail("UNIT_MISMATCH", "terminal formula/field unit"));
        }
    }
    Ok(())
}

/// Extract exact typed direct dependencies from closed candidate syntax before
/// storage resolution. These locators are untrusted discovery hints: complete
/// type/closure validation still requires normalize_retained_definition, and
/// fresh retention additionally requires normalize_definition/code support.
pub fn candidate_dependency_refs(raw: &[u8]) -> Result<Vec<DefinitionRef>> {
    let value = canonical::parse_bounded(raw, 262144)?;
    reject_null(&value)?;
    match value["schema"].as_str() {
        Some("ledger-workflow-product/1") => super::product::candidate_fact_schema_refs(raw),
        Some("ledger-workflow-policy/1") => {
            let policy: PolicyDto = canonical::dto(normalize_policy(value)?)?;
            let mut refs = Vec::new();
            for (r, kind) in [
                (policy.product_ref, "workflow-product"),
                (policy.context_schema_ref, "workflow-fact-schema"),
                (policy.slot_template_ref, "workflow-slot-template"),
                (policy.compiler_ref, "workflow-compiler-descriptor"),
            ] {
                if r.kind() != kind {
                    return Err(fail("DEFINITION_KIND", "candidate dependency family"));
                }
                refs.push(r);
            }
            canonical::sort_set(&mut refs)?;
            Ok(refs)
        }
        Some("ledger-workflow-fact-schema/1") => {
            FactSchema::parse(raw)?;
            Ok(Vec::new())
        }
        Some("ledger-workflow-slot-template/1") => {
            if value.get("previous").is_some() {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "source ancestry not implemented",
                ));
            }
            SlotTemplate::parse(raw, None)?;
            Ok(Vec::new())
        }
        Some("ledger-workflow-compiler-descriptor/1") => {
            CompilerDescriptor::parse(raw)?;
            Ok(Vec::new())
        }
        _ => Err(fail(
            "UNSUPPORTED_SCHEMA_PROFILE",
            "candidate source family/private data",
        )),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptorDto {
    schema: String,
    compiler_semantics: String,
    evaluator: String,
    grammar: String,
    facts_normalization: String,
    operation_normalization: String,
    limits_profile: String,
    primitive_versions: Vec<String>,
    fact_types: Vec<String>,
    predicate_ops: Vec<String>,
    formula_kinds: Vec<String>,
    plan_schema: String,
    limits_digest: String,
    mode_matrix_digest: String,
    contract_manifest_digest: String,
}
#[derive(Clone, Debug)]
pub struct CompilerDescriptor {
    document: DefinitionDocument,
}
impl CompilerDescriptor {
    /// Validates descriptor data. This does not attest that a supplied/retained
    /// descriptor belongs to the executing binary. Coordinator must establish
    /// that separate support anchor before enabling any publication runtime.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = canonical::parse(bytes)?;
        reject_null(&value)?;
        let mut dto: DescriptorDto = canonical::dto(value)?;
        if matches!(
            dto.schema.as_str(),
            "ledger-workflow-compiler-descriptor/5" | "ledger-workflow-compiler-descriptor/6"
        ) {
            return Self::app_cost_data(dto);
        }
        if matches!(
            dto.schema.as_str(),
            "ledger-workflow-compiler-descriptor/3" | "ledger-workflow-compiler-descriptor/4"
        ) {
            return Self::supplier_cost_data(dto);
        }
        if dto.schema == "ledger-workflow-compiler-descriptor/2" {
            return Self::family_close_data(dto);
        }
        if dto.schema != "ledger-workflow-compiler-descriptor/1"
            || dto.compiler_semantics != "ledger-policy-compiler/1"
            || dto.evaluator != "ledger-workflow-evaluator/1"
            || dto.grammar != "ledger-policy-grammar/1"
            || dto.facts_normalization != "ledger-facts-normalization/1"
            || dto.operation_normalization != "ledger-operation-normalization/2"
            || dto.limits_profile != "workflow-limits/1"
            || dto.plan_schema != "ledger-workflow-compiled-plan/1"
        {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "compiler descriptor profiles",
            ));
        }
        for (values, supported) in [
            (&mut dto.primitive_versions, "unit/1"),
            (&mut dto.fact_types, "integer/1"),
            (&mut dto.predicate_ops, "true/1"),
            (&mut dto.formula_kinds, "unit/1"),
        ] {
            canonical::sort_set(values)?;
            if values.as_slice() != [supported] {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "selected compiler supports integer/unit/true only",
                ));
            }
        }
        for digest in [
            &dto.limits_digest,
            &dto.mode_matrix_digest,
            &dto.contract_manifest_digest,
        ] {
            crate::domain::prefixed(digest, "sha256:")?;
        }
        let body = serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?;
        Ok(Self {
            document: DefinitionDocument::document("workflow-compiler-descriptor", body)?,
        })
    }
    /// Closed Descriptor2 DATA only. Its declarations never become a binary
    /// support anchor through this parser. The selected compiler must compare
    /// the entire descriptor with its own actually implemented capability.
    fn family_close_data(mut dto: DescriptorDto) -> Result<Self> {
        if dto.compiler_semantics != "ledger-policy-compiler/family-close-1"
            || dto.evaluator != "ledger-workflow-evaluator/1"
            || dto.grammar != "ledger-policy-grammar/1"
            || dto.facts_normalization != "ledger-facts-normalization/1"
            || dto.operation_normalization != "ledger-operation-normalization/2"
            || dto.limits_profile != "workflow-limits/1"
            || dto.plan_schema != "ledger-workflow-compiled-plan/2"
        {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "family-close descriptor profiles",
            ));
        }
        for values in [
            &mut dto.primitive_versions,
            &mut dto.fact_types,
            &mut dto.predicate_ops,
            &mut dto.formula_kinds,
        ] {
            if values.len() > 64
                || values.iter().any(|value| {
                    value.is_empty()
                        || value.chars().count() > 128
                        || value.chars().any(|c| c <= '\u{001f}' || c == '\u{007f}')
                })
            {
                return Err(fail(
                    "INVALID_SHAPE",
                    "family-close descriptor declaration set",
                ));
            }
            canonical::sort_set(values)?;
        }
        for required in [
            "family_no_success_close/1",
            "outcome_family_binding_dependency/1",
        ] {
            if !dto
                .primitive_versions
                .iter()
                .any(|primitive| primitive == required)
            {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "family-close descriptor declaration",
                ));
            }
        }
        for digest in [
            &dto.limits_digest,
            &dto.mode_matrix_digest,
            &dto.contract_manifest_digest,
        ] {
            crate::domain::prefixed(digest, "sha256:")?;
        }
        let body = serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?;
        Ok(Self {
            document: DefinitionDocument::document("workflow-compiler-descriptor", body)?,
        })
    }
    /// Closed Descriptor3 DATA, never proof of a compiler or financial support.
    /// The real selected compiler must bind its complete code-owned capability
    /// and resource envelope independently before publication/admission.
    fn supplier_cost_data(mut dto: DescriptorDto) -> Result<Self> {
        let scoped = dto.schema == "ledger-workflow-compiler-descriptor/4";
        let (semantics, plan) = if scoped {
            (
                "ledger-policy-compiler/supplier-scoped-consent-1",
                "ledger-workflow-compiled-plan/4",
            )
        } else {
            (
                "ledger-policy-compiler/owner-local-cost-1",
                "ledger-workflow-compiled-plan/3",
            )
        };
        if dto.compiler_semantics != semantics
            || dto.evaluator != "ledger-workflow-evaluator/1"
            || dto.grammar != "ledger-policy-grammar/1"
            || dto.facts_normalization != "ledger-facts-normalization/1"
            || dto.operation_normalization != "ledger-operation-normalization/2"
            || dto.limits_profile != "workflow-limits/1"
            || dto.plan_schema != plan
        {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "supplier descriptor profiles",
            ));
        }
        for values in [
            &mut dto.primitive_versions,
            &mut dto.fact_types,
            &mut dto.predicate_ops,
            &mut dto.formula_kinds,
        ] {
            if values.len() > 64
                || values.iter().any(|v| {
                    v.is_empty()
                        || v.chars().count() > 128
                        || v.chars().any(|c| c <= '\u{001f}' || c == '\u{007f}')
                })
            {
                return Err(fail("INVALID_SHAPE", "supplier descriptor declarations"));
            }
            canonical::sort_set(values)?;
        }
        for required in [
            "family_no_success_close/1",
            "outcome_family_binding_dependency/1",
            "owner_local_supplier_cost/1",
            "owned_obligation_high_water/1",
        ] {
            if !dto.primitive_versions.iter().any(|p| p == required) {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "supplier descriptor declaration",
                ));
            }
        }
        if scoped
            && !dto
                .primitive_versions
                .iter()
                .any(|p| p == "supplier-semantic-projection/1")
        {
            return Err(fail(
                "PROGRAM_UNSUPPORTED_PRIMITIVE",
                "supplier scoped projection declaration",
            ));
        }
        for digest in [
            &dto.limits_digest,
            &dto.mode_matrix_digest,
            &dto.contract_manifest_digest,
        ] {
            crate::domain::prefixed(digest, "sha256:")?;
        }
        let body = serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?;
        Ok(Self {
            document: DefinitionDocument::document("workflow-compiler-descriptor", body)?,
        })
    }
    /// Closed app descriptor DATA. Only actual binary support may enable it.
    fn app_cost_data(mut dto: DescriptorDto) -> Result<Self> {
        let (semantics, plan) = if dto.schema == "ledger-workflow-compiler-descriptor/6" {
            (
                "ledger-policy-compiler/app-cost-causal-1",
                "ledger-workflow-compiled-plan/6",
            )
        } else {
            (
                "ledger-policy-compiler/app-owned-cost-1",
                "ledger-workflow-compiled-plan/5",
            )
        };
        if dto.compiler_semantics != semantics
            || dto.plan_schema != plan
            || dto.evaluator != "ledger-workflow-evaluator/1"
            || dto.grammar != "ledger-policy-grammar/1"
            || dto.facts_normalization != "ledger-facts-normalization/1"
            || dto.operation_normalization != "ledger-operation-normalization/2"
            || dto.limits_profile != "workflow-limits/1"
        {
            return Err(fail(
                "UNSUPPORTED_SCHEMA_PROFILE",
                "app descriptor profiles",
            ));
        }
        for values in [
            &mut dto.primitive_versions,
            &mut dto.fact_types,
            &mut dto.predicate_ops,
            &mut dto.formula_kinds,
        ] {
            if values.len() > 64
                || values.iter().any(|v| {
                    v.is_empty()
                        || v.chars().count() > 128
                        || v.chars().any(|c| c <= '\u{001f}' || c == '\u{007f}')
                })
            {
                return Err(fail("INVALID_SHAPE", "app descriptor declarations"));
            }
            canonical::sort_set(values)?;
        }
        for digest in [
            &dto.limits_digest,
            &dto.mode_matrix_digest,
            &dto.contract_manifest_digest,
        ] {
            crate::domain::prefixed(digest, "sha256:")?;
        }
        let body = serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))?;
        Ok(Self {
            document: DefinitionDocument::document("workflow-compiler-descriptor", body)?,
        })
    }
    pub fn reference(&self) -> &DefinitionRef {
        self.document.reference()
    }
    pub fn canonical_value(&self) -> &Value {
        self.document.canonical_value()
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.canonical_bytes()
    }
}

/// Binary-owned support anchor. There is no public request/DB/config constructor.
/// The builtin factory verifies immutable embedded contract artifacts against
/// the implemented capability set. Synthetic support exists only in tests.
#[derive(Clone, Debug)]
pub struct VerifiedCompilerSupport {
    descriptor: CompilerDescriptor,
}
impl VerifiedCompilerSupport {
    /// Derive support exclusively from the artifacts embedded in this binary.
    /// Request, database and ambient configuration never supply these bytes.
    pub fn builtin() -> Result<Self> {
        let limits = canonical::parse(include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/runtime-support/limits.json"
        ))?;
        let mode = canonical::parse(include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/runtime-support/mode-matrix.json"
        ))?;
        let contract = canonical::parse(include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/runtime-support/contract-profile.json"
        ))?;
        let expected_limits = json!({"schema":"ledger-workflow-builtin-limits/1","profile":"workflow-limits/1","document_bytes":262144,"source_documents":128,"source_closure_bytes":1048576,"fact_slots":1088,"ast_nodes":256,"predicate_depth":8,"predicate_cells":4096,"rules":64,"graph_hops":16,"loaded_refs":4096,"actions":128,"explanations":256,"canonical_decision_bytes":4194304,"resolved_bytes":8388608,"compile_visits":1048576,"runtime_visits":65536,"max_atoms":"999999999999999999999999999999"});
        let expected_mode = json!({"schema":"ledger-workflow-builtin-mode-matrix/1","profile":"retail-unit-publication/1","scope":"compile-publication-only","policy_modes":["component_workflow"],"books":["retail"],"phases":["base"],"primitives":["unit/1"],"fact_types":["integer/1"],"predicate_ops":["true/1"],"formula_kinds":["unit/1"],"fact_constraints":["bounded-nonnegative","caller","required"],"activity_primitives":["work"],"excluded":["basis","caps","delayed-cost","families","links","optional-facts","shares","stages","unit-conversion"],"supports_previous_sources":false,"requires_terminal_units":true,"aggregate_proof":"positive-root-maxima-by-activity-book-slot-and-activity-book/1"});
        if limits != expected_limits || mode != expected_mode {
            return Err(fail(
                "UNSUPPORTED_COMPILER_DESCRIPTOR",
                "embedded catalog differs from implemented support",
            ));
        }
        let files: [(&str, &[u8]); 5] = [
            (
                "normative-amendment.md",
                include_bytes!(
                    "../../../../../contracts/candidates/full-suite-v1/core/normative-amendment.md"
                ),
            ),
            (
                "publication-boundary.md",
                include_bytes!(
                    "../../../../../contracts/candidates/full-suite-v1/core/publication-boundary.md"
                ),
            ),
            (
                "registry-contract.json",
                include_bytes!(
                    "../../../../../contracts/candidates/full-suite-v1/core/registry-contract.json"
                ),
            ),
            (
                "workflow.schema.json",
                include_bytes!(
                    "../../../../../contracts/candidates/full-suite-v1/core/workflow.schema.json"
                ),
            ),
            (
                "publication-runtime-seams.md",
                include_bytes!(
                    "../../../../../contracts/candidates/full-suite-v1/core/publication-runtime-seams.md"
                ),
            ),
        ];
        let expected_contract = json!({"schema":"ledger-workflow-builtin-contract-profile/1","profile":"PUB-ENC-01+PUB-RUNTIME-SEAMS-01","encoding_files":files[..4].iter().map(|(path,bytes)|json!({"path":path,"sha256":raw_digest(bytes)})).collect::<Vec<_>>(),"semantic_amendment":{"path":files[4].0,"sha256":raw_digest(files[4].1)}});
        if contract != expected_contract {
            return Err(fail(
                "UNSUPPORTED_COMPILER_DESCRIPTOR",
                "embedded contract profile membership/digests",
            ));
        }
        let descriptor = json!({"schema":"ledger-workflow-compiler-descriptor/1","compiler_semantics":"ledger-policy-compiler/1","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","facts_normalization":"ledger-facts-normalization/1","operation_normalization":"ledger-operation-normalization/2","limits_profile":"workflow-limits/1","primitive_versions":mode["primitives"],"fact_types":mode["fact_types"],"predicate_ops":mode["predicate_ops"],"formula_kinds":mode["formula_kinds"],"plan_schema":"ledger-workflow-compiled-plan/1","limits_digest":raw_digest(CanonicalBytes::from_value(&limits)?.as_slice()),"mode_matrix_digest":raw_digest(CanonicalBytes::from_value(&mode)?.as_slice()),"contract_manifest_digest":raw_digest(CanonicalBytes::from_value(&contract)?.as_slice())});
        Ok(Self {
            descriptor: CompilerDescriptor::parse(
                CanonicalBytes::from_value(&descriptor)?.as_slice(),
            )?,
        })
    }
    pub fn descriptor(&self) -> &CompilerDescriptor {
        &self.descriptor
    }
    #[cfg(test)]
    fn fixture(descriptor: CompilerDescriptor) -> Result<Self> {
        if descriptor.reference().id()
            != "wdoc_9429a85f7a411a37980a99f1f01cfca42e3fde4a71e58de8d1c157572f664c75"
        {
            return Err(fail(
                "UNSUPPORTED_COMPILER_DESCRIPTOR",
                "not the independently fixed synthetic test descriptor",
            ));
        }
        Ok(Self { descriptor })
    }
}
fn raw_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", canonical::hex(&Sha256::digest(bytes)))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingSlot {
    name: String,
    book: TemplateBook,
    funding_leg: String,
    allowed_activities: Vec<String>,
    correction_source_slot: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Predicate {
    op: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Formula {
    kind: String,
    quantity_field: String,
    unit: String,
    rate: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normalize_to: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    rule_id: String,
    activity: String,
    component_name: String,
    component_key: String,
    binding_slot: String,
    phase: String,
    when: Predicate,
    formula: Formula,
    dependency: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDto {
    schema: String,
    policy_id: String,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<DefinitionRef>,
    product_ref: DefinitionRef,
    context_schema_ref: DefinitionRef,
    slot_template_ref: DefinitionRef,
    compiler_ref: DefinitionRef,
    currency: String,
    scale: u8,
    rounding: String,
    mode: String,
    binding_slots: Vec<BindingSlot>,
    rules: Vec<Rule>,
    families: Vec<Value>,
    stages: Vec<Value>,
    delayed_costs: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocation: Option<Value>,
}
fn normalize_policy(value: Value) -> Result<Value> {
    // Reject unsupported AST forms explicitly, before typed alternatives can
    // drop fields or normalize a source that the selected compiler cannot read.
    if let Some(rules) = value.get("rules").and_then(Value::as_array) {
        for rule in rules {
            if rule
                .get("formula")
                .and_then(|v| v.get("kind"))
                .and_then(Value::as_str)
                != Some("unit")
            {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "unit formula required",
                ));
            }
            if rule
                .get("when")
                .and_then(|v| v.get("op"))
                .and_then(Value::as_str)
                != Some("true")
            {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "true predicate required",
                ));
            }
        }
    }
    let mut dto: PolicyDto = canonical::dto(value)?;
    if dto.schema != "ledger-workflow-policy/1"
        || dto.currency != "USD"
        || dto.scale != 18
        || dto.rounding != "nearest_ties_away"
    {
        return Err(fail("UNSUPPORTED_SCHEMA_PROFILE", "policy money/profile"));
    }
    if dto.mode != "component_workflow"
        || !dto.families.is_empty()
        || !dto.stages.is_empty()
        || !dto.delayed_costs.is_empty()
        || dto.allocation.is_some()
        || dto.previous.is_some()
    {
        return Err(fail(
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
            "selected publication compiler excludes outcomes/stages/costs/allocation/previous policy closure",
        ));
    }
    name(&dto.policy_id)?;
    let version = counter(&dto.version)?;
    if version == 0 {
        return Err(fail("PRODUCT_VERSION", "policy version"));
    }
    dto.version = version.to_string();
    if dto.binding_slots.len() != 1 || dto.rules.is_empty() || dto.rules.len() > 64 {
        return Err(fail("PROGRAM_LIMIT", "selected retail slot/rules"));
    }
    for slot in &mut dto.binding_slots {
        name(&slot.name)?;
        name(&slot.funding_leg)?;
        name(&slot.correction_source_slot)?;
        if slot.book != TemplateBook::Retail {
            return Err(fail(
                "PROGRAM_UNSUPPORTED_PRIMITIVE",
                "retail unit source only",
            ));
        }
        if slot.allowed_activities.is_empty() || slot.allowed_activities.len() > 16 {
            return Err(fail("PROGRAM_LIMIT", "allowed activities"));
        }
        for activity in &slot.allowed_activities {
            name(activity)?;
        }
        canonical::sort_set(&mut slot.allowed_activities)?;
    }
    let mut rules = BTreeSet::new();
    for rule in &dto.rules {
        for value in [
            &rule.rule_id,
            &rule.activity,
            &rule.component_name,
            &rule.component_key,
            &rule.binding_slot,
            &rule.formula.quantity_field,
            &rule.formula.unit,
        ] {
            name(value)?;
        }
        if let Some(to) = &rule.formula.normalize_to {
            name(to)?;
        }
        if !rules.insert(&rule.rule_id) {
            return Err(fail("PROGRAM_DUPLICATE_RULE", "rule identity"));
        }
        if rule.phase != "base" || rule.dependency != "on_missing_refuse" {
            return Err(fail(
                "PROGRAM_UNSUPPORTED_PRIMITIVE",
                "base/no-missing-ref unit source only",
            ));
        }
    }
    serde_json::to_value(dto).map_err(|e| fail("JSON", &e.to_string()))
}

/// Canonical source closure, validated without computing a compiled plan.
/// Original request reconciliation can use this without rerating/recompiling.
#[derive(Clone, Debug)]
pub struct NormalizedPublicationSources {
    documents: Vec<DefinitionDocument>,
    root: DefinitionRef,
    product: Product,
    template: SlotTemplate,
    context: FactSchema,
    compiler: CompilerDescriptor,
    policy: PolicyDto,
}
impl NormalizedPublicationSources {
    pub fn source_documents(&self) -> &[DefinitionDocument] {
        &self.documents
    }
    pub fn root_policy_ref(&self) -> &DefinitionRef {
        &self.root
    }
    pub fn compiler_descriptor(&self) -> &CompilerDescriptor {
        &self.compiler
    }
    pub fn descriptor_refs(&self) -> Result<Vec<DefinitionRef>> {
        let mut refs: Vec<_> = self
            .documents
            .iter()
            .filter(|d| {
                matches!(
                    d.kind(),
                    "workflow-fact-schema" | "workflow-compiler-descriptor"
                )
            })
            .map(|d| d.reference.clone())
            .collect();
        canonical::sort_set(&mut refs)?;
        Ok(refs)
    }
}

pub fn normalize_publication_sources(
    raw_documents: &[Vec<u8>],
    root_policy_ref: &DefinitionRef,
) -> Result<NormalizedPublicationSources> {
    if raw_documents.is_empty()
        || raw_documents.len() > 128
        || root_policy_ref.kind() != "workflow-policy"
    {
        return Err(fail("DEFINITION_LIMIT", "source closure/root"));
    }
    let mut total = 0usize;
    let mut documents = BTreeMap::new();
    let mut facts = Vec::new();
    let mut deferred = Vec::new();
    for bytes in raw_documents {
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| fail("DEFINITION_LIMIT", "source bytes"))?;
        if total > 1024 * 1024 {
            return Err(fail("DEFINITION_LIMIT", "source closure bytes"));
        }
        let value = canonical::parse(bytes)?;
        reject_null(&value)?;
        match value.get("schema").and_then(Value::as_str) {
            Some("ledger-workflow-fact-schema/1") => {
                let schema = FactSchema::parse(bytes)?;
                let doc = DefinitionDocument::document(
                    "workflow-fact-schema",
                    schema.canonical_value().clone(),
                )?;
                insert_source(&mut documents, doc)?;
                facts.push(schema);
            }
            Some("ledger-workflow-slot-template/1") => {
                if value.get("previous").is_some() {
                    return Err(fail(
                        "PROGRAM_UNSUPPORTED_PRIMITIVE",
                        "template ancestry closure not yet compiled",
                    ));
                }
                let template = SlotTemplate::parse(bytes, None)?;
                let doc = DefinitionDocument::document(
                    "workflow-slot-template",
                    template.canonical_value().clone(),
                )?;
                insert_source(&mut documents, doc)?;
            }
            Some("ledger-workflow-compiler-descriptor/1") => {
                let descriptor = CompilerDescriptor::parse(bytes)?;
                insert_source(&mut documents, descriptor.document)?;
            }
            Some("ledger-workflow-product/1" | "ledger-workflow-policy/1") => deferred.push(value),
            _ => {
                return Err(fail(
                    "UNSUPPORTED_SCHEMA_PROFILE",
                    "source definition family/private data",
                ));
            }
        }
    }
    for value in deferred
        .iter()
        .filter(|v| v["schema"] == "ledger-workflow-product/1")
    {
        let product = Product::parse(CanonicalBytes::from_value(value)?.as_slice(), &facts)?;
        let doc =
            DefinitionDocument::document("workflow-product", product.canonical_value().clone())?;
        insert_source(&mut documents, doc)?;
    }
    for value in deferred
        .into_iter()
        .filter(|v| v["schema"] == "ledger-workflow-policy/1")
    {
        let body = normalize_policy(value)?;
        let doc = DefinitionDocument::document("workflow-policy", body)?;
        insert_source(&mut documents, doc)?;
    }
    let policy_doc = documents
        .get(root_policy_ref.id())
        .ok_or_else(|| fail("DEFINITION_MISSING", "root policy"))?;
    if &policy_doc.reference != root_policy_ref {
        return Err(fail("DEFINITION_DIGEST_MISMATCH", "root policy"));
    }
    let policy: PolicyDto = canonical::dto(policy_doc.body.clone())?;
    let resolve = |reference: &DefinitionRef, kind: &str| -> Result<&DefinitionDocument> {
        if reference.kind() != kind {
            return Err(fail("DEFINITION_KIND", "source dependency"));
        }
        let document = documents
            .get(reference.id())
            .ok_or_else(|| fail("DEFINITION_MISSING", "source dependency"))?;
        if &document.reference != reference {
            return Err(fail("DEFINITION_DIGEST_MISMATCH", "source dependency"));
        }
        Ok(document)
    };
    let product_doc = resolve(&policy.product_ref, "workflow-product")?;
    let context_doc = resolve(&policy.context_schema_ref, "workflow-fact-schema")?;
    let template_doc = resolve(&policy.slot_template_ref, "workflow-slot-template")?;
    let compiler_doc = resolve(&policy.compiler_ref, "workflow-compiler-descriptor")?;
    let product = Product::parse(product_doc.canonical_bytes(), &facts)?;
    let context = FactSchema::parse(context_doc.canonical_bytes())?;
    context.validate_context_schema()?;
    let template = SlotTemplate::parse(template_doc.canonical_bytes(), None)?;
    let compiler = CompilerDescriptor::parse(compiler_doc.canonical_bytes())?;
    let mut expected = BTreeSet::from([
        root_policy_ref.id().to_owned(),
        policy.product_ref.id().to_owned(),
        policy.context_schema_ref.id().to_owned(),
        policy.slot_template_ref.id().to_owned(),
        policy.compiler_ref.id().to_owned(),
    ]);
    for reference in product.canonical_value()["fact_schemas"]
        .as_array()
        .expect("validated refs")
    {
        let reference: DefinitionRef = canonical::dto(reference.clone())?;
        resolve(&reference, "workflow-fact-schema")?;
        expected.insert(reference.id().to_owned());
    }
    if expected != documents.keys().cloned().collect() {
        return Err(fail(
            "DEFINITION_EXTRA",
            "exact current source closure required",
        ));
    }
    let mut schema_keys = BTreeSet::new();
    for document in documents
        .values()
        .filter(|d| d.kind() == "workflow-fact-schema")
    {
        let body = document.canonical_value();
        if !schema_keys.insert((
            body["schema_id"].as_str().expect("validated identity"),
            body["version"].as_str().expect("validated version"),
        )) {
            return Err(fail("DEFINITION_CONFLICT", "schema identity/version"));
        }
    }
    let mut documents: Vec<_> = documents.into_values().collect();
    documents.sort_by(|a, b| a.canonical_bytes().cmp(b.canonical_bytes()));
    Ok(NormalizedPublicationSources {
        documents,
        root: root_policy_ref.clone(),
        product,
        template,
        context,
        compiler,
        policy,
    })
}

#[derive(Clone, Debug)]
pub struct PreparedPublication {
    sources: NormalizedPublicationSources,
    plan: DefinitionDocument,
}
impl PreparedPublication {
    pub fn source_documents(&self) -> &[DefinitionDocument] {
        self.sources.source_documents()
    }
    pub fn root_policy_ref(&self) -> &DefinitionRef {
        self.sources.root_policy_ref()
    }
    pub fn compiler_descriptor(&self) -> &CompilerDescriptor {
        self.sources.compiler_descriptor()
    }
    pub fn compiled_plan(&self) -> &DefinitionDocument {
        &self.plan
    }
    pub fn descriptor_refs(&self) -> Result<Vec<DefinitionRef>> {
        self.sources.descriptor_refs()
    }
}

/// Compile against a separately verified binary-owned support anchor. Parsing
/// a descriptor from the request does not establish that anchor. Production
/// facade wiring stays held until that trusted host contract is implemented.
pub fn compile_publication(
    raw_documents: &[Vec<u8>],
    root_policy_ref: &DefinitionRef,
    expected_plan_digest: &str,
    supported: &VerifiedCompilerSupport,
) -> Result<PreparedPublication> {
    let sources = normalize_publication_sources(raw_documents, root_policy_ref)?;
    if sources.compiler.reference() != supported.descriptor.reference()
        || sources.compiler.canonical_bytes() != supported.descriptor.canonical_bytes()
    {
        return Err(fail(
            "UNSUPPORTED_COMPILER_DESCRIPTOR",
            "binary-owned descriptor mismatch",
        ));
    }
    let policy = &sources.policy;
    let product = &sources.product;
    let template = &sources.template;
    // The binary-owned runtime catalog advertises a deliberately narrower
    // compiler subset than the reusable declaration parsers. No unused broader
    // construct may be smuggled through under that support descriptor.
    if product.links().next().is_some()
        || product.units().any(|unit| unit.normalization().is_some())
        || product
            .activities()
            .any(|activity| activity.primitive() != Primitive::Work)
    {
        return Err(fail(
            "PROGRAM_UNSUPPORTED_PRIMITIVE",
            "selected compiler excludes links/unit conversion/outcome activities",
        ));
    }
    let slot = &policy.binding_slots[0];
    let mut activity_table = Vec::new();
    let mut fact_slots = Vec::new();
    let mut schema_refs = BTreeMap::new();
    for activity in product.activities() {
        let mut row = json!({"activity":activity.name(),"primitive":activity.primitive(),"fact_schema_ref":DefinitionRef::new("workflow-fact-schema",activity.fact_schema_id(),activity.fact_schema_digest())?,"source_slot":activity.source_slot()});
        if let Some(leg) = activity.funding_leg() {
            row["funding_leg"] = json!(leg);
        }
        activity_table.push(row);
        schema_refs.insert(
            activity.fact_schema_digest().to_owned(),
            DefinitionRef::new(
                "workflow-fact-schema",
                activity.fact_schema_id(),
                activity.fact_schema_digest(),
            )?,
        );
    }
    schema_refs.insert(
        policy.context_schema_ref.digest().to_owned(),
        policy.context_schema_ref.clone(),
    );
    for reference in schema_refs.values() {
        let schema = if reference == &policy.context_schema_ref {
            &sources.context
        } else {
            product
                .fact_schema(reference.id())
                .expect("validated source schema")
        };
        for field in schema.canonical_value()["fields"]
            .as_array()
            .expect("validated fields")
        {
            if field["type"]["kind"] != "integer" {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "integer descriptor field only",
                ));
            }
            if field["provenance"]["kind"] != "caller" {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "attested field verifier not implemented by selected compiler",
                ));
            }
            if field["required"] != true
                || field["type"]["min"]
                    .as_str()
                    .expect("validated numeric bound")
                    .starts_with('-')
            {
                return Err(fail(
                    "PROGRAM_UNSUPPORTED_PRIMITIVE",
                    "selected compiler requires nonnegative required integer fields",
                ));
            }
            let index = fact_slots.len();
            if index >= 1088 {
                return Err(fail("PROGRAM_LIMIT", "aggregate fact slots"));
            }
            fact_slots
                .push(json!({"index":index.to_string(),"schema_ref":reference,"field":field}));
        }
    }
    for activity in &slot.allowed_activities {
        if product.activity(activity).is_none() {
            return Err(fail("LINK_ACTIVITY_MISSING", "binding slot activity"));
        }
    }
    let mut producers = BTreeSet::new();
    let mut instructions = Vec::new();
    let mut rule_order = Vec::new();
    let mut explanations = Vec::new();
    let mut maximum_by_activity = BTreeMap::new();
    for (ordinal, rule) in policy.rules.iter().enumerate() {
        let activity = product
            .activity(&rule.activity)
            .ok_or_else(|| fail("LINK_ACTIVITY_MISSING", "rule activity"))?;
        if activity.primitive() != Primitive::Work
            || rule.binding_slot != slot.name
            || !slot.allowed_activities.contains(&rule.activity)
            || activity
                .funding_leg()
                .is_some_and(|leg| leg != slot.funding_leg)
        {
            return Err(fail(
                "PROGRAM_BINDING",
                "rule/retail slot/work funding mismatch",
            ));
        }
        let component = template.component_named(&rule.component_key, &rule.component_name)?;
        if component.book() != slot.book || component.root_key() != component.key() {
            return Err(fail("PROGRAM_COMPONENT", "unit root/book/role mismatch"));
        }
        if !producers.insert((&rule.activity, &rule.component_key)) {
            return Err(fail(
                "PROGRAM_DUPLICATE_RULE",
                "activity/component producer",
            ));
        }
        let schema = product
            .fact_schema(activity.fact_schema_id())
            .expect("validated activity schema");
        if schema.quantity_unit(&rule.formula.quantity_field)? != rule.formula.unit {
            return Err(fail("UNIT_MISMATCH", "formula/field unit"));
        }
        let field = schema.canonical_value()["fields"]
            .as_array()
            .expect("validated fields")
            .iter()
            .find(|f| f["name"] == rule.formula.quantity_field)
            .expect("validated quantity");
        let mut maximum = schema.normalize_quantity(
            &rule.formula.quantity_field,
            CanonicalBytes::from_value(&field["type"]["max"])?.as_slice(),
        )?;
        let mut ratio = match &mut maximum {
            super::FactValue::Integer(value) => value.ratio(),
            _ => return Err(fail("QUANTITY_TYPE", "integer quantity required")),
        };
        let unit = product
            .unit(&rule.formula.unit)
            .expect("validated schema unit");
        match (unit.normalization(), &rule.formula.normalize_to) {
            (Some((to, factor)), Some(actual)) if to == actual => ratio = ratio.mul(factor)?,
            (None, None) => (),
            _ => {
                return Err(fail(
                    "UNIT_MISMATCH",
                    "explicit terminal unit normalization required",
                ));
            }
        }
        let maximum = ratio
            .mul(&rule.formula.rate.atoms_ratio(18)?)?
            .round_atoms()?;
        let accumulated = maximum_by_activity.entry(&rule.activity).or_insert(0);
        *accumulated = crate::money::add_atoms(*accumulated, maximum)?;
        let index = fact_slots
            .iter()
            .find(|s| {
                s["schema_ref"]["id"] == activity.fact_schema_id()
                    && s["field"]["name"] == rule.formula.quantity_field
            })
            .expect("complete fact slots")["index"]
            .clone();
        instructions.push(json!({"opcode":"evaluate_rule","ordinal":ordinal.to_string(),"policy_ref":sources.root,"rule":rule,"fact_slots":[index],"binding_fact_slots":[],"basis_instructions":[]}));
        rule_order.push(json!({"policy_ref":sources.root,"rule_id":rule.rule_id}));
        explanations.push(json!({"ordinal":ordinal.to_string(),"policy_ref":sources.root,"rule_or_family":rule.rule_id,"fields":[rule.formula.quantity_field],"primitive":"unit"}));
    }
    // This bounded subset has true predicates (one representable cell), no
    // monetary edges and at most64 independent one-action unit producers.
    // Structural+arithmetic work is bounded by source/field/rule ceilings; no
    // general solver or opaque instruction is accepted under these constants.
    if policy.rules.len() * 3 > 256 {
        return Err(fail("PROGRAM_LIMIT", "AST nodes"));
    }
    let mut source_refs: Vec<_> = sources
        .documents
        .iter()
        .map(|d| d.reference.clone())
        .collect();
    canonical::sort_set(&mut source_refs)?;
    let components: Vec<_> = template.components().collect();
    let body = json!({"schema":"ledger-workflow-compiled-plan/1","compiler_ref":policy.compiler_ref,"compiler_semantics":"ledger-policy-compiler/1","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits_profile":"workflow-limits/1","source_refs":source_refs,"activity_table":activity_table,"fact_slots":fact_slots,"link_paths":[],"component_slots":components,"rule_order":rule_order,"typed_instructions":instructions,"dependency_edges":[],"worst_case":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"},"explanation_map":explanations});
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/workflow-plan/1\0");
    hash.update(CanonicalBytes::from_value(&body)?.as_slice());
    let digest = format!("sha256:{}", canonical::hex(&hash.finalize()));
    if digest != expected_plan_digest {
        return Err(fail("PLAN_DIGEST_MISMATCH", "expected compiled plan"));
    }
    let plan = DefinitionDocument::with_digest("workflow-compiled-plan", body, digest, "wplan_")?;
    Ok(PreparedPublication { sources, plan })
}
fn fail(code: &'static str, detail: &str) -> Error {
    Error::new(code, detail)
}
fn insert_source(
    documents: &mut BTreeMap<String, DefinitionDocument>,
    document: DefinitionDocument,
) -> Result<()> {
    match documents.entry(document.id().to_owned()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(document);
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(_) => Err(fail(
            "DEFINITION_DUPLICATE",
            "duplicate normalized ingress document",
        )),
    }
}

#[cfg(test)]
mod tests;
