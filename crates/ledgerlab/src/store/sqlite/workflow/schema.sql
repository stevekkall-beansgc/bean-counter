-- Additive generic workflow records; no product-specific table or price column.
-- The owning migration must apply this under an explicit dispatch-held upgrade.
CREATE TABLE workflow_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 profile TEXT NOT NULL CHECK(profile='workflow-dual-legacy-capacity/1'),
 operation_count INTEGER NOT NULL CHECK(operation_count BETWEEN 0 AND 100000),
 payload_bytes INTEGER NOT NULL CHECK(payload_bytes BETWEEN 0 AND 268435456)
) STRICT;
INSERT INTO workflow_state VALUES(1,'workflow-dual-legacy-capacity/1',0,0);
CREATE TRIGGER workflow_state_monotone BEFORE UPDATE ON workflow_state
WHEN NEW.singleton<>OLD.singleton OR NEW.profile<>OLD.profile
 OR NEW.operation_count<OLD.operation_count OR NEW.payload_bytes<OLD.payload_bytes
BEGIN SELECT RAISE(ABORT,'workflow meter must be monotone'); END;
CREATE TRIGGER workflow_state_no_delete BEFORE DELETE ON workflow_state
BEGIN SELECT RAISE(ABORT,'immutable workflow capacity profile'); END;
CREATE TABLE workflow_documents (
 kind TEXT NOT NULL CHECK(kind IN ('workflow-product','workflow-fact-schema','workflow-slot-template','workflow-policy','workflow-compiler-descriptor','workflow-compiled-plan','workflow-authority','workflow-publication','workflow-build-manifest')),
 id TEXT PRIMARY KEY, digest TEXT NOT NULL, schema_name TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND 1048576),
 UNIQUE(kind,id,digest)
) STRICT;
CREATE TABLE workflow_definition_registry (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, kind TEXT NOT NULL, semantic_id TEXT NOT NULL, version TEXT NOT NULL,
 document_id TEXT NOT NULL REFERENCES workflow_documents(id),
 PRIMARY KEY(logical_store_id,tenant,environment,kind,semantic_id,version)
) STRICT;
CREATE TABLE workflow_publications (
 publication_key BLOB PRIMARY KEY, publication_id TEXT NOT NULL REFERENCES workflow_documents(id),
 plan_id TEXT NOT NULL REFERENCES workflow_documents(id), root_policy_id TEXT NOT NULL REFERENCES workflow_documents(id)
) STRICT;
CREATE TABLE workflow_operations (
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
 publication_id TEXT NOT NULL REFERENCES workflow_documents(id),
 plan_id TEXT NOT NULL REFERENCES workflow_documents(id),
 commit_sequence INTEGER NOT NULL UNIQUE CHECK(commit_sequence BETWEEN 1 AND 100000),
 accepted_at_us INTEGER NOT NULL,
 UNIQUE(logical_store_id,tenant,environment,source,operation_id)
) STRICT;
CREATE TABLE workflow_manifest_members (
 identity_key BLOB NOT NULL REFERENCES workflow_operations(identity_key),
 kind TEXT NOT NULL, id TEXT NOT NULL, digest TEXT NOT NULL,
 PRIMARY KEY(identity_key,kind,id,digest)
) STRICT;
CREATE TABLE workflow_feed (
 commit_sequence INTEGER PRIMARY KEY REFERENCES workflow_operations(commit_sequence),
 identity_key BLOB NOT NULL UNIQUE REFERENCES workflow_operations(identity_key)
) STRICT;
CREATE TRIGGER workflow_documents_update_immutable BEFORE UPDATE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_documents_delete_immutable BEFORE DELETE ON workflow_documents BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_definition_registry_update_immutable BEFORE UPDATE ON workflow_definition_registry BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_definition_registry_delete_immutable BEFORE DELETE ON workflow_definition_registry BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_publications_update_immutable BEFORE UPDATE ON workflow_publications BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_publications_delete_immutable BEFORE DELETE ON workflow_publications BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_operations_delete_immutable BEFORE DELETE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_manifest_members_update_immutable BEFORE UPDATE ON workflow_manifest_members BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_manifest_members_delete_immutable BEFORE DELETE ON workflow_manifest_members BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_feed_update_immutable BEFORE UPDATE ON workflow_feed BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_feed_delete_immutable BEFORE DELETE ON workflow_feed BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
