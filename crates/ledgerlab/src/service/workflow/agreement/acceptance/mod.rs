//! Agreement input preparation only until the private resolver and atomic
//! coordinator authenticate origins, assent, current grants and head pins.
pub(super) mod authority;
pub(super) mod construct;
mod payload;
mod prepare;
mod resolve;

pub(super) use payload::AcceptInput;
pub(super) use resolve::PublicationInputs;
