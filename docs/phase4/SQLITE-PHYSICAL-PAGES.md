# SQLite retained and transient database-page worksheet

This worksheet addresses native database pages for the typed `persist.rs` projection.
It does not establish actual host reservation or an allocator/RSS bound. WAL has a
separate worksheet. All claims require independent review on the assembled source.

Premises: pinned SQLite3.51.3,4096-byte pages, zero reserved bytes, auto_vacuum=0,
fixed schema/index set, typed bounded plan writes, no IndexChange, and R3 genesis
before the initial cumulative allocation. Provisioning now explicitly refuses any
preexisting R3 journal when the allocation profile is absent; occupied baseline pages
alone cannot substitute for old prepaid history. Original-profile rows are charged to
a separate finite legacy allowance. Existing profile reopen verifies its exact stored
identity, limits and incarnation; it never resets historical allocation.

Let S be a template's segment-byte bound and N=ceil(S/4096). Let O be its object
bound(min(records−1,216)), H its head-write bound(min(index_cardinality,256)), G its
namespace bound(4 only for ENROLL), and A its held-intention bound(0 for enrollment,
128 otherwise). Each decoded object body occurs base64-encoded in the same bounded
segment, so aggregate decoded bytes≤S and object-page rows P≤N+O. Delivery rows D≤H
and partial first-closure index entries F≤H.

| Projection | Table records T | Index records I |
|---|---:|---:|
| Journal, segment, command |3|5|
| Segment pages |N|N|
| Objects |O|3O|
| Object pages |P|P|
| Current heads and versions |2H|2H+F|
| Deliveries |D|2D|
| Namespaces |G|G|
| Held intentions |A|A|

Updates count their current-head row again, conservatively. Every valid non-root
B-tree page is nonempty. Table leaves≤T; internal fanout≥2 bounds table-tree pages
by2T plus fixed roots. Index cells hold the index records in internal or leaf pages,
so index pages≤I plus roots. For record payload B, overflow pages≤T+I+ceil(B/4092).
Thus occupied pages≤3T+2I+ceil(B/4092)+fixed roots. Substituting the bounds above:

3T+2I≤19+10N+14O+19H+5G+5A.

The current R=3+2N+3O+3H+G+A and9R=27+18N+27O+27H+9G+9A dominate this term. This is
cumulative occupancy accounting, not a claim that an individual insertion splits
at most nine pages. A depth-based gross allocation estimate is not the retained
credit requirement; already paid immutable history provides the occupancy credits.
Fixed roots/genesis are covered by actual baseline pages plus the256-page fixed margin.

Payload bounds use journal≤1115, origin≤2048, object key≤4096, head key≤1115, closed
FactKind≤20ASCII bytes, typed ASCII digest64,128header bytes per record and8bytes per
integer/rowid. Summing table and all index payloads:

| Component | Payload bound |
|---|---:|
| Journal/segment/command overhead |30880|
| Segment-page key/header |2542N|
| Objects and three indexes |29884O|
| Object-page key/header |14966P|
| Current head/version/partial index |536222H|
| Delivery and both indexes |19583D|
| Namespace and index |2083G|
| Held intention and index |10738A|
| Segment, decoded objects, command/result |3S|

After substitution, B≤3S+17508N+44850O+555805H+30880+2083G+10738A.
The existing bytes+16384R expression expands to
3S+43008N+73728O+602112H+73728+18432G+26624A, dominating every coefficient.
The padding therefore covers object-page multiplicity and all secondary indexes.
The regression checks this independently expanded inequality for every frozen template.

Transient DB-page allocation is separate. A single persist statement affects at most
four B-trees: object table plus primary, fact and authority indexes. A9MiB maximum
record overbounds even the schema's8MiB result plus command, keys and receipt. For
each affected tree, allow both old/new maximum overflow chains, three old plus five
replacement balancing pages across20levels plus a root iteration, and two extra
root pages:2×ceil(9MiB/4092)+8×21+2=4784. Four trees need≤19136pages, below the
24576-page transient allowance. This intentionally overcounts both existing pages
and final retained pages. Balancing reuses old pages and moves existing overflow
references; it does not copy every neighboring overflow chain. Free-list reuse does
not consume another allocation: the admitted ceiling covers cumulative live credits
plus the largest concurrently transient operation; any earlier larger high-water
remains within that same nondecreasing ceiling.

Cached sqlite3.c references: page types74478–74542, nonempty child validation77875–77887,
overflow allocation/freeing79390–79585, balance arrays80678, nonempty redistribution
81020–81067, page reuse81075, root growth81458, parent traversal81542, new-before-old
replacement82013–82034. The transient argument concerns these fixed R3 statements;
arbitrary legacy statements and extra triggers are not silently included. Legacy
original-base growth is separately charged before commit, and optional legacy work
cannot spend beyond its allowance. Failed optional enrollment may be refused before
it creates any central ingress promise; successful enrollment retains its original
base atomically with R3 history.

Remaining physical requirements include actual reserved DB/WAL/temp/workspace backing,
heap/subjournal/SQLx/Rust bounds, all-consumer host enforcement, real saturation and
independent validation of this worksheet's native source assumptions.
