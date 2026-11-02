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
    /// A non-admission operation keeps the activation in its actual Work
    /// producer's Chain result. The completed prefix owns both producers; a
    /// later mutable Chain head or today's agreement boundary is not consulted.
    pub(in super::super) async fn completed_work_activation(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding_ref: &ExactRef,
        binding: &Value,
        work_ref: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        let encoded = wire::reference(work_ref, s)?;
        let work_value = s
            .parse_representation(encoded.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let sequence = self
            ._inventory
            .introduction_sequence(work_value.value(), s)?;
        let work_body = self._inventory.body(work_value.value(), s)?;
        let work = s
            .parse_representation(work_body.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(work.value(), "schema", s)?,
            "ledger-workflow-work-record/1",
            s,
        )?;
        let payload = request_wrapper::field(work.value(), "payload", s)?;
        let expected_binding = wire::reference(binding_ref, s)?;
        let supplied = request_wrapper::field(payload, "binding_ref", s)?;
        let supplied = s
            .canonical_representation(supplied, RepresentationKind::Record)
            .map_err(core_error)?;
        same(supplied.bytes(), expected_binding.bytes(), s)?;
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&sequence.into(), &1i64.into()])
            .map_err(core_error)?;
        let previous = sequence
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let producer = next(tx, key, previous, sequence, s)
            .await?
            .ok_or(ServiceError::IntegrityFailure)?
            .checked_wrappers(tx, s)
            .await?;
        request_wrapper::same_text(
            request_wrapper::text(producer.wrappers().operation(), "kind", s)?,
            "work.accept",
            s,
        )?;
        require(
            request_wrapper::text(producer.wrappers().receipt(), "result_kind", s)?
                != "semantic_alias",
            "INTEGRITY_FAILURE",
        )?;
        let mut work_found = false;
        let mut chain = None;
        for value in request_wrapper::field(producer.wrappers().receipt(), "result_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let actual = s
                .canonical_representation(value, RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(actual.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(encoded.bytes().len())
                .map_err(core_error)?;
            s.semantic_work()
                .visit(Visit::MemberCompare)
                .map_err(core_error)?;
            if actual.bytes() == encoded.bytes() {
                require(!work_found, "INTEGRITY_FAILURE")?;
                work_found = true;
            }
            if super::super::acceptance::same_recipe_text(
                request_wrapper::text(value, "kind", s)?,
                "workflow-chain-state",
                s,
            )? {
                require(chain.is_none(), "INTEGRITY_FAILURE")?;
                let born = self._inventory.introduction_sequence(value, s)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&born.into(), &sequence.into()])
                    .map_err(core_error)?;
                require(born == sequence, "INTEGRITY_FAILURE")?;
                chain = Some(self._inventory.body(value, s)?);
            }
        }
        require(work_found, "INTEGRITY_FAILURE")?;
        let chain = chain.ok_or(ServiceError::IntegrityFailure)?;
        let chain = s
            .parse_representation(chain.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(chain.value(), "schema", s)?,
            "ledger-workflow-chain-state/1",
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(chain.value(), "logical_store_id", s)?,
            &key.logical_store_id,
            s,
        )?;
        let scope = request_wrapper::field(chain.value(), "scope", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&scope.len().into(), &2usize.into()])
            .map_err(core_error)?;
        require(scope.len() == 2, "INTEGRITY_FAILURE")?;
        for (value, expected) in scope.iter().zip(&key.scope) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            request_wrapper::same_text(
                value.as_str().ok_or(ServiceError::IntegrityFailure)?,
                expected,
                s,
            )?;
        }
        for field in ["customer", "chain"] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            request_wrapper::same_text(
                request_wrapper::text(chain.value(), field, s)?,
                request_wrapper::text(payload, field, s)?,
                s,
            )?;
        }
        request_wrapper::same_text(
            request_wrapper::text(payload, "customer", s)?,
            request_wrapper::text(binding, "customer", s)?,
            s,
        )?;
        let actual_binding = request_wrapper::field(chain.value(), "binding_ref", s)?;
        let actual_binding = s
            .canonical_representation(actual_binding, RepresentationKind::Record)
            .map_err(core_error)?;
        same(actual_binding.bytes(), expected_binding.bytes(), s)?;
        let selected_ref = request_wrapper::field(chain.value(), "activation_ref", s)?;
        let (selected_ref, _selected_owner) =
            current::source::from_value(selected_ref, "workflow-activation", s)?;
        let latest = self
            .activation_head(binding, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let latest = s
            .parse_representation(latest.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let revision = super::super::acceptance::counter(
            request_wrapper::text(latest.value(), "new_revision", s)?,
            s,
        )?;
        let schedule = self.activation_schedule(key, binding, revision, s)?;
        let mut selected = None;
        for transition in &schedule.transitions {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let reference = &transition.activation.reference;
            if super::super::acceptance::same_recipe_text(&reference.kind, &selected_ref.kind, s)?
                && super::super::acceptance::same_recipe_text(&reference.id, &selected_ref.id, s)?
                && super::super::acceptance::same_recipe_text(
                    &reference.digest,
                    &selected_ref.digest,
                    s,
                )?
            {
                require(selected.replace(transition).is_none(), "INTEGRITY_FAILURE")?;
            }
        }
        let selected = selected.ok_or(ServiceError::IntegrityFailure)?;
        require(
            selected.action
                != crate::store::sqlite::workflow::agreement::acceptance::ActivationAction::Revoke,
            "INTEGRITY_FAILURE",
        )?;
        for (a, b) in [
            (&selected.binding.reference.kind, &binding_ref.kind),
            (&selected.binding.reference.id, &binding_ref.id),
            (&selected.binding.reference.digest, &binding_ref.digest),
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            request_wrapper::same_text(a, b, s)?;
        }
        let _time = s
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(4 * 64 + 27)
            .map_err(core_error)?;
        let at = Timestamp::parse_with_work(
            request_wrapper::text(producer.wrappers().operation(), "accepted_at", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&selected.effective.micros().into(), &at.micros().into()],
            )
            .map_err(core_error)?;
        require(
            selected.effective.micros() <= at.micros(),
            "INTEGRITY_FAILURE",
        )?;
        completed_activation_data(selected, binding, s)
    }

    pub(in super::super) fn completed_initial_activation(
        &mut self,
        key: &OperationKey,
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
            return self.completed_current_activation(key, binding_ref, binding, at, s);
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
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn current_initial_activation(
        &mut self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        if self._prefix.is_none() {
            return local_current_activation(
                tx,
                key,
                self.acceptance.binding.reference(),
                binding,
                at,
                (before, true),
                Some(ledger),
                None,
                s,
            )
            .await;
        }
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
            let completed = self
                ._prefix
                .as_mut()
                .ok_or(ServiceError::Unavailable)?
                .completed_initial_activation(
                    key,
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

#[allow(clippy::too_many_arguments)]
async fn local_current_activation(
    tx: &mut SqliteTx,
    key: &OperationKey,
    binding_ref: &ExactRef,
    binding: &Value,
    at: &Timestamp,
    context: (i64, bool),
    ledger: Option<&LocalWorkflowLedger>,
    original_work: Option<&ExactRef>,
    s: &mut OriginalAuditSession,
) -> Result<CompletedInitialActivation> {
    let (before, current) = context;
    let customer = request_wrapper::text(binding, "customer", s)?;
    let agreement = request_wrapper::text(binding, "agreement_id", s)?;
    let rows = tx
        .workflow_agreement_activation_history(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
        )
        .await
        .map_err(crate::service::store_error)?;
    let mut initial = None;
    for row in &rows {
        if row.head.revision == 1
            && row.action
                == crate::store::sqlite::workflow::agreement::acceptance::ActivationAction::Activate
            && row.binding.id == binding_ref.id
            && row.binding.digest == binding_ref.digest
        {
            require(initial.replace(row).is_none(), "INTEGRITY_FAILURE")?;
        }
    }
    if current && rows.len() > 1 {
        return full_current_activation(
            ledger.ok_or(ServiceError::IntegrityFailure)?,
            tx,
            key,
            binding_ref,
            binding,
            at,
            before,
            s,
        )
        .await;
    }
    if !current && initial.is_none() {
        if let Some(work) = original_work {
            return full_work_activation(
                ledger.ok_or(ServiceError::IntegrityFailure)?,
                tx,
                key,
                binding_ref,
                binding,
                work,
                before,
                s,
            )
            .await;
        }
        if let Some(ledger) = ledger {
            return full_current_activation(ledger, tx, key, binding_ref, binding, at, before, s)
                .await;
        }
    }
    if current {
        require(rows.len() == 1, "UNSUPPORTED_COMPOSITION")?;
    }
    let row = initial.ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    require(
        row.head.revision == 1
            && row.action
                == crate::store::sqlite::workflow::agreement::acceptance::ActivationAction::Activate
            && row.target.is_none(),
        "AUTHORITY_REQUIRED",
    )?;
    require(
        row.effective_at_us <= at.micros() && row.sequence < before,
        "AUTHORITY_REQUIRED",
    )?;
    let expected = wire::reference(binding_ref, s)?;
    let actual = s
        .canonical_representation(
            &serde_json::to_value(&row.binding).map_err(|_| ServiceError::IntegrityFailure)?,
            RepresentationKind::Record,
        )
        .map_err(core_error)?;
    same(expected.bytes(), actual.bytes(), s)?;
    let href =
        serde_json::to_value(&row.head.transition).map_err(|_| ServiceError::IntegrityFailure)?;
    let aref = serde_json::to_value(&row.activation).map_err(|_| ServiceError::IntegrityFailure)?;
    let head = current::source::retained_record(
        tx,
        key,
        &href,
        "workflow-head-transition",
        "ledger-workflow-head-transition/1",
        before,
        s,
    )
    .await?;
    let activation = current::source::retained_record(
        tx,
        key,
        &aref,
        "workflow-activation",
        "ledger-workflow-activation-transition/1",
        before,
        s,
    )
    .await?;
    for r in [head.reference(), activation.reference()] {
        verify_selected_association(tx, key, r, before, s).await?;
    }
    request_wrapper::same_text(
        request_wrapper::text(activation.body(), "action", s)?,
        "activate",
        s,
    )?;
    request_wrapper::same_text(
        request_wrapper::text(head.body(), "new_revision", s)?,
        "1",
        s,
    )?;
    let hv = request_wrapper::field(activation.body(), "head_transition_ref", s)?;
    let h = s
        .canonical_representation(hv, RepresentationKind::Record)
        .map_err(core_error)?;
    let wanted = wire::reference(head.reference(), s)?;
    same(h.bytes(), wanted.bytes(), s)?;
    if current {
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
        for (a, b) in [
            (held.revision.bytes(), b"1".as_slice()),
            (held.kind.bytes(), head.reference().kind.as_bytes()),
            (held.id.bytes(), head.reference().id.as_bytes()),
            (held.digest.bytes(), head.reference().digest.as_bytes()),
        ] {
            same(a, b, s)?;
        }
    }
    let storage = s
        .reserve_external_payload(std::mem::size_of::<CompletedInitialActivation>())
        .map_err(core_error)?;
    let h = s
        .copy_representation(head.canonical_body(), RepresentationKind::Record)
        .map_err(core_error)?;
    let a = s
        .copy_representation(activation.canonical_body(), RepresentationKind::Record)
        .map_err(core_error)?;
    let original_key = request_wrapper::field(head.body(), "operation_key", s)?;
    let head = wire::agreement_record(
        wire::AgreementRecord::ActivationHead,
        h,
        original_key,
        customer,
        agreement,
        "1",
        s,
    )?;
    let activation = wire::agreement_record(
        wire::AgreementRecord::Activation,
        a,
        original_key,
        customer,
        agreement,
        "1",
        s,
    )?;
    for (record, expected) in [(&head, &href), (&activation, &aref)] {
        let r = wire::reference(record.reference(), s)?;
        let e = s
            .canonical_representation(expected, RepresentationKind::Record)
            .map_err(core_error)?;
        same(r.bytes(), e.bytes(), s)?;
    }
    Ok(CompletedInitialActivation {
        head,
        activation,
        _storage: storage,
    })
}

impl LocatedAppAcceptance {
    /// Unattached facts retain the released initial-Binding behavior. A later
    /// Binding must have a completed current boundary; this does not admit Work.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn original_fact_activation(
        &mut self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        require(self._prefix.is_none(), "UNSUPPORTED_SCHEMA_PROFILE")?;
        local_current_activation(
            tx,
            key,
            self.acceptance.binding.reference(),
            binding,
            at,
            (before, false),
            Some(ledger),
            None,
            s,
        )
        .await
    }

    pub(in super::super) async fn original_initial_activation(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        require(self._prefix.is_none(), "UNSUPPORTED_SCHEMA_PROFILE")?;
        local_current_activation(
            tx,
            key,
            self.acceptance.binding.reference(),
            binding,
            at,
            (before, false),
            None,
            None,
            s,
        )
        .await
    }
}

/// Chronological DATA already folded from owning original operations. The
/// leases retain the legacy schedule kernel's copied Document inputs; the
/// prefix remains the authority for their actual introductions.
pub(in super::super) struct OwnedActivationSchedule {
    pub(in super::super) transitions: Vec<activation_verify::schedule::Transition>,
    _leases: Vec<ExternalBufferLease>,
    _controls: ExternalBufferLease,
}

pub(in super::super) fn schedule_document(
    reference: &ExactRef,
    schema: &str,
    body: &[u8],
    s: &mut OriginalAuditSession,
) -> Result<(
    crate::store::sqlite::workflow::Document,
    ExternalBufferLease,
)> {
    let lengths = [
        reference.kind.len(),
        reference.id.len(),
        reference.digest.len(),
        schema.len(),
        body.len(),
    ];
    let mut bytes = std::mem::size_of::<crate::store::sqlite::workflow::Document>();
    for n in lengths {
        s.semantic_work()
            .arithmetic(Op::Add, &[&bytes.into(), &n.into()])
            .map_err(core_error)?;
        bytes = bytes.checked_add(n).ok_or(ServiceError::Unavailable)?;
    }
    let storage = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().scan_bytes(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    Ok((
        crate::store::sqlite::workflow::Document {
            reference: crate::store::sqlite::workflow::RecordRef {
                kind: reference.kind.clone(),
                id: reference.id.clone(),
                digest: reference.digest.clone(),
            },
            schema: schema.to_owned(),
            bytes: body.to_vec(),
        },
        storage,
    ))
}

impl FoldedInventory {
    /// Materialize only this already-folded agreement's revision-indexed DATA.
    /// No row or this return value asserts an origin or head is complete.
    pub(in super::super) fn activation_schedule(
        &mut self,
        key: &OperationKey,
        binding: &Value,
        through_revision: u64,
        s: &mut OriginalAuditSession,
    ) -> Result<OwnedActivationSchedule> {
        let customer_value = request_wrapper::field(binding, "customer", s)?;
        let customer_bytes = s
            .canonical_representation(customer_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let agreement_value = request_wrapper::field(binding, "agreement_id", s)?;
        let agreement_bytes = s
            .canonical_representation(agreement_value, RepresentationKind::SourceDefinition)
            .map_err(core_error)?;
        let customer = request_wrapper::text(binding, "customer", s)?;
        let agreement = request_wrapper::text(binding, "agreement_id", s)?;
        let count = usize::try_from(through_revision).map_err(|_| ServiceError::Unavailable)?;
        let per = std::mem::size_of::<activation_verify::schedule::Transition>()
            + 6 * std::mem::size_of::<ExternalBufferLease>();
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&count.into(), &per.into()])
            .map_err(core_error)?;
        let bytes = count.checked_mul(per).ok_or(ServiceError::Unavailable)?;
        let controls = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        let mut transitions = Vec::new();
        let mut leases = Vec::new();
        transitions
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&count.into(), &6usize.into()])
            .map_err(core_error)?;
        let lease_count = count.checked_mul(6).ok_or(ServiceError::Unavailable)?;
        leases
            .try_reserve_exact(lease_count)
            .map_err(|_| ServiceError::Unavailable)?;
        for (actual, paid) in [
            (transitions.capacity(), count),
            (leases.capacity(), lease_count),
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&actual.into(), &paid.into()])
                .map_err(core_error)?;
            require(actual <= paid, "PROGRAM_BUDGET_EXCEEDED")?;
        }
        for n in 1..=through_revision {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let revision = wire::counter_string(n, s)?;
            let head_key = inventory::StateKey::new(
                "agreement-activation-semantic",
                &[
                    customer_bytes.bytes(),
                    agreement_bytes.bytes(),
                    revision.bytes(),
                ],
                s,
            )?;
            let boundary_name = if n == 1 {
                "agreement-initial-activation"
            } else {
                "agreement-activation-boundary-revision"
            };
            let boundary_key = if n == 1 {
                inventory::StateKey::new(
                    boundary_name,
                    &[customer_bytes.bytes(), agreement_bytes.bytes()],
                    s,
                )?
            } else {
                inventory::StateKey::new(
                    boundary_name,
                    &[
                        customer_bytes.bytes(),
                        agreement_bytes.bytes(),
                        revision.bytes(),
                    ],
                    s,
                )?
            };
            let head_bytes = self
                ._inventory
                .head_body(&head_key, s)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let activation_bytes = self
                ._inventory
                .head_body(&boundary_key, s)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let head_value = s
                .parse_representation(head_bytes.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let activation_value = s
                .parse_representation(activation_bytes.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let key_value = request_wrapper::field(head_value.value(), "operation_key", s)?;
            let revision_text = request_wrapper::text(head_value.value(), "new_revision", s)?;
            require(
                super::super::acceptance::counter(revision_text, s)? == n,
                "INTEGRITY_FAILURE",
            )?;
            let head = wire::agreement_record(
                wire::AgreementRecord::ActivationHead,
                head_bytes,
                key_value,
                customer,
                agreement,
                revision_text,
                s,
            )?;
            let activation = wire::agreement_record(
                wire::AgreementRecord::Activation,
                activation_bytes,
                key_value,
                customer,
                agreement,
                revision_text,
                s,
            )?;
            let binding_value = request_wrapper::field(activation_value.value(), "binding_ref", s)?;
            let (binding_ref, _binding_owner) =
                current::source::from_value(binding_value, "workflow-binding", s)?;
            let binding_body = self._inventory.body(binding_value, s)?;
            let (head_doc, h) =
                schedule_document(head.reference(), head.schema(), head.bytes(), s)?;
            let (activation_doc, a) = schedule_document(
                activation.reference(),
                activation.schema(),
                activation.bytes(),
                s,
            )?;
            let (binding_doc, b) = schedule_document(
                &binding_ref,
                "ledger-workflow-binding/2",
                binding_body.bytes(),
                s,
            )?;
            // Inventory records were introduced by completed earlier owners;
            // reconstruct their DATA with the same paid semantic identities.
            self._inventory
                .compare_record(head.reference(), head.bytes(), s)?;
            self._inventory
                .compare_record(activation.reference(), activation.bytes(), s)?;
            let (transition, ref_leases) = paid_transition(
                key,
                customer,
                agreement,
                head_doc,
                activation_doc,
                binding_doc,
                s,
            )?;
            activation_verify::schedule::check_next_with_session(&transitions, &transition, s)?;
            transitions.push(transition);
            leases.extend([h, a, b]);
            leases.extend(ref_leases.into_iter().flatten());
        }
        Ok(OwnedActivationSchedule {
            transitions,
            _leases: leases,
            _controls: controls,
        })
    }
}

impl FoldedInventory {
    /// Verify stop statements only against this already-completed prefix.
    /// The caller separately checks the selected acceptance and schedule target.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn verify_stop_evidence(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        payload: &Value,
        binding: &Value,
        actor: &str,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let evidence = request_wrapper::field(payload, "evidence_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(!evidence.is_empty(), "AUTHORITY_REQUIRED")?;
        for reference in evidence {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let retained = current::source::retained_record(
                tx,
                key,
                reference,
                "workflow-evidence",
                "ledger-workflow-retained-evidence/1",
                before,
                s,
            )
            .await?;
            self._inventory
                .compare_record(retained.reference(), retained.canonical_body(), s)?;
            let body = retained.body();
            for (field, expected) in [
                ("purpose", "admission_stop"),
                ("retained_by", actor),
                ("installation_mode", "real_local"),
            ] {
                let actual = request_wrapper::text(body, field, s)?;
                require(
                    super::super::acceptance::same_recipe_text(actual, expected, s)?,
                    "AUTHORITY_REQUIRED",
                )?;
            }
            let subject = request_wrapper::field(body, "subject", s)?;
            for field in ["customer", "agreement_id"] {
                let actual = request_wrapper::text(subject, field, s)?;
                let expected = request_wrapper::text(binding, field, s)?;
                require(
                    super::super::acceptance::same_recipe_text(actual, expected, s)?,
                    "AUTHORITY_REQUIRED",
                )?;
            }
            let metadata = request_wrapper::field(body, "metadata", s)?;
            for (evidence_field, request_field) in [
                ("binding_ref", "accepted_binding_ref"),
                ("target_transition_ref", "transition_ref"),
                ("reason", "reason"),
            ] {
                let actual_value = request_wrapper::field(metadata, evidence_field, s)?;
                let expected_value = request_wrapper::field(payload, request_field, s)?;
                let actual = s
                    .canonical_representation(actual_value, RepresentationKind::Record)
                    .map_err(core_error)?;
                let expected = s
                    .canonical_representation(expected_value, RepresentationKind::Record)
                    .map_err(core_error)?;
                s.semantic_work()
                    .visit(Visit::MemberCompare)
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(actual.bytes().len())
                    .map_err(core_error)?;
                s.semantic_work()
                    .scan_bytes(expected.bytes().len())
                    .map_err(core_error)?;
                require(actual.bytes() == expected.bytes(), "AUTHORITY_REQUIRED")?;
            }
        }
        Ok(())
    }
}

impl LocatedAppAcceptance {
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn original_work_activation(
        &mut self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        work: &ExactRef,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        require(self._prefix.is_none(), "UNSUPPORTED_SCHEMA_PROFILE")?;
        local_current_activation(
            tx,
            key,
            self.acceptance.binding.reference(),
            binding,
            at,
            (before, false),
            Some(ledger),
            Some(work),
            s,
        )
        .await
    }

    /// Only a fully replayed acceptance advanced through the requested cutoff
    /// can expose its chronological schedule to a transition consumer.
    pub(in super::super) fn completed_schedule(
        &mut self,
        key: &OperationKey,
        binding: &Value,
        before: i64,
        revision: u64,
        s: &mut OriginalAuditSession,
    ) -> Result<OwnedActivationSchedule> {
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
            .map_err(core_error)?;
        let cutoff = before
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&self.through.into(), &cutoff.into()])
            .map_err(core_error)?;
        require(self.through == cutoff, "INTEGRITY_FAILURE")?;
        let binding_bytes = s
            .canonical_representation(binding, RepresentationKind::Record)
            .map_err(core_error)?;
        same(binding_bytes.bytes(), self.acceptance.binding.bytes(), s)?;
        let inventory = self
            ._prefix
            .as_mut()
            .ok_or(ServiceError::IntegrityFailure)?;
        inventory.require_record(self.acceptance.binding.reference(), s)?;
        let head = inventory
            .activation_head(binding, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let head = s
            .parse_representation(head.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let actual = super::super::acceptance::counter(
            request_wrapper::text(head.value(), "new_revision", s)?,
            s,
        )?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&revision.into(), &actual.into()])
            .map_err(core_error)?;
        require(revision == actual, "INTEGRITY_FAILURE")?;
        inventory.activation_schedule(key, binding, actual, s)
    }
    pub(in super::super) fn completed_schedule_at_cutoff(
        &mut self,
        key: &OperationKey,
        binding: &Value,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<OwnedActivationSchedule> {
        let head = self
            ._prefix
            .as_mut()
            .ok_or(ServiceError::IntegrityFailure)?
            .activation_head(binding, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let parsed = s
            .parse_representation(head.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let revision = super::super::acceptance::counter(
            request_wrapper::text(parsed.value(), "new_revision", s)?,
            s,
        )?;
        self.completed_schedule(key, binding, before, revision, s)
    }

    /// Evidence is checked only after this exact accepted Binding owner has
    /// been advanced through the original operation's exclusive cutoff.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn completed_stop_evidence(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        payload: &Value,
        binding: &Value,
        actor: &str,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let cutoff = before
            .checked_sub(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&before.into(), &1i64.into()])
            .map_err(core_error)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&self.through.into(), &cutoff.into()])
            .map_err(core_error)?;
        require(self.through == cutoff, "INTEGRITY_FAILURE")?;
        let body = s
            .canonical_representation(binding, RepresentationKind::Record)
            .map_err(core_error)?;
        same(body.bytes(), self.acceptance.binding.bytes(), s)?;
        let expected = wire::reference(self.acceptance.binding.reference(), s)?;
        let supplied_value = request_wrapper::field(payload, "accepted_binding_ref", s)?;
        let supplied = s
            .canonical_representation(supplied_value, RepresentationKind::Record)
            .map_err(core_error)?;
        same(expected.bytes(), supplied.bytes(), s)?;
        let inventory = self
            ._prefix
            .as_mut()
            .ok_or(ServiceError::IntegrityFailure)?;
        inventory.require_record(self.acceptance.binding.reference(), s)?;
        inventory
            .verify_stop_evidence(tx, key, payload, binding, actor, before, s)
            .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn full_work_activation(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    binding_ref: &ExactRef,
    binding: &Value,
    work: &ExactRef,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<CompletedInitialActivation> {
    let sequence = tx
        .workflow_selected_association_sequence(work, &key.logical_store_id, &key.scope, before, s)
        .await
        .map_err(read_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&sequence.into(), &1i64.into()])
        .map_err(core_error)?;
    let cutoff = sequence
        .checked_add(1)
        .ok_or(ServiceError::IntegrityFailure)?;
    let size = {
        let future = super::before_inputs(ledger, tx, key, None, cutoff, None, s);
        std::mem::size_of_val(&future)
    };
    let _storage = s.reserve_external_payload(size).map_err(core_error)?;
    s.semantic_work().write_bytes(size).map_err(core_error)?;
    let (mut prefix, _) =
        Box::pin(super::before_inputs(ledger, tx, key, None, cutoff, None, s)).await?;
    prefix
        .as_mut()
        .ok_or(ServiceError::IntegrityFailure)?
        .completed_work_activation(tx, key, binding_ref, binding, work, s)
        .await
}

