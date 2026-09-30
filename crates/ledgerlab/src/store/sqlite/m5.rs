//! Additive M5 persistence primitives. The coordinator owns terms, authority,
//! arithmetic and statement composition; this module owns exact retained bytes.
use crate::store::errors::StoreError;
use ledgerlab_core::canonical::{parse_bounded, CanonicalBytes};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

const MAX_ROWS: i64 = 100_000;
const MAX_BYTES: i64 = 268_435_456;
const MIGRATION_ID: &str = "bean-counter/m5/schema-11/1";
// M3 ordinals are global across the original, M2, and M3 retained tiers.
const ASSIGNED_OUTCOMES: &str = "SELECT COALESCE((SELECT ingress FROM billing_entries WHERE ordinal=i.ordinal),(SELECT ingress FROM billing_m2_entries WHERE ordinal=i.ordinal),(SELECT ingress FROM billing_m3_entries WHERE ordinal=i.ordinal)),a.term_version,a.period_index FROM billing_m3_index i JOIN billing_m5_assignments a ON a.source_stream='m3' AND a.source_sequence=i.ordinal WHERE i.customer=? AND i.source=? AND i.target=? AND i.kind='outcome' ORDER BY i.ordinal";

type StoredCommandRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64);
type RetainedRecord = (i64, String, String, i64, Vec<u8>, Vec<u8>);
type CommandIntegrityRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64);
type ActivityIdentityRow = (String, String, String, i64, i64, Vec<u8>, Vec<u8>);
type ResolutionProjectionRow = (String, i64, i64, String, i64, i64, i64, Option<String>);
type AssignmentProjectionRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    String,
    i64,
);
type AdjustmentProjectionRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    i64,
    String,
);
type PeriodCloseProjectionRow = (
    String,
    i64,
    i64,
    String,
    i64,
    i64,
    i64,
    String,
    i64,
    Vec<u8>,
);
type PeriodCloseM3Row = (
    i64,
    String,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    Option<String>,
    Option<i64>,
);
type PresentableProjectionRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
);
type PresentableSourceRow = (String, String, Vec<u8>, Vec<u8>, String, i64);
type ClaimReconciliationRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    Vec<u8>,
    String,
    i64,
);

#[derive(Clone)]
pub(crate) struct Child<'a> {
    pub family: &'a str,
    pub customer: Option<&'a str>,
    pub source: Option<&'a str>,
    pub child_key: &'a [u8],
    pub payload: &'a [u8],
}

pub(crate) struct Command<'a> {
    pub family: &'a str,
    pub domain: &'a str,
    pub customer: Option<&'a str>,
    pub source: Option<&'a str>,
    pub identity_key: &'a [u8],
    pub accepted_at_us: i64,
    pub request: &'a [u8],
    pub response: &'a [u8],
    /// Already sorted by canonical child-key bytes; every child has a stable
    /// natural key and an exact canonical payload envelope.
    pub children: &'a [Child<'a>],
}

#[derive(Debug)]
pub(crate) struct StoredCommand {
    #[cfg(test)]
    pub sequence: i64,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    #[cfg(test)]
    pub records: Vec<(i64, String, String, Vec<u8>)>,
}

pub(crate) fn hash(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(bytes);
    h.finalize().into()
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
fn canonical(bytes: &[u8], max: usize) -> Result<Value, StoreError> {
    let value =
        parse_bounded(bytes, max).map_err(|_| StoreError::Integrity("M5 canonical JSON"))?;
    if CanonicalBytes::from_value(&value)
        .map_err(|_| StoreError::Integrity("M5 canonical JSON"))?
        .as_slice()
        != bytes
    {
        return Err(StoreError::Integrity("M5 noncanonical JSON"));
    }
    Ok(value)
}
fn bytes_len(parts: &[&[u8]]) -> Result<i64, StoreError> {
    parts.iter().try_fold(0i64, |n, part| {
        n.checked_add(part.len() as i64)
            .ok_or(StoreError::BillingHistoryLimit)
    })
}
pub(crate) fn record_id(identity: &[u8], child_key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"bean-counter/m5/record-id/1\0");
    h.update(identity);
    h.update([0]);
    h.update(child_key);
    format!("m5r_{}", hex(&h.finalize()))
}
fn payload_hash(family: &str, payload: &Value) -> Result<[u8; 32], StoreError> {
    let mut unsigned = payload.clone();
    unsigned["record"]
        .as_object_mut()
        .ok_or(StoreError::Integrity("M5 record envelope"))?
        .remove("payload_hash");
    let bytes =
        CanonicalBytes::from_value(&unsigned).map_err(|_| StoreError::Integrity("M5 payload"))?;
    let mut h = Sha256::new();
    h.update(b"bean-counter/m5/record/1\0");
    h.update(family.as_bytes());
    h.update([0]);
    h.update(bytes.as_slice());
    Ok(h.finalize().into())
}

pub(crate) fn seal_child(family: &str, payload: &mut Value) -> Result<Vec<u8>, StoreError> {
    let digest = payload_hash(family, payload)?;
    payload["record"]["payload_hash"] = Value::String(hex(&digest));
    Ok(CanonicalBytes::from_value(payload)
        .map_err(|_| StoreError::Integrity("M5 child encoding"))?
        .into_vec())
}

#[derive(Debug)]
pub(crate) struct TermHistory {
    pub ordinal: i64,
    pub customer: String,
    pub source: String,
    pub record_id: String,
    pub kind: String,
    pub accepted_at_us: i64,
}

pub(crate) struct TermState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub revision: i64,
    pub next_term_version: i64,
    pub history: Vec<TermHistory>,
}

pub(crate) struct FiscalState {
    pub revision: i64,
    pub next_calendar_version: i64,
    pub command_sequence: i64,
    pub first_record_sequence: i64,
}

pub(crate) struct FiscalReportSource {
    pub ordinal: i64,
    pub customer: String,
    pub source: String,
    pub kind: String,
    pub id: String,
    pub accepted_at_us: i64,
    pub ingress: Vec<u8>,
    pub bundle: Vec<u8>,
    pub agreement_id: Option<String>,
    pub agreement_version: Option<i64>,
}

pub(crate) struct FiscalReportState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub timezone: String,
    pub timezone_rules_version: String,
    pub calendar_bytes: Vec<u8>,
    pub snapshot_boundary_id: i64,
    pub m3_high_water: i64,
    pub m5_high_water: i64,
    pub sources: Vec<FiscalReportSource>,
}

#[derive(Debug, Clone)]
pub(crate) struct RecurrenceVersion {
    pub agreement_id: String,
    pub agreement_version: i64,
    pub recurrence_version: i64,
    pub rule: Value,
    pub cancelled_at_us: Option<i64>,
}

pub(crate) struct RecurrenceState {
    pub revision: i64,
    pub next_version: i64,
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub versions: Vec<RecurrenceVersion>,
}

pub(crate) async fn recurrence_state(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
) -> Result<RecurrenceState, StoreError> {
    let schema: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if schema != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let rows: Vec<(String,i64,i64,Vec<u8>,Option<i64>)> = sqlx::query_as(
        "SELECT v.agreement_id,v.agreement_version,v.recurrence_version,v.rule_bytes,c.cancelled_at_us FROM billing_m5_recurrence_versions v LEFT JOIN billing_m5_recurrence_cancellations c ON c.customer=v.customer AND c.source=v.source AND c.recurrence_version=v.recurrence_version WHERE v.customer=? AND v.source=? ORDER BY v.recurrence_version"
    ).bind(customer).bind(source).fetch_all(&mut *conn).await?;
    let mut versions = Vec::with_capacity(rows.len());
    for (i, (agreement_id, agreement_version, recurrence_version, rule_bytes, cancelled_at_us)) in
        rows.into_iter().enumerate()
    {
        if recurrence_version != i as i64 + 1 {
            return Err(StoreError::InvalidStore("M5 recurrence sequence"));
        }
        versions.push(RecurrenceVersion {
            agreement_id,
            agreement_version,
            recurrence_version,
            rule: canonical(&rule_bytes, 262_144)?,
            cancelled_at_us,
        });
    }
    let revision: i64=sqlx::query_scalar("SELECT count(*) FROM billing_m5_commands WHERE family IN ('ledger-billing-recurrence/1','ledger-billing-recurrence-cancel/1') AND customer=? AND source=?")
        .bind(customer).bind(source).fetch_one(&mut *conn).await?;
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(RecurrenceState {
        revision,
        next_version: versions.len() as i64 + 1,
        command_sequence,
        first_record_sequence,
        versions,
    })
}

pub(crate) async fn append_recurrence_versions(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
) -> Result<(), StoreError> {
    append(conn, command).await?;
    for child in command.children {
        if child.family != "ledger-billing-recurrence-version/1" {
            return Err(StoreError::Integrity("M5 recurrence child"));
        }
        let payload = canonical(child.payload, 262_144)?;
        let customer = payload["customer"]
            .as_str()
            .ok_or(StoreError::Integrity("M5 recurrence customer"))?;
        let source = payload["source"]
            .as_str()
            .ok_or(StoreError::Integrity("M5 recurrence source"))?;
        let agreement_id = payload["agreement_id"]
            .as_str()
            .ok_or(StoreError::Integrity("M5 recurrence agreement"))?;
        let agreement_version = decimal(&payload["agreement_version"])?;
        let recurrence_version = decimal(&payload["recurrence_version"])?;
        let next:i64=sqlx::query_scalar("SELECT COALESCE(max(recurrence_version),0)+1 FROM billing_m5_recurrence_versions WHERE customer=? AND source=?")
            .bind(customer).bind(source).fetch_one(&mut *conn).await?;
        if Some(customer) != command.customer
            || Some(source) != command.source
            || recurrence_version != next
            || agreement_version < 1
        {
            return Err(StoreError::Integrity("M5 recurrence version"));
        }
        let rule = CanonicalBytes::from_value(&payload["rule"])
            .map_err(|_| StoreError::Integrity("M5 recurrence rule"))?;
        let renewal = CanonicalBytes::from_value(&payload["renewal"])
            .map_err(|_| StoreError::Integrity("M5 recurrence renewal"))?;
        sqlx::query("INSERT INTO billing_m5_recurrence_versions(customer,source,agreement_id,agreement_version,recurrence_version,record_sequence,rule_bytes,renewal_bytes) VALUES(?,?,?,?,?,?,?,?)")
            .bind(customer).bind(source).bind(agreement_id).bind(agreement_version).bind(recurrence_version)
            .bind(payload["record"]["sequence"].as_str().and_then(|s|s.parse::<i64>().ok()).ok_or(StoreError::Integrity("M5 recurrence sequence"))?)
            .bind(rule.as_slice()).bind(renewal.as_slice()).execute(&mut *conn).await?;
    }
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn append_recurrence_cancel(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
) -> Result<(), StoreError> {
    if command.children.len() != 1 {
        return Err(StoreError::Integrity("M5 cancellation child"));
    }
    append(conn, command).await?;
    let payload = canonical(command.children[0].payload, 262_144)?;
    if payload["customer"] != command.customer.unwrap_or("")
        || payload["source"] != command.source.unwrap_or("")
    {
        return Err(StoreError::Integrity("M5 cancellation scope"));
    }
    sqlx::query("INSERT INTO billing_m5_recurrence_cancellations(customer,source,recurrence_version,cancelled_at_us,record_sequence) VALUES(?,?,?,?,?)")
        .bind(command.customer.ok_or(StoreError::Integrity("M5 cancellation customer"))?)
        .bind(command.source.ok_or(StoreError::Integrity("M5 cancellation source"))?)
        .bind(decimal(&payload["recurrence_version"])?).bind(command.accepted_at_us)
        .bind(decimal(&payload["record"]["sequence"])?).execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn append_occurrence(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    receipt_id: &str,
) -> Result<(), StoreError> {
    if command.children.len() != 1
        || command.children[0].family != "ledger-billing-occurrence-acceptance-record/1"
    {
        return Err(StoreError::Integrity("M5 occurrence child"));
    }
    append(conn, command).await?;
    let payload = canonical(command.children[0].payload, 262_144)?;
    if payload["accepted_m3_receipt_id"] != receipt_id
        || payload["customer"] != command.customer.unwrap_or("")
        || payload["source"] != command.source.unwrap_or("")
    {
        return Err(StoreError::Integrity("M5 occurrence receipt"));
    }
    sqlx::query("INSERT INTO billing_m5_occurrence_acceptances(customer,source,occurrence_id,accepted_m3_receipt_id,record_sequence) VALUES(?,?,?,?,?)")
        .bind(command.customer.ok_or(StoreError::Integrity("M5 occurrence customer"))?)
        .bind(command.source.ok_or(StoreError::Integrity("M5 occurrence source"))?)
        .bind(payload["occurrence_id"].as_str().ok_or(StoreError::Integrity("M5 occurrence ID"))?)
        .bind(receipt_id).bind(decimal(&payload["record"]["sequence"])?).execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn occurrence_accepted(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    id: &str,
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m5_occurrence_acceptances WHERE customer=? AND source=? AND occurrence_id=?)")
        .bind(customer).bind(source).bind(id).fetch_one(&mut *conn).await.map_err(StoreError::from)
}

pub(crate) async fn fiscal_state(conn: &mut SqliteConnection) -> Result<FiscalState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let (count, maximum): (i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(max(calendar_version),0) FROM billing_m5_fiscal_versions",
    )
    .fetch_one(&mut *conn)
    .await?;
    if count != maximum {
        return Err(StoreError::InvalidStore("M5 fiscal version sequence"));
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(FiscalState {
        revision: count,
        next_calendar_version: maximum + 1,
        command_sequence,
        first_record_sequence,
    })
}

pub(crate) async fn fiscal_report_state(
    conn: &mut SqliteConnection,
    calendar_version: i64,
    requested_snapshot: Option<(i64, i64)>,
    start_at_us: i64,
    end_at_us: i64,
) -> Result<FiscalReportState, StoreError> {
    if calendar_version < 1 || end_at_us <= start_at_us {
        return Err(StoreError::BillingPeriod);
    }
    let (timezone, timezone_rules_version, calendar_bytes): (String, String, Vec<u8>) =
        sqlx::query_as("SELECT timezone,timezone_rules_version,calendar_bytes FROM billing_m5_fiscal_versions WHERE calendar_version=?")
            .bind(calendar_version)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(StoreError::BillingPeriod)?;
    if requested_snapshot.is_none() {
        let active: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(calendar_version),0) FROM billing_m5_fiscal_versions",
        )
        .fetch_one(&mut *conn)
        .await?;
        if active != calendar_version {
            return Err(StoreError::BillingPeriod);
        }
    }
    let boundary: Option<(i64, i64, i64)> = match requested_snapshot {
        Some((m3, m5)) => sqlx::query_as(
            "SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE m3_high_water=? AND m5_high_water=? ORDER BY boundary_id DESC LIMIT 1",
        )
        .bind(m3)
        .bind(m5)
        .fetch_optional(&mut *conn)
        .await?,
        None => sqlx::query_as(
            "SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id DESC LIMIT 1",
        )
        .fetch_optional(&mut *conn)
        .await?,
    };
    let (snapshot_boundary_id, m3_high_water, m5_high_water) =
        boundary.ok_or(StoreError::BillingPeriod)?;
    type FiscalSourceRow = (
        i64,
        String,
        String,
        String,
        String,
        i64,
        Vec<u8>,
        Vec<u8>,
        Option<String>,
        Option<i64>,
    );
    let rows: Vec<FiscalSourceRow> = sqlx::query_as(
        "SELECT a.source_sequence,a.customer,a.source_scope,a.source_record_kind,a.source_record_id,i.accepted_at_us,e.ingress,e.bundle,g.agreement_id,g.agreement_version FROM billing_m5_assignments a JOIN billing_m3_index i ON i.ordinal=a.source_sequence JOIN billing_entries e ON e.ordinal=a.source_sequence JOIN billing_setup s ON s.singleton=1 JOIN billing_agreements g ON g.customer=a.customer AND g.source=a.source_scope AND g.revision=1 AND g.transition='start' AND g.setup_bytes=s.canonical_bytes WHERE a.source_stream='m3' AND a.source_sequence<=? AND i.accepted_at_us<? UNION ALL SELECT a.source_sequence,a.customer,a.source_scope,a.source_record_kind,a.source_record_id,i.accepted_at_us,e.ingress,e.bundle,e.agreement_id,e.agreement_version FROM billing_m5_assignments a JOIN billing_m3_index i ON i.ordinal=a.source_sequence JOIN billing_m2_entries e ON e.ordinal=a.source_sequence WHERE a.source_stream='m3' AND a.source_sequence<=? AND i.accepted_at_us<? UNION ALL SELECT a.source_sequence,a.customer,a.source_scope,a.source_record_kind,a.source_record_id,i.accepted_at_us,e.ingress,e.bundle,e.agreement_id,e.agreement_version FROM billing_m5_assignments a JOIN billing_m3_index i ON i.ordinal=a.source_sequence JOIN billing_m3_entries e ON e.ordinal=a.source_sequence WHERE a.source_stream='m3' AND a.source_sequence<=? AND i.accepted_at_us<? ORDER BY 1",
    )
    .bind(m3_high_water).bind(end_at_us)
    .bind(m3_high_water).bind(end_at_us)
    .bind(m3_high_water).bind(end_at_us)
    .fetch_all(&mut *conn)
    .await?;
    let mut ordinals = BTreeSet::new();
    if rows.iter().any(|row| !ordinals.insert(row.0)) {
        return Err(StoreError::InvalidStore("M5 fiscal source tier"));
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(FiscalReportState {
        command_sequence,
        first_record_sequence,
        timezone,
        timezone_rules_version,
        calendar_bytes,
        snapshot_boundary_id,
        m3_high_water,
        m5_high_water,
        sources: rows
            .into_iter()
            .map(
                |(
                    ordinal,
                    customer,
                    source,
                    kind,
                    id,
                    accepted_at_us,
                    ingress,
                    bundle,
                    agreement_id,
                    agreement_version,
                )| FiscalReportSource {
                    ordinal,
                    customer,
                    source,
                    kind,
                    id,
                    accepted_at_us,
                    ingress,
                    bundle,
                    agreement_id,
                    agreement_version,
                },
            )
            .collect(),
    })
}

pub(crate) struct CurrentTerm {
    pub term_version: i64,
    pub effective_at_us: i64,
    pub term_bytes: Vec<u8>,
    pub payload_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolutionHead {
    pub resolution_id: String,
    pub record_sequence: i64,
    pub start_at_us: i64,
    pub end_at_us: i64,
}

pub(crate) struct TermTransitionState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub revision: i64,
    pub next_term_version: i64,
    pub current: CurrentTerm,
    pub has_pending_successor: bool,
}

pub(crate) struct PeriodResolveState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub term: CurrentTerm,
    pub successor_effective_at_us: Option<i64>,
    pub existing: Option<ResolutionHead>,
}

