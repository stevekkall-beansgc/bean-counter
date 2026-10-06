# Bean Counter open beta

Owner decision, October 6, 2026: market the supported local SQLite product as an open beta. Named trial participants and completed M9 unfamiliar-human trials are not prerequisites for the beta. Feedback is welcome after users choose to participate; no one is contacted automatically. M9 remains unrun for eventual OSS 1.0 acceptance.

Suggested public description:

> Bean Counter is an open-beta local billing engine. Record agreed fixed or usage-based charges, retain exact receipts, reconcile retries, make traceable corrections, and export billing statements. You run it on your own computer and manage storage and backups. Start with the synthetic examples before using it for your business.

The beta supports one business per private local installation, multiple customers/sources, fixed and scale-18 per-work usage, explicit effective agreements, immutable close statements, linked post-close corrections and finance CSV exports. Operators provide agreement/authority evidence, trusted work facts, host resources and verified quiescent backups. An unknown result requires original-identity reconciliation.

Native qualification covers macOS 26.6.2 Apple silicon and Ubuntu 24.04.5 x86-64/glibc 2.39 (native GitHub CI). Linux supplemental checks and fresh public-download smoke used offline Docker emulation on Apple silicon. Signing/notarization, clean-account launch, other platforms and unfamiliar-human adoption remain unverified. Author tests establish their documented behavior, not general usability or capacity guarantees.

Hosted ChatGPT OAuth, remote authentication/backend/storage, PostgreSQL billing, multi-host writers, payments and tax/legal invoice claims remain outside the beta. Marketing the local beta does not certify those integrations or declare v1 complete.

[v0.9.3 is released](https://github.com/stevekkall-beansgc/bean-counter/releases/tag/v0.9.3) from `d9de6e2e50e846b2210c8c889f8d7493e127d5f7`, with optimized-Python-safe synthetic verification and bundled scope. [Start here](../START-HERE.md) for public download, verified installation and a synthetic receipt. The [qualification record](https://github.com/stevekkall-beansgc/bean-counter/releases/download/v0.9.3/QUALIFICATION.json) binds source and exact assets to actual checks. Published archives retain their build-time documents; current web guidance confirms publication without modifying those immutable assets.

Before publishing a new version, identify its exact source/version, pass manifest-owned local QA and exact-SHA CI, verify each newly built native archive and installed journey, and follow the governed release procedure. Published v0.9.2 remains a separate immutable baseline. Its synthetic helper must run without PYTHONOPTIMIZE; v0.9.3 includes the separately qualified helper fix. Beta positioning does not waive failed checks or authorize changing published tags/assets.

Optional feedback tasks: install and follow [first use](billing-quickstart.md), try a [Python or Node caller](agreement-and-integration.md), retry an original identity, inspect a correction and statement/export, and follow [backup/recovery](billing-recovery.md). Record observed failures, interventions and actual platform/runtime versions. Names and advance nomination are optional; production records should remain private.
