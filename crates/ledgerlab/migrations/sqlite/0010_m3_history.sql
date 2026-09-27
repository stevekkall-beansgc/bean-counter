-- M3 raises the retained decision ceiling. Schema-7, schema-8 and schema-9
-- tables and their exact bytes are never altered, rewritten or re-encoded; new
-- appends land in scoped sidecars. The frozen tables are appended by no
-- schema-10 path, so a migrated store keeps one continuous ordinal sequence
-- across billing_entries, billing_m2_entries and billing_m3_entries.
--
-- billing_m3_index is the durable target index. It carries one row per
-- retained decision from all three tiers so the coordinator resolves a duplicate
-- identity, a semantic key, a target history and the ledger clock without
-- decoding any retained bundle. Every column is derived from the entry's own
-- validated receipt or ingress bytes; the store never re-prices anything.
CREATE TABLE billing_m3_index (
  ordinal INTEGER PRIMARY KEY CHECK(ordinal BETWEEN 1 AND 100000),
  customer TEXT NOT NULL CHECK(length(CAST(customer AS BLOB)) BETWEEN 1 AND 128),
  source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
  external_id TEXT NOT NULL CHECK(length(CAST(external_id AS BLOB)) BETWEEN 1 AND 128),
  semantic_key BLOB NOT NULL CHECK(length(semantic_key) BETWEEN 1 AND 262144),
  target TEXT NOT NULL CHECK(length(CAST(target AS BLOB)) BETWEEN 1 AND 256),
  kind TEXT NOT NULL CHECK(kind IN ('base','outcome','correction')),
  accepted_at_us INTEGER NOT NULL CHECK(accepted_at_us BETWEEN -62135596800000000 AND 253402300799999999),
  UNIQUE(customer,source,external_id), UNIQUE(customer,source,semantic_key)
) STRICT;
CREATE INDEX billing_m3_index_target ON billing_m3_index(customer,source,target);
CREATE INDEX billing_m3_index_clock ON billing_m3_index(accepted_at_us);
CREATE TABLE billing_m3_entries (
  ordinal INTEGER PRIMARY KEY CHECK(ordinal BETWEEN 1 AND 100000),
  customer TEXT NOT NULL REFERENCES billing_customers(customer),
  source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
  external_id TEXT NOT NULL CHECK(length(CAST(external_id AS BLOB)) BETWEEN 1 AND 128),
  semantic_key BLOB NOT NULL CHECK(length(semantic_key) BETWEEN 1 AND 262144),
  ingress BLOB NOT NULL CHECK(length(ingress) BETWEEN 1 AND 262144),
  facts BLOB NOT NULL CHECK(length(facts) BETWEEN 1 AND 262144),
  bundle BLOB NOT NULL CHECK(length(bundle) BETWEEN 1 AND 8388608),
  accepted_at_us INTEGER NOT NULL CHECK(accepted_at_us BETWEEN -62135596800000000 AND 253402300799999999),
  agreement_id TEXT NOT NULL CHECK(length(CAST(agreement_id AS BLOB)) BETWEEN 1 AND 128),
  agreement_version INTEGER NOT NULL CHECK(agreement_version BETWEEN 1 AND 1000),
  UNIQUE(customer,source,semantic_key)
) STRICT;
CREATE TABLE billing_m3_aliases (
  customer TEXT NOT NULL REFERENCES billing_customers(customer),
  source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
  external_id TEXT NOT NULL CHECK(length(CAST(external_id AS BLOB)) BETWEEN 1 AND 128),
  ingress BLOB NOT NULL CHECK(length(ingress) BETWEEN 1 AND 262144),
  ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 100000),
  PRIMARY KEY(customer,source,external_id)
) STRICT;
CREATE TABLE billing_m3_bounds (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  guard INTEGER NOT NULL CHECK(guard>=0),
  entry_count INTEGER NOT NULL CHECK(entry_count>=0),
  alias_count INTEGER NOT NULL CHECK(alias_count>=0),
  entry_bytes INTEGER NOT NULL CHECK(entry_bytes>=0),
  alias_bytes INTEGER NOT NULL CHECK(alias_bytes>=0)
) STRICT;
INSERT INTO billing_m3_bounds(singleton,guard,entry_count,alias_count,entry_bytes,alias_bytes)
SELECT 1,
  (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases),
  (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries),
  (SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases),
  COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_entries),0)
    +COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m2_entries),0),
  COALESCE((SELECT sum(length(ingress)) FROM billing_aliases),0)
    +COALESCE((SELECT sum(length(ingress)) FROM billing_m2_aliases),0);
-- guard counts every retained decision row in every tier, so it can only grow by
-- one per append and can never be lowered to hide a deleted or forged row.
-- billing_m3_upgrade records which pre-M3 schema this store was migrated from,
-- so a schema-10 retry after an unknown commit re-derives the frozen digest of
-- that same schema instead of guessing. A store created directly at schema 10
-- has no marker and never had a pre-M3 history to reconcile.
CREATE TABLE billing_m3_upgrade (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  source_version INTEGER NOT NULL CHECK(source_version IN (8,9))
) STRICT;

-- The ledger clock is a bounded index scan, never a bundle decode. A migrated
-- store reads the validated receipt times backfilled by the explicit upgrade.
-- Billing_m3_index is strictly append-only, so a replaced row can never remove
-- an earlier decision's identity.
CREATE TRIGGER billing_m3_index_insert_guard BEFORE INSERT ON billing_m3_index BEGIN
  SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_m3_index WHERE ordinal=NEW.ordinal OR (customer=NEW.customer AND source=NEW.source AND (external_id=NEW.external_id OR semantic_key=NEW.semantic_key))) THEN RAISE(ABORT,'billing M3 index identity') END;
