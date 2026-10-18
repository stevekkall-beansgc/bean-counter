//! Bounded row acquisition for the existing local billing metadata reader.
//! Rows are literal DATA, never an admin/authority or completed-origin proof.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease, validation_work::RepresentationKind,
};
use std::mem;

#[derive(Clone, Copy)]
pub(crate) enum ManagementTable {
    Customers,
    Agreements,
    Controls,
    LegacyPermissions,
    ScopedPermissions,
}
pub(crate) struct ManagementRow {
    pub row_id: i64,
    pub text: [AuditedBytes; 4],
    pub numbers: [i64; 4],
    pub bodies: [Option<AuditedBytes>; 2],
    _control: ExternalBufferLease,
}
macro_rules! queries {
    ($projection:literal) => { queries!($projection, "rid") };
    ($projection:literal, $cursor:literal) => {(
        concat!("WITH r AS (", $projection, ") SELECT rid,octet_length(t0),octet_length(t1),octet_length(t2),octet_length(t3),n0,n1,n2,n3,length(b0),length(b1) FROM r WHERE (?1 IS NULL OR ", $cursor, ">?2) AND (?3 IS NULL OR t0=?3) AND (?4 IS NULL OR t1=?4) AND (?5 IS NULL OR t2=?5) AND (?6 IS NULL OR n0=?6) ORDER BY ", $cursor, " LIMIT 1"),
        concat!("WITH r AS (", $projection, ") SELECT rid,t0,t1,t2,t3,b0,b1 FROM r WHERE (?1 IS NULL OR ", $cursor, ">?2) AND (?3 IS NULL OR t0=?3) AND (?4 IS NULL OR t1=?4) AND (?5 IS NULL OR t2=?5) AND (?6 IS NULL OR n0=?6) ORDER BY ", $cursor, " LIMIT 1"),
        concat!("WITH r AS (", $projection, ") SELECT COALESCE(max(octet_length(t0)),0),COALESCE(max(octet_length(t1)),0),COALESCE(max(octet_length(t2)),0),COALESCE(max(octet_length(t3)),0),COALESCE(max(octet_length(b0)),0),COALESCE(max(octet_length(b1)),0) FROM r"),
        concat!("WITH r AS (", $projection, ") SELECT rid,t0,t1,t2,t3,n0,n1,n2,n3,b0,b1 FROM r WHERE (?1 IS NULL OR rid>?2) ORDER BY rid LIMIT 1")
    )};
}
fn queries(table: ManagementTable, scoped: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    match table {
        ManagementTable::Customers => queries!(
            "SELECT rowid rid,customer t0,tenant t1,environment t2,'' t3,0 n0,0 n1,0 n2,0 n3,NULL b0,NULL b1 FROM billing_customers"
        ),
        ManagementTable::Agreements => queries!(
            "SELECT rowid rid,customer t0,source t1,transition t2,agreement_id t3,revision n0,agreement_version n1,effective_at_us n2,recorded_at_us n3,setup_bytes b0,NULL b1 FROM billing_agreements"
        ),
        ManagementTable::Controls => queries!(
            "SELECT rowid rid,customer t0,source t1,change_id t2,operation t3,recorded_at_us n0,0 n1,0 n2,0 n3,request b0,response b1 FROM billing_m2_changes"
        ),
        ManagementTable::LegacyPermissions => queries!(
            "SELECT rowid rid,'' t0,'' t1,'' t2,'' t3,revision n0,0 n1,0 n2,0 n3,canonical_bytes b0,NULL b1 FROM billing_permissions"
        ),
        ManagementTable::ScopedPermissions if scoped => queries!(
            "SELECT rowid rid,customer t0,source t1,'' t2,'' t3,revision n0,recorded_at_us n1,0 n2,0 n3,canonical_bytes b0,NULL b1 FROM billing_m2_permissions",
            "n0"
        ),
        ManagementTable::ScopedPermissions => queries!(
            "SELECT rowid rid,customer t0,source t1,'' t2,'' t3,revision n0,recorded_at_us n1,0 n2,0 n3,canonical_bytes b0,NULL b1 FROM billing_m2_permissions"
        ),
    }
}
#[derive(Clone, Copy)]
struct Selection<'a> {
    after: Option<i64>,
    customer: Option<&'a str>,
    source: Option<&'a str>,
    change: Option<&'a str>,
    scoped: bool,
    revision: Option<i64>,
}
fn bind<'a>(
    sql: &'static str,
    selection: Selection<'a>,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    sqlx::query(sql)
        .bind(selection.after)
        .bind(selection.after)
        .bind(selection.customer)
        .bind(selection.source)
        .bind(selection.change)
        .bind(selection.revision)
}
fn text_maxima(table: ManagementTable) -> [i64; 4] {
    match table {
        ManagementTable::Customers => [128, 128, 128, 0],
        ManagementTable::Agreements => [128, 256, 5, 128],
        ManagementTable::Controls => [128, 256, 128, 11],
        ManagementTable::LegacyPermissions => [0; 4],
        ManagementTable::ScopedPermissions => [128, 256, 0, 0],
    }
}
pub(crate) struct ManagementInstallation {
    installation: crate::store::records::Installation,
    _storage: ExternalBufferLease,
}
impl ManagementInstallation {
    pub(crate) fn installation(&self) -> &crate::store::records::Installation {
        &self.installation
    }
}
/// Current fixed-host path producer. These borrowed paths never outlive the
/// live store owner. No new path copy, locator authority or numeric quota.
#[cfg(target_os = "macos")]
pub(crate) struct ManagementPaths<'a> {
    directory: &'a std::path::Path,
    database: &'a std::path::Path,
    _storage: ExternalBufferLease,
}
#[cfg(target_os = "macos")]
impl ManagementPaths<'_> {
    pub(crate) fn directory(&self) -> &std::path::Path {
        self.directory
    }
    pub(crate) fn database(&self) -> &std::path::Path {
        self.database
    }
}
#[cfg(target_os = "macos")]
impl super::super::super::SqliteStore {
    pub(crate) fn workflow_management_paths(
        &self,
        s: &mut OriginalAuditSession,
    ) -> Result<ManagementPaths<'_>, FinancialReadError> {
        // Darwin realpath's existing PATH_MAX is1024 (SDK sys/syslimits.h).
        // Its three token/link arrays, attrs name, two statfs names, mount
        // temporary, malloc result and Rust result fit the EXISTING256KiB
        // native I/O workspace, including fixed metadata/control overlap.
        // Add the actual input CString. This is not a new path limit or pool;
        // complete source/backend accounting remains an enclosing obligation.
        const NATIVE: usize = 262144 + std::mem::size_of::<ManagementPaths<'static>>();
        let input = self
            .inner
            ._owner
            .database
            .as_os_str()
            .as_encoded_bytes()
            .len();
        s.semantic_work()
            .arithmetic(ArithmeticOperation::Add, &[&NATIVE.into(), &input.into()])?;
        let bytes = NATIVE.checked_add(input).ok_or_else(invalid)?;
        s.semantic_work()
            .arithmetic(ArithmeticOperation::Add, &[&bytes.into(), &1usize.into()])?;
        let bytes = bytes.checked_add(1).ok_or_else(invalid)?;
        let storage = s.reserve_external_payload(bytes)?;
        s.semantic_work().scan_bytes(bytes)?;
        s.semantic_work().write_bytes(bytes)?;
        self.inner._owner.verify_path_with_work(s.semantic_work())?;
        Ok(ManagementPaths {
            directory: &self.inner._owner.directory,
            database: &self.inner._owner.database,
            _storage: storage,
        })
    }
}
impl SqliteTx {
    /// Actual singleton identity is compared in the same held snapshot before
    /// materializing unconstrained legacy TEXT. A mismatch is DATA, not an
    /// authorization grant. Successful output retains its complete owner.
    pub(crate) async fn workflow_management_installation(
        &mut self,
        store: &str,
        tenant: &str,
        environment: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementInstallation>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned management installation").into());
        }
        self.failed = true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
            let bindings=[store.as_bytes(),tenant.as_bytes(),environment.as_bytes()];
            let payload=io::binding_payload(&bindings,&[],0,0,s)?;
            self.financial_reserve_io(s,payload)?;
            s.semantic_work().write_bytes(mem::size_of::<(i64,i64,i64,i64,i64,bool)>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let row:(i64,i64,i64,i64,i64,bool)=sqlx::query_as("SELECT octet_length(logical_store_id),octet_length(tenant),octet_length(environment),octet_length(mode),octet_length(admission),logical_store_id=? AND tenant=? AND environment=? FROM installation WHERE singleton=1")
                .bind(store).bind(tenant).bind(environment).fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            if !row.5 { return Ok(None); }
            // UTF8 and UTF16 encodings of the exact equal identity differ by
            // at most two in either direction. Enum bounds are schema CHECKs.
            for (length,bound) in [(row.0,store.len()),(row.1,tenant.len()),(row.2,environment.len()),(row.3,7usize),(row.4,17usize)] {
                s.semantic_work().visit(ValidationVisit::Element)?;
                s.semantic_work().arithmetic(ArithmeticOperation::Multiply,&[&bound.into(),&2usize.into()])?;
                let bound=bound.checked_mul(2).ok_or_else(invalid)?;
                let bound=i64::try_from(bound).map_err(|_|invalid())?;
                if resource_compare(s,length,0)?.is_lt() || resource_compare(s,length,bound)?.is_gt() {
                    return Err(StoreError::Integrity("management installation text association").into());
                }
            }
            let mut output=mem::size_of::<ManagementInstallation>();
            for length in [row.0,row.1,row.2,row.3,row.4] {
                s.semantic_work().visit(ValidationVisit::Element)?;
                s.semantic_work().arithmetic(ArithmeticOperation::Multiply,&[&length.into(),&8i64.into()])?;
                let bytes=length.checked_mul(8).ok_or_else(invalid)?;
                s.semantic_work().arithmetic(ArithmeticOperation::Add,&[&output.into(),&bytes.into()])?;
                output=output.checked_add(usize::try_from(bytes).map_err(|_|invalid())?).ok_or_else(invalid)?;
            }
            let storage=s.reserve_external_payload(output)?;
            s.semantic_work().write_bytes(output)?;
            let payload=io::binding_payload(&[],&[],0,output,s)?;
            self.financial_reserve_io(s,payload)?;
            s.semantic_work().scan_bytes(output)?;
            s.semantic_work().write_bytes(output)?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let installation=crate::store::sqlite::read::installation(self.conn()).await?;
            self.financial_finish_io(s).await?;
            Ok(Some(ManagementInstallation{installation,_storage:storage}))
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("management installation retains failed work").into(),
                );
            }
            self.failed = false;
        }
        result
    }
    /// Same exclusive next-workflow cutoff as workflow_sequence, with actual
    /// scalar results and max+one arithmetic paid by the consuming parent.
    pub(crate) async fn workflow_management_sequence(
        &mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<i64, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned management sequence").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
            self.financial_reserve_io(s, 262144)?;
            s.semantic_work().write_bytes(mem::size_of::<i64>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let version: i64 = sqlx::query_scalar("PRAGMA user_version")
                .fetch_one(self.conn())
                .await?;
            let mut supported = false;
            for allowed in [12i64, 13, 14, 15, 16, 17] {
                if resource_compare(s, version, allowed)?.is_eq() {
                    supported = true;
                    break;
                }
            }
            if !supported {
                return Err(StoreError::BillingUpgradeRequired.into());
            }
            s.semantic_work().write_bytes(mem::size_of::<i64>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let last: i64 = sqlx::query_scalar(
                "SELECT COALESCE(max(commit_sequence),0) FROM workflow_operations",
            )
            .fetch_one(self.conn())
            .await?;
            let sequence = resource_add(s, last, 1)?;
            if self.workflow_operator.is_none() && resource_compare(s, sequence, 100000)?.is_gt() {
                return Err(StoreError::BillingHistoryLimit.into());
            }
            self.financial_finish_io(s).await?;
            Ok(sequence)
        })
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("management sequence retains failed work").into(),
                );
            }
            self.failed = false;
        }
        result
    }
    /// Connection-local mutation baseline for one live held management owner.
    /// Never refreshed by a reuse check; BEGIN IMMEDIATE excludes other writers.
    pub(crate) async fn workflow_management_changes(
        &mut self, s: &mut OriginalAuditSession,
    ) -> Result<i64, FinancialReadError> {
        if self.failed || self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(StoreError::Integrity("poisoned management baseline").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
            self.financial_reserve_io(s, 262144)?;
            s.semantic_work().write_bytes(mem::size_of::<i64>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let changes = sqlx::query_scalar("SELECT total_changes()")
                .fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;
            Ok(changes)
        }).await.map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) || self.financial_io.is_some() {
                return Err(StoreError::Integrity("management baseline retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    /// Actual final guard, called from the SAME consuming commit owner. This
    /// replaces the legacy Noop pass there; an earlier append check never
    /// suppresses these executed retained-meter equations.
    pub(crate) async fn workflow_management_commit_guard(
        &mut self,
        before: Option<i64>,
        s: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        self.financial_reserve_io(s, 262144)?;
        s.semantic_work().write_bytes(mem::size_of::<i64>())?;
        s.semantic_work().visit(ValidationVisit::RefLookup)?;
        let after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(self.conn())
            .await?;
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        let before = before.ok_or(StoreError::Integrity("workflow mutation origin missing"))?;
        if !resource_compare(s, after, before)?.is_eq() {
            s.semantic_work().write_bytes(mem::size_of::<i64>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let version = self.billing_schema_version().await?;
            let mut supported = false;
            for allowed in [12i64, 13, 14, 15, 16, 17] {
                if resource_compare(s, version, allowed)?.is_eq() {
                    supported = true;
                    break;
                }
            }
            if supported {
                {let limits=self.workflow_operator;super::super::capacity::check_with_session(self.conn(), 0, 0, s,limits).await?;}
            } else if !resource_compare(s, version, 11)?.is_eq() {
                return Err(StoreError::InvalidStore("unsupported guarded workflow schema").into());
            }
        }
        self.financial_finish_io(s).await?;
        Ok(())
    }
    /// Counts preserve the old global metadata caps before any per-row read.
    /// No all-history Vec/String/BLOB snapshot is constructed by this entry.
    pub(crate) async fn workflow_management_counts(
        &mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<[i64; 5], FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned management counts").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
            let payload = io::binding_payload(&[], &[], 0, 0, s)?;
            self.financial_reserve_io(s, payload)?;
            s.semantic_work().write_bytes(mem::size_of::<[i64;5]>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let row:(i64,i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM billing_customers),(SELECT count(*) FROM billing_agreements),(SELECT count(*) FROM billing_m2_changes),(SELECT count(*) FROM billing_permissions),(SELECT count(*) FROM billing_m2_permissions)").fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;
            let permissions = resource_add(s, row.3, row.4)?;
            if resource_compare(s, permissions, 1000)?.is_gt() { return Err(StoreError::InvalidStore("billing permission history bound").into()); }
            for (count, maximum) in [(row.0,1000),(row.1,1000),(row.2,2000)] {
                if resource_compare(s, count, maximum)?.is_gt() { return Err(StoreError::InvalidStore("billing customer/control history bound").into()); }
            }
            Ok([row.0,row.1,row.2,row.3,row.4])
        }).await.map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("management read retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    pub(crate) async fn workflow_management_setup_bytes(
        &mut self,
        s: &mut OriginalAuditSession,
    ) -> Result<AuditedBytes, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned management setup").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), async {
            let payload = io::binding_payload(&[], &[], 0, 0, s)?;
            self.financial_reserve_io(s, payload)?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            s.semantic_work().write_bytes(mem::size_of::<i64>())?;
            let length: i64 = sqlx::query_scalar(
                "SELECT length(canonical_bytes) FROM billing_setup WHERE singleton=1",
            )
            .fetch_one(self.conn())
            .await?;
            if resource_compare(s, length, 0)?.is_lt()
                || resource_compare(s, length, 65536)?.is_gt()
            {
                return Err(StoreError::Integrity("management setup bounds").into());
            }
            self.financial_finish_io(s).await?;
            let payload = io::binding_payload(&[], &[], 0, 2 * 65536, s)?;
            s.semantic_work().scan_bytes(2 * 65536)?;
            s.semantic_work().write_bytes(2 * 65536)?;
            self.financial_reserve_io(s, payload)?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let row = sqlx::query("SELECT canonical_bytes FROM billing_setup WHERE singleton=1")
                .fetch_one(self.conn())
                .await?;
            let raw: &[u8] = row.try_get(0)?;
            if !resource_compare(s, raw.len() as i64, length)?.is_eq() {
                return Err(StoreError::Integrity("management setup association").into());
            }
            let result = s.copy_representation(raw, RepresentationKind::Record)?;
            drop(row);
            self.financial_finish_io(s).await?;
            Ok(result)
        })
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("management read retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    /// Preserve the legacy physical type/UTF-8 refusal predicates without
    /// constructing unused owned String/BLOB/row graphs. The entire phase owns
    /// one native workspace; at most one LIMIT-1 row is borrowed at a time.
    pub(crate) async fn workflow_management_physical_rows(
        &mut self,table:ManagementTable,count:i64,s:&mut OriginalAuditSession,
    )->Result<(),FinancialReadError> {
        if self.failed || self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(StoreError::Integrity("poisoned management physical read").into());
        }
        self.failed=true;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
            let (_,_,bounds,rows)=queries(table,false);
            let mut materialized=2*65536i64;
            for maximum in text_maxima(table) {
                s.semantic_work().arithmetic(ArithmeticOperation::Multiply,&[&maximum.into(),&8i64.into()])?;
                materialized=resource_add(s,materialized,maximum*8)?;
            }
            let payload=io::binding_payload(&[],&[],0,materialized as usize,s)?;
            self.financial_reserve_io(s,payload)?;
            s.semantic_work().write_bytes(std::mem::size_of::<[i64;6]>())?;
            s.semantic_work().visit(ValidationVisit::RefLookup)?;
            let maxima:(i64,i64,i64,i64,i64,i64)=sqlx::query_as(bounds).fetch_one(self.conn()).await?;
            for (length,maximum) in [maxima.0,maxima.1,maxima.2,maxima.3].into_iter().zip(text_maxima(table)) {
                if resource_compare(s,length,0)?.is_lt() || resource_compare(s,length,maximum)?.is_gt() {
                    return Err(StoreError::Integrity("management text bounds").into());
                }
            }
            for length in [maxima.4,maxima.5] {
                if resource_compare(s,length,0)?.is_lt() || resource_compare(s,length,65536)?.is_gt() {
                    return Err(StoreError::Integrity("management body bounds").into());
                }
            }
            let mut after=None;
            for _ in 0..count {
                s.semantic_work().visit(ValidationVisit::Element)?;
                // Includes SQLx/SQLite integer/null bindings and possible UTF16
                // conversion before a malformed UTF-8/type refusal. No row data
                // escapes or accumulates outside this preadmitted workspace.
                s.semantic_work().scan_bytes(materialized as usize)?;
                s.semantic_work().write_bytes(materialized as usize)?;
                s.semantic_work().visit(ValidationVisit::RefLookup)?;
                let row=sqlx::query(rows).bind(after).bind(after).fetch_optional(self.conn()).await?;
                s.semantic_work().visit(ValidationVisit::Constraint)?;
                let row=row.ok_or(StoreError::Integrity("management row disappeared"))?;
                s.semantic_work().visit(ValidationVisit::Field)?;
                let row_id:i64=row.try_get(0)?;
                for index in [5usize,6,7,8] {
                    s.semantic_work().visit(ValidationVisit::Field)?;
                    let _:i64=row.try_get(index)?;
                }
                for index in [1usize,2,3,4] {
                    s.semantic_work().visit(ValidationVisit::Field)?;
                    let _:&str=row.try_get(index)?;
                }
                for (index,required) in [(9usize,!matches!(table,ManagementTable::Customers|ManagementTable::Agreements)),(10usize,matches!(table,ManagementTable::Controls))] {
                    s.semantic_work().visit(ValidationVisit::Field)?;
                    let body:Option<&[u8]>=row.try_get(index)?;
                    s.semantic_work().visit(ValidationVisit::Constraint)?;
                    if required && body.is_none() {return Err(StoreError::Integrity("management required body is null").into());}
                }
                after=Some(row_id);
                drop(row);
            }
            self.financial_finish_io(s).await?;
            Ok(())
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) || self.financial_io.is_some() {
                return Err(StoreError::Integrity("management physical read retains failed work").into());
            }
            self.failed=false;
        }
        result
    }
    /// Actual next physical metadata row, preserving all old String/i64/BLOB
    /// decoding even for rows unrelated to the installation's effective pair.
    /// Rowid is only a same-snapshot cursor; no historical authority is inferred.
    pub(crate) async fn workflow_management_row(
        &mut self,
        table: ManagementTable,
        after: Option<i64>,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned management row").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.management_row_inner(
                table,
                Selection {
                    after,
                    customer: None,
                    source: None,
                    change: None,
                    scoped: false,
                    revision: None,
                },
                s,
            ),
        )
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("management read retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    pub(crate) async fn workflow_management_scoped_permission(
        &mut self,
        customer: &str,
        source: &str,
        after_revision: Option<i64>,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        self.management_selected(
            ManagementTable::ScopedPermissions,
            Selection {
                after: after_revision,
                customer: Some(customer),
                source: Some(source),
                change: None,
                scoped: true,
                revision: None,
            },
            s,
        )
        .await
    }
    pub(crate) async fn workflow_management_permission_control(
        &mut self,
        customer: &str,
        source: &str,
        change_id: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        self.management_selected(
            ManagementTable::Controls,
            Selection {
                after: None,
                customer: Some(customer),
                source: Some(source),
                change: Some(change_id),
                scoped: false,
                revision: None,
            },
            s,
        )
        .await
    }
    pub(crate) async fn workflow_management_initial_terms(
        &mut self,
        customer: &str,
        source: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        self.management_selected(
            ManagementTable::Agreements,
            Selection {
                after: None,
                customer: Some(customer),
                source: Some(source),
                change: Some("start"),
                scoped: false,
                revision: Some(1),
            },
            s,
        )
        .await
    }
    async fn management_selected(
        &mut self,
        table: ManagementTable,
        selection: Selection<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned selected management row").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.management_row_inner(table, selection, s),
        )
        .await
        .map_err(|_| self.workflow_timeout_error())?;
        if result.is_ok() {
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity("management read retains failed work").into());
            }
            self.failed = false;
        }
        result
    }
    async fn management_row_inner(
        &mut self,
        table: ManagementTable,
        selection: Selection<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<ManagementRow>, FinancialReadError> {
        let (probe, values, _, _) = queries(table, selection.scoped);
        let mut bindings = [b"".as_slice(); 3];
        let mut count = 0usize;
        for text in [selection.customer, selection.source, selection.change]
            .into_iter()
            .flatten()
        {
            s.semantic_work().visit(ValidationVisit::Element)?;
            bindings[count] = text.as_bytes();
            s.semantic_work()
                .arithmetic(ArithmeticOperation::Add, &[&count.into(), &1usize.into()])?;
            count += 1;
        }
        let bindings = &bindings[..count];
        let payload = io::binding_payload(bindings, &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work()
            .write_bytes(mem::size_of::<[i64; 9]>() + mem::size_of::<[Option<i64>; 2]>())?;
        s.semantic_work().visit(ValidationVisit::RefLookup)?;
        let row = bind(probe, selection).fetch_optional(self.conn()).await?;
        let Some(row) = row else {
            self.financial_finish_io(s).await?;
            return Ok(None);
        };
        let row_id: i64 = row.try_get(0)?;
        let numbers: [i64; 4] = [
            row.try_get(5)?,
            row.try_get(6)?,
            row.try_get(7)?,
            row.try_get(8)?,
        ];
        let lengths: [Option<i64>; 2] = [row.try_get(9)?, row.try_get(10)?];
        for (index, maximum) in text_maxima(table).into_iter().enumerate() {
            s.semantic_work().visit(ValidationVisit::Element)?;
            s.semantic_work()
                .arithmetic(ArithmeticOperation::Add, &[&index.into(), &1usize.into()])?;
            let length: i64 = row.try_get(index + 1)?;
            if resource_compare(s, length, 0)?.is_lt()
                || resource_compare(s, length, maximum)?.is_gt()
            {
                return Err(StoreError::Integrity("management text bounds").into());
            }
        }
        for (index, length) in lengths.iter().enumerate() {
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            let required = match table {
                ManagementTable::Controls => true,
                ManagementTable::LegacyPermissions | ManagementTable::ScopedPermissions => {
                    index == 0
                }
                _ => false,
            };
            if required && length.is_none() {
                return Err(StoreError::Integrity("management required body is null").into());
            }
        }
        for length in lengths.into_iter().flatten() {
            if resource_compare(s, length, 0)?.is_lt()
                || resource_compare(s, length, 65536)?.is_gt()
            {
                return Err(StoreError::Integrity("management body bounds").into());
            }
        }
        drop(row);
        self.financial_finish_io(s).await?;
        // UTF16 text conversion uses the same conservative8M overlap as other
        // owning readers. Both maximum64KiB BLOB fields fit one <=256KiB row.
        let mut materialized = 2 * 65536;
        for maximum in text_maxima(table) {
            s.semantic_work().arithmetic(
                ArithmeticOperation::Multiply,
                &[&maximum.into(), &8i64.into()],
            )?;
            materialized = resource_add(s, materialized, maximum * 8)?;
        }
        let payload = io::binding_payload(bindings, &[], 0, materialized as usize, s)?;
        s.semantic_work().scan_bytes(materialized as usize)?;
        s.semantic_work().write_bytes(materialized as usize)?;
        self.financial_reserve_io(s, payload)?;
        const CONTROLS: usize = mem::size_of::<ManagementRow>()
            + mem::size_of::<[Option<AuditedBytes>; 4]>()
            + mem::size_of::<[Option<AuditedBytes>; 2]>()
            + mem::size_of::<[i64; 4]>()
            + mem::size_of::<[Option<i64>; 2]>();
        let control = s.reserve_external_payload(CONTROLS)?;
        s.semantic_work().write_bytes(CONTROLS)?;
        s.semantic_work().visit(ValidationVisit::RefLookup)?;
        let row = bind(values, selection)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(|| StoreError::Integrity("management row disappeared"))?;
        if !resource_compare(s, row.try_get(0)?, row_id)?.is_eq() {
            return Err(StoreError::Integrity("management row association").into());
        }
        let mut text: [Option<AuditedBytes>; 4] = [None, None, None, None];
        for (index, target) in text.iter_mut().enumerate() {
            s.semantic_work().visit(ValidationVisit::Element)?;
            s.semantic_work()
                .arithmetic(ArithmeticOperation::Add, &[&index.into(), &1usize.into()])?;
            let raw: &str = row.try_get(index + 1)?;
            *target = Some(s.copy_representation(raw.as_bytes(), RepresentationKind::Record)?);
        }
        let mut bodies: [Option<AuditedBytes>; 2] = [None, None];
        for (index, target) in bodies.iter_mut().enumerate() {
            s.semantic_work().visit(ValidationVisit::Element)?;
            s.semantic_work()
                .arithmetic(ArithmeticOperation::Add, &[&index.into(), &5usize.into()])?;
            let raw: Option<&[u8]> = row.try_get(index + 5)?;
            match (raw, lengths[index]) {
                (Some(raw), Some(length)) => {
                    if !resource_compare(s, raw.len() as i64, length)?.is_eq() {
                        return Err(StoreError::Integrity("management body association").into());
                    }
                    *target = Some(s.copy_representation(raw, RepresentationKind::Record)?);
                }
                (None, None) => {}
                _ => return Err(StoreError::Integrity("management body null association").into()),
            }
        }
        drop(row);
        self.financial_finish_io(s).await?;
        let [a, b, c, d] = text;
        Ok(Some(ManagementRow {
            row_id,
            text: [
                a.ok_or_else(invalid)?,
                b.ok_or_else(invalid)?,
                c.ok_or_else(invalid)?,
                d.ok_or_else(invalid)?,
            ],
            numbers,
            bodies,
            _control: control,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ports::{AcceptanceStore,AcceptanceTx};
    #[tokio::test]
    async fn physical_management_decode_preserves_invalid_utf8_refusal_and_rollback() {
        let directory=tempfile::tempdir().unwrap();
        let path=directory.path().canonicalize().unwrap().join("billing");
        crate::billing::BillingLedger::init(&path,include_bytes!("../../../../../../../examples/billing/setup.json")).await.unwrap();
        let store=crate::store::sqlite::SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let deadline=||tokio::time::Instant::now()+std::time::Duration::from_secs(10);
        let mut legacy=store.begin(deadline()).await.unwrap();
        sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES('invalid-utf8-customer',CAST(X'80' AS TEXT),'local')").execute(legacy.conn()).await.unwrap();
        assert!(matches!(legacy.billing_meta().await,Err(StoreError::Database(_))));
        legacy.rollback().await.unwrap();
        let arena=ledgerlab_core::canonical::audit_arena::AuditArena::new().unwrap();
        let baseline=arena.usage().unwrap().live_bytes;
        let mut session=OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let mut current=store.begin(deadline()).await.unwrap();
        let counts=current.workflow_management_counts(&mut session).await.unwrap();
        current.workflow_management_physical_rows(ManagementTable::Customers,counts[0],&mut session).await.unwrap();
        sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES('invalid-utf8-customer',CAST(X'80' AS TEXT),'local')").execute(current.conn()).await.unwrap();
        assert!(matches!(current.workflow_management_physical_rows(ManagementTable::Customers,counts[0]+1,&mut session).await,Err(FinancialReadError::Store(StoreError::Database(_)))));
        current.rollback().await.unwrap();
        assert_eq!(arena.usage().unwrap().live_bytes,baseline);
        let mut reopened=store.begin(deadline()).await.unwrap();
        reopened.billing_meta().await.unwrap();
        reopened.rollback().await.unwrap();
        store.close().await;
    }
}
