# Phase 3 R1: enforce host write authorization

This bounded successor addresses P1/R1 in the independent review of
`930497b1d21e684a7814c2edcd72f7676b9d2937`. That candidate only invoked the
mandatory host verifier with `write=false`, so a host that allowed reads but
explicitly denied writes could still accept a fresh operation. The new SQLite
regression reproduced fresh registration acceptance on the reviewed source.

The coordinator now resolves original-identity retries and ordinary semantic
aliases using read authorization. Its semantic lookup pass disables both
submission and correction rights, so it cannot construct a fresh accepted
decision. If no read-only result resolves the request, the coordinator invokes
`OutcomeAuthority::verify(..., true)` under the complete held locks, validates
that returned proof against the current grant and request, and uses it to build
the fresh plan. Registration terms/finality, economic evidence and early-closure
rights come from that write proof. Store adapters still decide neither economics
nor authority. Unknown-commit handling and the five-second admission budget are
unchanged.

Coordinator regressions deliberately supply different read and write proofs.
All four operation types accept with minimal read authority and the correct
write proof; the persisted observation contains the write proof's authority.
Missing verified registration terms/finality, economic evidence or early-close
rights in the write proof reject before append even when the read proof contains them.
The host callback checks the complete lock set when write authorization is
requested. Existing read-only retry and ordinary-alias tests now explicitly deny
host write authorization.

SQLite and PostgreSQL denial regressions exercise fresh registration, ordinary
submission, correction and closure. Each verifies the host's denial is returned
and every physical table remains exactly unchanged, including after close/reopen.
After authorized acceptance, denied-write original retries retain the original
state; an ordinary alias after correction/closure retains the original receipt
pair and only adds the alias identity. Tests use synthetic local authority and
do not certify a production host implementation.

## Validation

Final source validation (installed Rust 1.98.1, locked offline dependencies,
linked SQLite 3.51.3 and cached local PostgreSQL images):

| Gate | Result |
| --- | --- |
| Complete `sh scripts/check.sh` | PASS: 172 Rust tests, 0 failed, 32 ignored; formatting, strict Clippy, no-default-feature, architecture and independent contract/freeze checks |
| PostgreSQL 17.11 affected outcome suite | PASS: 13/13, 170.75s |
| PostgreSQL 18.6 affected outcome suite | PASS: 13/13, 170.77s |
| Separate final PG17 identity/correction race rerun | PASS: 1/1, 19.12s |
| Fresh reopened SQLite/PG17/PG18 independent comparison | PASS: four prefixes, 82 retained records and eight negative probes per store; exact records/original receipt pairs/anchors/heads |
| Reviewed-parent versus final SQLite canonical prefixes | PASS: exact records, original receipts, anchors and heads |
| Preservation | PASS: all 173 saved baseline pins; 159 legacy frozen files; 13 reservation files plus five controls; all eight migrations, core code and dependency manifests unchanged |

The 32 ignored offline entries comprise 30 opt-in PostgreSQL tests and two
pre-existing deferred fake-destination gates. The affected PostgreSQL filter
runs the 13 Phase 3 outcome tests; it does not rerun unchanged legacy
service/outbox/upgrade/TLS suites. Each major includes 386 validated-plan failure
positions, 14 primitive positions, eight actual COMMIT request/reply cut trials,
real cancellation and races. PostgreSQL cancellation is not claimed at all 386
positions. The aggregate includes SQLite's 360 failure/cancellation positions
and eight process kills.

An initial implementation passed its offline aggregate but its PG17 run reported
12 passes and one concurrent-closure `Unavailable` failure. Those logs and its
diff are retained. Local proof validation was subsequently consolidated at the
start of planning, and a missing-finality negative case was added; all final
source checks above were rerun. This does not establish the cause of the failure.

Exact commands and hashed evidence are recorded in the external R1 handoff.
Operational logs and database artifacts remain outside source control. All 20
prior integration evidence entries retain their original hashes. Temporary local
containers/volumes were removed, the initially stopped test VM was stopped, and
Docker context was restored to `default`. Infrastructure spending: $0.

All saved baseline pins, frozen contracts and controls, all eight migrations,
core economic rules and dependency manifests remain unchanged. Only coordinator
control flow, its fixture caller, authorization regressions and this disposition
are changed. The independent review's FAIL remains the disposition of the exact
parent; this successor requires a focused independent re-review before Phase 3
exit may be declared complete.

## Retained qualifications

The earlier concurrent PostgreSQL closure returned `Unavailable`; an initial R1
PG17 run reproduced the same reported failure. Its cause remains untraced. Subsequent
passing runs do not establish its cause or certify arbitrary-load availability. No deadline was enlarged and the error was not
reclassified. The original diagnostic and the earlier PG17 observer-query failure
remain in the prior evidence; neither is erased by these new runs.

The one-final-base/no-predecessor-chain boundary, bounded four-step durable
comparison, deferred real host authority/provisioning/public submission and all
other qualifications in `PRODUCT-PHASE-3-INTEGRATION.md` remain applicable. No
Phase 4, main merge, push, tag, release, publication, deployment, remote service
registration or spending is authorized by this fix.
