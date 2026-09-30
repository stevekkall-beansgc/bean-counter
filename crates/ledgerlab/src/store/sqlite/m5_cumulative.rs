use super::*;
use ledgerlab_core::domain::term_service;

type IdentityLookupRow = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
type AssignedCumulativeRow = (i64, String, Vec<u8>, String, String, String, String);
type BasisProjectionRow = (String, String, String, i64, i64, i64, i64, Vec<u8>);
type SemanticProjectionRow = (String, String, String, i64, i64, Vec<u8>, Vec<u8>, Vec<u8>);
type DeliveryProjectionRow = (String, String, String, Vec<u8>, Vec<u8>, String, Vec<u8>);

pub(crate) struct CumulativeSequences {
    pub command: i64,
    pub record: i64,
}

pub(crate) async fn cumulative_sequences(
    conn: &mut SqliteConnection,
) -> Result<CumulativeSequences, StoreError> {
    let (command, record): (i64, i64) = sqlx::query_as(
        "SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(CumulativeSequences { command, record })
}

pub(crate) async fn cumulative_last_accepted(
    conn: &mut SqliteConnection,
) -> Result<Option<i64>, StoreError> {
    let last: Option<i64> =
        sqlx::query_scalar("SELECT max(accepted_at_us) FROM billing_m5_commands")
            .fetch_one(&mut *conn)
            .await?;
    Ok(last)
}

pub(crate) async fn cumulative_basis_revision(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
) -> Result<i64, StoreError> {
    let revision = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_cumulative_basis_versions WHERE customer=? AND source=? AND agreement_id=? AND agreement_version=?",
    )
    .bind(customer)
    .bind(source)
    .bind(agreement_id)
    .bind(agreement_version)
    .fetch_one(&mut *conn)
    .await?;
    Ok(revision)
}

pub(crate) async fn cumulative_latest_basis_effective(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
) -> Result<Option<i64>, StoreError> {
    sqlx::query_scalar("SELECT effective_at_us FROM billing_m5_cumulative_basis_versions WHERE customer=? AND source=? AND agreement_id=? AND agreement_version=? ORDER BY basis_version DESC LIMIT 1")
        .bind(customer).bind(source).bind(agreement_id).bind(agreement_version)
        .fetch_optional(&mut *conn).await.map_err(Into::into)
}

pub(crate) struct RetainedBasis {
    pub version: i64,
    pub effective_at_us: i64,
    pub basis_bytes: Vec<u8>,
}

pub(crate) async fn cumulative_basis_at(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
    at_us: i64,
) -> Result<Option<RetainedBasis>, StoreError> {
    let row: Option<(i64, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT basis_version,effective_at_us,basis_bytes FROM billing_m5_cumulative_basis_versions WHERE customer=? AND source=? AND agreement_id=? AND agreement_version=? AND effective_at_us<=? ORDER BY basis_version DESC LIMIT 1",
    )
    .bind(customer)
    .bind(source)
    .bind(agreement_id)
    .bind(agreement_version)
    .bind(at_us)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(
        row.map(|(version, effective_at_us, basis_bytes)| RetainedBasis {
            version,
            effective_at_us,
            basis_bytes,
        }),
    )
}

pub(crate) async fn cumulative_basis_version(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
    basis_version: i64,
) -> Result<Option<Vec<u8>>, StoreError> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT basis_bytes FROM billing_m5_cumulative_basis_versions WHERE customer=? AND source=? AND agreement_id=? AND agreement_version=? AND basis_version=?",
    )
    .bind(customer).bind(source).bind(agreement_id).bind(agreement_version).bind(basis_version)
    .fetch_optional(&mut *conn).await?;
    Ok(row.map(|row| row.0))
}

pub(crate) async fn cumulative_period_at(
    conn: &mut SqliteConnection,
    customer: &str,
    at_us: i64,
) -> Result<(i64, i64), StoreError> {
    let (version, effective, term_bytes) = initial_assignment_at(conn, customer, at_us)
        .await?
        .ok_or(StoreError::BillingPeriod)?;
    let term_payload: Vec<u8> = sqlx::query_scalar(
        "SELECT r.payload_bytes FROM billing_m5_term_versions t JOIN billing_m5_records r ON r.sequence=t.record_sequence WHERE t.customer=? AND t.term_version=?",
    )
    .bind(customer)
    .bind(version)
    .fetch_one(&mut *conn)
    .await?;
    let payload = canonical(&term_payload, 262_144)?;
    if utc_us(&payload["effective_at"])? != effective {
        return Err(StoreError::InvalidStore(
            "M5 cumulative term effective time",
        ));
    }
    let term = canonical(&term_bytes, 262_144)?;
    let verification = CanonicalBytes::from_value(&json!({
        "schema":"ledger-billing-term/1","customer":customer,
        "change_id":"cumulative-period","expected_revision":"0",
        "effective":{"mode":"initial","at":payload["effective_at"]},"term":term
    }))
    .map_err(|_| StoreError::InvalidStore("M5 cumulative term"))?;
    let request = term_service::parse_initial_request(verification.as_slice())
        .map_err(|_| StoreError::InvalidStore("M5 cumulative term"))?;
    let history = [term_service::BillableHistoryRow {
        ordinal: 1,
        accepted_at_us: at_us,
    }];
    let plan = term_service::plan_initial_activation(request, version as u64, &history)
        .map_err(|_| StoreError::BillingPeriod)?;
    Ok((version, plan.assignments[0].period_index as i64))
}

