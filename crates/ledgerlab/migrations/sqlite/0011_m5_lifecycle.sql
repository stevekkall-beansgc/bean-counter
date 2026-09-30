-- Candidate M5 schema-11 additive sidecar DDL.
-- Apply only through the reviewed explicit schema-10-to-11 upgrade transaction.
-- No existing M1-M4 table, row, alias, identity, or payload is rewritten.

CREATE TABLE billing_m5_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    migration_id TEXT NOT NULL UNIQUE,
    record_count INTEGER NOT NULL CHECK (record_count BETWEEN 0 AND 100000),
    canonical_bytes INTEGER NOT NULL CHECK (canonical_bytes BETWEEN 0 AND 268435456),
    activity_identity_count INTEGER NOT NULL CHECK (activity_identity_count BETWEEN 0 AND 100000),
    activity_identity_bytes INTEGER NOT NULL CHECK (activity_identity_bytes BETWEEN 0 AND 268435456),
    next_command_sequence INTEGER NOT NULL CHECK (next_command_sequence >= 1),
    next_record_sequence INTEGER NOT NULL CHECK (next_record_sequence >= 1)
) STRICT;

CREATE TRIGGER billing_m5_state_monotone BEFORE UPDATE ON billing_m5_state
WHEN NEW.record_count < OLD.record_count
  OR NEW.canonical_bytes < OLD.canonical_bytes
  OR NEW.activity_identity_count < OLD.activity_identity_count
  OR NEW.activity_identity_bytes < OLD.activity_identity_bytes
  OR NEW.record_count + NEW.next_command_sequence - 1 + NEW.activity_identity_count > 100000
  OR NEW.canonical_bytes + NEW.activity_identity_bytes > 268435456
  OR NEW.next_command_sequence < OLD.next_command_sequence
  OR NEW.next_record_sequence < OLD.next_record_sequence
  OR NEW.singleton <> OLD.singleton
  OR NEW.migration_id <> OLD.migration_id
BEGIN SELECT RAISE(ABORT, 'M5 bounds must be monotone'); END;
CREATE TRIGGER billing_m5_state_no_delete BEFORE DELETE ON billing_m5_state
BEGIN SELECT RAISE(ABORT, 'immutable M5 migration identity'); END;

-- A command owns exactly one idempotency identity and one saved response.
-- Its domain records are one-to-many and never carry a unique command key.
CREATE TABLE billing_m5_commands (
    command_sequence INTEGER PRIMARY KEY CHECK (command_sequence >= 1),
    family TEXT NOT NULL,
    identity_domain TEXT NOT NULL CHECK (identity_domain IN ('application','customer-admin','installation-admin')),
    customer TEXT,
    source TEXT,
    identity_key BLOB NOT NULL UNIQUE CHECK (length(identity_key) <= 4096),
    accepted_at_us INTEGER NOT NULL,
    child_count INTEGER NOT NULL CHECK (child_count >= 1),
    request_bytes BLOB NOT NULL CHECK (length(request_bytes) <= 262144),
    request_sha256 BLOB NOT NULL CHECK (length(request_sha256) = 32),
    response_bytes BLOB NOT NULL CHECK (length(response_bytes) <= 262144),
    response_sha256 BLOB NOT NULL CHECK (length(response_sha256) = 32),
    CHECK ((identity_domain = 'application' AND customer IS NOT NULL AND source IS NOT NULL)
        OR (identity_domain = 'customer-admin' AND customer IS NOT NULL)
        OR (identity_domain = 'installation-admin' AND customer IS NULL AND source IS NULL))
) STRICT;

CREATE TABLE billing_m5_records (
    sequence INTEGER PRIMARY KEY CHECK (sequence >= 1),
    record_id TEXT NOT NULL UNIQUE,
    family TEXT NOT NULL,
    command_sequence INTEGER NOT NULL REFERENCES billing_m5_commands(command_sequence),
    customer TEXT,
    source TEXT,
    payload_bytes BLOB NOT NULL CHECK (length(payload_bytes) <= 262144),
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    CHECK ((customer IS NULL AND source IS NULL) OR customer IS NOT NULL),
    CHECK (length(record_id) = 68 AND substr(record_id, 1, 4) = 'm5r_'),
    UNIQUE (command_sequence, sequence)
) STRICT;

CREATE INDEX billing_m5_records_command ON billing_m5_records(command_sequence, sequence);
CREATE INDEX billing_m5_records_family ON billing_m5_records(family, sequence);

