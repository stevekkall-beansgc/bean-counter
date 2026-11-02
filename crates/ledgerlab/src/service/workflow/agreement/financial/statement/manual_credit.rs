//! Capture only exact requested, completed producer DATA. No partial capture is
//! returned before the entire same-writer prefix has finished successfully.
use super::*;
use ledgerlab_core::domain::workflow_statement::manual_credit;

pub(crate) struct CompletedManualSources {
    pub(crate) data: AuditedValue,
    pub(crate) through: i64,
}
pub(super) struct Capture<'a> {
    request: &'a Value,
    sources: Vec<AuditedValue>,
    completed: Option<AuditedValue>,
    _controls: ExternalBufferLease,
}
impl<'a> Capture<'a> {
    pub(super) fn new(request: &'a Value, s: &mut OriginalAuditSession) -> Result<Self> {
        manual_credit::request(request, s).map_err(core_error)?;
        let count = request_wrapper::field(request, "source_action_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
            .len();
        s.work()
            .arithmetic(
                Op::Multiply,
                &[&count.into(), &std::mem::size_of::<AuditedValue>().into()],
            )
            .map_err(core_error)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<AuditedValue>())
            .ok_or(ServiceError::Unavailable)?;
        let controls = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.work().write_bytes(bytes).map_err(core_error)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(count)
            .map_err(|_| ServiceError::Unavailable)?;
        s.work()
            .arithmetic(Op::Compare, &[&sources.capacity().into(), &count.into()])
            .map_err(core_error)?;
        require(sources.capacity() <= count, "INTEGRITY_FAILURE")?;
        Ok(Self {
            request,
            sources,
            completed: None,
            _controls: controls,
        })
    }
    pub(super) fn capture(
        &mut self,
        line: &Value,
        decision: &Value,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let subject_ref = request_wrapper::field(decision, "subject_ref", s)?;
        let subject = prefix._inventory.body(subject_ref, s)?;
        let subject = s
            .parse_representation(subject.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let refs = request_wrapper::field(decision, "dependency_refs", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        let per =
            std::mem::size_of::<AuditedValue>() + std::mem::size_of::<WorkflowRecordData<'_>>();
        s.work()
            .arithmetic(Op::Multiply, &[&refs.len().into(), &per.into()])
            .map_err(core_error)?;
        let bytes = refs
            .len()
            .checked_mul(per)
            .ok_or(ServiceError::Unavailable)?;
        let _controls = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.work().write_bytes(bytes).map_err(core_error)?;
        let mut bodies = Vec::new();
        bodies
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        let mut dependencies = Vec::new();
        dependencies
            .try_reserve_exact(refs.len())
            .map_err(|_| ServiceError::Unavailable)?;
        s.work()
            .arithmetic(
                Op::Compare,
                &[
                    &bodies.capacity().into(),
                    &dependencies.capacity().into(),
                    &refs.len().into(),
                ],
            )
            .map_err(core_error)?;
        require(
            bodies.capacity() <= refs.len() && dependencies.capacity() <= refs.len(),
            "INTEGRITY_FAILURE",
        )?;
        for r in refs {
            s.work().visit(Visit::Element).map_err(core_error)?;
            let body = prefix._inventory.body(r, s)?;
            bodies.push(
                s.parse_representation(body.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?,
            );
        }
        for (reference, body) in refs.iter().zip(&bodies) {
            s.work().visit(Visit::Element).map_err(core_error)?;
            dependencies.push(WorkflowRecordData {
                reference,
                body: body.value(),
            });
        }
        let packed = manual_credit::pack_source(
            manual_credit::Source {
                projected: line,
                decision,
                subject: WorkflowRecordData {
                    reference: subject_ref,
                    body: subject.value(),
                },
                dependencies: &dependencies,
            },
            s,
        )
        .map_err(core_error)?;
        s.work()
            .arithmetic(
                Op::Compare,
                &[&self.sources.len().into(), &self.sources.capacity().into()],
            )
            .map_err(core_error)?;
        require(
            self.sources.len() < self.sources.capacity(),
            "INTEGRITY_FAILURE",
        )?;
        s.work()
            .write_bytes(std::mem::size_of::<AuditedValue>())
            .map_err(core_error)?;
        self.sources.push(packed);
        Ok(())
    }
    pub(super) fn finish(
        &mut self,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        require(self.completed.is_none(), "INTEGRITY_FAILURE")?;
        let first = self.sources.first().ok_or(ServiceError::IntegrityFailure)?;
        let projected = request_wrapper::field(first.value(), "projected", s)?;
        let work_ref = request_wrapper::field(self.request, "work_ref", s)?;
        let stage_ref = request_wrapper::field(self.request, "stage_instance_ref", s)?;
        let binding_ref = request_wrapper::field(projected, "binding_ref", s)?;
        let roles_ref = request_wrapper::field(projected, "role_ref", s)?;
        let work = prefix._inventory.body(work_ref, s)?;
        let work = s
            .parse_representation(work.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let stage = prefix._inventory.body(stage_ref, s)?;
        let stage = s
            .parse_representation(stage.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let binding = prefix._inventory.body(binding_ref, s)?;
        let binding = s
            .parse_representation(binding.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        let roles = prefix._inventory.body(roles_ref, s)?;
        let roles = s
            .parse_representation(roles.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        self.completed = Some(
            manual_credit::pack_originals(
                WorkflowRecordData {
                    reference: work_ref,
                    body: work.value(),
                },
                WorkflowRecordData {
                    reference: stage_ref,
                    body: stage.value(),
                },
                WorkflowRecordData {
                    reference: binding_ref,
                    body: binding.value(),
                },
                WorkflowRecordData {
                    reference: roles_ref,
                    body: roles.value(),
                },
                &self.sources,
                s,
            )
            .map_err(core_error)?,
        );
        Ok(())
    }
    pub(super) fn take(&mut self) -> Result<AuditedValue> {
        self.completed.take().ok_or(ServiceError::IntegrityFailure)
    }
}

impl LocalWorkflowLedger {
    /// Distinct local customer-admin authority; no source grant or policy
    /// participant authority is returned. Fresh credits additionally require
    /// the existing held/open/dispatch-disabled installation admission.
    pub(crate) async fn manual_credit_authorize(
        &self,
        tx: &mut SqliteTx,
        fresh: bool,
        s: &mut OriginalAuditSession,
    ) -> Result<AuditedValue> {
        let (key, _owner) = self.statement_key(s)?;
        proof_session::with_proofs(tx, &key, |tx| {
            Box::pin(async {
                let setup = if fresh {
                    self.admin.participant_owner(&self.store, tx, &key).await?
                } else {
                    self.admin.read(&self.store, tx, &key).await?
                };
                let operator = wire::string(&setup.operator, s)?;
                let host = wire::string(&setup.host, s)?;
                let b = wire::object(
                    &[("host", host.bytes()), ("operator", operator.bytes())],
                    262144,
                    s,
                )?;
                s.parse_representation(b.bytes(), RepresentationKind::Record)
                    .map_err(core_error)
            })
        })
        .await
    }
    pub(crate) fn manual_credit_scope(
        &self,
        scope: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        s.work().visit(Visit::Constraint).map_err(core_error)?;
        require(
            scope.as_object().is_some_and(|v| v.len() == 2),
            "AUTHORITY_REQUIRED",
        )?;
        require(
            acceptance::same_recipe_text(
                request_wrapper::text(scope, "store", s)?,
                &self.admin.logical_store_id,
                s,
            )?,
            "AUTHORITY_REQUIRED",
        )?;
        let components = request_wrapper::field(scope, "scope", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(components.len() == 2, "AUTHORITY_REQUIRED")?;
        for (given, expected) in components.iter().zip(&self.admin.scope) {
            s.work().visit(Visit::Element).map_err(core_error)?;
            require(
                acceptance::same_recipe_text(
                    given.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    expected,
                    s,
                )?,
                "AUTHORITY_REQUIRED",
            )?;
        }
        Ok(())
    }
    pub(crate) async fn manual_credit_sources(
        &self,
        tx: &mut SqliteTx,
        request: &Value,
        cutoff: Option<i64>,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedManualSources> {
        manual_credit::request(request, s).map_err(core_error)?;
        let scope = request_wrapper::field(request, "workflow_scope", s)?;
        self.manual_credit_scope(scope, s)?;
        let customer = request_wrapper::text(request, "customer", s)?;
        let requested = request_wrapper::field(request, "source_action_refs", s)?;
        let (key, _owner) = self.statement_key(s)?;
        proof_session::with_proofs(tx, &key, |tx| {
            Box::pin(async {
                self.admin.read(&self.store, tx, &key).await?;
                let next = tx
                    .workflow_management_sequence(s)
                    .await
                    .map_err(prefix::read_error)?;
                s.work()
                    .arithmetic(Op::Subtract, &[&next.into(), &1i64.into()])
                    .map_err(core_error)?;
                let available = next.checked_sub(1).ok_or(ServiceError::IntegrityFailure)?;
                let through = cutoff.unwrap_or(available);
                s.work()
                    .arithmetic(
                        Op::Compare,
                        &[&through.into(), &0i64.into(), &available.into()],
                    )
                    .map_err(core_error)?;
                require(through >= 0 && through <= available, "INTEGRITY_FAILURE")?;
                let mut collector =
                    Collector::new(customer, i64::MIN, i64::MAX, Some(requested), s)?;
                collector.manual = Some(Capture::new(request, s)?);
                prefix::fold_statement_prefix(self, tx, &key, through, s, &mut collector).await?;
                let data = collector
                    .manual
                    .as_mut()
                    .ok_or(ServiceError::IntegrityFailure)?
                    .take()?;
                s.check_cancelled().map_err(core_error)?;
                Ok(CompletedManualSources { data, through })
            })
        })
        .await
    }
}
