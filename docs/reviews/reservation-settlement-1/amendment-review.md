# PASS — successor contract review

Exact candidate: **d311d9622b527863c79fd5e5820f5b890ab2c6b1**. Predecessor: **557a4a6de2a518ad1670935849fef680cac460f7**. Reviewed Phase 2 base: **6194376a053b8a27887a9b09459054a7af3a1769**.

**R1 is resolved. No remaining blocking findings were identified. The exact successor is safe for a contract-only freeze within the boundary below.** This supersedes the earlier FAIL only for the successor; it does not retroactively approve the predecessor. No freeze action or implementation work was performed.

## Findings by severity

- **P1 R1 — resolved:** accepted journal rows are now unique by `(scope,kind,id)`; new accepted decisions are unique by `(scope,source,external_id)` across operation kinds. Reference registries use the semantic identity without the content hash and reject a conflicting hash. Repeated references to the same immutable record remain valid.
- **New blocking findings:** none.

Exact code evidence: `scripts/reservation_settlement/audit.py:54–69,110–120,204–215`; independent Node checks at `scripts/reservation_settlement/verify.mjs:43–45,63–64,92–93`.

I reused the exact saved counterexample from the initial review, without regenerating it to accommodate the fix. Python now rejects it with `DELIVERY_IDENTITY_REUSED`. The original Node adversarial entry point independently verifies every rebuilt row identity and content hash, then rejects the history. I also generated all **78 earlier-label/later-step reuse permutations** across the 12 positive histories. Both validators reject every fully rehashed permutation. These include registration, ordinary, post-hoc and closure collisions.

Additional direct reference checks confirm that identical repeated references are allowed, different hashes under the same scoped identity reject, and distinct scopes remain isolated. The existing nine new adversarial regressions cover reused transition identities, conflicting reference hashes, cross-operation labels and identical retry append attempts.

## Retry and unknown-outcome evidence

Python `audit.py:76–84,227–247` and Node `verify.mjs:47,96` implement the synthetic non-appending lookup checks. The 13 cases in each runtime verify original ordinary, correction, closure and no-op closure receipts; changed command/ingress conflicts; read denial without disclosure; known durable acceptance; proven absence; and inconclusive absence remaining unknown. The delivery registry is compared before and after each lookup.

The comparison retains exact additive settlement receipt bytes and the original synthetic economic receipt reference. It does **not** establish full original economic receipt byte preservation, real authentication, actual current-write-authority revocation, live stale-guard handling, or primary database commit resolution. Fields describing later write authority/time in the fixture are contextual; this lookup helper intentionally has no current-write-authority or clock input. Runtime ordering still needs implementation evidence. Ordinary semantic aliases are specified, but this helper is not their production implementation.

Those limits are stated in the successor documentation and do not undermine this bounded contract fix.

## Preserved semantics and bytes

The seven changed paths are confined to the two new validators, added adversarial/lookup fixtures, review inventory and explanatory status documents. The following are byte-identical to the predecessor:

- `records.schema.json` — SHA-256 `361c8e8a0f3f25af4deb790b73d6544afbba592e5687d8f68881d69d1cf51aa5`
- `histories.json` — SHA-256 `d04163099f0f827cd91317f478a923a7a63c9f3ac6625ecdea29c5f151b214c9`
- `vectors.json` — SHA-256 `dda072418bdd5a3059c9d083f490b80bacdb35319bd535cf951bc2ccbbc4287c`
- `PORT.md` — SHA-256 `cae8abd1a3de4bc360f9cb7ecec5be54fe81a28e99cbdd7defc0028acfa1a3aa`
- Canonical reconstruction helper and standalone check script.

Exact Git-object comparison against the Phase 2 base confirms **all 159 frozen files and the old freeze registry remain byte-identical**. Old validators, production code, Cargo files and migrations are unchanged.

The predecessor's reviewed accounting, closed schema, hash domains, topology, registration linkage, frozen membership, deadline equality, closure authority requirements, lock ordering and post-hoc invariants therefore carry forward. Ten independent numeric projections from oracle commit `6ae64d14fac96068caa3e0d91a70f5e6ba23143d` were rerun through the successor Python validator: **87 records pass**. The original oracle's 28 methods passed in the earlier review; they were not rerun during this focused successor review. The projection bridge remains an accounting comparison, not a connected coordinator or store adapter.

## Validation rerun

- Candidate Python/Node checks: **PASS**, 12 histories / 44 steps / 112 records; 47 adversarial cases, including 46 independently hash-verified Node cases; nine strict JSON negatives; 13 non-appending lookup cases in each runtime.
- Saved original R1 counterexample: **correctly rejected** in Python and Node.
- Reviewer-generated label-reuse permutations: **78/78 rejected** in both runtimes after canonical reconstruction; Node verifies all row hashes before semantic rejection.
- Independent numeric projections: **PASS**, 10 histories / 87 records through successor Python.
- Exact-byte comparisons and review-manifest inventory: **PASS**.
- Full offline suite: **PASS**, 128 Rust tests passed, zero failed, 19 intentionally ignored; formatting, warnings-denied Clippy, no-default build, source/dependency boundaries and all old Python/Node contract/freeze checks passed.

Logs: `r1-candidate-check.log`, `r1-original-counterexample.log`, `r1-reviewer-probes.log`, and `r1-full-offline.log`. Machine-readable review evidence: `r1-review-evidence.json`.

## Precise freeze-only boundary

This is approval to freeze the exact reviewed contract package, not approval to implement it. A subsequent owner-authorized freeze-only change may:

1. Add a separate approval/freeze manifest outside the candidate directory, binding the exact candidate SHA and file digests, this verdict, and the reviewed base/oracle provenance. Preserve historical candidate labels and existing reviewed file bytes.
2. Add a status overlay recording that contract-only approval.
3. Add a read-only checker for that manifest and wire it and the existing reservation check into the aggregate offline gate, without changing canonical reconstruction or acceptance semantics.

**Preserve all 159 existing frozen assets, their pins, and the legacy freeze registry/validator unchanged.** The legacy validator intentionally requires its existing exact registry shape. A separate additive freeze manifest avoids weakening that gate and avoids the candidate directory's exact inventory check. A proposal to modify the legacy registry/validator, change reviewed normative bytes, move assets, alter hash domains, or regenerate expected vectors needs separate review; it is not covered by this approval.

No SQL, runtime, production port, migration, new authority provider, merge, push, tag, publication, deployment or service registration is approved here. Genuine assent/authority, complete composite economic replay, both-store atomicity, actual concurrency, crash/cancellation and unresolved-active-commit proof remain later implementation gates. Synthetic fixtures are not evidence for those gates.

Review performed in a local no-network clone at the exact successor. No tracked source changes or commits. Spend: **$0**.
