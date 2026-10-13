//! Pure generic workflow semantics. This initial increment validates fact
//! schemas and facts; it does not establish publication or economic acceptance.
mod facts;
pub use facts::{FactSchema, FactValue, NormalizedFacts, SignedScalar};
