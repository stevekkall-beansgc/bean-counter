-- Mechanical operator-quota migration; no retained bytes/identities change.
PRAGMA defer_foreign_keys=ON;
CREATE TABLE workflow_state_resource_upgrade (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 profile TEXT NOT NULL CHECK(profile='workflow-dual-legacy-capacity/1'),
 operation_count INTEGER NOT NULL CHECK(operation_count>=0),
 payload_bytes INTEGER NOT NULL CHECK(payload_bytes>=0)
) STRICT;


INSERT INTO workflow_state_resource_upgrade SELECT * FROM workflow_state;
DROP TABLE workflow_state;
ALTER TABLE workflow_state_resource_upgrade RENAME TO workflow_state;
CREATE TRIGGER workflow_state_monotone BEFORE UPDATE ON workflow_state
WHEN NEW.singleton<>OLD.singleton OR NEW.profile<>OLD.profile
 OR NEW.operation_count<OLD.operation_count OR NEW.payload_bytes<OLD.payload_bytes
BEGIN SELECT RAISE(ABORT,'workflow meter must be monotone'); END;
CREATE TRIGGER workflow_state_no_delete BEFORE DELETE ON workflow_state
BEGIN SELECT RAISE(ABORT,'immutable workflow capacity profile'); END;
CREATE TABLE workflow_operations_resource_upgrade (
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
 commit_sequence INTEGER NOT NULL UNIQUE CHECK(commit_sequence>0),
 accepted_at_us INTEGER NOT NULL,
 operation_kind TEXT NOT NULL CHECK(operation_kind IN ('policy.publish','evidence.retain','authority.change','agreement.accept','agreement.activate','agreement.revoke','work.accept','work.reverse','quantity.correct','outcome.submit','outcome.correct','outcome.close','link.assert','invocation.authorize','invocation.close','cost.finalize','cost.correct')),
 CHECK((operation_kind='policy.publish' AND publication_id IS NOT NULL AND plan_id IS NOT NULL)
    OR (operation_kind<>'policy.publish' AND publication_id IS NULL AND plan_id IS NULL)),
 UNIQUE(logical_store_id,tenant,environment,source,operation_id)
) STRICT;
INSERT INTO workflow_operations_resource_upgrade SELECT * FROM workflow_operations;
DROP TABLE workflow_operations;
ALTER TABLE workflow_operations_resource_upgrade RENAME TO workflow_operations;
CREATE TRIGGER workflow_operations_update_immutable BEFORE UPDATE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;
CREATE TRIGGER workflow_operations_delete_immutable BEFORE DELETE ON workflow_operations BEGIN SELECT RAISE(ABORT,'immutable workflow history'); END;