-- Cumulative activity has two application identities: its customer/source
-- scoped semantic operation and every delivery ID used to submit that
-- operation. The first accepted activity owns one semantic row and one
-- delivery row. A semantically identical retry with a fresh delivery ID adds
-- only a delivery alias to the original command/activity; it appends no
-- command, economic record, quantity, or new acceptance timestamp.
CREATE TABLE billing_m5_activity_semantics (
    customer TEXT NOT NULL CHECK (length(CAST(customer AS BLOB)) BETWEEN 1 AND 128),
    source TEXT NOT NULL CHECK (length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
    operation_id TEXT NOT NULL CHECK (length(CAST(operation_id AS BLOB)) BETWEEN 1 AND 128),
    command_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_commands(command_sequence),
    activity_sequence INTEGER NOT NULL UNIQUE,
    facts_bytes BLOB NOT NULL CHECK (length(facts_bytes) BETWEEN 1 AND 262144),
    facts_sha256 BLOB NOT NULL CHECK (length(facts_sha256) = 32),
    PRIMARY KEY (customer, source, operation_id),
    FOREIGN KEY (command_sequence, activity_sequence)
        REFERENCES billing_m5_records(command_sequence, sequence)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_activity_deliveries (
    customer TEXT NOT NULL CHECK (length(CAST(customer AS BLOB)) BETWEEN 1 AND 128),
    source TEXT NOT NULL CHECK (length(CAST(source AS BLOB)) BETWEEN 1 AND 256),
    external_id TEXT NOT NULL CHECK (length(CAST(external_id AS BLOB)) BETWEEN 1 AND 128),
    command_sequence INTEGER NOT NULL REFERENCES billing_m5_commands(command_sequence),
    activity_sequence INTEGER NOT NULL,
    ingress_bytes BLOB NOT NULL CHECK (length(ingress_bytes) BETWEEN 1 AND 262144),
    ingress_sha256 BLOB NOT NULL CHECK (length(ingress_sha256) = 32),
    PRIMARY KEY (customer, source, external_id),
    FOREIGN KEY (command_sequence, activity_sequence)
        REFERENCES billing_m5_records(command_sequence, sequence)
) STRICT, WITHOUT ROWID;

CREATE INDEX billing_m5_activity_delivery_command
    ON billing_m5_activity_deliveries(command_sequence, external_id);
CREATE INDEX billing_m5_activity_delivery_record
    ON billing_m5_activity_deliveries(activity_sequence);

-- This is the only admissible pair of ledger high-water marks for a report.
-- A schema-11 writer appends one boundary in the same transaction as each M3
-- or M5 mutation, after all child records have been inserted. Equal high-water
-- pairs are allowed: an M3 semantic alias mutation may change retry metadata
-- without advancing either economic stream. The same rule applies to an M5 activity
-- delivery alias. Pair lookup chooses max(boundary_id).
CREATE TABLE billing_m5_snapshot_boundaries (
    boundary_id INTEGER PRIMARY KEY CHECK (boundary_id >= 1),
    m3_high_water INTEGER NOT NULL CHECK (m3_high_water >= 0),
    m5_high_water INTEGER NOT NULL CHECK (m5_high_water >= 0),
    UNIQUE (boundary_id, m3_high_water, m5_high_water)
) STRICT;

CREATE INDEX billing_m5_snapshot_boundary_pair
    ON billing_m5_snapshot_boundaries(m3_high_water, m5_high_water, boundary_id);

CREATE TRIGGER billing_m5_snapshot_boundaries_monotone BEFORE INSERT ON billing_m5_snapshot_boundaries
WHEN NEW.boundary_id <> COALESCE((SELECT max(boundary_id) + 1 FROM billing_m5_snapshot_boundaries), 1)
  OR NEW.m3_high_water < COALESCE((SELECT max(m3_high_water) FROM billing_m5_snapshot_boundaries), 0)
  OR NEW.m5_high_water < COALESCE((SELECT max(m5_high_water) FROM billing_m5_snapshot_boundaries), 0)
BEGIN SELECT RAISE(ABORT, 'M5 snapshot boundaries must advance monotonically'); END;

CREATE TRIGGER billing_m5_commands_no_update BEFORE UPDATE ON billing_m5_commands
BEGIN SELECT RAISE(ABORT, 'immutable M5 command'); END;
CREATE TRIGGER billing_m5_commands_no_delete BEFORE DELETE ON billing_m5_commands
BEGIN SELECT RAISE(ABORT, 'immutable M5 command'); END;
CREATE TRIGGER billing_m5_snapshot_boundaries_no_update BEFORE UPDATE ON billing_m5_snapshot_boundaries
BEGIN SELECT RAISE(ABORT, 'immutable M5 snapshot boundary'); END;
CREATE TRIGGER billing_m5_snapshot_boundaries_no_delete BEFORE DELETE ON billing_m5_snapshot_boundaries
BEGIN SELECT RAISE(ABORT, 'immutable M5 snapshot boundary'); END;

CREATE TABLE billing_m5_term_versions (
    customer TEXT NOT NULL,
    term_version INTEGER NOT NULL CHECK (term_version >= 1),
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    effective_at_us INTEGER NOT NULL,
    term_bytes BLOB NOT NULL,
    PRIMARY KEY (customer, term_version)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_fiscal_versions (
    calendar_version INTEGER PRIMARY KEY CHECK (calendar_version >= 1),
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    timezone TEXT NOT NULL,
    timezone_rules_version TEXT NOT NULL,
    calendar_bytes BLOB NOT NULL
) STRICT;

CREATE TABLE billing_m5_cumulative_basis_versions (
    customer TEXT NOT NULL,
    source TEXT NOT NULL,
    agreement_id TEXT NOT NULL,
    agreement_version INTEGER NOT NULL CHECK (agreement_version >= 1),
    basis_version INTEGER NOT NULL CHECK (basis_version >= 1),
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    effective_at_us INTEGER NOT NULL,
    basis_bytes BLOB NOT NULL,
    PRIMARY KEY (customer, source, agreement_id, agreement_version, basis_version)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_recurrence_versions (
    customer TEXT NOT NULL,
    source TEXT NOT NULL,
    agreement_id TEXT NOT NULL,
    agreement_version INTEGER NOT NULL CHECK (agreement_version >= 1),
    recurrence_version INTEGER NOT NULL CHECK (recurrence_version >= 1),
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    rule_bytes BLOB NOT NULL,
    renewal_bytes BLOB NOT NULL,
    PRIMARY KEY (customer, source, recurrence_version)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_recurrence_cancellations (
    customer TEXT NOT NULL,
    source TEXT NOT NULL,
    recurrence_version INTEGER NOT NULL CHECK (recurrence_version >= 1),
    cancelled_at_us INTEGER NOT NULL,
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    PRIMARY KEY (customer, source, recurrence_version)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_occurrence_acceptances (
    customer TEXT NOT NULL,
    source TEXT NOT NULL,
    occurrence_id TEXT NOT NULL,
    accepted_m3_receipt_id TEXT NOT NULL,
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    PRIMARY KEY (customer, source, occurrence_id),
    UNIQUE (customer, source, accepted_m3_receipt_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_period_resolutions (
    customer TEXT NOT NULL,
    term_version INTEGER NOT NULL CHECK (term_version >= 1),
    period_index INTEGER NOT NULL CHECK (period_index >= 0),
    resolution_id TEXT NOT NULL UNIQUE,
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    start_at_us INTEGER NOT NULL,
    end_at_us INTEGER NOT NULL CHECK (end_at_us > start_at_us),
    supersedes_resolution_id TEXT,
    PRIMARY KEY (customer, term_version, period_index, record_sequence)
) STRICT, WITHOUT ROWID;

CREATE INDEX billing_m5_period_resolution_lookup
    ON billing_m5_period_resolutions(customer, term_version, period_index, record_sequence);

CREATE TABLE billing_m5_assignments (
    customer TEXT NOT NULL,
    source_scope TEXT NOT NULL,
    source_record_kind TEXT NOT NULL,
    source_record_id TEXT NOT NULL,
    source_stream TEXT NOT NULL CHECK (source_stream IN ('m3','m5')),
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    term_version INTEGER NOT NULL CHECK (term_version >= 1),
    period_index INTEGER NOT NULL CHECK (period_index >= 0),
    assignment_basis TEXT NOT NULL CHECK (assignment_basis IN ('acceptance-time','linked-open-period','post-close-adjustment')),
    assignment_at_us INTEGER NOT NULL,
    PRIMARY KEY (customer, source_scope, source_record_kind, source_record_id),
    UNIQUE (source_stream, source_sequence)
) STRICT, WITHOUT ROWID;

CREATE INDEX billing_m5_assignment_period
    ON billing_m5_assignments(customer, source_scope, term_version, period_index, source_sequence);

CREATE TABLE billing_m5_adjustments (
    customer TEXT NOT NULL,
    source_scope TEXT NOT NULL,
    adjustment_id TEXT NOT NULL,
    cause_kind TEXT NOT NULL CHECK (cause_kind IN ('per-work-quantity-correction','cumulative-quantity-correction','outcome-correction')),
    cause_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    original_term_version INTEGER NOT NULL CHECK (original_term_version >= 1),
    original_period_index INTEGER NOT NULL CHECK (original_period_index >= 0),
    assigned_term_version INTEGER NOT NULL CHECK (assigned_term_version >= 1),
    assigned_period_index INTEGER NOT NULL CHECK (assigned_period_index >= 0),
    source_stream TEXT NOT NULL CHECK (source_stream IN ('m3','m5')),
    source_sequence INTEGER NOT NULL CHECK (source_sequence >= 1),
    signed_delta_atoms TEXT NOT NULL,
    PRIMARY KEY (customer, source_scope, adjustment_id),
    UNIQUE (source_stream, source_sequence),
    UNIQUE (customer, source_scope, cause_kind, cause_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_period_closes (
    customer TEXT NOT NULL,
    term_version INTEGER NOT NULL CHECK (term_version >= 1),
    period_index INTEGER NOT NULL CHECK (period_index >= 0),
    boundary_resolution_id TEXT NOT NULL,
    snapshot_boundary_id INTEGER NOT NULL REFERENCES billing_m5_snapshot_boundaries(boundary_id),
    m3_high_water INTEGER NOT NULL CHECK (m3_high_water >= 0),
    m5_high_water INTEGER NOT NULL CHECK (m5_high_water >= 0),
    statement_hash TEXT NOT NULL CHECK (length(statement_hash) = 64),
    close_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    statement_bytes BLOB NOT NULL,
    PRIMARY KEY (customer, term_version, period_index),
    FOREIGN KEY (snapshot_boundary_id, m3_high_water, m5_high_water)
        REFERENCES billing_m5_snapshot_boundaries(boundary_id, m3_high_water, m5_high_water)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_presentation_claims (
    customer TEXT NOT NULL,
    source_scope TEXT NOT NULL,
    adjustment_id TEXT NOT NULL,
    presentation_kind TEXT NOT NULL CHECK (presentation_kind IN ('standard-period','ad-hoc')),
    statement_id TEXT NOT NULL,
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    PRIMARY KEY (customer, source_scope, adjustment_id),
    FOREIGN KEY (customer, source_scope, adjustment_id)
        REFERENCES billing_m5_adjustments(customer, source_scope, adjustment_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE billing_m5_fiscal_reports (
    report_id TEXT PRIMARY KEY,
    calendar_version INTEGER NOT NULL REFERENCES billing_m5_fiscal_versions(calendar_version),
    m3_high_water INTEGER NOT NULL CHECK (m3_high_water >= 0),
    m5_high_water INTEGER NOT NULL CHECK (m5_high_water >= 0),
    snapshot_boundary_id INTEGER NOT NULL REFERENCES billing_m5_snapshot_boundaries(boundary_id),
    report_hash TEXT NOT NULL CHECK (length(report_hash) = 64),
    record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
    report_bytes BLOB NOT NULL,
    FOREIGN KEY (snapshot_boundary_id, m3_high_water, m5_high_water)
        REFERENCES billing_m5_snapshot_boundaries(boundary_id, m3_high_water, m5_high_water)
) STRICT;

-- Prevent rewrites and deletion of durable event or projection rows.
CREATE TRIGGER billing_m5_records_no_update BEFORE UPDATE ON billing_m5_records
BEGIN SELECT RAISE(ABORT, 'immutable M5 record'); END;
CREATE TRIGGER billing_m5_records_no_delete BEFORE DELETE ON billing_m5_records
BEGIN SELECT RAISE(ABORT, 'immutable M5 record'); END;
CREATE TRIGGER billing_m5_activity_semantics_no_update BEFORE UPDATE ON billing_m5_activity_semantics
BEGIN SELECT RAISE(ABORT, 'immutable M5 activity semantic identity'); END;
CREATE TRIGGER billing_m5_activity_semantics_no_delete BEFORE DELETE ON billing_m5_activity_semantics
BEGIN SELECT RAISE(ABORT, 'immutable M5 activity semantic identity'); END;
CREATE TRIGGER billing_m5_activity_deliveries_no_update BEFORE UPDATE ON billing_m5_activity_deliveries
BEGIN SELECT RAISE(ABORT, 'immutable M5 activity delivery identity'); END;
CREATE TRIGGER billing_m5_activity_deliveries_no_delete BEFORE DELETE ON billing_m5_activity_deliveries
BEGIN SELECT RAISE(ABORT, 'immutable M5 activity delivery identity'); END;
CREATE TRIGGER billing_m5_term_versions_no_update BEFORE UPDATE ON billing_m5_term_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 term version'); END;
CREATE TRIGGER billing_m5_term_versions_no_delete BEFORE DELETE ON billing_m5_term_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 term version'); END;
CREATE TRIGGER billing_m5_fiscal_versions_no_update BEFORE UPDATE ON billing_m5_fiscal_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 fiscal version'); END;
CREATE TRIGGER billing_m5_fiscal_versions_no_delete BEFORE DELETE ON billing_m5_fiscal_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 fiscal version'); END;
CREATE TRIGGER billing_m5_cumulative_basis_versions_no_update BEFORE UPDATE ON billing_m5_cumulative_basis_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 cumulative basis'); END;
CREATE TRIGGER billing_m5_cumulative_basis_versions_no_delete BEFORE DELETE ON billing_m5_cumulative_basis_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 cumulative basis'); END;
CREATE TRIGGER billing_m5_recurrence_versions_no_update BEFORE UPDATE ON billing_m5_recurrence_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 recurrence'); END;
CREATE TRIGGER billing_m5_recurrence_versions_no_delete BEFORE DELETE ON billing_m5_recurrence_versions
BEGIN SELECT RAISE(ABORT, 'immutable M5 recurrence'); END;
CREATE TRIGGER billing_m5_recurrence_cancellations_no_update BEFORE UPDATE ON billing_m5_recurrence_cancellations
BEGIN SELECT RAISE(ABORT, 'immutable M5 recurrence cancellation'); END;
CREATE TRIGGER billing_m5_recurrence_cancellations_no_delete BEFORE DELETE ON billing_m5_recurrence_cancellations
BEGIN SELECT RAISE(ABORT, 'immutable M5 recurrence cancellation'); END;
CREATE TRIGGER billing_m5_occurrence_acceptances_no_update BEFORE UPDATE ON billing_m5_occurrence_acceptances
BEGIN SELECT RAISE(ABORT, 'immutable M5 occurrence acceptance'); END;
CREATE TRIGGER billing_m5_occurrence_acceptances_no_delete BEFORE DELETE ON billing_m5_occurrence_acceptances
BEGIN SELECT RAISE(ABORT, 'immutable M5 occurrence acceptance'); END;
CREATE TRIGGER billing_m5_period_resolutions_no_update BEFORE UPDATE ON billing_m5_period_resolutions
BEGIN SELECT RAISE(ABORT, 'immutable M5 period resolution'); END;
CREATE TRIGGER billing_m5_period_resolutions_no_delete BEFORE DELETE ON billing_m5_period_resolutions
BEGIN SELECT RAISE(ABORT, 'immutable M5 period resolution'); END;
CREATE TRIGGER billing_m5_assignments_no_update BEFORE UPDATE ON billing_m5_assignments
BEGIN SELECT RAISE(ABORT, 'immutable M5 assignment'); END;
CREATE TRIGGER billing_m5_assignments_no_delete BEFORE DELETE ON billing_m5_assignments
BEGIN SELECT RAISE(ABORT, 'immutable M5 assignment'); END;
CREATE TRIGGER billing_m5_adjustments_no_update BEFORE UPDATE ON billing_m5_adjustments
BEGIN SELECT RAISE(ABORT, 'immutable M5 adjustment'); END;
CREATE TRIGGER billing_m5_adjustments_no_delete BEFORE DELETE ON billing_m5_adjustments
BEGIN SELECT RAISE(ABORT, 'immutable M5 adjustment'); END;
CREATE TRIGGER billing_m5_period_closes_no_update BEFORE UPDATE ON billing_m5_period_closes
BEGIN SELECT RAISE(ABORT, 'immutable M5 close'); END;
CREATE TRIGGER billing_m5_period_closes_no_delete BEFORE DELETE ON billing_m5_period_closes
BEGIN SELECT RAISE(ABORT, 'immutable M5 close'); END;
CREATE TRIGGER billing_m5_presentation_claims_no_update BEFORE UPDATE ON billing_m5_presentation_claims
BEGIN SELECT RAISE(ABORT, 'immutable M5 presentation claim'); END;
CREATE TRIGGER billing_m5_presentation_claims_no_delete BEFORE DELETE ON billing_m5_presentation_claims
BEGIN SELECT RAISE(ABORT, 'immutable M5 presentation claim'); END;
CREATE TRIGGER billing_m5_fiscal_reports_no_update BEFORE UPDATE ON billing_m5_fiscal_reports
BEGIN SELECT RAISE(ABORT, 'immutable M5 fiscal report'); END;
CREATE TRIGGER billing_m5_fiscal_reports_no_delete BEFORE DELETE ON billing_m5_fiscal_reports
BEGIN SELECT RAISE(ABORT, 'immutable M5 fiscal report'); END;
