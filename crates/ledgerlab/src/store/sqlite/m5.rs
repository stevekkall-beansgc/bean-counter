//! Additive M5 persistence primitives. The coordinator owns terms, authority,
//! arithmetic and statement composition; this module owns exact retained bytes.
use crate::store::errors::StoreError;
use ledgerlab_core::canonical::{parse_bounded, CanonicalBytes};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::collections::BTreeMap;

const MAX_ROWS: i64 = 100_000;
const MAX_BYTES: i64 = 268_435_456;
const MIGRATION_ID: &str = "bean-counter/m5/schema-11/1";

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
    pub sequence: i64,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
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

pub(crate) async fn append_initial_term(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    customer: &str,
    term_bytes: &[u8],
    effective_at_us: i64,
    end_at_us: i64,
    resolution_id: &str,
    term_version: i64,
    assignments: &[(i64, i64, &TermHistory)],
) -> Result<(), StoreError> {
    let sequence = append(conn, command).await?;
    let first = sqlx::query_scalar::<_, i64>(
        "SELECT min(sequence) FROM billing_m5_records WHERE command_sequence=?",
    )
    .bind(sequence)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO billing_m5_term_versions(customer,term_version,record_sequence,effective_at_us,term_bytes) VALUES(?,?,?,?,?)")
        .bind(customer).bind(term_version).bind(first+1).bind(effective_at_us).bind(term_bytes).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO billing_m5_period_resolutions(customer,term_version,period_index,resolution_id,record_sequence,start_at_us,end_at_us,supersedes_resolution_id) VALUES(?,?,0,?,?,?,?,NULL)")
        .bind(customer).bind(term_version).bind(resolution_id).bind(first).bind(effective_at_us).bind(end_at_us).execute(&mut *conn).await?;
    for &(term_version, period_index, row) in assignments {
        sqlx::query("INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES(?,?,?,?,'m3',?,?,?,'acceptance-time',?)")
            .bind(customer).bind(&row.source).bind(&row.kind).bind(&row.record_id)
            .bind(row.ordinal).bind(term_version).bind(period_index).bind(row.accepted_at_us)
            .execute(&mut *conn).await?;
    }
    append_boundary(conn).await?;
    Ok(())
}

/// Transactional term lookup for a later M3 writer. The coordinator resolves
/// the logical period with the pure calendar service before inserting its M3
/// assignment in the same transaction.
#[allow(dead_code)] // M3 writer integration is a separate bounded slice.
pub(crate) async fn initial_assignment_at(
    conn: &mut SqliteConnection,
    customer: &str,
    accepted_at_us: i64,
) -> Result<Option<(i64, i64, Vec<u8>)>, StoreError> {
    let row = sqlx::query_as("SELECT term_version,effective_at_us,term_bytes FROM billing_m5_term_versions WHERE customer=? AND effective_at_us<=? ORDER BY term_version DESC LIMIT 1")
        .bind(customer).bind(accepted_at_us).fetch_optional(&mut *conn).await?;
    Ok(row)
}

/// Identity lookup returns exact original response and every domain child.
/// The caller must compare the submitted request to the retained request before
/// treating it as a retry, and does so before mutable authority checks.
pub(crate) async fn lookup(
    conn: &mut SqliteConnection,
    identity: &[u8],
) -> Result<Option<StoredCommand>, StoreError> {
    canonical(identity, 4096)?;
    let row: Option<(i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i64)> = sqlx::query_as(
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
            || parsed["record"]["sequence"] != record_sequence.to_string()
            || parsed["record"]["command_sequence"] != sequence.to_string()
            || parsed["record"]["payload_hash"] != hex(&digest)
            || retained_hash.as_slice() != digest
        {
            return Err(StoreError::Integrity("M5 retained child"));
        }
    }
    Ok(Some(StoredCommand {
        sequence,
        request,
        response,
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
        if child.payload.len() > 262_144
            || child.child_key.len() > 4096
            || prior_key.is_some_and(|old| old >= child.child_key)
        {
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
            || record["sequence"] != record_sequence.to_string()
            || record["command_sequence"] != sequence.to_string()
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
    let command_rows: Vec<(i64,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,i64)> = sqlx::query_as(
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
    let record_rows: Vec<(i64,String,String,i64,Vec<u8>,Vec<u8>)> = sqlx::query_as(
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
            || value["record"]["sequence"] != sequence.to_string()
            || value["record"]["record_id"] != *id
            || value["record"]["command_sequence"] != command_sequence.to_string()
            || value["record"]["payload_hash"] != hex(&expected)
            || stored_hash.as_slice() != expected
        {
            return Err(StoreError::InvalidStore("M5 record integrity"));
        }
    }
    verify_term_projections(conn, &record_rows).await?;
    let semantics: Vec<(String,String,String,i64,i64,Vec<u8>,Vec<u8>)> = sqlx::query_as(
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
    let deliveries: Vec<(String,String,String,i64,i64,Vec<u8>,Vec<u8>)> = sqlx::query_as(
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

type RetainedRecord = (i64, String, String, i64, Vec<u8>, Vec<u8>);

async fn verify_term_projections(
    conn: &mut SqliteConnection,
    records: &[RetainedRecord],
) -> Result<(), StoreError> {
    let terms: Vec<(String,i64,i64,i64,Vec<u8>)> = sqlx::query_as(
        "SELECT customer,term_version,record_sequence,effective_at_us,term_bytes FROM billing_m5_term_versions ORDER BY record_sequence"
    ).fetch_all(&mut *conn).await?;
    let resolutions: Vec<(String,i64,i64,String,i64,i64,i64,Option<String>)> = sqlx::query_as(
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
            if value["period_id"]["term_version"] != version.to_string() {
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
    for (customer, version, sequence, _, term_bytes) in &terms {
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
    let assignments: Vec<(String,String,String,String,String,i64,i64,i64,String,i64)> = sqlx::query_as(
        "SELECT customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us FROM billing_m5_assignments ORDER BY source_stream,source_sequence"
    ).fetch_all(&mut *conn).await?;
    let mut assigned_m3 = std::collections::BTreeSet::new();
    for (customer, scope, kind, id, stream, source_sequence, version, index, basis, at) in
        assignments
    {
        let request = term_requests
            .get(&(customer.clone(), version))
            .ok_or(StoreError::InvalidStore("M5 assignment term"))?;
        if stream != "m3" {
            // M5 source assignments are verified by their owning commands.
            continue;
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
        if basis != "acceptance-time" {
            // Linked or post-close period selection is checked by its owner.
            continue;
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
