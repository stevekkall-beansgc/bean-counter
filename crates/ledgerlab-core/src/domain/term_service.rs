//! Pure customer-term activation and transition planning. The manager supplies
//! validated retained state and persists the returned plan atomically.

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
    TransitionAtBoundary,
    PendingTermTransition,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TermChangeMode {
    Initial,
    NextBoundary,
    Immediate,
}

/// A strictly parsed customer-term command. For `initial`, `term_template`
/// carries the caller's effective instant. For a transition, that field is a
/// placeholder only; `plan_term_transition` replaces it with the instant
/// selected under the writer lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TermChangeRequest {
    pub customer: String,
    pub change_id: String,
    pub expected_revision: Revision,
    pub mode: TermChangeMode,
    pub requested_effective_at: Option<DateTime<Utc>>,
    pub term_template: CalendarTerm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TermTransitionPlan {
    pub request: TermChangeRequest,
    pub term_version: u64,
    pub effective_at: DateTime<Utc>,
    pub term: CalendarTerm,
    pub period_zero: UtcPeriod,
    /// For an immediate transition, the predecessor's still-open logical
    /// period with its resolved end clipped to the accepted instant. If the
    /// transition lands exactly on that period's start, planning refuses with
    /// `TransitionAtBoundary` instead of producing a zero-length period.
    pub predecessor_resolution: Option<UtcPeriod>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeriodResolveRequest {
    pub customer: String,
    pub term_version: u64,
    pub period_index: u64,
}

pub type PeriodCloseRequest = PeriodResolveRequest;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePeriodResolve {
    schema: String,
    customer: String,
    period_id: WirePeriodId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePeriodId {
    term_version: String,
    period_index: String,
}

/// Parse the exact `ledger-billing-term/1` initial form. Lexical JSON parsing
/// rejects duplicate keys before serde converts the strict field structure.
pub fn parse_initial_request(bytes: &[u8]) -> Result<InitialTermRequest, TermPlanError> {
    let value = canonical::parse(bytes).map_err(|_| TermPlanError::InvalidRequest)?;
    if value["term"]["anchor"]
        .get("time")
        .is_some_and(serde_json::Value::is_null)
        || value["term"]
            .get("week_start")
            .is_some_and(serde_json::Value::is_null)
    {
        return Err(TermPlanError::InvalidRequest);
    }
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

/// Parse all supported customer-term request modes. Transition timestamps are
/// intentionally absent from the wire request: the manager selects the
/// effective instant after taking the serialized writer lock.
pub fn parse_term_change_request(bytes: &[u8]) -> Result<TermChangeRequest, TermPlanError> {
    let value = canonical::parse(bytes).map_err(|_| TermPlanError::InvalidRequest)?;
    let effective = value
        .get("effective")
        .and_then(serde_json::Value::as_object)
        .ok_or(TermPlanError::InvalidRequest)?;
    let mode = match effective.get("mode").and_then(serde_json::Value::as_str) {
        Some("initial") => TermChangeMode::Initial,
        Some("next_boundary") => TermChangeMode::NextBoundary,
        Some("immediate") => TermChangeMode::Immediate,
        _ => return Err(TermPlanError::UnsupportedMode),
    };

    if mode == TermChangeMode::Initial {
        let request = parse_initial_request(bytes)?;
        let effective_at = request.term.effective_at;
        return Ok(TermChangeRequest {
            customer: request.customer,
            change_id: request.change_id,
            expected_revision: request.expected_revision,
            mode,
            requested_effective_at: Some(effective_at),
            term_template: request.term,
        });
    }

    // Transition modes are clocked by the accepted command instant or the
    // current term's next boundary. Accepting a caller-supplied time here
    // would allow a delayed command to backdate a term change.
    if effective.len() != 1 || effective.contains_key("at") {
        return Err(TermPlanError::InvalidRequest);
    }

    // Reuse the complete strict term/request validator with a private clock
    // placeholder. The placeholder never reaches a plan or persistent record;
    // the writer-locked transition planner substitutes the resolved instant.
    let mut normalized = value;
    normalized["effective"] = serde_json::json!({
        "mode": "initial",
        "at": "1970-01-01T00:00:00Z"
    });
    let normalized = canonical::CanonicalBytes::from_value(&normalized)
        .map_err(|_| TermPlanError::InvalidRequest)?;
    let template = parse_initial_request(normalized.as_slice())?;
    Ok(TermChangeRequest {
        customer: template.customer,
        change_id: template.change_id,
        expected_revision: template.expected_revision,
        mode,
        requested_effective_at: None,
        term_template: template.term,
    })
}

pub fn parse_period_resolve_request(bytes: &[u8]) -> Result<PeriodResolveRequest, TermPlanError> {
    parse_period_request(bytes, "ledger-billing-period-resolve/1")
}

pub fn parse_period_close_request(bytes: &[u8]) -> Result<PeriodCloseRequest, TermPlanError> {
    parse_period_request(bytes, "ledger-billing-period-close/1")
}

fn parse_period_request(
    bytes: &[u8],
    expected_schema: &str,
) -> Result<PeriodResolveRequest, TermPlanError> {
    let value = canonical::parse(bytes).map_err(|_| TermPlanError::InvalidRequest)?;
    let wire: WirePeriodResolve =
        serde_json::from_value(value).map_err(|_| TermPlanError::InvalidRequest)?;
    if wire.schema != expected_schema {
        return Err(TermPlanError::UnsupportedSchema);
    }
    check_id(&wire.customer)?;
    let term_version = Revision::parse(&wire.period_id.term_version)
        .map_err(|_| TermPlanError::InvalidRequest)?
        .value();
    let period_index = Revision::parse(&wire.period_id.period_index)
        .map_err(|_| TermPlanError::InvalidRequest)?
        .value();
    if term_version == 0 {
        return Err(TermPlanError::InvalidVersion);
    }
    Ok(PeriodResolveRequest {
        customer: wire.customer,
        term_version,
        period_index,
    })
}

/// Resolve a versioned term transition using the current term and the
/// post-lock accepted instant. The caller persists the returned term and any
/// predecessor resolution atomically with its command record.
pub fn plan_term_transition(
    request: TermChangeRequest,
    current_term: &CalendarTerm,
    pending_effective_at: Option<DateTime<Utc>>,
    accepted_at: DateTime<Utc>,
    term_version: u64,
) -> Result<TermTransitionPlan, TermPlanError> {
    if term_version == 0 || term_version > i64::MAX as u64 {
        return Err(TermPlanError::InvalidVersion);
    }
    if request.mode == TermChangeMode::Initial {
        return Err(TermPlanError::UnsupportedMode);
    }
    if accepted_at < current_term.effective_at {
        return Err(TermPlanError::HistoryBeforeEffective);
    }
    let effective_at = match request.mode {
        TermChangeMode::Immediate => accepted_at,
        TermChangeMode::NextBoundary => current_term.period_for(accepted_at)?.end,
        TermChangeMode::Initial => return Err(TermPlanError::UnsupportedMode),
    };
    // Term versions are immutable and tied to the revision chain in command
    // order. Inserting before an already-pending successor would branch that
    // chain even when the requested instant is earlier than the pending one.
    if pending_effective_at.is_some() {
        return Err(TermPlanError::PendingTermTransition);
    }

    let mut term = request.term_template.clone();
    term.effective_at = effective_at;
    term.validate()?;
    let period_zero = term.period(0)?;

    let predecessor_resolution = if request.mode == TermChangeMode::Immediate {
        let mut predecessor = current_term.period_for(accepted_at)?;
        if predecessor.start >= accepted_at {
            // A zero-length predecessor cannot be persisted, and an M3 write
            // serialized just before this transition may carry the same
            // microsecond. Refuse until a strictly later accepted instant can
            // clip the retained predecessor without changing that assignment.
            return Err(TermPlanError::TransitionAtBoundary);
        }
        predecessor.end = accepted_at;
        Some(predecessor)
    } else {
        None
    };

    Ok(TermTransitionPlan {
        request,
        term_version,
        effective_at,
        term,
        period_zero,
        predecessor_resolution,
    })
}

/// Convert the facade timestamp into the calendar engine's UTC instant without
/// making infrastructure crates depend directly on the calendar library.
pub fn transition_instant(timestamp: &Timestamp) -> Result<DateTime<Utc>, TermPlanError> {
    utc_micros(timestamp.micros())
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

    fn transition(mode: &str) -> Vec<u8> {
        let mut value = canonical::parse(&request()).unwrap();
        value["change_id"] = serde_json::json!(format!("{mode}-change"));
        value["expected_revision"] = serde_json::json!("1");
        value["effective"] = serde_json::json!({"mode": mode});
        canonical::CanonicalBytes::from_value(&value)
            .unwrap()
            .into_vec()
    }

    fn accepted_at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
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
    fn immediate_transition_uses_locked_acceptance_time_and_clips_open_period() {
        let req = parse_term_change_request(&transition("immediate")).unwrap();
        assert_eq!(req.mode, TermChangeMode::Immediate);
        assert_eq!(req.requested_effective_at, None);
        let current = parse_initial_request(&request()).unwrap().term;
        let accepted = accepted_at("2026-01-15T12:00:00Z");
        let plan = plan_term_transition(req, &current, None, accepted, 2).unwrap();
        assert_eq!(plan.effective_at, accepted);
        assert_eq!(plan.term.effective_at, accepted);
        assert_eq!(plan.period_zero.start, accepted);
        let predecessor = plan.predecessor_resolution.unwrap();
        assert_eq!(predecessor.index, 0);
        assert_eq!(predecessor.start, accepted_at("2026-01-10T09:00:00Z"));
        assert_eq!(predecessor.end, accepted);
    }

    #[test]
    fn regular_transition_uses_next_boundary_of_current_term() {
        let req = parse_term_change_request(&transition("next_boundary")).unwrap();
        let current = parse_initial_request(&request()).unwrap().term;
        let accepted = accepted_at("2026-01-15T12:00:00Z");
        let plan = plan_term_transition(req, &current, None, accepted, 2).unwrap();
        assert_eq!(plan.effective_at, accepted_at("2026-02-01T00:00:00Z"));
        assert_eq!(plan.period_zero.start, plan.effective_at);
        assert!(plan.predecessor_resolution.is_none());
    }

    #[test]
    fn immediate_transition_at_existing_boundary_fails_closed() {
        let req = parse_term_change_request(&transition("immediate")).unwrap();
        let current = parse_initial_request(&request()).unwrap().term;
        let accepted = accepted_at("2026-02-01T00:00:00Z");
        assert_eq!(
            plan_term_transition(req, &current, None, accepted, 2).unwrap_err(),
            TermPlanError::TransitionAtBoundary
        );
    }

    #[test]
    fn any_term_change_refuses_while_a_successor_is_pending() {
        let current = parse_initial_request(&request()).unwrap().term;
        let accepted = accepted_at("2026-01-15T12:00:00Z");
        let pending = accepted_at("2026-03-01T00:00:00Z");
        for mode in ["immediate", "next_boundary"] {
            let req = parse_term_change_request(&transition(mode)).unwrap();
            assert_eq!(
                plan_term_transition(req, &current, Some(pending), accepted, 2).unwrap_err(),
                TermPlanError::PendingTermTransition,
                "mode {mode}"
            );
        }
    }

    #[test]
    fn transition_modes_reject_caller_supplied_effective_time() {
        let raw = String::from_utf8(transition("immediate")).unwrap();
        let changed = raw.replace(
            "\"effective\":{\"mode\":\"immediate\"}",
            "\"effective\":{\"mode\":\"immediate\",\"at\":\"2026-01-15T12:00:00Z\"}",
        );
        assert_eq!(
            parse_term_change_request(changed.as_bytes()).unwrap_err(),
            TermPlanError::InvalidRequest
        );
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

    #[test]
    fn optional_term_fields_refuse_explicit_null() {
        let raw = String::from_utf8(request()).unwrap();
        let null_time = raw.replace(
            "\"date\":\"2026-01-01\"",
            "\"date\":\"2026-01-01\",\"time\":null",
        );
        assert_eq!(
            parse_initial_request(null_time.as_bytes()).unwrap_err(),
            TermPlanError::InvalidRequest
        );
        let null_week = raw.replace(
            "\"timezone\":\"UTC\"",
            "\"timezone\":\"UTC\",\"week_start\":null",
        );
        assert_eq!(
            parse_initial_request(null_week.as_bytes()).unwrap_err(),
            TermPlanError::InvalidRequest
        );
    }
}
