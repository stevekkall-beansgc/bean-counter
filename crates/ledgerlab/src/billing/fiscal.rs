use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::{
    canonical::CanonicalBytes,
    domain::{fiscal_calendar::FiscalCalendarConfig, Revision},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const FISCAL_SET: &str = "ledger-fiscal-calendar/1";
const FISCAL_VERSION: &str = "ledger-fiscal-calendar-version/1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FiscalSetRequest {
    schema: String,
    change_id: String,
    expected_revision: String,
    timezone: String,
    timezone_rules_version: String,
    #[allow(dead_code)]
    reason: Option<String>,
    calendar: ledgerlab_core::domain::fiscal_calendar::FiscalCalendar,
}

fn bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))
}

fn fiscal_store_error(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => service::reject("BILLING_M5_BOUNDS"),
        StoreError::BillingUpgradeRequired => service::reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => {
            service::reject("BILLING_M5_INTEGRITY")
        }
        other => store_error(other),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|character| character.is_control())
}

impl BillingLedger {
    pub async fn fiscal_set(&self, raw: &[u8]) -> local::Result<Value> {
        self.fiscal_set_inner(raw, None).await
    }

    #[cfg(test)]
    async fn fiscal_set_at(
        &self,
        raw: &[u8],
        accepted: ledgerlab_core::domain::Timestamp,
    ) -> local::Result<Value> {
        self.fiscal_set_inner(raw, Some(accepted)).await
    }

