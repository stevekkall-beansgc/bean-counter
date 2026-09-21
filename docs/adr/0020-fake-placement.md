# ADR 020: Fake-placement

Status: accepted for v0; implementation evidence remains gated.

Decision: Runtime fake adapter belongs in facade outbox; testkit supplies independent scenarios.

Consequence: No fourth production crate or required local PG fake-receipt file.

Validation: workspace graph and later independent-destination tests.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
