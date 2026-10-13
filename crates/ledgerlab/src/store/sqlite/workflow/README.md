# Mechanical publication persistence checkpoint

This increment has no public publication or raw-append endpoint. The authority
contract for local OS-owner administration versus logical source revocation is
pending author disposition. No legacy `submit` permission becomes
`policy.publish`. The pure compiler, original-descriptor retry comparison,
coordinator authority checks, complete retained verification and facade wiring
remain required.

The adapter retains immutable typed documents, permanent scoped definition
versions independent of transport source, publication identities, original
operation/manifest/receipt bytes, manifest membership and feed records in one
tracked SQLite transaction. A known hash is disclosed only through committed
manifest membership in the authorized store/scope/source; the coordinator must
independently authenticate every scope and any administrator bypass.

The integration owner reserves schema12 and root module registration. `schema.sql`
is the owned proposed additive DDL, not an applied production migration. Reads
and appends require schema12. Old-writer refusal, explicit migration and upgraded
legacy read/write compatibility remain integration gates.

## Narrow verification

The test uses the PUB-ENC-01 independent selected fixture (12 envelope bodies,
10 manifest members), with no production compiler as an expected-value oracle.
It checks exact retained source/plan/authority/operation/manifest/receipt bytes,
all27 insert-failure boundaries with complete rollback of all six tables,
same-scope cross-source permanent registry conflict, immutable row refusal,
wrong store/tenant/environment/source hash reads and scoped administrator reads.
It also proves receipt bytes survive close and a new direct SQLite connection.
That reopen is deliberately not facade reopen or historical verification.
Commit followed by original lookup models a discarded reply; an injected
service reply-loss/process-crash proof remains unrun.

Ingress scaffold tests exercise strict raw parsing before DTO conversion,
duplicate escaped keys, unknown outer fields, nulls, invalid UTF8/BOM, prohibited
number lexemes, equivalent whitespace with distinct retained raw bytes and the
256KiB/+1 request ceiling. Inline source bodies remain non-authoritative Values;
the pure source normalizer must still reject unknown nested fields/types and
derive descriptor closure before publication.

Tests ran in `/private/tmp/bean-counter-persistence-test-20261012`, built from
tracked HEAD plus these owned files. The only disposable integration injections
were `pub(crate) mod workflow;` in service/mod.rs and store/sqlite/mod.rs. No
worktree shared exports, manifests or migration registration were changed.
Rust1.98.1 was verified from the installed standalone compiler; dependencies
were locked with `/private/tmp/bean-counter-waves12-cargo`, and target artifacts
were isolated under `/private/tmp/bean-counter-persistence-target`.

Command: installed Rust1.98.1 cargo `test --locked -p ledgerlab workflow --
--nocapture`. Candidate `check_publication_freeze.py` and
`verify_publication_independent.py` passed for the selected encoding boundary.
These are not whole-Wave1, Wave2, full compiler or product PASS statements.

Admission currently counts retained payload/history and conservatively includes
reused prepared documents in additional bytes. It does not establish indexed
constant-work admission or resource guarantees; exact reusable-byte deltas and
verified durable counters must be qualified before such a claim.