    async fn fiscal_set_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<ledgerlab_core::domain::Timestamp>,
    ) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let request: FiscalSetRequest = serde_json::from_value(value.clone())
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let expected_revision = Revision::parse(&request.expected_revision)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let config = FiscalCalendarConfig {
            timezone: request.timezone.clone(),
            timezone_rules_version: request.timezone_rules_version.clone(),
            calendar: request.calendar,
        };
        if request.schema != FISCAL_SET
            || !valid_id(&request.change_id)
            || request.timezone.is_empty()
            || request.timezone.len() > 128
            || request.timezone_rules_version.is_empty()
            || request.timezone_rules_version.len() > 64
            || value
                .get("reason")
                .is_some_and(|reason| !reason.is_string())
            || request
                .reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 8192)
            || config.validate().is_err()
        {
            return Err(service::reject("BILLING_M5_REQUEST").into());
        }
        let identity = bytes(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"installation-admin",
            "family":FISCAL_SET,"key_kind":"change_id","key":request.change_id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(fiscal_store_error)? {
            if saved.request != raw {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        service::validate_snapshot(&snapshot)?;
        let state = tx.m5_fiscal_state().await.map_err(fiscal_store_error)?;
        if expected_revision.value() != state.revision as u64 {
            return Err(service::reject("BILLING_M5_STALE_REVISION").into());
        }
        let calendar_version = state.next_calendar_version.to_string();
        let accepted = match accepted_override {
            Some(accepted) => accepted,
            None => local::now()?,
        };
        let accepted_at = accepted.as_str().to_owned();
        let key = bytes(&json!({
            "role":"fiscal-calendar-version","kind":FISCAL_VERSION,
            "key":{"calendar_version":calendar_version}
        }))?;
        let record_id = m5::record_id(&identity, &key);
        let mut payload = json!({
            "schema":FISCAL_VERSION,"calendar_version":calendar_version,
            "timezone":request.timezone,
            "timezone_rules_version":request.timezone_rules_version,
            "calendar":value["calendar"],
            "record":{"record_id":record_id,"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}
        });
        let payload = m5::seal_child(FISCAL_VERSION, &mut payload).map_err(fiscal_store_error)?;
        let child = Child {
            family: FISCAL_VERSION,
            customer: None,
            source: None,
            child_key: &key,
            payload: &payload,
        };
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/request/1\0");
        digest.update(raw);
        let result = json!({
            "schema":"ledger-fiscal-calendar-result/1","status":"calendar_updated",
            "revision":state.next_calendar_version.to_string(),
            "calendar_version":calendar_version,
            "receipt":{"schema":"ledger-billing-m5-receipt/1",
                "command_sequence":state.command_sequence.to_string(),"accepted_at":accepted_at,
                "record_ids":[record_id],"request_hash":m5::hex(&digest.finalize())}
        });
        let response = bytes(&result)?;
        let command = Command {
            family: FISCAL_SET,
            domain: "installation-admin",
            customer: None,
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            request: raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        let calendar_bytes = bytes(&value["calendar"])?;
        tx.m5_append_fiscal_version(
            &command,
            &m5::FiscalVersionProjection {
                calendar_version: state.next_calendar_version,
                timezone: &request.timezone,
                timezone_rules_version: &request.timezone_rules_version,
                calendar_bytes: &calendar_bytes,
            },
        )
        .await
        .map_err(fiscal_store_error)?;
        match tx.commit().await {
            Ok(()) => Ok(result),
            Err(CommitError::RolledBack(error)) => Err(store_error(error).into()),
            Err(CommitError::OutcomeUnknown) => {
                Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN").into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;

    fn request(change_id: &str, expected_revision: &str) -> Vec<u8> {
        CanonicalBytes::from_value(&json!({
            "schema":"ledger-fiscal-calendar/1","change_id":change_id,
            "expected_revision":expected_revision,"timezone":"America/New_York",
            "timezone_rules_version":"IANA-2025b","reason":"operator fiscal policy",
            "calendar":{"kind":"gregorian_months","fiscal_year_start_month":2,
                "fiscal_year_start_day":29,"week_start":"monday"}
        }))
        .unwrap()
        .into_vec()
    }

    #[tokio::test]
    async fn fiscal_versions_are_scoped_to_the_installation_and_retry_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let first = request("fiscal-1", "0");
        let mut null_reason: Value = serde_json::from_slice(&first).unwrap();
        null_reason["change_id"] = json!("fiscal-null");
        null_reason["reason"] = Value::Null;
        let null_reason = CanonicalBytes::from_value(&null_reason).unwrap().into_vec();
        assert!(matches!(
            ledger.fiscal_set(&null_reason).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code)))
                if code == "BILLING_M5_REQUEST"
        ));
        let result = ledger.fiscal_set(&first).await.unwrap();
        assert_eq!(result["status"], "calendar_updated");
        assert_eq!(result["revision"], "1");
        assert_eq!(result["calendar_version"], "1");
        assert_eq!(ledger.fiscal_set(&first).await.unwrap(), result);
        let mut conflict: Value = serde_json::from_slice(&first).unwrap();
        conflict["timezone"] = json!("UTC");
        let conflict = CanonicalBytes::from_value(&conflict).unwrap().into_vec();
        assert!(matches!(
            ledger.fiscal_set(&conflict).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code)))
                if code == "IDENTITY_CONFLICT"
        ));
        assert!(matches!(
            ledger.fiscal_set(&request("fiscal-stale", "0")).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code)))
                if code == "BILLING_M5_STALE_REVISION"
        ));
        assert_eq!(
            ledger.fiscal_set(&request("fiscal-2", "1")).await.unwrap()["calendar_version"],
            "2"
        );
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let counts: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM billing_m5_fiscal_versions),(SELECT count(*) FROM billing_m5_commands WHERE family='ledger-fiscal-calendar/1'),(SELECT count(*) FROM billing_m5_snapshot_boundaries)",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(counts, (2, 2, 3));
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn fiscal_calendar_command_matches_the_frozen_golden() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../../contracts/candidates/billing-lifecycle-m5/vectors/m5-command-goldens.json"
        ))
        .unwrap();
        let command = &oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "fiscal-calendar-version-control")
            .unwrap()["command"];
        let raw = CanonicalBytes::from_value(&command["request"])
            .unwrap()
            .into_vec();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let accepted =
            ledgerlab_core::domain::Timestamp::parse("2026-01-02T09:00:00.000000Z").unwrap();
        assert_eq!(
            ledger.fiscal_set_at(&raw, accepted).await.unwrap(),
            command["result"]
        );
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let (identity, response, payload): (Vec<u8>, Vec<u8>, Vec<u8>) = sqlx::query_as(
            "SELECT c.identity_key,c.response_bytes,r.payload_bytes FROM billing_m5_commands c JOIN billing_m5_records r ON r.command_sequence=c.command_sequence WHERE c.command_sequence=1",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(
            identity,
            command["identity_key_canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
        assert_eq!(
            response,
            command["result_canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
        assert_eq!(
            payload,
            command["domain_children"][0]["payload_canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
        conn.close().await.unwrap();
    }
}
