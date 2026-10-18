//! Private parsing data for exactly one invocation; never authority.
use super::{CoreMeter, data::ExactRef, records, shape};
use crate::{Result, domain::Timestamp, canonical::audit_arena::ExternalBufferLease, validation_work as w};
use std::cell::RefCell;
pub(super) struct InvocationToken {
    _nonzero_sized: u8,
    times: RefCell<Option<TimeMemo>>,
}
impl InvocationToken {
    pub(super) fn new() -> Self {
        Self { _nonzero_sized: 1, times: RefCell::new(None) }
    }
}
// A bounded optimization, not an accepted-input bound: longer strings or a full
// memo execute the ordinary parser. Only successful exact raw strings are saved.
const TIME_SLOTS:usize=32;
const TIME_BYTES:usize=64;
struct TimeEntry {raw:[u8;TIME_BYTES],len:usize,micros:i64}
struct TimeMemo {entries:Vec<TimeEntry>,_ownership:ExternalBufferLease}
#[derive(Clone,Copy)]
pub(super) struct ParsedMicros(i64);
impl ParsedMicros {pub(super) fn micros(&self)->i64{self.0}}
impl InvocationToken {
    #[cfg(test)]
    pub(super) fn memo_len(&self)->usize{self.times.borrow().as_ref().map_or(0,|v|v.entries.len())}
    pub(super) fn timestamp(&self,m:&mut CoreMeter,raw:&str,site:&'static str)->Result<ParsedMicros>{
        let Some(arena)=m.representation_arena() else {
            return Ok(ParsedMicros(m.scoped(site,|m|Timestamp::parse_with_work(raw,m))?.micros()));
        };
        m.visit("parsed.time_memo_presence")?;
        let mut state=self.times.borrow_mut();
        if state.is_none(){
            // Includes inline control and one producer's fixed stack temporary.
            const PAYLOAD:usize=std::mem::size_of::<InvocationToken>()+std::mem::size_of::<TimeMemo>()
                +(TIME_SLOTS+1)*std::mem::size_of::<TimeEntry>()
                +std::mem::size_of::<Timestamp>()+27;
            let lease=arena.reserve_external_payload(PAYLOAD,m)?;
            let mut entries=Vec::new();
            entries.try_reserve_exact(TIME_SLOTS).map_err(|_|shape::fail("timestamp memo allocation"))?;
            *state=Some(TimeMemo{entries,_ownership:lease});
        }
        let memo=state.as_mut().ok_or_else(||shape::fail("timestamp memo owner"))?;
        arena.require_external_owner(&memo._ownership,m)?;
        for entry in &memo.entries {
            m.visit("parsed.time_memo_entry")?;
            m.scan_bytes(raw.len())?;m.scan_bytes(entry.len)?;
            if m.check("parsed.time_memo_exact_bytes",||raw.as_bytes()==&entry.raw[..entry.len])?{
                return Ok(ParsedMicros(entry.micros));
            }
        }
        let parsed=m.scoped(site,|m|Timestamp::parse_with_work(raw,m))?;
        let micros=parsed.micros();
        if w::compare(m,raw.len(),TIME_BYTES)?.is_le()
            && w::compare(m,memo.entries.len(),TIME_SLOTS)?.is_lt(){
            m.scan_bytes(raw.len())?;
            // Initialize the fixed raw/control entry, copy the raw bytes into it,
            // then move the whole entry into the already reserved vector.
            m.write_bytes(2*std::mem::size_of::<TimeEntry>())?;
            m.write_bytes(raw.len())?;
            let mut entry=TimeEntry{raw:[0;TIME_BYTES],len:raw.len(),micros};
            entry.raw[..raw.len()].copy_from_slice(raw.as_bytes());
            memo.entries.push(entry);
        }
        Ok(ParsedMicros(micros))
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
    starts: ParsedMicros,
    ends: ParsedMicros,
    owner: &'inv InvocationToken,
}
impl<'inv> ParsedGrantInterval<'inv> {
    pub(super) fn parse(
        m: &mut CoreMeter,
        token: &'inv InvocationToken,
        value: &serde_json::Value,
    ) -> Result<Self> {
        let start = shape::text(m, value, "starts_at")?;
        let starts = token.timestamp(m,start,"Timestamp::parse_with_work.starts_at")?;
        let end = shape::text(m, value, "ends_at")?;
        let ends = token.timestamp(m,end,"Timestamp::parse_with_work.ends_at")?;
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
    ) -> Result<(&'a ParsedMicros, &'a ParsedMicros)> {
        owner(m, token, self.owner)?;
        Ok((&self.starts, &self.ends))
    }
}
