//! Bounded driver proof, NOT an integrated store or a persistence oracle.
#![forbid(unsafe_code)]

pub mod connect;
pub mod tls;
pub mod tx;

#[cfg(test)]
mod tests;
