# Candidate.4 scalar audit

Status: candidate, not frozen. Semantic source: `1e0ba3f`. This is canonical
storage after approved normalization; it does not redefine ingress normalization.
Every `$defs` string with a length constraint also has `x-utf8-maxBytes`. Every
record and embedded typed value passes the same extended schema validator in
Python and the independent Node implementation.

| Scalar type | Canonical constraint | Approved source |
|---|---|---|
| `text`, `retained-identity` | 1–128 UTF-8 bytes; no Unicode controls; no normalization | domain `text` |
| `source` | 1–256 UTF-8 bytes; absolute ASCII scheme; no whitespace or controls | `validate_source` |
| `slug` | ASCII `[a-z][a-z0-9_.-]{0,63}` | domain `slug` |
| Internal IDs/hashes | Fixed prefix and 64 lowercase hex; all native IDs remain unchanged; original doc/event IDs retain v1 formulas | domain `prefixed`, canonical domains |
| `decimal` | Nonnegative canonical decimal; coefficient <=30 digits; fractional scale <=18; no sign/exponent/redundant zero | `Decimal::parse` + canonical display |
| `positive-decimal` | Decimal strictly greater than zero | successful Event quantity; compiled Binding maximum quantity |
| `decimal-percent` | Decimal in [0,100] | base pricing `Decimal::percent` |
| `atoms` | Canonical signed integer string; magnitude <=10^30−1 | `parse_atoms`, Money |
| `nonnegative-atoms` | Same bound, no minus sign | capacities and gross totals |
| `money` | Three ASCII uppercase currency letters; integer scale 0–18; bounded signed atoms | Money |
| `nonnegative-money` | Money atoms >=0 | binding/invocation exposure, held, ceilings and capacities |
| `ratio` | Canonical signed numerator; positive denominator; reduced; each <=512 bits; zero=0/1 | `ExactRatio::from_canonical` |
| `uint` | Canonical unsigned string, <=i64::MAX | Revision |
| `time` | Gregorian year 0001–9999, UTC microseconds, no leap seconds | Timestamp canonical representation |
| Outcome terms durations | Integer window_us 1..90 days; report_grace_us 0..7 days | compiled `Binding.outcome` |
| JSON integral values | Safe signed integer range; no fraction/exponent/negative-zero token | strict canonical parser |
| Fixed flags/discriminators | Explicit schema boolean/enum/const; no coercion or null | typed variants |
| Extension values | String, boolean or safe integer; <=16 entries and <=4096 canonical bytes; opaque to policy | Event normalization |
| Embedded canonical strings | Strict JCS, <=256 KiB, typed schema recursively validated | candidate retained codec |
| Evidence document content | Strict canonical JSON object, <=256 KiB; inert bytes bound to original document hash | retained evidence boundary |

All text usages inherit the byte constraint, including policy_version, binding_id,
agreement_id, operation_id, invocation id, chain_id, principal, customer and every
role. Original policy version, original Binding id/agreement and all context,
stage, rule, action and invocation identifiers are validated inside source bytes.
No check relies on an incomplete list of property names. Strings inside opaque
evidence or permitted extensions are subject to their payload bounds, not field-
name-based economic checks.

Binding maximum_quantity and successful event quantity are positive. Invocation
maximum_quantity is nonnegative structurally; nominated work must have quantity
<= that ceiling <= the frozen Binding ceiling. Exposures, held, booked-net,
consumption/release, discount-capacity and premium values are nonnegative. Signed
outcome results, inverses and signed percentage numerators are still permitted.
Outcome percentages have no base-price 0–100 restriction. Exact multiplication
and division cross-cancel before checking the approved temporary/result bounds.

Whole-string canonical spelling is mandatory. Python `re.fullmatch` and every
schema pattern's absolute-end assertion reject the terminal-newline exception
of a plain `$` anchor. Node uses its own absolute-end grammar. Integer spelling
is validated before `int`, `BigInt`, fraction or ratio conversion. The rule
applies to atoms, ratio numerator/denominator, counters, IDs/hashes, slugs,
decimals and timestamps, recursively inside retained source bytes. Opaque text
is not reclassified as a numeric field by its name.

The audit covers 216 shared Python/Node/approved-Rust scalar cases, including
terminal LF, CR, CRLF, tab, space, C1 and Unicode line separators; eight original
or candidate ID/hash kinds; all ratio components; positive/nonnegative and
precision limits. It adds 38 byte boundaries across record text fields, stored
Decimal normalization cases, schema coverage and two structural source round
trips. There are 207 scalar/field/normalization negative checks.

Five complete noncanonical scalar histories exercise coherently rewritten fixed
atoms and policy-document terms, numerator and denominator terms, explanation
ratios, and native/projected original action atoms. Every affected identity,
digest, reference, manifest and receipt is rebuilt first. Python and Node prove
hash-only integrity (205 records/five decisions); Python, Node and approved Rust
then reject the scalar spellings. These are separate from the 49 semantic attacks
that pass both schema and complete hash integrity before semantic rejection.

All 23 accepted bases also undergo exact complete typed Evaluation roundtrips
against the approved core. That proof retains actual IDs, all fields and ordered
vectors; it does not reduce source equality to monetary values or record counts.