impl OwnedActivationSchedule {
    /// The target must be an actual completed transition of the selected
    /// accepted Binding. The existing schedule check then selects whether that
    /// exact transition is the permitted pending or active stop target.
    pub(in super::super) fn require_stop_target(
        &self,
        payload: &Value,
        binding: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let target = request_wrapper::field(payload, "transition_ref", s)?;
        self.require_target_ref(target, binding, s)
    }
    pub(in super::super) fn require_target_ref(
        &self,
        target: &Value,
        binding: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let (target, _owner) = current::source::from_value(target, "workflow-activation", s)?;
        let mut found = false;
        for transition in &self.transitions {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let reference = &transition.activation.reference;
            let equal =
                super::super::acceptance::same_recipe_text(&reference.kind, &target.kind, s)?
                    && super::super::acceptance::same_recipe_text(&reference.id, &target.id, s)?
                    && super::super::acceptance::same_recipe_text(
                        &reference.digest,
                        &target.digest,
                        s,
                    )?;
            if equal {
                let actual = &transition.binding.reference;
                require(
                    super::super::acceptance::same_recipe_text(&actual.kind, &binding.kind, s)?
                        && super::super::acceptance::same_recipe_text(&actual.id, &binding.id, s)?
                        && super::super::acceptance::same_recipe_text(
                            &actual.digest,
                            &binding.digest,
                            s,
                        )?,
                    "INVALID_REFERENCE",
                )?;
                require(!found, "INTEGRITY_FAILURE")?;
                found = true;
            }
        }
        require(found, "INVALID_REFERENCE")
    }
}

