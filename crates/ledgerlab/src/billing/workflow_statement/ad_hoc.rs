//! Explicit post-close sources under the same held presentation/claim owner.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{ArithmeticOperation as Op, RepresentationKind, ValidationVisit as Visit},
};
const AD_HOC: &str = "ledger-billing-ad-hoc-statement/2";
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    customer: String,
    command_id: String,
    workflow_scope: Value,
    adjustments: Value,
    workflow_actions: Value,
    #[serde(default)]
    manual_credits: Option<Value>,
}
fn id(v: &str, max: usize) -> bool {
    !v.is_empty() && v.len() <= max && !v.chars().any(char::is_control)
}
fn parse(raw: &[u8]) -> Result<Request, ServiceError> {
    let v = ledgerlab_core::canonical::parse_bounded(raw, 262144)
        .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    service::require(
        v.as_object().is_some_and(|o| {
            o.len()
                == if v["schema"] == "ledger-billing-ad-hoc-statement/3" {
                    7
                } else {
                    6
                }
        }),
        "BILLING_M5_REQUEST",
    )?;
    let r: Request =
        serde_json::from_value(v).map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
    service::require(
        matches!(
            r.schema.as_str(),
            AD_HOC | "ledger-billing-ad-hoc-statement/3"
        ) && (r.manual_credits.is_some() == (r.schema == "ledger-billing-ad-hoc-statement/3"))
            && id(&r.customer, 128)
            && id(&r.command_id, 128),
        "BILLING_M5_REQUEST",
    )?;
    let adjustments = r
        .adjustments
        .as_array()
        .ok_or_else(|| service::reject("BILLING_M5_REQUEST"))?;
    let actions = r
        .workflow_actions
        .as_array()
        .ok_or_else(|| service::reject("BILLING_M5_REQUEST"))?;
    service::require(
        !adjustments.is_empty()
            || !actions.is_empty()
            || r.manual_credits
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|v| !v.is_empty()),
        "BILLING_M5_REQUEST",
    )?;
    if let Some(manual) = &r.manual_credits {
        service::require(manual.is_array(), "BILLING_M5_REQUEST")?;
    }
    let mut previous = None;
    for a in adjustments {
        service::require(
            a.as_object().is_some_and(|o| o.len() == 2)
                && a["source"].as_str().is_some_and(|v| id(v, 256))
                && a["adjustment_id"].as_str().is_some_and(|v| id(v, 128)),
            "BILLING_M5_REQUEST",
        )?;
        let key = (
            a["source"].as_str().unwrap(),
            a["adjustment_id"].as_str().unwrap(),
        );
        service::require(previous.is_none_or(|p| p < key), "BILLING_M5_REQUEST")?;
        previous = Some(key);
    }
    let mut previous = None;
    for a in actions {
        service::require(
            a.as_object().is_some_and(|o| o.len() == 3)
                && a["kind"] == "workflow-action"
                && a["id"].as_str().is_some_and(|v| id(v, 256))
                && a["digest"].as_str().is_some_and(|v| {
                    v.len() == 71
                        && v.starts_with("sha256:")
                        && v.as_bytes()[7..]
                            .iter()
                            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
                }),
            "BILLING_M5_REQUEST",
        )?;
        let encoded = bytes(a)?;
        service::require(
            previous.as_ref().is_none_or(|p| p < &encoded),
            "BILLING_M5_REQUEST",
        )?;
        previous = Some(encoded);
    }
    Ok(r)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn selected_ad_hoc(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    scope: &Value,
    customer: &str,
    command_id: &str,
    requested: &Value,
    legacy_lines: &Value,
    cutoff: Option<i64>,
    recovery_claims: Option<(&str, i64)>,
    s: &mut OriginalAuditSession,
) -> Result<SelectedLines, ServiceError> {
    let requested_value = requested;
    let requested = requested_value
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    // The filter only limits collected DATA; statement_rows still completes the
    // entire physical prefix and every original producer at the held cutoff.
    let completed = ledger
        .statement_rows(
            tx,
            scope,
            customer,
            i64::MIN,
            i64::MAX,
            cutoff,
            Some(requested_value),
            s,
        )
        .await?;
    s.work()
        .arithmetic(
            Op::Compare,
            &[&completed.rows.len().into(), &requested.len().into()],
        )
        .map_err(core)?;
    service::require(completed.rows.len() == requested.len(), "BILLING_M5_SCOPE")?;
    let legacy = legacy_lines
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.work()
        .arithmetic(
            Op::Add,
            &[&completed.rows.len().into(), &legacy.len().into()],
        )
        .map_err(core)?;
    let count = completed
        .rows
        .len()
        .checked_add(legacy.len())
        .ok_or(ServiceError::Unavailable)?;
    let per = std::mem::size_of::<Value>() + std::mem::size_of::<ExternalBufferLease>();
    s.work()
        .arithmetic(Op::Multiply, &[&count.into(), &per.into()])
        .map_err(core)?;
    let storage = count.checked_mul(per).ok_or(ServiceError::Unavailable)?;
    let controls = s.reserve_external_payload(storage).map_err(core)?;
    s.work().write_bytes(storage).map_err(core)?;
    let mut payloads = Vec::new();
    let mut lines = Vec::new();
    payloads
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    lines
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    s.work()
        .arithmetic(
            Op::Compare,
            &[
                &payloads.capacity().into(),
                &lines.capacity().into(),
                &count.into(),
            ],
        )
        .map_err(core)?;
    service::require(
        payloads.capacity() <= count && lines.capacity() <= count,
        "INTEGRITY_FAILURE",
    )?;
    let view = ledger.statement_ad_hoc_view_identity(customer, command_id, s)?;
    for row in &completed.rows {
        s.work().visit(Visit::Element).map_err(core)?;
        service::require(row.is_adjustment, "BILLING_M5_SCOPE")?;
        let close = tx
            .workflow_statement_original_close(customer, row.original_accepted_at, s)
            .await
            .map_err(crate::service::workflow::statement_read_error)?
            .ok_or_else(|| service::reject("BILLING_M5_SCOPE"))?;
        service::require(
            matches!(
                close.value()["schema"].as_str(),
                Some("ledger-billing-statement/5" | "ledger-billing-statement/6")
            ),
            "BILLING_M5_PERIOD",
        )?;
        let through = close.value()["workflow_boundary"]["commit_sequence"]
            .as_str()
            .and_then(|v| v.parse::<i64>().ok())
            .ok_or(ServiceError::IntegrityFailure)?;
        let at = close.value()["close_acceptance_time"]
            .as_str()
            .ok_or(ServiceError::IntegrityFailure)?;
        let _time = s.reserve_external_payload(27).map_err(core)?;
        let at = ledgerlab_core::domain::Timestamp::parse_with_work(at, s.semantic_work())
            .map_err(core)?
            .micros();
        s.work()
            .arithmetic(
                Op::Compare,
                &[
                    &through.into(),
                    &row.sequence.into(),
                    &at.into(),
                    &row.accepted_at.into(),
                ],
            )
            .map_err(core)?;
        service::require(
            through < row.sequence && at <= row.accepted_at,
            "BILLING_M5_SCOPE",
        )?;
        if tx
            .workflow_statement_claimed(
                customer,
                scope,
                &row.line.value()["source_record_ref"],
                recovery_claims,
                s,
            )
            .await
            .map_err(crate::service::workflow::statement_read_error)?
        {
            return Err(service::reject(if recovery_claims.is_some() {
                "INTEGRITY_FAILURE"
            } else {
                "BILLING_M5_PRESENTED"
            }));
        }
        let line = workflow_statement::line("ad_hoc", view.value(), scope, row.line.value(), s)
            .map_err(core)?;
        let bound = {
            let (a, w) = s.parts();
            a.value_copy_payload_bound(line.value(), w).map_err(core)?
        };
        let owner = s.reserve_external_payload(bound).map_err(core)?;
        lines.push(line.value().clone());
        payloads.push(owner);
    }
    for line in legacy {
        s.work().visit(Visit::Element).map_err(core)?;
        let bound = {
            let (a, w) = s.parts();
            a.value_copy_payload_bound(line, w).map_err(core)?
        };
        let owner = s.reserve_external_payload(bound).map_err(core)?;
        lines.push(line.clone());
        payloads.push(owner);
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

impl BillingLedger {
    pub(in crate::billing) async fn workflow_adjustment_statement(
        &self,
        raw: &[u8],
    ) -> local::Result<Value> {
        let r = parse(raw)?;
        let identity = bytes(
            &json!({"schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin","family":"ledger-billing-ad-hoc-statement/1","customer":r.customer,"key_kind":"command_id","key":r.command_id}),
        )?;
        let ledger = LocalWorkflowLedger::from_billing_store(&self.path, &self.store).await?;
        let snapshot = ledger.statement_snapshot()?;
        let arena = snapshot.core_arena().map_err(core)?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core)?;
        let mut tx = ledger.statement_begin(&snapshot, &mut s).await?;
        let result = crate::store::sqlite::workflow::until(snapshot.deadline()?, async {
            if let Some(saved) = tx.m5_lookup(&identity).await.map_err(store_error)? {
                service::require(saved.request == raw, "IDENTITY_CONFLICT")?;
                return s
                    .parse_representation(&saved.response, RepresentationKind::Record)
                    .map(CloseResult::Original)
                    .map_err(core);
            }
            if r.manual_credits.is_some() {
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
            let accepted = local::now().map_err(|_| ServiceError::Unavailable)?;
            let references = r
                .adjustments
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .iter()
                .map(|a| {
                    (
                        a["source"].as_str().unwrap().to_owned(),
                        a["adjustment_id"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>();
            let state = tx
                .m5_ad_hoc_state(&r.customer, &references)
                .await
                .map_err(store_error)?;
            for (source, adjustment_id) in &references {
                if !state
                    .adjustments
                    .iter()
                    .any(|a| &a.source == source && &a.adjustment_id == adjustment_id)
                {
                    let claimed = tx
                        .m5_adjustment_claimed(&r.customer, source, adjustment_id)
                        .await
                        .map_err(store_error)?;
                    return Err(service::reject(if claimed == Some(true) {
                        "BILLING_M5_PRESENTED"
                    } else {
                        "BILLING_M5_SCOPE"
                    }));
                }
            }
            let position = tx.m5_statement_position().await.map_err(store_error)?;
            service::require(
                position.0 == state.command_sequence && position.1 == state.first_record_sequence,
                "INTEGRITY_FAILURE",
            )?;
            let legacy_view = json!({"customer":r.customer,"statement_id":r.command_id});
            let mut legacy_lines = Vec::new();
            for adjustment in &state.adjustments {
                let (line, _) = crate::billing::presentation::adjustment_line(
                    &r.customer,
                    "ad_hoc",
                    &legacy_view,
                    adjustment,
                )
                .map_err(|_| ServiceError::IntegrityFailure)?;
                legacy_lines.push(line);
            }
            let selected = selected_ad_hoc(
                &ledger,
                &mut tx,
                &r.workflow_scope,
                &r.customer,
                &r.command_id,
                &r.workflow_actions,
                &Value::Array(legacy_lines),
                None,
                None,
                &mut s,
            )
            .await?;
            let manual = if let Some(requested) = &r.manual_credits {
                let view =
                    ledger.statement_ad_hoc_view_identity(&r.customer, &r.command_id, &mut s)?;
                Some(
                    crate::billing::manual_credit::selected(
                        &ledger,
                        &mut tx,
                        &r.customer,
                        &r.workflow_scope,
                        "ad_hoc",
                        view.value(),
                        &selected.lines,
                        selected.through,
                        position.3,
                        i64::MIN,
                        i64::MAX,
                        Some(requested),
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
            let artifacts = crate::billing::workflow_statement_frame::prepare_ad_hoc(
                &identity,
                &r.customer,
                &r.command_id,
                &r.workflow_scope,
                &state.adjustments,
                &r.adjustments,
                &r.workflow_actions,
                position,
                &accepted,
                lines,
                totals,
                selected.through,
                manual.as_ref().map(|m| m.references.value()),
                &mut s,
            )?;
            let count = artifacts.children.len();
            s.work()
                .arithmetic(
                    Op::Multiply,
                    &[&count.into(), &std::mem::size_of::<m5::Child<'_>>().into()],
                )
                .map_err(core)?;
            let _controls = s
                .reserve_external_payload(
                    count
                        .checked_mul(std::mem::size_of::<m5::Child<'_>>())
                        .ok_or(ServiceError::Unavailable)?,
                )
                .map_err(core)?;
            s.work()
                .write_bytes(
                    count
                        .checked_mul(std::mem::size_of::<m5::Child<'_>>())
                        .ok_or(ServiceError::Unavailable)?,
                )
                .map_err(core)?;
            let mut children = Vec::new();
            children
                .try_reserve_exact(count)
                .map_err(|_| ServiceError::Unavailable)?;
            s.work()
                .arithmetic(Op::Compare, &[&children.capacity().into(), &count.into()])
                .map_err(core)?;
            service::require(children.capacity() <= count, "INTEGRITY_FAILURE")?;
            for child in &artifacts.children {
                s.work().visit(Visit::Element).map_err(core)?;
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
            Ok(CloseResult::Appended(a)) => match tx.commit_with_resource_session(s).await {
                Ok(()) => Ok(a.statement.value),
                Err(crate::store::sqlite::FinancialCommitError::Resource(e)) => Err(core(e)),
                Err(crate::store::sqlite::FinancialCommitError::Commit(
                    CommitError::RolledBack(e),
                )) => Err(store_error(e)),
                Err(crate::store::sqlite::FinancialCommitError::Commit(
                    CommitError::OutcomeUnknown,
                )) => Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN")),
            },
            Err(e) => {
                tx.rollback().await.map_err(store_error)?;
                Err(e)
            }
        };
        drop(ledger);
        result.map_err(Into::into)
    }
}
