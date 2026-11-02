//! Mechanical custody for explicit operator credits, separate from workflow
//! Actions and legacy causal adjustments. Semantic authority stays with the
//! completed held-prefix producer; opening checks retained byte associations.
use super::{m5, SqliteTx};
use crate::store::errors::StoreError;
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde_json::{json, Value};
use sqlx::SqliteConnection;

mod paid;
pub(crate) use paid::StoredManualCredit;
pub(crate) const REQUEST: &str = "ledger-billing-manual-credit/1";
pub(crate) const RECORD: &str = "ledger-billing-manual-credit-record/1";
pub(crate) const CLAIM: &str = "ledger-billing-manual-credit-presentation-claim/1";
pub(crate) const REF_KIND: &str = "billing-manual-credit";
fn bad() -> StoreError {
    StoreError::Integrity("manual credit custody")
}
fn bytes(v: &Value) -> Result<Vec<u8>, StoreError> {
    CanonicalBytes::from_value(v)
        .map(CanonicalBytes::into_vec)
        .map_err(|_| bad())
}
fn parse(raw: &[u8]) -> Result<Value, StoreError> {
    let v = canonical::parse_bounded(raw, 262144).map_err(|_| bad())?;
    if bytes(&v)? != raw {
        return Err(bad());
    }
    Ok(v)
}
fn txt(v: &Value) -> Result<&str, StoreError> {
    v.as_str().ok_or_else(bad)
}
fn number(v: &Value) -> Result<i64, StoreError> {
    let raw = txt(v)?;
    let n: i64 = raw.parse().map_err(|_| bad())?;
    if n < 0 || n.to_string() != raw {
        return Err(bad());
    }
    Ok(n)
}
fn closed(v: &Value, fields: &[&str]) -> Result<(), StoreError> {
    let object = v.as_object().ok_or_else(bad)?;
    if object.len() != fields.len() || fields.iter().any(|k| !object.contains_key(*k)) {
        return Err(bad());
    }
    Ok(())
}
fn scope(v: &Value) -> Result<(&str, &str, &str), StoreError> {
    closed(v, &["store", "scope"])?;
    let parts = v["scope"].as_array().ok_or_else(bad)?;
    if parts.len() != 2 {
        return Err(bad());
    }
    let result = (txt(&v["store"])?, txt(&parts[0])?, txt(&parts[1])?);
    if result.0.is_empty() || result.1.is_empty() || result.2.is_empty() {
        return Err(bad());
    }
    Ok(result)
}
fn credit_ref(record: &Value) -> Result<Value, StoreError> {
    Ok(json!({"kind":REF_KIND,"id":record["record"]["record_id"],
        "digest":format!("sha256:{}",m5::hex(&m5::hash(b"",&bytes(record)?)))}))
}
fn identity(request: &Value) -> Value {
    json!({"schema":"ledger-billing-m5-command-identity/1","domain":"customer-admin","family":REQUEST,
        "customer":request["customer"],"key_kind":"manual_credit",
        "key":{"workflow_scope":request["workflow_scope"],"adjustment_id":request["adjustment_id"]}})
}
fn child_key(request: &Value) -> Value {
    json!({"role":"manual-credit","customer":request["customer"],
        "workflow_scope":request["workflow_scope"],"adjustment_id":request["adjustment_id"]})
}
fn claim_key(claim: &Value) -> Value {
    json!({"role":"manual-credit-membership","customer":claim["customer"],
        "workflow_scope":claim["workflow_scope"],"manual_credit_ref":claim["manual_credit_ref"]})
}
fn hash_record(record: &Value, family: &str) -> Result<String, StoreError> {
    let mut unsigned = record.clone();
    unsigned["record"]
        .as_object_mut()
        .ok_or_else(bad)?
        .remove("payload_hash");
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    h.update(b"bean-counter/m5/record/1\0");
    h.update(family.as_bytes());
    h.update([0]);
    h.update(bytes(&unsigned)?);
    Ok(m5::hex(&h.finalize()))
}
/// Closed byte/census checks only; this does not authenticate an original
/// workflow source or confer local operator permission.
fn check_credit(
    request: &Value,
    response: &Value,
    record: &Value,
    command: i64,
    sequence: i64,
    accepted: i64,
) -> Result<(), StoreError> {
    use ledgerlab_core::domain::workflow_statement::manual_credit::{DATA_FIELDS, REQUEST_FIELDS};
    closed(request, &REQUEST_FIELDS)?;
    let object = record.as_object().ok_or_else(bad)?;
    if object.len() != DATA_FIELDS.len() + 1
        || !object.contains_key("record")
        || DATA_FIELDS.iter().any(|k| !object.contains_key(*k))
    {
        return Err(bad());
    }
    closed(response, &["schema", "status", "credit_ref", "record"])?;
    scope(&request["workflow_scope"])?;
    if request["schema"] != REQUEST
        || record["schema"] != RECORD
        || response["schema"] != "ledger-billing-manual-credit-result/1"
        || response["status"] != "committed"
        || response["record"] != *record
        || response["credit_ref"] != credit_ref(record)?
    {
        return Err(bad());
    }
    for field in REQUEST_FIELDS.iter().filter(|k| **k != "schema") {
        if request[field] != record[field] {
            return Err(bad());
        }
    }
    closed(
        &record["record"],
        &[
            "record_id",
            "sequence",
            "command_sequence",
            "accepted_at",
            "payload_hash",
        ],
    )?;
    let expected_id = m5::record_id(&bytes(&identity(request))?, &bytes(&child_key(request))?);
    if record["record"]["record_id"] != expected_id
        || number(&record["record"]["sequence"])? != sequence
        || number(&record["record"]["command_sequence"])? != command
        || record["record"]["accepted_at"] != record["accepted_at"]
        || ledgerlab_core::domain::Timestamp::parse(txt(&record["accepted_at"])?)
            .map_err(|_| bad())?
            .micros()
            != accepted
        || record["record"]["payload_hash"] != hash_record(record, RECORD)?
    {
        return Err(bad());
    }
    closed(
        &record["workflow_boundary"],
        &["store", "scope", "commit_sequence"],
    )?;
    if record["workflow_boundary"]["store"] != record["workflow_scope"]["store"]
        || record["workflow_boundary"]["scope"] != record["workflow_scope"]["scope"]
        || number(&record["workflow_boundary"]["commit_sequence"])? < 1
        || record["book"] != "retail"
    {
        return Err(bad());
    }
    closed(&record["amount"], &["atoms", "currency", "scale"])?;
    let amount = txt(&record["amount"]["atoms"])?;
    let digits = amount.strip_prefix('-').ok_or_else(bad)?;
    if digits.is_empty()
        || digits.len() > 30
        || digits.starts_with('0')
        || !digits.bytes().all(|v| v.is_ascii_digit())
        || record["amount"]["currency"] != "USD"
        || record["amount"]["scale"] != 18
        || txt(&record["reason"])?.is_empty()
    {
        return Err(bad());
    }
    Ok(())
}
fn check_claim(claim: &Value, statement: &Value, record: &Value) -> Result<(), StoreError> {
    closed(
        claim,
        &[
            "schema",
            "record",
            "customer",
            "workflow_scope",
            "manual_credit_ref",
            "presentation_kind",
            "statement_id",
            "statement_hash",
        ],
    )?;
    let standard = match txt(&statement["schema"])? {
        "ledger-billing-statement/6" => true,
        "ledger-billing-ad-hoc-statement-result/3" => false,
        _ => return Err(bad()),
    };
    let reference = credit_ref(record)?;
    if claim["schema"] != CLAIM
        || claim["customer"] != record["customer"]
        || claim["workflow_scope"] != record["workflow_scope"]
        || claim["manual_credit_ref"] != reference
        || statement["customer"] != record["customer"]
        || statement["workflow_boundary"]["store"] != record["workflow_scope"]["store"]
        || statement["workflow_boundary"]["scope"] != record["workflow_scope"]["scope"]
        || claim["statement_hash"] != statement["statement_hash"]
        || claim["presentation_kind"]
            != if standard {
                "standard-period"
            } else {
                "ad-hoc"
            }
        || &claim["statement_id"]
            != if standard {
                &statement["statement_hash"]
            } else {
                &statement["statement_id"]
            }
        || claim["record"]["payload_hash"] != hash_record(claim, CLAIM)?
    {
        return Err(bad());
    }
    closed(
        &claim["record"],
        &[
            "record_id",
            "sequence",
            "command_sequence",
            "accepted_at",
            "payload_hash",
        ],
    )?;
    let credit_at = ledgerlab_core::domain::Timestamp::parse(txt(&record["accepted_at"])?)
        .map_err(|_| bad())?
        .micros();
    let presented_at =
        ledgerlab_core::domain::Timestamp::parse(txt(&claim["record"]["accepted_at"])?)
            .map_err(|_| bad())?
            .micros();
    if number(&record["record"]["sequence"])? > number(&statement["m5_high_water"])?
        || credit_at > presented_at
        || number(&record["workflow_boundary"]["commit_sequence"])?
            > number(&statement["workflow_boundary"]["commit_sequence"])?
    {
        return Err(bad());
    }
    if standard {
        let start = ledgerlab_core::domain::Timestamp::parse(txt(&statement["start_utc"])?)
            .map_err(|_| bad())?
            .micros();
        let end = ledgerlab_core::domain::Timestamp::parse(txt(&statement["end_utc"])?)
            .map_err(|_| bad())?
            .micros();
        if credit_at < start
            || credit_at >= end
            || claim["record"]["accepted_at"] != statement["close_acceptance_time"]
        {
            return Err(bad());
        }
    }
    let lines = statement["lines"].as_array().ok_or_else(bad)?;
    let matching: Vec<&Value> = lines
        .iter()
        .filter(|line| line.get("manual_credit_ref") == Some(&reference))
        .collect();
    if matching.len() != 1 {
        return Err(bad());
    }
    let line = matching[0];
    closed(
        line,
        &ledgerlab_core::domain::workflow_statement::manual_credit::LINE_FIELDS,
    )?;
    for name in [
        "binding_ref",
        "agreement_id",
        "agreement_version",
        "book",
        "role_ref",
        "slot_terms_ref",
        "funding_leg",
        "amount",
        "reason",
        "work_ref",
        "stage_instance_ref",
        "source_action_refs",
    ] {
        if line[name] != record[name] {
            return Err(bad());
        }
    }
    let mut data = line.clone();
    data.as_object_mut().ok_or_else(bad)?.remove("line_id");
    let view = if standard {
        json!({"customer":record["customer"],"period_id":statement["period_id"]})
    } else {
        json!({"customer":record["customer"],"command_id":statement["statement_id"]})
    };
    let frame = json!({"view_kind":if standard{"standard"}else{"ad_hoc"},"view_identity":view,"workflow_scope":record["workflow_scope"],"line":data});
    if line["line_id"]
        != m5::hex(&m5::hash(
            b"bean-counter/m5/manual-credit-line/1\0",
            &bytes(&frame)?,
        ))
    {
        return Err(bad());
    }
    Ok(())
}

