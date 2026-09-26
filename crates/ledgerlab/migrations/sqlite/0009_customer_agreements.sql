-- M2 is additive: original setup, identities, receipts and canonical bundles
-- remain in their schema-8 tables. New records live in scoped sidecars.
CREATE TABLE billing_customers (
 customer TEXT PRIMARY KEY CHECK(length(CAST(customer AS BLOB)) BETWEEN 1 AND 128),
 tenant TEXT NOT NULL CHECK(length(CAST(tenant AS BLOB)) BETWEEN 1 AND 128),
 environment TEXT NOT NULL CHECK(length(CAST(environment AS BLOB)) BETWEEN 1 AND 128),
 UNIQUE(tenant,environment)
) STRICT;
CREATE TABLE billing_agreements (
 customer TEXT NOT NULL REFERENCES billing_customers(customer),
 source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
 revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 1000),
 agreement_id TEXT NOT NULL CHECK(length(CAST(agreement_id AS BLOB)) BETWEEN 1 AND 128),
 agreement_version INTEGER NOT NULL CHECK(agreement_version BETWEEN 1 AND 1000),
 transition TEXT NOT NULL CHECK(transition IN ('start','amend','end')),
 effective_at_us INTEGER NOT NULL CHECK(effective_at_us BETWEEN -62135596800000000 AND 253402300799999999),
 recorded_at_us INTEGER NOT NULL CHECK(recorded_at_us BETWEEN -62135596800000000 AND 253402300799999999),
 setup_bytes BLOB CHECK(length(setup_bytes) BETWEEN 1 AND 65536),
 CHECK((transition='end' AND setup_bytes IS NULL) OR (transition<>'end' AND setup_bytes IS NOT NULL)),
 PRIMARY KEY(customer,source,revision)
) STRICT;
CREATE UNIQUE INDEX billing_agreement_versions ON billing_agreements(customer,source,agreement_id,agreement_version) WHERE transition<>'end';
CREATE TABLE billing_m2_changes (
 customer TEXT NOT NULL REFERENCES billing_customers(customer),
 source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
 change_id TEXT NOT NULL CHECK(length(CAST(change_id AS BLOB)) BETWEEN 1 AND 128),
 operation TEXT NOT NULL CHECK(operation IN ('start','amend','end','permissions')),
 request BLOB NOT NULL CHECK(length(request) BETWEEN 1 AND 65536),
 response BLOB NOT NULL CHECK(length(response) BETWEEN 1 AND 65536),
 recorded_at_us INTEGER NOT NULL CHECK(recorded_at_us BETWEEN -62135596800000000 AND 253402300799999999),
 PRIMARY KEY(customer,source,change_id)
) STRICT;
CREATE TABLE billing_m2_permissions (
 customer TEXT NOT NULL REFERENCES billing_customers(customer),
 source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
 revision INTEGER NOT NULL CHECK(revision BETWEEN 2 AND 1001),
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 1 AND 65536),
 recorded_at_us INTEGER NOT NULL CHECK(recorded_at_us BETWEEN -62135596800000000 AND 253402300799999999),
 PRIMARY KEY(customer,source,revision)
) STRICT;
CREATE TABLE billing_m2_entries (
 ordinal INTEGER PRIMARY KEY CHECK(ordinal BETWEEN 1 AND 1000),
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
 UNIQUE(customer,source,external_id), UNIQUE(customer,source,semantic_key)
) STRICT;
CREATE TABLE billing_m2_aliases (
 customer TEXT NOT NULL REFERENCES billing_customers(customer),
 source TEXT NOT NULL CHECK(length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
 external_id TEXT NOT NULL CHECK(length(CAST(external_id AS BLOB)) BETWEEN 1 AND 128),
 ingress BLOB NOT NULL CHECK(length(ingress) BETWEEN 1 AND 262144),
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 1000),
 PRIMARY KEY(customer,source,external_id)
) STRICT;

