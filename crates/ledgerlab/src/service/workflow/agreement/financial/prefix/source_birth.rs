//! Resolve implicit State1/Grant1 through the enrollment's actual completed
//! earliest origin. Result rows remain DATA; no new proof key or ID-latest
//! lookup replaces the prior chronological source.enroll completion.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;
pub(in super::super) struct SourceBirthInputs {
    pub(in super::super) state: current::source::SourceRecord,
    pub(in super::super) grant: current::source::SourceRecord,
    _storage: ExternalBufferLease,
}
fn same(a: &[u8], b: &[u8], s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    require(a == b, "INTEGRITY_FAILURE")
}
fn result_ref<'a>(
    rows: &'a [Value],
    kind: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    let mut found = None;
    for row in rows {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        if super::super::acceptance::same_recipe_text(
            request_wrapper::text(row, "kind", s)?,
            kind,
            s,
        )? {
            s.semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            require(found.replace(row).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    found.ok_or(ServiceError::IntegrityFailure)
}
impl LocatedAppAcceptance {
    pub(in super::super) async fn source_birth_inputs(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        enrollment: &ExactRef,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<SourceBirthInputs> {
        let result = async {
            let storage = s
                .reserve_external_payload(std::mem::size_of::<SourceBirthInputs>())
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
                .map_err(core_error)?;
            let through = before
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&through.into(), &self.through.into()])
                .map_err(core_error)?;
            require(through == self.through, "INTEGRITY_FAILURE")?;
            let expected = wire::reference(enrollment, s)?;
            let parsed = s
                .parse_representation(expected.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let sequence=if let Some(prefix)=self._prefix.as_mut(){prefix._inventory.introduction_sequence(parsed.value(),s)?}
            else{tx.workflow_selected_association_sequence(enrollment,&key.logical_store_id,&key.scope,before,s).await.map_err(read_error)?.ok_or(ServiceError::IntegrityFailure)?};
            s.semantic_work()
                .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
                .map_err(core_error)?;
            let previous = sequence
                .checked_sub(1)
                .ok_or(ServiceError::IntegrityFailure)?;
            let origin = next(tx, key, previous, sequence, s)
                .await?
                .ok_or(ServiceError::IntegrityFailure)?
                .checked_wrappers(tx, s)
                .await?;
            let payload =
                request_wrapper::field(origin.wrappers().original_request().value(), "payload", s)?;
            request_wrapper::same_text(
                request_wrapper::text(payload, "schema", s)?,
                "ledger-workflow-source-enroll/1",
                s,
            )?;
            request_wrapper::same_text(
                request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
                "administrative",
                s,
            )?;
            let rows = request_wrapper::field(origin.wrappers().receipt(), "result_refs", s)?
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&rows.len().into(), &3usize.into()])
                .map_err(core_error)?;
            require(rows.len() == 3, "INTEGRITY_FAILURE")?;
            let actual = result_ref(rows, "workflow-source-enrollment", s)?;
            let actual = s
                .canonical_representation(actual, RepresentationKind::Record)
                .map_err(core_error)?;
            same(actual.bytes(), expected.bytes(), s)?;
            let state_ref = result_ref(rows, "workflow-source-state", s)?;
            let grant_ref = result_ref(rows, "workflow-source-grant", s)?;
            origin.require_member(state_ref, s)?;
            origin.require_member(grant_ref, s)?;
            // This inventory consists only of already completed earlier
            // originals. Literal wrapper outputs alone confer no completion.
            if let Some(prefix)=self._prefix.as_mut(){prefix._inventory.require_value(state_ref,s)?;prefix._inventory.require_value(grant_ref,s)?;}
            let state = current::source::retained_record(
                tx,
                key,
                state_ref,
                "workflow-source-state",
                "ledger-workflow-source-state/1",
                before,
                s,
            )
            .await?;
            let grant = current::source::retained_record(
                tx,
                key,
                grant_ref,
                "workflow-source-grant",
                "ledger-workflow-source-grant/1",
                before,
                s,
            )
            .await?;
            if let Some(prefix)=self._prefix.as_mut(){prefix._inventory.compare_record(state.reference(),state.canonical_body(),s)?;prefix._inventory.compare_record(grant.reference(),grant.canonical_body(),s)?;}
            else {for r in [state.reference(),grant.reference()]{verify_selected_association(tx,key,r,before,s).await?;}}
            request_wrapper::same_text(
                request_wrapper::text(state.body(), "revision", s)?,
                "1",
                s,
            )?;
            request_wrapper::same_text(
                request_wrapper::text(grant.body(), "revision", s)?,
                "1",
                s,
            )?;
            for (body, field) in [
                (state.body(), "enrollment_ref"),
                (grant.body(), "original_ceiling_ref"),
            ] {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let value = request_wrapper::field(body, field, s)?;
                let actual = s
                    .canonical_representation(value, RepresentationKind::Record)
                    .map_err(core_error)?;
                same(actual.bytes(), expected.bytes(), s)?;
            }
            let value = request_wrapper::field(state.body(), "current_grant_ref", s)?;
            let actual = s
                .canonical_representation(value, RepresentationKind::Record)
                .map_err(core_error)?;
            let expected = wire::reference(grant.reference(), s)?;
            same(actual.bytes(), expected.bytes(), s)?;
            s.semantic_work()
                .write_bytes(std::mem::size_of::<SourceBirthInputs>())
                .map_err(core_error)?;
            Ok(SourceBirthInputs {
                state,
                grant,
                _storage: storage,
            })
        }
        .await;
        if result.is_err() {
            s.invalidate();
            tx.workflow_proof_failure_handle()
                .store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }
}
