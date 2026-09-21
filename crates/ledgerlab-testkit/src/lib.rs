//! Independent Phase 1 oracle and reusable, adapter-driven conformance runners.
//!
//! No production evaluator computes expected values. [`FixtureOracle`] audits and
//! reads the frozen journal using the Python standard library. The runners require
//! real stores; their unit tests exercise assertion sensitivity, not persistence.
#![forbid(unsafe_code)]

pub mod cases;
pub mod failpoints;
pub mod history;
pub mod oracle;
pub mod stores;

pub use oracle::{FixtureOracle, HarnessError, Result};
