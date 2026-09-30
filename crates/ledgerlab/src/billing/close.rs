use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::{canonical::CanonicalBytes, domain::term_service};
use sha2::{Digest, Sha256};

const CLOSE: &str = "ledger-billing-period-close/1";
const CLOSE_RECORD: &str = "ledger-billing-period-close-record/1";
const MAX_CLOSE_ARTIFACT_BYTES: usize = 262_144;

#[derive(Clone, Copy)]
struct CloseLimits {
    response: usize,
    payload: usize,
}

const PRODUCTION_CLOSE_LIMITS: CloseLimits = CloseLimits {
    response: MAX_CLOSE_ARTIFACT_BYTES,
    payload: MAX_CLOSE_ARTIFACT_BYTES,
};

fn bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))
}

fn output_bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    let encoded = serde_json::to_vec(value).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
    if encoded.len() > MAX_CLOSE_ARTIFACT_BYTES {
        return Err(service::reject("BILLING_M5_BOUNDS"));
    }
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))
}

fn close_store_error(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => service::reject("BILLING_M5_BOUNDS"),
        StoreError::BillingUpgradeRequired => service::reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::BillingPeriod => service::reject("BILLING_M5_PERIOD"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => {
            service::reject("BILLING_M5_INTEGRITY")
        }
        other => store_error(other),
    }
}

fn text<'a>(value: &'a Value, code: &'static str) -> Result<&'a str, ServiceError> {
    value.as_str().ok_or_else(|| service::reject(code))
}

fn integer(value: &Value, code: &'static str) -> Result<i128, ServiceError> {
    text(value, code)?
        .parse()
        .map_err(|_| service::reject(code))
}

fn close_period_bounds(
    state: &m5::PeriodCloseState,
    customer: &str,
    term_version: i64,
    period_index: i64,
) -> Result<(Value, i64, i64), ServiceError> {
    let payload = ledgerlab_core::canonical::parse(&state.term.payload_bytes)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let retained_term = ledgerlab_core::canonical::parse(&state.term.term_bytes)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if bytes(&payload["term"])? != state.term.term_bytes
        || payload["customer"] != customer
        || payload["term_version"]
            .as_str()
            .and_then(|version| version.parse::<i64>().ok())
            != Some(term_version)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let verification = bytes(&json!({
        "schema":"ledger-billing-term/1","customer":customer,
        "change_id":"period-close","expected_revision":"0",
        "effective":{"mode":"initial","at":payload["effective_at"]},
        "term":retained_term
    }))?;
    let term = term_service::parse_initial_request(&verification)
        .map_err(|_| ServiceError::IntegrityFailure)?
        .term;
    let period = term
        .period(period_index)
        .map_err(term_service::TermPlanError::from)
        .map_err(|_| service::reject("BILLING_M5_PERIOD"))?;
    Ok((
        retained_term,
        period.start.timestamp_micros(),
        period.end.timestamp_micros(),
    ))
}

