//! Mechanical storage for the ordinary retail profile. No pricing or authority.
use super::*;
use crate::service::billing::ValidatedEntry;

pub(crate) struct BillingEntry {
    pub ordinal: i64,
    /// `None` identifies an unchanged schema-8 row, which belongs to the
    /// installation's original customer and terms profile.
    pub customer: Option<String>,
    pub source: String,
    pub external_id: String,
    pub semantic_key: Vec<u8>,
    pub ingress: Vec<u8>,
    pub facts: Vec<u8>,
    pub bundle: Vec<u8>,
    pub accepted_at_us: Option<i64>,
    pub agreement_id: Option<String>,
    pub agreement_version: Option<i64>,
}
pub(crate) struct BillingAlias {
    pub customer: Option<String>,
    pub source: String,
    pub external_id: String,
    pub ingress: Vec<u8>,
    pub ordinal: i64,
}
pub(crate) struct BillingCustomer {
    pub customer: String,
    pub tenant: String,
    pub environment: String,
}
pub(crate) struct BillingAgreement {
    pub customer: String,
    pub source: String,
    pub revision: i64,
    pub transition: String,
    pub agreement_id: String,
    pub agreement_version: i64,
    pub effective_at_us: i64,
    pub recorded_at_us: i64,
    pub setup: Option<Vec<u8>>,
}
pub(crate) struct BillingControl {
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub operation: String,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub recorded_at_us: i64,
}
pub(crate) struct BillingScopedPermission {
    pub customer: String,
    pub source: String,
    pub revision: i64,
    pub canonical_bytes: Vec<u8>,
    pub recorded_at_us: i64,
}
pub(crate) struct BillingSnapshot {
    pub permissions: Vec<Vec<u8>>,
    pub scoped_permissions: Vec<BillingScopedPermission>,
    pub customers: Vec<BillingCustomer>,
    pub agreements: Vec<BillingAgreement>,
    pub controls: Vec<BillingControl>,
    pub aliases: Vec<BillingAlias>,
    pub setup: Vec<u8>,
    pub entries: Vec<BillingEntry>,
}
pub(crate) struct BillingM2Initialization<'a> {
    pub customer: &'a str,
    pub tenant: &'a str,
    pub environment: &'a str,
    pub source: &'a str,
    pub agreement_id: &'a str,
    pub accepted_at_us: i64,
    pub setup: &'a [u8],
}

type AgreementRow = (
    String,
    String,
    i64,
    String,
    String,
    i64,
    i64,
    i64,
    Option<Vec<u8>>,
);
type ControlRow = (String, String, String, String, Vec<u8>, Vec<u8>, i64);

