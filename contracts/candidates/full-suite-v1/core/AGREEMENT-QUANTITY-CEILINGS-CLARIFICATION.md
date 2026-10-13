# AGR quantity ceiling clarification

Astra, 2026-10-13. Additive candidate technical clarification; independent review required. No reviewed schema, registry, fixture or terms hash is rewritten. No new rate, permission, unit conversion, commercial ceiling or assent-time requirement is introduced.

## Source authority and exact decision

- S12-TECHNICAL-PLAN §3.1, lines85–95: each ActivityDef pins one exact FactSchema; each numeric FieldDef pins at most one declared unit, which is required for a monetary quantity. Unit normalization is an explicitly declared exact multiplier to a terminal unit, not an inferred display alias.
- S12 §3.3, lines125–131: quantity entries are `{activity,field,unit,maximum}`; accepted terms retain exact original schema/policy and real/synthetic assent. §10.1 line330 requires corrections to use the original numeric type/unit and forbids a display-unit quantity in another source field.
- `publication-boundary.md` §1 line7 explicitly enumerates name-keyed collections; **other set arrays sort by canonical JCS bytes**. `normative-amendment.md` line35 requires duplicate semantic names/keys to refuse even when contents differ.
- `AGREEMENT-BOUNDARY.md` lines17,33: SlotBindingTerms is a constraint/authority document, policy supplies rates, and quantity ceilings must be representable at the original TypeDef/unit. The existing schema quantity_ceilings arrays are sets with maximum64 entries; schema uniqueItems alone does not implement semantic uniqueness.

For BindingInputs, AssentBindingInputs, accepted Binding and SlotBindingTerms, normalize every quantity-ceiling entry's maximum using the existing nonnegative Decimal lexical/canonical rules. Preserve exact case-sensitive activity/field/unit names. Then reject a repeated **(activity,field)** pair within that array, irrespective of unit or maximum. Identical entries, equivalent Decimal spellings (`4`/`4.00`) and different bounds (`4`/`5`) all refuse. Do not deduplicate, sum, intersect, choose the minimum/maximum, or apply first/last precedence. Unit is an assertion about the selected field, not another independent ceiling identity.

After duplicate validation, sort complete normalized entry objects by canonical JCS bytes. Do not introduce a new tuple sort. For valid unique ASCII activity/field entries this commonly gives the same order, but the frozen canonical rule remains JCS. Other keyed binding/source/funding/exposure collections retain their individually declared existing normalization; this clarification does not normalize unrelated arrays anew.

Semantic acceptance resolves activity in the exact original Product, then field in that activity's exact immutable FactSchema. The field must be the required numeric quantity the ceiling constrains; optional/string/enum/boolean fields cannot be promoted to quantities by this entry. `unit` must equal that original FieldDef.type.unit and name a unit declared by the Product. A dimension-compatible alias or terminal normalization target is not accepted in place of the field's declared unit. Unit conversion used by a reviewed price formula remains separate and unchanged.

`maximum` is first normalized as the dictionary's Decimal, then must be **numerically** representable in the original TypeDef and lie within its inclusive finite min/max. For integer type, normalized Decimal must have no fractional component (`4.00`→`4` is representable, `4.5` is not); this is a ceiling document normalization rule, not permission to supply a decimal token to an integer fact field. For decimal type, its normalized fractional precision cannot exceed the original max_scale. Existing raw Decimal byte/scale/coefficient limits apply before this semantic check. No float conversion, rounding, truncation or automatic clamp. Zero is not given a new global prohibition here: it is allowed only where original type/range permits it, and never authorizes nonpositive completed-work quantity contrary to original work rules.

Different immutable authority/terms documents may independently constrain the same quantity. They retain separate identities/assent and every applicable original/current constraint still applies. This is not a duplicate within one array and does not authorize flattening their documents into a single merged or rerated term. Actual work remains bounded by all applicable ceilings and original source authority.

## Validation boundary and errors

Pure document codec: closed shape, raw Decimal normalization, duplicate (activity,field) rejection and canonical JCS order. It can perform these without claiming authentication or looking up Product/FactSchema. A structurally normalized document is not accepted terms.

Pure resolved semantic validator: receives actual immutable Product/FactSchema operands and checks the exact field/unit/representability/range relationships. Coordinator still proves their retained provenance, actual source authority and assent; neither parser nor LocalOwnerAdmin substitutes for that evidence. Wrong/missing activity/field/unit uses existing INVALID_REFERENCE; duplicate pair/invalid object shape uses INVALID_SHAPE; invalid numeric lexical form/representability/range uses INVALID_SCALAR. These are existing registry categories, not new operation/error spellings.

AssentRecord.accepted_at is normalized as the existing timestamp and must match its exact evidence/terms during semantic acceptance. The codec introduces **no** rule that it must occur on or after a future Binding.starts_at. Agreeing now to future effective terms remains representable. No other authority/time/deadline check is weakened.

## Additive test vectors and handoff

`quantity-ceilings-fixture-input.json` contains exact valid normalized sets and refusal cases, including same-pair/same-unit different bounds, normalized aliases, different units, representability, original units and range. `validate_quantity_ceilings_candidate.py` is an author-side executable reference check without production imports. Its expected results are literal fixture data, not obtained from product output. Independent review is required before any compatibility claim.

Implementation handoff: add these codec checks to the isolated pure AGR terms parser; keep original schema/terms digest framing and old vectors unchanged. Add resolved field/unit/range checks where the immutable Product/FactSchema is actually supplied, without turning parsing into authority validation. Retain all pending actual assent/source/capacity/runtime gates. No financial audit/resource or post-deadline causal choice is resolved by this clarification.
