use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::{canonical::CanonicalBytes, domain::term_service};
use sha2::{Digest, Sha256};

const TERM: &str = "ledger-billing-term/1";
const RESOLUTION: &str = "ledger-billing-boundary-resolution/1";
const VERSION: &str = "ledger-billing-term-version/1";

fn bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))
}
fn period_error(error: term_service::TermPlanError) -> ServiceError {
    match error {
        term_service::TermPlanError::HistoryBeforeEffective
        | term_service::TermPlanError::InvalidHistory
        | term_service::TermPlanError::InvalidVersion => service::reject("BILLING_M5_PERIOD"),
        _ => service::reject("BILLING_M5_REQUEST"),
    }
}
fn m5_store_error(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => service::reject("BILLING_M5_BOUNDS"),
        StoreError::BillingUpgradeRequired => service::reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => {
            service::reject("BILLING_M5_INTEGRITY")
        }
        other => store_error(other),
    }
}

impl BillingLedger {
    /// Activate a customer's first term. The complete M3 history and every
    /// projection are validated under the same writer transaction.
    pub async fn term_set(&self, raw: &[u8]) -> local::Result<Value> {
        let request = term_service::parse_initial_request(raw).map_err(period_error)?;
        let request_value = ledgerlab_core::canonical::parse(raw)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let identity = bytes(&json!({
            "schema":"ledger-billing-m5-command-identity/1", "domain":"customer-admin",
            "family":TERM, "customer":request.customer, "key_kind":"change_id", "key":request.change_id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(m5_store_error)? {
            if saved.request != raw {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let mut state = tx
            .m5_term_state(&request.customer)
            .await
            .map_err(m5_store_error)?;
        if state.revision != 0 || request.expected_revision.value() != 0 {
            return Err(service::reject("BILLING_M5_STALE_REVISION").into());
        }
        // A full retained snapshot rechecks each M3 index row against its
        // original receipt bundle before assigning the history.
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        service::validate_snapshot(&snapshot)?;
        service::require(
            snapshot
                .customers
                .iter()
                .any(|c| c.customer == request.customer),
            "BILLING_M5_SCOPE",
        )?;
        let mut receipt_refs = service::validated_m3_receipt_refs(&snapshot)?;
        for row in &mut state.history {
            if row.customer != request.customer {
                return Err(ServiceError::IntegrityFailure.into());
            }
            let (kind, id) = receipt_refs
                .remove(&row.ordinal)
                .ok_or(ServiceError::IntegrityFailure)?;
            if (row.kind == "base") != (kind == "base-acceptance") || id.is_empty() {
                return Err(ServiceError::IntegrityFailure.into());
            }
            row.kind = kind;
            row.record_id = id;
        }
        let history = state
            .history
            .iter()
            .map(|row| {
                Ok(term_service::BillableHistoryRow {
                    ordinal: u64::try_from(row.ordinal)
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                    accepted_at_us: row.accepted_at_us,
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        let plan = term_service::plan_initial_activation(
            request,
            u64::try_from(state.next_term_version)
                .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?,
            &history,
        )
        .map_err(period_error)?;
        let term_version = plan.term_version.to_string();
        let accepted = local::now()?;
        let accepted_at = accepted.as_str().to_owned();
        let effective_at = plan
            .request
            .term
            .effective_at
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        let end_at = plan
            .period_zero
            .end
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        let term_bytes = bytes(&request_value["term"])?;
        let resolution_id = format!("resolution-{term_version}-0");
        let keys = [
            bytes(
                &json!({"role":"boundary-resolution","customer":plan.request.customer,"kind":RESOLUTION,"key":{"resolution_id":resolution_id}}),
            )?,
            bytes(
                &json!({"role":"term-version","customer":plan.request.customer,"kind":VERSION,"key":{"term_version":term_version}}),
            )?,
        ];
        let ids = [
            m5::record_id(&identity, &keys[0]),
            m5::record_id(&identity, &keys[1]),
        ];
        let mut resolution = json!({
            "schema":RESOLUTION,"customer":plan.request.customer,
            "period_id":{"term_version":term_version,"period_index":"0"},
            "resolution_id":resolution_id,"term_version":term_version,
            "start_utc":effective_at,"end_utc":end_at,
            "record":{"record_id":ids[0],"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}
        });
        let mut version_payload = json!({
            "schema":VERSION,"customer":plan.request.customer,"term_version":term_version,
            "effective_at":effective_at,"term":request_value["term"],
            "record":{"record_id":ids[1],"sequence":(state.first_record_sequence+1).to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}
        });
        let payloads = [
            m5::seal_child(RESOLUTION, &mut resolution).map_err(m5_store_error)?,
            m5::seal_child(VERSION, &mut version_payload).map_err(m5_store_error)?,
        ];
        let children = [
            Child {
                family: RESOLUTION,
                customer: Some(&plan.request.customer),
                source: None,
                child_key: &keys[0],
                payload: &payloads[0],
            },
            Child {
                family: VERSION,
                customer: Some(&plan.request.customer),
                source: None,
                child_key: &keys[1],
                payload: &payloads[1],
            },
        ];
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/request/1\0");
        digest.update(raw);
        let result = json!({
            "schema":"ledger-billing-term-result/1","status":"term_updated",
            "revision":"1","term_version":term_version,"effective_at":effective_at,
            "receipt":{"schema":"ledger-billing-m5-receipt/1", "command_sequence":state.command_sequence.to_string(),
                "accepted_at":accepted_at,"record_ids":ids,"request_hash":m5::hex(&digest.finalize())}
        });
        let response = bytes(&result)?;
        let command = Command {
            family: TERM,
            domain: "customer-admin",
            customer: Some(&plan.request.customer),
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            request: raw,
            response: &response,
            children: &children,
        };
        let mut assignments = Vec::with_capacity(state.history.len());
        for assignment in &plan.assignments {
            let row = state
                .history
                .iter()
                .find(|row| row.ordinal as u64 == assignment.ordinal)
                .ok_or(ServiceError::IntegrityFailure)?;
            assignments.push((
                assignment.term_version as i64,
                assignment.period_index as i64,
                row,
            ));
        }
        tx.m5_append_initial_term(
            &command,
            &plan.request.customer,
            &term_bytes,
            plan.request.term.effective_at.timestamp_micros(),
            plan.period_zero.end.timestamp_micros(),
            &resolution_id,
            state.next_term_version,
            &assignments,
        )
        .await
        .map_err(m5_store_error)?;
        match tx.commit().await {
            Ok(()) => Ok(result),
            Err(CommitError::RolledBack(error)) => Err(store_error(error).into()),
            Err(CommitError::OutcomeUnknown) => {
                Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN").into())
            }
        }
    }
}
