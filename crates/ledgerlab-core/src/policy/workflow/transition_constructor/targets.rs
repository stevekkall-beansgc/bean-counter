//! Original component targets and predeclared family state DATA. Inputs come
//! only from the shared constructor's original program/authority and newly
//! built work decision. These are not reservation or admission certificates.
use super::super::selected_program::SelectedProgram;
use super::emitted_record::ConstructedEnvelope;
use super::{
    arithmetic::{self, Ratio},
    authority::Operands,
    body,
    data::{DraftContextData, ExactRef},
    meter, shape, CoreMeter,
};
use crate::{domain::Timestamp, Error, Result};
use num_bigint::BigInt;
use num_traits::{One, ToPrimitive};
use serde_json::{json, Value};

macro_rules! call {
    ($f:path, $m:expr, $a:expr $(,)?) => {{
        let a = $a;
        $f($m, a)
    }};
    ($f:path, $m:expr, $a:expr, $b:expr $(,)?) => {{
        let (a, b) = ($a, $b);
        $f($m, a, b)
    }};
    ($f:path, $m:expr, $a:expr, $b:expr, $c:expr $(,)?) => {{
        let (a, b, c) = ($a, $b, $c);
        $f($m, a, b, c)
    }};
}
pub(super) struct Targets {
    pub records: Vec<ConstructedEnvelope>,
    pub heads: Vec<(ExactRef, String, String, ExactRef)>,
    pub obligations: Vec<super::working_value::WorkingValue>,
    pub(super) _ownership: Option<crate::canonical::audit_arena::ExternalBufferLease>,
}
fn fail(s: &str) -> Error {
    shape::fail(s)
}
fn unsupported(s: &str) -> Error {
    Error::new("UNSUPPORTED_SCHEMA_PROFILE", s)
}
fn amount(n: &BigInt) -> Value {
    json!({"currency":"USD","scale":18,"atoms":n.to_string()})
}
// Preserve the original target's amount-shaped encoding without importing
// an additional posting-atom ceiling into a summed original component net.
pub(super) fn owned_amount(
    m: &mut CoreMeter,
    arena: &crate::canonical::audit_arena::AuditArena,
    n: &BigInt,
) -> Result<crate::canonical::audit_arena::AuditedValue> {
    let constants = m.parse_owned_representation(
        arena,
        br#"{"currency":"USD","scale":18}"#,
        crate::validation_work::RepresentationKind::SourceDefinition,
    )?;
    let atoms = super::cost_postings::scalar(m, arena, n)?;
    arena.object_from_fields(
        &[
            ("currency", &constants.value()["currency"]),
            ("scale", &constants.value()["scale"]),
            ("atoms", atoms.value()),
        ],
        m,
    )
}
fn number(m: &mut CoreMeter, value: &Value) -> Result<BigInt> {
    arithmetic::integer(
        m,
        value.as_str().ok_or_else(|| fail("target scalar"))?,
        false,
    )
}
fn positive_offset(m: &mut CoreMeter, terms: &Value, key: &str) -> Result<BigInt> {
    let v = call!(number, m, call!(shape::field, m, terms, key)?)?;
    if call!(meter::cmp, m, &v, &0.into())?.is_lt() {
        return Err(fail("negative family window duration"));
    }
    Ok(v)
}
fn timestamp(m: &mut CoreMeter, value: &BigInt) -> Result<String> {
    if call!(meter::cmp, m, value, &BigInt::from(i64::MIN))?.is_lt()
        || call!(meter::cmp, m, value, &BigInt::from(i64::MAX))?.is_gt()
    {
        return Err(fail("family window overflow"));
    }
    let raw = value
        .to_i64()
        .ok_or_else(|| fail("family window overflow"))?;
    let parsed = Timestamp::from_micros_with_work(raw, m)?;
    if m.representation_arena().is_some() {
        m.scan_bytes(parsed.as_str().len())?;
    }
    m.write_bytes(parsed.as_str().len())?;
    Ok(parsed.as_str().to_owned())
}
/// Original S12 window arithmetic; no current clock and no deadline extension.
pub(super) fn frozen_window(
    m: &mut CoreMeter,
    terms: &Value,
    work: &ExactRef,
    at: &str,
) -> Result<Option<super::working_value::WorkingValue>> {
    if !call!(
        shape::same_text,
        m,
        call!(shape::text, m, terms, "anchor")?,
        "work_accepted_at"
    )? {
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, terms, "anchor")?,
            "verified_predecessor_occurred_at",
        )? {
            return Err(unsupported("original family anchor"));
        }
        return Ok(None);
    }
    if !call!(
        meter::cmp,
        m,
        &call!(shape::array, m, terms, "anchor_path")?.len().into(),
        &0.into(),
    )?
    .is_eq()
    {
        return Err(fail("own-work family anchor path"));
    }
    let anchor = BigInt::from(Timestamp::parse_with_work(at, m)?.micros());
    let deadline_offset = positive_offset(m, terms, "activation_deadline_us")?;
    let deadline = meter::add(m, &anchor, &deadline_offset)?;
    let start_offset = call!(number, m, call!(shape::field, m, terms, "start_offset_us")?)?;
    let starts = meter::add(m, &anchor, &start_offset)?;
    let duration = positive_offset(m, terms, "occurrence_us")?;
    if !call!(meter::cmp, m, &duration, &0.into())?.is_gt() {
        return Err(fail("positive occurrence window"));
    }
    let occurs_before = meter::add(m, &starts, &duration)?;
    let receipt = positive_offset(m, terms, "receipt_grace_us")?;
    let received_by = meter::add(m, &occurs_before, &receipt)?;
    let acceptance = positive_offset(m, terms, "acceptance_grace_us")?;
    let accepted_by = meter::add(m, &received_by, &acceptance)?;
    let correction = positive_offset(m, terms, "correction_us")?;
    let correction_until = meter::add(m, &accepted_by, &correction)?;
    if let Some(arena) = m.representation_arena() {
        let n =
            6 * (std::mem::size_of::<String>() + 29) + 8 * std::mem::size_of::<(&str, &Value)>();
        let _controls = arena.reserve_external_payload(n, m)?;
        m.write_bytes(n)?;
        let reference = super::working_value::owning_reference(m, &arena, work)?;
        let anchor = arena.string_value(at, m)?;
        let raw = timestamp(m, &deadline)?;
        let deadline = arena.string_value(&raw, m)?;
        drop(raw);
        let raw = timestamp(m, &starts)?;
        let starts = arena.string_value(&raw, m)?;
        drop(raw);
        let raw = timestamp(m, &occurs_before)?;
        let occurs = arena.string_value(&raw, m)?;
        drop(raw);
        let raw = timestamp(m, &received_by)?;
        let received = arena.string_value(&raw, m)?;
        drop(raw);
        let raw = timestamp(m, &accepted_by)?;
        let accepted = arena.string_value(&raw, m)?;
        drop(raw);
        let raw = timestamp(m, &correction_until)?;
        let correction = arena.string_value(&raw, m)?;
        drop(raw);
        let value = arena.object_from_fields(
            &[
                ("anchor_ref", reference.value()),
                ("anchor_time", anchor.value()),
                ("activation_deadline", deadline.value()),
                ("starts_at", starts.value()),
                ("occurs_before", occurs.value()),
                ("received_by", received.value()),
                ("accepted_by", accepted.value()),
                ("correction_until", correction.value()),
            ],
            m,
        )?;
        return Ok(Some(super::working_value::WorkingValue::Owned(value)));
    }
    Ok(Some(super::working_value::WorkingValue::Legacy(
        json!({"anchor_ref":work,"anchor_time":at,"activation_deadline":timestamp(m,&deadline)?,"starts_at":timestamp(m,&starts)?,"occurs_before":timestamp(m,&occurs_before)?,"received_by":timestamp(m,&received_by)?,"accepted_by":timestamp(m,&accepted_by)?,"correction_until":timestamp(m,&correction_until)?}),
    )))
}
/// Max over original alternatives after action rounding, separately per sign.
pub(super) fn maxima(m: &mut CoreMeter, family: &Value, net: &BigInt) -> Result<(BigInt, BigInt)> {
    let limit = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    let mut premium = BigInt::from(0);
    let mut discount = BigInt::from(0);
    for code in call!(shape::array, m, family, "codes")? {
        m.visit("target.original_code_alternative")?;
        let effect = call!(shape::field, m, code, "effect")?;
        let kind = call!(shape::text, m, effect, "kind")?;
        if call!(shape::same_text, m, kind, "none")? {
            continue;
        }
        let is_premium = call!(shape::same_text, m, kind, "premium")?;
        if !is_premium && !call!(shape::same_text, m, kind, "discount")? {
            return Err(fail("family effect"));
        }
        let formula = call!(shape::field, m, effect, "amount")?;
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, formula, "kind")?,
            "percent"
        )? {
            return Err(unsupported("selected family formula"));
        }
        let scalar = call!(shape::text, m, formula, "percent")?;
        let percent = arithmetic::decimal(m, scalar)?;
        if call!(meter::cmp, m, &percent.numerator, &0.into())?.is_lt() {
            return Err(fail("negative family percent"));
        }
        let hundred = meter::mul(m, &percent.denominator, &100.into())?;
        if call!(meter::cmp, m, &percent.numerator, &hundred)?.is_gt() {
            return Err(fail("family percent above 100"));
        }
        let ratio = arithmetic::rational(m, percent.numerator, hundred)?;
        let value = arithmetic::multiply(
            m,
            &Ratio {
                numerator: net.clone(),
                denominator: BigInt::one(),
            },
            &ratio,
        )?;
        let rounded = BigInt::from(arithmetic::rounded(m, &value, &limit)?);
        let max = if is_premium {
            &mut premium
        } else {
            &mut discount
        };
        if call!(meter::cmp, m, &rounded, &*max)?.is_gt() {
            *max = rounded;
        }
    }
    for (n, key) in [
        (&premium, "maximum_premium"),
        (&discount, "maximum_discount"),
    ] {
        let bound = call!(shape::field, m, family, key)?;
        let atoms = call!(number, m, call!(shape::field, m, bound, "atoms")?)?;
        if call!(meter::cmp, m, n, &atoms)?.is_gt() {
            return Err(fail("family original ceiling"));
        }
    }
    Ok((premium, discount))
}

