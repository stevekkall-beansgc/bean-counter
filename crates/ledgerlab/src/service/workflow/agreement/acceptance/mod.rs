//! Agreement input preparation only until the private resolver and atomic
//! coordinator authenticate origins, assent, current grants and head pins.
mod payload;
mod resolve;

pub(super) use payload::AcceptInput;
pub(super) use resolve::PublicationInputs;
