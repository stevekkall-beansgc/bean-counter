-- Additive app attribution/consumption projections; schemas12–14 remain unchanged.
PRAGMA defer_foreign_keys=ON;
CREATE TABLE workflow_documents_app_upgrade (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest','workflow-evidence','workflow-customer','workflow-source-enrollment','workflow-source-grant','workflow-source-state','workflow-definition-fragment','workflow-roles','workflow-slot-catalog','workflow-slot-instantiation','workflow-slot-terms','workflow-acceptance-terms','workflow-assent','workflow-binding','workflow-accepted-agreement','workflow-head-transition','workflow-agreement-intent','workflow-activation','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-offer','workflow-invocation','workflow-cost','workflow-verification')),
 id TEXT PRIMARY KEY, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND
   CASE WHEN kind IN ('workflow-compiled-plan','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-invocation','workflow-cost') THEN 4194304 ELSE 1048576 END),
 UNIQUE(kind,id,digest)
) STRICT;
INSERT INTO workflow_documents_app_upgrade SELECT * FROM workflow_documents;
DROP TABLE workflow_documents;
ALTER TABLE workflow_documents_app_upgrade RENAME TO workflow_documents;
CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TABLE workflow_financial_heads_app_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('decision','family','chain','capacity','invocation','cost','cost_consumption','app_cost')),
 key_a TEXT NOT NULL, key_b TEXT NOT NULL, key_c TEXT NOT NULL, key_d TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(length(revision) BETWEEN 1 AND 20 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<20 OR revision<='18446744073709551615')),
 record_kind TEXT NOT NULL, record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 CHECK((head_kind IN ('decision','capacity','invocation') AND key_a<>'' AND key_b='' AND key_c='' AND key_d='') OR (head_kind='chain' AND key_a<>'' AND key_b<>'' AND key_c='' AND key_d='') OR (head_kind IN ('cost','cost_consumption','app_cost') AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d='') OR (head_kind='family' AND key_a<>'' AND key_b<>'' AND key_c<>'' AND key_d<>'')),
 CHECK((head_kind='decision' AND record_kind='workflow-decision') OR (head_kind='family' AND record_kind='workflow-family-state') OR (head_kind='chain' AND record_kind='workflow-chain-state') OR (head_kind='capacity' AND record_kind='workflow-correction-capacity') OR (head_kind='invocation' AND record_kind='workflow-invocation') OR (head_kind IN ('cost','cost_consumption','app_cost') AND record_kind='workflow-cost')),
 PRIMARY KEY(logical_store_id,tenant,environment,head_kind,key_a,key_b,key_c,key_d),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
