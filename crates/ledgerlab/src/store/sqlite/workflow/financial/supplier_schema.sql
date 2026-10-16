-- Additive supplier storage upgrade. Schemas12 and13 remain immutable.
-- Foreign keys are deferred only within the existing held writer transaction.
PRAGMA defer_foreign_keys=ON;

CREATE TABLE workflow_documents_supplier_upgrade (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest','workflow-evidence','workflow-customer','workflow-source-enrollment','workflow-source-grant','workflow-source-state','workflow-definition-fragment','workflow-roles','workflow-slot-catalog','workflow-slot-instantiation','workflow-slot-terms','workflow-acceptance-terms','workflow-assent','workflow-binding','workflow-accepted-agreement','workflow-head-transition','workflow-agreement-intent','workflow-activation','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-offer','workflow-invocation','workflow-cost')),
 id TEXT PRIMARY KEY, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND
   CASE WHEN kind IN ('workflow-compiled-plan','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-invocation','workflow-cost') THEN 4194304 ELSE 1048576 END),
 UNIQUE(kind,id,digest)
) STRICT;
INSERT INTO workflow_documents_supplier_upgrade SELECT * FROM workflow_documents;
DROP TABLE workflow_documents;
ALTER TABLE workflow_documents_supplier_upgrade RENAME TO workflow_documents;
CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TABLE workflow_operations_supplier_upgrade (
 identity_key BLOB PRIMARY KEY,
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, operation_id TEXT NOT NULL,
 operation_record_id TEXT NOT NULL UNIQUE, operation_digest TEXT NOT NULL,
 operation_bytes BLOB NOT NULL CHECK(length(operation_bytes) BETWEEN 1 AND 8388608),
 normalized_request_hash TEXT NOT NULL,
 manifest_id TEXT NOT NULL UNIQUE, manifest_digest TEXT NOT NULL,
 manifest_bytes BLOB NOT NULL CHECK(length(manifest_bytes) BETWEEN 1 AND 4194304),
 receipt_id TEXT NOT NULL UNIQUE, receipt_digest TEXT NOT NULL,
 receipt_bytes BLOB NOT NULL CHECK(length(receipt_bytes) BETWEEN 1 AND 1048576),
 publication_id TEXT REFERENCES workflow_documents(id),
 plan_id TEXT REFERENCES workflow_documents(id),
 commit_sequence INTEGER NOT NULL UNIQUE CHECK(commit_sequence BETWEEN 1 AND 100000),
 accepted_at_us INTEGER NOT NULL,
 operation_kind TEXT NOT NULL CHECK(operation_kind IN ('policy.publish','evidence.retain','authority.change','agreement.accept','agreement.activate','agreement.revoke','work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert','invocation.authorize','invocation.close','cost.finalize','cost.correct')),
 CHECK((operation_kind='policy.publish' AND publication_id IS NOT NULL AND plan_id IS NOT NULL)
    OR (operation_kind<>'policy.publish' AND publication_id IS NULL AND plan_id IS NULL)),
 UNIQUE(logical_store_id,tenant,environment,source,operation_id)
) STRICT;
INSERT INTO workflow_operations_supplier_upgrade SELECT * FROM workflow_operations;
DROP TABLE workflow_operations;
ALTER TABLE workflow_operations_supplier_upgrade RENAME TO workflow_operations;
CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_operations_delete_immutable BEFORE DELETE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TABLE workflow_financial_heads_supplier_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('decision','family','chain','capacity','invocation','cost','cost_consumption')),
 key_a TEXT NOT NULL, key_b TEXT NOT NULL, key_c TEXT NOT NULL, key_d TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(length(revision) BETWEEN 1 AND 20 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<20 OR revision<='18446744073709551615')),
 record_kind TEXT NOT NULL, record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 CHECK((head_kind IN ('decision','capacity','invocation') AND key_a<>'' AND key_b='' AND key_c='' AND key_d='') OR (head_kind='chain' AND key_a<>'' AND key_b<>'' AND key_c='' AND key_d='') OR (head_kind IN ('cost','cost_consumption') AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d='') OR (head_kind='family' AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d<>'')),
 CHECK((head_kind='decision' AND record_kind='workflow-decision') OR (head_kind='family' AND record_kind='workflow-family-state') OR (head_kind='chain' AND record_kind='workflow-chain-state') OR (head_kind='capacity' AND record_kind='workflow-correction-capacity') OR (head_kind='invocation' AND record_kind='workflow-invocation') OR (head_kind IN ('cost','cost_consumption') AND record_kind='workflow-cost')),
 PRIMARY KEY(logical_store_id,tenant,environment,head_kind,key_a,key_b,key_c,key_d),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
