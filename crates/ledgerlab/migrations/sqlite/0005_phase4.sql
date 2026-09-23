-- Additive R3 host-local journals. No pricing or authority decisions in SQL.
-- Ordinals/revisions use fixed-width big-endian u128; canonical wire counters stay decimal.
CREATE TABLE r3_journals (
 journal BLOB PRIMARY KEY CHECK(length(journal) BETWEEN 1 AND 1115),
 identity BLOB NOT NULL CHECK(length(identity) BETWEEN 2 AND 4096),
 ordinal BLOB NOT NULL CHECK(length(ordinal)=16 AND ordinal<=x'0000000c9f2c9cd04674edea3fffffff'),
 segment TEXT NOT NULL CHECK(length(segment)=64),
 replay_root TEXT NOT NULL CHECK(length(replay_root)=64)
) STRICT;
CREATE TABLE r3_segments (
 journal BLOB NOT NULL REFERENCES r3_journals(journal) DEFERRABLE INITIALLY DEFERRED,
 ordinal BLOB NOT NULL CHECK(length(ordinal)=16 AND ordinal<=x'0000000c9f2c9cd04674edea3fffffff'),
 segment TEXT NOT NULL CHECK(length(segment)=64),
 replay_root TEXT NOT NULL CHECK(length(replay_root)=64),
 byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 2 AND 8388608),
 page_count INTEGER NOT NULL CHECK(page_count BETWEEN 1 AND 2048),
 PRIMARY KEY(journal,ordinal), UNIQUE(journal,segment)
) STRICT;
CREATE TABLE r3_segment_pages (
 journal BLOB NOT NULL, ordinal BLOB NOT NULL, page INTEGER NOT NULL CHECK(page BETWEEN 0 AND 2047),
 bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,ordinal,page),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments(journal,ordinal) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_objects (
 journal BLOB NOT NULL, ordinal BLOB NOT NULL, kind TEXT NOT NULL,
 origin BLOB NOT NULL CHECK(length(origin) BETWEEN 2 AND 2048),
 full_key BLOB NOT NULL CHECK(length(full_key) BETWEEN 1 AND 4096),
 body_hash TEXT NOT NULL CHECK(length(body_hash)=64),
 byte_length INTEGER NOT NULL CHECK(byte_length BETWEEN 2 AND 262144),
 metadata BLOB NOT NULL CHECK(length(metadata) BETWEEN 2 AND 8192),
 PRIMARY KEY(journal,origin,kind,full_key,body_hash),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments(journal,ordinal) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_object_pages (
 journal BLOB NOT NULL, origin BLOB NOT NULL, kind TEXT NOT NULL, full_key BLOB NOT NULL, body_hash TEXT NOT NULL,
 page INTEGER NOT NULL CHECK(page BETWEEN 0 AND 63), bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,origin,kind,full_key,body_hash,page),
 FOREIGN KEY(journal,origin,kind,full_key,body_hash) REFERENCES r3_objects DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_heads (
 journal BLOB NOT NULL REFERENCES r3_journals(journal) DEFERRABLE INITIALLY DEFERRED,
 kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 17), full_key BLOB NOT NULL CHECK(length(full_key) BETWEEN 1 AND 1115),
 revision BLOB NOT NULL CHECK(length(revision)=16 AND revision<=x'0000000c9f2c9cd04674edea3fffffff'), value BLOB NOT NULL CHECK(length(value) BETWEEN 2 AND 8388608),
 PRIMARY KEY(journal,kind,full_key)
) STRICT;
CREATE TABLE r3_head_versions (
 journal BLOB NOT NULL, kind INTEGER NOT NULL CHECK(kind BETWEEN 0 AND 17), full_key BLOB NOT NULL CHECK(length(full_key) BETWEEN 1 AND 1115),
 ordinal BLOB NOT NULL, revision BLOB NOT NULL CHECK(length(revision)=16 AND revision<=x'0000000c9f2c9cd04674edea3fffffff'), value BLOB NOT NULL CHECK(length(value) BETWEEN 2 AND 8388608),
 PRIMARY KEY(journal,kind,full_key,ordinal),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_commands (
 journal BLOB NOT NULL, delivery BLOB NOT NULL CHECK(length(delivery) BETWEEN 2 AND 4096), ordinal BLOB NOT NULL,
 command BLOB NOT NULL CHECK(length(command) BETWEEN 2 AND 262144),
 result BLOB NOT NULL CHECK(length(result) BETWEEN 2 AND 8388608),
 receipt BLOB CHECK(receipt IS NULL OR length(receipt) BETWEEN 2 AND 8192),
 PRIMARY KEY(journal,delivery), FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_namespaces (
 tenant TEXT NOT NULL, environment TEXT NOT NULL, tag TEXT NOT NULL CHECK(length(tag)=32 AND tag NOT GLOB '*[^0-9a-f]*'),
 gateway TEXT NOT NULL, journal BLOB NOT NULL,
 PRIMARY KEY(tenant,environment,tag),
 FOREIGN KEY(journal) REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_deliveries (
 tenant TEXT NOT NULL, environment TEXT NOT NULL, source TEXT NOT NULL, external_id TEXT NOT NULL,
 journal BLOB NOT NULL, value BLOB NOT NULL CHECK(length(value) BETWEEN 2 AND 16384),
 PRIMARY KEY(tenant,environment,source,external_id),
 FOREIGN KEY(journal) REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_index_pages (
 journal BLOB NOT NULL REFERENCES r3_journals DEFERRABLE INITIALLY DEFERRED,
 hash TEXT NOT NULL CHECK(length(hash)=64), bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 4096),
 PRIMARY KEY(journal,hash)
) STRICT;
CREATE TABLE r3_index_roots (
 journal BLOB NOT NULL, full_key BLOB NOT NULL CHECK(length(full_key) BETWEEN 1 AND 1115), ordinal BLOB NOT NULL,
 root TEXT NOT NULL CHECK(length(root)=64), PRIMARY KEY(journal,full_key,ordinal),
 FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE r3_held_intentions (
 journal BLOB NOT NULL, ordinal BLOB NOT NULL, position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 127),
 action BLOB NOT NULL CHECK(length(action) BETWEEN 2 AND 8192), state TEXT NOT NULL DEFAULT 'held' CHECK(state='held'),
 PRIMARY KEY(journal,ordinal,position), FOREIGN KEY(journal,ordinal) REFERENCES r3_segments DEFERRABLE INITIALLY DEFERRED
) STRICT;
-- These indexes make enrollment's old-occupancy scan an indexed prefix lookup.
CREATE INDEX r3_legacy_prefix ON delivery_keys(tenant,environment,substr(external_id,1,37));
CREATE INDEX r3_outcome_prefix ON outcome_deliveries(tenant,environment,substr(external_id,1,37));
CREATE INDEX r3_delivery_prefix ON r3_deliveries(tenant,environment,substr(external_id,1,37));
CREATE TRIGGER r3_namespace_prior_occupancy BEFORE INSERT ON r3_namespaces BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM delivery_keys WHERE tenant=NEW.tenant AND environment=NEW.environment AND substr(external_id,1,37)='gw1.'||NEW.tag||'.')
 OR EXISTS(SELECT 1 FROM outcome_deliveries WHERE tenant=NEW.tenant AND environment=NEW.environment AND substr(external_id,1,37)='gw1.'||NEW.tag||'.')
 OR EXISTS(SELECT 1 FROM r3_deliveries WHERE tenant=NEW.tenant AND environment=NEW.environment AND substr(external_id,1,37)='gw1.'||NEW.tag||'.')
 THEN RAISE(ABORT,'R3 namespace already occupied') END;
END;
CREATE TRIGGER r3_legacy_namespace_guard BEFORE INSERT ON delivery_keys BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM r3_namespaces WHERE tenant=NEW.tenant AND environment=NEW.environment AND 'gw1.'||tag||'.'=substr(NEW.external_id,1,37))
 OR EXISTS(SELECT 1 FROM r3_deliveries WHERE tenant=NEW.tenant AND environment=NEW.environment AND source=NEW.source AND external_id=NEW.external_id)
 THEN RAISE(ABORT,'R3 delivery namespace owned') END;
END;
CREATE TRIGGER r3_outcome_namespace_guard BEFORE INSERT ON outcome_deliveries BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM r3_namespaces WHERE tenant=NEW.tenant AND environment=NEW.environment AND 'gw1.'||tag||'.'=substr(NEW.external_id,1,37))
 OR EXISTS(SELECT 1 FROM r3_deliveries WHERE tenant=NEW.tenant AND environment=NEW.environment AND source=NEW.source AND external_id=NEW.external_id)
 THEN RAISE(ABORT,'R3 delivery namespace owned') END;
