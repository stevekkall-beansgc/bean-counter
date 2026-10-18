//! Profile B payload accounting. This counts capacity units, never prices.
use super::financial::FinancialReadError;
use crate::store::errors::StoreError;
use ledgerlab_core::validation_work::{
    ArithmeticOperation as Op, NoopValidationWork, OriginalAuditSession, ValidationVisit,
    ValidationWork,
};
use sqlx::SqliteConnection;

pub(crate) const PROFILE: &str = "workflow-dual-legacy-capacity/1";
const ROWS: i64 = 100000;
const BYTES: i64 = 268435456;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WorkflowMeter {
    pub operations: i64,
    pub payload_bytes: i64,
}

/// Verify all retained meters under the shared writer transaction and compare
/// both legacy pools against the exact prospective workflow delta.
pub(crate) async fn check(
    conn: &mut SqliteConnection,
    additional_operations: i64,
    additional_bytes: i64,
) -> Result<WorkflowMeter, StoreError> {
    check_with_work(
        conn,
        additional_operations,
        additional_bytes,
        &mut NoopValidationWork,
        None,
    )
    .await
    .map_err(|error| match error {
        FinancialReadError::Store(error) => error,
        // The compatibility adapter cannot fail a work callback.
        FinancialReadError::Resource(_) => StoreError::Integrity("noop capacity work failed"),
    })
}
/// SAME retained-meter equations with executing callbacks. The financial
/// caller retains its SQL worker lease across this fixed scalar result set.
pub(super) async fn check_with_session(
    conn: &mut SqliteConnection,
    additional_operations: i64,
    additional_bytes: i64,
    session: &mut OriginalAuditSession,
    limits:Option<super::OperatorStorage>,
) -> Result<WorkflowMeter, FinancialReadError> {
    check_with_work(
        conn,
        additional_operations,
        additional_bytes,
        &mut SessionWork(session),
        limits,
    )
    .await
}
// The concrete borrow remains Send across SQL awaits. The dyn work view is
// opened only for each synchronous callback, never held across an await.
pub(in crate::store::sqlite) struct SessionWork<'a>(pub(in crate::store::sqlite) &'a mut OriginalAuditSession);
impl ValidationWork for SessionWork<'_> {
    fn visit(&mut self, kind: ValidationVisit) -> ledgerlab_core::Result<()> {
        self.0.semantic_work().visit(kind)
    }
    fn arithmetic(
        &mut self,
        op: Op,
        operands: &[&ledgerlab_core::validation_work::BigInt],
    ) -> ledgerlab_core::Result<()> {
        self.0.semantic_work().arithmetic(op, operands)
    }
    fn scan_bytes(&mut self, bytes: usize) -> ledgerlab_core::Result<()> {
        self.0.semantic_work().scan_bytes(bytes)
    }
    fn write_bytes(&mut self, bytes: usize) -> ledgerlab_core::Result<()> {
        self.0.semantic_work().write_bytes(bytes)
    }
    fn hash_bytes(&mut self, bytes: usize) -> ledgerlab_core::Result<()> {
        self.0.semantic_work().hash_bytes(bytes)
    }
}
async fn check_with_work<W: ValidationWork + Send>(
    conn: &mut SqliteConnection,
    additional_operations: i64,
    additional_bytes: i64,
    work: &mut W,
    limits:Option<super::OperatorStorage>,
) -> Result<WorkflowMeter, FinancialReadError> {
    if compare(additional_operations, 0, work)?.is_lt()
        || compare(additional_bytes, 0, work)?.is_lt()
    {
        return Err(StoreError::Integrity("negative workflow delta").into());
    }
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(bool, i64, i64)>())?;
    let (profile, w, wb): (bool, i64, i64) = sqlx::query_as(
        "SELECT profile='workflow-dual-legacy-capacity/1',operation_count,payload_bytes FROM workflow_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(i64, i64)>())?;
    let counted:(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(identity_key)+length(operation_bytes)+length(manifest_bytes)+length(receipt_bytes)),0)+(SELECT COALESCE(sum(length(canonical_bytes)),0) FROM workflow_documents) FROM workflow_operations").fetch_one(&mut *conn).await?;
    work.visit(ValidationVisit::Constraint)?;
    if !profile || !equal_fields(&[w, wb], &[counted.0, counted.1], work)? {
        return Err(StoreError::Integrity("workflow retained meter").into());
    }
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(i64, i64, i64, i64, i64)>())?;
    let (guard,e,a,eb,ab):(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT guard,entry_count,alias_count,entry_bytes,alias_bytes FROM billing_m3_bounds WHERE singleton=1").fetch_one(&mut *conn).await?;
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(i64, i64, i64, i64, i64)>())?;
    let actual_m3:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)+(SELECT count(*) FROM billing_m3_aliases),COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m2_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m3_entries),0),COALESCE((SELECT sum(length(ingress)) FROM billing_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m2_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m3_aliases),0),(SELECT count(*) FROM billing_m3_index)").fetch_one(&mut *conn).await?;
    if !equal_fields(
        &[
            actual_m3.0,
            actual_m3.1,
            actual_m3.2,
            actual_m3.3,
            actual_m3.4,
        ],
        &[e, a, eb, ab, e],
        work,
    )? || !compare(guard, arithmetic(e, a, Op::Add, work)?, work)?.is_eq()
        || compare(a, ROWS, work)?.is_gt()
        || compare(ab, 67108864, work)?.is_gt()
    {
        return Err(StoreError::Integrity("legacy M3 retention meter").into());
    }
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(i64, i64, i64, i64, i64, i64)>())?;
    let (r,cb,i,ib,next_command,next_record):(i64,i64,i64,i64,i64,i64)=sqlx::query_as("SELECT record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1").fetch_one(&mut *conn).await?;
    work.visit(ValidationVisit::RefLookup)?;
    work.write_bytes(std::mem::size_of::<(i64, i64, i64, i64, i64)>())?;
    let actual_m5:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_records),(SELECT count(*) FROM billing_m5_commands),(SELECT count(*) FROM billing_m5_activity_semantics)+(SELECT count(*) FROM billing_m5_activity_deliveries),COALESCE((SELECT sum(length(identity_key)+length(request_bytes)+length(response_bytes)) FROM billing_m5_commands),0)+COALESCE((SELECT sum(length(payload_bytes)) FROM billing_m5_records),0),COALESCE((SELECT sum(length(facts_bytes)) FROM billing_m5_activity_semantics),0)+COALESCE((SELECT sum(length(ingress_bytes)) FROM billing_m5_activity_deliveries),0)").fetch_one(&mut *conn).await?;
    let commands = arithmetic(next_command, 1, Op::Subtract, work)?;
    if !equal_fields(
        &[
            actual_m5.0,
            actual_m5.1,
            actual_m5.2,
            actual_m5.3,
            actual_m5.4,
        ],
        &[r, commands, i, cb, ib],
        work,
    )? || !compare(next_record, arithmetic(r, 1, Op::Add, work)?, work)?.is_eq()
    {
        return Err(StoreError::Integrity("legacy M5 retention meter").into());
    }
    // Repeat the subtraction at its original second executing site; no
    // historical/prior callback credit is inferred from equal operands.
    let commands = arithmetic(next_command, 1, Op::Subtract, work)?;
    let l = arithmetic(arithmetic(r, commands, Op::Add, work)?, i, Op::Add, work)?;
    let mb = arithmetic(cb, ib, Op::Add, work)?;
    check_values_with_work(
        e,
        l,
        eb,
        mb,
        w,
        wb,
        additional_operations,
        additional_bytes,
        work,
        limits,
    )?;
    Ok(WorkflowMeter {
        operations: w,
        payload_bytes: wb,
    })
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn check_values(
    e: i64,
    l: i64,
    eb: i64,
    mb: i64,
    w: i64,
    wb: i64,
    dw: i64,
    dwb: i64,
) -> Result<(), StoreError> {
    check_values_with_work(e, l, eb, mb, w, wb, dw, dwb, &mut NoopValidationWork,None).map_err(|error| {
        match error {
            FinancialReadError::Store(error) => error,
            FinancialReadError::Resource(_) => StoreError::Integrity("noop capacity work failed"),
        }
    })
}
#[allow(clippy::too_many_arguments)]
fn check_values_with_work(
    e: i64,
    l: i64,
    eb: i64,
    mb: i64,
    w: i64,
    wb: i64,
    dw: i64,
    dwb: i64,
    work: &mut dyn ValidationWork,
    limits:Option<super::OperatorStorage>,
) -> Result<(), FinancialReadError> {
    for value in [e, l, eb, mb, w, wb, dw, dwb] {
        work.visit(ValidationVisit::Element)?;
        if compare(value, 0, work)?.is_lt() {
            return Err(StoreError::Integrity("negative retained meter").into());
        }
    }
    let w = arithmetic(w, dw, Op::Add, work)?;
    let wb = arithmetic(wb, dwb, Op::Add, work)?;
    // Returning old data or checking integrity never consumes append quota.
    if dw>0 || dwb>0 {
      let rows=limits.map(|l|l.operations).unwrap_or(Some(ROWS));
      let bytes=limits.map(|l|l.bytes).unwrap_or(Some(BYTES));
      for (setting,maximum,values,current) in [("retained_operations",rows,[e,l],w),("retained_bytes",bytes,[eb,mb],wb)] {
       for value in values {
        work.visit(ValidationVisit::Element)?;
        let observed=arithmetic(value,current,Op::Add,work)?;
        if let Some(maximum)=maximum {if compare(observed,maximum,work)?.is_gt(){return Err(if limits.is_none(){StoreError::BillingHistoryLimit.into()}else{StoreError::ResourceQuota{setting,limit:maximum as u64,observed:observed as u64}.into()});}}
       }
      }
    }
    Ok(())
}
fn compare(
    a: i64,
    b: i64,
    work: &mut dyn ValidationWork,
) -> Result<std::cmp::Ordering, FinancialReadError> {
    work.arithmetic(Op::Compare, &[&a.into(), &b.into()])?;
    Ok(a.cmp(&b))
}
fn arithmetic(
    a: i64,
    b: i64,
    op: Op,
    work: &mut dyn ValidationWork,
) -> Result<i64, FinancialReadError> {
    work.arithmetic(op, &[&a.into(), &b.into()])?;
    match op {
        Op::Add => a.checked_add(b),
        Op::Subtract => a.checked_sub(b),
        _ => unreachable!("capacity integer operation"),
    }
    .ok_or_else(|| StoreError::BillingHistoryLimit.into())
}
fn equal_fields(
    a: &[i64],
    b: &[i64],
    work: &mut dyn ValidationWork,
) -> Result<bool, FinancialReadError> {
    // Only equal-length fixed tuples from the queries above call this helper.
    for (a, b) in a.iter().zip(b) {
        work.visit(ValidationVisit::Element)?;
        if !compare(*a, *b, work)?.is_eq() {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_b_preserves_legacy_compositions_and_charges_both_pools() {
        assert!(check_values(60000, 50000, 0, 0, 0, 0, 1, 1).is_ok());
        assert!(check_values(0, 100000, 0, 0, 0, 0, 1, 1).is_err());
        assert!(check_values(99999, 0, 0, 0, 1, 1, 1, 1).is_err());
        assert!(check_values(0, 99999, 0, 0, 1, 1, 1, 1).is_err());
        assert!(check_values(0, 0, 0, BYTES, 0, 0, 1, 1).is_err());
        assert!(check_values(99999, 99999, BYTES - 1, BYTES - 1, 1, 1, 0, 0).is_ok());
    }
}

pub(crate) async fn check_with_limits(conn:&mut SqliteConnection,operations:i64,bytes:i64,limits:Option<super::OperatorStorage>)->Result<WorkflowMeter,StoreError>{
 check_with_work(conn,operations,bytes,&mut NoopValidationWork,limits).await.map_err(|e|match e{FinancialReadError::Store(e)=>e,FinancialReadError::Resource(_)=>StoreError::ReadBudgetExhausted})
}
