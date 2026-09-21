# Outcome adjustment contract candidate v2

**Candidate, not frozen. Independent review pending.** Revision `2-candidate.1`.
This namespace does not advertise product read/write support. Nothing here
extends `ledger-policy/1`, `ledger-event/1`, the first-slice assembler, or a store.

The [design](../../../docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md) specifies
bytes, identity tuples, admission, lineage, manifest membership and limitations.
The [candidate ADR](../../../docs/adr/candidates/outcome-records-v2.md) records
explicit encoding choices and the independent-review gate. The closed
[schema](schemas/canonical-records.schema.json) defines 21 record bodies and
kind-specific envelopes. All schema references are local fragments; validation
must never fetch evidence or schemas from the network.

`history-inputs.json` is the hand-authored source of synthetic histories, with
literal expected amounts and rational intermediates. `goldens/` contains exact
canonical JSON bytes, with **no trailing newline**, original receipt byte
strings, rejection/retry commands, hash vectors and a file inventory.
`reason-codes.json` defines the candidate public reason vocabulary. These are
contract examples, not proof of authentication, real assent, store atomicity,
concurrency, or a working evaluator. Synthetic seed base postings stand for
already accepted base economics; they do not define a new base pricing engine.

Run `sh scripts/check-candidate-contracts.sh` from the repository root with the
same Python audit dependencies as v1. `sh scripts/check-contracts.sh` runs both
v1 and candidate checks; `sh scripts/check.sh` also runs the full repository
checks. Python reconstructs every candidate byte from the literal histories.
Node separately reconstructs all record identities, hashes and manifest
memberships without importing Python, Rust or expected hash vectors. Semantic
and malformed-journal checks are independent of production Rust.

The explicit authorship command is
`python3 scripts/contract_checks/v2_candidate/reconstruct.py --write`.
It writes only this candidate's golden directory. **No check invokes it.** A
change to candidate inputs/encoding requires a reviewed diff and regenerated
candidate vectors; a passed checksum is not independent design approval.
After an intentional reviewed edit, refresh `review-manifest.json` with the
separate explicit `--write-review-inventory` flag on that same script. This
retains the pending-review status; it is not a freeze approval.

The original `contracts/schemas/`, `contracts/freeze.json`, `fixtures/`, source
reports, numbered ADRs, receipts and published v1 IDs are untouched. Candidate
schemas and examples deliberately live outside those frozen inventories.