// Complete command/record/projection custody is checked on every open. These
// bounded legacy-M5 checks are not a replacement for public original replay.
pub(crate) async fn verify(conn: &mut SqliteConnection) -> Result<(), StoreError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    let (commands, records, claims): (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM billing_m5_commands WHERE family='ledger-billing-manual-credit/1'),(SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-manual-credit-record/1'),(SELECT count(*) FROM billing_m5_records WHERE family='ledger-billing-manual-credit-presentation-claim/1')").fetch_one(&mut *conn).await?;
    if version < 21 {
        if commands != 0 || records != 0 || claims != 0 {
            return Err(bad());
        }
        return Ok(());
    }
    if version != 21 {
        return Err(bad());
    }
    let (credits, memberships): (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM billing_manual_credits),(SELECT count(*) FROM billing_manual_credit_membership)").fetch_one(&mut *conn).await?;
    if commands != records || records != credits || claims != memberships {
        return Err(bad());
    }
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT record_sequence FROM billing_manual_credits ORDER BY record_sequence",
    )
    .fetch_all(&mut *conn)
    .await?;
    for sequence in sequences {
        let row = load_row(conn, sequence).await?.ok_or_else(bad)?;
        check_row(&row)?;
    }
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT claim_sequence FROM billing_manual_credit_membership ORDER BY claim_sequence",
    )
    .fetch_all(&mut *conn)
    .await?;
    for sequence in sequences {
        let row = load_claim(conn, sequence).await?.ok_or_else(bad)?;
        check_claim_row(&row)?;
    }
    // No orphan new-profile manual line may escape the claim projection census.
    let rows: Vec<Vec<u8>> = sqlx::query_scalar("SELECT response_bytes FROM billing_m5_commands WHERE family IN ('ledger-billing-period-close/3','ledger-billing-ad-hoc-statement/3') ORDER BY command_sequence").fetch_all(&mut *conn).await?;
    let mut line_count = 0i64;
    for raw in rows {
        let v = parse(&raw)?;
        for line in v["lines"].as_array().ok_or_else(bad)? {
            if let Some(r) = line.get("manual_credit_ref") {
                line_count = line_count.checked_add(1).ok_or_else(bad)?;
                let scoped = json!({"store":v["workflow_boundary"]["store"],"scope":v["workflow_boundary"]["scope"]});
                let (store, tenant, environment) = scope(&scoped)?;
                let count:i64=sqlx::query_scalar("SELECT count(*) FROM billing_manual_credit_membership WHERE customer=? AND logical_store_id=? AND tenant=? AND environment=? AND credit_kind=? AND credit_id=? AND credit_digest=? AND statement_hash=?")
                    .bind(txt(&v["customer"])?).bind(store).bind(tenant).bind(environment).bind(txt(&r["kind"])?).bind(txt(&r["id"])?).bind(txt(&r["digest"])?).bind(txt(&v["statement_hash"])?).fetch_one(&mut *conn).await?;
                if count != 1 {
                    return Err(bad());
                }
            }
        }
    }
    if line_count != claims {
        return Err(bad());
    }
    Ok(())
}

