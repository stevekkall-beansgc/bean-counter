use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::{
    canonical::CanonicalBytes,
    domain::{fiscal_calendar::FiscalCalendarConfig, Revision, Timestamp},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const FISCAL_SET: &str = "ledger-fiscal-calendar/1";
const FISCAL_VERSION: &str = "ledger-fiscal-calendar-version/1";
const FISCAL_REPORT: &str = "ledger-fiscal-report-request/1";
const FISCAL_REPORT_RUN: &str = "ledger-fiscal-report-run/1";
const MAX_FISCAL_ARTIFACT_BYTES: usize = 262_144;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FiscalSnapshotRequest {
    m3_high_water: String,
    m5_high_water: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FiscalReportRequest {
    schema: String,
    command_id: String,
    calendar_version: String,
    start: String,
    end: String,
    #[serde(default)]
    snapshot: Option<FiscalSnapshotRequest>,
}

fn bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))
}

fn output_bytes(value: &Value) -> Result<Vec<u8>, ServiceError> {
    let encoded = serde_json::to_vec(value).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
    if encoded.len() > MAX_FISCAL_ARTIFACT_BYTES {
        return Err(service::reject("BILLING_M5_BOUNDS"));
    }
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))
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

fn text(value: &Value) -> Result<&str, ServiceError> {
    value
        .as_str()
        .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))
}

fn atoms(value: &Value) -> Result<i128, ServiceError> {
    text(value)?
        .parse()
        .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))
}

fn scale_18(value: i128, scale: u64) -> Result<i128, ServiceError> {
    match scale {
        18 => Ok(value),
        2 => value
            .checked_mul(10_000_000_000_000_000)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS")),
        _ => Err(service::reject("BILLING_M5_INTEGRITY")),
    }
}

fn fiscal_line_id(view: &Value, mut line: Value) -> Result<Value, ServiceError> {
    let encoded = output_bytes(&json!({
        "view_kind":"fiscal","view_identity":view,"line":line
    }))?;
    let mut digest = Sha256::new();
    digest.update(b"bean-counter/m5/statement-line/1\0");
    digest.update(encoded);
    line["line_id"] = json!(m5::hex(&digest.finalize()));
    Ok(line)
}

