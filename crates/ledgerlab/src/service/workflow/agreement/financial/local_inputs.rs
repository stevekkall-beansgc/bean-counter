//! Bounded semantic selected-body acquisition under the held local snapshot.
//! The core independently selects consumed I; queued locator DATA is not I.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, ExternalBufferLease},
    policy::workflow::transition_constructor::{
        EconomicRecordView, ExactRef, OperationKey as CoreKey, select_component_local_input_edges,
        select_local_input_edges, select_stage_local_input_edges,
    },
    validation_work::{OriginalAuditSession, RepresentationKind},
};
mod lifecycle;
pub(super) use lifecycle::{core_key_with_owner, retain_work_claim};

struct Row {
    reference: ExactRef,
    body: Option<AuditedBytes>,
    native_schema: Option<AuditedBytes>,
    lifecycle_processed: bool,
    _reference: ExternalBufferLease,
}
pub(super) struct LocalInputs {
    rows: Vec<Row>,
    _control: ExternalBufferLease,
    total: usize,
    stage_invocation_revisions: bool,
    component_records: bool,
    wrapper_inputs: bool,
}
impl LocalInputs {
    pub(super) fn new(s: &mut OriginalAuditSession) -> Result<Self> {
        let cap = 32usize;
        let control = s
            .reserve_external_payload(cap * std::mem::size_of::<Row>())
            .map_err(core_error)?;
        Ok(Self {
            rows: Vec::with_capacity(cap),
            _control: control,
            total: 0,
            stage_invocation_revisions: false,
            component_records: false,
            wrapper_inputs: false,
        })
    }
    /// Exact selected Stage inventory supports original/current revisions of
    /// the one stable Invocation identity. This is DATA representation only.
    pub(super) fn select_stage_recipe(
        &mut self,
        program: &ledgerlab_core::policy::workflow::selected_program::SelectedProgram,
    ) {
        self.stage_invocation_revisions = program.uses_stage_lifecycle_recipe();
        self.component_records = program.uses_component_lifecycle_recipe();
        self.wrapper_inputs = program.uses_shared_namespace_recipe();
    }
    pub(super) fn select_saved_registry(
        &mut self,
        registry: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        s.semantic_work()
            .scan_bytes(registry.len())
            .map_err(core_error)?;
        self.stage_invocation_revisions = registry == super::wrappers::STAGE_LIFECYCLE_REGISTRY;
        self.component_records = registry == super::wrappers::COMPONENT_LIFECYCLE_REGISTRY;
        self.wrapper_inputs = self.stage_invocation_revisions
            || self.component_records
            || registry == super::wrappers::DECLARED_LINK_REGISTRY;
        Ok(())
    }
    pub(super) fn add(&mut self, r: &ExactRef, s: &mut OriginalAuditSession) -> Result<()> {
        for old in &self.rows {
            if old.reference.kind == r.kind && old.reference.id == r.id {
                if old.reference.digest == r.digest {
                    return Ok(());
                }
                require(
                    self.stage_invocation_revisions && r.kind == "workflow-invocation",
                    "INTEGRITY_FAILURE",
                )?;
            }
        }
        let count = self
            .rows
            .len()
            .checked_add(1)
            .ok_or(ServiceError::Unavailable)?;
        let maximum = s
            .resource_limits()
            .map(|v| v.resolved_records)
            .unwrap_or(Some(4096));
        if let Some(maximum) = maximum {
            if count > maximum {
                return Err(ServiceError::ResourceLimit {
                    setting: "resolved_records",
                    limit: maximum as u64,
                    observed: count as u64,
                });
            }
        }
        if self.rows.len() == self.rows.capacity() {
            let next = self
                .rows
                .capacity()
                .checked_mul(2)
                .ok_or(ServiceError::Unavailable)?;
            let bytes = next
                .checked_mul(std::mem::size_of::<Row>())
                .ok_or(ServiceError::Unavailable)?;
            let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
            s.semantic_work().write_bytes(bytes).map_err(core_error)?;
            let mut rows = Vec::new();
            rows.try_reserve_exact(next)
                .map_err(|_| ServiceError::Unavailable)?;
            rows.append(&mut self.rows);
            self.rows = rows;
            self._control = owner;
        }
        let v = serde_json::to_value(r).map_err(|_| ServiceError::IntegrityFailure)?;
        let (reference, owner) = current::source::from_value_local(&v, s)?;
        self.rows.push(Row {
            reference,
            body: None,
            native_schema: None,
            lifecycle_processed: false,
            _reference: owner,
        });
        Ok(())
    }
    /// Actual normalized N supplied by its owning producer. Seed all pending
    /// bodies before completing their fixed edges, so no pending Ref is
    /// misclassified as historical I or fetched from a current head.
    pub(super) fn seed_pending(
        &mut self,
        key: &OperationKey,
        records: &[EconomicRecordView<'_>],
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let (ck, _key_owner) = core_key_with_owner(key, s)?;
        for record in records {
            self.add(record.reference, s)?;
            let mut index = None;
            for (i, row) in self.rows.iter().enumerate() {
                s.semantic_work()
                    .visit(ledgerlab_core::validation_work::ValidationVisit::MemberCompare)
                    .map_err(core_error)?;
                for (a, b) in [
                    (&row.reference.kind, &record.reference.kind),
                    (&row.reference.id, &record.reference.id),
                    (&row.reference.digest, &record.reference.digest),
                ] {
                    s.semantic_work()
                        .scan_bytes(a.len() + b.len())
                        .map_err(core_error)?;
                }
                if row.reference == *record.reference {
                    index = Some(i);
                    break;
                }
            }
            let index = index.ok_or(ServiceError::IntegrityFailure)?;
            if let Some(body) = self.rows[index].body.as_ref() {
                s.semantic_work()
                    .scan_bytes(body.bytes().len() + record.canonical_body.len())
                    .map_err(core_error)?;
                require(body.bytes() == record.canonical_body, "INTEGRITY_FAILURE")?;
                continue;
            }
            let parsed = s
                .parse_representation(record.canonical_body, RepresentationKind::Record)
                .map_err(core_error)?;
            let canonical = s
                .canonical_representation(parsed.value(), RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .scan_bytes(canonical.bytes().len() + record.canonical_body.len())
                .map_err(core_error)?;
            require(
                canonical.bytes() == record.canonical_body,
                "INTEGRITY_FAILURE",
            )?;
            let schema = super::request_wrapper::text(parsed.value(), "schema", s)?;
            let schema = s
                .copy_representation(schema.as_bytes(), RepresentationKind::SourceDefinition)
                .map_err(core_error)?;
            self.total = self
                .total
                .checked_add(record.canonical_body.len())
                .ok_or(ServiceError::Unavailable)?;
            if let Some(maximum) = s
                .resource_limits()
                .map(|v| v.resolved_bytes)
                .unwrap_or(Some(8388608))
            {
                if self.total > maximum {
                    return Err(ServiceError::ResourceLimit {
                        setting: "resolved_bytes",
                        limit: maximum as u64,
                        observed: self.total as u64,
                    });
                }
            }
            self.rows[index].body = Some(canonical);
            self.rows[index].native_schema = Some(schema);
        }
        for record in records {
            let edges = if self.component_records {
                select_component_local_input_edges(
                    super::wrappers::COMPONENT_LIFECYCLE_REGISTRY,
                    &ck,
                    *record,
                    s,
                )
            } else if self.stage_invocation_revisions {
                select_stage_local_input_edges(
                    super::wrappers::STAGE_LIFECYCLE_REGISTRY,
                    &ck,
                    *record,
                    s,
                )
            } else {
                select_local_input_edges(&ck, *record, s)
            }
            .map_err(core_error)?;
            for reference in edges.direct_refs() {
                self.add(reference, s)?;
            }
        }
        Ok(())
    }
    pub(super) fn add_value(&mut self, v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
        let (r, _l) = current::source::from_value_local(v, s)?;
        self.add(&r, s)
    }
    pub(super) fn payload_refs(&mut self, v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
        fn walk(
            out: &mut LocalInputs,
            v: &Value,
            depth: usize,
            s: &mut OriginalAuditSession,
        ) -> Result<()> {
            require(depth <= 64, "INVALID_SHAPE")?;
            match v {
                Value::Object(o)
                    if o.len() == 3
                        && o.contains_key("kind")
                        && o.contains_key("id")
                        && o.contains_key("digest") =>
                {
                    out.add_value(v, s)
                }
                Value::Object(o) => {
                    for (k, v) in o {
                        if k != "facts" && k != "context" {
                            walk(out, v, depth + 1, s)?;
                        }
                    }
                    Ok(())
                }
                Value::Array(a) => {
                    for v in a {
                        walk(out, v, depth + 1, s)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        walk(self, v, 0, s)
    }
    pub(super) async fn complete(
        &mut self,
        tx: &mut SqliteTx,
        key: &OperationKey,
        before: i64,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let ck = CoreKey {
            logical_store_id: key.logical_store_id.clone(),
            scope: key.scope.clone(),
            source: key.source.clone(),
            operation_id: key.operation_id.clone(),
        };
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].body.is_some() {
                i += 1;
                continue;
            }
            let r = &self.rows[i].reference;
            if self.wrapper_inputs
                && matches!(r.kind.as_str(), "workflow-operation" | "workflow-receipt")
            {
                let sequence = tx
                    .workflow_wrapper_reference_sequence(
                        r,
                        &key.logical_store_id,
                        &key.scope,
                        before,
                        s,
                    )
                    .await
                    .map_err(prefix::read_error)?
                    .ok_or(ServiceError::IntegrityFailure)?;
                let prior = sequence
                    .checked_sub(1)
                    .ok_or(ServiceError::IntegrityFailure)?;
                let origin = prefix::next(tx, key, prior, sequence, s)
                    .await?
                    .ok_or(ServiceError::IntegrityFailure)?
                    .checked_wrappers(tx, s)
                    .await?;
                let pointer = origin.wrappers().origin_pointer(s)?;
                let pointer = s
                    .parse_representation(pointer.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let selected = super::request_wrapper::field(
                    pointer.value(),
                    if r.kind == "workflow-operation" {
                        "operation_ref"
                    } else {
                        "receipt_ref"
                    },
                    s,
                )?;
                let expected = super::wire::reference(r, s)?;
                let actual = s
                    .canonical_representation(selected, RepresentationKind::Record)
                    .map_err(core_error)?;
                require(expected.bytes() == actual.bytes(), "INTEGRITY_FAILURE")?;
                let value = if r.kind == "workflow-operation" {
                    origin.wrappers().operation()
                } else {
                    origin.wrappers().receipt()
                };
                let body = s
                    .canonical_representation(value, RepresentationKind::Record)
                    .map_err(core_error)?;
                self.total = self
                    .total
                    .checked_add(body.bytes().len())
                    .ok_or(ServiceError::Unavailable)?;
                if let Some(maximum) = s
                    .resource_limits()
                    .map(|v| v.resolved_bytes)
                    .unwrap_or(Some(8388608))
                {
                    if self.total > maximum {
                        return Err(ServiceError::ResourceLimit {
                            setting: "resolved_bytes",
                            limit: maximum as u64,
                            observed: self.total as u64,
                        });
                    }
                }
                let schema = super::request_wrapper::text(value, "schema", s)?;
                let schema = s
                    .copy_representation(schema.as_bytes(), RepresentationKind::SourceDefinition)
                    .map_err(core_error)?;
                self.rows[i].body = Some(body);
                self.rows[i].native_schema = Some(schema);
                // Wrapper bodies are a closed certificate boundary, never an
                // excuse to walk arbitrary nested original facts/evidence.
                self.rows[i].lifecycle_processed = true;
                i += 1;
                continue;
            }
            prefix::verify_selected_association(tx, key, r, before, s).await?;
            let stored = tx
                .workflow_financial_document(r, &key.logical_store_id, &key.scope, before, s)
                .await
                .map_err(prefix::read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            self.total = self
                .total
                .checked_add(stored.body.bytes().len())
                .ok_or(ServiceError::Unavailable)?;
            let maximum = s
                .resource_limits()
                .map(|v| v.resolved_bytes)
                .unwrap_or(Some(8388608));
            if let Some(maximum) = maximum {
                if self.total > maximum {
                    return Err(ServiceError::ResourceLimit {
                        setting: "resolved_bytes",
                        limit: maximum as u64,
                        observed: self.total as u64,
                    });
                }
            }
            let body = s
                .copy_representation(stored.body.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            // SAME canonical/schema/domain/fullRef kernel before selecting only this
            // recipe's concrete semantic roles. No generic historical Ref closure.
            let input = EconomicRecordView {
                reference: r,
                canonical_body: body.bytes(),
            };
            let edges = if self.component_records {
                select_component_local_input_edges(
                    super::wrappers::COMPONENT_LIFECYCLE_REGISTRY,
                    &ck,
                    input,
                    s,
                )
            } else if self.stage_invocation_revisions {
                select_stage_local_input_edges(
                    super::wrappers::STAGE_LIFECYCLE_REGISTRY,
                    &ck,
                    input,
                    s,
                )
            } else {
                select_local_input_edges(&ck, input, s)
            }
            .map_err(core_error)?;
            self.rows[i].body = Some(body);
            self.rows[i].native_schema = Some(stored.schema);
            for r in edges.direct_refs() {
                self.add(r, s)?;
            }
            i += 1;
        }
        Ok(())
    }
    /// DATA selection from an actual fixed member's retained claim and Work.
    /// The child still certifies membership, Invocation lineage and authority.
    pub(super) fn stage_member_invocations(
        &self,
        program: &ledgerlab_core::policy::workflow::selected_program::SelectedProgram,
        key: &CoreKey,
        claim: &ExactRef,
        s: &mut OriginalAuditSession,
    ) -> Result<ledgerlab_core::policy::workflow::transition_constructor::WorkInvocationHeadLocators>
    {
        request_wrapper::same_text(&claim.kind, "workflow-work-claim", s)?;
        let claim_body = s
            .parse_representation(self.body(claim)?, RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(claim_body.value(), "schema", s)?,
            "ledger-workflow-work-claim/1",
            s,
        )?;
        let (work, _owner) = current::source::from_value_local(
            request_wrapper::field(claim_body.value(), "work_ref", s)?,
            s,
        )?;
        request_wrapper::same_text(&work.kind, "workflow-work", s)?;
        let work_body = s
            .parse_representation(self.body(&work)?, RepresentationKind::Record)
            .map_err(core_error)?;
        request_wrapper::same_text(
            request_wrapper::text(work_body.value(), "schema", s)?,
            "ledger-workflow-work-record/1",
            s,
        )?;
        let payload_value = request_wrapper::field(work_body.value(), "payload", s)?;
        let payload = s
            .canonical_representation(payload_value, RepresentationKind::OriginalRequest)
            .map_err(core_error)?;
        ledgerlab_core::policy::workflow::transition_constructor::select_work_invocation_head_locators(program, key, payload.bytes(), &self.views(), s).map_err(core_error)
    }
    pub(super) fn body(&self, r: &ExactRef) -> Result<&[u8]> {
        self.rows
            .iter()
            .find(|v| v.reference == *r)
            .and_then(|v| v.body.as_ref())
            .map(|v| v.bytes())
            .ok_or(ServiceError::IntegrityFailure)
    }
    pub(super) fn views(&self) -> Vec<EconomicRecordView<'_>> {
        self.iter_views().collect()
    }
    pub(super) fn owned_views(
        &self,
        extra: usize,
        s: &mut OriginalAuditSession,
    ) -> Result<(Vec<EconomicRecordView<'_>>, ExternalBufferLease)> {
        let count = self
            .rows
            .len()
            .checked_add(extra)
            .ok_or(ServiceError::Unavailable)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<EconomicRecordView<'_>>())
            .and_then(|v| v.checked_add(std::mem::size_of::<Vec<EconomicRecordView<'_>>>()))
            .ok_or(ServiceError::Unavailable)?;
        let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        let mut views = Vec::with_capacity(count);
        views.extend(self.iter_views());
        Ok((views, owner))
    }
    pub(super) fn iter_views(&self) -> impl Iterator<Item = EconomicRecordView<'_>> {
        self.rows.iter().filter_map(|r| {
            r.body.as_ref().map(|b| EconomicRecordView {
                reference: &r.reference,
                canonical_body: b.bytes(),
            })
        })
    }
}

/// Fixed subject DATA before dependency expansion. The actual original Binding
/// selects the Program; full origin/membership and constructor checks follow.
/// Only fixed request->Claim/Target/Work paths are acquired here. The actual
/// original Program selects DATA grammar; legacy requests imply no new recipe.
pub(super) async fn bootstrap_work_subject(
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<Option<ledgerlab_core::policy::workflow::transition_constructor::LocalSubjectBinding>> {
    use ledgerlab_core::policy::workflow::transition_constructor::locate_local_subject_binding;
    let schema = request_wrapper::text(payload, "schema", s)?;
    let mut claim = None;
    let mut target = None;
    let mut work = None;
    match schema {
        "ledger-workflow-link-assert/1" => {
            work = Some(
                current::source::retained_local_record(
                    tx,
                    key,
                    request_wrapper::field(payload, "child", s)?,
                    ("workflow-work", "ledger-workflow-work-record/1"),
                    before,
                    s,
                )
                .await?,
            );
        }
        "ledger-workflow-work-reverse/2" | "ledger-workflow-quantity-correction/2" => {
            work = Some(
                current::source::retained_local_record(
                    tx,
                    key,
                    request_wrapper::field(payload, "work_ref", s)?,
                    ("workflow-work", "ledger-workflow-work-record/1"),
                    before,
                    s,
                )
                .await?,
            );
        }
        "ledger-workflow-outcome/1"
        | "ledger-workflow-outcome/2"
        | "ledger-workflow-outcome-close/2" => {
            target = Some(
                current::source::retained_local_record(
                    tx,
                    key,
                    request_wrapper::field(payload, "target_ref", s)?,
                    ("workflow-target", "ledger-workflow-component-target/1"),
                    before,
                    s,
                )
                .await?,
            );
        }
        "ledger-workflow-outcome-correction/1" | "ledger-workflow-outcome-correction/2" => {
            claim = Some(
                current::source::retained_outcome_claim(
                    tx,
                    key,
                    request_wrapper::field(payload, "original_claim_ref", s)?,
                    before,
                    s,
                )
                .await?,
            );
            target = Some(
                current::source::retained_local_record(
                    tx,
                    key,
                    request_wrapper::field(
                        claim.as_ref().ok_or(ServiceError::IntegrityFailure)?.body(),
                        "target_ref",
                        s,
                    )?,
                    ("workflow-target", "ledger-workflow-component-target/1"),
                    before,
                    s,
                )
                .await?,
            );
        }
        "ledger-workflow-family-close-attest/1" => {
            let subject = request_wrapper::field(
                request_wrapper::field(payload, "attestation", s)?,
                "subject",
                s,
            )?;
            target = Some(
                current::source::retained_local_record(
                    tx,
                    key,
                    request_wrapper::field(subject, "target_ref", s)?,
                    ("workflow-target", "ledger-workflow-component-target/1"),
                    before,
                    s,
                )
                .await?,
            );
        }
        "ledger-workflow-fact-attest/1" | "ledger-workflow-cost-observation-attest/1" => {
            let subject = request_wrapper::field(
                request_wrapper::field(payload, "attestation", s)?,
                "subject",
                s,
            )?;
            match request_wrapper::text(subject, "kind", s)? {
                "work" => {}
                "outcome" if schema == "ledger-workflow-fact-attest/1" => {
                    target = Some(
                        current::source::retained_local_record(
                            tx,
                            key,
                            request_wrapper::field(subject, "target_ref", s)?,
                            ("workflow-target", "ledger-workflow-component-target/1"),
                            before,
                            s,
                        )
                        .await?,
                    );
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    }
    if let Some(target) = &target {
        work = Some(
            current::source::retained_local_record(
                tx,
                key,
                request_wrapper::field(target.body(), "work_ref", s)?,
                ("workflow-work", "ledger-workflow-work-record/1"),
                before,
                s,
            )
            .await?,
        );
    }
    let (key, _key_owner) = core_key_with_owner(key, s)?;
    let raw = s
        .canonical_representation(payload, RepresentationKind::OriginalRequest)
        .map_err(core_error)?;
    let bytes = 3 * std::mem::size_of::<EconomicRecordView<'_>>()
        + std::mem::size_of::<Vec<EconomicRecordView<'_>>>();
    let _inputs_owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    s.semantic_work().write_bytes(bytes).map_err(core_error)?;
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(3)
        .map_err(|_| ServiceError::Unavailable)?;
    for record in [claim.as_ref(), target.as_ref(), work.as_ref()]
        .into_iter()
        .flatten()
    {
        s.semantic_work()
            .visit(ledgerlab_core::validation_work::ValidationVisit::Element)
            .map_err(core_error)?;
        inputs.push(EconomicRecordView {
            reference: record.reference(),
            canonical_body: record.canonical_body(),
        });
    }
    locate_local_subject_binding(&key, raw.bytes(), &inputs, s)
        .map(Some)
        .map_err(core_error)
}
