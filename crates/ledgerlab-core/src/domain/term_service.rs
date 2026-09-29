//! Pure initial customer-term activation planning. The manager supplies the
//! already validated billable M3 history and persists the returned plan.

use super::{
    Alignment, CalendarError, CalendarTerm, CalendarUnit, MonthEndRule, Revision, Timestamp,
    UtcPeriod, Weekday,
};
use crate::canonical;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TermPlanError {
    InvalidRequest,
    UnsupportedSchema,
    UnsupportedMode,
    InvalidTimestamp,
    InvalidTerm(CalendarError),
    InvalidVersion,
    HistoryBeforeEffective,
    InvalidHistory,
}

impl From<CalendarError> for TermPlanError {
    fn from(error: CalendarError) -> Self {
        Self::InvalidTerm(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialTermRequest {
    pub customer: String,
    pub change_id: String,
    pub expected_revision: Revision,
    pub term: CalendarTerm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BillableHistoryRow {
    pub ordinal: u64,
    pub accepted_at_us: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeriodAssignment {
    pub ordinal: u64,
    pub term_version: u64,
    pub period_index: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialTermPlan {
    pub request: InitialTermRequest,
    pub term_version: u64,
    pub period_zero: UtcPeriod,
    pub assignments: Vec<PeriodAssignment>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    schema: String,
    customer: String,
    change_id: String,
    expected_revision: String,
    effective: WireEffective,
    term: WireTerm,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEffective {
    mode: String,
    at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAnchor {
    date: String,
    time: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTerm {
    interval: i64,
    unit: String,
    alignment: String,
    anchor: WireAnchor,
    timezone: String,
    week_start: Option<String>,
    month_end_rule: String,
    boundary_rule_version: String,
    timezone_rules_version: String,
    proration: String,
}

/// Parse the exact `ledger-billing-term/1` initial form. Lexical JSON parsing
/// rejects duplicate keys before serde converts the strict field structure.
pub fn parse_initial_request(bytes: &[u8]) -> Result<InitialTermRequest, TermPlanError> {
    let value = canonical::parse(bytes).map_err(|_| TermPlanError::InvalidRequest)?;
    let wire: WireRequest =
        serde_json::from_value(value).map_err(|_| TermPlanError::InvalidRequest)?;
    if wire.schema != "ledger-billing-term/1" {
        return Err(TermPlanError::UnsupportedSchema);
    }
    if wire.effective.mode != "initial" {
        return Err(TermPlanError::UnsupportedMode);
    }
    let at = wire.effective.at.ok_or(TermPlanError::InvalidRequest)?;
    check_id(&wire.customer)?;
    check_id(&wire.change_id)?;
    let expected_revision =
        Revision::parse(&wire.expected_revision).map_err(|_| TermPlanError::InvalidRequest)?;
    let accepted = Timestamp::parse(&at).map_err(|_| TermPlanError::InvalidTimestamp)?;
    let effective_at = utc_micros(accepted.micros())?;
    let interval = u32::try_from(wire.term.interval).map_err(|_| TermPlanError::InvalidRequest)?;
    if interval == 0 || interval > 2_147_483_647 {
        return Err(TermPlanError::InvalidRequest);
    }
    let unit = match wire.term.unit.as_str() {
        "day" => CalendarUnit::Day,
        "week" => CalendarUnit::Week,
        "month" => CalendarUnit::Month,
        "year" => CalendarUnit::Year,
        _ => return Err(TermPlanError::InvalidRequest),
    };
    let alignment = match wire.term.alignment.as_str() {
        "anchored" => Alignment::Anchored,
        "calendar_aligned" => Alignment::CalendarAligned,
        _ => return Err(TermPlanError::InvalidRequest),
    };
    let month_end_rule = match wire.term.month_end_rule.as_str() {
        "preserve_anchor_and_clamp" => MonthEndRule::PreserveAnchorAndClamp,
        "explicit_end_of_month" => MonthEndRule::ExplicitEndOfMonth,
        _ => return Err(TermPlanError::InvalidRequest),
    };
    let week_start = match wire.term.week_start.as_deref() {
        None => None,
        Some("monday") => Some(Weekday::Monday),
        Some("tuesday") => Some(Weekday::Tuesday),
        Some("wednesday") => Some(Weekday::Wednesday),
        Some("thursday") => Some(Weekday::Thursday),
        Some("friday") => Some(Weekday::Friday),
        Some("saturday") => Some(Weekday::Saturday),
        Some("sunday") => Some(Weekday::Sunday),
        _ => return Err(TermPlanError::InvalidRequest),
    };
    if wire.term.proration != "none"
        || wire.term.timezone.is_empty()
        || wire.term.timezone.len() > 128
        || wire.term.boundary_rule_version.is_empty()
        || wire.term.boundary_rule_version.len() > 64
        || wire.term.timezone_rules_version.is_empty()
        || wire.term.timezone_rules_version.len() > 64
    {
        return Err(TermPlanError::InvalidRequest);
    }
    let date = NaiveDate::parse_from_str(&wire.term.anchor.date, "%Y-%m-%d")
        .map_err(|_| TermPlanError::InvalidRequest)?;
    if date.format("%Y-%m-%d").to_string() != wire.term.anchor.date {
        return Err(TermPlanError::InvalidRequest);
    }
    if !(1..=9999).contains(&chrono::Datelike::year(&date)) {
        return Err(TermPlanError::InvalidRequest);
    }
    let time = match wire.term.anchor.time {
        Some(ref time) => parse_local_time(time)?,
        None => NaiveTime::MIN,
    };
    let term = CalendarTerm {
        interval,
        unit,
        alignment,
        anchor: NaiveDateTime::new(date, time),
        timezone: wire.term.timezone,
        week_start,
        month_end_rule,
        effective_at,
        boundary_rule_version: wire.term.boundary_rule_version,
        timezone_rules_version: wire.term.timezone_rules_version,
    };
    term.validate()?;
    Ok(InitialTermRequest {
        customer: wire.customer,
        change_id: wire.change_id,
        expected_revision,
        term,
    })
}

/// Derive period zero and map every supplied billable acceptance exactly once.
/// The manager must provide the complete customer history under its writer lock.
pub fn plan_initial_activation(
    request: InitialTermRequest,
    term_version: u64,
    history: &[BillableHistoryRow],
) -> Result<InitialTermPlan, TermPlanError> {
    if term_version == 0 || term_version > i64::MAX as u64 {
        return Err(TermPlanError::InvalidVersion);
    }
    let period_zero = request.term.period(0)?;
    let effective_us = request.term.effective_at.timestamp_micros();
    let mut seen = BTreeSet::new();
    let mut assignments = Vec::with_capacity(history.len());
    for row in history {
        if !seen.insert(row.ordinal) {
            return Err(TermPlanError::InvalidHistory);
        }
        if row.accepted_at_us < effective_us {
            return Err(TermPlanError::HistoryBeforeEffective);
        }
        let accepted_at =
            utc_micros(row.accepted_at_us).map_err(|_| TermPlanError::InvalidHistory)?;
        let period = request.term.period_for(accepted_at)?;
        if !(period.start <= accepted_at && accepted_at < period.end) {
            return Err(TermPlanError::InvalidHistory);
        }
        assignments.push(PeriodAssignment {
            ordinal: row.ordinal,
            term_version,
            period_index: period.index,
        });
    }
    Ok(InitialTermPlan {
        request,
        term_version,
        period_zero,
        assignments,
    })
}

fn check_id(value: &str) -> Result<(), TermPlanError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        Err(TermPlanError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn utc_micros(value: i64) -> Result<DateTime<Utc>, TermPlanError> {
    DateTime::from_timestamp_micros(value).ok_or(TermPlanError::InvalidTimestamp)
}

fn parse_local_time(value: &str) -> Result<NaiveTime, TermPlanError> {
    let bytes = value.as_bytes();
    if bytes.len() < 8 || bytes.len() > 15 || bytes[2] != b':' || bytes[5] != b':' {
        return Err(TermPlanError::InvalidRequest);
    }
    if !bytes[..2]
        .iter()
        .chain(&bytes[3..5])
        .chain(&bytes[6..8])
        .all(u8::is_ascii_digit)
    {
        return Err(TermPlanError::InvalidRequest);
    }
    if bytes.len() > 8
        && (bytes[8] != b'.' || bytes.len() == 9 || !bytes[9..].iter().all(u8::is_ascii_digit))
    {
        return Err(TermPlanError::InvalidRequest);
    }
    NaiveTime::parse_from_str(value, "%H:%M:%S%.f").map_err(|_| TermPlanError::InvalidRequest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn request() -> Vec<u8> {
        format!(r#"{{"schema":"ledger-billing-term/1","customer":"c","change_id":"first","expected_revision":"0","effective":{{"mode":"initial","at":"2026-01-10T09:00:00Z"}},"term":{{"interval":1,"unit":"month","alignment":"calendar_aligned","anchor":{{"date":"2026-01-01"}},"timezone":"UTC","month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"{}","timezone_rules_version":"{}","proration":"none"}}}}"#, super::super::BOUNDARY_RULES_VERSION, super::super::IANA_TZDB_VERSION).into_bytes()
    }

    #[test]
    fn maps_retained_history_across_exact_end() {
        let req = parse_initial_request(&request()).unwrap();
        let first = Timestamp::parse("2026-01-10T09:00:00Z").unwrap().micros();
        let boundary = Timestamp::parse("2026-02-01T00:00:00Z").unwrap().micros();
        let plan = plan_initial_activation(
            req,
            1,
            &[
                BillableHistoryRow {
                    ordinal: 4,
                    accepted_at_us: first,
                },
                BillableHistoryRow {
                    ordinal: 8,
                    accepted_at_us: boundary,
                },
            ],
        )
        .unwrap();
        assert_eq!(plan.period_zero.start.timestamp_micros(), first);
        assert_eq!(plan.period_zero.end.timestamp_micros(), boundary);
        assert_eq!(plan.assignments[0].period_index, 0);
        assert_eq!(plan.assignments[1].period_index, 1);
    }

    #[test]
    fn refuses_uncovered_or_duplicate_history() {
        let req = parse_initial_request(&request()).unwrap();
        let before = Timestamp::parse("2026-01-10T08:59:59Z").unwrap().micros();
        assert_eq!(
            plan_initial_activation(
                req.clone(),
                1,
                &[BillableHistoryRow {
                    ordinal: 1,
                    accepted_at_us: before
                }]
            )
            .unwrap_err(),
            TermPlanError::HistoryBeforeEffective
        );
        let at = Timestamp::parse("2026-01-10T09:00:00Z").unwrap().micros();
        assert_eq!(
            plan_initial_activation(
                req,
                1,
                &[
                    BillableHistoryRow {
                        ordinal: 1,
                        accepted_at_us: at
                    },
                    BillableHistoryRow {
                        ordinal: 1,
                        accepted_at_us: at
                    }
                ]
            )
            .unwrap_err(),
            TermPlanError::InvalidHistory
        );
    }

    #[test]
    fn strict_initial_wire_and_pinned_rules() {
        let raw = String::from_utf8(request()).unwrap();
        assert_eq!(
            parse_initial_request(
                raw.replace(
                    "\"customer\":\"c\"",
                    "\"customer\":\"c\",\"customer\":\"c\""
                )
                .as_bytes()
            )
            .unwrap_err(),
            TermPlanError::InvalidRequest
        );
        assert_eq!(
            parse_initial_request(
                raw.replace("\"mode\":\"initial\"", "\"mode\":\"immediate\"")
                    .as_bytes()
            )
            .unwrap_err(),
            TermPlanError::UnsupportedMode
        );
        assert_eq!(
            parse_initial_request(
                raw.replace(super::super::IANA_TZDB_VERSION, "latest")
                    .as_bytes()
            )
            .unwrap_err(),
            TermPlanError::InvalidTerm(CalendarError::UnsupportedTimezoneRules)
        );
        assert_eq!(
            parse_initial_request(
                raw.replace(
                    "\"proration\":\"none\"",
                    "\"proration\":\"none\",\"extra\":0"
                )
                .as_bytes()
            )
            .unwrap_err(),
            TermPlanError::InvalidRequest
        );
    }

    #[test]
    fn accepts_six_digit_local_anchor_time() {
        let raw = String::from_utf8(request()).unwrap();
        let request = raw.replace(
            "\"date\":\"2026-01-01\"",
            "\"date\":\"2026-01-01\",\"time\":\"09:30:00.123456\"",
        );
        let parsed = parse_initial_request(request.as_bytes()).unwrap();
        assert_eq!(parsed.term.anchor.time().nanosecond(), 123_456_000);
    }
}