pub(crate) struct PeriodCloseState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub term: CurrentTerm,
    pub successor_effective_at_us: Option<i64>,
    pub resolution: Option<ResolutionHead>,
    pub snapshot_boundary_id: i64,
    pub m3_high_water: i64,
    pub m5_high_water: i64,
    pub m3_assignments: Vec<PeriodCloseM3Assignment>,
    pub adjustments: Vec<PresentableAdjustment>,
    pub unsupported_assignment_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresentableAdjustment {
    pub source: String,
    pub adjustment_id: String,
    pub cause_kind: String,
    pub target_id: String,
    pub original_term_version: i64,
    pub original_period_index: i64,
    pub assigned_term_version: i64,
    pub assigned_period_index: i64,
    pub source_stream: String,
    pub source_sequence: i64,
    pub signed_delta_atoms: String,
    pub source_record_id: String,
    pub source_record_kind: String,
    pub bundle: Vec<u8>,
    pub ingress: Vec<u8>,
    pub agreement_id: String,
    pub agreement_version: i64,
    pub prior_outcome_id: Option<String>,
}

async fn prior_outcome_id(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    target: &str,
    before_sequence: i64,
    family: &str,
) -> Result<Option<String>, StoreError> {
    let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
        "SELECT COALESCE((SELECT ingress FROM billing_entries WHERE ordinal=i.ordinal),(SELECT ingress FROM billing_m2_entries WHERE ordinal=i.ordinal),(SELECT ingress FROM billing_m3_entries WHERE ordinal=i.ordinal)) FROM billing_m3_index i WHERE i.customer=? AND i.source=? AND i.target=? AND i.ordinal<? AND i.kind IN ('outcome','correction') ORDER BY i.ordinal DESC"
    ).bind(customer).bind(source).bind(target).bind(before_sequence).fetch_all(&mut *conn).await?;
    for (ingress,) in rows {
        let ingress = parse_bounded(&ingress, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 prior outcome ingress"))?;
        if ingress["family"] == family {
            return Ok(Some(
                ingress["id"]
                    .as_str()
                    .ok_or(StoreError::InvalidStore("M5 prior outcome id"))?
                    .to_owned(),
            ));
        }
    }
    Ok(None)
}

pub(crate) async fn unclaimed_adjustments(
    conn: &mut SqliteConnection,
    customer: &str,
    assigned_period: Option<(i64, i64)>,
    m3_high_water: i64,
    m5_high_water: i64,
) -> Result<Vec<PresentableAdjustment>, StoreError> {
    presentable_adjustments(
        conn,
        customer,
        assigned_period,
        m3_high_water,
        m5_high_water,
        None,
        None,
    )
    .await
}

async fn presentable_adjustments(
    conn: &mut SqliteConnection,
    customer: &str,
    assigned_period: Option<(i64, i64)>,
    m3_high_water: i64,
    m5_high_water: i64,
    claim_statement: Option<(&str, &str)>,
    requested: Option<&BTreeSet<(String, String)>>,
) -> Result<Vec<PresentableAdjustment>, StoreError> {
    let rows: Vec<PresentableProjectionRow> = sqlx::query_as(
        "SELECT a.source_scope,a.adjustment_id,a.cause_kind,a.target_id,a.original_term_version,a.original_period_index,a.assigned_term_version,a.assigned_period_index,a.source_stream,a.source_sequence,a.signed_delta_atoms,p.presentation_kind,p.statement_id FROM billing_m5_adjustments a LEFT JOIN billing_m5_presentation_claims p ON p.customer=a.customer AND p.source_scope=a.source_scope AND p.adjustment_id=a.adjustment_id WHERE a.customer=? ORDER BY a.source_scope,a.adjustment_id"
    ).bind(customer).fetch_all(&mut *conn).await?;
    let mut result = Vec::new();
    for (
        source,
        adjustment_id,
        cause_kind,
        target_id,
        original_term_version,
        original_period_index,
        assigned_term_version,
        assigned_period_index,
        source_stream,
        source_sequence,
        signed_delta_atoms,
        claim_kind,
        statement_id,
    ) in rows
    {
        if requested.is_some_and(|keys| !keys.contains(&(source.clone(), adjustment_id.clone()))) {
            continue;
        }
        if match claim_statement {
            None => statement_id.is_some(),
            Some((kind, expected)) => {
                statement_id.as_deref() != Some(expected) || claim_kind.as_deref() != Some(kind)
            }
        } {
            continue;
        }
        if assigned_period
            .is_some_and(|period| period != (assigned_term_version, assigned_period_index))
        {
            continue;
        }
        if (source_stream == "m3" && source_sequence > m3_high_water)
            || (source_stream == "m5" && source_sequence > m5_high_water)
        {
            continue;
        }
        let original_closed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?)")
            .bind(customer).bind(original_term_version).bind(original_period_index)
            .fetch_one(&mut *conn).await?;
        if !original_closed {
            return Err(StoreError::InvalidStore("M5 unclosed adjustment origin"));
        }
        // The M4 correction remains the authoritative monetary source. Its
        // adjustment is only a projection and does not advance the M5 stream.
        if source_stream != "m3" || cause_kind != "outcome-correction" {
            return Err(StoreError::BillingPeriod);
        }
        let retained: Option<PresentableSourceRow> = sqlx::query_as(
            "SELECT a.source_record_id,a.source_record_kind,e.bundle,e.ingress,e.agreement_id,e.agreement_version FROM billing_m5_assignments a JOIN billing_m3_entries e ON e.ordinal=a.source_sequence WHERE a.source_stream='m3' AND a.source_sequence=? AND a.customer=? AND a.source_scope=? AND a.assignment_basis='post-close-adjustment'"
        ).bind(source_sequence).bind(customer).bind(&source).fetch_optional(&mut *conn).await?;
        let (
            source_record_id,
            source_record_kind,
            bundle,
            ingress,
            agreement_id,
            agreement_version,
        ) = retained.ok_or(StoreError::InvalidStore("M5 adjustment source"))?;
        let parsed_ingress = parse_bounded(&ingress, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 adjustment ingress"))?;
        let family = parsed_ingress["family"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 adjustment family"))?;
        let prior_outcome_id =
            prior_outcome_id(conn, customer, &source, &target_id, source_sequence, family).await?;
        result.push(PresentableAdjustment {
            source,
            adjustment_id,
            cause_kind,
            target_id,
            original_term_version,
            original_period_index,
            assigned_term_version,
            assigned_period_index,
            source_stream,
            source_sequence,
            signed_delta_atoms,
            source_record_id,
            source_record_kind,
            bundle,
            ingress,
            agreement_id,
            agreement_version,
            prior_outcome_id,
        });
    }
    Ok(result)
}

pub(crate) struct PeriodCloseM3Assignment {
    pub source: String,
    pub kind: String,
    pub id: String,
    pub bundle: Vec<u8>,
    pub ingress: Vec<u8>,
    pub agreement_id: Option<String>,
    pub agreement_version: Option<i64>,
    pub prior_outcome_id: Option<String>,
}

pub(crate) struct AdHocState {
    pub command_sequence: i64,
    pub first_record_sequence: i64,
    pub adjustments: Vec<PresentableAdjustment>,
}

pub(crate) async fn ad_hoc_state(
    conn: &mut SqliteConnection,
    customer: &str,
    references: &[(String, String)],
) -> Result<AdHocState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let (m3_high_water,m5_high_water): (i64,i64) = sqlx::query_as(
        "SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id DESC LIMIT 1"
    ).fetch_one(&mut *conn).await?;
    let requested: BTreeSet<_> = references.iter().cloned().collect();
    let adjustments = presentable_adjustments(
        conn,
        customer,
        None,
        m3_high_water,
        m5_high_water,
        None,
        Some(&requested),
    )
    .await?;
    Ok(AdHocState {
        command_sequence,
        first_record_sequence,
        adjustments,
    })
}

pub(crate) async fn adjustment_claimed(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    adjustment_id: &str,
) -> Result<Option<bool>, StoreError> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT p.statement_id FROM billing_m5_adjustments a LEFT JOIN billing_m5_presentation_claims p ON p.customer=a.customer AND p.source_scope=a.source_scope AND p.adjustment_id=a.adjustment_id WHERE a.customer=? AND a.source_scope=? AND a.adjustment_id=?"
    ).bind(customer).bind(source).bind(adjustment_id).fetch_optional(&mut *conn).await?;
    Ok(row.map(|(claim,)| claim.is_some()))
}

async fn period_close_m3_assignments(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
    m3_high_water: i64,
) -> Result<Vec<PeriodCloseM3Assignment>, StoreError> {
    let assignment_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_assignments WHERE customer=? AND term_version=? AND period_index=? AND source_stream='m3' AND assignment_basis!='post-close-adjustment' AND source_sequence<=?",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m3_high_water)
    .fetch_one(&mut *conn)
    .await?;
    let rows: Vec<PeriodCloseM3Row> = sqlx::query_as(
        "SELECT a.source_sequence,a.source_scope,a.source_record_kind,a.source_record_id,e.bundle,e.ingress,g.agreement_id,g.agreement_version FROM billing_m5_assignments a JOIN billing_entries e ON e.ordinal=a.source_sequence JOIN billing_setup s ON s.singleton=1 JOIN billing_agreements g ON g.customer=a.customer AND g.source=a.source_scope AND g.revision=1 AND g.transition='start' AND g.setup_bytes=s.canonical_bytes WHERE a.customer=? AND a.term_version=? AND a.period_index=? AND a.source_stream='m3' AND a.assignment_basis!='post-close-adjustment' AND a.source_sequence<=? UNION ALL SELECT a.source_sequence,a.source_scope,a.source_record_kind,a.source_record_id,e.bundle,e.ingress,e.agreement_id,e.agreement_version FROM billing_m5_assignments a JOIN billing_m2_entries e ON e.ordinal=a.source_sequence WHERE a.customer=? AND a.term_version=? AND a.period_index=? AND a.source_stream='m3' AND a.assignment_basis!='post-close-adjustment' AND a.source_sequence<=? UNION ALL SELECT a.source_sequence,a.source_scope,a.source_record_kind,a.source_record_id,e.bundle,e.ingress,e.agreement_id,e.agreement_version FROM billing_m5_assignments a JOIN billing_m3_entries e ON e.ordinal=a.source_sequence WHERE a.customer=? AND a.term_version=? AND a.period_index=? AND a.source_stream='m3' AND a.assignment_basis!='post-close-adjustment' AND a.source_sequence<=? ORDER BY 1,2,3,4",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m3_high_water)
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m3_high_water)
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m3_high_water)
    .fetch_all(&mut *conn)
    .await?;
    let mut matches = BTreeMap::<i64, usize>::new();
    for (source_sequence, ..) in &rows {
        *matches.entry(*source_sequence).or_default() += 1;
    }
    if rows.len() != usize::try_from(assignment_count).unwrap_or(usize::MAX)
        || matches.len() != usize::try_from(assignment_count).unwrap_or(usize::MAX)
        || matches.values().any(|count| *count != 1)
    {
        return Err(StoreError::InvalidStore("M5 close assignment tier"));
    }
    let mut assignments = Vec::with_capacity(rows.len());
    for (sequence, source, kind, id, bundle, ingress, agreement_id, agreement_version) in rows {
        let prior = if kind == "receipt" {
            let parsed = parse_bounded(&ingress, 262_144)
                .map_err(|_| StoreError::InvalidStore("M5 outcome ingress"))?;
            let family = parsed["family"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 outcome family"))?;
            let target = parsed["target"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 outcome target"))?;
            prior_outcome_id(conn, customer, &source, target, sequence, family).await?
        } else {
            None
        };
        assignments.push(PeriodCloseM3Assignment {
            source,
            kind,
            id,
            bundle,
            ingress,
            agreement_id,
            agreement_version,
            prior_outcome_id: prior,
        });
    }
    Ok(assignments)
}

pub(crate) async fn term_state(
    conn: &mut SqliteConnection,
    customer: &str,
) -> Result<TermState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let revision: i64 =
        sqlx::query_scalar("SELECT count(*) FROM billing_m5_term_versions WHERE customer=?")
            .bind(customer)
            .fetch_one(&mut *conn)
            .await?;
    let next_term_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(term_version),0)+1 FROM billing_m5_term_versions")
            .fetch_one(&mut *conn)
            .await?;
    let rows: Vec<(i64,String,String,String,i64)> = sqlx::query_as(
        "SELECT ordinal,customer,source,kind,accepted_at_us FROM billing_m3_index WHERE customer=? ORDER BY ordinal"
    ).bind(customer).fetch_all(&mut *conn).await?;
    Ok(TermState {
        command_sequence,
        first_record_sequence,
        revision,
        next_term_version,
        history: rows
            .into_iter()
            .map(
                |(ordinal, customer, source, kind, accepted_at_us)| TermHistory {
                    ordinal,
                    customer,
                    source,
                    record_id: String::new(),
                    kind,
                    accepted_at_us,
                },
            )
            .collect(),
    })
}

pub(crate) async fn term_transition_state(
    conn: &mut SqliteConnection,
    customer: &str,
    accepted_at_us: i64,
) -> Result<TermTransitionState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let revision: i64 =
        sqlx::query_scalar("SELECT count(*) FROM billing_m5_term_versions WHERE customer=?")
            .bind(customer)
            .fetch_one(&mut *conn)
            .await?;
    let next_term_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(term_version),0)+1 FROM billing_m5_term_versions")
            .fetch_one(&mut *conn)
            .await?;
    let current: Option<(i64, i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT t.term_version,t.effective_at_us,t.term_bytes,r.payload_bytes FROM billing_m5_term_versions t JOIN billing_m5_records r ON r.sequence=t.record_sequence WHERE t.customer=? AND t.effective_at_us<=? ORDER BY t.effective_at_us DESC,t.term_version DESC LIMIT 1",
    )
    .bind(customer)
    .bind(accepted_at_us)
    .fetch_optional(&mut *conn)
    .await?;
    let (term_version, effective_at_us, term_bytes, payload_bytes) =
        current.ok_or(StoreError::BillingPeriod)?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_term_versions WHERE customer=? AND effective_at_us>?",
    )
    .bind(customer)
    .bind(accepted_at_us)
    .fetch_one(&mut *conn)
    .await?;
    Ok(TermTransitionState {
        command_sequence,
        first_record_sequence,
        revision,
        next_term_version,
        current: CurrentTerm {
            term_version,
            effective_at_us,
            term_bytes,
            payload_bytes,
        },
        has_pending_successor: pending != 0,
    })
}

pub(crate) async fn resolution_head(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
) -> Result<Option<ResolutionHead>, StoreError> {
    let row: Option<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT resolution_id,record_sequence,start_at_us,end_at_us FROM billing_m5_period_resolutions WHERE customer=? AND term_version=? AND period_index=? ORDER BY record_sequence DESC LIMIT 1",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(
        |(resolution_id, record_sequence, start_at_us, end_at_us)| ResolutionHead {
            resolution_id,
            record_sequence,
            start_at_us,
            end_at_us,
        },
    ))
}

pub(crate) async fn period_resolve_state(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
) -> Result<PeriodResolveState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let term: Option<(i64, i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT t.term_version,t.effective_at_us,t.term_bytes,r.payload_bytes FROM billing_m5_term_versions t JOIN billing_m5_records r ON r.sequence=t.record_sequence WHERE t.customer=? AND t.term_version=?",
    )
    .bind(customer)
    .bind(term_version)
    .fetch_optional(&mut *conn)
    .await?;
    let (term_version, effective_at_us, term_bytes, payload_bytes) =
        term.ok_or(StoreError::BillingPeriod)?;
    let successor_effective_at_us: Option<i64> = sqlx::query_scalar(
        "SELECT effective_at_us FROM billing_m5_term_versions WHERE customer=? AND term_version>? ORDER BY term_version LIMIT 1",
    )
    .bind(customer)
    .bind(term_version)
    .fetch_optional(&mut *conn)
    .await?;
    let existing = resolution_head(conn, customer, term_version, period_index).await?;
    Ok(PeriodResolveState {
        command_sequence,
        first_record_sequence,
        term: CurrentTerm {
            term_version,
            effective_at_us,
            term_bytes,
            payload_bytes,
        },
        successor_effective_at_us,
        existing,
    })
}

pub(crate) async fn period_close_state(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
) -> Result<PeriodCloseState, StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    if version != 11 {
        return Err(StoreError::BillingUpgradeRequired);
    }
    let already_closed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?)",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .fetch_one(&mut *conn)
    .await?;
    if already_closed {
        return Err(StoreError::InvalidStore("M5 duplicate period close"));
    }
    let (command_sequence, first_record_sequence): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    let term: Option<(i64, i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT t.term_version,t.effective_at_us,t.term_bytes,r.payload_bytes FROM billing_m5_term_versions t JOIN billing_m5_records r ON r.sequence=t.record_sequence WHERE t.customer=? AND t.term_version=?",
    )
    .bind(customer)
    .bind(term_version)
    .fetch_optional(&mut *conn)
    .await?;
    let (stored_version, effective_at_us, term_bytes, payload_bytes) =
        term.ok_or(StoreError::BillingPeriod)?;
    let successor_effective_at_us: Option<i64> = sqlx::query_scalar(
        "SELECT effective_at_us FROM billing_m5_term_versions WHERE customer=? AND term_version>? ORDER BY term_version LIMIT 1",
    )
    .bind(customer)
    .bind(term_version)
    .fetch_optional(&mut *conn)
    .await?;
    let resolution = resolution_head(conn, customer, term_version, period_index).await?;
    let (snapshot_boundary_id, m3_high_water, m5_high_water): (i64, i64, i64) =
        sqlx::query_as(
            "SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id DESC LIMIT 1",
        )
        .fetch_one(&mut *conn)
        .await?;
    if resolution
        .as_ref()
        .is_some_and(|resolution| resolution.record_sequence > m5_high_water)
    {
        return Err(StoreError::InvalidStore("M5 close resolution snapshot"));
    }
    let m3_assignments =
        period_close_m3_assignments(conn, customer, term_version, period_index, m3_high_water)
            .await?;
    let adjustments = unclaimed_adjustments(
        conn,
        customer,
        Some((term_version, period_index)),
        m3_high_water,
        m5_high_water,
    )
    .await?;
    let unsupported_assignment_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_assignments WHERE customer=? AND term_version=? AND period_index=? AND source_stream='m5' AND source_sequence<=?",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m5_high_water)
    .fetch_one(&mut *conn)
    .await?;
    Ok(PeriodCloseState {
        command_sequence,
        first_record_sequence,
        term: CurrentTerm {
            term_version: stored_version,
            effective_at_us,
            term_bytes,
            payload_bytes,
        },
        successor_effective_at_us,
        resolution,
        snapshot_boundary_id,
        m3_high_water,
        m5_high_water,
        m3_assignments,
        adjustments,
        unsupported_assignment_count,
    })
}

