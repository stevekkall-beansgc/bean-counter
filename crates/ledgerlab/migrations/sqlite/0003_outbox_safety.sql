-- Operational veto and terminal guards; no economic records change.
CREATE TABLE delivery_quarantines (
 tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
 intention_id TEXT NOT NULL COLLATE BINARY, digest TEXT NOT NULL,
 canonical_bytes BLOB NOT NULL,
 PRIMARY KEY(tenant,environment,intention_id),
 FOREIGN KEY(tenant,environment,intention_id) REFERENCES intentions(tenant,environment,id)
) STRICT;
CREATE TRIGGER delivery_quarantines_no_update BEFORE UPDATE ON delivery_quarantines BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_quarantines_no_delete BEFORE DELETE ON delivery_quarantines BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_terminal_guard BEFORE UPDATE ON delivery_state
WHEN (OLD.state='rejected' AND NEW.state<>'rejected') OR NEW.attempts<OLD.attempts
 OR (NEW.state='leased' AND EXISTS(SELECT 1 FROM delivery_quarantines q WHERE q.tenant=OLD.tenant AND q.environment=OLD.environment AND q.intention_id=OLD.intention_id))
BEGIN SELECT RAISE(ABORT,'DELIVERY_TERMINAL'); END;
PRAGMA user_version=3;