END;
CREATE TRIGGER r3_delivery_prior_occupancy BEFORE INSERT ON r3_deliveries BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM delivery_keys WHERE tenant=NEW.tenant AND environment=NEW.environment AND source=NEW.source AND external_id=NEW.external_id)
 OR EXISTS(SELECT 1 FROM outcome_deliveries WHERE tenant=NEW.tenant AND environment=NEW.environment AND source=NEW.source AND external_id=NEW.external_id)
 THEN RAISE(ABORT,'prior delivery identity occupied') END;
END;
CREATE TRIGGER r3_head_identity BEFORE UPDATE OF journal,kind,full_key ON r3_heads BEGIN SELECT RAISE(ABORT,'immutable R3 head identity'); END;
CREATE TRIGGER r3_journal_identity BEFORE UPDATE OF journal,identity ON r3_journals BEGIN SELECT RAISE(ABORT,'immutable R3 journal identity'); END;
CREATE TRIGGER r3_segments_immutable_update BEFORE UPDATE ON r3_segments BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_segments_immutable_delete BEFORE DELETE ON r3_segments BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_segment_pages_immutable_update BEFORE UPDATE ON r3_segment_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_segment_pages_immutable_delete BEFORE DELETE ON r3_segment_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_objects_immutable_update BEFORE UPDATE ON r3_objects BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_objects_immutable_delete BEFORE DELETE ON r3_objects BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_object_pages_immutable_update BEFORE UPDATE ON r3_object_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_object_pages_immutable_delete BEFORE DELETE ON r3_object_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_head_versions_immutable_update BEFORE UPDATE ON r3_head_versions BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_head_versions_immutable_delete BEFORE DELETE ON r3_head_versions BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_commands_immutable_update BEFORE UPDATE ON r3_commands BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_commands_immutable_delete BEFORE DELETE ON r3_commands BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_namespaces_immutable_update BEFORE UPDATE ON r3_namespaces BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_namespaces_immutable_delete BEFORE DELETE ON r3_namespaces BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_deliveries_immutable_update BEFORE UPDATE ON r3_deliveries BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_deliveries_immutable_delete BEFORE DELETE ON r3_deliveries BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_index_pages_immutable_update BEFORE UPDATE ON r3_index_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_index_pages_immutable_delete BEFORE DELETE ON r3_index_pages BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_index_roots_immutable_update BEFORE UPDATE ON r3_index_roots BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_index_roots_immutable_delete BEFORE DELETE ON r3_index_roots BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_held_intentions_immutable_update BEFORE UPDATE ON r3_held_intentions BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_held_intentions_immutable_delete BEFORE DELETE ON r3_held_intentions BEGIN SELECT RAISE(ABORT,'immutable R3 storage'); END;
CREATE TRIGGER r3_head_delete BEFORE DELETE ON r3_heads BEGIN SELECT RAISE(ABORT,'permanent R3 head identity'); END;
CREATE TRIGGER r3_journal_delete BEFORE DELETE ON r3_journals BEGIN SELECT RAISE(ABORT,'permanent R3 journal identity'); END;
CREATE TRIGGER r3_head_revision BEFORE UPDATE OF revision ON r3_heads WHEN NEW.revision<=OLD.revision BEGIN SELECT RAISE(ABORT,'R3 head revision must advance'); END;
CREATE TRIGGER r3_journal_ordinal BEFORE UPDATE OF ordinal ON r3_journals WHEN NEW.ordinal<=OLD.ordinal BEGIN SELECT RAISE(ABORT,'R3 journal ordinal must advance'); END;
PRAGMA user_version=5;
