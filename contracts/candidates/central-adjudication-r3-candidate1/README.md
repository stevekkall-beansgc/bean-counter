# Central adjudication R3 candidate1

Additive, unfrozen canonical contract for the authorized Phase4 slice. It is not a
production implementation or a backend proof. Existing source/frozen contracts
remain unchanged. The accepted base is
`b48dd50b89f7353f462ced9bf8bbe20c94a438bb`.

Read `PROTOCOL.md`, `protocol/schema.json`, `PROOF-WORKSHEET.md` and
`COVERAGE.md` together. `protocol/resources.json` derives closed byte, page,
workspace and counter envelopes from the schema and explicit binary index codec.
The complete typed object identity and every remote dependency are retained.

`validate.py TRACE.json` and the independently authored `validate.mjs TRACE.json`
reconstruct each journal, source proof, economic transition, resource account and
counter account. Output includes `accepted`, `summary`, and the complete diagnostic
`snapshot`. Exact positive trace schedules are `minimal-trace.json`,
`customer-trace.json` and `vectors/*.json`; invalid retained provenance attacks
are under `negative-vectors/`. The command schedule is a test interleaving, not a
canonical shared journal. `read-vectors.json`, `boundary-vectors.json` and
`index-vectors.json` specify read, comparison, byte, arithmetic and key outputs.

The customer trace atomically enrolls the unchanged original-profile base with
retail10000 and supplier3000, then reaches retail11450, gross adjustment250 and
five entitlements. The95-command witness matches the pinned S00–S14 order,
including DENY before the eligible upsell, all-family close and closed-family
correction. Exact authority sources accompany trusted host observations.
`authority_checks.py` verifies both retention attacks and all15 independent oracle
checkpoints. The alternative resolution1500 comparison totals11750 and
changes no actual decision. The original80 fixture remains a separate compatibility
control. The supplier release170 and partial-held200 regressions are separate.

Run all canonical checks offline with Python3 plus jsonschema and a Node runtime:

```text
PYTHONDONTWRITEBYTECODE=1 python3 check.py --output-dir /absolute/external/evidence
```

`PYTHONPATH` may identify an already cached jsonschema dependency directory;
`NODE` may identify an already cached Node executable. No dependency install,
network, service, database or runtime source change is performed. Evidence output
must be outside this candidate. `--python-only` is explicitly an incomplete author
check and cannot satisfy the two-validator gate. Default derivation/test commands
read and compare committed vectors; only explicit author flags `--write` and
`--write-vectors` generate them.

The Python base bridge invokes unchanged original-profile audits on exact retained
bytes, then checks independent byte/math facts. `SOURCE-PROVENANCE.json` pins those
source dependencies. Node independently reconstructs the same original facts and
new protocol without reusing Python implementation. `ARTIFACTS.json` inventories
candidate files except itself; its external commit/tree pins close that inventory.

SQLite, PostgreSQL17, PostgreSQL18, actual gateway fencing, physical page/WAL/work
bounds, crash/unknown-commit behavior, and observed nonposting remain PENDING.
Canonical validator agreement is not evidence for those gates. Separate exact
commit acceptance and runtime stages remain required; nothing here freezes a
contract or authorizes deployment.
