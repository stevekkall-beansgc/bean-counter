//! New workflow custody I/O uses the existing same-writer payload/ACK owner.
//! Legacy M5 validation remains bounded by its released recipe; this module
//! does not claim to meter that whole historical validator or confer economics.
use super::*;
use crate::store::sqlite::workflow::financial::{io, FinancialReadError};
use ledgerlab_core::{
    canonical::audit_arena::AuditedValue,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use std::sync::atomic::Ordering;

type Result<T> = std::result::Result<T, FinancialReadError>;
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])?;
    a.checked_add(b).ok_or_else(|| bad().into())
}
fn mul(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b).ok_or_else(|| bad().into())
}
fn length(n: i64, max: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &0i64.into(), &max.into()])?;
    if n <= 0 || usize::try_from(n).map_err(|_| bad())? > max {
        return Err(bad().into());
    }
    Ok(n as usize)
}
fn value(
    raw: &[u8],
    kind: RepresentationKind,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let v = s.parse_representation(raw, kind)?;
    let canonical = s.canonical_representation(v.value(), kind)?;
    s.semantic_work().visit(Visit::MemberCompare)?;
    s.semantic_work().scan_bytes(raw.len())?;
    s.semantic_work().scan_bytes(canonical.bytes().len())?;
    if raw != canonical.bytes() {
        return Err(bad().into());
    }
    Ok(v)
}
impl SqliteTx {
    fn statement_start(&mut self) -> Result<()> {
        if self.failed || self.workflow_proof_failed.load(Ordering::Acquire) {
            return Err(bad().into());
        }
        self.failed = true;
        Ok(())
    }
    fn statement_finish<T>(
        &mut self,
        result: Result<T>,
        s: &mut OriginalAuditSession,
    ) -> Result<T> {
        if result.is_ok() {
            s.check_cancelled()?;
            if self.workflow_proof_failed.load(Ordering::Acquire) || self.financial_io.is_some() {
                return Err(bad().into());
            }
            self.failed = false;
        }
        result
    }
    async fn statement_document(
        &mut self,
        r: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<AuditedValue> {
        let k = txt(&r["kind"])?;
        let id = txt(&r["id"])?;
        let digest = txt(&r["digest"])?;
        let bound = [k.as_bytes(), id.as_bytes(), digest.as_bytes()];
        let payload = io::binding_payload(&bound, &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work().visit(Visit::RefLookup)?;
        let (count,n):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(length(canonical_bytes)),0) FROM workflow_documents WHERE kind=? AND id=? AND digest=?")
   .bind(k).bind(id).bind(digest).fetch_one(self.conn()).await?;
        self.financial_finish_io(s).await?;
        if count != 1 {
            return Err(bad().into());
        }
        let n = length(n, 4194304, s)?;
        let materialized = add(mul(n, 3, s)?, 2 * std::mem::size_of::<Vec<u8>>(), s)?;
        let payload = io::binding_payload(&bound, &[], 0, materialized, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work().visit(Visit::RefLookup)?;
        let rows:Vec<Vec<u8>>=sqlx::query_scalar("SELECT canonical_bytes FROM workflow_documents WHERE kind=? AND id=? AND digest=? LIMIT 2")
   .bind(k).bind(id).bind(digest).fetch_all(self.conn()).await?;
        if rows.len() != 1 || rows[0].len() != n {
            return Err(bad().into());
        }
        let owned = value(&rows[0], RepresentationKind::Record, s)?;
        drop(rows);
        self.financial_finish_io(s).await?;
        Ok(owned)
    }
    async fn statement_origin(
        &mut self,
        line: &Value,
        statement: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<(i64, AuditedValue, AuditedValue)> {
        let scope = &statement["workflow_boundary"];
        let store = txt(&scope["store"])?;
        let tenant = txt(&scope["scope"][0])?;
        let environment = txt(&scope["scope"][1])?;
        let id = txt(&line["producer_receipt_ref"]["id"])?;
        let digest = txt(&line["producer_receipt_ref"]["digest"])?;
        let bound = [
            store.as_bytes(),
            tenant.as_bytes(),
            environment.as_bytes(),
            id.as_bytes(),
            digest.as_bytes(),
        ];
        let payload = io::binding_payload(&bound, &[], 0, 0, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work().visit(Visit::RefLookup)?;
        let (count,opn,recn):(i64,i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(length(operation_bytes)),0),COALESCE(max(length(receipt_bytes)),0) FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND receipt_id=? AND receipt_digest=?")
   .bind(store).bind(tenant).bind(environment).bind(id).bind(digest).fetch_one(self.conn()).await?;
        self.financial_finish_io(s).await?;
        if count != 1 {
            return Err(bad().into());
        }
        let opn = length(opn, 8388608, s)?;
        let recn = length(recn, 1048576, s)?;
        let size = add(opn, recn, s)?;
        let materialized = add(
            mul(size, 3, s)?,
            2 * std::mem::size_of::<(i64, Vec<u8>, Vec<u8>)>(),
            s,
        )?;
        let payload = io::binding_payload(&bound, &[], 0, materialized, s)?;
        self.financial_reserve_io(s, payload)?;
        s.semantic_work().visit(Visit::RefLookup)?;
        let rows:Vec<(i64,Vec<u8>,Vec<u8>)>=sqlx::query_as("SELECT commit_sequence,operation_bytes,receipt_bytes FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND receipt_id=? AND receipt_digest=? LIMIT 2")
   .bind(store).bind(tenant).bind(environment).bind(id).bind(digest).fetch_all(self.conn()).await?;
        if rows.len() != 1 || rows[0].1.len() != opn || rows[0].2.len() != recn {
            return Err(bad().into());
        }
        let seq = rows[0].0;
        let op = value(&rows[0].1, RepresentationKind::OperationFrame, s)?;
        let receipt = value(&rows[0].2, RepresentationKind::ReceiptFrame, s)?;
        drop(rows);
        self.financial_finish_io(s).await?;
        Ok((seq, op, receipt))
    }
    async fn statement_checked(
        &mut self,
        statement: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        // Header SQL returns only scalar boundary/period tuples. These are new
        // bounded custody checks, distinct from the completed producer proof.
        let bound = [
            txt(&statement["customer"])?.as_bytes(),
            statement["boundary_resolution_id"]
                .as_str()
                .unwrap_or("")
                .as_bytes(),
            statement["timezone"].as_str().unwrap_or("").as_bytes(),
            statement["timezone_rules_version"]
                .as_str()
                .unwrap_or("")
                .as_bytes(),
            statement["boundary_rule_version"]
                .as_str()
                .unwrap_or("")
                .as_bytes(),
            txt(&statement["customer"])?.as_bytes(),
        ];
        // The equality query returns scalars, but SQLite's JSON functions read
        // the two retained term BLOBs. Keep their existing 256KiB source bounds
        // and parser/result overlap admitted until the worker acknowledges.
        let payload = io::binding_payload(&bound, &[], 0, 6 * 262144 + 128, s)?;
        self.financial_reserve_io(s, payload)?;
        check_statement_header(self.conn(), statement).await?;
        self.financial_finish_io(s).await?;
        for line in statement["lines"].as_array().ok_or_else(bad)? {
            s.semantic_work().visit(Visit::Element)?;
            if !workflow_line(line) {
                continue;
            }
            let a = self
                .statement_document(&line["source_record_ref"], s)
                .await?;
            let d = self.statement_document(&line["decision_ref"], s).await?;
            let e = self.statement_document(&line["evaluation_ref"], s).await?;
            let b = self.statement_document(&line["binding_ref"], s).await?;
            let (seq, op, receipt) = self.statement_origin(line, statement, s).await?;
            check_line_loaded(
                line,
                statement,
                [a.value(), d.value(), e.value(), b.value()],
                (seq, op.value(), receipt.value()),
            )?;
        }
        Ok(())
    }
    pub(crate) async fn workflow_statement_claimed(
        &mut self,
        customer: &str,
        scope: &Value,
        r: &Value,
        recovery: Option<(&str, i64)>,
        s: &mut OriginalAuditSession,
    ) -> Result<bool> {
        self.statement_start()?;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
   let store=txt(&scope["store"])?;let tenant=txt(&scope["scope"][0])?;let environment=txt(&scope["scope"][1])?;
   let kind=txt(&r["kind"])?;let id=txt(&r["id"])?;let digest=txt(&r["digest"])?;
   let bound=[customer.as_bytes(),store.as_bytes(),tenant.as_bytes(),environment.as_bytes(),kind.as_bytes(),id.as_bytes(),digest.as_bytes()];
   let payload=io::binding_payload(&bound,&[],0,0,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   let count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_statement_membership WHERE customer=? AND logical_store_id=? AND tenant=? AND environment=? AND action_kind=? AND action_id=? AND action_digest=?")
    .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
   self.financial_finish_io(s).await?;
   if count==0{return Ok(false)}
   if count!=1{return Err(bad().into())}
   let payload=io::binding_payload(&bound,&[],0,0,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   let (joined,cn,sn):(i64,i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(length(r.payload_bytes)),0),COALESCE(max(length(c.response_bytes)),0) FROM workflow_statement_membership p JOIN billing_m5_records r ON r.sequence=p.claim_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE p.customer=? AND p.logical_store_id=? AND p.tenant=? AND p.environment=? AND p.action_kind=? AND p.action_id=? AND p.action_digest=?")
    .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
   self.financial_finish_io(s).await?;
   if joined!=count{return Err(bad().into())}let cn=length(cn,262144,s)?;let sn=length(sn,262144,s)?;
   let materialized=add(mul(add(cn,sn,s)?,3,s)?,2*std::mem::size_of::<(i64,Vec<u8>,Vec<u8>)>(),s)?;
   let payload=io::binding_payload(&bound,&[],0,materialized,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   let rows:Vec<(i64,Vec<u8>,Vec<u8>)>=sqlx::query_as("SELECT r.sequence,r.payload_bytes,c.response_bytes FROM workflow_statement_membership p JOIN billing_m5_records r ON r.sequence=p.claim_sequence JOIN billing_m5_commands c ON c.command_sequence=r.command_sequence WHERE p.customer=? AND p.logical_store_id=? AND p.tenant=? AND p.environment=? AND p.action_kind=? AND p.action_id=? AND p.action_digest=? LIMIT 2")
    .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).fetch_all(self.conn()).await?;
   if rows.len()!=1||rows[0].1.len()!=cn||rows[0].2.len()!=sn{return Err(bad().into())}
   let sequence=rows[0].0;let claim=value(&rows[0].1,RepresentationKind::Record,s)?;let statement=value(&rows[0].2,RepresentationKind::Record,s)?;
   drop(rows);self.financial_finish_io(s).await?;
   let c=claim.value();let v=statement.value();
   check_claim(c,v)?;
   if c["customer"]!=customer||c["workflow_scope"]!=*scope||c["source_record_ref"]!=*r||number(&c["record"]["sequence"])?!=sequence||!v["lines"].as_array().is_some_and(|lines|lines.iter().any(|line|line["source_record_ref"]==*r)){return Err(bad().into())}
   self.statement_checked(v,s).await?;
   if let Some((own,cutoff))=recovery {
    let hash=txt(&v["statement_hash"])?;
    s.semantic_work().visit(Visit::MemberCompare)?;s.semantic_work().scan_bytes(hash.len())?;s.semantic_work().scan_bytes(own.len())?;
    if hash==own{return Ok(false)}
    s.semantic_work().arithmetic(Op::Compare,&[&sequence.into(),&cutoff.into()])?;
    if sequence>cutoff{return Ok(false)}
   }
   Ok(true)
  }).await.map_err(|_|self.workflow_timeout_error())?;
        self.statement_finish(result, s)
    }
    pub(crate) async fn workflow_statement_original_close(
        &mut self,
        customer: &str,
        at: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AuditedValue>> {
        self.statement_start()?;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
   let bound=[customer.as_bytes()];let payload=io::binding_payload(&bound,&[],0,0,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   let (count,n):(i64,i64)=sqlx::query_as("SELECT count(*),COALESCE(max(length(c.statement_bytes)),0) FROM billing_m5_period_closes c JOIN billing_m5_period_resolutions r ON r.customer=c.customer AND r.term_version=c.term_version AND r.period_index=c.period_index AND r.resolution_id=c.boundary_resolution_id WHERE c.customer=? AND r.start_at_us<=? AND r.end_at_us>?")
    .bind(customer).bind(at).bind(at).fetch_one(self.conn()).await?;
   self.financial_finish_io(s).await?;
   if count==0{return Ok(None)}
   if count!=1{return Err(bad().into())}let n=length(n,262144,s)?;
   let materialized=add(mul(n,3,s)?,2*std::mem::size_of::<Vec<u8>>(),s)?;
   let payload=io::binding_payload(&bound,&[],0,materialized,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   let rows:Vec<Vec<u8>>=sqlx::query_scalar("SELECT c.statement_bytes FROM billing_m5_period_closes c JOIN billing_m5_period_resolutions r ON r.customer=c.customer AND r.term_version=c.term_version AND r.period_index=c.period_index AND r.resolution_id=c.boundary_resolution_id WHERE c.customer=? AND r.start_at_us<=? AND r.end_at_us>? LIMIT 2")
    .bind(customer).bind(at).bind(at).fetch_all(self.conn()).await?;
   if rows.len()!=1||rows[0].len()!=n{return Err(bad().into())}let owned=value(&rows[0],RepresentationKind::Record,s)?;
   drop(rows);self.financial_finish_io(s).await?;Ok(Some(owned))
  }).await.map_err(|_|self.workflow_timeout_error())?;
        self.statement_finish(result, s)
    }
}

impl SqliteTx {
    pub(crate) async fn workflow_statement_append(
        &mut self,
        prepared: &crate::billing::workflow_statement::PreparedWorkflowPresentation<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.statement_start()?;
        let result=crate::store::sqlite::workflow::until(self.workflow_deadline(),async {
   let (command,statement)=prepared.parts(self.workflow_proof_failure_handle().identity())?;
   self.statement_checked(statement,s).await?;
   let is_standard=standard(statement)?;let (presentation_kind,presentation_id)=presentation(statement)?;
   let legacy_expected=prepared.legacy_claims(self.workflow_proof_failure_handle().identity())?;
   let request=s.parse_representation(command.request,RepresentationKind::Record)?;let request=request.value();
   let request_fields:&[&str]=if is_standard{&["schema","customer","period_id","workflow_scope"]}else{&["schema","customer","command_id","workflow_scope","adjustments","workflow_actions"]};
   closed(request,request_fields)?;
   if command.family!=if is_standard{CLOSE}else{AD_HOC}||command.domain!="customer-admin"||command.customer!=Some(txt(&statement["customer"])? )||command.source.is_some()
    ||request["schema"]!=command.family||request["customer"]!=statement["customer"]||request["workflow_scope"]["store"]!=statement["workflow_boundary"]["store"]||request["workflow_scope"]["scope"]!=statement["workflow_boundary"]["scope"]{return Err(bad().into())}
   if is_standard {
    if request["period_id"]!=statement["period_id"]||command.accepted_at_us!=ledgerlab_core::domain::Timestamp::parse(txt(&statement["close_acceptance_time"])?).map_err(|_|bad())?.micros(){return Err(bad().into())}
   }else if request["command_id"]!=statement["statement_id"]||request["adjustments"]!=statement["adjustments"]||request["workflow_actions"]!=statement["workflow_actions"]{return Err(bad().into())}
   // Bound the same borrowed payloads before invoking the unchanged M5 writer.
   // Fixed generated IDs/digests are 68/32 bytes. Additional repeated packed
   // copies cover row/index records; this is not a claim to meter all M5 logic.
   if command.children.is_empty()||command.children.len()>100000||command.identity_key.len()>4096||command.request.len()>262144||command.response.len()>262144{return Err(StoreError::BillingHistoryLimit.into())}
   let slots=add(mul(command.children.len(),7,s)?,7,s)?;
   let controls_bytes=mul(slots,std::mem::size_of::<&[u8]>(),s)?;
   let _controls=s.reserve_external_payload(controls_bytes)?;s.semantic_work().write_bytes(controls_bytes)?;
   let mut bound:Vec<&[u8]>=Vec::new();bound.try_reserve_exact(slots).map_err(|_|bad())?;
   if bound.capacity()>slots{return Err(bad().into())}
   let id_bytes=[0u8;68];let digest_bytes=[0u8;32];
   bound.extend([command.family.as_bytes(),command.domain.as_bytes(),command.customer.unwrap_or("").as_bytes(),command.source.unwrap_or("").as_bytes(),command.identity_key,command.request,command.response]);
   let mut materialized=64usize;
   for child in command.children {
    s.semantic_work().visit(Visit::Element)?;
    if child.payload.len()>262144||child.child_key.len()>4096{return Err(StoreError::BillingHistoryLimit.into())}
    bound.extend([child.family.as_bytes(),child.customer.unwrap_or("").as_bytes(),child.source.unwrap_or("").as_bytes(),child.child_key,child.payload,&id_bytes,&digest_bytes]);
    materialized=add(materialized,add(child.payload.len(),child.child_key.len(),s)?,s)?;
   }
   // Explicit extra packed/index overlap, proportional to actual command bytes.
   materialized=mul(materialized,3,s)?;
   let ints=add(mul(command.children.len(),3,s)?,12,s)?;
   let payload=io::binding_payload(&bound,&bound,ints,materialized,s)?;
   self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   m5::append(self.conn(),command).await?;
   self.financial_finish_io(s).await?;
   // M5 append has checked each exact child envelope and successfully inserted
   // its literal bytes. Reuse that same prepared source, rather than rereading
   // and materializing an unbounded post-append result set.
   let count=command.children.len();let source_controls=mul(count,std::mem::size_of::<ledgerlab_core::canonical::audit_arena::AuditedBytes>(),s)?;
   let _sources=s.reserve_external_payload(source_controls)?;s.semantic_work().write_bytes(source_controls)?;
   let mut sources=Vec::new();sources.try_reserve_exact(count).map_err(|_|bad())?;if sources.capacity()>count{return Err(bad().into())}
   let mut close=None;let mut legacy_count=0usize;
   let legacy_controls=mul(legacy_expected.len(),std::mem::size_of::<u8>(),s)?;
   let _legacy_controls=s.reserve_external_payload(legacy_controls)?;s.semantic_work().write_bytes(legacy_controls)?;
   let mut legacy_seen=Vec::new();legacy_seen.try_reserve_exact(legacy_controls).map_err(|_|bad())?;if legacy_seen.capacity()>legacy_controls{return Err(bad().into())}legacy_seen.resize(legacy_controls,0u8);
   for child in command.children {
    s.semantic_work().visit(Visit::Element)?;
    let owner=value(child.payload,RepresentationKind::Record,s)?;let v=owner.value();
    let record_seq=number(&v["record"]["sequence"])?;
    if child.family==if is_standard{RECORD}else{AD_HOC_RECORD} {
     if close.replace(record_seq).is_some()||v["statement_hash"]!=statement["statement_hash"]||v["workflow_boundary"]!=statement["workflow_boundary"]||v["lines"]!=statement["lines"]||v["totals"]!=statement["totals"] {return Err(bad().into())}
    } else if child.family==CLAIM {
     let r=&v["source_record_ref"];let scope=&v["workflow_scope"];
     check_claim(v,statement)?;
     let key=value(child.child_key,RepresentationKind::Record,s)?;check_workflow_claim_key(key.value(),v)?;
     let source=s.canonical_representation(r,RepresentationKind::Record)?;
     for prior in &sources {
      s.semantic_work().visit(Visit::MemberCompare)?;
      let prior:&ledgerlab_core::canonical::audit_arena::AuditedBytes=prior;
      s.semantic_work().scan_bytes(prior.bytes().len())?;s.semantic_work().scan_bytes(source.bytes().len())?;
      if prior.bytes()==source.bytes(){return Err(bad().into())}
     }
     sources.push(source);
     let customer=txt(&v["customer"])?;let store=txt(&scope["store"])?;let tenant=txt(&scope["scope"][0])?;let environment=txt(&scope["scope"][1])?;
     let kind=txt(&r["kind"])?;let id=txt(&r["id"])?;let digest=txt(&r["digest"])?;let presentation=txt(&v["presentation_kind"])?;let statement_id=txt(&v["statement_id"])?;let hash=txt(&v["statement_hash"])?;
     let bound=[customer.as_bytes(),store.as_bytes(),tenant.as_bytes(),environment.as_bytes(),kind.as_bytes(),id.as_bytes(),digest.as_bytes(),presentation.as_bytes(),statement_id.as_bytes(),hash.as_bytes()];
     // Primary key and unique claim_sequence index retain these exact keys.
     let payload=io::binding_payload(&bound,&bound,3,source_controls,s)?;self.financial_reserve_io(s,payload)?;
     s.semantic_work().visit(Visit::RefLookup)?;
     sqlx::query("INSERT INTO workflow_statement_membership(customer,logical_store_id,tenant,environment,action_kind,action_id,action_digest,claim_sequence,presentation_kind,statement_id,statement_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
      .bind(customer).bind(store).bind(tenant).bind(environment).bind(kind).bind(id).bind(digest).bind(record_seq).bind(presentation).bind(statement_id).bind(hash).execute(self.conn()).await?;
     self.financial_finish_io(s).await?;
    } else if child.family==LEGACY_CLAIM {
     check_legacy_claim(v,statement)?;
     let mut matched=None;
     for (index,r) in legacy_expected.iter().enumerate(){
      s.semantic_work().visit(Visit::MemberCompare)?;
      let mut equal=true;
      for (k,b) in [("source",r.source.as_str()),("adjustment_id",r.adjustment_id.as_str())] {
       let a=txt(&v[k])?;s.semantic_work().scan_bytes(a.len())?;s.semantic_work().scan_bytes(b.len())?;equal&=a==b;
      }
      if equal && matched.replace(index).is_some(){return Err(bad().into())}
     }
     let index=matched.ok_or_else(bad)?;if legacy_seen[index]!=0{return Err(bad().into())}legacy_seen[index]=1;legacy_count=add(legacy_count,1,s)?;
     let customer=txt(&v["customer"])?;let source=txt(&v["source"])?;let id=txt(&v["adjustment_id"])?;
     let bound=[customer.as_bytes(),source.as_bytes(),id.as_bytes(),presentation_kind.as_bytes(),presentation_id.as_bytes()];
     let payload=io::binding_payload(&bound,&bound,3,0,s)?;self.financial_reserve_io(s,payload)?;s.semantic_work().visit(Visit::RefLookup)?;
     sqlx::query("INSERT INTO billing_m5_presentation_claims(customer,source_scope,adjustment_id,presentation_kind,statement_id,record_sequence) VALUES(?,?,?,?,?,?)")
      .bind(customer).bind(source).bind(id).bind(presentation_kind).bind(presentation_id).bind(record_seq).execute(self.conn()).await?;
     self.financial_finish_io(s).await?;
    } else {return Err(bad().into())}
   }
   if close.is_none()||legacy_count!=legacy_seen.len(){return Err(bad().into())}
   let lines=statement["lines"].as_array().ok_or_else(bad)?;
   let mut workflow_count=0usize;
   for line in lines {
    s.semantic_work().visit(Visit::Element)?;
    if !workflow_line(line){continue;}workflow_count=add(workflow_count,1,s)?;
    let expected=s.canonical_representation(&line["source_record_ref"],RepresentationKind::Record)?;
    let mut found=false;
    for source in &sources {
     s.semantic_work().visit(Visit::MemberCompare)?;s.semantic_work().scan_bytes(source.bytes().len())?;s.semantic_work().scan_bytes(expected.bytes().len())?;
     if source.bytes()==expected.bytes(){found=true;break;}
    }if !found{return Err(bad().into())}
   }
   if sources.len()!=workflow_count{return Err(bad().into())}
   if is_standard {
   let customer=txt(&statement["customer"])?;let boundary=txt(&statement["boundary_resolution_id"])?;let hash=txt(&statement["statement_hash"])?;
   let bound=[customer.as_bytes(),boundary.as_bytes(),hash.as_bytes(),command.response];
   let payload=io::binding_payload(&bound,&bound,12,0,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   sqlx::query("INSERT INTO billing_m5_period_closes(customer,term_version,period_index,boundary_resolution_id,snapshot_boundary_id,m3_high_water,m5_high_water,statement_hash,close_sequence,statement_bytes) VALUES(?,?,?,?,?,?,?,?,?,?)")
    .bind(customer).bind(number(&statement["period_id"]["term_version"])?).bind(number(&statement["period_id"]["period_index"])?)
    .bind(boundary).bind(number(&statement["snapshot_boundary_id"])?).bind(number(&statement["m3_high_water"])?).bind(number(&statement["m5_high_water"])?)
    .bind(hash).bind(close.ok_or_else(bad)?).bind(command.response).execute(self.conn()).await?;
   self.financial_finish_io(s).await?;
   }
   let payload=io::binding_payload(&[],&[],8,128,s)?;self.financial_reserve_io(s,payload)?;
   s.semantic_work().visit(Visit::RefLookup)?;
   m5::append_boundary(self.conn()).await?;self.financial_finish_io(s).await?;Ok(())
  }).await.map_err(|_|self.workflow_timeout_error())?;
        self.statement_finish(result, s)
    }
}
