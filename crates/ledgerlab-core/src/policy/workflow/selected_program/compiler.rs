//! Source-derived selected instructions. This module never consumes oracle output.
use super::super::{
    transition_constructor::{arithmetic, codec, CoreMeter},
    DefinitionRef, FactSchema, Product, SlotTemplate,
};
use super::{
    model::{Effect, Formula, Policy},
    SelectedCompilerSupport,
};
use crate::canonical::audit_arena::{AuditedBytes, AuditedValue, ExternalBufferLease};
use crate::validation_work::{self as work, RepresentationKind};
use crate::{canonical, Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::{mem, sync::Arc};
fn err(detail: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", detail)
}
pub struct SelectedSourceInput {
    pub documents: Vec<Vec<u8>>,
    pub root_policy_ref: DefinitionRef,
}
enum SourceValue {
    Legacy(Value),
    Owned(AuditedValue),
}
impl std::ops::Deref for SourceValue {
    type Target = Value;
    fn deref(&self) -> &Value {
        match self {
            Self::Legacy(v) => v,
            Self::Owned(v) => v.value(),
        }
    }
}
impl SourceValue {
    fn parse(m: &mut CoreMeter, raw: &[u8]) -> Result<Self> {
        if let Some(arena) = m.representation_arena() {
            let value =
                m.parse_owned_representation(&arena, raw, RepresentationKind::SourceDefinition)?;
            codec::validate_selected_types(m, value.value())?;
            Ok(Self::Owned(value))
        } else {
            Ok(Self::Legacy(codec::parse(m, raw, 262144)?))
        }
    }
    /// The existing typed source normalizer consumes Value. This controlled
    /// copy is admitted BEFORE cloning and its lease follows the compiler's
    /// retained typed output; it does not cover additional DTO/map producers.
    fn projection(self, m: &mut CoreMeter, owners: &mut Vec<ExternalBufferLease>) -> Result<Value> {
        self.projection_with_payload(m, owners, 0)
    }
    fn projection_with_payload(
        self,
        m: &mut CoreMeter,
        owners: &mut Vec<ExternalBufferLease>,
        normalizer_payload: usize,
    ) -> Result<Value> {
        match self {
            Self::Legacy(v) => Ok(v),
            Self::Owned(v) => normalizer_projection(m, v.value(), owners, normalizer_payload),
        }
    }
    fn document(self, m: &mut CoreMeter) -> Result<SelectedDocument> {
        match self {
            Self::Legacy(v) => doc(m, v),
            Self::Owned(v) => finish_owned_document(m, v, false),
        }
    }
}
/// One lease covers the raw Value copy plus the SAME normalizer's separately
/// derived private DTO/maps/body transient bound, BEFORE either producer.
fn normalizer_projection(
    m: &mut CoreMeter,
    value: &Value,
    owners: &mut Vec<ExternalBufferLease>,
    normalizer_payload: usize,
) -> Result<Value> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| err("source projection arena"))?;
    let copy = crate::canonical::audit_arena::value_copy_payload_bound(value, m)?;
    let bytes = work::usize_add(m, copy, normalizer_payload)?;
    let owner = arena.reserve_external_payload(bytes, m)?;
    owners.push(owner);
    Ok(value.clone())
}
#[derive(Clone, Debug)]
pub struct SelectedDocument {
    storage: DocumentStorage,
}
#[derive(Clone, Debug)]
enum DocumentStorage {
    Legacy(LegacyDocument),
    Owned(Arc<OwnedDocument>),
}
#[derive(Clone, Debug)]
struct LegacyDocument {
    reference: DefinitionRef,
    body: Value,
    bytes: Vec<u8>,
}
// A cloned public document view shares this entire owner, including its Ref.
// No RP01 clone allocates another naked Value/Vec/String graph.
struct OwnedDocument {
    reference: DefinitionRef,
    body: AuditedValue,
    bytes: AuditedBytes,
    _metadata: ExternalBufferLease,
}
impl std::fmt::Debug for OwnedDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedDocument")
            .field("reference", &self.reference)
            .finish_non_exhaustive()
    }
}
impl SelectedDocument {
    pub fn reference(&self) -> &DefinitionRef {
        match &self.storage {
            DocumentStorage::Legacy(v) => &v.reference,
            DocumentStorage::Owned(v) => &v.reference,
        }
    }
    pub fn canonical_value(&self) -> &Value {
        match &self.storage {
            DocumentStorage::Legacy(v) => &v.body,
            DocumentStorage::Owned(v) => v.body.value(),
        }
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        match &self.storage {
            DocumentStorage::Legacy(v) => &v.bytes,
            DocumentStorage::Owned(v) => v.bytes.bytes(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct StaticAnalysis {
    pub fields: usize,
    pub rules: usize,
    pub families: usize,
    pub maximum_path: usize,
    pub compile_work: Option<u64>,
}
#[derive(Clone, Copy)]
enum ProgramProvenance {
    Compiled,
    Reloaded,
}
pub struct SelectedProgram {
    provenance: ProgramProvenance,
    recipe: SelectedRecipe,
    root: DefinitionRef,
    root_position: usize,
    sources: Vec<SelectedDocument>,
    plan: SelectedDocument,
    policy: Policy,
    schemas: Vec<(DefinitionRef, FactSchema)>,
    analysis: StaticAnalysis,
    family_close_recipe: bool,
    owner_local_cost_recipe: bool,
    app_owned_cost_recipe: bool,
    app_cost_causal_recipe: bool,
    supplier_scoped_consent_recipe: bool,
    execution_registry: Option<&'static str>,
    // Exact source/schema vector slots and retained typed Ref copies. Typed
    // schema/policy contents and other compiler intermediates are separate.
    _policy_decode_ownership: Option<ExternalBufferLease>,
    _source_projection_ownership: Vec<ExternalBufferLease>,
    _control_ownership: Option<ExternalBufferLease>,
}
impl SelectedProgram {
    /// Supported release surface for NEW local promises only. Historical
    /// source/plan DATA loading and original receipt recovery do not call it.
    pub fn require_local_admission_with_session(
        &self,
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<()> {
        let result = self.require_local_admission(session.meter_mut());
        if result.is_err() {
            session.invalidate();
        }
        result
    }
    pub(in crate::policy::workflow) fn require_local_admission(
        &self,
        m: &mut CoreMeter,
    ) -> Result<()> {
        self.require_attempt_owner(m)?;
        if !self.uses_local_execution_recipe() {
            return Err(err("supported local admission requires local recipe"));
        }
        super::local_admission::check(m, &self.policy, &self.sources, &self.schemas)
    }
    /// Binary recipe identity, not current-support freshness or authority.
    pub fn uses_family_close_recipe(&self) -> bool {
        self.family_close_recipe
    }
    pub fn uses_owner_local_cost_recipe(&self) -> bool {
        self.owner_local_cost_recipe
    }
    pub fn uses_app_owned_cost_recipe(&self) -> bool {
        self.app_owned_cost_recipe
    }
    pub fn uses_app_cost_causal_recipe(&self) -> bool {
        self.app_cost_causal_recipe
    }
    pub fn uses_supplier_scoped_consent_recipe(&self) -> bool {
        self.supplier_scoped_consent_recipe
    }
    pub fn execution_registry(&self) -> Option<&'static str> {
        self.execution_registry
    }
    pub fn uses_local_execution_recipe(&self) -> bool {
        self.execution_registry
            == Some("sha256:d0a21becb08150a781770cfef80b5521eeb93c3b45aa9ebdab23c800ef44ee4a")
    }
    pub(in crate::policy::workflow) fn uses_local_parent_budget_recipe(&self) -> bool {
        self.uses_local_execution_recipe()
            && matches!(
                self.recipe,
                SelectedRecipe::LocalParentBudget1 | SelectedRecipe::LocalOperator1
            )
    }
    pub(in crate::policy::workflow) fn uses_local_operator_resource_recipe(&self) -> bool {
        self.uses_local_execution_recipe() && matches!(self.recipe, SelectedRecipe::LocalOperator1)
    }
    pub(in crate::policy::workflow) fn require_attempt_owner(
        &self,
        m: &mut CoreMeter,
    ) -> Result<()> {
        if let Some(owner) = &self._control_ownership {
            let arena = m
                .representation_arena()
                .ok_or_else(|| err("owning program requires original attempt"))?;
            arena.require_external_owner(owner, m)
        } else if self.execution_registry.is_some() {
            Err(err("financial program has no owning compiler output"))
        } else {
            Ok(())
        }
    }
    /// Exact binary recipe selection; shared wire shapes do not select latest.
    pub(in crate::policy::workflow) fn uses_commitment_first_consent_recipe(&self) -> bool {
        self.uses_local_execution_recipe()
            || self.execution_registry
                == Some("sha256:762b708044ffad44717074a9b3456b91ebbe62d7a12d76cfa80d6a37ecbfe276")
    }
    pub fn root_policy_ref(&self) -> &DefinitionRef {
        &self.root
    }
    pub fn original_policy_value(&self) -> &Value {
        self.sources[self.root_position].canonical_value()
    }
    pub fn compiler_descriptor_ref(&self) -> &DefinitionRef {
        &self.policy.compiler_ref
    }
    pub fn product_ref(&self) -> &DefinitionRef {
        &self.policy.product_ref
    }
    pub fn template_ref(&self) -> &DefinitionRef {
        &self.policy.slot_template_ref
    }
    pub fn context_schema_ref(&self) -> &DefinitionRef {
        &self.policy.context_schema_ref
    }
    /// Borrow the actual compiled context schema under the caller's SAME
    /// semantic work owner. No schema is recompiled or cloned, and this view
    /// does not establish publication/origin custody.
    pub fn context_schema_with_work(
        &self,
        work: &mut dyn crate::validation_work::ValidationWork,
    ) -> Result<&FactSchema> {
        let target = &self.policy.context_schema_ref;
        for (reference, schema) in &self.schemas {
            work.visit(crate::validation_work::ValidationVisit::MemberCompare)?;
            for (left, right) in [
                (reference.kind(), target.kind()),
                (reference.id(), target.id()),
                (reference.digest(), target.digest()),
            ] {
                let bytes = crate::validation_work::usize_add(work, left.len(), right.len())?;
                work.scan_bytes(bytes)?;
            }
            if reference == target {
                return Ok(schema);
            }
        }
        Err(err("compiled context schema missing"))
    }
    pub fn source_documents(&self) -> &[SelectedDocument] {
        &self.sources
    }
    pub fn compiled_plan(&self) -> &SelectedDocument {
        &self.plan
    }
    pub fn static_analysis(&self) -> &StaticAnalysis {
        &self.analysis
    }
    pub(in crate::policy::workflow) fn fact_schemas(&self) -> &[(DefinitionRef, FactSchema)] {
        &self.schemas
    }
    pub(super) fn policy(&self) -> &Policy {
        &self.policy
    }
}
fn doc(m: &mut CoreMeter, body: Value) -> Result<SelectedDocument> {
    if m.representation_arena().is_some() {
        return owned_document(m, &body, false);
    }
    let schema = body["schema"]
        .as_str()
        .ok_or_else(|| err("source schema"))?;
    let kind = schema
        .strip_prefix("ledger-")
        .and_then(|s| {
            s.strip_suffix("/1")
                .or_else(|| s.strip_suffix("/2"))
                .or_else(|| s.strip_suffix("/3"))
                .or_else(|| s.strip_suffix("/4"))
                .or_else(|| s.strip_suffix("/5"))
                .or_else(|| s.strip_suffix("/6"))
                .or_else(|| s.strip_suffix("/7"))
                .or_else(|| s.strip_suffix("/8"))
        })
        .ok_or_else(|| err("source schema version"))?;
    let bytes = codec::encode(m, &body, 262144)?;
    m.visit("source.hash")?;
    m.read_bytes(bytes.len())?;
    let framed = codec::encode(m, &json!([schema, body]), 4194304)?;
    m.hash_bytes(b"bean-counter/workflow-document/1\0".len() + framed.len())?;
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/workflow-document/1\0");
    hash.update(&framed);
    let digest = format!("sha256:{}", canonical::hex(&hash.finalize()));
    let reference = DefinitionRef::new(kind, &format!("wdoc_{}", &digest[7..]), &digest)?;
    Ok(SelectedDocument {
        storage: DocumentStorage::Legacy(LegacyDocument {
            reference,
            body,
            bytes,
        }),
    })
}
fn borrowed_doc(m: &mut CoreMeter, body: &Value) -> Result<SelectedDocument> {
    if m.representation_arena().is_some() {
        owned_document(m, body, false)
    } else {
        doc(m, body.clone())
    }
}
/// Retained compiler document output uses the SAME attempt arena. The source
/// normalizers/typed AST/intermediate producers remain separately accounted;
/// this function does not qualify those producers or enable admission.
fn owned_document(m: &mut CoreMeter, body: &Value, plan: bool) -> Result<SelectedDocument> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| err("owning selected document arena"))?;
    let owned = arena.clone_value(body, m)?;
    finish_owned_document(m, owned, plan)
}
pub(in crate::policy::workflow) fn finish_owned_document(
    m: &mut CoreMeter,
    owned: AuditedValue,
    plan: bool,
) -> Result<SelectedDocument> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| err("owning selected document arena"))?;
    let body = owned.value();
    codec::validate_selected_types(m, body)?;
    let schema = body
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| err("source schema"))?;
    let kind = if plan {
        "workflow-compiled-plan"
    } else {
        schema
            .strip_prefix("ledger-")
            .and_then(|s| s.rsplit_once('/').map(|(kind, _)| kind))
            .ok_or_else(|| err("source schema version"))?
    };
    let prefix = if plan { "wplan_" } else { "wdoc_" };
    // The final Ref plus hex/digest/ID construction overlaps and Arc payload
    // are admitted BEFORE any of their String/Arc allocations.
    let id_length = work::usize_add(m, prefix.len(), 64)?;
    let n = work::usize_add(
        m,
        mem::size_of::<OwnedDocument>(),
        2 * mem::size_of::<usize>(),
    )?;
    let mut n = work::usize_add(m, n, kind.len())?;
    for length in [id_length, 71, 64, id_length, 71] {
        n = work::usize_add(m, n, length)?;
    }
    let metadata = arena.reserve_external_payload(n, m)?;
    let encoding = if plan {
        RepresentationKind::Record
    } else {
        RepresentationKind::SourceDefinition
    };
    let bytes = m.canonical_owned_representation(&arena, owned.value(), encoding)?;
    let mut hash = Sha256::new();
    let domain = if plan {
        b"bean-counter/workflow-plan/1\0".as_slice()
    } else {
        b"bean-counter/workflow-document/1\0".as_slice()
    };
    m.hash_bytes(domain.len())?;
    hash.update(domain);
    if plan {
        m.hash_bytes(bytes.bytes().len())?;
        hash.update(bytes.bytes());
    } else {
        let schema_value = arena.string_value(schema, m)?;
        let schema_bytes = m.canonical_owned_representation(
            &arena,
            schema_value.value(),
            RepresentationKind::SourceDefinition,
        )?;
        for part in [
            b"[".as_slice(),
            schema_bytes.bytes(),
            b",".as_slice(),
            bytes.bytes(),
            b"]".as_slice(),
        ] {
            m.visit("source.document_hash_part")?;
            m.hash_bytes(part.len())?;
            hash.update(part);
        }
    }
    let hex = canonical::hex_with_work(&hash.finalize(), m)?;
    fn prefixed(m: &mut CoreMeter, prefix: &str, hex: &str, length: usize) -> Result<String> {
        let mut output = String::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| err("selected document identity allocation"))?;
        if work::compare(m, output.capacity(), length)?.is_gt() {
            return Err(err("selected document identity capacity"));
        }
        m.scan_bytes(prefix.len())?;
        m.scan_bytes(hex.len())?;
        m.write_bytes(length)?;
        output.push_str(prefix);
        output.push_str(hex);
        Ok(output)
    }
    let digest = prefixed(m, "sha256:", &hex, 71)?;
    let id = prefixed(m, prefix, &hex, id_length)?;
    let reference = DefinitionRef::new_with_work(kind, &id, &digest, m)?;
    Ok(SelectedDocument {
        storage: DocumentStorage::Owned(Arc::new(OwnedDocument {
            reference,
            body: owned,
            bytes,
            _metadata: metadata,
        })),
    })
}
fn lookup<'a>(
    m: &mut CoreMeter,
    docs: &'a [SelectedDocument],
    r: &DefinitionRef,
) -> Result<&'a SelectedDocument> {
    for d in docs {
        let same = if m.representation_arena().is_some() {
            m.visit("source.ref_compare")?;
            let actual = d.reference();
            same_literal(m, actual.kind(), r.kind())?
                && same_literal(m, actual.id(), r.id())?
                && same_literal(m, actual.digest(), r.digest())?
        } else {
            m.check("source.ref_compare", || d.reference() == r)?
        };
        if same {
            return Ok(d);
        }
    }
    Err(err("missing exact source"))
}
fn set(m: &mut CoreMeter, values: Vec<Value>) -> Result<Vec<Value>> {
    let mut out = Vec::<(Vec<u8>, Value)>::new();
    for v in values {
        let b = codec::encode(m, &v, 262144)?;
        let mut position = out.len();
        for (i, (old, _)) in out.iter().enumerate() {
            m.visit("set.compare")?;
            match b.cmp(old) {
                std::cmp::Ordering::Equal => return Err(err("duplicate set member")),
                std::cmp::Ordering::Less => {
                    position = i;
                    break;
                }
                _ => {}
            }
        }
        out.insert(position, (b, v));
    }
    Ok(out.into_iter().map(|(_, v)| v).collect())
}
/// Fresh compilation dispatches an exact binary-owned recipe, never arbitrary
/// descriptor contents. Historical restore below dispatches the same retained
/// v1 implementation directly and compares its complete source-derived plan.
pub fn compile_selected_program(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
) -> Result<SelectedProgram> {
    m.enter_compile()?;
    let recipe = require_selected_recipe(support, m)?;
    let program = compile_selected_recipe(input, support, m, recipe)?;
    if program.uses_local_execution_recipe() {
        // Compilation also exposes non-owning DATA for source/plan comparison.
        // Check its supported structure here. Actual promise admission still
        // calls require_local_admission and requires this attempt's owner.
        super::local_admission::check(m, &program.policy, &program.sources, &program.schemas)?;
    }
    Ok(program)
}
#[derive(Clone, Copy)]
enum SelectedRecipe {
    RetailDraft1,
    FamilyClose1,
    OwnerLocalCost1,
    SupplierScopedConsent1,
    AppOwnedCost1,
    AppCostCausal1,
    LocalParentBudget1,
    LocalOperator1,
}
impl SelectedRecipe {
    fn policy_schema(self) -> &'static str {
        match self {
            Self::RetailDraft1 => "ledger-workflow-policy/1",
            Self::AppCostCausal1 | Self::LocalParentBudget1 | Self::LocalOperator1 => {
                "ledger-workflow-policy/4"
            }
            Self::AppOwnedCost1 => "ledger-workflow-policy/3",
            Self::FamilyClose1 | Self::OwnerLocalCost1 | Self::SupplierScopedConsent1 => {
                "ledger-workflow-policy/2"
            }
        }
    }
    fn plan_schema(self) -> &'static str {
        match self {
            Self::RetailDraft1 => "ledger-workflow-compiled-plan/1",
            Self::FamilyClose1 => "ledger-workflow-compiled-plan/2",
            Self::OwnerLocalCost1 => "ledger-workflow-compiled-plan/3",
            Self::LocalOperator1 => "ledger-workflow-compiled-plan/8",
            Self::LocalParentBudget1 => "ledger-workflow-compiled-plan/7",
            Self::AppCostCausal1 => "ledger-workflow-compiled-plan/6",
            Self::AppOwnedCost1 => "ledger-workflow-compiled-plan/5",
            Self::SupplierScopedConsent1 => "ledger-workflow-compiled-plan/4",
        }
    }
    fn compiler_semantics(self) -> &'static str {
        match self {
            Self::RetailDraft1 => "ledger-policy-compiler/1",
            Self::FamilyClose1 => "ledger-policy-compiler/family-close-1",
            Self::OwnerLocalCost1 => "ledger-policy-compiler/owner-local-cost-1",
            Self::AppCostCausal1 | Self::LocalParentBudget1 | Self::LocalOperator1 => {
                "ledger-policy-compiler/app-cost-causal-1"
            }
            Self::AppOwnedCost1 => "ledger-policy-compiler/app-owned-cost-1",
            Self::SupplierScopedConsent1 => "ledger-policy-compiler/supplier-scoped-consent-1",
        }
    }
}
pub(super) fn same_literal(m: &mut CoreMeter, a: &str, b: &str) -> Result<bool> {
    m.visit("compile.recipe_text_identity")?;
    if !crate::validation_work::compare(m, a.len(), b.len())?.is_eq() {
        return Ok(false);
    }
    for (a, b) in a.bytes().zip(b.bytes()) {
        m.scan_bytes(2)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
fn semantic_text_order(m: &mut CoreMeter, left: &str, right: &str) -> Result<std::cmp::Ordering> {
    m.visit("compile.text_order")?;
    let bytes = work::usize_add(m, left.len(), right.len())?;
    m.scan_bytes(bytes)?;
    Ok(left.cmp(right))
}
fn require_selected_recipe(
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
) -> Result<SelectedRecipe> {
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/8" {
        m.visit("compile.local_operator_resource_recipe_dispatch")?;
        if support.execution_registry() != Some(super::local::REGISTRY)
            || !super::support::same_bytes(
                m,
                support.canonical_bytes(),
                super::local::OPERATOR_DESCRIPTOR,
            )?
        {
            return Err(err("exact local operator compiler identity"));
        }
        return Ok(SelectedRecipe::LocalOperator1);
    }
    if m.resource_limits().is_some() {
        return Err(err("operator resources require exact local compiler8"));
    }
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/7" {
        m.visit("compile.local_parent_budget_recipe_dispatch")?;
        if support.execution_registry() != Some(super::local::REGISTRY)
            || !super::support::same_bytes(
                m,
                support.canonical_bytes(),
                super::local::PARENT_DESCRIPTOR,
            )?
        {
            return Err(err("exact local parent-budget compiler identity"));
        }
        return Ok(SelectedRecipe::LocalParentBudget1);
    }
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/6" {
        m.visit("compile.app_cost_causal_recipe_dispatch")?;
        if !same_literal(
            m,
            support.canonical_value()["compiler_semantics"]
                .as_str()
                .unwrap_or(""),
            "ledger-policy-compiler/app-cost-causal-1",
        )? || !same_literal(
            m,
            support.canonical_value()["plan_schema"]
                .as_str()
                .unwrap_or(""),
            "ledger-workflow-compiled-plan/6",
        )? {
            return Err(err("closed app cost causal compiler tuple"));
        }
        return Ok(SelectedRecipe::AppCostCausal1);
    }
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/5" {
        m.visit("compile.app_owned_cost_recipe_dispatch")?;
        if !same_literal(
            m,
            support.canonical_value()["compiler_semantics"]
                .as_str()
                .unwrap_or(""),
            "ledger-policy-compiler/app-owned-cost-1",
        )? || !same_literal(
            m,
            support.canonical_value()["plan_schema"]
                .as_str()
                .unwrap_or(""),
            "ledger-workflow-compiled-plan/5",
        )? {
            return Err(err("closed app-owned cost compiler tuple"));
        }
        return Ok(SelectedRecipe::AppOwnedCost1);
    }

    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/4" {
        m.visit("compile.supplier_scoped_consent_recipe_dispatch")?;
        if !same_literal(
            m,
            support.canonical_value()["compiler_semantics"]
                .as_str()
                .unwrap_or(""),
            "ledger-policy-compiler/supplier-scoped-consent-1",
        )? || !same_literal(
            m,
            support.canonical_value()["plan_schema"]
                .as_str()
                .unwrap_or(""),
            "ledger-workflow-compiled-plan/4",
        )? {
            return Err(err("closed supplier-scoped consent compiler tuple"));
        }
        return Ok(SelectedRecipe::SupplierScopedConsent1);
    }
    // SelectedCompilerSupport has private fields and no arbitrary-document
    // constructor. New-recipe support can only come from its code-owned
    // catalog factory/exact saved verifier, not descriptor schema alone.
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/3" {
        m.visit("compile.owner_local_cost_recipe_dispatch")?;
        if !same_literal(
            m,
            support.canonical_value()["compiler_semantics"]
                .as_str()
                .unwrap_or(""),
            "ledger-policy-compiler/owner-local-cost-1",
        )? || !same_literal(
            m,
            support.canonical_value()["plan_schema"]
                .as_str()
                .unwrap_or(""),
            "ledger-workflow-compiled-plan/3",
        )? {
            return Err(err("closed owner-local cost compiler tuple"));
        }
        return Ok(SelectedRecipe::OwnerLocalCost1);
    }
    if support.canonical_value()["schema"] == "ledger-workflow-compiler-descriptor/2" {
        m.visit("compile.family_close_recipe_dispatch")?;
        if !same_literal(
            m,
            support.canonical_value()["compiler_semantics"]
                .as_str()
                .unwrap_or(""),
            "ledger-policy-compiler/family-close-1",
        )? || !same_literal(
            m,
            support.canonical_value()["plan_schema"]
                .as_str()
                .unwrap_or(""),
            "ledger-workflow-compiled-plan/2",
        )? {
            return Err(err("closed family-close compiler tuple"));
        }
        return Ok(SelectedRecipe::FamilyClose1);
    }
    require_retail_selected_v1(support, m)?;
    Ok(SelectedRecipe::RetailDraft1)
}
fn require_retail_selected_v1(support: &SelectedCompilerSupport, m: &mut CoreMeter) -> Result<()> {
    let actual = support.reference().id().as_bytes();
    let expected = b"wdoc_f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b";
    m.visit("compile.recipe_dispatch")?;
    if !crate::validation_work::compare(m, actual.len(), expected.len())?.is_eq() {
        return Err(Error::new(
            "ORIGINAL_VERIFIER_UNAVAILABLE",
            "selected compiler recipe unavailable",
        ));
    }
    for (a, b) in actual.iter().zip(expected) {
        m.scan_bytes(2)?;
        if a != b {
            return Err(Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "selected compiler recipe unavailable",
            ));
        }
    }
    Ok(())
}
/// Fresh publication compilation under the SAME per-origin work owner.
/// No caller allowance, saved-plan substitution, or independent child reset.
pub fn compile_selected_publication(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let result = (|| {
        let mut compiler = session.meter_mut().original_compile_meter()?;
        let program = compile_selected_program(input, support, &mut compiler)?;
        session.meter_mut().finish_original_compile(compiler)?;
        Ok(program)
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
pub fn restore_selected_program(
    input: SelectedSourceInput,
    saved_descriptor: &[u8],
    saved_plan: &[u8],
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let result = restore_selected_inner(input, saved_descriptor, saved_plan, session);
    if result.is_err() {
        session.invalidate();
    }
    result
}
fn restore_selected_inner(
    input: SelectedSourceInput,
    saved_descriptor: &[u8],
    saved_plan: &[u8],
    session: &mut crate::validation_work::OriginalAuditSession,
) -> Result<SelectedProgram> {
    let mut compiler = session.meter_mut().original_compile_meter()?;
    compiler.enter_compile()?;
    let support = SelectedCompilerSupport::restore_saved(saved_descriptor, &mut compiler)?;
    let recipe = require_selected_recipe(&support, &mut compiler)?;
    let program = compile_selected_recipe(input, &support, &mut compiler, recipe)?;
    let _compiler_usage = session.meter_mut().finish_original_compile(compiler)?;
    let work = session.work();
    let actual = program.compiled_plan().canonical_bytes();
    work.visit(crate::validation_work::ValidationVisit::MemberCompare)?;
    if !crate::validation_work::compare(work, actual.len(), saved_plan.len())?.is_eq() {
        return Err(Error::new(
            "INTEGRITY_FAILURE",
            "saved selected plan length",
        ));
    }
    for (a, b) in actual.iter().zip(saved_plan) {
        work.scan_bytes(2)?;
        if a != b {
            return Err(Error::new(
                "INTEGRITY_FAILURE",
                "saved selected plan differs from original source computation",
            ));
        }
    }
    Ok(program)
}
fn compile_selected_recipe(
    input: SelectedSourceInput,
    support: &SelectedCompilerSupport,
    m: &mut CoreMeter,
    recipe: SelectedRecipe,
) -> Result<SelectedProgram> {
    m.visit("compile.input")?;
    if !m.check("compile.count", || {
        !input.documents.is_empty() && input.documents.len() <= 128
    })? {
        return Err(err("source count"));
    }
    let source_count = input.documents.len();
    let control_ownership = if let Some(arena) = m.representation_arena() {
        let per = mem::size_of::<SelectedDocument>()
            + mem::size_of::<FactSchema>()
            + mem::size_of::<(DefinitionRef, FactSchema)>()
            + mem::size_of::<ExternalBufferLease>();
        crate::validation_work::ValidationWork::arithmetic(
            m,
            work::ArithmeticOperation::Multiply,
            &[&source_count.into(), &per.into()],
        )?;
        let controls = source_count
            .checked_mul(per)
            .ok_or_else(|| err("compiled control vector product"))?;
        // These are the closed DefinitionRef producer dimensions: SHA256/64,
        // wplan_ or wdoc_ prefix, and the longest admitted definition kind.
        // This is requested copy storage, never a caller quota/capacity bound.
        let refs = work::usize_add(m, source_count, 1)?;
        let per_ref =
            "workflow-compiler-descriptor".len() + "wplan_".len() + 64 + "sha256:".len() + 64;
        crate::validation_work::ValidationWork::arithmetic(
            m,
            work::ArithmeticOperation::Multiply,
            &[&refs.into(), &per_ref.into()],
        )?;
        let references = refs
            .checked_mul(per_ref)
            .ok_or_else(|| err("compiled Ref copy product"))?;
        let size = work::usize_add(m, controls, references)?;
        Some(arena.reserve_external_payload(size, m)?)
    } else {
        None
    };
    let mut source_projection_ownership = Vec::new();
    let mut docs = Vec::new();
    let mut identities = BTreeSet::new();
    let mut semantic = BTreeSet::new();
    let mut schemas = Vec::new();
    if control_ownership.is_some() {
        docs.try_reserve_exact(source_count)
            .map_err(|_| err("compiled document slots"))?;
        schemas
            .try_reserve_exact(source_count)
            .map_err(|_| err("compiled schema slots"))?;
        source_projection_ownership
            .try_reserve_exact(source_count)
            .map_err(|_| err("compiled source projection owner slots"))?;
    }
    let mut product_raw = None;
    let mut template_raw = None;
    let mut policy_raw = None;
    for raw in input.documents {
        let body = SourceValue::parse(m, &raw)?;
        let schema = body["schema"]
            .as_str()
            .ok_or_else(|| err("source schema"))?;
        m.visit("source.kind")?;
        match schema {
            "ledger-workflow-fact-schema/1" => {
                for field in body["fields"]
                    .as_array()
                    .ok_or_else(|| err("fact fields"))?
                {
                    if !m.check("source.fact_type", || {
                        ["enum", "integer", "timestamp"]
                            .contains(&field["type"]["kind"].as_str().unwrap_or(""))
                    })? {
                        return Err(err("selected fact type"));
                    }
                }

                let parsed = if m.representation_arena().is_some() {
                    let bytes = FactSchema::normalization_payload_bound_with_work(&body, m)?;
                    let value =
                        body.projection_with_payload(m, &mut source_projection_ownership, bytes)?;
                    FactSchema::from_value_complete_with_work(value, m)?
                } else {
                    FactSchema::from_value_with_work(
                        body.projection(m, &mut source_projection_ownership)?,
                        m,
                    )?
                };
                let document = borrowed_doc(m, parsed.canonical_value())?;
                schemas.push(parsed);
                docs.push(document);
            }
            "ledger-workflow-product/1" => {
                for unit in body["units"].as_array().ok_or_else(|| err("unit array"))? {
                    if !m.check("source.unit_terminal", || {
                        unit.get("normalization").is_none()
                    })? {
                        return Err(err("selected terminal unit"));
                    }
                }

                if product_raw.replace(body).is_some() {
                    return Err(err("multiple products"));
                }
            }
            "ledger-workflow-slot-template/1" => {
                if template_raw.replace(body).is_some() {
                    return Err(err("multiple templates"));
                }
            }
            "ledger-workflow-policy/1"
            | "ledger-workflow-policy/2"
            | "ledger-workflow-policy/3"
            | "ledger-workflow-policy/4" => {
                if schema != recipe.policy_schema() {
                    return Err(err("policy/compiler recipe version"));
                }
                if !matches!(
                    recipe,
                    SelectedRecipe::AppOwnedCost1
                        | SelectedRecipe::AppCostCausal1
                        | SelectedRecipe::LocalParentBudget1
                        | SelectedRecipe::LocalOperator1
                ) && body.get("app_delayed_costs").is_some()
                {
                    return Err(err("old policy dictionary has no app declarations"));
                }
                if matches!(
                    recipe,
                    SelectedRecipe::AppOwnedCost1
                        | SelectedRecipe::AppCostCausal1
                        | SelectedRecipe::LocalParentBudget1
                        | SelectedRecipe::LocalOperator1
                ) {
                    for declaration in body["app_delayed_costs"]
                        .as_array()
                        .ok_or_else(|| err("app declaration array"))?
                    {
                        m.visit("compile.original_app_declaration_version")?;
                        if matches!(recipe, SelectedRecipe::AppOwnedCost1) {
                            if declaration.get("correction_anchor").is_some()
                                || declaration.get("causal_reversal").is_some()
                            {
                                return Err(err("old app declaration cannot carry new timing"));
                            }
                        } else if declaration
                            .get("causal_reversal")
                            .is_some_and(Value::is_null)
                        {
                            return Err(err("null app causal clause"));
                        }
                    }
                }
                if matches!(recipe, SelectedRecipe::RetailDraft1) {
                    // Preserve the old closed serde dictionary and paid trace.
                    // Policy1 never gains the optional Policy2 declaration.
                    for family in body["families"]
                        .as_array()
                        .ok_or_else(|| err("family array"))?
                    {
                        if family.get("no_success_close").is_some() {
                            return Err(err("Policy1 family-close field"));
                        }
                    }
                } else {
                    for family in body["families"]
                        .as_array()
                        .ok_or_else(|| err("family array"))?
                    {
                        m.visit("compile.family_close_declaration")?;
                        if let Some(declaration) = family.get("no_success_close") {
                            if declaration.is_null() {
                                return Err(err("null original close declaration"));
                            }
                        }
                    }
                }
                if policy_raw.replace(body).is_some() {
                    return Err(err("multiple policies"));
                }
            }
            "ledger-workflow-compiler-descriptor/1"
            | "ledger-workflow-compiler-descriptor/2"
            | "ledger-workflow-compiler-descriptor/3"
            | "ledger-workflow-compiler-descriptor/4"
            | "ledger-workflow-compiler-descriptor/5"
            | "ledger-workflow-compiler-descriptor/6"
            | "ledger-workflow-compiler-descriptor/7"
            | "ledger-workflow-compiler-descriptor/8" => {
                let same = if m.representation_arena().is_some() {
                    super::super::transition_constructor::shape::same(
                        m,
                        &body,
                        support.canonical_value(),
                    )?
                } else {
                    m.check("source.support", || *body == *support.canonical_value())?
                };
                if !same {
                    return Err(Error::new(
                        "UNSUPPORTED_COMPILER_DESCRIPTOR",
                        "code owned selected support",
                    ));
                }
                docs.push(body.document(m)?);
            }
            _ => return Err(err("unsupported source family")),
        }
    }
    let product_body = product_raw.ok_or_else(|| err("product missing"))?;
    let product = if m.representation_arena().is_some() {
        let bytes = Product::normalization_payload_bound_with_work(&product_body, &schemas, m)?;
        let value =
            product_body.projection_with_payload(m, &mut source_projection_ownership, bytes)?;
        Product::from_value_complete_with_work(value, &schemas, m)?
    } else {
        Product::from_value_with_work(
            product_body.projection(m, &mut source_projection_ownership)?,
            &schemas,
            m,
        )?
    };
    let template_body = template_raw.ok_or_else(|| err("template missing"))?;
    let template = if m.representation_arena().is_some() {
        let bytes = SlotTemplate::normalization_payload_bound_with_work(&template_body, None, m)?;
        let value =
            template_body.projection_with_payload(m, &mut source_projection_ownership, bytes)?;
        SlotTemplate::from_value_complete_with_work(value, None, m)?
    } else {
        SlotTemplate::from_value_with_work(
            template_body.projection(m, &mut source_projection_ownership)?,
            None,
            m,
        )?
    };
    docs.push(borrowed_doc(m, product.canonical_value())?);
    docs.push(borrowed_doc(m, template.canonical_value())?);
    let body = policy_raw.ok_or_else(|| err("policy missing"))?;
    for rule in body["rules"].as_array().ok_or_else(|| err("rule array"))? {
        m.visit("predicate.closed")?;
        let predicate = rule["when"]
            .as_object()
            .ok_or_else(|| err("predicate object"))?;
        if !m.check("predicate.unit_keys", || {
            predicate.len() == 1 && predicate.contains_key("op")
        })? {
            return Err(err("closed true predicate"));
        }
    }
    for family in body["families"]
        .as_array()
        .ok_or_else(|| err("family array"))?
    {
        for code in family["codes"]
            .as_array()
            .ok_or_else(|| err("family codes"))?
        {
            m.visit("effect.closed")?;
            let effect = code["effect"]
                .as_object()
                .ok_or_else(|| err("effect object"))?;
            if m.check("effect.none", || {
                effect.get("kind").is_some_and(|v| v == "none")
            })? && !m.check("effect.none_keys", || effect.len() == 1)?
            {
                return Err(err("closed none effect"));
            }
        }
    }
    let policy_decode_ownership = if let Some(arena) = m.representation_arena() {
        let bytes = super::policy_decode::payload_bound(m, &body)?;
        Some(arena.reserve_external_payload(bytes, m)?)
    } else {
        None
    };
    let body = body.projection(m, &mut source_projection_ownership)?;
    let (mut policy, policy_decode_ownership) = if let Some(owner) = policy_decode_ownership {
        let (policy, owner) = super::policy_decode::decode(m, body, owner)?;
        (policy, Some(owner))
    } else {
        let policy: Policy =
            serde_json::from_value(body).map_err(|_| err("closed selected policy"))?;
        (policy, None)
    };
    if !m.check("policy.profile", || {
        policy.schema == recipe.policy_schema()
            && policy.currency == "USD"
            && policy.scale == 18
            && policy.rounding == "nearest_ties_away"
            && policy.mode == "component_workflow"
            && policy.previous_policy_ref.is_none()
            && policy.stages.is_empty()
            && (if matches!(
                recipe,
                SelectedRecipe::AppOwnedCost1
                    | SelectedRecipe::AppCostCausal1
                    | SelectedRecipe::LocalParentBudget1
                    | SelectedRecipe::LocalOperator1
            ) {
                policy
                    .app_delayed_costs
                    .as_ref()
                    .is_some_and(|rows| !rows.is_empty() && rows.len() <= 64)
            } else {
                policy.app_delayed_costs.is_none()
            })
            && (matches!(
                recipe,
                SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
            ) || policy.delayed_costs.is_empty())
    })? {
        return Err(err("selected policy profile"));
    }
    super::normalize::policy(m, &mut policy)?;
    arithmetic::integer(m, &policy.version, false)?;
    super::validate::policy(
        m,
        &policy,
        &product,
        &template,
        &schemas,
        matches!(
            recipe,
            SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
        ),
    )?;
    for rule in &mut policy.rules {
        m.visit("rule.validate")?;
        let activity = product
            .activity_with_work(&rule.activity, m)?
            .ok_or_else(|| err("rule activity"))?;
        template.component_named_with_work(&rule.component_key, &rule.component_name, m)?;
        let rule_slot = if m.representation_arena().is_some() {
            let mut found = false;
            for slot in &policy.binding_slots {
                m.visit("compile.rule_binding_slot")?;
                if !same_literal(m, &slot.name, &rule.binding_slot)? {
                    continue;
                }
                let book = same_literal(m, &slot.book, "retail")?
                    || matches!(
                        recipe,
                        SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
                    ) && same_literal(m, &slot.book, "supplier")?;
                if !book {
                    continue;
                }
                for activity in &slot.allowed_activities {
                    m.visit("compile.rule_allowed_activity")?;
                    if same_literal(m, activity, &rule.activity)? {
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
            found
        } else {
            m.check("rule.slot", || {
                policy.binding_slots.iter().any(|s| {
                    s.name == rule.binding_slot
                        && (s.book == "retail"
                            || matches!(
                                recipe,
                                SelectedRecipe::OwnerLocalCost1
                                    | SelectedRecipe::SupplierScopedConsent1
                            ) && s.book == "supplier")
                        && s.allowed_activities.contains(&rule.activity)
                })
            })?
        };
        if !rule_slot {
            return Err(err("rule binding slot"));
        }
        match &mut rule.formula {
            Formula::Fixed { amount } => {
                arithmetic::decimal(m, amount)?;
            }
            Formula::Unit {
                quantity_field,
                unit,
                rate,
            } => {
                arithmetic::decimal(m, rate)?;
                let mut found_schema = None;
                for schema in &schemas {
                    let id = schema.document_id_with_work(m)?;
                    if m.check("unit.schema_compare", || id == activity.fact_schema_id())? {
                        found_schema = Some(schema);
                        break;
                    }
                }
                let sr = found_schema.ok_or_else(|| err("quantity schema"))?;
                if sr.quantity_unit_with_work(quantity_field, m)? != unit {
                    return Err(err("quantity unit"));
                }
                if product
                    .unit_with_work(unit, m)?
                    .ok_or_else(|| err("unit missing"))?
                    .normalization()
                    .is_some()
                {
                    return Err(err("terminal unit only"));
                }
            }
            Formula::Discount {
                basis,
                mode,
                amount,
            } => {
                if !m.check("discount.profile", || {
                    mode == "additive" && basis.view == "booked_base" && amount.kind == "percent"
                })? {
                    return Err(err("selected discount"));
                }
                arithmetic::decimal(m, &amount.percent)?;
            }
            Formula::FinalCost {
                markup,
                multiplier,
                ceiling,
                ..
            } => {
                if !matches!(
                    recipe,
                    SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
                ) {
                    return Err(err("final cost compiler recipe"));
                }
                arithmetic::decimal(m, markup)?;
                let n = arithmetic::integer(m, &multiplier.numerator, false)?;
                let d = arithmetic::integer(m, &multiplier.denominator, false)?;
                arithmetic::rational(m, n, d)?;
                arithmetic::integer(m, &ceiling.atoms, false)?;
            }
            Formula::AppFinalCost {
                markup,
                multiplier,
                ceiling,
                ..
            } => {
                if !matches!(
                    recipe,
                    SelectedRecipe::AppOwnedCost1
                        | SelectedRecipe::AppCostCausal1
                        | SelectedRecipe::LocalParentBudget1
                        | SelectedRecipe::LocalOperator1
                ) {
                    return Err(err("app cost compiler recipe"));
                }
                arithmetic::decimal(m, markup)?;
                let n = arithmetic::integer(m, &multiplier.numerator, false)?;
                let d = arithmetic::integer(m, &multiplier.denominator, false)?;
                arithmetic::rational(m, n, d)?;
                arithmetic::integer(m, &ceiling.atoms, false)?;
            }
            Formula::SupplierFinalCost { ceiling, .. } => {
                if !matches!(
                    recipe,
                    SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
                ) {
                    return Err(err("supplier cost compiler recipe"));
                }
                arithmetic::integer(m, &ceiling.atoms, false)?;
            }
        }
    }
    for family in &policy.families {
        m.visit("family.validate")?;
        template.family_named_with_work(&family.family_key, &family.family_name, m)?;
        product.validate_selector_path_with_work(
            &family.trigger_activity,
            &family.selector.path,
            &family.selector.target_activity,
            m,
        )?;
        if !m.check("family.path_bound", || {
            !family.selector.path.is_empty() && family.selector.path.len() <= 4
        })? {
            return Err(err("selected monetary path"));
        }
        if !m.check("family.window_anchor", || {
            family.window.anchor == "work_accepted_at" && family.window.anchor_path.is_empty()
        })? {
            return Err(err("selected window anchor"));
        }
        for value in [
            &family.window.start_offset_us,
            &family.window.occurrence_us,
            &family.window.receipt_grace_us,
            &family.window.acceptance_grace_us,
            &family.window.activation_deadline_us,
            &family.window.correction_us,
        ] {
            arithmetic::integer(m, value, false)?;
        }
        for code in &family.codes {
            m.visit("family.code")?;
            match &code.effect {
                Effect::None => {}
                Effect::Premium { amount } | Effect::Discount { amount } => {
                    if !m.check("family.percent", || amount.kind == "percent")? {
                        return Err(err("family percent"));
                    }
                    arithmetic::decimal(m, &amount.percent)?;
                }
            }
        }
    }
    for i in 1..policy.families.len() {
        let mut j = i;
        while j > 0 {
            m.visit("family.sort_compare")?;
            let ordered = if m.representation_arena().is_some() {
                !semantic_text_order(
                    m,
                    &policy.families[j - 1].family_key,
                    &policy.families[j].family_key,
                )?
                .is_gt()
            } else {
                policy.families[j - 1].family_key <= policy.families[j].family_key
            };
            if ordered {
                break;
            }
            policy.families.swap(j - 1, j);
            j -= 1;
        }
    }
    for i in 1..policy.binding_slots.len() {
        let mut j = i;
        while j > 0 {
            m.visit("binding.sort_compare")?;
            let ordered = if m.representation_arena().is_some() {
                !semantic_text_order(
                    m,
                    &policy.binding_slots[j - 1].name,
                    &policy.binding_slots[j].name,
                )?
                .is_gt()
            } else {
                policy.binding_slots[j - 1].name <= policy.binding_slots[j].name
            };
            if ordered {
                break;
            }
            policy.binding_slots.swap(j - 1, j);
            j -= 1;
        }
    }
    let root_position = docs.len();
    if m.representation_arena().is_some() {
        let policy_body = super::owning_policy::build(m, &policy)?;
        docs.push(finish_owned_document(m, policy_body, false)?);
    } else {
        let policy_body = serde_json::to_value(&policy).map_err(|_| err("policy encoding"))?;
        docs.push(doc(m, policy_body)?);
    }
    let root = lookup(m, &docs, &input.root_policy_ref)?.reference();
    let root = if control_ownership.is_some() {
        DefinitionRef::new_with_work(root.kind(), root.id(), root.digest(), m)?
    } else {
        root.clone()
    };
    if !m.check("policy.root", || root.kind() == "workflow-policy")? {
        return Err(err("root policy kind"));
    }
    lookup(m, &docs, &policy.product_ref)?;
    lookup(m, &docs, &policy.slot_template_ref)?;
    lookup(m, &docs, &policy.context_schema_ref)?;
    lookup(m, &docs, &policy.compiler_ref)?;
    if m.representation_arena().is_some() {
        // The normalized sources already own their identity scalars. Compare
        // those borrowed fields directly; do not allocate BTree keys or debug
        // formatting graphs merely to detect the same semantic duplicates.
        fn semantic_name(body: &Value) -> Option<&Value> {
            body.get("schema_id")
                .or_else(|| body.get("product_id"))
                .or_else(|| body.get("template_id"))
                .or_else(|| body.get("policy_id"))
        }
        fn optional_same(m: &mut CoreMeter, a: Option<&Value>, b: Option<&Value>) -> Result<bool> {
            m.visit("source.optional_identity_presence")?;
            match (a, b) {
                (None, None) => Ok(true),
                (Some(a), Some(b)) => super::super::transition_constructor::shape::same(m, a, b),
                _ => Ok(false),
            }
        }
        for (i, d) in docs.iter().enumerate() {
            m.visit("source.identity")?;
            for prior in &docs[..i] {
                if same_literal(m, d.reference().id(), prior.reference().id())? {
                    return Err(err("duplicate source identity"));
                }
            }
            for prior in &docs[..i] {
                if same_literal(m, d.reference().kind(), prior.reference().kind())?
                    && optional_same(
                        m,
                        semantic_name(d.canonical_value()),
                        semantic_name(prior.canonical_value()),
                    )?
                    && optional_same(
                        m,
                        d.canonical_value().get("version"),
                        prior.canonical_value().get("version"),
                    )?
                {
                    return Err(err("duplicate semantic definition"));
                }
            }
        }
    } else {
        for d in &docs {
            m.visit("source.identity")?;
            if !identities.insert(d.reference().id().to_owned()) {
                return Err(err("duplicate source identity"));
            }
            let key = (
                d.reference().kind().to_owned(),
                d.canonical_value()
                    .get("schema_id")
                    .or_else(|| d.canonical_value().get("product_id"))
                    .or_else(|| d.canonical_value().get("template_id"))
                    .or_else(|| d.canonical_value().get("policy_id"))
                    .cloned(),
                d.canonical_value().get("version").cloned(),
            );
            if !semantic.insert(format!("{:?}", key)) {
                return Err(err("duplicate semantic definition"));
            }
        }
    }
    let total = if m.representation_arena().is_some() {
        let mut total = 0;
        for d in &docs {
            m.visit("source.aggregate_document_bytes")?;
            total = work::usize_add(m, total, d.canonical_bytes().len())?;
        }
        total
    } else {
        docs.iter().try_fold(0usize, |n, d| {
            n.checked_add(d.canonical_bytes().len())
                .ok_or_else(|| err("source bytes"))
        })?
    };
    let exceeds = if m.representation_arena().is_some() {
        work::compare(m, total, 1048576usize)?.is_gt()
    } else {
        total > 1048576
    };
    if exceeds {
        return Err(Error::new("PROGRAM_BUDGET_EXCEEDED", "source closure"));
    }
    let (compiled_document, compiled_fields) = if m.representation_arena().is_some() {
        let output = super::owning_plan::build(
            m,
            &docs,
            &root,
            &policy,
            &product,
            &template,
            recipe.plan_schema(),
            recipe.compiler_semantics(),
        )?;
        let document = finish_owned_document(m, output.body, true)?;
        (document, output.fields)
    } else {
        let mut reachable = vec![
            json!(root),
            json!(policy.product_ref),
            json!(policy.slot_template_ref),
            json!(policy.compiler_ref),
            json!(policy.context_schema_ref),
        ];
        for r in product.canonical_value()["fact_schemas"]
            .as_array()
            .ok_or_else(|| err("product schema refs"))?
        {
            m.visit("closure.ref")?;
            let mut exists = false;
            for prior in &reachable {
                if m.check("closure.ref_compare", || prior == r)? {
                    exists = true;
                    break;
                }
            }
            if !exists {
                reachable.push(r.clone());
            }
        }
        if !m.check("closure.count", || reachable.len() == docs.len())? {
            return Err(err("unused source document"));
        }
        for d in &docs {
            let value = json!(d.reference());
            let mut exists = false;
            for r in &reachable {
                if m.check("closure.member", || r == &value)? {
                    exists = true;
                    break;
                }
            }
            if !exists {
                return Err(err("unreachable source"));
            }
        }
        let source_refs = set(m, docs.iter().map(|d| json!(d.reference())).collect())?;
        let mut schema_refs = product.canonical_value()["fact_schemas"]
            .as_array()
            .ok_or_else(|| err("fact refs"))?
            .clone();
        let cr = json!(policy.context_schema_ref);
        if !schema_refs.contains(&cr) {
            schema_refs.push(cr);
        }
        schema_refs.sort_by(|a, b| a["digest"].as_str().cmp(&b["digest"].as_str()));
        let mut slots = Vec::new();
        for r in schema_refs {
            let reference: DefinitionRef =
                serde_json::from_value(r.clone()).map_err(|_| err("schema ref"))?;
            let d = lookup(m, &docs, &reference)?;
            let mut fields = d.canonical_value()["fields"]
                .as_array()
                .ok_or_else(|| err("fields"))?
                .clone();
            fields.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            for field in fields {
                m.visit("plan.fact_slot")?;
                slots.push(json!({"index":slots.len().to_string(),"schema_ref":r,"field":field}));
            }
        }
        let mut instructions = Vec::new();
        let mut edges = Vec::new();
        let mut explanations = Vec::new();
        let mut order = Vec::new();
        let mut producers = BTreeMap::<(String, String, String), String>::new();
        let mut ordered_rules: Vec<(usize, &super::model::Rule)> =
            policy.rules.iter().enumerate().collect();
        for i in 1..ordered_rules.len() {
            let mut j = i;
            while j > 0 {
                m.visit("rule.phase_sort")?;
                let key = |x: &(usize, &super::model::Rule)| {
                    (
                        if x.1.phase == "base" { 0 } else { 1 },
                        x.1.binding_slot.clone(),
                        x.0,
                    )
                };
                if key(&ordered_rules[j - 1]) <= key(&ordered_rules[j]) {
                    break;
                }
                ordered_rules.swap(j - 1, j);
                j -= 1;
            }
        }
        for (_, rule) in ordered_rules {
            m.visit("compile.rule_instruction")?;
            let ordinal = instructions.len().to_string();
            let sr = product.canonical_value()["activities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["name"] == rule.activity)
                .ok_or_else(|| err("activity"))?["fact_schema"]
                .clone();
            let (fields, kind, basis) = match &rule.formula {
                Formula::Fixed { .. } => (vec![], "fixed", vec![]),
                Formula::Unit { quantity_field, .. } => {
                    (vec![quantity_field.clone()], "unit", vec![])
                }
                Formula::Discount { basis, .. } => (
                    vec![],
                    "discount",
                    vec![producers
                        .get(&(
                            rule.activity.clone(),
                            rule.binding_slot.clone(),
                            basis.component_key.clone(),
                        ))
                        .ok_or_else(|| err("missing prior basis producer"))?
                        .clone()],
                ),
                Formula::AppFinalCost { .. } => {
                    let declaration = policy
                        .app_delayed_costs
                        .as_ref()
                        .and_then(|rows| {
                            rows.iter().find(|d| {
                                d.component_key == rule.component_key && d.activity == rule.activity
                            })
                        })
                        .ok_or_else(|| err("app cost declaration missing from compiled facts"))?;
                    (
                        vec![declaration.usage_field.clone()],
                        "app_final_cost",
                        vec![],
                    )
                }
                Formula::FinalCost { .. } => (vec![], "final_cost", vec![]),
                Formula::SupplierFinalCost { .. } => (vec![], "supplier_final_cost", vec![]),
            };
            let indices: Vec<_> = slots
                .iter()
                .filter(|s| {
                    s["schema_ref"] == sr && fields.iter().any(|f| s["field"]["name"] == *f)
                })
                .map(|s| s["index"].clone())
                .collect();
            let body = serde_json::to_value(rule).map_err(|_| err("rule encoding"))?;
            instructions.push(json!({"opcode":"evaluate_rule","ordinal":ordinal,"policy_ref":root,"rule":body,"fact_slots":indices,"binding_fact_slots":[],"basis_instructions":basis}));
            for b in basis {
                edges.push(json!({"producer":b,"consumer":ordinal}));
            }
            producers.insert(
                (
                    rule.activity.clone(),
                    rule.binding_slot.clone(),
                    rule.component_key.clone(),
                ),
                ordinal.clone(),
            );
            order.push(json!({"policy_ref":root,"rule_id":rule.rule_id}));
            explanations.push(json!({"ordinal":ordinal,"policy_ref":root,"rule_or_family":rule.rule_id,"fields":fields,"primitive":kind}));
        }
        for family in &policy.families {
            m.visit("compile.family_instruction")?;
            let ordinal = instructions.len().to_string();
            let sr = product.canonical_value()["activities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["name"] == family.trigger_activity)
                .ok_or_else(|| err("family activity"))?["fact_schema"]
                .clone();
            let fields: Vec<_> = slots
                .iter()
                .filter(|s| {
                    s["schema_ref"] == sr
                        && (s["field"]["name"] == family.code_field
                            || s["field"]["provenance"]["evidence_kind"] == "occurrence")
                })
                .map(|s| s["field"]["name"].clone())
                .collect();
            let indices: Vec<_> = slots
                .iter()
                .filter(|s| s["schema_ref"] == sr && fields.contains(&s["field"]["name"]))
                .map(|s| s["index"].clone())
                .collect();
            instructions.push(json!({"opcode":"evaluate_family","ordinal":ordinal,"policy_ref":root,"family":family,"fact_slots":indices,"target_component_key":family.selector.component_key}));
            explanations.push(json!({"ordinal":ordinal,"policy_ref":root,"rule_or_family":family.family_key,"fields":fields,"primitive":"family"}));
        }
        let activities:Vec<_>=product.canonical_value()["activities"].as_array().unwrap().iter().map(|a|{let mut v=json!({"activity":a["name"],"primitive":a["primitive"],"fact_schema_ref":a["fact_schema"],"source_slot":a["source_slot"]});if let Some(f)=a.get("funding_leg"){v["funding_leg"]=f.clone();}v}).collect();
        let mut plan = json!({"schema":recipe.plan_schema(),"compiler_ref":policy.compiler_ref,"compiler_semantics":recipe.compiler_semantics(),"evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits_profile":"workflow-limits/1","source_refs":source_refs,"activity_table":activities,"fact_slots":slots,"link_paths":set(m,policy.families.iter().map(|f|json!(f.selector)).collect())?,"component_slots":template.canonical_value()["components"],"rule_order":order,"typed_instructions":instructions,"dependency_edges":edges,"worst_case":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"},"explanation_map":explanations});
        if matches!(
            recipe,
            SelectedRecipe::LocalParentBudget1 | SelectedRecipe::LocalOperator1
        ) {
            plan["limits_profile"] = json!("workflow-local-limits/1");
            plan["worst_case"]
                .as_object_mut()
                .expect("closed plan")
                .remove("runtime_work");
        }
        if matches!(recipe, SelectedRecipe::LocalOperator1) {
            plan["limits_profile"] = json!("workflow-local-operator-limits/1");
            for field in ["compile_work", "loaded_refs", "bytes"] {
                plan["worst_case"]
                    .as_object_mut()
                    .expect("closed plan")
                    .remove(field);
            }
        }
        let compiled_document = if m.representation_arena().is_some() {
            owned_document(m, &plan, true)?
        } else {
            let bytes = codec::encode(m, &plan, 4194304)?;
            m.visit("plan.hash")?;
            m.read_bytes(bytes.len())?;
            let mut hash = Sha256::new();
            m.hash_bytes(b"bean-counter/workflow-plan/1\0".len() + bytes.len())?;
            hash.update(b"bean-counter/workflow-plan/1\0");
            hash.update(&bytes);
            let digest = format!("sha256:{}", canonical::hex(&hash.finalize()));
            let reference = DefinitionRef::new(
                "workflow-compiled-plan",
                &format!("wplan_{}", &digest[7..]),
                &digest,
            )?;
            SelectedDocument {
                storage: DocumentStorage::Legacy(LegacyDocument {
                    reference,
                    body: plan,
                    bytes,
                }),
            }
        };
        (compiled_document, slots.len())
    };
    let mut analysis = StaticAnalysis {
        fields: compiled_fields,
        rules: policy.rules.len(),
        families: policy.families.len(),
        maximum_path: policy
            .families
            .iter()
            .map(|f| f.selector.path.len())
            .max()
            .unwrap_or(0),
        compile_work: Some(m.usage().semantic_work),
    };
    let mut prepared_schemas = Vec::new();
    if control_ownership.is_some() {
        prepared_schemas
            .try_reserve_exact(source_count)
            .map_err(|_| err("compiled prepared-schema slots"))?;
    }
    for schema in schemas {
        let mut reference = None;
        for d in &docs {
            let same = if m.representation_arena().is_some() {
                super::super::transition_constructor::shape::same(
                    m,
                    d.canonical_value(),
                    schema.canonical_value(),
                )?
            } else {
                m.check("compile.prepared_schema", || {
                    *d.canonical_value() == *schema.canonical_value()
                })?
            };
            if same {
                let r = d.reference();
                reference = Some(if control_ownership.is_some() {
                    DefinitionRef::new_with_work(r.kind(), r.id(), r.digest(), m)?
                } else {
                    r.clone()
                });
                break;
            }
        }
        prepared_schemas.push((
            reference.ok_or_else(|| err("prepared schema missing"))?,
            schema,
        ));
    }
    analysis.compile_work = Some(m.usage().semantic_work);
    Ok(SelectedProgram {
        provenance: ProgramProvenance::Compiled,
        recipe,
        root_position,
        family_close_recipe: matches!(
            recipe,
            SelectedRecipe::FamilyClose1
                | SelectedRecipe::OwnerLocalCost1
                | SelectedRecipe::SupplierScopedConsent1
                | SelectedRecipe::AppOwnedCost1
                | SelectedRecipe::AppCostCausal1
                | SelectedRecipe::LocalParentBudget1
                | SelectedRecipe::LocalOperator1
        ),
        app_cost_causal_recipe: matches!(
            recipe,
            SelectedRecipe::AppCostCausal1
                | SelectedRecipe::LocalParentBudget1
                | SelectedRecipe::LocalOperator1
        ),
        app_owned_cost_recipe: matches!(
            recipe,
            SelectedRecipe::AppOwnedCost1
                | SelectedRecipe::AppCostCausal1
                | SelectedRecipe::LocalParentBudget1
                | SelectedRecipe::LocalOperator1
        ),
        supplier_scoped_consent_recipe: matches!(recipe, SelectedRecipe::SupplierScopedConsent1),
        owner_local_cost_recipe: matches!(
            recipe,
            SelectedRecipe::OwnerLocalCost1 | SelectedRecipe::SupplierScopedConsent1
        ),
        execution_registry: support.execution_registry(),
        root,
        sources: docs,
        plan: compiled_document,
        policy,
        schemas: prepared_schemas,
        analysis,
        _policy_decode_ownership: policy_decode_ownership,
        _source_projection_ownership: source_projection_ownership,
        _control_ownership: control_ownership,
    })
}

#[cfg(test)]
mod owning_document_tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    #[test]
    fn selected_document_output_preserves_hash_bytes_and_shared_lease() {
        let body = json!({"schema":"ledger-workflow-product/1","product_id":"product","version":"1","text":"\u{1f600}\n"});
        let mut legacy = CoreMeter::for_compile(1048576).unwrap();
        let old = doc(&mut legacy, body.clone()).unwrap();
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let new = doc(session.meter_mut(), body.clone()).unwrap();
        assert_eq!(new.reference(), old.reference());
        assert_eq!(new.canonical_value(), old.canonical_value());
        assert_eq!(new.canonical_bytes(), old.canonical_bytes());
        assert!(matches!(new.storage, DocumentStorage::Owned(_)));
        let held = arena.usage().unwrap().live_bytes;
        assert!(held > baseline);
        let copy = new.clone();
        assert_eq!(arena.usage().unwrap().live_bytes, held);
        drop(new);
        assert_eq!(arena.usage().unwrap().live_bytes, held);
        drop(copy);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        let plan = json!({"schema":"ledger-workflow-compiled-plan/6","typed_instructions":[]});
        let output = owned_document(session.meter_mut(), &plan, true).unwrap();
        let bytes = canonical::CanonicalBytes::from_value(&plan).unwrap();
        let mut hash = Sha256::new();
        hash.update(b"bean-counter/workflow-plan/1\0");
        hash.update(bytes.as_slice());
        assert_eq!(
            output.reference().digest(),
            format!("sha256:{}", canonical::hex(&hash.finalize()))
        );
        assert_eq!(output.canonical_bytes(), bytes.as_slice());
        drop(output);
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
        arena.cancellation_handle().cancel();
        assert!(doc(session.meter_mut(), body).is_err());
    }
}

#[path = "execution_reload.rs"]
mod execution_reload;
pub use execution_reload::{
    load_local_selected_execution, normalize_local_publication_input, rehydrate_selected_execution,
    NormalizedLocalSourceInput, SelectedExecutionReloadKey,
};
