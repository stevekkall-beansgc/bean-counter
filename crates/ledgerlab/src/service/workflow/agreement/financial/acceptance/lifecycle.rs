//! Exact accepted-context Reference operands under the acceptance's own cutoff.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    select_fact_reference_inputs, validate_lifecycle_context_references, EconomicRecordView,
    ExactRef, FactReferenceInputs,
};

pub(super) struct ContextReferences {
    pool: super::super::local_inputs::LocalInputs,
    _selected: FactReferenceInputs,
}
impl ContextReferences {
    pub(super) fn views(&self) -> impl Iterator<Item = EconomicRecordView<'_>> {
        self.pool.iter_views()
    }
    pub(super) fn references(&self) -> impl Iterator<Item = &ExactRef> {
        self.views().map(|v| v.reference)
    }
}
pub(super) async fn acquire(
    tx: &mut SqliteTx,
    key: &OperationKey,
    publication: &prefix::RecompiledPublication,
    binding: &wire::SourceRecord,
    cutoff: i64,
    at: &str,
    s: &mut OriginalAuditSession,
) -> Result<Option<ContextReferences>> {
    let program = publication.program();
    if !program.uses_work_lifecycle_recipe() {
        return Ok(None);
    }
    let body = s
        .parse_representation(binding.bytes(), Kind::Record)
        .map_err(core_error)?;
    let context = request_wrapper::field(body.value(), "accepted_context", s)?;
    let selected = select_fact_reference_inputs(
        program,
        request_wrapper::field(context, "fact_schema_ref", s)?,
        request_wrapper::field(context, "facts", s)?,
        s,
    )
    .map_err(core_error)?;
    let mut pool = super::super::local_inputs::LocalInputs::new(s)?;
    for field in selected.entries() {
        pool.add(field.reference(), s)?;
    }
    pool.complete_lifecycle(program, tx, key, cutoff, s).await?;
    let (core_key, _key_owner) = super::super::local_inputs::core_key_with_owner(key, s)?;
    validate_lifecycle_context_references(
        program,
        &core_key,
        EconomicRecordView {
            reference: binding.reference(),
            canonical_body: binding.bytes(),
        },
        &pool.views(),
        at,
        s,
    )
    .map_err(core_error)?;
    Ok(Some(ContextReferences {
        pool,
        _selected: selected,
    }))
}
