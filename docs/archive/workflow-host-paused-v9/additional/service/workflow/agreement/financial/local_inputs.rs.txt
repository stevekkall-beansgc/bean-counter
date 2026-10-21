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
struct Row {
    reference: ExactRef,
    body: Option<AuditedBytes>,
    _reference: ExternalBufferLease,
}
pub(super) struct LocalInputs {
    rows: Vec<Row>,
    _control: ExternalBufferLease,
    total: usize,
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
        })
    }
    pub(super) fn add(&mut self, r: &ExactRef, s: &mut OriginalAuditSession) -> Result<()> {
        for old in &self.rows {
            if old.reference.kind == r.kind && old.reference.id == r.id {
                require(old.reference.digest == r.digest, "INTEGRITY_FAILURE")?;
                return Ok(());
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
            for r in edges.direct_refs() {
                self.add(r, s)?;
            }
            i += 1;
        }
        Ok(())
    }
    pub(super) fn references(&self) -> impl Iterator<Item = &ExactRef> {
        self.rows.iter().map(|r| &r.reference)
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
        self.rows
            .iter()
            .filter_map(|r| {
                r.body.as_ref().map(|b| EconomicRecordView {
                    reference: &r.reference,
                    canonical_body: b.bytes(),
                })
            })
            .collect()
    }
}