fn per_work_lines(
    customer: &str,
    period_id: &Value,
    assignments: &[m5::PeriodCloseM3Assignment],
) -> Result<(Vec<Value>, Vec<Value>, i128), ServiceError> {
    let mut lines = Vec::with_capacity(assignments.len());
    let mut included = Vec::with_capacity(assignments.len());
    let mut net = 0i128;
    for assignment in assignments {
        if assignment.kind != "base-acceptance" {
            return Err(service::reject("BILLING_M5_PERIOD"));
        }
        let agreement_id = assignment
            .agreement_id
            .as_deref()
            .ok_or_else(|| service::reject("BILLING_M5_PERIOD"))?;
        let agreement_version = assignment
            .agreement_version
            .filter(|version| *version > 0)
            .ok_or_else(|| service::reject("BILLING_M5_PERIOD"))?;
        let bundle = ledgerlab_core::canonical::parse_bounded(&assignment.bundle, 8 * 1024 * 1024)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let rows = bundle.as_array().ok_or(ServiceError::IntegrityFailure)?;
        if !rows
            .iter()
            .any(|row| row["kind"] == assignment.kind && row["id"] == assignment.id)
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let event = rows
            .iter()
            .find(|row| row["kind"] == "event")
            .ok_or(ServiceError::IntegrityFailure)?;
        let postings = rows
            .iter()
            .filter(|row| row["kind"] == "base-posting")
            .collect::<Vec<_>>();
        if postings.is_empty() {
            return Err(ServiceError::IntegrityFailure);
        }
        let payer = text(&postings[0]["body"]["roles"]["payer"], "BILLING_M5_PERIOD")?;
        let recipient = text(
            &postings[0]["body"]["roles"]["recipient"],
            "BILLING_M5_PERIOD",
        )?;
        let mut booked = 0i128;
        let mut scale = None;
        for posting in postings {
            if posting["body"]["agreement_id"] != agreement_id
                || posting["body"]["roles"]["payer"] != payer
                || posting["body"]["roles"]["recipient"] != recipient
                || posting["body"]["amount"]["currency"] != "USD"
            {
                return Err(ServiceError::IntegrityFailure);
            }
            let posting_scale = posting["body"]["amount"]["scale"]
                .as_u64()
                .ok_or(ServiceError::IntegrityFailure)?;
            if scale
                .replace(posting_scale)
                .is_some_and(|old| old != posting_scale)
            {
                return Err(ServiceError::IntegrityFailure);
            }
            booked = booked
                .checked_add(integer(
                    &posting["body"]["amount"]["atoms"],
                    "BILLING_M5_BOUNDS",
                )?)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        }
        let source_record = json!({
            "customer":customer,"source":assignment.source,"kind":assignment.kind,"id":assignment.id
        });
        included.push(source_record.clone());
        let (basis, amount, calculation) = if scale == Some(18) {
            let quantity = text(&event["body"]["data"]["quantity"], "BILLING_M5_PERIOD")?;
            let unit = text(&event["body"]["data"]["unit"], "BILLING_M5_PERIOD")?;
            let quantity = quantity
                .parse::<i128>()
                .ok()
                .filter(|quantity| *quantity > 0)
                .ok_or_else(|| service::reject("BILLING_M5_PERIOD"))?;
            if booked % quantity != 0 {
                return Err(ServiceError::IntegrityFailure);
            }
            let rate = booked / quantity;
            (
                "per_work",
                booked,
                json!({"kind":"per_work","exact_atoms_numerator":booked.to_string(),
                    "exact_atoms_denominator":"1","booked_atoms":booked.to_string(),"rounding":"none",
                    "operands":{"agreement_id":agreement_id,
                        "agreement_version":agreement_version.to_string(),"unit":unit,
                        "quantity":quantity.to_string(),"rate_atoms_per_unit":rate.to_string()}}),
            )
        } else if scale == Some(2) {
            let amount = booked
                .checked_mul(10_000_000_000_000_000)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            (
                "fixed",
                amount,
                json!({"kind":"fixed","exact_atoms_numerator":amount.to_string(),
                    "exact_atoms_denominator":"1","booked_atoms":amount.to_string(),"rounding":"none",
                    "operands":{"agreement_id":agreement_id,
                        "agreement_version":agreement_version.to_string(),
                        "fixed_atoms_scale_2":booked.to_string(),
                "usd_scale_18_multiplier":"10000000000000000"}}),
            )
        } else {
            return Err(ServiceError::IntegrityFailure);
        };
        net = net
            .checked_add(amount)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let mut line = json!({
            "source_records":[source_record],"basis":basis,"agreement_id":agreement_id,
            "agreement_version":agreement_version.to_string(),"payer":payer,"recipient":recipient,
            "currency":"USD","scale":18,"amount_atoms":amount.to_string(),
            "calculation":calculation
        });
        let line_identity = bytes(&json!({
            "view_kind":"standard","view_identity":{"customer":customer,"period_id":period_id},
            "line":line
        }))?;
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/statement-line/1\0");
        digest.update(line_identity);
        line["line_id"] = json!(m5::hex(&digest.finalize()));
        lines.push(line);
    }
    included.sort_by(|left, right| {
        (
            left["customer"].as_str(),
            left["source"].as_str(),
            left["kind"].as_str(),
            left["id"].as_str(),
        )
            .cmp(&(
                right["customer"].as_str(),
                right["source"].as_str(),
                right["kind"].as_str(),
                right["id"].as_str(),
            ))
    });
    lines.sort_by(|left, right| left["line_id"].as_str().cmp(&right["line_id"].as_str()));
    Ok((lines, included, net))
}

