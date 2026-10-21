//! Actual original-binding/current-source observations under the held writer.
//! This is NOT a completed financial prefix, eligibility or prepared commit.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    policy::workflow::transition_constructor::ExactRef,
    validation_work::{ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind},
};

pub(super) mod app;
pub(super) mod party;
pub(super) mod source;
