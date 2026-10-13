//! Agreement input preparation only until the private resolver and atomic
//! coordinator authenticate origins, assent, current grants and head pins.
mod payload;

pub(super) use payload::AcceptInput;
