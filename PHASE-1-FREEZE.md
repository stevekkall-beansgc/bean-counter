# Phase 1 canonical contract freeze

**Frozen, unreleased. Owner roadmap Phase 1 is complete. Phase 2+ is not started.**

The independent reviewer issued **PASS TO FREEZE** for exact reviewed commit
`429aa027a696a09cfeb8dc1eada8420e732dda6b`, against approved semantic commit
`1e0ba3f886788c08f427d3aae1d916b341187e76`. Reviewer task:
`01a0c4c8-ca3d-7e71-b9be-3682e6555f4d`. The owner task
`01a0bf74-3b70-72b2-a1b0-ee6fb5f99dfb` subsequently authorized only these final
Phase 1 freeze actions. This records that approval; it does not infer approval
from the author's checks or grant later-phase work.

The established [frozen inventory](contracts/freeze.json) now registers the
[exact outcome freeze](contracts/freezes/outcomes-2-candidate.4.json). All existing
99 v1 pins remain unchanged. The outcome assets are frozen **in place** under
`contracts/candidates/v2/`; no schema, vector or source move is required. The
reviewed design, ADR, candidate report and entire candidate asset directory remain
byte-identical. There are 55 unchanged reviewed files, including the original
review manifest. The only overlays to its 56-file review inventory are:

- `ROADMAP.md`: accurately closes the independently approved Phase 1 gate and
  records the authorized freeze. Every byte from the Phase 2 heading onward is
  unchanged.
- `scripts/contract_checks/v2_candidate/audit.py`: uses the frozen approval/status
  inventory and reports frozen status. A digest of every byte outside its
  package-status block and final diagnostic print matches the reviewed auditor.

The new freeze manifest records both original and current hashes for those two
status overlays. The central inventory pins all 159 files, including the 99 v1
files, 55 unchanged reviewed files, two status overlays and three new freeze
metadata/check files. The audited register rejects missing reviewed pins, changed
approval/commit metadata or new exceptions for reviewed content.

The immutable profile is still **`2-candidate.4`**. Historical `candidate-not-frozen`
labels in reviewed files and the historical pending-review manifest are preserved
bytes, not current approval state. This document and the registered freeze
manifest establish current status. Do not regenerate or relabel reviewed sources,
change hash domains, or reinterpret old receipts to remove those historical
labels. Contract changes require another independent review. Authorized future
status work must keep every contract pin and the reviewed semantic code intact.

## Verification

`sh scripts/check.sh` passes the full offline suite: **110 Rust tests pass;
13 intentional opt-in/later gates remain ignored**, plus formatting,
warnings-denied Clippy, no-default compilation, source/dependency boundaries,
all frozen-v1 checks and independent Python/Node reconstruction.

- Frozen v1: **99 unchanged files**, 60 hash vectors, 25 accepted immutable
  records, 29 manifest members, original receipts and the 80-atom result.
- Outcome contract: **27 record kinds**, 24 immutable histories, 1,450 record
  vectors, 43 accepted decisions, 23 captured original Evaluations and 218
  explicit original-to-projection mappings.
- **49 fully rehashed semantic attacks**: 3,570 records/109 decisions pass
  Python/Node schema/hash integrity and then reject semantically.
- **Five fully rehashed scalar attacks**: 205 records/five decisions pass hash
  integrity and reject scalar validation. There are **272 packaged negative
  assertions**, plus four new freeze-metadata rejection checks.
- **217 scalar cases**, 38 record-field byte boundaries, and exhaustive text/source
  classification over **1,112,064 Unicode scalar values**: **2,224,128 checks per
  runtime** in Python, Node and the exact approved Rust archive.
- The accepted rehashed U+FEFF source regression adds 41 records/one decision.
  Approved Rust comparison passes **24 exact typed Evaluation roundtrips and
  44 decisions**, the original 86-attempt/23-history semantic suite, 11 exact
  deadline/ordering cases and the five scalar rejections.

The independent review additionally confirmed six separate mapping attacks
(246 records/six decisions) and 33 separate scalar attacks (1,353 records/33
decisions). Those are reviewer-reported evidence, separate from the packaged
attack counts above. No substantive Phase 1 blocker remains.

## Stop and later gates

The [owner roadmap](ROADMAP.md#phase-1--reconcile-and-freeze-the-canonical-contract-complete)
records completion. Production historical decoding/v1 bridging, authenticated
complete history, authority/supplier-reservation locking and atomic
crash/race/unknown-commit behavior on both stores remain later-phase gates.

No semantic sibling changes, production changes, dependency changes, Phase 2+
implementation, main merge, push, publication, deployment, service registration
or spending are part of this freeze. The lane stops at a clean local commit on
`codex/phase2-canonical`.
