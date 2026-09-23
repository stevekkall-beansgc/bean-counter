//! Bounded indexed requests. A cursor alone is never proof of verified history.
use super::{
    commands::{Case, ExpectedPrefix},
    ensure,
    types::{Count, Digest},
};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageAddress {
    pub segment: Digest,
    pub page: Count,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFragment {
    pub address: PageAddress,
    pub offset: u16,
    pub bytes: Vec<u8>,
    pub total_bytes: Count,
}
impl PageFragment {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.bytes.is_empty()
                && self.bytes.len() <= super::PAGE_BYTES
                && usize::from(self.offset) + self.bytes.len() <= super::PAGE_BYTES,
            "PAGE",
            "bounded indexed fragment",
        )
    }
}
/// Complete identity and exact selected head, not a caller-selected hash shortcut.
impl ExpectedPrefix {
    pub fn matches(&self, actual: &Self) -> Result<()> {
        ensure(
            self == actual,
            "EXPECTED_PREFIX",
            "scope, target, profile, enrollment and exact head",
        )
    }
}
/// A derived view: CLOSE persists one bounded certificate, never N case updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferWitness {
    pub case: Case,
    pub admission: Digest,
    pub first_affecting_certificate: Digest,
    pub selected_prefix: ExpectedPrefix,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectiveLifecycle {
    OrdinaryPending,
    AdjustmentPending {
        transfer: Option<Box<TransferWitness>>,
        reason: AdjustmentReason,
    },
    FinalAllow {
        revision: Count,
        transfer: Option<Box<TransferWitness>>,
    },
    FinalDeny {
        transfer: Option<Box<TransferWitness>>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustmentReason {
    ClosureTransfer,
    DelayedImport,
    PostClosure,
    UnresolvedClock,
}