END;
CREATE TRIGGER billing_m3_index_immutable_update BEFORE UPDATE ON billing_m3_index BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_index_immutable_delete BEFORE DELETE ON billing_m3_index BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_entries_insert_guard BEFORE INSERT ON billing_m3_entries BEGIN
  SELECT CASE WHEN (SELECT entry_count FROM billing_m3_bounds WHERE singleton=1)>=100000 THEN RAISE(ABORT,'billing entry bound') END;
  SELECT CASE WHEN NEW.ordinal<>1+(SELECT entry_count FROM billing_m3_bounds WHERE singleton=1) THEN RAISE(ABORT,'billing entry ordinal') END;
  SELECT CASE WHEN length(NEW.bundle)+length(NEW.ingress)+length(NEW.facts)+length(NEW.semantic_key)+(SELECT entry_bytes FROM billing_m3_bounds WHERE singleton=1)>268435456 THEN RAISE(ABORT,'billing entry byte bound') END;
  SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source AND agreement_id=NEW.agreement_id AND agreement_version=NEW.agreement_version AND transition<>'end') THEN RAISE(ABORT,'billing entry agreement') END;
  SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_m3_aliases WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_m2_aliases WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_aliases WHERE source=NEW.source AND external_id=NEW.external_id AND (SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1)=NEW.customer) THEN RAISE(ABORT,'billing entry identity') END;
END;
CREATE TRIGGER billing_m3_aliases_insert_guard BEFORE INSERT ON billing_m3_aliases BEGIN
  SELECT CASE WHEN (SELECT alias_count FROM billing_m3_bounds WHERE singleton=1)>=100000 THEN RAISE(ABORT,'billing alias bound') END;
  SELECT CASE WHEN length(NEW.ingress)+(SELECT alias_bytes FROM billing_m3_bounds WHERE singleton=1)>67108864 THEN RAISE(ABORT,'billing alias byte bound') END;
  SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM billing_m3_index WHERE ordinal=NEW.ordinal AND customer=NEW.customer AND source=NEW.source) AND NOT EXISTS(SELECT 1 FROM billing_m2_entries WHERE ordinal=NEW.ordinal AND customer=NEW.customer AND source=NEW.source) AND NOT EXISTS(SELECT 1 FROM billing_entries WHERE source=NEW.source AND ordinal=NEW.ordinal AND (SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1)=NEW.customer) THEN RAISE(ABORT,'billing alias scoped target') END;
  SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_m2_entries WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_entries WHERE source=NEW.source AND external_id=NEW.external_id AND (SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1)=NEW.customer) OR EXISTS(SELECT 1 FROM billing_m2_aliases WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_aliases WHERE source=NEW.source AND external_id=NEW.external_id AND (SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1)=NEW.customer) THEN RAISE(ABORT,'billing alias identity') END;
END;
CREATE TRIGGER billing_m3_bounds_guard BEFORE UPDATE ON billing_m3_bounds
WHEN NOT (
  (NEW.singleton=1 AND NEW.guard=OLD.guard+1 AND NEW.entry_count=OLD.entry_count+1
    AND NEW.alias_count=OLD.alias_count AND NEW.alias_bytes=OLD.alias_bytes
    AND NEW.entry_bytes>OLD.entry_bytes)
  OR
  (NEW.singleton=1 AND NEW.guard=OLD.guard+1 AND NEW.alias_count=OLD.alias_count+1
    AND NEW.entry_count=OLD.entry_count AND NEW.entry_bytes=OLD.entry_bytes
    AND NEW.alias_bytes>OLD.alias_bytes)
) BEGIN
  SELECT RAISE(ABORT,'billing bounds guard');
END;
CREATE TRIGGER billing_m3_entries_meter AFTER INSERT ON billing_m3_entries BEGIN
  UPDATE billing_m3_bounds SET guard=guard+1,entry_count=entry_count+1,entry_bytes=entry_bytes+length(NEW.bundle)+length(NEW.ingress)+length(NEW.facts)+length(NEW.semantic_key) WHERE singleton=1;
END;
CREATE TRIGGER billing_m3_aliases_meter AFTER INSERT ON billing_m3_aliases BEGIN
  UPDATE billing_m3_bounds SET guard=guard+1,alias_count=alias_count+1,alias_bytes=alias_bytes+length(NEW.ingress) WHERE singleton=1;
END;
CREATE TRIGGER billing_m3_entries_immutable_update BEFORE UPDATE ON billing_m3_entries BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_entries_immutable_delete BEFORE DELETE ON billing_m3_entries BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_aliases_immutable_update BEFORE UPDATE ON billing_m3_aliases BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_aliases_immutable_delete BEFORE DELETE ON billing_m3_aliases BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_bounds_immutable_insert BEFORE INSERT ON billing_m3_bounds WHEN (SELECT count(*) FROM billing_m3_bounds)>0 BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_bounds_immutable_delete BEFORE DELETE ON billing_m3_bounds BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_upgrade_single_insert BEFORE INSERT ON billing_m3_upgrade WHEN (SELECT count(*) FROM billing_m3_upgrade)>0 BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_upgrade_immutable_update BEFORE UPDATE ON billing_m3_upgrade BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
CREATE TRIGGER billing_m3_upgrade_immutable_delete BEFORE DELETE ON billing_m3_upgrade BEGIN SELECT RAISE(ABORT,'immutable billing M3 record'); END;
PRAGMA user_version=10;
