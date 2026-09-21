# Frozen contracts

Contract family: `ledgerlab-contracts/1`. These files describe intended behavior, not an implemented runtime. Precedence is the five source reports in order, then `docs/design/CANONICAL-RECORDS-V1.md` for its explicit repairs. Schemas use Draft 2020-12 and immutable URNs. Resolve references only from this directory; acceptance must not fetch schemas/evidence over the network.

- `schemas/v1/event`, `link`, `policy`, `terms`, `offer`, `binding`, `assent`, `source-grant`, `context`, `roles`, `payer-delegation`, `invocation` describe inputs/documents.
- `canonical-records` and its action/explanation/snapshot/intention/receipt wrappers freeze the first-slice record family. The full schema appendix and every first-slice fixture remain byte-identical to the approved repair.
- `export.schema.json` transposes the manifest field dictionary from detailed design §20. The record payload is original base64 bytes; Phase 5 owns executable import/export and an exact portable operational record suite. No accept/rerate path is permitted for import.
- `compatibility.json`, `limits.json`, `relations.json` record independently versioned surfaces, rejection thresholds and child→predecessor relation semantics. Empty supported storage arrays mean **unimplemented**, not version-zero storage support.
- `freeze.json` binds schema, metadata, source and fixture bytes. Checks verify it; no test rewrites it. Change it only with a reviewed contract amendment.

## Required validation beyond JSON Schema

Reject duplicate keys, invalid UTF-8/BOM/surrogates, fractional/exponent/negative-zero JSON numbers and unsafe integers before ordinary deserialization. Apply UTF-8 byte bounds, Gregorian microsecond time validation, normalized decimal coefficient/scale bounds, reduced ratios, economic/reference/order checks and current authority under locks. Null and unknown economic fields reject. Internal ID fields require their domain prefixes even where a structural definition also admits an external ID. The tests deliberately exercise rehashed malformed journals so digest equality alone cannot pass.

`binding` structure alone does not authorize a payable. Supplier bindings additionally require accepted offer, exposure, invocation and retained authority. Bearer≠payer needs a valid delegation; outcome windows use the nominated source and half-open occurrence interval with the pinned report grace. A real obligation requires real assent evidence. Shape-only examples are labeled as such and must never seed an accepted supplier decision.

## Newly transposed metadata names

Export source logical/backend versions are named `logical_schema`, `backend`, `backend_schema`; profile names are `canonical_profile`, `hash_profile`; version arrays are `dsl_versions`, `evaluator_versions`. `chain_heads` entries are `{chain_id,revision}`; file entries are `{path,bytes,records,sha256}` with counter strings and a raw SHA-256 hex. No financial rule changes. `root_digest=H("export-manifest", manifest without root_digest)`. The canonical record root uses the sorted `[kind,scope,id,content_hash]` list specified in §20; import verifies original body bytes and all references.

OpenAPI 3.1.1 and HTTP `/v1` are compatibility commitments. The full endpoint specification/generator belongs to Phase 6 and is not represented by a misleading empty OpenAPI file here.

## Fixtures and verification

The complete `journals/first-slice` is the exact Phase 1 oracle: 60 vectors, 25 accepted immutable records, 29 manifest members, +100−20=80. Its authorship helper is not run in tests. `canonical` adds raw negative parser and Unicode examples. Other journal directories contain independent economic postings/totals from the supplied worked examples, not fabricated full canonical ledgers. `authority` contains expected scenarios and clearly named structural examples; `failures` freezes the slice's per-item write schedule.

`sh scripts/check-contracts.sh` validates all schemas/references, source/addendum/fixture drift, values and negative cases, Python/Node byte equivalence and independent arithmetic. These are document tests; only later real-store tests can establish acceptance correctness.
