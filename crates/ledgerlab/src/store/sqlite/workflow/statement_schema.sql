-- Explicit schema-19-to-20 presentation-only migration. No financial row changes.
CREATE TABLE workflow_statement_membership (
 customer TEXT NOT NULL,
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 action_kind TEXT NOT NULL CHECK(action_kind='workflow-action'), action_id TEXT NOT NULL, action_digest TEXT NOT NULL,
 claim_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
 presentation_kind TEXT NOT NULL CHECK(presentation_kind IN ('standard-period','ad-hoc')),
 statement_id TEXT NOT NULL, statement_hash TEXT NOT NULL,
 PRIMARY KEY(customer,logical_store_id,tenant,environment,action_kind,action_id,action_digest),
 FOREIGN KEY(action_kind,action_id,action_digest) REFERENCES workflow_documents(kind,id,digest)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER workflow_statement_membership_no_update BEFORE UPDATE ON workflow_statement_membership BEGIN SELECT RAISE(ABORT,'immutable workflow presentation'); END;
CREATE TRIGGER workflow_statement_membership_no_delete BEFORE DELETE ON workflow_statement_membership BEGIN SELECT RAISE(ABORT,'immutable workflow presentation'); END;
