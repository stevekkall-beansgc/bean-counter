//! Additive SupplierOffer2 normalization DATA. No supplied offer or Ref becomes
//! retained consent, source custody, owner authority or executable support.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    agreement_id: String,
    version: String,
    provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    policy_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commitment_ref: Option<Value>,
    valuation_mode: String,
    role_refs: Vec<Value>,
    activity_names: Vec<String>,
    unit_ceilings: Vec<QuantityCeiling>,
    exposure_ceiling: Amount,
    allowed_component_slots: Vec<String>,
    source_slots: Vec<String>,
    starts_at: Timestamp,
    ends_at: Timestamp,
    evidence_refs: Vec<Value>,
}
// Exact frozen RecordKind DATA vocabulary. Prefix, original body/reference,
// membership and read/role/consent eligibility are separately checked by the
// actual original semantic handler; this list is not a provenance registry.
const RECORD_KINDS: &[&str] = &[
    "legacy-document",
    "legacy-event",
    "workflow-acceptance-terms",
    "workflow-accepted-agreement",
    "workflow-action",
    "workflow-activation",
    "workflow-agreement-intent",
    "workflow-assent",
    "workflow-authority",
    "workflow-binding",
    "workflow-build-manifest",
    "workflow-capacity",
    "workflow-capacity-retirement",
    "workflow-chain-state",
    "workflow-compiled-plan",
    "workflow-compiler-descriptor",
    "workflow-correction",
    "workflow-correction-capacity",
    "workflow-cost",
    "workflow-customer",
    "workflow-decision",
    "workflow-delegation",
    "workflow-dependency",
    "workflow-economic-authority",
    "workflow-evaluation",
    "workflow-evidence",
    "workflow-evidence-verification",
    "workflow-fact-attestation",
    "workflow-fact-schema",
    "workflow-family-state",
    "workflow-head-transition",
    "workflow-invocation",
    "workflow-link",
    "workflow-manifest",
    "workflow-offer",
    "workflow-operation",
    "workflow-outcome",
    "workflow-outcome-claim",
    "workflow-policy",
    "workflow-product",
    "workflow-publication",
    "workflow-receipt",
    "workflow-roles",
    "workflow-slot-catalog",
    "workflow-slot-instantiation",
    "workflow-slot-template",
    "workflow-slot-terms",
    "workflow-source-enrollment",
    "workflow-source-grant",
    "workflow-source-state",
    "workflow-stage-instance",
    "workflow-target",
    "workflow-verification",
    "workflow-verified-fact",
    "workflow-work",
    "workflow-work-claim",
    "workflow-authority-prefix-observation",
    "workflow-authority-audit-verifier",
    "workflow-financial-history-observation",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefData {
    kind: String,
    id: String,
    digest: String,
}
fn generic_refs(values: &mut Vec<Value>, maximum: usize) -> Result<()> {
    if values.is_empty() || values.len() > maximum {
        return Err(fail("offer reference count"));
    }
    for value in values.iter_mut() {
        let r: RefData = canonical::dto(value.clone())?;
        if !RECORD_KINDS.contains(&r.kind.as_str()) {
            return Err(fail("offer reference data family"));
        }
        crate::domain::text(&r.id, 128)?;
        crate::domain::prefixed(&r.digest, "sha256:")?;
        *value = serde_json::to_value(r).map_err(|_| fail("offer reference"))?;
    }
    canonical::sort_set(values)
}
#[derive(Clone, Debug)]
pub struct SupplierOffer {
    input: Input,
    document: Document,
}
impl SupplierOffer {
    /// Closed original offer normalization. fixed_unit remains DATA; the
    /// owner-local final-evidence executor separately requires final_evidence.
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 262144)?;
        reject_null(&value)?;
        let mut input: Input = canonical::dto(value)?;
        if !matches!(
            input.schema.as_str(),
            "ledger-workflow-offer/2" | "ledger-workflow-offer/3"
        ) || !matches!(
            input.valuation_mode.as_str(),
            "fixed_unit" | "final_evidence"
        ) {
            return Err(fail("supplier offer schema/mode"));
        }
        crate::domain::text(&input.agreement_id, 128)?;
        crate::domain::text(&input.provider, 128)?;
        input.version = super::super::facts::counter(&input.version)?.to_string();
        match input.schema.as_str() {
            "ledger-workflow-offer/2" if input.commitment_ref.is_none() => {
                reference(
                    input
                        .policy_ref
                        .as_mut()
                        .ok_or_else(|| fail("offer policy required"))?,
                    "workflow-policy",
                )?;
            }
            "ledger-workflow-offer/3" if input.policy_ref.is_none() => {
                reference(
                    input
                        .commitment_ref
                        .as_mut()
                        .ok_or_else(|| fail("offer commitment required"))?,
                    "workflow-offer",
                )?;
            }
            _ => return Err(fail("offer reference recipe")),
        }
        generic_refs(&mut input.role_refs, 16)?;
        generic_refs(&mut input.evidence_refs, 16)?;
        if !(1..=16).contains(&input.activity_names.len())
            || !(1..=128).contains(&input.allowed_component_slots.len())
            || !(1..=32).contains(&input.source_slots.len())
        {
            return Err(fail("supplier offer set count"));
        }
        for n in input.activity_names.iter().chain(input.source_slots.iter()) {
            name(n)?;
        }
        for slot in &input.allowed_component_slots {
            crate::domain::text(slot, 128)?;
        }
        canonical::sort_set(&mut input.activity_names)?;
        canonical::sort_set(&mut input.allowed_component_slots)?;
        canonical::sort_set(&mut input.source_slots)?;
        quantities(&mut input.unit_ceilings)?;
        if input
            .unit_ceilings
            .iter()
            .any(|q| !input.activity_names.contains(&q.activity))
        {
            return Err(fail("offer quantity outside allowed activity"));
        }
        input.exposure_ceiling.validate()?;
        if input.starts_at.micros() >= input.ends_at.micros() {
            return Err(fail("supplier offer interval"));
        }
        let document = Document::new(
            serde_json::to_value(&input).map_err(|_| fail("supplier offer"))?,
            "workflow-offer",
        )?;
        Ok(Self { input, document })
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
    pub fn validate_product(&self, product: &Product) -> Result<()> {
        for activity in &self.input.activity_names {
            if product.activity(activity).is_none() {
                return Err(fail("offer activity outside product"));
            }
        }
        validate_product_quantities(&self.input.unit_ceilings, product)
    }
}
