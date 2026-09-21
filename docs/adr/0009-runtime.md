# ADR 009: Runtime

Status: accepted for v0; implementation evidence remains gated.

Decision: Three production crates; optional Axum CLI HTTP; separate dev and serve.

Consequence: Facade embeds without nesting runtime; only ledger ships; testkit is dev-only.

Validation: workspace graph and future lifecycle checks.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
