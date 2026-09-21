//! Bounded, pure Phase 2 pricing over explicit resolved input.
//!
//! This is an in-memory Rust API, not a new wire/canonical record version and
//! not an acceptance API. The coordinator must supply the complete pinned
//! binding set, retained evidence, complete chain history, and authority under
//! ordered locks. Results cannot be passed to the Phase 1 store append port.
//! No current price, clock, party, causal link, or authority is inferred.
mod compile;
mod evaluate;
mod model;
mod reversal;

/// Final-base outcome claims and atomic authorized corrections.
pub mod outcomes;

pub use model::*;
pub use reversal::reverse;

#[cfg(test)]
mod tests;

pub mod retained;
