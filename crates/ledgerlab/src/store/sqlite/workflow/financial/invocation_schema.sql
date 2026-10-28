-- Immutable fullRef Invocation revisions; all other IDs remain unique.
PRAGMA defer_foreign_keys=ON;

ALTER TABLE workflow_documents ADD COLUMN non_invocation_id TEXT GENERATED ALWAYS AS (CASE WHEN kind='workflow-invocation' AND schema_name='ledger-workflow-invocation/1' THEN NULL ELSE id END) VIRTUAL;

CREATE UNIQUE INDEX workflow_documents_invocation_guard_upgrade ON workflow_documents(non_invocation_id);

CREATE TABLE workflow_agreement_acceptances_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0),
 accepted_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 binding_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 intent_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,version),
 UNIQUE(logical_store_id,tenant,environment,binding_id),
 UNIQUE(logical_store_id,tenant,environment,accepted_id),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer)
) STRICT;

INSERT INTO workflow_agreement_acceptances_invocation_upgrade("logical_store_id","tenant","environment","agreement_id","customer","version","accepted_id","binding_id","intent_id","transition_id") SELECT "logical_store_id","tenant","environment","agreement_id","customer","version","accepted_id","binding_id","intent_id","transition_id" FROM workflow_agreement_acceptances;

DROP TABLE workflow_agreement_acceptances;

ALTER TABLE workflow_agreement_acceptances_invocation_upgrade RENAME TO workflow_agreement_acceptances;

CREATE TRIGGER workflow_agreement_acceptances_delete_immutable BEFORE DELETE ON workflow_agreement_acceptances BEGIN SELECT RAISE(ABORT,'immutable acceptance history'); END;

CREATE TRIGGER workflow_agreement_acceptances_update_immutable BEFORE UPDATE ON workflow_agreement_acceptances BEGIN SELECT RAISE(ABORT,'immutable acceptance history'); END;

CREATE TABLE workflow_agreement_activations_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 activation_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 binding_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 intent_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 action TEXT NOT NULL CHECK(action IN ('activate','replace','revoke')),
 effective_at_us INTEGER NOT NULL, accepted_at_us INTEGER NOT NULL,
 commit_sequence INTEGER NOT NULL CHECK(commit_sequence>0),
 target_id TEXT REFERENCES workflow_documents(non_invocation_id),
 CHECK((action='activate' AND target_id IS NULL) OR (action<>'activate' AND target_id IS NOT NULL)),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,revision),
 UNIQUE(logical_store_id,tenant,environment,customer,agreement_id,activation_id),
 UNIQUE(logical_store_id,tenant,environment,transition_id),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer),
 FOREIGN KEY(logical_store_id,tenant,environment,customer,agreement_id,target_id) REFERENCES workflow_agreement_activations(logical_store_id,tenant,environment,customer,agreement_id,activation_id)
) STRICT;

INSERT INTO workflow_agreement_activations_invocation_upgrade("logical_store_id","tenant","environment","agreement_id","customer","revision","activation_id","transition_id","binding_id","intent_id","action","effective_at_us","accepted_at_us","commit_sequence","target_id") SELECT "logical_store_id","tenant","environment","agreement_id","customer","revision","activation_id","transition_id","binding_id","intent_id","action","effective_at_us","accepted_at_us","commit_sequence","target_id" FROM workflow_agreement_activations;

DROP TABLE workflow_agreement_activations;

ALTER TABLE workflow_agreement_activations_invocation_upgrade RENAME TO workflow_agreement_activations;

CREATE TRIGGER workflow_agreement_activations_delete_immutable BEFORE DELETE ON workflow_agreement_activations BEGIN SELECT RAISE(ABORT,'immutable activation history'); END;

CREATE TRIGGER workflow_agreement_activations_update_immutable BEFORE UPDATE ON workflow_agreement_activations BEGIN SELECT RAISE(ABORT,'immutable activation history'); END;

CREATE TABLE workflow_agreement_heads_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('agreement_acceptance','agreement_activation')),
 revision INTEGER NOT NULL CHECK(revision>0),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,head_kind),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer)
) STRICT;

INSERT INTO workflow_agreement_heads_invocation_upgrade("logical_store_id","tenant","environment","agreement_id","customer","head_kind","revision","transition_id") SELECT "logical_store_id","tenant","environment","agreement_id","customer","head_kind","revision","transition_id" FROM workflow_agreement_heads;

