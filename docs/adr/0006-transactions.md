# ADR 006: Transactions

Status: accepted for v0; implementation evidence remains gated.

Decision: SQLite tracked BEGIN IMMEDIATE; PostgreSQL SERIALIZABLE with ordered shared scopes.

Consequence: Retry the complete rolled-back transaction; keep unknown commit outcome distinct.

Validation: all write/commit/cancel failpoints and forced races.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