pub(super) fn prospective_decision(
    m: &mut CoreMeter,
    target_id: &str,
    family_slot: &str,
) -> Result<String> {
    if let Some(arena) = m.representation_arena() {
        let target = arena.string_value(target_id, m)?;
        let slot = arena.string_value(family_slot, m)?;
        let discriminator = arena.string_value("outcome", m)?;
        let tuple =
            arena.array_from_values(&[target.value(), slot.value(), discriminator.value()], m)?;
        return body::semantic_id(m, "wdecision_", "workflow-decision", tuple.value());
    }
    body::semantic_id(
        m,
        "wdecision_",
        "workflow-decision",
        &json!([target_id, family_slot, "outcome"]),
    )
}
pub(super) fn build(
    program: &SelectedProgram,
    c: &DraftContextData,
    payload: &Value,
    operands: &Operands,
    work: &ConstructedEnvelope,
    selected_operands: (
        &ConstructedEnvelope,
        &ConstructedEnvelope,
        &[ConstructedEnvelope],
    ),
    m: &mut CoreMeter,
) -> Result<Targets> {
    let (claim, decision, actions) = selected_operands;
    let plan = program.compiled_plan().canonical_value();
    let mut family_capacity = 0usize;
    let ownership = if let Some(arena) = m.representation_arena() {
        use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
        let mut slot_bytes = 0usize;
        for row in shape::array(m, plan, "typed_instructions")? {
            m.visit("target.output_dimension")?;
            let opcode = shape::text(m, row, "opcode")?;
            if !shape::same_text(m, opcode, "evaluate_family")? {
                continue;
            }
            let family = &row["family"];
            if !shape::same(
                m,
                &family["selector"]["target_activity"],
                &payload["activity"],
            )? {
                continue;
            }
            family_capacity = w::usize_add(m, family_capacity, 1)?;
            let key = shape::text(m, family, "family_key")?;
            let mappings = shape::array(m, &operands.instantiation, "family_map")?;
            let mapping = shape::named(m, mappings, "template_key", key)?;
            let slot = shape::text(m, mapping, "slot_id")?;
            slot_bytes = w::usize_add(m, slot_bytes, slot.len())?;
        }
        let per = std::mem::size_of::<(std::borrow::Cow<'_, str>, Vec<&Value>)>()
            + 2 * std::mem::size_of::<ConstructedEnvelope>()
            + std::mem::size_of::<(ExactRef, String, String, ExactRef)>()
            + std::mem::size_of::<crate::canonical::audit_arena::AuditedValue>()
            + std::mem::size_of::<super::working_value::WorkingValue>()
            + std::mem::size_of::<super::record_body::RecordBody>()
            + std::mem::size_of::<(
                std::borrow::Cow<'_, str>,
                Option<super::working_value::WorkingValue>,
            )>()
            + 5 * (std::mem::size_of::<ExactRef>() + 64 + 128 + 71)
            + 8 * (std::mem::size_of::<String>() + 128)
            + 48 * std::mem::size_of::<(&str, &Value)>()
            + 3 * std::mem::size_of::<&Value>();
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&family_capacity.into(), &per.into()],
        )?;
        let n = family_capacity
            .checked_mul(per)
            .ok_or_else(|| fail("target output dimension"))?;
        // Each root's family Vec is explicitly reserved to this actual
        // original family count; sum <=F*F pointers, with no growth overlap.
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&family_capacity.into(), &family_capacity.into()],
        )?;
        let f2 = family_capacity
            .checked_mul(family_capacity)
            .ok_or_else(|| fail("target grouping dimension"))?;
        let ptr = std::mem::size_of::<&Value>();
        ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&f2.into(), &ptr.into()])?;
        let groups = f2
            .checked_mul(ptr)
            .ok_or_else(|| fail("target grouping payload"))?;
        let n = w::usize_add(m, n, groups)?;
        let n = w::usize_add(m, n, slot_bytes)?;
        let n = w::usize_add(
            m,
            n,
            std::mem::size_of::<Targets>() + 10 * std::mem::size_of::<Vec<Value>>(),
        )?;
        let owner = arena.reserve_external_payload(n, m)?;
        m.write_bytes(n)?;
        Some(owner)
    } else {
        None
    };
    let mut roots: Vec<(std::borrow::Cow<'_, str>, Vec<&Value>)> = vec![];
    if ownership.is_some() {
        roots
            .try_reserve_exact(family_capacity)
            .map_err(|_| fail("target grouping allocation"))?;
    }
    for instruction in call!(shape::array, m, plan, "typed_instructions")? {
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, instruction, "opcode")?,
            "evaluate_family"
        )? {
            continue;
        }
        let family = call!(shape::field, m, instruction, "family")?;
        let selector = call!(shape::field, m, family, "selector")?;
        if !call!(
            shape::same,
            m,
            call!(shape::field, m, selector, "target_activity")?,
            call!(shape::field, m, payload, "activity")?,
        )? {
            continue;
        }
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, selector, "basis")?,
            "booked_original_net"
        )? {
            return Err(unsupported("selected target basis"));
        }
        let root_key = call!(shape::text, m, selector, "component_key")?;
        if !program.root_is_selected(
            m,
            &payload["activity"],
            root_key,
            &operands.binding["accepted_context"],
        )? {
            continue;
        }

        let mapped = call!(
            shape::named,
            m,
            call!(shape::array, m, &operands.instantiation, "component_map")?,
            "template_key",
            root_key,
        )?;
        let slot = call!(shape::text, m, mapped, "slot_id")?;
        let mut found = None;
        for (i, (prior, _)) in roots.iter().enumerate() {
            if call!(shape::same_text, m, prior, slot)? {
                found = Some(i);
                break;
            }
        }
        match found {
            Some(i) => roots[i].1.push(family),
            None => {
                let slot = if ownership.is_some() {
                    std::borrow::Cow::Borrowed(slot)
                } else {
                    std::borrow::Cow::Owned(slot.to_owned())
                };
                let mut families = Vec::new();
                if ownership.is_some() {
                    families
                        .try_reserve_exact(family_capacity)
                        .map_err(|_| fail("target family grouping allocation"))?;
                }
                families.push(family);
                roots.push((slot, families));
            }
        }
    }
    let mut out = Targets {
        records: vec![],
        heads: vec![],
        obligations: vec![],
        _ownership: ownership,
    };
    if out._ownership.is_some() {
        use crate::validation_work::{ArithmeticOperation, ValidationWork};
        ValidationWork::arithmetic(
            m,
            ArithmeticOperation::Multiply,
            &[&family_capacity.into(), &2usize.into()],
        )?;
        let records = family_capacity
            .checked_mul(2)
            .ok_or_else(|| fail("target record allocation dimension"))?;
        out.records
            .try_reserve_exact(records)
            .map_err(|_| fail("target record allocation"))?;
        out.heads
            .try_reserve_exact(family_capacity)
            .map_err(|_| fail("target head allocation"))?;
        out.obligations
            .try_reserve_exact(family_capacity)
            .map_err(|_| fail("target obligation allocation"))?;
    }
    for (root_slot, families) in roots {
        m.visit("target.original_root")?;
        let catalog_component = call!(
            shape::named,
            m,
            call!(shape::array, m, &operands.catalog, "components")?,
            "slot_id",
            &root_slot,
        )?;
        if !call!(
            shape::same_text,
            m,
            call!(shape::text, m, catalog_component, "root_slot")?,
            &root_slot,
        )? {
            return Err(fail("family target must be direct root"));
        }
        let mut basis = vec![];
        let mut basis_views = Vec::new();
        let _basis_owner = if let Some(arena) = m.representation_arena() {
            use crate::validation_work::{self as w, ArithmeticOperation, ValidationWork};
            let n = actions.len();
            let per = std::mem::size_of::<&ExactRef>();
            ValidationWork::arithmetic(
                m,
                ArithmeticOperation::Multiply,
                &[&n.into(), &per.into()],
            )?;
            let bytes = n
                .checked_mul(per)
                .ok_or_else(|| fail("target basis views"))?;
            let bytes = w::usize_add(m, bytes, std::mem::size_of::<Vec<&ExactRef>>())?;
            let owner = arena.reserve_external_payload(bytes, m)?;
            m.write_bytes(bytes)?;
            basis_views
                .try_reserve_exact(n)
                .map_err(|_| fail("target basis allocation"))?;
            Some(owner)
        } else {
            None
        };
        let mut net = BigInt::from(0);
        for action in actions {
            m.visit("target.original_action")?;
            let a = action.decoded(m, 4194304)?;
            if !call!(
                shape::same_text,
                m,
                call!(shape::text, m, &a, "root_component_slot")?,
                &root_slot
            )? {
                continue;
            }
            if let Some(arena) = m.representation_arena() {
                let book = shape::text(m, &a, "book")?;
                let role = super::working_value::owning_reference(m, &arena, &operands.role_ref)?;
                if !shape::same_text(m, book, "retail")?
                    || !shape::same(m, &a["role_ref"], role.value())?
                {
                    return Err(fail("family target book/role"));
                }
            } else {
                if !call!(
                    shape::same_text,
                    m,
                    call!(shape::text, m, &a, "book")?,
                    "retail"
                )? || !call!(
                    shape::same,
                    m,
                    call!(shape::field, m, &a, "role_ref")?,
                    &json!(operands.role_ref)
                )? {
                    return Err(fail("family target book/role"));
                }
            }
            let amount = call!(shape::field, m, &a, "amount")?;
            let raw_atoms = call!(shape::text, m, amount, "atoms")?;
            let atoms = arithmetic::integer(m, raw_atoms, true)?;
            net = meter::add(m, &net, &atoms)?;
            if m.representation_arena().is_some() {
                basis_views.push(&action.reference);
            } else {
                basis.push(action.reference.clone());
            }
        }
        if call!(meter::cmp, m, &net, &0.into())?.is_lt() {
            return Err(fail("negative original target net"));
        }
        let basis = if let Some(arena) = m.representation_arena() {
            super::working_value::WorkingValue::Owned(super::app_cost::reference_set(
                m,
                &arena,
                &basis_views,
            )?)
        } else {
            super::working_value::WorkingValue::Legacy(json!(body::ref_set(m, basis)?))
        };
        let target_id = if m.representation_arena().is_some() {
            super::work_keys::target(
                m,
                &c.key,
                &operands.binding["agreement_id"],
                &claim.reference.id,
                &root_slot,
            )?
        } else {
            body::semantic_id(
                m,
                "wtarget_",
                "workflow-target",
                &json!([
                    c.key.scope,
                    operands.binding["agreement_id"],
                    claim.reference.id,
                    root_slot
                ]),
            )?
        };
        let mut target_families = vec![];
        let mut owned_families = Vec::new();
        let mut family_states = vec![];
        if out._ownership.is_some() {
            owned_families
                .try_reserve_exact(family_capacity)
                .map_err(|_| fail("target family value owners"))?;
            family_states
                .try_reserve_exact(family_capacity)
                .map_err(|_| fail("target family state controls"))?;
        }
        let mut total_discount = BigInt::from(0);
        for family in families {
            m.visit("target.predeclared_family")?;
            let key = call!(shape::text, m, family, "family_key")?;
            let mapping = call!(
                shape::named,
                m,
                call!(shape::array, m, &operands.instantiation, "family_map")?,
                "template_key",
                key,
            )?;
            let slot = call!(shape::text, m, mapping, "slot_id")?;
            let declaration = call!(
                shape::named,
                m,
                call!(shape::array, m, &operands.catalog, "families")?,
                "slot_id",
                slot,
            )?;
            if !call!(
                shape::same_text,
                m,
                call!(shape::text, m, declaration, "target_component_slot")?,
                &root_slot,
            )? || !call!(
                shape::same,
                m,
                call!(shape::field, m, declaration, "role_slot")?,
                call!(shape::field, m, catalog_component, "role_slot")?,
            )? {
                return Err(fail("family original root/role"));
            }
            let (premium, discount) = maxima(m, family, &net)?;
            total_discount = meter::add(m, &total_discount, &discount)?;
            let prospective = prospective_decision(m, &target_id, slot)?;
            let window_terms = call!(shape::field, m, family, "window")?;
            if let Some(arena) = m.representation_arena() {
                let key = arena.string_value(key, m)?;
                let slot = arena.string_value(slot, m)?;
                let prospective = arena.string_value(&prospective, m)?;
                let premium = owned_amount(m, &arena, &premium)?;
                let discount = owned_amount(m, &arena, &discount)?;
                let value = arena.object_from_fields(
                    &[
                        ("family_key", key.value()),
                        ("family_slot", slot.value()),
                        ("prospective_decision_id", prospective.value()),
                        ("window_terms", window_terms),
                        ("maximum_premium", premium.value()),
                        ("maximum_discount", discount.value()),
                    ],
                    m,
                )?;
                owned_families.push(value);
            } else {
                target_families.push(json!({"family_key":key,"family_slot":slot,"prospective_decision_id":prospective,"window_terms":window_terms,"maximum_premium":amount(&premium),"maximum_discount":amount(&discount)}));
            }
            let window = frozen_window(m, window_terms, &work.reference, &c.accepted_at)?;
            let slot = if out._ownership.is_some() {
                std::borrow::Cow::Borrowed(slot)
            } else {
                std::borrow::Cow::Owned(slot.to_owned())
            };
            family_states.push((slot, window));
        }
        if call!(meter::cmp, m, &total_discount, &net)?.is_gt() {
            return Err(fail("aggregate original family discounts"));
        }
        let target = if let Some(arena) = m.representation_arena() {
            let mut views = Vec::new();
            views
                .try_reserve_exact(owned_families.len())
                .map_err(|_| fail("target family views"))?;
            for row in &owned_families {
                m.visit("target.family_value_view")?;
                views.push(row.value());
            }
            let families = arena.array_from_values(&views, m)?;
            let schema = arena.string_value("ledger-workflow-component-target/1", m)?;
            let key = super::cost_postings::operation_key(m, &arena, c)?;
            let claimed = super::working_value::owning_reference(m, &arena, &claim.reference)?;
            let worked = super::working_value::owning_reference(m, &arena, &work.reference)?;
            let decided = super::working_value::owning_reference(m, &arena, &decision.reference)?;
            let slot = arena.string_value(&root_slot, m)?;
            let net = owned_amount(m, &arena, &net)?;
            let at = arena.string_value(&c.accepted_at, m)?;
            let definition = program.compiled_plan().reference();
            let kind = arena.string_value(definition.kind(), m)?;
            let id = arena.string_value(definition.id(), m)?;
            let digest = arena.string_value(definition.digest(), m)?;
            let compiled = arena.object_from_fields(
                &[
                    ("kind", kind.value()),
                    ("id", id.value()),
                    ("digest", digest.value()),
                ],
                m,
            )?;
            let value = arena.object_from_fields(
                &[
                    ("schema", schema.value()),
                    ("scope", &key.value()["scope"]),
                    ("agreement_id", &operands.binding["agreement_id"]),
                    ("work_claim_ref", claimed.value()),
                    ("work_ref", worked.value()),
                    ("root_component_slot", slot.value()),
                    ("original_decision_ref", decided.value()),
                    ("original_action_refs", &basis),
                    ("original_net", net.value()),
                    ("policy_ref", &payload["policy_ref"]),
                    ("product_ref", &payload["product_ref"]),
                    ("binding_ref", &payload["binding_ref"]),
                    ("compiled_plan_ref", compiled.value()),
                    ("families", families.value()),
                    ("original_work_accepted_at", at.value()),
                ],
                m,
            )?;
            body::record_borrowed(m, &c.key, "workflow-target", target_id, value.value())?
        } else {
            let target = body::record(
                m,
                &c.key,
                "workflow-target",
                target_id,
                json!({"schema":"ledger-workflow-component-target/1","scope":c.key.scope,"agreement_id":operands.binding["agreement_id"],"work_claim_ref":claim.reference,"work_ref":work.reference,"root_component_slot":root_slot,"original_decision_ref":decision.reference,"original_action_refs":basis,"original_net":amount(&net),"policy_ref":payload["policy_ref"],"product_ref":payload["product_ref"],"binding_ref":payload["binding_ref"],"compiled_plan_ref":program.compiled_plan().reference(),"families":target_families,"original_work_accepted_at":c.accepted_at}),
            )?;
            out.obligations.push(super::working_value::WorkingValue::Legacy(json!({"kind":"original-family-reservation-required","target_ref":target.reference,"root_decision_ref":decision.reference,"families":target_families})));
            target
        };
        if let Some(arena) = m.representation_arena() {
            let value = target.decoded(m, 4194304)?;
            let kind = arena.string_value("original-family-reservation-required", m)?;
            let target_view = super::working_value::owning_reference(m, &arena, &target.reference)?;
            let root = super::working_value::owning_reference(m, &arena, &decision.reference)?;
            let obligation = arena.object_from_fields(
                &[
                    ("kind", kind.value()),
                    ("target_ref", target_view.value()),
                    ("root_decision_ref", root.value()),
                    ("families", &value["families"]),
                ],
                m,
            )?;
            out.obligations
                .push(super::working_value::WorkingValue::Owned(obligation));
        }
        if m.representation_arena().is_some() {
            for text in [
                &target.reference.kind,
                &target.reference.id,
                &target.reference.digest,
            ] {
                m.scan_bytes(text.len())?;
                m.write_bytes(text.len())?;
            }
        }
        let target_ref = target.reference.clone();
        out.records.push(target);
        for (slot, window) in family_states {
            if let Some(arena) = m.representation_arena() {
                let constants=m.parse_owned_representation(&arena,br#"{"schema":"ledger-workflow-family-state/1","revision":"1","open":"open","awaiting":"awaiting_anchor"}"#,crate::validation_work::RepresentationKind::SourceDefinition)?;
                let target = super::working_value::owning_reference(m, &arena, &target_ref)?;
                let target_id = arena.string_value(&target_ref.id, m)?;
                let slot_value = arena.string_value(&slot, m)?;
                let tuple = arena.array_from_values(
                    &[
                        target_id.value(),
                        slot_value.value(),
                        &constants.value()["revision"],
                    ],
                    m,
                )?;
                let id = body::semantic_id(m, "wfamily_", "workflow-family-state", tuple.value())?;
                let key = super::cost_postings::operation_key(m, &arena, c)?;
                let at = arena.string_value(&c.accepted_at, m)?;
                let empty = arena.array_from_values(&[], m)?;
                let value = arena.object_from_fields(
                    &[
                        ("schema", &constants.value()["schema"]),
                        ("target_ref", target.value()),
                        ("family_slot", slot_value.value()),
                        ("revision", &constants.value()["revision"]),
                        (
                            "state",
                            if window.is_some() {
                                &constants.value()["open"]
                            } else {
                                &constants.value()["awaiting"]
                            },
                        ),
                        ("closure_evidence_refs", empty.value()),
                        ("operation_key", key.value()),
                        ("accepted_at", at.value()),
                    ],
                    m,
                )?;
                let value = if let Some(window) = window {
                    let work = super::working_value::owning_reference(m, &arena, &work.reference)?;
                    super::working_value::WorkingValue::Owned(value)
                        .with_fields(m, &[("window", &window), ("anchor_work_ref", work.value())])?
                        .into_owned()?
                } else {
                    value
                };
                let envelope =
                    body::record_borrowed(m, &c.key, "workflow-family-state", id, value.value())?;
                for reference in [&target_ref, &envelope.reference] {
                    for text in [&reference.kind, &reference.id, &reference.digest] {
                        m.scan_bytes(text.len())?;
                        m.write_bytes(text.len())?;
                    }
                }
                m.scan_bytes(slot.len())?;
                m.write_bytes(slot.len())?;
                m.write_bytes(1)?;
                out.heads.push((
                    target_ref.clone(),
                    slot.into_owned(),
                    "1".into(),
                    envelope.reference.clone(),
                ));
                out.records.push(envelope);
                continue;
            }
            let id = body::semantic_id(
                m,
                "wfamily_",
                "workflow-family-state",
                &json!([target_ref.id, slot, "1"]),
            )?;
            let mut state = json!({"schema":"ledger-workflow-family-state/1","target_ref":target_ref,"family_slot":slot,"revision":"1","state":if window.is_some(){"open"}else{"awaiting_anchor"},"closure_evidence_refs":[],"operation_key":c.key,"accepted_at":c.accepted_at});
            if let Some(window) = window {
                state["window"] = match window {
                    super::working_value::WorkingValue::Legacy(value) => value,
                    super::working_value::WorkingValue::Owned(_) => {
                        return Err(fail("owning window requires owning family producer"))
                    }
                };
                state["anchor_work_ref"] = json!(work.reference);
            }
            let envelope = body::record(m, &c.key, "workflow-family-state", id, state)?;
            out.heads.push((
                target_ref.clone(),
                slot.into_owned(),
                "1".into(),
                envelope.reference.clone(),
            ));
            out.records.push(envelope);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn family() -> Value {
        let input: Value = serde_json::from_slice(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/source-input.json")).unwrap();
        input["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["schema"] == "ledger-workflow-policy/1")
            .unwrap()["families"][0]
            .clone()
    }
    #[test]
    fn prospective_identity_matches_independent_original_outcome_domain() {
        let mut m = CoreMeter::new(65536).unwrap();
        let id = prospective_decision(
            &mut m,
            "wtarget_6f6a9b02079433b05887cc4cbf3fc1295f367abb8a07eaceb0825b5a0fd31a2e",
            "A-assessment",
        )
        .unwrap();
        assert_eq!(
            id,
            "wdecision_0d02b3805910828ce8f3802720b3536ac61ffff449c294f51bf42f10b314a3c3"
        );
    }
    #[test]
    fn original_code_maxima_are_rounded_from_named_net_not_copied_ceilings() {
        let mut m = CoreMeter::new(65536).unwrap();
        let family = family();
        let net = arithmetic::integer(&mut m, "999000000000000000", false).unwrap();
        let (premium, discount) = maxima(&mut m, &family, &net).unwrap();
        assert_eq!(premium.to_string(), "199800000000000000");
        assert_eq!(discount.to_string(), "99900000000000000");
        let (premium, discount) = maxima(&mut m, &family, &BigInt::from(0)).unwrap();
        assert_eq!((premium, discount), (0.into(), 0.into()));
        let mut lower = family;
        lower["maximum_premium"]["atoms"] = json!("1");
        assert!(maxima(&mut m, &lower, &net).is_err());
        // Summed target net is distinct from an individual posting atom cap.
        let wide = BigInt::parse_bytes(b"127999999999999999999999999999872", 10).unwrap();
        let arena = crate::canonical::audit_arena::AuditArena::new().unwrap();
        let mut session =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child = session.meter_mut().original_economic_meter().unwrap();
        let encoded = owned_amount(&mut child, &arena, &wide).unwrap();
        assert_eq!(encoded.value(), &amount(&wide));
        drop(encoded);
        session.meter_mut().finish_original_economic(child).unwrap();
    }
    #[test]
    fn original_window_is_frozen_once_with_original_graces_and_overflow_refusal() {
        let mut m = CoreMeter::new(65536).unwrap();
        let family = family();
        let r = ExactRef {
            kind: "workflow-work".into(),
            id: format!("wevent_{}", "1".repeat(64)),
            digest: format!("sha256:{}", "2".repeat(64)),
        };
        let window = frozen_window(&mut m, &family["window"], &r, "2026-10-13T01:00:01.000000Z")
            .unwrap()
            .unwrap();
        assert_eq!(window["occurs_before"], "2026-10-13T02:00:01.000000Z");
        assert_eq!(window["accepted_by"], "2026-10-13T02:02:01.000000Z");
        assert_eq!(window["correction_until"], "2026-10-14T02:02:01.000000Z");
        // Same arithmetic/body recipe with an actual owning return value.
        // This is window DATA parity, not a target/source/admission proof.
        let arena = crate::canonical::audit_arena::AuditArena::new().unwrap();
        let before = arena.usage().unwrap().live_bytes;
        let mut original =
            crate::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut child = original.meter_mut().original_economic_meter().unwrap();
        let owned = frozen_window(
            &mut child,
            &family["window"],
            &r,
            "2026-10-13T01:00:01.000000Z",
        )
        .unwrap()
        .unwrap();
        assert_eq!(&*owned, &*window);
        assert!(matches!(
            owned,
            super::super::working_value::WorkingValue::Owned(_)
        ));
        drop(owned);
        original
            .meter_mut()
            .finish_original_economic(child)
            .unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes, before);

        assert!(
            frozen_window(&mut m, &family["window"], &r, "9999-12-31T23:59:59.999999Z").is_err()
        );
        let mut anchored = family["window"].clone();
        anchored["anchor"] = json!("verified_predecessor_occurred_at");
        assert!(
            frozen_window(&mut m, &anchored, &r, "2026-10-13T01:00:01.000000Z")
                .unwrap()
                .is_none()
        );
    }
}