DROP TABLE workflow_agreement_heads;

ALTER TABLE workflow_agreement_heads_invocation_upgrade RENAME TO workflow_agreement_heads;

CREATE TRIGGER workflow_agreement_heads_monotone BEFORE UPDATE ON workflow_agreement_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.agreement_id<>OLD.agreement_id OR NEW.customer<>OLD.customer OR NEW.head_kind<>OLD.head_kind OR NEW.revision<>OLD.revision+1
BEGIN SELECT RAISE(ABORT,'agreement head must advance independently'); END;

CREATE TRIGGER workflow_agreement_heads_no_delete BEFORE DELETE ON workflow_agreement_heads BEGIN SELECT RAISE(ABORT,'permanent agreement head lineage'); END;

CREATE TABLE workflow_customer_registrations_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 customer TEXT NOT NULL, document_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer)
) STRICT;

INSERT INTO workflow_customer_registrations_invocation_upgrade("logical_store_id","tenant","environment","customer","document_id") SELECT "logical_store_id","tenant","environment","customer","document_id" FROM workflow_customer_registrations;

DROP TABLE workflow_customer_registrations;

ALTER TABLE workflow_customer_registrations_invocation_upgrade RENAME TO workflow_customer_registrations;

CREATE TRIGGER workflow_customer_registrations_delete_immutable BEFORE DELETE ON workflow_customer_registrations BEGIN SELECT RAISE(ABORT,'immutable workflow customer registration'); END;

CREATE TRIGGER workflow_customer_registrations_update_immutable BEFORE UPDATE ON workflow_customer_registrations BEGIN SELECT RAISE(ABORT,'immutable workflow customer registration'); END;

CREATE TABLE workflow_definition_registry_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, kind TEXT NOT NULL, semantic_id TEXT NOT NULL, version TEXT NOT NULL,
 document_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 PRIMARY KEY(logical_store_id,tenant,environment,kind,semantic_id,version)
) STRICT;

INSERT INTO workflow_definition_registry_invocation_upgrade("logical_store_id","tenant","environment","source","kind","semantic_id","version","document_id") SELECT "logical_store_id","tenant","environment","source","kind","semantic_id","version","document_id" FROM workflow_definition_registry;

DROP TABLE workflow_definition_registry;

ALTER TABLE workflow_definition_registry_invocation_upgrade RENAME TO workflow_definition_registry;

CREATE TRIGGER workflow_definition_registry_delete_immutable BEFORE DELETE ON workflow_definition_registry BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TRIGGER workflow_definition_registry_update_immutable BEFORE UPDATE ON workflow_definition_registry BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TABLE workflow_definition_upload_parts_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, upload_id TEXT NOT NULL, part INTEGER NOT NULL CHECK(part IN (0,1)),
 document_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 PRIMARY KEY(logical_store_id,tenant,environment,source,upload_id,part),
 FOREIGN KEY(logical_store_id,tenant,environment,source,upload_id) REFERENCES workflow_definition_uploads(logical_store_id,tenant,environment,source,upload_id)
) STRICT;

INSERT INTO workflow_definition_upload_parts_invocation_upgrade("logical_store_id","tenant","environment","source","upload_id","part","document_id") SELECT "logical_store_id","tenant","environment","source","upload_id","part","document_id" FROM workflow_definition_upload_parts;

DROP TABLE workflow_definition_upload_parts;

ALTER TABLE workflow_definition_upload_parts_invocation_upgrade RENAME TO workflow_definition_upload_parts;

CREATE TRIGGER workflow_definition_upload_parts_delete_immutable BEFORE DELETE ON workflow_definition_upload_parts BEGIN SELECT RAISE(ABORT,'immutable source upload part'); END;

CREATE TRIGGER workflow_definition_upload_parts_update_immutable BEFORE UPDATE ON workflow_definition_upload_parts BEGIN SELECT RAISE(ABORT,'immutable source upload part'); END;

