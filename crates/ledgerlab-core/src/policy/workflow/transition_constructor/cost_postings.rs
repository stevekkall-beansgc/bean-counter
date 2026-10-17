//! Shared original-cost posting construction. Evidence eligibility and customer
//! authority are established by the actual app/AP executor, not this helper.
//! This code contains no supplier claim, invocation, owner or consumption step.
//! Both routes use the SAME pricing::PricedInstruction and existing action IDs.
use super::{
    CoreMeter, body,
    data::{DraftContextData, ExactRef},
    emitted_record::ConstructedEnvelope,
    meter, pricing, shape,
    working_value::owning_reference,
};
use crate::{
    Result,
    canonical::audit_arena::{AuditArena, AuditedValue},
    validation_work::{self as w, RepresentationKind, ValidationWork},
};
use num_bigint::BigInt;
use serde_json::Value;

pub(super) struct CostPosting {
    pub action: Option<ConstructedEnvelope>,
    pub step: AuditedValue,
}
pub(super) fn scalar(m: &mut CoreMeter, arena: &AuditArena, n: &BigInt) -> Result<AuditedValue> {
    let _scratch = arena.reserve_external_payload(512, m)?;
    // The existing arithmetic rule bounds signed scalars to 512 bits (156
    // decimal bytes). Output and transient formatting overlap are prepaid.
    m.scan_bytes(156)?;
    m.write_bytes(156)?;
    let raw = n.to_string();
    if w::compare(m, raw.len(), 156usize)?.is_gt() {
        return Err(shape::fail("cost exact scalar encoding"));
    }
    arena.string_value(&raw, m)
}
pub(super) fn amount(m: &mut CoreMeter, arena: &AuditArena, n: &BigInt) -> Result<AuditedValue> {
    let magnitude = meter::absolute(m, n)?;
    let maximum = super::arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, &magnitude, &0.into())?.is_lt() || meter::cmp(m, &magnitude, &maximum)?.is_gt()
    {
        return Err(shape::fail("cost posting Amount bound"));
    }
    let constants = m.parse_owned_representation(
        arena,
        br#"{"currency":"USD","scale":18}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let atoms = scalar(m, arena, n)?;
    arena.object_from_fields(
        &[
            ("currency", &constants.value()["currency"]),
            ("scale", &constants.value()["scale"]),
            ("atoms", atoms.value()),
        ],
        m,
    )
}
pub(super) fn operation_key(
    m: &mut CoreMeter,
    arena: &AuditArena,
    c: &DraftContextData,
) -> Result<AuditedValue> {
    let store = arena.string_value(&c.key.logical_store_id, m)?;
    let source = arena.string_value(&c.key.source, m)?;
    let id = arena.string_value(&c.key.operation_id, m)?;
    let first = arena.string_value(&c.key.scope[0], m)?;
    let second = arena.string_value(&c.key.scope[1], m)?;
    let scope = arena.array_from_values(&[first.value(), second.value()], m)?;
    arena.object_from_fields(
        &[
            ("logical_store_id", store.value()),
            ("scope", scope.value()),
            ("source", source.value()),
            ("operation_id", id.value()),
        ],
        m,
    )
}
pub(super) fn original_effect(
    m: &mut CoreMeter,
    arena: &AuditArena,
    c: &DraftContextData,
    binding: &Value,
    original_work_claim: &ExactRef,
    component_slot: &Value,
) -> Result<String> {
    let key = operation_key(m, arena, c)?;
    let claim = arena.string_value(&original_work_claim.id, m)?;
    let constants = m.parse_owned_representation(
        arena,
        br#"["self","original"]"#,
        RepresentationKind::SourceDefinition,
    )?;
    let tuple = arena.array_from_values(
        &[
            &key.value()["scope"],
            &binding["agreement_id"],
            component_slot,
            claim.value(),
            &constants.value()[0],
            &constants.value()[1],
        ],
        m,
    )?;
    body::semantic_id(m, "weffect_", "workflow-effect", tuple.value())
}
pub(super) fn action_id(
    m: &mut CoreMeter,
    arena: &AuditArena,
    effect: &Value,
    revision: &Value,
    posting: &Value,
) -> Result<String> {
    let tuple = arena.array_from_values(&[effect, revision, posting], m)?;
    body::semantic_id(m, "waction_", "workflow-action", tuple.value())
}
/// Internal, already selected original operands. No public constructor or host
/// success flag exists; each complete executor must validate these exact joins.
pub(super) struct OriginalCostPosting<'a> {
    pub binding: &'a Value,
    pub original_work_claim: &'a ExactRef,
    pub component_slot: &'a Value,
    pub root_slot: &'a Value,
    pub book: &'a Value,
    pub terms_ref: &'a ExactRef,
    pub terms: &'a Value,
    pub role_ref: &'a ExactRef,
    pub decision_id: &'a str,
    pub revision: &'a BigInt,
    pub replacement: bool,
}
/// Emit one existing original/replacement action and its exact explanation.
/// Zero retains the original instruction/rational/rounding step without an
/// action. AP lineage is deliberately outside this shared customer path.
/// Caller output-vector/Ref metadata pre-admission remains mandatory; all JSON
/// bodies and numeric formatting below retain their owning arena artifacts.
pub(super) fn original_cost_posting(
    m: &mut CoreMeter,
    c: &DraftContextData,
    original: OriginalCostPosting<'_>,
    price: &pricing::PricedInstruction,
) -> Result<CostPosting> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning original cost posting"))?;
    if !w::compare(m, price.basis_components.len(), 0usize)?.is_eq() {
        return Err(shape::fail("cost formula unexpected action basis"));
    }
    let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-action/1","original":"original","replacement":"replacement","true":"true","posted":"posted","zero":"zero"}"#,RepresentationKind::SourceDefinition)?;
    let revision = scalar(m, &arena, original.revision)?;
    let decision = arena.string_value(original.decision_id, m)?;
    let key = operation_key(m, &arena, c)?;
    let terms = owning_reference(m, &arena, original.terms_ref)?;
    let role = owning_reference(m, &arena, original.role_ref)?;
    let effect = original_effect(
        m,
        &arena,
        c,
        original.binding,
        original.original_work_claim,
        original.component_slot,
    )?;
    let effect = arena.string_value(&effect, m)?;
    let rounded = amount(m, &arena, &price.atoms.into())?;
    let empty = arena.array_from_values(&[], m)?;
    let nonzero = !meter::cmp(m, &price.atoms.into(), &0.into())?.is_eq();
    let posting = if original.replacement {
        &constants.value()["replacement"]
    } else {
        &constants.value()["original"]
    };
    let action = if nonzero {
        let id = action_id(m, &arena, effect.value(), revision.value(), posting)?;
        let value = arena.object_from_fields(
            &[
                ("schema", &constants.value()["schema"]),
                ("effect_id", effect.value()),
                ("decision_id", decision.value()),
                ("revision", revision.value()),
                ("posting_role", posting),
                ("book", original.book),
                ("component_slot", original.component_slot),
                ("root_component_slot", original.root_slot),
                ("role_ref", role.value()),
                ("slot_terms_ref", terms.value()),
                ("funding_leg", &original.terms["funding_leg"]),
                ("amount", rounded.value()),
                ("basis_refs", empty.value()),
                ("operation_key", key.value()),
            ],
            m,
        )?;
        Some(body::record_borrowed(
            m,
            &c.key,
            "workflow-action",
            id,
            value.value(),
        )?)
    } else {
        None
    };
    let instruction = arena.string_value(&price.ordinal, m)?;
    let numerator = scalar(m, &arena, &price.rational.numerator)?;
    let denominator = scalar(m, &arena, &price.rational.denominator)?;
    let step_fields = [
        ("instruction", instruction.value()),
        ("component_slot", original.component_slot),
        ("predicate", &constants.value()["true"]),
        ("basis_refs", empty.value()),
        ("rational_numerator", numerator.value()),
        ("rational_denominator", denominator.value()),
        ("rounded", rounded.value()),
        (
            "disposition",
            if nonzero {
                &constants.value()["posted"]
            } else {
                &constants.value()["zero"]
            },
        ),
    ];
    let mut step = arena.object_from_fields(&step_fields, m)?;
    if let Some(action) = &action {
        let reference = owning_reference(m, &arena, &action.reference)?;
        step = super::working_value::WorkingValue::Owned(step)
            .with_fields(m, &[("action_ref", reference.value())])?
            .into_owned()?;
    }
    Ok(CostPosting { action, step })
}

