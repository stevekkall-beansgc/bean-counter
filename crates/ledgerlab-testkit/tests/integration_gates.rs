//! Deliberate visible gates. These are not passing product conformance tests.
//! Replace each body with the specified generic runner when its real adapter is
//! linked. Running --ignored before integration MUST fail, never silently skip.
macro_rules! gate {
    ($name:ident, $reason:literal) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            panic!($reason);
        }
    };
}
// Acceptance, cancellation and barrier races now execute in ledgerlab's private
// service test adapters: service::tests (SQLite) and service::pg_tests (PG17/18).
// PG cases are explicit opt-in local-server tests; see PHASE-1-STATUS.md for runs.
gate!(sqlite_fake_lost_response, "GATED: production outbox/fake destination, durable independent receipt and reconcile adapter absent; run_delivery");
gate!(postgres18_fake_lost_response, "GATED: production PG outbox/fake isolated durable namespace and reconcile adapter absent; run_delivery");
