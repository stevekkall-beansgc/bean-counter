//! Current delivery authority for a fresh transport alias of an authenticated
//! original Work. No pricing, new Work/claim, chain advance or receipt is made.
use super::super::selected_program::SelectedProgram;
use super::{
    authority::EconomicMode, data::*, records::Records, shape,
    source_records::InitialSourceCorrespondence, CoreMeter,
};
use crate::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{OriginalAuditSession, RepresentationKind},
    Result,
};
pub struct LocalWorkAliasAuthority {
    snapshot: AuditedValue,
    normalized: AuditedBytes,
    consumed: super::records::OwnedConsumed,
}
impl LocalWorkAliasAuthority {
    pub fn authority_snapshot(&self) -> &serde_json::Value {
        self.snapshot.value()
    }
    pub fn normalized_payload(&self) -> &[u8] {
        self.normalized.bytes()
    }
    pub(super) fn into_parts(self) -> (AuditedValue, AuditedBytes, super::records::OwnedConsumed) {
        (self.snapshot, self.normalized, self.consumed)
    }
}
fn same(m: &mut CoreMeter, left: &[u8], right: &[u8]) -> Result<()> {
    if !shape::same_bytes(m, left, right)? {
        return Err(crate::Error::new(
            "IDENTITY_CONFLICT",
            "Work alias economic intent changed",
        ));
    }
    Ok(())
}
/// The host first authenticates original producer/member/receipt/cutoff and
/// captures today's actual delivering source. This consumer repeats original
/// identity/intent and current+original nominated Work rights. The host must
/// create a NEW semantic_alias wrapper/evidence/receipt under the incoming key,
/// associate the original result_refs and preserve the permanent WorkClaim.
pub fn construct_local_work_alias_authority(
    program: &SelectedProgram,
    c: &DraftContextData,
    original_work: EconomicRecordView<'_>,
    incoming_raw: &[u8],
    selected: (&[EconomicRecordView<'_>], &[&InitialSourceCorrespondence]),
    session: &mut OriginalAuditSession,
) -> Result<LocalWorkAliasAuthority> {
    let result = (|| {
        session.check_cancelled()?;
        program.require_attempt_owner(session.meter_mut())?;
        if !program.uses_shared_namespace_recipe() {
            return Err(shape::fail("exact shared namespace alias recipe required"));
        }
        let a = session.arena().shared_handle();
        let mut records = Records::borrowed_parent_selected(
            session.meter_mut(),
            &a,
            &c.key,
            selected.0,
            program,
        )?;
        records.install_initial_source_outputs(session.meter_mut(), selected.1)?;
        let work = records.get(
            session.meter_mut(),
            &c.key,
            original_work.reference,
            "workflow-work",
            "ledger-workflow-work-record/1",
        )?;
        // A selected view must be the same retained immutable body, not merely
        // a Ref whose bytes the caller replaced in another input partition.
        let actual = records.canonical(session.meter_mut(), original_work.reference)?;
        same(session.meter_mut(), actual, original_work.canonical_body)?;
        let payload = shape::field(session.meter_mut(), &work, "payload")?;
        let original =
            session.canonical_representation(payload, RepresentationKind::OriginalRequest)?;
        let comparison = super::intent::normalize_local_work_alias_intent(
            program,
            original.bytes(),
            incoming_raw,
            session,
        )?;
        same(session.meter_mut(), original.bytes(), comparison.bytes())?;
        let incoming =
            session.parse_representation(incoming_raw, RepresentationKind::OriginalRequest)?;
        let mut normalized = session
            .parse_representation(comparison.bytes(), RepresentationKind::OriginalRequest)?;
        let event = shape::field(session.meter_mut(), incoming.value(), "external_event_id")?;
        let event = a.clone_value(event, session.meter_mut())?;
        normalized.replace_existing_owned("external_event_id", event, session.meter_mut())?;
        let namespace = shape::text(session.meter_mut(), &work, "source_namespace")?;
        let (_, activity) = super::input_selection::work_activity(
            program,
            normalized.value(),
            session.meter_mut(),
        )?;
        let source_slot = shape::text(session.meter_mut(), activity, "source_slot")?;
        let resolved = super::app_cost::binding_resolved(
            session.meter_mut(),
            &mut records,
            c,
            normalized.value(),
            program,
            EconomicMode::WorkAlias,
            (source_slot, &[], namespace),
        )?;
        let snapshot = resolved.into_alias_snapshot()?;
        let normalized = session
            .canonical_representation(normalized.value(), RepresentationKind::OriginalRequest)?;
        Ok(LocalWorkAliasAuthority {
            snapshot,
            normalized,
            consumed: records.into_consumed_owned(),
        })
    })();
    if result.is_err() {
        session.invalidate();
    }
    result
}