pub(crate) struct PeriodResolutionProjection<'a> {
    pub customer: &'a str,
    pub term_version: i64,
    pub period_index: i64,
    pub resolution_id: &'a str,
    pub start_at_us: i64,
    pub end_at_us: i64,
}

pub(crate) struct FiscalVersionProjection<'a> {
    pub calendar_version: i64,
    pub timezone: &'a str,
    pub timezone_rules_version: &'a str,
    pub calendar_bytes: &'a [u8],
}

pub(crate) async fn append_fiscal_version(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &FiscalVersionProjection<'_>,
) -> Result<(), StoreError> {
    if command.children.len() != 1 || projection.calendar_version < 1 {
        return Err(StoreError::Integrity("M5 fiscal append"));
    }
    let sequence = append(conn, command).await?;
    let record_sequence: i64 =
        sqlx::query_scalar("SELECT sequence FROM billing_m5_records WHERE command_sequence=?")
            .bind(sequence)
            .fetch_one(&mut *conn)
            .await?;
    sqlx::query("INSERT INTO billing_m5_fiscal_versions(calendar_version,record_sequence,timezone,timezone_rules_version,calendar_bytes) VALUES(?,?,?,?,?)")
        .bind(projection.calendar_version)
        .bind(record_sequence)
        .bind(projection.timezone)
        .bind(projection.timezone_rules_version)
        .bind(projection.calendar_bytes)
        .execute(&mut *conn)
        .await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct FiscalReportProjection<'a> {
    pub report_id: &'a str,
    pub calendar_version: i64,
    pub m3_high_water: i64,
    pub m5_high_water: i64,
    pub snapshot_boundary_id: i64,
    pub report_hash: &'a str,
    pub report_bytes: &'a [u8],
}

pub(crate) async fn append_fiscal_report(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &FiscalReportProjection<'_>,
) -> Result<(), StoreError> {
    if command.children.len() != 1
        || projection.report_id.is_empty()
        || projection.calendar_version < 1
        || projection.m3_high_water < 0
        || projection.m5_high_water < 0
        || projection.snapshot_boundary_id < 1
        || projection.report_hash.len() != 64
        || projection.report_bytes != command.response
    {
        return Err(StoreError::Integrity("M5 fiscal report append"));
    }
    let sequence = append(conn, command).await?;
    let (record_sequence, family, payload_bytes): (i64, String, Vec<u8>) = sqlx::query_as(
        "SELECT sequence,family,payload_bytes FROM billing_m5_records WHERE command_sequence=?",
    )
    .bind(sequence)
    .fetch_one(&mut *conn)
    .await?;
    let payload = canonical(&payload_bytes, 262_144)?;
    let result = canonical(projection.report_bytes, 262_144)?;
    let identity = canonical(command.identity_key, 4096)?;
    let child_key = canonical(command.children[0].child_key, 4096)?;
    let mut unsigned = result.clone();
    unsigned
        .as_object_mut()
        .ok_or(StoreError::Integrity("M5 fiscal report result"))?
        .remove("report_hash");
    let unsigned = CanonicalBytes::from_value(&unsigned)
        .map_err(|_| StoreError::Integrity("M5 fiscal report hash"))?;
    let boundary: Option<(i64, i64)> = sqlx::query_as(
        "SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE boundary_id=?",
    )
    .bind(projection.snapshot_boundary_id)
    .fetch_optional(&mut *conn)
    .await?;
    let calendar_version = projection.calendar_version.to_string();
    let m3_high_water = projection.m3_high_water.to_string();
    let m5_high_water = projection.m5_high_water.to_string();
    let snapshot_boundary_id = projection.snapshot_boundary_id.to_string();
    if identity["key"] != projection.report_id
        || child_key["key"]["command_id"] != projection.report_id
        || family != "ledger-fiscal-report-run/1"
        || record_sequence <= projection.m5_high_water
        || boundary != Some((projection.m3_high_water, projection.m5_high_water))
        || payload["calendar_version"].as_str() != Some(calendar_version.as_str())
        || payload["m3_high_water"].as_str() != Some(m3_high_water.as_str())
        || payload["m5_high_water"].as_str() != Some(m5_high_water.as_str())
        || payload["snapshot_boundary_id"].as_str() != Some(snapshot_boundary_id.as_str())
        || payload["report_hash"] != projection.report_hash
        || result["schema"] != "ledger-fiscal-report/1"
        || result["status"] != "complete"
        || result["currency"] != "USD"
        || result["scale"] != 18
        || result["complete"] != true
        || result["report_hash"] != projection.report_hash
        || result["calendar_version"] != payload["calendar_version"]
        || result["timezone"] != payload["timezone"]
        || result["timezone_rules_version"] != payload["timezone_rules_version"]
        || result["start_utc"] != payload["start_utc"]
        || result["end_utc"] != payload["end_utc"]
        || result["m3_high_water"] != payload["m3_high_water"]
        || result["m5_high_water"] != payload["m5_high_water"]
        || result["monetary_lines"] != payload["monetary_lines"]
        || result["nonmonetary_quantities"] != payload["nonmonetary_quantities"]
        || result["net_atoms"] != payload["net_atoms"]
        || result["snapshot_boundary_id"] != payload["snapshot_boundary_id"]
        || hex(&hash(
            b"bean-counter/m5/fiscal-report/1\0",
            unsigned.as_slice(),
        )) != projection.report_hash
    {
        return Err(StoreError::Integrity("M5 fiscal report projection"));
    }
    sqlx::query("INSERT INTO billing_m5_fiscal_reports(report_id,calendar_version,m3_high_water,m5_high_water,snapshot_boundary_id,report_hash,record_sequence,report_bytes) VALUES(?,?,?,?,?,?,?,?)")
        .bind(projection.report_id)
        .bind(projection.calendar_version)
        .bind(projection.m3_high_water)
        .bind(projection.m5_high_water)
        .bind(projection.snapshot_boundary_id)
        .bind(projection.report_hash)
        .bind(record_sequence)
        .bind(projection.report_bytes)
        .execute(&mut *conn)
        .await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn append_period_resolution(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &PeriodResolutionProjection<'_>,
) -> Result<(), StoreError> {
    if projection.term_version <= 0
        || projection.period_index < 0
        || projection.end_at_us <= projection.start_at_us
        || resolution_head(
            conn,
            projection.customer,
            projection.term_version,
            projection.period_index,
        )
        .await?
        .is_some()
    {
        return Err(StoreError::Integrity("M5 period resolution projection"));
    }
    let sequence = append(conn, command).await?;
    let row: (i64, String, Vec<u8>) = sqlx::query_as(
        "SELECT sequence,family,payload_bytes FROM billing_m5_records WHERE command_sequence=?",
    )
    .bind(sequence)
    .fetch_one(&mut *conn)
    .await?;
    let value = canonical(&row.2, 262_144)?;
    if row.1 != "ledger-billing-boundary-resolution/1"
        || value["customer"] != projection.customer
        || decimal(&value["term_version"])? != projection.term_version
        || decimal(&value["period_id"]["term_version"])? != projection.term_version
        || decimal(&value["period_id"]["period_index"])? != projection.period_index
        || value["resolution_id"] != projection.resolution_id
        || utc_us(&value["start_utc"])? != projection.start_at_us
        || utc_us(&value["end_utc"])? != projection.end_at_us
        || value.get("supersedes_resolution_id").is_some()
    {
        return Err(StoreError::Integrity("M5 period resolution child"));
    }
    sqlx::query("INSERT INTO billing_m5_period_resolutions(customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id) VALUES(?,?,?,?,?,?,?,NULL)")
        .bind(projection.customer).bind(projection.term_version).bind(projection.period_index)
        .bind(projection.resolution_id).bind(row.0).bind(projection.start_at_us).bind(projection.end_at_us)
        .execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct PeriodCloseProjection<'a> {
    pub customer: &'a str,
    pub term_version: i64,
    pub period_index: i64,
    pub boundary_resolution_id: &'a str,
    pub snapshot_boundary_id: i64,
    pub m3_high_water: i64,
    pub m5_high_water: i64,
    pub statement_hash: &'a str,
    pub statement_bytes: &'a [u8],
    pub adjustments: &'a [PresentableAdjustment],
}

pub(crate) async fn append_period_close(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &PeriodCloseProjection<'_>,
) -> Result<(), StoreError> {
    if projection.term_version <= 0
        || projection.period_index < 0
        || projection.snapshot_boundary_id <= 0
        || projection.m3_high_water < 0
        || projection.m5_high_water < 0
        || projection.statement_hash.len() != 64
    {
        return Err(StoreError::Integrity("M5 period close projection"));
    }
    let boundary: Option<(i64, i64)> = sqlx::query_as(
        "SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE boundary_id=?",
    )
    .bind(projection.snapshot_boundary_id)
    .fetch_optional(&mut *conn)
    .await?;
    if boundary != Some((projection.m3_high_water, projection.m5_high_water)) {
        return Err(StoreError::Integrity("M5 period close snapshot"));
    }
    let head = resolution_head(
        conn,
        projection.customer,
        projection.term_version,
        projection.period_index,
    )
    .await?;
    if head
        .as_ref()
        .is_none_or(|head| head.resolution_id != projection.boundary_resolution_id)
    {
        return Err(StoreError::Integrity("M5 period close resolution"));
    }
    let statement = canonical(projection.statement_bytes, 262_144)?;
    let mut unsigned = statement.clone();
    unsigned
        .as_object_mut()
        .ok_or(StoreError::Integrity("M5 period close statement"))?
        .remove("statement_hash");
    let unsigned = CanonicalBytes::from_value(&unsigned)
        .map_err(|_| StoreError::Integrity("M5 period close statement"))?;
    let expected_hash = hex(&hash(b"bean-counter/m5/statement/4\0", unsigned.as_slice()));
    let selected = unclaimed_adjustments(
        conn,
        projection.customer,
        Some((projection.term_version, projection.period_index)),
        projection.m3_high_water,
        projection.m5_high_water,
    )
    .await?;
    let selected_keys: BTreeSet<_> = selected
        .iter()
        .map(|a| (a.source.clone(), a.adjustment_id.clone()))
        .collect();
    if selected != projection.adjustments {
        return Err(StoreError::Integrity("M5 close claim selection"));
    }
    let (expected_lines, expected_included, expected_net) = expected_close_economics(
        conn,
        projection.customer,
        projection.term_version,
        projection.period_index,
        projection.m3_high_water,
        projection.m5_high_water,
        projection.adjustments,
    )
    .await
    .map_err(|_| StoreError::Integrity("M5 period close economics"))?;
    let expected_direction = if expected_net == "0" {
        "none"
    } else if expected_net.starts_with('-') {
        "payable"
    } else {
        "receivable"
    };
    if statement["schema"] != "ledger-billing-statement/4"
        || statement["customer"] != projection.customer
        || decimal(&statement["period_id"]["term_version"])? != projection.term_version
        || decimal(&statement["period_id"]["period_index"])? != projection.period_index
        || statement["boundary_resolution_id"] != projection.boundary_resolution_id
        || decimal(&statement["snapshot_boundary_id"])? != projection.snapshot_boundary_id
        || decimal(&statement["m3_high_water"])? != projection.m3_high_water
        || decimal(&statement["m5_high_water"])? != projection.m5_high_water
        || statement["lines"] != expected_lines
        || statement["net_atoms"] != expected_net
        || statement["direction"] != expected_direction
        || statement["statement_hash"] != projection.statement_hash
        || projection.statement_hash != expected_hash
    {
        return Err(StoreError::Integrity("M5 period close statement"));
    }
    let sequence = append(conn, command).await?;
    let claim_rows: Vec<(i64,Vec<u8>)> = sqlx::query_as(
        "SELECT sequence,payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-presentation-claim/1' ORDER BY sequence"
    ).bind(sequence).fetch_all(&mut *conn).await?;
    if claim_rows.len() != selected.len() {
        return Err(StoreError::Integrity("M5 close claim count"));
    }
    let mut claimed = BTreeSet::new();
    for (claim_sequence, payload_bytes) in claim_rows {
        let claim = canonical(&payload_bytes, 262_144)?;
        let source = claim["source"]
            .as_str()
            .ok_or(StoreError::Integrity("M5 close claim"))?;
        let adjustment_id = claim["adjustment_id"]
            .as_str()
            .ok_or(StoreError::Integrity("M5 close claim"))?;
        if claim["schema"] != "ledger-billing-presentation-claim/1"
            || claim["customer"] != projection.customer
            || claim["presentation_kind"] != "standard-period"
            || claim["statement_id"] != projection.statement_hash
            || !selected_keys.contains(&(source.to_owned(), adjustment_id.to_owned()))
            || !claimed.insert((source.to_owned(), adjustment_id.to_owned()))
        {
            return Err(StoreError::Integrity("M5 close claim"));
        }
        sqlx::query("INSERT INTO billing_m5_presentation_claims(customer,source_scope,adjustment_id,presentation_kind,statement_id,record_sequence) VALUES(?,?,?,'standard-period',?,?)")
            .bind(projection.customer).bind(source).bind(adjustment_id)
            .bind(projection.statement_hash).bind(claim_sequence).execute(&mut *conn).await?;
    }
    let row: (i64, String, Vec<u8>) = sqlx::query_as(
        "SELECT sequence,family,payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-period-close-record/1'",
    )
    .bind(sequence)
    .fetch_one(&mut *conn)
    .await?;
    let value = canonical(&row.2, 262_144)?;
    if row.1 != "ledger-billing-period-close-record/1"
        || value["customer"] != projection.customer
        || decimal(&value["period_id"]["term_version"])? != projection.term_version
        || decimal(&value["period_id"]["period_index"])? != projection.period_index
        || value["boundary_resolution_id"] != projection.boundary_resolution_id
        || decimal(&value["snapshot_boundary_id"])? != projection.snapshot_boundary_id
        || decimal(&value["m3_high_water"])? != projection.m3_high_water
        || decimal(&value["m5_high_water"])? != projection.m5_high_water
        || value["included_records"] != expected_included
        || value["lines"] != expected_lines
        || value["net_atoms"] != expected_net
        || value["statement_hash"] != projection.statement_hash
    {
        return Err(StoreError::Integrity("M5 period close child"));
    }
    sqlx::query("INSERT INTO billing_m5_period_closes(customer,term_version,period_index,boundary_resolution_id,snapshot_boundary_id,m3_high_water,m5_high_water,statement_hash,close_sequence,statement_bytes) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(projection.customer).bind(projection.term_version).bind(projection.period_index)
        .bind(projection.boundary_resolution_id).bind(projection.snapshot_boundary_id)
        .bind(projection.m3_high_water).bind(projection.m5_high_water)
        .bind(projection.statement_hash).bind(row.0).bind(projection.statement_bytes)
        .execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct AdHocProjection<'a> {
    pub customer: &'a str,
    pub statement_id: &'a str,
    pub adjustments: &'a [PresentableAdjustment],
    pub statement_hash: &'a str,
    pub statement_bytes: &'a [u8],
}

pub(crate) async fn append_ad_hoc(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &AdHocProjection<'_>,
) -> Result<(), StoreError> {
    let requested: Vec<_> = projection
        .adjustments
        .iter()
        .map(|a| (a.source.clone(), a.adjustment_id.clone()))
        .collect();
    let state = ad_hoc_state(conn, projection.customer, &requested).await?;
    let expected: BTreeSet<_> = projection
        .adjustments
        .iter()
        .map(|a| (a.source.clone(), a.adjustment_id.clone()))
        .collect();
    let available: BTreeSet<_> = state
        .adjustments
        .iter()
        .map(|a| (a.source.clone(), a.adjustment_id.clone()))
        .collect();
    if expected.is_empty()
        || expected.len() != projection.adjustments.len()
        || !expected.is_subset(&available)
        || projection
            .adjustments
            .iter()
            .any(|a| !state.adjustments.contains(a))
    {
        return Err(StoreError::Integrity("M5 ad hoc adjustment selection"));
    }
    let result = canonical(projection.statement_bytes, 262_144)?;
    let mut unsigned = result.clone();
    unsigned
        .as_object_mut()
        .ok_or(StoreError::Integrity("M5 ad hoc result"))?
        .remove("statement_hash");
    let unsigned = CanonicalBytes::from_value(&unsigned)
        .map_err(|_| StoreError::Integrity("M5 ad hoc result"))?;
    let hash = hex(&hash(
        b"bean-counter/m5/ad-hoc-statement/1\0",
        unsigned.as_slice(),
    ));
    let view = json!({"customer":projection.customer,"statement_id":projection.statement_id});
    let mut lines = Vec::new();
    let mut net = 0i128;
    let mut refs = Vec::new();
    for adjustment in projection.adjustments {
        let (line, amount) = crate::billing::presentation::adjustment_line(
            projection.customer,
            "ad_hoc",
            &view,
            adjustment,
        )
        .map_err(StoreError::Integrity)?;
        lines.push(line);
        net = net
            .checked_add(amount)
            .ok_or(StoreError::Integrity("M5 ad hoc net"))?;
        refs.push(json!({"source":adjustment.source,"adjustment_id":adjustment.adjustment_id}));
    }
    lines.sort_by(|left, right| left["line_id"].as_str().cmp(&right["line_id"].as_str()));
    refs.sort_by(|left, right| {
        (left["source"].as_str(), left["adjustment_id"].as_str())
            .cmp(&(right["source"].as_str(), right["adjustment_id"].as_str()))
    });
    if result["schema"] != "ledger-billing-ad-hoc-statement-result/1"
        || result["status"] != "issued"
        || result["customer"] != projection.customer
        || result["statement_id"] != projection.statement_id
        || result["adjustments"] != json!(refs)
        || result["lines"] != json!(lines)
        || result["net_atoms"] != net.to_string()
        || result["direction"]
            != if net == 0 {
                "none"
            } else if net < 0 {
                "payable"
            } else {
                "receivable"
            }
        || result["currency"] != "USD"
        || result["scale"] != 18
        || result["complete"] != true
        || result["statement_hash"] != hash
        || projection.statement_hash != hash
    {
        return Err(StoreError::Integrity("M5 ad hoc result"));
    }
    let sequence = append(conn, command).await?;
    let rows: Vec<(i64,String,Vec<u8>)> = sqlx::query_as(
        "SELECT sequence,family,payload_bytes FROM billing_m5_records WHERE command_sequence=? ORDER BY sequence"
    ).bind(sequence).fetch_all(&mut *conn).await?;
    if rows.len() != expected.len() + 1 {
        return Err(StoreError::Integrity("M5 ad hoc children"));
    }
    let mut claimed = BTreeSet::new();
    let mut statement_seen = false;
    for (record_sequence, family, payload_bytes) in rows {
        let payload = canonical(&payload_bytes, 262_144)?;
        if family == "ledger-billing-presentation-claim/1" {
            let source = payload["source"]
                .as_str()
                .ok_or(StoreError::Integrity("M5 ad hoc claim"))?;
            let adjustment_id = payload["adjustment_id"]
                .as_str()
                .ok_or(StoreError::Integrity("M5 ad hoc claim"))?;
            if payload["customer"] != projection.customer
                || payload["presentation_kind"] != "ad-hoc"
                || payload["statement_id"] != projection.statement_id
                || !expected.contains(&(source.to_owned(), adjustment_id.to_owned()))
                || !claimed.insert((source.to_owned(), adjustment_id.to_owned()))
            {
                return Err(StoreError::Integrity("M5 ad hoc claim"));
            }
            sqlx::query("INSERT INTO billing_m5_presentation_claims(customer,source_scope,adjustment_id,presentation_kind,statement_id,record_sequence) VALUES(?,?,?,'ad-hoc',?,?)")
                .bind(projection.customer).bind(source).bind(adjustment_id)
                .bind(projection.statement_id).bind(record_sequence).execute(&mut *conn).await?;
        } else if family == "ledger-billing-ad-hoc-statement-record/1" && !statement_seen {
            statement_seen = true;
            let complete_refs = refs.iter().map(|r| json!({"customer":projection.customer,"source":r["source"],"adjustment_id":r["adjustment_id"]})).collect::<Vec<_>>();
            if payload["customer"] != projection.customer
                || payload["statement_id"] != projection.statement_id
                || payload["adjustments"] != json!(complete_refs)
                || payload["net_atoms"] != net.to_string()
                || payload["statement_hash"] != hash
            {
                return Err(StoreError::Integrity("M5 ad hoc child"));
            }
        } else {
            return Err(StoreError::Integrity("M5 ad hoc child"));
        }
    }
    if claimed != expected || !statement_seen {
        return Err(StoreError::Integrity("M5 ad hoc children"));
    }
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct TransitionProjection<'a> {
    pub customer: &'a str,
    pub previous_term_version: i64,
    pub term_version: i64,
    pub effective_at_us: i64,
    pub term_bytes: &'a [u8],
    pub successor_resolution_id: &'a str,
    pub successor_end_at_us: i64,
    pub predecessor: Option<(&'a ResolutionHead, &'a str, i64)>,
}

pub(crate) async fn append_term_transition(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &TransitionProjection<'_>,
) -> Result<(), StoreError> {
    if projection.term_version <= 0
        || projection.previous_term_version <= 0
        || projection.successor_end_at_us <= projection.effective_at_us
    {
        return Err(StoreError::Integrity("M5 term transition projection"));
    }
    let latest_version: Option<i64> = sqlx::query_scalar(
        "SELECT term_version FROM billing_m5_term_versions WHERE customer=? ORDER BY term_version DESC LIMIT 1",
    )
    .bind(projection.customer)
    .fetch_optional(&mut *conn)
    .await?;
    if latest_version != Some(projection.previous_term_version) {
        return Err(StoreError::Integrity("M5 term transition predecessor"));
    }
    if let Some((head, _, clipped_end_at_us)) = projection.predecessor {
        let period_index: i64 = sqlx::query_scalar(
            "SELECT period_index FROM billing_m5_period_resolutions WHERE record_sequence=?",
        )
        .bind(head.record_sequence)
        .fetch_one(&mut *conn)
        .await?;
        let retained = resolution_head(
            conn,
            projection.customer,
            projection.previous_term_version,
            period_index,
        )
        .await?;
        if retained.as_ref() != Some(head)
            || clipped_end_at_us <= head.start_at_us
            || clipped_end_at_us >= head.end_at_us
        {
            return Err(StoreError::Integrity("M5 resolution head changed"));
        }
    }
    let sequence = append(conn, command).await?;
    let records: Vec<(i64, String, Vec<u8>)> = sqlx::query_as(
        "SELECT sequence,family,payload_bytes FROM billing_m5_records WHERE command_sequence=? ORDER BY sequence",
    )
    .bind(sequence)
    .fetch_all(&mut *conn)
    .await?;
    let mut term_sequence = None;
    let mut successor_sequence = None;
    let mut predecessor_sequence = None;
    for (record_sequence, family, raw) in records {
        let value = canonical(&raw, 262_144)?;
        if family == "ledger-billing-term-version/1" {
            if value["customer"] != projection.customer
                || decimal(&value["term_version"])? != projection.term_version
                || decimal(&value["previous_term_version"])? != projection.previous_term_version
                || utc_us(&value["effective_at"])? != projection.effective_at_us
                || CanonicalBytes::from_value(&value["term"])
                    .map_err(|_| StoreError::Integrity("M5 transition term"))?
                    .as_slice()
                    != projection.term_bytes
                || term_sequence.replace(record_sequence).is_some()
            {
                return Err(StoreError::Integrity("M5 transition term child"));
            }
        } else if family == "ledger-billing-boundary-resolution/1" {
            let resolution_id = value["resolution_id"]
                .as_str()
                .ok_or(StoreError::Integrity("M5 transition resolution"))?;
            if resolution_id == projection.successor_resolution_id {
                if decimal(&value["term_version"])? != projection.term_version
                    || decimal(&value["period_id"]["period_index"])? != 0
                    || utc_us(&value["start_utc"])? != projection.effective_at_us
                    || utc_us(&value["end_utc"])? != projection.successor_end_at_us
                    || value.get("supersedes_resolution_id").is_some()
                    || successor_sequence.replace(record_sequence).is_some()
                {
                    return Err(StoreError::Integrity("M5 successor resolution child"));
                }
            } else if let Some((head, replacement_id, clipped_end_at_us)) = projection.predecessor {
                if resolution_id != replacement_id
                    || value["supersedes_resolution_id"] != head.resolution_id
                    || decimal(&value["term_version"])? != projection.previous_term_version
                    || utc_us(&value["start_utc"])? != head.start_at_us
                    || utc_us(&value["end_utc"])? != clipped_end_at_us
                    || predecessor_sequence.replace(record_sequence).is_some()
                {
                    return Err(StoreError::Integrity("M5 predecessor resolution child"));
                }
            } else {
                return Err(StoreError::Integrity("M5 unexpected transition resolution"));
            }
        } else {
            return Err(StoreError::Integrity("M5 transition child family"));
        }
    }
    let term_sequence = term_sequence.ok_or(StoreError::Integrity("M5 missing term child"))?;
    let successor_sequence =
        successor_sequence.ok_or(StoreError::Integrity("M5 missing successor resolution"))?;
    if projection.predecessor.is_some() != predecessor_sequence.is_some() {
        return Err(StoreError::Integrity("M5 predecessor resolution count"));
    }
    sqlx::query("INSERT INTO billing_m5_term_versions(customer,term_version,record_sequence,effective_at_us,term_bytes) VALUES(?,?,?,?,?)")
        .bind(projection.customer).bind(projection.term_version).bind(term_sequence)
        .bind(projection.effective_at_us).bind(projection.term_bytes).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO billing_m5_period_resolutions(customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id) VALUES(?,?,0,?,?,?,?,NULL)")
        .bind(projection.customer).bind(projection.term_version).bind(projection.successor_resolution_id)
        .bind(successor_sequence).bind(projection.effective_at_us).bind(projection.successor_end_at_us)
        .execute(&mut *conn).await?;
    if let (Some((head, replacement_id, clipped_end_at_us)), Some(record_sequence)) =
        (projection.predecessor, predecessor_sequence)
    {
        let period_index: i64 = sqlx::query_scalar(
            "SELECT period_index FROM billing_m5_period_resolutions WHERE record_sequence=?",
        )
        .bind(head.record_sequence)
        .fetch_one(&mut *conn)
        .await?;
        sqlx::query("INSERT INTO billing_m5_period_resolutions(customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id) VALUES(?,?,?,?,?,?,?,?)")
            .bind(projection.customer).bind(projection.previous_term_version).bind(period_index)
            .bind(replacement_id).bind(record_sequence).bind(head.start_at_us).bind(clipped_end_at_us)
            .bind(&head.resolution_id).execute(&mut *conn).await?;
    }
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct InitialTermProjection<'a> {
    pub customer: &'a str,
    pub term_bytes: &'a [u8],
    pub effective_at_us: i64,
    pub end_at_us: i64,
    pub resolution_id: &'a str,
    pub term_version: i64,
    pub assignments: &'a [(i64, i64, &'a TermHistory)],
}

