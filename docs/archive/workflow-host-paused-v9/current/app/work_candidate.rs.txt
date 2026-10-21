//! Actual borrowed bridge into the complete core constructor. This cannot
//! manufacture observations, audit correspondence, eligibility or append.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    EconomicCandidate, EconomicObservationInput, EconomicStateData, RecomputedAuditDigest,
};
use std::mem;

impl ObservedAppBinding {
    /// The caller must retain its actual header/context owners for this call.
    /// Core independently checks every selected input and held-state relation,
    /// then executes the existing full capacity/resource gate under ONE child.
    pub(super) fn construct_root_work(
        &mut self,
        context: &work_context::OwnedWorkContext,
        raw_payload: &[u8],
        observation: &EconomicObservationInput<'_>,
        audit: &RecomputedAuditDigest<'_>,
        s: &mut OriginalAuditSession,
    ) -> Result<EconomicCandidate> {
        let result = (|| {
            let _control = s
                .reserve_external_payload(mem::size_of::<EconomicStateData<'_>>())
                .map_err(core_error)?;
            let state = self.work_state.as_ref().ok_or(ServiceError::Unavailable)?;
            let bodies = self
                .work_input_bodies
                .as_ref()
                .ok_or(ServiceError::Unavailable)?;
            let views = bodies.views(s)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<EconomicStateData<'_>>())
                .map_err(core_error)?;
            let state = EconomicStateData {
                // Work creates no assertion and performs no assertion-key lookup.
                retained_assertion: None,
                financial: &state.financial,
                capacity_heads: &state.capacities,
                supplier: None,
                app: None,
                current_party_authorizations: &state.parties,
                observation: Some(observation),
                audit: Some(audit),
                // The completed source-output correspondence is selected by
                // the exact original enrollment in the prefix-owned bridge.
                initial_source_correspondences: &[],
            };
            self.original
                .construct_root_work(&context.context, raw_payload, &views.rows, &state, s)
        })();
        if result.is_err() {
            s.invalidate();
        }
        result
    }
}
