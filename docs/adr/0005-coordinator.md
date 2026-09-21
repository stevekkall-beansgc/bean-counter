# ADR 005: Coordinator

Status: accepted for v0; implementation evidence remains gated.

Decision: Exactly one coordinator resolves authority and evaluates the complete binding set.

Consequence: No adapter/HTTP/TypeScript economic decision path or public raw-action append.

Validation: boundary checks and two-store golden equality.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
