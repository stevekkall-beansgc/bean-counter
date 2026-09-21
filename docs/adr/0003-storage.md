# ADR 003: Storage

Status: accepted for v0; implementation evidence remains gated.

Decision: Both real SQLite and PostgreSQL belong in the first slice and v0.

Consequence: A mocked or placeholder PG adapter cannot pass Phase 1.

Validation: STORE, ATOMIC, RACE.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
