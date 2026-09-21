# Reservation settlement / 1 — contract-only freeze

**Frozen, unreleased. No runtime, SQL or production port implementation is authorized.**

The owner authorized this bounded freeze after two independent PASS verdicts for
exact reviewed candidate `d311d9622b527863c79fd5e5820f5b890ab2c6b1`, based on Phase 2
commit `6194376a053b8a27887a9b09459054a7af3a1769`. R1 (reused delivery and immutable
record identities) is closed. This status overlay supersedes only historical
candidate/pending-review status language; it changes no normative contract byte.

The separate inventory is `contracts/freezes/reservation-settlement-1.json`.
It pins all 13 reviewed files, including the original candidate review manifest,
in their original locations. The profile remains `reservation-settlement/1`.
Historical `candidate-not-frozen` labels and reviewed prose remain unchanged.
The existing `contracts/freeze.json`, its 159 assets/pins, the old freeze validator
and all old schemas, IDs, hashes, fixtures and semantics remain byte-identical.
No existing registry entry is removed, extended or reinterpreted.

## Approval and provenance

- Fresh amendment reviewer: task `01a0c585-75f4-7cd0-ad22-f01dd8efb86d`, PASS for
  exact `d311d96`, safe for this contract-only freeze. Its original report is
  retained byte-for-byte at `docs/reviews/reservation-settlement-1/amendment-review.md`.
- Independent conformance reviewer: task `01a0c551-f10d-73c3-871c-7242c663e851`,
  PASS for the same candidate; review commit
  `6a81c93f15211b89bbc9812c5e39645026ba19d6`. The exact committed report is retained
  at `docs/reviews/reservation-settlement-1/conformance-review.md`.
- The precommitted independent numeric oracle remains
  `6ae64d14fac96068caa3e0d91a70f5e6ba23143d`. Its three source/document digests and
  the review-report digests are bound in the separate freeze inventory. No oracle
  source or expectation was changed by this task.
- Owner authorization: task `01a0bf74-3b70-72b2-a1b0-ee6fb5f99dfb`, limited to
  separate freeze metadata, this status overlay, a read-only checker and aggregate
  check wiring. It does not authorize an implementation, merge or release.

The fresh reviewer additionally rejected 78/78 fully rehashed earlier-label reuse
permutations in both runtimes and verified distinct-scope reference isolation.
Conformance independently reconstructed 112 positive and 63 original attack
records and compared 10 precommitted numeric history variants. These are reviewer
results; they are not counted as new real-store tests or as executions by the
freeze checker.

## Validation and reproduction

With the existing offline toolchain/dependency environment active:

```sh
python3 scripts/contract_checks/check_reservation_freeze.py
sh scripts/check-reservation-settlement.sh
sh scripts/check.sh
```

The aggregate script retains its original checks byte-for-byte and appends the
new freeze checker and the unchanged reservation check. Old validators do not
need to recognize a new registry shape. The new checker verifies exact review
membership, all reviewed bytes, separate approval/oracle provenance, the preserved
legacy registry/assets, and exact aggregate wiring. Negative probes are in memory;
it has no authoring, regeneration, update or freeze-writing mode.

Contract reconstruction covers three record kinds, 12 histories / 44 steps / 112
records, 47 adversarial cases (46 independently hash-verified before rejection),
nine strict JSON negatives, and 13 non-appending lookup cases in Python and Node.
The full offline baseline has 128 passing Rust tests and 19 intentionally ignored
entries, plus formatting, Clippy, no-default builds, boundaries and all old frozen
contract checks. This freeze adds metadata checks only; it creates no new Rust
implementation or real-store acceptance evidence.

## Remaining gates and change control

Actual authority and assent, complete original economic receipt/replay bridging,
semantic aliases, production unknown-commit resolution, both-store atomicity,
reopen equality, real races, cancellation and crash behavior remain implementation
gates. Synthetic input observations and successful hash checks do not establish
those properties. The proposed `PORT.md` remains an unimplemented design.

Any normative, schema, identity/hash-domain, vector or semantic change requires
another fresh independent review. Future status changes require explicit owner
scope and must preserve the reviewed bytes and old frozen inventory. No moving
assets or modifying legacy validators is authorized by this freeze.

The lane stops at a clean local freeze commit. No SQL, runtime, port, migration,
main merge, push, tag, publication, deployment, service registration or spending.
Spend: $0.
