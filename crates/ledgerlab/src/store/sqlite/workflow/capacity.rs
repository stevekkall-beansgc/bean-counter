//! Profile B payload accounting. This counts capacity units, never prices.
use crate::store::errors::StoreError;
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
    if additional_operations < 0 || additional_bytes < 0 {
        return Err(StoreError::Integrity("negative workflow delta"));
    }
    let (profile, w, wb): (String, i64, i64) = sqlx::query_as(
        "SELECT profile,operation_count,payload_bytes FROM workflow_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let counted:(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(identity_key)+length(operation_bytes)+length(manifest_bytes)+length(receipt_bytes)),0)+(SELECT COALESCE(sum(length(canonical_bytes)),0) FROM workflow_documents) FROM workflow_operations").fetch_one(&mut *conn).await?;
    if profile != PROFILE || (w, wb) != counted {
        return Err(StoreError::Integrity("workflow retained meter"));
    }
    let (guard,e,a,eb,ab):(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT guard,entry_count,alias_count,entry_bytes,alias_bytes FROM billing_m3_bounds WHERE singleton=1").fetch_one(&mut *conn).await?;
    let actual_m3:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)+(SELECT count(*) FROM billing_m3_aliases),COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m2_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m3_entries),0),COALESCE((SELECT sum(length(ingress)) FROM billing_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m2_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m3_aliases),0),(SELECT count(*) FROM billing_m3_index)").fetch_one(&mut *conn).await?;
    if actual_m3 != (e, a, eb, ab, e) || guard != e + a || a > ROWS || ab > 67108864 {
        return Err(StoreError::Integrity("legacy M3 retention meter"));
    }
    let (r,cb,i,ib,next_command,next_record):(i64,i64,i64,i64,i64,i64)=sqlx::query_as("SELECT record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1").fetch_one(&mut *conn).await?;
    let actual_m5:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_records),(SELECT count(*) FROM billing_m5_commands),(SELECT count(*) FROM billing_m5_activity_semantics)+(SELECT count(*) FROM billing_m5_activity_deliveries),COALESCE((SELECT sum(length(identity_key)+length(request_bytes)+length(response_bytes)) FROM billing_m5_commands),0)+COALESCE((SELECT sum(length(payload_bytes)) FROM billing_m5_records),0),COALESCE((SELECT sum(length(facts_bytes)) FROM billing_m5_activity_semantics),0)+COALESCE((SELECT sum(length(ingress_bytes)) FROM billing_m5_activity_deliveries),0)").fetch_one(&mut *conn).await?;
    if actual_m5 != (r, next_command - 1, i, cb, ib) || next_record != r + 1 {
        return Err(StoreError::Integrity("legacy M5 retention meter"));
    }
    let l = r
        .checked_add(next_command - 1)
        .and_then(|n| n.checked_add(i))
        .ok_or(StoreError::BillingHistoryLimit)?;
    let mb = cb.checked_add(ib).ok_or(StoreError::BillingHistoryLimit)?;
    check_values(e, l, eb, mb, w, wb, additional_operations, additional_bytes)?;
    Ok(WorkflowMeter {
        operations: w,
        payload_bytes: wb,
    })
}

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
    if [e, l, eb, mb, w, wb, dw, dwb].iter().any(|n| *n < 0) {
        return Err(StoreError::Integrity("negative retained meter"));
    }
    let w = w.checked_add(dw).ok_or(StoreError::BillingHistoryLimit)?;
    let wb = wb.checked_add(dwb).ok_or(StoreError::BillingHistoryLimit)?;
    if [e, l]
        .iter()
        .any(|n| n.checked_add(w).is_none_or(|n| n > ROWS))
        || [eb, mb]
            .iter()
            .any(|n| n.checked_add(wb).is_none_or(|n| n > BYTES))
    {
        return Err(StoreError::BillingHistoryLimit);
    }
    Ok(())
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
