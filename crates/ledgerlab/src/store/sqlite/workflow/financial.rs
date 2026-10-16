//! Scoped immutable financial candidate bytes. This is neither an origin
//! verifier nor an admission/append path. Administrative readers stay closed.
use super::SqliteTx;
use crate::store::errors::StoreError;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    validation_work::{ArithmeticOperation, BigInt, OriginalAuditSession, ValidationVisit},
};
use sqlx::Row;
use tokio::time::timeout_at;
mod effects;
mod append;
pub(crate) use append::{FinancialAppend, FinancialDocument, FinancialJournal};

pub(crate) enum FinancialReadError {
    Store(StoreError),
    Resource(ledgerlab_core::Error),
}
impl From<StoreError> for FinancialReadError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<sqlx::Error> for FinancialReadError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}
impl From<ledgerlab_core::Error> for FinancialReadError {
    fn from(e: ledgerlab_core::Error) -> Self {
        Self::Resource(e)
    }
}
/// Raw scoped frame. Owning resource artifacts are not origin capabilities.
/// No parsed graph or indexed member list is copied into this structure.
pub(crate) struct FinancialFrame {
    pub kind: &'static str,
    pub operation: AuditedBytes,
    pub manifest: AuditedBytes,
    pub receipt: AuditedBytes,
    pub operation_id: AuditedBytes,
    pub operation_digest: AuditedBytes,
    pub manifest_id: AuditedBytes,
    pub manifest_digest: AuditedBytes,
    pub receipt_id: AuditedBytes,
    pub receipt_digest: AuditedBytes,
    pub request_hash: AuditedBytes,
    pub sequence: i64,
    pub accepted_at_us: i64,
    pub member_count: i64,
}
fn resource_compare(
    session: &mut OriginalAuditSession,
    left: i64,
    right: i64,
) -> Result<std::cmp::Ordering, FinancialReadError> {
    session.work().arithmetic(
        ArithmeticOperation::Compare,
        &[&BigInt::from(left), &BigInt::from(right)],
    )?;
    Ok(left.cmp(&right))
}
fn resource_add(
    session: &mut OriginalAuditSession,
    left: i64,
    right: i64,
) -> Result<i64, FinancialReadError> {
    session.work().arithmetic(
        ArithmeticOperation::Add,
        &[&BigInt::from(left), &BigInt::from(right)],
    )?;
    left.checked_add(right)
        .ok_or_else(|| StoreError::Integrity("financial read offset overflow").into())
}
fn invalid() -> FinancialReadError {
    StoreError::Integrity("invalid financial candidate frame").into()
}

