# ADR 013: Compatibility

Status: accepted for v0; implementation evidence remains gated.

Decision: Wire, DSL, evaluator, canonical, storage and export versions are independent.

Consequence: Retain evaluator1 for released-v0 replay; never rewrite history on upgrade.

Validation: compatibility drift and later upgrade/restore tests.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