pub(crate) async fn append_initial_term(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    projection: &InitialTermProjection<'_>,
) -> Result<(), StoreError> {
    let sequence = append(conn, command).await?;
    let first = sqlx::query_scalar::<_, i64>(
        "SELECT min(sequence) FROM billing_m5_records WHERE command_sequence=?",
    )
    .bind(sequence)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO billing_m5_term_versions(customer,term_version,record_sequence,effective_at_us,term_bytes) VALUES(?,?,?,?,?)")
        .bind(projection.customer).bind(projection.term_version).bind(first+1).bind(projection.effective_at_us).bind(projection.term_bytes).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO billing_m5_period_resolutions(customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id) VALUES(?,?,0,?,?,?,?,NULL)")
        .bind(projection.customer).bind(projection.term_version).bind(projection.resolution_id).bind(first).bind(projection.effective_at_us).bind(projection.end_at_us).execute(&mut *conn).await?;
    for &(term_version, period_index, row) in projection.assignments {
        sqlx::query("INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES(?,?,?,?,'m3',?,?,?,'acceptance-time',?)")
            .bind(projection.customer).bind(&row.source).bind(&row.kind).bind(&row.record_id)
            .bind(row.ordinal).bind(term_version).bind(period_index).bind(row.accepted_at_us)
            .execute(&mut *conn).await?;
    }
    append_boundary(conn).await?;
    Ok(())
}

/// Transactional term lookup for a later M3 writer. The coordinator resolves
/// the logical period with the pure calendar service before inserting its M3
/// assignment in the same transaction.
pub(crate) async fn initial_assignment_at(
    conn: &mut SqliteConnection,
    customer: &str,
    accepted_at_us: i64,
) -> Result<Option<(i64, i64, Vec<u8>)>, StoreError> {
    let row = sqlx::query_as("SELECT term_version,effective_at_us,term_bytes FROM billing_m5_term_versions WHERE customer=? AND effective_at_us<=? ORDER BY term_version DESC LIMIT 1")
        .bind(customer).bind(accepted_at_us).fetch_optional(&mut *conn).await?;
    Ok(row)
}

pub(crate) struct M3Assignment {
    pub term_version: i64,
    pub period_index: i64,
    pub basis: &'static str,
    pub receipt_kind: &'static str,
    pub receipt_id: String,
    pub adjustment: Option<M3Adjustment>,
}

pub(crate) struct M3Adjustment {
    pub original_term_version: i64,
    pub original_period_index: i64,
    pub signed_delta_atoms: String,
}