-- Triggers also reject INSERT OR REPLACE; SQLite does not necessarily invoke
-- DELETE triggers for REPLACE when recursive_triggers is disabled.
CREATE TRIGGER billing_customers_insert_guard BEFORE INSERT ON billing_customers BEGIN
 SELECT CASE WHEN (SELECT count(*) FROM billing_customers)>=1000 OR EXISTS(SELECT 1 FROM billing_customers WHERE customer=NEW.customer OR (tenant=NEW.tenant AND environment=NEW.environment)) THEN RAISE(ABORT,'billing customer bound or identity') END;
END;
CREATE TRIGGER billing_agreements_insert_guard BEFORE INSERT ON billing_agreements BEGIN
 SELECT CASE WHEN (SELECT count(*) FROM billing_agreements)>=1000 OR NEW.revision<>1+COALESCE((SELECT max(revision) FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source),0) THEN RAISE(ABORT,'billing agreement bound or revision') END;
 SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source AND effective_at_us>=NEW.effective_at_us) THEN RAISE(ABORT,'billing agreement effective order') END;
 SELECT CASE WHEN NEW.revision>1 AND NEW.effective_at_us<NEW.recorded_at_us THEN RAISE(ABORT,'billing agreement retroactive') END;
 SELECT CASE WHEN NEW.revision=1 AND (NEW.transition<>'start' OR NEW.agreement_version<>1) THEN RAISE(ABORT,'billing agreement initial transition') END;
 SELECT CASE WHEN NEW.revision>1 AND NOT EXISTS(SELECT 1 FROM billing_agreements a WHERE a.customer=NEW.customer AND a.source=NEW.source AND a.revision=NEW.revision-1 AND ((NEW.transition='start' AND a.transition='end' AND NEW.agreement_version=1 AND NEW.agreement_id<>a.agreement_id) OR (NEW.transition='amend' AND a.transition<>'end' AND NEW.agreement_id=a.agreement_id AND NEW.agreement_version=a.agreement_version+1) OR (NEW.transition='end' AND a.transition<>'end' AND NEW.agreement_id=a.agreement_id AND NEW.agreement_version=a.agreement_version))) THEN RAISE(ABORT,'billing agreement transition') END;
 SELECT CASE WHEN NEW.transition<>'end' AND EXISTS(SELECT 1 FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source AND agreement_id=NEW.agreement_id AND agreement_version=NEW.agreement_version) THEN RAISE(ABORT,'billing agreement version identity') END;
END;
CREATE TRIGGER billing_m2_changes_insert_guard BEFORE INSERT ON billing_m2_changes BEGIN
 SELECT CASE WHEN (SELECT count(*) FROM billing_m2_changes)>=2000 OR EXISTS(SELECT 1 FROM billing_m2_changes WHERE customer=NEW.customer AND source=NEW.source AND change_id=NEW.change_id) THEN RAISE(ABORT,'billing control bound or identity') END;
END;
CREATE TRIGGER billing_m2_permissions_insert_guard BEFORE INSERT ON billing_m2_permissions BEGIN
 SELECT CASE WHEN (SELECT count(*) FROM billing_permissions)+(SELECT count(*) FROM billing_m2_permissions)>=1000 THEN RAISE(ABORT,'billing permission bound') END;
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source) OR NEW.revision<>1+COALESCE((SELECT max(revision) FROM (SELECT revision FROM billing_m2_permissions WHERE customer=NEW.customer AND source=NEW.source UNION ALL SELECT p.revision FROM billing_permissions p WHERE EXISTS(SELECT 1 FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1 AND a.customer=NEW.customer AND a.source=NEW.source))),1) THEN RAISE(ABORT,'billing permission revision') END;
END;
CREATE TRIGGER billing_m2_entries_insert_guard BEFORE INSERT ON billing_m2_entries BEGIN
 SELECT CASE WHEN NEW.ordinal<>1+MAX(COALESCE((SELECT max(ordinal) FROM billing_entries),0),COALESCE((SELECT max(ordinal) FROM billing_m2_entries),0)) OR (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)>=1000 THEN RAISE(ABORT,'billing entry ordinal or bound') END;
 SELECT CASE WHEN length(NEW.bundle)+length(NEW.ingress)+length(NEW.facts)+length(NEW.semantic_key)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m2_entries),0)>33554432 THEN RAISE(ABORT,'billing entry byte bound') END;
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM billing_agreements WHERE customer=NEW.customer AND source=NEW.source AND agreement_id=NEW.agreement_id AND agreement_version=NEW.agreement_version AND transition<>'end') THEN RAISE(ABORT,'billing entry agreement') END;
 SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_m2_entries WHERE customer=NEW.customer AND source=NEW.source AND (external_id=NEW.external_id OR semantic_key=NEW.semantic_key)) OR EXISTS(SELECT 1 FROM billing_m2_aliases WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1 AND a.customer=NEW.customer AND a.source=NEW.source AND (EXISTS(SELECT 1 FROM billing_entries WHERE source=NEW.source AND (external_id=NEW.external_id OR semantic_key=NEW.semantic_key)) OR EXISTS(SELECT 1 FROM billing_aliases WHERE source=NEW.source AND external_id=NEW.external_id))) THEN RAISE(ABORT,'billing entry identity') END;
