-- PostgreSQL-native R3 projections. No capability is issued by this migration.
-- Compact indexed digests retain full keys in-row; adapters must compare complete
-- bytes before accepting a lookup. A collision refuses; it never merges identity.
-- Large byte columns may TOAST: the production physical envelope must account for
-- that storage before admitting R3 writes. This schema is not a backing proof.
SET LOCAL search_path=ledgerlab,pg_catalog;
-- Permanent missing-row lock identities can precede journal creation.
CREATE TABLE r3_scope_locks (
 journal BYTEA NOT NULL CHECK(octet_length(journal) BETWEEN 1 AND 1115),
 class SMALLINT NOT NULL CHECK(class BETWEEN 9 AND 16),
 full_key BYTEA NOT NULL CHECK(octet_length(full_key) BETWEEN 1 AND 1115),
 PRIMARY KEY(journal,class,full_key)
);
CREATE TRIGGER r3_scope_locks_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_scope_locks FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();

CREATE TABLE r3_journals (
 journal BYTEA PRIMARY KEY CHECK(octet_length(journal) BETWEEN 1 AND 1115),
 identity BYTEA NOT NULL CHECK(octet_length(identity) BETWEEN 2 AND 4096),
 ordinal BYTEA NOT NULL CHECK(octet_length(ordinal)=16 AND ordinal<=decode('0000000c9f2c9cd04674edea3fffffff','hex')),
 segment TEXT COLLATE "C" NOT NULL CHECK(octet_length(segment)=64),
 replay_root TEXT COLLATE "C" NOT NULL CHECK(octet_length(replay_root)=64)
);
CREATE TABLE r3_storage_profile (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 journal BYTEA NOT NULL UNIQUE REFERENCES r3_journals(journal) DEFERRABLE INITIALLY DEFERRED,
 profile BYTEA NOT NULL CHECK(octet_length(profile) BETWEEN 2 AND 8192),
 backing_identity BYTEA NOT NULL CHECK(octet_length(backing_identity) BETWEEN 2 AND 8192),
 legacy_allowance BYTEA NOT NULL CHECK(octet_length(legacy_allowance)=16),
 legacy_used BYTEA NOT NULL CHECK(octet_length(legacy_used)=16)
);
CREATE TABLE r3_segments (
 journal BYTEA NOT NULL REFERENCES r3_journals(journal) DEFERRABLE INITIALLY DEFERRED,
 ordinal BYTEA NOT NULL CHECK(octet_length(ordinal)=16 AND ordinal<=decode('0000000c9f2c9cd04674edea3fffffff','hex')),
 segment TEXT COLLATE "C" NOT NULL CHECK(octet_length(segment)=64),
 replay_root TEXT COLLATE "C" NOT NULL CHECK(octet_length(replay_root)=64),
 byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 2 AND 8388608),
 page_count INTEGER NOT NULL CHECK(page_count BETWEEN 1 AND 2048),
 PRIMARY KEY(journal,ordinal), UNIQUE(journal,segment)
);
CREATE TABLE r3_segment_pages (
 journal BYTEA NOT NULL, ordinal BYTEA NOT NULL, page INTEGER NOT NULL CHECK(page BETWEEN 0 AND 2047),
 bytes BYTEA NOT NULL CHECK(octet_length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,ordinal,page),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments(journal,ordinal) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_objects (
 origin_hash BYTEA GENERATED ALWAYS AS (sha256(origin)) STORED,
 key_hash BYTEA GENERATED ALWAYS AS (sha256(full_key)) STORED,
 journal BYTEA NOT NULL, ordinal BYTEA NOT NULL, kind TEXT COLLATE "C" NOT NULL CHECK(octet_length(kind) BETWEEN 1 AND 32),
 origin BYTEA NOT NULL CHECK(octet_length(origin) BETWEEN 2 AND 2048),
 full_key BYTEA NOT NULL CHECK(octet_length(full_key) BETWEEN 1 AND 4096),
 body_hash TEXT COLLATE "C" NOT NULL CHECK(octet_length(body_hash)=64),
 byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 2 AND 262144),
 metadata BYTEA NOT NULL CHECK(octet_length(metadata) BETWEEN 2 AND 8192),
 PRIMARY KEY(journal,origin_hash,kind,key_hash,body_hash),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments(journal,ordinal) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_object_pages (
 origin_hash BYTEA GENERATED ALWAYS AS (sha256(origin)) STORED,
 key_hash BYTEA GENERATED ALWAYS AS (sha256(full_key)) STORED,
 journal BYTEA NOT NULL, origin BYTEA NOT NULL, kind TEXT COLLATE "C" NOT NULL CHECK(octet_length(kind) BETWEEN 1 AND 32), full_key BYTEA NOT NULL, body_hash TEXT COLLATE "C" NOT NULL,
 page INTEGER NOT NULL CHECK(page BETWEEN 0 AND 63), bytes BYTEA NOT NULL CHECK(octet_length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,origin_hash,kind,key_hash,body_hash,page),
 FOREIGN KEY(journal,origin_hash,kind,key_hash,body_hash) REFERENCES r3_objects DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX r3_fact_lookup ON r3_objects(journal,kind,key_hash);
CREATE TABLE r3_heads (
 journal BYTEA NOT NULL REFERENCES r3_journals(journal) DEFERRABLE INITIALLY DEFERRED,
 kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 17), full_key BYTEA NOT NULL CHECK(octet_length(full_key) BETWEEN 1 AND 1115),
 revision BYTEA NOT NULL CHECK(octet_length(revision)=16 AND revision<=decode('0000000c9f2c9cd04674edea3fffffff','hex')), value BYTEA NOT NULL CHECK(octet_length(value) BETWEEN 2 AND 8388608),
 PRIMARY KEY(journal,kind,full_key)
);
CREATE TABLE r3_head_versions (
 journal BYTEA NOT NULL, kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 17), full_key BYTEA NOT NULL CHECK(octet_length(full_key) BETWEEN 1 AND 1115),
 ordinal BYTEA NOT NULL, revision BYTEA NOT NULL CHECK(octet_length(revision)=16 AND revision<=decode('0000000c9f2c9cd04674edea3fffffff','hex')), value BYTEA NOT NULL CHECK(octet_length(value) BETWEEN 2 AND 8388608),
 PRIMARY KEY(journal,kind,full_key,ordinal),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_commands (
 delivery_hash BYTEA GENERATED ALWAYS AS (sha256(delivery)) STORED,
 journal BYTEA NOT NULL, delivery BYTEA NOT NULL CHECK(octet_length(delivery) BETWEEN 2 AND 4096), ordinal BYTEA NOT NULL,
 command BYTEA NOT NULL CHECK(octet_length(command) BETWEEN 2 AND 262144),
 result BYTEA NOT NULL CHECK(octet_length(result) BETWEEN 2 AND 8388608),
 receipt BYTEA CHECK(receipt IS NULL OR octet_length(receipt) BETWEEN 2 AND 8192),
 PRIMARY KEY(journal,delivery_hash), FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_namespaces (
 tenant TEXT COLLATE "C" NOT NULL, environment TEXT COLLATE "C" NOT NULL, tag TEXT COLLATE "C" NOT NULL CHECK(tag ~ '^[0-9a-f]{32}$'),
 gateway TEXT COLLATE "C" NOT NULL, journal BYTEA NOT NULL,
 PRIMARY KEY(tenant,environment,tag),
 FOREIGN KEY(journal) REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_deliveries (
 tenant TEXT COLLATE "C" NOT NULL, environment TEXT COLLATE "C" NOT NULL, source TEXT COLLATE "C" NOT NULL, external_id TEXT COLLATE "C" NOT NULL,
 journal BYTEA NOT NULL, value BYTEA NOT NULL CHECK(octet_length(value) BETWEEN 2 AND 16384),
 PRIMARY KEY(tenant,environment,source,external_id),
 FOREIGN KEY(journal) REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_index_pages (
 journal BYTEA NOT NULL REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED,
 hash TEXT COLLATE "C" NOT NULL CHECK(octet_length(hash)=64), bytes BYTEA NOT NULL CHECK(octet_length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,hash)
);
CREATE TABLE r3_index_roots (
 journal BYTEA NOT NULL, full_key BYTEA NOT NULL CHECK(octet_length(full_key) BETWEEN 1 AND 1115), ordinal BYTEA NOT NULL,
 root TEXT COLLATE "C" NOT NULL CHECK(octet_length(root)=64), PRIMARY KEY(journal,full_key,ordinal),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE r3_held_intentions (
 journal BYTEA NOT NULL, ordinal BYTEA NOT NULL, position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 127),
 action BYTEA NOT NULL CHECK(octet_length(action) BETWEEN 2 AND 8192), state TEXT COLLATE "C" NOT NULL DEFAULT 'held' CHECK(state='held'),
 PRIMARY KEY(journal,ordinal,position), FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
);

CREATE UNIQUE INDEX r3_command_ordinal ON r3_commands(journal,ordinal);
CREATE INDEX r3_authority_hash ON r3_objects(journal,kind,body_hash,ordinal);
-- Recovery has one persistent slot; idle does not erase its prior resolution.
-- Admission/recovery code must hold the cluster session gate through resolution.
CREATE TABLE r3_unresolved_work (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 generation BYTEA NOT NULL CHECK(octet_length(generation)=16),
 state TEXT COLLATE "C" NOT NULL CHECK(state IN ('IDLE','RESOLVING')),
 backend_pid INTEGER, backend_start TIMESTAMPTZ,
 journal BYTEA, delivery BYTEA, command_hash TEXT COLLATE "C",
 CHECK((state='IDLE') OR (backend_pid IS NOT NULL AND backend_start IS NOT NULL
   AND octet_length(journal) BETWEEN 1 AND 1115
   AND octet_length(delivery) BETWEEN 2 AND 4096 AND command_hash ~ '^[0-9a-f]{64}$'))
);
CREATE TRIGGER r3_recovery_identity BEFORE UPDATE OF singleton ON r3_unresolved_work FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_recovery_permanent BEFORE DELETE OR TRUNCATE ON r3_unresolved_work FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_segments_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_segments FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_segment_pages_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_segment_pages FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_objects_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_objects FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_object_pages_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_object_pages FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_head_versions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_head_versions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_commands_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_commands FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_namespaces_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_namespaces FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_deliveries_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_deliveries FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_index_pages_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_index_pages FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_index_roots_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_index_roots FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_held_intentions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON r3_held_intentions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_heads_identity BEFORE UPDATE OF journal,kind,full_key ON r3_heads FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_heads_permanent BEFORE DELETE OR TRUNCATE ON r3_heads FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_journals_identity BEFORE UPDATE OF journal,identity ON r3_journals FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_journals_permanent BEFORE DELETE OR TRUNCATE ON r3_journals FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_storage_profile_identity BEFORE UPDATE OF singleton,journal,profile,backing_identity,legacy_allowance ON r3_storage_profile FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER r3_storage_profile_permanent BEFORE DELETE OR TRUNCATE ON r3_storage_profile FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE FUNCTION ledgerlab.r3_successor(value BYTEA) RETURNS BYTEA LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog AS $$
DECLARE result BYTEA := value; i INTEGER; digit INTEGER;
BEGIN
 IF octet_length(value) <> 16 OR value >= decode('0000000c9f2c9cd04674edea3fffffff','hex') THEN
  RAISE EXCEPTION 'R3_COUNTER_EXHAUSTED' USING ERRCODE='22003';
 END IF;
 FOR i IN REVERSE 15..0 LOOP
  digit := get_byte(result,i);
  IF digit < 255 THEN RETURN set_byte(result,i,digit+1); END IF;
  result := set_byte(result,i,0);
 END LOOP;
 RAISE EXCEPTION 'R3_COUNTER_EXHAUSTED' USING ERRCODE='22003';
END; $$;
CREATE FUNCTION ledgerlab.r3_monotonic_head() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.revision <> ledgerlab.r3_successor(OLD.revision) THEN RAISE EXCEPTION 'R3_HEAD_REVISION' USING ERRCODE='23000'; END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER r3_head_revision BEFORE UPDATE ON r3_heads FOR EACH ROW EXECUTE FUNCTION ledgerlab.r3_monotonic_head();
CREATE FUNCTION ledgerlab.r3_monotonic_journal() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF NEW.ordinal <> ledgerlab.r3_successor(OLD.ordinal) THEN RAISE EXCEPTION 'R3_JOURNAL_ORDINAL' USING ERRCODE='23000'; END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER r3_journal_ordinal BEFORE UPDATE ON r3_journals FOR EACH ROW EXECUTE FUNCTION ledgerlab.r3_monotonic_journal();
-- A single exact-key uniqueness arbiter continues to cover every writer profile.
ALTER TABLE acceptance_delivery_namespace DROP CONSTRAINT acceptance_delivery_namespace_profile_check;
ALTER TABLE acceptance_delivery_namespace ADD CHECK(profile IN ('v1','outcome','r3'));
CREATE INDEX r3_namespace_prior_prefix ON acceptance_delivery_namespace(tenant,environment,(left(external_id,37)));
CREATE FUNCTION ledgerlab.r3_claim_namespace() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 PERFORM pg_advisory_xact_lock(714215265);
 IF EXISTS(SELECT 1 FROM ledgerlab.acceptance_delivery_namespace WHERE tenant=NEW.tenant AND environment=NEW.environment AND left(external_id,37)='gw1.'||NEW.tag||'.') THEN
  RAISE EXCEPTION 'R3_NAMESPACE_OCCUPIED' USING ERRCODE='23000';
 END IF;
 RETURN NEW;
END; $$;
REVOKE ALL ON FUNCTION ledgerlab.r3_claim_namespace() FROM PUBLIC;
CREATE TRIGGER r3_namespace_prior_occupancy BEFORE INSERT ON r3_namespaces FOR EACH ROW EXECUTE FUNCTION ledgerlab.r3_claim_namespace();
CREATE OR REPLACE FUNCTION ledgerlab.claim_delivery_namespace() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
 PERFORM pg_advisory_xact_lock(714215265);
 IF TG_ARGV[0] <> 'r3' AND EXISTS(SELECT 1 FROM ledgerlab.r3_namespaces WHERE tenant=NEW.tenant AND environment=NEW.environment AND 'gw1.'||tag||'.'=left(NEW.external_id,37)) THEN
  RAISE EXCEPTION 'R3_NAMESPACE_OWNED' USING ERRCODE='23000';
 END IF;
 INSERT INTO ledgerlab.acceptance_delivery_namespace VALUES(NEW.tenant,NEW.environment,NEW.source,NEW.external_id,TG_ARGV[0]);
 RETURN NEW;
END; $$;
CREATE TRIGGER r3_deliveries_shared_namespace BEFORE INSERT ON r3_deliveries FOR EACH ROW EXECUTE FUNCTION ledgerlab.claim_delivery_namespace('r3');
