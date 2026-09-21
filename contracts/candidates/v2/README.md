# Reconciled outcome contract candidate v2

**Candidate `2-candidate.2`, not frozen. Fresh-context review pending.**
Reconciled against semantic commit `1e0ba3f886788c08f427d3aae1d916b341187e76`;
`semantic-source.json` records the exact read-only sources. Candidate `.1` remains
historical at `c95cae9`; the new hash domain makes the incompatible changes clear.

The [design](../../../docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md),
[ADR](../../../docs/adr/candidates/outcome-records-v2.md) and
[roadmap](../../../ROADMAP.md) describe semantics, exact bytes/IDs, manifest
membership and later gates. Only roadmap Phase 1 is being executed.
The closed [schema](schemas/canonical-records.schema.json) defines 26 record kinds.
No production read/write version or new policy DSL is advertised.

`history-inputs.json` supplies hand-authored synthetic expected amounts/rational
intermediates. `goldens/` contains canonical bytes without final LF, original
receipt echoes, full retained records, boundary/retry probes, vectors and hashes.
Seed material is synthetic original base provenance, not real assent. The
cap-containing target preparation is explicitly rejected and has no accepted
base receipt; all other histories carry a complete original base-acceptance root.

Run `sh scripts/check-candidate-contracts.sh`. Python independently reconstructs
every golden byte, then checks retained-record semantics. Node independently
reconstructs identities, hashes, references and memberships. Thirteen adversarial
histories have all affected IDs/hashes/manifests/receipts rebuilt, pass Python
and Node integrity checks, then must fail semantics. A separate book-key check
proves book cannot split claim eligibility. Accepted-history semantic audit
requires the original base-acceptance reference supplied as a trust anchor.

`sh scripts/check-contracts.sh` runs v1 and candidate audits; `sh scripts/check.sh`
also runs complete offline repository checks. Checks never regenerate files.
Use `python3 scripts/contract_checks/v2_candidate/reconstruct.py --write` only
for intentional candidate authorship, and `--write-review-inventory` to refresh
the pending-review package inventory. Neither command constitutes approval.

All frozen v1 files remain unchanged. This package provides no persistence,
production historical decoder, authorization service, schema migration, outbox
execution, CLI integration or release. Independent review must precede freeze
or production integration.