/// Selects an M3 source's logical period from the active frozen term under the
/// writer lock. A correction follows its first outcome until that period has
/// closed; the M3 posting remains the only monetary source.
pub(crate) async fn assignment_for_m3(
    conn: &mut SqliteConnection,
    plan: &crate::service::billing::ValidatedEntry,
) -> Result<Option<M3Assignment>, StoreError> {
    if plan.alias().is_some() {
        return Ok(None);
    }
    let customer = plan
        .customer()
        .ok_or(StoreError::InvalidStore("M5 M3 customer"))?;
    let at = plan
        .accepted_at_us()
        .ok_or(StoreError::InvalidStore("M5 M3 time"))?;
    let terms: i64 =
        sqlx::query_scalar("SELECT count(*) FROM billing_m5_term_versions WHERE customer=?")
            .bind(customer)
            .fetch_one(&mut *conn)
            .await?;
    if terms == 0 {
        return Ok(None);
    }
    let (version, effective, term_bytes) = initial_assignment_at(conn, customer, at)
        .await?
        .ok_or(StoreError::BillingPeriod)?;
    let term = canonical(&term_bytes, 262_144)?;
    let effective_wire: Vec<u8> = sqlx::query_scalar("SELECT payload_bytes FROM billing_m5_records WHERE sequence=(SELECT record_sequence FROM billing_m5_term_versions WHERE customer=? AND term_version=?)")
        .bind(customer).bind(version).fetch_one(&mut *conn).await?;
    let effective_wire = canonical(&effective_wire, 262_144)?;
    let effective_at = effective_wire["effective_at"]
        .as_str()
        .ok_or(StoreError::InvalidStore("M5 term effective time"))?;
    if utc_us(&effective_wire["effective_at"])? != effective {
        return Err(StoreError::InvalidStore("M5 term effective time"));
    }
    let verification = serde_json::json!({
        "schema":"ledger-billing-term/1", "customer":customer,
        "change_id":"m3-assignment", "expected_revision":"0",
        "effective":{"mode":"initial","at":effective_at}, "term":term
    });
    let request = CanonicalBytes::from_value(&verification)
        .map_err(|_| StoreError::InvalidStore("M5 M3 term"))?;
    let request = ledgerlab_core::domain::term_service::parse_initial_request(request.as_slice())
        .map_err(|_| StoreError::InvalidStore("M5 M3 term"))?;
    let ordinal = plan
        .expected_count()
        .checked_add(1)
        .ok_or(StoreError::InvalidStore("M5 M3 ordinal"))?;
    let history = [ledgerlab_core::domain::term_service::BillableHistoryRow {
        ordinal: ordinal as u64,
        accepted_at_us: at,
    }];
    let period = ledgerlab_core::domain::term_service::plan_initial_activation(
        request,
        version as u64,
        &history,
    )
    .map_err(|_| StoreError::InvalidStore("M5 M3 period"))?
    .assignments[0]
        .period_index as i64;
    let kind = if plan.kind() == Some("base") {
        "base-acceptance"
    } else {
        "receipt"
    };
    let bundle = parse_bounded(plan.bundle(), 8 * 1024 * 1024)
        .map_err(|_| StoreError::InvalidStore("M5 M3 bundle"))?;
    let rows = bundle
        .as_array()
        .ok_or(StoreError::InvalidStore("M5 M3 bundle"))?;
    let receipt_id = rows
        .iter()
        .find(|row| row["kind"] == kind)
        .and_then(|row| row["id"].as_str())
        .ok_or(StoreError::InvalidStore("M5 M3 receipt"))?
        .to_owned();
    let mut assignment = M3Assignment {
        term_version: version,
        period_index: period,
        basis: "acceptance-time",
        receipt_kind: kind,
        receipt_id,
        adjustment: None,
    };
    if plan.kind() == Some("correction") {
        let target = plan
            .target()
            .ok_or(StoreError::InvalidStore("M5 correction target"))?;
        let ingress = parse_bounded(plan.ingress(), 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 correction ingress"))?;
        let family = ingress["family"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction family"))?;
        let candidates: Vec<(Vec<u8>, i64, i64)> = sqlx::query_as(ASSIGNED_OUTCOMES)
            .bind(customer)
            .bind(plan.source())
            .bind(target)
            .fetch_all(&mut *conn)
            .await?;
        let mut original = None;
        for (ingress, original_version, original_period) in candidates {
            let candidate = parse_bounded(&ingress, 262_144)
                .map_err(|_| StoreError::InvalidStore("M5 outcome ingress"))?;
            if candidate["family"] == family
                && original
                    .replace((original_version, original_period))
                    .is_some()
            {
                return Err(StoreError::InvalidStore("M5 duplicate first outcome"));
            }
        }
        let (original_version, original_period) =
            original.ok_or(StoreError::InvalidStore("M5 missing outcome assignment"))?;
        let closed: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?")
            .bind(customer).bind(original_version).bind(original_period).fetch_one(&mut *conn).await?;
        if closed == 0 {
            assignment.term_version = original_version;
            assignment.period_index = original_period;
            assignment.basis = "linked-open-period";
        } else {
            assignment.basis = "post-close-adjustment";
            let delta = rows.iter().filter(|row| row["kind"] == "action").try_fold(
                0i128,
                |total, row| {
                    let atoms = row["body"]["amount"]["atoms"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 correction action"))?
                        .parse::<i128>()
                        .map_err(|_| StoreError::InvalidStore("M5 correction action"))?;
                    total
                        .checked_add(atoms)
                        .ok_or(StoreError::InvalidStore("M5 correction action"))
                },
            )?;
            assignment.adjustment = Some(M3Adjustment {
                original_term_version: original_version,
                original_period_index: original_period,
                signed_delta_atoms: delta.to_string(),
            });
        }
    }
    let destination_closed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?",
    )
    .bind(customer)
    .bind(assignment.term_version)
    .bind(assignment.period_index)
    .fetch_one(&mut *conn)
    .await?;
    if destination_closed != 0 {
        return Err(StoreError::BillingPeriod);
    }
    Ok(Some(assignment))
}

/// Identity lookup returns exact original response and every domain child.
/// The caller must compare the submitted request to the retained request before
/// treating it as a retry, and does so before mutable authority checks.
pub(crate) async fn lookup(
    conn: &mut SqliteConnection,
    identity: &[u8],
) -> Result<Option<StoredCommand>, StoreError> {
    canonical(identity, 4096)?;
    let row: Option<StoredCommandRow> = sqlx::query_as(
        "SELECT command_sequence,request_bytes,response_bytes,request_sha256,response_sha256,child_count FROM billing_m5_commands WHERE identity_key=?",
    ).bind(identity).fetch_optional(&mut *conn).await?;
    let Some((sequence, request, response, request_hash, response_hash, child_count)) = row else {
        return Ok(None);
    };
    if parse_bounded(&request, 262_144).is_err()
        || canonical(&response, 262_144).is_err()
        || request_hash.as_slice() != hash(b"bean-counter/m5/request/1\0", &request)
        || response_hash.as_slice() != hash(b"bean-counter/m5/response/1\0", &response)
    {
        return Err(StoreError::Integrity("M5 retained command"));
    }
    let records: Vec<(i64,String,String,Vec<u8>)> = sqlx::query_as("SELECT sequence,record_id,family,payload_bytes FROM billing_m5_records WHERE command_sequence=? ORDER BY sequence")
        .bind(sequence).fetch_all(&mut *conn).await?;
    if records.len() as i64 != child_count {
        return Err(StoreError::Integrity("M5 missing child"));
    }
    for (record_sequence, id, family, payload) in &records {
        let parsed = canonical(payload, 262_144)?;
        let digest = payload_hash(family, &parsed)?;
        let retained_hash: Vec<u8> =
            sqlx::query_scalar("SELECT content_sha256 FROM billing_m5_records WHERE sequence=?")
                .bind(record_sequence)
                .fetch_one(&mut *conn)
                .await?;
        if parsed["record"]["record_id"] != *id
            || decimal(&parsed["record"]["sequence"])? != *record_sequence
            || decimal(&parsed["record"]["command_sequence"])? != sequence
            || parsed["record"]["payload_hash"] != hex(&digest)
            || retained_hash.as_slice() != digest
        {
            return Err(StoreError::Integrity("M5 retained child"));
        }
    }
    Ok(Some(StoredCommand {
        #[cfg(test)]
        sequence,
        request,
        response,
        #[cfg(test)]
        records,
    }))
}

/// Append under the coordinator's existing immediate writer transaction. The
/// caller must add projections, then call `append_boundary` before committing.
pub(crate) async fn append(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
) -> Result<i64, StoreError> {
    if command.children.is_empty()
        || command.children.len() > 100_000
        || command.identity_key.len() > 4096
        || command.request.len() > 262_144
        || command.response.len() > 262_144
    {
        return Err(StoreError::BillingHistoryLimit);
    }
    let identity = canonical(command.identity_key, 4096)?;
    let request = parse_bounded(command.request, 262_144)
        .map_err(|_| StoreError::Integrity("M5 request JSON"))?;
    canonical(command.response, 262_144)?;
    if identity["schema"] != "ledger-billing-m5-command-identity/1"
        || identity["family"] != command.family
        || identity["domain"] != command.domain
        || request["schema"] != command.family
        || identity.get("customer").and_then(Value::as_str) != command.customer
        || identity.get("source").and_then(Value::as_str) != command.source
    {
        return Err(StoreError::Integrity("M5 command identity"));
    }
    let (record_count, canonical_bytes, identity_count, identity_bytes, sequence, first_record): (i64,i64,i64,i64,i64,i64) = sqlx::query_as(
        "SELECT record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    ).fetch_one(&mut *conn).await?;
    let mut amount = bytes_len(&[command.identity_key, command.request, command.response])?;
    let mut prior_key: Option<&[u8]> = None;
    for (i, child) in command.children.iter().enumerate() {
        if child.payload.len() > 262_144 || child.child_key.len() > 4096 {
            return Err(StoreError::BillingHistoryLimit);
        }
        if prior_key.is_some_and(|old| old >= child.child_key) {
            return Err(StoreError::Integrity("M5 child order"));
        }
        prior_key = Some(child.child_key);
        let key = canonical(child.child_key, 4096)?;
        let payload = canonical(child.payload, 262_144)?;
        let expected_id = record_id(command.identity_key, child.child_key);
        let record = &payload["record"];
        let record_sequence = first_record + i as i64;
        if key["kind"] != child.family
            || payload["schema"] != child.family
            || key.get("customer").and_then(Value::as_str) != child.customer
            || key.get("source").and_then(Value::as_str) != child.source
            || record["record_id"] != expected_id
            || decimal(&record["sequence"])? != record_sequence
            || decimal(&record["command_sequence"])? != sequence
            || ledgerlab_core::domain::Timestamp::parse(
                record["accepted_at"]
                    .as_str()
                    .ok_or(StoreError::Integrity("M5 accepted time"))?,
            )
            .map_err(|_| StoreError::Integrity("M5 accepted time"))?
            .micros()
                != command.accepted_at_us
            || record["payload_hash"] != hex(&payload_hash(child.family, &payload)?)
        {
            return Err(StoreError::Integrity("M5 child envelope"));
        }
        amount = amount
            .checked_add(child.payload.len() as i64)
            .ok_or(StoreError::BillingHistoryLimit)?;
    }
    if record_count + sequence - 1 + identity_count + 1 + command.children.len() as i64 > MAX_ROWS
        || canonical_bytes + identity_bytes + amount > MAX_BYTES
    {
        return Err(StoreError::BillingHistoryLimit);
    }
    sqlx::query("INSERT INTO billing_m5_commands(command_sequence,family,identity_domain,customer,source,identity_key,accepted_at_us,child_count,request_bytes,request_sha256,response_bytes,response_sha256) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(sequence).bind(command.family).bind(command.domain).bind(command.customer).bind(command.source)
        .bind(command.identity_key).bind(command.accepted_at_us).bind(command.children.len() as i64)
        .bind(command.request).bind(hash(b"bean-counter/m5/request/1\0", command.request).to_vec())
        .bind(command.response).bind(hash(b"bean-counter/m5/response/1\0", command.response).to_vec())
        .execute(&mut *conn).await?;
    for (i, child) in command.children.iter().enumerate() {
        let payload = canonical(child.payload, 262_144)?;
        let sequence_record = first_record + i as i64;
        sqlx::query("INSERT INTO billing_m5_records(sequence,record_id,family,command_sequence,customer,source,payload_bytes,content_sha256) VALUES(?,?,?,?,?,?,?,?)")
            .bind(sequence_record).bind(record_id(command.identity_key, child.child_key)).bind(child.family)
            .bind(sequence).bind(child.customer).bind(child.source).bind(child.payload)
            .bind(payload_hash(child.family, &payload)?.to_vec()).execute(&mut *conn).await?;
    }
    sqlx::query("UPDATE billing_m5_state SET record_count=record_count+?,canonical_bytes=canonical_bytes+?,next_command_sequence=next_command_sequence+1,next_record_sequence=next_record_sequence+? WHERE singleton=1")
        .bind(command.children.len() as i64).bind(amount).bind(command.children.len() as i64)
        .execute(&mut *conn).await?;
    Ok(sequence)
}

pub(crate) async fn preflight_capacity(
    conn: &mut SqliteConnection,
    additional_commands: i64,
    additional_records: i64,
    additional_bytes: i64,
) -> Result<(), StoreError> {
    if additional_commands < 0 || additional_records < 0 || additional_bytes < 0 {
        return Err(StoreError::Integrity("M5 capacity preflight"));
    }
    let (record_count, canonical_bytes, identity_count, identity_bytes, next_command):
        (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence FROM billing_m5_state WHERE singleton=1",
        )
        .fetch_one(&mut *conn)
        .await?;
    let boundary: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(boundary_id),0) FROM billing_m5_snapshot_boundaries",
    )
    .fetch_one(&mut *conn)
    .await?;
    capacity_within_limits(
        record_count,
        canonical_bytes,
        identity_count,
        identity_bytes,
        next_command,
        boundary,
        additional_commands,
        additional_records,
        additional_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn capacity_within_limits(
    record_count: i64,
    canonical_bytes: i64,
    identity_count: i64,
    identity_bytes: i64,
    next_command: i64,
    boundary: i64,
    additional_commands: i64,
    additional_records: i64,
    additional_bytes: i64,
) -> Result<(), StoreError> {
    let rows = record_count
        .checked_add(next_command - 1)
        .and_then(|value| value.checked_add(identity_count))
        .and_then(|value| value.checked_add(additional_commands))
        .and_then(|value| value.checked_add(additional_records))
        .ok_or(StoreError::BillingHistoryLimit)?;
    let bytes = canonical_bytes
        .checked_add(identity_bytes)
        .and_then(|value| value.checked_add(additional_bytes))
        .ok_or(StoreError::BillingHistoryLimit)?;
    let boundaries = boundary
        .checked_add(additional_commands)
        .ok_or(StoreError::BillingHistoryLimit)?;
    if rows > MAX_ROWS || bytes > MAX_BYTES || boundaries > MAX_ROWS * 3 + 1 {
        return Err(StoreError::BillingHistoryLimit);
    }
    Ok(())
}

/// Store the only complete report cut, including same-cut alias mutations.
pub(crate) async fn append_boundary(conn: &mut SqliteConnection) -> Result<i64, StoreError> {
    let (next, previous_m3, previous_m5): (i64,i64,i64) = sqlx::query_as(
        "SELECT boundary_id+1,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id DESC LIMIT 1",
    ).fetch_one(&mut *conn).await?;
    let m3: i64 = sqlx::query_scalar("SELECT COALESCE(max(ordinal),0) FROM billing_m3_index")
        .fetch_one(&mut *conn)
        .await?;
    let m5: i64 =
        sqlx::query_scalar("SELECT next_record_sequence-1 FROM billing_m5_state WHERE singleton=1")
            .fetch_one(&mut *conn)
            .await?;
    if m3 < previous_m3 || m5 < previous_m5 || next > MAX_ROWS * 3 + 1 {
        return Err(StoreError::Integrity("M5 snapshot boundary"));
    }
    sqlx::query("INSERT INTO billing_m5_snapshot_boundaries(boundary_id,m3_high_water,m5_high_water) VALUES(?,?,?)")
        .bind(next).bind(m3).bind(m5).execute(&mut *conn).await?;
    Ok(next)
}

/// Fail closed on counters, links, hashes or a torn boundary at store open.
pub(crate) async fn verify(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let rows: Vec<(String,i64,i64,i64,i64,i64,i64)> = sqlx::query_as(
        "SELECT migration_id,record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence,next_record_sequence FROM billing_m5_state",
    ).fetch_all(&mut *conn).await?;
    if rows.len() != 1 || rows[0].0 != MIGRATION_ID {
        return Err(StoreError::InvalidStore("M5 migration identity"));
    }
    let (
        _,
        record_count,
        canonical_bytes,
        identity_count,
        identity_bytes,
        next_command,
        next_record,
    ) = &rows[0];
    let (commands,records,retained_bytes): (i64,i64,i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM billing_m5_commands),(SELECT count(*) FROM billing_m5_records),(SELECT COALESCE(sum(length(identity_key)+length(request_bytes)+length(response_bytes)),0) FROM billing_m5_commands)+(SELECT COALESCE(sum(length(payload_bytes)),0) FROM billing_m5_records)",
    ).fetch_one(&mut *conn).await?;
    let (semantic_count,delivery_count,retained_identity_bytes): (i64,i64,i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM billing_m5_activity_semantics),(SELECT count(*) FROM billing_m5_activity_deliveries),(SELECT COALESCE(sum(length(facts_bytes)),0) FROM billing_m5_activity_semantics)+(SELECT COALESCE(sum(length(ingress_bytes)),0) FROM billing_m5_activity_deliveries)",
    ).fetch_one(&mut *conn).await?;
    if *record_count != records
        || *canonical_bytes != retained_bytes
        || *identity_count != semantic_count + delivery_count
        || *identity_bytes != retained_identity_bytes
        || *next_command != commands + 1
        || *next_record != records + 1
        || commands + records + semantic_count + delivery_count > MAX_ROWS
        || retained_bytes + retained_identity_bytes > MAX_BYTES
    {
        return Err(StoreError::InvalidStore("M5 retention counters"));
    }
    let boundary: Option<(i64,i64,i64)> = sqlx::query_as(
        "SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id DESC LIMIT 1",
    ).fetch_optional(&mut *conn).await?;
    let m3: i64 = sqlx::query_scalar("SELECT COALESCE(max(ordinal),0) FROM billing_m3_index")
        .fetch_one(&mut *conn)
        .await?;
    if boundary.is_none_or(|(id, cut_m3, cut_m5)| id < 1 || cut_m3 != m3 || cut_m5 != records) {
        return Err(StoreError::InvalidStore("M5 incomplete snapshot boundary"));
    }
    let boundaries: Vec<(i64,i64,i64)> = sqlx::query_as("SELECT boundary_id,m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries ORDER BY boundary_id")
        .fetch_all(&mut *conn).await?;
    if boundaries
        .first()
        .is_none_or(|first| first.0 != 1 || first.2 != 0)
        || boundaries.iter().enumerate().any(|(i, row)| {
            row.0 != i as i64 + 1
                || (i > 0 && (row.1 < boundaries[i - 1].1 || row.2 < boundaries[i - 1].2))
        })
    {
        return Err(StoreError::InvalidStore("M5 snapshot sequence"));
    }
    let command_rows: Vec<CommandIntegrityRow> = sqlx::query_as(
        "SELECT command_sequence,identity_key,request_bytes,request_sha256,response_bytes,response_sha256,child_count FROM billing_m5_commands ORDER BY command_sequence",
    ).fetch_all(&mut *conn).await?;
    for (i, (sequence, identity, request, request_hash, response, response_hash, child_count)) in
        command_rows.iter().enumerate()
    {
        if *sequence != i as i64 + 1
            || canonical(identity, 4096).is_err()
            || parse_bounded(request, 262_144).is_err()
            || canonical(response, 262_144).is_err()
            || request_hash.as_slice() != hash(b"bean-counter/m5/request/1\0", request)
            || response_hash.as_slice() != hash(b"bean-counter/m5/response/1\0", response)
            || *child_count
                != sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM billing_m5_records WHERE command_sequence=?",
                )
                .bind(sequence)
                .fetch_one(&mut *conn)
                .await?
        {
            return Err(StoreError::InvalidStore("M5 command integrity"));
        }
    }
    let record_rows: Vec<RetainedRecord> = sqlx::query_as(
        "SELECT sequence,record_id,family,command_sequence,payload_bytes,content_sha256 FROM billing_m5_records ORDER BY sequence",
    ).fetch_all(&mut *conn).await?;
    for (i, (sequence, id, family, command_sequence, payload, stored_hash)) in
        record_rows.iter().enumerate()
    {
        let value =
            canonical(payload, 262_144).map_err(|_| StoreError::InvalidStore("M5 record JSON"))?;
        let expected =
            payload_hash(family, &value).map_err(|_| StoreError::InvalidStore("M5 record hash"))?;
        if *sequence != i as i64 + 1
            || decimal(&value["record"]["sequence"])? != *sequence
            || value["record"]["record_id"] != *id
            || decimal(&value["record"]["command_sequence"])? != *command_sequence
            || value["record"]["payload_hash"] != hex(&expected)
            || stored_hash.as_slice() != expected
        {
            return Err(StoreError::InvalidStore("M5 record integrity"));
        }
    }
    verify_period_closes(conn).await?;
    verify_presentation_claims(conn).await?;
    verify_ad_hoc_statements(conn).await?;
    verify_term_projections(conn, &record_rows).await?;
    verify_fiscal_projections(conn, &record_rows).await?;
    verify_recurrence_projections(conn, &record_rows).await?;
    let semantics: Vec<ActivityIdentityRow> = sqlx::query_as(
        "SELECT customer,source,operation_id,command_sequence,activity_sequence,facts_bytes,facts_sha256 FROM billing_m5_activity_semantics ORDER BY customer,source,operation_id",
    ).fetch_all(&mut *conn).await?;
    for (customer, source, operation, command, activity, facts, digest) in semantics {
        let linked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m5_records WHERE command_sequence=? AND sequence=?)")
            .bind(command).bind(activity).fetch_one(&mut *conn).await?;
        let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m5_activity_deliveries WHERE customer=? AND source=? AND command_sequence=? AND activity_sequence=?")
            .bind(&customer).bind(&source).bind(command).bind(activity).fetch_one(&mut *conn).await?;
        if operation.is_empty()
            || canonical(&facts, 262_144).is_err()
            || !linked
            || deliveries < 1
            || digest.as_slice() != hash(b"bean-counter/m5/activity-facts/1\0", &facts)
        {
            return Err(StoreError::InvalidStore("M5 semantic identity"));
        }
    }
    let deliveries: Vec<ActivityIdentityRow> = sqlx::query_as(
        "SELECT customer,source,external_id,command_sequence,activity_sequence,ingress_bytes,ingress_sha256 FROM billing_m5_activity_deliveries ORDER BY customer,source,external_id",
    ).fetch_all(&mut *conn).await?;
    for (customer, source, external, command, activity, ingress, digest) in deliveries {
        let linked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m5_activity_semantics WHERE customer=? AND source=? AND command_sequence=? AND activity_sequence=?)")
            .bind(&customer).bind(&source).bind(command).bind(activity).fetch_one(&mut *conn).await?;
        if external.is_empty()
            || canonical(&ingress, 262_144).is_err()
            || !linked
            || digest.as_slice() != hash(b"bean-counter/m5/activity-ingress/1\0", &ingress)
        {
            return Err(StoreError::InvalidStore("M5 delivery identity"));
        }
    }
    Ok(())
}

