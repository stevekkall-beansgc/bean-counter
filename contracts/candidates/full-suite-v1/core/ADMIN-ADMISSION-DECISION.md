# Administrative publication admission — owner decision required

2026-10-12, Astra contract lane. **Neither option is approved or active.** Root requested a concrete choice for Steve; root cannot approve a scope change. Pure compiler, typed validation, immutable encoding, read/status and mechanical storage tests can proceed. Public fresh publication/alias mutation and workflow enable must wait for this choice and its all-writer implementation/review. No capacity/performance qualification is claimed.

## Observed state

Published v0.9.7 M3 verifies all three generations of billing entries/aliases: at most100000 entries,100000 aliases,268435456 entry payload bytes and67108864 alias ingress bytes. M5 separately admits at most100000 units (retained records+commands+activity identities) and268435456 canonical+identity bytes. Thus the implementation already has separate ledgers; “one installation100000 decisions” is not an implemented aggregate of these unlike counters. M5 units cannot safely be renamed decisions. P2 draft added a third independent workflow100000operation/268435456byte pool; that draft must not ship silently as an installation limit.

Use E=M3 entry count, L=M5 record+command+identity count, W=workflow operation count. EB and MB are their existing verified payload meters, WB the exact unique-doc+operation/key/manifest/receipt payload meter in `publication-runtime-seams.md` §3. Aliases and their64MiB ingress retain the original independent bound. Every new-key workflow publication or semantic alias costs one W unit and actual retained bytes; original-key retry costs zero. Source/plan docs already present cost no new document bytes; different content under the same identity conflicts. Oldsubgates and per-operation limits always apply.

## Choice A — one conservative shared pool

Require E+L+W≤100000 and EB+MB+WB≤268435456. This is the simplest one-installation accounting rule, but is stricter than currently implemented separate pools. For example valid legacy E60000,L50000 can retain/read/retry its history but cannot enable workflow or accept new writes under this profile. No reset, deletion or corruption verdict is allowed. Legacy-only operation remains available under existing rules until an explicitly chosen migration/profile change. Once opted in, all old and new writers obey the shared rule under one lock. Capacity units are conservative storage units, not promised100000economic decisions.

User impact: some previously valid legacy combinations lose new-write capacity on opting into the new profile; even one administrative publication consumes one unit from that same pool. Choose this only with explicit acceptance of the stricter profile.

## Choice B — preserve legacy admission; workflow charges both pools

Retain existing two pools and require E+W≤100000 AND L+W≤100000; EB+WB≤268435456 AND MB+WB≤268435456. This preserves every existing legacy-only admission when W=WB=0 and avoids an independent third allowance. New workflow operations consume the remaining allowance of **both** pools. Example E60000,L50000 permits at most40000 additional workflow units, subject to bytes and other guards. E100000,L0 has no fresh workflow capacity; status/read/retries remain available. If either pool is already over its original ceiling, existing integrity/profile handling applies; the other pool cannot lend capacity.

This can be implemented with two verified remaining-budget comparisons and all-writer hooks: after workflow enabled, M3 checks E+W and EB+WB; M5 checks L+W and MB+WB; workflow checks both. All checks use the same writer transaction and exact row-derived meters. It preserves the existing separate-pool meaning; it **does not claim** a single100000unit/256MiB installation cap. Algebraically total E+L+W≤200000−W and payload EB+MB+WB≤536870912−WB, before separate alias ingress and nonpayload overhead. These maxima already come from legacy's separate pools; no new workflow third budget. Correct public wording must disclose this rather than repeat the current ambiguous aggregate claim.

User impact: no reduced legacy-only admission; each accepted workflow operation progressively reduces headroom in each legacy pool. This is the recommended compatibility option if preserving existing admitted combinations matters more than imposing a new unified lower ceiling. No automatic selection is authorized.

## Shared implementation and evidence

Both choices require exact unique-doc deltas, atomic verified meters, original retry-before-capacity, unknown-commit status, old-writer fencing, every M3/M5/workflow admission hook, restore/import/reopen meter verification and explicit opt-in profile identity. No public workflow writer until these exist. Meter discrepancies are integrity failures; valid history exceeding only a newly selected stricter profile is an enable/admission failure, not corruption. Neither option activates the unapproved million-record profile or proves throughput. Implementation should use the selected profile identifier `workflow-shared-capacity/1` (A) or `workflow-dual-legacy-capacity/1` (B); profile changes cannot reinterpret prior operation identity, cents or supplier allocations.

Requested decision: approve A, approve B, or defer new workflow mutation while retaining compiler/read progress. Exact acceptance is recorded separately by root; this document does not supply it.
