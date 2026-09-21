# Proposed Phase 2 economics

These hand-authored values are review inputs, not additions to the Phase 0
freeze. The focused Rust tests consume them. The production evaluator is not
used to derive their expected values. Existing first-slice, 335/315, BYOK and
reversal goldens remain unchanged in the repository's frozen `fixtures/` tree.

The linked-discount proposal is deliberately explicit: an approved acquisition
links to an existing publication; accepted retail terms name that publication's
booked component. Each approved claim appends -10 atoms against the original
50-atom basis. Five claims exhaust it. A sixth fails, never silently floors.
The original 50-atom posting stays intact. Reversing one -10 effect appends +10
and restores exactly that discount capacity. This is not arbitrary rerating.

The direct publication path follows the frozen acquired -> published relation.
The generation is retained through publication's explicit lineage. The other
allowed match is acquired -> published -> optimized. No acquisition -> generation
relation or general graph matcher has been introduced. Combining this proposed
prior-component discount with a closure cap is rejected pending a reviewed basis
contract. It is not enabled in `ledger-policy/1` parsing.

Independent calculations, all in integer atoms:

- 50 × 20 / 100 = 10; 5 × 10 = 50; -(-10) = +10.
- 0.10 × 1 × 100 = 10 retail; 0.03 × 1 × 100 = 3 supplier;
  100 held = 3 consumed + 97 released.
- 200 - 20 = 180; 180 × 25 / 100 = 45;
  45 + 135 = 180. Allocation adds no payable.

Canonical acquisition/reversal facts, authority snapshots, stage and invocation
transitions, allocation encoding and complete manifests still need reviewed
extensions and independent byte verification before Phase 3 persistence.
