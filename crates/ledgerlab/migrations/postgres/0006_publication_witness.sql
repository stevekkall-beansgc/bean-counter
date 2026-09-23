-- Rollback-independent publication anchor projection. This table alone grants
-- no recovery/physical capability. The trusted migration owner binds an anchor
-- explicitly while writers are excluded; runtime cannot change anchor identity.
CREATE TABLE ledgerlab.r3_commit_witness (
    singleton smallint PRIMARY KEY CHECK (singleton = 1),
    anchor text COLLATE "C" NOT NULL CHECK (octet_length(anchor) = 64 AND anchor ~ '^[0-9a-f]{64}$'),
    witness text COLLATE "C" NOT NULL CHECK (octet_length(witness) = 64 AND witness ~ '^[0-9a-f]{64}$'),
    CHECK ((anchor = repeat('0', 64)) = (witness = repeat('0', 64)))
);
-- Closed UNBOUND state for legacy databases. It is not an authoritative witness.
INSERT INTO ledgerlab.r3_commit_witness VALUES (1, repeat('0', 64), repeat('0', 64));
