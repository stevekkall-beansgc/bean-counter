//! Borrowed app acceptance write projection. All assent, original/current
//! authority and compiled-source semantics belong to the private coordinator.
//! These rows are candidate indices, never origin or authority capabilities.
use super::append::{FinancialDocument, FinancialJournal, JournalKey};
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;

pub(crate) struct AppAcceptanceAppend<'a> {
    pub logical_store_id: &'a str,
    pub scope: &'a [String; 2],
    pub source: &'a str,
    pub operation_id: &'a str,
    pub documents: &'a [FinancialDocument<'a>],
    pub journal: FinancialJournal<'a>,
    pub customer: &'a str,
    pub agreement: &'a str,
    pub semantic_key: &'a [u8],
    pub expected_revision: i64,
    pub revision: i64,
    pub version: i64,
    pub head: &'a ExactRef,
    pub accepted: &'a ExactRef,
    pub binding: &'a ExactRef,
    pub intent: &'a ExactRef,
}
fn same(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool, FinancialReadError> {
    let w = s.semantic_work();
    w.visit(ValidationVisit::MemberCompare)?;
    w.scan_bytes(a.len())?;
    w.scan_bytes(b.len())?;
    Ok(a == b)
}
impl SqliteTx {
    /// Called only by the service's privately authenticated prepared acceptance.
    /// This mechanical DATA boundary cannot normalize or approve its operands.
    pub(crate) async fn workflow_app_acceptance_append(
        &mut self,
        p: &AppAcceptanceAppend<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned app acceptance transaction").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(self.workflow_deadline(), self.app_acceptance_append_inner(p, s))
            .await
            .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            s.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(StoreError::Integrity(
                    "app acceptance completion retains poisoned work",
                )
                .into());
            }
            self.failed = false;
        }
        result
    }
    async fn app_acceptance_append_inner(
        &mut self,
        p: &AppAcceptanceAppend<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if !same(p.journal.kind, "agreement.accept", s)? {
            return Err(StoreError::Integrity("app acceptance journal kind").into());
        }
        let next = resource_add(s, p.expected_revision, 1)?;
        if resource_compare(s, p.expected_revision, 0)?.is_lt()
            || !resource_compare(s, next, p.revision)?.is_eq()
            || !resource_compare(s, p.version, p.revision)?.is_eq()
        {
            return Err(StoreError::ExpectedCurrent.into());
        }
        let key = JournalKey {
            logical_store_id: p.logical_store_id,
            scope: p.scope,
            source: p.source,
            operation_id: p.operation_id,
        };
        let delta = self
            .append_journal_rows(key, p.documents, &p.journal, s)
            .await?;
        // Every projection is a full exact immutable member. Never use an
        // ID-only latest document or reparse an asserted expected body here.
        for (r, kind, schema) in [
            (
                p.head,
                "workflow-head-transition",
                "ledger-workflow-head-transition/1",
            ),
            (
                p.accepted,
                "workflow-accepted-agreement",
                "ledger-workflow-accepted-agreement/1",
            ),
            (p.binding, "workflow-binding", "ledger-workflow-binding/2"),
            (
                p.intent,
                "workflow-agreement-intent",
                "ledger-workflow-agreement-intent/1",
            ),
        ] {
            if !same(&r.kind, kind, s)? {
                return Err(invalid());
            }
            let bound = [
                p.journal.identity,
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
                schema.as_bytes(),
            ];
            let payload = io::binding_payload(&bound, &[], 0, 0, s)?;
            self.financial_reserve_io(s, payload)?;
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents d JOIN workflow_manifest_members m ON m.kind=d.kind AND m.id=d.id AND m.digest=d.digest WHERE m.identity_key=? AND d.kind=? AND d.id=? AND d.digest=? AND d.schema_name=?)")
                .bind(p.journal.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).bind(schema).fetch_one(self.conn()).await?;
            self.financial_finish_io(s).await?;
            s.semantic_work().visit(ValidationVisit::Constraint)?;
            if !present {
                return Err(StoreError::Integrity(
                    "app acceptance projection outside exact journal",
                )
                .into());
            }
        }
        let owner = [
            p.logical_store_id.as_bytes(),
            p.scope[0].as_bytes(),
            p.scope[1].as_bytes(),
            p.agreement.as_bytes(),
            p.customer.as_bytes(),
        ];
        // Table five fields, PK four fields+rowid, UNIQUE five fields+rowid.
        // A prior owner is immutable. The following full customer FKs on BOTH
        // acceptance and head enforce its exact owner; no prior TEXT is fetched.
        let payload = io::binding_payload_sets(&owner, &[&owner, &owner[..4], &owner], 2, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        sqlx::query("INSERT INTO workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer) VALUES(?,?,?,?,?) ON CONFLICT(logical_store_id,tenant,environment,agreement_id) DO NOTHING")
            .bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.agreement).bind(p.customer).execute(self.conn()).await?;
        self.financial_finish_io(s).await?;
        let head = [
            p.logical_store_id.as_bytes(),
            p.scope[0].as_bytes(),
            p.scope[1].as_bytes(),
            p.agreement.as_bytes(),
            p.customer.as_bytes(),
            b"agreement_acceptance".as_slice(),
            p.head.id.as_bytes(),
        ];
        let head_key = [head[0], head[1], head[2], head[4], head[3], head[5]];
        let head_bindings = [head[0], head[1], head[2], head[3], head[4], head[6]];
        let payload = io::binding_payload_sets(&head_bindings, &[&head, &head_key], 2, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        let affected = if resource_compare(s, p.expected_revision, 0)?.is_eq() {
            sqlx::query("INSERT INTO workflow_agreement_heads(logical_store_id,tenant,environment,agreement_id,customer,head_kind,revision,transition_id) VALUES(?,?,?,?,?,'agreement_acceptance',?,?)")
                .bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.agreement).bind(p.customer).bind(p.revision).bind(&p.head.id).execute(self.conn()).await?.rows_affected()
        } else {
            sqlx::query("UPDATE workflow_agreement_heads SET revision=?,transition_id=? WHERE logical_store_id=? AND tenant=? AND environment=? AND customer=? AND agreement_id=? AND head_kind='agreement_acceptance' AND revision=?")
                .bind(p.revision).bind(&p.head.id).bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.customer).bind(p.agreement).bind(p.expected_revision).execute(self.conn()).await?.rows_affected()
        };
        self.financial_finish_io(s).await?;
        s.semantic_work().arithmetic(
            ArithmeticOperation::Compare,
            &[&affected.into(), &1u64.into()],
        )?;
        if affected != 1 {
            return Err(StoreError::ExpectedCurrent.into());
        }
        let acceptance = [
            owner[0],
            owner[1],
            owner[2],
            owner[3],
            owner[4],
            p.accepted.id.as_bytes(),
            p.binding.id.as_bytes(),
            p.intent.id.as_bytes(),
            p.head.id.as_bytes(),
        ];
        let acceptance_key = [owner[0], owner[1], owner[2], owner[4], owner[3]];
        let binding_key = [owner[0], owner[1], owner[2], p.binding.id.as_bytes()];
        let accepted_key = [owner[0], owner[1], owner[2], p.accepted.id.as_bytes()];
        // Table version + PK(version,rowid) + both UNIQUE rowids = five ints.
        let payload = io::binding_payload_sets(
            &acceptance,
            &[&acceptance, &acceptance_key, &binding_key, &accepted_key],
            5,
            0,
            s,
        )?;
        self.financial_reserve_io(s, payload)?;
        sqlx::query("INSERT INTO workflow_agreement_acceptances(logical_store_id,tenant,environment,agreement_id,customer,version,accepted_id,binding_id,intent_id,transition_id) VALUES(?,?,?,?,?,?,?,?,?,?)")
            .bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.agreement).bind(p.customer).bind(p.version).bind(&p.accepted.id).bind(&p.binding.id).bind(&p.intent.id).bind(&p.head.id).execute(self.conn()).await?;
        self.financial_finish_io(s).await?;
        let semantics = [
            owner[0],
            owner[1],
            owner[2],
            b"agreement.accept".as_slice(),
            p.semantic_key,
            p.journal.identity,
        ];
        let payload =
            io::binding_payload_sets(&semantics, &[&semantics, &semantics[..5]], 1, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        sqlx::query("INSERT INTO workflow_agreement_semantics(logical_store_id,tenant,environment,operation_kind,semantic_key,original_identity_key) VALUES(?,?,?,?,?,?)")
            .bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind("agreement.accept").bind(p.semantic_key).bind(p.journal.identity).execute(self.conn()).await?;
        self.financial_finish_io(s).await?;
        self.finish_journal_rows(&p.journal, delta, s).await
    }
}

