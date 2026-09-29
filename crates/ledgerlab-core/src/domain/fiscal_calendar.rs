//! Pure organization fiscal calendar parsing and deterministic UTC period membership.
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday as CWeekday};
use serde::{Deserialize, Serialize};

use super::calendar::{resolve_timezone_local, CalendarError, IANA_TZDB_VERSION};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiscalKind {
    GregorianMonths,
    GregorianQuarters,
    GregorianYears,
    WeekPattern,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiscalWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}
impl FiscalWeekday {
    fn chrono(self) -> CWeekday {
        match self {
            Self::Monday => CWeekday::Mon,
            Self::Tuesday => CWeekday::Tue,
            Self::Wednesday => CWeekday::Wed,
            Self::Thursday => CWeekday::Thu,
            Self::Friday => CWeekday::Fri,
            Self::Saturday => CWeekday::Sat,
            Self::Sunday => CWeekday::Sun,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekAlignment {
    Nearest,
    Last,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WeekPattern {
    #[serde(rename = "4-4-5")]
    FourFourFive,
    #[serde(rename = "4-5-4")]
    FourFiveFour,
    #[serde(rename = "5-4-4")]
    FiveFourFour,
}
impl WeekPattern {
    fn lengths(self) -> [u8; 12] {
        match self {
            Self::FourFourFive => [4, 4, 5, 4, 4, 5, 4, 4, 5, 4, 4, 5],
            Self::FourFiveFour => [4, 5, 4, 4, 5, 4, 4, 5, 4, 4, 5, 4],
            Self::FiveFourFour => [5, 4, 4, 5, 4, 4, 5, 4, 4, 5, 4, 4],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FiscalCalendar {
    #[serde(rename = "gregorian_months")]
    GregorianMonths {
        fiscal_year_start_month: u8,
        fiscal_year_start_day: u8,
        #[serde(default)]
        week_start: Option<FiscalWeekday>,
    },
    #[serde(rename = "gregorian_quarters")]
    GregorianQuarters {
        fiscal_year_start_month: u8,
        fiscal_year_start_day: u8,
        #[serde(default)]
        week_start: Option<FiscalWeekday>,
    },
    #[serde(rename = "gregorian_years")]
    GregorianYears {
        fiscal_year_start_month: u8,
        fiscal_year_start_day: u8,
    },
    #[serde(rename = "week_pattern")]
    WeekPattern {
        fiscal_year_end_month: u8,
        fiscal_year_end_day: u8,
        pattern: WeekPattern,
        week_end_day: FiscalWeekday,
        week_end_alignment: WeekAlignment,
        extra_week_period: u8,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiscalCalendarConfig {
    pub timezone: String,
    pub timezone_rules_version: String,
    pub calendar: FiscalCalendar,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiscalPeriod {
    pub fiscal_year: i32,
    pub period: u8,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub weeks: Option<u8>,
}

impl FiscalCalendarConfig {
    pub fn validate(&self) -> Result<(), CalendarError> {
        if self.timezone_rules_version != IANA_TZDB_VERSION {
            return Err(CalendarError::UnsupportedTimezoneRules);
        }
        let _: chrono_tz::Tz = self
            .timezone
            .parse()
            .map_err(|_| CalendarError::UnknownTimezone)?;
        match &self.calendar {
            FiscalCalendar::GregorianMonths {
                fiscal_year_start_month,
                fiscal_year_start_day,
                ..
            }
            | FiscalCalendar::GregorianQuarters {
                fiscal_year_start_month,
                fiscal_year_start_day,
                ..
            }
            | FiscalCalendar::GregorianYears {
                fiscal_year_start_month,
                fiscal_year_start_day,
            } => {
                date(2000, *fiscal_year_start_month, *fiscal_year_start_day)?;
            }
            FiscalCalendar::WeekPattern {
                fiscal_year_end_month,
                fiscal_year_end_day,
                extra_week_period,
                ..
            } => {
                date(2000, *fiscal_year_end_month, *fiscal_year_end_day)?;
                if !(1..=12).contains(extra_week_period) {
                    return Err(CalendarError::OutOfRange);
                }
            }
        }
        Ok(())
    }
    /// Resolve the fiscal period containing the UTC instant (half-open boundaries).
    pub fn period_for(&self, instant: DateTime<Utc>) -> Result<FiscalPeriod, CalendarError> {
        self.validate()?;
        let tz: chrono_tz::Tz = self
            .timezone
            .parse()
            .map_err(|_| CalendarError::UnknownTimezone)?;
        let local = instant.with_timezone(&tz).date_naive();
        let (fy_start, fy_end_exclusive, kind) = self.fiscal_year_bounds(local)?;
        match kind {
            0 => {
                let mut s = fy_start;
                for n in 0..12 {
                    let e = month_step(s, 1, fy_start.day())?;
                    if local >= s && local < e {
                        return self.result(fy_start.year(), n + 1, s, e, None);
                    }
                    s = e;
                }
                Err(CalendarError::OutOfRange)
            }
            1 => {
                let mut s = fy_start;
                for n in 0..4 {
                    let e = month_step(s, 3, fy_start.day())?;
                    if local >= s && local < e {
                        return self.result(fy_start.year(), n + 1, s, e, None);
                    }
                    s = e;
                }
                Err(CalendarError::OutOfRange)
            }
            2 => self.result(fy_start.year(), 1, fy_start, fy_end_exclusive, None),
            _ => {
                let (pattern, extra) = match &self.calendar {
                    FiscalCalendar::WeekPattern {
                        pattern,
                        extra_week_period,
                        ..
                    } => (*pattern, *extra_week_period),
                    _ => unreachable!(),
                };
                let weeks = ((fy_end_exclusive - fy_start).num_days() / 7) as u8;
                let mut lengths = pattern.lengths();
                if weeks == 53 {
                    lengths[(extra - 1) as usize] += 1
                } else if weeks != 52 {
                    return Err(CalendarError::OutOfRange);
                }
                let mut s = fy_start;
                for (i, w) in lengths.iter().enumerate() {
                    let e = s + Duration::weeks(*w as i64);
                    if local >= s && local < e {
                        return self.result(fy_start.year(), (i + 1) as u8, s, e, Some(*w));
                    }
                    s = e;
                }
                Err(CalendarError::OutOfRange)
            }
        }
    }
    fn fiscal_year_bounds(
        &self,
        d: NaiveDate,
    ) -> Result<(NaiveDate, NaiveDate, u8), CalendarError> {
        match &self.calendar {
            FiscalCalendar::GregorianMonths {
                fiscal_year_start_month,
                fiscal_year_start_day,
                ..
            }
            | FiscalCalendar::GregorianQuarters {
                fiscal_year_start_month,
                fiscal_year_start_day,
                ..
            }
            | FiscalCalendar::GregorianYears {
                fiscal_year_start_month,
                fiscal_year_start_day,
            } => {
                let mut s =
                    date_clamped(d.year(), *fiscal_year_start_month, *fiscal_year_start_day)?;
                if d < s {
                    s = date_clamped(
                        d.year() - 1,
                        *fiscal_year_start_month,
                        *fiscal_year_start_day,
                    )?;
                }
                let e = date_clamped(
                    s.year() + 1,
                    *fiscal_year_start_month,
                    *fiscal_year_start_day,
                )?;
                let k = match &self.calendar {
                    FiscalCalendar::GregorianMonths { .. } => 0,
                    FiscalCalendar::GregorianQuarters { .. } => 1,
                    _ => 2,
                };
                Ok((s, e, k))
            }
            FiscalCalendar::WeekPattern {
                fiscal_year_end_month,
                fiscal_year_end_day,
                week_end_day,
                week_end_alignment,
                ..
            } => {
                let anchor = date(d.year(), *fiscal_year_end_month, *fiscal_year_end_day)?;
                let end = aligned_end(anchor, *week_end_day, *week_end_alignment)?;
                let (s, e) = if d > end {
                    let next_anchor =
                        date(d.year() + 1, *fiscal_year_end_month, *fiscal_year_end_day)?;
                    (
                        end + Duration::days(1),
                        aligned_end(next_anchor, *week_end_day, *week_end_alignment)?
                            + Duration::days(1),
                    )
                } else {
                    let prior = date(d.year() - 1, *fiscal_year_end_month, *fiscal_year_end_day)?;
                    let prior_end = aligned_end(prior, *week_end_day, *week_end_alignment)?;
                    (prior_end + Duration::days(1), end + Duration::days(1))
                };
                Ok((s, e, 3))
            }
        }
    }
    fn result(
        &self,
        year: i32,
        period: u8,
        start: NaiveDate,
        end: NaiveDate,
        weeks: Option<u8>,
    ) -> Result<FiscalPeriod, CalendarError> {
        let tz: chrono_tz::Tz = self
            .timezone
            .parse()
            .map_err(|_| CalendarError::UnknownTimezone)?;
        let s = resolve_timezone_local(
            &tz,
            start
                .and_hms_opt(0, 0, 0)
                .ok_or(CalendarError::OutOfRange)?,
        )?;
        let e = resolve_timezone_local(
            &tz,
            end.and_hms_opt(0, 0, 0).ok_or(CalendarError::OutOfRange)?,
        )?;
        Ok(FiscalPeriod {
            fiscal_year: year,
            period,
            start: s,
            end: e,
            weeks,
        })
    }
}
fn date(y: i32, m: u8, d: u8) -> Result<NaiveDate, CalendarError> {
    NaiveDate::from_ymd_opt(y, m as u32, d as u32).ok_or(CalendarError::OutOfRange)
}
fn date_clamped(y: i32, m: u8, d: u8) -> Result<NaiveDate, CalendarError> {
    if let Some(v) = NaiveDate::from_ymd_opt(y, m as u32, d as u32) {
        return Ok(v);
    }
    for day in (1..d).rev() {
        if let Some(v) = NaiveDate::from_ymd_opt(y, m as u32, day as u32) {
            return Ok(v);
        }
    }
    Err(CalendarError::OutOfRange)
}
fn month_step(d: NaiveDate, n: u32, anchor_day: u32) -> Result<NaiveDate, CalendarError> {
    let t = d.year() as i64 * 12 + d.month0() as i64 + n as i64;
    let y = i32::try_from(t.div_euclid(12)).map_err(|_| CalendarError::OutOfRange)?;
    let m = (t.rem_euclid(12) + 1) as u32;
    date_clamped(y, m as u8, anchor_day.min(31) as u8)
}
fn aligned_end(
    anchor: NaiveDate,
    day: FiscalWeekday,
    align: WeekAlignment,
) -> Result<NaiveDate, CalendarError> {
    let delta = (day.chrono().num_days_from_monday() as i64
        - anchor.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    let d = match align {
        WeekAlignment::Nearest => {
            if delta <= 3 {
                anchor + Duration::days(delta)
            } else {
                anchor - Duration::days(7 - delta)
            }
        }
        WeekAlignment::Last => anchor + Duration::days(delta),
    };
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn instant(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn cfg(calendar: FiscalCalendar) -> FiscalCalendarConfig {
        FiscalCalendarConfig {
            timezone: "America/New_York".into(),
            timezone_rules_version: IANA_TZDB_VERSION.into(),
            calendar,
        }
    }
    #[test]
    fn serde_uses_frozen_fiscal_calendar_spellings() {
        let months = FiscalCalendar::GregorianMonths {
            fiscal_year_start_month: 1,
            fiscal_year_start_day: 1,
            week_start: Some(FiscalWeekday::Monday),
        };
        let months_json = serde_json::to_value(&months).unwrap();
        assert_eq!(months_json["week_start"], "monday");
        assert!(months_json.get("week_start_day").is_none());
        assert_eq!(
            serde_json::from_value::<FiscalCalendar>(months_json).unwrap(),
            months
        );

        let pattern = FiscalCalendar::WeekPattern {
            fiscal_year_end_month: 1,
            fiscal_year_end_day: 31,
            pattern: WeekPattern::FourFourFive,
            week_end_day: FiscalWeekday::Saturday,
            week_end_alignment: WeekAlignment::Nearest,
            extra_week_period: 12,
        };
        let pattern_json = serde_json::to_value(&pattern).unwrap();
        assert_eq!(pattern_json["pattern"], "4-4-5");
        assert_eq!(
            serde_json::from_value::<FiscalCalendar>(pattern_json).unwrap(),
            pattern
        );
        for (wire, value) in [
            ("4-4-5", WeekPattern::FourFourFive),
            ("4-5-4", WeekPattern::FourFiveFour),
            ("5-4-4", WeekPattern::FiveFourFour),
        ] {
            assert_eq!(
                serde_json::from_str::<WeekPattern>(&format!("\"{wire}\"")).unwrap(),
                value
            );
        }
    }
    #[test]
    fn frozen_445_53_week_vector() {
        let c = cfg(FiscalCalendar::WeekPattern {
            fiscal_year_end_month: 1,
            fiscal_year_end_day: 31,
            pattern: WeekPattern::FourFourFive,
            week_end_day: FiscalWeekday::Saturday,
            week_end_alignment: WeekAlignment::Nearest,
            extra_week_period: 12,
        });
        let p = c.period_for(instant("2028-01-30T12:00:00Z")).unwrap();
        assert_eq!((p.fiscal_year, p.period, p.weeks), (2028, 1, Some(4)));
        let end = instant("2029-02-04T05:00:00Z");
        let q = c.period_for(end).unwrap();
        assert_eq!((q.fiscal_year, q.period, q.weeks), (2029, 1, Some(4)));
        let last = c.period_for(instant("2029-01-31T12:00:00Z")).unwrap();
        assert_eq!(
            (last.fiscal_year, last.period, last.weeks),
            (2028, 12, Some(6))
        );
        assert_eq!(last.end, end);
    }
    #[test]
    fn gregorian_month_period_is_half_open() {
        let c = cfg(FiscalCalendar::GregorianMonths {
            fiscal_year_start_month: 4,
            fiscal_year_start_day: 1,
            week_start: None,
        });
        let p = c.period_for(instant("2026-04-01T04:00:00Z")).unwrap();
        assert_eq!((p.fiscal_year, p.period), (2026, 1));
        assert_eq!(p.start, instant("2026-04-01T04:00:00Z"));
        assert_eq!(p.end, instant("2026-05-01T04:00:00Z"));
    }

    #[test]
    fn fiscal_midnight_gap_and_overlap_follow_pinned_boundary_rules() {
        let mut gap = cfg(FiscalCalendar::GregorianYears {
            fiscal_year_start_month: 11,
            fiscal_year_start_day: 4,
        });
        gap.timezone = "America/Sao_Paulo".into();
        let gap_period = gap.period_for(instant("2018-11-04T12:00:00Z")).unwrap();
        assert_eq!(gap_period.start, instant("2018-11-04T03:00:00Z"));

        let mut overlap = cfg(FiscalCalendar::GregorianYears {
            fiscal_year_start_month: 11,
            fiscal_year_start_day: 1,
        });
        overlap.timezone = "America/Havana".into();
        let overlap_period = overlap.period_for(instant("2020-11-01T12:00:00Z")).unwrap();
        assert_eq!(overlap_period.start, instant("2020-11-01T04:00:00Z"));
    }
    #[test]
    fn rejects_wrong_timezone_rules_pin() {
        let mut c = cfg(FiscalCalendar::GregorianYears {
            fiscal_year_start_month: 1,
            fiscal_year_start_day: 1,
        });
        c.timezone_rules_version = "IANA-1900a".into();
        assert_eq!(c.validate(), Err(CalendarError::UnsupportedTimezoneRules));
    }
}
