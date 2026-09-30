use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::canonical::CanonicalBytes;
use ledgerlab_core::domain::{
    Alignment, CalendarTerm, CalendarUnit, DateTime, MonthEndRule, NaiveDate, NaiveDateTime,
    NaiveTime, Timestamp, Utc,
};
use serde::Deserialize;

const SET: &str = "ledger-billing-recurrence/1";
const VERSION: &str = "ledger-billing-recurrence-version/1";
const CANCEL: &str = "ledger-billing-recurrence-cancel/1";
const CANCELLATION: &str = "ledger-billing-recurrence-cancellation/1";
const ACCEPT: &str = "ledger-billing-occurrence-acceptance/1";
const ACCEPT_RECORD: &str = "ledger-billing-occurrence-acceptance-record/1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRule {
    interval: i64,
    unit: String,
    anchor: WireAnchor,
    timezone: String,
    effective_from: String,
    boundary_rule_version: String,
    timezone_rules_version: String,
    proration: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;
    fn encode(value: &Value) -> Vec<u8> {
        CanonicalBytes::from_value(value).unwrap().into_vec()
    }
    #[test]
    fn frozen_occurrence_identity_matches_oracle() {
        assert_eq!(
            occurrence_id(
                "customer-usage-1",
                "urn:example:usage-work",
                "agreement-usage-1",
                1,
                1,
                "2026-09-01T00:00:00"
            )
            .unwrap(),
            "occ_45e6ff2cfc19479d408c56a785c460f6e51907c8db847ae6cd44023561ac3da6"
        );
    }
    #[tokio::test]
    async fn manual_recurrence_due_accept_cancel_and_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({"schema":"ledger-billing-term/1","customer":"customer-1","change_id":"term-r1","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored","anchor":{"date":"2026-09-01","time":"00:00:00"},
                "timezone":"UTC","month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}});
        ledger.term_set(&encode(&term)).await.unwrap();
        let setup = json!({"schema":SET,"customer":"customer-1","source":"urn:example:work","change_id":"recurrence-1",
            "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
            "rule":{"interval":1,"unit":"month","anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "effective_from":"2026-09-01T00:00:00.000000Z","boundary_rule_version":"billing-boundary/1","timezone_rules_version":"IANA-2025b","proration":"none"},
            "renewal":{"mode":"manual"}});
        let setup_bytes = encode(&setup);
        let result = ledger.recurrence_set(&setup_bytes).await.unwrap();
        assert_eq!(result["recurrence_version"], "1");
        assert_eq!(ledger.recurrence_set(&setup_bytes).await.unwrap(), result);
        let query = json!({"schema":"ledger-billing-occurrence-query/1","customer":"customer-1","source":"urn:example:work",
            "recurrence_version":"1","due_through":"2026-09-30T00:00:00.000000Z","limit":10});
        let due = ledger.occurrences(&encode(&query)).await.unwrap();
        assert_eq!(due["occurrences"].as_array().unwrap().len(), 1);
        let occurrence_id = due["occurrences"][0]["occurrence_id"].as_str().unwrap();
        let event = json!({"schema":"ledger-event/1","id":occurrence_id,"operation_id":"operation-occurrence-1",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z","status":"succeeded"});
        let accept = json!({"schema":ACCEPT,"customer":"customer-1","source":"urn:example:work","occurrence_id":occurrence_id,"event":event});
        let accept_bytes = encode(&accept);
        let accepted = ledger.occurrence_accept(&accept_bytes).await.unwrap();
        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["receipt"]["kind"], "base-acceptance");
        let identity = encode(&json!({
            "schema":"ledger-billing-m5-command-identity/1",
            "domain":"application",
            "family":ACCEPT,
            "customer":"customer-1",
            "source":"urn:example:work",
            "key_kind":"occurrence_id",
            "key":occurrence_id,
        }));
        let mut tx = ledger
            .store
            .begin(Instant::now() + std::time::Duration::from_secs(5))
            .await
            .unwrap();
        let saved = tx.m5_lookup(&identity).await.unwrap().unwrap();
        let child: Value = serde_json::from_slice(&saved.records[0].3).unwrap();
        assert_eq!(child["agreement_version"], "1");
        assert_eq!(child["recurrence_version"], "1");
        tx.rollback().await.unwrap();
        assert!(
            ledger.occurrences(&encode(&query)).await.unwrap()["occurrences"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let cancel = json!({"schema":CANCEL,"customer":"customer-1","source":"urn:example:work","change_id":"cancel-1",
            "expected_revision":"1","recurrence_version":"1"});
        let cancel_bytes = encode(&cancel);
        let cancelled = ledger.recurrence_cancel(&cancel_bytes).await.unwrap();
        assert_eq!(cancelled["status"], "recurrence_cancelled");
        assert_eq!(
            ledger.recurrence_cancel(&cancel_bytes).await.unwrap(),
            cancelled
        );
        assert_eq!(
            ledger.occurrence_accept(&accept_bytes).await.unwrap(),
            accepted
        );
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        assert_eq!(
            reopened.occurrence_accept(&accept_bytes).await.unwrap(),
            accepted
        );
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        sqlx::query("DROP TRIGGER billing_m5_occurrence_acceptances_no_update")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("UPDATE billing_m5_occurrence_acceptances SET accepted_m3_receipt_id='ba2_tampered' WHERE customer='customer-1'").execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();
        assert!(BillingLedger::open(&path).await.is_err());
    }

    #[tokio::test]
    async fn automatic_opt_in_requires_exact_preaccepted_m2_successor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let rule = json!({"interval":1,"unit":"month","anchor":{"date":"2026-09-01","time":"00:00:00"},
            "timezone":"UTC","effective_from":"2026-09-01T00:00:00.000000Z","boundary_rule_version":"billing-boundary/1",
            "timezone_rules_version":"IANA-2025b","proration":"none"});
        let mut next_rule = rule.clone();
        next_rule["anchor"]["date"] = json!("2026-10-01");
        next_rule["effective_from"] = json!("2026-10-01T00:00:00.000000Z");
        let request = json!({"schema":SET,"customer":"customer-1","source":"urn:example:work","change_id":"auto-1",
            "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1","rule":rule,
            "renewal":{"mode":"automatic_opt_in","m2_change_id":"m2-renewal-2","next_agreement_version":"2","next_rule":next_rule}});
        let raw = encode(&request);
        assert!(ledger.recurrence_set(&raw).await.is_err());
        let setup: Value =
            serde_json::from_slice(include_bytes!("../../../../examples/billing/setup.json"))
                .unwrap();
        let amendment = json!({"schema":"ledger-billing-amendment/2","customer":"customer-1","source":"urn:example:work",
            "change_id":"m2-renewal-2","expected_revision":"1","effective_at":"2026-10-01T00:00:00.000000Z","setup":setup});
        ledger
            .agreement_control("customer-1", "urn:example:work", &encode(&amendment))
            .await
            .unwrap();
        let first = ledger.recurrence_set(&raw).await.unwrap();
        assert_eq!(first["recurrence_version"], "1");
        assert_eq!(first["receipt"]["record_ids"].as_array().unwrap().len(), 2);
        let second = ledger.recurrence_set(&raw).await.unwrap();
        assert_eq!(second, first);
        let future_id = occurrence_id(
            "customer-1",
            "urn:example:work",
            "agreement-1",
            2,
            2,
            "2026-10-01T00:00:00",
        )
        .unwrap();
        let event = json!({"schema":"ledger-event/1","id":future_id,"operation_id":"future-op",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-10-01T00:00:00.000000Z","status":"succeeded"});
        let accept = json!({"schema":ACCEPT,"customer":"customer-1","source":"urn:example:work","occurrence_id":future_id,"event":event});
        assert!(ledger.occurrence_accept(&encode(&accept)).await.is_err());
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        let state = {
            let mut tx = reopened
                .store
                .begin(Instant::now() + std::time::Duration::from_secs(5))
                .await
                .unwrap();
            let state = tx
                .m5_recurrence_state("customer-1", "urn:example:work")
                .await
                .unwrap();
            tx.rollback().await.unwrap();
            state
        };
        assert_eq!(
            (state.revision, state.next_version, state.versions.len()),
            (1, 3, 2)
        );
        reopened.close().await;
    }

    #[tokio::test]
    async fn weekly_query_pages_without_mutation_and_binds_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let setup = json!({"schema":SET,"customer":"customer-1","source":"urn:example:work","change_id":"weekly-1",
            "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
            "rule":{"interval":1,"unit":"week","anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "effective_from":"2026-09-01T00:00:00.000000Z","boundary_rule_version":"billing-boundary/1","timezone_rules_version":"IANA-2025b","proration":"none"},
            "renewal":{"mode":"manual"}});
        ledger.recurrence_set(&encode(&setup)).await.unwrap();
        let mut query = json!({"schema":"ledger-billing-occurrence-query/1","customer":"customer-1","source":"urn:example:work",
            "recurrence_version":"1","due_through":"2026-09-29T00:00:00.000000Z","limit":2});
        let first = ledger.occurrences(&encode(&query)).await.unwrap();
        assert_eq!(first["occurrences"].as_array().unwrap().len(), 2);
        assert_eq!(first["has_more"], true);
        query["cursor"] = first["next_cursor"].clone();
        let second = ledger.occurrences(&encode(&query)).await.unwrap();
        assert_eq!(second["occurrences"].as_array().unwrap().len(), 2);
        assert_ne!(
            first["occurrences"][0]["occurrence_id"],
            second["occurrences"][0]["occurrence_id"]
        );
        query["due_through"] = json!("2026-09-28T00:00:00.000000Z");
        assert!(ledger.occurrences(&encode(&query)).await.is_err());
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let (commands,records):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_commands),(SELECT count(*) FROM billing_m5_records)").fetch_one(&mut conn).await.unwrap();
        assert_eq!((commands, records), (1, 1));
        conn.close().await.unwrap();
        let reopened = BillingLedger::open(&path).await.unwrap();
        let cancel = json!({"schema":CANCEL,"customer":"customer-1","source":"urn:example:work","change_id":"weekly-cancel",
            "expected_revision":"1","recurrence_version":"1"});
        reopened.recurrence_cancel(&encode(&cancel)).await.unwrap();
        let occurrence_id = first["occurrences"][0]["occurrence_id"].as_str().unwrap();
        let event = json!({"schema":"ledger-event/1","id":occurrence_id,"operation_id":"cancelled-occurrence-op",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z","status":"succeeded"});
        let accept = json!({"schema":ACCEPT,"customer":"customer-1","source":"urn:example:work","occurrence_id":occurrence_id,"event":event});
        assert!(matches!(reopened.occurrence_accept(&encode(&accept)).await,
            Err(super::super::LocalError::Service(super::super::ServiceError::Rejection(code))) if code=="BILLING_M5_CANCELLED"));
        reopened.close().await;
    }

    #[tokio::test]
    async fn preexisting_m4_delivery_and_semantic_identities_refuse_without_m5_link() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let setup = json!({"schema":SET,"customer":"customer-1","source":"urn:example:work","change_id":"weekly-conflict",
            "expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
            "rule":{"interval":1,"unit":"week","anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "effective_from":"2026-09-01T00:00:00.000000Z","boundary_rule_version":"billing-boundary/1","timezone_rules_version":"IANA-2025b","proration":"none"},
            "renewal":{"mode":"manual"}});
        ledger.recurrence_set(&encode(&setup)).await.unwrap();
        let first_id = occurrence_id(
            "customer-1",
            "urn:example:work",
            "agreement-1",
            1,
            1,
            "2026-09-01T00:00:00",
        )
        .unwrap();
        let second_id = occurrence_id(
            "customer-1",
            "urn:example:work",
            "agreement-1",
            1,
            1,
            "2026-09-08T00:00:00",
        )
        .unwrap();
        let first_event = json!({"schema":"ledger-event/1","id":first_id,"operation_id":"direct-first",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z","status":"succeeded"});
        ledger
            .accept("customer-1", "urn:example:work", &encode(&first_event))
            .await
            .unwrap();
        let first_accept = json!({"schema":ACCEPT,"customer":"customer-1","source":"urn:example:work","occurrence_id":first_id,"event":first_event});
        assert!(
            matches!(ledger.occurrence_accept(&encode(&first_accept)).await,
            Err(super::super::LocalError::Service(super::super::ServiceError::Rejection(code))) if code=="BILLING_M5_OCCURRENCE_CONFLICT")
        );
        let other = json!({"schema":"ledger-event/1","id":"direct-other","operation_id":"shared-semantic",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z","status":"succeeded"});
        ledger
            .accept("customer-1", "urn:example:work", &encode(&other))
            .await
            .unwrap();
        let second_event = json!({"schema":"ledger-event/1","id":second_id,"operation_id":"shared-semantic",
            "type":"content.generated","customer":"customer-1","occurred_at":"2026-09-01T00:00:00.000000Z","status":"succeeded"});
        let second_accept = json!({"schema":ACCEPT,"customer":"customer-1","source":"urn:example:work","occurrence_id":second_id,"event":second_event});
        assert!(
            matches!(ledger.occurrence_accept(&encode(&second_accept)).await,
            Err(super::super::LocalError::Service(super::super::ServiceError::Rejection(code))) if code=="BILLING_M5_OCCURRENCE_CONFLICT")
        );
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM billing_m5_occurrence_acceptances")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn usage_occurrence_uses_m4_quantity_evaluator_and_exact_receipt_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/usage/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({"schema":"ledger-billing-term/1","customer":"customer-usage-1","change_id":"usage-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored","anchor":{"date":"2026-09-01","time":"00:00:00"},
                "timezone":"UTC","month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}});
        ledger.term_set(&encode(&term)).await.unwrap();
        let setup = json!({"schema":SET,"customer":"customer-usage-1","source":"urn:example:usage-work","change_id":"usage-recurrence",
            "expected_revision":"0","agreement_id":"agreement-usage-1","agreement_version":"1",
            "rule":{"interval":1,"unit":"month","anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "effective_from":"2026-09-01T00:00:00.000000Z","boundary_rule_version":"billing-boundary/1","timezone_rules_version":"IANA-2025b","proration":"none"},
            "renewal":{"mode":"manual"}});
        ledger.recurrence_set(&encode(&setup)).await.unwrap();
        let id = occurrence_id(
            "customer-usage-1",
            "urn:example:usage-work",
            "agreement-usage-1",
            1,
            1,
            "2026-09-01T00:00:00",
        )
        .unwrap();
        let mut event: Value = serde_json::from_slice(include_bytes!(
            "../../../../examples/billing/usage/event.json"
        ))
        .unwrap();
        event["id"] = json!(id);
        event["operation_id"] = json!("usage-occurrence-op");
        let mut invalid = event.clone();
        invalid["unit"] = json!("wrong-unit");
        let invalid_request = json!({"schema":ACCEPT,"customer":"customer-usage-1","source":"urn:example:usage-work","occurrence_id":id,"event":invalid});
        assert!(ledger
            .occurrence_accept(&encode(&invalid_request))
            .await
            .is_err());
        let request = json!({"schema":ACCEPT,"customer":"customer-usage-1","source":"urn:example:usage-work","occurrence_id":id,"event":event});
        let accepted = ledger.occurrence_accept(&encode(&request)).await.unwrap();
        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["receipt"]["kind"], "base-acceptance");
        assert!(accepted["receipt"]["body"]["original_receipt_utf8"]
            .as_str()
            .unwrap()
            .contains("target"));
        assert!(accepted["receipt"].get("command_sequence").is_none());
        ledger.close().await;
        BillingLedger::open(&path).await.unwrap().close().await;
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAnchor {
    date: String,
    time: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRenewal {
    mode: String,
    m2_change_id: Option<String>,
    next_agreement_version: Option<String>,
    next_rule: Option<WireRule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSet {
    schema: String,
    customer: String,
    source: String,
    change_id: String,
    expected_revision: String,
    agreement_id: String,
    agreement_version: String,
    rule: WireRule,
    renewal: WireRenewal,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCancel {
    schema: String,
    customer: String,
    source: String,
    change_id: String,
    expected_revision: String,
    recurrence_version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireQuery {
    schema: String,
    customer: String,
    source: String,
    recurrence_version: String,
    due_through: String,
    limit: u16,
    cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAccept {
    schema: String,
    customer: String,
    source: String,
    occurrence_id: String,
    event: Value,
}

fn reject(code: &'static str) -> ServiceError {
    service::reject(code)
}
fn request_error() -> ServiceError {
    reject("BILLING_M5_REQUEST")
}
fn no_null(value: &Value) -> Result<(), ServiceError> {
    match value {
        Value::Null => Err(request_error()),
        Value::Array(items) => items.iter().try_for_each(no_null),
        Value::Object(fields) => fields.values().try_for_each(no_null),
        _ => Ok(()),
    }
}
fn bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| request_error())
}
fn id(s: &str, max: usize) -> Result<(), ServiceError> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(request_error())
    } else {
        Ok(())
    }
}
fn decimal(s: &str, zero: bool) -> Result<i64, ServiceError> {
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(request_error());
    }
    let n = s.parse::<i64>().map_err(|_| request_error())?;
    if !zero && n == 0 {
        return Err(request_error());
    }
    Ok(n)
}
fn parse_time(raw: &str) -> Result<DateTime<Utc>, ServiceError> {
    let at = Timestamp::parse(raw).map_err(|_| request_error())?;
    DateTime::from_timestamp_micros(at.micros()).ok_or_else(request_error)
}
fn recurrence_calendar(rule: &WireRule) -> Result<CalendarTerm, ServiceError> {
    let interval = u32::try_from(rule.interval).map_err(|_| request_error())?;
    if interval == 0 {
        return Err(request_error());
    }
    let unit = match rule.unit.as_str() {
        "day" => CalendarUnit::Day,
        "week" => CalendarUnit::Week,
        "month" => CalendarUnit::Month,
        "year" => CalendarUnit::Year,
        _ => return Err(request_error()),
    };
    let date =
        NaiveDate::parse_from_str(&rule.anchor.date, "%Y-%m-%d").map_err(|_| request_error())?;
    if date.format("%Y-%m-%d").to_string() != rule.anchor.date {
        return Err(request_error());
    }
    let time = if let Some(s) = &rule.anchor.time {
        let b = s.as_bytes();
        if !(b.len() == 8 || (10..=15).contains(&b.len()))
            || b.get(2) != Some(&b':')
            || b.get(5) != Some(&b':')
            || !b[..2]
                .iter()
                .chain(&b[3..5])
                .chain(&b[6..8])
                .all(u8::is_ascii_digit)
            || (b.len() > 8 && (b[8] != b'.' || !b[9..].iter().all(u8::is_ascii_digit)))
        {
            return Err(request_error());
        }
        NaiveTime::parse_from_str(s, "%H:%M:%S%.f").map_err(|_| request_error())?
    } else {
        NaiveTime::MIN
    };
    if rule
        .anchor
        .time
        .as_ref()
        .is_some_and(|s| s.len() > 15 || s.ends_with('.') || s.contains('Z'))
        || rule.proration != "none"
    {
        return Err(request_error());
    }
    let calendar = CalendarTerm {
        interval,
        unit,
        alignment: Alignment::Anchored,
        anchor: NaiveDateTime::new(date, time),
        timezone: rule.timezone.clone(),
        week_start: if unit == CalendarUnit::Week {
            Some(ledgerlab_core::domain::Weekday::Monday)
        } else {
            None
        },
        month_end_rule: MonthEndRule::PreserveAnchorAndClamp,
        effective_at: parse_time(&rule.effective_from)?,
        boundary_rule_version: rule.boundary_rule_version.clone(),
        timezone_rules_version: rule.timezone_rules_version.clone(),
    };
    calendar.validate().map_err(|_| request_error())?;
    Ok(calendar)
}
fn recurrence_identity(
    family: &str,
    customer: &str,
    source: &str,
    change_id: &str,
) -> Result<Vec<u8>, ServiceError> {
    bytes(
        &json!({"schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin",
        "family":family,"customer":customer,"source":source,"key_kind":"change_id","key":change_id}),
    )
}
fn request_hash(raw: &[u8]) -> String {
    m5::hex(&m5::hash(b"bean-counter/m5/request/1\0", raw))
}
fn label(local: NaiveDateTime) -> String {
    let base = local.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = local.and_utc().timestamp_subsec_micros();
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}
fn occurrence_id(
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
    recurrence_version: i64,
    label: &str,
) -> Result<String, ServiceError> {
    let identity = bytes(
        &json!({"customer":customer,"source":source,"agreement_id":agreement_id,
        "agreement_version":agreement_version.to_string(),"recurrence_version":recurrence_version.to_string(),
        "scheduled_local_label":label}),
    )?;
    Ok(format!(
        "occ_{}",
        m5::hex(&m5::hash(b"bean-counter/m5/occurrence/1\0", &identity))
    ))
}
fn page_cursor(
    customer: &str,
    source: &str,
    version: i64,
    due_through: &str,
    ordinal: u64,
) -> Result<String, ServiceError> {
    let facts = bytes(
        &json!({"customer":customer,"source":source,"recurrence_version":version.to_string(),
        "due_through":due_through,"ordinal":ordinal.to_string()}),
    )?;
    Ok(format!(
        "m5o1:{ordinal}:{}",
        m5::hex(&m5::hash(b"bean-counter/m5/occurrence-cursor/1\0", &facts))
    ))
}
fn store_error_m5(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => reject("BILLING_M5_BOUNDS"),
        StoreError::BillingUpgradeRequired => reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => reject("BILLING_M5_INTEGRITY"),
        e => store_error(e),
    }
}

impl BillingLedger {
    pub async fn recurrence_set(&self, raw: &[u8]) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw).map_err(|_| request_error())?;
        no_null(&value)?;
        let wire: WireSet = serde_json::from_value(value.clone()).map_err(|_| request_error())?;
        if wire.schema != SET {
            return Err(request_error().into());
        }
        id(&wire.customer, 128)?;
        id(&wire.source, 256)?;
        id(&wire.change_id, 128)?;
        id(&wire.agreement_id, 128)?;
        let expected = decimal(&wire.expected_revision, true)?;
        let agreement_version = decimal(&wire.agreement_version, false)?;
        let current_rule = recurrence_calendar(&wire.rule)?;
        let successor = match wire.renewal.mode.as_str() {
            "manual"
                if wire.renewal.m2_change_id.is_none()
                    && wire.renewal.next_agreement_version.is_none()
                    && wire.renewal.next_rule.is_none() =>
            {
                None
            }
            "automatic_opt_in" => {
                let change = wire
                    .renewal
                    .m2_change_id
                    .as_deref()
                    .ok_or_else(request_error)?;
                id(change, 128)?;
                let next = decimal(
                    wire.renewal
                        .next_agreement_version
                        .as_deref()
                        .ok_or_else(request_error)?,
                    false,
                )?;
                if next != agreement_version + 1 {
                    return Err(request_error().into());
                }
                let rule = wire.renewal.next_rule.as_ref().ok_or_else(request_error)?;
                let calendar = recurrence_calendar(rule)?;
                Some((change.to_owned(), next, calendar))
            }
            _ => return Err(request_error().into()),
        };
        let identity = recurrence_identity(SET, &wire.customer, &wire.source, &wire.change_id)?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error_m5)? {
            if saved.request != raw {
                return Err(reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let state = tx
            .m5_recurrence_state(&wire.customer, &wire.source)
            .await
            .map_err(store_error_m5)?;
        if state.revision != expected {
            return Err(reject("BILLING_M5_STALE_REVISION").into());
        }
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        service::validate_snapshot(&snapshot)?;
        let mut linked = snapshot.agreements.iter().filter(|a| {
            a.customer == wire.customer
                && a.source == wire.source
                && a.agreement_id == wire.agreement_id
                && a.agreement_version == agreement_version
                && a.transition != "end"
        });
        let agreement = linked.next().ok_or_else(|| reject("BILLING_M5_SCOPE"))?;
        if linked.next().is_some()
            || current_rule.effective_at.timestamp_micros() < agreement.effective_at_us
        {
            return Err(reject("BILLING_M5_RECURRENCE").into());
        }
        let agreement_end = snapshot
            .agreements
            .iter()
            .filter(|a| {
                a.customer == wire.customer
                    && a.source == wire.source
                    && a.effective_at_us > agreement.effective_at_us
            })
            .map(|a| a.effective_at_us)
            .min();
        if agreement_end.is_some_and(|end| current_rule.effective_at.timestamp_micros() >= end) {
            return Err(reject("BILLING_M5_RECURRENCE").into());
        }
        if let Some((change, next, calendar)) = &successor {
            let successor_agreement = snapshot
                .agreements
                .iter()
                .find(|a| {
                    a.customer == wire.customer
                        && a.source == wire.source
                        && a.agreement_id == wire.agreement_id
                        && a.agreement_version == *next
                        && a.transition == "amend"
                        && a.effective_at_us == calendar.effective_at.timestamp_micros()
                })
                .ok_or_else(|| reject("BILLING_M5_RECURRENCE"))?;
            let control = snapshot
                .controls
                .iter()
                .find(|c| {
                    c.customer == wire.customer
                        && c.source == wire.source
                        && c.change_id == *change
                        && c.operation == "amend"
                })
                .ok_or_else(|| reject("BILLING_M5_RECURRENCE"))?;
            let control_request = ledgerlab_core::canonical::parse(&control.request)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let control_response = ledgerlab_core::canonical::parse(&control.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            if successor_agreement.revision != agreement.revision + 1
                || agreement_end != Some(successor_agreement.effective_at_us)
                || control_request["schema"] != "ledger-billing-amendment/2"
                || control_request["effective_at"]
                    .as_str()
                    .and_then(|s| Timestamp::parse(s).ok())
                    .is_none_or(|at| at.micros() != successor_agreement.effective_at_us)
                || control_request["setup"]["agreement"] != wire.agreement_id
                || control_response["agreement_id"] != wire.agreement_id
                || control_response["agreement_version"]
                    .as_str()
                    .and_then(|v| v.parse::<i64>().ok())
                    != Some(*next)
                || control_response["revision"]
                    .as_str()
                    .and_then(|v| v.parse::<i64>().ok())
                    != Some(successor_agreement.revision)
            {
                return Err(reject("BILLING_M5_RECURRENCE").into());
            }
        }
        let accepted = local::now()?;
        let accepted_at = accepted.as_str();
        let mut children_data = Vec::new();
        for i in 0..if successor.is_some() { 2 } else { 1 } {
            let version = state.next_version + i;
            let version_s = version.to_string();
            let key = bytes(
                &json!({"role":"recurrence-version","customer":wire.customer,"source":wire.source,"kind":VERSION,"key":{"recurrence_version":version_s}}),
            )?;
            let record_id = m5::record_id(&identity, &key);
            let rule = if i == 0 {
                value["rule"].clone()
            } else {
                value["renewal"]["next_rule"].clone()
            };
            let renewal = if i == 0 {
                value["renewal"].clone()
            } else {
                json!({"mode":"manual"})
            };
            let agreement_v = if i == 0 {
                agreement_version
            } else {
                successor.as_ref().unwrap().1
            };
            let payload = json!({"schema":VERSION,"customer":wire.customer,"source":wire.source,
                "agreement_id":wire.agreement_id,"agreement_version":agreement_v.to_string(),
                "recurrence_version":version_s,"rule":rule,"renewal":renewal,
                "record":{"record_id":record_id,"accepted_at":accepted_at,
                    "command_sequence":state.command_sequence.to_string()}});
            children_data.push((key, payload, record_id));
        }
        children_data.sort_by(|left, right| left.0.cmp(&right.0));
        let children_data = children_data
            .into_iter()
            .enumerate()
            .map(|(i, (key, mut payload, record_id))| {
                payload["record"]["sequence"] =
                    json!((state.first_record_sequence + i as i64).to_string());
                let data = m5::seal_child(VERSION, &mut payload).map_err(store_error_m5)?;
                Ok((key, data, record_id))
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        let ids = children_data
            .iter()
            .map(|(_, _, id)| id.clone())
            .collect::<Vec<_>>();
        let response = json!({"schema":"ledger-billing-recurrence-result/1","status":"recurrence_updated",
            "revision":(expected+1).to_string(),"recurrence_version":state.next_version.to_string(),
            "agreement_id":wire.agreement_id,"agreement_version":wire.agreement_version,"effective_at":wire.rule.effective_from,
            "receipt":{"schema":"ledger-billing-m5-receipt/1","command_sequence":state.command_sequence.to_string(),
                "accepted_at":accepted_at,"record_ids":ids,"request_hash":request_hash(raw)}});
        let response_bytes = bytes(&response)?;
        let children = children_data
            .iter()
            .map(|(key, data, _)| Child {
                family: VERSION,
                customer: Some(&wire.customer),
                source: Some(&wire.source),
                child_key: key,
                payload: data,
            })
            .collect::<Vec<_>>();
        let command = Command {
            family: SET,
            domain: "customer-admin",
            customer: Some(&wire.customer),
            source: Some(&wire.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock: true,
            request: raw,
            response: &response_bytes,
            children: &children,
        };
        tx.m5_append_recurrence_versions(&command)
            .await
            .map_err(store_error_m5)?;
        match tx.commit().await {
            Ok(()) => Ok(response),
            Err(CommitError::RolledBack(e)) => Err(store_error_m5(e).into()),
            Err(CommitError::OutcomeUnknown) => Err(reject("BILLING_M5_OUTCOME_UNKNOWN").into()),
        }
    }

    pub async fn recurrence_cancel(&self, raw: &[u8]) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw).map_err(|_| request_error())?;
        no_null(&value)?;
        let wire: WireCancel = serde_json::from_value(value).map_err(|_| request_error())?;
        if wire.schema != CANCEL {
            return Err(request_error().into());
        }
        id(&wire.customer, 128)?;
        id(&wire.source, 256)?;
        id(&wire.change_id, 128)?;
        let expected = decimal(&wire.expected_revision, true)?;
        let version = decimal(&wire.recurrence_version, false)?;
        let identity = recurrence_identity(CANCEL, &wire.customer, &wire.source, &wire.change_id)?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error_m5)? {
            if saved.request != raw {
                return Err(reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let state = tx
            .m5_recurrence_state(&wire.customer, &wire.source)
            .await
            .map_err(store_error_m5)?;
        if state.revision != expected {
            return Err(reject("BILLING_M5_STALE_REVISION").into());
        }
        let target = state
            .versions
            .iter()
            .find(|v| v.recurrence_version == version)
            .ok_or_else(|| reject("BILLING_M5_RECURRENCE"))?;
        if target.cancelled_at_us.is_some() {
            return Err(reject("BILLING_M5_RECURRENCE_CANCELLED").into());
        }
        let accepted = local::now()?;
        let accepted_at = accepted.as_str();
        let key = bytes(
            &json!({"role":"recurrence-cancellation","customer":wire.customer,"source":wire.source,"kind":CANCELLATION,"key":{"recurrence_version":wire.recurrence_version}}),
        )?;
        let record_id = m5::record_id(&identity, &key);
        let mut payload = json!({"schema":CANCELLATION,"customer":wire.customer,"source":wire.source,"recurrence_version":wire.recurrence_version,
            "cancelled_at":accepted_at,"record":{"record_id":record_id,"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}});
        let payload_bytes = m5::seal_child(CANCELLATION, &mut payload).map_err(store_error_m5)?;
        let response = json!({"schema":"ledger-billing-recurrence-cancel-result/1","status":"recurrence_cancelled",
            "revision":(expected+1).to_string(),"recurrence_version":wire.recurrence_version,"cancelled_at":accepted_at,
            "receipt":{"schema":"ledger-billing-m5-receipt/1","command_sequence":state.command_sequence.to_string(),
                "accepted_at":accepted_at,"record_ids":[record_id],"request_hash":request_hash(raw)}});
        let response_bytes = bytes(&response)?;
        let child = Child {
            family: CANCELLATION,
            customer: Some(&wire.customer),
            source: Some(&wire.source),
            child_key: &key,
            payload: &payload_bytes,
        };
        let command = Command {
            family: CANCEL,
            domain: "customer-admin",
            customer: Some(&wire.customer),
            source: Some(&wire.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock: true,
            request: raw,
            response: &response_bytes,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_recurrence_cancel(&command)
            .await
            .map_err(store_error_m5)?;
        match tx.commit().await {
            Ok(()) => Ok(response),
            Err(CommitError::RolledBack(e)) => Err(store_error_m5(e).into()),
            Err(CommitError::OutcomeUnknown) => Err(reject("BILLING_M5_OUTCOME_UNKNOWN").into()),
        }
    }

    pub async fn occurrences(&self, raw: &[u8]) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw).map_err(|_| request_error())?;
        no_null(&value)?;
        let wire: WireQuery = serde_json::from_value(value).map_err(|_| request_error())?;
        if wire.schema != "ledger-billing-occurrence-query/1"
            || wire.limit == 0
            || wire.limit > 1000
        {
            return Err(request_error().into());
        }
        id(&wire.customer, 128)?;
        id(&wire.source, 256)?;
        let version = decimal(&wire.recurrence_version, false)?;
        let due = parse_time(&wire.due_through)?;
        let now = local::now()?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::REPORT_BUDGET)
            .await
            .map_err(store_error)?;
        let state = tx
            .m5_recurrence_state(&wire.customer, &wire.source)
            .await
            .map_err(store_error_m5)?;
        let target = state
            .versions
            .iter()
            .find(|v| v.recurrence_version == version)
            .ok_or_else(|| reject("BILLING_M5_RECURRENCE"))?;
        let rule: WireRule = serde_json::from_value(target.rule.clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let calendar = recurrence_calendar(&rule)?;
        let cutoff = if target
            .cancelled_at_us
            .is_some_and(|cancel| now.micros() >= cancel)
        {
            i64::MIN
        } else {
            due.timestamp_micros().min(now.micros())
        };
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        service::validate_snapshot(&snapshot)?;
        let linked = snapshot
            .agreements
            .iter()
            .find(|a| {
                a.customer == wire.customer
                    && a.source == wire.source
                    && a.agreement_id == target.agreement_id
                    && a.agreement_version == target.agreement_version
            })
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_end = snapshot
            .agreements
            .iter()
            .filter(|a| {
                a.customer == wire.customer
                    && a.source == wire.source
                    && a.effective_at_us > linked.effective_at_us
            })
            .map(|a| a.effective_at_us)
            .min()
            .unwrap_or(i64::MAX);
        let start = match wire.cursor {
            None => 0,
            Some(cursor) => {
                let parts = cursor.strip_prefix("m5o1:").ok_or_else(request_error)?;
                let (n, _digest) = parts.split_once(':').ok_or_else(request_error)?;
                let ordinal = decimal(n, true)? as u64;
                if page_cursor(
                    &wire.customer,
                    &wire.source,
                    version,
                    &wire.due_through,
                    ordinal,
                )? != cursor
                {
                    return Err(request_error().into());
                }
                ordinal
            }
        };
        let mut occurrences = Vec::new();
        let mut ordinal = start;
        loop {
            if ordinal > 2_000_000 {
                return Err(reject("BILLING_M5_BOUNDS").into());
            }
            let local = calendar
                .scheduled_local(ordinal)
                .map_err(|_| request_error())?;
            let scheduled = calendar.resolve_local(local).map_err(|_| request_error())?;
            let us = scheduled.timestamp_micros();
            if us > cutoff || us >= agreement_end {
                break;
            }
            if us >= calendar.effective_at.timestamp_micros() {
                let scheduled_label = label(local);
                let id = occurrence_id(
                    &wire.customer,
                    &wire.source,
                    &target.agreement_id,
                    target.agreement_version,
                    version,
                    &scheduled_label,
                )?;
                if !tx
                    .m5_occurrence_accepted(&wire.customer, &wire.source, &id)
                    .await
                    .map_err(store_error_m5)?
                {
                    occurrences.push(json!({"occurrence_id":id,"scheduled_local_label":scheduled_label,
                        "scheduled_at":scheduled.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
                        "agreement_id":target.agreement_id,"agreement_version":target.agreement_version.to_string()}));
                }
                if occurrences.len() > wire.limit as usize {
                    break;
                }
            }
            ordinal += 1;
        }
        let has_more = occurrences.len() > wire.limit as usize;
        if has_more {
            occurrences.pop();
        }
        let mut response = json!({"schema":"ledger-billing-occurrence-list/1","status":"ok",
            "due_through":wire.due_through,"has_more":has_more,"occurrences":occurrences});
        if has_more {
            response["next_cursor"] = json!(page_cursor(
                &wire.customer,
                &wire.source,
                version,
                &wire.due_through,
                ordinal
            )?);
        }
        tx.rollback().await.map_err(store_error)?;
        Ok(response)
    }

    pub async fn occurrence_accept(&self, raw: &[u8]) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw).map_err(|_| request_error())?;
        no_null(&value)?;
        let wire: WireAccept = serde_json::from_value(value).map_err(|_| request_error())?;
        if wire.schema != ACCEPT {
            return Err(request_error().into());
        }
        id(&wire.customer, 128)?;
        id(&wire.source, 256)?;
        if wire.occurrence_id.len() != 68
            || !wire.occurrence_id.starts_with("occ_")
            || !wire.occurrence_id[4..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(request_error().into());
        }
        if wire.event["id"] != wire.occurrence_id
            || wire.event["customer"] != wire.customer
            || wire.event["type"] != "content.generated"
            || wire.event["status"] != "succeeded"
        {
            return Err(request_error().into());
        }
        let identity = bytes(
            &json!({"schema":"ledger-billing-m5-command-identity/1","domain":"application",
            "family":ACCEPT,"customer":wire.customer,"source":wire.source,"key_kind":"occurrence_id","key":wire.occurrence_id}),
        )?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        let accepted = local::now()?;
        let snapshot = tx.billing_meta().await.map_err(store_error)?;
        let event_bytes = bytes(&wire.event)?;
        let submission = service::begin(
            &snapshot,
            &wire.customer,
            &wire.source,
            &event_bytes,
            &accepted,
        )?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error_m5)? {
            if saved.request != raw {
                return Err(reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let state = tx
            .m5_recurrence_state(&wire.customer, &wire.source)
            .await
            .map_err(store_error_m5)?;
        if tx
            .billing_identity_lookup(&wire.customer, &wire.source, submission.external_id())
            .await
            .map_err(store_error)?
            .is_some()
            || tx
                .billing_semantic_lookup(&wire.customer, &wire.source, submission.semantic_key())
                .await
                .map_err(store_error)?
                .is_some()
        {
            return Err(reject("BILLING_M5_OCCURRENCE_CONFLICT").into());
        }
        let mut selected = None;
        for version in &state.versions {
            let rule: WireRule = serde_json::from_value(version.rule.clone())
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let calendar = recurrence_calendar(&rule)?;
            for ordinal in 0..=2_000_000u64 {
                let local = calendar
                    .scheduled_local(ordinal)
                    .map_err(|_| reject("BILLING_M5_BOUNDS"))?;
                let scheduled = calendar
                    .resolve_local(local)
                    .map_err(|_| reject("BILLING_M5_BOUNDS"))?;
                if scheduled.timestamp_micros() > accepted.micros() {
                    break;
                }
                if scheduled < calendar.effective_at {
                    continue;
                }
                let scheduled_label = label(local);
                if occurrence_id(
                    &wire.customer,
                    &wire.source,
                    &version.agreement_id,
                    version.agreement_version,
                    version.recurrence_version,
                    &scheduled_label,
                )? == wire.occurrence_id
                {
                    if version.cancelled_at_us.is_some() {
                        return Err(reject("BILLING_M5_CANCELLED").into());
                    }
                    selected = Some((version, scheduled_label));
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (version, scheduled_label) = selected.ok_or_else(|| reject("BILLING_M5_RECURRENCE"))?;
        let (m4_result, plan) = service::finish(&snapshot, &submission, &service::Dedup::empty())?;
        let plan = plan.ok_or_else(|| reject("BILLING_M5_OCCURRENCE_CONFLICT"))?;
        if plan.agreement_id() != Some(version.agreement_id.as_str())
            || plan.agreement_version() != Some(version.agreement_version)
            || plan.alias().is_some()
        {
            return Err(reject("BILLING_M5_RECURRENCE").into());
        }
        let assignment = tx
            .m5_assignment_for_occurrence(&plan)
            .await
            .map_err(store_error_m5)?
            .ok_or_else(|| reject("BILLING_M5_PERIOD"))?;
        let receipt_id = m4_result["receipt"]["id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .to_owned();
        let key = bytes(
            &json!({"role":"occurrence-acceptance","customer":wire.customer,"source":wire.source,
            "kind":ACCEPT_RECORD,"key":{"occurrence_id":wire.occurrence_id}}),
        )?;
        let record_id = m5::record_id(&identity, &key);
        let mut payload = json!({"schema":ACCEPT_RECORD,"customer":wire.customer,"source":wire.source,
            "occurrence_id":wire.occurrence_id,"scheduled_local_label":scheduled_label,
            "agreement_version":version.agreement_version.to_string(),
            "recurrence_version":version.recurrence_version.to_string(),
            "accepted_m3_receipt_id":receipt_id,"period_id":{"term_version":assignment.term_version.to_string(),
                "period_index":assignment.period_index.to_string()},
            "record":{"record_id":record_id,"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":state.command_sequence.to_string()}});
        let payload_bytes = m5::seal_child(ACCEPT_RECORD, &mut payload).map_err(store_error_m5)?;
        let response = m4_result;
        let response_bytes = bytes(&response)?;
        let child = Child {
            family: ACCEPT_RECORD,
            customer: Some(&wire.customer),
            source: Some(&wire.source),
            child_key: &key,
            payload: &payload_bytes,
        };
        let command = Command {
            family: ACCEPT,
            domain: "application",
            customer: Some(&wire.customer),
            source: Some(&wire.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock: true,
            request: raw,
            response: &response_bytes,
            children: std::slice::from_ref(&child),
        };
        tx.append_billing_m3_for_occurrence(&plan)
            .await
            .map_err(store_error_m5)?;
        tx.m5_append_occurrence(&command, &receipt_id)
            .await
            .map_err(store_error_m5)?;
        match tx.commit().await {
            Ok(()) => Ok(response),
            Err(CommitError::RolledBack(e)) => Err(store_error_m5(e).into()),
            Err(CommitError::OutcomeUnknown) => Err(reject("BILLING_M5_OUTCOME_UNKNOWN").into()),
        }
    }
}
