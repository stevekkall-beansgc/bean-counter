-- First-slice physical schema; canonical/economic validation belongs to the coordinator.
CREATE TABLE installation (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), tenant TEXT NOT NULL COLLATE BINARY,
 environment TEXT NOT NULL COLLATE BINARY, logical_store_id TEXT NOT NULL,
 mode TEXT NOT NULL CHECK(mode IN ('sandbox','real')),
 admission TEXT NOT NULL CHECK(admission IN ('open','frozen','import_incomplete','retired')),
 dispatch_hold INTEGER NOT NULL CHECK(dispatch_hold IN (0,1)),
 dispatch_enabled INTEGER NOT NULL CHECK(dispatch_enabled IN (0,1)),
 logical_schema INTEGER NOT NULL CHECK(logical_schema=1), generation INTEGER NOT NULL CHECK(generation>=0)
) STRICT;
CREATE TABLE documents (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=68 AND substr(id,1,4)='doc_' AND substr(id,5) NOT GLOB '*[^0-9a-f]*'),
  kind TEXT NOT NULL CHECK(kind IN ('policy','roles','assent','source-grant','context','binding','snapshot')),
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 262144), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE parties (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  role_metadata_doc TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  FOREIGN KEY(tenant,environment,role_metadata_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE source_grants (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  principal_id TEXT NOT NULL COLLATE BINARY,
  source TEXT NOT NULL COLLATE BINARY,
  grant_doc TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  FOREIGN KEY(tenant,environment,grant_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE bindings (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  agreement_id TEXT NOT NULL COLLATE BINARY,
  version INTEGER NOT NULL CHECK(version BETWEEN 1 AND 9223372036854775807),
  policy_doc TEXT NOT NULL COLLATE BINARY,
  assent_doc TEXT NOT NULL COLLATE BINARY,
  roles_doc TEXT NOT NULL COLLATE BINARY,
  context_doc TEXT NOT NULL COLLATE BINARY,
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency NOT GLOB '*[^A-Z]*'),
  scale INTEGER NOT NULL CHECK(scale BETWEEN 0 AND 18),
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,agreement_id,version),
  FOREIGN KEY(tenant,environment,policy_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,assent_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,roles_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,context_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE authority_heads (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  grant_id TEXT NOT NULL COLLATE BINARY,
  revision INTEGER NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 1),
  FOREIGN KEY(tenant,environment,grant_id) REFERENCES source_grants(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE binding_heads (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  selector_doc TEXT NOT NULL COLLATE BINARY,
  binding_id TEXT NOT NULL COLLATE BINARY,
  revision INTEGER NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  active INTEGER NOT NULL CHECK(active BETWEEN 0 AND 1),
  FOREIGN KEY(tenant,environment,selector_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,binding_id) REFERENCES bindings(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE chains (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY,
  customer TEXT NOT NULL COLLATE BINARY,
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency NOT GLOB '*[^A-Z]*'),
  scale INTEGER NOT NULL CHECK(scale BETWEEN 0 AND 18),
  binding_set_doc TEXT NOT NULL COLLATE BINARY,
  context_doc TEXT NOT NULL COLLATE BINARY,
  revision INTEGER NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  event_count INTEGER NOT NULL CHECK(event_count BETWEEN 0 AND 1000),
  FOREIGN KEY(tenant,environment,customer) REFERENCES parties(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,binding_set_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,context_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE events (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='ev_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  source TEXT NOT NULL COLLATE BINARY,
  external_id TEXT NOT NULL COLLATE BINARY,
  operation_id TEXT NOT NULL COLLATE BINARY,
  kind TEXT NOT NULL COLLATE BINARY,
  chain_id TEXT NOT NULL COLLATE BINARY,
  decision_id TEXT NOT NULL COLLATE BINARY,
  ingress_hash TEXT NOT NULL COLLATE BINARY,
  claim_facts_hash TEXT NOT NULL COLLATE BINARY,
  ingress_bytes BLOB NOT NULL CHECK(length(ingress_bytes)<=262144),
  occurred_us INTEGER,
  received_us INTEGER NOT NULL,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 262144), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,external_id),
  UNIQUE(tenant,environment,decision_id),
  FOREIGN KEY(tenant,environment,chain_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,source,external_id,id) REFERENCES delivery_keys(tenant,environment,source,external_id,canonical_event_id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE snapshots (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='sr_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  document_id TEXT NOT NULL COLLATE BINARY,
  purpose TEXT NOT NULL CHECK(purpose IN ('policy','roles','assent','source_grant','binding','chain_context','decision_snapshot')),
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id,purpose,document_id),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,document_id) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE delivery_keys (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  source TEXT NOT NULL COLLATE BINARY,
  external_id TEXT NOT NULL COLLATE BINARY,
  ingress_hash TEXT NOT NULL COLLATE BINARY,
  canonical_event_id TEXT NOT NULL COLLATE BINARY,
  kind TEXT NOT NULL CHECK(kind IN ('original','alias')),
  observed_us INTEGER NOT NULL,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,external_id,canonical_event_id),
  FOREIGN KEY(tenant,environment,canonical_event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,source,external_id)
) STRICT;

CREATE TABLE claims (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='cl_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  source TEXT NOT NULL COLLATE BINARY,
  operation_id TEXT NOT NULL COLLATE BINARY,
  kind TEXT NOT NULL COLLATE BINARY,
  token TEXT NOT NULL COLLATE BINARY,
  facts_hash TEXT NOT NULL COLLATE BINARY,
  event_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,operation_id,kind,token),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE effects (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='ef_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  agreement_id TEXT NOT NULL COLLATE BINARY,
  component TEXT NOT NULL COLLATE BINARY,
  claim_id TEXT NOT NULL COLLATE BINARY,
  namespace TEXT NOT NULL COLLATE BINARY,
  facts_hash TEXT NOT NULL COLLATE BINARY,
  action_id TEXT NOT NULL COLLATE BINARY,
  match_key_bytes BLOB NOT NULL,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,agreement_id,component,claim_id,match_key_bytes,namespace),
  UNIQUE(tenant,environment,action_id),
  FOREIGN KEY(tenant,environment,claim_id) REFERENCES claims(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE actions (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='ac_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  decision_id TEXT NOT NULL COLLATE BINARY,
  effect_id TEXT NOT NULL COLLATE BINARY,
  obligation_id TEXT NOT NULL COLLATE BINARY,
  kind TEXT NOT NULL COLLATE BINARY,
  book TEXT NOT NULL COLLATE BINARY,
  component TEXT NOT NULL COLLATE BINARY,
  binding_id TEXT NOT NULL COLLATE BINARY,
  snapshot_doc TEXT NOT NULL COLLATE BINARY,
  roles_doc TEXT NOT NULL COLLATE BINARY,
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency NOT GLOB '*[^A-Z]*'),
  scale INTEGER NOT NULL CHECK(scale BETWEEN 0 AND 18),
  atoms TEXT NOT NULL CHECK(atoms <> '0' AND ((length(atoms) BETWEEN 1 AND 30 AND substr(atoms,1,1) BETWEEN '1' AND '9' AND atoms NOT GLOB '*[^0-9]*') OR (length(atoms) BETWEEN 2 AND 31 AND substr(atoms,1,1)='-' AND substr(atoms,2,1) BETWEEN '1' AND '9' AND substr(atoms,2) NOT GLOB '*[^0-9]*'))),
  reverses TEXT,
  allocation_parent TEXT,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,effect_id),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,effect_id) REFERENCES effects(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,binding_id) REFERENCES bindings(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,snapshot_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,roles_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,reverses) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,allocation_parent) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE action_sources (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  action_id TEXT NOT NULL COLLATE BINARY,
  event_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,action_id,event_id)
) STRICT;

CREATE TABLE action_dependencies (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  action_id TEXT NOT NULL COLLATE BINARY,
  input_action_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  CHECK(action_id<>input_action_id),
  FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,input_action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,action_id,input_action_id)
) STRICT;

CREATE TABLE explanations (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='xp_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 255),
  code TEXT NOT NULL COLLATE BINARY,
  rule_id TEXT,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id,ordinal),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE intentions (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='in_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  obligation_id TEXT NOT NULL COLLATE BINARY,
  destination_id TEXT NOT NULL COLLATE BINARY,
  idempotency_key TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,destination_id,idempotency_key),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE control_transitions (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='ct_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  control_kind TEXT NOT NULL CHECK(control_kind='chain'),
  control_id TEXT NOT NULL COLLATE BINARY,
  from_revision INTEGER NOT NULL CHECK(from_revision BETWEEN 0 AND 9223372036854775807),
  to_revision INTEGER NOT NULL CHECK(to_revision BETWEEN 1 AND 9223372036854775807),
  from_event_count INTEGER NOT NULL CHECK(from_event_count BETWEEN 0 AND 999),
  to_event_count INTEGER NOT NULL CHECK(to_event_count BETWEEN 1 AND 1000),
  event_id TEXT NOT NULL COLLATE BINARY,
  document_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  CHECK(to_revision=from_revision+1 AND to_event_count=from_event_count+1),
  UNIQUE(tenant,environment,control_kind,control_id,to_revision),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,document_id) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,control_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE chain_revisions (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  chain_id TEXT NOT NULL COLLATE BINARY,
  revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9223372036854775807),
  event_id TEXT NOT NULL COLLATE BINARY,
  decision_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  FOREIGN KEY(tenant,environment,chain_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,chain_id,revision)
) STRICT;

CREATE TABLE decision_manifests (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='dc_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  chain_id TEXT NOT NULL COLLATE BINARY,
  revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9223372036854775807),
  decision_hash TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,chain_id,revision) REFERENCES chain_revisions(tenant,environment,chain_id,revision) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE accepted_receipts (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  id TEXT NOT NULL COLLATE BINARY CHECK(length(id)=67 AND substr(id,1,3)='rc_' AND substr(id,4) NOT GLOB '*[^0-9a-f]*'),
  event_id TEXT NOT NULL COLLATE BINARY,
  decision_id TEXT NOT NULL COLLATE BINARY,
  canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) NOT GLOB '*[^0-9a-f]*'), schema_version INTEGER NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,id)
) STRICT;

