-- Operational veto and terminal guards; no economic records change.
CREATE TABLE ledgerlab.delivery_quarantines (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 intention_id TEXT NOT NULL COLLATE "C", digest TEXT NOT NULL,
 canonical_bytes BYTEA NOT NULL,
 PRIMARY KEY(tenant,environment,intention_id),
 FOREIGN KEY(tenant,environment,intention_id) REFERENCES ledgerlab.intentions(tenant,environment,id)
);
CREATE TRIGGER delivery_quarantines_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON ledgerlab.delivery_quarantines FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE FUNCTION ledgerlab.guard_delivery_terminal() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (OLD.state='rejected' AND NEW.state<>'rejected') OR NEW.attempts<OLD.attempts
 OR (NEW.state='leased' AND EXISTS(SELECT 1 FROM ledgerlab.delivery_quarantines q WHERE q.tenant=OLD.tenant AND q.environment=OLD.environment AND q.intention_id=OLD.intention_id)) THEN
  RAISE EXCEPTION 'DELIVERY_TERMINAL';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER delivery_terminal_guard BEFORE UPDATE ON ledgerlab.delivery_state FOR EACH ROW EXECUTE FUNCTION ledgerlab.guard_delivery_terminal();