async fn verify_recurrence_projections(
    conn: &mut SqliteConnection,
    records: &[RetainedRecord],
) -> Result<(), StoreError> {
    let versions:Vec<(String,String,String,i64,i64,i64,Vec<u8>,Vec<u8>)>=sqlx::query_as(
        "SELECT customer,source,agreement_id,agreement_version,recurrence_version,record_sequence,rule_bytes,renewal_bytes FROM billing_m5_recurrence_versions ORDER BY customer,source,recurrence_version"
    ).fetch_all(&mut *conn).await?;
    let mut prior: Option<(String, String, i64)> = None;
    for (customer, source, agreement_id, agreement_version, version, sequence, rule, renewal) in
        &versions
    {
        let expected = match &prior {
            Some((c, s, n)) if c == customer && s == source => n + 1,
            _ => 1,
        };
        if *version != expected {
            return Err(StoreError::InvalidStore("M5 recurrence version sequence"));
        }
        prior = Some((customer.clone(), source.clone(), *version));
        let record = records
            .iter()
            .find(|r| r.0 == *sequence)
            .ok_or(StoreError::InvalidStore("M5 recurrence record"))?;
        let payload = canonical(&record.4, 262_144)?;
        if record.2 != "ledger-billing-recurrence-version/1"
            || payload["customer"] != *customer
            || payload["source"] != *source
            || payload["agreement_id"] != *agreement_id
            || decimal(&payload["agreement_version"])? != *agreement_version
            || decimal(&payload["recurrence_version"])? != *version
            || CanonicalBytes::from_value(&payload["rule"])
                .map_err(|_| StoreError::InvalidStore("M5 recurrence rule"))?
                .as_slice()
                != rule
            || CanonicalBytes::from_value(&payload["renewal"])
                .map_err(|_| StoreError::InvalidStore("M5 recurrence renewal"))?
                .as_slice()
                != renewal
        {
            return Err(StoreError::InvalidStore("M5 recurrence projection"));
        }
    }
    if records
        .iter()
        .filter(|r| r.2 == "ledger-billing-recurrence-version/1")
        .count()
        != versions.len()
    {
        return Err(StoreError::InvalidStore("M5 recurrence projection count"));
    }
    let cancels:Vec<(String,String,i64,i64,i64)>=sqlx::query_as(
        "SELECT customer,source,recurrence_version,cancelled_at_us,record_sequence FROM billing_m5_recurrence_cancellations"
    ).fetch_all(&mut *conn).await?;
    for (customer, source, version, at, sequence) in &cancels {
        if !versions
            .iter()
            .any(|v| v.0 == *customer && v.1 == *source && v.4 == *version)
        {
            return Err(StoreError::InvalidStore("M5 cancellation target"));
        }
        let record = records
            .iter()
            .find(|r| r.0 == *sequence)
            .ok_or(StoreError::InvalidStore("M5 cancellation record"))?;
        let payload = canonical(&record.4, 262_144)?;
        let timestamp = ledgerlab_core::domain::Timestamp::parse(
            payload["cancelled_at"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 cancellation time"))?,
        )
        .map_err(|_| StoreError::InvalidStore("M5 cancellation time"))?;
        if record.2 != "ledger-billing-recurrence-cancellation/1"
            || payload["customer"] != *customer
            || payload["source"] != *source
            || decimal(&payload["recurrence_version"])? != *version
            || timestamp.micros() != *at
        {
            return Err(StoreError::InvalidStore("M5 cancellation projection"));
        }
    }
    if records
        .iter()
        .filter(|r| r.2 == "ledger-billing-recurrence-cancellation/1")
        .count()
        != cancels.len()
    {
        return Err(StoreError::InvalidStore("M5 cancellation projection count"));
    }
    let occurrences:Vec<(String,String,String,String,i64)>=sqlx::query_as(
        "SELECT customer,source,occurrence_id,accepted_m3_receipt_id,record_sequence FROM billing_m5_occurrence_acceptances"
    ).fetch_all(&mut *conn).await?;
    for (customer, source, id, receipt_id, sequence) in &occurrences {
        let record = records
            .iter()
            .find(|r| r.0 == *sequence)
            .ok_or(StoreError::InvalidStore("M5 occurrence record"))?;
        let payload = canonical(&record.4, 262_144)?;
        if record.2 != "ledger-billing-occurrence-acceptance-record/1"
            || payload["customer"] != *customer
            || payload["source"] != *source
            || payload["occurrence_id"] != *id
            || payload["accepted_m3_receipt_id"] != *receipt_id
        {
            return Err(StoreError::InvalidStore("M5 occurrence projection"));
        }
        let m3:Option<(i64,Vec<u8>)>=sqlx::query_as(
            "SELECT e.ordinal,e.bundle FROM billing_m3_entries e WHERE e.customer=? AND e.source=? AND e.external_id=?"
        ).bind(customer).bind(source).bind(id).fetch_optional(&mut *conn).await?;
        let (ordinal, bundle) = m3.ok_or(StoreError::InvalidStore("M5 occurrence M3 entry"))?;
        let rows = canonical(&bundle, 8 * 1024 * 1024)?;
        if !rows.as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["kind"] == "base-acceptance" && row["id"] == *receipt_id)
        }) {
            return Err(StoreError::InvalidStore("M5 occurrence M3 receipt"));
        }
        let assignment:Option<(i64,i64)>=sqlx::query_as(
            "SELECT term_version,period_index FROM billing_m5_assignments WHERE source_stream='m3' AND source_sequence=? AND customer=? AND source_scope=?"
        ).bind(ordinal).bind(customer).bind(source).fetch_optional(&mut *conn).await?;
        if assignment.is_none_or(|(term, index)| {
            payload["period_id"]["term_version"] != term.to_string()
                || payload["period_id"]["period_index"] != index.to_string()
        }) {
            return Err(StoreError::InvalidStore("M5 occurrence assignment"));
        }
    }
    if records
        .iter()
        .filter(|r| r.2 == "ledger-billing-occurrence-acceptance-record/1")
        .count()
        != occurrences.len()
    {
        return Err(StoreError::InvalidStore("M5 occurrence projection count"));
    }
    Ok(())
}

