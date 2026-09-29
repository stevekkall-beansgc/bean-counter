//! Deterministic M5 billing calendar arithmetic.
//!
//! The module has no runtime dependencies beyond its pinned chrono tables. Callers
//! persist the returned UTC boundaries and the rules version alongside the term.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
};
use chrono_tz::{GapInfo, Tz};

/// The IANA tzdb release compiled into the pinned `chrono-tz` dependency.
pub const IANA_TZDB_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;
pub const BOUNDARY_RULES_VERSION: &str = "m5-calendar/1";
const MAX_BOUNDARY_SEARCH: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalendarUnit {
    Day,
    Week,
    Month,
    Year,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    Anchored,
    CalendarAligned,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonthEndRule {
    PreserveAnchorAndClamp,
    ExplicitEndOfMonth,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarTerm {
    pub interval: u32,
    pub unit: CalendarUnit,
    pub alignment: Alignment,
    pub anchor: NaiveDateTime,
    pub timezone: String,
    pub week_start: Option<Weekday>,
    pub month_end_rule: MonthEndRule,
    pub effective_at: DateTime<Utc>,
    pub boundary_rule_version: String,
    pub timezone_rules_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UtcPeriod {
    pub index: u64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CalendarError {
    InvalidInterval,
    UnsupportedBoundaryRules,
    UnsupportedTimezoneRules,
    UnknownTimezone,
    WeekStartRequired,
    InvalidMonthEndAnchor,
    OutOfRange,
    NegativePeriod,
}

impl std::fmt::Display for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CalendarError {}

pub(crate) fn resolve_timezone_local(
    tz: &Tz,
    local: NaiveDateTime,
) -> Result<DateTime<Utc>, CalendarError> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(a, b) => Ok(a.with_timezone(&Utc).min(b.with_timezone(&Utc))),
        LocalResult::None => GapInfo::new(&local, tz)
            .and_then(|gap| gap.end)
            .map(|dt| dt.with_timezone(&Utc))
            .ok_or(CalendarError::OutOfRange),
    }
}

impl CalendarTerm {
    pub fn validate(&self) -> Result<Tz, CalendarError> {
        if self.interval == 0 {
            return Err(CalendarError::InvalidInterval);
        }
        if self.boundary_rule_version != BOUNDARY_RULES_VERSION {
            return Err(CalendarError::UnsupportedBoundaryRules);
        }
        if self.timezone_rules_version != IANA_TZDB_VERSION {
            return Err(CalendarError::UnsupportedTimezoneRules);
        }
        let tz: Tz = self
            .timezone
            .parse()
            .map_err(|_| CalendarError::UnknownTimezone)?;
        if self.unit == CalendarUnit::Week && self.week_start.is_none() {
            return Err(CalendarError::WeekStartRequired);
        }
        if self.month_end_rule == MonthEndRule::ExplicitEndOfMonth {
            let last = last_day(self.anchor.year(), self.anchor.month())
                .ok_or(CalendarError::OutOfRange)?;
            if self.anchor.day() != last {
                return Err(CalendarError::InvalidMonthEndAnchor);
            }
        }
        Ok(tz)
    }

    /// The scheduled local boundary at cadence ordinal `ordinal` (zero based).
    pub fn scheduled_local(&self, ordinal: u64) -> Result<NaiveDateTime, CalendarError> {
        self.validate()?;
        let anchor = match self.alignment {
            Alignment::Anchored => self.anchor,
            Alignment::CalendarAligned => self.first_calendar_boundary()?,
        };
        let steps = ordinal
            .checked_mul(self.interval as u64)
            .ok_or(CalendarError::OutOfRange)?;
        match self.unit {
            CalendarUnit::Day => anchor
                .checked_add_signed(Duration::days(
                    i64::try_from(steps).map_err(|_| CalendarError::OutOfRange)?,
                ))
                .ok_or(CalendarError::OutOfRange),
            CalendarUnit::Week => anchor
                .checked_add_signed(Duration::weeks(
                    i64::try_from(steps).map_err(|_| CalendarError::OutOfRange)?,
                ))
                .ok_or(CalendarError::OutOfRange),
            CalendarUnit::Month => self.month_at(anchor, steps),
            CalendarUnit::Year => self.month_at(
                anchor,
                steps.checked_mul(12).ok_or(CalendarError::OutOfRange)?,
            ),
        }
    }

    fn month_at(&self, anchor: NaiveDateTime, months: u64) -> Result<NaiveDateTime, CalendarError> {
        let base = (anchor.year() as i64)
            .checked_mul(12)
            .and_then(|v| v.checked_add(anchor.month0() as i64))
            .ok_or(CalendarError::OutOfRange)?;
        let total = base
            .checked_add(i64::try_from(months).map_err(|_| CalendarError::OutOfRange)?)
            .ok_or(CalendarError::OutOfRange)?;
        let year = i32::try_from(total.div_euclid(12)).map_err(|_| CalendarError::OutOfRange)?;
        let month = (total.rem_euclid(12) + 1) as u32;
        let target_last = last_day(year, month).ok_or(CalendarError::OutOfRange)?;
        let day = if self.month_end_rule == MonthEndRule::ExplicitEndOfMonth {
            target_last
        } else {
            anchor.day().min(target_last)
        };
        NaiveDate::from_ymd_opt(year, month, day)
            .map(|d| d.and_time(anchor.time()))
            .ok_or(CalendarError::OutOfRange)
    }

    fn first_calendar_boundary(&self) -> Result<NaiveDateTime, CalendarError> {
        let d = self.anchor.date();
        let (date, time) = match self.unit {
            CalendarUnit::Day => (d, NaiveTime::MIN),
            CalendarUnit::Week => {
                let wanted = to_chrono(self.week_start.ok_or(CalendarError::WeekStartRequired)?);
                let delta = (wanted.num_days_from_monday() as i64
                    - d.weekday().num_days_from_monday() as i64)
                    .rem_euclid(7);
                (
                    d.checked_add_signed(Duration::days(delta))
                        .ok_or(CalendarError::OutOfRange)?,
                    NaiveTime::MIN,
                )
            }
            CalendarUnit::Month => (
                NaiveDate::from_ymd_opt(d.year(), d.month(), 1).ok_or(CalendarError::OutOfRange)?,
                NaiveTime::MIN,
            ),
            CalendarUnit::Year => (
                NaiveDate::from_ymd_opt(d.year(), 1, 1).ok_or(CalendarError::OutOfRange)?,
                NaiveTime::MIN,
            ),
        };
        let mut candidate = date.and_time(time);
        if candidate < self.anchor {
            candidate = match self.unit {
                CalendarUnit::Day => candidate.checked_add_signed(Duration::days(1)),
                CalendarUnit::Week => candidate.checked_add_signed(Duration::weeks(1)),
                CalendarUnit::Month => {
                    let next = self.month_at(candidate, 1).ok();
                    next
                }
                CalendarUnit::Year => self.month_at(candidate, 12).ok(),
            }
            .ok_or(CalendarError::OutOfRange)?;
        }
        Ok(candidate)
    }

    pub fn resolve_local(&self, local: NaiveDateTime) -> Result<DateTime<Utc>, CalendarError> {
        let tz = self.validate()?;
        resolve_timezone_local(&tz, local)
    }

    /// Resolve period zero. Its start is `effective_at`; its end is the first
    /// scheduled boundary strictly later than that instant. Later periods use
    /// the same immutable schedule sequence. If a historical timezone jump
    /// maps adjacent local labels to the same UTC instant, the duplicate label
    /// is skipped so every period has positive duration.
    pub fn period(&self, index: i64) -> Result<UtcPeriod, CalendarError> {
        if index < 0 {
            return Err(CalendarError::NegativePeriod);
        }
        let i = index as u64;
        if i > MAX_BOUNDARY_SEARCH {
            return Err(CalendarError::OutOfRange);
        }
        let first_end = self.first_boundary_after(self.effective_at, 0)?;
        if i == 0 {
            return Ok(UtcPeriod {
                index: 0,
                start: self.effective_at,
                end: first_end.1,
            });
        }
        let mut start = first_end.1;
        let mut next_ordinal = first_end
            .0
            .checked_add(1)
            .ok_or(CalendarError::OutOfRange)?;
        for period_index in 1..=i {
            let (ordinal, end) = self.first_boundary_after(start, next_ordinal)?;
            if period_index == i {
                return Ok(UtcPeriod {
                    index: i,
                    start,
                    end,
                });
            }
            start = end;
            next_ordinal = ordinal.checked_add(1).ok_or(CalendarError::OutOfRange)?;
        }
        Err(CalendarError::OutOfRange)
    }

    /// Find the unique half-open period containing an accepted UTC instant.
    pub fn period_for(&self, accepted_at: DateTime<Utc>) -> Result<UtcPeriod, CalendarError> {
        if accepted_at < self.effective_at {
            return Err(CalendarError::OutOfRange);
        }
        let first_end = self.first_boundary_after(self.effective_at, 0)?;
        if accepted_at < first_end.1 {
            return Ok(UtcPeriod {
                index: 0,
                start: self.effective_at,
                end: first_end.1,
            });
        }
        let mut index = 0_u64;
        let mut start = first_end.1;
        let mut next_ordinal = first_end
            .0
            .checked_add(1)
            .ok_or(CalendarError::OutOfRange)?;
        loop {
            if index >= MAX_BOUNDARY_SEARCH {
                return Err(CalendarError::OutOfRange);
            }
            let (ordinal, end) = self.first_boundary_after(start, next_ordinal)?;
            index += 1;
            if accepted_at < end {
                return Ok(UtcPeriod { index, start, end });
            }
            start = end;
            next_ordinal = ordinal.checked_add(1).ok_or(CalendarError::OutOfRange)?;
        }
    }

    fn first_boundary_after(
        &self,
        instant: DateTime<Utc>,
        mut ordinal: u64,
    ) -> Result<(u64, DateTime<Utc>), CalendarError> {
        for _ in 0..MAX_BOUNDARY_SEARCH {
            let boundary = self.resolve_local(self.scheduled_local(ordinal)?)?;
            if boundary > instant {
                return Ok((ordinal, boundary));
            }
            ordinal = ordinal.checked_add(1).ok_or(CalendarError::OutOfRange)?;
        }
        Err(CalendarError::OutOfRange)
    }
}

fn last_day(year: i32, month: u32) -> Option<u32> {
    let (ny, nm) = if month == 12 {
        (year.checked_add(1)?, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)?
        .pred_opt()
        .map(|d| d.day())
}
fn to_chrono(day: Weekday) -> chrono::Weekday {
    match day {
        Weekday::Monday => chrono::Weekday::Mon,
        Weekday::Tuesday => chrono::Weekday::Tue,
        Weekday::Wednesday => chrono::Weekday::Wed,
        Weekday::Thursday => chrono::Weekday::Thu,
        Weekday::Friday => chrono::Weekday::Fri,
        Weekday::Saturday => chrono::Weekday::Sat,
        Weekday::Sunday => chrono::Weekday::Sun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn term(
        unit: CalendarUnit,
        alignment: Alignment,
        anchor: &str,
        effective: &str,
        zone: &str,
        rule: MonthEndRule,
    ) -> CalendarTerm {
        CalendarTerm {
            interval: 1,
            unit,
            alignment,
            anchor: NaiveDateTime::parse_from_str(anchor, "%Y-%m-%d %H:%M:%S").unwrap(),
            timezone: zone.into(),
            week_start: Some(Weekday::Monday),
            month_end_rule: rule,
            effective_at: DateTime::parse_from_rfc3339(effective)
                .unwrap()
                .with_timezone(&Utc),
            boundary_rule_version: BOUNDARY_RULES_VERSION.into(),
            timezone_rules_version: IANA_TZDB_VERSION.into(),
        }
    }
    #[test]
    fn leap_day_yearly_anchor_returns_in_leap_year() {
        let t = term(
            CalendarUnit::Year,
            Alignment::Anchored,
            "2024-02-29 09:00:00",
            "2024-03-01T00:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        assert_eq!(
            t.scheduled_local(1).unwrap().date().to_string(),
            "2025-02-28"
        );
        assert_eq!(
            t.scheduled_local(4).unwrap().date().to_string(),
            "2028-02-29"
        );
    }
    #[test]
    fn month_end_modes_are_anchor_based() {
        let clamp = term(
            CalendarUnit::Month,
            Alignment::Anchored,
            "2025-01-30 10:00:00",
            "2025-01-30T00:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        assert_eq!(
            clamp.scheduled_local(1).unwrap().date().to_string(),
            "2025-02-28"
        );
        assert_eq!(
            clamp.scheduled_local(2).unwrap().date().to_string(),
            "2025-03-30"
        );
        let eom = term(
            CalendarUnit::Month,
            Alignment::Anchored,
            "2025-01-31 10:00:00",
            "2025-01-31T00:00:00Z",
            "UTC",
            MonthEndRule::ExplicitEndOfMonth,
        );
        assert_eq!(
            eom.scheduled_local(1).unwrap().date().to_string(),
            "2025-02-28"
        );
        assert_eq!(
            eom.scheduled_local(2).unwrap().date().to_string(),
            "2025-03-31"
        );
    }
    #[test]
    fn calendar_alignment_starts_on_or_after_anchor() {
        let t = term(
            CalendarUnit::Month,
            Alignment::CalendarAligned,
            "2025-01-17 14:00:00",
            "2025-01-17T00:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        assert_eq!(
            t.scheduled_local(0).unwrap().to_string(),
            "2025-02-01 00:00:00"
        );
    }
    #[test]
    fn weekly_calendar_alignment_uses_declared_start() {
        let mut t = term(
            CalendarUnit::Week,
            Alignment::CalendarAligned,
            "2025-01-08 12:00:00",
            "2025-01-08T00:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        t.week_start = Some(Weekday::Sunday);
        assert_eq!(
            t.scheduled_local(0).unwrap().to_string(),
            "2025-01-12 00:00:00"
        );
    }
    #[test]
    fn dst_gap_moves_to_first_valid_second_and_overlap_uses_earlier_utc() {
        let gap = term(
            CalendarUnit::Day,
            Alignment::Anchored,
            "2025-03-08 02:30:00",
            "2025-03-08T00:00:00Z",
            "America/New_York",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        assert_eq!(
            gap.resolve_local(
                NaiveDate::from_ymd_opt(2025, 3, 9)
                    .unwrap()
                    .and_hms_opt(2, 30, 0)
                    .unwrap()
            )
            .unwrap()
            .to_rfc3339(),
            "2025-03-09T07:00:00+00:00"
        );
        assert_eq!(
            gap.resolve_local(
                NaiveDate::from_ymd_opt(2025, 3, 9)
                    .unwrap()
                    .and_hms_micro_opt(2, 30, 0, 500_000)
                    .unwrap()
            )
            .unwrap()
            .to_rfc3339(),
            "2025-03-09T07:00:00+00:00"
        );
        let overlap = term(
            CalendarUnit::Day,
            Alignment::Anchored,
            "2025-11-01 01:30:00",
            "2025-11-01T00:00:00Z",
            "America/New_York",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        assert_eq!(
            overlap
                .resolve_local(
                    NaiveDate::from_ymd_opt(2025, 11, 2)
                        .unwrap()
                        .and_hms_opt(1, 30, 0)
                        .unwrap()
                )
                .unwrap()
                .to_rfc3339(),
            "2025-11-02T05:30:00+00:00"
        );
    }
    #[test]
    fn period_zero_starts_at_effective_time_and_end_is_exclusive() {
        let t = term(
            CalendarUnit::Month,
            Alignment::CalendarAligned,
            "2025-01-17 00:00:00",
            "2025-01-17T13:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        let p0 = t.period(0).unwrap();
        assert_eq!(p0.start.to_rfc3339(), "2025-01-17T13:00:00+00:00");
        assert_eq!(p0.end.to_rfc3339(), "2025-02-01T00:00:00+00:00");
        let exact_end = t.period_for(p0.end).unwrap();
        assert_eq!(exact_end.index, 1);
        assert_eq!(exact_end.start, p0.end);
        assert!(t.period(-1).is_err());
    }
    #[test]
    fn skipped_civil_date_does_not_create_zero_length_utc_period() {
        let t = term(
            CalendarUnit::Day,
            Alignment::Anchored,
            "2011-12-28 00:00:00",
            "2011-12-28T10:00:00Z",
            "Pacific/Apia",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        let p0 = t.period(0).unwrap();
        let p1 = t.period(1).unwrap();
        let p2 = t.period(2).unwrap();
        assert_eq!(p0.end, p1.start);
        assert_eq!(p1.end, p2.start);
        assert!(p0.start < p0.end);
        assert!(p1.start < p1.end);
        assert!(p2.start < p2.end);
        assert_eq!(t.period_for(p1.end).unwrap(), p2);
    }
    #[test]
    fn refuses_unpinned_zone_data_and_unknown_zones() {
        let mut t = term(
            CalendarUnit::Day,
            Alignment::Anchored,
            "2025-01-01 00:00:00",
            "2025-01-01T00:00:00Z",
            "UTC",
            MonthEndRule::PreserveAnchorAndClamp,
        );
        t.timezone_rules_version = "latest".into();
        assert_eq!(
            t.validate().unwrap_err(),
            CalendarError::UnsupportedTimezoneRules
        );
        t.timezone_rules_version = IANA_TZDB_VERSION.into();
        t.timezone = "Not/A_Zone".into();
        assert_eq!(t.validate().unwrap_err(), CalendarError::UnknownTimezone);
    }
}
