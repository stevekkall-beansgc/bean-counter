//! Locked projection locators only. Every returned reference/revision must be
//! authenticated against the chronological prefix and immutable head body.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::CostLineKey;

pub(crate) enum HeadKey<'a> {
    Decision(&'a str),
    Family {
        target_kind: &'a str,
        target_id: &'a str,
        target_digest: &'a str,
        slot: &'a str,
    },
    Chain {
        customer: &'a str,
        chain: &'a str,
    },
    Capacity(&'a str),
    Invocation(&'a str),
    Cost(&'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef),
    CostConsumption(&'a ledgerlab_core::policy::workflow::transition_constructor::ExactRef),
    CostOwner(&'a CostLineKey),
}
impl<'a> HeadKey<'a> {
    fn parts(&self) -> Result<(&'static str, [&'a str; 4]), FinancialReadError> {
        Ok(match self {
            Self::Decision(id) => ("decision", [id, "", "", ""]),
            Self::Family {
                target_kind,
                target_id,
                target_digest,
                slot,
            } => ("family", [target_kind, target_id, target_digest, slot]),
            Self::Chain { customer, chain } => ("chain", [customer, chain, "", ""]),
            Self::Capacity(id) => ("capacity", [id, "", "", ""]),
            Self::Invocation(id) => ("invocation", [id, "", "", ""]),
            Self::Cost(r) => ("cost", [&r.kind, &r.id, &r.digest, ""]),
            Self::CostConsumption(r) => ("cost_consumption", [&r.kind, &r.id, &r.digest, ""]),
            Self::CostOwner(_) => return Err(invalid()),
        })
    }
}
/// No ordinary String/Ref clones, no authority token and no naked row body.
/// These artifacts remain in the attempt arena until the caller drops them.
pub(crate) struct HeadLocator {
    pub revision: AuditedBytes,
    pub kind: AuditedBytes,
    pub id: AuditedBytes,
    pub digest: AuditedBytes,
}
const LENGTHS: &str = "SELECT octet_length(revision),octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=?";
const VALUES: &str = "SELECT revision,record_kind,record_id,record_digest FROM workflow_financial_heads WHERE logical_store_id=? AND tenant=? AND environment=? AND head_kind=? AND key_a=? AND key_b=? AND key_c=? AND key_d=?";
const OWNER_LENGTHS: &str = "SELECT octet_length(revision),octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_supplier_cost_owner_heads WHERE cost_authority_id=? AND provider_namespace=? AND provider_account_id=? AND provider_environment=? AND original_document_id=? AND original_line_id=?";
const OWNER_VALUES: &str = "SELECT revision,record_kind,record_id,record_digest FROM workflow_supplier_cost_owner_heads WHERE cost_authority_id=? AND provider_namespace=? AND provider_account_id=? AND provider_environment=? AND original_document_id=? AND original_line_id=?";
fn bind<'q>(
    sql: &'static str,
    bindings: &[&'q str],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    let mut query = sqlx::query(sql);
    for binding in bindings {
        query = query.bind(*binding);
    }
    query
}
fn locator_text(
    value: &str,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<(), FinancialReadError> {
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    if resource_compare(session, value.len() as i64, maximum as i64)?.is_gt() {
        return Err(invalid());
    }
    // This is only the already-declared key representation bound. Actual
    // subject, original identity and currentness are not established here.
    session.semantic_work().scan_bytes(value.len())?;
    for character in value.chars() {
        session.semantic_work().visit(ValidationVisit::Constraint)?;
        if character.is_control() {
            return Err(invalid());
        }
    }
    Ok(())
}
fn preflight_fields(
    lengths: &sqlx::sqlite::SqliteRow,
    session: &mut OriginalAuditSession,
) -> Result<(), FinancialReadError> {
    for (position, maximum) in [(0usize, 20i64), (1, 48), (2, 104), (3, 71)] {
        session.semantic_work().visit(ValidationVisit::Field)?;
        let n: i64 = lengths.try_get(position)?;
        // octet_length reports the stored encoding's byte size without
        // overflow materialization (pinned SQLite BYTELEN direct-column
        // opcode). UTF16 ASCII may occupy twice its eventual UTF8 size.
        // This is a prefetch allocation bound, NOT a larger Ref language.
        session.semantic_work().arithmetic(
            ArithmeticOperation::Multiply,
            &[&maximum.into(), &2i64.into()],
        )?;
        let stored_maximum = maximum * 2;
        if !resource_compare(session, n, 0)?.is_gt()
            || resource_compare(session, n, stored_maximum)?.is_gt()
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn utf8_fields(
    row: &sqlx::sqlite::SqliteRow,
    session: &mut OriginalAuditSession,
) -> Result<(), FinancialReadError> {
    for (position, maximum) in [(0usize, 20i64), (1, 48), (2, 104), (3, 71)] {
        session.semantic_work().visit(ValidationVisit::Field)?;
        let text = row.try_get::<&str, _>(position)?;
        if !resource_compare(session, text.len() as i64, 0)?.is_gt()
            || resource_compare(session, text.len() as i64, maximum)?.is_gt()
        {
            return Err(invalid());
        }
    }
    Ok(())
}
impl SqliteTx {
    pub(crate) async fn workflow_financial_head(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        let was_failed = self.failed;
        if was_failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned financial head observation").into());
        }
        self.failed = true;
        let result = timeout_at(
            self.deadline,
            self.financial_head_inner(store, scope, key, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            // An Ok SQL result cannot erase a failed/cancelled child, a caught
            // proof error or a worker payload still awaiting finalization.
            // The executing guard uses this SAME original session lifecycle.
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("financial completion retains poisoned work").into(),
                );
            }
            self.failed = was_failed;
        }
        result
    }
    pub(super) async fn financial_head_during_append(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        if !self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("financial head read outside active append").into());
        }
        self.financial_head_inner(store, scope, key, session).await
    }
    async fn financial_head_inner(
        &mut self,
        store: &str,
        scope: &[String; 2],
        key: HeadKey<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        locator_text(store, 128, session)?;
        locator_text(&scope[0], 128, session)?;
        locator_text(&scope[1], 128, session)?;
        session.check_cancelled()?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(session, version, 13)?.is_eq()
            && !resource_compare(session, version, 14)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        if let HeadKey::CostOwner(line) = key {
            if !resource_compare(session, version, 14)?.is_eq() {
                return Err(StoreError::BillingUpgradeRequired.into());
            }
            let account = line.provider_account_identity();
            let bindings = [
                line.cost_authority_id(),
                account.provider_namespace(),
                account.provider_account_id(),
                account.environment(),
                line.original_document_id(),
                line.original_line_id(),
            ];
            for value in bindings {
                locator_text(value, 128, session)?;
            }
            return self
                .read_financial_head_projection(OWNER_LENGTHS, OWNER_VALUES, bindings, session)
                .await;
        }
        let (kind, parts) = key.parts()?;
        for part in parts {
            locator_text(part, 128, session)?;
        }
        self.read_financial_head_projection(
            LENGTHS,
            VALUES,
            [
                store, &scope[0], &scope[1], kind, parts[0], parts[1], parts[2], parts[3],
            ],
            session,
        )
        .await
    }
    async fn read_financial_head_projection<const N: usize>(
        &mut self,
        length_sql: &'static str,
        value_sql: &'static str,
        bindings: [&str; N],
        session: &mut OriginalAuditSession,
    ) -> Result<Option<HeadLocator>, FinancialReadError> {
        let bytes = bindings.map(str::as_bytes);
        let payload = io::binding_payload(&bytes, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let lengths = bind(length_sql, &bindings)
            .fetch_optional(self.conn())
            .await?;
        let Some(lengths) = lengths else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        preflight_fields(&lengths, session)?;
        drop(lengths);
        self.financial_finish_io(session).await?;
        // Finalize the probe before fetching text. For a UTF16 database, the
        // stored bound is2M. Pinned MemTranslate requests2*n+1 (at most
        // 4M+1), while the statement and duplicated row can retain2M each:
        // 8M+1 total requested payload, including conversion overlap. The
        // per-cell terminators/headers fit the fixed I/O metadata reservation.
        // The complete SQLx/SQLite copy-lifetime certificate remains mandatory
        // at the caller; these fixed field bounds are not a process/RSS claim.
        let payload = io::binding_payload(&bytes, &[], 0, 8 * (20 + 48 + 104 + 71), session)?;
        self.financial_reserve_io(session, payload)?;
        let row = bind(value_sql, &bindings)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        utf8_fields(&row, session)?;
        let revision = session.copy_representation(
            row.try_get::<&str, _>(0)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let record_kind = session.copy_representation(
            row.try_get::<&str, _>(1)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let id = session.copy_representation(
            row.try_get::<&str, _>(2)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        let digest = session.copy_representation(
            row.try_get::<&str, _>(3)?.as_bytes(),
            ledgerlab_core::validation_work::RepresentationKind::Record,
        )?;
        // Returned artifacts now have their own reservations. The source row
        // is destroyed before cache finalization releases its worker lease.
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(HeadLocator {
            revision,
            kind: record_kind,
            id,
            digest,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Connection, Executor};
    #[tokio::test]
    async fn direct_octet_probes_bound_unicode_nul_and_utf16_before_owned_copy() {
        for encoding in ["UTF-8", "UTF-16le"] {
            let mut c = sqlx::SqliteConnection::connect("sqlite::memory:")
                .await
                .unwrap();
            if encoding == "UTF-16le" {
                c.execute("PRAGMA encoding='UTF-16le'").await.unwrap();
            }
            c.execute("CREATE TABLE workflow_financial_heads(logical_store_id TEXT,tenant TEXT,environment TEXT,head_kind TEXT,key_a TEXT,key_b TEXT,key_c TEXT,key_d TEXT,revision TEXT,record_kind TEXT,record_id TEXT,record_digest TEXT); CREATE TABLE workflow_supplier_cost_owner_heads(cost_authority_id TEXT,provider_namespace TEXT,provider_account_id TEXT,provider_environment TEXT,original_document_id TEXT,original_line_id TEXT,revision TEXT,record_kind TEXT,record_id TEXT,record_digest TEXT)").await.unwrap();
            for (probe, values, bindings, table) in [
                (
                    LENGTHS,
                    VALUES,
                    &[
                        "store",
                        "tenant",
                        "environment",
                        "decision",
                        "key",
                        "",
                        "",
                        "",
                    ][..],
                    "workflow_financial_heads",
                ),
                (
                    OWNER_LENGTHS,
                    OWNER_VALUES,
                    &[
                        "authority",
                        "provider",
                        "account",
                        "environment",
                        "document",
                        "line",
                    ][..],
                    "workflow_supplier_cost_owner_heads",
                ),
            ] {
                for text in [
                    "workflow-cost".to_owned(),
                    "é".repeat(29),
                    format!("x\0{}", "z".repeat(1024)),
                ] {
                    let sql = if table == "workflow_financial_heads" {
                        "INSERT INTO workflow_financial_heads VALUES('store','tenant','environment','decision','key','','','','1',?,'id','digest')"
                    } else {
                        "INSERT INTO workflow_supplier_cost_owner_heads VALUES('authority','provider','account','environment','document','line','1',?,'id','digest')"
                    };
                    sqlx::query(sql).bind(&text).execute(&mut c).await.unwrap();
                    let arena = ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
                    let mut session =
                        OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
                    let lengths = bind(probe, bindings).fetch_one(&mut c).await.unwrap();
                    let expected = text.len() * if encoding == "UTF-8" { 1 } else { 2 };
                    // All tested non-ASCII characters have one UTF16 unit and
                    // two UTF8 bytes, so derive exact stored bytes independently.
                    let expected = if encoding == "UTF-16le" {
                        text.encode_utf16().count() * 2
                    } else {
                        expected
                    };
                    assert_eq!(lengths.try_get::<i64, _>(1).unwrap(), expected as i64);
                    if text.contains('\0') {
                        assert!(preflight_fields(&lengths, &mut session).is_err());
                    } else {
                        preflight_fields(&lengths, &mut session).unwrap();
                        let row = bind(values, bindings).fetch_one(&mut c).await.unwrap();
                        assert_eq!(
                            utf8_fields(&row, &mut session).is_ok(),
                            text == "workflow-cost"
                        );
                    }
                    let delete = if table == "workflow_financial_heads" {
                        "DELETE FROM workflow_financial_heads"
                    } else {
                        "DELETE FROM workflow_supplier_cost_owner_heads"
                    };
                    c.execute(delete).await.unwrap();
                }
            }
        }
    }
}