struct CloseArtifacts {
    result: Value,
    response: Vec<u8>,
    key: Vec<u8>,
    payload: Vec<u8>,
    statement_hash: String,
}

#[allow(clippy::too_many_arguments)]
fn prepare_close_artifacts(
    identity: &[u8],
    customer: &str,
    period_id: &Value,
    term: &Value,
    boundary_resolution_id: &str,
    start_at_us: i64,
    end_at_us: i64,
    accepted: &ledgerlab_core::domain::Timestamp,
    lines: &[Value],
    included_records: &[Value],
    net: i128,
    m3_high_water: i64,
    m5_high_water: i64,
    snapshot_boundary_id: i64,
    first_record_sequence: i64,
    command_sequence: i64,
) -> Result<CloseArtifacts, ServiceError> {
    let start_utc = ledgerlab_core::domain::Timestamp::from_micros(start_at_us)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?
        .as_str()
        .to_owned();
    let end_utc = ledgerlab_core::domain::Timestamp::from_micros(end_at_us)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?
        .as_str()
        .to_owned();
    let accepted_at = accepted.as_str().to_owned();
    let direction = if net == 0 {
        "none"
    } else if net < 0 {
        "payable"
    } else {
        "receivable"
    };
    let mut result = json!({
        "schema":"ledger-billing-statement/4","status":"closed",
        "customer":customer,"period_id":period_id,
        "boundary_resolution_id":boundary_resolution_id,
        "timezone":term["timezone"],
        "timezone_rules_version":term["timezone_rules_version"],
        "boundary_rule_version":term["boundary_rule_version"],
        "start_utc":start_utc,"end_utc":end_utc,"currency":"USD","scale":18,
        "lines":lines,"net_atoms":net.to_string(),"direction":direction,"complete":true,
        "close_acceptance_time":accepted_at,
        "m3_high_water":m3_high_water.to_string(),
        "m5_high_water":m5_high_water.to_string(),
        "snapshot_boundary_id":snapshot_boundary_id.to_string()
    });
    let unsigned = output_bytes(&result)?;
    let mut statement_digest = Sha256::new();
    statement_digest.update(b"bean-counter/m5/statement/4\0");
    statement_digest.update(&unsigned);
    let statement_hash = m5::hex(&statement_digest.finalize());
    result["statement_hash"] = json!(statement_hash);
    let response = output_bytes(&result)?;
    let key = bytes(&json!({
        "role":"period-close","customer":customer,"kind":CLOSE_RECORD,
        "key":{"period_id":period_id}
    }))?;
    let record_id = m5::record_id(identity, &key);
    let mut payload = json!({
        "schema":CLOSE_RECORD,"customer":customer,"period_id":period_id,
        "boundary_resolution_id":boundary_resolution_id,
        "start_utc":start_utc,"end_utc":end_utc,
        "m3_high_water":m3_high_water.to_string(),
        "m5_high_water":m5_high_water.to_string(),
        "included_records":included_records,"lines":lines,"net_atoms":net.to_string(),
        "statement_hash":statement_hash,
        "snapshot_boundary_id":snapshot_boundary_id.to_string(),
        "record":{"record_id":record_id,"sequence":first_record_sequence.to_string(),
            "accepted_at":accepted_at,"command_sequence":command_sequence.to_string()}
    });
    payload["record"]["payload_hash"] = json!("0".repeat(64));
    output_bytes(&payload)?;
    let payload = m5::seal_child(CLOSE_RECORD, &mut payload).map_err(close_store_error)?;
    Ok(CloseArtifacts {
        result,
        response,
        key,
        payload,
        statement_hash,
    })
}

