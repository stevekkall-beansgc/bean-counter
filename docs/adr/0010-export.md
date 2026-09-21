# ADR 010: Export

Status: accepted for v0; implementation evidence remains gated.

Decision: One fenced dispatcher, fake destination only.

Consequence: Intention identity survives retries/restore; delivery is not payment or exactly-once across systems.

Validation: DELIVERY/RECOVERY.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