pub(crate) async fn append_cumulative_basis(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    agreement_id: &str,
    agreement_version: i64,
    basis_version: i64,
    effective_at_us: i64,
    basis_bytes: &[u8],
) -> Result<(), StoreError> {
    if command.children.len() != 1
        || command.children[0].family != "ledger-billing-cumulative-basis-version/1"
    {
        return Err(StoreError::Integrity("M5 cumulative basis child"));
    }
    let child = canonical(command.children[0].payload, 262_144)?;
    if child["customer"] != command.customer.unwrap_or("")
        || child["source"] != command.source.unwrap_or("")
        || child["agreement_id"] != agreement_id
        || decimal(&child["agreement_version"])? != agreement_version
        || decimal(&child["basis_version"])? != basis_version
        || utc_us(&child["effective_at"])? != effective_at_us
        || CanonicalBytes::from_value(&child["basis"])
            .map_err(|_| StoreError::Integrity("M5 cumulative basis"))?
            .as_slice()
            != basis_bytes
    {
        return Err(StoreError::Integrity("M5 cumulative basis projection"));
    }
    let _ = canonical(basis_bytes, 262_144)?;
    append(conn, command).await?;
    let sequence: i64 = sqlx::query_scalar(
        "SELECT sequence FROM billing_m5_records WHERE command_sequence=(SELECT next_command_sequence-1 FROM billing_m5_state WHERE singleton=1)",
    )
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("INSERT INTO billing_m5_cumulative_basis_versions(customer,source,agreement_id,agreement_version,basis_version,record_sequence,effective_at_us,basis_bytes) VALUES(?,?,?,?,?,?,?,?)")
        .bind(command.customer).bind(command.source).bind(agreement_id).bind(agreement_version)
        .bind(basis_version).bind(sequence).bind(effective_at_us).bind(basis_bytes)
        .execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct ActivityIdentity {
    pub retained_bytes: Vec<u8>,
    pub identity_key: Vec<u8>,
    pub response: Vec<u8>,
}

pub(crate) async fn activity_delivery(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    id: &str,
) -> Result<Option<ActivityIdentity>, StoreError> {
    let row: Option<IdentityLookupRow> = sqlx::query_as(
        "SELECT d.ingress_bytes,d.ingress_sha256,c.identity_key,c.response_bytes FROM billing_m5_activity_deliveries d JOIN billing_m5_commands c ON c.command_sequence=d.command_sequence WHERE d.customer=? AND d.source=? AND d.external_id=?",
    )
    .bind(customer).bind(source).bind(id).fetch_optional(&mut *conn).await?;
    row.map(|(retained_bytes, digest, identity_key, response)| {
        canonical(&retained_bytes, 262_144)?;
        if digest.as_slice() != hash(b"bean-counter/m5/activity-ingress/1\0", &retained_bytes) {
            return Err(StoreError::Integrity("M5 activity delivery"));
        }
        Ok(ActivityIdentity {
            retained_bytes,
            identity_key,
            response,
        })
    })
    .transpose()
}

pub(crate) async fn activity_semantic(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    operation_id: &str,
) -> Result<Option<ActivityIdentity>, StoreError> {
    let row: Option<IdentityLookupRow> = sqlx::query_as(
        "SELECT s.facts_bytes,s.facts_sha256,c.identity_key,c.response_bytes FROM billing_m5_activity_semantics s JOIN billing_m5_commands c ON c.command_sequence=s.command_sequence WHERE s.customer=? AND s.source=? AND s.operation_id=?",
    )
    .bind(customer).bind(source).bind(operation_id).fetch_optional(&mut *conn).await?;
    row.map(|(retained_bytes, digest, identity_key, response)| {
        canonical(&retained_bytes, 262_144)?;
        if digest.as_slice() != hash(b"bean-counter/m5/activity-facts/1\0", &retained_bytes) {
            return Err(StoreError::Integrity("M5 activity semantic"));
        }
        Ok(ActivityIdentity {
            retained_bytes,
            identity_key,
            response,
        })
    })
    .transpose()
}

async fn reserve_identity_capacity(
    conn: &mut SqliteConnection,
    rows: i64,
    bytes: i64,
) -> Result<(), StoreError> {
    let (record_count, canonical_bytes, identity_count, identity_bytes, next_command):
        (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT record_count,canonical_bytes,activity_identity_count,activity_identity_bytes,next_command_sequence FROM billing_m5_state WHERE singleton=1",
        ).fetch_one(&mut *conn).await?;
    if rows < 0
        || bytes < 0
        || record_count + next_command - 1 + identity_count + rows > MAX_ROWS
        || canonical_bytes + identity_bytes + bytes > MAX_BYTES
    {
        return Err(StoreError::BillingHistoryLimit);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn append_activity(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    operation_id: &str,
    external_id: &str,
    facts: &[u8],
    ingress: &[u8],
    term_version: i64,
    period_index: i64,
) -> Result<(), StoreError> {
    if command.children.len() != 1
        || command.children[0].family != "ledger-billing-activity-record/1"
    {
        return Err(StoreError::Integrity("M5 activity command"));
    }
    let normalized = CanonicalBytes::from_value(
        &parse_bounded(command.request, 262_144)
            .map_err(|_| StoreError::Integrity("M5 activity request"))?,
    )
    .map_err(|_| StoreError::Integrity("M5 activity request"))?;
    if normalized.as_slice() != ingress {
        return Err(StoreError::Integrity("M5 activity ingress"));
    }
    canonical(facts, 262_144)?;
    canonical(ingress, 262_144)?;
    let retained_bytes = [
        command.identity_key,
        command.request,
        command.response,
        command.children[0].payload,
        facts,
        ingress,
    ]
    .iter()
    .try_fold(0i64, |total, part| {
        total
            .checked_add(part.len() as i64)
            .ok_or(StoreError::BillingHistoryLimit)
    })?;
    reserve_identity_capacity(conn, 4, retained_bytes).await?;
    let command_sequence = append(conn, command).await?;
    let record_sequence: i64 =
        sqlx::query_scalar("SELECT sequence FROM billing_m5_records WHERE command_sequence=?")
            .bind(command_sequence)
            .fetch_one(&mut *conn)
            .await?;
    let child = canonical(command.children[0].payload, 262_144)?;
    let record_id = child["record"]["record_id"]
        .as_str()
        .ok_or(StoreError::Integrity("M5 activity ID"))?;
    if child["activity_id"] != external_id
        || child["operation_id"] != operation_id
        || decimal(&child["period_id"]["term_version"])? != term_version
        || decimal(&child["period_id"]["period_index"])? != period_index
    {
        return Err(StoreError::Integrity("M5 activity projection"));
    }
    sqlx::query("INSERT INTO billing_m5_activity_semantics(customer,source,operation_id,command_sequence,activity_sequence,facts_bytes,facts_sha256) VALUES(?,?,?,?,?,?,?)")
        .bind(command.customer).bind(command.source).bind(operation_id).bind(command_sequence)
        .bind(record_sequence).bind(facts).bind(hash(b"bean-counter/m5/activity-facts/1\0", facts).to_vec())
        .execute(&mut *conn).await?;
    sqlx::query("INSERT INTO billing_m5_activity_deliveries(customer,source,external_id,command_sequence,activity_sequence,ingress_bytes,ingress_sha256) VALUES(?,?,?,?,?,?,?)")
        .bind(command.customer).bind(command.source).bind(external_id).bind(command_sequence)
        .bind(record_sequence).bind(ingress).bind(hash(b"bean-counter/m5/activity-ingress/1\0", ingress).to_vec())
        .execute(&mut *conn).await?;
    sqlx::query("UPDATE billing_m5_state SET activity_identity_count=activity_identity_count+2,activity_identity_bytes=activity_identity_bytes+? WHERE singleton=1")
        .bind((facts.len()+ingress.len()) as i64).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES(?,?,?,?,'m5',?,?,?,'acceptance-time',?)")
        .bind(command.customer).bind(command.source).bind("ledger-billing-activity-record/1")
        .bind(record_id).bind(record_sequence).bind(term_version).bind(period_index)
        .bind(command.accepted_at_us).execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn append_activity_alias(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    external_id: &str,
    ingress: &[u8],
    identity_key: &[u8],
) -> Result<(), StoreError> {
    canonical(ingress, 262_144)?;
    let (command_sequence, activity_sequence): (i64, i64) = sqlx::query_as(
        "SELECT s.command_sequence,s.activity_sequence FROM billing_m5_activity_semantics s JOIN billing_m5_commands c ON c.command_sequence=s.command_sequence WHERE s.customer=? AND s.source=? AND c.identity_key=?",
    ).bind(customer).bind(source).bind(identity_key).fetch_one(&mut *conn).await?;
    reserve_identity_capacity(conn, 1, ingress.len() as i64).await?;
    sqlx::query("INSERT INTO billing_m5_activity_deliveries(customer,source,external_id,command_sequence,activity_sequence,ingress_bytes,ingress_sha256) VALUES(?,?,?,?,?,?,?)")
        .bind(customer).bind(source).bind(external_id).bind(command_sequence).bind(activity_sequence)
        .bind(ingress).bind(hash(b"bean-counter/m5/activity-ingress/1\0", ingress).to_vec())
        .execute(&mut *conn).await?;
    sqlx::query("UPDATE billing_m5_state SET activity_identity_count=activity_identity_count+1,activity_identity_bytes=activity_identity_bytes+? WHERE singleton=1")
        .bind(ingress.len() as i64).execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) struct CumulativeAssignedRecord {
    pub sequence: i64,
    pub family: String,
    pub payload: Vec<u8>,
}

pub(crate) async fn cumulative_period_records(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
    high_water: i64,
) -> Result<Vec<CumulativeAssignedRecord>, StoreError> {
    let rows: Vec<AssignedCumulativeRow> = sqlx::query_as(
        "SELECT r.sequence,r.family,r.payload_bytes,a.source_record_kind,a.source_record_id,a.source_scope,r.record_id FROM billing_m5_assignments a JOIN billing_m5_records r ON r.sequence=a.source_sequence WHERE a.customer=? AND a.term_version=? AND a.period_index=? AND a.source_stream='m5' AND a.source_sequence<=? ORDER BY r.sequence",
    )
    .bind(customer).bind(term_version).bind(period_index).bind(high_water)
    .fetch_all(&mut *conn).await?;
    let mut result = Vec::with_capacity(rows.len());
    for (sequence, family, payload, kind, id, scope, record_id) in rows {
        let value = canonical(&payload, 262_144)?;
        if kind != family
            || id != record_id
            || value["record"]["record_id"] != record_id
            || value["source"] != scope
            || value["customer"] != customer
        {
            return Err(StoreError::InvalidStore("M5 cumulative assignment source"));
        }
        result.push(CumulativeAssignedRecord {
            sequence,
            family,
            payload,
        });
    }
    Ok(result)
}

pub(crate) async fn cumulative_activity_target(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    target_id: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT r.payload_bytes FROM billing_m5_activity_deliveries d JOIN billing_m5_records r ON r.sequence=d.activity_sequence WHERE d.customer=? AND d.source=? AND d.external_id=?",
    )
    .bind(customer).bind(source).bind(target_id).fetch_optional(&mut *conn).await?;
    Ok(row.map(|r| r.0))
}

pub(crate) async fn cumulative_target_deltas(
    conn: &mut SqliteConnection,
    customer: &str,
    source: &str,
    activity_id: &str,
) -> Result<Vec<Vec<u8>>, StoreError> {
    let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
        "SELECT r.payload_bytes FROM billing_m5_records r WHERE r.family='ledger-billing-quantity-correction-record/1' AND r.customer=? AND r.source=? ORDER BY r.sequence",
    ).bind(customer).bind(source).fetch_all(&mut *conn).await?;
    let mut matching = Vec::new();
    for (raw,) in rows {
        let value = canonical(&raw, 262_144)?;
        if value["target_activity_id"] == activity_id {
            matching.push(raw);
        }
    }
    Ok(matching)
}

pub(crate) async fn cumulative_period_closed(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?)")
        .bind(customer).bind(term_version).bind(period_index).fetch_one(&mut *conn).await.map_err(Into::into)
}

pub(crate) async fn append_cumulative_correction(
    conn: &mut SqliteConnection,
    command: &Command<'_>,
    term_version: i64,
    period_index: i64,
) -> Result<(), StoreError> {
    if command.children.len() != 1
        || command.children[0].family != "ledger-billing-quantity-correction-record/1"
    {
        return Err(StoreError::Integrity("M5 cumulative correction child"));
    }
    let payload = canonical(command.children[0].payload, 262_144)?;
    if payload["mode"] != "cumulative"
        || payload["correction_route"] != "original-open-period"
        || decimal(&payload["original_period_id"]["term_version"])? != term_version
        || decimal(&payload["original_period_id"]["period_index"])? != period_index
        || payload["assigned_period_id"] != payload["original_period_id"]
        || cumulative_period_closed(
            conn,
            command
                .customer
                .ok_or(StoreError::Integrity("M5 correction customer"))?,
            term_version,
            period_index,
        )
        .await?
    {
        return Err(StoreError::BillingPeriod);
    }
    let sequence = append(conn, command).await?;
    let record_sequence: i64 =
        sqlx::query_scalar("SELECT sequence FROM billing_m5_records WHERE command_sequence=?")
            .bind(sequence)
            .fetch_one(&mut *conn)
            .await?;
    sqlx::query("INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES(?,?,?,?,'m5',?,?,?,'linked-open-period',?)")
        .bind(command.customer).bind(command.source).bind("ledger-billing-quantity-correction-record/1")
        .bind(payload["record"]["record_id"].as_str()).bind(record_sequence).bind(term_version).bind(period_index)
        .bind(command.accepted_at_us).execute(&mut *conn).await?;
    append_boundary(conn).await?;
    Ok(())
}

pub(crate) async fn verify_cumulative_basis_projections(
    conn: &mut SqliteConnection,
) -> Result<(), StoreError> {
    let rows: Vec<BasisProjectionRow> = sqlx::query_as(
        "SELECT customer,source,agreement_id,agreement_version,basis_version,record_sequence,effective_at_us,basis_bytes FROM billing_m5_cumulative_basis_versions ORDER BY customer,source,agreement_id,agreement_version,basis_version",
    ).fetch_all(&mut *conn).await?;
    let record_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-cumulative-basis-version/1'",
    ).fetch_one(&mut *conn).await?;
    if rows.len() as i64 != record_count {
        return Err(StoreError::InvalidStore("M5 cumulative basis count"));
    }
    let mut revisions = BTreeMap::<(String, String, String, i64), i64>::new();
    for (
        customer,
        source,
        agreement_id,
        agreement_version,
        version,
        sequence,
        effective,
        basis_bytes,
    ) in rows
    {
        let row: Option<(String,Vec<u8>,Vec<u8>)> = sqlx::query_as(
            "SELECT c.family,c.request_bytes,r.payload_bytes FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE r.sequence=?",
        ).bind(sequence).fetch_optional(&mut *conn).await?;
        let (command_family, request_bytes, payload_bytes) =
            row.ok_or(StoreError::InvalidStore("M5 cumulative basis record"))?;
        let request = parse_bounded(&request_bytes, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 cumulative basis request"))?;
        let payload = canonical(&payload_bytes, 262_144)?;
        let basis = canonical(&basis_bytes, 262_144)?;
        let key = (
            customer.clone(),
            source.clone(),
            agreement_id.clone(),
            agreement_version,
        );
        let previous = revisions.entry(key).or_default();
        *previous += 1;
        if command_family != "ledger-billing-cumulative-agreement/1"
            || *previous != version
            || payload["schema"] != "ledger-billing-cumulative-basis-version/1"
            || payload["customer"] != customer
            || payload["source"] != source
            || payload["agreement_id"] != agreement_id
            || decimal(&payload["agreement_version"])? != agreement_version
            || decimal(&payload["basis_version"])? != version
            || utc_us(&payload["effective_at"])? != effective
            || payload["basis"] != basis
            || request["customer"] != customer
            || request["source"] != source
            || request["agreement_id"] != agreement_id
            || decimal(&request["agreement_version"])? != agreement_version
            || normalized_cumulative_basis(&request["basis"])? != basis
            || utc_us(&request["effective_at"])? != effective
            || decimal(&request["expected_revision"])? != version - 1
        {
            return Err(StoreError::InvalidStore("M5 cumulative basis projection"));
        }
    }
    Ok(())
}

fn activity_facts(value: &Value) -> Result<Vec<u8>, StoreError> {
    let facts = json!({
        "schema":"ledger-billing-activity-facts/1","customer":value["customer"],
        "target":value["target"],"quantity":value["quantity"],
        "occurred_at":value["occurred_at"],"evidence":value["evidence"]
    });
    Ok(CanonicalBytes::from_value(&facts)
        .map_err(|_| StoreError::InvalidStore("M5 activity facts"))?
        .into_vec())
}

pub(crate) async fn verify_cumulative_activity_identities(
    conn: &mut SqliteConnection,
) -> Result<(), StoreError> {
    let assignments: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_assignments WHERE source_stream='m5' AND source_record_kind='ledger-billing-activity-record/1'",
    ).fetch_one(&mut *conn).await?;
    let activity_records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-activity-record/1'",
    )
    .fetch_one(&mut *conn)
    .await?;
    let semantic_records: i64 =
        sqlx::query_scalar("SELECT count(*) FROM billing_m5_activity_semantics")
            .fetch_one(&mut *conn)
            .await?;
    let correction_assignments: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_assignments WHERE source_stream='m5' AND source_record_kind='ledger-billing-quantity-correction-record/1'",
    ).fetch_one(&mut *conn).await?;
    let correction_records: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-quantity-correction-record/1'",
    ).fetch_one(&mut *conn).await?;
    if assignments != activity_records
        || semantic_records != activity_records
        || correction_assignments != correction_records
    {
        return Err(StoreError::InvalidStore(
            "M5 cumulative source assignment count",
        ));
    }
    let semantics: Vec<SemanticProjectionRow> = sqlx::query_as(
        "SELECT s.customer,s.source,s.operation_id,s.command_sequence,s.activity_sequence,s.facts_bytes,c.request_bytes,r.payload_bytes FROM billing_m5_activity_semantics s JOIN billing_m5_commands c ON c.command_sequence=s.command_sequence JOIN billing_m5_records r ON r.sequence=s.activity_sequence ORDER BY s.customer,s.source,s.operation_id",
    ).fetch_all(&mut *conn).await?;
    for (customer, source, operation, command, activity, facts, request, payload) in semantics {
        let request = parse_bounded(&request, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 activity request"))?;
        let payload = canonical(&payload, 262_144)?;
        let assigned: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM billing_m5_assignments WHERE source_stream='m5' AND source_sequence=? AND customer=? AND source_scope=? AND source_record_id=?",
        ).bind(activity).bind(&customer).bind(&source).bind(payload["record"]["record_id"].as_str()).fetch_one(&mut *conn).await?;
        let original_delivery: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM billing_m5_activity_deliveries WHERE customer=? AND source=? AND external_id=? AND command_sequence=? AND activity_sequence=?",
        ).bind(&customer).bind(&source).bind(request["id"].as_str()).bind(command).bind(activity).fetch_one(&mut *conn).await?;
        if request["schema"] != "ledger-billing-activity/1"
            || request["customer"] != customer
            || request["source"] != source
            || request["operation_id"] != operation
            || activity_facts(&request)? != facts
            || payload["schema"] != "ledger-billing-activity-record/1"
            || payload["customer"] != customer
            || payload["source"] != source
            || payload["operation_id"] != operation
            || payload["activity_id"] != request["id"]
            || payload["target"] != request["target"]
            || payload["quantity"] != request["quantity"]
            || payload["occurred_at"] != request["occurred_at"]
            || assigned != 1
            || original_delivery != 1
        {
            return Err(StoreError::InvalidStore("M5 activity semantic projection"));
        }
    }
    let deliveries: Vec<DeliveryProjectionRow> = sqlx::query_as(
        "SELECT d.customer,d.source,d.external_id,d.ingress_bytes,s.facts_bytes,s.operation_id,c.request_bytes FROM billing_m5_activity_deliveries d JOIN billing_m5_activity_semantics s ON s.command_sequence=d.command_sequence AND s.activity_sequence=d.activity_sequence JOIN billing_m5_commands c ON c.command_sequence=d.command_sequence ORDER BY d.customer,d.source,d.external_id",
    ).fetch_all(&mut *conn).await?;
    for (customer, source, external, ingress, facts, operation, original) in deliveries {
        let value = canonical(&ingress, 262_144)?;
        let original = parse_bounded(&original, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 activity original"))?;
        if value["schema"] != "ledger-billing-activity/1"
            || value["customer"] != customer
            || value["source"] != source
            || value["id"] != external
            || value["operation_id"] != operation
            || activity_facts(&value)? != facts
            || original["operation_id"] != operation
        {
            return Err(StoreError::InvalidStore("M5 activity delivery projection"));
        }
    }
    Ok(())
}

