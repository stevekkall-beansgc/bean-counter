//! Bounded semantic selected-body acquisition under the held local snapshot.
//! The core independently selects consumed I; queued locator DATA is not I.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, ExternalBufferLease},
    policy::workflow::transition_constructor::{
        select_local_input_edges, EconomicRecordView, ExactRef, OperationKey as CoreKey,
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
            wrapper_inputs: false,
        })
    }
    /// Exact selected Stage inventory supports original/current revisions of
    /// the one stable Invocation identity. This is DATA representation only.
    pub(super) fn select_stage_recipe(&mut self, program: &ledgerlab_core::policy::workflow::selected_program::SelectedProgram) {
        self.stage_invocation_revisions = program.uses_stage_lifecycle_recipe();
        self.wrapper_inputs = program.uses_shared_namespace_recipe();
    }
    pub(super) fn select_saved_registry(&mut self, registry: &str, s: &mut OriginalAuditSession) -> Result<()> {
        s.semantic_work().scan_bytes(registry.len()).map_err(core_error)?;
        self.stage_invocation_revisions = registry == super::wrappers::STAGE_LIFECYCLE_REGISTRY;
        self.wrapper_inputs = self.stage_invocation_revisions || registry == super::wrappers::DECLARED_LINK_REGISTRY;
        Ok(())
    }
    pub(super) fn add(&mut self, r: &ExactRef, s: &mut OriginalAuditSession) -> Result<()> {
        for old in &self.rows {
            if old.reference.kind == r.kind && old.reference.id == r.id {
                if old.reference.digest == r.digest { return Ok(()); }
                require(self.stage_invocation_revisions && r.kind == "workflow-invocation", "INTEGRITY_FAILURE")?;
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
            if self.wrapper_inputs && matches!(r.kind.as_str(), "workflow-operation" | "workflow-receipt") {
                let sequence = tx.workflow_wrapper_reference_sequence(r, &key.logical_store_id, &key.scope, before, s).await.map_err(prefix::read_error)?.ok_or(ServiceError::IntegrityFailure)?;
                let prior = sequence.checked_sub(1).ok_or(ServiceError::IntegrityFailure)?;
                let origin = prefix::next(tx, key, prior, sequence, s).await?.ok_or(ServiceError::IntegrityFailure)?.checked_wrappers(tx, s).await?;
                let pointer = origin.wrappers().origin_pointer(s)?;
                let pointer = s.parse_representation(pointer.bytes(), RepresentationKind::Record).map_err(core_error)?;
                let selected = super::request_wrapper::field(pointer.value(), if r.kind == "workflow-operation" { "operation_ref" } else { "receipt_ref" }, s)?;
                let expected = super::wire::reference(r,s)?;
                let actual = s.canonical_representation(selected,RepresentationKind::Record).map_err(core_error)?;
                require(expected.bytes()==actual.bytes(),"INTEGRITY_FAILURE")?;
                let value = if r.kind == "workflow-operation" {origin.wrappers().operation()} else {origin.wrappers().receipt()};
                let body=s.canonical_representation(value,RepresentationKind::Record).map_err(core_error)?;
                self.total=self.total.checked_add(body.bytes().len()).ok_or(ServiceError::Unavailable)?;
                if let Some(maximum)=s.resource_limits().map(|v|v.resolved_bytes).unwrap_or(Some(8388608)) {
                    if self.total>maximum {return Err(ServiceError::ResourceLimit{setting:"resolved_bytes",limit:maximum as u64,observed:self.total as u64});}
                }
                let schema=super::request_wrapper::text(value,"schema",s)?;
                let schema=s.copy_representation(schema.as_bytes(),RepresentationKind::SourceDefinition).map_err(core_error)?;
                self.rows[i].body=Some(body);self.rows[i].native_schema=Some(schema);
                // Wrapper bodies are a closed certificate boundary, never an
                // excuse to walk arbitrary nested original facts/evidence.
                self.rows[i].lifecycle_processed=true;
                i+=1;continue;
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
            let edges = select_local_input_edges(
                &ck,
                EconomicRecordView {
                    reference: r,
                    canonical_body: body.bytes(),
                },
                s,
            )
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
    pub(super) fn owned_views(&self,extra:usize,s:&mut OriginalAuditSession)->Result<(Vec<EconomicRecordView<'_>>,ExternalBufferLease)>{
        let count=self.rows.len().checked_add(extra).ok_or(ServiceError::Unavailable)?;
        let bytes=count.checked_mul(std::mem::size_of::<EconomicRecordView<'_>>()).and_then(|v|v.checked_add(std::mem::size_of::<Vec<EconomicRecordView<'_>>>())).ok_or(ServiceError::Unavailable)?;
        let owner=s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        let mut views=Vec::with_capacity(count);
        views.extend(self.iter_views());
        Ok((views,owner))
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
