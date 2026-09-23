//! Mechanical storage for the ordinary retail profile. No pricing or authority.
use super::*;
use crate::service::billing::ValidatedEntry;

pub(crate) struct BillingEntry {
    pub ordinal: i64,
    pub source: String,
    pub external_id: String,
    pub semantic_key: Vec<u8>,
    pub ingress: Vec<u8>,
    pub facts: Vec<u8>,
    pub bundle: Vec<u8>,
}
pub(crate) struct BillingAlias {
    pub source: String,
    pub external_id: String,
    pub ingress: Vec<u8>,
    pub ordinal: i64,
}
pub(crate) struct BillingSnapshot {
    pub permissions: Vec<Vec<u8>>,
    pub aliases: Vec<BillingAlias>,
    pub setup: Vec<u8>,
    pub entries: Vec<BillingEntry>,
}
impl SqliteTx {
    pub(crate) async fn billing_setup(&mut self, setup: &[u8]) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM billing_setup)+(SELECT count(*) FROM events)+(SELECT count(*) FROM outcome_records)+(SELECT count(*) FROM r3_commit_witness)").fetch_one(self.conn()).await?;
            if count!=0 || setup.len()>65536 {return Err(StoreError::InvalidStore("billing installation is not empty"));}
            sqlx::query("INSERT INTO billing_setup VALUES(1,?)").bind(setup).execute(self.conn()).await?;
            Ok::<_,StoreError>(())
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    pub(crate) async fn billing_snapshot(&mut self) -> Result<BillingSnapshot, StoreError> {
        let failed = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            let setup:Vec<u8>=sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1").fetch_one(self.conn()).await?;
            let (count,size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_entries").fetch_one(self.conn()).await?;
            if count>1000 || size>33_554_432 {return Err(StoreError::InvalidStore("billing history bound"));}
            let (alias_count,alias_size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_aliases").fetch_one(self.conn()).await?;
            if alias_count>1000 || alias_size>33_554_432 {return Err(StoreError::InvalidStore("billing alias bound"));}
            let permissions:Vec<Vec<u8>>=sqlx::query_scalar("SELECT canonical_bytes FROM billing_permissions ORDER BY revision").fetch_all(self.conn()).await?;
            if permissions.len()>1000 {return Err(StoreError::InvalidStore("billing permission history bound"));}
            let aliases:Vec<(String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT source,external_id,ingress,ordinal FROM billing_aliases ORDER BY source,external_id").fetch_all(self.conn()).await?;
            type Row=(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>);
            let rows:Vec<Row>=sqlx::query_as("SELECT ordinal,source,external_id,semantic_key,ingress,facts,bundle FROM billing_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            Ok::<_,StoreError>(BillingSnapshot {setup,permissions,aliases:aliases.into_iter().map(|(source,external_id,ingress,ordinal)|BillingAlias {source,external_id,ingress,ordinal}).collect(),entries:rows.into_iter().map(|(ordinal,source,external_id,semantic_key,ingress,facts,bundle)|BillingEntry {ordinal,source,external_id,semantic_key,ingress,facts,bundle}).collect()})
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = failed;
        }
        result
    }
    pub(crate) async fn billing_permissions(
        &mut self,
        plan: &crate::service::billing::permissions::ValidatedPermissions,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_permissions")
                .fetch_one(self.conn())
                .await?;
            if count >= 1000 || count + 2 != plan.revision() {
                return Err(StoreError::InvalidStore("billing permission revision"));
            }
            sqlx::query("INSERT INTO billing_permissions VALUES(?,?)")
                .bind(plan.revision())
                .bind(plan.bytes())
                .execute(self.conn())
                .await?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    pub(crate) async fn append_billing(&mut self, plan: &ValidatedEntry) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            let (count,size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_entries").fetch_one(self.conn()).await?;
            if count!=plan.expected_count() {return Err(StoreError::InvalidStore("billing current count"));}
            if let Some(ordinal) = plan.alias() {
                let (n,size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_aliases").fetch_one(self.conn()).await?;
                if n>=1000 || size+plan.ingress().len() as i64>33_554_432 {return Err(StoreError::InvalidStore("billing alias bound"));}
                sqlx::query("INSERT INTO billing_aliases VALUES(?,?,?,?)").bind(plan.source()).bind(plan.external_id()).bind(plan.ingress()).bind(ordinal).execute(self.conn()).await?;
                return Ok(());
            }
            if count>=1000 || size+plan.byte_len() as i64>33_554_432 {return Err(StoreError::InvalidStore("billing history bound/current count"));}
            sqlx::query("INSERT INTO billing_entries VALUES(?,?,?,?,?,?,?)")
                .bind(count+1).bind(plan.source()).bind(plan.external_id()).bind(plan.semantic_key()).bind(plan.ingress()).bind(plan.facts()).bind(plan.bundle()).execute(self.conn()).await?;
            Ok::<_,StoreError>(())
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        billing::BillingLedger,
        service::billing as service,
        store::errors::CommitError,
        store::ports::{AcceptanceStore, AcceptanceTx},
    };
    const SETUP: &[u8] = include_bytes!("../../../../../examples/billing/setup.json");
    const EVENT: &[u8] = include_bytes!("../../../../../examples/billing/event.json");
    async fn plan(store: &SqliteStore) -> (SqliteTx, service::ValidatedEntry) {
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        let snapshot = tx.billing_snapshot().await.unwrap();
        let (_, plan) = service::prepare(&snapshot, EVENT, &crate::local::now().unwrap()).unwrap();
        (tx, plan.unwrap())
    }
    #[tokio::test]
    async fn billing_write_failure_cancel_and_unknown_commit_reopen() {
        for cut in [0, 1, 2, 3, 4] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
            if cut == 0 {
                sqlx::raw_sql("CREATE TRIGGER injected_billing_failure BEFORE INSERT ON billing_entries BEGIN SELECT RAISE(ABORT,'test write failure'); END;").execute(&store.inner.writer).await.unwrap();
            }
            let old_limit: i64 = sqlx::query_scalar("PRAGMA max_page_count")
                .fetch_one(&store.inner.writer)
                .await
                .unwrap();
            if cut == 4 {
                let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
                    .fetch_one(&store.inner.writer)
                    .await
                    .unwrap();
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "PRAGMA max_page_count={pages}"
                )))
                .execute(&store.inner.writer)
                .await
                .unwrap();
            }
            let (mut tx, plan) = plan(&store).await;
            if cut == 0 || cut == 4 {
                let error = tx.append_billing(&plan).await.unwrap_err();
                if cut == 4 {
                    assert!(error.to_string().contains("full"), "{error}");
                }
                assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
                if cut == 0 {
                    sqlx::raw_sql("DROP TRIGGER injected_billing_failure")
                        .execute(&store.inner.writer)
                        .await
                        .unwrap();
                } else {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "PRAGMA max_page_count={old_limit}"
                    )))
                    .execute(&store.inner.writer)
                    .await
                    .unwrap();
                }
            } else {
                tx.append_billing(&plan).await.unwrap();
                if cut == 3 {
                    drop(tx);
                } else {
                    store.inner.fence_cut.store(cut, Ordering::Release);
                    assert!(matches!(
                        tx.commit().await,
                        Err(CommitError::OutcomeUnknown)
                    ));
                }
            }
            store.close().await;
            let ledger = BillingLedger::open(&path).await.unwrap();
            let statement = ledger.statement("customer-1", None).await.unwrap();
            assert_eq!(
                statement["entries"].as_array().unwrap().len(),
                usize::from(cut == 2)
            );
            let accepted = ledger.accept(EVENT).await.unwrap();
            assert_eq!(
                accepted["status"],
                if cut == 2 { "duplicate" } else { "accepted" }
            );
            assert_eq!(
                ledger.statement("customer-1", None).await.unwrap()["net_atoms"],
                "250"
            );
            ledger.close().await;
        }
    }
    #[tokio::test]
    async fn populated_billing_tables_refuse_mutation_without_changing_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger.accept(EVENT).await.unwrap();
        let mut alias: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        alias["id"] = serde_json::json!("alias");
        ledger
            .accept(&serde_json::to_vec(&alias).unwrap())
            .await
            .unwrap();
        ledger.permissions(br#"{"schema":"ledger-billing-permissions/1","expected_revision":"1","permissions":["read"],"reason":"test revocation"}"#).await.unwrap();
        let before = ledger.statement("customer-1", None).await.unwrap();
        let permissions = ledger.permission_status().await.unwrap();
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        for (table, column) in [
            ("billing_setup", "canonical_bytes"),
            ("billing_entries", "bundle"),
            ("billing_aliases", "ingress"),
            ("billing_permissions", "canonical_bytes"),
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&store.inner.readers)
                    .await
                    .unwrap();
            assert!(count > 0);
            for sql in [
                format!("UPDATE {table} SET {column}={column}"),
                format!("DELETE FROM {table}"),
            ] {
                let error = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(&store.inner.writer)
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("immutable billing"), "{error}");
            }
        }
        store.close().await;
        let ledger = BillingLedger::open(&path).await.unwrap();
        assert_eq!(ledger.statement("customer-1", None).await.unwrap(), before);
        assert_eq!(ledger.permission_status().await.unwrap(), permissions);
        assert_eq!(
            ledger
                .accept(&serde_json::to_vec(&alias).unwrap())
                .await
                .unwrap()["status"],
            "duplicate"
        );
        ledger.close().await;
    }
    #[test]
    #[ignore = "subprocess crash helper; invoked by billing_process_exit_reopens_atomically"]
    fn billing_crash_child() {
        let path = std::env::var_os("LEDGER_BILLING_CRASH_PATH").expect("parent supplies path");
        let cut: u8 = std::env::var("LEDGER_BILLING_CRASH_CUT")
            .unwrap()
            .parse()
            .unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let store = SqliteStore::open(&std::path::PathBuf::from(path).join(".ledger"))
                    .await
                    .unwrap();
                let (mut tx, plan) = plan(&store).await;
                tx.append_billing(&plan).await.unwrap();
                store.inner.fence_cut.store(cut, Ordering::Release);
                tx.commit().await.unwrap();
            });
        panic!("crash cut not reached");
    }
    #[tokio::test]
    async fn billing_process_exit_reopens_atomically() {
        for cut in [11, 12] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::sqlite::billing::tests::billing_crash_child",
                    "--ignored",
                ])
                .env("LEDGER_BILLING_CRASH_PATH", &path)
                .env("LEDGER_BILLING_CRASH_CUT", cut.to_string())
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
            let ledger = BillingLedger::open(&path).await.unwrap();
            let result = ledger.accept(EVENT).await.unwrap();
            assert_eq!(
                result["status"],
                if cut == 12 { "duplicate" } else { "accepted" }
            );
            assert_eq!(
                ledger.statement("customer-1", None).await.unwrap()["net_atoms"],
                "250"
            );
            ledger.close().await;
        }
    }
}
