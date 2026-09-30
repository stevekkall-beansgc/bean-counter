use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{self, Child, Command};
use ledgerlab_core::{canonical::CanonicalBytes, domain::term_service};
use sha2::{Digest, Sha256};

const TERM: &str = "ledger-billing-term/1";
const PERIOD_RESOLVE: &str = "ledger-billing-period-resolve/1";
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
        | term_service::TermPlanError::InvalidVersion
        | term_service::TermPlanError::TransitionAtBoundary
        | term_service::TermPlanError::PendingTermTransition => {
            service::reject("BILLING_M5_PERIOD")
        }
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
    pub async fn term_set(&self, raw: &[u8]) -> local::Result<Value> {
        let request = term_service::parse_term_change_request(raw).map_err(period_error)?;
        match request.mode {
            term_service::TermChangeMode::Initial => self.term_set_initial(raw).await,
            term_service::TermChangeMode::NextBoundary
            | term_service::TermChangeMode::Immediate => {
                self.term_set_transition(raw, request, None, false).await
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn term_set_at(
        &self,
        raw: &[u8],
        accepted: ledgerlab_core::domain::Timestamp,
    ) -> local::Result<Value> {
        let request = term_service::parse_term_change_request(raw).map_err(period_error)?;
        match request.mode {
            term_service::TermChangeMode::Initial => self.term_set_initial(raw).await,
            term_service::TermChangeMode::NextBoundary
            | term_service::TermChangeMode::Immediate => {
                self.term_set_transition(raw, request, Some(accepted), false)
                    .await
            }
        }
    }

    /// Activate a customer's first term. The complete M3 history and every
    /// projection are validated under the same writer transaction.
    async fn term_set_initial(&self, raw: &[u8]) -> local::Result<Value> {
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
        // The calendar can resolve beyond the wire timestamp domain (for
        // example the next monthly boundary after December 9999).
        ledgerlab_core::domain::Timestamp::parse(&effective_at)
            .and_then(|_| ledgerlab_core::domain::Timestamp::parse(&end_at))
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
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
            &m5::InitialTermProjection {
                customer: &plan.request.customer,
                term_bytes: &term_bytes,
                effective_at_us: plan.request.term.effective_at.timestamp_micros(),
                end_at_us: plan.period_zero.end.timestamp_micros(),
                resolution_id: &resolution_id,
                term_version: state.next_term_version,
                assignments: &assignments,
            },
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

    async fn term_set_transition(
        &self,
        raw: &[u8],
        request: term_service::TermChangeRequest,
        accepted_override: Option<ledgerlab_core::domain::Timestamp>,
        resolved_missing_period: bool,
    ) -> local::Result<Value> {
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

        // The transition clock is sampled only after the serialized writer
        // transaction is held and after exact retry lookup has completed.
        let retry_override = accepted_override.clone();
        let accepted = match accepted_override {
            Some(value) => value,
            None => local::now()?,
        };
        let accepted_at = accepted.as_str().to_owned();
        let accepted_instant = term_service::transition_instant(&accepted).map_err(period_error)?;
        let state = tx
            .m5_term_transition_state(&request.customer, accepted.micros())
            .await
            .map_err(m5_store_error)?;
        if request.expected_revision.value() != state.revision as u64 || state.revision == 0 {
            return Err(service::reject("BILLING_M5_STALE_REVISION").into());
        }
        let current_payload = ledgerlab_core::canonical::parse(&state.current.payload_bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let retained_term = ledgerlab_core::canonical::parse(&state.current.term_bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let retained_term_bytes = bytes(&current_payload["term"])?;
        if retained_term_bytes != state.current.term_bytes
            || current_payload["customer"] != request.customer
            || current_payload["term_version"]
                .as_str()
                .and_then(|value| value.parse::<i64>().ok())
                != Some(state.current.term_version)
            || current_payload["effective_at"]
                .as_str()
                .and_then(|value| ledgerlab_core::domain::Timestamp::parse(value).ok())
                .is_none_or(|value| value.micros() != state.current.effective_at_us)
        {
            return Err(ServiceError::IntegrityFailure.into());
        }
        let verification = bytes(&json!({
            "schema":TERM,"customer":request.customer,"change_id":"transition-current",
            "expected_revision":"0","effective":{"mode":"initial","at":current_payload["effective_at"]},
            "term":retained_term
        }))?;
        let current_term = term_service::parse_initial_request(&verification)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .term;
        let pending = state.has_pending_successor.then_some(accepted_instant);
        let plan = term_service::plan_term_transition(
            request,
            &current_term,
            pending,
            accepted_instant,
            u64::try_from(state.next_term_version)
                .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?,
        )
        .map_err(period_error)?;
        let previous_term_version = state.current.term_version;
        let term_version =
            i64::try_from(plan.term_version).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let effective_at = plan
            .effective_at
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        let end_at = plan
            .period_zero
            .end
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        ledgerlab_core::domain::Timestamp::parse(&effective_at)
            .and_then(|_| ledgerlab_core::domain::Timestamp::parse(&end_at))
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let term_bytes = bytes(&request_value["term"])?;
        let successor_resolution_id = format!("resolution-{term_version}-0-a");

        let predecessor = if let Some(clipped) = &plan.predecessor_resolution {
            let index =
                i64::try_from(clipped.index).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
            let head = tx
                .m5_resolution_head(&plan.request.customer, previous_term_version, index)
                .await
                .map_err(m5_store_error)?;
            let head = if let Some(head) = head {
                head
            } else {
                if resolved_missing_period {
                    return Err(ServiceError::IntegrityFailure.into());
                }
                let customer = plan.request.customer.clone();
                tx.rollback().await.map_err(store_error)?;
                self.period_resolve_at(&customer, previous_term_version, index, &accepted)
                    .await?;
                let retry = term_service::parse_term_change_request(raw).map_err(period_error)?;
                return Box::pin(self.term_set_transition(raw, retry, retry_override, true)).await;
            };
            let natural = current_term
                .period(index)
                .map_err(term_service::TermPlanError::from)
                .map_err(period_error)?;
            if head.start_at_us != natural.start.timestamp_micros()
                || head.end_at_us != natural.end.timestamp_micros()
            {
                return Err(ServiceError::IntegrityFailure.into());
            }
            let replacement_id = head.resolution_id.strip_suffix("-a").map_or_else(
                || format!("{}-b", head.resolution_id),
                |stem| format!("{stem}-b"),
            );
            Some((head, replacement_id, clipped.clone()))
        } else {
            None
        };

        enum PlannedChild {
            Predecessor,
            Successor,
            Term,
        }
        let mut planned = vec![
            (
                bytes(
                    &json!({"role":"boundary-resolution","customer":plan.request.customer,
                    "kind":RESOLUTION,"key":{"resolution_id":successor_resolution_id}}),
                )?,
                PlannedChild::Successor,
            ),
            (
                bytes(
                    &json!({"role":"term-version","customer":plan.request.customer,
                    "kind":VERSION,"key":{"term_version":term_version.to_string()}}),
                )?,
                PlannedChild::Term,
            ),
        ];
        if let Some((_, replacement_id, _)) = &predecessor {
            planned.push((
                bytes(
                    &json!({"role":"boundary-resolution","customer":plan.request.customer,
                    "kind":RESOLUTION,"key":{"resolution_id":replacement_id}}),
                )?,
                PlannedChild::Predecessor,
            ));
        }
        planned.sort_by(|left, right| left.0.cmp(&right.0));
        let record_ids = planned
            .iter()
            .map(|(key, _)| m5::record_id(&identity, key))
            .collect::<Vec<_>>();
        let mut owned_children = Vec::with_capacity(planned.len());
        for (offset, (key, kind)) in planned.into_iter().enumerate() {
            let record = json!({
                "record_id":record_ids[offset],
                "sequence":(state.first_record_sequence + offset as i64).to_string(),
                "accepted_at":accepted_at,
                "command_sequence":state.command_sequence.to_string()
            });
            let (family, mut payload) = match kind {
                PlannedChild::Predecessor => {
                    let (head, replacement_id, clipped) =
                        predecessor.as_ref().ok_or(ServiceError::IntegrityFailure)?;
                    (
                        RESOLUTION,
                        json!({
                            "schema":RESOLUTION,"customer":plan.request.customer,
                            "period_id":{"term_version":previous_term_version.to_string(),
                                "period_index":clipped.index.to_string()},
                            "resolution_id":replacement_id,"term_version":previous_term_version.to_string(),
                            "start_utc":clipped.start.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
                            "end_utc":effective_at,"supersedes_resolution_id":head.resolution_id,
                            "record":record
                        }),
                    )
                }
                PlannedChild::Successor => (
                    RESOLUTION,
                    json!({
                        "schema":RESOLUTION,"customer":plan.request.customer,
                        "period_id":{"term_version":term_version.to_string(),"period_index":"0"},
                        "resolution_id":successor_resolution_id,"term_version":term_version.to_string(),
                        "start_utc":effective_at,"end_utc":end_at,"record":record
                    }),
                ),
                PlannedChild::Term => (
                    VERSION,
                    json!({
                        "schema":VERSION,"customer":plan.request.customer,
                        "term_version":term_version.to_string(),"effective_at":effective_at,
                        "previous_term_version":previous_term_version.to_string(),
                        "term":request_value["term"],"record":record
                    }),
                ),
            };
            let payload = m5::seal_child(family, &mut payload).map_err(m5_store_error)?;
            owned_children.push((family, key, payload));
        }
        let children = owned_children
            .iter()
            .map(|(family, key, payload)| Child {
                family,
                customer: Some(&plan.request.customer),
                source: None,
                child_key: key,
                payload,
            })
            .collect::<Vec<_>>();
        let revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/request/1\0");
        digest.update(raw);
        let result = json!({
            "schema":"ledger-billing-term-result/1","status":"term_updated",
            "revision":revision.to_string(),"term_version":term_version.to_string(),
            "effective_at":effective_at,
            "receipt":{"schema":"ledger-billing-m5-receipt/1",
                "command_sequence":state.command_sequence.to_string(),"accepted_at":accepted_at,
                "record_ids":record_ids,"request_hash":m5::hex(&digest.finalize())}
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
        let predecessor_projection = predecessor
            .as_ref()
            .map(|(head, id, clipped)| (head, id.as_str(), clipped.end.timestamp_micros()));
        tx.m5_append_term_transition(
            &command,
            &m5::TransitionProjection {
                customer: &plan.request.customer,
                previous_term_version,
                term_version,
                effective_at_us: plan.effective_at.timestamp_micros(),
                term_bytes: &term_bytes,
                successor_resolution_id: &successor_resolution_id,
                successor_end_at_us: plan.period_zero.end.timestamp_micros(),
                predecessor: predecessor_projection,
            },
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

    pub(super) async fn period_resolve_at(
        &self,
        customer: &str,
        term_version: i64,
        period_index: i64,
        accepted: &ledgerlab_core::domain::Timestamp,
    ) -> local::Result<()> {
        let raw = bytes(&json!({
            "schema":PERIOD_RESOLVE,"customer":customer,
            "period_id":{"term_version":term_version.to_string(),"period_index":period_index.to_string()}
        }))?;
        let request = term_service::parse_period_resolve_request(&raw).map_err(period_error)?;
        let identity = bytes(&json!({
            "schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin",
            "family":PERIOD_RESOLVE,"customer":customer,"key_kind":"logical_period_id",
            "key":format!("{term_version}:{period_index}")
        }))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(m5_store_error)? {
            if saved.request != raw {
                return Err(ServiceError::IntegrityFailure.into());
            }
            tx.rollback().await.map_err(store_error)?;
            return Ok(());
        }
        let state = tx
            .m5_period_resolve_state(customer, term_version, period_index)
            .await
            .map_err(m5_store_error)?;
        if state.existing.is_some()
            || request.term_version != term_version as u64
            || request.period_index != period_index as u64
            || request.customer != customer
        {
            return Err(ServiceError::IntegrityFailure.into());
        }
        let payload = ledgerlab_core::canonical::parse(&state.term.payload_bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let retained_term = ledgerlab_core::canonical::parse(&state.term.term_bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if bytes(&payload["term"])? != state.term.term_bytes
            || payload["term_version"]
                .as_str()
                .and_then(|value| value.parse::<i64>().ok())
                != Some(term_version)
            || payload["customer"] != customer
        {
            return Err(ServiceError::IntegrityFailure.into());
        }
        let verification = bytes(&json!({
            "schema":TERM,"customer":customer,"change_id":"period-resolution",
            "expected_revision":"0","effective":{"mode":"initial","at":payload["effective_at"]},
            "term":retained_term
        }))?;
        let term = term_service::parse_initial_request(&verification)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .term;
        let period = term
            .period(period_index)
            .map_err(term_service::TermPlanError::from)
            .map_err(period_error)?;
        if state.successor_effective_at_us.is_some_and(|successor| {
            period.start.timestamp_micros() >= successor
                || period.end.timestamp_micros() > successor
        }) {
            return Err(service::reject("BILLING_M5_PERIOD").into());
        }
        if period.start.timestamp_micros() > accepted.micros() {
            return Err(service::reject("BILLING_M5_PERIOD").into());
        }
        let start_at = period.start.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string();
        let end_at = period.end.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string();
        ledgerlab_core::domain::Timestamp::parse(&start_at)
            .and_then(|_| ledgerlab_core::domain::Timestamp::parse(&end_at))
            .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let resolution_id = format!("boundary-resolution-{term_version}-{period_index}");
        let key = bytes(&json!({
            "role":"boundary-resolution","customer":customer,"kind":RESOLUTION,
            "key":{"resolution_id":resolution_id}
        }))?;
        let record_id = m5::record_id(&identity, &key);
        let accepted_at = accepted.as_str().to_owned();
        let mut resolution = json!({
            "schema":RESOLUTION,"customer":customer,
            "period_id":{"term_version":term_version.to_string(),"period_index":period_index.to_string()},
            "resolution_id":resolution_id,"term_version":term_version.to_string(),
            "start_utc":start_at,"end_utc":end_at,
            "record":{"record_id":record_id,"sequence":state.first_record_sequence.to_string(),
                "accepted_at":accepted_at,"command_sequence":state.command_sequence.to_string()}
        });
        let payload = m5::seal_child(RESOLUTION, &mut resolution).map_err(m5_store_error)?;
        let child = Child {
            family: RESOLUTION,
            customer: Some(customer),
            source: None,
            child_key: &key,
            payload: &payload,
        };
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/request/1\0");
        digest.update(&raw);
        let result = json!({
            "schema":"ledger-billing-period-resolution-result/1","status":"resolved",
            "customer":customer,"period_id":{"term_version":term_version.to_string(),
                "period_index":period_index.to_string()},
            "boundary_resolution_id":resolution_id,"record_sequence":state.first_record_sequence.to_string(),
            "start_utc":start_at,"end_utc":end_at,
            "receipt":{"schema":"ledger-billing-m5-receipt/1",
                "command_sequence":state.command_sequence.to_string(),"accepted_at":accepted_at,
                "record_ids":[record_id],"request_hash":m5::hex(&digest.finalize())}
        });
        let response = bytes(&result)?;
        let command = Command {
            family: PERIOD_RESOLVE,
            domain: "customer-admin",
            customer: Some(customer),
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            request: &raw,
            response: &response,
            children: std::slice::from_ref(&child),
        };
        tx.m5_append_period_resolution(
            &command,
            &m5::PeriodResolutionProjection {
                customer,
                term_version,
                period_index,
                resolution_id: &resolution_id,
                start_at_us: period.start.timestamp_micros(),
                end_at_us: period.end.timestamp_micros(),
            },
        )
        .await
        .map_err(m5_store_error)?;
        match tx.commit().await {
            Ok(()) => Ok(()),
            Err(CommitError::RolledBack(error)) => Err(store_error(error).into()),
            Err(CommitError::OutcomeUnknown) => {
                Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN").into())
            }
        }
    }
}
