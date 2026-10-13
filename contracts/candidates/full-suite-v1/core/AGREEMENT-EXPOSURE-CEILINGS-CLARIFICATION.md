# Exposure ceiling identity and normalization

Astra, 2026-10-13. Additive candidate clarification, pending independent review. Historical schemas, registry, bodies and freeze hashes remain unchanged. This selects no new price, exposure allowance or permission.

S12-TECHNICAL-PLAN §3.3 defines each exposure ceiling as `{binding_slot:Name,maximum:Amount}` and preserves original supplier exposure/funding guards. `normative-amendment.md` §5 requires rejection of duplicate semantic names/keys even where objects differ. `publication-boundary.md` §1 enumerates the primary-key sorting exceptions; exposure_ceilings is not among them, so its normalized objects retain canonical JCS set order. Schema uniqueItems is only structural validation, not a replacement for semantic key uniqueness.

**Within one exposure_ceilings array the semantic key is binding_slot.** Reject a repeated binding_slot even if the maxima differ, including identical repeats. There is one asserted maximum for that slot in the document, not an unordered list of competing ceilings. Do not sum, merge, select a minimum/maximum, or infer first/last precedence. Amount validation remains the existing closed NonnegativeAmount contract (USD, scale18, nonnegative canonical bounded atoms); this clarification does not introduce Decimal-to-Amount coercion or change rounding.

After validating amounts and uniqueness, sort the complete entry objects by canonical JCS bytes. An empty array remains allowed by the existing schema. Distinct slots may have the same maximum. Different accepted documents may separately constrain the same slot and every applicable constraint still applies; do not flatten their authority/assent into this one-array uniqueness rule. Semantic acceptance must resolve each binding_slot against the exact original policy/binding slot mapping; parsing alone proves no role, source authority, exposure availability or customer assent.

Duplicate slots/object shape use existing INVALID_SHAPE. Malformed canonical amount uses existing INVALID_SCALAR; unresolved slot uses existing INVALID_REFERENCE in the resolved validator. Preserve the schema's existing maximum16 entries. No assent-before-binding rule is added. SourceGrant.exposure_ceiling and supplier offer exposure_ceiling are singular fields and unchanged.

`validate_exposure_ceilings_candidate.py` contains eight literal author vectors, independent of product code. It checks normalization/uniqueness and canonical amount boundaries only. Adoption requires independent contract review; no runtime acceptance or economic proof is claimed.