impl SqliteTx {
    /// Preadmitted chunked BLOB read for the actual original-audit caller. SQL
    /// row storage is the separate bounded I/O lane; destination bytes remain
    /// owned by the caller's SINGLE attempt arena across subsequent awaits.
    pub(crate) async fn workflow_financial_frame(
        &mut self,
        identity: &[u8],
        store: &str,
        scope: &[String; 2],
        session: &mut OriginalAuditSession,
    ) -> Result<Option<FinancialFrame>, FinancialReadError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            session.work().visit(ValidationVisit::RefLookup)?;
            // No BLOB or potentially unbounded text is selected by this probe.
            let sizes=sqlx::query("SELECT length(CAST(operation_kind AS BLOB)) AS kl,length(operation_bytes) AS ob,length(manifest_bytes) AS mb,length(receipt_bytes) AS rb,length(CAST(operation_record_id AS BLOB)) AS oi,length(CAST(operation_digest AS BLOB)) AS od,length(CAST(manifest_id AS BLOB)) AS mi,length(CAST(manifest_digest AS BLOB)) AS md,length(CAST(receipt_id AS BLOB)) AS ri,length(CAST(receipt_digest AS BLOB)) AS rd,length(CAST(normalized_request_hash AS BLOB)) AS rh,publication_id IS NULL AS pn,plan_id IS NULL AS qn FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?")
                .bind(identity).bind(store).bind(&scope[0]).bind(&scope[1]).fetch_optional(self.conn()).await?;
            let Some(sizes)=sizes else { return Ok(None) };
            for (field,maximum) in [("kl",32),("ob",8388608),("mb",4194304),("rb",1048576),("oi",68),("od",71),("mi",74),("md",71),("ri",73),("rd",71),("rh",71)] {
                let n=sizes.try_get::<i64,_>(field)?;
                if !resource_compare(session,n,0)?.is_gt() || resource_compare(session,n,maximum)?.is_gt() { return Err(invalid()); }
            }
            for field in ["pn","qn"] { if !resource_compare(session,sizes.try_get::<i64,_>(field)?,1)?.is_eq() {return Err(invalid());} }
            let ob=sizes.try_get::<i64,_>("ob")?;
            let mb=sizes.try_get::<i64,_>("mb")?;
            let rb=sizes.try_get::<i64,_>("rb")?;
            drop(sizes);
            session.work().visit(ValidationVisit::RefLookup)?;
            let row=sqlx::query("SELECT operation_kind,operation_record_id,operation_digest,manifest_id,manifest_digest,receipt_id,receipt_digest,normalized_request_hash,commit_sequence,accepted_at_us FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?")
                .bind(identity).bind(store).bind(&scope[0]).bind(&scope[1]).fetch_one(self.conn()).await?;
            session.work().visit(ValidationVisit::Constraint)?;
            let kind=match row.try_get::<&str,_>("operation_kind")? {
                "work.accept"=>"work.accept", "work.reverse"=>"work.reverse", "quantity.correct"=>"quantity.correct", "outcome.submit"=>"outcome.submit", "outcome.correct"=>"outcome.correct", "outcome.close"=>"outcome.close", "link.assert"=>"link.assert", _=>return Err(invalid()),
            };
            let sequence=row.try_get::<i64,_>("commit_sequence")?;
            let accepted_at_us=row.try_get::<i64,_>("accepted_at_us")?;
            let copy=|field:&str, session:&mut OriginalAuditSession| -> Result<AuditedBytes,FinancialReadError> {
                session.work().visit(ValidationVisit::Field)?;
                let raw=row.try_get::<&str,_>(field)?.as_bytes();
                let (arena,work)=session.parts();
                Ok(arena.copy_bytes(raw,work)?)
            };
            let operation_id=copy("operation_record_id",session)?;
            let operation_digest=copy("operation_digest",session)?;
            let manifest_id=copy("manifest_id",session)?;
            let manifest_digest=copy("manifest_digest",session)?;
            let receipt_id=copy("receipt_id",session)?;
            let receipt_digest=copy("receipt_digest",session)?;
            let request_hash=copy("normalized_request_hash",session)?;
            drop(row);
            session.work().visit(ValidationVisit::RefLookup)?;
            let member_count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_manifest_members WHERE identity_key=?")
                .bind(identity).fetch_one(self.conn()).await?;
            if !resource_compare(session,member_count,0)?.is_gt() || resource_compare(session,member_count,4096)?.is_gt() {return Err(invalid());}
            let operation=self.financial_blob(identity,store,scope,0,ob,session).await?;
            let manifest=self.financial_blob(identity,store,scope,1,mb,session).await?;
            let receipt=self.financial_blob(identity,store,scope,2,rb,session).await?;
            Ok(Some(FinancialFrame {kind,operation,manifest,receipt,operation_id,operation_digest,manifest_id,manifest_digest,receipt_id,receipt_digest,request_hash,sequence,accepted_at_us,member_count}))
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
    async fn financial_blob(
        &mut self,
        identity: &[u8],
        store: &str,
        scope: &[String; 2],
        column: u8,
        length: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<AuditedBytes, FinancialReadError> {
        let mut output = {
            let (arena, work) = session.parts();
            arena.byte_builder(length as usize, work)?
        };
        let sql = match column {
            0 => {
                "SELECT substr(operation_bytes,?,?) AS chunk FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?"
            }
            1 => {
                "SELECT substr(manifest_bytes,?,?) AS chunk FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?"
            }
            2 => {
                "SELECT substr(receipt_bytes,?,?) AS chunk FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?"
            }
            _ => return Err(invalid()),
        };
        let mut offset = 0i64;
        while resource_compare(session, offset, length)?.is_lt() {
            let start = resource_add(session, offset, 1)?;
            session.work().visit(ValidationVisit::RefLookup)?;
            // One row per query; no fetch_all of multi-MiB bodies or refs.
            let row = sqlx::query(sql)
                .bind(start)
                .bind(262144i64)
                .bind(identity)
                .bind(store)
                .bind(&scope[0])
                .bind(&scope[1])
                .fetch_one(self.conn())
                .await?;
            let chunk = row.try_get::<&[u8], _>("chunk")?;
            if !resource_compare(session, chunk.len() as i64, 0)?.is_gt()
                || resource_compare(session, chunk.len() as i64, 262144)?.is_gt()
            {
                return Err(invalid());
            }
            output.append(chunk, session.work())?;
            offset = resource_add(session, offset, chunk.len() as i64)?;
        }
        if !resource_compare(session, offset, length)?.is_eq() {
            return Err(invalid());
        }
        Ok(output.finish(session.work())?)
    }
    pub(crate) async fn workflow_financial_member_present(
        &mut self,
        identity: &[u8],
        kind: &str,
        id: &str,
        digest: &str,
        session: &mut OriginalAuditSession,
    ) -> Result<bool, FinancialReadError> {
        let prior = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline, async {
            session.work().visit(ValidationVisit::RefLookup)?;
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_manifest_members WHERE identity_key=? AND kind=? AND id=? AND digest=?")
                .bind(identity).bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
            if resource_compare(session,count,0)?.is_eq() {Ok(false)}
            else if resource_compare(session,count,1)?.is_eq() {Ok(true)}
            else {Err(invalid())}
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            self.failed = prior;
        }
        result
    }
}
