# Canonical records v1 — validation and Phase 0 handoff

**All five canonical-record blockers are closed. The original Phase 0 lead can resume remaining Phase 0 work.** This is a bounded specification repair; it does not establish overall Phase 0 completion or readiness for Phase 1.

The normative addendum is `docs/design/CANONICAL-RECORDS-V1.md`. Its complete schema appendix matches `contracts/schemas/v1/canonical-records.schema.json`. Eight convenience schemas resolve immutable URNs through a local registry without network access. All six input document bodies and every accepted immutable body are frozen in the addendum; the machine artifacts are their canonical transcriptions.

Decisions frozen:

- Snapshot document type `snapshot`; six typed document associations plus `decision_snapshot`, seven independently identified association rows.
- Exact explanation tagged inputs and named basis; ratios are reduced signed numerator/positive denominator strings.
- `effect-facts` hashes the stated economic projection, excluding rule/version/snapshot provenance; both payloads and digests are retained.
- Composite record IDs are actual scoped JSON arrays. All immutable edges, original delivery mapping and chain revision are manifest members.
- Manifest membership is 23 new immutable records plus six retained input documents (29 total); manifest/receipt themselves and mutable state are excluded. Acceptance creates 25 immutable rows plus one delivery-state row and one chain-head update.
- All timestamps, omission/null rules, kind tokens, six seed document bodies, four immutable seed rows, operational state, intention payload, control transition and chain revision are frozen. Every newly chosen encoding is identified as an amendment.

Validation completed:

- The original ten E/C/D/R/F1/F2/A1/A2/O/I values are unchanged and independently rederived in Python and Node.
- Both implementations reconstruct 60 complete framed hash vectors and all 16 fixture files byte-for-byte, including all 25 accepted records and the full manifest/receipt.
- Nine schemas (79 bundled definitions) pass Draft 2020-12 metaschema validation; every immutable fixture envelope validates. Unmodified convenience schemas resolve locally. All seven explanation input tags validate.
- 26 negative checks reject missing edges/members, wrong ordering, duplicate IDs, invalid tags/fields/nulls/numbers/ratios/dates and changed economics. Rehashed corrupt manifests/actions also fail semantic checks, so this is more than checking stale hashes.
- Existing design checks still pass: 12 JSON and six YAML blocks, two source schemas, six event/three policy examples, 15 exact arithmetic groups and Unicode canonicalization vectors.
- All five source-report SHA-256 values remain equal to the original source-digest evidence. No source design/report was edited.

Document/fixture checks do not validate production parsing/JCS, authority, database durability/atomicity/concurrency, cancellation, Rust, TLS, native artifacts or release readiness. Existing toolchain, remaining schemas/goldens/ADRs/scaffold and platform gates remain with the Phase 0 lead. This repair does not add broader claim-facts or reservation-transition variants by inference.

## Exact hashes

| Item | Value |
|---|---|
| S | `doc_2fd45201a54061473b72cae1cc3725bdf6e170174ce9dd691f8c31ff6c850dc6` |
| XP0.content | `sha256:9c23339fe7730dc24344a4d6d5b8aca6229769d77271eb7a0fdcdd95894eff25` |
| XP1.content | `sha256:df960040b355d5d41e203a29558d7d481de167d387b7dfbe1cf89778af949118` |
| effect-facts.1 | `sha256:a98afa4aa04787ae5e2c2a959dcc002bd32e9ca2357e02b247bda79467f08b5b` |
| effect-facts.2 | `sha256:20606dc8f571d94bec98513f528efd9573525461e9ec21dbca75f528f7ef543f` |
| decision-content | `sha256:33dd38b0ae18a1a037b18494115b3689c47e9378e9af466fd57837e70091c650` |
| R.content | `sha256:e14fb72aa8086999c6b8f67794f35399b031cb331c0a06fb7a2a45aa5a0af1bd` |
| intention-payload | `sha256:150e18f1f982cde7317f9d771405994d3c3b537349c9211b96526df230e588f1` |
| Accepted journal, raw SHA-256 | `51c768879cdd78a0bbcbe436d3ca91c3f6bdeb0df15f02d47e9f3e1ba67eb1a2` |
| Addendum, raw SHA-256 | `2a28e800a9037de36c9e82070225632eca449be16c26a74e11336c210464c0e8` |

## Reproduction

From the shared target:

```sh
PYTHONPATH=../ledger-lab-v0-detailed-design/work/check-deps python3 work/phase0-audit/check_design.py --design ../ledger-lab-v0-detailed-design/outputs/LEDGER-LAB-V0-DETAILED-DESIGN.md --output work/phase0-audit/check-result.json
node work/phase0-audit/check_ids.mjs --journal .
```

Machine evidence is `work/phase0-audit/check-result.json`, including the nested canonical-record result. Audits are read-only. `freeze_records.py` is an explicit contract-authoring helper, never called by the audits; do not run it to make a production implementation pass. Any fixture change requires a new documented design amendment/review.

No product implementation, Rust scaffold, database, Git repository/commit, deployment or release was created.

## Direct notification status

The completed files are available in the shared target. Direct messaging to the calling task was rejected twice by automatic approval review: first for unverified destination ownership/authorization, then because task-history evidence was not accepted as authorization. No workaround or indirect send was attempted. Explicit approval for the prepared message has been requested. This notification block does not affect the completed canonical-record repair or its validation results.
