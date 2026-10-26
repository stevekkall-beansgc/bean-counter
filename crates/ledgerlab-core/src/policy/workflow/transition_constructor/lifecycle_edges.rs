//! Fixed immutable identity operands omitted from explicit record Refs. These
//! selectors are acquisition DATA, never producer/member or relation authority.
use super::super::selected_program::SelectedProgram;
use super::{
    body,
    data::{EconomicRecordView, ExactRef, OperationKey},
    input_edges::{select_local_input_edges, LocalInputEdges},
    records::Records,
    shape, CoreMeter,
};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, OriginalAuditSession},
    Result,
};
use std::mem;

pub struct LifecycleRecordIdentity {
    kind: &'static str,
    id: String,
    schemas: &'static [&'static str],
    role: &'static str,
}
impl LifecycleRecordIdentity {
    pub fn kind(&self) -> &str {
        self.kind
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn schemas(&self) -> &[&'static str] {
        self.schemas
    }
    pub fn role(&self) -> &str {
        self.role
    }
}
pub struct LifecycleInputEdges {
    explicit: LocalInputEdges,
    identities: Vec<LifecycleRecordIdentity>,
    _ownership: ExternalBufferLease,
}
impl LifecycleInputEdges {
    pub fn direct_refs(&self) -> impl Iterator<Item = &ExactRef> {
        self.explicit.direct_refs()
    }
    pub fn record_identities(&self) -> &[LifecycleRecordIdentity] {
        &self.identities
    }
}
fn name<'a>(m: &mut CoreMeter, v: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    let value = shape::text(m, v, key)?;
    if w::compare(m, value.len(), 128usize)?.is_gt() || w::compare(m, value.len(), 0usize)?.is_eq()
    {
        return Err(shape::fail("lifecycle immutable identity grammar"));
    }
    Ok(value)
}
/// Adds only fixed immutable identities: Work's birth claim and Action's exact
/// named decision revision. The ordinary typed Ref kernel remains unchanged.
/// In particular an inverse's named revision does not prove which operation
/// produced it; its actual producer, inverse_of and effect joins remain due.
/// The coordinator must process every actual body, including deduped rows, at
/// the held original cutoff. No current head or arbitrary history is selected.
pub fn select_lifecycle_input_edges(
    program: &SelectedProgram,
    key: &OperationKey,
    input: EconomicRecordView<'_>,
    session: &mut OriginalAuditSession,
) -> Result<LifecycleInputEdges> {
    let result = (|| {
        session.check_cancelled()?;
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_work_lifecycle_recipe() {
            return Err(shape::fail("exact lifecycle input recipe required"));
        }
        let explicit = select_local_input_edges(key, input, session)?;
        let a = session.arena().shared_handle();
        let m = session.meter_mut();
        let inputs = [input];
        let mut r = Records::borrowed_parent_app(m, &a, key, &inputs)?;
        let v = r.provenance(m, key, input.reference)?;
        let schema = shape::text(m, &v, "schema")?;
        let work = shape::same_text(m, schema, "ledger-workflow-work-record/1")?;
        let action = shape::same_text(m, schema, "ledger-workflow-action/1")?;
        if (work && !shape::same_text(m, &input.reference.kind, "workflow-work")?)
            || (action && !shape::same_text(m, &input.reference.kind, "workflow-action")?)
        {
            return Err(shape::fail("lifecycle implicit identity record kind"));
        }
        let count = if work { 2 } else { usize::from(action) };
        let id_bytes = if work {
            {
                let n = name(m, &v, "claim_id")?.len();
                w::usize_add(m, n, 70)?
            }
        } else if action {
            70
        } else {
            0
        };
        let bytes = w::usize_add(m, mem::size_of::<LifecycleInputEdges>(), id_bytes)?;
        let bytes = w::usize_add(
            m,
            bytes,
            count
                .checked_mul(mem::size_of::<LifecycleRecordIdentity>())
                .ok_or_else(|| shape::fail("identity controls"))?,
        )?;
        let ownership = a.reserve_external_payload(bytes, m)?;
        m.write_bytes(bytes)?;
        let mut identities = Vec::new();
        identities
            .try_reserve_exact(count)
            .map_err(|_| shape::fail("lifecycle identity vector"))?;
        if work {
            let id = name(m, &v, "claim_id")?;
            m.scan_bytes(id.len())?;
            m.write_bytes(id.len())?;
            identities.push(LifecycleRecordIdentity {
                kind: "workflow-work-claim",
                id: id.to_owned(),
                schemas: &["ledger-workflow-work-claim/1"],
                role: "work_birth_claim",
            });
            // Work's initial revision is an immutable birth, not a current head.
            let _scratch = a.reserve_external_payload(1024, m)?;
            let decision = super::work_keys::decision(m, id)?;
            let decision = a.string_value(&decision, m)?;
            let one = a.string_value("1", m)?;
            let tuple = a.array_from_values(&[decision.value(), one.value()], m)?;
            let id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
            identities.push(LifecycleRecordIdentity {
                kind: "workflow-decision",
                id,
                schemas: &["ledger-workflow-decision-revision/1"],
                role: "work_initial_decision",
            });
        } else if action {
            let decision = name(m, &v, "decision_id")?;
            let revision = shape::text(m, &v, "revision")?;
            let n = super::arithmetic::integer(m, revision, false)?;
            if !super::meter::cmp(m, &n, &1.into())?.is_ge() {
                return Err(shape::fail("positive action decision revision"));
            }
            let decision = a.string_value(decision, m)?;
            let revision = a.string_value(revision, m)?;
            let tuple = a.array_from_values(&[decision.value(), revision.value()], m)?;
            // Serial raw hash formatting overlaps its returned digest and the
            // final identity. Tuple/encoded fragments have arena owners; the
            // final70-byte ID has the retained wrapper owner above.
            let scratch = 3 * mem::size_of::<String>()
                + "bean-counter/workflow-decision-revision/1\0".len()
                + 64
                + 71;
            let _scratch = a.reserve_external_payload(scratch, m)?;
            m.write_bytes(scratch)?;
            let id = body::semantic_id(m, "wdrev_", "workflow-decision-revision", tuple.value())?;
            identities.push(LifecycleRecordIdentity {
                kind: "workflow-decision",
                id,
                schemas: &[
                    "ledger-workflow-decision-revision/1",
                    "ledger-workflow-decision-revision/2",
                    "ledger-workflow-decision-revision/3",
                ],
                role: "action_named_decision_revision",
            });
        }
        Ok(LifecycleInputEdges {
            explicit,
            identities,
            _ownership: ownership,
        })
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
