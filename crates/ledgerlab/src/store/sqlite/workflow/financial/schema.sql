-- Explicit financial storage upgrade. Schema-12 migration remains immutable.
-- Foreign keys are deferred only within the existing held writer transaction.
PRAGMA defer_foreign_keys=ON;

CREATE TABLE workflow_documents_financial_upgrade (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest','workflow-evidence','workflow-customer','workflow-source-enrollment','workflow-source-grant','workflow-source-state','workflow-definition-fragment','workflow-roles','workflow-slot-catalog','workflow-slot-instantiation','workflow-slot-terms','workflow-acceptance-terms','workflow-assent','workflow-binding','workflow-accepted-agreement','workflow-head-transition','workflow-agreement-intent','workflow-activation','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement')),
 id TEXT PRIMARY KEY, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND
   CASE WHEN kind IN ('workflow-compiled-plan','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement') THEN 4194304 ELSE 1048576 END),
 UNIQUE(kind,id,digest)
) STRICT;
INSERT INTO workflow_documents_financial_upgrade SELECT * FROM workflow_documents;
DROP TABLE workflow_documents;
ALTER TABLE workflow_documents_financial_upgrade RENAME TO workflow_documents;
CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TABLE workflow_operations_financial_upgrade (
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
 operation_kind TEXT NOT NULL CHECK(operation_kind IN ('policy.publish','evidence.retain','authority.change','agreement.accept','agreement.activate','agreement.revoke','work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert')),
 CHECK((operation_kind='policy.publish' AND publication_id IS NOT NULL AND plan_id IS NOT NULL)
    OR (operation_kind<>'policy.publish' AND publication_id IS NULL AND plan_id IS NULL)),
 UNIQUE(logical_store_id,tenant,environment,source,operation_id)
) STRICT;
INSERT INTO workflow_operations_financial_upgrade SELECT * FROM workflow_operations;
DROP TABLE workflow_operations;
ALTER TABLE workflow_operations_financial_upgrade RENAME TO workflow_operations;
CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_operations_delete_immutable BEFORE DELETE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

-- Derived candidates only. Each referenced body still requires the full
-- original-certificate/prefix and same-constructor validation by the host.
CREATE TABLE workflow_financial_heads (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('decision','family','chain','capacity')),
 key_a TEXT NOT NULL, key_b TEXT NOT NULL, key_c TEXT NOT NULL, key_d TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(length(revision) BETWEEN 1 AND 20 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<20 OR revision<='18446744073709551615')),
 record_kind TEXT NOT NULL, record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 CHECK((head_kind IN ('decision','capacity') AND key_a<>'' AND key_b='' AND key_c='' AND key_d='') OR (head_kind='chain' AND key_a<>'' AND key_b<>'' AND key_c='' AND key_d='') OR (head_kind='family' AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d<>'')),
 CHECK((head_kind='decision' AND record_kind='workflow-decision') OR (head_kind='family' AND record_kind='workflow-family-state') OR (head_kind='chain' AND record_kind='workflow-chain-state') OR (head_kind='capacity' AND record_kind='workflow-correction-capacity')),
 PRIMARY KEY(logical_store_id,tenant,environment,head_kind,key_a,key_b,key_c,key_d),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_financial_work_claims (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source_namespace TEXT NOT NULL, business_operation_id TEXT NOT NULL, completion TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-work-claim'), record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,source_namespace,business_operation_id,completion),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_financial_outcome_claims (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, family_slot TEXT NOT NULL, target_id TEXT NOT NULL,
 source_namespace TEXT NOT NULL, business_outcome_token TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-outcome-claim'), record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,agreement_id,family_slot,target_id),
 UNIQUE(logical_store_id,tenant,environment,source_namespace,family_slot,business_outcome_token),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_financial_links (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 child_kind TEXT NOT NULL, child_id TEXT NOT NULL, child_digest TEXT NOT NULL, slot TEXT NOT NULL,
 predecessor_kind TEXT NOT NULL, predecessor_id TEXT NOT NULL, predecessor_digest TEXT NOT NULL,
 product_kind TEXT NOT NULL, product_id TEXT NOT NULL, product_digest TEXT NOT NULL,
 binding_kind TEXT NOT NULL, binding_id TEXT NOT NULL, binding_digest TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-link'), record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,child_kind,child_id,child_digest,slot),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(child_kind,child_id,child_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(predecessor_kind,predecessor_id,predecessor_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(product_kind,product_id,product_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(binding_kind,binding_id,binding_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TABLE workflow_financial_dependencies (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 ancestor_decision_id TEXT NOT NULL, dependent_decision_id TEXT NOT NULL,
 dependency_kind TEXT NOT NULL CHECK(dependency_kind IN ('outcome_family','work_basis','delayed_cost')),
 target_kind TEXT NOT NULL, target_id TEXT NOT NULL, target_digest TEXT NOT NULL,
 family_slot TEXT NOT NULL,
 terms_kind TEXT NOT NULL, terms_id TEXT NOT NULL, terms_digest TEXT NOT NULL,
 capacity_kind TEXT NOT NULL, capacity_id TEXT NOT NULL, capacity_digest TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-dependency'), record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 PRIMARY KEY(record_kind,record_id,record_digest),
 UNIQUE(logical_store_id,tenant,environment,ancestor_decision_id,dependent_decision_id,dependency_kind,target_kind,target_id,target_digest,family_slot),
 FOREIGN KEY(target_kind,target_id,target_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(terms_kind,terms_id,terms_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(capacity_kind,capacity_id,capacity_digest) REFERENCES workflow_documents(kind,id,digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_financial_heads_no_delete BEFORE DELETE ON workflow_financial_heads BEGIN SELECT RAISE(ABORT,'permanent financial head lineage'); END;
CREATE TRIGGER workflow_financial_heads_fixed_key BEFORE UPDATE ON workflow_financial_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.head_kind<>OLD.head_kind OR NEW.key_a<>OLD.key_a OR NEW.key_b<>OLD.key_b OR NEW.key_c<>OLD.key_c OR NEW.key_d<>OLD.key_d
BEGIN SELECT RAISE(ABORT,'immutable financial head key'); END;
CREATE TRIGGER workflow_financial_work_claims_update_immutable BEFORE UPDATE ON workflow_financial_work_claims BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_work_claims_delete_immutable BEFORE DELETE ON workflow_financial_work_claims BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_outcome_claims_update_immutable BEFORE UPDATE ON workflow_financial_outcome_claims BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_outcome_claims_delete_immutable BEFORE DELETE ON workflow_financial_outcome_claims BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_links_update_immutable BEFORE UPDATE ON workflow_financial_links BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_links_delete_immutable BEFORE DELETE ON workflow_financial_links BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_dependencies_update_immutable BEFORE UPDATE ON workflow_financial_dependencies BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
CREATE TRIGGER workflow_financial_dependencies_delete_immutable BEFORE DELETE ON workflow_financial_dependencies BEGIN SELECT RAISE(ABORT,'immutable financial projection'); END;
