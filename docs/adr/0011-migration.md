# ADR 011: Migration

Status: accepted for v0; implementation evidence remains gated.

Decision: Offline verified SQLite→PostgreSQL transfer preserves original bytes/IDs.

Consequence: No dual writing or accept/rerate on import; freeze source and hold dispatch.

Validation: TRANSFER/RECOVERY.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
