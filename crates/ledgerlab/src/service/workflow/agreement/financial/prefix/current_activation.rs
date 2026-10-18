//! Initial boundary DATA from a completely replayed activation. The scratch
//! locators below are populated only after exact original journal equality.
//! Later schedule transitions still require their own chronological recipes.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::ExactRef;

pub(in super::super) struct CompletedInitialActivation {
    pub(in super::super) head: wire::SourceRecord,
    pub(in super::super) activation: wire::SourceRecord,
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
impl FoldedInventory {
    fn completed_initial_activation(
        &mut self,
        binding_ref: &ExactRef,
        binding: &Value,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        let storage = s
            .reserve_external_payload(std::mem::size_of::<CompletedInitialActivation>())
            .map_err(core_error)?;
        let customer_value = request_wrapper::field(binding, "customer", s)?;
        let customer = s
            .canonical_representation(customer_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let agreement_value = request_wrapper::field(binding, "agreement_id", s)?;
        let agreement = s
            .canonical_representation(agreement_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let boundary = inventory::StateKey::new(
            "agreement-initial-activation",
            &[customer.bytes(), agreement.bytes()],
            s,
        )?;
        let head = self
            .activation_head(binding, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let activation = self
            ._inventory
            .head_body(&boundary, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let parsed_head = s
            .parse_representation(head.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let parsed_activation = s
            .parse_representation(activation.bytes(), RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let revision = request_wrapper::text(parsed_head.value(), "new_revision", s)?;
        if !super::super::acceptance::same_recipe_text(revision, "1", s)? {
            return Err(ServiceError::Unavailable);
        }
        request_wrapper::same_text(
            request_wrapper::text(parsed_activation.value(), "action", s)?,
            "activate",
            s,
        )?;
        let expected = wire::reference(binding_ref, s)?;
        for (body, field) in [
            (parsed_head.value(), "accepted_binding_ref"),
            (parsed_activation.value(), "binding_ref"),
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let actual_value = request_wrapper::field(body, field, s)?;
            let actual = s
                .canonical_representation(actual_value, RepresentationKind::SourceDefinition)
                .map_err(core_error)?;
            same(actual.bytes(), expected.bytes(), s)?;
        }
        let customer = request_wrapper::text(binding, "customer", s)?;
        let agreement = request_wrapper::text(binding, "agreement_id", s)?;
        let key = request_wrapper::field(parsed_head.value(), "operation_key", s)?;
        let head = wire::agreement_record(
            wire::AgreementRecord::ActivationHead,
            head,
            key,
            customer,
            agreement,
            revision,
            s,
        )?;
        self._inventory
            .compare_record(head.reference(), head.bytes(), s)?;
        let head_ref = wire::reference(head.reference(), s)?;
        let actual_value =
            request_wrapper::field(parsed_activation.value(), "head_transition_ref", s)?;
        let actual = s
            .canonical_representation(actual_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        same(actual.bytes(), head_ref.bytes(), s)?;
        let activation = wire::agreement_record(
            wire::AgreementRecord::Activation,
            activation,
            key,
            customer,
            agreement,
            revision,
            s,
        )?;
        self._inventory
            .compare_record(activation.reference(), activation.bytes(), s)?;
        let _time = s
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        let effective = Timestamp::parse_with_work(
            request_wrapper::text(parsed_activation.value(), "effective_at", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&effective.micros().into(), &at.micros().into()],
            )
            .map_err(core_error)?;
        require(effective.micros() <= at.micros(), "AUTHORITY_REQUIRED")?;
        Ok(CompletedInitialActivation {
            head,
            activation,
            _storage: storage,
        })
    }
}
impl LocatedAppAcceptance {
    pub(in super::super) async fn current_initial_activation(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        let result = async {
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
            let completed = self._prefix.completed_initial_activation(
                self.acceptance.binding.reference(),
                binding,
                at,
                s,
            )?;
            let customer = request_wrapper::text(binding, "customer", s)?;
            let agreement = request_wrapper::text(binding, "agreement_id", s)?;
            let held = tx
                .workflow_financial_head(
                    &key.logical_store_id,
                    &key.scope,
                    crate::store::sqlite::workflow::financial::HeadKey::AgreementActivation {
                        customer,
                        agreement,
                    },
                    s,
                )
                .await
                .map_err(read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            same(held.revision.bytes(), b"1", s)?;
            same(
                held.kind.bytes(),
                completed.head.reference().kind.as_bytes(),
                s,
            )?;
            same(held.id.bytes(), completed.head.reference().id.as_bytes(), s)?;
            same(
                held.digest.bytes(),
                completed.head.reference().digest.as_bytes(),
                s,
            )?;
            Ok(completed)
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