#[allow(clippy::too_many_arguments)]
fn period_resolution_append_bytes(
    customer: &str,
    term_version: i64,
    period_index: i64,
    start_at_us: i64,
    end_at_us: i64,
    accepted: &ledgerlab_core::domain::Timestamp,
    command_sequence: i64,
    record_sequence: i64,
) -> Result<i64, ServiceError> {
    let raw = bytes(&json!({
        "schema":"ledger-billing-period-resolve/1","customer":customer,
        "period_id":{"term_version":term_version.to_string(),"period_index":period_index.to_string()}
    }))?;
    let identity = bytes(&json!({
        "schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin",
        "family":"ledger-billing-period-resolve/1","customer":customer,
        "key_kind":"logical_period_id","key":format!("{term_version}:{period_index}")
    }))?;
    let resolution_id = format!("boundary-resolution-{term_version}-{period_index}");
    let key = bytes(&json!({
        "role":"boundary-resolution","customer":customer,
        "kind":"ledger-billing-boundary-resolution/1","key":{"resolution_id":resolution_id}
    }))?;
    let record_id = m5::record_id(&identity, &key);
    let start_at = ledgerlab_core::domain::Timestamp::from_micros(start_at_us)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?
        .as_str()
        .to_owned();
    let end_at = ledgerlab_core::domain::Timestamp::from_micros(end_at_us)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?
        .as_str()
        .to_owned();
    let accepted_at = accepted.as_str().to_owned();
    let mut payload = json!({
        "schema":"ledger-billing-boundary-resolution/1","customer":customer,
        "period_id":{"term_version":term_version.to_string(),"period_index":period_index.to_string()},
        "resolution_id":resolution_id,"term_version":term_version.to_string(),
        "start_utc":start_at,"end_utc":end_at,
        "record":{"record_id":record_id,"sequence":record_sequence.to_string(),
            "accepted_at":accepted_at,"command_sequence":command_sequence.to_string()}
    });
    let payload = m5::seal_child("ledger-billing-boundary-resolution/1", &mut payload)
        .map_err(close_store_error)?;
    let mut digest = Sha256::new();
    digest.update(b"bean-counter/m5/request/1\0");
    digest.update(&raw);
    let response = bytes(&json!({
        "schema":"ledger-billing-period-resolution-result/1","status":"resolved",
        "customer":customer,"period_id":{"term_version":term_version.to_string(),
            "period_index":period_index.to_string()},
        "boundary_resolution_id":resolution_id,"record_sequence":record_sequence.to_string(),
        "start_utc":start_at,"end_utc":end_at,
        "receipt":{"schema":"ledger-billing-m5-receipt/1",
            "command_sequence":command_sequence.to_string(),"accepted_at":accepted_at,
            "record_ids":[record_id],"request_hash":m5::hex(&digest.finalize())}
    }))?;
    let total = [&identity[..], &raw[..], &response[..], &payload[..]]
        .into_iter()
        .try_fold(0i64, |total, part| {
            total
                .checked_add(part.len() as i64)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))
        })?;
    Ok(total)
}

impl BillingLedger {
    pub async fn period_close(&self, raw: &[u8]) -> local::Result<Value> {
        self.period_close_inner(raw, None, false, PRODUCTION_CLOSE_LIMITS)
            .await
    }

    #[cfg(test)]
    pub(super) async fn period_close_at(
        &self,
        raw: &[u8],
        accepted: ledgerlab_core::domain::Timestamp,
    ) -> local::Result<Value> {
        self.period_close_inner(raw, Some(accepted), false, PRODUCTION_CLOSE_LIMITS)
            .await
    }

