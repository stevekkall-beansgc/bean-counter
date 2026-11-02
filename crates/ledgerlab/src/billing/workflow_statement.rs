//! Additive workflow statement close through the existing M5 customer owner.
use super::*;
use crate::store::sqlite::m5;
use crate::LocalWorkflowLedger;
use ledgerlab_core::{
    canonical::CanonicalBytes, domain::workflow_statement, validation_work::OriginalAuditSession,
};
const CLOSE: &str = "ledger-billing-period-close/2";
mod ad_hoc;
pub(crate) use ad_hoc::selected_ad_hoc;
fn bytes(v: &Value) -> Result<Vec<u8>, ServiceError> {
    CanonicalBytes::from_value(v)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| ServiceError::IntegrityFailure)
}
fn core(e: ledgerlab_core::Error) -> ServiceError {
    crate::service::workflow::core_error(e)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    customer: String,
    period_id: Value,
    workflow_scope: Value,
}
fn parse(
    raw: &[u8],
) -> Result<
    (
        Request,
        ledgerlab_core::domain::term_service::PeriodCloseRequest,
    ),
    ServiceError,
> {
    let value = ledgerlab_core::canonical::parse_bounded(raw, 262144)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    let r: Request =
        serde_json::from_value(value).map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    service::require(
        matches!(r.schema.as_str(), CLOSE | "ledger-billing-period-close/3")
            && r.workflow_scope.as_object().is_some_and(|o| {
                o.len() == 2 && o.contains_key("store") && o.contains_key("scope")
            }),
        "BILLING_M5_REQUEST",
    )?;
    let normalized = bytes(
        &json!({"schema":"ledger-billing-period-close/1","customer":r.customer,"period_id":r.period_id}),
    )?;
    let period = ledgerlab_core::domain::term_service::parse_period_close_request(&normalized)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    Ok((r, period))
}
pub(crate) struct SelectedLines {
    pub(crate) lines: Value,
    pub(crate) totals: ledgerlab_core::canonical::audit_arena::AuditedValue,
    pub(crate) through: i64,
    // Values drop before their copy leases and the vector control reservation.
    _payloads: Vec<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>,
    _controls: ledgerlab_core::canonical::audit_arena::ExternalBufferLease,
}
#[allow(clippy::too_many_arguments)]
pub(crate) async fn selected_lines(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    scope: &Value,
    customer: &str,
    start: i64,
    end: i64,
    cutoff: Option<i64>,
    period: &Value,
    legacy_lines: &Value,
    recovery_claims: Option<(&str, i64)>,
    s: &mut OriginalAuditSession,
) -> Result<SelectedLines, ServiceError> {
    use ledgerlab_core::validation_work::{ArithmeticOperation as Op, ValidationVisit as Visit};
    let completed = ledger
        .statement_rows(tx, scope, customer, start, end, cutoff, None, s)
        .await?;
    let legacy_lines = legacy_lines
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(
            Op::Add,
            &[&completed.rows.len().into(), &legacy_lines.len().into()],
        )
        .map_err(core)?;
    let count = completed
        .rows
        .len()
        .checked_add(legacy_lines.len())
        .ok_or(ServiceError::Unavailable)?;
    let per = std::mem::size_of::<Value>()
        + std::mem::size_of::<ledgerlab_core::canonical::audit_arena::ExternalBufferLease>();
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&count.into(), &per.into()])
        .map_err(core)?;
    let size = count.checked_mul(per).ok_or(ServiceError::Unavailable)?;
    let controls = s.reserve_external_payload(size).map_err(core)?;
    s.semantic_work().write_bytes(size).map_err(core)?;
    // Declare payload owners before local data so cancellation/error drops data
    // before releasing its reservation. SelectedLines preserves the same order.
    let mut payloads = Vec::new();
    let mut lines = Vec::new();
    payloads
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    lines
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    s.semantic_work()
        .arithmetic(
            Op::Compare,
            &[
                &lines.capacity().into(),
                &payloads.capacity().into(),
                &count.into(),
            ],
        )
        .map_err(core)?;
    service::require(
        lines.capacity() <= count && payloads.capacity() <= count,
        "INTEGRITY_FAILURE",
    )?;
    let view = ledger.statement_view_identity(customer, period, s)?;
    for row in &completed.rows {
        s.semantic_work().visit(Visit::Element).map_err(core)?;
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&start.into(), &row.original_accepted_at.into(), &end.into()],
            )
            .map_err(core)?;
        let include = if start <= row.original_accepted_at && row.original_accepted_at < end {
            true
        } else {
            s.check_cancelled().map_err(core)?;
            if let Some(closed) = tx
                .workflow_statement_original_close(customer, row.original_accepted_at, s)
                .await
                .map_err(crate::service::workflow::statement_read_error)?
            {
                s.check_cancelled().map_err(core)?;
                service::require(
                    matches!(
                        closed.value()["schema"].as_str(),
                        Some("ledger-billing-statement/5" | "ledger-billing-statement/6")
                    ),
                    "BILLING_M5_PERIOD",
                )?;
                let old = closed.value()["workflow_boundary"]["commit_sequence"]
                    .as_str()
                    .and_then(|v| v.parse::<i64>().ok())
                    .ok_or(ServiceError::IntegrityFailure)?;
                s.semantic_work()
                    .arithmetic(
                        Op::Compare,
                        &[
                            &old.into(),
                            &row.sequence.into(),
                            &start.into(),
                            &row.accepted_at.into(),
                            &end.into(),
                        ],
                    )
                    .map_err(core)?;
                old < row.sequence && start <= row.accepted_at && row.accepted_at < end
            } else {
                false
            }
        };
        if !include {
            continue;
        }
        let refv = &row.line.value()["source_record_ref"];
        s.check_cancelled().map_err(core)?;
        let claimed = tx
            .workflow_statement_claimed(customer, scope, refv, recovery_claims, s)
            .await
            .map_err(crate::service::workflow::statement_read_error)?;
        s.check_cancelled().map_err(core)?;
        if claimed {
            continue;
        }
        let line = workflow_statement::line("standard", view.value(), scope, row.line.value(), s)
            .map_err(core)?;
        let copy_bytes = {
            let (arena, work) = s.parts();
            arena
                .value_copy_payload_bound(line.value(), work)
                .map_err(core)?
        };
        let payload = s.reserve_external_payload(copy_bytes).map_err(core)?;
        lines.push(line.value().clone());
        payloads.push(payload);
    }
    for line in legacy_lines {
        s.semantic_work().visit(Visit::Element).map_err(core)?;
        let copy_bytes = {
            let (arena, work) = s.parts();
            arena.value_copy_payload_bound(line, work).map_err(core)?
        };
        let payload = s.reserve_external_payload(copy_bytes).map_err(core)?;
        lines.push(line.clone());
        payloads.push(payload);
    }
    ledgerlab_core::validation_work::try_sort_by(&mut lines, s.semantic_work(), |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        let a = a["line_id"].as_str().ok_or_else(|| ledgerlab_core::Error {
            code: "INTEGRITY_FAILURE",
            detail: String::new(),
        })?;
        let b = b["line_id"].as_str().ok_or_else(|| ledgerlab_core::Error {
            code: "INTEGRITY_FAILURE",
            detail: String::new(),
        })?;
        w.scan_bytes(a.len())?;
        w.scan_bytes(b.len())?;
        Ok(a.cmp(b))
    })
    .map_err(core)?;
    let totals = workflow_statement::totals(&lines, s).map_err(core)?;
    Ok(SelectedLines {
        lines: Value::Array(lines),
        totals,
        through: completed.through,
        _payloads: payloads,
        _controls: controls,
    })
}
/// Constructible only by the completed close coordinator below. An arbitrary
/// well-formed subset of Actions cannot be submitted through the store API.
pub(crate) struct PreparedWorkflowPresentation<'a> {
    command: m5::Command<'a>,
    statement: &'a Value,
    legacy_adjustments: &'a [m5::PresentableAdjustment],
    transaction_identity: usize,
}
impl<'a> PreparedWorkflowPresentation<'a> {
    pub(crate) fn legacy_claims(
        &self,
        transaction_identity: usize,
    ) -> Result<&[m5::PresentableAdjustment], crate::store::errors::StoreError> {
        self.parts(transaction_identity)?;
        Ok(self.legacy_adjustments)
    }
    pub(crate) fn parts(
        &self,
        transaction_identity: usize,
    ) -> Result<(&m5::Command<'a>, &Value), crate::store::errors::StoreError> {
        if self.transaction_identity != transaction_identity {
            return Err(crate::store::errors::StoreError::Integrity(
                "statement transaction owner",
            ));
        }
        Ok((&self.command, self.statement))
    }
}
enum CloseResult {
    Original(ledgerlab_core::canonical::audit_arena::AuditedValue),
    Appended(super::workflow_statement_frame::Artifacts),
}
impl BillingLedger {
    pub(super) async fn workflow_period_close(&self, raw: &[u8]) -> local::Result<Value> {
        let (r, period) = parse(raw)?;
        let term_version =
            i64::try_from(period.term_version).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let period_index =
            i64::try_from(period.period_index).map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        // Same customer+period business identity across wire versions.
        let identity = bytes(
            &json!({"schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin","family":"ledger-billing-period-close/1","customer":r.customer,"key_kind":"logical_period_id","key":format!("{term_version}:{period_index}")}),
        )?;
        let ledger = LocalWorkflowLedger::from_billing_store(&self.path, &self.store).await?;
        let snapshot = ledger.statement_snapshot()?;
        let arena = snapshot.core_arena().map_err(core)?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core)?;
        let mut tx = ledger.statement_begin(&snapshot, &mut s).await?;
        let deadline = snapshot.deadline()?;
        let result = crate::store::sqlite::workflow::until(deadline, async {
            if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error)? {
                service::require(saved.request == raw, "IDENTITY_CONFLICT")?;
                return s
                    .parse_representation(
                        &saved.response,
                        ledgerlab_core::validation_work::RepresentationKind::Record,
                    )
                    .map(CloseResult::Original)
                    .map_err(core);
            }
            if r.schema == "ledger-billing-period-close/3" {
                tx.workflow_enable_manual_credit_schema()
                    .await
                    .map_err(store_error)?;
            } else {
                tx.workflow_enable_statement_schema()
                    .await
                    .map_err(store_error)?;
            }
            let legacy = tx.billing_snapshot().await.map_err(store_error)?;
            service::validate_snapshot(&legacy)?;
            let state = tx
                .m5_period_close_state(&r.customer, term_version, period_index)
                .await
                .map_err(store_error)?;
            let (term, calculated_start, calculated_end) =
                super::close::close_period_bounds(&state, &r.customer, term_version, period_index)?;
            // A resolved immutable boundary is required before this additive close.
            let resolution = state
                .resolution
                .as_ref()
                .ok_or_else(|| service::reject("BILLING_M5_PERIOD"))?;
            service::require(
                resolution.start_at_us == calculated_start
                    && resolution.end_at_us <= calculated_end
                    && state
                        .successor_effective_at_us
                        .is_none_or(|t| resolution.end_at_us <= t)
                    && state
                        .successor_effective_at_us
                        .is_none_or(|t| calculated_start < t),
                "BILLING_M5_PERIOD",
            )?;
            let (start, end) = (resolution.start_at_us, resolution.end_at_us);
            let accepted = local::now().map_err(|_| ServiceError::Unavailable)?;
            service::require(accepted.micros() >= end, "BILLING_M5_NOT_DUE")?;
            service::require(state.unsupported_assignment_count == 0, "BILLING_M5_PERIOD")?;
            let (legacy_lines, included_records, _) = tx
                .m5_workflow_legacy_close(
                    &r.customer,
                    term_version,
                    period_index,
                    state.m3_high_water,
                    state.m5_high_water,
                    &state.adjustments,
                )
                .await
                .map_err(store_error)?;
            let selected = selected_lines(
                &ledger,
                &mut tx,
                &r.workflow_scope,
                &r.customer,
                start,
                end,
                None,
                &r.period_id,
                &legacy_lines,
                None,
                &mut s,
            )
            .await?;
            let manual = if r.schema == "ledger-billing-period-close/3" {
                let view = ledger.statement_view_identity(&r.customer, &r.period_id, &mut s)?;
                Some(
                    super::manual_credit::selected(
                        &ledger,
                        &mut tx,
                        &r.customer,
                        &r.workflow_scope,
                        "standard",
                        view.value(),
                        &selected.lines,
                        selected.through,
                        state.m5_high_water,
                        start,
                        end,
                        None,
                        None,
                        &mut s,
                    )
                    .await?,
                )
            } else {
                None
            };
            let (lines, totals) = if let Some(manual) = &manual {
                (manual.lines.value(), manual.totals.value())
            } else {
                (&selected.lines, selected.totals.value())
            };
            let artifacts = super::workflow_statement_frame::prepare(
                &identity,
                &r.customer,
                &r.period_id,
                &r.workflow_scope,
                &term,
                &state,
                start,
                end,
                &accepted,
                lines,
                totals,
                selected.through,
                &included_records,
                manual.as_ref().map(|m| m.references.value()),
                &mut s,
            )?;
            let view_count = artifacts.children.len();
            let _view_controls = s
                .reserve_external_payload(
                    view_count
                        .checked_mul(std::mem::size_of::<m5::Child<'_>>())
                        .ok_or(ServiceError::Unavailable)?,
                )
                .map_err(core)?;
            let mut children = Vec::new();
            children
                .try_reserve_exact(view_count)
                .map_err(|_| ServiceError::Unavailable)?;
            service::require(children.capacity() <= view_count, "INTEGRITY_FAILURE")?;
            for child in &artifacts.children {
                s.work()
                    .write_bytes(std::mem::size_of::<m5::Child<'_>>())
                    .map_err(core)?;
                children.push(m5::Child {
                    family: child.family,
                    customer: Some(&r.customer),
                    source: child.source.as_ref().map(|v| v.value.as_str()),
                    child_key: child.key.bytes(),
                    payload: child.payload.bytes(),
                });
            }
            let command = m5::Command {
                family: &r.schema,
                domain: "customer-admin",
                customer: Some(&r.customer),
                source: None,
                identity_key: &identity,
                accepted_at_us: accepted.micros(),
                enforce_clock: true,
                request: raw,
                response: artifacts.response.bytes(),
                children: &children,
            };
            let prepared = PreparedWorkflowPresentation {
                command,
                statement: &artifacts.statement.value,
                legacy_adjustments: &state.adjustments,
                transaction_identity: tx.workflow_proof_failure_handle().identity(),
            };
            s.check_cancelled().map_err(core)?;
            snapshot.recheck()?;
            tx.workflow_statement_append(&prepared, &mut s)
                .await
                .map_err(crate::service::workflow::statement_read_error)?;
            drop(children);
            Ok(CloseResult::Appended(artifacts))
        })
        .await
        .map_err(|_| {
            snapshot
                .timeout_error()
                .map(store_error)
                .unwrap_or(ServiceError::Unavailable)
        })
        .and_then(|r| r);
        let result = match result {
            Ok(CloseResult::Original(v)) => {
                tx.rollback().await.map_err(store_error)?;
                Ok(v.value().clone())
            }
            Ok(CloseResult::Appended(artifacts)) => {
                match tx.commit_with_resource_session(s).await {
                    Ok(()) => Ok(artifacts.statement.value),
                    Err(crate::store::sqlite::FinancialCommitError::Resource(e)) => Err(core(e)),
                    Err(crate::store::sqlite::FinancialCommitError::Commit(
                        CommitError::RolledBack(e),
                    )) => Err(store_error(e)),
                    Err(crate::store::sqlite::FinancialCommitError::Commit(
                        CommitError::OutcomeUnknown,
                    )) => Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN")),
                }
            }
            Err(e) => {
                tx.rollback().await.map_err(store_error)?;
                Err(e)
            }
        };
        // The billing facade retains the shared store owner and closes it.
        drop(ledger);
        result.map_err(Into::into)
    }
}
