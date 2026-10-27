-- Mechanical stage projections; authority and semantics remain selected-core checks.
PRAGMA defer_foreign_keys=ON;
CREATE TABLE workflow_documents_stage_upgrade (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest','workflow-evidence','workflow-customer','workflow-source-enrollment','workflow-source-grant','workflow-source-state','workflow-definition-fragment','workflow-roles','workflow-slot-catalog','workflow-slot-instantiation','workflow-slot-terms','workflow-acceptance-terms','workflow-assent','workflow-binding','workflow-accepted-agreement','workflow-head-transition','workflow-agreement-intent','workflow-activation','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-offer','workflow-invocation','workflow-cost','workflow-verification','workflow-stage-instance','workflow-stage-state','workflow-stage-closure-claim','workflow-allocation')),
 id TEXT PRIMARY KEY, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND
   CASE WHEN kind IN ('workflow-compiled-plan','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-invocation','workflow-cost','workflow-stage-instance','workflow-stage-state','workflow-stage-closure-claim','workflow-allocation') THEN 4194304 ELSE 1048576 END),
 UNIQUE(kind,id,digest)
) STRICT;
INSERT INTO workflow_documents_stage_upgrade SELECT * FROM workflow_documents;
DROP TABLE workflow_documents;
ALTER TABLE workflow_documents_stage_upgrade RENAME TO workflow_documents;
CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TABLE workflow_financial_heads_stage_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('decision','family','chain','capacity','invocation','cost','cost_consumption','app_cost','stage')),
 key_a TEXT NOT NULL, key_b TEXT NOT NULL, key_c TEXT NOT NULL, key_d TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(length(revision) BETWEEN 1 AND 20 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<20 OR revision<='18446744073709551615')),
 record_kind TEXT NOT NULL, record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 CHECK((head_kind IN ('decision','capacity','invocation') AND key_a<>'' AND key_b='' AND key_c='' AND key_d='') OR (head_kind='chain' AND key_a<>'' AND key_b<>'' AND key_c='' AND key_d='') OR (head_kind IN ('cost','cost_consumption','app_cost','stage') AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d='') OR (head_kind='family' AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d<>'')),
 CHECK((head_kind='decision' AND record_kind='workflow-decision') OR (head_kind='family' AND record_kind='workflow-family-state') OR (head_kind='chain' AND record_kind='workflow-chain-state') OR (head_kind='capacity' AND record_kind='workflow-correction-capacity') OR (head_kind='invocation' AND record_kind='workflow-invocation') OR (head_kind IN ('cost','cost_consumption','app_cost') AND record_kind='workflow-cost') OR (head_kind='stage' AND record_kind='workflow-stage-state')),
 PRIMARY KEY(logical_store_id,tenant,environment,head_kind,key_a,key_b,key_c,key_d),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
INSERT INTO workflow_financial_heads_stage_upgrade SELECT * FROM workflow_financial_heads;
DROP TABLE workflow_financial_heads;
ALTER TABLE workflow_financial_heads_stage_upgrade RENAME TO workflow_financial_heads;
CREATE TRIGGER workflow_financial_heads_no_delete BEFORE DELETE ON workflow_financial_heads BEGIN SELECT RAISE(ABORT,'permanent financial head lineage'); END;
CREATE TRIGGER workflow_financial_heads_fixed_key BEFORE UPDATE ON workflow_financial_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.head_kind<>OLD.head_kind OR NEW.key_a<>OLD.key_a OR NEW.key_b<>OLD.key_b OR NEW.key_c<>OLD.key_c OR NEW.key_d<>OLD.key_d
BEGIN SELECT RAISE(ABORT,'immutable financial head key'); END;
