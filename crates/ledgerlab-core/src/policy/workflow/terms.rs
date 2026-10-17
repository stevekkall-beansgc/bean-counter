//! Normalized agreement constraint assertions, not authenticated acceptance,
//! funding, grants, spend reservations or execution authority.
use super::{
    AgreementRef, Product, RoleAssignment, TemplateBook,
    facts::{name, reject_null},
};
use crate::{
    Error, Result,
    canonical::{self, CanonicalBytes},
    domain::Timestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Amount {
    currency: String,
    scale: u8,
    atoms: String,
}
impl Amount {
    fn validate(&self) -> Result<()> {
        if self.currency != "USD" || self.scale != 18 {
            return Err(Error::new("INVALID_SCALAR", "nonnegative USD18 amount"));
        }
        let atoms = crate::money::parse_atoms(&self.atoms)
            .map_err(|_| Error::new("INVALID_SCALAR", "canonical bounded amount"))?;
        if atoms < 0 {
            return Err(Error::new("INVALID_SCALAR", "nonnegative amount"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuantityCeiling {
    activity: String,
    field: String,
    unit: String,
    maximum: String,
}
fn quantities(rows: &mut Vec<QuantityCeiling>) -> Result<()> {
    if rows.len() > 64 {
        return Err(Error::new("INVALID_SHAPE", "quantity ceiling count"));
    }
    for row in rows.iter_mut() {
        for s in [&row.activity, &row.field, &row.unit] {
            name(s).map_err(|_| Error::new("INVALID_SHAPE", "quantity name"))?;
        }
        if row.maximum.starts_with('-') {
            return Err(Error::new("INVALID_SCALAR", "nonnegative quantity ceiling"));
        }
        row.maximum = crate::money::Decimal::parse(&row.maximum)
            .map_err(|_| Error::new("INVALID_SCALAR", "quantity decimal"))?
            .to_string();
    }
    // Each original activity/field has one declared unit. A repeated pair
    // refuses before JCS ordering, regardless of its spelling, unit or bound.
    let mut keys = BTreeSet::new();
    for row in rows.iter() {
        if !keys.insert((row.activity.clone(), row.field.clone())) {
            return Err(Error::new(
                "INVALID_SHAPE",
                "duplicate quantity activity/field",
            ));
        }
    }
    canonical::sort_set(rows)
}
fn validate_product_quantities(rows: &[QuantityCeiling], product: &Product) -> Result<()> {
    for row in rows {
        let activity = product
            .activity(&row.activity)
            .ok_or_else(|| Error::new("INVALID_REFERENCE", "quantity activity"))?;
        let schema = product
            .fact_schema(activity.fact_schema_id())
            .ok_or_else(|| Error::new("INVALID_REFERENCE", "quantity schema"))?;
        if schema
            .quantity_unit(&row.field)
            .map_err(|_| Error::new("INVALID_REFERENCE", "required numeric quantity field"))?
            != row.unit
        {
            return Err(Error::new("INVALID_REFERENCE", "quantity field unit"));
        }
        schema
            .normalize_quantity(
                &row.field,
                CanonicalBytes::from_value(&Value::String(row.maximum.clone()))?.as_slice(),
            )
            .map_err(|_| Error::new("INVALID_SCALAR", "quantity original range/precision"))?;
    }
    Ok(())
}
fn reference(value: &mut Value, kind: &str) -> Result<AgreementRef> {
    let r = AgreementRef::parse(CanonicalBytes::from_value(value)?.as_slice())?;
    if r.kind() != kind {
        return Err(fail("reference family"));
    }
    *value = serde_json::to_value(&r).map_err(|_| fail("reference"))?;
    Ok(r)
}
fn refs(values: &mut Vec<Value>, kind: &str, min: usize, max: usize) -> Result<()> {
    if !(min..=max).contains(&values.len()) {
        return Err(fail("reference count"));
    }
    for r in values.iter_mut() {
        reference(r, kind)?;
    }
    canonical::sort_set(values)
}
fn ids(scope: &[String; 2], agreement: &str, customer: &str) -> Result<()> {
    for s in [&scope[0], &scope[1], agreement, customer] {
        crate::domain::text(s, 128)?;
    }
    Ok(())
}
#[derive(Clone, Debug)]
struct Document {
    body: Value,
    bytes: CanonicalBytes,
    reference: AgreementRef,
}
impl Document {
    fn new(body: Value, kind: &str) -> Result<Self> {
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > 262144 {
            return Err(fail("normalized document byte limit"));
        }
        let schema = body["schema"]
            .as_str()
            .ok_or_else(|| fail("document schema"))?;
        let digest = super::document_digest(schema, &body)?;
        let reference=AgreementRef::parse(CanonicalBytes::from_value(&serde_json::json!({"kind":kind,"id":format!("wdoc_{}",&digest[7..]),"digest":digest}))?.as_slice())?;
        Ok(Self {
            body,
            bytes,
            reference,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CausalReversalClause {
    root_component_slot: String,
    family_slot: String,
    correction_source_slot: String,
    mode: String,
    standalone_deadline_effect: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppCausalClause {
    root_relation: String,
    mode: String,
    standalone_deadline_effect: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppCostDeclaration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    correction_anchor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    causal_reversal: Option<AppCausalClause>,
    component_key: String,
    activity: String,
    resource_funding_leg: String,
    usage_field: String,
    usage_unit: String,
    work_source_slot: String,
    cost_source_slot: String,
    correction_source_slot: String,
    finalize_within_us: String,
    correction_within_us: String,
    maximum_final_amount: Amount,
    on_missing: String,
    cost_basis: String,
    comparable_quantity_rule: String,
    allocation: String,
}
fn app_terms(input: &mut SlotInput) -> Result<()> {
    if !matches!(
        input.schema.as_str(),
        "ledger-workflow-slot-binding-terms/6" | "ledger-workflow-slot-binding-terms/7"
    ) {
        return if input.app_cost_terms.is_none() {
            Ok(())
        } else {
            Err(fail("app declarations require terms6"))
        };
    }
    let rows = input
        .app_cost_terms
        .as_mut()
        .ok_or_else(|| fail("app declarations required"))?;
    if !(1..=64).contains(&rows.len()) {
        return Err(fail("app declaration count"));
    }
    let mut keys = BTreeSet::new();
    for row in rows.iter_mut() {
        if input.schema == "ledger-workflow-slot-binding-terms/7" {
            if row.correction_anchor.as_deref() != Some("initial_cost_accepted_at") {
                return Err(fail("explicit non-sliding app correction anchor"));
            }
            if let Some(clause) = &row.causal_reversal {
                if clause.root_relation != "original_work_delayed_cost_dependency"
                    || clause.mode != "exact_inverse_and_permanent_close"
                    || clause.standalone_deadline_effect
                        != "does_not_expire_preaccepted_causal_reversal"
                {
                    return Err(fail("exact accepted app causal clause"));
                }
            }
        } else if row.correction_anchor.is_some() || row.causal_reversal.is_some() {
            return Err(fail("old app terms cannot acquire timing or late reversal"));
        }
        for value in [
            &row.component_key,
            &row.activity,
            &row.resource_funding_leg,
            &row.usage_field,
            &row.usage_unit,
            &row.work_source_slot,
            &row.cost_source_slot,
            &row.correction_source_slot,
        ] {
            name(value)?;
        }
        if !keys.insert((row.activity.clone(), row.component_key.clone()))
            || !input.allowed_activities.contains(&row.activity)
            || row.on_missing != "wait"
            || row.cost_basis != "app_attributed_final_amount"
            || row.comparable_quantity_rule != "when_present_lte_original_observed"
            || row.allocation != "one_call_one_component"
        {
            return Err(fail("app declaration identity or semantics"));
        }
        row.finalize_within_us = super::facts::counter(&row.finalize_within_us)?.to_string();
        row.correction_within_us = super::facts::counter(&row.correction_within_us)?.to_string();
        row.maximum_final_amount.validate()?;
    }
    canonical::sort_set(rows)
}
fn causal_clauses(input: &mut SlotInput) -> Result<()> {
    let supplier_fields = input.offer_ref.is_some()
        || input.cost_authority_ref.is_some()
        || input.provider_account_identity.is_some()
        || input.consumption_basis.is_some()
        || input.commitment_ref.is_some();
    if matches!(
        input.schema.as_str(),
        "ledger-workflow-slot-binding-terms/4" | "ledger-workflow-slot-binding-terms/5"
    ) {
        if input.schema == "ledger-workflow-slot-binding-terms/5" {
            reference(
                input
                    .commitment_ref
                    .as_mut()
                    .ok_or_else(|| fail("supplier commitment required"))?,
                "workflow-offer",
            )?;
        } else if input.commitment_ref.is_some() {
            return Err(fail("commitment requires supplier terms5"));
        }
        if input.book != TemplateBook::Supplier
            || input.causal_reversal_clauses.is_some()
            || input.exposure_scope.is_some()
            || input.consumption_basis.as_deref() != Some("owned_obligation_high_water")
        {
            return Err(fail("supplier original high-water terms"));
        }
        reference(
            input
                .offer_ref
                .as_mut()
                .ok_or_else(|| fail("supplier offer required"))?,
            "workflow-offer",
        )?;
        reference(
            input
                .cost_authority_ref
                .as_mut()
                .ok_or_else(|| fail("cost authority required"))?,
            "workflow-authority",
        )?;
        let account = input
            .provider_account_identity
            .as_ref()
            .ok_or_else(|| fail("provider account required"))?;
        crate::domain::text(&account.provider_namespace, 256)?;
        crate::domain::text(&account.provider_account_id, 128)?;
        crate::domain::text(&account.environment, 128)?;
        return Ok(());
    }
    // Additive fields may never widen any older wire recipe or its consent.
    if supplier_fields {
        return Err(fail("supplier fields require terms4"));
    }
    match input.schema.as_str() {
        "ledger-workflow-slot-binding-terms/1"
            if input.causal_reversal_clauses.is_none() && input.exposure_scope.is_none() =>
        {
            Ok(())
        }
        "ledger-workflow-slot-binding-terms/2"
        | "ledger-workflow-slot-binding-terms/3"
        | "ledger-workflow-slot-binding-terms/6"
        | "ledger-workflow-slot-binding-terms/7" => {
            let original_work = matches!(
                input.schema.as_str(),
                "ledger-workflow-slot-binding-terms/3"
                    | "ledger-workflow-slot-binding-terms/6"
                    | "ledger-workflow-slot-binding-terms/7"
            );
            if original_work {
                if input.exposure_scope.as_deref() != Some("original_work")
                    || input.book != TemplateBook::Retail
                {
                    return Err(fail("retail original-work exposure scope"));
                }
            } else if input.exposure_scope.is_some() {
                return Err(fail("old terms cannot acquire exposure scope"));
            }
            let rows = input
                .causal_reversal_clauses
                .as_mut()
                .ok_or_else(|| fail("causal clauses required"))?;
            if rows.len() > 64 || (!original_work && rows.is_empty()) {
                return Err(fail("causal clause count"));
            }
            let mut keys = BTreeSet::new();
            for row in rows.iter() {
                crate::domain::text(&row.root_component_slot, 128)
                    .map_err(|_| fail("causal root slot"))?;
                crate::domain::text(&row.family_slot, 128)
                    .map_err(|_| fail("causal family slot"))?;
                name(&row.correction_source_slot).map_err(|_| fail("causal correction source"))?;
                if row.mode != "exact_inverse_and_permanent_close"
                    || row.standalone_deadline_effect
                        != "does_not_expire_preaccepted_causal_reversal"
                    || !keys.insert((&row.root_component_slot, &row.family_slot))
                {
                    return Err(fail("causal clause semantics or duplicate identity"));
                }
            }
            canonical::sort_set(rows)
        }
        _ => Err(fail("slot terms schema")),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplierAccount {
    provider_namespace: String,
    provider_account_id: String,
    environment: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SlotInput {
    schema: String,
    scope: [String; 2],
    agreement_id: String,
    customer: String,
    slot: String,
    book: TemplateBook,
    role_slot: String,
    role_ref: Value,
    funding_leg: String,
    allowed_activities: Vec<String>,
    quantity_ceilings: Vec<QuantityCeiling>,
    exposure_ceiling: Amount,
    source_grant_refs: Vec<Value>,
    correction_source_slot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    causal_reversal_clauses: Option<Vec<CausalReversalClause>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exposure_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offer_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cost_authority_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_account_identity: Option<SupplierAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    consumption_basis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commitment_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app_cost_terms: Option<Vec<AppCostDeclaration>>,
}
#[derive(Clone, Debug)]
pub struct SlotBindingTerms {
    input: SlotInput,
    document: Document,
}
impl SlotBindingTerms {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let mut input: SlotInput = canonical::dto(value)?;
        app_terms(&mut input)?;
        causal_clauses(&mut input)?;
        ids(&input.scope, &input.agreement_id, &input.customer)?;
        for s in [
            &input.slot,
            &input.role_slot,
            &input.funding_leg,
            &input.correction_source_slot,
        ] {
            name(s)?;
        }
        if !(1..=16).contains(&input.allowed_activities.len()) {
            return Err(fail("allowed activity count"));
        }
        for activity in &input.allowed_activities {
            name(activity)?;
        }
        canonical::sort_set(&mut input.allowed_activities)?;
        reference(&mut input.role_ref, "workflow-roles")?;
        refs(&mut input.source_grant_refs, "workflow-source-grant", 1, 32)?;
        quantities(&mut input.quantity_ceilings)?;
        if input
            .quantity_ceilings
            .iter()
            .any(|q| !input.allowed_activities.contains(&q.activity))
        {
            return Err(fail("quantity ceiling outside allowed activity"));
        }
        input.exposure_ceiling.validate()?;
        let document = Document::new(
            serde_json::to_value(&input).map_err(|_| fail("slot terms"))?,
            "workflow-slot-terms",
        )?;
        Ok(Self { input, document })
    }
    /// Supplied roles/product remain assertions until the host authenticates
    /// their retained origins and verifies grants, funding and accepted policy.
    pub fn validate_role(&self, role: &RoleAssignment) -> Result<()> {
        let body = role.canonical_value();
        if self.input.role_ref["id"] != role.document_id()
            || self.input.role_ref["digest"] != role.content_digest()
            || body["scope"]
                != serde_json::to_value(&self.input.scope).map_err(|_| fail("scope"))?
            || body["agreement_id"] != self.input.agreement_id
            || body["customer"] != self.input.customer
            || body["role_slot"] != self.input.role_slot
        {
            return Err(fail("exact role ownership/reference"));
        }
        if self.input.book == TemplateBook::Retail
            && (body["bearer"] != self.input.customer || body["payer"] != self.input.customer)
        {
            return Err(fail("initial retail bearer/payer without delegation"));
        }
        Ok(())
    }
    pub fn validate_product(&self, product: &Product) -> Result<()> {
        for activity in &self.input.allowed_activities {
            if product.activity(activity).is_none() {
                return Err(fail("allowed product activity"));
            }
        }
        validate_product_quantities(&self.input.quantity_ceilings, product)
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.document.reference
    }
    pub fn slot(&self) -> &str {
        &self.input.slot
    }
    pub fn book(&self) -> TemplateBook {
        self.input.book
    }
    pub fn role_slot(&self) -> &str {
        &self.input.role_slot
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSlot {
    name: String,
    sources: Vec<String>,
    evidence_kinds: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    fact_schema_ref: Value,
    facts: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingEntry {
    slot: String,
    binding_ref: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FundingLeg {
    name: String,
    mode: String,
    resource_owner: String,
    payer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    evidence_refs: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supplier_binding_slot: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exposure {
    binding_slot: String,
    maximum: Amount,
}
fn exposures(rows: &mut Vec<Exposure>) -> Result<()> {
    if rows.len() > 16 {
        return Err(Error::new("INVALID_SHAPE", "exposure ceiling count"));
    }
    let mut slots = BTreeSet::new();
    for row in rows.iter() {
        name(&row.binding_slot).map_err(|_| Error::new("INVALID_SHAPE", "exposure slot name"))?;
        if !slots.insert(&row.binding_slot) {
            return Err(Error::new(
                "INVALID_SHAPE",
                "duplicate exposure binding slot",
            ));
        }
        row.maximum.validate()?;
    }
    canonical::sort_set(rows)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Projection {
    agreement_id: String,
    version: String,
    customer: String,
    source_namespace: String,
    slot_catalog_ref: Value,
    slot_instantiation_ref: Value,
    roles_refs: Vec<Value>,
    source_slots: Vec<SourceSlot>,
    accepted_context: Context,
    bindings: Vec<BindingEntry>,
    funding_legs: Vec<FundingLeg>,
    starts_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ends_at: Option<Timestamp>,
    quantity_ceilings: Vec<QuantityCeiling>,
    exposure_ceilings: Vec<Exposure>,
}
fn unique_names<'a>(values: impl Iterator<Item = &'a str>, min: usize, max: usize) -> Result<()> {
    let mut set = BTreeSet::new();
    let mut count = 0;
    for value in values {
        name(value)?;
        if !set.insert(value) {
            return Err(fail("duplicate named entry"));
        }
        count += 1;
    }
    if !(min..=max).contains(&count) {
        return Err(fail("named entry count"));
    }
    Ok(())
}
mod owned;
mod paid;
pub use owned::OwnedAcceptanceTerms;
impl Projection {
    fn normalize(&mut self, context: &super::FactSchema, app: bool) -> Result<()> {
        crate::domain::text(&self.agreement_id, 128)?;
        crate::domain::text(&self.customer, 128)?;
        crate::domain::text(&self.source_namespace, 256)?;
        let version = super::facts::counter(&self.version)?;
        if version == 0 {
            return Err(fail("positive binding version"));
        }
        self.version = version.to_string();
        reference(&mut self.slot_catalog_ref, "workflow-slot-catalog")?;
        reference(
            &mut self.slot_instantiation_ref,
            "workflow-slot-instantiation",
        )?;
        refs(&mut self.roles_refs, "workflow-roles", 1, 16)?;
        unique_names(self.source_slots.iter().map(|r| r.name.as_str()), 1, 32)?;
        for row in &mut self.source_slots {
            if !(1..=16).contains(&row.sources.len()) || row.evidence_kinds.len() > 5 {
                return Err(fail("source declaration count"));
            }
            for source in &row.sources {
                crate::domain::text(source, 256)?;
            }
            canonical::sort_set(&mut row.sources)?;
            for kind in &row.evidence_kinds {
                if ![
                    "occurrence",
                    "outcome",
                    "supplier_final_cost",
                    "meter_quantity",
                    "allocation_input",
                ]
                .contains(&kind.as_str())
                    && !(app && ["app_usage", "app_final_cost"].contains(&kind.as_str()))
                {
                    return Err(fail("source evidence kind"));
                }
            }
            canonical::sort_set(&mut row.evidence_kinds)?;
        }
        self.source_slots.sort_by(|a, b| a.name.cmp(&b.name));
        let r = reference(
            &mut self.accepted_context.fact_schema_ref,
            "workflow-fact-schema",
        )?;
        if r.id() != context.document_id()? || r.digest() != context.content_digest()? {
            return Err(fail("exact context schema"));
        }
        self.accepted_context.facts = context
            .normalize_context(
                CanonicalBytes::from_value(&self.accepted_context.facts)?.as_slice(),
            )?
            .canonical_value()
            .clone();
        unique_names(self.bindings.iter().map(|r| r.slot.as_str()), 1, 16)?;
        for row in &mut self.bindings {
            reference(&mut row.binding_ref, "workflow-slot-terms")?;
        }
        self.bindings.sort_by(|a, b| a.slot.cmp(&b.slot));
        unique_names(self.funding_legs.iter().map(|r| r.name.as_str()), 1, 16)?;
        for row in &mut self.funding_legs {
            if ![
                "customer_api",
                "customer_subscription",
                "customer_local",
                "platform",
            ]
            .contains(&row.mode.as_str())
            {
                return Err(fail("funding mode"));
            }
            for party in [&row.resource_owner, &row.payer] {
                crate::domain::text(party, 128)?;
            }
            if let Some(p) = &row.provider {
                crate::domain::text(p, 128)?;
            }
            if let Some(slot) = &row.supplier_binding_slot {
                name(slot)?;
            }
            refs(&mut row.evidence_refs, "workflow-evidence", 0, 16)?;
        }
        self.funding_legs.sort_by(|a, b| a.name.cmp(&b.name));
        if self
            .ends_at
            .as_ref()
            .is_some_and(|end| self.starts_at.micros() >= end.micros())
        {
            return Err(fail("nonempty half-open binding interval"));
        }
        quantities(&mut self.quantity_ceilings)?;
        exposures(&mut self.exposure_ceilings)?;
        for row in &self.exposure_ceilings {
            if !self.bindings.iter().any(|b| b.slot == row.binding_slot) {
                return Err(Error::new(
                    "INVALID_REFERENCE",
                    "exposure references bound slot",
                ));
            }
        }
        Ok(())
    }
    fn normalize_with_work<W: crate::validation_work::ValidationWork>(
        &mut self,
        context_ref: &AgreementRef,
        app: bool,
        work: &mut W,
        mut normalize_context: impl FnMut(&Value, &mut W) -> Result<Value>,
    ) -> Result<()> {
        use crate::validation_work::{self as w, ValidationVisit as Visit};
        paid::text(&self.agreement_id, 128, work)?;
        paid::text(&self.customer, 128, work)?;
        paid::text(&self.source_namespace, 256, work)?;
        let version = super::facts::counter_with_work(&self.version, work)?;
        let positive = !w::compare(work, version, 0u64)?.is_eq();
        paid::check(work, positive, "positive binding version")?;
        self.version = w::unsigned_string(u128::from(version), work)?;
        paid::reference(&self.slot_catalog_ref, "workflow-slot-catalog", work)?;
        paid::reference(
            &self.slot_instantiation_ref,
            "workflow-slot-instantiation",
            work,
        )?;
        paid::refs(&mut self.roles_refs, "workflow-roles", 1, 16, work)?;
        paid::names(
            self.source_slots.iter().map(|r| r.name.as_str()),
            1,
            32,
            work,
        )?;
        for row in &mut self.source_slots {
            work.visit(Visit::Element)?;
            if w::compare(work, row.sources.len(), 1usize)?.is_lt()
                || w::compare(work, row.sources.len(), 16usize)?.is_gt()
                || w::compare(work, row.evidence_kinds.len(), 5usize)?.is_gt()
            {
                return Err(fail("source declaration count"));
            }
            for source in &row.sources {
                work.visit(Visit::Element)?;
                paid::text(source, 256, work)?;
            }
            paid::string_set(&mut row.sources, work)?;
            for kind in &row.evidence_kinds {
                work.visit(Visit::Element)?;
                let old = paid::member(
                    kind,
                    &[
                        "occurrence",
                        "outcome",
                        "supplier_final_cost",
                        "meter_quantity",
                        "allocation_input",
                    ],
                    work,
                )?;
                work.visit(Visit::Constraint)?;
                if !old && !(app && paid::member(kind, &["app_usage", "app_final_cost"], work)?) {
                    return Err(fail("source evidence kind"));
                }
            }
            paid::string_set(&mut row.evidence_kinds, work)?;
        }
        paid::sort(&mut self.source_slots, work, |a, b, work| {
            paid::compare_text(&a.name, &b.name, work)
        })?;
        let r = paid::reference(
            &self.accepted_context.fact_schema_ref,
            "workflow-fact-schema",
            work,
        )?;
        if !paid::compare_text(r.id(), context_ref.id(), work)?.is_eq()
            || !paid::compare_text(r.digest(), context_ref.digest(), work)?.is_eq()
        {
            return Err(fail("exact context schema"));
        }
        self.accepted_context.facts = normalize_context(&self.accepted_context.facts, work)?;
        paid::names(self.bindings.iter().map(|r| r.slot.as_str()), 1, 16, work)?;
        for row in &self.bindings {
            work.visit(Visit::Element)?;
            paid::reference(&row.binding_ref, "workflow-slot-terms", work)?;
        }
        paid::sort(&mut self.bindings, work, |a, b, work| {
            paid::compare_text(&a.slot, &b.slot, work)
        })?;
        paid::names(
            self.funding_legs.iter().map(|r| r.name.as_str()),
            1,
            16,
            work,
        )?;
        for row in &mut self.funding_legs {
            work.visit(Visit::Element)?;
            if !paid::member(
                &row.mode,
                &[
                    "customer_api",
                    "customer_subscription",
                    "customer_local",
                    "platform",
                ],
                work,
            )? {
                return Err(fail("funding mode"));
            }
            for party in [&row.resource_owner, &row.payer] {
                paid::text(party, 128, work)?;
            }
            if let Some(provider) = &row.provider {
                work.visit(Visit::Constraint)?;
                paid::text(provider, 128, work)?;
            }
            if let Some(slot) = &row.supplier_binding_slot {
                work.visit(Visit::Constraint)?;
                super::facts::name_with_work(slot, work)?;
            }
            // Preserve each original row's validation/refusal order.
            paid::refs(&mut row.evidence_refs, "workflow-evidence", 0, 16, work)?;
        }
        paid::sort(&mut self.funding_legs, work, |a, b, work| {
            paid::compare_text(&a.name, &b.name, work)
        })?;
        if let Some(end) = &self.ends_at {
            work.visit(Visit::Constraint)?;
            if !w::compare(work, self.starts_at.micros(), end.micros())?.is_lt() {
                return Err(fail("nonempty half-open binding interval"));
            }
        }
        paid::quantities(&mut self.quantity_ceilings, work)?;
        paid::exposures(&mut self.exposure_ceilings, work)?;
        for row in &self.exposure_ceilings {
            work.visit(Visit::Element)?;
            let mut found = false;
            for binding in &self.bindings {
                work.visit(Visit::RefLookup)?;
                if paid::compare_text(&binding.slot, &row.binding_slot, work)?.is_eq() {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(Error::new(
                    "INVALID_REFERENCE",
                    "exposure references bound slot",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TermsInput {
    schema: String,
    scope: [String; 2],
    publication_ref: Value,
    agreement_id: String,
    customer: String,
    version: String,
    binding_inputs: Projection,
}
#[derive(Clone, Debug)]
pub struct AcceptanceTerms {
    input: TermsInput,
    document: Document,
    terms_digest: String,
}
impl AcceptanceTerms {
    /// Supplied context is data; its original publication membership, customer
    /// assent and all referenced role/grant/funding rights remain host checks.
    pub fn parse(raw: &[u8], context: &super::FactSchema) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let mut input: TermsInput = canonical::dto(value)?;
        if !matches!(
            input.schema.as_str(),
            "ledger-workflow-acceptance-terms/1" | "ledger-workflow-acceptance-terms/2"
        ) {
            return Err(fail("acceptance terms schema"));
        }
        ids(&input.scope, &input.agreement_id, &input.customer)?;
        reference(&mut input.publication_ref, "workflow-publication")?;
        input.binding_inputs.normalize(
            context,
            input.schema == "ledger-workflow-acceptance-terms/2",
        )?;
        input.version = super::facts::counter(&input.version)?.to_string();
        if input.agreement_id != input.binding_inputs.agreement_id
            || input.customer != input.binding_inputs.customer
            || input.version != input.binding_inputs.version
        {
            return Err(fail("exact duplicated agreement/customer/version"));
        }
        let document = Document::new(
            serde_json::to_value(&input).map_err(|_| fail("terms"))?,
            "workflow-acceptance-terms",
        )?;
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"bean-counter/workflow-acceptance-terms/1\0");
        h.update(document.bytes.as_slice());
        let terms_digest = format!("sha256:{}", canonical::hex(&h.finalize()));
        Ok(Self {
            input,
            document,
            terms_digest,
        })
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.document.reference
    }
    pub fn terms_digest(&self) -> &str {
        &self.terms_digest
    }
    pub fn validate_product(&self, product: &Product) -> Result<()> {
        validate_product_quantities(&self.input.binding_inputs.quantity_ceilings, product)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssentInput {
    schema: String,
    scope: [String; 2],
    agreement_id: String,
    customer: String,
    terms_digest: String,
    mode: String,
    acceptor: String,
    accepted_at: Timestamp,
    evidence_ref: Value,
    operator: String,
    operator_attestation_ref: Value,
}
#[derive(Clone, Debug)]
pub struct AssentRecord {
    document: Document,
}
impl AssentRecord {
    /// Exact terms/timestamp assertions only. The host verifies actual customer
    /// enrollment/evidence/operator/mode/time; this is not a signature proof.
    pub fn parse(raw: &[u8], terms: &AcceptanceTerms) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let mut input: AssentInput = canonical::dto(value)?;
        if input.schema != "ledger-workflow-assent/1"
            || !["real", "synthetic"].contains(&input.mode.as_str())
        {
            return Err(fail("assent schema/mode"));
        }
        ids(&input.scope, &input.agreement_id, &input.customer)?;
        for s in [&input.acceptor, &input.operator] {
            crate::domain::text(s, 128)?;
        }
        reference(&mut input.evidence_ref, "workflow-evidence")?;
        reference(&mut input.operator_attestation_ref, "workflow-evidence")?;
        if input.scope != terms.input.scope
            || input.agreement_id != terms.input.agreement_id
            || input.customer != terms.input.customer
            || input.terms_digest != terms.terms_digest
        {
            return Err(fail("exact assent terms identity"));
        }
        // Signing before a future binding begins is not prohibited by this
        // data codec. Applicable authenticated evidence-time rules are host work.
        Ok(Self {
            document: Document::new(
                serde_json::to_value(input).map_err(|_| fail("assent"))?,
                "workflow-assent",
            )?,
        })
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        self.document.bytes.as_slice()
    }
    pub fn canonical_value(&self) -> &Value {
        &self.document.body
    }
    pub fn reference(&self) -> &AgreementRef {
        &self.document.reference
    }
}
fn fail(s: &str) -> Error {
    Error::new("AGREEMENT_TERMS", s)
}
#[cfg(test)]
mod tests;

mod offer;
pub use offer::SupplierOffer;