/// Reconstruct the same closed transition DATA as schedule::transition using
/// the original session's representation, hash and retained reference owners.
/// The caller must already have completed the exact producer origins.
pub(in super::super) fn paid_transition(
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    head: crate::store::sqlite::workflow::Document,
    activation: crate::store::sqlite::workflow::Document,
    binding: crate::store::sqlite::workflow::Document,
    s: &mut OriginalAuditSession,
) -> Result<(
    activation_verify::schedule::Transition,
    [Option<ExternalBufferLease>; 3],
)> {
    use crate::store::sqlite::workflow::{agreement::acceptance::ActivationAction, RecordRef};
    let h = s
        .parse_representation(&head.bytes, RepresentationKind::Record)
        .map_err(core_error)?;
    let a = s
        .parse_representation(&activation.bytes, RepresentationKind::Record)
        .map_err(core_error)?;
    let b = s
        .parse_representation(&binding.bytes, RepresentationKind::Record)
        .map_err(core_error)?;
    for (body, bytes, schema) in [
        (h.value(), head.bytes.as_slice(), head.schema.as_str()),
        (
            a.value(),
            activation.bytes.as_slice(),
            activation.schema.as_str(),
        ),
        (b.value(), binding.bytes.as_slice(), binding.schema.as_str()),
    ] {
        let canonical = s
            .canonical_representation(body, RepresentationKind::Record)
            .map_err(core_error)?;
        same(canonical.bytes(), bytes, s)?;
        request_wrapper::same_text(request_wrapper::text(body, "schema", s)?, schema, s)?;
    }
    let hv = h.value();
    let av = a.value();
    let bv = b.value();
    let old = super::super::acceptance::counter(request_wrapper::text(hv, "old_revision", s)?, s)?;
    let revision =
        super::super::acceptance::counter(request_wrapper::text(hv, "new_revision", s)?, s)?;
    s.semantic_work()
        .arithmetic(Op::Add, &[&old.into(), &1u64.into()])
        .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&revision.into(), &i64::MAX.into()])
        .map_err(core_error)?;
    require(
        old.checked_add(1) == Some(revision) && revision <= i64::MAX as u64,
        "INTEGRITY_FAILURE",
    )?;
    let head_keys: &[&str] = if old == 0 {
        &[
            "schema",
            "head_kind",
            "scope",
            "customer",
            "agreement_id",
            "operation_key",
            "actor",
            "old_revision",
            "new_revision",
            "accepted_binding_ref",
            "accepted_at",
            "commit_sequence",
            "resolved_effective_time",
        ]
    } else {
        &[
            "schema",
            "head_kind",
            "scope",
            "customer",
            "agreement_id",
            "operation_key",
            "actor",
            "old_revision",
            "new_revision",
            "accepted_binding_ref",
            "accepted_at",
            "commit_sequence",
            "resolved_effective_time",
            "prior_head_ref",
        ]
    };
    request_wrapper::closed(hv, head_keys, s)?;
    let action = request_wrapper::text(av, "action", s)?;
    let act = match action {
        "activate" => ActivationAction::Activate,
        "replace" => ActivationAction::Replace,
        "revoke" => ActivationAction::Revoke,
        _ => return Err(ServiceError::IntegrityFailure),
    };
    let keys: &[&str] = if act == ActivationAction::Activate {
        &[
            "schema",
            "action",
            "head_transition_ref",
            "binding_ref",
            "effective_at",
            "commit_sequence",
            "accepted_at",
        ]
    } else {
        &[
            "schema",
            "action",
            "head_transition_ref",
            "binding_ref",
            "effective_at",
            "commit_sequence",
            "accepted_at",
            "target_transition_ref",
        ]
    };
    request_wrapper::closed(av, keys, s)?;
    request_wrapper::same_text(&head.schema, "ledger-workflow-head-transition/1", s)?;
    request_wrapper::same_text(
        &activation.schema,
        "ledger-workflow-activation-transition/1",
        s,
    )?;
    request_wrapper::same_text(&binding.schema, "ledger-workflow-binding/2", s)?;
    request_wrapper::same_text(
        request_wrapper::text(hv, "head_kind", s)?,
        "agreement_activation",
        s,
    )?;
    for body in [hv, bv] {
        request_wrapper::same_text(request_wrapper::text(body, "customer", s)?, customer, s)?;
        request_wrapper::same_text(
            request_wrapper::text(body, "agreement_id", s)?,
            agreement,
            s,
        )?;
    }
    let operation = request_wrapper::field(hv, "operation_key", s)?;
    request_wrapper::closed(
        operation,
        &["logical_store_id", "scope", "source", "operation_id"],
        s,
    )?;
    for name in ["source", "operation_id"] {
        require(
            !request_wrapper::text(operation, name, s)?.is_empty(),
            "INTEGRITY_FAILURE",
        )?;
    }
    request_wrapper::same_text(
        request_wrapper::text(operation, "logical_store_id", s)?,
        &key.logical_store_id,
        s,
    )?;
    let scope = request_wrapper::field(operation, "scope", s)?
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&scope.len().into(), &2usize.into()])
        .map_err(core_error)?;
    require(scope.len() == 2, "INTEGRITY_FAILURE")?;
    for (v, expected) in scope.iter().zip(&key.scope) {
        request_wrapper::same_text(
            v.as_str().ok_or(ServiceError::IntegrityFailure)?,
            expected,
            s,
        )?;
    }
    let left_value = request_wrapper::field(hv, "scope", s)?;
    let right_value = request_wrapper::field(operation, "scope", s)?;
    let left = s
        .canonical_representation(left_value, RepresentationKind::Record)
        .map_err(core_error)?;
    let right = s
        .canonical_representation(right_value, RepresentationKind::Record)
        .map_err(core_error)?;
    same(left.bytes(), right.bytes(), s)?;
    let exact = |v: &Value,
                 d: &crate::store::sqlite::workflow::Document,
                 s: &mut OriginalAuditSession|
     -> Result<()> {
        let (r, _lease) = current::source::from_value(v, &d.reference.kind, s)?;
        for (x, y) in [
            (&r.kind, &d.reference.kind),
            (&r.id, &d.reference.id),
            (&r.digest, &d.reference.digest),
        ] {
            request_wrapper::same_text(x, y, s)?;
        }
        Ok(())
    };
    exact(
        request_wrapper::field(hv, "accepted_binding_ref", s)?,
        &binding,
        s,
    )?;
    exact(request_wrapper::field(av, "binding_ref", s)?, &binding, s)?;
    exact(
        request_wrapper::field(av, "head_transition_ref", s)?,
        &head,
        s,
    )?;
    for (left, right) in [
        ("resolved_effective_time", "effective_at"),
        ("accepted_at", "accepted_at"),
        ("commit_sequence", "commit_sequence"),
    ] {
        request_wrapper::same_text(
            request_wrapper::text(hv, left, s)?,
            request_wrapper::text(av, right, s)?,
            s,
        )?;
    }
    let sequence =
        super::super::acceptance::counter(request_wrapper::text(hv, "commit_sequence", s)?, s)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &0u64.into()])
        .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&sequence.into(), &i64::MAX.into()])
        .map_err(core_error)?;
    require(
        sequence > 0 && sequence <= i64::MAX as u64,
        "INTEGRITY_FAILURE",
    )?;
    for (name, number) in [
        ("old_revision", old),
        ("new_revision", revision),
        ("commit_sequence", sequence),
    ] {
        let original = request_wrapper::field(hv, name, s)?;
        let original = s
            .canonical_representation(original, RepresentationKind::Record)
            .map_err(core_error)?;
        let normalized = wire::counter_string(number, s)?;
        same(original.bytes(), normalized.bytes(), s)?;
    }
    let timestamp_lease = s
        .reserve_external_payload(2 * (4 * 64 + 27))
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(2 * (4 * 64 + 27))
        .map_err(core_error)?;
    let at = Timestamp::parse_with_work(
        request_wrapper::text(hv, "accepted_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let effective = Timestamp::parse_with_work(
        request_wrapper::text(av, "effective_at", s)?,
        s.semantic_work(),
    )
    .map_err(core_error)?;
    let revision_text = request_wrapper::text(hv, "new_revision", s)?;
    for (which, document) in [
        (wire::AgreementRecord::ActivationHead, &head),
        (wire::AgreementRecord::Activation, &activation),
    ] {
        let body = s
            .copy_representation(&document.bytes, RepresentationKind::Record)
            .map_err(core_error)?;
        let produced = wire::agreement_record(
            which,
            body,
            operation,
            customer,
            agreement,
            revision_text,
            s,
        )?;
        for (x, y) in [
            (&produced.reference().kind, &document.reference.kind),
            (&produced.reference().id, &document.reference.id),
            (&produced.reference().digest, &document.reference.digest),
        ] {
            request_wrapper::same_text(x, y, s)?;
        }
    }
    let convert = |v: &Value,
                   kind: &str,
                   s: &mut OriginalAuditSession|
     -> Result<(RecordRef, ExternalBufferLease)> {
        let (r, lease) = current::source::from_value(v, kind, s)?;
        Ok((
            RecordRef {
                kind: r.kind,
                id: r.id,
                digest: r.digest,
            },
            lease,
        ))
    };
    let (prior, prior_lease) = if old == 0 {
        (None, None)
    } else {
        let (r, l) = convert(
            request_wrapper::field(hv, "prior_head_ref", s)?,
            "workflow-head-transition",
            s,
        )?;
        (Some(r), Some(l))
    };
    let (target, target_lease) = if act == ActivationAction::Activate {
        (None, None)
    } else {
        let (r, l) = convert(
            request_wrapper::field(av, "target_transition_ref", s)?,
            "workflow-activation",
            s,
        )?;
        (Some(r), Some(l))
    };
    Ok((
        activation_verify::schedule::Transition {
            head,
            activation,
            binding,
            revision: revision as i64,
            sequence: sequence as i64,
            at,
            effective,
            action: act,
            target,
            prior,
        },
        [prior_lease, target_lease, Some(timestamp_lease)],
    ))
}

