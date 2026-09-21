# ADR 002: Purity

Status: accepted for v0; implementation evidence remains gated.

Decision: One deterministic synchronous core takes all environment values as data.

Consequence: No runtime/database/network/clock/entropy/model access or first-party unsafe code.

Validation: resolved dependency graph and source checks.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
