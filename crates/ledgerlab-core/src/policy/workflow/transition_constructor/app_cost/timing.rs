//! Exact original app clock. The immutable initial committed decision anchors
//! all standalone corrections, including when its actual amount was zero.
use super::super::super::selected_program::SelectedProgram;
use super::super::{arithmetic, data::DraftContextData, meter, shape, CoreMeter};
use super::plan::AppPlan;
use crate::{
    domain::Timestamp,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Error, Result,
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::Value;
fn timestamp(m: &mut CoreMeter, v: &Value, key: &str) -> Result<i64> {
    let text = shape::text(m, v, key)?;
    Ok(Timestamp::parse_with_work(text, m)?.micros())
}
fn add_duration(m: &mut CoreMeter, base: i64, source: &Value, key: &str) -> Result<i64> {
    let raw = shape::text(m, source, key)?;
    let duration = arithmetic::integer(m, raw, false)?;
    if meter::cmp(m, &duration, &0.into())?.is_lt() {
        return Err(shape::fail("negative original app duration"));
    }
    let sum = meter::add(m, &base.into(), &duration)?;
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Compare,
        &[&sum, &BigInt::from(i64::MIN)],
    )?;
    ValidationWork::arithmetic(
        m,
        ArithmeticOperation::Compare,
        &[&sum, &BigInt::from(i64::MAX)],
    )?;
    let micros = sum
        .to_i64()
        .ok_or_else(|| shape::fail("original app horizon overflow"))?;
    Timestamp::from_micros_with_work(micros, m)?;
    Ok(micros)
}
pub(super) struct AppClocks {
    pub finalize_by: i64,
    pub standalone_correction_until: Option<i64>,
    pub prospective_standalone_until: i64,
}
pub(super) fn standalone(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    plan: &AppPlan,
) -> Result<AppClocks> {
    let work = timestamp(m, &plan.original.work, "accepted_at")?;
    let finalize_by = add_duration(m, work, plan.original.declaration, "finalize_within_us")?;
    let prospective_standalone_until = add_duration(
        m,
        finalize_by,
        plan.original.declaration,
        "correction_within_us",
    )?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if meter::cmp(m, &now.into(), &work.into())?.is_lt() {
        return Err(shape::fail("app acceptance before original work"));
    }
    let standalone_correction_until = if let Some(current) = &plan.current {
        if !program.uses_app_cost_causal_recipe() {
            return Err(Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "standalone app anchor requires exact declared recipe",
            ));
        }
        let anchor = shape::text(m, plan.original.declaration, "correction_anchor")?;
        if !shape::same_text(m, anchor, "initial_cost_accepted_at")? {
            return Err(shape::fail("original app correction anchor"));
        }
        let initial = timestamp(m, &current.initial_decision, "accepted_at")?;
        if meter::cmp(m, &initial.into(), &work.into())?.is_lt()
            || meter::cmp(m, &initial.into(), &finalize_by.into())?.is_gt()
        {
            return Err(shape::fail(
                "initial app decision outside original finalize horizon",
            ));
        }
        let prior = timestamp(m, &current.decision, "accepted_at")?;
        if meter::cmp(m, &prior.into(), &initial.into())?.is_lt()
            || meter::cmp(m, &now.into(), &prior.into())?.is_lt()
        {
            return Err(shape::fail("app correction trusted clock regression"));
        }
        let until = add_duration(
            m,
            initial,
            plan.original.declaration,
            "correction_within_us",
        )?;
        if meter::cmp(m, &now.into(), &until.into())?.is_gt() {
            return Err(Error::new(
                "WINDOW_EXPIRED",
                "original non-sliding app correction horizon",
            ));
        }
        Some(until)
    } else {
        if meter::cmp(m, &now.into(), &finalize_by.into())?.is_gt() {
            return Err(Error::new(
                "WINDOW_EXPIRED",
                "original app finalization horizon",
            ));
        }
        None
    };
    Ok(AppClocks {
        finalize_by,
        standalone_correction_until,
        prospective_standalone_until,
    })
}

/// Causal closure keeps the standalone clock intact. Its only late exemption
/// is the exact originally accepted app clause; the parent separately proves
/// its own still-current original root authority/window and complete closure.
pub(super) fn causal(
    m: &mut CoreMeter,
    c: &DraftContextData,
    original: &super::original::OriginalApp,
    current: &super::plan::CurrentApp,
) -> Result<(Timestamp, bool)> {
    let work = timestamp(m, &original.work, "accepted_at")?;
    let final_by = add_duration(m, work, original.declaration, "finalize_within_us")?;
    let initial = timestamp(m, &current.initial_decision, "accepted_at")?;
    if meter::cmp(m, &initial.into(), &work.into())?.is_lt()
        || meter::cmp(m, &initial.into(), &final_by.into())?.is_gt()
    {
        return Err(shape::fail(
            "causal initial cost outside original finalization window",
        ));
    }
    let anchor = shape::text(m, original.declaration, "correction_anchor")?;
    if !shape::same_text(m, anchor, "initial_cost_accepted_at")? {
        return Err(shape::fail("causal original correction anchor"));
    }
    let until = add_duration(m, initial, original.declaration, "correction_within_us")?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    let prior = timestamp(m, &current.decision, "accepted_at")?;
    if meter::cmp(m, &prior.into(), &initial.into())?.is_lt()
        || meter::cmp(m, &now.into(), &prior.into())?.is_lt()
    {
        return Err(shape::fail("causal cost clock regression"));
    }
    let late = meter::cmp(m, &now.into(), &until.into())?.is_gt();
    if late {
        clause(m, original.declaration)?;
    }
    Ok((Timestamp::from_micros_with_work(until, m)?, late))
}
pub(super) fn clause(m: &mut CoreMeter, declaration: &Value) -> Result<()> {
    let clause = shape::field(m, declaration, "causal_reversal")?;
    shape::closed(
        m,
        clause,
        &["root_relation", "mode", "standalone_deadline_effect"],
        &[],
    )?;
    for (key, wanted) in [
        ("root_relation", "original_work_delayed_cost_dependency"),
        ("mode", "exact_inverse_and_permanent_close"),
        (
            "standalone_deadline_effect",
            "does_not_expire_preaccepted_causal_reversal",
        ),
    ] {
        let actual = shape::text(m, clause, key)?;
        if !shape::same_text(m, actual, wanted)? {
            return Err(shape::fail("original app causal clause"));
        }
    }
    Ok(())
}

/// Before the initial work creates a future app dependency, prove that every
/// latest permissible first-finalization plus original correction duration is
/// representable. An eventual revision1 T0 may be earlier, never later than
/// W+F. Separate parent causal rights are not clipped to this clock.
pub(in crate::policy::workflow::transition_constructor) fn prework(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    payload: &Value,
) -> Result<()> {
    let work = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    for declaration in shape::array(m, program.original_policy_value(), "app_delayed_costs")? {
        m.visit("app.promise_original_clock")?;
        if !shape::same(m, &declaration["activity"], &payload["activity"])? {
            continue;
        }
        let finalized = add_duration(m, work, declaration, "finalize_within_us")?;
        let _latest = add_duration(m, finalized, declaration, "correction_within_us")?;
        if program.uses_app_cost_causal_recipe() {
            let anchor = shape::text(m, declaration, "correction_anchor")?;
            if !shape::same_text(m, anchor, "initial_cost_accepted_at")? {
                return Err(shape::fail("promised app initial correction anchor"));
            }
        }
    }
    Ok(())
}