CREATE TABLE workflow_operations_invocation_upgrade (
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
 publication_id TEXT REFERENCES workflow_documents(non_invocation_id),
 plan_id TEXT REFERENCES workflow_documents(non_invocation_id),
 commit_sequence INTEGER NOT NULL UNIQUE CHECK(commit_sequence>0),
 accepted_at_us INTEGER NOT NULL,
 operation_kind TEXT NOT NULL CHECK(operation_kind IN ('policy.publish','evidence.retain','authority.change','agreement.accept','agreement.activate','agreement.revoke','work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert','invocation.authorize','invocation.close','cost.finalize','cost.correct')),
 CHECK((operation_kind='policy.publish' AND publication_id IS NOT NULL AND plan_id IS NOT NULL)
    OR (operation_kind<>'policy.publish' AND publication_id IS NULL AND plan_id IS NULL)),
 UNIQUE(logical_store_id,tenant,environment,source,operation_id)
) STRICT;

INSERT INTO workflow_operations_invocation_upgrade("identity_key","logical_store_id","tenant","environment","source","operation_id","operation_record_id","operation_digest","operation_bytes","normalized_request_hash","manifest_id","manifest_digest","manifest_bytes","receipt_id","receipt_digest","receipt_bytes","publication_id","plan_id","commit_sequence","accepted_at_us","operation_kind") SELECT "identity_key","logical_store_id","tenant","environment","source","operation_id","operation_record_id","operation_digest","operation_bytes","normalized_request_hash","manifest_id","manifest_digest","manifest_bytes","receipt_id","receipt_digest","receipt_bytes","publication_id","plan_id","commit_sequence","accepted_at_us","operation_kind" FROM workflow_operations;

DROP TABLE workflow_operations;

ALTER TABLE workflow_operations_invocation_upgrade RENAME TO workflow_operations;

CREATE TRIGGER workflow_operations_delete_immutable BEFORE DELETE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TABLE workflow_publications_invocation_upgrade (
 publication_key BLOB PRIMARY KEY, publication_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 plan_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id), root_policy_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id)
) STRICT;

INSERT INTO workflow_publications_invocation_upgrade("publication_key","publication_id","plan_id","root_policy_id") SELECT "publication_key","publication_id","plan_id","root_policy_id" FROM workflow_publications;

DROP TABLE workflow_publications;

ALTER TABLE workflow_publications_invocation_upgrade RENAME TO workflow_publications;

CREATE TRIGGER workflow_publications_delete_immutable BEFORE DELETE ON workflow_publications BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TRIGGER workflow_publications_update_immutable BEFORE UPDATE ON workflow_publications BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TABLE workflow_source_enrollments_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 enrollment_id TEXT NOT NULL, document_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 source TEXT NOT NULL, canonical_economic_namespace TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,enrollment_id)
) STRICT;

INSERT INTO workflow_source_enrollments_invocation_upgrade("logical_store_id","tenant","environment","enrollment_id","document_id","source","canonical_economic_namespace") SELECT "logical_store_id","tenant","environment","enrollment_id","document_id","source","canonical_economic_namespace" FROM workflow_source_enrollments;

DROP TABLE workflow_source_enrollments;

ALTER TABLE workflow_source_enrollments_invocation_upgrade RENAME TO workflow_source_enrollments;

CREATE TRIGGER workflow_source_enrollments_delete_immutable BEFORE DELETE ON workflow_source_enrollments BEGIN SELECT RAISE(ABORT,'immutable workflow original ceiling'); END;

CREATE TRIGGER workflow_source_enrollments_update_immutable BEFORE UPDATE ON workflow_source_enrollments BEGIN SELECT RAISE(ABORT,'immutable workflow original ceiling'); END;

CREATE TABLE workflow_source_heads_invocation_upgrade (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 enrollment_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
 state_id TEXT NOT NULL REFERENCES workflow_documents(non_invocation_id),
 grant_id TEXT REFERENCES workflow_documents(non_invocation_id),
 status TEXT NOT NULL CHECK(status IN ('active','revoked')),
 CHECK((status='active' AND grant_id IS NOT NULL) OR (status='revoked' AND grant_id IS NULL)),
 PRIMARY KEY(logical_store_id,tenant,environment,enrollment_id),
 FOREIGN KEY(logical_store_id,tenant,environment,enrollment_id) REFERENCES workflow_source_enrollments(logical_store_id,tenant,environment,enrollment_id)
) STRICT;

