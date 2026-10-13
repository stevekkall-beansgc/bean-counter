-- Internal projections for reviewed administrative enrollment operations.
-- Included by the integration owner's locked additive migration; not standalone.
CREATE TABLE workflow_agreement_semantics (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 operation_kind TEXT NOT NULL, semantic_key BLOB NOT NULL,
 original_identity_key BLOB NOT NULL REFERENCES workflow_operations(identity_key),
 PRIMARY KEY(logical_store_id,tenant,environment,operation_kind,semantic_key)
) STRICT;
CREATE TABLE workflow_customer_registrations (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 customer TEXT NOT NULL, document_id TEXT NOT NULL REFERENCES workflow_documents(id),
 PRIMARY KEY(logical_store_id,tenant,environment,customer)
) STRICT;
CREATE TABLE workflow_source_namespaces (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, canonical_economic_namespace TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,source)
) STRICT;
CREATE TABLE workflow_source_enrollments (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 enrollment_id TEXT NOT NULL, document_id TEXT NOT NULL REFERENCES workflow_documents(id),
 source TEXT NOT NULL, canonical_economic_namespace TEXT NOT NULL,
 PRIMARY KEY(logical_store_id,tenant,environment,enrollment_id)
) STRICT;
CREATE TABLE workflow_source_heads (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 enrollment_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
 state_id TEXT NOT NULL REFERENCES workflow_documents(id),
 grant_id TEXT REFERENCES workflow_documents(id),
 status TEXT NOT NULL CHECK(status IN ('active','revoked')),
 CHECK((status='active' AND grant_id IS NOT NULL) OR (status='revoked' AND grant_id IS NULL)),
 PRIMARY KEY(logical_store_id,tenant,environment,enrollment_id),
 FOREIGN KEY(logical_store_id,tenant,environment,enrollment_id) REFERENCES workflow_source_enrollments(logical_store_id,tenant,environment,enrollment_id)
) STRICT;
CREATE TRIGGER workflow_agreement_semantics_update_immutable BEFORE UPDATE ON workflow_agreement_semantics BEGIN SELECT RAISE(ABORT,'immutable workflow semantic identity'); END;
CREATE TRIGGER workflow_agreement_semantics_delete_immutable BEFORE DELETE ON workflow_agreement_semantics BEGIN SELECT RAISE(ABORT,'immutable workflow semantic identity'); END;
CREATE TRIGGER workflow_customer_registrations_update_immutable BEFORE UPDATE ON workflow_customer_registrations BEGIN SELECT RAISE(ABORT,'immutable workflow customer registration'); END;
CREATE TRIGGER workflow_customer_registrations_delete_immutable BEFORE DELETE ON workflow_customer_registrations BEGIN SELECT RAISE(ABORT,'immutable workflow customer registration'); END;
CREATE TRIGGER workflow_source_namespaces_update_immutable BEFORE UPDATE ON workflow_source_namespaces BEGIN SELECT RAISE(ABORT,'permanent workflow economic namespace'); END;
CREATE TRIGGER workflow_source_namespaces_delete_immutable BEFORE DELETE ON workflow_source_namespaces BEGIN SELECT RAISE(ABORT,'permanent workflow economic namespace'); END;
CREATE TRIGGER workflow_source_enrollments_update_immutable BEFORE UPDATE ON workflow_source_enrollments BEGIN SELECT RAISE(ABORT,'immutable workflow original ceiling'); END;
CREATE TRIGGER workflow_source_enrollments_delete_immutable BEFORE DELETE ON workflow_source_enrollments BEGIN SELECT RAISE(ABORT,'immutable workflow original ceiling'); END;
CREATE TRIGGER workflow_source_heads_monotone BEFORE UPDATE ON workflow_source_heads
WHEN NEW.logical_store_id<>OLD.logical_store_id OR NEW.tenant<>OLD.tenant OR NEW.environment<>OLD.environment OR NEW.enrollment_id<>OLD.enrollment_id OR NEW.revision<>OLD.revision+1 OR OLD.status='revoked'
BEGIN SELECT RAISE(ABORT,'workflow source state must advance without reenabling'); END;
CREATE TRIGGER workflow_source_heads_no_delete BEFORE DELETE ON workflow_source_heads BEGIN SELECT RAISE(ABORT,'permanent workflow source lineage'); END;
