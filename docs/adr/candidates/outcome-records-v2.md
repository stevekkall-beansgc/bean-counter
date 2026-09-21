# Candidate ADR: immutable outcome claim revisions

Status: proposed; independent review pending. **Not frozen.**

The frozen first-slice contract cannot encode an authorized outcome adjustment,
accepted zero claim, or revision-level inverse plus replacement without
inventing undeclared v1 variants. Phase 2's existing typed proposals also use
different bases and trigger vocabularies. This task's assigned v0 outcome
requirements need an explicit record family before a persistence bridge.

Propose the separate `2-candidate.1` profile described in
[Canonical records v2](../../design/CANONICAL-RECORDS-V2-CANDIDATE.md), with
scoped stable target/agreement/book/family claims, pinned original booked net,
policy and authority evidence, append-only revisions, exact inverses and a
manifest containing all retained replay inputs and new immutable records.

Explicit candidate choices for review:

- Candidate hash domains and `*2_` prefixes isolate every ID from v1. Candidate
  content-derived document IDs include scope; row content hashes use one generic
  rule except manifests. Promoting the candidate is not silently dropping its
  profile suffix.
- One command owns one target/agreement/book/family slot. Family is independent
  of policy version and authorized by the host. Policy/code changes cannot
  create another claim. Multi-family atomic command encoding is not proposed.
- Correction commands compare an immutable revision ID, keep the original
  policy/basis, and atomically append inverse plus replacement. This exception
  applies only to uncapped outcome claims, not generic v1 reversal semantics.
- Explicit absolute occurrence/report/correction boundaries are pinned. A
  correction may use its separately agreed deadline after the original report
  deadline. Review this boundary alongside the semantic lane before freezing.
- Aggregate limits constrain gross premiums and gross discounts independently;
  discount capacity is at most the original booked target net. No cross-family
  offsetting or silent saturation. All families in a group pin the same limits.
- A zero claim and zero-net correction have a full decision/receipt, with no
  zero action/intention. Original receipts remain byte-identical on retry.
- All prior intentions for a claim remain export dependencies, even through
  zero or zero-net revisions. Supplier corrections are separately explicit.
- Synthetic seed base records describe already accepted economics. Mapping
  production v1 records into hash-verified target references, complete
  invocation/reservation transitions, and the production replay decoder require
  later reviewed work; this is not an approved append-port expansion.

Consequences: schemas and vectors can be reviewed now without modifying frozen
files or production crates. Candidate audits join the normal repository checks.
The candidate has a conservative complete-prefix replay encoding with explicit
bounds. Compact closures, production authentication, two-store transaction and
retry races, and export execution remain later implementation gates.

Approval is deliberately absent. Successful automated reconstruction and local
self-review do not mark this ADR accepted or the contract frozen.
