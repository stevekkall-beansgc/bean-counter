//! Exact permanent WorkClaim tuple lookup under the current writer. The row is
//! DATA until its immutable original and scoped index association are checked.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::PermanentClaimKey;
const LENGTHS: &str = "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_financial_work_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND business_operation_id=? AND completion=?";
const VALUES: &str = "SELECT record_kind,record_id,record_digest FROM workflow_financial_work_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND business_operation_id=? AND completion=?";
impl SqliteTx {
    pub(crate) async fn workflow_work_claim(
        &mut self,
        key: &PermanentClaimKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned work claim read").into());
        }
        self.failed = true;
        let result = timeout_at(self.deadline, self.work_claim_inner(key, s))
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
                return Err(StoreError::Integrity("work claim read retained worker").into());
            }
            self.failed = false;
        }
        result
    }
    async fn work_claim_inner(
        &mut self,
        key: &PermanentClaimKey,
        s: &mut OriginalAuditSession,
    ) -> Result<Option<AppClaimLocator>, FinancialReadError> {
        s.semantic_work().visit(ValidationVisit::Constraint)?;
        let PermanentClaimKey::Work {
            logical_store_id,
            scope,
            source_namespace,
            business_operation_id,
            completion,
        } = key
        else {
            return Err(StoreError::Integrity("non-work permanent key").into());
        };
        for (value, maximum) in [
            (logical_store_id.as_str(), 128usize),
            (scope[0].as_str(), 128),
            (scope[1].as_str(), 128),
            (source_namespace.as_str(), 256),
            (business_operation_id.as_str(), 128),
            (completion.as_str(), 128),
        ] {
            heads::locator_text(value, maximum, s)?;
        }
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        if !resource_compare(s, version, 13)?.is_eq()
            && !resource_compare(s, version, 14)?.is_eq()
            && !resource_compare(s, version, 15)?.is_eq()
            && !resource_compare(s, version, 16)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        let values = [
            logical_store_id.as_str(),
            scope[0].as_str(),
            scope[1].as_str(),
            source_namespace.as_str(),
            business_operation_id.as_str(),
            completion.as_str(),
        ];
        let bytes = values.map(str::as_bytes);
        self.read_financial_claim_projection(LENGTHS, VALUES, &values, &bytes, s)
            .await
    }
}
impl SqliteTx {
    /// Exact immutable identity locator; positive rows still require canonical
    /// body and committed operation/manifest/receipt association verification.
    pub(crate) async fn workflow_local_record_identity(&mut self,store:&str,scope:&[String;2],kind:&str,id:&str,schema:&str,s:&mut OriginalAuditSession)->Result<Option<AppClaimLocator>,FinancialReadError>{
        if self.failed||self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire){return Err(invalid());}
        self.failed=true;
        let result=timeout_at(self.deadline,async{
            let values=[kind,id,schema,store,&scope[0],&scope[1]];
            for v in values {heads::locator_text(v,256,s)?;}
            let bound=values.map(str::as_bytes);
            self.read_financial_claim_projection(
                "SELECT octet_length(d.kind),octet_length(d.id),octet_length(d.digest) FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.schema_name=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=?)",
                "SELECT d.kind,d.id,d.digest FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.schema_name=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=?)",&values,&bound,s).await
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok(){s.check_cancelled()?;if self.financial_io.is_some()||self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire){return Err(invalid());}self.failed=false;}
        result
    }
    pub(crate) async fn workflow_outcome_claim(&mut self,store:&str,key:&PermanentClaimKey,s:&mut OriginalAuditSession)->Result<Option<AppClaimLocator>,FinancialReadError>{
        if self.failed||self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire){return Err(invalid());}
        self.failed=true;
        let result=timeout_at(self.deadline,async{
            let PermanentClaimKey::Outcome{scope,agreement_id,family_slot,target_id,source_namespace,business_outcome_token}=key else{return Err(invalid());};
            let values=[store,&scope[0],&scope[1],agreement_id,family_slot,target_id,source_namespace,business_outcome_token];
            for v in values{heads::locator_text(v,256,s)?;}
            let target=[store,scope[0].as_str(),scope[1].as_str(),agreement_id.as_str(),family_slot.as_str(),target_id.as_str()];
            let target_bound=target.map(str::as_bytes);
            let selected=self.read_financial_claim_projection(
                "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_financial_outcome_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND agreement_id=? AND family_slot=? AND target_id=?",
                "SELECT record_kind,record_id,record_digest FROM workflow_financial_outcome_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND agreement_id=? AND family_slot=? AND target_id=?",&target,&target_bound,s).await?;
            let token=[store,scope[0].as_str(),scope[1].as_str(),source_namespace.as_str(),family_slot.as_str(),business_outcome_token.as_str()];
            let token_bound=token.map(str::as_bytes);
            let other=self.read_financial_claim_projection(
                "SELECT octet_length(record_kind),octet_length(record_id),octet_length(record_digest) FROM workflow_financial_outcome_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND family_slot=? AND business_outcome_token=?",
                "SELECT record_kind,record_id,record_digest FROM workflow_financial_outcome_claims WHERE logical_store_id=? AND tenant=? AND environment=? AND source_namespace=? AND family_slot=? AND business_outcome_token=?",&token,&token_bound,s).await?;
            if let (Some(a),Some(b))=(&selected,&other){
                s.semantic_work().visit(ValidationVisit::MemberCompare)?;
                if a.kind.bytes()!=b.kind.bytes()||a.id.bytes()!=b.id.bytes()||a.digest.bytes()!=b.digest.bytes(){return Err(StoreError::DeliveryConflict.into());}
            }
            Ok(selected.or(other))
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok(){s.check_cancelled()?;if self.financial_io.is_some()||self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire){return Err(invalid());}self.failed=false;}
        result
    }
}
