//! Original v2 base acceptance for R3 enrollment, without Phase 3 contingent
//! reservation registration. All constructors stay inside the acceptance service.
use crate::store::outcomes::*;

#[derive(Clone, Debug)]
pub(crate) struct ValidatedOriginalBasePlan {
    pub(in crate::service::accept) resolve: OutcomeResolve,
    pub(in crate::service::accept) observed: Vec<ObservedOutcomeHead>,
    pub(in crate::service::accept) records: Vec<Vec<u8>>,
    pub(in crate::service::accept) writes: Vec<OutcomeHeadWrite>,
    pub(in crate::service::accept) ingress: Vec<u8>,
    pub(in crate::service::accept) ingress_hash: String,
    pub(in crate::service::accept) receipt: Vec<u8>,
}
impl ValidatedOriginalBasePlan {
    pub(crate) fn resolution(&self) -> &OutcomeResolve {
        &self.resolve
    }
    pub(crate) fn observed_heads(&self) -> &[ObservedOutcomeHead] {
        &self.observed
    }
    pub(crate) fn records(&self) -> &[Vec<u8>] {
        &self.records
    }
    pub(crate) fn head_writes(&self) -> &[OutcomeHeadWrite] {
        &self.writes
    }
    pub(crate) fn delivery_key(&self) -> &ScopedDelivery {
        &self.resolve.delivery
    }
    pub(crate) fn ingress(&self) -> &[u8] {
        &self.ingress
    }
    pub(crate) fn ingress_hash(&self) -> &str {
        &self.ingress_hash
    }
    pub(crate) fn receipt(&self) -> &[u8] {
        &self.receipt
    }
}
