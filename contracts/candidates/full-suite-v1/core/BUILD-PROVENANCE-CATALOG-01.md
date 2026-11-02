# PUB-BUILD-CATALOG-01 — bounded integrated build selection

Additive build-only contract, 2026-10-31, implementing the owner's explicit
approval of a finite build-owned catalog. This supplements PUB-BUILD-01 for
opt-in integrated builds; it does not replace any existing descriptor, plan,
BuildInputs/1, BuildManifest/1, publication, original receipt or hash recipe.
The prior single-profile build path and its exact compiler equality remain.

## Closed capture

`--local-execution --operator-resources --integrated-local-profiles` captures
exactly the three existing profiles required by the approved integrated
Stage/Component/A-B packet. It is mutually exclusive with the existing
single-profile flags. There is no runtime configuration switch, caller-provided
entry, wildcard, arbitrary profile list or fallback to another compiler.

The embedded build artifact is the closed dictionary
`{schema:"ledger-workflow-build-input-catalog/1",entries:[BuildInputs/1,...]}`.
`build-provenance-catalog.schema.json` freezes its shape. It contains exactly
three complete entries, in the order below (also canonical fullRef byte order):

| Existing profile | Compiler descriptor digest hex |
| --- | --- |
| local-policy-semantics/2 (original-event context) | 1aa64de36f29072d1b850cfb52d30e94623bf1a66048f096383452d832b47613 |
| local-stage-lifecycle/2 | 5c581b238a4387c7de3afdcbeccc6d7c19c1ec63227a8c58a4dd665dad9558bc |
| local-component-lifecycle/1 | f38dc832f70070165a5bbba7502a9599c00ecc483ddd26b9063b9e045f4e8e6b |

Every reference has kind `workflow-compiler-descriptor`, ID `wdoc_` plus that
hex, and digest `sha256:` plus that hex. All three complete `source` objects
and every recipe field except `compiler_ref` must be equal. The entries
describe one actual build with three already implemented compiler anchors,
not three separately executed builds. Duplicate, missing, extra, reordered,
unknown or malformed entries refuse, even if the requested entry is valid.

The entire embedded catalog obeys the existing 65,536-byte build-metadata
bound; every entry still obeys the unchanged BuildInputs/1 contract. Inventory
limits, executable observation, financial limits and resource settings remain
unchanged. The capture tool derives each reference from its immutable captured
descriptor bytes and verifies the captured limits/mode/manifest identities.
This amendment and its schema are selected build inputs. The catalog is not
added to any existing compiler's defining manifest or descriptor identity.

## Selection and retained provenance

Fresh publication first acquires and authenticates the exact binary-owned
compiler support using the existing factory. Only its resulting full compiler
reference selects the matching complete embedded BuildInputs/1 entry. Runtime
does not replace an embedded field with a request value or manufacture a recipe.
All entries and common build fields are validated before selection succeeds.
The selected entry's compiler reference must still equal the actual support.
Catalog membership alone grants no executable capability or authority.

The private coordinator observes the current executable using the unchanged
same-handle observation, then retains exactly one unchanged BuildManifest/1
containing the selected BuildInputs/1. The catalog is not a retained document,
new verification member, request field or semantic input. Each profile may
therefore produce its own manifest identity under the same executable hash;
source inventory and all noncompiler build fields remain identical. No self
hash cycle is introduced. Existing canonical document framing, manifest raw
digest, publication projection and accounting rules apply without change.

Original retry/status and semantic alias keep the original retained manifest;
they never substitute the current catalog or executable. Original identities,
historical bytes, old plan recipes and old readers remain unchanged. A fresh
publication under a newly catalog-enabled anchor is new behavior, not proof
that an older single-profile executable supported that publication.

## Qualification boundary

Focused capture/selection tests establish metadata rules and old-format byte
preservation only. They do not prove financial execution, authority, original
recovery, same-binary integrated journeys, holdout, native packaging or release.
The existing single-profile native package recipe remains restricted to its
previous qualified profile. No new packaging/publication claim is authorized.