CREATE TABLE delivery_state (
  tenant TEXT NOT NULL COLLATE BINARY, environment TEXT NOT NULL COLLATE BINARY,
  intention_id TEXT NOT NULL COLLATE BINARY,
  state TEXT NOT NULL CHECK(state IN ('held','pending','leased','delivered','retry','unknown','rejected')),
  attempts INTEGER NOT NULL CHECK(attempts BETWEEN 0 AND 9223372036854775807),
  next_attempt_us INTEGER NOT NULL,
  lease_owner TEXT,
  generation INTEGER NOT NULL CHECK(generation BETWEEN 0 AND 9223372036854775807),
  lease_until_us INTEGER,
  last_observation TEXT,
  CHECK((state='leased' AND lease_owner IS NOT NULL AND lease_until_us IS NOT NULL) OR (state<>'leased' AND lease_owner IS NULL AND lease_until_us IS NULL)),
  FOREIGN KEY(tenant,environment,intention_id) REFERENCES intentions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED,
  PRIMARY KEY(tenant,environment,intention_id)
) STRICT;
CREATE INDEX events_chain ON events(tenant,environment,chain_id,id);
CREATE INDEX grants_principal ON source_grants(tenant,environment,principal_id,source);
CREATE INDEX action_input ON action_dependencies(tenant,environment,input_action_id,action_id);
CREATE UNIQUE INDEX one_reversal ON actions(tenant,environment,reverses) WHERE reverses IS NOT NULL;
CREATE INDEX delivery_due ON delivery_state(state,next_attempt_us,intention_id);
CREATE TRIGGER documents_no_update BEFORE UPDATE ON documents BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER documents_no_delete BEFORE DELETE ON documents BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER parties_no_update BEFORE UPDATE ON parties BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER parties_no_delete BEFORE DELETE ON parties BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER source_grants_no_update BEFORE UPDATE ON source_grants BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER source_grants_no_delete BEFORE DELETE ON source_grants BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER bindings_no_update BEFORE UPDATE ON bindings BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER bindings_no_delete BEFORE DELETE ON bindings BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER events_no_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER snapshots_no_update BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER snapshots_no_delete BEFORE DELETE ON snapshots BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_keys_no_update BEFORE UPDATE ON delivery_keys BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER delivery_keys_no_delete BEFORE DELETE ON delivery_keys BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER claims_no_update BEFORE UPDATE ON claims BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER claims_no_delete BEFORE DELETE ON claims BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER effects_no_update BEFORE UPDATE ON effects BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER effects_no_delete BEFORE DELETE ON effects BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER actions_no_update BEFORE UPDATE ON actions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER actions_no_delete BEFORE DELETE ON actions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER action_sources_no_update BEFORE UPDATE ON action_sources BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER action_sources_no_delete BEFORE DELETE ON action_sources BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER action_dependencies_no_update BEFORE UPDATE ON action_dependencies BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER action_dependencies_no_delete BEFORE DELETE ON action_dependencies BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER explanations_no_update BEFORE UPDATE ON explanations BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER explanations_no_delete BEFORE DELETE ON explanations BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER intentions_no_update BEFORE UPDATE ON intentions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER intentions_no_delete BEFORE DELETE ON intentions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER control_transitions_no_update BEFORE UPDATE ON control_transitions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER control_transitions_no_delete BEFORE DELETE ON control_transitions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER chain_revisions_no_update BEFORE UPDATE ON chain_revisions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER chain_revisions_no_delete BEFORE DELETE ON chain_revisions BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER decision_manifests_no_update BEFORE UPDATE ON decision_manifests BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER decision_manifests_no_delete BEFORE DELETE ON decision_manifests BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER accepted_receipts_no_update BEFORE UPDATE ON accepted_receipts BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER accepted_receipts_no_delete BEFORE DELETE ON accepted_receipts BEGIN SELECT RAISE(ABORT,'IMMUTABLE_RECORD'); END;
CREATE TRIGGER installation_identity BEFORE UPDATE OF tenant,environment,logical_store_id,mode ON installation BEGIN SELECT RAISE(ABORT,'IMMUTABLE_STORE_IDENTITY'); END;
CREATE TRIGGER chain_identity BEFORE UPDATE OF tenant,environment,id,customer,currency,scale,binding_set_doc,context_doc ON chains BEGIN SELECT RAISE(ABORT,'IMMUTABLE_CHAIN_CONTEXT'); END;
CREATE TABLE dispatcher_head (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 owner TEXT, generation INTEGER NOT NULL CHECK(generation>=0),
 lease_until_us INTEGER, enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 CHECK((owner IS NULL AND lease_until_us IS NULL) OR (owner IS NOT NULL AND lease_until_us IS NOT NULL))
) STRICT;
INSERT INTO dispatcher_head (singleton,generation,enabled) VALUES (1,0,0);
PRAGMA user_version=1;
