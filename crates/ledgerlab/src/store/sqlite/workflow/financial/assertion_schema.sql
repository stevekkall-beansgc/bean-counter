-- Rebuildable local projection of the shared immutable assertion identity.
-- Earlier migration bytes remain unchanged. Only an explicit held-writer
-- coordinator invokes this migration; ordinary open/read never upgrades.
CREATE TABLE workflow_assertion_claims (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 verified_source TEXT NOT NULL, attestation_id TEXT NOT NULL,
 record_kind TEXT NOT NULL CHECK(record_kind='workflow-fact-attestation'),
 record_id TEXT NOT NULL, record_digest TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,verified_source,attestation_id),
 FOREIGN KEY(record_kind,record_id,record_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT;
