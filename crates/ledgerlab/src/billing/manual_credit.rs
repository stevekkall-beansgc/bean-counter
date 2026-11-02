//! Explicit local customer-admin fixed credit. The original workflow producer
//! is recertified as an immutable reference; no workflow head is changed.
use super::workflow_statement_frame::{self as frame, Object};
use super::*;
use crate::{store::sqlite::m5, LocalWorkflowLedger};
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    domain::workflow_statement::manual_credit as data,
    validation_work::{
        ArithmeticOperation, OriginalAuditSession, RepresentationKind, ValidationVisit,
    },
};
fn core(e: ledgerlab_core::Error) -> ServiceError {
    crate::service::workflow::core_error(e)
}
fn store(e: crate::store::sqlite::workflow::financial::FinancialReadError) -> ServiceError {
    crate::service::workflow::statement_read_error(e)
}
fn text<'a>(
    v: &'a Value,
    key: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a str, ServiceError> {
    s.work().visit(ValidationVisit::Constraint).map_err(core)?;
    field(v, key, s)?
        .as_str()
        .ok_or(ServiceError::IntegrityFailure)
}
fn field<'a>(
    v: &'a Value,
    key: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value, ServiceError> {
    s.work().visit(ValidationVisit::Constraint).map_err(core)?;
    for (candidate, value) in v.as_object().ok_or(ServiceError::IntegrityFailure)? {
        s.work().visit(ValidationVisit::RefLookup).map_err(core)?;
        if same_bytes(candidate.as_bytes(), key.as_bytes(), s)? {
            return Ok(value);
        }
    }
    Err(ServiceError::IntegrityFailure)
}
fn same(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<bool, ServiceError> {
    let a = s
        .canonical_representation(a, RepresentationKind::Record)
        .map_err(core)?;
    let b = s
        .canonical_representation(b, RepresentationKind::Record)
        .map_err(core)?;
    same_bytes(a.bytes(), b.bytes(), s)
}
fn same_bytes(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<bool, ServiceError> {
    s.work()
        .visit(ValidationVisit::MemberCompare)
        .map_err(core)?;
    s.work().scan_bytes(a.len()).map_err(core)?;
    s.work().scan_bytes(b.len()).map_err(core)?;
    Ok(a == b)
}
fn counter(raw: &str, s: &mut OriginalAuditSession) -> Result<i64, ServiceError> {
    s.work().scan_bytes(raw.len()).map_err(core)?;
    s.work().visit(ValidationVisit::Constraint).map_err(core)?;
    service::require(
        !raw.is_empty()
            && (raw == "0" || !raw.starts_with('0'))
            && raw.bytes().all(|b| b.is_ascii_digit()),
        "INTEGRITY_FAILURE",
    )?;
    let mut n = 0i64;
    for b in raw.bytes() {
        s.work().visit(ValidationVisit::Element).map_err(core)?;
        s.work()
            .arithmetic(ArithmeticOperation::Multiply, &[&n.into(), &10i64.into()])
            .map_err(core)?;
        n = n.checked_mul(10).ok_or(ServiceError::IntegrityFailure)?;
        let digit = i64::from(b - b'0');
        s.work()
            .arithmetic(ArithmeticOperation::Add, &[&n.into(), &digit.into()])
            .map_err(core)?;
        n = n.checked_add(digit).ok_or(ServiceError::IntegrityFailure)?;
    }
    Ok(n)
}
fn identity(
    customer: &str,
    scope: &Value,
    id: &str,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes, ServiceError> {
    let mut key = Object::new(br#"{"adjustment_id":null,"workflow_scope":null}"#, s)?;
    key.text("adjustment_id", id, s)?;
    key.set("workflow_scope", scope, s)?;
    let mut v=Object::new(br#"{"customer":null,"domain":"customer-admin","family":"ledger-billing-manual-credit/1","key":null,"key_kind":"manual_credit","schema":"ledger-billing-m5-command-identity/1"}"#,s)?;
    v.text("customer", customer, s)?;
    v.set("key", &key.value, s)?;
    let b = v.encoded(s)?;
    service::require(b.bytes().len() <= 4096, "BILLING_M5_BOUNDS")?;
    Ok(b)
}
pub(crate) struct PreparedManualCredit<'a> {
    command: m5::Command<'a>,
    record: &'a Value,
    transaction_identity: usize,
}
impl PreparedManualCredit<'_> {
    pub(crate) fn parts(
        &self,
        id: usize,
    ) -> Result<(&m5::Command<'_>, &Value), crate::store::errors::StoreError> {
        if self.transaction_identity != id {
            return Err(crate::store::errors::StoreError::Integrity(
                "manual credit transaction owner",
            ));
        }
        Ok((&self.command, self.record))
    }
}
struct Artifacts {
    record: AuditedValue,
    response: AuditedValue,
    response_bytes: AuditedBytes,
    child: frame::Child,
}
fn artifacts(
    identity: &[u8],
    projected: &Value,
    accepted: &ledgerlab_core::domain::Timestamp,
    command: i64,
    sequence: i64,
    s: &mut OriginalAuditSession,
) -> Result<Artifacts, ServiceError> {
    let mut key = Object::new(
        br#"{"adjustment_id":null,"customer":null,"role":"manual-credit","workflow_scope":null}"#,
        s,
    )?;
    for name in ["adjustment_id", "customer", "workflow_scope"] {
        key.set(name, field(projected, name, s)?, s)?;
    }
    let key = key.encoded(s)?;
    let mut value=Object::new(br#"{"accepted_at":null,"adjustment_id":null,"agreement_id":null,"agreement_version":null,"amount":null,"binding_ref":null,"book":null,"customer":null,"funding_leg":null,"host":null,"operator":null,"reason":null,"record":null,"role_ref":null,"schema":null,"slot_terms_ref":null,"source_action_refs":null,"stage_instance_ref":null,"work_ref":null,"workflow_boundary":null,"workflow_scope":null}"#,s)?;
    for name in data::DATA_FIELDS {
        value.set(name, field(projected, name, s)?, s)?;
    }
    let child = frame::seal(
        value,
        key,
        data::RECORD,
        identity,
        accepted.as_str(),
        command,
        sequence,
        s,
    )?;
    let record = s
        .parse_representation(child.payload.bytes(), RepresentationKind::Record)
        .map_err(core)?;
    let digest = frame::hash("sha256:", &[child.payload.bytes()], s)?;
    let mut reference = Object::new(
        br#"{"digest":null,"id":null,"kind":"billing-manual-credit"}"#,
        s,
    )?;
    reference.text("digest", &digest.value, s)?;
    reference.set("id", &record.value()["record"]["record_id"], s)?;
    let mut response=Object::new(br#"{"credit_ref":null,"record":null,"schema":"ledger-billing-manual-credit-result/1","status":"committed"}"#,s)?;
    response.set("credit_ref", &reference.value, s)?;
    response.set("record", record.value(), s)?;
    let response_bytes = response.encoded(s)?;
    let response = s
        .parse_representation(response_bytes.bytes(), RepresentationKind::Record)
        .map_err(core)?;
    Ok(Artifacts {
        record,
        response,
        response_bytes,
        child,
    })
}
// Keep the bounded owned artifacts inline through commit. Boxing would add a
// separate heap allocation and lease solely to shrink this local control value.
#[allow(clippy::large_enum_variant)]
enum Outcome {
    Original(AuditedValue),
    Appended(Artifacts),
}
impl BillingLedger {
    pub async fn manual_credit(&self, raw: &[u8]) -> local::Result<Value> {
        service::require(raw.len() <= 262144, "BILLING_M5_REQUEST")?;
        let ledger = LocalWorkflowLedger::from_billing_store(&self.path, &self.store).await?;
        let snapshot = ledger.statement_snapshot()?;
        let arena = snapshot.core_arena().map_err(core)?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core)?;
        let request = s
            .parse_representation(raw, RepresentationKind::Record)
            .map_err(core)?;
        data::request(request.value(), &mut s).map_err(core)?;
        let r = request.value();
        let customer = text(r, "customer", &mut s)?;
        let mut tx = ledger.statement_begin(&snapshot, &mut s).await?;
        let result = crate::store::sqlite::workflow::until(snapshot.deadline()?, async {
            ledger
                .manual_credit_authorize(&mut tx, false, &mut s)
                .await?;
            ledger.manual_credit_scope(field(r, "workflow_scope", &mut s)?, &mut s)?;
            let identity = identity(
                customer,
                field(r, "workflow_scope", &mut s)?,
                text(r, "adjustment_id", &mut s)?,
                &mut s,
            )?;
            if let Some(old) = tx
                .manual_credit_lookup(identity.bytes(), &mut s)
                .await
                .map_err(store)?
            {
                service::require(
                    same_bytes(old.request.bytes(), raw, &mut s)?,
                    "IDENTITY_CONFLICT",
                )?;
                return Ok(Outcome::Original(old.response));
            }
            let authority = ledger
                .manual_credit_authorize(&mut tx, true, &mut s)
                .await?;
            tx.workflow_enable_manual_credit_schema()
                .await
                .map_err(store_error)?;
            let sources = ledger
                .manual_credit_sources(&mut tx, r, None, &mut s)
                .await?;
            let mut boundary = Object::new(
                br#"{"commit_sequence":null,"scope":null,"store":null}"#,
                &mut s,
            )?;
            boundary.counter("commit_sequence", sources.through, &mut s)?;
            boundary.set("scope", &r["workflow_scope"]["scope"], &mut s)?;
            boundary.set("store", &r["workflow_scope"]["store"], &mut s)?;
            let accepted = local::now().map_err(|_| ServiceError::Unavailable)?;
            let projected = data::project_packed(
                r,
                sources.data.value(),
                text(authority.value(), "operator", &mut s)?,
                text(authority.value(), "host", &mut s)?,
                accepted.as_str(),
                &boundary.value,
                &mut s,
            )
            .map_err(core)?;
            let (command_sequence, first_record) =
                tx.manual_credit_position(&mut s).await.map_err(store)?;
            let artifacts = artifacts(
                identity.bytes(),
                projected.value(),
                &accepted,
                command_sequence,
                first_record,
                &mut s,
            )?;
            let children = [m5::Child {
                family: data::RECORD,
                customer: Some(customer),
                source: None,
                child_key: artifacts.child.key.bytes(),
                payload: artifacts.child.payload.bytes(),
            }];
            let prepared = PreparedManualCredit {
                command: m5::Command {
                    family: data::REQUEST,
                    domain: "customer-admin",
                    customer: Some(customer),
                    source: None,
                    identity_key: identity.bytes(),
                    accepted_at_us: accepted.micros(),
                    enforce_clock: true,
                    request: raw,
                    response: artifacts.response_bytes.bytes(),
                    children: &children,
                },
                record: artifacts.record.value(),
                transaction_identity: tx.workflow_proof_failure_handle().identity(),
            };
            ledger
                .manual_credit_authorize(&mut tx, true, &mut s)
                .await?;
            snapshot.recheck()?;
            s.check_cancelled().map_err(core)?;
            tx.manual_credit_append(&prepared, &mut s)
                .await
                .map_err(store)?;
            Ok(Outcome::Appended(artifacts))
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
            Ok(Outcome::Original(v)) => {
                tx.rollback().await.map_err(store_error)?;
                Ok(v.value().clone())
            }
            Ok(Outcome::Appended(a)) => match tx.commit_with_resource_session(s).await {
                Ok(()) => Ok(a.response.value().clone()),
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

fn base64(raw: &[u8], s: &mut OriginalAuditSession) -> Result<frame::Text, ServiceError> {
    s.work()
        .arithmetic(
            ArithmeticOperation::Add,
            &[&raw.len().into(), &2usize.into()],
        )
        .map_err(core)?;
    let rounded = raw.len().checked_add(2).ok_or(ServiceError::Unavailable)?;
    s.work()
        .arithmetic(
            ArithmeticOperation::DivideRemainder,
            &[&rounded.into(), &3usize.into()],
        )
        .map_err(core)?;
    let groups = rounded / 3;
    s.work()
        .arithmetic(
            ArithmeticOperation::Multiply,
            &[&groups.into(), &4usize.into()],
        )
        .map_err(core)?;
    let size = groups.checked_mul(4).ok_or(ServiceError::Unavailable)?;
    let control = std::mem::size_of::<String>();
    s.work()
        .arithmetic(ArithmeticOperation::Add, &[&size.into(), &control.into()])
        .map_err(core)?;
    let owner = s
        .reserve_external_payload(size.checked_add(control).ok_or(ServiceError::Unavailable)?)
        .map_err(core)?;
    let mut encoded = String::new();
    encoded
        .try_reserve_exact(size)
        .map_err(|_| ServiceError::Unavailable)?;
    s.work()
        .arithmetic(
            ArithmeticOperation::Compare,
            &[&encoded.capacity().into(), &size.into()],
        )
        .map_err(core)?;
    service::require(encoded.capacity() <= size, "INTEGRITY_FAILURE")?;
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in raw.chunks(3) {
        s.work().visit(ValidationVisit::Element).map_err(core)?;
        s.work().scan_bytes(chunk.len()).map_err(core)?;
        s.work().write_bytes(4).map_err(core)?;
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(a >> 2) as usize] as char);
        encoded.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    Ok(frame::Text::owned(encoded, owner))
}
impl BillingLedger {
    pub async fn manual_credit_status(&self, raw: &[u8]) -> local::Result<Value> {
        self.manual_credit_query(raw, false).await
    }
    pub async fn manual_credit_read(&self, raw: &[u8]) -> local::Result<Value> {
        self.manual_credit_query(raw, true).await
    }
    async fn manual_credit_query(&self, raw: &[u8], read: bool) -> local::Result<Value> {
        service::require(raw.len() <= 262144, "BILLING_M5_REQUEST")?;
        let ledger = LocalWorkflowLedger::from_billing_store(&self.path, &self.store).await?;
        let snapshot = ledger.statement_snapshot()?;
        let arena = snapshot.core_arena().map_err(core)?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core)?;
        let request = s
            .parse_representation(raw, RepresentationKind::Record)
            .map_err(core)?;
        let r = request.value();
        data::selector(r, read, &mut s).map_err(core)?;
        let customer = text(r, "customer", &mut s)?;
        let scope = field(r, "workflow_scope", &mut s)?;
        let mut tx = ledger.statement_begin(&snapshot, &mut s).await?;
        let result=crate::store::sqlite::workflow::until(snapshot.deadline()?,async{
            let owner=ledger.manual_credit_authorize(&mut tx,false,&mut s).await?;
            ledger.manual_credit_scope(scope,&mut s)?;
            let output=if read{
                data::credit_ref(field(r,"credit_ref",&mut s)?,&mut s).map_err(core)?;
                let saved=tx.manual_credit_exact(customer,scope,field(r,"credit_ref",&mut s)?,&mut s).await.map_err(store)?.ok_or_else(||service::reject("MANUAL_CREDIT_NOT_FOUND"))?;
                recertify(&ledger,&mut tx,&saved,owner.value(),&mut s).await?;
                let bytes=s.canonical_representation(saved.record.value(),RepresentationKind::Record).map_err(core)?;
                let encoded=base64(bytes.bytes(),&mut s)?;let digest=frame::hash("",&[bytes.bytes()],&mut s)?;
                let mut out=Object::new(br#"{"body_bytes":null,"canonical_bytes_base64":null,"canonical_bytes_sha256":null,"credit_ref":null,"record":null,"schema":"ledger-billing-manual-credit-read-result/1"}"#,&mut s)?;
                out.set("credit_ref",field(r,"credit_ref",&mut s)?,&mut s)?;out.set("record",saved.record.value(),&mut s)?;
                out.text("canonical_bytes_base64",&encoded.value,&mut s)?;out.text("canonical_bytes_sha256",&digest.value,&mut s)?;
                out.counter("body_bytes",i64::try_from(bytes.bytes().len()).map_err(|_|ServiceError::Unavailable)?,&mut s)?;out
            }else{
                let id=text(r,"adjustment_id",&mut s)?;
                let key=identity(customer,scope,id,&mut s)?;
                let saved=tx.manual_credit_lookup(key.bytes(),&mut s).await.map_err(store)?;
                let mut out=Object::new(if saved.is_some(){br#"{"adjustment_id":null,"customer":null,"request_bytes_sha256":null,"result":null,"schema":"ledger-billing-manual-credit-status-result/1","status":"committed","workflow_scope":null}"#}else{br#"{"adjustment_id":null,"customer":null,"schema":"ledger-billing-manual-credit-status-result/1","status":"not_observed","workflow_scope":null}"#},&mut s)?;
                out.text("customer",customer,&mut s)?;out.text("adjustment_id",id,&mut s)?;out.set("workflow_scope",scope,&mut s)?;
                if let Some(saved)=saved{
                    let digest=frame::hash("",&[saved.request.bytes()],&mut s)?;
                    out.text("request_bytes_sha256",&digest.value,&mut s)?;out.set("result",saved.response.value(),&mut s)?;
                }out
            };
            snapshot.recheck()?;s.check_cancelled().map_err(core)?;Ok(output)
        }).await.map_err(|_|snapshot.timeout_error().map(store_error).unwrap_or(ServiceError::Unavailable)).and_then(|v|v);
        tx.rollback().await.map_err(store_error)?;
        drop(ledger);
        result.map(|v| v.value).map_err(Into::into)
    }
}
/// Recertification for both explicit reads and statement selection. The record's
/// own saved complete workflow cutoff is used, never the latest financial head.
pub(crate) async fn recertify(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    saved: &crate::store::sqlite::m5_manual::StoredManualCredit,
    owner: &Value,
    s: &mut OriginalAuditSession,
) -> Result<(), ServiceError> {
    let request = s
        .parse_representation(saved.request.bytes(), RepresentationKind::Record)
        .map_err(core)?;
    let record = saved.record.value();
    service::require(
        same(
            field(record, "operator", s)?,
            field(owner, "operator", s)?,
            s,
        )? && same(field(record, "host", s)?, field(owner, "host", s)?, s)?,
        "AUTHORITY_REQUIRED",
    )?;
    let cut = text(field(record, "workflow_boundary", s)?, "commit_sequence", s)?;
    s.work().scan_bytes(cut.len()).map_err(core)?;
    let cutoff = counter(cut, s)?;
    let sources = ledger
        .manual_credit_sources(tx, request.value(), Some(cutoff), s)
        .await?;
    let projected = data::project_packed(
        request.value(),
        sources.data.value(),
        text(record, "operator", s)?,
        text(record, "host", s)?,
        text(record, "accepted_at", s)?,
        field(record, "workflow_boundary", s)?,
        s,
    )
    .map_err(core)?;
    data::compare_record(projected.value(), record, s).map_err(core)
}

pub(crate) struct SelectedManualLines {
    pub(crate) lines: AuditedValue,
    pub(crate) totals: AuditedValue,
    pub(crate) references: AuditedValue,
}
#[allow(clippy::too_many_arguments)]
pub(crate) async fn selected(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    customer: &str,
    scope: &Value,
    view_kind: &str,
    view_identity: &Value,
    base_lines: &Value,
    through: i64,
    m5_cut: i64,
    start: i64,
    end: i64,
    requested: Option<&Value>,
    recovery: Option<(&str, i64)>,
    s: &mut OriginalAuditSession,
) -> Result<SelectedManualLines, ServiceError> {
    let owner = ledger.manual_credit_authorize(tx, false, s).await?;
    ledger.manual_credit_scope(scope, s)?;
    let retained = if requested.is_none() {
        Some(
            tx.manual_credit_selected(customer, scope, start, end, m5_cut, recovery, s)
                .await
                .map_err(store)?,
        )
    } else {
        None
    };
    let requested = requested
        .map(|v| v.as_array().ok_or(ServiceError::IntegrityFailure))
        .transpose()?;
    let count = if let Some(r) = requested {
        r.len()
    } else {
        retained
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?
            .rows
            .len()
    };
    s.work()
        .arithmetic(
            ArithmeticOperation::Multiply,
            &[&count.into(), &std::mem::size_of::<AuditedValue>().into()],
        )
        .map_err(core)?;
    let size = count
        .checked_mul(std::mem::size_of::<AuditedValue>())
        .ok_or(ServiceError::Unavailable)?;
    let _controls = s.reserve_external_payload(size).map_err(core)?;
    s.work().write_bytes(size).map_err(core)?;
    let mut lines = Vec::new();
    lines
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    s.work()
        .arithmetic(
            ArithmeticOperation::Compare,
            &[&lines.capacity().into(), &count.into()],
        )
        .map_err(core)?;
    service::require(lines.capacity() <= count, "INTEGRITY_FAILURE")?;
    if let Some(requested) = requested {
        let mut prior: Option<AuditedBytes> = None;
        for r in requested {
            s.work().visit(ValidationVisit::Element).map_err(core)?;
            data::credit_ref(r, s).map_err(core)?;
            let key = s
                .canonical_representation(r, RepresentationKind::Record)
                .map_err(core)?;
            if let Some(prior) = &prior {
                s.work()
                    .visit(ValidationVisit::MemberCompare)
                    .map_err(core)?;
                s.work()
                    .scan_bytes(prior.bytes().len() + key.bytes().len())
                    .map_err(core)?;
                service::require(prior.bytes() < key.bytes(), "BILLING_M5_REQUEST")?;
            }
            prior = Some(key);
            service::require(
                !tx.manual_credit_claimed(customer, scope, r, recovery, s)
                    .await
                    .map_err(store)?,
                "BILLING_M5_PRESENTED",
            )?;
            let saved = tx
                .manual_credit_exact(customer, scope, r, s)
                .await
                .map_err(store)?
                .ok_or_else(|| service::reject("BILLING_M5_SCOPE"))?;
            lines.push(
                selected_line(
                    ledger,
                    tx,
                    &saved,
                    owner.value(),
                    scope,
                    view_kind,
                    view_identity,
                    through,
                    m5_cut,
                    s,
                )
                .await?,
            );
        }
    } else if let Some(retained) = &retained {
        for saved in &retained.rows {
            s.work().visit(ValidationVisit::Element).map_err(core)?;
            lines.push(
                selected_line(
                    ledger,
                    tx,
                    saved,
                    owner.value(),
                    scope,
                    view_kind,
                    view_identity,
                    through,
                    m5_cut,
                    s,
                )
                .await?,
            );
        }
    }
    let references = data::line_references(&lines, s).map_err(core)?;
    let lines = data::combine_lines(base_lines, &lines, s).map_err(core)?;
    let totals = ledgerlab_core::domain::workflow_statement::totals_with_manual(
        lines
            .value()
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?,
        s,
    )
    .map_err(core)?;
    Ok(SelectedManualLines {
        lines,
        totals,
        references,
    })
}
#[allow(clippy::too_many_arguments)]
async fn selected_line(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    saved: &crate::store::sqlite::m5_manual::StoredManualCredit,
    owner: &Value,
    scope: &Value,
    view_kind: &str,
    view_identity: &Value,
    through: i64,
    m5_cut: i64,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue, ServiceError> {
    let cut = text(
        field(saved.record.value(), "workflow_boundary", s)?,
        "commit_sequence",
        s,
    )?;
    s.work().scan_bytes(cut.len()).map_err(core)?;
    let cut = counter(cut, s)?;
    s.work()
        .arithmetic(
            ArithmeticOperation::Compare,
            &[
                &saved.sequence.into(),
                &m5_cut.into(),
                &cut.into(),
                &through.into(),
            ],
        )
        .map_err(core)?;
    service::require(
        saved.sequence <= m5_cut && cut <= through,
        "INTEGRITY_FAILURE",
    )?;
    recertify(ledger, tx, saved, owner, s).await?;
    data::line(
        field(saved.response.value(), "credit_ref", s)?,
        saved.record.value(),
        view_kind,
        view_identity,
        scope,
        s,
    )
    .map_err(core)
}