INSERT INTO workflow_source_heads_invocation_upgrade("logical_store_id","tenant","environment","enrollment_id","revision","state_id","grant_id","status") SELECT "logical_store_id","tenant","environment","enrollment_id","revision","state_id","grant_id","status" FROM workflow_source_heads;

DROP TABLE workflow_source_heads;

ALTER TABLE workflow_source_heads_invocation_upgrade RENAME TO workflow_source_heads;

CREATE TRIGGER workflow_source_heads_monotone BEFORE UPDATE ON workflow_source_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.enrollment_id<>OLD.enrollment_id OR NEW.revision<>OLD.revision+1 OR OLD.status='revoked'
BEGIN SELECT RAISE(ABORT,'workflow source state must advance without reenabling'); END;

CREATE TRIGGER workflow_source_heads_no_delete BEFORE DELETE ON workflow_source_heads BEGIN SELECT RAISE(ABORT,'permanent workflow source lineage'); END;

CREATE TABLE workflow_documents_invocation_upgrade (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest','workflow-evidence','workflow-customer','workflow-source-enrollment','workflow-source-grant','workflow-source-state','workflow-definition-fragment','workflow-roles','workflow-slot-catalog','workflow-slot-instantiation','workflow-slot-terms','workflow-acceptance-terms','workflow-assent','workflow-binding','workflow-accepted-agreement','workflow-head-transition','workflow-agreement-intent','workflow-activation','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-offer','workflow-invocation','workflow-cost','workflow-verification','workflow-stage-instance','workflow-stage-state','workflow-stage-closure-claim','workflow-allocation')),
 id TEXT NOT NULL, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND
   CASE WHEN kind IN ('workflow-compiled-plan','workflow-economic-authority','workflow-authority-prefix-observation','workflow-authority-audit-verifier','workflow-financial-history-observation','workflow-verified-fact','workflow-fact-attestation','workflow-evidence-verification','workflow-intent','workflow-work','workflow-work-claim','workflow-outcome-claim','workflow-decision','workflow-action','workflow-evaluation','workflow-target','workflow-family-state','workflow-link','workflow-chain-state','workflow-correction-capacity','workflow-dependency','workflow-capacity-retirement','workflow-invocation','workflow-cost','workflow-stage-instance','workflow-stage-state','workflow-stage-closure-claim','workflow-allocation') THEN 4194304 ELSE 1048576 END),
 non_invocation_id TEXT GENERATED ALWAYS AS (CASE WHEN kind='workflow-invocation' AND schema_name='ledger-workflow-invocation/1' THEN NULL ELSE id END) VIRTUAL UNIQUE,
 PRIMARY KEY(kind,id,digest)
) STRICT;

INSERT INTO workflow_documents_invocation_upgrade(kind,id,digest,schema_name,canonical_bytes) SELECT kind,id,digest,schema_name,canonical_bytes FROM workflow_documents;

DROP TABLE workflow_documents;

ALTER TABLE workflow_documents_invocation_upgrade RENAME TO workflow_documents;

CREATE INDEX workflow_documents_by_id ON workflow_documents(id);

CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

CREATE TRIGGER workflow_documents_invocation_identity BEFORE INSERT ON workflow_documents
WHEN EXISTS(SELECT 1 FROM workflow_documents d WHERE d.id=NEW.id AND
 (NEW.kind<>'workflow-invocation' OR NEW.schema_name<>'ledger-workflow-invocation/1' OR
  d.kind<>'workflow-invocation' OR d.schema_name<>'ledger-workflow-invocation/1') AND
 (d.kind<>NEW.kind OR d.digest<>NEW.digest OR d.schema_name<>NEW.schema_name))
BEGIN SELECT RAISE(ABORT,'immutable workflow document identity'); END;

CREATE TRIGGER workflow_documents_fullref_identity BEFORE INSERT ON workflow_documents
WHEN EXISTS(SELECT 1 FROM workflow_documents d WHERE d.kind=NEW.kind AND d.id=NEW.id AND d.digest=NEW.digest AND
 CASE WHEN d.schema_name<>NEW.schema_name OR length(d.canonical_bytes)<>length(NEW.canonical_bytes) THEN 1 ELSE d.canonical_bytes<>NEW.canonical_bytes END)
BEGIN SELECT RAISE(ABORT,'immutable workflow full reference'); END;
