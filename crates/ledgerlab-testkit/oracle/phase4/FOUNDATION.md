# Supplier foundation observer — not full Phase 4

`foundation-expected.json` is a separate, literal, language-neutral oracle for
the existing persisted supplier fixture. It does not change `stories.json`, the
prior 66-case mapping, any canonical profile, or any accepted term. Its retail
basis is **10000 atoms**, not the earlier hypothetical retail story's 8000.
This difference was found by inspecting the unchanged original input policy:
`supplier-separation` has one fixed retail price 100 USD and supplier price
30 USD, no retail discount. Integration owner explicitly confirmed using that
existing basis. `outcome_fixture_seed.rs` adds only the supplier contingent rule
and fixed fee code; `outcome_fixture.rs` selects supplier events only.

Hand-audited expected history:

| Prefix | Supplier live net | Retail live net | Held | Consumed | Released |
|---|---:|---:|---:|---:|---:|
| Registration | 3000 | 10000 | 12000 | 3000 | 0 |
| Ordinary fixed +2500 | 5500 | 10000 | 9500 | 5500 | 0 |
| Correction to zero, exact inverse -2500 | 3000 | 10000 | 9500 | 5500 | 0 |
| Explicit closure | 3000 | 10000 | 0 | 5500 | 9500 |

Original control and two distinct alternatives preserve every supplier code
amount; only unobserved retail amounts vary. The retail family stays unobserved,
not zero-claimed. The supplier family retains revision 2, code `none`, amount 0
and closed ordinary status after correction/closure. No supplier replenishment,
activation, policy upgrade, new claim or comparison posting is implied.

`foundation.py` consumes the actual facade JSON on stdin:

```sh
python3 -B crates/ledgerlab-testkit/oracle/phase4/foundation.py < report.json
python3 -B crates/ledgerlab-testkit/oracle/phase4/foundation.py --evidence < evidence.json
python3 -B -m unittest discover -s crates/ledgerlab-testkit/oracle/phase4 -p test_foundation.py -v
cargo test -p ledgerlab-testkit --test phase4_foundation_oracle --locked --offline
```

Report checks cover all four prefixes (0–3 outcome/closure steps), exact reduced
fractions/inverse/replacement/delta, binding totals and differences, latest family
values, six roles/book/limits, reservation projection, complete unchanged
supplier amounts, complete family/code membership, explicit noncommit and exact
foundation scope. It independently reconstructs candidate and report private
fingerprints using documented byte framing; label/order variations are allowed.
These hashes are not financial authority. Original snapshot/activity hashes and
receipt references cannot prove anchored canonical provenance without retained
source records; that verification remains the facade/retained audit's job.
The receipt shape includes registration's base acceptance reference, ordinary
and correction economic references, and no closure economic reference.

The `--evidence` object contains `report`, `backend`, `B0`, `B1`, `B2`,
`attempted_writes`, `external_calls`, and `metadata` keyed by B0/B1/B2. Every
inventory preserves actual full column lists and opaque DB-produced row strings.
SQLite uses `[table,columns,rows]` entries for 33 application tables plus
`sqlite_schema`, `user_version` (columns `["value"]`, one decimal string row),
and optional `_sqlx_migrations`. PostgreSQL uses all 37 tables mapped to
`{columns,rows}`. Metadata contains nonempty schema/index/constraint information
and SQLite `user_version` as a one-string list. B0/B1/B2 equality is exact;
missing/unexpected tables, metadata mutation or nonzero attempted writes/calls
fail. The harness must collect trace counts; passing literal zero without a real
observer is not trace evidence. PostgreSQL schema metadata is supplied separately
because row/column inventories alone do not cover indexes or constraints.

Fourteen tests exercise checker sensitivity, including 792 result-leaf mutations and 642 per-table mutations
across B1/B2, malformed/incomplete inventories, all four prefixes, source policy input verification, supplier-term changes with all
private fingerprints recomputed, extra/missing/duplicate membership, and report
promotion attempts. Synthetic checker seeds are explicitly not product output.
The Rust entry only wires those tests into the existing suite, adding no core or
facade dependency. No expected amounts are imported from Rust.

The observed SQLite report supplied by integration passed independently without
changing numeric literals. Other real reports/envelopes must be checked directly;
checker tests alone certify neither stores nor host authority. Full retail
comparison remains forbidden: the foundation cannot seed retail outcomes through
the existing supplier-only acceptance path, and supplier candidates cannot vary
supplier terms. Do not call this a complete Phase 4 exit.