END;
CREATE TRIGGER billing_m2_aliases_insert_guard BEFORE INSERT ON billing_m2_aliases BEGIN
 SELECT CASE WHEN (SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)>=1000 OR length(NEW.ingress)+COALESCE((SELECT sum(length(ingress)) FROM billing_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m2_aliases),0)>33554432 THEN RAISE(ABORT,'billing alias bound') END;
 SELECT CASE WHEN NOT EXISTS(SELECT 1 FROM billing_m2_entries WHERE ordinal=NEW.ordinal AND customer=NEW.customer AND source=NEW.source) AND NOT EXISTS(SELECT 1 FROM billing_entries e JOIN billing_agreements a ON a.source=e.source JOIN billing_setup s ON s.canonical_bytes=a.setup_bytes WHERE e.ordinal=NEW.ordinal AND a.revision=1 AND a.customer=NEW.customer AND a.source=NEW.source) THEN RAISE(ABORT,'billing alias scoped target') END;
 SELECT CASE WHEN EXISTS(SELECT 1 FROM billing_m2_entries WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_m2_aliases WHERE customer=NEW.customer AND source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1 AND a.customer=NEW.customer AND a.source=NEW.source AND (EXISTS(SELECT 1 FROM billing_entries WHERE source=NEW.source AND external_id=NEW.external_id) OR EXISTS(SELECT 1 FROM billing_aliases WHERE source=NEW.source AND external_id=NEW.external_id))) THEN RAISE(ABORT,'billing alias identity') END;
END;
CREATE TRIGGER billing_legacy_entries_frozen BEFORE INSERT ON billing_entries BEGIN SELECT RAISE(ABORT,'schema 8 billing history is frozen'); END;
CREATE TRIGGER billing_legacy_aliases_frozen BEFORE INSERT ON billing_aliases BEGIN SELECT RAISE(ABORT,'schema 8 billing history is frozen'); END;
CREATE TRIGGER billing_legacy_permissions_frozen BEFORE INSERT ON billing_permissions BEGIN SELECT RAISE(ABORT,'schema 8 billing history is frozen'); END;
CREATE TRIGGER billing_customers_immutable_update BEFORE UPDATE ON billing_customers BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_customers_immutable_delete BEFORE DELETE ON billing_customers BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_agreements_immutable_update BEFORE UPDATE ON billing_agreements BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_agreements_immutable_delete BEFORE DELETE ON billing_agreements BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_changes_immutable_update BEFORE UPDATE ON billing_m2_changes BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_changes_immutable_delete BEFORE DELETE ON billing_m2_changes BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_permissions_immutable_update BEFORE UPDATE ON billing_m2_permissions BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_permissions_immutable_delete BEFORE DELETE ON billing_m2_permissions BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_entries_immutable_update BEFORE UPDATE ON billing_m2_entries BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_entries_immutable_delete BEFORE DELETE ON billing_m2_entries BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_aliases_immutable_update BEFORE UPDATE ON billing_m2_aliases BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
CREATE TRIGGER billing_m2_aliases_immutable_delete BEFORE DELETE ON billing_m2_aliases BEGIN SELECT RAISE(ABORT,'immutable billing M2 record'); END;
PRAGMA user_version=9;
