# Reconciled outcome contract candidate v2

**Candidate `2-candidate.4`, not frozen. Fresh-context review pending.**
Semantic authority: `1e0ba3f886788c08f427d3aae1d916b341187e76`.
Reviewed candidate `.3` remains at `09b076a3034064a85fd3da8d626ad02a9fb9a38a`.
This correction preserves actual original Evaluation identities, rejects repeated
resolved documents, and enforces whole-string scalar spelling. It changes the
candidate hash domain explicitly; missing original values cannot be migrated
from earlier projections by guessing.

The [design](../../../docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md) and
[ADR](../../../docs/adr/candidates/outcome-records-v2.md) specify all fields,
identities, ordering, trust and compatibility. The unchanged
[roadmap](../../../ROADMAP.md) governs later integration. The closed
[schema](schemas/canonical-records.schema.json) defines 27 record kinds.

`original-evaluations.json` holds 23 complete synthetic Evaluations captured from
the exact approved core. Original `ev_`, `cl_`, `ac_`, `ef_`, `ob_`, document IDs,
source references and dependencies stay unchanged. A separate one-to-one mapping
binds each original internal ID/binding/invocation to its candidate projection.
Native fields and vector order must survive complete typed roundtrips; amount
or action-count equality alone is insufficient.

`history-inputs.json` supplies synthetic expected outcome amounts and rational
intermediates. `goldens/` contains 24 histories, 1,450 retained records, 43 accepted
decisions, original receipts, replay probes and byte/hash vectors. The capped
preparation has no accepted Evaluation or base receipt. Every other history has
an immutable original base-acceptance root and explicit mapping closure.
The document-reuse example accepts a later correction through a different wrapper
for an earlier document; retry facts resolve to the original document and receipt.

Run `sh scripts/check-candidate-contracts.sh`. Python independently reconstructs
every golden byte and checks retained semantics. Node independently checks the
schema/scalars, keys, hashes, original document/event hashes and memberships.
Accepted-history audits require an original base-acceptance reference supplied
as an external trust anchor. Checks never regenerate candidate files.

The audit includes 49 fully rehashed semantic attacks (3,570 records/109
decisions): all pass Python/Node schema/hash integrity before semantic rejection.
Five additional noncanonical scalar histories (205 records/five decisions) pass
hash-only integrity, then fail schema/scalar validation. A separate book-key
invariant, malformed JSON and scalar/field boundaries bring negative assertions
to 272. The [scalar audit](SCALARS.md) includes 216 shared Python/Node/Rust cases
and 38 record-field byte boundaries. Custom schema keywords are normative;
ordinary shape validation alone is insufficient.

`sh scripts/check-contracts.sh` runs frozen-v1 and candidate audits;
`sh scripts/check.sh` runs the complete offline repository suite. Intentional
authorship uses `python3 scripts/contract_checks/v2_candidate/reconstruct.py --write`,
followed by `--write-review-inventory`. Neither command grants approval.

With the existing offline toolchain activated, run:

```sh
python3 scripts/contract_checks/v2_candidate/compare_semantic.py --source /path/to/approved-semantic-worktree
```

The runner requires exact HEAD `1e0ba3f`, first runs the current read-only candidate
audit, then archives approved source under ignored `work/` and adds temporary
test-only serde adapters there. It compares every original field/ID/vector through
23 typed Evaluation roundtrips, all 43 decisions and duplicate-document
rejections, document reuse and retry, 216 scalar cases, all five rehashed scalar
histories, and 11 deadline/ordering cases. The unchanged approved suite also
checks 86 attempts across 23 histories. The sibling remains read-only.

All frozen v1 files remain unchanged. This package adds no production decoder,
persistence, authentication service, migration, outbox execution or CLI behavior.
A separate fresh-context reviewer must decide freeze readiness before any later
integration, merge, push or deployment.
