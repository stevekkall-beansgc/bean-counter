# Owner authorization: complete remaining Phase 4

The owner replied “Approved” to the coordinator's concrete request to complete the remaining Phase 4 requirements in other sessions with monitoring here. The approved remaining scope is:

- Freeze the validated contract and review the runtime interfaces.
- Implement the required behavior in the runtime, SQLite, PostgreSQL and offline gateway.
- Verify crash recovery, writer exclusion, storage limits and protected completion.
- Demonstrate the customer comparison without writes, then independently review one integrated build.

This supersedes the four-blocker-only stopping boundary for these remaining Phase 4 requirements. The four-blocker corrections are closed and must not be redone without a demonstrated regression. Ordinary implementation choices and fixes needed to meet the existing Phase 4 requirements are authorized; a new product requirement or expansion beyond Phase 4 needs explicit owner approval. Do not turn routine line-level implementation adjustments into approval requests.

## Exact starting points and requirements

Canonical accepted candidate: c8fbddc682e22e10c6122abbcf4c8212a1c407fa, tree96c0dc5d60cc48c5a3e559e6d2d7e4692ecc01d2. Candidate inventory SHA256301766b5f458ccaf175e90f9f962daa7fa6fa1cf144e19f1526003d9229f78be. Owner approval now includes its additive freeze disposition and remaining runtime continuation, subject to required technical gates and preservation of old frozen bytes.

Separately accepted PostgreSQL baseline test correction: b0b3984ce879f653f8816fe5e8844cd7b076f78c. Adopted documentation/foundation base: b48dd50b89f7353f462ced9bf8bbe20c94a438bb, underlying runtime foundation84616f43b8900de17392887f46d8947b9087797c. Integration owner combines reviewed work in isolated lineage and verifies the resulting source; separate prior passes do not prove the assembled build.

Governing scope: outputs/TAKEOVER-RECONCILIATION-AND-PLAN.md section5, especially all G2 acceptance rows; full R3 SHA256341043f03c7879c0d1ef02b76e054e5fd7ab287d745f48d434eb59162d148db6 and all16 section13 requirements; independent oracle under outputs/execution/oracle; outputs/execution/phase4-full/lead/REQUIREMENTS-RESET.md for the audited remaining-work mapping. Latest bounded closure is outputs/execution/phase4-full/acceptance/four-blockers/FOUR-BLOCKER-CLOSURE-DECISION.md. Preserve these and historical failures/WIP.

## Execution and review

Resume existing execution lead and independent acceptance tasks. Work remains in other sessions, using the previously authorized Astra/Sol/Luna models and bounded children. This coordinator only supervises, verifies reports and communicates with the owner. No redundant new planning phase. First deliver additive freeze and one concrete typed-interface review, then implement against that interface in bounded waves. Use at most two concurrent authors with disjoint owned paths; one integration owner controls shared interfaces, manifests, migrations and lockfiles. Reviewers inspect immutable exact targets.

Deliver the smallest actual end-to-end path first, then required receipt/alias/unused-token branches, cancellation/recovery, economics and scale. This is sequencing only, not permission to reduce the final scope. Reuse the adopted foundation and approved synthetic oracle; do not rebuild old lanes or change settled policy. Keep reviewers engaged with concrete targets rather than withholding results for repeated design refinements.

Full acceptance requires actual runtime/store/gateway evidence, SQLite and both cached PostgreSQL majors, independent durability and resource-bound checks, genuine comparison and observed nonposting, and a fresh independent decision on one clean assembled Phase4 candidate. Reference-model validation and historical foundation passes are not substitutes. Run required checks meaningfully; retain ignored/deferred counts and all limitations. Use existing cached/offline tools and isolated owned test services; preserve files/volumes and stop test services after completion.

Report exact commits, changed behavior, fresh evidence, remaining gates and concrete blockers to coordinator01a0ca4a-a824-7bb0-aa27-68fb336ace4f, using local output artifacts if messaging fails. Maintain one compact current Phase4 execution status with gate/owner/target/result/next step. Progress means working code, exact test results or a resolved blocker, not repeated planning or documentation. The coordinator's heartbeat monitors drift and stalls and reports meaningful milestones or required decisions.

## Stop boundary

Stop after independently accepted full Phase4 and a consolidated owner handoff, or at a genuine external/owner-decision blocker while continuing unaffected authorized work. Do not claim closure without all required G2 evidence. No Phase5/6, main merge, push/tag/release/deployment, destructive cleanup, new hosted/service infrastructure, purchases, production data or outreach. No new product/economic/authority/availability policy or scope reduction. These require explicit owner permission; routine engineering within the approved Phase4 requirements does not.
