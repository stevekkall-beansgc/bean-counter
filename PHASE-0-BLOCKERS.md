> Historical report. The five encoding blockers were resolved by `docs/design/CANONICAL-RECORDS-V1.md`; the local toolchain was subsequently supplied and verified. See `docs/phase-gates.md` for current status.

> **Resolution — 20 September 2026:** The five encoding blockers below are closed by [CANONICAL-RECORDS-V1.md](docs/design/CANONICAL-RECORDS-V1.md). All ten published IDs are preserved. Independent Python/Node reconstruction verifies 60 hash vectors, 25 accepted immutable records, 29 manifest members and all 16 fixture files. The original Phase 0 lead may resume remaining Phase 0 work; Phase 1 readiness is not implied. See [validation evidence](docs/design/CANONICAL-RECORDS-V1-VALIDATION.md). The original stopped report below is retained as history, not current blocker status.

# Ledger Lab v0 — Phase 0 stopped at the canonical-record gate

Status: **not frozen; not ready for parallel Phase 1 implementation**.

All five required source reports were read completely, in the supplied order, before files were written. The shared target directory was absent and has now been created. No production Rust code, crate scaffold, storage schema, evaluator, paid service, deployment, publication or release was created. No Git repository or commit was created.

The task explicitly requires stopping if exact first-slice fixtures cannot be derived without changing the design. The detailed design also directs the implementing agent to report fields/outcomes that cannot be derived before changing a golden journal (§1, reader guide). That condition is reached: the economic result and ten published IDs are unambiguous, but the full canonical journal and receipt are not yet uniquely specified. This is an encoding-contract gap, not a disagreement about who owes the 80 atoms.

## Blocking definitions

The controlling source is `LEDGER-LAB-V0-DETAILED-DESIGN.md`, revision 1. Line references below identify the current source; its SHA-256 is recorded in the adjacent audit evidence. §27 resolves earlier packaging/product differences but does not define these missing record encodings.

| Missing definition | Source evidence | Why this blocks an exact frozen journal | Required addendum |
|---|---|---|---|
| Complete canonical snapshot S body | §26 line 1404 lists semantic contents (DSL/semantics, six documents, tier, principal/source, grant revision, currency/scale). §5 defines a smaller `PolicySnapshot` in Rust notation. | No exact JSON keys/containers, document ordering, or `document_type` token for the S hash are specified. Several faithful encodings produce different DocumentIds. Actions reference S, so their record hashes also change. | Complete `ledger-snapshot/1` schema and one exact first-slice body, with its document hash type and input-document reference organization. |
| Seven snapshot association purpose values | §26 line 1417 requires seven distinct purpose IDs and hashes `[event_id,purpose,document_id]`. §12 line 808 gives `purpose` without an enum. | `policy` and `policy_source`, for example, are both unrestricted strings under the supplied model, but produce different `sr_` IDs for the same E/document. These IDs are manifest members. | Freeze all seven literal purpose values and their document mapping. |
| Explanation wire encoding | §5 lines 272–276 defines `ExplanationStep`; line 297 describes `ExplanationInput` as a tagged value without its tag/value wire shape. §26 lines 1408–1409 supplies rational values and named basis semantics. | Domain structs and values do not define the precise tagged JSON form, nor where the named basis string is retained alongside `basis: ExactRatio`. Complete explanation bytes and their generic record hashes cannot yet be calculated. | Exact `ledger-explanation/1` and tagged-input schemas, rational numerator/denominator string rules, named basis field, and both first-slice explanation bodies. |
| Effect facts hash | §12 line 809 requires `facts_hash`; §26 line 1405 says it records the exact action economics. §6 defines `claim-facts` and generic record hashing but no effect-facts payload. | Hashing a whole action, an economics projection, or the effect content are distinct choices. No exact selection or domain is established. | Define the effect facts hash domain and complete payload, including which provenance/ID fields are included or excluded. |
| Manifest membership and composite-key record identity | §6 line 367 requires `{kind,id,content_hash}` memberships. §12 line 815 says every accepted record is listed; line 833 requires the original delivery mapping. §26 line 1414 names a narrower list. Source/dependency edges and chain revisions have composite keys, with no complete manifest-ID encoding supplied. | It is unclear whether all immutable join/revision/mapping rows are separate members, or covered by parent canonical bodies, and how to identify/hash composite-key members. Both approaches can preserve the economic model but yield different decision hashes. | Enumerate the complete first-slice membership set, exact kind tokens/order, composite-key ID/hash representation, and canonical manifest schema/body. Explicitly exclude self/receipt and mutable delivery metadata. |