pub(crate) struct AppActivationAppend<'a>{
 pub logical_store_id:&'a str,pub scope:&'a[String;2],pub source:&'a str,pub operation_id:&'a str,
 pub documents:&'a[FinancialDocument<'a>],pub journal:FinancialJournal<'a>,
 pub customer:&'a str,pub agreement:&'a str,pub expected_revision:i64,pub revision:i64,
 pub head:&'a ExactRef,pub activation:&'a ExactRef,pub binding:&'a ExactRef,pub intent:&'a ExactRef,
 pub effective_at_us:i64,
}
impl SqliteTx{
 pub(crate) async fn workflow_app_activation_append(&mut self,p:&AppActivationAppend<'_>,s:&mut OriginalAuditSession)->Result<(),FinancialReadError>{
  if self.failed||self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire){return Err(invalid());}
  self.failed=true;
  let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),self.app_activation_inner(p,s)).await.map_err(|_|FinancialReadError::Store(self.workflow_timeout_error()))?;
  if result.is_ok(){s.check_cancelled()?;if self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire)||self.financial_io.is_some(){return Err(invalid());}self.failed=false;}
  result
 }
 async fn app_activation_inner(&mut self,p:&AppActivationAppend<'_>,s:&mut OriginalAuditSession)->Result<(),FinancialReadError>{
  // This producer is the exact initial activation recipe. Replacement/revocation
  // retain their separate target/schedule validation; no empty target is forged.
  if !same(p.journal.kind,"agreement.activate",s)?||p.expected_revision!=0||p.revision!=1{return Err(StoreError::ExpectedCurrent.into());}
  let delta=self.append_journal_rows(JournalKey{logical_store_id:p.logical_store_id,scope:p.scope,source:p.source,operation_id:p.operation_id},p.documents,&p.journal,s).await?;
  for(r,kind,schema)in[(p.head,"workflow-head-transition","ledger-workflow-head-transition/1"),(p.activation,"workflow-activation","ledger-workflow-activation-transition/1"),(p.binding,"workflow-binding","ledger-workflow-binding/2"),(p.intent,"workflow-agreement-intent","ledger-workflow-agreement-intent/1")]{
   if !same(&r.kind,kind,s)?{return Err(invalid());}
   let b=[p.journal.identity,r.kind.as_bytes(),r.id.as_bytes(),r.digest.as_bytes(),schema.as_bytes()];let payload=io::binding_payload(&b,&[],0,0,s)?;self.financial_reserve_io(s,payload)?;
   let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents d JOIN workflow_manifest_members m ON m.kind=d.kind AND m.id=d.id AND m.digest=d.digest WHERE m.identity_key=? AND d.kind=? AND d.id=? AND d.digest=? AND d.schema_name=?)").bind(p.journal.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).bind(schema).fetch_one(self.conn()).await?;self.financial_finish_io(s).await?;
   if !present{return Err(invalid());}
  }
  let fields=[p.logical_store_id.as_bytes(),p.scope[0].as_bytes(),p.scope[1].as_bytes(),p.customer.as_bytes(),p.agreement.as_bytes(),p.binding.id.as_bytes()];
  let payload=io::binding_payload(&fields,&[],0,0,s)?;self.financial_reserve_io(s,payload)?;
  let accepted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_agreement_acceptances a JOIN workflow_agreement_owners o ON o.logical_store_id=a.logical_store_id AND o.tenant=a.tenant AND o.environment=a.environment AND o.agreement_id=a.agreement_id AND o.customer=a.customer WHERE a.logical_store_id=? AND a.tenant=? AND a.environment=? AND a.customer=? AND a.agreement_id=? AND a.binding_id=?)").bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.customer).bind(p.agreement).bind(&p.binding.id).fetch_one(self.conn()).await?;self.financial_finish_io(s).await?;if !accepted{return Err(invalid());}
  let head_fields=[fields[0],fields[1],fields[2],fields[4],fields[3],p.head.id.as_bytes()];let payload=io::binding_payload(&head_fields,&[],2,0,s)?;self.financial_reserve_io(s,payload)?;
  sqlx::query("INSERT INTO workflow_agreement_heads(logical_store_id,tenant,environment,agreement_id,customer,head_kind,revision,transition_id) VALUES(?,?,?,?,?,'agreement_activation',1,?)").bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.agreement).bind(p.customer).bind(&p.head.id).execute(self.conn()).await?;self.financial_finish_io(s).await?;
  let row=[fields[0],fields[1],fields[2],fields[4],fields[3],p.head.id.as_bytes(),p.activation.id.as_bytes(),p.binding.id.as_bytes(),p.intent.id.as_bytes()];let payload=io::binding_payload(&row,&[],6,0,s)?;self.financial_reserve_io(s,payload)?;
  sqlx::query("INSERT INTO workflow_agreement_activations(logical_store_id,tenant,environment,agreement_id,customer,revision,transition_id,activation_id,binding_id,intent_id,action,effective_at_us,accepted_at_us,commit_sequence,target_id) VALUES(?,?,?,?,?,1,?,?,?,?,'activate',?,?,?,NULL)").bind(p.logical_store_id).bind(&p.scope[0]).bind(&p.scope[1]).bind(p.agreement).bind(p.customer).bind(&p.head.id).bind(&p.activation.id).bind(&p.binding.id).bind(&p.intent.id).bind(p.effective_at_us).bind(p.journal.accepted_at_us).bind(p.journal.sequence).execute(self.conn()).await?;self.financial_finish_io(s).await?;
  self.finish_journal_rows(&p.journal,delta,s).await
 }
}
