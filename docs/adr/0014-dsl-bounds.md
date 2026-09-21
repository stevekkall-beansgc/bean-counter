# ADR 014: DSL-bounds

Status: accepted for v0; implementation evidence remains gated.

Decision: One closure cap and one share with fixed paths; general floors/running caps deferred.

Consequence: Bounded deterministic order and named booked bases; no arbitrary code or network calls.

Validation: independent arithmetic and later policy compiler tests.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
