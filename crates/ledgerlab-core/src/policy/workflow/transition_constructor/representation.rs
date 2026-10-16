//! Private parsing data for exactly one invocation; never authority or a cache.
use super::{CoreMeter, data::ExactRef, records, shape};
use crate::{Result, domain::Timestamp};
pub(super) struct InvocationToken {
    _nonzero_sized: u8,
}
impl InvocationToken {
    pub(super) fn new() -> Self {
        Self { _nonzero_sized: 1 }
    }
}
fn owner(m: &mut CoreMeter, expected: &InvocationToken, actual: &InvocationToken) -> Result<()> {
    if !m.check("parsed.invocation_owner", || std::ptr::eq(expected, actual))? {
        return Err(shape::fail("different invocation representation"));
    }
    Ok(())
}
pub(super) struct ParsedRef<'inv> {
    exact: ExactRef,
    owner: &'inv InvocationToken,
}
impl<'inv> ParsedRef<'inv> {
    pub(super) fn parse(
        m: &mut CoreMeter,
        token: &'inv InvocationToken,
        value: &serde_json::Value,
    ) -> Result<Self> {
        let exact = shape::reference(m, value)?;
        Ok(Self {
            exact,
            owner: token,
        })
    }
    pub(super) fn exact(&self) -> &ExactRef {
        &self.exact
    }
    pub(super) fn require_owner(&self, m: &mut CoreMeter, token: &InvocationToken) -> Result<()> {
        owner(m, token, self.owner)
    }
}
pub(super) struct ParsedDraftTimes<'inv> {
    received: Timestamp,
    accepted: Timestamp,
    owner: &'inv InvocationToken,
}
impl<'inv> ParsedDraftTimes<'inv> {
    pub(super) fn parse(
        m: &mut CoreMeter,
        token: &'inv InvocationToken,
        received: &str,
        accepted: &str,
    ) -> Result<Self> {
        let accepted = m.scoped("Timestamp::parse_with_work.accepted_at", |m| {
            Timestamp::parse_with_work(accepted, m)
        })?;
        let received = m.scoped("Timestamp::parse_with_work.received_at", |m| {
            Timestamp::parse_with_work(received, m)
        })?;
        Ok(Self {
            received,
            accepted,
            owner: token,
        })
    }
    pub(super) fn get<'a>(
        &'a self,
        m: &mut CoreMeter,
        token: &InvocationToken,
    ) -> Result<(&'a Timestamp, &'a Timestamp)> {
        owner(m, token, self.owner)?;
        Ok((&self.received, &self.accepted))
    }
}
pub(super) struct ParsedGrantInterval<'inv> {
    starts: Timestamp,
    ends: Timestamp,
    owner: &'inv InvocationToken,
}
impl<'inv> ParsedGrantInterval<'inv> {
    pub(super) fn parse(
        m: &mut CoreMeter,
        token: &'inv InvocationToken,
        value: &serde_json::Value,
    ) -> Result<Self> {
        let start = shape::text(m, value, "starts_at")?;
        let starts = m.scoped("Timestamp::parse_with_work.starts_at", |m| {
            Timestamp::parse_with_work(start, m)
        })?;
        let end = shape::text(m, value, "ends_at")?;
        let ends = m.scoped("Timestamp::parse_with_work.ends_at", |m| {
            Timestamp::parse_with_work(end, m)
        })?;
        Ok(Self {
            starts,
            ends,
            owner: token,
        })
    }
    pub(super) fn get<'a>(
        &'a self,
        m: &mut CoreMeter,
        token: &InvocationToken,
    ) -> Result<(&'a Timestamp, &'a Timestamp)> {
        owner(m, token, self.owner)?;
        Ok((&self.starts, &self.ends))
    }
}
