//! Agreement input preparation only until the private resolver and atomic
//! coordinator authenticate origins, assent, current grants and head pins.
pub(super) mod authority;
mod construct;
mod payload;
mod resolve;

pub(super) use payload::AcceptInput;
pub(super) use resolve::PublicationInputs;
