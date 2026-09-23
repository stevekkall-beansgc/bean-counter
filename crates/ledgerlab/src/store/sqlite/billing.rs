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
pub(crate) struct BillingSnapshot {
    pub setup: Vec<u8>,
    pub entries: Vec<BillingEntry>,
}
impl SqliteTx {
    pub(crate) async fn billing_setup(&mut self, setup: &[u8]) -> Result<(), StoreError> {
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
            type Row=(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>);
            let rows:Vec<Row>=sqlx::query_as("SELECT ordinal,source,external_id,semantic_key,ingress,facts,bundle FROM billing_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            Ok::<_,StoreError>(BillingSnapshot {setup,entries:rows.into_iter().map(|(ordinal,source,external_id,semantic_key,ingress,facts,bundle)|BillingEntry {ordinal,source,external_id,semantic_key,ingress,facts,bundle}).collect()})
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = failed;
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
            if count!=plan.expected_count() || count>=1000 || size+plan.byte_len() as i64>33_554_432 {return Err(StoreError::InvalidStore("billing history bound/current count"));}
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