async fn verify_fiscal_projections(
    conn: &mut SqliteConnection,
    records: &[RetainedRecord],
) -> Result<(), StoreError> {
    let rows: Vec<(i64, i64, String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT calendar_version,record_sequence,timezone,timezone_rules_version,calendar_bytes FROM billing_m5_fiscal_versions ORDER BY calendar_version",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (index, (version, sequence, timezone, rules, calendar_bytes)) in rows.iter().enumerate() {
        if *version != index as i64 + 1 || canonical(calendar_bytes, 262_144).is_err() {
            return Err(StoreError::InvalidStore("M5 fiscal projection"));
        }
        let (_, _, family, _, payload_bytes, _) = records
            .iter()
            .find(|record| record.0 == *sequence)
            .ok_or(StoreError::InvalidStore("M5 fiscal record"))?;
        let payload = canonical(payload_bytes, 262_144)?;
        if family != "ledger-fiscal-calendar-version/1"
            || decimal(&payload["calendar_version"])? != *version
            || payload["timezone"] != *timezone
            || payload["timezone_rules_version"] != *rules
            || CanonicalBytes::from_value(&payload["calendar"])
                .map_err(|_| StoreError::InvalidStore("M5 fiscal calendar"))?
                .as_slice()
                != calendar_bytes
        {
            return Err(StoreError::InvalidStore("M5 fiscal projection"));
        }
    }
    let child_count = records
        .iter()
        .filter(|record| record.2 == "ledger-fiscal-calendar-version/1")
        .count();
    if child_count != rows.len() {
        return Err(StoreError::InvalidStore("M5 fiscal projection count"));
    }
    type ReportRow = (String, i64, i64, i64, i64, String, i64, Vec<u8>);
    let reports: Vec<ReportRow> = sqlx::query_as(
        "SELECT report_id,calendar_version,m3_high_water,m5_high_water,snapshot_boundary_id,report_hash,record_sequence,report_bytes FROM billing_m5_fiscal_reports ORDER BY record_sequence",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (
        report_id,
        calendar_version,
        m3_high_water,
        m5_high_water,
        boundary_id,
        report_hash,
        record_sequence,
        report_bytes,
    ) in &reports
    {
        let (_, _, family, command_sequence, payload_bytes, _) = records
            .iter()
            .find(|record| record.0 == *record_sequence)
            .ok_or(StoreError::InvalidStore("M5 fiscal report record"))?;
        let command: Option<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT family,identity_key,response_bytes FROM billing_m5_commands WHERE command_sequence=?",
        )
        .bind(command_sequence)
        .fetch_optional(&mut *conn)
        .await?;
        let (command_family, identity_bytes, response_bytes) =
            command.ok_or(StoreError::InvalidStore("M5 fiscal report command"))?;
        let identity = canonical(&identity_bytes, 4096)?;
        let payload = canonical(payload_bytes, 262_144)?;
        let result = canonical(report_bytes, 262_144)?;
        let mut unsigned = result.clone();
        unsigned
            .as_object_mut()
            .ok_or(StoreError::InvalidStore("M5 fiscal report result"))?
            .remove("report_hash");
        let unsigned = CanonicalBytes::from_value(&unsigned)
            .map_err(|_| StoreError::InvalidStore("M5 fiscal report hash"))?;
        let boundary: Option<(i64, i64)> = sqlx::query_as(
            "SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE boundary_id=?",
        )
        .bind(boundary_id)
        .fetch_optional(&mut *conn)
        .await?;
        let calendar: Option<(String, String, Vec<u8>)> = sqlx::query_as(
            "SELECT timezone,timezone_rules_version,calendar_bytes FROM billing_m5_fiscal_versions WHERE calendar_version=?",
        )
        .bind(calendar_version)
        .fetch_optional(&mut *conn)
        .await?;
        let (calendar_timezone, calendar_rules, calendar_bytes) =
            calendar.ok_or(StoreError::InvalidStore("M5 fiscal report calendar"))?;
        let calendar_value = canonical(&calendar_bytes, 262_144)?;
        let calendar = ledgerlab_core::domain::fiscal_calendar::FiscalCalendarConfig {
            timezone: calendar_timezone.clone(),
            timezone_rules_version: calendar_rules.clone(),
            calendar: serde_json::from_value(calendar_value)
                .map_err(|_| StoreError::InvalidStore("M5 fiscal report calendar"))?,
        };
        let start_at_us = utc_us(&payload["start_utc"])?;
        let end_at_us = utc_us(&payload["end_utc"])?;
        let endpoints_are_boundaries = calendar
            .period_for_micros(start_at_us)
            .map(|period| period.start.timestamp_micros())
            == Ok(start_at_us)
            && calendar
                .period_for_micros(end_at_us)
                .map(|period| period.start.timestamp_micros())
                == Ok(end_at_us);
        let included = payload["included_records"]
            .as_array()
            .ok_or(StoreError::InvalidStore("M5 fiscal report sources"))?;
        let mut prior_source: Option<(&str, &str, &str, &str)> = None;
        let mut included_set = BTreeSet::new();
        for source in included {
            let item = (
                source["customer"]
                    .as_str()
                    .ok_or(StoreError::InvalidStore("M5 fiscal report source"))?,
                source["source"]
                    .as_str()
                    .ok_or(StoreError::InvalidStore("M5 fiscal report source"))?,
                source["kind"]
                    .as_str()
                    .ok_or(StoreError::InvalidStore("M5 fiscal report source"))?,
                source["id"]
                    .as_str()
                    .ok_or(StoreError::InvalidStore("M5 fiscal report source"))?,
            );
            if prior_source.is_some_and(|prior| prior >= item) {
                return Err(StoreError::InvalidStore("M5 fiscal report source order"));
            }
            prior_source = Some(item);
            included_set.insert(item);
        }
        let view = json!({
            "calendar_version":calendar_version.to_string(),
            "start_utc":payload["start_utc"],"end_utc":payload["end_utc"],
            "m3_high_water":m3_high_water.to_string(),
            "m5_high_water":m5_high_water.to_string()
        });
        let lines = payload["monetary_lines"]
            .as_array()
            .ok_or(StoreError::InvalidStore("M5 fiscal report lines"))?;
        let mut prior_line = None;
        let mut calculated_net = 0i128;
        for line in lines {
            let line_id = line["line_id"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 fiscal report line"))?;
            if prior_line.is_some_and(|prior| prior >= line_id) {
                return Err(StoreError::InvalidStore("M5 fiscal report line order"));
            }
            prior_line = Some(line_id);
            let mut unsigned_line = line.clone();
            unsigned_line
                .as_object_mut()
                .ok_or(StoreError::InvalidStore("M5 fiscal report line"))?
                .remove("line_id");
            let line_identity = CanonicalBytes::from_value(&json!({
                "view_kind":"fiscal","view_identity":view,"line":unsigned_line
            }))
            .map_err(|_| StoreError::InvalidStore("M5 fiscal report line"))?;
            if hex(&hash(
                b"bean-counter/m5/statement-line/1\0",
                line_identity.as_slice(),
            )) != line_id
            {
                return Err(StoreError::InvalidStore("M5 fiscal report line hash"));
            }
            calculated_net = calculated_net
                .checked_add(
                    line["amount_atoms"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 fiscal report amount"))?
                        .parse::<i128>()
                        .map_err(|_| StoreError::InvalidStore("M5 fiscal report amount"))?,
                )
                .ok_or(StoreError::InvalidStore("M5 fiscal report amount"))?;
            let line_sources = line["source_records"]
                .as_array()
                .ok_or(StoreError::InvalidStore("M5 fiscal report line sources"))?;
            for source in line_sources {
                let item = (
                    source["customer"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 fiscal report line source"))?,
                    source["source"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 fiscal report line source"))?,
                    source["kind"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 fiscal report line source"))?,
                    source["id"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 fiscal report line source"))?,
                );
                if !included_set.contains(&item) {
                    return Err(StoreError::InvalidStore("M5 fiscal report line source"));
                }
            }
        }
        let calendar_version_text = calendar_version.to_string();
        let m3_high_water_text = m3_high_water.to_string();
        let m5_high_water_text = m5_high_water.to_string();
        let boundary_id_text = boundary_id.to_string();
        let calculated_net_text = calculated_net.to_string();
        if report_id.is_empty()
            || *calendar_version < 1
            || family != "ledger-fiscal-report-run/1"
            || command_family != "ledger-fiscal-report-request/1"
            || response_bytes != *report_bytes
            || identity["key"] != *report_id
            || *record_sequence <= *m5_high_water
            || boundary != Some((*m3_high_water, *m5_high_water))
            || payload["calendar_version"].as_str() != Some(calendar_version_text.as_str())
            || payload["m3_high_water"].as_str() != Some(m3_high_water_text.as_str())
            || payload["m5_high_water"].as_str() != Some(m5_high_water_text.as_str())
            || payload["snapshot_boundary_id"].as_str() != Some(boundary_id_text.as_str())
            || payload["report_hash"] != *report_hash
            || payload["timezone"] != calendar_timezone
            || payload["timezone_rules_version"] != calendar_rules
            || !endpoints_are_boundaries
            || payload["net_atoms"].as_str() != Some(calculated_net_text.as_str())
            || result["schema"] != "ledger-fiscal-report/1"
            || result["status"] != "complete"
            || result["currency"] != "USD"
            || result["scale"] != 18
            || result["complete"] != true
            || result["report_hash"] != *report_hash
            || result["calendar_version"] != payload["calendar_version"]
            || result["timezone"] != payload["timezone"]
            || result["timezone_rules_version"] != payload["timezone_rules_version"]
            || result["start_utc"] != payload["start_utc"]
            || result["end_utc"] != payload["end_utc"]
            || result["m3_high_water"] != payload["m3_high_water"]
            || result["m5_high_water"] != payload["m5_high_water"]
            || result["monetary_lines"] != payload["monetary_lines"]
            || result["nonmonetary_quantities"] != payload["nonmonetary_quantities"]
            || result["net_atoms"] != payload["net_atoms"]
            || result["snapshot_boundary_id"] != payload["snapshot_boundary_id"]
            || hex(&hash(
                b"bean-counter/m5/fiscal-report/1\0",
                unsigned.as_slice(),
            )) != *report_hash
        {
            return Err(StoreError::InvalidStore("M5 fiscal report projection"));
        }
    }
    let report_child_count = records
        .iter()
        .filter(|record| record.2 == "ledger-fiscal-report-run/1")
        .count();
    if report_child_count != reports.len() {
        return Err(StoreError::InvalidStore(
            "M5 fiscal report projection count",
        ));
    }
    Ok(())
}

async fn expected_close_economics(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
    m3_high_water: i64,
    m5_high_water: i64,
    adjustments: &[PresentableAdjustment],
) -> Result<(Value, Value, String), StoreError> {
    let unsupported: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_assignments WHERE customer=? AND term_version=? AND period_index=? AND source_stream='m5' AND source_sequence<=?",
    )
    .bind(customer)
    .bind(term_version)
    .bind(period_index)
    .bind(m5_high_water)
    .fetch_one(&mut *conn)
    .await?;
    if unsupported != 0 {
        return Err(StoreError::InvalidStore("M5 unsupported closed assignment"));
    }
    let assignments =
        period_close_m3_assignments(conn, customer, term_version, period_index, m3_high_water)
            .await?;
    let period_id = json!({
        "term_version":term_version.to_string(),"period_index":period_index.to_string()
    });
    let mut lines = Vec::with_capacity(assignments.len());
    let mut included = Vec::with_capacity(assignments.len());
    let mut net = 0i128;
    for assignment in assignments {
        if assignment.kind == "receipt" {
            let (line, amount) = crate::billing::presentation::outcome_assignment_line(
                customer,
                "standard",
                &json!({"customer":customer,"period_id":period_id}),
                &assignment,
            )
            .map_err(StoreError::InvalidStore)?;
            included.extend(
                line["source_records"]
                    .as_array()
                    .ok_or(StoreError::InvalidStore("M5 outcome line"))?
                    .iter()
                    .cloned(),
            );
            lines.push(line);
            net = net
                .checked_add(amount)
                .ok_or(StoreError::InvalidStore("M5 close net"))?;
            continue;
        }
        if assignment.kind != "base-acceptance" {
            return Err(StoreError::InvalidStore(
                "M5 unsupported closed M3 assignment",
            ));
        }
        let agreement_id = assignment
            .agreement_id
            .filter(|id| !id.is_empty())
            .ok_or(StoreError::InvalidStore("M5 close agreement"))?;
        let agreement_version = assignment
            .agreement_version
            .filter(|version| *version > 0)
            .ok_or(StoreError::InvalidStore("M5 close agreement"))?;
        let bundle = canonical(&assignment.bundle, 8 * 1024 * 1024)?;
        let rows = bundle
            .as_array()
            .ok_or(StoreError::InvalidStore("M5 close bundle"))?;
        if !rows
            .iter()
            .any(|row| row["kind"] == assignment.kind && row["id"] == assignment.id)
        {
            return Err(StoreError::InvalidStore("M5 close receipt"));
        }
        let event = rows
            .iter()
            .find(|row| row["kind"] == "event")
            .ok_or(StoreError::InvalidStore("M5 close event"))?;
        let postings = rows
            .iter()
            .filter(|row| row["kind"] == "base-posting")
            .collect::<Vec<_>>();
        let first = postings
            .first()
            .ok_or(StoreError::InvalidStore("M5 close postings"))?;
        let payer = first["body"]["roles"]["payer"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 close roles"))?;
        let recipient = first["body"]["roles"]["recipient"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 close roles"))?;
        let mut booked = 0i128;
        let mut scale = None;
        for posting in postings {
            if posting["body"]["agreement_id"] != agreement_id
                || posting["body"]["roles"]["payer"] != payer
                || posting["body"]["roles"]["recipient"] != recipient
                || posting["body"]["amount"]["currency"] != "USD"
            {
                return Err(StoreError::InvalidStore("M5 close posting"));
            }
            let posting_scale = posting["body"]["amount"]["scale"]
                .as_u64()
                .ok_or(StoreError::InvalidStore("M5 close posting"))?;
            if scale
                .replace(posting_scale)
                .is_some_and(|old| old != posting_scale)
            {
                return Err(StoreError::InvalidStore("M5 close posting scale"));
            }
            let atoms = posting["body"]["amount"]["atoms"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 close posting"))?
                .parse::<i128>()
                .map_err(|_| StoreError::InvalidStore("M5 close posting"))?;
            booked = booked
                .checked_add(atoms)
                .ok_or(StoreError::InvalidStore("M5 close amount"))?;
        }
        let source_record = json!({
            "customer":customer,"source":assignment.source,
            "kind":assignment.kind,"id":assignment.id
        });
        included.push(source_record.clone());
        let (basis, amount, calculation) = if scale == Some(18) {
            let quantity = event["body"]["data"]["quantity"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 close quantity"))?
                .parse::<i128>()
                .map_err(|_| StoreError::InvalidStore("M5 close quantity"))?;
            let unit = event["body"]["data"]["unit"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 close unit"))?;
            if quantity <= 0 || booked % quantity != 0 {
                return Err(StoreError::InvalidStore("M5 close quantity"));
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
                .ok_or(StoreError::InvalidStore("M5 close amount"))?;
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
            return Err(StoreError::InvalidStore("M5 close scale"));
        };
        net = net
            .checked_add(amount)
            .ok_or(StoreError::InvalidStore("M5 close net"))?;
        let mut line = json!({
            "source_records":[source_record],"basis":basis,"agreement_id":agreement_id,
            "agreement_version":agreement_version.to_string(),"payer":payer,"recipient":recipient,
            "currency":"USD","scale":18,"amount_atoms":amount.to_string(),
            "calculation":calculation
        });
        let identity = CanonicalBytes::from_value(&json!({
            "view_kind":"standard","view_identity":{"customer":customer,"period_id":period_id},
            "line":line
        }))
        .map_err(|_| StoreError::InvalidStore("M5 close line"))?;
        line["line_id"] = json!(hex(&hash(
            b"bean-counter/m5/statement-line/1\0",
            identity.as_slice()
        )));
        lines.push(line);
    }
    for adjustment in adjustments {
        let (line, amount) = crate::billing::presentation::adjustment_line(
            customer,
            "standard",
            &json!({"customer":customer,"period_id":period_id}),
            adjustment,
        )
        .map_err(StoreError::InvalidStore)?;
        included.extend(
            line["source_records"]
                .as_array()
                .ok_or(StoreError::InvalidStore("M5 adjustment line"))?
                .iter()
                .cloned(),
        );
        lines.push(line);
        net = net
            .checked_add(amount)
            .ok_or(StoreError::InvalidStore("M5 close net"))?;
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
    Ok((json!(lines), json!(included), net.to_string()))
}

async fn verify_period_closes(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let rows: Vec<PeriodCloseProjectionRow> = sqlx::query_as(
        "SELECT customer,term_version,period_index,boundary_resolution_id,snapshot_boundary_id,m3_high_water,m5_high_water,statement_hash,close_sequence,statement_bytes FROM billing_m5_period_closes ORDER BY close_sequence",
    )
    .fetch_all(&mut *conn)
    .await?;
    let retained: BTreeSet<i64> = sqlx::query_scalar(
        "SELECT sequence FROM billing_m5_records WHERE family='ledger-billing-period-close-record/1' ORDER BY sequence",
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .collect();
    if rows.len() != retained.len() {
        return Err(StoreError::InvalidStore("M5 period close projection count"));
    }
    let mut projected = BTreeSet::new();
    for (
        customer,
        term_version,
        period_index,
        resolution_id,
        boundary_id,
        m3_high_water,
        m5_high_water,
        statement_hash,
        close_sequence,
        statement_bytes,
    ) in rows
    {
        if !retained.contains(&close_sequence) || !projected.insert(close_sequence) {
            return Err(StoreError::InvalidStore("M5 period close record"));
        }
        let (command_accepted_at, response_bytes, payload_bytes): (i64, Vec<u8>, Vec<u8>) =
            sqlx::query_as(
                "SELECT c.accepted_at_us,c.response_bytes,r.payload_bytes FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE r.sequence=? AND c.family='ledger-billing-period-close/1'",
            )
            .bind(close_sequence)
            .fetch_one(&mut *conn)
            .await?;
        if response_bytes != statement_bytes {
            return Err(StoreError::InvalidStore("M5 period close response"));
        }
        let statement = canonical(&statement_bytes, 262_144)?;
        let payload = canonical(&payload_bytes, 262_144)?;
        let mut unsigned = statement.clone();
        unsigned
            .as_object_mut()
            .ok_or(StoreError::InvalidStore("M5 period close statement"))?
            .remove("statement_hash");
        let unsigned = CanonicalBytes::from_value(&unsigned)
            .map_err(|_| StoreError::InvalidStore("M5 period close statement"))?;
        let expected_hash = hex(&hash(b"bean-counter/m5/statement/4\0", unsigned.as_slice()));
        let snapshot: Option<(i64, i64)> = sqlx::query_as(
            "SELECT m3_high_water,m5_high_water FROM billing_m5_snapshot_boundaries WHERE boundary_id=?",
        )
        .bind(boundary_id)
        .fetch_optional(&mut *conn)
        .await?;
        let resolution: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT resolution_id,start_at_us,end_at_us FROM billing_m5_period_resolutions WHERE customer=? AND term_version=? AND period_index=? AND resolution_id=?",
        )
        .bind(&customer)
        .bind(term_version)
        .bind(period_index)
        .bind(&resolution_id)
        .fetch_optional(&mut *conn)
        .await?;
        let Some((_, start_at_us, end_at_us)) = resolution else {
            return Err(StoreError::InvalidStore("M5 period close resolution"));
        };
        let net = statement["net_atoms"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 period close net"))?;
        let claimed_adjustments = presentable_adjustments(
            conn,
            &customer,
            Some((term_version, period_index)),
            m3_high_water,
            m5_high_water,
            Some(("standard-period", &statement_hash)),
            None,
        )
        .await?;
        let (expected_lines, expected_included, expected_net) = expected_close_economics(
            conn,
            &customer,
            term_version,
            period_index,
            m3_high_water,
            m5_high_water,
            &claimed_adjustments,
        )
        .await?;
        let direction = if net == "0" {
            "none"
        } else if net.starts_with('-') {
            "payable"
        } else {
            "receivable"
        };
        if statement["schema"] != "ledger-billing-statement/4"
            || statement["status"] != "closed"
            || statement["customer"] != customer
            || decimal(&statement["period_id"]["term_version"])? != term_version
            || decimal(&statement["period_id"]["period_index"])? != period_index
            || statement["boundary_resolution_id"] != resolution_id
            || statement["currency"] != "USD"
            || statement["scale"] != 18
            || statement["complete"] != true
            || statement["direction"] != direction
            || utc_us(&statement["start_utc"])? != start_at_us
            || utc_us(&statement["end_utc"])? != end_at_us
            || utc_us(&statement["close_acceptance_time"])? != command_accepted_at
            || decimal(&statement["snapshot_boundary_id"])? != boundary_id
            || decimal(&statement["m3_high_water"])? != m3_high_water
            || decimal(&statement["m5_high_water"])? != m5_high_water
            || statement["statement_hash"] != statement_hash
            || statement_hash != expected_hash
            || snapshot != Some((m3_high_water, m5_high_water))
            || payload["schema"] != "ledger-billing-period-close-record/1"
            || payload["customer"] != customer
            || payload["period_id"] != statement["period_id"]
            || payload["boundary_resolution_id"] != resolution_id
            || payload["start_utc"] != statement["start_utc"]
            || payload["end_utc"] != statement["end_utc"]
            || payload["m3_high_water"] != statement["m3_high_water"]
            || payload["m5_high_water"] != statement["m5_high_water"]
            || payload["lines"] != statement["lines"]
            || payload["net_atoms"] != statement["net_atoms"]
            || payload["statement_hash"] != statement_hash
            || payload["snapshot_boundary_id"] != statement["snapshot_boundary_id"]
            || !payload["included_records"].is_array()
            || statement["lines"] != expected_lines
            || payload["included_records"] != expected_included
            || net != expected_net
        {
            return Err(StoreError::InvalidStore("M5 period close projection"));
        }
    }
    if projected != retained {
        return Err(StoreError::InvalidStore("M5 period close projection set"));
    }
    Ok(())
}

async fn verify_presentation_claims(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let rows: Vec<ClaimReconciliationRow> = sqlx::query_as(
        "SELECT p.customer,p.source_scope,p.adjustment_id,p.presentation_kind,p.statement_id,p.record_sequence,r.payload_bytes,c.family,c.command_sequence FROM billing_m5_presentation_claims p JOIN billing_m5_records r ON r.sequence=p.record_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence ORDER BY p.customer,p.source_scope,p.adjustment_id"
    ).fetch_all(&mut *conn).await?;
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-presentation-claim/1'")
        .fetch_one(&mut *conn).await?;
    if rows.len() as i64 != retained {
        return Err(StoreError::InvalidStore("M5 claim projection count"));
    }
    for (
        customer,
        source,
        adjustment_id,
        kind,
        statement_id,
        sequence,
        payload_bytes,
        family,
        command_sequence,
    ) in rows
    {
        let payload = canonical(&payload_bytes, 262_144)?;
        if payload["schema"] != "ledger-billing-presentation-claim/1"
            || payload["customer"] != customer
            || payload["source"] != source
            || payload["adjustment_id"] != adjustment_id
            || payload["presentation_kind"] != kind
            || payload["statement_id"] != statement_id
            || decimal(&payload["record"]["sequence"])? != sequence
        {
            return Err(StoreError::InvalidStore("M5 claim projection"));
        }
        match kind.as_str() {
            "standard-period" if family == "ledger-billing-period-close/1" => {
                let close: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM billing_m5_period_closes p JOIN billing_m5_records r ON r.sequence=p.close_sequence WHERE p.customer=? AND p.statement_hash=? AND r.command_sequence=?)"
                ).bind(&customer).bind(&statement_id).bind(command_sequence).fetch_one(&mut *conn).await?;
                if !close {
                    return Err(StoreError::InvalidStore("M5 standard claim owner"));
                }
            }
            "ad-hoc" if family == "ledger-billing-ad-hoc-statement/1" => {
                let response: Vec<u8> = sqlx::query_scalar(
                    "SELECT response_bytes FROM billing_m5_commands WHERE command_sequence=?",
                )
                .bind(command_sequence)
                .fetch_one(&mut *conn)
                .await?;
                let response = canonical(&response, 262_144)?;
                if response["customer"] != customer || response["statement_id"] != statement_id {
                    return Err(StoreError::InvalidStore("M5 ad hoc claim owner"));
                }
            }
            _ => return Err(StoreError::InvalidStore("M5 claim owner")),
        }
    }
    Ok(())
}

async fn verify_ad_hoc_statements(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let commands: Vec<(i64,Vec<u8>,Vec<u8>)> = sqlx::query_as(
        "SELECT command_sequence,request_bytes,response_bytes FROM billing_m5_commands WHERE family='ledger-billing-ad-hoc-statement/1' ORDER BY command_sequence"
    ).fetch_all(&mut *conn).await?;
    let records: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-ad-hoc-statement-record/1'")
        .fetch_one(&mut *conn).await?;
    if commands.len() as i64 != records {
        return Err(StoreError::InvalidStore("M5 ad hoc statement count"));
    }
    for (command_sequence, request_bytes, response_bytes) in commands {
        let request = parse_bounded(&request_bytes, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 ad hoc request"))?;
        let result = canonical(&response_bytes, 262_144)?;
        let customer = result["customer"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 ad hoc customer"))?;
        let statement_id = result["statement_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 ad hoc identity"))?;
        let hash_value = result["statement_hash"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 ad hoc hash"))?;
        let (payload_bytes,): (Vec<u8>,) = sqlx::query_as(
            "SELECT payload_bytes FROM billing_m5_records WHERE command_sequence=? AND family='ledger-billing-ad-hoc-statement-record/1'"
        ).bind(command_sequence).fetch_one(&mut *conn).await?;
        let payload = canonical(&payload_bytes, 262_144)?;
        let adjustments = presentable_adjustments(
            conn,
            customer,
            None,
            i64::MAX,
            i64::MAX,
            Some(("ad-hoc", statement_id)),
            None,
        )
        .await?;
        let mut lines = Vec::new();
        let mut net = 0i128;
        let mut refs = Vec::new();
        let mut full_refs = Vec::new();
        let view = json!({"customer":customer,"statement_id":statement_id});
        for adjustment in &adjustments {
            let (line, amount) = crate::billing::presentation::adjustment_line(
                customer, "ad_hoc", &view, adjustment,
            )
            .map_err(StoreError::InvalidStore)?;
            lines.push(line);
            net = net
                .checked_add(amount)
                .ok_or(StoreError::InvalidStore("M5 ad hoc amount"))?;
            refs.push(json!({"source":adjustment.source,"adjustment_id":adjustment.adjustment_id}));
            full_refs.push(json!({"customer":customer,"source":adjustment.source,"adjustment_id":adjustment.adjustment_id}));
        }
        lines.sort_by(|left, right| left["line_id"].as_str().cmp(&right["line_id"].as_str()));
        let mut unsigned = result.clone();
        unsigned
            .as_object_mut()
            .ok_or(StoreError::InvalidStore("M5 ad hoc result"))?
            .remove("statement_hash");
        let unsigned = CanonicalBytes::from_value(&unsigned)
            .map_err(|_| StoreError::InvalidStore("M5 ad hoc result"))?;
        let expected_hash = hex(&hash(
            b"bean-counter/m5/ad-hoc-statement/1\0",
            unsigned.as_slice(),
        ));
        if adjustments.is_empty()
            || request["schema"] != "ledger-billing-ad-hoc-statement/1"
            || request["customer"] != customer
            || request["command_id"] != statement_id
            || request["adjustments"] != json!(refs)
            || result["schema"] != "ledger-billing-ad-hoc-statement-result/1"
            || result["status"] != "issued"
            || result["adjustments"] != json!(refs)
            || result["lines"] != json!(lines)
            || result["net_atoms"] != net.to_string()
            || result["direction"]
                != if net == 0 {
                    "none"
                } else if net < 0 {
                    "payable"
                } else {
                    "receivable"
                }
            || result["complete"] != true
            || result["currency"] != "USD"
            || result["scale"] != 18
            || hash_value != expected_hash
            || payload["schema"] != "ledger-billing-ad-hoc-statement-record/1"
            || payload["customer"] != customer
            || payload["statement_id"] != statement_id
            || payload["adjustments"] != json!(full_refs)
            || payload["net_atoms"] != net.to_string()
            || payload["statement_hash"] != hash_value
        {
            return Err(StoreError::InvalidStore("M5 ad hoc statement"));
        }
    }
    Ok(())
}

async fn verify_term_projections(
    conn: &mut SqliteConnection,
    records: &[RetainedRecord],
) -> Result<(), StoreError> {
    let terms: Vec<(String,i64,i64,i64,Vec<u8>)> = sqlx::query_as(
        "SELECT customer,term_version,record_sequence,effective_at_us,term_bytes FROM billing_m5_term_versions ORDER BY record_sequence"
    ).fetch_all(&mut *conn).await?;
    let resolutions: Vec<ResolutionProjectionRow> = sqlx::query_as(
        "SELECT customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id FROM billing_m5_period_resolutions ORDER BY record_sequence"
    ).fetch_all(&mut *conn).await?;
    let mut expected_terms = BTreeMap::new();
    let mut expected_resolutions = BTreeMap::new();
    for (sequence, _, family, _, raw, _) in records {
        if family != "ledger-billing-term-version/1"
            && family != "ledger-billing-boundary-resolution/1"
        {
            continue;
        }
        let value = canonical(raw, 262_144)?;
        let customer = value["customer"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 term customer"))?
            .to_owned();
        let version = decimal(&value["term_version"])?;
        if family == "ledger-billing-term-version/1" {
            let effective = utc_us(&value["effective_at"])?;
            let term = CanonicalBytes::from_value(&value["term"])
                .map_err(|_| StoreError::InvalidStore("M5 term bytes"))?
                .into_vec();
            if expected_terms
                .insert(*sequence, (customer, version, effective, term))
                .is_some()
            {
                return Err(StoreError::InvalidStore("M5 duplicate term child"));
            }
        } else {
            let index = decimal(&value["period_id"]["period_index"])?;
            if decimal(&value["period_id"]["term_version"])? != version {
                return Err(StoreError::InvalidStore("M5 resolution period"));
            }
            let id = value["resolution_id"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 resolution ID"))?
                .to_owned();
            let start = utc_us(&value["start_utc"])?;
            let end = utc_us(&value["end_utc"])?;
            let supersedes = value
                .get("supersedes_resolution_id")
                .map(|v| v.as_str().ok_or(StoreError::InvalidStore("M5 supersedes")))
                .transpose()?
                .map(str::to_owned);
            if expected_resolutions
                .insert(
                    *sequence,
                    (customer, version, index, id, start, end, supersedes),
                )
                .is_some()
            {
                return Err(StoreError::InvalidStore("M5 duplicate resolution child"));
            }
        }
    }
    if terms.len() != expected_terms.len() || resolutions.len() != expected_resolutions.len() {
        return Err(StoreError::InvalidStore("M5 missing term projection"));
    }
    for (customer, version, sequence, effective, term) in &terms {
        if expected_terms.get(sequence)
            != Some(&(customer.clone(), *version, *effective, term.clone()))
        {
            return Err(StoreError::InvalidStore("M5 term projection"));
        }
    }
    for (customer, version, index, id, sequence, start, end, supersedes) in &resolutions {
        if expected_resolutions.get(sequence)
            != Some(&(
                customer.clone(),
                *version,
                *index,
                id.clone(),
                *start,
                *end,
                supersedes.clone(),
            ))
        {
            return Err(StoreError::InvalidStore("M5 resolution projection"));
        }
    }
    let raw_by_sequence: BTreeMap<i64, &[u8]> = records
        .iter()
        .map(|record| (record.0, record.4.as_slice()))
        .collect();
    let mut term_requests = BTreeMap::new();
    let mut initial_term_cuts = BTreeMap::new();
    for (customer, version, sequence, _, term_bytes) in &terms {
        let cut: Option<(i64,)> = sqlx::query_as("SELECT m3_high_water FROM billing_m5_snapshot_boundaries WHERE m5_high_water=? ORDER BY boundary_id LIMIT 1")
            .bind(sequence).fetch_optional(&mut *conn).await?;
        let (cut,) = cut.ok_or(StoreError::InvalidStore("M5 initial term boundary"))?;
        initial_term_cuts.insert((customer.clone(), *version), cut);
        let raw = raw_by_sequence
            .get(sequence)
            .ok_or(StoreError::InvalidStore("M5 assignment term child"))?;
        let effective = canonical(raw, 262_144)?["effective_at"].clone();
        let term = canonical(term_bytes, 262_144)?;
        let verification = serde_json::json!({"schema":"ledger-billing-term/1","customer":customer,
            "change_id":"projection-verify","expected_revision":"0",
            "effective":{"mode":"initial","at":effective},"term":term});
        let bytes = CanonicalBytes::from_value(&verification)
            .map_err(|_| StoreError::InvalidStore("M5 assignment term"))?;
        let request = ledgerlab_core::domain::term_service::parse_initial_request(bytes.as_slice())
            .map_err(|_| StoreError::InvalidStore("M5 assignment term"))?;
        term_requests.insert((customer.clone(), *version), request);
    }
    let assignments: Vec<AssignmentProjectionRow> = sqlx::query_as(
        "SELECT customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us FROM billing_m5_assignments ORDER BY source_stream,source_sequence"
    ).fetch_all(&mut *conn).await?;
    let mut assigned_m3 = std::collections::BTreeSet::new();
    for (customer, scope, kind, id, stream, source_sequence, version, index, basis, at) in
        assignments
    {
        let request = term_requests
            .get(&(customer.clone(), version))
            .ok_or(StoreError::InvalidStore("M5 assignment term"))?;
        let initial_cut = *initial_term_cuts
            .get(&(customer.clone(), version))
            .ok_or(StoreError::InvalidStore("M5 initial term boundary"))?;
        if stream != "m3" {
            return Err(StoreError::InvalidStore("M5 assignment source stream"));
        }
        assigned_m3.insert(source_sequence);
        let source: Option<(String, String, String, i64)> = sqlx::query_as(
            "SELECT customer,source,kind,accepted_at_us FROM billing_m3_index WHERE ordinal=?",
        )
        .bind(source_sequence)
        .fetch_optional(&mut *conn)
        .await?;
        let (source_customer, source_scope, index_kind, source_at) =
            source.ok_or(StoreError::InvalidStore("M5 assignment M3 source"))?;
        if customer != source_customer
            || scope != source_scope
            || at != source_at
            || (index_kind == "base") != (kind == "base-acceptance")
        {
            return Err(StoreError::InvalidStore("M5 assignment M3 index"));
        }
        let bundles: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT bundle FROM billing_entries WHERE ordinal=? UNION ALL SELECT bundle FROM billing_m2_entries WHERE ordinal=? UNION ALL SELECT bundle FROM billing_m3_entries WHERE ordinal=?"
        ).bind(source_sequence).bind(source_sequence).bind(source_sequence).fetch_all(&mut *conn).await?;
        if bundles.len() != 1 {
            return Err(StoreError::InvalidStore("M5 assignment M3 bundle"));
        }
        let bundle = parse_bounded(&bundles[0].0, 8 * 1024 * 1024)
            .map_err(|_| StoreError::InvalidStore("M5 assignment M3 bundle"))?;
        let rows = bundle
            .as_array()
            .ok_or(StoreError::InvalidStore("M5 assignment M3 bundle"))?;
        let receipt = rows
            .iter()
            .find(|row| row["kind"] == kind && row["id"] == id)
            .ok_or(StoreError::InvalidStore("M5 assignment receipt"))?;
        if utc_us(&receipt["body"]["accepted_at"])? != at {
            return Err(StoreError::InvalidStore("M5 assignment time"));
        }
        if basis == "acceptance-time" || basis == "post-close-adjustment" {
            let first_version = terms
                .iter()
                .filter(|term| term.0 == customer)
                .map(|term| term.1)
                .min()
                .ok_or(StoreError::InvalidStore("M5 assignment term"))?;
            let active_version = terms
                .iter()
                .filter(|term| {
                    if term.0 != customer || term.3 > at {
                        return false;
                    }
                    if term.1 == first_version {
                        return true;
                    }
                    initial_term_cuts
                        .get(&(customer.clone(), term.1))
                        .is_some_and(|cut| source_sequence > *cut)
                })
                .max_by_key(|term| (term.3, term.1))
                .map(|term| term.1);
            if active_version != Some(version) {
                return Err(StoreError::InvalidStore("M5 assignment active term"));
            }
            if basis == "acceptance-time"
                && index_kind == "correction"
                && source_sequence > initial_cut
            {
                return Err(StoreError::InvalidStore("M5 correction assignment basis"));
            }
            let history = [ledgerlab_core::domain::term_service::BillableHistoryRow {
                ordinal: source_sequence as u64,
                accepted_at_us: at,
            }];
            let plan = ledgerlab_core::domain::term_service::plan_initial_activation(
                request.clone(),
                version as u64,
                &history,
            )
            .map_err(|_| StoreError::InvalidStore("M5 assignment period"))?;
            if plan.assignments[0].period_index != index as u64 {
                return Err(StoreError::InvalidStore("M5 assignment period"));
            }
        } else if basis == "linked-open-period" {
            if index_kind != "correction" || source_sequence <= initial_cut {
                return Err(StoreError::InvalidStore("M5 linked assignment kind"));
            }
            let (target, ingress): (String, Vec<u8>) = sqlx::query_as(
                "SELECT i.target,e.ingress FROM billing_m3_index i JOIN billing_m3_entries e ON e.ordinal=i.ordinal WHERE i.ordinal=?")
                .bind(source_sequence).fetch_one(&mut *conn).await?;
            let ingress = parse_bounded(&ingress, 262_144)
                .map_err(|_| StoreError::InvalidStore("M5 linked assignment ingress"))?;
            let family = ingress["family"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 linked assignment family"))?;
            let outcomes: Vec<(Vec<u8>, i64, i64)> = sqlx::query_as(ASSIGNED_OUTCOMES)
                .bind(&customer)
                .bind(&scope)
                .bind(&target)
                .fetch_all(&mut *conn)
                .await?;
            let mut linked = 0;
            for (outcome_ingress, outcome_version, outcome_period) in outcomes {
                let outcome_ingress = parse_bounded(&outcome_ingress, 262_144)
                    .map_err(|_| StoreError::InvalidStore("M5 linked outcome ingress"))?;
                if outcome_ingress["family"] == family {
                    if (outcome_version, outcome_period) != (version, index) {
                        return Err(StoreError::InvalidStore("M5 linked assignment period"));
                    }
                    linked += 1;
                }
            }
            let close_cut: Option<(i64,)> = sqlx::query_as("SELECT m3_high_water FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?")
                .bind(&customer).bind(version).bind(index).fetch_optional(&mut *conn).await?;
            if linked != 1 || close_cut.is_some_and(|(cut,)| source_sequence > cut) {
                return Err(StoreError::InvalidStore("M5 linked assignment owner"));
            }
        } else {
            return Err(StoreError::InvalidStore("M5 assignment basis"));
        }
    }
    let indexed: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT ordinal,customer,accepted_at_us FROM billing_m3_index ORDER BY ordinal",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut first_effective = BTreeMap::new();
    for (customer, _, _, effective, _) in &terms {
        first_effective
            .entry(customer.as_str())
            .and_modify(|earliest: &mut i64| *earliest = (*earliest).min(*effective))
            .or_insert(*effective);
    }
    for (ordinal, customer, accepted_at_us) in indexed {
        if let Some(effective) = first_effective.get(customer.as_str()) {
            if accepted_at_us < *effective || !assigned_m3.contains(&ordinal) {
                return Err(StoreError::InvalidStore("M5 missing M3 assignment"));
            }
        }
    }
    // Adjustment links carry no separate economics. Every post-close M3
    // correction must have exactly one link to its posting, and every retained
    // link must reconcile to that posting and its closed original period.
    let adjustments: Vec<AdjustmentProjectionRow> =
        sqlx::query_as("SELECT customer,source_scope,adjustment_id,cause_kind,cause_id,target_id,original_term_version,original_period_index,assigned_term_version,assigned_period_index,source_stream,source_sequence,signed_delta_atoms FROM billing_m5_adjustments ORDER BY source_stream,source_sequence")
            .fetch_all(&mut *conn).await?;
    let mut adjustment_sources = std::collections::BTreeSet::new();
    for (
        customer,
        scope,
        adjustment_id,
        cause_kind,
        cause_id,
        target,
        original_version,
        original_period,
        assigned_version,
        assigned_period,
        stream,
        sequence,
        delta,
    ) in adjustments
    {
        if stream != "m3" {
            return Err(StoreError::InvalidStore("M5 adjustment source stream"));
        }
        if cause_kind != "outcome-correction"
            || cause_id != adjustment_id
            || !adjustment_sources.insert(sequence)
        {
            return Err(StoreError::InvalidStore("M5 M3 adjustment identity"));
        }
        let source: Option<(String,String,String,String,Vec<u8>)> = sqlx::query_as(
            "SELECT i.customer,i.source,i.external_id,i.target,e.bundle FROM billing_m3_index i JOIN billing_m3_entries e ON e.ordinal=i.ordinal WHERE i.ordinal=? AND i.kind='correction'")
            .bind(sequence).fetch_optional(&mut *conn).await?;
        let (source_customer, source_scope, source_id, source_target, bundle) =
            source.ok_or(StoreError::InvalidStore("M5 M3 adjustment source"))?;
        if (
            customer.as_str(),
            scope.as_str(),
            adjustment_id.as_str(),
            target.as_str(),
        ) != (
            source_customer.as_str(),
            source_scope.as_str(),
            source_id.as_str(),
            source_target.as_str(),
        ) {
            return Err(StoreError::InvalidStore("M5 M3 adjustment source"));
        }
        let assignment: Option<(i64,i64,String)> = sqlx::query_as(
            "SELECT term_version,period_index,assignment_basis FROM billing_m5_assignments WHERE source_stream='m3' AND source_sequence=?")
            .bind(sequence).fetch_optional(&mut *conn).await?;
        if assignment.as_ref().is_none_or(|(v, p, b)| {
            *v != assigned_version || *p != assigned_period || b != "post-close-adjustment"
        }) {
            return Err(StoreError::InvalidStore("M5 M3 adjustment assignment"));
        }
        let close_cut: Option<(i64,)> = sqlx::query_as("SELECT m3_high_water FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?")
            .bind(&customer).bind(original_version).bind(original_period).fetch_optional(&mut *conn).await?;
        if close_cut.is_none_or(|(cut,)| sequence <= cut) {
            return Err(StoreError::InvalidStore("M5 M3 adjustment close"));
        }
        let correction_ingress: Vec<u8> =
            sqlx::query_scalar("SELECT ingress FROM billing_m3_entries WHERE ordinal=?")
                .bind(sequence)
                .fetch_one(&mut *conn)
                .await?;
        let correction_ingress = parse_bounded(&correction_ingress, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 M3 adjustment ingress"))?;
        let family = correction_ingress["family"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 M3 adjustment family"))?;
        let outcomes: Vec<(Vec<u8>, i64, i64)> = sqlx::query_as(ASSIGNED_OUTCOMES)
            .bind(&customer)
            .bind(&scope)
            .bind(&target)
            .fetch_all(&mut *conn)
            .await?;
        let mut linked = 0;
        for (ingress, version, period) in outcomes {
            let ingress = parse_bounded(&ingress, 262_144)
                .map_err(|_| StoreError::InvalidStore("M5 M3 outcome ingress"))?;
            if ingress["family"] == family {
                if (version, period) != (original_version, original_period) {
                    return Err(StoreError::InvalidStore("M5 M3 adjustment original period"));
                }
                linked += 1;
            }
        }
        if linked != 1 {
            return Err(StoreError::InvalidStore(
                "M5 M3 adjustment original outcome",
            ));
        }
        let bundle = parse_bounded(&bundle, 8 * 1024 * 1024)
            .map_err(|_| StoreError::InvalidStore("M5 M3 adjustment bundle"))?;
        let rows = bundle
            .as_array()
            .ok_or(StoreError::InvalidStore("M5 M3 adjustment bundle"))?;
        let actual =
            rows.iter()
                .filter(|row| row["kind"] == "action")
                .try_fold(0i128, |sum, row| {
                    let atoms = row["body"]["amount"]["atoms"]
                        .as_str()
                        .ok_or(StoreError::InvalidStore("M5 M3 adjustment amount"))?
                        .parse::<i128>()
                        .map_err(|_| StoreError::InvalidStore("M5 M3 adjustment amount"))?;
                    sum.checked_add(atoms)
                        .ok_or(StoreError::InvalidStore("M5 M3 adjustment amount"))
                })?;
        if delta != actual.to_string() {
            return Err(StoreError::InvalidStore("M5 M3 adjustment amount"));
        }
    }
    let post_close: Vec<(i64,)> = sqlx::query_as(
        "SELECT source_sequence FROM billing_m5_assignments WHERE source_stream='m3' AND assignment_basis='post-close-adjustment'")
        .fetch_all(&mut *conn).await?;
    for (sequence,) in post_close {
        if !adjustment_sources.contains(&sequence) {
            return Err(StoreError::InvalidStore("M5 missing M3 adjustment"));
        }
    }
    Ok(())
}

fn decimal(value: &Value) -> Result<i64, StoreError> {
    let s = value
        .as_str()
        .ok_or(StoreError::InvalidStore("M5 decimal"))?;
    let parsed = ledgerlab_core::domain::Revision::parse(s)
        .map_err(|_| StoreError::InvalidStore("M5 decimal"))?;
    i64::try_from(parsed.value()).map_err(|_| StoreError::InvalidStore("M5 decimal"))
}
fn utc_us(value: &Value) -> Result<i64, StoreError> {
    ledgerlab_core::domain::Timestamp::parse(
        value
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 timestamp"))?,
    )
    .map(|time| time.micros())
    .map_err(|_| StoreError::InvalidStore("M5 timestamp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        records::{Installation, Scope},
        sqlite::SqliteStore,
    };
    use sqlx::Connection;

    fn encode(value: &Value) -> Vec<u8> {
        CanonicalBytes::from_value(value).unwrap().into_vec()
    }

    #[test]
    fn initial_term_golden_keeps_child_order_and_exact_hashes() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../../../contracts/candidates/billing-lifecycle-m5/vectors/m5-command-goldens.json"
        ))
        .unwrap();
        let golden = oracle["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "initial-customer-term-setup")
            .unwrap();
        let command = &golden["command"];
        let identity = encode(&command["identity"]);
        assert_eq!(
            identity,
            command["identity_key_canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
        let children = command["domain_children"].as_array().unwrap();
        let mut previous = None;
        for child in children {
            let key = encode(&child["child_key"]);
            assert_eq!(
                key,
                child["child_key_canonical_utf8"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
            assert!(previous.as_ref().is_none_or(|prior| prior < &key));
            previous = Some(key.clone());
            assert_eq!(record_id(&identity, &key), child["record_id"]);
            let family = child["family"].as_str().unwrap();
            assert_eq!(
                hex(&payload_hash(family, &child["payload"]).unwrap()),
                child["payload_hash"]
            );
            assert_eq!(
                encode(&child["payload"]),
                child["payload_canonical_utf8"].as_str().unwrap().as_bytes()
            );
        }
        assert_eq!(
            encode(&command["result"]),
            command["result_canonical_utf8"]
                .as_str()
                .unwrap()
                .as_bytes()
        );
    }

    #[test]
    fn capacity_preflight_reserves_the_resolution_and_close_together() {
        let current_bytes = MAX_BYTES - 100;
        assert!(capacity_within_limits(0, current_bytes, 0, 0, 1, 1, 1, 1, 100,).is_ok());
        assert!(matches!(
            capacity_within_limits(0, current_bytes, 0, 0, 1, 1, 2, 2, 101,),
            Err(StoreError::BillingHistoryLimit)
        ));
    }

    #[tokio::test]
    async fn command_append_lookup_and_snapshot_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::create(
            dir.path(),
            Installation {
                scope: Scope {
                    tenant: "tenant".into(),
                    environment: "dev".into(),
                },
                logical_store_id: "store".into(),
                mode: "real".into(),
                admission: "open".into(),
                dispatch_hold: true,
                dispatch_enabled: false,
                generation: 0,
            },
        )
        .await
        .unwrap();
        store.close().await;
        let mut conn = SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.path().join("local.db"))
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let identity = encode(
            &serde_json::json!({"schema":"ledger-billing-m5-command-identity/1", "domain":"installation-admin", "family":"ledger-billing-fiscal-calendar/1", "key_kind":"change_id", "key":"set-1"}),
        );
        let request = encode(
            &serde_json::json!({"schema":"ledger-billing-fiscal-calendar/1", "change_id":"set-1"}),
        );
        let response = encode(
            &serde_json::json!({"schema":"ledger-billing-fiscal-calendar-result/1", "calendar_version":"1"}),
        );
        let child_key = encode(
            &serde_json::json!({"role":"fiscal-calendar", "kind":"ledger-billing-fiscal-calendar-record/1", "key":{"calendar_version":"1"}}),
        );
        let id = record_id(&identity, &child_key);
        let at = "2026-09-29T00:00:00.000000Z";
        let micros = ledgerlab_core::domain::Timestamp::parse(at)
            .unwrap()
            .micros();
        let mut payload = serde_json::json!({"schema":"ledger-billing-fiscal-calendar-record/1", "calendar_version":"1", "record":{"record_id":id,"sequence":"1","accepted_at":at,"command_sequence":"1"}});
        let digest = payload_hash("ledger-billing-fiscal-calendar-record/1", &payload).unwrap();
        payload["record"]["payload_hash"] = serde_json::json!(hex(&digest));
        let payload = encode(&payload);
        let children = [Child {
            family: "ledger-billing-fiscal-calendar-record/1",
            customer: None,
            source: None,
            child_key: &child_key,
            payload: &payload,
        }];
        let command = Command {
            family: "ledger-billing-fiscal-calendar/1",
            domain: "installation-admin",
            customer: None,
            source: None,
            identity_key: &identity,
            accepted_at_us: micros,
            request: &request,
            response: &response,
            children: &children,
        };
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert_eq!(append(&mut tx, &command).await.unwrap(), 1);
        assert_eq!(append_boundary(&mut tx).await.unwrap(), 2);
        tx.commit().await.unwrap();
        verify(&mut conn).await.unwrap();
        let stored = lookup(&mut conn, &identity).await.unwrap().unwrap();
        assert_eq!(stored.sequence, 1);
        assert_eq!(stored.request, request);
        assert_eq!(stored.response, response);
        assert_eq!(stored.records.len(), 1);
        conn.close().await.unwrap();
        let store = SqliteStore::open(dir.path()).await.unwrap();
        store.close().await;
    }
}
