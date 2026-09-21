# Compatibility policy

The contract family is ledgerlab-contracts/1; the internal Cargo version is 0.0.0 and is not a published release. The development toolchain is pinned to the verified 1.98.1 release. No MSRV or SQLx/SQLite/Rustls/JCS package pin has been guessed.

Event/link/terms schema, DSL, evaluator semantics, canonical/hash profile, HTTP, logical storage/backend migration and export versions evolve independently. Preserve old canonical bytes and IDs. A changed numeric/order interpretation needs a new evaluator semantics version even if its JSON shape is unchanged. Unknown economic discriminators fail closed. Import must reject unsupported requirements before loading; startup must reject unsupported write schemas. No automatic downgrade or historical rerating.

The first public v0 must retain evaluator1 for original replay. Reading booked history never requires rerunning an evaluator. Correcting historic errors requires authorized compensating events. Coordinated 0.x minor releases may document breaking CLI/SDK changes; patches preserve contracts except explicit correctness/security fixes. Public releases require the Bean release gates and exact-artifact evidence; none is claimed here.
