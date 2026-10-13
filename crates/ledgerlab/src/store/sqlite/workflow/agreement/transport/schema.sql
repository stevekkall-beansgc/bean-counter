-- Permanent bounded upload identity, committed inside the same Profile B journal.
CREATE TABLE workflow_definition_uploads (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, upload_id TEXT NOT NULL,
 document_schema TEXT NOT NULL, canonical_document_sha256 TEXT NOT NULL,
 document_bytes INTEGER NOT NULL CHECK(document_bytes BETWEEN 1 AND 262144),
 PRIMARY KEY(logical_store_id,tenant,environment,source,upload_id)
) STRICT;
CREATE TABLE workflow_definition_upload_parts (
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 source TEXT NOT NULL, upload_id TEXT NOT NULL, part INTEGER NOT NULL CHECK(part IN (0,1)),
 document_id TEXT NOT NULL REFERENCES workflow_documents(id),
 PRIMARY KEY(logical_store_id,tenant,environment,source,upload_id,part),
 FOREIGN KEY(logical_store_id,tenant,environment,source,upload_id) REFERENCES workflow_definition_uploads(logical_store_id,tenant,environment,source,upload_id)
) STRICT;
CREATE TRIGGER workflow_definition_uploads_update_immutable BEFORE UPDATE ON workflow_definition_uploads BEGIN SELECT RAISE(ABORT,'permanent source upload metadata'); END;
CREATE TRIGGER workflow_definition_uploads_delete_immutable BEFORE DELETE ON workflow_definition_uploads BEGIN SELECT RAISE(ABORT,'permanent source upload metadata'); END;
CREATE TRIGGER workflow_definition_upload_parts_update_immutable BEFORE UPDATE ON workflow_definition_upload_parts BEGIN SELECT RAISE(ABORT,'immutable source upload part'); END;
CREATE TRIGGER workflow_definition_upload_parts_delete_immutable BEFORE DELETE ON workflow_definition_upload_parts BEGIN SELECT RAISE(ABORT,'immutable source upload part'); END;
