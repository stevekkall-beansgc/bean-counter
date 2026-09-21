# ADR 007: Representation

Status: accepted for v0; implementation evidence remains gated.

Decision: Canonical1/JCS/SHA-256; exact bounded arithmetic; one currency and scale per chain.

Consequence: UTF-16 keys, no Unicode folding, string money, nearest ties away, fixed limits.

Validation: CANON/MATH plus independent byte/hash checks.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
