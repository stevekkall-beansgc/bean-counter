//! Selected initial A.setup supplied-DATA closure. Every validator is executed
//! by the common draft/replay path. Broader lineages and host authentication
//! remain explicit unsupported branches or unresolved admission obligations.
#[macro_use]
mod check;
pub(super) mod evidence;
pub(super) mod slots;
pub(super) mod source;

#[cfg(test)]
mod tests;
pub(super) mod transitions;

pub(super) mod publication;

pub(super) mod observation;

pub(super) mod binding;
#[cfg(test)]
mod completion_tests;
pub(super) mod entry;
mod historical;
mod request;
