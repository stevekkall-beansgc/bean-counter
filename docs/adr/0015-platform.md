# ADR 015: Platform

Status: accepted for v0; implementation evidence remains gated.

Decision: Adopt the exact Linux x64/ARM64, Mac15 local and Windows build-only contract.

Consequence: Certification requires native exact-artifact evidence; local Mac26 is development only.

Validation: release/targets.toml and future PLATFORM matrix.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
