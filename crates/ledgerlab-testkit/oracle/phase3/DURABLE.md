# Integrated durable lifecycle check

`durable.py` connects the previously independent numeric oracle to the bounded
four-step coordinator fixture on file SQLite and isolated PostgreSQL 17/18.
The fixture supplies synthetic original inputs and test authority evidence.
Each command is accepted through the real private coordinator and adapter; the
observer closes the store, reopens it, then reads the actual complete record
partition, heads, anchors and original delivery pairs. It also compares all
physical tables before and after reopen. An independent direct SQL inventory
must agree with the partition's full set of stored envelopes.

The observer passes no expected accepted bytes from a plan to Python. Plans
are used only to identify the partition/locks and original delivery lookup keys.
Python verifies the persisted economic history with the existing independent
contract validator (including replay closure, original evaluation material,
claim revisions, exact inverse/replacement equations and economic limits).
The numeric `lifecycle.py` oracle independently expects base consumed 3,000,
held 12,000; ordinary +2,500; correction to zero without changing reservation;
and closure releasing the remaining 9,500. Settlement envelope IDs/hashes are
reconstructed independently with the frozen Python codec. The check compares
reservation heads, complete receipt replay membership and all original pairs
after every later step. Eight negative probes reject missing records, a changed
historical pair and a changed reservation revision.

The Rust test entry `outcome_durable_independent_oracle` exists for each adapter.
SQLite executes in the ordinary offline workspace suite. PostgreSQL's entry is
explicitly ignored until an isolated TLS service is supplied. Set the optional
`LEDGERLAB_P3_EVIDENCE_DIR` to retain observed JSON outside source control. With
three generated observations, run:

```sh
python3 -B crates/ledgerlab-testkit/oracle/phase3/durable.py --compare \
  "$LEDGERLAB_P3_EVIDENCE_DIR/sqlite.json" \
  "$LEDGERLAB_P3_EVIDENCE_DIR/postgres17.json" \
  "$LEDGERLAB_P3_EVIDENCE_DIR/postgres18.json"
```

The comparison requires identical canonical envelope bytes, original delivery
pairs, anchors and heads at all four durable prefixes. Physical inventories are
compared within each store; PostgreSQL's namespace, scope-lock and held-intention
tables legitimately differ from SQLite's schema. Existing adapter suites prove
write/cancel/process cuts, aliases, races and populated upgrades separately.

This bounded bridge does not execute every history in the numeric oracle against
every store, and does not connect the older `conformance.run_history` factory.
It adds no product API, accepted-plan constructor, authority verifier, dispatch,
new contract encoding or frozen fixture. Synthetic authority does not prove
real-world assent. Broader base chains, host authority/provisioning, public
submission and arbitrary-load availability remain separate gates.
