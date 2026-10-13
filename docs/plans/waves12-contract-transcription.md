# Wave 1 contract transcription handoff — 2026-10-12

GPT-6 Astra authored this bounded first pass in `codex/bean-counter-waves12-contracts-20261012`, source369caa2afc25b66c7d0b478e54fb64ac5eabeb6f. Owner authorizes Wave1–2 planning and implementation; this contract packet remains **partial, unfrozen**, and cannot certify whole Wave2. Canonical product and previous planning files were not edited. Owned output only `contracts/candidates/full-suite-v1/core/**` and this handoff. No runtime code, product dependency, database, release or push.

Source authority: isolated October7 design S12 revision2 §§3–10/15, all32 S12 requirements, CORE §2–6 and financial/recovery amendment. Read contributor/current scope, implementation, contracts/README, phase gates, detailed design §§1–11/26/27, canonical-record addendum. The runtime remains v0.9.7 contractv0.3/schema11 until separately implemented/migrated. Root coordinates; the technical choices in this packet are Astra-authored, not root-written architecture.

## Delivered

- `workflow.schema.json`:89 concrete closed Draft2020-12 definitions; typed data documents, bounded grammar, product/fact schemas, template/catalog/instantiation, policy/context/binding, sourcegrant/offer, compiler descriptor and symbolic typed compiled-plan ABI; eight operation kinds and original receipts/evidence/descriptor/status/head transitions. Dynamic Facts keys are closed semantically by exact pinned schema, never arbitrary economic JSON.
- `normative-amendment.md`: exact semantic validations that JSONSchema cannot prove, deterministic compilation/table/index/worst-case encoding, record-kind catalog, identity/head/window/correction-capacity obligations and mechanical transcription choices requiring review. Legacy schemas/fixtures unchanged. New rules do not change financial math.
- `registry.json`, `coverage.json`: explicit supported-shaped subset, no runtime capability claim, missing required families and semantic freeze blockers.
- `shape-fixtures.json`, `byte-vectors.json`, `audit.py`: seven authored structural positives/negatives and two ASCII canonical byte/domain vectors; checks do not serve as independent compiler/economic oracle. Reviewer authored separate expectations; independent equality/fulljournal freeze remains pending.
- `build_schema.py`: deterministic authoring helper; reproduces schema only. Never a source of economic expected values.

## First implementation boundary

Implementation agent may implement reviewed structural FactSchema/Product/SlotTemplate normalization/validation and candidate compiler/table APIs without claiming publication semantics passed. Initial future end-to-end target stays realSQLite publish→accept→activate→generic work→lost reply→original status→quantity correction→reopen, two customers sharing one publication, independent head revisions, stable identities and named-component outcomes. Eight shape variants do not prove those workflows. Source grants must authenticate actual authority; cannot seed arbitrary mock signed rights into supported real path.

Blocking complete Wave2: full canonical action/effect/manifest/decision/transition journal dictionaries and independent full-byte fixtures; all-mode compiler proofs and independently derived compiled-plan fixture; source authority/assent/roles bootstrap; complete evidence/attestation retention and original anchor recipes; cost/invocation/link/reverse/close plus stage outcome variants; existing mutation wrappers; capacity/settlement/supplier ownership/recovery metadata and projections where Wave2 owns them; migration, cold replay/unknown commit, read/preview/SDK normalization completeness and independent integrated tests. Refer packet remaining inventory; no scope item is removed. Financial meaning remains financial amendment; unresolved meaning stops dependent implementation, while syntax work may continue.

## Validation

Ran Python3 schema generator and local audit (89definitions,7shape cases,2ASCII byte/hash vectors). JSONSchema initially unavailable in system/Homebrew/bundled Python; coordinator supplied isolated test-only `/private/tmp/bean-counter-schema-review-20261012/bin/python`, jsonschema4.26.0. Ran Draft202012Validator.check_schema against full schema and each fixture through its exact $defs entry: all seven expected shape outcomes matched. This is shape/meta-schema evidence only. No full production gates, runtime/compiler economics, SQLite acceptance, provider tests, freeze or launch claims. No fixture silently altered to pass runtime.

Repro commands from worktree root:

```
python3 contracts/candidates/full-suite-v1/core/build_schema.py
python3 contracts/candidates/full-suite-v1/core/audit.py
/private/tmp/bean-counter-schema-review-20261012/bin/python contracts/candidates/full-suite-v1/core/validate_schema.py
```

Independent reviewer `/root/adversarial_money` has read candidate revisions. Reviewer previously authored the financial architecture; independent review here is of this new transcription/implementation, not independent approval of that earlier financial architecture. Reviewer identified head counter result encoding, raw scalar precision ambiguity and deterministic worst-case encoding; all received explicit candidate fixes. Separate reviewer final disposition remains separately owned; this handoff is not self-certification. No whole-wave time guarantee; next estimate requires actual integrated spike and reviewed remaining contract closure.

Reviewer checkpoint disposition received: accepts the partial **UNFROZEN** transcription for handoff; does not approve full freeze. HeadTransition, ordinary outcome/revoke omissions and raw scalar decision repaired. Deterministic worst_case uses fixed encoded ceilings plus mandatory proof. The disclosed1024 aggregate fact-slot ceiling remains a candidate mechanical bound requiring freeze review, not an already owner-approved full-suite capability constraint. Full journal/authority/registry/independent compiled-plan fixture blockers remain. Reviewer's exact final hash binding is maintained in their separate report.
