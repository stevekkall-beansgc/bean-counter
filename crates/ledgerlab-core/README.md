# Pure core

Approved Phase 2 v0 target adjustments are documented in
[outcomes](src/policy/chaining/outcomes/README.md). That contract supersedes the
old linked-discount proposal; the old operator now rejects explicitly.

The frozen Phase 1 acceptance path below remains unchanged. The additive
`policy::chaining` module supplies the bounded Phase 2 typed pricing API. See
[Phase 2 core integration notes](../../PHASE-2-CORE.md) for its semantics,
proposed linked discounts, validation and the required persistence extensions.

## Phase 1 acceptance boundary

This crate implements the frozen completion first slice through a synchronous,
validated policy/evaluation/record boundary. It reads no files, database, network,
environment, clock, entropy, or model service, and forbids first-party unsafe code.

## Coordinator integration

1. `domain::normalize(bytes, scope, default_source)` returns a `Candidate` with
   scoped event identity and original normalized ingress bytes/hash. Omitted
   source and chain remain omitted in ingress; resolved content contains them.
   Resolve identity retries against the retained ingress before current pricing.
2. `Candidate::resolve(parent_chain)` returns an immutable `Event`. Supply a
   common parent chain only after the coordinator validates topology and aliases.
   Unlinked omitted chains are derived deterministically.
3. `Document::parse(body_bytes)` strictly parses and normalizes the six retained
   input document types. IDs/hashes are recomputed, never trusted from an envelope.
4. `ResolvedInput::new(event, documents, AcceptanceContext)` checks the supported
   input family, document references, source/principal/grant, binding scope,
   quantity/unit/currency, demo assent and roles. Context provides locked authority
   observations, revision/count, and an injected timestamp. The first slice accepts
   sandbox demo standing retail terms; real assent, supplier terms, stage inputs,
   invocation/correction/evidence decisions, and linked evaluation are unsupported.
5. `policy::evaluate(&input)` (also exposed as `domain::assemble`) produces a
   `DecisionPlan`: private immutable typed outputs, new journal records, receipt,
   claim/effect-facts projections, and canonical journal bytes. The compiler uses
   typed fixed bases, predicates, and earlier named booked bases for additive
   percentage discounts. It rejects unsupported operators; no SQL-specific path
   or fixture bytes are embedded in production code. Failed work emits one
   `FAILED_WORK` explanation with no actions or intentions. Zero rounded rules
   emit explanations without zero actions/effects.
6. The coordinator owns complete resolution, scope authorization under ordered
   locks, stable grant/binding heads, receipt-read rights, semantic duplicate
   lookup, chain head consistency, transaction lifetimes, and atomic insertion.
   Construct the context from actual locked state; its flags are observations,
   not an authorization token. Supply already verified retained evidence where
   applicable; this bounded demo slice does not establish real assent.
7. Records are in canonical journal order, not SQL insert order. The six reused
   input documents are manifest members but not new records. Read action and
   intention data from the plan; never calculate prices in adapters. The original
   input bundle must be retained for `domain::verify`, which regenerates the plan
   and compares the entire canonical journal. Verification has no side effects
   and does not reauthorize submission, append, or dispatch.

`AcceptanceContext` contains `principal_id`, `grant_revision`, `grant_active`,
`binding_active`, `chain_revision`, `chain_event_count`, and `received_at`.
The timestamp checks the injected current rights interval; no rule consumes it,
so the frozen snapshot retains an empty `decision_context`. Historical replay
must supply the original authority observations, not current mutable heads.

The event normalizer validates all compact DTO variants structurally, including
byte limits, defaults, optional omission, set ordering, timestamps, and local
relation cardinality. Only completion claim facts and the first-slice record
family are encoded here. Cross-event topology/authority is coordinator work;
acquisition/link/reversal facts and further record variants need their reviewed
contract extensions before the acceptance assembler can persist them.

## Resolved dependencies

All direct dependencies were actually resolved/downloaded from crates.io, are
used, and are MIT OR Apache-2.0. Exact direct pins and transitive checksums are
recorded in the manifest and workspace lockfile.

| Dependency | Version | Use |
| --- | --- | --- |
| serde | 1.0.229 | Typed wire and record serialization; derive enabled |
| serde_json | 1.0.151 | String escaping/unescaping and typed DTO materialization |
| sha2 | 0.11.0 | Domain-separated SHA-256; default features disabled |
| num-bigint | 0.5.1 | Exact rationals; every component/intermediate bounded at 512 bits |
| num-integer | 0.1.47 | GCD cancellation and quotient/remainder |
| num-traits | 0.2.19 | Checked integer conversion/sign/zero/one |

The strict parser and JCS serializer implement the integer-only
`ledger-canonical-v1` profile locally. They reject fractional/exponent tokens,
negative numeric zero, unsafe JSON integers, duplicate keys, BOM, invalid UTF-8,
and lone surrogates before DTO construction. Object keys compare UTF-16 units;
there is no Unicode normalization. This does not expose general floating-point
RFC 8785 input support. Economic decimals and atoms remain strings.

The resolved default/all/no-default dependency graphs pass the workspace purity
check. No runtime/database/network/entropy dependency is reachable from core.
Rust 1.98.1 was used as the verified development compiler; no MSRV is claimed.

## Validation evidence

The package has six arithmetic unit tests and twenty integration conformance
cases. They reconstruct all 60 framed hash vectors, all six input envelopes,
the entire 25-row journal and 29-member manifest, separate facts/explanation/
receipt bytes, and exact +100/-20/80 atoms from typed inputs. Negative coverage
includes lexical/recursive shape failures, normalization equivalence, Unicode,
byte/depth/precision limits, timestamp offsets/calendar boundaries, revision
exhaustion, 512-bit cancellation/overflow, atom/total overflow, unknown operators,
missing bases, combined discounts exceeding basis, zero/failed outputs, and
rehashed malformed journal rejection. Rule rename and alternate price cases
verify stable economic identity and evaluation from actual typed inputs.

The full `scripts/check.sh` passed, including fmt, workspace tests, warnings-denied
clippy, no-default-feature compilation, resolved graph/source boundary negatives,
and independent Python/Node frozen-contract checks. Run with the verified local
activation script and, on this machine, these environment overrides:

```sh
RUSTUP_TOOLCHAIN=stable \
PYTHONPATH=/Users/stephenkall/Documents/Codex/2026-09-20/ledger-lab-v0-detailed-design/work/check-deps \
sh scripts/check.sh
```

The installed `stable` toolchain reports exactly `rustc 1.98.1`; the version-named
alias is absent. These checks establish pure-core behavior and contract fidelity,
not persistence, cancellation, TLS, platform certification, or MSRV evidence.
