//! Same-session bounded I/O for manual-credit custody. Returned DATA owns its
//! arena leases; neither this module nor its SQL results grant original proof.
use super::*;
use crate::store::sqlite::workflow::financial::{io, FinancialReadError};
use ledgerlab_core::domain::workflow_statement::manual_credit as data;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, AuditedValue, ExternalBufferLease},
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use std::sync::atomic::Ordering;
type Result<T> = std::result::Result<T, FinancialReadError>;

pub(crate) struct StoredManualCredit {
    pub request: AuditedBytes,
    pub response: AuditedValue,
    pub record: AuditedValue,
    pub sequence: i64,
}
pub(crate) struct ManualCreditSelection {
    pub rows: Vec<StoredManualCredit>,
    _controls: ExternalBufferLease,
}
fn optional_field<'a>(
    v: &'a Value,
    name: &str,
    s: &mut OriginalAuditSession,
) -> Result<Option<&'a Value>> {
    s.work().visit(Visit::Constraint)?;
    for (key, value) in v.as_object().ok_or_else(bad)? {
        s.work().visit(Visit::Field)?;
        if text_equal(key, name, s)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn path<'a>(mut v: &'a Value, names: &[&str], s: &mut OriginalAuditSession) -> Result<&'a Value> {
    for name in names {
        s.work().visit(Visit::Element)?;
        v = optional_field(v, name, s)?.ok_or_else(bad)?;
    }
    Ok(v)
}
fn text<'a>(v: &'a Value, s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.work().visit(Visit::Constraint)?;
    let value = v.as_str().ok_or_else(bad)?;
    s.work().scan_bytes(value.len())?;
    Ok(value)
}
fn scope<'a>(v: &'a Value, s: &mut OriginalAuditSession) -> Result<(&'a str, &'a str, &'a str)> {
    fields(v, &["store", "scope"], s)?;
    let parts = path(v, &["scope"], s)?.as_array().ok_or_else(bad)?;
    s.work()
        .arithmetic(Op::Compare, &[&parts.len().into(), &2usize.into()])?;
    require(parts.len() == 2, s)?;
    s.work().visit(Visit::Element)?;
    let tenant = text(&parts[0], s)?;
    s.work().visit(Visit::Element)?;
    let environment = text(&parts[1], s)?;
    let store = text(path(v, &["store"], s)?, s)?;
    require(
        !store.is_empty() && !tenant.is_empty() && !environment.is_empty(),
        s,
    )?;
    Ok((store, tenant, environment))
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.work().arithmetic(Op::Add, &[&a.into(), &b.into()])?;
    a.checked_add(b).ok_or_else(|| bad().into())
}
fn mul(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.work().arithmetic(Op::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b).ok_or_else(|| bad().into())
}
fn length(n: i64, max: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.work()
        .arithmetic(Op::Compare, &[&n.into(), &0i64.into(), &max.into()])?;
    if n <= 0 || usize::try_from(n).map_err(|_| bad())? > max {
        return Err(bad().into());
    }
    Ok(n as usize)
}
fn compare(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<bool> {
    let a = s.canonical_representation(a, RepresentationKind::Record)?;
    let b = s.canonical_representation(b, RepresentationKind::Record)?;
    s.work().visit(Visit::MemberCompare)?;
    s.work().scan_bytes(a.bytes().len())?;
    s.work().scan_bytes(b.bytes().len())?;
    Ok(a.bytes() == b.bytes())
}
fn value(raw: &[u8], s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let v = s.parse_representation(raw, RepresentationKind::Record)?;
    let encoded = s.canonical_representation(v.value(), RepresentationKind::Record)?;
    s.work().visit(Visit::MemberCompare)?;
    s.work().scan_bytes(raw.len())?;
    s.work().scan_bytes(encoded.bytes().len())?;
    if raw != encoded.bytes() {
        return Err(bad().into());
    }
    Ok(v)
}
fn require(ok: bool, s: &mut OriginalAuditSession) -> Result<()> {
    s.work().visit(Visit::Constraint)?;
    if ok {
        Ok(())
    } else {
        Err(bad().into())
    }
}
fn fields(v: &Value, names: &[&str], s: &mut OriginalAuditSession) -> Result<()> {
    let object = v.as_object().ok_or_else(bad)?;
    s.work()
        .arithmetic(Op::Compare, &[&object.len().into(), &names.len().into()])?;
    require(object.len() == names.len(), s)?;
    for name in names {
        s.work().visit(Visit::Element)?;
        require(optional_field(v, name, s)?.is_some(), s)?;
    }
    Ok(())
}
fn text_equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.work().visit(Visit::MemberCompare)?;
    s.work().scan_bytes(a.len())?;
    s.work().scan_bytes(b.len())?;
    Ok(a == b)
}
fn hashed(parts: &[&[u8]], s: &mut OriginalAuditSession) -> Result<[u8; 32]> {
    use sha2::Digest;
    let mut hash = sha2::Sha256::new();
    for part in parts {
        s.work().visit(Visit::Element)?;
        s.work().hash_bytes(part.len())?;
        hash.update(part);
    }
    Ok(hash.finalize().into())
}
fn hex_matches(
    prefix: &[u8],
    digest: &[u8; 32],
    expected: &str,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let mut encoded = [0u8; 71];
    let size = add(prefix.len(), 64, s)?;
    require(size <= encoded.len(), s)?;
    s.work().write_bytes(size)?;
    encoded[..prefix.len()].copy_from_slice(prefix);
    for (i, byte) in digest.iter().enumerate() {
        s.work().visit(Visit::Element)?;
        encoded[prefix.len() + i * 2] = b"0123456789abcdef"[(byte >> 4) as usize];
        encoded[prefix.len() + i * 2 + 1] = b"0123456789abcdef"[(byte & 15) as usize];
    }
    require(
        text_equal(
            std::str::from_utf8(&encoded[..size]).map_err(|_| bad())?,
            expected,
            s,
        )?,
        s,
    )
}
fn number(v: &Value, s: &mut OriginalAuditSession) -> Result<i64> {
    let raw = text(v, s)?;
    s.work().scan_bytes(raw.len())?;
    require(!raw.is_empty() && raw.len() <= 19, s)?;
    require(raw == "0" || !raw.starts_with('0'), s)?;
    require(raw.bytes().all(|b| b.is_ascii_digit()), s)?;
    let mut result = 0i64;
    for digit in raw.bytes() {
        s.work().visit(Visit::Element)?;
        let digit = i64::from(digit - b'0');
        s.work()
            .arithmetic(Op::Multiply, &[&result.into(), &10i64.into()])?;
        result = result.checked_mul(10).ok_or_else(bad)?;
        s.work()
            .arithmetic(Op::Add, &[&result.into(), &digit.into()])?;
        result = result.checked_add(digit).ok_or_else(bad)?;
    }
    Ok(result)
}
fn identity_owned(req: &Value, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let bytes = data::identity(
        text(path(req, &["customer"], s)?, s)?,
        path(req, &["workflow_scope"], s)?,
        text(path(req, &["adjustment_id"], s)?, s)?,
        s,
    )?;
    value(bytes.bytes(), s)
}
fn child_owned(req: &Value, claim: bool, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let bytes = if claim {
        data::claim_child_key(req, s)?
    } else {
        data::child_key(req, s)?
    };
    value(bytes.bytes(), s)
}
#[allow(clippy::too_many_arguments)]
fn envelope(
    record: &Value,
    sequence: i64,
    command: i64,
    accepted: i64,
    identity: &[u8],
    key: &[u8],
    family: &str,
    s: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    let r = path(record, &["record"], s)?;
    fields(
        r,
        &[
            "record_id",
            "sequence",
            "command_sequence",
            "accepted_at",
            "payload_hash",
        ],
        s,
    )?;
    s.work()
        .arithmetic(Op::Compare, &[&sequence.into(), &command.into()])?;
    require(
        number(path(r, &["sequence"], s)?, s)? == sequence
            && number(path(r, &["command_sequence"], s)?, s)? == command,
        s,
    )?;
    let at = ledgerlab_core::domain::Timestamp::parse_with_work(
        text(path(r, &["accepted_at"], s)?, s)?,
        s.work(),
    )?
    .micros();
    s.work()
        .arithmetic(Op::Compare, &[&at.into(), &accepted.into()])?;
    require(at == accepted, s)?;
    let id_hash = hashed(&[b"bean-counter/m5/record-id/1\0", identity, b"\0", key], s)?;
    hex_matches(b"m5r_", &id_hash, text(path(r, &["record_id"], s)?, s)?, s)?;
    let expected = data::payload_hash(record, family, s)?;
    require(
        compare(expected.value(), path(r, &["payload_hash"], s)?, s)?,
        s,
    )?;
    let raw = text(expected.value(), s)?.as_bytes();
    require(raw.len() == 64, s)?;
    s.work().scan_bytes(raw.len())?;
    s.work().write_bytes(32)?;
    let mut hash = [0u8; 32];
    for (i, pair) in raw.as_chunks::<2>().0.iter().enumerate() {
        s.work().visit(Visit::Element)?;
        let digit = |b: u8| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(bad()),
        };
        hash[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(hash)
}
#[allow(clippy::too_many_arguments)]
fn credit_checked(
    req: &Value,
    response: &Value,
    record: &Value,
    command: i64,
    sequence: i64,
    accepted: i64,
    id: &[u8],
    key: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<[u8; 32]> {
    use ledgerlab_core::domain::workflow_statement::manual_credit::{DATA_FIELDS, REQUEST_FIELDS};
    ledgerlab_core::domain::workflow_statement::manual_credit::request(req, s)?;
    let mut names = [""; 21];
    names[..20].copy_from_slice(&DATA_FIELDS);
    names[20] = "record";
    fields(record, &names, s)?;
    fields(response, &["schema", "status", "credit_ref", "record"], s)?;
    require(
        text_equal(text(path(record, &["schema"], s)?, s)?, RECORD, s)?,
        s,
    )?;
    require(
        text_equal(
            text(path(response, &["schema"], s)?, s)?,
            "ledger-billing-manual-credit-result/1",
            s,
        )?,
        s,
    )?;
    require(
        text_equal(text(path(response, &["status"], s)?, s)?, "committed", s)?,
        s,
    )?;
    require(compare(path(response, &["record"], s)?, record, s)?, s)?;
    for name in REQUEST_FIELDS.iter().filter(|name| **name != "schema") {
        s.work().visit(Visit::Field)?;
        require(
            compare(path(req, &[name], s)?, path(record, &[name], s)?, s)?,
            s,
        )?;
    }
    let expected = identity_owned(req, s)?;
    let actual = value(id, s)?;
    require(compare(actual.value(), expected.value(), s)?, s)?;
    let expected = child_owned(req, false, s)?;
    let actual = value(key, s)?;
    require(compare(actual.value(), expected.value(), s)?, s)?;
    require(
        compare(
            path(record, &["record", "accepted_at"], s)?,
            path(record, &["accepted_at"], s)?,
            s,
        )?,
        s,
    )?;
    let hash = envelope(record, sequence, command, accepted, id, key, RECORD, s)?;
    let r = path(response, &["credit_ref"], s)?;
    ledgerlab_core::domain::workflow_statement::manual_credit::credit_ref(r, s)?;
    require(
        compare(
            path(r, &["id"], s)?,
            path(record, &["record", "record_id"], s)?,
            s,
        )?,
        s,
    )?;
    let encoded = s.canonical_representation(record, RepresentationKind::Record)?;
    let full = hashed(&[encoded.bytes()], s)?;
    hex_matches(b"sha256:", &full, text(path(r, &["digest"], s)?, s)?, s)?;
    fields(
        path(record, &["workflow_boundary"], s)?,
        &["store", "scope", "commit_sequence"],
        s,
    )?;
    require(
        compare(
            path(record, &["workflow_boundary", "store"], s)?,
            path(record, &["workflow_scope", "store"], s)?,
            s,
        )?,
        s,
    )?;
    require(
        compare(
            path(record, &["workflow_boundary", "scope"], s)?,
            path(record, &["workflow_scope", "scope"], s)?,
            s,
        )?,
        s,
    )?;
    require(
        number(
            path(record, &["workflow_boundary", "commit_sequence"], s)?,
            s,
        )? > 0,
        s,
    )?;
    require(
        text_equal(text(path(record, &["book"], s)?, s)?, "retail", s)?,
        s,
    )?;
    Ok(hash)
}
#[allow(clippy::too_many_arguments)]
fn claim_checked(
    claim: &Value,
    statement: &Value,
    record: &Value,
    sequence: i64,
    command: i64,
    accepted: i64,
    id: &[u8],
    key: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<()> {
    fields(
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
        s,
    )?;
    let standard = match text(path(statement, &["schema"], s)?, s)? {
        "ledger-billing-statement/6" => true,
        "ledger-billing-ad-hoc-statement-result/3" => false,
        _ => return Err(bad().into()),
    };
    require(
        text_equal(text(path(claim, &["schema"], s)?, s)?, CLAIM, s)?,
        s,
    )?;
    require(
        compare(
            path(claim, &["customer"], s)?,
            path(record, &["customer"], s)?,
            s,
        )?,
        s,
    )?;
    require(
        compare(
            path(statement, &["customer"], s)?,
            path(record, &["customer"], s)?,
            s,
        )?,
        s,
    )?;
    require(
        compare(
            path(claim, &["workflow_scope"], s)?,
            path(record, &["workflow_scope"], s)?,
            s,
        )?,
        s,
    )?;
    for name in ["store", "scope"] {
        s.work().visit(Visit::Field)?;
        require(
            compare(
                path(statement, &["workflow_boundary", name], s)?,
                path(record, &["workflow_scope", name], s)?,
                s,
            )?,
            s,
        )?;
    }
    require(
        compare(
            path(claim, &["statement_hash"], s)?,
            path(statement, &["statement_hash"], s)?,
            s,
        )?,
        s,
    )?;
    require(
        text_equal(
            text(path(claim, &["presentation_kind"], s)?, s)?,
            if standard {
                "standard-period"
            } else {
                "ad-hoc"
            },
            s,
        )?,
        s,
    )?;
    require(
        compare(
            path(claim, &["statement_id"], s)?,
            if standard {
                path(statement, &["statement_hash"], s)?
            } else {
                path(statement, &["statement_id"], s)?
            },
            s,
        )?,
        s,
    )?;
    let r = path(claim, &["manual_credit_ref"], s)?;
    ledgerlab_core::domain::workflow_statement::manual_credit::credit_ref(r, s)?;
    require(
        compare(
            path(r, &["id"], s)?,
            path(record, &["record", "record_id"], s)?,
            s,
        )?,
        s,
    )?;
    let encoded = s.canonical_representation(record, RepresentationKind::Record)?;
    let full = hashed(&[encoded.bytes()], s)?;
    hex_matches(b"sha256:", &full, text(path(r, &["digest"], s)?, s)?, s)?;
    let child = child_owned(claim, true, s)?;
    let actual = value(key, s)?;
    require(compare(actual.value(), child.value(), s)?, s)?;
    envelope(claim, sequence, command, accepted, id, key, CLAIM, s)?;
    let credit_at = ledgerlab_core::domain::Timestamp::parse_with_work(
        text(path(record, &["accepted_at"], s)?, s)?,
        s.work(),
    )?
    .micros();
    let credit_seq = number(path(record, &["record", "sequence"], s)?, s)?;
    let cutoff = number(path(statement, &["m5_high_water"], s)?, s)?;
    let credit_workflow = number(
        path(record, &["workflow_boundary", "commit_sequence"], s)?,
        s,
    )?;
    let statement_workflow = number(
        path(statement, &["workflow_boundary", "commit_sequence"], s)?,
        s,
    )?;
    s.work().arithmetic(
        Op::Compare,
        &[&credit_workflow.into(), &statement_workflow.into()],
    )?;
    require(credit_workflow <= statement_workflow, s)?;
    s.work().arithmetic(
        Op::Compare,
        &[
            &credit_at.into(),
            &accepted.into(),
            &credit_seq.into(),
            &cutoff.into(),
        ],
    )?;
    require(credit_at <= accepted && credit_seq <= cutoff, s)?;
    if standard {
        let start = ledgerlab_core::domain::Timestamp::parse_with_work(
            text(path(statement, &["start_utc"], s)?, s)?,
            s.work(),
        )?
        .micros();
        let end = ledgerlab_core::domain::Timestamp::parse_with_work(
            text(path(statement, &["end_utc"], s)?, s)?,
            s.work(),
        )?
        .micros();
        s.work().arithmetic(
            Op::Compare,
            &[&credit_at.into(), &start.into(), &end.into()],
        )?;
        require(start <= credit_at && credit_at < end, s)?;
        require(
            compare(
                path(claim, &["record", "accepted_at"], s)?,
                path(statement, &["close_acceptance_time"], s)?,
                s,
            )?,
            s,
        )?;
    }
    let view = data::presentation_view(statement, standard, s)?;
    let expected = ledgerlab_core::domain::workflow_statement::manual_credit::line(
        r,
        record,
        if standard { "standard" } else { "ad_hoc" },
        view.value(),
        path(record, &["workflow_scope"], s)?,
        s,
    )?;
    let mut count = 0usize;
    for line in path(statement, &["lines"], s)?.as_array().ok_or_else(bad)? {
        s.work().visit(Visit::Element)?;
        if let Some(reference) = optional_field(line, "manual_credit_ref", s)? {
            if compare(reference, r, s)? {
                require(compare(line, expected.value(), s)?, s)?;
                count = add(count, 1, s)?;
            }
        }
    }
    require(count == 1, s)
}
impl SqliteTx {
    fn manual_start(&mut self) -> Result<()> {
        if self.failed || self.workflow_proof_failed.load(Ordering::Acquire) {
            return Err(bad().into());
        }
        self.failed = true;
        Ok(())
    }
    fn manual_finish<T>(&mut self, result: Result<T>, s: &mut OriginalAuditSession) -> Result<T> {
        if result.is_ok() {
            s.check_cancelled()?;
            if self.workflow_proof_failed.load(Ordering::Acquire) || self.financial_io.is_some() {
                return Err(bad().into());
            }
            self.failed = false;
        }
        result
    }
    async fn manual_ready(&mut self, s: &mut OriginalAuditSession) -> Result<bool> {
        let payload = io::binding_payload(&[], &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let v: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        self.financial_finish_io(s).await?;
        s.work().arithmetic(
            Op::Compare,
            &[&v.into(), &19i64.into(), &20i64.into(), &21i64.into()],
        )?;
        match v {
            19 | 20 => Ok(false),
            21 => Ok(true),
            _ => Err(StoreError::BillingUpgradeRequired.into()),
        }
    }
    async fn manual_loaded(
        &mut self,
        seq: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<StoredManualCredit> {
        let payload = io::binding_payload(&[], &[], 1, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let (count,n):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(length(c.identity_key)+length(c.request_bytes)+length(c.response_bytes)+length(r.payload_bytes)),0) FROM billing_manual_credits p JOIN billing_m5_records r ON r.sequence=p.record_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE p.record_sequence=? AND length(c.identity_key) BETWEEN 1 AND 4096 AND length(c.request_bytes) BETWEEN 1 AND 262144 AND length(c.response_bytes) BETWEEN 1 AND 262144 AND length(r.payload_bytes) BETWEEN 1 AND 262144 AND length(c.request_sha256)=32 AND length(c.response_sha256)=32 AND length(r.content_sha256)=32 AND length(p.credit_digest)=71")
            .bind(seq).fetch_one(self.conn()).await?;
        self.financial_finish_io(s).await?;
        s.work()
            .arithmetic(Op::Compare, &[&count.into(), &1i64.into()])?;
        if count != 1 {
            return Err(bad().into());
        }
        let n = length(n, 3 * 262144 + 4096, s)?;
        let materialized = add(
            mul(n, 3, s)?,
            2 * std::mem::size_of::<CreditRow>() + 2048,
            s,
        )?;
        let payload = io::binding_payload(&[], &[], 1, materialized, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let row = load_row(self.conn(), seq).await?.ok_or_else(bad)?;
        let actual = [row.3.len(), row.4.len(), row.5.len(), row.6.len()]
            .into_iter()
            .try_fold(0, |a, b| add(a, b, s))?;
        s.work()
            .arithmetic(Op::Compare, &[&actual.into(), &n.into()])?;
        if actual != n {
            return Err(bad().into());
        }
        let req = s.parse_representation(&row.4, RepresentationKind::Record)?;
        ledgerlab_core::domain::workflow_statement::manual_credit::request(req.value(), s)?;
        let response = value(&row.5, s)?;
        let record = value(&row.6, s)?;
        require(row.11, s)?;
        let child = child_owned(req.value(), false, s)?;
        let child_bytes = s.canonical_representation(child.value(), RepresentationKind::Record)?;
        let content = credit_checked(
            req.value(),
            response.value(),
            record.value(),
            row.0,
            row.1,
            row.2,
            &row.3,
            child_bytes.bytes(),
            s,
        )?;
        let rq = hashed(&[b"bean-counter/m5/request/1\0", &row.4], s)?;
        let rs = hashed(&[b"bean-counter/m5/response/1\0", &row.5], s)?;
        s.work().scan_bytes(192)?;
        s.work().visit(Visit::MemberCompare)?;
        require(
            row.7.as_slice() == rq && row.8.as_slice() == rs && row.9.as_slice() == content,
            s,
        )?;
        require(
            text_equal(
                &row.10,
                text(path(response.value(), &["credit_ref", "digest"], s)?, s)?,
                s,
            )?,
            s,
        )?;
        let request = s.copy_representation(&row.4, RepresentationKind::Record)?;
        drop(req);
        drop(row);
        self.financial_finish_io(s).await?;
        Ok(StoredManualCredit {
            request,
            response,
            record,
            sequence: seq,
        })
    }
    pub(crate) async fn manual_credit_position(
        &mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<(i64, i64)> {
        self.manual_start()?;
        let result=super::super::workflow::until(self.workflow_deadline(),async{
            self.manual_ready(s).await?;
            let payload=io::binding_payload(&[],&[],0,0,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
            let result=sqlx::query_as("SELECT next_command_sequence,next_record_sequence FROM billing_m5_state WHERE singleton=1").fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;Ok(result)
        }).await.map_err(|_|self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    pub(crate) async fn manual_credit_lookup(
        &mut self,
        identity: &[u8],
        s: &mut OriginalAuditSession,
    ) -> Result<Option<StoredManualCredit>> {
        self.manual_start()?;
        let result=super::super::workflow::until(self.workflow_deadline(),async{
            if !self.manual_ready(s).await?{return Ok(None);}
            let expected=value(identity,s)?;
            let payload=io::binding_payload(&[identity],&[],0,0,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
            let (count,seq):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(r.sequence),0) FROM billing_m5_commands c LEFT JOIN billing_m5_records r ON r.command_sequence=c.command_sequence WHERE c.identity_key=?")
                .bind(identity).fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;
            s.work().arithmetic(Op::Compare,&[&count.into(),&0i64.into(),&1i64.into()])?;
            if count==0{return Ok(None);}
            if count!=1{return Err(bad().into());}
            let loaded=self.manual_loaded(seq,s).await?;
            let req=s.parse_representation(loaded.request.bytes(),RepresentationKind::Record)?;
            let framed=identity_owned(req.value(),s)?;
            if !compare(expected.value(),framed.value(),s)?{return Err(bad().into());}
            Ok(Some(loaded))
        }).await.map_err(|_|self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    async fn manual_exact_inner(
        &mut self,
        customer: &str,
        sc: &Value,
        r: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<StoredManualCredit>> {
        let (store, tenant, environment) = scope(sc, s)?;
        ledgerlab_core::domain::workflow_statement::manual_credit::credit_ref(r, s)?;
        let kind = text(path(r, &["kind"], s)?, s)?;
        let id = text(path(r, &["id"], s)?, s)?;
        let digest = text(path(r, &["digest"], s)?, s)?;
        let bound = [
            customer.as_bytes(),
            store.as_bytes(),
            tenant.as_bytes(),
            environment.as_bytes(),
            kind.as_bytes(),
            id.as_bytes(),
            digest.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let (count,seq):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(record_sequence),0) FROM billing_manual_credits WHERE customer=? AND logical_store_id=? AND tenant=? AND environment=? AND credit_kind=? AND credit_id=? AND credit_digest=?")
            .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
        self.financial_finish_io(s).await?;
        s.work()
            .arithmetic(Op::Compare, &[&count.into(), &0i64.into(), &1i64.into()])?;
        if count == 0 {
            return Ok(None);
        }
        if count != 1 {
            return Err(bad().into());
        }
        let loaded = self.manual_loaded(seq, s).await?;
        if !text_equal(
            text(path(loaded.record.value(), &["customer"], s)?, s)?,
            customer,
            s,
        )? || !compare(path(loaded.record.value(), &["workflow_scope"], s)?, sc, s)?
            || !compare(path(loaded.response.value(), &["credit_ref"], s)?, r, s)?
        {
            return Err(bad().into());
        }
        Ok(Some(loaded))
    }
    pub(crate) async fn manual_credit_exact(
        &mut self,
        customer: &str,
        sc: &Value,
        r: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<StoredManualCredit>> {
        self.manual_start()?;
        let result = super::super::workflow::until(self.workflow_deadline(), async {
            if !self.manual_ready(s).await? {
                return Ok(None);
            }
            self.manual_exact_inner(customer, sc, r, s).await
        })
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    async fn manual_claimed_inner(
        &mut self,
        customer: &str,
        sc: &Value,
        r: &Value,
        recovery: Option<(&str, i64)>,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        let (store, tenant, environment) = scope(sc, s)?;
        let kind = text(path(r, &["kind"], s)?, s)?;
        let id = text(path(r, &["id"], s)?, s)?;
        let digest = text(path(r, &["digest"], s)?, s)?;
        let bound = [
            customer.as_bytes(),
            store.as_bytes(),
            tenant.as_bytes(),
            environment.as_bytes(),
            kind.as_bytes(),
            id.as_bytes(),
            digest.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let (count,seq,n,bounded):(i64,i64,i64,bool)=sqlx::query_as("SELECT count(*),COALESCE(max(p.claim_sequence),0),COALESCE(max(length(r.payload_bytes)+length(c.response_bytes)+length(a.payload_bytes)+length(c.identity_key)),0),COALESCE(min(length(r.payload_bytes) BETWEEN 1 AND 262144 AND length(c.response_bytes) BETWEEN 1 AND 262144 AND length(a.payload_bytes) BETWEEN 1 AND 262144 AND length(c.identity_key) BETWEEN 1 AND 4096),0) FROM billing_manual_credit_membership p LEFT JOIN billing_m5_records r ON r.sequence=p.claim_sequence LEFT JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence LEFT JOIN billing_manual_credits cr ON cr.customer=p.customer AND cr.logical_store_id=p.logical_store_id AND cr.tenant=p.tenant AND cr.environment=p.environment AND cr.credit_kind=p.credit_kind AND cr.credit_id=p.credit_id AND cr.credit_digest=p.credit_digest LEFT JOIN billing_m5_records a ON a.sequence=cr.record_sequence WHERE p.customer=? AND p.logical_store_id=? AND p.tenant=? AND p.environment=? AND p.credit_kind=? AND p.credit_id=? AND p.credit_digest=?")
            .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
        self.financial_finish_io(s).await?;
        s.work()
            .arithmetic(Op::Compare, &[&count.into(), &0i64.into(), &1i64.into()])?;
        if count == 0 {
            return Ok(false);
        }
        if count != 1 {
            return Err(bad().into());
        }
        require(bounded, s)?;
        let n = length(n, 3 * 262144 + 4096, s)?;
        let materialized = add(mul(n, 3, s)?, 2 * std::mem::size_of::<ClaimRow>() + 2048, s)?;
        let payload = io::binding_payload(&[], &[], 1, materialized, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let row = load_claim(self.conn(), seq).await?.ok_or_else(bad)?;
        let claim = value(&row.1, s)?;
        let statement = value(&row.2, s)?;
        let credit = value(&row.3, s)?;
        require(row.5, s)?;
        let child = child_owned(claim.value(), true, s)?;
        let child_bytes = s.canonical_representation(child.value(), RepresentationKind::Record)?;
        claim_checked(
            claim.value(),
            statement.value(),
            credit.value(),
            row.0,
            row.6,
            row.7,
            &row.4,
            child_bytes.bytes(),
            s,
        )?;
        if !text_equal(
            text(path(claim.value(), &["customer"], s)?, s)?,
            customer,
            s,
        )? || !compare(path(claim.value(), &["workflow_scope"], s)?, sc, s)?
            || !compare(path(claim.value(), &["manual_credit_ref"], s)?, r, s)?
        {
            return Err(bad().into());
        }
        drop(row);
        self.financial_finish_io(s).await?;
        if let Some((own, cutoff)) = recovery {
            let hash = text(path(statement.value(), &["statement_hash"], s)?, s)?;
            s.work().visit(Visit::MemberCompare)?;
            s.work().scan_bytes(hash.len())?;
            s.work().scan_bytes(own.len())?;
            if hash == own {
                return Ok(false);
            }
            s.work()
                .arithmetic(Op::Compare, &[&seq.into(), &cutoff.into()])?;
            if seq > cutoff {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(crate) async fn manual_credit_claimed(
        &mut self,
        customer: &str,
        sc: &Value,
        r: &Value,
        recovery: Option<(&str, i64)>,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        self.manual_start()?;
        let result = super::super::workflow::until(self.workflow_deadline(), async {
            if !self.manual_ready(s).await? {
                return Ok(false);
            }
            if self.manual_exact_inner(customer, sc, r, s).await?.is_none() {
                return Err(bad().into());
            }
            self.manual_claimed_inner(customer, sc, r, recovery, s)
                .await
        })
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn manual_credit_selected(
        &mut self,
        customer: &str,
        sc: &Value,
        start: i64,
        end: i64,
        m5_cut: i64,
        recovery: Option<(&str, i64)>,
        s: &mut OriginalAuditSession,
    ) -> Result<ManualCreditSelection> {
        self.manual_start()?;
        let result=super::super::workflow::until(self.workflow_deadline(),async{
            let ready=self.manual_ready(s).await?;let (store,tenant,environment)=scope(sc, s)?;
            let bound=[customer.as_bytes(),store.as_bytes(),tenant.as_bytes(),environment.as_bytes()];
            let count=if ready{
                let payload=io::binding_payload(&bound,&[],3,0,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
                let count:i64=sqlx::query_scalar("SELECT count(*) FROM billing_manual_credits WHERE customer=? AND logical_store_id=? AND tenant=? AND environment=? AND accepted_at_us>=? AND accepted_at_us<? AND record_sequence<=?")
                    .bind(customer).bind(store).bind(tenant).bind(environment).bind(start).bind(end).bind(m5_cut).fetch_one(self.conn()).await?;
                self.financial_finish_io(s).await?;count
            }else{0};
            s.work().arithmetic(Op::Compare,&[&count.into(),&0i64.into(),&100000i64.into()])?;
            if !(0..=100000).contains(&count){return Err(StoreError::BillingHistoryLimit.into());}
            let count=count as usize;let control=mul(count,std::mem::size_of::<StoredManualCredit>(),s)?;
            let lease=s.reserve_external_payload(control)?;s.work().write_bytes(control)?;
            let mut rows=Vec::new();rows.try_reserve_exact(count).map_err(|_|bad())?;
            s.work().arithmetic(Op::Compare,&[&rows.capacity().into(),&count.into()])?;
            if rows.capacity()>count{return Err(bad().into());}
            let mut prior=0i64;
            for _ in 0..count{
                s.work().visit(Visit::Element)?;
                let payload=io::binding_payload(&bound,&[],4,0,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
                let seq:i64=sqlx::query_scalar("SELECT record_sequence FROM billing_manual_credits WHERE customer=? AND logical_store_id=? AND tenant=? AND environment=? AND accepted_at_us>=? AND accepted_at_us<? AND record_sequence<=? AND record_sequence>? ORDER BY record_sequence LIMIT 1")
                    .bind(customer).bind(store).bind(tenant).bind(environment).bind(start).bind(end).bind(m5_cut).bind(prior).fetch_one(self.conn()).await?;
                self.financial_finish_io(s).await?;
                let row=self.manual_loaded(seq,s).await?;prior=seq;
                if !self.manual_claimed_inner(customer,sc,path(row.response.value(), &["credit_ref"], s)?,recovery,s).await?{rows.push(row);}
            }
            Ok(ManualCreditSelection{rows,_controls:lease})
        }).await.map_err(|_|self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    pub(crate) async fn manual_credit_append(
        &mut self,
        prepared: &crate::billing::manual_credit::PreparedManualCredit<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.manual_start()?;
        let result=super::super::workflow::until(self.workflow_deadline(),async{
            if !self.manual_ready(s).await?{return Err(StoreError::BillingUpgradeRequired.into());}
            let (command,record)=prepared.parts(self.workflow_proof_failure_handle().identity())?;
            let request=s.parse_representation(command.request,RepresentationKind::Record)?;
            ledgerlab_core::domain::workflow_statement::manual_credit::request(request.value(),s)?;
            let response=value(command.response,s)?;
            require(text_equal(command.family, REQUEST, s)? && text_equal(command.domain, "customer-admin", s)? && command.source.is_none(), s)?;
            let customer=command.customer.ok_or_else(bad)?;
            require(text_equal(customer, text(path(request.value(), &["customer"], s)?, s)?, s)?, s)?;
            s.work().arithmetic(Op::Compare, &[&command.children.len().into(), &1usize.into(), &command.request.len().into(), &command.response.len().into(), &command.identity_key.len().into(), &262144usize.into(), &4096usize.into()])?;
            require(command.children.len()==1 && command.request.len()<=262144 && command.response.len()<=262144 && command.identity_key.len()<=4096, s)?;
            s.work().visit(Visit::Element)?;
            let child=&command.children[0];
            require(text_equal(child.family, RECORD, s)? && child.source.is_none(), s)?;
            require(text_equal(child.customer.ok_or_else(bad)?, customer, s)?, s)?;
            s.work().arithmetic(Op::Compare, &[&child.payload.len().into(), &262144usize.into(), &child.child_key.len().into(), &4096usize.into()])?;
            require(child.payload.len()<=262144 && child.child_key.len()<=4096, s)?;
            let data=value(child.payload,s)?;if !compare(data.value(),record,s)?{return Err(bad().into());}
            let id=value(command.identity_key,s)?;let key=value(child.child_key,s)?;
            let expected_id=identity_owned(request.value(),s)?;let expected_key=child_owned(request.value(),false,s)?;
            if !compare(id.value(),expected_id.value(),s)?||!compare(key.value(),expected_key.value(),s)?{return Err(bad().into());}
            let seq=number(path(record, &["record", "sequence"], s)?, s)?;let cmd=number(path(record, &["record", "command_sequence"], s)?, s)?;
            credit_checked(request.value(),response.value(),record,cmd,seq,command.accepted_at_us,command.identity_key,child.child_key,s)?;
            let bound=[command.family.as_bytes(),command.domain.as_bytes(),command.customer.unwrap_or("").as_bytes(),command.identity_key,command.request,command.response,child.family.as_bytes(),child.child_key,child.payload];
            let total=bound.iter().try_fold(0,|a,b|add(a,b.len(),s))?;let materialized=add(mul(total,8,s)?,2048,s)?;
            let payload=io::binding_payload(&bound,&bound,16,materialized,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
            m5::append(self.conn(),command).await?;
            self.financial_finish_io(s).await?;
            let customer=text(path(record, &["customer"], s)?, s)?;let (store,tenant,environment)=scope(path(record, &["workflow_scope"], s)?, s)?;
            let adjustment=text(path(record, &["adjustment_id"], s)?, s)?;let r=path(response.value(), &["credit_ref"], s)?;let kind=text(path(r, &["kind"], s)?, s)?;let id=text(path(r, &["id"], s)?, s)?;let digest=text(path(r, &["digest"], s)?, s)?;
            let bound=[customer.as_bytes(),store.as_bytes(),tenant.as_bytes(),environment.as_bytes(),adjustment.as_bytes(),kind.as_bytes(),id.as_bytes(),digest.as_bytes()];
            let payload=io::binding_payload(&bound,&bound,4,0,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
            sqlx::query("INSERT INTO billing_manual_credits(customer,logical_store_id,tenant,environment,adjustment_id,record_sequence,credit_kind,credit_id,credit_digest,accepted_at_us) VALUES(?,?,?,?,?,?,?,?,?,?)")
                .bind(customer).bind(store).bind(tenant).bind(environment).bind(adjustment).bind(seq).bind(kind).bind(id).bind(digest).bind(command.accepted_at_us).execute(self.conn()).await?;
            self.financial_finish_io(s).await?;
            let payload=io::binding_payload(&[],&[],8,128,s)?;self.financial_reserve_io(s,payload)?;s.work().visit(Visit::RefLookup)?;
            m5::append_boundary(self.conn()).await?;self.financial_finish_io(s).await?;Ok(())
        }).await.map_err(|_|self.workflow_timeout_error())?;
        self.manual_finish(result, s)
    }
    /// Called only inside the already-poisoned prepared presentation append,
    /// after the unchanged M5 child insertion. This creates no source authority.
    pub(crate) async fn manual_credit_claim_append(
        &mut self,
        claim: &Value,
        statement: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if !self.failed || self.workflow_proof_failed.load(Ordering::Acquire) {
            return Err(bad().into());
        }
        let customer = text(path(claim, &["customer"], s)?, s)?;
        let sc = path(claim, &["workflow_scope"], s)?;
        let r = path(claim, &["manual_credit_ref"], s)?;
        let credit = self
            .manual_exact_inner(customer, sc, r, s)
            .await?
            .ok_or_else(bad)?;
        let encoded = s.canonical_representation(claim, RepresentationKind::Record)?;
        let statement_encoded =
            s.canonical_representation(statement, RepresentationKind::Record)?;
        let seq = number(path(claim, &["record", "sequence"], s)?, s)?;
        let bound = [encoded.bytes(), statement_encoded.bytes()];
        let payload = io::binding_payload(
            &bound,
            &[],
            1,
            3 * 8192 + 2 * std::mem::size_of::<(Vec<u8>, Vec<u8>, i64, i64)>(),
            s,
        )?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        let row:Option<(Vec<u8>,i64,i64)>=sqlx::query_as("SELECT c.identity_key,c.command_sequence,c.accepted_at_us FROM billing_m5_records r JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE r.sequence=? AND r.family='ledger-billing-manual-credit-presentation-claim/1' AND r.payload_bytes=? AND c.response_bytes=? AND c.family IN ('ledger-billing-period-close/3','ledger-billing-ad-hoc-statement/3') AND length(c.identity_key) BETWEEN 1 AND 4096")
            .bind(seq).bind(encoded.bytes()).bind(statement_encoded.bytes()).fetch_optional(self.conn()).await?;
        let (id, command, accepted) = row.ok_or_else(bad)?;
        let child = child_owned(claim, true, s)?;
        let key = s.canonical_representation(child.value(), RepresentationKind::Record)?;
        claim_checked(
            claim,
            statement,
            credit.record.value(),
            seq,
            command,
            accepted,
            &id,
            key.bytes(),
            s,
        )?;
        drop(key);
        drop(id);
        self.financial_finish_io(s).await?;
        let (store, tenant, environment) = scope(sc, s)?;
        let kind = text(path(r, &["kind"], s)?, s)?;
        let id = text(path(r, &["id"], s)?, s)?;
        let digest = text(path(r, &["digest"], s)?, s)?;
        let presentation = text(path(claim, &["presentation_kind"], s)?, s)?;
        let statement_id = text(path(claim, &["statement_id"], s)?, s)?;
        let hash = text(path(claim, &["statement_hash"], s)?, s)?;
        let bound = [
            customer.as_bytes(),
            store.as_bytes(),
            tenant.as_bytes(),
            environment.as_bytes(),
            kind.as_bytes(),
            id.as_bytes(),
            digest.as_bytes(),
            presentation.as_bytes(),
            statement_id.as_bytes(),
            hash.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &bound, 3, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.work().visit(Visit::RefLookup)?;
        sqlx::query("INSERT INTO billing_manual_credit_membership(customer,logical_store_id,tenant,environment,credit_kind,credit_id,credit_digest,claim_sequence,presentation_kind,statement_id,statement_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).bind(seq).bind(presentation).bind(statement_id).bind(hash).execute(self.conn()).await?;
        self.financial_finish_io(s).await?;
        Ok(())
    }
}
