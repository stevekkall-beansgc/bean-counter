-- Candidate-only indices for independently accepted and activated agreements.
-- Bodies and originating committed certificates remain the verification source.
CREATE TABLE workflow_agreement_owners (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,agreement_id),
 UNIQUE(logical_store_id,tenant,environment,agreement_id,customer),
 FOREIGN KEY(logical_store_id,tenant,environment,customer) REFERENCES workflow_customer_registrations(logical_store_id,tenant,environment,customer)
) STRICT;
CREATE TABLE workflow_agreement_heads (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 head_kind TEXT NOT NULL CHECK(head_kind IN ('agreement_acceptance','agreement_activation')),
 revision INTEGER NOT NULL CHECK(revision>0),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,head_kind),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer)
) STRICT;
CREATE TABLE workflow_agreement_acceptances (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version>0),
 accepted_id TEXT NOT NULL REFERENCES workflow_documents(id),
 binding_id TEXT NOT NULL REFERENCES workflow_documents(id),
 intent_id TEXT NOT NULL REFERENCES workflow_documents(id),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,version),
 UNIQUE(logical_store_id,tenant,environment,binding_id),
 UNIQUE(logical_store_id,tenant,environment,accepted_id),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer)
) STRICT;
CREATE TABLE workflow_agreement_activations (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 agreement_id TEXT NOT NULL, customer TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 activation_id TEXT NOT NULL REFERENCES workflow_documents(id),
 transition_id TEXT NOT NULL REFERENCES workflow_documents(id),
 binding_id TEXT NOT NULL REFERENCES workflow_documents(id),
 intent_id TEXT NOT NULL REFERENCES workflow_documents(id),
 action TEXT NOT NULL CHECK(action IN ('activate','replace','revoke')),
 effective_at_us INTEGER NOT NULL, accepted_at_us INTEGER NOT NULL,
 commit_sequence INTEGER NOT NULL CHECK(commit_sequence>0),
 target_id TEXT REFERENCES workflow_documents(id),
 CHECK((action='activate' AND target_id IS NULL) OR (action<>'activate' AND target_id IS NOT NULL)),
 PRIMARY KEY(logical_store_id,tenant,environment,customer,agreement_id,revision),
 UNIQUE(logical_store_id,tenant,environment,customer,agreement_id,activation_id),
 UNIQUE(logical_store_id,tenant,environment,transition_id),
 FOREIGN KEY(logical_store_id,tenant,environment,agreement_id,customer) REFERENCES workflow_agreement_owners(logical_store_id,tenant,environment,agreement_id,customer),
 FOREIGN KEY(logical_store_id,tenant,environment,customer,agreement_id,target_id) REFERENCES workflow_agreement_activations(logical_store_id,tenant,environment,customer,agreement_id,activation_id)
) STRICT;
CREATE TRIGGER workflow_agreement_owners_update_immutable BEFORE UPDATE ON workflow_agreement_owners BEGIN SELECT RAISE(ABORT,'permanent agreement ownership'); END;
CREATE TRIGGER workflow_agreement_owners_delete_immutable BEFORE DELETE ON workflow_agreement_owners BEGIN SELECT RAISE(ABORT,'permanent agreement ownership'); END;
CREATE TRIGGER workflow_agreement_acceptances_update_immutable BEFORE UPDATE ON workflow_agreement_acceptances BEGIN SELECT RAISE(ABORT,'immutable acceptance history'); END;
CREATE TRIGGER workflow_agreement_acceptances_delete_immutable BEFORE DELETE ON workflow_agreement_acceptances BEGIN SELECT RAISE(ABORT,'immutable acceptance history'); END;
CREATE TRIGGER workflow_agreement_activations_update_immutable BEFORE UPDATE ON workflow_agreement_activations BEGIN SELECT RAISE(ABORT,'immutable activation history'); END;
CREATE TRIGGER workflow_agreement_activations_delete_immutable BEFORE DELETE ON workflow_agreement_activations BEGIN SELECT RAISE(ABORT,'immutable activation history'); END;
CREATE TRIGGER workflow_agreement_heads_monotone BEFORE UPDATE ON workflow_agreement_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.agreement_id<>OLD.agreement_id OR NEW.customer<>OLD.customer OR NEW.head_kind<>OLD.head_kind OR NEW.revision<>OLD.revision+1
BEGIN SELECT RAISE(ABORT,'agreement head must advance independently'); END;
CREATE TRIGGER workflow_agreement_heads_no_delete BEFORE DELETE ON workflow_agreement_heads BEGIN SELECT RAISE(ABORT,'permanent agreement head lineage'); END;
