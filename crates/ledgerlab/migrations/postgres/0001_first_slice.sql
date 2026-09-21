-- PostgreSQL 17/18 first slice. Run only by the explicit migration owner.
-- Runtime queries always qualify ledgerlab; startup search_path is pg_catalog.
CREATE SCHEMA ledgerlab;
REVOKE ALL ON SCHEMA ledgerlab FROM PUBLIC;
SET LOCAL search_path=ledgerlab,pg_catalog;
-- First-slice physical schema; canonical/economic validation belongs to the coordinator.
CREATE TABLE installation (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), tenant TEXT NOT NULL COLLATE "C",
 environment TEXT NOT NULL COLLATE "C", logical_store_id TEXT NOT NULL,
 mode TEXT NOT NULL CHECK(mode IN ('sandbox','real')),
 admission TEXT NOT NULL CHECK(admission IN ('open','frozen','import_incomplete','retired')),
 dispatch_hold BIGINT NOT NULL CHECK(dispatch_hold IN (0,1)),
 dispatch_enabled BIGINT NOT NULL CHECK(dispatch_enabled IN (0,1)),
 logical_schema BIGINT NOT NULL CHECK(logical_schema=1), generation BIGINT NOT NULL CHECK(generation>=0)
);
CREATE TABLE documents (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=68 AND substr(id,1,4)='doc_' AND substr(id,5) !~ '[^0-9a-f]'),
  kind TEXT NOT NULL CHECK(kind IN ('policy','roles','assent','source-grant','context','binding','snapshot')),
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 262144), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE parties (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  role_metadata_doc TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE source_grants (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  principal_id TEXT NOT NULL COLLATE "C",
  source TEXT NOT NULL COLLATE "C",
  grant_doc TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE bindings (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  agreement_id TEXT NOT NULL COLLATE "C",
  version BIGINT NOT NULL CHECK(version BETWEEN 1 AND 9223372036854775807),
  policy_doc TEXT NOT NULL COLLATE "C",
  assent_doc TEXT NOT NULL COLLATE "C",
  roles_doc TEXT NOT NULL COLLATE "C",
  context_doc TEXT NOT NULL COLLATE "C",
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency !~ '[^A-Z]'),
  scale BIGINT NOT NULL CHECK(scale BETWEEN 0 AND 18),
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,agreement_id,version),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE authority_heads (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  grant_id TEXT NOT NULL COLLATE "C",
  revision BIGINT NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  active BIGINT NOT NULL CHECK(active BETWEEN 0 AND 1),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE binding_heads (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  selector_doc TEXT NOT NULL COLLATE "C",
  binding_id TEXT NOT NULL COLLATE "C",
  revision BIGINT NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  active BIGINT NOT NULL CHECK(active BETWEEN 0 AND 1),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE chains (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C",
  customer TEXT NOT NULL COLLATE "C",
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency !~ '[^A-Z]'),
  scale BIGINT NOT NULL CHECK(scale BETWEEN 0 AND 18),
  binding_set_doc TEXT NOT NULL COLLATE "C",
  context_doc TEXT NOT NULL COLLATE "C",
  revision BIGINT NOT NULL CHECK(revision BETWEEN 0 AND 9223372036854775807),
  event_count BIGINT NOT NULL CHECK(event_count BETWEEN 0 AND 1000),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE events (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='ev_' AND substr(id,4) !~ '[^0-9a-f]'),
  source TEXT NOT NULL COLLATE "C",
  external_id TEXT NOT NULL COLLATE "C",
  operation_id TEXT NOT NULL COLLATE "C",
  kind TEXT NOT NULL COLLATE "C",
  chain_id TEXT NOT NULL COLLATE "C",
  decision_id TEXT NOT NULL COLLATE "C",
  ingress_hash TEXT NOT NULL COLLATE "C",
  claim_facts_hash TEXT NOT NULL COLLATE "C",
  ingress_bytes BYTEA NOT NULL CHECK(length(ingress_bytes)<=262144),
  occurred_us BIGINT,
  received_us BIGINT NOT NULL,
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 262144), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,external_id),
  UNIQUE(tenant,environment,decision_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE snapshots (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='sr_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  document_id TEXT NOT NULL COLLATE "C",
  purpose TEXT NOT NULL CHECK(purpose IN ('policy','roles','assent','source_grant','binding','chain_context','decision_snapshot')),
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id,purpose,document_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE delivery_keys (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  source TEXT NOT NULL COLLATE "C",
  external_id TEXT NOT NULL COLLATE "C",
  ingress_hash TEXT NOT NULL COLLATE "C",
  canonical_event_id TEXT NOT NULL COLLATE "C",
  kind TEXT NOT NULL CHECK(kind IN ('original','alias')),
  observed_us BIGINT NOT NULL,
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,external_id,canonical_event_id),
  PRIMARY KEY(tenant,environment,source,external_id)
);

CREATE TABLE claims (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='cl_' AND substr(id,4) !~ '[^0-9a-f]'),
  source TEXT NOT NULL COLLATE "C",
  operation_id TEXT NOT NULL COLLATE "C",
  kind TEXT NOT NULL COLLATE "C",
  token TEXT NOT NULL COLLATE "C",
  facts_hash TEXT NOT NULL COLLATE "C",
  event_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,source,operation_id,kind,token),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE effects (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='ef_' AND substr(id,4) !~ '[^0-9a-f]'),
  agreement_id TEXT NOT NULL COLLATE "C",
  component TEXT NOT NULL COLLATE "C",
  claim_id TEXT NOT NULL COLLATE "C",
  namespace TEXT NOT NULL COLLATE "C",
  facts_hash TEXT NOT NULL COLLATE "C",
  action_id TEXT NOT NULL COLLATE "C",
  match_key_bytes BYTEA NOT NULL,
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,agreement_id,component,claim_id,match_key_bytes,namespace),
  UNIQUE(tenant,environment,action_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE actions (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='ac_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  decision_id TEXT NOT NULL COLLATE "C",
  effect_id TEXT NOT NULL COLLATE "C",
  obligation_id TEXT NOT NULL COLLATE "C",
  kind TEXT NOT NULL COLLATE "C",
  book TEXT NOT NULL COLLATE "C",
  component TEXT NOT NULL COLLATE "C",
  binding_id TEXT NOT NULL COLLATE "C",
  snapshot_doc TEXT NOT NULL COLLATE "C",
  roles_doc TEXT NOT NULL COLLATE "C",
  currency TEXT NOT NULL CHECK(length(currency)=3 AND currency !~ '[^A-Z]'),
  scale BIGINT NOT NULL CHECK(scale BETWEEN 0 AND 18),
  atoms TEXT NOT NULL CHECK(atoms <> '0' AND ((length(atoms) BETWEEN 1 AND 30 AND substr(atoms,1,1) BETWEEN '1' AND '9' AND atoms !~ '[^0-9]') OR (length(atoms) BETWEEN 2 AND 31 AND substr(atoms,1,1)='-' AND substr(atoms,2,1) BETWEEN '1' AND '9' AND substr(atoms,2) !~ '[^0-9]'))),
  reverses TEXT,
  allocation_parent TEXT,
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,effect_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE action_sources (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  action_id TEXT NOT NULL COLLATE "C",
  event_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  PRIMARY KEY(tenant,environment,action_id,event_id)
);

CREATE TABLE action_dependencies (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  action_id TEXT NOT NULL COLLATE "C",
  input_action_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  CHECK(action_id<>input_action_id),
  PRIMARY KEY(tenant,environment,action_id,input_action_id)
);

CREATE TABLE explanations (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='xp_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  ordinal BIGINT NOT NULL CHECK(ordinal BETWEEN 0 AND 255),
  code TEXT NOT NULL COLLATE "C",
  rule_id TEXT,
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id,ordinal),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE intentions (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='in_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  obligation_id TEXT NOT NULL COLLATE "C",
  destination_id TEXT NOT NULL COLLATE "C",
  idempotency_key TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,destination_id,idempotency_key),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE control_transitions (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='ct_' AND substr(id,4) !~ '[^0-9a-f]'),
  control_kind TEXT NOT NULL CHECK(control_kind='chain'),
  control_id TEXT NOT NULL COLLATE "C",
  from_revision BIGINT NOT NULL CHECK(from_revision BETWEEN 0 AND 9223372036854775807),
  to_revision BIGINT NOT NULL CHECK(to_revision BETWEEN 1 AND 9223372036854775807),
  from_event_count BIGINT NOT NULL CHECK(from_event_count BETWEEN 0 AND 999),
  to_event_count BIGINT NOT NULL CHECK(to_event_count BETWEEN 1 AND 1000),
  event_id TEXT NOT NULL COLLATE "C",
  document_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  CHECK(to_revision=from_revision+1 AND to_event_count=from_event_count+1),
  UNIQUE(tenant,environment,control_kind,control_id,to_revision),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE chain_revisions (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  chain_id TEXT NOT NULL COLLATE "C",
  revision BIGINT NOT NULL CHECK(revision BETWEEN 1 AND 9223372036854775807),
  event_id TEXT NOT NULL COLLATE "C",
  decision_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  PRIMARY KEY(tenant,environment,chain_id,revision)
);

CREATE TABLE decision_manifests (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='dc_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  chain_id TEXT NOT NULL COLLATE "C",
  revision BIGINT NOT NULL CHECK(revision BETWEEN 1 AND 9223372036854775807),
  decision_hash TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE accepted_receipts (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  id TEXT NOT NULL COLLATE "C" CHECK(length(id)=67 AND substr(id,1,3)='rc_' AND substr(id,4) !~ '[^0-9a-f]'),
  event_id TEXT NOT NULL COLLATE "C",
  decision_id TEXT NOT NULL COLLATE "C",
  canonical_bytes BYTEA NOT NULL CHECK(length(canonical_bytes) BETWEEN 2 AND 4194304), content_hash TEXT NOT NULL CHECK(length(content_hash)=71 AND substr(content_hash,1,7)='sha256:' AND substr(content_hash,8) !~ '[^0-9a-f]'), schema_version BIGINT NOT NULL CHECK(schema_version=1),
  UNIQUE(tenant,environment,event_id),
  PRIMARY KEY(tenant,environment,id)
);

CREATE TABLE delivery_state (
  tenant TEXT NOT NULL COLLATE "C", environment TEXT NOT NULL COLLATE "C",
  intention_id TEXT NOT NULL COLLATE "C",
  state TEXT NOT NULL CHECK(state IN ('held','pending','leased','delivered','retry','unknown','rejected')),
  attempts BIGINT NOT NULL CHECK(attempts BETWEEN 0 AND 9223372036854775807),
  next_attempt_us BIGINT NOT NULL,
  lease_owner TEXT,
  generation BIGINT NOT NULL CHECK(generation BETWEEN 0 AND 9223372036854775807),
  lease_until_us BIGINT,
  last_observation TEXT,
  CHECK((state='leased' AND lease_owner IS NOT NULL AND lease_until_us IS NOT NULL) OR (state<>'leased' AND lease_owner IS NULL AND lease_until_us IS NULL)),
  PRIMARY KEY(tenant,environment,intention_id)
);
CREATE INDEX events_chain ON events(tenant,environment,chain_id,id);
CREATE INDEX grants_principal ON source_grants(tenant,environment,principal_id,source);
CREATE INDEX action_input ON action_dependencies(tenant,environment,input_action_id,action_id);
CREATE UNIQUE INDEX one_reversal ON actions(tenant,environment,reverses) WHERE reverses IS NOT NULL;
CREATE INDEX delivery_due ON delivery_state(state,next_attempt_us,intention_id);
CREATE TABLE dispatcher_head (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1),
 owner TEXT, generation BIGINT NOT NULL CHECK(generation>=0),
 lease_until_us BIGINT, enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
 CHECK((owner IS NULL AND lease_until_us IS NULL) OR (owner IS NOT NULL AND lease_until_us IS NOT NULL))
);
INSERT INTO dispatcher_head (singleton,generation,enabled) VALUES (1,0,0);

ALTER TABLE parties ADD FOREIGN KEY(tenant,environment,role_metadata_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE source_grants ADD FOREIGN KEY(tenant,environment,grant_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE bindings ADD FOREIGN KEY(tenant,environment,policy_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE bindings ADD FOREIGN KEY(tenant,environment,assent_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE bindings ADD FOREIGN KEY(tenant,environment,roles_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE bindings ADD FOREIGN KEY(tenant,environment,context_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE authority_heads ADD FOREIGN KEY(tenant,environment,grant_id) REFERENCES source_grants(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE binding_heads ADD FOREIGN KEY(tenant,environment,selector_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE binding_heads ADD FOREIGN KEY(tenant,environment,binding_id) REFERENCES bindings(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chains ADD FOREIGN KEY(tenant,environment,customer) REFERENCES parties(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chains ADD FOREIGN KEY(tenant,environment,binding_set_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chains ADD FOREIGN KEY(tenant,environment,context_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE events ADD FOREIGN KEY(tenant,environment,chain_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE events ADD FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE events ADD FOREIGN KEY(tenant,environment,source,external_id,id) REFERENCES delivery_keys(tenant,environment,source,external_id,canonical_event_id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE snapshots ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE snapshots ADD FOREIGN KEY(tenant,environment,document_id) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE delivery_keys ADD FOREIGN KEY(tenant,environment,canonical_event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE claims ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE effects ADD FOREIGN KEY(tenant,environment,claim_id) REFERENCES claims(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE effects ADD FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,effect_id) REFERENCES effects(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,binding_id) REFERENCES bindings(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,snapshot_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,roles_doc) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,reverses) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE actions ADD FOREIGN KEY(tenant,environment,allocation_parent) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE action_sources ADD FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE action_sources ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE action_dependencies ADD FOREIGN KEY(tenant,environment,action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE action_dependencies ADD FOREIGN KEY(tenant,environment,input_action_id) REFERENCES actions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE explanations ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE intentions ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE control_transitions ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE control_transitions ADD FOREIGN KEY(tenant,environment,document_id) REFERENCES documents(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE control_transitions ADD FOREIGN KEY(tenant,environment,control_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chain_revisions ADD FOREIGN KEY(tenant,environment,chain_id) REFERENCES chains(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chain_revisions ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE chain_revisions ADD FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE decision_manifests ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE decision_manifests ADD FOREIGN KEY(tenant,environment,chain_id,revision) REFERENCES chain_revisions(tenant,environment,chain_id,revision) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE accepted_receipts ADD FOREIGN KEY(tenant,environment,event_id) REFERENCES events(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE accepted_receipts ADD FOREIGN KEY(tenant,environment,decision_id) REFERENCES decision_manifests(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE delivery_state ADD FOREIGN KEY(tenant,environment,intention_id) REFERENCES intentions(tenant,environment,id) DEFERRABLE INITIALLY DEFERRED;
CREATE FUNCTION ledgerlab.reject_mutation() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$ BEGIN RAISE EXCEPTION 'IMMUTABLE_RECORD' USING ERRCODE='23000'; END; $$;
CREATE TRIGGER documents_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON documents FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER parties_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON parties FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER source_grants_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON source_grants FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER bindings_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON bindings FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON events FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER snapshots_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON snapshots FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER delivery_keys_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON delivery_keys FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER claims_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON claims FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER effects_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON effects FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER actions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON actions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER action_sources_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON action_sources FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER action_dependencies_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON action_dependencies FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER explanations_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON explanations FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER intentions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON intentions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER control_transitions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON control_transitions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER chain_revisions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON chain_revisions FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER decision_manifests_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON decision_manifests FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER accepted_receipts_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON accepted_receipts FOR EACH STATEMENT EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER installation_identity BEFORE UPDATE OF tenant,environment,logical_store_id,mode ON installation FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TRIGGER chain_identity BEFORE UPDATE OF tenant,environment,id,customer,currency,scale,binding_set_doc,context_doc ON chains FOR EACH ROW EXECUTE FUNCTION ledgerlab.reject_mutation();
CREATE TABLE migration_history (version BIGINT PRIMARY KEY, checksum TEXT NOT NULL);
