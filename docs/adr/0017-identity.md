# ADR 017: Identity

Status: accepted for v0; implementation evidence remains gated.

Decision: Dual ingress/content hashes and canonical acquisition/link claim overrides.

Consequence: Identity retries use original receipt; renamed delivery/rules cannot rebill.

Validation: CANON, alias/claim RACE cases.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
