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
gate!(sqlite_acceptance_83_cases, "GATED: file SQLite facade/fixture initializer/readback adapter absent; run acceptance_cases + run_case for all 83 cases");
gate!(postgres18_acceptance_83_cases, "GATED: PG adapter blocked on reviewed PEM-only TLS driver decision; local PG18 absent; run all 83 cases");
gate!(postgres17_acceptance_83_cases, "GATED: PG adapter blocked on reviewed PEM-only TLS driver decision; local PG17 absent; mandatory before public v0");
gate!(sqlite_cancellation_all_awaits, "GATED: SQLite await catalogue, fault hooks, bounded cancellation/drain and pool transaction probe absent; run_cancellation");
gate!(postgres18_cancellation_all_awaits, "GATED: PG18 await catalogue, fault hooks, primary resolution and pool transaction probe absent; run_cancellation");
gate!(sqlite_concurrent_identical, "GATED: real file SQLite contention/barrier trace adapter absent; run_race with at least two connections");
gate!(
    postgres18_concurrent_identical,
    "GATED: real PG18 two-connection lock/retry barrier trace adapter absent; run_race"
);
gate!(sqlite_fake_lost_response, "GATED: production outbox/fake destination, durable independent receipt and reconcile adapter absent; run_delivery");
gate!(postgres18_fake_lost_response, "GATED: production PG outbox/fake isolated durable namespace and reconcile adapter absent; run_delivery");