INSERT INTO workflow_financial_heads_supplier_upgrade SELECT * FROM workflow_financial_heads;
DROP TABLE workflow_financial_heads;
ALTER TABLE workflow_financial_heads_supplier_upgrade RENAME TO workflow_financial_heads;
CREATE TRIGGER workflow_financial_heads_no_delete BEFORE DELETE ON workflow_financial_heads BEGIN SELECT RAISE(ABORT,'permanent financial head lineage'); END;
CREATE TRIGGER workflow_financial_heads_fixed_key BEFORE UPDATE ON workflow_financial_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.head_kind<>OLD.head_kind OR NEW.key_a<>OLD.key_a OR NEW.key_b<>OLD.key_b OR NEW.key_c<>OLD.key_c OR NEW.key_d<>OLD.key_d
BEGIN SELECT RAISE(ABORT,'immutable financial head key'); END;
-- Installation-wide account and line ownership; deliberately no scope prefix.
CREATE TABLE workflow_supplier_account_enrollments (
 provider_namespace TEXT NOT NULL,provider_account_id TEXT NOT NULL,provider_environment TEXT NOT NULL,
 authority_kind TEXT NOT NULL CHECK(authority_kind='workflow-authority'),authority_id TEXT NOT NULL,authority_digest TEXT NOT NULL,
 activation_kind TEXT NOT NULL CHECK(activation_kind='workflow-authority'),activation_id TEXT NOT NULL,activation_digest TEXT NOT NULL,
 PRIMARY KEY(provider_namespace,provider_account_id,provider_environment),
 FOREIGN KEY(authority_kind,authority_id,authority_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(activation_kind,activation_id,activation_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_supplier_cost_line_claims (
 cost_authority_id TEXT NOT NULL, provider_namespace TEXT NOT NULL, provider_account_id TEXT NOT NULL, provider_environment TEXT NOT NULL, original_document_id TEXT NOT NULL, original_line_id TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(cost_authority_id,provider_namespace,provider_account_id,provider_environment,original_document_id,original_line_id),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_supplier_cost_component_claims (
 logical_store_id TEXT NOT NULL,tenant TEXT NOT NULL,environment TEXT NOT NULL,original_work_claim_id TEXT NOT NULL,supplier_component_slot TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,original_work_claim_id,supplier_component_slot),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_supplier_cost_owner_heads (
 cost_authority_id TEXT NOT NULL, provider_namespace TEXT NOT NULL, provider_account_id TEXT NOT NULL, provider_environment TEXT NOT NULL, original_document_id TEXT NOT NULL, original_line_id TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(length(revision) BETWEEN 1 AND 20 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<20 OR revision<='18446744073709551615')),
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(cost_authority_id,provider_namespace,provider_account_id,provider_environment,original_document_id,original_line_id),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_supplier_account_enrollments_update_immutable BEFORE UPDATE ON workflow_supplier_account_enrollments BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_account_enrollments_delete_immutable BEFORE DELETE ON workflow_supplier_account_enrollments BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_cost_line_claims_update_immutable BEFORE UPDATE ON workflow_supplier_cost_line_claims BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_cost_line_claims_delete_immutable BEFORE DELETE ON workflow_supplier_cost_line_claims BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_cost_component_claims_update_immutable BEFORE UPDATE ON workflow_supplier_cost_component_claims BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_cost_component_claims_delete_immutable BEFORE DELETE ON workflow_supplier_cost_component_claims BEGIN SELECT RAISE(ABORT,'immutable supplier projection'); END;
CREATE TRIGGER workflow_supplier_cost_owner_heads_no_delete BEFORE DELETE ON workflow_supplier_cost_owner_heads BEGIN SELECT RAISE(ABORT,'permanent supplier ownership lineage'); END;
CREATE TRIGGER workflow_supplier_cost_owner_heads_fixed_key BEFORE UPDATE ON workflow_supplier_cost_owner_heads WHEN NEW.cost_authority_id<>OLD.cost_authority_id OR NEW.provider_namespace<>OLD.provider_namespace OR NEW.provider_account_id<>OLD.provider_account_id OR NEW.provider_environment<>OLD.provider_environment OR NEW.original_document_id<>OLD.original_document_id OR NEW.original_line_id<>OLD.original_line_id BEGIN SELECT RAISE(ABORT,'immutable supplier line key'); END;
