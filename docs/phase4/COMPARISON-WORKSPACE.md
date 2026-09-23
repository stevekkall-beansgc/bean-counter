# SQLite comparison workspace accounting

This implementation reserves one optional comparison session per live SQLite host,
separately from the 16 MiB SQL reader allowance and mandatory completion lane. Its
permit lasts through session Drop, including idle partial buffers and canceled
advance calls. No SQL snapshot or business gate lasts between calls. Host backing
projection includes the separate allocation before accepting an R3 profile.

The following is a conservative application allocation envelope, not a measured
RSS ceiling or a claim of physically verified host backing. Full physical-store
acceptance remains necessary. It intentionally overestimates generic JSON trees;
shape-aware accounting could reduce it without weakening the bound.

## Inputs and pinned layout

Rust 1.98.1, cached serde_json, 64-bit target: Value is32 bytes; String, Vec and
BTreeMap are24 bytes; monomorphized BTree leaf/internal allocations are632/728
bytes. The latter were inspected in LLVM output of the pinned compiler and must
be re-audited on toolchain/layout changes. Public type-size assertions only check
public layouts. The canonical parser limits depth to32.

For a canonical input of x bytes, use D(x)=256*x+65536. Object nodes are bounded
by member count (at least five canonical bytes per nonempty member); array
allocations, including resize overlap, are conservatively bounded by128 bytes
per child. The remaining allowance covers strings, sort pointers and bounded
recursive temporaries. This bounds live application allocations under the pinned
container layouts, not malloc fragmentation or OS residency.

Comparison enforces these input limits before the respective large allocations:

- S=3411620: maximum segment in the frozen worksheet, checked before accumulation.
- O=262144: maximum original base object and point value.
- B=1048576: total decoded OriginalBase bytes; at most128 objects.
- At most128 unique canonical member identities before legacy decode_base clones
  the seed. Records rejects duplicate identities, preventing repeated-reference
  multiplication.
- U=83*21979 encoded authority bytes; A=524288 decoded authority bytes.
- E=O for enrollment; H=34*O for economic observations. This deliberately uses
  the generic point cap rather than the tighter variant-specific state bound.

## Malformed original replay

Bounds must hold before a final byte mismatch rejects malformed retained output.
Original compile.rs admits at most64 rules, one share rule and16 policies.
Evaluation produces at most66 actions,64 explanations,66 temporary delta groups
(before its final32-group check), and16 consumptions. Each action can copy an
input Binding and each delta can copy an input Roles subtree. Counting eight
further input equivalents plus32768 bytes per generated record for bounded
fields, references and structural slack gives:

R <= 140*O + (66+66+64+16)*32768 = 43646976 < 192*O.

Use R=192*O=50331648. Empty original history prevents expansion through historical
dependencies. Relevant source paths are ledgerlab-core/src/policy/chaining/compile.rs,
evaluate.rs and retained.rs, and ledgerlab/src/service/retained/base.rs.

## Simultaneous phases

Let C=5*S+8*O+4*D(E)+3*D(O)+2*D(U)+D(A)=1557809972.

| Phase | Envelope | Bytes |
| --- | --- | ---: |
| Segment parse | 7*S+2*D(S)+4*D(E) | 2039459452 |
| Base after matching replay | C+D(S)+2*B+4*D(B)+32*D(O) | 5656932148 |
| Base replay before byte comparison | C+D(S)+2*B+4*D(B)+12*D(O)+2*D(R)+4*R | 30284705588 |
| Economic comparison | C+D(S)+D(H)+8*D(O) | 5250412340 |

5*S covers retained/working partial buffers and growth overlap. 4*D(E) covers
stored/working progress, candidate enrollment and evaluator copy. 3*D(O) covers
command DOM, parsed command and candidate. 4*D(B) covers Records, cloned seed,
membership representations and identity bookkeeping. During replay12*D(O)
covers outer records, decoded inputs, normalization, compile maps and temporary
canonical copies; 2*D(R)+4*R covers generated replay, its serialization tree and
encoding buffers. After matching replay32*D(O) conservatively covers projection,
policy/proof mappings, Target's replay copy and family validation.

Round the maximum upward to a MiB:30284972032 bytes, or28882 MiB. The physical
projection derives this constant arithmetically. It is separate from retained
bytes and SQL reader work, and cannot be borrowed by mandatory obligations.

## Request budgets and limitations

Preparing a comparison explicitly takes a preparation budget and exposes its
measured charge. Each advance reports only that call's charged work. Offered
budgets are clamped to a bounded physical slice. A call unable to pay its known
snapshot initialization performs no I/O and returns INCOMPLETE with no progress.
A funded one-byte fragment preserves ordinal/byte offset. All old dependency
reads consume the same call's allowance; exhaustion preserves raw input without
committing a partially verified economic fold. Queue contention remains distinct
from work-budget exhaustion. Completion counts are charged at final fragments.

Bytes are conservative fetch charges; pages currently count bounded logical
fetches, including metadata, rather than claiming independently measured native
BTree traversal. The full engine page/temporary/allocator/backing acceptance gate
remains open. The accounting above must not be cited as having closed that gate.
