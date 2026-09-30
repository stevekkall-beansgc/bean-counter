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
        service::require(gcd(numerator, denominator) == 1, "BILLING_M5_REQUEST")?;
        let _ = rate_atoms(&self.rate_usd_per_billable_unit)?;
        let maximum = positive(&self.maximum_period_quantity)?;
        let _ = self.book(maximum)?;
        Ok(())
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

async fn require_new_time(
    tx: &mut SqliteTx,
    snapshot: &crate::store::sqlite::BillingSnapshot,
    at: &Timestamp,
) -> Result<(), ServiceError> {
    let mut maximum = snapshot.ledger_time_max;
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
        request.basis.validate()?;
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
        let accepted = accepted_override.unwrap_or(local::now()?);
        require_new_time(&mut tx, &snapshot, &accepted).await?;
        let sequences = tx.m5_cumulative_sequences().await.map_err(store_error)?;
        let basis_version = revision
            .checked_add(1)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let basis_value = json!({
            "mode":"cumulative_period","source_unit":request.basis.source_unit,
            "billable_unit":request.basis.billable_unit,
            "conversion_numerator":request.basis.conversion_numerator,
            "conversion_denominator":request.basis.conversion_denominator,
            "rate_usd_per_billable_unit":request.basis.rate_usd_per_billable_unit,
            "maximum_period_quantity":request.basis.maximum_period_quantity
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
        let _ = Timestamp::parse(&request.occurred_at)
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
        let snapshot = tx
            .billing_snapshot()
            .await
            .map_err(crate::service::store_error)?;
        service::validate_snapshot(&snapshot)?;
        let authority =
            service::permissions::effective_for(&snapshot, &request.customer, &request.source)?;
        service::require(
            authority.permissions.iter().any(|p| p == "submit"),
            "BILLING_PERMISSION",
        )?;
        let accepted = accepted_override.unwrap_or(local::now()?);
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
        let _ = Timestamp::parse(&request.occurred_at)
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
        let authority =
            service::permissions::effective_for(&snapshot, &request.customer, &request.source)?;
        service::require(
            authority.permissions.iter().any(|p| p == "correct"),
            "BILLING_PERMISSION",
        )?;
        let target_raw = tx
            .m5_cumulative_activity_target(&request.customer, &request.source, &request.target)
            .await
            .map_err(store_error)?
            .ok_or_else(|| service::reject("BILLING_M5_TARGET"))?;
        let target = retained_result(&target_raw)?;
        let original_id = target["activity_id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_id = target["agreement_id"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let agreement_version = target["agreement_version"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
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
        let basis_version = target["basis_version"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let term_version = target["period_id"]["term_version"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let period_index = target["period_id"]["period_index"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i64>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if tx
            .m5_cumulative_period_closed(&request.customer, term_version, period_index)
            .await
            .map_err(store_error)?
        {
            return Err(service::reject("BILLING_M5_PERIOD_CLOSED").into());
        }
        let basis_bytes = tx
            .m5_cumulative_basis_version(
                &request.customer,
                &request.source,
                agreement_id,
                agreement_version,
                basis_version,
            )
            .await
            .map_err(store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let basis: Basis =
            serde_json::from_slice(&basis_bytes).map_err(|_| ServiceError::IntegrityFailure)?;
        basis.validate()?;
        let mut resulting = target["quantity"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?
            .parse::<i128>()
            .map_err(|_| ServiceError::IntegrityFailure)?;
        for raw_delta in tx
            .m5_cumulative_target_deltas(&request.customer, &request.source, original_id)
            .await
            .map_err(store_error)?
        {
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
        service::require(
            resulting >= 0 && resulting <= basis.maximum()?,
            "BILLING_M5_QUANTITY",
        )?;
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
        let total = bucket_quantity(
            &rows,
            &request.source,
            agreement_id,
            agreement_version,
            basis_version,
        )?;
        let new_total = total
            .checked_add(delta)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        service::require(
            new_total >= 0 && new_total <= basis.maximum()?,
            "BILLING_M5_QUANTITY",
        )?;
        let accepted = accepted_override.unwrap_or(local::now()?);
        require_new_time(&mut tx, &snapshot, &accepted).await?;
        let period_id = json!({"term_version":term_version.to_string(),"period_index":period_index.to_string()});
        let key = canonical(&json!({
            "role":"quantity-correction","customer":request.customer,"source":request.source,
            "kind":"ledger-billing-quantity-correction-record/1","key":{"correction_id":request.id}
        }))?;
        let id = m5::record_id(&identity, &key);
        let mut payload = json!({
            "schema":"ledger-billing-quantity-correction-record/1","customer":request.customer,
            "source":request.source,"correction_id":request.id,"target_activity_id":original_id,
            "quantity_delta":request.quantity_delta,"mode":"cumulative","unit":basis.source_unit,
            "basis_version":basis_version.to_string(),"resulting_quantity":resulting.to_string(),
            "original_period_id":period_id,"assigned_period_id":period_id,
            "correction_route":"original-open-period",
            "record":{"record_id":id,"sequence":sequences.record.to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":sequences.command.to_string()}
        });
        let payload = m5::seal_child("ledger-billing-quantity-correction-record/1", &mut payload)
            .map_err(store_error)?;
        let result =
            json!({"status":"accepted","receipt":receipt(sequences.command,&[id],&accepted,raw)});
        let response = canonical(&result)?;
        let child = Child {
            family: "ledger-billing-quantity-correction-record/1",
            customer: Some(&request.customer),
            source: Some(&request.source),
            child_key: &key,
            payload: &payload,
        };
        let command = Command {
            family: "ledger-billing-quantity-correction/1",
            domain: "application",
            customer: Some(&request.customer),
            source: Some(&request.source),
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            request: raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_cumulative_correction(&command, term_version, period_index)
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
    for row in &rows {
        if row.family != "ledger-billing-quantity-correction-record/1" {
            continue;
        }
        let value = retained_result(&row.payload)?;
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
    let mut lines = Vec::with_capacity(buckets.len());
    let mut included = Vec::new();
    let mut net = 0i128;
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

    fn wire(value: Value) -> Vec<u8> {
        canonical(&value).unwrap()
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
        ledger.term_set(&term).await.unwrap();
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
        assert!(
            matches!(ledger.quantity_correct_at(&after_close,Timestamp::parse("2026-11-06T11:00:01.000000Z").unwrap()).await,
            Err(local::LocalError::Service(ServiceError::Rejection(code))) if code=="BILLING_M5_PERIOD_CLOSED")
        );
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
        assert_eq!(zero_statement["net_atoms"], "0");
        assert_eq!(
            zero_statement["lines"][0]["calculation"]["exact_atoms_numerator"],
            "0"
        );
        assert_eq!(
            zero_statement["lines"][0]["calculation"]["exact_atoms_denominator"],
            "1"
        );
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
}
