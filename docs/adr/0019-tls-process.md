# ADR 019: TLS-process

Status: accepted for v0; implementation evidence remains gated.

Decision: Explicit Rustls verified trust, foreground process, read-only-root PostgreSQL.

Consequence: No ambient Keychain/OpenSSL/HOME trust; one port, operator TLS proxy, stable identity.

Validation: PEM/public negative TLS and OCI lifecycle gates.

Authority: detailed design §27. Changing this decision requires an explicit amendment and review of affected fixtures; unimplemented behavior is not an exception.