fn fiscal_m3_lines(
    state: &m5::FiscalReportState,
    start_at_us: i64,
    view: &Value,
) -> Result<(Vec<Value>, Vec<Value>, i128), ServiceError> {
    let mut lines = Vec::new();
    let mut included = Vec::new();
    let mut net = 0i128;
    let mut outcomes = BTreeMap::<(String, String, String, String), (String, i128)>::new();
    let mut prior_ordinal = 0i64;
    for source in &state.sources {
        if source.ordinal <= prior_ordinal {
            return Err(service::reject("BILLING_M5_INTEGRITY"));
        }
        prior_ordinal = source.ordinal;
        let bundle = ledgerlab_core::canonical::parse_bounded(&source.bundle, 8 * 1024 * 1024)
            .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))?;
        let rows = bundle
            .as_array()
            .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
        if !rows
            .iter()
            .any(|row| row["kind"] == source.kind && row["id"] == source.id)
        {
            return Err(service::reject("BILLING_M5_INTEGRITY"));
        }
        let postings = rows
            .iter()
            .filter(|row| row["kind"] == "base-posting" || row["kind"] == "action")
            .collect::<Vec<_>>();
        if postings.is_empty() {
            return Err(service::reject("BILLING_M5_INTEGRITY"));
        }
        let agreement_id = source
            .agreement_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
        let agreement_version = source
            .agreement_version
            .filter(|version| *version > 0)
            .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
        let payer = text(&postings[0]["body"]["roles"]["payer"])?;
        let recipient = text(&postings[0]["body"]["roles"]["recipient"])?;
        let mut signed = 0i128;
        for posting in &postings {
            if posting["body"]["agreement_id"] != agreement_id
                || posting["body"]["roles"]["payer"] != payer
                || posting["body"]["roles"]["recipient"] != recipient
                || posting["body"]["amount"]["currency"] != "USD"
            {
                return Err(service::reject("BILLING_M5_INTEGRITY"));
            }
            let scale = posting["body"]["amount"]["scale"]
                .as_u64()
                .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
            signed = signed
                .checked_add(scale_18(
                    atoms(&posting["body"]["amount"]["atoms"])?,
                    scale,
                )?)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        }
        let source_record = json!({
            "customer":source.customer,"source":source.source,"kind":source.kind,"id":source.id
        });
        let is_base = source.kind == "base-acceptance";
        let calculation = if is_base {
            let event = rows
                .iter()
                .find(|row| row["kind"] == "event")
                .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
            let posting_scale = postings[0]["body"]["amount"]["scale"]
                .as_u64()
                .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
            if posting_scale == 18 {
                let quantity = text(&event["body"]["data"]["quantity"])?
                    .parse::<i128>()
                    .ok()
                    .filter(|quantity| *quantity > 0)
                    .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
                let unit = text(&event["body"]["data"]["unit"])?;
                if signed % quantity != 0 {
                    return Err(service::reject("BILLING_M5_INTEGRITY"));
                }
                json!({"kind":"per_work","exact_atoms_numerator":signed.to_string(),
                    "exact_atoms_denominator":"1","booked_atoms":signed.to_string(),"rounding":"none",
                    "operands":{"agreement_id":agreement_id,
                        "agreement_version":agreement_version.to_string(),"unit":unit,
                        "quantity":quantity.to_string(),"rate_atoms_per_unit":(signed / quantity).to_string()}})
            } else if posting_scale == 2 {
                let original = postings.iter().try_fold(0i128, |sum, posting| {
                    sum.checked_add(atoms(&posting["body"]["amount"]["atoms"])?)
                        .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))
                })?;
                json!({"kind":"fixed","exact_atoms_numerator":signed.to_string(),
                    "exact_atoms_denominator":"1","booked_atoms":signed.to_string(),"rounding":"none",
                    "operands":{"agreement_id":agreement_id,
                        "agreement_version":agreement_version.to_string(),
                        "fixed_atoms_scale_2":original.to_string(),
                        "usd_scale_18_multiplier":"10000000000000000"}})
            } else {
                return Err(service::reject("BILLING_M5_INTEGRITY"));
            }
        } else {
            let ingress = ledgerlab_core::canonical::parse_bounded(&source.ingress, 262_144)
                .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))?;
            let target = text(&ingress["target"])?;
            let family = text(&ingress["family"])?;
            let outcome_id = text(&ingress["id"])?;
            let revision = rows
                .iter()
                .filter(|row| row["kind"] == "claim-revision")
                .max_by_key(|row| {
                    row["body"]["number"]
                        .as_str()
                        .and_then(|number| number.parse::<u64>().ok())
                        .unwrap_or(0)
                })
                .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
            let current_scale = revision["body"]["amount"]["scale"]
                .as_u64()
                .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
            let accepted_atoms =
                scale_18(atoms(&revision["body"]["amount"]["atoms"])?, current_scale)?;
            let key = (
                source.customer.clone(),
                source.source.clone(),
                target.to_owned(),
                family.to_owned(),
            );
            let prior = outcomes.insert(key, (outcome_id.to_owned(), accepted_atoms));
            let mut operands = if ingress["schema"] == "ledger-billing-outcome/1"
                || ingress["schema"] == "ledger-billing-outcome/2"
            {
                if prior.is_some() {
                    return Err(service::reject("BILLING_M5_INTEGRITY"));
                }
                json!({"change_kind":"first","target_id":target,"outcome_id":outcome_id,
                    "accepted_atoms":accepted_atoms.to_string()})
            } else {
                let (prior_id, prior_atoms) =
                    prior.ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
                let change = if ingress["replacement"]["kind"] == "reverse" {
                    "reversal"
                } else {
                    "replacement"
                };
                json!({"change_kind":change,"target_id":target,"outcome_id":outcome_id,
                    "prior_outcome_id":prior_id,"prior_atoms":prior_atoms.to_string(),
                    "accepted_atoms":accepted_atoms.to_string()})
            };
            if operands["change_kind"] == "reversal" && accepted_atoms != 0 {
                return Err(service::reject("BILLING_M5_INTEGRITY"));
            }
            json!({"kind":"outcome_revision","exact_atoms_numerator":signed.to_string(),
                "exact_atoms_denominator":"1","booked_atoms":signed.to_string(),
                "rounding":"none","operands":operands.take()})
        };
        if source.accepted_at_us < start_at_us {
            continue;
        }
        net = net
            .checked_add(signed)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        included.push(source_record.clone());
        let basis = if is_base {
            calculation["kind"].clone()
        } else {
            json!("outcome_revision")
        };
        lines.push(fiscal_line_id(
            view,
            json!({"source_records":[source_record],"basis":basis,
                "agreement_id":agreement_id,"agreement_version":agreement_version.to_string(),
                "payer":payer,"recipient":recipient,"currency":"USD","scale":18,
                "amount_atoms":signed.to_string(),"calculation":calculation}),
        )?);
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

