-- Explicit schema20-to21 adoption. Original workflow and legacy economics stay immutable.
CREATE TABLE billing_manual_credits (
 customer TEXT NOT NULL,
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 adjustment_id TEXT NOT NULL,
 record_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
 credit_kind TEXT NOT NULL CHECK(credit_kind='billing-manual-credit'),
 credit_id TEXT NOT NULL, credit_digest TEXT NOT NULL,
 accepted_at_us INTEGER NOT NULL,
 PRIMARY KEY(customer,logical_store_id,tenant,environment,adjustment_id),
 UNIQUE(customer,logical_store_id,tenant,environment,credit_kind,credit_id,credit_digest)
) STRICT, WITHOUT ROWID;
CREATE INDEX billing_manual_credits_by_time ON billing_manual_credits(customer,logical_store_id,tenant,environment,accepted_at_us,record_sequence);
CREATE TRIGGER billing_manual_credits_no_update BEFORE UPDATE ON billing_manual_credits BEGIN SELECT RAISE(ABORT,'immutable manual credit'); END;
CREATE TRIGGER billing_manual_credits_no_delete BEFORE DELETE ON billing_manual_credits BEGIN SELECT RAISE(ABORT,'immutable manual credit'); END;
CREATE TABLE billing_manual_credit_membership (
 customer TEXT NOT NULL,
 logical_store_id TEXT NOT NULL, tenant TEXT NOT NULL, environment TEXT NOT NULL,
 credit_kind TEXT NOT NULL CHECK(credit_kind='billing-manual-credit'),
 credit_id TEXT NOT NULL, credit_digest TEXT NOT NULL,
 claim_sequence INTEGER NOT NULL UNIQUE REFERENCES billing_m5_records(sequence),
 presentation_kind TEXT NOT NULL CHECK(presentation_kind IN ('standard-period','ad-hoc')),
 statement_id TEXT NOT NULL, statement_hash TEXT NOT NULL,
 PRIMARY KEY(customer,logical_store_id,tenant,environment,credit_kind,credit_id,credit_digest),
 FOREIGN KEY(customer,logical_store_id,tenant,environment,credit_kind,credit_id,credit_digest)
  REFERENCES billing_manual_credits(customer,logical_store_id,tenant,environment,credit_kind,credit_id,credit_digest)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER billing_manual_credit_membership_no_update BEFORE UPDATE ON billing_manual_credit_membership BEGIN SELECT RAISE(ABORT,'immutable manual credit presentation'); END;
CREATE TRIGGER billing_manual_credit_membership_no_delete BEFORE DELETE ON billing_manual_credit_membership BEGIN SELECT RAISE(ABORT,'immutable manual credit presentation'); END;
