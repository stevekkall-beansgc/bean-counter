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
mod chronology;
mod document;
mod effects;
mod heads;
mod source;
pub(crate) use source::SourceHeadLocator;
mod app;
mod party;
pub(crate) use app::AppClaimLocator;
pub(crate) use party::{PartyHeadEffect, PartyHeadKey};
mod supplier;
pub(crate) use chronology::OriginalLocator;
pub(crate) use document::RetainedDocument;
pub(crate) use heads::{HeadKey, HeadLocator};
mod agreement;
mod append;
pub(crate) use agreement::AppAcceptanceAppend;
pub(crate) mod io;
pub(crate) use append::{FinancialAppend, FinancialDocument, FinancialJournal};

#[derive(Debug)]
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
pub(crate) struct OriginalFrame {
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
    pub publication_id: Option<AuditedBytes>,
    pub plan_id: Option<AuditedBytes>,
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
    pub(crate) async fn workflow_original_frame(
        &mut self,
        identity: &[u8],
        store: &str,
        scope: &[String; 2],
        session: &mut OriginalAuditSession,
    ) -> Result<Option<OriginalFrame>, FinancialReadError> {
        let prior = self.failed;
        self.failed = true;
        // substr may materialize its source SQLite value; a 256KiB returned
        // chunk is NOT proof that the VM holds only 256KiB. Reserve the existing
        // maximum operation field plus both result/owned-row chunk copies.
        let mut argument_bytes = identity.len();
        for text in [store, scope[0].as_str(), scope[1].as_str()] {
            session.semantic_work().arithmetic(
                ArithmeticOperation::Add,
                &[&argument_bytes.into(), &text.len().into()],
            )?;
            argument_bytes = argument_bytes.checked_add(text.len()).ok_or_else(invalid)?;
        }
        session.semantic_work().arithmetic(
            ArithmeticOperation::Multiply,
            &[&argument_bytes.into(), &3usize.into()],
        )?;
        let arguments = argument_bytes.checked_mul(3).ok_or_else(invalid)?;
        session.semantic_work().arithmetic(
            ArithmeticOperation::Add,
            &[&arguments.into(), &(8388608usize + 2 * 262144).into()],
        )?;
        self.financial_reserve_io(
            session,
            arguments
                .checked_add(8388608 + 2 * 262144)
                .ok_or_else(invalid)?,
        )?;
        let result=timeout_at(self.deadline, async {
            self.workflow_schema().await?;
            session.work().visit(ValidationVisit::RefLookup)?;
            // No BLOB or potentially unbounded text is selected by this probe.
            let sizes=sqlx::query("SELECT octet_length(operation_kind) AS kl,length(operation_bytes) AS ob,length(manifest_bytes) AS mb,length(receipt_bytes) AS rb,octet_length(operation_record_id) AS oi,octet_length(operation_digest) AS od,octet_length(manifest_id) AS mi,octet_length(manifest_digest) AS md,octet_length(receipt_id) AS ri,octet_length(receipt_digest) AS rd,octet_length(normalized_request_hash) AS rh,publication_id IS NULL AS pn,plan_id IS NULL AS qn,coalesce(octet_length(publication_id),0) AS pl,coalesce(octet_length(plan_id),0) AS ql FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?")
                .bind(identity).bind(store).bind(&scope[0]).bind(&scope[1]).fetch_optional(self.conn()).await?;
            let Some(sizes)=sizes else { return Ok(None) };
            for (field,maximum) in [("kl",32),("ob",8388608),("mb",4194304),("rb",1048576),("oi",68),("od",71),("mi",74),("md",71),("ri",73),("rd",71),("rh",71)] {
                let n=sizes.try_get::<i64,_>(field)?;
                let bound=if matches!(field,"ob"|"mb"|"rb") {maximum} else {session.semantic_work().arithmetic(ArithmeticOperation::Multiply,&[&maximum.into(),&2i64.into()])?;maximum*2};
                if !resource_compare(session,n,0)?.is_gt() || resource_compare(session,n,bound)?.is_gt() { return Err(invalid()); }
            }
            let publication_none=sizes.try_get::<i64,_>("pn")?;
            let plan_none=sizes.try_get::<i64,_>("qn")?;
            let publication_length=sizes.try_get::<i64,_>("pl")?;
            let plan_length=sizes.try_get::<i64,_>("ql")?;
            for (none,length,maximum) in [(publication_none,publication_length,138),(plan_none,plan_length,140)] {
                if resource_compare(session,none,0)?.is_lt() || resource_compare(session,none,1)?.is_gt() || resource_compare(session,length,0)?.is_lt() || resource_compare(session,length,maximum)?.is_gt() {return Err(invalid());}
            }
            let ob=sizes.try_get::<i64,_>("ob")?;
            let mb=sizes.try_get::<i64,_>("mb")?;
            let rb=sizes.try_get::<i64,_>("rb")?;
            drop(sizes);
            session.work().visit(ValidationVisit::RefLookup)?;
            let row=sqlx::query("SELECT operation_kind,operation_record_id,operation_digest,manifest_id,manifest_digest,receipt_id,receipt_digest,normalized_request_hash,commit_sequence,accepted_at_us,publication_id,plan_id FROM workflow_operations WHERE identity_key=? AND logical_store_id=? AND tenant=? AND environment=?")
                .bind(identity).bind(store).bind(&scope[0]).bind(&scope[1]).fetch_one(self.conn()).await?;
            session.work().visit(ValidationVisit::Constraint)?;
            let raw_kind=row.try_get::<&str,_>("operation_kind")?;
            if resource_compare(session,raw_kind.len() as i64,32)?.is_gt() {return Err(invalid());}
            let kind=match raw_kind {
                "policy.publish"=>"policy.publish", "evidence.retain"=>"evidence.retain", "authority.change"=>"authority.change", "agreement.accept"=>"agreement.accept", "agreement.activate"=>"agreement.activate", "agreement.revoke"=>"agreement.revoke", "work.accept"=>"work.accept", "work.reverse"=>"work.reverse", "quantity.correct"=>"quantity.correct", "outcome.submit"=>"outcome.submit", "outcome.correct"=>"outcome.correct", "outcome.close"=>"outcome.close", "link.assert"=>"link.assert", "invocation.authorize"=>"invocation.authorize", "invocation.close"=>"invocation.close", "cost.finalize"=>"cost.finalize", "cost.correct"=>"cost.correct", _=>return Err(invalid()),
            };
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            if kind=="policy.publish" {
                for (none,length,maximum) in [(publication_none,publication_length,138),(plan_none,plan_length,140)] {
                    if !resource_compare(session,none,0)?.is_eq() || !resource_compare(session,length,0)?.is_gt() || resource_compare(session,length,maximum)?.is_gt() { return Err(invalid()); }
                }
            } else {
                for (none,length) in [(publication_none,publication_length),(plan_none,plan_length)] {
                    if !resource_compare(session,none,1)?.is_eq() || !resource_compare(session,length,0)?.is_eq() {return Err(invalid());}
                }
            }
            let sequence=row.try_get::<i64,_>("commit_sequence")?;
            let accepted_at_us=row.try_get::<i64,_>("accepted_at_us")?;
            let copy=|field:&str, session:&mut OriginalAuditSession| -> Result<AuditedBytes,FinancialReadError> {
                session.work().visit(ValidationVisit::Field)?;
                let raw=row.try_get::<&str,_>(field)?.as_bytes();
                // Stored-byte probes admit UTF16 representation, but the
                // original canonical UTF8 identity language is unchanged.
                let maximum=match field {"operation_record_id"=>68,"manifest_id"=>74,"receipt_id"=>73,"publication_id"=>69,"plan_id"=>70,_=>71};
                if !resource_compare(session,raw.len() as i64,0)?.is_gt() || resource_compare(session,raw.len() as i64,maximum)?.is_gt() {return Err(invalid());}
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
            let (publication_id,plan_id)=if kind=="policy.publish" {(Some(copy("publication_id",session)?),Some(copy("plan_id",session)?))} else {(None,None)};
            drop(row);
            session.work().visit(ValidationVisit::RefLookup)?;
            let member_count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_manifest_members WHERE identity_key=?")
                .bind(identity).fetch_one(self.conn()).await?;
            if !resource_compare(session,member_count,0)?.is_gt() || resource_compare(session,member_count,4096)?.is_gt() {return Err(invalid());}
            let operation=self.financial_blob(identity,store,scope,0,ob,session).await?;
            let manifest=self.financial_blob(identity,store,scope,1,mb,session).await?;
            let receipt=self.financial_blob(identity,store,scope,2,rb,session).await?;
            Ok(Some(OriginalFrame {kind,operation,manifest,receipt,operation_id,operation_digest,manifest_id,manifest_digest,receipt_id,receipt_digest,request_hash,sequence,accepted_at_us,member_count,publication_id,plan_id}))
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        self.financial_finish_io(session).await?;
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