fn fiscal_m5_effects(
    state: &m5::FiscalReportState,
    view: &Value,
) -> Result<(Vec<Value>, Vec<Value>, Vec<Value>, i128), ServiceError> {
    let mut lines = Vec::new();
    let mut included = Vec::new();
    let mut quantities = Vec::new();
    let mut net = 0i128;
    let mut prior_ordinal = 0i64;
    for source in &state.m5_sources {
        if source.ordinal <= prior_ordinal || source.ordinal > state.m5_high_water {
            return Err(service::reject("BILLING_M5_INTEGRITY"));
        }
        prior_ordinal = source.ordinal;
        let payload = ledgerlab_core::canonical::parse_bounded(&source.payload, 262_144)
            .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))?;
        if payload["record"]["record_id"] != source.id
            || payload["record"]["sequence"] != source.ordinal.to_string()
            || Timestamp::parse(text(&payload["record"]["accepted_at"])?)
                .map(|accepted| accepted.micros())
                != Ok(source.accepted_at_us)
        {
            return Err(service::reject("BILLING_M5_INTEGRITY"));
        }
        match source.kind.as_str() {
            "ledger-billing-activity-record/1" => {
                let basis = source
                    .basis
                    .as_deref()
                    .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
                let basis = ledgerlab_core::canonical::parse_bounded(basis, 262_144)
                    .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))?;
                let customer = text(&payload["customer"])?;
                let application = text(&payload["source"])?;
                quantities.push(json!({"customer":customer,"source":application,
                    "kind":"activity","id":payload["activity_id"],
                    "unit":basis["source_unit"],"quantity":payload["quantity"],
                    "accepted_at":payload["record"]["accepted_at"],
                    "period_id":payload["period_id"]}));
                included.push(json!({"customer":customer,"source":application,
                    "kind":source.kind,"id":source.id}));
            }
            "ledger-billing-quantity-correction-record/1" => {
                let customer = text(&payload["customer"])?;
                let application = text(&payload["source"])?;
                quantities.push(json!({"customer":customer,"source":application,
                    "kind":"quantity-correction","id":payload["correction_id"],
                    "unit":payload["unit"],"quantity":payload["quantity_delta"],
                    "accepted_at":payload["record"]["accepted_at"],
                    "period_id":payload["assigned_period_id"]}));
                included.push(json!({"customer":customer,"source":application,
                    "kind":source.kind,"id":source.id}));
                if payload["mode"] == "per-work"
                    && payload["correction_route"] == "original-open-period"
                {
                    return Err(service::reject("BILLING_M5_INTEGRITY"));
                }
            }
            "ledger-billing-period-close-record/1" => {
                let customer = text(&payload["customer"])?;
                let retained_lines = payload["lines"]
                    .as_array()
                    .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
                for retained in retained_lines {
                    if retained["basis"] != "cumulative_close" {
                        continue;
                    }
                    let original_sources = retained["source_records"]
                        .as_array()
                        .filter(|sources| !sources.is_empty())
                        .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?;
                    let application = text(&original_sources[0]["source"])?;
                    if original_sources
                        .iter()
                        .any(|item| item["customer"] != customer || item["source"] != application)
                    {
                        return Err(service::reject("BILLING_M5_INTEGRITY"));
                    }
                    let source_record = json!({"customer":customer,"source":application,
                        "kind":source.kind,"id":source.id});
                    let mut line = retained.clone();
                    line.as_object_mut()
                        .ok_or_else(|| service::reject("BILLING_M5_INTEGRITY"))?
                        .remove("line_id");
                    line["source_records"] = json!([source_record.clone()]);
                    let amount = atoms(&line["amount_atoms"])?;
                    if line["calculation"]["booked_atoms"] != line["amount_atoms"] {
                        return Err(service::reject("BILLING_M5_INTEGRITY"));
                    }
                    net = net
                        .checked_add(amount)
                        .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                    included.push(source_record);
                    lines.push(fiscal_line_id(view, line)?);
                }
            }
            "ledger-billing-post-close-adjustment/1" => {
                return Err(service::reject("BILLING_M5_INTEGRITY"));
            }
            _ => return Err(service::reject("BILLING_M5_INTEGRITY")),
        }
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
    included.dedup();
    quantities.sort_by(|left, right| {
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
    Ok((lines, included, quantities, net))
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
        let enforce_clock = accepted_override.is_none();
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
            enforce_clock,
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

    pub async fn fiscal_report(&self, raw: &[u8]) -> local::Result<Value> {
        self.fiscal_report_inner(raw, None).await
    }

    #[cfg(test)]
    async fn fiscal_report_at(&self, raw: &[u8], accepted: Timestamp) -> local::Result<Value> {
        self.fiscal_report_inner(raw, Some(accepted)).await
    }

    async fn fiscal_report_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<Timestamp>,
    ) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse(raw)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let request: FiscalReportRequest = serde_json::from_value(value.clone())
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let calendar_version = Revision::parse(&request.calendar_version)
            .ok()
            .filter(|version| version.value() > 0)
            .and_then(|version| i64::try_from(version.value()).ok())
            .ok_or_else(|| service::reject("BILLING_M5_REQUEST"))?;
        let start =
            Timestamp::parse(&request.start).map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let end =
            Timestamp::parse(&request.end).map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let requested_snapshot = request
            .snapshot
            .as_ref()
            .map(|snapshot| {
                let m3 = Revision::parse(&snapshot.m3_high_water)
                    .ok()
                    .and_then(|revision| i64::try_from(revision.value()).ok())
                    .ok_or_else(|| service::reject("BILLING_M5_REQUEST"))?;
                let m5 = Revision::parse(&snapshot.m5_high_water)
                    .ok()
                    .and_then(|revision| i64::try_from(revision.value()).ok())
                    .ok_or_else(|| service::reject("BILLING_M5_REQUEST"))?;
                Ok::<_, ServiceError>((m3, m5))
            })
            .transpose()?;
        if request.schema != FISCAL_REPORT
            || !valid_id(&request.command_id)
            || end.micros() <= start.micros()
            || value
                .get("snapshot")
                .is_some_and(|snapshot| !snapshot.is_object())
        {
            return Err(service::reject("BILLING_M5_REQUEST").into());
        }
        let identity = bytes(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"installation-admin",
            "family":FISCAL_REPORT,"key_kind":"command_id","key":request.command_id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::REPORT_BUDGET)
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
        let state = tx
            .m5_fiscal_report_state(
                calendar_version,
                requested_snapshot,
                start.micros(),
                end.micros(),
            )
            .await
            .map_err(fiscal_store_error)?;
        let calendar = serde_json::from_slice(&state.calendar_bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let config = FiscalCalendarConfig {
            timezone: state.timezone.clone(),
            timezone_rules_version: state.timezone_rules_version.clone(),
            calendar,
        };
        if config
            .period_for_micros(start.micros())
            .map(|period| period.start.timestamp_micros())
            != Ok(start.micros())
            || config
                .period_for_micros(end.micros())
                .map(|period| period.start.timestamp_micros())
                != Ok(end.micros())
        {
            return Err(service::reject("BILLING_M5_PERIOD").into());
        }
        let start_utc = start.as_str();
        let end_utc = end.as_str();
        let view = json!({
            "calendar_version":calendar_version.to_string(),"start_utc":start_utc,
            "end_utc":end_utc,"m3_high_water":state.m3_high_water.to_string(),
            "m5_high_water":state.m5_high_water.to_string()
        });
        let (mut monetary_lines, mut included_records, mut net_atoms) =
            fiscal_m3_lines(&state, start.micros(), &view)?;
        let (m5_lines, m5_records, nonmonetary_quantities, m5_net) =
            fiscal_m5_effects(&state, &view)?;
        monetary_lines.extend(m5_lines);
        monetary_lines
            .sort_by(|left, right| left["line_id"].as_str().cmp(&right["line_id"].as_str()));
        included_records.extend(m5_records);
        included_records.sort_by(|left, right| {
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
        included_records.dedup();
        net_atoms = net_atoms
            .checked_add(m5_net)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let mut result = json!({
            "schema":"ledger-fiscal-report/1","status":"complete",
            "calendar_version":calendar_version.to_string(),"timezone":state.timezone,
            "timezone_rules_version":state.timezone_rules_version,
            "start_utc":start_utc,"end_utc":end_utc,
            "m3_high_water":state.m3_high_water.to_string(),
            "m5_high_water":state.m5_high_water.to_string(),
            "monetary_lines":monetary_lines,"nonmonetary_quantities":nonmonetary_quantities,
            "net_atoms":net_atoms.to_string(),"currency":"USD","scale":18,"complete":true,
            "snapshot_boundary_id":state.snapshot_boundary_id.to_string()
        });
        let mut report_digest = Sha256::new();
        report_digest.update(b"bean-counter/m5/fiscal-report/1\0");
        report_digest.update(output_bytes(&result)?);
        let report_hash = m5::hex(&report_digest.finalize());
        result["report_hash"] = json!(report_hash);
        let response = output_bytes(&result)?;
        let enforce_clock = accepted_override.is_none();
        let accepted = match accepted_override {
            Some(accepted) => accepted,
            None => local::now()?,
        };
        let accepted_at = accepted.as_str();
        let key = bytes(&json!({
            "role":"fiscal-report","kind":FISCAL_REPORT_RUN,
            "key":{"command_id":request.command_id}
        }))?;
        let record_id = m5::record_id(&identity, &key);
        let mut payload = json!({
            "schema":FISCAL_REPORT_RUN,"calendar_version":calendar_version.to_string(),
            "timezone":config.timezone,"timezone_rules_version":config.timezone_rules_version,
            "start_utc":start_utc,"end_utc":end_utc,
            "m3_high_water":state.m3_high_water.to_string(),
            "m5_high_water":state.m5_high_water.to_string(),
            "included_records":included_records,"monetary_lines":result["monetary_lines"],
            "nonmonetary_quantities":result["nonmonetary_quantities"],
            "net_atoms":net_atoms.to_string(),"report_hash":report_hash,
            "snapshot_boundary_id":state.snapshot_boundary_id.to_string(),
            "record":{"record_id":record_id,"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}
        });
        let payload =
            m5::seal_child(FISCAL_REPORT_RUN, &mut payload).map_err(fiscal_store_error)?;
        let child = Child {
            family: FISCAL_REPORT_RUN,
            customer: None,
            source: None,
            child_key: &key,
            payload: &payload,
        };
        let command = Command {
            family: FISCAL_REPORT,
            domain: "installation-admin",
            customer: None,
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock,
            request: raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_fiscal_report(
            &command,
            &m5::FiscalReportProjection {
                report_id: &request.command_id,
                calendar_version,
                m3_high_water: state.m3_high_water,
                m5_high_water: state.m5_high_water,
                snapshot_boundary_id: state.snapshot_boundary_id,
                report_hash: &report_hash,
                report_bytes: &response,
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

    fn utc_year_calendar(change_id: &str) -> Vec<u8> {
        CanonicalBytes::from_value(&json!({
            "schema":"ledger-fiscal-calendar/1","change_id":change_id,
            "expected_revision":"0","timezone":"UTC",
            "timezone_rules_version":"IANA-2025b",
            "calendar":{"kind":"gregorian_years","fiscal_year_start_month":1,
                "fiscal_year_start_day":1}
        }))
        .unwrap()
        .into_vec()
    }

    fn report(command_id: &str, start: &str, end: &str) -> Vec<u8> {
        CanonicalBytes::from_value(&json!({
            "schema":"ledger-fiscal-report-request/1","command_id":command_id,
            "calendar_version":"1","start":start,"end":end
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

    #[tokio::test]
    async fn fiscal_report_pins_the_pre_run_snapshot_and_replays_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger
            .fiscal_set_at(
                &utc_year_calendar("fiscal-year"),
                Timestamp::parse("2026-01-01T00:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let term = CanonicalBytes::from_value(&json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"fiscal-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-01-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-01-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp",
                "boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }))
        .unwrap()
        .into_vec();
        ledger
            .term_set_at(
                &term,
                Timestamp::parse("2026-09-02T00:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let accepted_work = ledger
            .accept_at(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../../examples/billing/event.json"),
                Timestamp::parse("2026-09-15T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let target = accepted_work["receipt"]["body"]["target"].as_str().unwrap();
        let mut outcome: Value =
            serde_json::from_slice(include_bytes!("../../../../examples/billing/outcome.json"))
                .unwrap();
        outcome["target"] = json!(target);
        let outcome = CanonicalBytes::from_value(&outcome).unwrap().into_vec();
        ledger
            .outcome_at(
                "customer-1",
                "urn:example:work",
                &outcome,
                Timestamp::parse("2026-09-23T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let mut correction: Value = serde_json::from_slice(include_bytes!(
            "../../../../examples/billing/correction.json"
        ))
        .unwrap();
        correction["target"] = json!(target);
        let correction = CanonicalBytes::from_value(&correction).unwrap().into_vec();
        ledger
            .correct_at(
                "customer-1",
                "urn:example:work",
                &correction,
                Timestamp::parse("2026-09-24T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let request = report(
            "fiscal-report-1",
            "2026-01-01T00:00:00.000000Z",
            "2027-01-01T00:00:00.000000Z",
        );
        let result = ledger
            .fiscal_report_at(
                &request,
                Timestamp::parse("2027-01-02T09:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["monetary_lines"].as_array().unwrap().len(), 3);
        let outcome_lines = result["monetary_lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|line| line["basis"] == "outcome_revision")
            .collect::<Vec<_>>();
        assert_eq!(outcome_lines.len(), 2);
        assert!(outcome_lines
            .iter()
            .any(|line| { line["calculation"]["operands"]["change_kind"] == "first" }));
        assert!(outcome_lines.iter().any(|line| {
            line["calculation"]["operands"]["change_kind"] == "replacement"
                && line["calculation"]["operands"]
                    .get("prior_outcome_id")
                    .is_some()
        }));
        let summed = result["monetary_lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                line["amount_atoms"]
                    .as_str()
                    .unwrap()
                    .parse::<i128>()
                    .unwrap()
            })
            .sum::<i128>();
        assert_eq!(result["net_atoms"], summed.to_string());
        assert_eq!(ledger.fiscal_report(&request).await.unwrap(), result);
        assert!(matches!(
            ledger
                .fiscal_report(&report(
                    "not-a-boundary",
                    "2026-01-02T00:00:00.000000Z",
                    "2027-01-01T00:00:00.000000Z"
                ))
                .await,
            Err(local::LocalError::Service(ServiceError::Rejection(code)))
                if code == "BILLING_M5_PERIOD"
        ));
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
        let row: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT count(*),min(m5_high_water),max(record_sequence),count(DISTINCT report_hash) FROM billing_m5_fiscal_reports",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(row.0, 1);
        assert!(row.2 > row.1);
        assert_eq!(row.3, 1);
        conn.close().await.unwrap();
    }

    #[test]
    fn fiscal_report_hashes_match_the_frozen_golden() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../../contracts/candidates/billing-lifecycle-m5/vectors/m5-command-goldens.json"
        ))
        .unwrap();
        let command = &oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "fiscal-report-snapshot-replay")
            .unwrap()["command"];
        let mut line = command["result"]["monetary_lines"][0].clone();
        let expected_line_id = line["line_id"].clone();
        line.as_object_mut().unwrap().remove("line_id");
        let view = json!({
            "calendar_version":"3","start_utc":"2026-01-01T00:00:00.000000Z",
            "end_utc":"2027-01-01T00:00:00.000000Z","m3_high_water":"70",
            "m5_high_water":"40"
        });
        assert_eq!(
            fiscal_line_id(&view, line).unwrap()["line_id"],
            expected_line_id
        );
        let mut result = command["result"].clone();
        let expected_report_hash = result["report_hash"].as_str().unwrap().to_owned();
        result.as_object_mut().unwrap().remove("report_hash");
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/fiscal-report/1\0");
        digest.update(output_bytes(&result).unwrap());
        assert_eq!(m5::hex(&digest.finalize()), expected_report_hash);
    }
}
