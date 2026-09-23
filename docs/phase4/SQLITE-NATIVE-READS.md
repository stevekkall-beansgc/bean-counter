# SQLite paged-reader native query allowance

This is an engine-specific traversal worksheet for the SELECT programs in
`store/sqlite/adjudication/reader.rs` and their shared helpers. It does not claim
host RAM reservation, total SQLite allocation bounds, schema-loading or WAL-index
I/O bounds, or protected completion. Those remain separate physical gates.

Premises are the pinned SQLite3.51.3 source, 4096-byte pages, zero header reserved
bytes, no auto-vacuum, memory mapping disabled and forced exact/range indexes.
Migration6 validates existing object kinds and installs immutable-row insert guards
for the closed16 FactKind values, each at most18 ASCII bytes. Full persisted index
keys, including record headers, rowid/page suffixes and up to256 UTF-8 bytes in a
SQL length64 digest, are below8192 bytes. No digest-only identity substitution occurs.

The cached engine bounds B-tree cursor depth at20 and cells per4096-byte page at681.
Binary search takes at most10 comparisons per level. Budget256 index-record/page
operations for200 comparisons, descent/range movement and additional key operations.
An8192-byte key may require ceil(8192/4092)=3 overflow pages, so the index allowance
is256×(1+3)=1024 page visits. Allow40 more for rowid-table traversal. For up to8 table
columns and a maximum270336-byte accessed record prefix, charge
1024+40+9×(1+ceil(270336/4092))=1676; round each SELECT up to2048 visits.

Direct BLOB length() uses the OP_Column length flag and reads its length from the
record header, not an oversized body. This allows rejecting schema-valid8MiB
head/result values before materializing them. The body query follows a same-snapshot
262144-byte limit. Command precedes result in the command row. Gateway saved-result
materialization first proves command+result+receipt<=524288; its two-table join is
charged4096. The unrestricted saved helper outside this budgeted gateway path is
not covered by that smaller allowance. Page substr() materializes at most4096 bytes,
plus its bounded preceding keys; it is not described as incremental blob I/O.

| Operation | Charged SELECT visits |
|---|---:|
| Segment hash |2048|
| State or bounded current point |4096 maximum|
| Segment/object fragment |6144|
| Gateway with saved outcome |12288|
| Complete certificate path |14336|
| Authority metadata plus four object fragments |26624|
| Current/historical initialization |12288/14336|

Every allowance is subtracted before its SQL executes. Initialization charges8192
plus2048 for a historical segment, while enrollment state charges its own4096.
Limits keep native visits separate from segment count: up to2^24 visits and4096
segments per bounded reader request. This permits progress on a complete maximum
frozen3411620-byte segment while preserving per-call byte limits and continuation.

The native-plan test extracts actual SQL literals from the production modules and
records full EXPLAIN programs. It requires indexed SEARCH plans, explicit seek
opcodes, no table rewind, sorting or automatic index, metadata-only length flags,
and no runtime JSON filtering for the first-closure partial index. Source audit
locations in cached sqlite3.c:71643,71911,77548–77738,78451–78680,92020–92120,
96026–96100,98282–98565. Installation text sizes use metadata-only octet_length()
(116828–116845 and135088), avoiding preflight materialization through CAST.

This worksheet and exact query programs require independent review; passing tests
alone do not prove worst-case filesystem or allocator bounds.