pub(crate) async fn verify_cumulative_corrections(
    conn: &mut SqliteConnection,
) -> Result<(), StoreError> {
    let rows:Vec<(i64,Vec<u8>,Vec<u8>,i64)> = sqlx::query_as(
        "SELECT r.sequence,r.payload_bytes,c.request_bytes,c.accepted_at_us FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE r.family='ledger-billing-quantity-correction-record/1' ORDER BY r.sequence",
    ).fetch_all(&mut *conn).await?;
    let mut quantities = BTreeMap::<(String, String, String), i128>::new();
    for (sequence, payload_bytes, request_bytes, accepted_at_us) in rows {
        let payload = canonical(&payload_bytes, 262_144)?;
        let request = parse_bounded(&request_bytes, 262_144)
            .map_err(|_| StoreError::InvalidStore("M5 correction request"))?;
        let customer = request["customer"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction customer"))?;
        let source = request["source"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction source"))?;
        let target = request["target"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction target"))?;
        let activity_bytes = cumulative_activity_target(conn, customer, source, target)
            .await?
            .ok_or(StoreError::InvalidStore("M5 correction target"))?;
        let activity = canonical(&activity_bytes, 262_144)?;
        let activity_id = activity["activity_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction activity"))?;
        let key = (
            customer.to_owned(),
            source.to_owned(),
            activity_id.to_owned(),
        );
        let previous = *quantities
            .get(&key)
            .unwrap_or(&cumulative_integer(&activity["quantity"])?);
        let delta = cumulative_integer(&request["quantity_delta"])?;
        let resulting = previous
            .checked_add(delta)
            .ok_or(StoreError::InvalidStore("M5 correction quantity"))?;
        let agreement_id = activity["agreement_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction agreement"))?;
        let agreement_version = decimal(&activity["agreement_version"])?;
        let basis_version = decimal(&activity["basis_version"])?;
        let basis_bytes = cumulative_basis_version(
            conn,
            customer,
            source,
            agreement_id,
            agreement_version,
            basis_version,
        )
        .await?
        .ok_or(StoreError::InvalidStore("M5 correction basis"))?;
        let basis = canonical(&basis_bytes, 262_144)?;
        let term_version = decimal(&activity["period_id"]["term_version"])?;
        let period_index = decimal(&activity["period_id"]["period_index"])?;
        let close_cut:Option<i64>=sqlx::query_scalar("SELECT m5_high_water FROM billing_m5_period_closes WHERE customer=? AND term_version=? AND period_index=?")
            .bind(customer).bind(term_version).bind(period_index).fetch_optional(&mut *conn).await?;
        if request["schema"] != "ledger-billing-quantity-correction/1"
            || payload["schema"] != "ledger-billing-quantity-correction-record/1"
            || payload["customer"] != customer
            || payload["source"] != source
            || payload["correction_id"] != request["id"]
            || payload["target_activity_id"] != activity_id
            || payload["quantity_delta"] != request["quantity_delta"]
            || request["quantity_delta"] != delta.to_string()
            || delta == 0
            || resulting < 0
            || resulting > cumulative_integer(&basis["maximum_period_quantity"])?
            || payload["resulting_quantity"] != resulting.to_string()
            || payload["mode"] != "cumulative"
            || payload["unit"] != basis["source_unit"]
            || decimal(&payload["basis_version"])? != basis_version
            || payload["original_period_id"] != activity["period_id"]
            || payload["assigned_period_id"] != activity["period_id"]
            || payload["correction_route"] != "original-open-period"
            || utc_us(&payload["record"]["accepted_at"])? != accepted_at_us
            || close_cut.is_some_and(|cut| sequence > cut)
        {
            return Err(StoreError::InvalidStore(
                "M5 cumulative correction projection",
            ));
        }
        quantities.insert(key, resulting);
    }
    Ok(())
}

fn cumulative_integer(value: &Value) -> Result<i128, StoreError> {
    value
        .as_str()
        .ok_or(StoreError::InvalidStore("M5 cumulative integer"))?
        .parse::<i128>()
        .map_err(|_| StoreError::InvalidStore("M5 cumulative integer"))
}

fn normalized_cumulative_basis(value: &Value) -> Result<Value, StoreError> {
    let mut basis = value.clone();
    let numerator = cumulative_integer(&basis["conversion_numerator"])?;
    let denominator = cumulative_integer(&basis["conversion_denominator"])?;
    if numerator <= 0 || denominator <= 0 {
        return Err(StoreError::InvalidStore("M5 cumulative conversion"));
    }
    let divisor = cumulative_gcd(numerator, denominator);
    basis["conversion_numerator"] = json!((numerator / divisor).to_string());
    basis["conversion_denominator"] = json!((denominator / divisor).to_string());
    Ok(basis)
}

fn cumulative_rate_atoms(rate: &str) -> Result<i128, StoreError> {
    let (whole, fraction) = rate.split_once('.').unwrap_or((rate, ""));
    if whole.is_empty()
        || fraction.len() > 18
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(StoreError::InvalidStore("M5 cumulative rate"));
    }
    ledgerlab_core::money::Decimal::parse(rate)
        .and_then(|decimal| decimal.atoms_exact(18))
        .ok()
        .filter(|n| *n > 0)
        .ok_or(StoreError::InvalidStore("M5 cumulative rate"))
}

