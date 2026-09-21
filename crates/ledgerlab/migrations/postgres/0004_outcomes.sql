-- Additive persistence for frozen outcome/settlement records. No economics here.
SET LOCAL search_path=ledgerlab,pg_catalog;
CREATE TABLE outcome_scope_locks (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 class SMALLINT NOT NULL CHECK(class BETWEEN 0 AND 8),
 key BYTEA NOT NULL CHECK(length(key) BETWEEN 1 AND 4096),
 PRIMARY KEY(tenant,environment,class,key)
);
CREATE TABLE outcome_heads (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 class SMALLINT NOT NULL, key BYTEA NOT NULL,
 revision BIGINT NOT NULL CHECK(revision>=0),
 value BYTEA NOT NULL CHECK(length(value) BETWEEN 2 AND 262144),
 PRIMARY KEY(tenant,environment,class,key),
 FOREIGN KEY(tenant,environment,class,key) REFERENCES outcome_scope_locks
);
CREATE TABLE outcome_records (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 kind TEXT NOT NULL COLLATE "C" CHECK(kind IN ('evidence','policy-snapshot','binding-snapshot','base-evaluation','base-identity','target-snapshot','base-acceptance','authority-decision','event','base-posting','target-basis','admission','claim','claim-revision','effect','action','obligation','link','dependency','limit-evidence','explanation','replay-input','intention','delivery-key','chain-revision','decision-manifest','receipt','reservation-observation','reservation-transition','reservation-receipt')),
 id BYTEA NOT NULL CHECK(length(id) BETWEEN 1 AND 4096),
 content_hash TEXT NOT NULL CHECK(content_hash ~ '^sha256:[0-9a-f]{64}$'),
 envelope BYTEA NOT NULL CHECK(length(envelope) BETWEEN 2 AND 4194304),
 PRIMARY KEY(tenant,environment,kind,id),
 UNIQUE(tenant,environment,kind,id,content_hash)
);
CREATE TABLE outcome_members (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 target TEXT NOT NULL COLLATE "C", invocation_id TEXT NOT NULL COLLATE "C",
 kind TEXT NOT NULL COLLATE "C", id BYTEA NOT NULL,
 PRIMARY KEY(tenant,environment,target,invocation_id,kind,id),
 FOREIGN KEY(tenant,environment,kind,id) REFERENCES outcome_records DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE outcome_anchors (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 target TEXT NOT NULL COLLATE "C", invocation_id TEXT NOT NULL COLLATE "C",
 kind TEXT NOT NULL COLLATE "C", id BYTEA NOT NULL, content_hash TEXT NOT NULL,
 PRIMARY KEY(tenant,environment,target,invocation_id,kind,id),
 FOREIGN KEY(tenant,environment,kind,id,content_hash) REFERENCES outcome_records(tenant,environment,kind,id,content_hash) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE outcome_deliveries (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 source TEXT NOT NULL COLLATE "C", external_id TEXT NOT NULL COLLATE "C",
 canonical_source TEXT NOT NULL COLLATE "C", canonical_external_id TEXT NOT NULL COLLATE "C",
 command BYTEA NOT NULL CHECK(length(command) BETWEEN 2 AND 262144),
 ingress BYTEA NOT NULL CHECK(length(ingress) BETWEEN 2 AND 262144),
 ingress_hash TEXT NOT NULL CHECK(ingress_hash ~ '^sha256:[0-9a-f]{64}$'),
 economic_kind TEXT COLLATE "C" CHECK(economic_kind IN ('base-acceptance','receipt')),
 economic_id BYTEA,
 settlement_kind TEXT NOT NULL DEFAULT 'reservation-receipt' CHECK(settlement_kind='reservation-receipt'),
 settlement_id BYTEA NOT NULL,
 PRIMARY KEY(tenant,environment,source,external_id),
 CHECK((economic_kind IS NULL)=(economic_id IS NULL)),
 FOREIGN KEY(tenant,environment,economic_kind,economic_id) REFERENCES outcome_records DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant,environment,settlement_kind,settlement_id) REFERENCES outcome_records DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant,environment,canonical_source,canonical_external_id) REFERENCES outcome_deliveries DEFERRABLE INITIALLY DEFERRED
);
-- A single uniqueness arbiter works in both directions, including old writers.
-- Populate it from legacy accepted keys without changing a single legacy byte.
CREATE TABLE acceptance_delivery_namespace (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 source TEXT NOT NULL COLLATE "C", external_id TEXT NOT NULL COLLATE "C",
 profile TEXT NOT NULL CHECK(profile IN ('v1','outcome')),
 PRIMARY KEY(tenant,environment,source,external_id)
);
INSERT INTO acceptance_delivery_namespace SELECT tenant,environment,source,external_id,'v1' FROM delivery_keys;
CREATE FUNCTION ledgerlab.claim_delivery_namespace() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 INSERT INTO ledgerlab.acceptance_delivery_namespace VALUES(NEW.tenant,NEW.environment,NEW.source,NEW.external_id,TG_ARGV[0]);
 RETURN NEW;
END; $$;
REVOKE ALL ON FUNCTION ledgerlab.claim_delivery_namespace() FROM PUBLIC;
CREATE TRIGGER delivery_keys_shared_namespace BEFORE INSERT ON delivery_keys FOR EACH ROW EXECUTE FUNCTION ledgerlab.claim_delivery_namespace('v1');
CREATE TRIGGER outcome_deliveries_shared_namespace BEFORE INSERT ON outcome_deliveries FOR EACH ROW EXECUTE FUNCTION ledgerlab.claim_delivery_namespace('outcome');
CREATE TABLE outcome_held_intentions (
 tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
 kind TEXT NOT NULL DEFAULT 'intention' CHECK(kind='intention'), id BYTEA NOT NULL,
 state TEXT NOT NULL DEFAULT 'held' CHECK(state='held'),
 PRIMARY KEY(tenant,environment,id),
 FOREIGN KEY(tenant,environment,kind,id) REFERENCES outcome_records DEFERRABLE INITIALLY DEFERRED
);
CREATE TRIGGER outcome_records_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON outcome_records FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER outcome_members_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON outcome_members FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER outcome_anchors_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON outcome_anchors FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER outcome_deliveries_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON outcome_deliveries FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER acceptance_delivery_namespace_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON acceptance_delivery_namespace FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER outcome_scope_identity BEFORE UPDATE OR DELETE OR TRUNCATE ON outcome_scope_locks FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER outcome_head_identity BEFORE UPDATE OF tenant,environment,class,key ON outcome_heads FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE FUNCTION ledgerlab.outcome_head_revision() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.revision <> OLD.revision + 1 THEN RAISE EXCEPTION 'OUTCOME_HEAD_REVISION' USING ERRCODE='23000'; END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER outcome_head_revision BEFORE UPDATE ON outcome_heads FOR EACH ROW EXECUTE FUNCTION ledgerlab.outcome_head_revision();
