-- Operational delivery evidence only. No economic records or hashes change.
ALTER TABLE ledgerlab.dispatcher_head ADD COLUMN revision BIGINT NOT NULL DEFAULT 0 CHECK(revision>=0);
CREATE TABLE ledgerlab.dispatch_attempts (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 intention_id TEXT NOT NULL COLLATE "C", attempt BIGINT NOT NULL CHECK(attempt>0),
 generation BIGINT NOT NULL CHECK(generation>0), started_us BIGINT NOT NULL,
 request_hash TEXT NOT NULL, canonical_bytes BYTEA NOT NULL,
 PRIMARY KEY(tenant,environment,intention_id,attempt),
 FOREIGN KEY(tenant,environment,intention_id) REFERENCES ledgerlab.intentions(tenant,environment,id)
);
CREATE TABLE ledgerlab.delivery_observations (
 sequence BIGINT PRIMARY KEY CHECK(sequence>0), generation BIGINT NOT NULL CHECK(generation>=0),
 canonical_bytes BYTEA NOT NULL
);
CREATE TABLE ledgerlab.reconciliation_reports (
 sequence BIGINT NOT NULL UNIQUE,
 digest TEXT PRIMARY KEY COLLATE "C", generation BIGINT NOT NULL CHECK(generation>=0),
 canonical_bytes BYTEA NOT NULL
);
CREATE TRIGGER dispatch_attempts_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON ledgerlab.dispatch_attempts FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER delivery_observations_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON ledgerlab.delivery_observations FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER reconciliation_reports_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON ledgerlab.reconciliation_reports FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