impl SqliteTx {
    pub(crate) async fn billing_m2_initialize(
        &mut self,
        initialization: BillingM2Initialization<'_>,
    ) -> Result<(), StoreError> {
        let BillingM2Initialization {
            customer,
            tenant,
            environment,
            source,
            agreement_id,
            accepted_at_us,
            setup,
        } = initialization;
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            let counts: (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing_customers),(SELECT count(*) FROM billing_agreements)",
            )
            .fetch_one(self.conn())
            .await?;
            if counts != (0, 0) || setup.len() > 65_536 {
                return Err(StoreError::InvalidStore("M2 billing initialization state"));
            }
            sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
                .bind(customer)
                .bind(tenant)
                .bind(environment)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,1,?,1,'start',?,?,?)")
                .bind(customer)
                .bind(source)
                .bind(agreement_id)
                .bind(accepted_at_us)
                .bind(accepted_at_us)
                .bind(setup)
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
            let (m2_count,m2_size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)),0) FROM billing_m2_entries").fetch_one(self.conn()).await?;
            if count+m2_count>1000 || size+m2_size>33_554_432 {return Err(StoreError::InvalidStore("billing history bound"));}
            let (alias_count,alias_size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_aliases").fetch_one(self.conn()).await?;
            let (m2_alias_count,m2_alias_size):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(sum(length(ingress)),0) FROM billing_m2_aliases").fetch_one(self.conn()).await?;
            if alias_count+m2_alias_count>1000 || alias_size+m2_alias_size>33_554_432 {return Err(StoreError::InvalidStore("billing alias bound"));}
            let permissions:Vec<Vec<u8>>=sqlx::query_scalar("SELECT canonical_bytes FROM billing_permissions ORDER BY revision").fetch_all(self.conn()).await?;
            let scoped_permission_rows:Vec<(String,String,i64,Vec<u8>,i64)>=sqlx::query_as("SELECT customer,source,revision,canonical_bytes,recorded_at_us FROM billing_m2_permissions ORDER BY customer,source,revision").fetch_all(self.conn()).await?;
            if permissions.len()+scoped_permission_rows.len()>1000 {return Err(StoreError::InvalidStore("billing permission history bound"));}
            let customer_rows:Vec<(String,String,String)>=sqlx::query_as("SELECT customer,tenant,environment FROM billing_customers ORDER BY customer").fetch_all(self.conn()).await?;
            let agreement_rows: Vec<AgreementRow> = sqlx::query_as("SELECT customer,source,revision,transition,agreement_id,agreement_version,effective_at_us,recorded_at_us,setup_bytes FROM billing_agreements ORDER BY customer,source,revision").fetch_all(self.conn()).await?;
            let control_rows: Vec<ControlRow> = sqlx::query_as("SELECT customer,source,change_id,operation,request,response,recorded_at_us FROM billing_m2_changes ORDER BY customer,source,change_id").fetch_all(self.conn()).await?;
            if customer_rows.len()>1000 || agreement_rows.len()>1000 || control_rows.len()>2000 {return Err(StoreError::InvalidStore("billing customer/control history bound"));}
            let aliases:Vec<(String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT source,external_id,ingress,ordinal FROM billing_aliases ORDER BY source,external_id").fetch_all(self.conn()).await?;
            let m2_aliases:Vec<(String,String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT customer,source,external_id,ingress,ordinal FROM billing_m2_aliases ORDER BY customer,source,external_id").fetch_all(self.conn()).await?;
            type Row=(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>);
            let rows:Vec<Row>=sqlx::query_as("SELECT ordinal,source,external_id,semantic_key,ingress,facts,bundle FROM billing_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            type M2Row=(i64,String,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,i64,String,i64);
            let m2_rows:Vec<M2Row>=sqlx::query_as("SELECT ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version FROM billing_m2_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            let mut entries:Vec<BillingEntry>=rows.into_iter().map(|(ordinal,source,external_id,semantic_key,ingress,facts,bundle)|BillingEntry {ordinal,customer:None,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us:None,agreement_id:None,agreement_version:None}).collect();
            entries.extend(m2_rows.into_iter().map(|(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version)|BillingEntry {ordinal,customer:Some(customer),source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us:Some(accepted_at_us),agreement_id:Some(agreement_id),agreement_version:Some(agreement_version)}));
            entries.sort_by_key(|e|e.ordinal);
            let mut combined_aliases:Vec<BillingAlias>=aliases.into_iter().map(|(source,external_id,ingress,ordinal)|BillingAlias {customer:None,source,external_id,ingress,ordinal}).collect();
            combined_aliases.extend(m2_aliases.into_iter().map(|(customer,source,external_id,ingress,ordinal)|BillingAlias {customer:Some(customer),source,external_id,ingress,ordinal}));
            Ok::<_, StoreError>(BillingSnapshot {
                setup,
                permissions,
                scoped_permissions: scoped_permission_rows
                    .into_iter()
                    .map(|(customer, source, revision, canonical_bytes, recorded_at_us)| {
                        BillingScopedPermission { customer, source, revision, canonical_bytes, recorded_at_us }
                    })
                    .collect(),
                customers: customer_rows
                    .into_iter()
                    .map(|(customer, tenant, environment)| BillingCustomer { customer, tenant, environment })
                    .collect(),
                agreements: agreement_rows
                    .into_iter()
                    .map(|(customer, source, revision, transition, agreement_id, agreement_version, effective_at_us, recorded_at_us, setup)| {
                        BillingAgreement { customer, source, revision, transition, agreement_id, agreement_version, effective_at_us, recorded_at_us, setup }
                    })
                    .collect(),
                controls: control_rows
                    .into_iter()
                    .map(|(customer, source, change_id, operation, request, response, recorded_at_us)| {
                        BillingControl { customer, source, change_id, operation, request, response, recorded_at_us }
                    })
                    .collect(),
                aliases: combined_aliases,
                entries,
            })
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = failed;
        }
        result
    }
    pub(crate) async fn billing_m2_permissions(
        &mut self,
        plan: &crate::service::billing::permissions::ValidatedScopedPermissions,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            let (legacy, current): (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing_permissions),(SELECT count(*) FROM billing_m2_permissions)",
            )
            .fetch_one(self.conn())
            .await?;
            if legacy + current >= 1000 || plan.request.len() > 65_536 || plan.response.len() > 65_536 {
                return Err(StoreError::InvalidStore("billing permission history bound"));
            }
            sqlx::query("INSERT INTO billing_m2_changes(customer,source,change_id,operation,request,response,recorded_at_us) VALUES(?,?,?,'permissions',?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(&plan.change_id)
                .bind(&plan.request)
                .bind(&plan.response)
                .bind(plan.recorded_at_us)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_m2_permissions(customer,source,revision,canonical_bytes,recorded_at_us) VALUES(?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(plan.revision)
                .bind(&plan.request)
                .bind(plan.recorded_at_us)
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

    pub(crate) async fn billing_m2_agreement(
        &mut self,
        plan: &crate::service::billing::control::ValidatedControl,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            if let Some((tenant, environment)) = &plan.new_customer_scope {
                sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
                    .bind(&plan.customer)
                    .bind(tenant)
                    .bind(environment)
                    .execute(self.conn())
                    .await?;
            }
            sqlx::query("INSERT INTO billing_m2_changes(customer,source,change_id,operation,request,response,recorded_at_us) VALUES(?,?,?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(&plan.change_id)
                .bind(&plan.operation)
                .bind(&plan.request)
                .bind(&plan.response)
                .bind(plan.recorded_at_us)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(plan.revision)
                .bind(&plan.agreement_id)
                .bind(plan.agreement_version)
                .bind(&plan.transition)
                .bind(plan.effective_at_us)
                .bind(plan.recorded_at_us)
                .bind(&plan.setup_bytes)
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

    pub(crate) async fn append_billing_m2(
        &mut self,
        plan: &ValidatedEntry,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            let customer = plan
                .customer()
                .ok_or(StoreError::InvalidStore("M2 billing customer"))?;
            let (legacy_count, m2_count, legacy_aliases, m2_aliases): (i64, i64, i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing_entries),(SELECT count(*) FROM billing_m2_entries),(SELECT count(*) FROM billing_aliases),(SELECT count(*) FROM billing_m2_aliases)",
            )
            .fetch_one(self.conn())
            .await?;
            if legacy_count + m2_count != plan.expected_count() {
                return Err(StoreError::InvalidStore("billing current count"));
            }
            if let Some(ordinal) = plan.alias() {
                if legacy_aliases + m2_aliases >= 1000 || plan.ingress().len() > 262_144 {
                    return Err(StoreError::InvalidStore("billing alias bound"));
                }
                sqlx::query("INSERT INTO billing_m2_aliases(customer,source,external_id,ingress,ordinal) VALUES(?,?,?,?,?)")
                    .bind(customer)
                    .bind(plan.source())
                    .bind(plan.external_id())
                    .bind(plan.ingress())
                    .bind(ordinal)
                    .execute(self.conn())
                    .await?;
                return Ok(());
            }
            let accepted_at_us = plan
                .accepted_at_us()
                .ok_or(StoreError::InvalidStore("M2 billing acceptance time"))?;
            let agreement_id = plan
                .agreement_id()
                .ok_or(StoreError::InvalidStore("M2 billing agreement"))?;
            let agreement_version = plan
                .agreement_version()
                .ok_or(StoreError::InvalidStore("M2 billing agreement version"))?;
            if legacy_count + m2_count >= 1000 || plan.byte_len() > 33_554_432 {
                return Err(StoreError::InvalidStore("billing history bound/current count"));
            }
            sqlx::query("INSERT INTO billing_m2_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                .bind(legacy_count + m2_count + 1)
                .bind(customer)
                .bind(plan.source())
                .bind(plan.external_id())
                .bind(plan.semantic_key())
                .bind(plan.ingress())
                .bind(plan.facts())
                .bind(plan.bundle())
                .bind(accepted_at_us)
                .bind(agreement_id)
                .bind(agreement_version)
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
        let (_, plan) = service::prepare(
            &snapshot,
            "customer-1",
            "urn:example:work",
            EVENT,
            &crate::local::now().unwrap(),
        )
        .unwrap();
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
                sqlx::raw_sql("CREATE TRIGGER injected_billing_failure BEFORE INSERT ON billing_m2_entries BEGIN SELECT RAISE(ABORT,'test write failure'); END;").execute(&store.inner.writer).await.unwrap();
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
                let error = tx.append_billing_m2(&plan).await.unwrap_err();
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
                tx.append_billing_m2(&plan).await.unwrap();
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
            let accepted = ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
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
    async fn billing_control_unknown_commit_and_rollback_reconcile_exactly() {
        for permission_change in [false, true] {
            for cut in [1, 2] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().canonicalize().unwrap().join("billing");
                BillingLedger::init(&path, SETUP).await.unwrap();
                let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
                let mut tx = store
                    .begin(Instant::now() + Duration::from_secs(5))
                    .await
                    .unwrap();
                let snapshot = tx.billing_snapshot().await.unwrap();
                // Match the production ordering: begin the serialized write,
                // read the snapshot, then sample ledger time.
                let at = crate::local::now().unwrap();
                let raw = if permission_change {
                    serde_json::to_vec(&serde_json::json!({
                        "schema":"ledger-billing-permissions/2",
                        "customer":"customer-1",
                        "source":"urn:example:work",
                        "change_id":"unknown-permission-result",
                        "expected_revision":"1",
                        "permissions":["submit"],
                        "reason":"Revoke event reads and recover a lost acknowledgement"
                    }))
                    .unwrap()
                } else {
                    let mut setup: serde_json::Value = serde_json::from_slice(SETUP).unwrap();
                    setup["price"] = serde_json::json!("5.00");
                    serde_json::to_vec(&serde_json::json!({
                        "schema":"ledger-billing-amendment/2",
                        "customer":"customer-1",
                        "source":"urn:example:work",
                        "change_id":"unknown-agreement-result",
                        "expected_revision":"1",
                        "effective_at":"9999-12-31T23:59:59.999999Z",
                        "setup":setup
                    }))
                    .unwrap()
                };
                let expected = if permission_change {
                    let (result, plan) = service::permissions::prepare_scoped(
                        &snapshot,
                        "customer-1",
                        "urn:example:work",
                        &raw,
                        &at,
                    )
                    .unwrap();
                    tx.billing_m2_permissions(&plan.unwrap()).await.unwrap();
                    result
                } else {
                    let (result, plan) = service::control::prepare(
                        &snapshot,
                        &raw,
                        &at,
                        "customer-1",
                        "urn:example:work",
                    )
                    .unwrap();
                    tx.billing_m2_agreement(&plan.unwrap()).await.unwrap();
                    result
                };
                store.inner.fence_cut.store(cut, Ordering::Release);
                assert!(matches!(
                    tx.commit().await,
                    Err(CommitError::OutcomeUnknown)
                ));
                store.close().await;

                let ledger = BillingLedger::open(&path).await.unwrap();
                let retry = if permission_change {
                    ledger
                        .permissions("customer-1", "urn:example:work", &raw)
                        .await
                        .unwrap()
                } else {
                    ledger
                        .agreement_control("customer-1", "urn:example:work", &raw)
                        .await
                        .unwrap()
                };
                assert_eq!(retry, expected);
                if permission_change {
                    assert_eq!(
                        ledger
                            .permission_status("customer-1", "urn:example:work")
                            .await
                            .unwrap()["permissions"],
                        serde_json::json!(["submit"])
                    );
                }
                ledger.close().await;

                let reopened = SqliteStore::open(&path.join(".ledger")).await.unwrap();
                let mut tx = reopened
                    .begin(Instant::now() + Duration::from_secs(5))
                    .await
                    .unwrap();
                let snapshot = tx.billing_snapshot().await.unwrap();
                assert_eq!(snapshot.controls.len(), 1);
                if permission_change {
                    assert_eq!(snapshot.scoped_permissions.len(), 1);
                    assert_eq!(snapshot.agreements.len(), 1);
                } else {
                    assert_eq!(snapshot.scoped_permissions.len(), 0);
                    assert_eq!(snapshot.agreements.len(), 2);
                }
                tx.rollback().await.unwrap();
                reopened.close().await;
            }
        }
    }

    #[tokio::test]
    async fn populated_billing_tables_refuse_mutation_without_changing_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        let mut alias: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        alias["id"] = serde_json::json!("alias");
        ledger
            .accept(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&alias).unwrap(),
            )
            .await
            .unwrap();
        ledger.permissions("customer-1", "urn:example:work", br#"{"schema":"ledger-billing-permissions/2","customer":"customer-1","source":"urn:example:work","change_id":"revoke-submit","expected_revision":"1","permissions":["read"],"reason":"test revocation"}"#).await.unwrap();
        let before = ledger.statement("customer-1", None).await.unwrap();
        let permissions = ledger
            .permission_status("customer-1", "urn:example:work")
            .await
            .unwrap();
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        for (table, column) in [
            ("billing_setup", "canonical_bytes"),
            ("billing_customers", "customer"),
            ("billing_agreements", "agreement_id"),
            ("billing_m2_entries", "bundle"),
            ("billing_m2_aliases", "ingress"),
            ("billing_m2_changes", "request"),
            ("billing_m2_permissions", "canonical_bytes"),
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
        assert_eq!(
            ledger
                .permission_status("customer-1", "urn:example:work")
                .await
                .unwrap(),
            permissions
        );
        assert_eq!(
            ledger
                .accept(
                    "customer-1",
                    "urn:example:work",
                    &serde_json::to_vec(&alias).unwrap(),
                )
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
                tx.append_billing_m2(&plan).await.unwrap();
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
            let result = ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
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