/// Exact inverse of an ACTUAL current action already checked by the enclosing
/// original-price verifier. Saved money is used only for this exact reversal,
/// never as the input to the replacement formula. No supplier/AP state enters.
pub(super) fn exact_inverse(
    m: &mut CoreMeter,
    c: &DraftContextData,
    old_ref: &ExactRef,
    old: &Value,
    original_instruction: &pricing::PricedInstruction,
    new_revision: &BigInt,
) -> Result<CostPosting> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning exact cost inverse"))?;
    shape::closed(
        m,
        old,
        &[
            "schema",
            "effect_id",
            "decision_id",
            "revision",
            "posting_role",
            "book",
            "component_slot",
            "root_component_slot",
            "role_ref",
            "slot_terms_ref",
            "funding_leg",
            "amount",
            "basis_refs",
            "operation_key",
        ],
        &[],
    )?;
    let schema = shape::text(m, old, "schema")?;
    if !shape::same_text(m, schema, "ledger-workflow-action/1")? {
        return Err(shape::fail("exact cost inverse action schema"));
    }
    let role = shape::text(m, old, "posting_role")?;
    if !shape::same_text(m, role, "original")? && !shape::same_text(m, role, "replacement")? {
        return Err(shape::fail(
            "exact cost inverse requires active original posting",
        ));
    }
    shape::closed(m, &old["amount"], &["currency", "scale", "atoms"], &[])?;
    let currency = shape::text(m, &old["amount"], "currency")?;
    if !shape::same_text(m, currency, "USD")?
        || !shape::same(m, &old["amount"]["scale"], &Value::from(18))?
    {
        return Err(shape::fail("exact cost inverse units"));
    }
    let atoms = shape::text(m, &old["amount"], "atoms")?;
    let atoms = super::arithmetic::integer(m, atoms, false)?;
    if !meter::cmp(m, &atoms, &0.into())?.is_gt() {
        return Err(shape::fail(
            "exact cost inverse requires nonzero actual posting",
        ));
    }
    let negative = meter::neg(m, &atoms)?;
    let rounded = amount(m, &arena, &negative)?;
    let revision = scalar(m, &arena, new_revision)?;
    let operation = operation_key(m, &arena, c)?;
    let inverse_of = owning_reference(m, &arena, old_ref)?;
    let constants = m.parse_owned_representation(
        &arena,
        br#"{"inverse":"inverse","true":"true","one":"1"}"#,
        RepresentationKind::SourceDefinition,
    )?;
    let copied = arena.clone_value(old, m)?;
    let inverse = super::working_value::WorkingValue::Owned(copied).with_fields(
        m,
        &[
            ("revision", revision.value()),
            ("posting_role", &constants.value()["inverse"]),
            ("amount", rounded.value()),
            ("operation_key", operation.value()),
            ("inverse_of", inverse_of.value()),
        ],
    )?;
    let id = action_id(
        m,
        &arena,
        &old["effect_id"],
        revision.value(),
        &constants.value()["inverse"],
    )?;
    let action = body::record_borrowed(m, &c.key, "workflow-action", id, &inverse)?;
    let instruction = arena.string_value(&original_instruction.ordinal, m)?;
    let numerator = scalar(m, &arena, &negative)?;
    let reference = owning_reference(m, &arena, &action.reference)?;
    let step = arena.object_from_fields(
        &[
            ("instruction", instruction.value()),
            ("component_slot", &old["component_slot"]),
            ("predicate", &constants.value()["true"]),
            ("basis_refs", &old["basis_refs"]),
            ("rational_numerator", numerator.value()),
            ("rational_denominator", &constants.value()["one"]),
            ("rounded", rounded.value()),
            ("disposition", &constants.value()["inverse"]),
            ("action_ref", reference.value()),
        ],
        m,
    )?;
    Ok(CostPosting {
        action: Some(action),
        step,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{arithmetic, data::OperationKey};
    use super::*;
    use crate::validation_work::OriginalAuditSession;
    use serde_json::json;
    fn reference(kind: &str, prefix: &str, n: char) -> ExactRef {
        let hex = n.to_string().repeat(64);
        ExactRef {
            kind: kind.into(),
            id: format!("{prefix}{hex}"),
            digest: format!("sha256:{hex}"),
        }
    }
    fn price(m: &mut CoreMeter, eligible: i128) -> pricing::PricedInstruction {
        // Literal original formula and actual cost input, not a saved posting
        // or precomputed expected price supplied to the constructor.
        let formula = json!({"multiplier":{"numerator":"1","denominator":"1"},"markup":"0"});
        let eligible = arithmetic::Ratio {
            numerator: eligible.into(),
            denominator: 1.into(),
        };
        let scale = arithmetic::Ratio {
            numerator: 1_000_000_000_000_000_000_i128.into(),
            denominator: 1.into(),
        };
        let rational =
            pricing::customer_final_cost_rational(m, &formula, &eligible, &scale).unwrap();
        let maximum = arithmetic::integer(m, "999999999999999999999999999999", false).unwrap();
        let atoms = arithmetic::rounded(m, &rational, &maximum).unwrap();
        pricing::PricedInstruction {
            ordinal: "2".into(),
            component_key: "model:cost".into(),
            root_key: "model:cost".into(),
            role_slot: "model:retail".into(),
            binding_slot: "model:customer".into(),
            basis_components: vec![],
            rational,
            atoms,
        }
    }
    #[test]
    fn customer_cost_posting_inverse_replacement_and_zero_need_no_ap_records() {
        let arena = AuditArena::new().unwrap();
        let baseline = arena.usage().unwrap().live_bytes;
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut m = session.meter_mut().original_economic_meter().unwrap();
        let metadata = arena
            .reserve_external_payload(
                4 * (std::mem::size_of::<ConstructedEnvelope>() + 263) + 4096,
                &mut m,
            )
            .unwrap();
        let c = DraftContextData {
            key: OperationKey {
                logical_store_id: "store".into(),
                scope: ["tenant".into(), "project".into()],
                source: "urn:app".into(),
                operation_id: "cost".into(),
            },
            actor: "backend".into(),
            canonical_economic_namespace: "urn:app".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-17T10:00:00.000000Z".into(),
            accepted_at: "2026-10-17T10:00:00.000000Z".into(),
            authorities: vec![],
            source_observation_inputs: vec![],
        };
        let work_claim = reference("workflow-work-claim", "wclaim_", 'a');
        let terms_ref = reference("workflow-slot-terms", "wdoc_", 'b');
        let role_ref = reference("workflow-roles", "wdoc_", 'c');
        let binding = json!({"agreement_id":"agreement"});
        let terms = json!({"funding_leg":"model:platform"});
        let slot = json!("model_component");
        let book = json!("retail");
        macro_rules! selection {
            ($revision:expr,$replacement:expr) => {
                OriginalCostPosting {
                    binding: &binding,
                    original_work_claim: &work_claim,
                    component_slot: &slot,
                    root_slot: &slot,
                    book: &book,
                    terms_ref: &terms_ref,
                    terms: &terms,
                    role_ref: &role_ref,
                    decision_id: "cost_decision",
                    revision: $revision,
                    replacement: $replacement,
                }
            };
        }
        let initial_price = price(&mut m, 2_000_000_000_000_000_000);
        let initial =
            original_cost_posting(&mut m, &c, selection!(&1.into(), false), &initial_price)
                .unwrap();
        let original = initial.action.as_ref().unwrap();
        let original_body = original.decoded(&mut m, 4194304).unwrap();
        assert_eq!(original_body["book"], "retail");
        assert_eq!(original_body["amount"]["atoms"], "2000000000000000000");
        let inverse = exact_inverse(
            &mut m,
            &c,
            &original.reference,
            &original_body,
            &initial_price,
            &2.into(),
        )
        .unwrap();
        let inverse_body = inverse
            .action
            .as_ref()
            .unwrap()
            .decoded(&mut m, 4194304)
            .unwrap();
        assert_eq!(inverse_body["amount"]["atoms"], "-2000000000000000000");
        assert_eq!(inverse_body["inverse_of"], json!(original.reference));
        let replacement_price = price(&mut m, 1_600_000_000_000_000_000);
        let replacement =
            original_cost_posting(&mut m, &c, selection!(&2.into(), true), &replacement_price)
                .unwrap();
        let replacement_body = replacement
            .action
            .as_ref()
            .unwrap()
            .decoded(&mut m, 4194304)
            .unwrap();
        assert_eq!(replacement_body["amount"]["atoms"], "1600000000000000000");
        assert_eq!(replacement_body["posting_role"], "replacement");
        assert_eq!(original_body["effect_id"], replacement_body["effect_id"]);
        assert_eq!(original_body["effect_id"], inverse_body["effect_id"]);
        let zero_price = price(&mut m, 0);
        let zero =
            original_cost_posting(&mut m, &c, selection!(&3.into(), true), &zero_price).unwrap();
        assert!(zero.action.is_none());
        assert_eq!(zero.step.value()["disposition"], "zero");
        assert_eq!(zero.step.value()["rational_numerator"], "0");
        assert!(zero.step.value().get("action_ref").is_none());
        session.meter_mut().finish_original_economic(m).unwrap();
        // No invocation, supplier claim/owner/allocation/consumption or supplier
        // action was constructed. This is the common posting DATA unit only;
        // app evidence/authority/full candidate remain the actual entry gates.
        drop((
            initial,
            inverse,
            replacement,
            zero,
            original_body,
            inverse_body,
            replacement_body,
            metadata,
        ));
        assert_eq!(arena.usage().unwrap().live_bytes, baseline);
    }
}
