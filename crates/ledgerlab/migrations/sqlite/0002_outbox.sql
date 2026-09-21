-- Operational delivery evidence only. No economic records or hashes change.
ALTER TABLE dispatcher_head ADD COLUMN revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0);
CREATE TABLE dispatch_attempts (
 tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
 intention_id TEXT NOT NULL COLLATE BINARY, attempt INTEGER NOT NULL CHECK(attempt>0),
 generation INTEGER NOT NULL CHECK(generation>0), started_us INTEGER NOT NULL,
 request_hash TEXT NOT NULL, canonical_bytes BLOB NOT NULL,
 PRIMARY KEY(tenant,environment,intention_id,attempt),
 FOREIGN KEY(tenant,environment,intention_id) REFERENCES intentions(tenant,environment,id)
) STRICT;
CREATE TABLE delivery_observations (
 sequence INTEGER PRIMARY KEY CHECK(sequence>0), generation INTEGER NOT NULL CHECK(generation>=0),
 canonical_bytes BLOB NOT NULL
) STRICT;
CREATE TABLE reconciliation_reports (
 sequence INTEGER NOT NULL UNIQUE,
 digest TEXT PRIMARY KEY COLLATE BINARY, generation INTEGER NOT NULL CHECK(generation>=0),
 canonical_bytes BLOB NOT NULL
) STRICT;
CREATE TRIGGER dispatch_attempts_no_update BEFORE UPDATE ON dispatch_attempts BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER dispatch_attempts_no_delete BEFORE DELETE ON dispatch_attempts BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_observations_no_update BEFORE UPDATE ON delivery_observations BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_observations_no_delete BEFORE DELETE ON delivery_observations BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER reconciliation_reports_no_update BEFORE UPDATE ON reconciliation_reports BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER reconciliation_reports_no_delete BEFORE DELETE ON reconciliation_reports BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
PRAGMA user_version=2;