These gaps must not be filled by silently choosing JSON names and then calling the resulting bytes a transposition of the design. A short canonical-record addendum can settle them without expanding the product's financial behavior. It should also show the six exact preseed document bodies and all accepted record bodies, including the intention payload and control transition, so two independent implementations can reproduce the same bytes. Any fixture-only labels/timestamps chosen during that work should be identified as newly frozen constants.

The obligation hash's `roles` value is the six-field role object, with no `schema` or null payer delegation. This was independently confirmed against the published O vector. A retained roles document may carry its own schema; implementations must not accidentally hash that wrapper into the obligation tuple.

## Completed local checks

The independent document audit passed:

- 12 JSON blocks parse; six YAML blocks parse with the specified JSON-compatible Boolean interpretation (`on` remains a key).
- Both supplied Draft 2020-12 schemas pass metaschema validation; six event examples and three policy examples validate structurally.
- The normalized §26 event is byte-for-byte equal to its published canonical line, with no final newline in hash input.
- All ten published E/C/D/R/F1/F2/A1/A2/O/I values agree with Python and a separate Node canonical-byte/SHA-256 calculation.
- The supplementary-plane/U+E000 key-order case, composed/decomposed identity distinction and escaped/literal equivalence pass.
- Fifteen independent exact-fraction arithmetic groups pass: first slice, priority, unit/fraction, signed ties/non-ties, additive/sequential discounts, positive/negative and weighted allocation, uncapped/capped supplier totals, share ceiling, reversals, rounding stage and booked limit comparison.

This is document evidence only. It does not validate a production strict parser, full JCS implementation, authority checks, Rust arithmetic overflow handling, storage atomicity, cancellation, concurrency, TLS, artifact support or release readiness. No snapshot S, full decision hash or receipt was invented to fill the gaps.

Reproduction from the shared target (using the already installed document-check dependencies on this machine):

```sh
PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps \
python3 work/phase0-audit/check_design.py \
  --design /Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/outputs/LEDGER-LAB-V0-DETAILED-DESIGN.md \
  --output work/phase0-audit/check-result.json
```

Observed tools: Python 3.14.7, Node v22.23.2, jsonschema 4.25.1 and PyYAML 6.0.2. These are document-check observations, not proposed production dependency pins. The helper code is deliberately under `work/`, not in the production core or testkit.

## Machine and remaining gates

- `rustc`, `cargo` and `rustup` were not found on PATH; the standard `~/.cargo/bin` and `~/.rustup/toolchains` locations were also absent. No Rust toolchain, dependency version or MSRV has been claimed or pinned.
- The available `git` command fails with the unaccepted Xcode license notice. No license was accepted on the user's behalf; no Git initialization or commit was possible through that tool.
- Package/npm/image namespace availability and actual native runner availability remain unchecked. No remote or cloud-authenticated operation is necessary to inspect the current blockers.
- After the addendum, Phase 0 still needs the three-production-crate/unpublished-testkit scaffold, all versioned schemas, compatibility metadata, accepted ADRs, full first-slice/authority/golden fixtures, boundary checks and release target contract. The source's financial decisions are not being reopened.
- Phase 1 still owns resolving exact Rust/MSRV/SQLx/SQLite/Rustls/JCS versions; tracked transactions and cleanup/cancellation; both real stores; PEM-only/public trust tests; separate SQLx offline metadata or tested typed runtime queries; first-slice write/commit failure tests; and native build feasibility.

The `repo-release` skill was read for local repository setup. Remote publication/registration/release work was excluded by the explicit task scope. None was performed.

## Files and handoff

Created in the shared target:

- `README.md` — blocked status and navigation.
- `PHASE-0-BLOCKERS.md` — this report.
- `work/phase0-audit/check_design.py` — read-only schema/byte/ID/arithmetic checks.
- `work/phase0-audit/check_ids.mjs` — independent Node JSON-byte/hash check.
- `work/phase0-audit/check-result.json` — successful document-check evidence.
- `work/phase0-audit/source-digests.json` — exact source-report hashes.

Commit: **none**. Repository readiness for parallel Phase 1 sessions: **no**. An agreed canonical-record addendum is the first dependency; a usable local Rust/Git toolchain is the next setup gate.

Direct delivery of preliminary status to the calling Codex task was rejected by automatic approval review, which cited private project details and insufficient authorization for the destination. No indirect send or workaround was attempted. The final report and evidence are available in this task's outputs for the user/calling task to inspect.