type CreditRow = (
    i64,
    i64,
    i64,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    String,
    bool,
);
const CREDIT_ROW: &str = "SELECT c.command_sequence,r.sequence,c.accepted_at_us,c.identity_key,c.request_bytes,c.response_bytes,r.payload_bytes,c.request_sha256,c.response_sha256,r.content_sha256,p.credit_digest,c.family='ledger-billing-manual-credit/1' AND c.identity_domain='customer-admin' AND c.source IS NULL AND r.family='ledger-billing-manual-credit-record/1' AND r.source IS NULL AND c.child_count=1 AND c.customer=p.customer AND r.customer=p.customer AND p.accepted_at_us=c.accepted_at_us AND p.credit_kind='billing-manual-credit' AND p.credit_id=r.record_id AND json_extract(CAST(r.payload_bytes AS TEXT),'$.customer')=p.customer AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.store')=p.logical_store_id AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.scope[0]')=p.tenant AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.scope[1]')=p.environment AND json_extract(CAST(r.payload_bytes AS TEXT),'$.adjustment_id')=p.adjustment_id FROM billing_manual_credits p JOIN billing_m5_records r ON r.sequence=p.record_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE p.record_sequence=?";
async fn load_row(
    conn: &mut SqliteConnection,
    sequence: i64,
) -> Result<Option<CreditRow>, StoreError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(CREDIT_ROW))
        .bind(sequence)
        .fetch_optional(conn)
        .await?)
}
fn check_row(row: &CreditRow) -> Result<(), StoreError> {
    let (
        command,
        sequence,
        accepted,
        id,
        request,
        response,
        record,
        rq,
        rs,
        content,
        digest,
        joined,
    ) = row;
    let req = canonical::parse_bounded(request, 262144).map_err(|_| bad())?;
    let resp = parse(response)?;
    let rec = parse(record)?;
    if !joined
        || parse(id)? != identity(&req)
        || rq.as_slice() != m5::hash(b"bean-counter/m5/request/1\0", request)
        || rs.as_slice() != m5::hash(b"bean-counter/m5/response/1\0", response)
        || m5::hex(content) != hash_record(&rec, RECORD)?
        || *digest != format!("sha256:{}", m5::hex(&m5::hash(b"", record)))
    {
        return Err(bad());
    }
    check_credit(&req, &resp, &rec, *command, *sequence, *accepted)
}
type ClaimRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, bool, i64, i64);
const CLAIM_ROW: &str = "SELECT r.sequence,r.payload_bytes,c.response_bytes,credit.payload_bytes,c.identity_key,r.family='ledger-billing-manual-credit-presentation-claim/1' AND r.customer=p.customer AND r.source IS NULL AND c.customer=p.customer AND c.source IS NULL AND c.identity_domain='customer-admin' AND c.family IN ('ledger-billing-period-close/3','ledger-billing-ad-hoc-statement/3') AND json_extract(CAST(r.payload_bytes AS TEXT),'$.customer')=p.customer AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.store')=p.logical_store_id AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.scope[0]')=p.tenant AND json_extract(CAST(r.payload_bytes AS TEXT),'$.workflow_scope.scope[1]')=p.environment AND json_extract(CAST(r.payload_bytes AS TEXT),'$.manual_credit_ref.kind')=p.credit_kind AND json_extract(CAST(r.payload_bytes AS TEXT),'$.manual_credit_ref.id')=p.credit_id AND json_extract(CAST(r.payload_bytes AS TEXT),'$.manual_credit_ref.digest')=p.credit_digest AND json_extract(CAST(r.payload_bytes AS TEXT),'$.presentation_kind')=p.presentation_kind AND json_extract(CAST(r.payload_bytes AS TEXT),'$.statement_id')=p.statement_id AND json_extract(CAST(r.payload_bytes AS TEXT),'$.statement_hash')=p.statement_hash,c.command_sequence,c.accepted_at_us FROM billing_manual_credit_membership p JOIN billing_m5_records r ON r.sequence=p.claim_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence JOIN billing_manual_credits a ON a.customer=p.customer AND a.logical_store_id=p.logical_store_id AND a.tenant=p.tenant AND a.environment=p.environment AND a.credit_kind=p.credit_kind AND a.credit_id=p.credit_id AND a.credit_digest=p.credit_digest JOIN billing_m5_records credit ON credit.sequence=a.record_sequence WHERE p.claim_sequence=?";
async fn load_claim(conn: &mut SqliteConnection, seq: i64) -> Result<Option<ClaimRow>, StoreError> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(CLAIM_ROW))
        .bind(seq)
        .fetch_optional(conn)
        .await?)
}
fn check_claim_row(row: &ClaimRow) -> Result<(), StoreError> {
    let (seq, raw, statement, credit, identity, joined, command, accepted) = row;
    let c = parse(raw)?;
    let v = parse(statement)?;
    let r = parse(credit)?;
    if !joined
        || number(&c["record"]["sequence"])? != *seq
        || number(&c["record"]["command_sequence"])? != *command
        || ledgerlab_core::domain::Timestamp::parse(txt(&c["record"]["accepted_at"])?)
            .map_err(|_| bad())?
            .micros()
            != *accepted
        || c["record"]["record_id"] != m5::record_id(identity, &bytes(&claim_key(&c))?)
    {
        return Err(bad());
    }
    check_claim(&c, &v, &r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Connection;

    // This exercises physical custody only. It is not a source-authority or
    // presentation-constructor certificate; those remain held host proofs.
    #[tokio::test]
    async fn manual_credit_projection_claims_are_shared_and_transactional() {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE billing_m5_records(sequence INTEGER PRIMARY KEY)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("BEGIN").execute(&mut conn).await.unwrap();
        sqlx::raw_sql(include_str!("workflow/manual_credit_schema.sql"))
            .execute(&mut conn)
            .await
            .unwrap();
        for seq in 1..=6i64 {
            sqlx::query("INSERT INTO billing_m5_records VALUES(?)")
                .bind(seq)
                .execute(&mut conn)
                .await
                .unwrap();
        }
        // Distinct explicit adjustment identities have independent claims; no
        // uniqueness rule or exhaustion counter is imposed on source Actions.
        for (id, seq) in [("first", 1i64), ("second", 2)] {
            sqlx::query("INSERT INTO billing_manual_credits VALUES('customer','store','tenant','env',?,?,'billing-manual-credit',?,'digest',10)")
                .bind(id).bind(seq).bind(id).execute(&mut conn).await.unwrap();
        }
        for (id, seq, first, second) in [
            ("first", 3i64, "ad-hoc", "standard-period"),
            ("second", 5i64, "standard-period", "ad-hoc"),
        ] {
            sqlx::query("INSERT INTO billing_manual_credit_membership VALUES('customer','store','tenant','env','billing-manual-credit',?,'digest',?,?,?,'hash')")
                .bind(id).bind(seq).bind(first).bind(first).execute(&mut conn).await.unwrap();
            let rejected = sqlx::query("INSERT INTO billing_manual_credit_membership VALUES('customer','store','tenant','env','billing-manual-credit',?,'digest',?,?,?,'other-hash')")
                .bind(id).bind(seq+1).bind(second).bind(second).execute(&mut conn).await.unwrap_err();
            assert!(rejected
                .as_database_error()
                .is_some_and(|e| e.is_unique_violation()));
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM billing_manual_credit_membership")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            2
        );
        assert!(
            sqlx::query("UPDATE billing_manual_credits SET accepted_at_us=11")
                .execute(&mut conn)
                .await
                .is_err()
        );
        assert!(sqlx::query("DELETE FROM billing_manual_credit_membership")
            .execute(&mut conn)
            .await
            .is_err());
        sqlx::query("ROLLBACK").execute(&mut conn).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM billing_m5_records")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM sqlite_schema WHERE substr(name,1,15)='billing_manual_'"
            )
            .fetch_one(&mut conn)
            .await
            .unwrap(),
            0
        );
    }
}
