//! Exact arithmetic for the period-based cumulative billing mode.
use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::canonical::CanonicalBytes;
use ledgerlab_core::domain::Timestamp;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Basis {
    pub mode: String,
    pub source_unit: String,
    pub billable_unit: String,
    pub conversion_numerator: String,
    pub conversion_denominator: String,
    pub rate_usd_per_billable_unit: String,
    pub maximum_period_quantity: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExactBooking {
    pub numerator: String,
    pub denominator: String,
    pub booked_atoms: i128,
}

fn positive(value: &str) -> Result<i128, ServiceError> {
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(service::reject("BILLING_M5_REQUEST"));
    }
    value
        .parse::<i128>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(super) fn rate_atoms(rate: &str) -> Result<i128, ServiceError> {
    let (whole, fraction) = rate.split_once('.').unwrap_or((rate, ""));
    if whole.is_empty()
        || (whole.len() > 1 && whole.starts_with('0'))
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 18
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (rate.contains('.') && fraction.is_empty())
    {
        return Err(service::reject("BILLING_M5_REQUEST"));
    }
    let atoms = ledgerlab_core::money::Decimal::parse(rate)
        .and_then(|decimal| decimal.atoms_exact(18))
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
    Some(atoms)
        .filter(|n| *n > 0)
        .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))
}

impl Basis {
    pub(super) fn validate(&self) -> Result<(), ServiceError> {
        service::require(
            self.mode == "cumulative_period"
                && !self.source_unit.is_empty()
                && self.source_unit.len() <= 128
                && !self.billable_unit.is_empty()
                && self.billable_unit.len() <= 128,
            "BILLING_M5_REQUEST",
        )?;
        let numerator = positive(&self.conversion_numerator)?;
        let denominator = positive(&self.conversion_denominator)?;
        let _ = gcd(numerator, denominator);
        let _ = rate_atoms(&self.rate_usd_per_billable_unit)?;
        let maximum = positive(&self.maximum_period_quantity)?;
        let _ = self.book(maximum)?;
        Ok(())
    }

    fn normalized(&self) -> Result<Self, ServiceError> {
        self.validate()?;
        let numerator = positive(&self.conversion_numerator)?;
        let denominator = positive(&self.conversion_denominator)?;
        let divisor = gcd(numerator, denominator);
        let mut normalized = self.clone();
        normalized.conversion_numerator = (numerator / divisor).to_string();
        normalized.conversion_denominator = (denominator / divisor).to_string();
        Ok(normalized)
    }

    pub(super) fn maximum(&self) -> Result<i128, ServiceError> {
        positive(&self.maximum_period_quantity)
    }

    pub(super) fn book(&self, quantity: i128) -> Result<ExactBooking, ServiceError> {
        service::require(quantity >= 0, "BILLING_M5_QUANTITY")?;
        let numerator = positive(&self.conversion_numerator)?;
        let denominator = positive(&self.conversion_denominator)?;
        let rate = rate_atoms(&self.rate_usd_per_billable_unit)?;
        let factor = |n: i128| {
            ledgerlab_core::money::ExactRatio::from_canonical(&n.to_string(), "1")
                .map_err(|_| service::reject("BILLING_M5_BOUNDS"))
        };
        let quantity_ratio = factor(quantity)?;
        let numerator_ratio = factor(numerator)?;
        let rate_ratio = factor(rate)?;
        let denominator_ratio = factor(denominator)?;
        let exact = quantity_ratio
            .mul(&numerator_ratio)
            .and_then(|value| value.mul(&rate_ratio))
            .and_then(|value| value.div(&denominator_ratio))
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let booked_atoms = exact
            .round_atoms()
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let value = serde_json::to_value(&exact).map_err(|_| ServiceError::IntegrityFailure)?;
        let exact_numerator = value["numerator"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .to_owned();
        let exact_denominator = value["denominator"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .to_owned();
        Ok(ExactBooking {
            numerator: exact_numerator,
            denominator: exact_denominator,
            booked_atoms,
        })
    }
}

pub(super) fn canonical(value: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(value)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))
}

