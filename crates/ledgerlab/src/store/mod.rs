// The private storage boundary is integrated by the acceptance coordinator.
pub(crate) mod errors;
pub(crate) mod ports;
pub(crate) mod records;
pub(crate) mod sqlite;

pub(crate) mod postgres;

#[cfg(test)]
pub(crate) mod outcome_evidence;
pub(crate) mod outcomes;
