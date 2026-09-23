-- Ordinary local billing profile. Economic bundles retain frozen v2 envelopes.
-- No supplier reservation or R3 resource capability is represented here.
CREATE TABLE billing_setup (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes)<=65536)
) STRICT;
CREATE TABLE billing_entries (
  ordinal INTEGER PRIMARY KEY CHECK(ordinal>0 AND ordinal<=1000),
  source TEXT NOT NULL,
  external_id TEXT NOT NULL,
  semantic_key BLOB NOT NULL,
  ingress BLOB NOT NULL CHECK(length(ingress)<=262144),
  facts BLOB NOT NULL CHECK(length(facts)<=262144),
  bundle BLOB NOT NULL CHECK(length(bundle)<=8388608),
  UNIQUE(source,external_id), UNIQUE(source,semantic_key)
) STRICT;
CREATE TRIGGER billing_setup_no_update BEFORE UPDATE ON billing_setup BEGIN SELECT RAISE(ABORT,'immutable billing setup'); END;
CREATE TRIGGER billing_setup_no_delete BEFORE DELETE ON billing_setup BEGIN SELECT RAISE(ABORT,'immutable billing setup'); END;
CREATE TRIGGER billing_entries_no_update BEFORE UPDATE ON billing_entries BEGIN SELECT RAISE(ABORT,'immutable billing entry'); END;
CREATE TRIGGER billing_entries_no_delete BEFORE DELETE ON billing_entries BEGIN SELECT RAISE(ABORT,'immutable billing entry'); END;
PRAGMA user_version=7;