fn store_error(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => service::reject("BILLING_M5_BOUNDS"),
        StoreError::BillingPeriod => service::reject("BILLING_M5_PERIOD"),
        StoreError::BillingUpgradeRequired => service::reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => {
            service::reject("BILLING_M5_INTEGRITY")
        }
        other => crate::service::store_error(other),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasisSetup {
    schema: String,
    customer: String,
    source: String,
    change_id: String,
    expected_revision: String,
    agreement_id: String,
    agreement_version: String,
    effective_at: String,
    basis: Basis,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activity {
    schema: String,
    customer: String,
    source: String,
    id: String,
    operation_id: String,
    target: String,
    quantity: String,
    occurred_at: String,
    evidence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuantityCorrection {
    schema: String,
    customer: String,
    source: String,
    id: String,
    target: String,
    quantity_delta: String,
    occurred_at: String,
    evidence: String,
}

fn signed_nonzero(value: &str) -> Result<i128, ServiceError> {
    let parsed = value
        .parse::<i128>()
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    service::require(
        parsed != 0 && value == parsed.to_string(),
        "BILLING_M5_REQUEST",
    )?;
    Ok(parsed)
}

fn named(value: &str, limit: usize) -> Result<(), ServiceError> {
    service::require(
        !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control),
        "BILLING_M5_REQUEST",
    )
}

fn parse_request<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T, ServiceError> {
    let value = ledgerlab_core::canonical::parse_bounded(raw, 262_144)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    serde_json::from_value(value).map_err(|_| service::reject("BILLING_M5_REQUEST"))
}

fn receipt(command: i64, record_ids: &[String], accepted: &Timestamp, raw: &[u8]) -> Value {
    json!({
        "schema":"ledger-billing-m5-receipt/1",
        "command_sequence":command.to_string(),"accepted_at":accepted.as_str(),
        "record_ids":record_ids,
        "request_hash":m5::hex(&m5::hash(b"bean-counter/m5/request/1\0",raw))
    })
}

fn retained_result(bytes: &[u8]) -> Result<Value, ServiceError> {
    ledgerlab_core::canonical::parse_bounded(bytes, 262_144)
        .map_err(|_| ServiceError::IntegrityFailure)
}

fn accepted_commit(result: Value, outcome: Result<(), CommitError>) -> local::Result<Value> {
    match outcome {
        Ok(()) => Ok(result),
        Err(CommitError::RolledBack(error)) => Err(crate::service::store_error(error).into()),
        Err(CommitError::OutcomeUnknown) => {
            Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN").into())
        }
    }
}

struct PerWorkFacts {
    quantity: i128,
    maximum: i128,
    unit: String,
    rate: i128,
    payer: String,
    recipient: String,
}

fn per_work_facts(target: &m5::PerWorkTarget) -> Result<PerWorkFacts, ServiceError> {
    let bundle = ledgerlab_core::canonical::parse_bounded(&target.bundle, 8 * 1024 * 1024)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let rows = bundle.as_array().ok_or(ServiceError::IntegrityFailure)?;
    let event = rows
        .iter()
        .find(|row| row["kind"] == "event")
        .ok_or(ServiceError::IntegrityFailure)?;
    let quantity = event["body"]["data"]["quantity"]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?
        .parse::<i128>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let unit = event["body"]["data"]["unit"]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?
        .to_owned();
    let postings = rows
        .iter()
        .filter(|row| row["kind"] == "base-posting")
        .collect::<Vec<_>>();
    let first = postings.first().ok_or(ServiceError::IntegrityFailure)?;
    let payer = first["body"]["roles"]["payer"]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?
        .to_owned();
    let recipient = first["body"]["roles"]["recipient"]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?
        .to_owned();
    let mut booked = 0i128;
    for posting in postings {
        if posting["body"]["amount"]["currency"] != "USD"
            || posting["body"]["amount"]["scale"] != 18
            || posting["body"]["roles"]["payer"] != payer
            || posting["body"]["roles"]["recipient"] != recipient
            || posting["body"]["agreement_id"] != target.agreement_id
        {
            return Err(ServiceError::IntegrityFailure);
        }
        booked = booked
            .checked_add(
                posting["body"]["amount"]["atoms"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .parse::<i128>()
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            )
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
    }
    service::require(
        quantity > 0 && booked % quantity == 0,
        "BILLING_M5_QUANTITY",
    )?;
    let setup = ledgerlab_core::canonical::parse_bounded(&target.setup, 262_144)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let maximum = setup["maximum_quantity"]
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)?
        .parse::<i128>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    service::require(
        setup["unit"] == unit && maximum >= quantity,
        "BILLING_M5_QUANTITY",
    )?;
    Ok(PerWorkFacts {
        quantity,
        maximum,
        unit,
        rate: booked / quantity,
        payer,
        recipient,
    })
}

fn adjustment_id(identity: &[u8]) -> String {
    format!(
        "m5a_{}",
        m5::hex(&m5::hash(b"bean-counter/m5/adjustment-id/1\0", identity))
    )
}

async fn require_new_time(
    tx: &mut SqliteTx,
    snapshot: &crate::store::sqlite::BillingSnapshot,
    at: &Timestamp,
) -> Result<(), ServiceError> {
    let mut maximum = snapshot.cross_stream_time_max;
    for time in snapshot
        .agreements
        .iter()
        .map(|row| row.recorded_at_us)
        .chain(snapshot.controls.iter().map(|row| row.recorded_at_us))
        .chain(
            snapshot
                .scoped_permissions
                .iter()
                .map(|row| row.recorded_at_us),
        )
    {
        maximum = Some(maximum.map_or(time, |prior| prior.max(time)));
    }
    if let Some(m5) = tx
        .m5_cumulative_last_accepted()
        .await
        .map_err(store_error)?
    {
        maximum = Some(maximum.map_or(m5, |prior| prior.max(m5)));
    }
    service::require(
        maximum.is_none_or(|prior| at.micros() > prior),
        "BILLING_CLOCK_NOT_ADVANCED",
    )
}

impl BillingLedger {
    /// Pin a cumulative conversion basis to one already accepted M2 agreement version.
    pub async fn cumulative_agreement_set(&self, raw: &[u8]) -> local::Result<Value> {
        self.cumulative_agreement_set_inner(raw, None).await
    }

    #[cfg(test)]
    pub(super) async fn cumulative_agreement_set_at(
        &self,
        raw: &[u8],
        accepted: Timestamp,
    ) -> local::Result<Value> {
        self.cumulative_agreement_set_inner(raw, Some(accepted))
            .await
    }

    async fn cumulative_agreement_set_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<Timestamp>,
    ) -> local::Result<Value> {
        let request: BasisSetup = parse_request(raw)?;
        service::require(
            request.schema == "ledger-billing-cumulative-agreement/1",
            "BILLING_M5_REQUEST",
        )?;
        named(&request.customer, 128)?;
        named(&request.source, 256)?;
        named(&request.change_id, 128)?;
        named(&request.agreement_id, 128)?;
        let basis = request.basis.normalized()?;
        let agreement_version = positive(&request.agreement_version)?;
        let agreement_version =
            i64::try_from(agreement_version).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let expected = request
            .expected_revision
            .parse::<i64>()
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        service::require(
            expected >= 0 && request.expected_revision == expected.to_string(),
            "BILLING_M5_REQUEST",
        )?;
        let effective = Timestamp::parse(&request.effective_at)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let identity = canonical(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin",
            "family":"ledger-billing-cumulative-agreement/1","customer":request.customer,
            "source":request.source,"key_kind":"change_id","key":request.change_id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(crate::service::store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error)? {
            if saved.request != raw {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = retained_result(&saved.response)?;
            tx.rollback().await.map_err(crate::service::store_error)?;
            return Ok(result);
        }
        let snapshot = tx
            .billing_snapshot()
            .await
            .map_err(crate::service::store_error)?;
        service::validate_snapshot(&snapshot)?;
        let active = service::control::selected_terms(
            &snapshot,
            &request.customer,
            &request.source,
            &effective,
        )?
        .ok_or_else(|| service::reject("BILLING_M5_AGREEMENT"))?;
        service::require(
            active.0.agreement == request.agreement_id && active.1 == agreement_version,
            "BILLING_M5_AGREEMENT",
        )?;
        let revision = tx
            .m5_cumulative_basis_revision(
                &request.customer,
                &request.source,
                &request.agreement_id,
                agreement_version,
            )
            .await
            .map_err(store_error)?;
        service::require(revision == expected, "BILLING_M5_STALE_REVISION")?;
        if let Some(previous) = tx
            .m5_cumulative_latest_basis_effective(
                &request.customer,
                &request.source,
                &request.agreement_id,
                agreement_version,
            )
            .await
            .map_err(store_error)?
        {
            service::require(effective.micros() > previous, "BILLING_M5_EFFECTIVE_TIME")?;
        }
        let enforce_clock = accepted_override.is_none();
        let accepted = accepted_override.unwrap_or(local::now()?);
        require_new_time(&mut tx, &snapshot, &accepted).await?;
        let sequences = tx.m5_cumulative_sequences().await.map_err(store_error)?;
        let basis_version = revision
            .checked_add(1)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let basis_value = json!({
            "mode":"cumulative_period","source_unit":basis.source_unit,
            "billable_unit":basis.billable_unit,
            "conversion_numerator":basis.conversion_numerator,
            "conversion_denominator":basis.conversion_denominator,
            "rate_usd_per_billable_unit":basis.rate_usd_per_billable_unit,
            "maximum_period_quantity":basis.maximum_period_quantity
        });
        let basis_bytes = canonical(&basis_value)?;
        let key = canonical(&json!({
            "role":"cumulative-basis-version","customer":request.customer,"source":request.source,
            "kind":"ledger-billing-cumulative-basis-version/1",
            "key":{"agreement_id":request.agreement_id,"agreement_version":request.agreement_version,"basis_version":basis_version.to_string()}
        }))?;
        let id = m5::record_id(&identity, &key);
        let mut payload = json!({
            "schema":"ledger-billing-cumulative-basis-version/1","customer":request.customer,
            "source":request.source,"agreement_id":request.agreement_id,
            "agreement_version":request.agreement_version,"basis_version":basis_version.to_string(),
            "effective_at":request.effective_at,"basis":basis_value,
            "record":{"record_id":id,"sequence":sequences.record.to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":sequences.command.to_string()}
        });
        let payload = m5::seal_child("ledger-billing-cumulative-basis-version/1", &mut payload)
            .map_err(store_error)?;
        let result = json!({
            "schema":"ledger-billing-cumulative-agreement-result/1","status":"basis_updated",
            "revision":basis_version.to_string(),"agreement_id":request.agreement_id,
            "agreement_version":request.agreement_version,"basis_version":basis_version.to_string(),
            "receipt":receipt(sequences.command,&[id],&accepted,raw)
        });
        let response = canonical(&result)?;
        let child = Child {
            family: "ledger-billing-cumulative-basis-version/1",
            customer: Some(&request.customer),
            source: Some(&request.source),
            child_key: &key,
            payload: &payload,
        };
        let command = Command {
            family: "ledger-billing-cumulative-agreement/1",
            domain: "customer-admin",
            customer: Some(&request.customer),
            source: Some(&request.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock,
            request: raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_cumulative_basis(
            &command,
            &request.agreement_id,
            agreement_version,
            basis_version,
            effective.micros(),
            &basis_bytes,
        )
        .await
        .map_err(store_error)?;
        accepted_commit(result, tx.commit().await)
    }

    /// Retain additive activity once per semantic operation, with permanent delivery aliases.
    pub async fn activity_submit(&self, raw: &[u8]) -> local::Result<Value> {
        self.activity_submit_inner(raw, None).await
    }

    #[cfg(test)]
    pub(super) async fn activity_submit_at(
        &self,
        raw: &[u8],
        accepted: Timestamp,
    ) -> local::Result<Value> {
        self.activity_submit_inner(raw, Some(accepted)).await
    }

    async fn activity_submit_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<Timestamp>,
    ) -> local::Result<Value> {
        let request: Activity = parse_request(raw)?;
        service::require(
            request.schema == "ledger-billing-activity/1",
            "BILLING_M5_REQUEST",
        )?;
        named(&request.customer, 128)?;
        named(&request.source, 256)?;
        named(&request.id, 128)?;
        named(&request.operation_id, 128)?;
        named(&request.target, 128)?;
        service::require(
            !request.evidence.is_empty() && request.evidence.len() <= 8192,
            "BILLING_M5_REQUEST",
        )?;
        let quantity = positive(&request.quantity)?;
        let occurred = Timestamp::parse(&request.occurred_at)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let value = ledgerlab_core::canonical::parse_bounded(raw, 262_144)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let ingress = canonical(&value)?;
        let facts = canonical(&json!({
            "schema":"ledger-billing-activity-facts/1","customer":request.customer,
            "target":request.target,"quantity":request.quantity,
            "occurred_at":request.occurred_at,"evidence":request.evidence
        }))?;
        let identity = canonical(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"application",
            "family":"ledger-billing-activity/1","customer":request.customer,"source":request.source,
            "key_kind":"delivery_id","key":request.id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(crate::service::store_error)?;
        let snapshot = tx
            .billing_snapshot()
            .await
            .map_err(crate::service::store_error)?;
        service::validate_snapshot(&snapshot)?;
        let authority =
            service::permissions::effective_for(&snapshot, &request.customer, &request.source)?;
        service::require(
            authority.permissions.iter().any(|p| p == "read"),
            "BILLING_UNAUTHORIZED",
        )?;
        if let Some(saved) = tx
            .m5_activity_delivery(&request.customer, &request.source, &request.id)
            .await
            .map_err(store_error)?
        {
            let original = tx
                .m5_lookup(&saved.identity_key)
                .await
                .map_err(store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            service::require(original.response == saved.response, "BILLING_M5_INTEGRITY")?;
            if saved.retained_bytes != ingress {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = retained_result(&saved.response)?;
            tx.rollback().await.map_err(crate::service::store_error)?;
            return Ok(result);
        }
        if let Some(saved) = tx
            .m5_activity_semantic(&request.customer, &request.source, &request.operation_id)
            .await
            .map_err(store_error)?
        {
            let original = tx
                .m5_lookup(&saved.identity_key)
                .await
                .map_err(store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            service::require(original.response == saved.response, "BILLING_M5_INTEGRITY")?;
            if saved.retained_bytes != facts {
                return Err(service::reject("SEMANTIC_CONFLICT").into());
            }
            service::require(
                authority.permissions.iter().any(|p| p == "submit"),
                "BILLING_PERMISSION",
            )?;
            tx.m5_append_activity_alias(
                &request.customer,
                &request.source,
                &request.id,
                &ingress,
                &saved.identity_key,
            )
            .await
            .map_err(store_error)?;
            let result = retained_result(&saved.response)?;
            return accepted_commit(result, tx.commit().await);
        }
        service::require(
            authority.permissions.iter().any(|p| p == "submit"),
            "BILLING_PERMISSION",
        )?;
        let enforce_clock = accepted_override.is_none();
        let accepted = accepted_override.unwrap_or(local::now()?);
        service::require(occurred.micros() <= accepted.micros(), "BILLING_M5_REQUEST")?;
        require_new_time(&mut tx, &snapshot, &accepted).await?;
        let (agreement, agreement_version) = service::control::selected_terms(
            &snapshot,
            &request.customer,
            &request.source,
            &accepted,
        )?
        .ok_or_else(|| service::reject("BILLING_M5_AGREEMENT"))?;
        let basis = tx
            .m5_cumulative_basis_at(
                &request.customer,
                &request.source,
                &agreement.agreement,
                agreement_version,
                accepted.micros(),
            )
            .await
            .map_err(store_error)?
            .ok_or_else(|| service::reject("BILLING_M5_BASIS"))?;
        service::require(
            basis.effective_at_us <= accepted.micros(),
            "BILLING_M5_INTEGRITY",
        )?;
        let basis_value = retained_result(&basis.basis_bytes)?;
        let basis_parsed: Basis =
            serde_json::from_value(basis_value).map_err(|_| ServiceError::IntegrityFailure)?;
        basis_parsed.validate()?;
        service::require(quantity <= basis_parsed.maximum()?, "BILLING_M5_QUANTITY")?;
        let (term_version, period_index) = tx
            .m5_cumulative_period_at(&request.customer, accepted.micros())
            .await
            .map_err(store_error)?;
        let sequences = tx.m5_cumulative_sequences().await.map_err(store_error)?;
        let rows = tx
            .m5_cumulative_period_records(
                &request.customer,
                term_version,
                period_index,
                sequences.record - 1,
            )
            .await
            .map_err(store_error)?;
        let aggregate = bucket_quantity(
            &rows,
            &request.source,
            &agreement.agreement,
            agreement_version,
            basis.version,
        )?;
        let revised = aggregate
            .checked_add(quantity)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        service::require(revised <= basis_parsed.maximum()?, "BILLING_M5_QUANTITY")?;
        let key = canonical(&json!({
            "role":"cumulative-activity","customer":request.customer,"source":request.source,
            "kind":"ledger-billing-activity-record/1","key":{"activity_id":request.id}
        }))?;
        let id = m5::record_id(&identity, &key);
        let mut payload = json!({
            "schema":"ledger-billing-activity-record/1","customer":request.customer,
            "source":request.source,"activity_id":request.id,"operation_id":request.operation_id,
            "target":request.target,"quantity":request.quantity,"occurred_at":request.occurred_at,
            "agreement_id":agreement.agreement,"agreement_version":agreement_version.to_string(),
            "basis_version":basis.version.to_string(),
            "period_id":{"term_version":term_version.to_string(),"period_index":period_index.to_string()},
            "record":{"record_id":id,"sequence":sequences.record.to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":sequences.command.to_string()}
        });
        let payload = m5::seal_child("ledger-billing-activity-record/1", &mut payload)
            .map_err(store_error)?;
        let result =
            json!({"status":"accepted","receipt":receipt(sequences.command,&[id],&accepted,raw)});
        let response = canonical(&result)?;
        let child = Child {
            family: "ledger-billing-activity-record/1",
            customer: Some(&request.customer),
            source: Some(&request.source),
            child_key: &key,
            payload: &payload,
        };
        let command = Command {
            family: "ledger-billing-activity/1",
            domain: "application",
            customer: Some(&request.customer),
            source: Some(&request.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock,
            request: raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_activity(
            &command,
            &request.operation_id,
            &request.id,
            &facts,
            &ingress,
            term_version,
            period_index,
        )
        .await
        .map_err(store_error)?;
        accepted_commit(result, tx.commit().await)
    }

    pub async fn quantity_correct(&self, raw: &[u8]) -> local::Result<Value> {
        self.quantity_correct_inner(raw, None).await
    }

    #[cfg(test)]
    pub(super) async fn quantity_correct_at(
        &self,
        raw: &[u8],
        accepted: Timestamp,
    ) -> local::Result<Value> {
        self.quantity_correct_inner(raw, Some(accepted)).await
    }

    async fn quantity_correct_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<Timestamp>,
    ) -> local::Result<Value> {
        let request: QuantityCorrection = parse_request(raw)?;
        service::require(
            request.schema == "ledger-billing-quantity-correction/1",
            "BILLING_M5_REQUEST",
        )?;
        named(&request.customer, 128)?;
        named(&request.source, 256)?;
        named(&request.id, 128)?;
        named(&request.target, 128)?;
        service::require(
            !request.evidence.is_empty() && request.evidence.len() <= 8192,
            "BILLING_M5_REQUEST",
        )?;
        let delta = signed_nonzero(&request.quantity_delta)?;
        let occurred = Timestamp::parse(&request.occurred_at)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let identity = canonical(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"application",
            "family":"ledger-billing-quantity-correction/1","customer":request.customer,
            "source":request.source,"key_kind":"delivery_id","key":request.id
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(crate::service::store_error)?;
        let snapshot = tx
            .billing_snapshot()
            .await
            .map_err(crate::service::store_error)?;
        service::validate_snapshot(&snapshot)?;
        let authority =
            service::permissions::effective_for(&snapshot, &request.customer, &request.source)?;
        service::require(
            authority.permissions.iter().any(|p| p == "read"),
            "BILLING_UNAUTHORIZED",
        )?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error)? {
            if saved.request != raw {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = retained_result(&saved.response)?;
            tx.rollback().await.map_err(crate::service::store_error)?;
            return Ok(result);
        }
        service::require(
            authority.permissions.iter().any(|p| p == "correct"),
            "BILLING_PERMISSION",
        )?;
        let cumulative_target = tx
            .m5_cumulative_activity_target(&request.customer, &request.source, &request.target)
            .await
            .map_err(store_error)?;
        let per_work_target = if cumulative_target.is_none() {
            tx.m5_per_work_target(&request.customer, &request.source, &request.target)
                .await
                .map_err(store_error)?
        } else {
            None
        };
        service::require(
            cumulative_target.is_some() || per_work_target.is_some(),
            "BILLING_M5_TARGET",
        )?;
        let target = cumulative_target
            .as_deref()
            .map(retained_result)
            .transpose()?;
        let mode = if target.is_some() {
            "cumulative"
        } else {
            "per-work"
        };
        let original_id = target
            .as_ref()
            .and_then(|v| v["activity_id"].as_str())
            .unwrap_or(&request.target);
        let agreement_id = target
            .as_ref()
            .and_then(|v| v["agreement_id"].as_str())
            .or_else(|| per_work_target.as_ref().map(|v| v.agreement_id.as_str()))
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_version = target
            .as_ref()
            .and_then(|v| v["agreement_version"].as_str())
            .and_then(|v| v.parse::<i64>().ok())
            .or_else(|| per_work_target.as_ref().map(|v| v.agreement_version))
            .ok_or(ServiceError::IntegrityFailure)?;
        let original_agreement = service::control::terms_for_entry(
            &snapshot,
            &request.customer,
            &request.source,
            agreement_id,
            agreement_version,
        )?;
        service::require(
            original_agreement
                .permissions
                .iter()
                .any(|right| right == "correct"),
            "BILLING_PERMISSION",
        )?;
        let basis_version = target
            .as_ref()
            .and_then(|v| v["basis_version"].as_str())
            .and_then(|v| v.parse::<i64>().ok());
        let (term_version, period_index) = if let Some(target) = &target {
            (
                target["period_id"]["term_version"]
                    .as_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or(ServiceError::IntegrityFailure)?,
                target["period_id"]["period_index"]
                    .as_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or(ServiceError::IntegrityFailure)?,
            )
        } else {
            let target = per_work_target
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            (target.term_version, target.period_index)
        };
        let closed = tx
            .m5_cumulative_period_closed(&request.customer, term_version, period_index)
            .await
            .map_err(store_error)?;
        let enforce_clock = accepted_override.is_none();
        let accepted = accepted_override.unwrap_or(local::now()?);
        service::require(occurred.micros() <= accepted.micros(), "BILLING_M5_REQUEST")?;
        require_new_time(&mut tx, &snapshot, &accepted).await?;
        let (assigned_term_version, assigned_period_index) = if closed {
            let assigned = tx
                .m5_cumulative_period_at(&request.customer, accepted.micros())
                .await
                .map_err(store_error)?;
            service::require(
                !tx.m5_cumulative_period_closed(&request.customer, assigned.0, assigned.1)
                    .await
                    .map_err(store_error)?,
                "BILLING_M5_PERIOD",
            )?;
            assigned
        } else {
            (term_version, period_index)
        };

        let (basis, per_work) = if let Some(version) = basis_version {
            let basis_bytes = tx
                .m5_cumulative_basis_version(
                    &request.customer,
                    &request.source,
                    agreement_id,
                    agreement_version,
                    version,
                )
                .await
                .map_err(store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let basis: Basis =
                serde_json::from_slice(&basis_bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            basis.validate()?;
            (Some(basis), None)
        } else {
            (
                None,
                Some(per_work_facts(
                    per_work_target
                        .as_ref()
                        .ok_or(ServiceError::IntegrityFailure)?,
                )?),
            )
        };
        let mut resulting = if let Some(target) = &target {
            target["quantity"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .parse::<i128>()
                .map_err(|_| ServiceError::IntegrityFailure)?
        } else {
            per_work
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?
                .quantity
        };
        let deltas = if mode == "cumulative" {
            tx.m5_cumulative_target_deltas(&request.customer, &request.source, original_id)
                .await
                .map_err(store_error)?
        } else {
            tx.m5_per_work_target_deltas(&request.customer, &request.source, original_id)
                .await
                .map_err(store_error)?
        };
        for raw_delta in deltas {
            let value = retained_result(&raw_delta)?;
            resulting = resulting
                .checked_add(
                    value["quantity_delta"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .parse::<i128>()
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                )
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        }
        resulting = resulting
            .checked_add(delta)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let maximum = basis
            .as_ref()
            .map(Basis::maximum)
            .transpose()?
            .or_else(|| per_work.as_ref().map(|facts| facts.maximum))
            .ok_or(ServiceError::IntegrityFailure)?;
        service::require(
            resulting >= 0 && resulting <= maximum,
            "BILLING_M5_QUANTITY",
        )?;
        let sequences = tx.m5_cumulative_sequences().await.map_err(store_error)?;
        let (old_booking, new_booking) =
            if let (Some(basis), Some(version)) = (&basis, basis_version) {
                let rows = tx
                    .m5_cumulative_original_period_records(
                        &request.customer,
                        term_version,
                        period_index,
                        sequences.record - 1,
                    )
                    .await
                    .map_err(store_error)?;
                let total = bucket_quantity(
                    &rows,
                    &request.source,
                    agreement_id,
                    agreement_version,
                    version,
                )?;
                let new_total = total
                    .checked_add(delta)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                service::require(
                    new_total >= 0 && new_total <= basis.maximum()?,
                    "BILLING_M5_QUANTITY",
                )?;
                (
                    Some((total, basis.book(total)?)),
                    Some((new_total, basis.book(new_total)?)),
                )
            } else {
                (None, None)
            };
        let original_period_id = json!({"term_version":term_version.to_string(),"period_index":period_index.to_string()});
        let assigned_period_id = json!({"term_version":assigned_term_version.to_string(),"period_index":assigned_period_index.to_string()});
        let correction_key = canonical(&json!({
            "role":"quantity-correction","customer":request.customer,"source":request.source,
            "kind":"ledger-billing-quantity-correction-record/1","key":{"correction_id":request.id}
        }))?;
        let correction_id = m5::record_id(&identity, &correction_key);
        let adjustment_name = closed.then(|| adjustment_id(&identity));
        let adjustment_key = adjustment_name.as_ref().map(|adjustment_id| canonical(&json!({
            "role":"post-close-adjustment","customer":request.customer,"source":request.source,
            "kind":"ledger-billing-post-close-adjustment/1","key":{"adjustment_id":adjustment_id}
        }))).transpose()?;
        let adjustment_record_id = adjustment_key
            .as_ref()
            .map(|key| m5::record_id(&identity, key));
        let correction_sequence = sequences.record + i64::from(closed);
        let unit = basis
            .as_ref()
            .map(|b| b.source_unit.as_str())
            .or_else(|| per_work.as_ref().map(|facts| facts.unit.as_str()))
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut payload = json!({
            "schema":"ledger-billing-quantity-correction-record/1","customer":request.customer,
            "source":request.source,"correction_id":request.id,"target_activity_id":original_id,
            "quantity_delta":request.quantity_delta,"mode":mode,"unit":unit,
            "resulting_quantity":resulting.to_string(),
            "original_period_id":original_period_id,"assigned_period_id":assigned_period_id,
            "correction_route":if closed {"post-close-adjustment"} else {"original-open-period"},
            "record":{"record_id":correction_id,"sequence":correction_sequence.to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":sequences.command.to_string()}
        });
        if let Some(version) = basis_version {
            payload["basis_version"] = json!(version.to_string());
        }
        if mode == "per-work" {
            payload["agreement_version"] = json!(agreement_version.to_string());
            if !closed {
                let facts = per_work.as_ref().ok_or(ServiceError::IntegrityFailure)?;
                let amount = facts
                    .rate
                    .checked_mul(delta)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                payload["calculation"] = json!({"kind":"quantity_correction","exact_atoms_numerator":amount.to_string(),
                    "exact_atoms_denominator":"1","booked_atoms":amount.to_string(),"rounding":"none",
                    "operands":{"agreement_version":agreement_version.to_string(),"target_id":original_id,
                        "quantity_delta":request.quantity_delta,"unit":facts.unit,"rate_atoms_per_unit":facts.rate.to_string()}});
            }
        }
        let payload = m5::seal_child("ledger-billing-quantity-correction-record/1", &mut payload)
            .map_err(store_error)?;
        let mut adjustment_payload = None;
        let mut signed_delta_atoms = None;
        if closed {
            let adjustment_id = adjustment_name
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            let adjustment_record_id = adjustment_record_id
                .as_ref()
                .ok_or(ServiceError::IntegrityFailure)?;
            let (cause_kind, signed, calculation) = if let (
                Some((old_quantity, old)),
                Some((new_quantity, new)),
            ) = (old_booking, new_booking)
            {
                let signed = new
                    .booked_atoms
                    .checked_sub(old.booked_atoms)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                (
                    "cumulative-quantity-correction",
                    signed,
                    json!({"kind":"post_close_adjustment",
                    "exact_atoms_numerator":signed.to_string(),"exact_atoms_denominator":"1","booked_atoms":signed.to_string(),"rounding":"none",
                    "operands":{"adjustment_id":adjustment_id,"cause_kind":"cumulative-quantity-correction","cause_id":request.id,
                        "original_period_id":original_period_id,"assigned_period_id":assigned_period_id,"basis_version":basis_version.unwrap().to_string(),
                        "target_id":original_id,"quantity_delta":request.quantity_delta,"unit":unit,
                        "old_quantity":old_quantity.to_string(),"new_quantity":new_quantity.to_string(),
                        "old_exact_atoms_numerator":old.numerator,"old_exact_atoms_denominator":old.denominator,"old_booked_atoms":old.booked_atoms.to_string(),
                        "new_exact_atoms_numerator":new.numerator,"new_exact_atoms_denominator":new.denominator,"new_booked_atoms":new.booked_atoms.to_string()}}),
                )
            } else {
                let facts = per_work.as_ref().ok_or(ServiceError::IntegrityFailure)?;
                let old_quantity = resulting
                    .checked_sub(delta)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                let old_atoms = facts
                    .rate
                    .checked_mul(old_quantity)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                let new_atoms = facts
                    .rate
                    .checked_mul(resulting)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                let signed = new_atoms
                    .checked_sub(old_atoms)
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
                (
                    "per-work-quantity-correction",
                    signed,
                    json!({"kind":"post_close_adjustment",
                    "exact_atoms_numerator":signed.to_string(),"exact_atoms_denominator":"1","booked_atoms":signed.to_string(),"rounding":"none",
                    "operands":{"adjustment_id":adjustment_id,"cause_kind":"per-work-quantity-correction","cause_id":request.id,
                        "original_period_id":original_period_id,"assigned_period_id":assigned_period_id,"target_id":original_id,
                        "quantity_delta":request.quantity_delta,"unit":facts.unit,"rate_atoms_per_unit":facts.rate.to_string(),
                        "old_quantity":old_quantity.to_string(),"new_quantity":resulting.to_string(),
                        "old_exact_atoms_numerator":old_atoms.to_string(),"old_exact_atoms_denominator":"1","old_booked_atoms":old_atoms.to_string(),
                        "new_exact_atoms_numerator":new_atoms.to_string(),"new_exact_atoms_denominator":"1","new_booked_atoms":new_atoms.to_string(),
                        "agreement_version":agreement_version.to_string()}}),
                )
            };
            signed_delta_atoms = Some(signed.to_string());
            let mut value = json!({"schema":"ledger-billing-post-close-adjustment/1","customer":request.customer,
                "source":request.source,"adjustment_id":adjustment_id,"cause_kind":cause_kind,"cause_id":request.id,
                "target_id":original_id,"original_period_id":original_period_id,"assigned_period_id":assigned_period_id,
                "signed_delta_atoms":signed.to_string(),"currency":"USD","scale":18,"calculation":calculation,
                "record":{"record_id":adjustment_record_id,"sequence":sequences.record.to_string(),
                    "accepted_at":accepted.as_str(),"command_sequence":sequences.command.to_string()}});
            adjustment_payload = Some(
                m5::seal_child("ledger-billing-post-close-adjustment/1", &mut value)
                    .map_err(store_error)?,
            );
        }
        let record_ids = if let Some(adjustment_record_id) = &adjustment_record_id {
            vec![adjustment_record_id.clone(), correction_id.clone()]
        } else {
            vec![correction_id.clone()]
        };
        let mut result = json!({"status":"accepted","receipt":receipt(sequences.command,&record_ids,&accepted,raw)});
        if let (Some(adjustment_id), Some(signed)) = (&adjustment_name, &signed_delta_atoms) {
            result["adjustment"] = json!({"source":request.source,"adjustment_id":adjustment_id,
                "basis":if mode=="cumulative" {"cumulative_quantity"} else {"per_work_quantity"},
                "original_period_id":original_period_id,"assigned_period_id":assigned_period_id,"signed_delta_atoms":signed});
        }
        let response = canonical(&result)?;
        let correction_child = Child {
            family: "ledger-billing-quantity-correction-record/1",
            customer: Some(&request.customer),
            source: Some(&request.source),
            child_key: &correction_key,
            payload: &payload,
        };
        let adjustment_child = adjustment_payload
            .as_ref()
            .zip(adjustment_key.as_ref())
            .map(|(payload, key)| Child {
                family: "ledger-billing-post-close-adjustment/1",
                customer: Some(&request.customer),
                source: Some(&request.source),
                child_key: key,
                payload,
            });
        let mut children = adjustment_child
            .into_iter()
            .chain(std::iter::once(correction_child))
            .collect::<Vec<_>>();
        children.sort_by(|left, right| left.child_key.cmp(right.child_key));
        let command = Command {
            family: "ledger-billing-quantity-correction/1",
            domain: "application",
            customer: Some(&request.customer),
            source: Some(&request.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock,
            request: raw,
            response: &response,
            children: &children,
        };
        tx.m5_append_quantity_correction(
            &command,
            &m5::QuantityCorrectionProjection {
                customer: &request.customer,
                source: &request.source,
                correction_id: &request.id,
                target_id: original_id,
                mode,
                original_term_version: term_version,
                original_period_index: period_index,
                assigned_term_version,
                assigned_period_index,
                adjustment_id: adjustment_name.as_deref(),
                signed_delta_atoms: signed_delta_atoms.as_deref(),
            },
        )
        .await
        .map_err(store_error)?;
        accepted_commit(result, tx.commit().await)
    }
}

pub(super) fn bucket_quantity(
    rows: &[m5::CumulativeAssignedRecord],
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
    basis_version: i64,
) -> Result<i128, ServiceError> {
    let mut activities = std::collections::BTreeMap::new();
    for row in rows {
        if row.family == "ledger-billing-activity-record/1" {
            let value = retained_result(&row.payload)?;
            let activity_id = value["activity_id"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?;
            activities.insert(
                (
                    value["source"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .to_owned(),
                    activity_id.to_owned(),
                ),
                (
                    value["agreement_id"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .to_owned(),
                    value["agreement_version"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .to_owned(),
                    value["basis_version"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .to_owned(),
                ),
            );
        }
    }
    let mut quantity = 0i128;
    let agreement_version_text = agreement_version.to_string();
    let basis_version_text = basis_version.to_string();
    for row in rows {
        let value = retained_result(&row.payload)?;
        if value["source"] != source {
            continue;
        }
        if row.family == "ledger-billing-activity-record/1" {
            if value["agreement_id"] == agreement_id
                && value["agreement_version"].as_str() == Some(agreement_version_text.as_str())
                && value["basis_version"].as_str() == Some(basis_version_text.as_str())
            {
                quantity = quantity
                    .checked_add(
                        value["quantity"]
                            .as_str()
                            .ok_or(ServiceError::IntegrityFailure)?
                            .parse::<i128>()
                            .map_err(|_| ServiceError::IntegrityFailure)?,
                    )
                    .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            }
        } else if row.family == "ledger-billing-quantity-correction-record/1"
            && value["mode"] == "cumulative"
            && value["basis_version"].as_str() == Some(basis_version_text.as_str())
            && activities.get(&(
                source.to_owned(),
                value["target_activity_id"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .to_owned(),
            )) == Some(&(
                agreement_id.to_owned(),
                agreement_version.to_string(),
                basis_version.to_string(),
            ))
        {
            quantity = quantity
                .checked_add(
                    value["quantity_delta"]
                        .as_str()
                        .ok_or(ServiceError::IntegrityFailure)?
                        .parse::<i128>()
                        .map_err(|_| ServiceError::IntegrityFailure)?,
                )
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        }
    }
    service::require(quantity >= 0, "BILLING_M5_QUANTITY")?;
    Ok(quantity)
}

#[derive(Default)]
struct Bucket {
    quantity: i128,
    source_records: Vec<Value>,
}

pub(super) async fn close_lines(
    tx: &mut SqliteTx,
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    period_id: &Value,
    term_version: i64,
    period_index: i64,
    high_water: i64,
) -> Result<(Vec<Value>, Vec<Value>, i128), ServiceError> {
    let rows = tx
        .m5_cumulative_period_records(customer, term_version, period_index, high_water)
        .await
        .map_err(store_error)?;
    type BucketKey = (String, String, i64, i64);
    let mut buckets = std::collections::BTreeMap::<BucketKey, Bucket>::new();
    let mut activities = std::collections::BTreeMap::<(String, String), BucketKey>::new();
    for row in &rows {
        service::require(
            row.sequence > 0 && row.sequence <= high_water,
            "BILLING_M5_INTEGRITY",
        )?;
        if row.family != "ledger-billing-activity-record/1" {
            continue;
        }
        let value = retained_result(&row.payload)?;
        let source = value["source"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let activity_id = value["activity_id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_id = value["agreement_id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_version = value["agreement_version"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let basis_version = value["basis_version"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let quantity = value["quantity"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i128>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        service::require(quantity > 0, "BILLING_M5_INTEGRITY")?;
        let key = (
            source.to_owned(),
            agreement_id.to_owned(),
            agreement_version,
            basis_version,
        );
        if activities
            .insert((source.to_owned(), activity_id.to_owned()), key.clone())
            .is_some()
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let bucket = buckets.entry(key).or_default();
        bucket.quantity = bucket
            .quantity
            .checked_add(quantity)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        bucket.source_records.push(json!({"customer":customer,"source":source,"kind":row.family,"id":value["record"]["record_id"]}));
    }
    let mut lines = Vec::new();
    let mut included = Vec::new();
    let mut net = 0i128;
    for row in &rows {
        if row.family != "ledger-billing-quantity-correction-record/1" {
            continue;
        }
        let value = retained_result(&row.payload)?;
        if value["correction_route"] == "post-close-adjustment" {
            continue;
        }
        if value["mode"] == "per-work" {
            service::require(
                value["correction_route"] == "original-open-period",
                "BILLING_M5_PERIOD",
            )?;
            let source = value["source"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?;
            let target_id = value["target_activity_id"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?;
            let target = tx
                .m5_per_work_target(customer, source, target_id)
                .await
                .map_err(store_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let facts = per_work_facts(&target)?;
            let calculation = value["calculation"].clone();
            let amount = calculation["booked_atoms"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .parse::<i128>()
                .map_err(|_| ServiceError::IntegrityFailure)?;
            service::require(
                calculation["kind"] == "quantity_correction"
                    && calculation["exact_atoms_numerator"] == amount.to_string()
                    && calculation["exact_atoms_denominator"] == "1"
                    && calculation["rounding"] == "none",
                "BILLING_M5_INTEGRITY",
            )?;
            let source_record = json!({"customer":customer,"source":source,"kind":row.family,
                "id":value["record"]["record_id"]});
            included.push(source_record.clone());
            let mut line = json!({"source_records":[source_record],"basis":"quantity_correction",
                "agreement_id":target.agreement_id,"agreement_version":target.agreement_version.to_string(),
                "payer":facts.payer,"recipient":facts.recipient,"currency":"USD","scale":18,
                "amount_atoms":amount.to_string(),"calculation":calculation});
            let identity = CanonicalBytes::from_value(&json!({"view_kind":"standard",
                "view_identity":{"customer":customer,"period_id":period_id},"line":line}))
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
            line["line_id"] = json!(m5::hex(&m5::hash(
                b"bean-counter/m5/statement-line/1\0",
                identity.as_slice()
            )));
            lines.push(line);
            net = net
                .checked_add(amount)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            continue;
        }
        if value["mode"] != "cumulative" || value["correction_route"] != "original-open-period" {
            return Err(service::reject("BILLING_M5_PERIOD"));
        }
        let source = value["source"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let target = value["target_activity_id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let key = activities
            .get(&(source.to_owned(), target.to_owned()))
            .ok_or(ServiceError::IntegrityFailure)?;
        let bucket = buckets.get_mut(key).ok_or(ServiceError::IntegrityFailure)?;
        let delta = value["quantity_delta"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i128>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        bucket.quantity = bucket
            .quantity
            .checked_add(delta)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        bucket.source_records.push(json!({"customer":customer,"source":source,"kind":row.family,"id":value["record"]["record_id"]}));
    }
    lines.reserve(buckets.len());
    for ((source, agreement_id, agreement_version, basis_version), mut bucket) in buckets {
        service::require(bucket.quantity >= 0, "BILLING_M5_QUANTITY")?;
        let basis_bytes = tx
            .m5_cumulative_basis_version(
                customer,
                &source,
                &agreement_id,
                agreement_version,
                basis_version,
            )
            .await
            .map_err(store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let basis: Basis =
            serde_json::from_slice(&basis_bytes).map_err(|_| ServiceError::IntegrityFailure)?;
        basis.validate()?;
        service::require(bucket.quantity <= basis.maximum()?, "BILLING_M5_QUANTITY")?;
        let booked = basis.book(bucket.quantity)?;
        let agreement = service::control::terms_for_entry(
            snapshot,
            customer,
            &source,
            &agreement_id,
            agreement_version,
        )?;
        bucket.source_records.sort_by(|a, b| {
            (
                a["customer"].as_str(),
                a["source"].as_str(),
                a["kind"].as_str(),
                a["id"].as_str(),
            )
                .cmp(&(
                    b["customer"].as_str(),
                    b["source"].as_str(),
                    b["kind"].as_str(),
                    b["id"].as_str(),
                ))
        });
        included.extend(bucket.source_records.iter().cloned());
        let calculation = json!({
            "kind":"cumulative_close","exact_atoms_numerator":booked.numerator.to_string(),
            "exact_atoms_denominator":booked.denominator.to_string(),
            "booked_atoms":booked.booked_atoms.to_string(),"rounding":"nearest_ties_away",
            "operands":{"basis_version":basis_version.to_string(),"source_unit":basis.source_unit,
                "billable_unit":basis.billable_unit,"quantity":bucket.quantity.to_string(),
                "conversion_numerator":basis.conversion_numerator,
                "conversion_denominator":basis.conversion_denominator,
                "rate_atoms_per_billable_unit":rate_atoms(&basis.rate_usd_per_billable_unit)?.to_string()}
        });
        let mut line = json!({
            "source_records":bucket.source_records,"basis":"cumulative_close","agreement_id":agreement_id,
            "agreement_version":agreement_version.to_string(),"payer":agreement.customer,
            "recipient":agreement.host,"currency":"USD","scale":18,
            "amount_atoms":booked.booked_atoms.to_string(),"calculation":calculation
        });
        let identity = canonical(
            &json!({"view_kind":"standard","view_identity":{"customer":customer,"period_id":period_id},"line":line}),
        )?;
        line["line_id"] = json!(m5::hex(&m5::hash(
            b"bean-counter/m5/statement-line/1\0",
            &identity
        )));
        lines.push(line);
        net = net
            .checked_add(booked.booked_atoms)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
    }
    included.sort_by(|a, b| {
        (
            a["customer"].as_str(),
            a["source"].as_str(),
            a["kind"].as_str(),
            a["id"].as_str(),
        )
            .cmp(&(
                b["customer"].as_str(),
                b["source"].as_str(),
                b["kind"].as_str(),
                b["id"].as_str(),
            ))
    });
    lines.sort_by(|a, b| a["line_id"].as_str().cmp(&b["line_id"].as_str()));
    Ok((lines, included, net))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn wire(value: Value) -> Vec<u8> {
        canonical(&value).unwrap()
    }

    async fn per_work_race_fixture(
        path: &std::path::Path,
        suffix: &str,
    ) -> (Arc<BillingLedger>, Vec<u8>, Vec<u8>) {
        BillingLedger::init(
            path,
            include_bytes!("../../../../examples/billing/usage/setup.json"),
        )
        .await
        .unwrap();
        let ledger = Arc::new(BillingLedger::open(path).await.unwrap());
        let term = wire(json!({
            "schema":"ledger-billing-term/1","customer":"customer-usage-1",
            "change_id":format!("race-term-{suffix}"),"expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp",
                "boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }));
        ledger
            .term_set_at(
                &term,
                Timestamp::parse("2026-09-01T00:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        ledger
            .accept_at(
                "customer-usage-1",
                "urn:example:usage-work",
                include_bytes!("../../../../examples/billing/usage/event.json"),
                Timestamp::parse("2026-09-15T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let close = wire(json!({
            "schema":"ledger-billing-period-close/1","customer":"customer-usage-1",
            "period_id":{"term_version":"1","period_index":"0"}
        }));
        let correction = wire(json!({
            "schema":"ledger-billing-quantity-correction/1",
            "customer":"customer-usage-1","source":"urn:example:usage-work",
            "id":format!("race-correction-{suffix}"),"target":"usage-work-1",
            "quantity_delta":"-1","occurred_at":"2026-10-02T10:00:00.000000Z",
            "evidence":"verified close/correction writer race"
        }));
        (ledger, close, correction)
    }

    #[test]
    fn fractional_tie_and_zero_round_once() {
        let basis = Basis {
            mode: "cumulative_period".into(),
            source_unit: "token".into(),
            billable_unit: "billable-token".into(),
            conversion_numerator: "1".into(),
            conversion_denominator: "2".into(),
            rate_usd_per_billable_unit: "0.000000000000000001".into(),
            maximum_period_quantity: "100".into(),
        };
        basis.validate().unwrap();
        assert_eq!(
            basis.book(0).unwrap(),
            ExactBooking {
                numerator: "0".into(),
                denominator: "1".into(),
                booked_atoms: 0
            }
        );
        assert_eq!(
            basis.book(1).unwrap(),
            ExactBooking {
                numerator: "1".into(),
                denominator: "2".into(),
                booked_atoms: 1
            }
        );
        assert_eq!(
            basis.book(2).unwrap(),
            ExactBooking {
                numerator: "1".into(),
                denominator: "1".into(),
                booked_atoms: 1
            }
        );
        assert_eq!(
            basis.book(3).unwrap(),
            ExactBooking {
                numerator: "3".into(),
                denominator: "2".into(),
                booked_atoms: 2
            }
        );
        let large = Basis {
            conversion_numerator: "100000000000000000001".into(),
            conversion_denominator: "100000000000000000000".into(),
            maximum_period_quantity: "100000000000000000000".into(),
            ..basis
        };
        large.validate().unwrap();
        assert_eq!(
            large.book(100000000000000000000).unwrap(),
            ExactBooking {
                numerator: "100000000000000000001".into(),
                denominator: "1".into(),
                booked_atoms: 100000000000000000001
            }
        );

        let unreduced = Basis {
            conversion_numerator: "2".into(),
            conversion_denominator: "4".into(),
            ..Basis {
                mode: "cumulative_period".into(),
                source_unit: "token".into(),
                billable_unit: "billable-token".into(),
                conversion_numerator: "1".into(),
                conversion_denominator: "2".into(),
                rate_usd_per_billable_unit: "0.000000000000000001".into(),
                maximum_period_quantity: "100".into(),
            }
        };
        let normalized = unreduced.normalized().unwrap();
        assert_eq!(normalized.conversion_numerator, "1");
        assert_eq!(normalized.conversion_denominator, "2");
    }

    #[tokio::test]
    async fn activity_alias_preclose_delta_and_fractional_close_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = wire(json!({
            "schema":"ledger-billing-term/1","customer":"customer-1","change_id":"cumulative-term",
            "expected_revision":"0","effective":{"mode":"initial","at":"2026-10-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored","anchor":{"date":"2026-10-01","time":"00:00:00"},
                "timezone":"UTC","month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }));
        ledger
            .term_set_at(
                &term,
                Timestamp::parse("2026-10-01T00:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let basis = serde_json::to_vec_pretty(&json!({
            "schema":"ledger-billing-cumulative-agreement/1","customer":"customer-1","source":"urn:example:work",
            "change_id":"basis-1","expected_revision":"0","agreement_id":"agreement-1","agreement_version":"1",
            "effective_at":"2026-10-01T00:00:00.000000Z",
            "basis":{"mode":"cumulative_period","source_unit":"token","billable_unit":"billable-token",
                "conversion_numerator":"1","conversion_denominator":"2","rate_usd_per_billable_unit":"0.000000000000000001",
                "maximum_period_quantity":"100"}
        })).unwrap();
        let at = Timestamp::parse("2026-10-02T09:00:00.000000Z").unwrap();
        let setup = ledger
            .cumulative_agreement_set_at(&basis, at.clone())
            .await
            .unwrap();
        assert_eq!(setup["basis_version"], "1");
        assert_eq!(
            ledger
                .cumulative_agreement_set_at(&basis, at)
                .await
                .unwrap(),
            setup
        );
        assert!(matches!(ledger.accept_at("customer-1","urn:example:work",
            include_bytes!("../../../../examples/billing/event.json"),
            Timestamp::parse("2026-10-03T09:00:00.000000Z").unwrap()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code))) if code=="BILLING_M5_PERIOD"));
        let activity = json!({
            "schema":"ledger-billing-activity/1","customer":"customer-1","source":"urn:example:work",
            "id":"activity-1","operation_id":"operation-1","target":"run-1","quantity":"2",
            "occurred_at":"2026-10-10T10:00:00.000000Z","evidence":"completed batch"
        });
        let first = serde_json::to_vec_pretty(&activity).unwrap();
        let at = Timestamp::parse("2026-10-10T10:00:01.000000Z").unwrap();
        let accepted = ledger.activity_submit_at(&first, at.clone()).await.unwrap();
        let mut zero_ingress = activity.clone();
        zero_ingress["id"] = json!("activity-invalid-zero");
        zero_ingress["operation_id"] = json!("operation-invalid-zero");
        zero_ingress["quantity"] = json!("0");
        assert!(ledger
            .activity_submit_at(&wire(zero_ingress), at.clone())
            .await
            .is_err());
        assert_eq!(
            ledger.activity_submit_at(&first, at.clone()).await.unwrap(),
            accepted
        );
        assert_eq!(
            ledger
                .activity_submit_at(&wire(activity.clone()), at.clone())
                .await
                .unwrap(),
            accepted
        );
        let mut alias = activity.clone();
        alias["id"] = json!("activity-1-alias");
        let alias_bytes = wire(alias);
        assert_eq!(
            ledger
                .activity_submit_at(&alias_bytes, at.clone())
                .await
                .unwrap(),
            accepted
        );
        let mut semantic_conflict = activity.clone();
        semantic_conflict["id"] = json!("activity-1-changed");
        semantic_conflict["occurred_at"] = json!("2026-10-10T10:00:02.000000Z");
        assert!(
            matches!(ledger.activity_submit_at(&wire(semantic_conflict),at.clone()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code))) if code=="SEMANTIC_CONFLICT")
        );
        let mut conflict = activity;
        conflict["quantity"] = json!("3");
        assert!(
            matches!(ledger.activity_submit_at(&wire(conflict),at.clone()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code))) if code=="IDENTITY_CONFLICT")
        );
        let correction = wire(json!({
            "schema":"ledger-billing-quantity-correction/1","customer":"customer-1","source":"urn:example:work",
            "id":"correction-1","target":"activity-1","quantity_delta":"1",
            "occurred_at":"2026-11-02T11:00:00.000000Z","evidence":"verified extra token"
        }));
        let corrected = ledger
            .quantity_correct_at(
                &correction,
                Timestamp::parse("2026-11-02T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(corrected["status"], "accepted");
        let rejected_delta = wire(json!({
            "schema":"ledger-billing-quantity-correction/1","customer":"customer-1","source":"urn:example:work",
            "id":"correction-out-of-bounds","target":"activity-1","quantity_delta":"-4",
            "occurred_at":"2026-11-02T11:00:02.000000Z","evidence":"bad negative total"
        }));
        assert!(
            matches!(ledger.quantity_correct_at(&rejected_delta,Timestamp::parse("2026-11-02T11:00:03.000000Z").unwrap()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code))) if code=="BILLING_M5_QUANTITY")
        );
        let close = wire(
            json!({"schema":"ledger-billing-period-close/1","customer":"customer-1",
            "period_id":{"term_version":"1","period_index":"0"}}),
        );
        let statement = ledger
            .period_close_at(
                &close,
                Timestamp::parse("2026-11-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(statement["net_atoms"], "2");
        assert_eq!(
            statement["lines"][0]["calculation"]["exact_atoms_numerator"],
            "3"
        );
        assert_eq!(
            statement["lines"][0]["calculation"]["exact_atoms_denominator"],
            "2"
        );
        let after_close = wire(json!({
            "schema":"ledger-billing-quantity-correction/1","customer":"customer-1","source":"urn:example:work",
            "id":"correction-after-close","target":"activity-1","quantity_delta":"1",
            "occurred_at":"2026-11-06T11:00:00.000000Z","evidence":"late evidence"
        }));
        let after_close_result = ledger
            .quantity_correct_at(
                &after_close,
                Timestamp::parse("2026-11-06T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(after_close_result["adjustment"]["signed_delta_atoms"], "0");
        assert_eq!(
            after_close_result["adjustment"]["basis"],
            "cumulative_quantity"
        );
        let second_after_close = wire(json!({
            "schema":"ledger-billing-quantity-correction/1","customer":"customer-1","source":"urn:example:work",
            "id":"correction-after-close-2","target":"activity-1","quantity_delta":"1",
            "occurred_at":"2026-11-07T11:00:00.000000Z","evidence":"second late evidence"
        }));
        let second_result = ledger
            .quantity_correct_at(
                &second_after_close,
                Timestamp::parse("2026-11-07T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_result["adjustment"]["signed_delta_atoms"], "1");
        let zero_activity = wire(json!({
            "schema":"ledger-billing-activity/1","customer":"customer-1","source":"urn:example:work",
            "id":"activity-zero","operation_id":"operation-zero","target":"run-zero","quantity":"1",
            "occurred_at":"2026-11-10T10:00:00.000000Z","evidence":"completed batch"
        }));
        ledger
            .activity_submit_at(
                &zero_activity,
                Timestamp::parse("2026-11-10T10:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let zero_correction = wire(json!({
            "schema":"ledger-billing-quantity-correction/1","customer":"customer-1","source":"urn:example:work",
            "id":"correction-zero","target":"activity-zero","quantity_delta":"-1",
            "occurred_at":"2026-12-02T11:00:00.000000Z","evidence":"verified cancellation"
        }));
        ledger
            .quantity_correct_at(
                &zero_correction,
                Timestamp::parse("2026-12-02T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let zero_close = wire(
            json!({"schema":"ledger-billing-period-close/1","customer":"customer-1",
            "period_id":{"term_version":"1","period_index":"1"}}),
        );
        let zero_statement = ledger
            .period_close_at(
                &zero_close,
                Timestamp::parse("2026-12-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(zero_statement["net_atoms"], "1");
        assert!(zero_statement["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line["basis"] == "post_close_adjustment"
                && line["calculation"]["operands"]["new_quantity"] == "5"
                && line["amount_atoms"] == "1"));
        for (id, operation, day) in [
            ("activity-bucket-a", "operation-bucket-a", "10"),
            ("activity-bucket-b", "operation-bucket-b", "11"),
        ] {
            let raw = wire(json!({
                "schema":"ledger-billing-activity/1","customer":"customer-1","source":"urn:example:work",
                "id":id,"operation_id":operation,"target":id,"quantity":"1",
                "occurred_at":format!("2026-12-{day}T10:00:00.000000Z"),"evidence":"completed batch"
            }));
            ledger
                .activity_submit_at(
                    &raw,
                    Timestamp::parse(&format!("2026-12-{day}T10:00:01.000000Z")).unwrap(),
                )
                .await
                .unwrap();
        }
        let mut second_basis: Value = serde_json::from_slice(&basis).unwrap();
        second_basis["change_id"] = json!("basis-2");
        second_basis["expected_revision"] = json!("1");
        second_basis["effective_at"] = json!("2026-12-15T00:00:00.000000Z");
        second_basis["basis"]["conversion_denominator"] = json!("1");
        ledger
            .cumulative_agreement_set_at(
                &wire(second_basis),
                Timestamp::parse("2026-12-15T09:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let third = wire(json!({
            "schema":"ledger-billing-activity/1","customer":"customer-1","source":"urn:example:work",
            "id":"activity-bucket-c","operation_id":"operation-bucket-c","target":"activity-bucket-c","quantity":"1",
            "occurred_at":"2026-12-20T10:00:00.000000Z","evidence":"completed batch"
        }));
        ledger
            .activity_submit_at(
                &third,
                Timestamp::parse("2026-12-20T10:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let bucket_close = wire(
            json!({"schema":"ledger-billing-period-close/1","customer":"customer-1",
            "period_id":{"term_version":"1","period_index":"2"}}),
        );
        let bucket_statement = ledger
            .period_close_at(
                &bucket_close,
                Timestamp::parse("2027-01-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bucket_statement["net_atoms"], "2");
        assert_eq!(bucket_statement["lines"].as_array().unwrap().len(), 2);
        assert!(bucket_statement["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |line| line["calculation"]["operands"]["basis_version"] == "1"
                    && line["calculation"]["operands"]["quantity"] == "2"
                    && line["amount_atoms"] == "1"
            ));
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        assert_eq!(
            reopened
                .activity_submit_at(&first, at.clone())
                .await
                .unwrap(),
            accepted
        );
        assert_eq!(
            reopened.activity_submit_at(&alias_bytes, at).await.unwrap(),
            accepted
        );
        assert_eq!(
            reopened
                .period_close_at(
                    &close,
                    Timestamp::parse("2026-11-06T12:00:00.000000Z").unwrap()
                )
                .await
                .unwrap(),
            statement
        );
        assert_eq!(
            reopened
                .period_close_at(
                    &zero_close,
                    Timestamp::parse("2026-12-06T12:00:00.000000Z").unwrap()
                )
                .await
                .unwrap(),
            zero_statement
        );
        assert_eq!(
            reopened
                .period_close_at(
                    &bucket_close,
                    Timestamp::parse("2027-01-06T12:00:00.000000Z").unwrap()
                )
                .await
                .unwrap(),
            bucket_statement
        );
        reopened.close().await;
    }

    #[tokio::test]
    async fn per_work_corrections_route_open_then_ad_hoc_after_close() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/usage/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = wire(json!({
            "schema":"ledger-billing-term/1","customer":"customer-usage-1","change_id":"per-work-term",
            "expected_revision":"0","effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }));
        ledger
            .term_set_at(
                &term,
                Timestamp::parse("2026-09-01T00:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        ledger
            .accept_at(
                "customer-usage-1",
                "urn:example:usage-work",
                include_bytes!("../../../../examples/billing/usage/event.json"),
                Timestamp::parse("2026-09-15T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        let open = wire(json!({"schema":"ledger-billing-quantity-correction/1",
            "customer":"customer-usage-1","source":"urn:example:usage-work","id":"work-open",
            "target":"usage-work-1","quantity_delta":"-1","occurred_at":"2026-10-02T11:00:00.000000Z",
            "evidence":"verified open correction"}));
        let open_result = ledger
            .quantity_correct_at(
                &open,
                Timestamp::parse("2026-10-02T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert!(open_result.get("adjustment").is_none());
        let close0 = wire(
            json!({"schema":"ledger-billing-period-close/1","customer":"customer-usage-1",
            "period_id":{"term_version":"1","period_index":"0"}}),
        );
        let statement0 = ledger
            .period_close_at(
                &close0,
                Timestamp::parse("2026-10-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert!(statement0["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line["basis"] == "quantity_correction"
                && line["amount_atoms"] == "-250000000000"));

        let late = wire(json!({"schema":"ledger-billing-quantity-correction/1",
            "customer":"customer-usage-1","source":"urn:example:usage-work","id":"work-late",
            "target":"usage-work-1","quantity_delta":"2","occurred_at":"2026-10-06T11:00:00.000000Z",
            "evidence":"verified late correction"}));
        let late_result = ledger
            .quantity_correct_at(
                &late,
                Timestamp::parse("2026-10-06T11:00:01.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            late_result["adjustment"]["signed_delta_atoms"],
            "500000000000"
        );
        assert_eq!(
            ledger
                .quantity_correct_at(
                    &late,
                    Timestamp::parse("2026-10-06T11:00:01.000000Z").unwrap()
                )
                .await
                .unwrap(),
            late_result
        );
        let adjustment_id = late_result["adjustment"]["adjustment_id"].as_str().unwrap();
        let issue =
            serde_json::to_vec_pretty(&json!({"schema":"ledger-billing-ad-hoc-statement/1",
            "customer":"customer-usage-1","command_id":"per-work-ad-hoc",
            "adjustments":[{"source":"urn:example:usage-work","adjustment_id":adjustment_id}]}))
            .unwrap();
        let issued = ledger
            .adjustment_statement_at(
                &issue,
                Timestamp::parse("2026-10-07T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(issued["net_atoms"], "500000000000");
        assert_eq!(issued["lines"][0]["basis"], "post_close_adjustment");
        let close1 = wire(
            json!({"schema":"ledger-billing-period-close/1","customer":"customer-usage-1",
            "period_id":{"term_version":"1","period_index":"1"}}),
        );
        let statement1 = ledger
            .period_close_at(
                &close1,
                Timestamp::parse("2026-11-05T12:00:00.000000Z").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(statement1["net_atoms"], "0");
        assert_eq!(statement1["lines"], json!([]));
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        assert_eq!(
            reopened
                .adjustment_statement_at(
                    &issue,
                    Timestamp::parse("2026-10-07T12:00:00.000000Z").unwrap()
                )
                .await
                .unwrap(),
            issued
        );
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        sqlx::query("DROP TRIGGER billing_m5_adjustments_no_update")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE billing_m5_adjustments SET signed_delta_atoms='1' WHERE cause_id='work-late'",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        conn.close().await.unwrap();
        assert!(BillingLedger::open(&path).await.is_err());
    }

    #[tokio::test]
    async fn close_and_quantity_correction_serialize_both_writer_race_orders() {
        for correction_first in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().canonicalize().unwrap().join("billing");
            let suffix = if correction_first {
                "correction-first"
            } else {
                "close-first"
            };
            let (ledger, close, correction) = per_work_race_fixture(&path, suffix).await;
            let pause = ledger.store.test_pause_next_begin();

            let (correction_result, close_result) = if correction_first {
                let first_ledger = Arc::clone(&ledger);
                let first_request = correction.clone();
                let first = tokio::spawn(async move {
                    first_ledger
                        .quantity_correct_at(
                            &first_request,
                            Timestamp::parse("2026-10-02T11:00:00.000000Z").unwrap(),
                        )
                        .await
                });
                pause.reached.notified().await;
                let second_ledger = Arc::clone(&ledger);
                let second_request = close.clone();
                let second = tokio::spawn(async move {
                    second_ledger
                        .period_close_at(
                            &second_request,
                            Timestamp::parse("2026-10-05T12:00:00.000000Z").unwrap(),
                        )
                        .await
                });
                tokio::task::yield_now().await;
                pause.release.notify_one();
                (
                    first.await.unwrap().unwrap(),
                    second.await.unwrap().unwrap(),
                )
            } else {
                let first_ledger = Arc::clone(&ledger);
                let first_request = close.clone();
                let first = tokio::spawn(async move {
                    first_ledger
                        .period_close_at(
                            &first_request,
                            Timestamp::parse("2026-10-05T11:00:00.000000Z").unwrap(),
                        )
                        .await
                });
                pause.reached.notified().await;
                let second_ledger = Arc::clone(&ledger);
                let second_request = correction.clone();
                let second = tokio::spawn(async move {
                    second_ledger
                        .quantity_correct_at(
                            &second_request,
                            Timestamp::parse("2026-10-06T12:00:00.000000Z").unwrap(),
                        )
                        .await
                });
                tokio::task::yield_now().await;
                pause.release.notify_one();
                (
                    second.await.unwrap().unwrap(),
                    first.await.unwrap().unwrap(),
                )
            };

            if correction_first {
                assert!(correction_result.get("adjustment").is_none());
                assert!(close_result["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|line| line["basis"] == "quantity_correction"));
            } else {
                assert!(close_result["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|line| line["basis"] != "quantity_correction"));
                assert_eq!(
                    correction_result["adjustment"]["basis"],
                    "per_work_quantity"
                );
                assert_eq!(
                    correction_result["adjustment"]["signed_delta_atoms"],
                    "-250000000000"
                );
            }
            assert_eq!(
                ledger
                    .quantity_correct_at(
                        &correction,
                        Timestamp::parse(if correction_first {
                            "2026-10-02T11:00:00.000000Z"
                        } else {
                            "2026-10-06T12:00:00.000000Z"
                        })
                        .unwrap(),
                    )
                    .await
                    .unwrap(),
                correction_result
            );
            match Arc::try_unwrap(ledger) {
                Ok(ledger) => ledger.close().await,
                Err(_) => panic!("race tasks retained the billing ledger"),
            }
        }
    }

    #[tokio::test]
    async fn committed_m5_unknown_result_reopens_and_replays_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = wire(json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"unknown-m5-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp",
                "boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        }));
        let accepted = Timestamp::parse("2026-09-01T00:00:01.000000Z").unwrap();
        ledger.store.test_commit_cut(2);
        assert!(matches!(
            ledger.term_set_at(&term, accepted.clone()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code)))
                if code == "BILLING_M5_OUTCOME_UNKNOWN"
        ));
        ledger.close().await;

        let reopened = BillingLedger::open(&path).await.unwrap();
        let recovered = reopened.term_set_at(&term, accepted.clone()).await.unwrap();
        assert_eq!(recovered["status"], "term_updated");
        assert_eq!(
            reopened.term_set_at(&term, accepted).await.unwrap(),
            recovered
        );
        reopened.close().await;
        let verified = BillingLedger::open(&path).await.unwrap();
        assert_eq!(
            verified
                .term_set_at(
                    &term,
                    Timestamp::parse("2026-09-01T00:00:01.000000Z").unwrap(),
                )
                .await
                .unwrap(),
            recovered
        );
        verified.close().await;
    }
}
