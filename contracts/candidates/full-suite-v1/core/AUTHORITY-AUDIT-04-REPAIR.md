# AUTH-AUDIT-04 — exact saved origin dispatch repair

Astra, 2026-10-13. Bounded candidate repair only. AUTH03's nine literal observations matched independent derivation but were diagnostic: its synthetic descriptor advertised `ledger-workflow-authority-change/1`, which matched neither actual customer registration nor source enrollment payload. Preserve all03 files and independent diagnostic bytes unchanged.

The new literal04 input changes only those two recipe values: `authority.change/customer.register` uses `ledger-workflow-customer-register/1`; `authority.change/source.enroll` uses `ledger-workflow-source-enroll/1`. Its raw artifact inventory adds this repair document. The source evidence recipe, action variants, source roots, actual selected original records, pointers, document/wrapper arrays, counts, accepted times and source-observation hash domains are unchanged. Descriptor identity and dependent observation/package bytes change explicitly.

For every selected original operation, the author gate derives this exact tuple from saved bytes:

`(descriptor.registry_digest, operation.kind, normalized_request.payload.schema, request_evidence.normalization_profile, original_manifest.schema, variant)`.

`variant` is the actual `payload.action` for authority.change and literal `all` for this non-discriminated evidence.retain schema. Require exact equality with one code-owned literal OriginRecipe. Check operation kind against saved normalized request and DescriptorResolution; normalized payload schema against DescriptorResolution.payload_schema; saved request evidence/normalization against its descriptor; and the saved request/descriptor copies against the top-level fixture records. The original registry must declare the actual operation/schema pair. No union-schema substitution, action-based schema rewrite or current-registry fallback is allowed. An exact support tuple is a prerequisite for the original semantic verifier, not proof that its semantics were verified.

The04 assembler checks all eight distinct selected origins before deriving observations. The read-only validator repeats exact dispatch coverage and exercises wrong-schema/right-action, right-schema/wrong-action, descriptor/request disagreement, wrong saved registry, wrong normalization and wrong closure-profile cases for all four A/B customer/enrollment origins. It also confirms the old03 literal recipes cannot cover those four origins. These checks supplement closed-shape validation; shape success alone did not establish dispatcher support.

The new inputs/output are `authority-audit-04-fixture-input.json` and `authority-audit-04-author.json`, using unchanged `authority-audit-03.schema.json`. Commands are `python3 .../derive_authority_audit_04_author.py` and the isolated jsonschema4.26.0 Python environment running `validate_authority_audit_04_candidate.py`. All02/03 artifacts remain historical; this file changes no financial seed/commitment/union semantics or synthetic financial graph.

Independent review is required before any named freeze. Complete actual financial origins/receipts, capacity and resource sufficiency, original verifier runtime, custody/reopen/concurrency and the pending owner causal-window choice remain gates. No source descriptor is a production capability or source of customer assent.