    #[cfg(test)]
    pub(super) async fn period_close_at_with_limits(
        &self,
        raw: &[u8],
        accepted: ledgerlab_core::domain::Timestamp,
        response: usize,
        payload: usize,
    ) -> local::Result<Value> {
        self.period_close_inner(
            raw,
            Some(accepted),
            false,
            CloseLimits { response, payload },
        )
        .await
    }

    async fn period_close_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<ledgerlab_core::domain::Timestamp>,
        resolved_missing_period: bool,
        limits: CloseLimits,
    ) -> local::Result<Value> {
        let request = term_service::parse_period_close_request(raw)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let term_version = i64::try_from(request.term_version)
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let period_index = i64::try_from(request.period_index)
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let identity = bytes(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin",
            "family":CLOSE,"customer":request.customer,"key_kind":"logical_period_id",
            "key":format!("{term_version}:{period_index}")
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(close_store_error)? {
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
        let preserve_accepted = accepted_override.is_some();
        let accepted = match accepted_override {
            Some(accepted) => accepted,
            None => local::now()?,
        };
        let state = tx
            .m5_period_close_state(&request.customer, term_version, period_index)
            .await
            .map_err(close_store_error)?;
        let (term, calculated_start_at_us, calculated_end_at_us) =
            close_period_bounds(&state, &request.customer, term_version, period_index)?;
        if state
            .successor_effective_at_us
            .is_some_and(|successor| calculated_start_at_us >= successor)
        {
            return Err(service::reject("BILLING_M5_PERIOD").into());
        }
        let (start_at_us, end_at_us) = match state.resolution.as_ref() {
            Some(resolution) => {
                if resolution.start_at_us != calculated_start_at_us
                    || resolution.end_at_us > calculated_end_at_us
                    || state
                        .successor_effective_at_us
                        .is_some_and(|successor| resolution.end_at_us > successor)
                {
                    return Err(ServiceError::IntegrityFailure.into());
                }
                (resolution.start_at_us, resolution.end_at_us)
            }
            None => {
                if resolved_missing_period
                    || state.successor_effective_at_us.is_some_and(|successor| {
                        calculated_start_at_us >= successor || calculated_end_at_us > successor
                    })
                {
                    return Err(service::reject("BILLING_M5_PERIOD").into());
                }
                (calculated_start_at_us, calculated_end_at_us)
            }
        };
        if accepted.micros() < end_at_us {
            return Err(service::reject("BILLING_M5_NOT_DUE").into());
        }
        if state.unsupported_assignment_count != 0 {
            return Err(service::reject("BILLING_M5_PERIOD").into());
        }
        let period_id = json!({
            "term_version":term_version.to_string(),"period_index":period_index.to_string()
        });
        let (lines, included_records, net) =
            per_work_lines(&request.customer, &period_id, &state.m3_assignments)?;
        if state.resolution.is_none() {
            let predicted_resolution_id =
                format!("boundary-resolution-{term_version}-{period_index}");
            let predicted_m5_high_water = state
                .m5_high_water
                .checked_add(1)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            let predicted_snapshot_boundary_id = state
                .snapshot_boundary_id
                .checked_add(1)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            let predicted_command_sequence = state
                .command_sequence
                .checked_add(1)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            let predicted_record_sequence = state
                .first_record_sequence
                .checked_add(1)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            let predicted = prepare_close_artifacts(
                &identity,
                &request.customer,
                &period_id,
                &term,
                &predicted_resolution_id,
                start_at_us,
                end_at_us,
                &accepted,
                &lines,
                &included_records,
                net,
                state.m3_high_water,
                predicted_m5_high_water,
                predicted_snapshot_boundary_id,
                predicted_record_sequence,
                predicted_command_sequence,
            )?;
            if predicted.response.len() > limits.response
                || predicted.payload.len() > limits.payload
            {
                return Err(service::reject("BILLING_M5_BOUNDS").into());
            }
            let resolution_bytes = period_resolution_append_bytes(
                &request.customer,
                term_version,
                period_index,
                start_at_us,
                end_at_us,
                &accepted,
                state.command_sequence,
                state.first_record_sequence,
            )?;
            let close_bytes = [&identity[..], raw, &predicted.response, &predicted.payload]
                .into_iter()
                .try_fold(0i64, |total, part| {
                    total
                        .checked_add(part.len() as i64)
                        .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))
                })?;
            let combined_bytes = resolution_bytes
                .checked_add(close_bytes)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            tx.m5_preflight_capacity(2, 2, combined_bytes)
                .await
                .map_err(close_store_error)?;
            tx.rollback().await.map_err(store_error)?;
            self.period_resolve_at(&request.customer, term_version, period_index, &accepted)
                .await?;
            let retry_accepted = preserve_accepted.then_some(accepted);
            return Box::pin(self.period_close_inner(raw, retry_accepted, true, limits)).await;
        }
        let resolution = state
            .resolution
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        let prepared = prepare_close_artifacts(
            &identity,
            &request.customer,
            &period_id,
            &term,
            &resolution.resolution_id,
            start_at_us,
            end_at_us,
            &accepted,
            &lines,
            &included_records,
            net,
            state.m3_high_water,
            state.m5_high_water,
            state.snapshot_boundary_id,
            state.first_record_sequence,
            state.command_sequence,
        )?;
        if prepared.response.len() > limits.response || prepared.payload.len() > limits.payload {
            return Err(service::reject("BILLING_M5_BOUNDS").into());
        }
        let child = Child {
            family: CLOSE_RECORD,
            customer: Some(&request.customer),
            source: None,
            child_key: &prepared.key,
            payload: &prepared.payload,
        };
        let command = Command {
            family: CLOSE,
            domain: "customer-admin",
            customer: Some(&request.customer),
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            request: raw,
            response: &prepared.response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_period_close(
            &command,
            &m5::PeriodCloseProjection {
                customer: &request.customer,
                term_version,
                period_index,
                boundary_resolution_id: &resolution.resolution_id,
                snapshot_boundary_id: state.snapshot_boundary_id,
                m3_high_water: state.m3_high_water,
                m5_high_water: state.m5_high_water,
                statement_hash: &prepared.statement_hash,
                statement_bytes: &prepared.response,
            },
        )
        .await
        .map_err(close_store_error)?;
        match tx.commit().await {
            Ok(()) => Ok(prepared.result),
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

    fn assert_close_bounds(lines: Vec<Value>, included_records: Vec<Value>) {
        let accepted =
            ledgerlab_core::domain::Timestamp::parse("2026-03-05T12:00:00.000000Z").unwrap();
        let result = prepare_close_artifacts(
            b"identity",
            "customer-1",
            &json!({"term_version":"1","period_index":"1"}),
            &json!({"timezone":"UTC","timezone_rules_version":"IANA-2025b","boundary_rule_version":"billing-boundary/1"}),
            "boundary-resolution-1-1",
            0,
            1,
            &accepted,
            &lines,
            &included_records,
            0,
            0,
            0,
            0,
            1,
            1,
        );
        assert!(matches!(
            result,
            Err(ServiceError::Rejection(code)) if code == "BILLING_M5_BOUNDS"
        ));
    }

    #[test]
    fn response_beyond_the_canonical_encoder_ceiling_is_a_bounds_refusal() {
        let lines = (0..4_200)
            .map(|index| json!({"index":index,"content":"x".repeat(1024)}))
            .collect();
        assert_close_bounds(lines, Vec::new());
    }

    #[test]
    fn child_beyond_the_canonical_encoder_ceiling_is_a_bounds_refusal() {
        let included = (0..4_200)
            .map(|index| json!({"index":index,"content":"x".repeat(1024)}))
            .collect();
        assert_close_bounds(Vec::new(), included);
    }
}