fn cumulative_gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b)
    }
    a
}

#[derive(Default)]
struct CloseBucket {
    quantity: i128,
    source_records: Vec<Value>,
}

pub(crate) async fn expected_cumulative_close_economics(
    conn: &mut SqliteConnection,
    customer: &str,
    term_version: i64,
    period_index: i64,
    m5_high_water: i64,
) -> Result<(Vec<Value>, Vec<Value>, i128), StoreError> {
    let rows = cumulative_period_records(conn, customer, term_version, period_index, m5_high_water)
        .await?;
    type BucketKey = (String, String, i64, i64);
    let mut buckets = BTreeMap::<BucketKey, CloseBucket>::new();
    let mut activities = BTreeMap::<(String, String), BucketKey>::new();
    for row in &rows {
        if row.family != "ledger-billing-activity-record/1" {
            continue;
        }
        let value = canonical(&row.payload, 262_144)?;
        let source = value["source"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 activity source"))?;
        let activity_id = value["activity_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 activity ID"))?;
        let agreement_id = value["agreement_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 activity agreement"))?;
        let agreement_version = decimal(&value["agreement_version"])?;
        let basis_version = decimal(&value["basis_version"])?;
        let quantity = cumulative_integer(&value["quantity"])?;
        if quantity <= 0 {
            return Err(StoreError::InvalidStore("M5 activity quantity"));
        }
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
            return Err(StoreError::InvalidStore("M5 activity duplicate"));
        }
        let bucket = buckets.entry(key).or_default();
        bucket.quantity = bucket
            .quantity
            .checked_add(quantity)
            .ok_or(StoreError::InvalidStore("M5 bucket quantity"))?;
        bucket.source_records.push(json!({"customer":customer,"source":source,"kind":row.family,"id":value["record"]["record_id"]}));
    }
    for row in &rows {
        if row.family != "ledger-billing-quantity-correction-record/1" {
            continue;
        }
        let value = canonical(&row.payload, 262_144)?;
        if value["mode"] != "cumulative" || value["correction_route"] != "original-open-period" {
            return Err(StoreError::InvalidStore("M5 cumulative correction route"));
        }
        let source = value["source"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction source"))?;
        let target = value["target_activity_id"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 correction target"))?;
        let key = activities
            .get(&(source.to_owned(), target.to_owned()))
            .ok_or(StoreError::InvalidStore("M5 correction activity"))?;
        if decimal(&value["basis_version"])? != key.3 {
            return Err(StoreError::InvalidStore("M5 correction basis"));
        }
        let bucket = buckets
            .get_mut(key)
            .ok_or(StoreError::InvalidStore("M5 correction bucket"))?;
        bucket.quantity = bucket
            .quantity
            .checked_add(cumulative_integer(&value["quantity_delta"])?)
            .ok_or(StoreError::InvalidStore("M5 bucket quantity"))?;
        bucket.source_records.push(json!({"customer":customer,"source":source,"kind":row.family,"id":value["record"]["record_id"]}));
    }
    let period_id =
        json!({"term_version":term_version.to_string(),"period_index":period_index.to_string()});
    let mut lines = Vec::new();
    let mut included = Vec::new();
    let mut net = 0i128;
    for ((source, agreement_id, agreement_version, basis_version), mut bucket) in buckets {
        let basis_bytes = cumulative_basis_version(
            conn,
            customer,
            &source,
            &agreement_id,
            agreement_version,
            basis_version,
        )
        .await?
        .ok_or(StoreError::InvalidStore("M5 close basis"))?;
        let basis = canonical(&basis_bytes, 262_144)?;
        let maximum = cumulative_integer(&basis["maximum_period_quantity"])?;
        if bucket.quantity < 0 || bucket.quantity > maximum {
            return Err(StoreError::InvalidStore("M5 close quantity"));
        }
        let numerator = cumulative_integer(&basis["conversion_numerator"])?;
        let denominator = cumulative_integer(&basis["conversion_denominator"])?;
        let rate = cumulative_rate_atoms(
            basis["rate_usd_per_billable_unit"]
                .as_str()
                .ok_or(StoreError::InvalidStore("M5 close rate"))?,
        )?;
        if numerator <= 0 || denominator <= 0 || cumulative_gcd(numerator, denominator) != 1 {
            return Err(StoreError::InvalidStore("M5 close conversion"));
        }
        let factor = |n: i128| {
            ledgerlab_core::money::ExactRatio::from_canonical(&n.to_string(), "1")
                .map_err(|_| StoreError::InvalidStore("M5 close amount"))
        };
        let (quantity_ratio, numerator_ratio, rate_ratio, denominator_ratio) = (
            factor(bucket.quantity)?,
            factor(numerator)?,
            factor(rate)?,
            factor(denominator)?,
        );
        let exact = quantity_ratio
            .mul(&numerator_ratio)
            .and_then(|value| value.mul(&rate_ratio))
            .and_then(|value| value.div(&denominator_ratio))
            .map_err(|_| StoreError::InvalidStore("M5 close amount"))?;
        let booked = exact
            .round_atoms()
            .map_err(|_| StoreError::InvalidStore("M5 close amount"))?;
        let exact_value = serde_json::to_value(&exact)
            .map_err(|_| StoreError::InvalidStore("M5 close amount"))?;
        let exact_numerator = exact_value["numerator"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 close amount"))?;
        let exact_denominator = exact_value["denominator"]
            .as_str()
            .ok_or(StoreError::InvalidStore("M5 close amount"))?;
        let setup_bytes: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT setup_bytes FROM billing_agreements WHERE customer=? AND source=? AND agreement_id=? AND agreement_version=? AND transition IN ('start','amend') ORDER BY revision DESC LIMIT 1",
        ).bind(customer).bind(&source).bind(&agreement_id).bind(agreement_version).fetch_optional(&mut *conn).await?;
        let setup = canonical(
            &setup_bytes
                .ok_or(StoreError::InvalidStore("M5 close agreement"))?
                .0,
            262_144,
        )?;
        if setup["customer"] != customer
            || setup["source"] != source
            || setup["agreement"] != agreement_id
        {
            return Err(StoreError::InvalidStore("M5 close agreement"));
        }
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
        let mut line = json!({
            "source_records":bucket.source_records,"basis":"cumulative_close","agreement_id":agreement_id,
            "agreement_version":agreement_version.to_string(),"payer":customer,"recipient":setup["host"],
            "currency":"USD","scale":18,"amount_atoms":booked.to_string(),
            "calculation":{"kind":"cumulative_close","exact_atoms_numerator":exact_numerator.to_string(),
                "exact_atoms_denominator":exact_denominator.to_string(),"booked_atoms":booked.to_string(),
                "rounding":"nearest_ties_away","operands":{"basis_version":basis_version.to_string(),
                    "source_unit":basis["source_unit"],"billable_unit":basis["billable_unit"],
                    "quantity":bucket.quantity.to_string(),"conversion_numerator":basis["conversion_numerator"],
                    "conversion_denominator":basis["conversion_denominator"],
                    "rate_atoms_per_billable_unit":rate.to_string()}}
        });
        let identity=CanonicalBytes::from_value(&json!({"view_kind":"standard","view_identity":{"customer":customer,"period_id":period_id},"line":line}))
            .map_err(|_|StoreError::InvalidStore("M5 close line"))?;
        line["line_id"] = json!(hex(&hash(
            b"bean-counter/m5/statement-line/1\0",
            identity.as_slice()
        )));
        lines.push(line);
        net = net
            .checked_add(booked)
            .ok_or(StoreError::InvalidStore("M5 close net"))?;
    }
    lines.sort_by(|a, b| a["line_id"].as_str().cmp(&b["line_id"].as_str()));
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
    Ok((lines, included, net))
}
