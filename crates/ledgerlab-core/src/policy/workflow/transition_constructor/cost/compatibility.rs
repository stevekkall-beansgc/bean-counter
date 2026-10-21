//! Existing helper-level conservation tests; no completed supplier AP claim.
use super::super::{arithmetic, meter, shape, CoreMeter};
use crate::{Error, Result};
use num_bigint::BigInt;
pub(super) struct InvocationAmounts {
    pub authorized: BigInt,
    pub consumed: BigInt,
    pub held: BigInt,
    pub released: BigInt,
}
pub(super) struct HighWaterTransition {
    pub eligible: BigInt,
    pub high_water: BigInt,
    pub delta: BigInt,
    pub invocation: InvocationAmounts,
}
pub(super) fn nonnegative(m: &mut CoreMeter, n: &BigInt) -> Result<()> {
    let maximum = arithmetic::integer(m, "999999999999999999999999999999", false)?;
    if meter::cmp(m, n, &0.into())?.is_lt() || meter::cmp(m, n, &maximum)?.is_gt() {
        return Err(shape::fail("supplier obligation Amount bound"));
    }
    Ok(())
}
pub(super) fn conserved(m: &mut CoreMeter, current: &InvocationAmounts) -> Result<()> {
    for amount in [
        &current.authorized,
        &current.consumed,
        &current.held,
        &current.released,
    ] {
        nonnegative(m, amount)?;
    }
    let used = meter::add(m, &current.consumed, &current.held)?;
    let total = meter::add(m, &used, &current.released)?;
    if !meter::cmp(m, &total, &current.authorized)?.is_eq() {
        return Err(shape::fail(
            "invocation authorized = consumed + held + released",
        ));
    }
    Ok(())
}
/// For the SAME permanent CostClaim only. That exact claim/invocation/line
/// identity and sum of the other current consumption heads are independently
/// established by the enclosing record validator; no new line receives credit.
pub(super) fn high_water(
    m: &mut CoreMeter,
    current: InvocationAmounts,
    prior: &BigInt,
    eligible: &BigInt,
) -> Result<HighWaterTransition> {
    conserved(m, &current)?;
    nonnegative(m, prior)?;
    nonnegative(m, eligible)?;
    if meter::cmp(m, prior, &current.consumed)?.is_gt() {
        return Err(shape::fail("line high-water exceeds total consumption"));
    }
    let high = if meter::cmp(m, eligible, prior)?.is_gt() {
        meter::add(m, eligible, &0.into())?
    } else {
        meter::add(m, prior, &0.into())?
    };
    let delta = meter::sub(m, &high, prior)?;
    if meter::cmp(m, &delta, &current.held)?.is_gt() {
        return Err(Error::new(
            "INVOCATION_EXPOSURE_EXCEEDED",
            "original held correction coverage",
        ));
    }
    let consumed = meter::add(m, &current.consumed, &delta)?;
    let held = meter::sub(m, &current.held, &delta)?;
    let next = InvocationAmounts {
        authorized: current.authorized,
        consumed,
        held,
        released: current.released,
    };
    conserved(m, &next)?;
    Ok(HighWaterTransition {
        eligible: meter::add(m, eligible, &0.into())?,
        high_water: high,
        delta,
        invocation: next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_obligation_down_up_keeps_permanent_consumption_and_original_conservation() {
        let mut m = CoreMeter::new(65536).unwrap();
        let mut current = InvocationAmounts {
            authorized: 300.into(),
            consumed: 200.into(),
            held: 100.into(),
            released: 0.into(),
        };
        let mut prior = BigInt::from(200);
        for (amount, expected_h, expected_delta, expected_held) in
            [(160, 200, 0, 100), (250, 250, 50, 50), (300, 300, 50, 0)]
        {
            let next = high_water(&mut m, current, &prior, &amount.into()).unwrap();
            assert_eq!(next.high_water, expected_h.into());
            assert_eq!(next.delta, expected_delta.into());
            assert_eq!(next.invocation.held, expected_held.into());
            assert_eq!(next.invocation.released, 0.into());
            prior = next.high_water;
            current = next.invocation;
        }
        assert_eq!(
            high_water(&mut m, current, &prior, &301.into())
                .err()
                .unwrap()
                .code,
            "INVOCATION_EXPOSURE_EXCEEDED"
        );
        let bad = InvocationAmounts {
            authorized: 300.into(),
            consumed: 200.into(),
            held: 99.into(),
            released: 0.into(),
        };
        assert_eq!(
            high_water(&mut m, bad, &200.into(), &160.into())
                .err()
                .unwrap()
                .code,
            "INTEGRITY_FAILURE"
        );
        // Other permanent claims remain in total consumed. A decrease of this
        // line does not make their obligations disappear or increase held.
        let other = InvocationAmounts {
            authorized: 500.into(),
            consumed: 350.into(),
            held: 100.into(),
            released: 50.into(),
        };
        let next = high_water(&mut m, other, &200.into(), &160.into()).unwrap();
        assert_eq!(next.invocation.consumed, 350.into());
        assert_eq!(next.invocation.held, 100.into());
        assert_eq!(next.invocation.released, 50.into());
    }
}
