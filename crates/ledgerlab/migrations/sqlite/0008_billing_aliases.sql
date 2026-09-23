CREATE TABLE billing_aliases (
 source TEXT NOT NULL,
 external_id TEXT NOT NULL,
 ingress BLOB NOT NULL CHECK(length(ingress)<=262144),
 ordinal INTEGER NOT NULL REFERENCES billing_entries(ordinal),
 PRIMARY KEY(source,external_id)
) STRICT;
CREATE TRIGGER billing_alias_immutable_update BEFORE UPDATE ON billing_aliases BEGIN SELECT RAISE(ABORT,'immutable billing alias'); END;
CREATE TRIGGER billing_alias_immutable_delete BEFORE DELETE ON billing_aliases BEGIN SELECT RAISE(ABORT,'immutable billing alias'); END;
CREATE TRIGGER billing_alias_identity BEFORE INSERT ON billing_aliases WHEN EXISTS(SELECT 1 FROM billing_entries WHERE source=NEW.source AND external_id=NEW.external_id) BEGIN SELECT RAISE(ABORT,'billing identity conflict'); END;
CREATE TRIGGER billing_entry_identity BEFORE INSERT ON billing_entries WHEN EXISTS(SELECT 1 FROM billing_aliases WHERE source=NEW.source AND external_id=NEW.external_id) BEGIN SELECT RAISE(ABORT,'billing identity conflict'); END;
CREATE TABLE billing_permissions (
 revision INTEGER PRIMARY KEY CHECK(revision BETWEEN 2 AND 1001),
 canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes)<=65536)
) STRICT;
CREATE TRIGGER billing_permissions_immutable_update BEFORE UPDATE ON billing_permissions BEGIN SELECT RAISE(ABORT,'immutable billing permission'); END;
CREATE TRIGGER billing_permissions_immutable_delete BEFORE DELETE ON billing_permissions BEGIN SELECT RAISE(ABORT,'immutable billing permission'); END;
PRAGMA user_version=8;