impl FoldedInventory {
    pub(in super::super) fn completed_fact_activation(
        &mut self,
        key: &OperationKey,
        binding_ref: &ExactRef,
        binding: &Value,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        let head = self
            .activation_head(binding, s)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let parsed = s
            .parse_representation(head.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let revision = super::super::acceptance::counter(
            request_wrapper::text(parsed.value(), "new_revision", s)?,
            s,
        )?;
        let schedule = self.activation_schedule(key, binding, revision, s)?;
        let first = schedule
            .transitions
            .first()
            .ok_or(ServiceError::IntegrityFailure)?;
        let matched = super::super::acceptance::same_recipe_text(
            &first.binding.reference.kind,
            &binding_ref.kind,
            s,
        )? && super::super::acceptance::same_recipe_text(
            &first.binding.reference.id,
            &binding_ref.id,
            s,
        )? && super::super::acceptance::same_recipe_text(
            &first.binding.reference.digest,
            &binding_ref.digest,
            s,
        )?;
        if matched {
            s.semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&first.effective.micros().into(), &at.micros().into()],
                )
                .map_err(core_error)?;
            require(
                first.effective.micros() <= at.micros(),
                "AUTHORITY_REQUIRED",
            )?;
            return completed_activation_data(first, binding, s);
        }
        drop(schedule);
        self.completed_current_activation(key, binding_ref, binding, at, s)
    }

    /// Select the completed unsuperseded boundary at the actual operation time.
    /// It never falls back to a different Binding after expiration or revoke.
    pub(in super::super) fn completed_current_activation(
        &mut self,
        key: &OperationKey,
        binding_ref: &ExactRef,
        binding: &Value,
        at: &Timestamp,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        let latest = self
            .activation_head(binding, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let latest = s
            .parse_representation(latest.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let revision = super::super::acceptance::counter(
            request_wrapper::text(latest.value(), "new_revision", s)?,
            s,
        )?;
        let schedule = self.activation_schedule(key, binding, revision, s)?;
        let selected = activation_verify::schedule::paid_boundary_at(&schedule.transitions, at, s)?
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        require(
            selected.action
                != crate::store::sqlite::workflow::agreement::acceptance::ActivationAction::Revoke,
            "AUTHORITY_REQUIRED",
        )?;
        for (a, b) in [
            (&selected.binding.reference.kind, &binding_ref.kind),
            (&selected.binding.reference.id, &binding_ref.id),
            (&selected.binding.reference.digest, &binding_ref.digest),
        ] {
            require(
                super::super::acceptance::same_recipe_text(a, b, s)?,
                "AUTHORITY_REQUIRED",
            )?;
        }
        let _time = s
            .reserve_external_payload(2 * (4 * 64 + 27))
            .map_err(core_error)?;
        s.semantic_work()
            .write_bytes(2 * (4 * 64 + 27))
            .map_err(core_error)?;
        let starts = Timestamp::parse_with_work(
            request_wrapper::text(binding, "starts_at", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        let ends = request_wrapper::optional_field(binding, "ends_at", s)?
            .map(|v| {
                Timestamp::parse_with_work(
                    v.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    s.semantic_work(),
                )
                .map_err(core_error)
            })
            .transpose()?;
        activation_verify::schedule::binding_interval_with_work(
            at,
            &starts,
            ends.as_ref(),
            s.semantic_work(),
        )
        .map_err(|e| match e {
            ServiceError::IntegrityFailure => reject("AUTHORITY_REQUIRED"),
            other => other,
        })?;
        completed_activation_data(selected, binding, s)
    }
}

fn completed_activation_data(
    selected: &activation_verify::schedule::Transition,
    binding: &Value,
    s: &mut OriginalAuditSession,
) -> Result<CompletedInitialActivation> {
    let body = s
        .copy_representation(&selected.head.bytes, RepresentationKind::Record)
        .map_err(core_error)?;
    let parsed = s
        .parse_representation(body.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let operation = request_wrapper::field(parsed.value(), "operation_key", s)?;
    let number = request_wrapper::text(parsed.value(), "new_revision", s)?;
    let customer = request_wrapper::text(binding, "customer", s)?;
    let agreement = request_wrapper::text(binding, "agreement_id", s)?;
    let head = wire::agreement_record(
        wire::AgreementRecord::ActivationHead,
        body,
        operation,
        customer,
        agreement,
        number,
        s,
    )?;
    let body = s
        .copy_representation(&selected.activation.bytes, RepresentationKind::Record)
        .map_err(core_error)?;
    let activation = wire::agreement_record(
        wire::AgreementRecord::Activation,
        body,
        operation,
        customer,
        agreement,
        number,
        s,
    )?;
    let storage = s
        .reserve_external_payload(std::mem::size_of::<CompletedInitialActivation>())
        .map_err(core_error)?;
    Ok(CompletedInitialActivation {
        head,
        activation,
        _storage: storage,
    })
}

impl LocatedAppAcceptance {
    /// Preserve the initial path; later heads use the complete original owner.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) async fn current_activation(
        &mut self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        key: &OperationKey,
        binding: &Value,
        at: &Timestamp,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedInitialActivation> {
        self.current_initial_activation(ledger, tx, key, binding, at, before, s)
            .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn full_current_activation(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    binding_ref: &ExactRef,
    binding: &Value,
    at: &Timestamp,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<CompletedInitialActivation> {
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
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let wanted = wire::reference(binding_ref, s)?;
    let wanted = s
        .parse_representation(wanted.bytes(), RepresentationKind::Record)
        .map_err(core_error)?;
    let mut owner =
        super::acceptance_for_original(ledger, tx, key, wanted.value(), before, s).await?;
    let setup = ledger.admin.read(&ledger.store, tx, key).await?;
    owner
        .current_sources(tx, key, (ledger, &setup), &[], before, s)
        .await?;
    let revision_text =
        std::str::from_utf8(held.revision.bytes()).map_err(|_| ServiceError::IntegrityFailure)?;
    let revision = super::super::acceptance::counter(revision_text, s)?;
    let canonical_revision = wire::counter_string(revision, s)?;
    let encoded_revision = wire::string(revision_text, s)?;
    same(canonical_revision.bytes(), encoded_revision.bytes(), s)?;
    let schedule = owner.completed_schedule(key, binding, before, revision, s)?;
    let latest = schedule
        .transitions
        .last()
        .ok_or(ServiceError::IntegrityFailure)?;
    for (a, b) in [
        (held.kind.bytes(), latest.head.reference.kind.as_bytes()),
        (held.id.bytes(), latest.head.reference.id.as_bytes()),
        (held.digest.bytes(), latest.head.reference.digest.as_bytes()),
    ] {
        same(a, b, s)?;
    }
    drop(schedule);
    owner
        ._prefix
        .as_mut()
        .ok_or(ServiceError::IntegrityFailure)?
        .completed_current_activation(key, binding_ref, binding, at, s)
}

#[cfg(test)]
mod paid_transition_tests {
    use super::*;
    use ledgerlab_core::{canonical::audit_arena::AuditArena, validation_work::ResourceLimits};
    #[test]
    fn paid_reconstruction_matches_existing_original_semantic_records() {
        let f: Value = serde_json::from_slice(include_bytes!(
            "../../testdata/r1-binding-scope-independent.json"
        ))
        .unwrap();
        let body = |name: &str| {
            f["vectors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .unwrap()["body"]
                .clone()
        };
        let mut binding = body("A.binding");
        binding["schema"] = serde_json::json!("ledger-workflow-binding/2");
        let binding =
            super::super::super::super::records::source_document("workflow-binding", binding)
                .unwrap();
        let mut h = body("A.activation-head-v1");
        h["accepted_binding_ref"] = serde_json::json!(binding.reference);
        let key = OperationKey::parse(
            &super::super::super::super::records::bytes(&h["operation_key"]).unwrap(),
        )
        .unwrap();
        let head = super::super::super::super::record::semantic_document(
            "workflow-head-transition",
            &key,
            "customer-A",
            "agreement-A",
            1,
            h,
        )
        .unwrap();
        let mut a = body("A.activation-v1");
        a["binding_ref"] = serde_json::json!(binding.reference);
        a["head_transition_ref"] = serde_json::json!(head.reference);
        let activation = super::super::super::super::record::semantic_document(
            "workflow-activation",
            &key,
            "customer-A",
            "agreement-A",
            1,
            a,
        )
        .unwrap();
        let copy = |d: &crate::store::sqlite::workflow::Document| {
            crate::store::sqlite::workflow::Document {
                reference: d.reference.clone(),
                schema: d.schema.clone(),
                bytes: d.bytes.clone(),
            }
        };
        let wrong_scope = OperationKey {
            scope: ["foreign".into(), "scope".into()],
            ..key.clone()
        };
        let arena = AuditArena::with_resource_limits(ResourceLimits::unlimited()).unwrap();
        let mut denied = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        assert!(paid_transition(
            &wrong_scope,
            "customer-A",
            "agreement-A",
            copy(&head),
            copy(&activation),
            copy(&binding),
            &mut denied
        )
        .is_err());
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let expected = activation.reference.clone();
        let (transition, _leases) = paid_transition(
            &key,
            "customer-A",
            "agreement-A",
            head,
            activation,
            binding,
            &mut session,
        )
        .unwrap();
        assert_eq!(transition.activation.reference, expected);
        assert_eq!(transition.revision, 1);
        assert!(session.usage().semantic_work > 0);
    }
}
