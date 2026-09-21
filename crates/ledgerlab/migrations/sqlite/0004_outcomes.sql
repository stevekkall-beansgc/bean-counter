-- Additive frozen outcome + reservation-settlement persistence.
-- Record bodies, economics and authorization are validated by the coordinator.
CREATE TABLE outcome_records (
 tenant TEXT NOT NULL, environment TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('evidence','policy-snapshot','event','base-posting','target-basis','admission','claim','claim-revision','effect','action','obligation','link','dependency','limit-evidence','explanation','replay-input','intention','delivery-key','chain-revision','decision-manifest','receipt','binding-snapshot','base-evaluation','base-identity','target-snapshot','base-acceptance','authority-decision','reservation-observation','reservation-transition','reservation-receipt')),
 id BLOB NOT NULL CHECK(length(id) BETWEEN 1 AND 4096),
 content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'),
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304),
 PRIMARY KEY(tenant,environment,kind,id),
 UNIQUE(tenant,environment,kind,id,content_hash)
) STRICT;
CREATE TABLE outcome_heads (
 class INTEGER NOT NULL CHECK(class BETWEEN 0 AND 8),
 key BLOB NOT NULL CHECK(length(key) BETWEEN 1 AND 4096),
 revision TEXT NOT NULL CHECK(revision='0' OR (length(revision) BETWEEN 1 AND 19 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*' AND (length(revision)<19 OR revision<='9223372036854775807'))),
 value BLOB NOT NULL CHECK(length(value) BETWEEN 2 AND 4194304),
 PRIMARY KEY(class,key)
) STRICT;
CREATE TABLE outcome_deliveries (
 tenant TEXT NOT NULL, environment TEXT NOT NULL, source TEXT NOT NULL, external_id TEXT NOT NULL,
 canonical_source TEXT NOT NULL, canonical_external_id TEXT NOT NULL,
 command BLOB NOT NULL CHECK(length(command) BETWEEN 2 AND 262144),
 ingress BLOB NOT NULL CHECK(length(ingress) BETWEEN 2 AND 262144), ingress_hash TEXT NOT NULL,
 economic_kind TEXT CHECK(economic_kind IN ('base-acceptance','receipt')),
 economic_id BLOB, economic_hash TEXT,
 settlement_kind TEXT NOT NULL CHECK(settlement_kind='reservation-receipt'), settlement_id BLOB NOT NULL, settlement_hash TEXT NOT NULL,
 PRIMARY KEY(tenant,environment,source,external_id),
 CHECK((economic_kind IS NULL AND economic_id IS NULL AND economic_hash IS NULL) OR (economic_kind IS NOT NULL AND economic_id IS NOT NULL AND economic_hash IS NOT NULL)),
 FOREIGN KEY(tenant,environment,canonical_source,canonical_external_id) REFERENCES outcome_deliveries(tenant,environment,source,external_id) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant,environment,economic_kind,economic_id,economic_hash) REFERENCES outcome_records(tenant,environment,kind,id,content_hash) DEFERRABLE INITIALLY DEFERRED,
 FOREIGN KEY(tenant,environment,settlement_kind,settlement_id,settlement_hash) REFERENCES outcome_records(tenant,environment,kind,id,content_hash) DEFERRABLE INITIALLY DEFERRED
) STRICT;
-- Both acceptance paths share a permanent identity namespace.
CREATE TRIGGER outcome_delivery_excludes_v1 BEFORE INSERT ON outcome_deliveries BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM delivery_keys d WHERE d.tenant=NEW.tenant AND d.environment=NEW.environment AND d.source=NEW.source AND d.external_id=NEW.external_id) THEN RAISE(ABORT,'delivery identity already retained by v1') END;
END;
CREATE TRIGGER v1_delivery_excludes_outcome BEFORE INSERT ON delivery_keys BEGIN
 SELECT CASE WHEN EXISTS(SELECT 1 FROM outcome_deliveries d WHERE d.tenant=NEW.tenant AND d.environment=NEW.environment AND d.source=NEW.source AND d.external_id=NEW.external_id) THEN RAISE(ABORT,'delivery identity already retained by outcome') END;
END;
CREATE TRIGGER outcome_records_immutable_update BEFORE UPDATE ON outcome_records BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TRIGGER outcome_records_immutable_delete BEFORE DELETE ON outcome_records BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TRIGGER outcome_deliveries_immutable_update BEFORE UPDATE ON outcome_deliveries BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TRIGGER outcome_deliveries_immutable_delete BEFORE DELETE ON outcome_deliveries BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TABLE outcome_members (
 tenant TEXT NOT NULL, environment TEXT NOT NULL, target TEXT NOT NULL, invocation_id TEXT NOT NULL,
 kind TEXT NOT NULL, id BLOB NOT NULL, content_hash TEXT NOT NULL,
 PRIMARY KEY(tenant,environment,target,invocation_id,kind,id),
 FOREIGN KEY(tenant,environment,kind,id,content_hash) REFERENCES outcome_records(tenant,environment,kind,id,content_hash) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER outcome_members_immutable_update BEFORE UPDATE ON outcome_members BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TRIGGER outcome_members_immutable_delete BEFORE DELETE ON outcome_members BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TABLE outcome_anchors (
 tenant TEXT NOT NULL, environment TEXT NOT NULL, target TEXT NOT NULL, invocation_id TEXT NOT NULL,
 kind TEXT NOT NULL, id BLOB NOT NULL, content_hash TEXT NOT NULL,
 PRIMARY KEY(tenant,environment,target,invocation_id,kind,id),
 FOREIGN KEY(tenant,environment,kind,id,content_hash) REFERENCES outcome_records(tenant,environment,kind,id,content_hash) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TRIGGER outcome_anchors_immutable_update BEFORE UPDATE ON outcome_anchors BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
CREATE TRIGGER outcome_anchors_immutable_delete BEFORE DELETE ON outcome_anchors BEGIN SELECT RAISE(ABORT,'immutable outcome storage'); END;
PRAGMA user_version=4;