INSERT INTO workflow_financial_heads_app_upgrade SELECT * FROM workflow_financial_heads;
DROP TABLE workflow_financial_heads;
ALTER TABLE workflow_financial_heads_app_upgrade RENAME TO workflow_financial_heads;
CREATE TRIGGER workflow_financial_heads_no_delete BEFORE DELETE ON workflow_financial_heads BEGIN SELECT RAISE(ABORT,'permanent financial head lineage'); END;
CREATE TRIGGER workflow_financial_heads_fixed_key BEFORE UPDATE ON workflow_financial_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.head_kind<>OLD.head_kind OR NEW.key_a<>OLD.key_a OR NEW.key_b<>OLD.key_b OR NEW.key_c<>OLD.key_c OR NEW.key_d<>OLD.key_d
BEGIN SELECT RAISE(ABORT,'immutable financial head key'); END;
CREATE TABLE workflow_app_call_claims (
 logical_store_id TEXT NOT NULL,
 source_namespace TEXT NOT NULL, call_id TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-fact-attestation'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,source_namespace,call_id),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_app_call_claims_update_immutable BEFORE UPDATE ON workflow_app_call_claims BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_call_claims_delete_immutable BEFORE DELETE ON workflow_app_call_claims BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TABLE workflow_app_component_observations (
 logical_store_id TEXT NOT NULL,
 work_kind TEXT NOT NULL, work_id TEXT NOT NULL, work_digest TEXT NOT NULL, component_slot TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-fact-attestation'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,work_kind,work_id,work_digest,component_slot),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest),
 CHECK(work_kind='workflow-work'),
 FOREIGN KEY(work_kind,work_id,work_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_app_component_observations_update_immutable BEFORE UPDATE ON workflow_app_component_observations BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_component_observations_delete_immutable BEFORE DELETE ON workflow_app_component_observations BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TABLE workflow_app_provider_calls (
 logical_store_id TEXT NOT NULL,
 provider TEXT NOT NULL, account TEXT NOT NULL, request_id TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-fact-attestation'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,provider,account,request_id),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_app_provider_calls_update_immutable BEFORE UPDATE ON workflow_app_provider_calls BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_provider_calls_delete_immutable BEFORE DELETE ON workflow_app_provider_calls BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TABLE workflow_app_cost_claims (
 logical_store_id TEXT NOT NULL,
 work_kind TEXT NOT NULL, work_id TEXT NOT NULL, work_digest TEXT NOT NULL, component_slot TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,work_kind,work_id,work_digest,component_slot),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest),
 CHECK(work_kind='workflow-work'),
 FOREIGN KEY(work_kind,work_id,work_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_app_cost_claims_update_immutable BEFORE UPDATE ON workflow_app_cost_claims BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_cost_claims_delete_immutable BEFORE DELETE ON workflow_app_cost_claims BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TABLE workflow_app_evidence_consumptions (
 logical_store_id TEXT NOT NULL,
 evidence_kind TEXT NOT NULL, evidence_id TEXT NOT NULL, evidence_digest TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,evidence_kind,evidence_id,evidence_digest),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest),
 CHECK(evidence_kind='workflow-fact-attestation'),
 FOREIGN KEY(evidence_kind,evidence_id,evidence_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_app_evidence_consumptions_update_immutable BEFORE UPDATE ON workflow_app_evidence_consumptions BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_evidence_consumptions_delete_immutable BEFORE DELETE ON workflow_app_evidence_consumptions BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TABLE workflow_app_observation_revisions (
 logical_store_id TEXT NOT NULL,
 observation_kind TEXT NOT NULL, observation_id TEXT NOT NULL, observation_digest TEXT NOT NULL, cost_revision TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-cost'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,observation_kind,observation_id,observation_digest,cost_revision),
 UNIQUE(record_kind,record_id,record_digest),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest),
 CHECK(observation_kind='workflow-fact-attestation'),
 FOREIGN KEY(observation_kind,observation_id,observation_digest) REFERENCES workflow_documents(kind,id,digest),
 CHECK(length(cost_revision) BETWEEN 1 AND 20 AND substr(cost_revision,1,1) BETWEEN '1' AND '9' AND cost_revision NOT GLOB '*[^0-9]*' AND (length(cost_revision)<20 OR cost_revision<='18446744073709551615'))
) STRICT;
CREATE TRIGGER workflow_app_observation_revisions_update_immutable BEFORE UPDATE ON workflow_app_observation_revisions BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
CREATE TRIGGER workflow_app_observation_revisions_delete_immutable BEFORE DELETE ON workflow_app_observation_revisions BEGIN SELECT RAISE(ABORT,'immutable app attribution/consumption'); END;
-- Local operator-attested party mappings remain distinct from customer
-- registration and source grants. A terminal revocation preserves originals.
CREATE TABLE workflow_party_authorization_heads (
 logical_store_id TEXT NOT NULL,tenant TEXT NOT NULL,environment TEXT NOT NULL,
 customer TEXT NOT NULL,agreement_id TEXT NOT NULL,authorization_id TEXT NOT NULL,
 revision TEXT NOT NULL CHECK(revision IN ('1','2')),
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-authority'),record_id TEXT NOT NULL,record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,authorization_id),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
CREATE TRIGGER workflow_party_authorization_heads_no_delete BEFORE DELETE ON workflow_party_authorization_heads BEGIN SELECT RAISE(ABORT,'permanent party mapping lineage'); END;
CREATE TRIGGER workflow_party_authorization_heads_fixed_key BEFORE UPDATE ON workflow_party_authorization_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.customer<>OLD.customer OR NEW.agreement_id<>OLD.agreement_id OR NEW.authorization_id<>OLD.authorization_id
BEGIN SELECT RAISE(ABORT,'immutable party authorization key'); END;
