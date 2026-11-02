//! Private, attempt-local statement collection after completed original proof.
//! Rows are DATA. Only the caller that finishes the entire held prefix may use
//! them to prepare a presentation; no partial collector escapes on failure.
use super::*;
mod manual_credit;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    domain::workflow_statement::{self, WorkflowActionData, WorkflowRecordData},
    policy::workflow::transition_constructor::EconomicCandidate,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};

pub(crate) struct Row {
    pub(crate) line: AuditedValue,
    pub(crate) accepted_at: i64,
    pub(crate) original_accepted_at: i64,
    pub(crate) sequence: i64,
    pub(crate) is_adjustment: bool,
}
pub(crate) struct CompletedRows {
    pub(crate) rows: Vec<Row>,
    pub(crate) through: i64,
    _controls: ExternalBufferLease,
}
pub(super) struct Collector<'a> {
    customer: &'a str,
    start: i64,
    end: i64,
    requested: Option<&'a Value>,
    rows: Vec<Row>,
    bytes: usize,
    manual: Option<manual_credit::Capture<'a>>,
    _controls: ExternalBufferLease,
}
fn timestamp(v: &Value, s: &mut OriginalAuditSession) -> Result<i64> {
    let text = v.as_str().ok_or(ServiceError::IntegrityFailure)?;
    // Timestamp's canonical owned spelling is at most 27 bytes.
    let _text = s.reserve_external_payload(27).map_err(core_error)?;
    ledgerlab_core::domain::Timestamp::parse_with_work(text, s.semantic_work())
        .map(|v| v.micros())
        .map_err(core_error)
}
fn same(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<bool> {
    let a = s
        .canonical_representation(a, RepresentationKind::Record)
        .map_err(core_error)?;
    let b = s
        .canonical_representation(b, RepresentationKind::Record)
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(a.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(b.bytes().len())
        .map_err(core_error)?;
    Ok(a.bytes() == b.bytes())
}
impl<'a> Collector<'a> {
    fn new(
        customer: &'a str,
        start: i64,
        end: i64,
        requested: Option<&'a Value>,
        s: &mut OriginalAuditSession,
    ) -> Result<Self> {
        // Every complete workflow line contains multiple full refs and is >64
        // bytes. Thus a 262144-byte existing close response cannot fit more.
        const CAPACITY: usize = 262144 / 64;
        s.semantic_work()
            .arithmetic(
                Op::Multiply,
                &[&CAPACITY.into(), &std::mem::size_of::<Row>().into()],
            )
            .map_err(core_error)?;
        let bytes = CAPACITY
            .checked_mul(std::mem::size_of::<Row>())
            .ok_or(ServiceError::Unavailable)?;
        let controls = s.reserve_external_payload(bytes).map_err(core_error)?;
        s.semantic_work().write_bytes(bytes).map_err(core_error)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(CAPACITY)
            .map_err(|_| ServiceError::Unavailable)?;
        s.semantic_work()
            .arithmetic(Op::Compare, &[&rows.capacity().into(), &CAPACITY.into()])
            .map_err(core_error)?;
        require(rows.capacity() <= CAPACITY, "INTEGRITY_FAILURE")?;
        Ok(Self {
            customer,
            start,
            end,
            requested,
            rows,
            bytes: 0,
            manual: None,
            _controls: controls,
        })
    }
    pub(super) fn finish(
        &mut self,
        inventory: Option<&mut prefix::FoldedInventory>,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if let Some(manual) = &mut self.manual {
            manual.finish(inventory.ok_or(ServiceError::IntegrityFailure)?, s)?;
        }
        Ok(())
    }
    pub(super) fn capture(
        &mut self,
        origin: &prefix::LocatedWrappers,
        candidate: &EconomicCandidate,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        if acceptance::same_recipe_text(
            request_wrapper::text(origin.wrappers().receipt(), "result_kind", s)?,
            "semantic_alias",
            s,
        )? {
            return Ok(());
        }
        let pointer = origin.wrappers().origin_pointer(s)?;
        let pointer = s
            .parse_representation(pointer.bytes(), RepresentationKind::Record)
            .map_err(core_error)?;
        for action in candidate.introduced() {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            if !acceptance::same_recipe_text(&action.reference().kind, "workflow-action", s)? {
                continue;
            }
            let ar = wire::reference(action.reference(), s)?;
            let ar = s
                .parse_representation(ar.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            if let Some(requested) = self.requested {
                let mut found = false;
                for r in requested.as_array().ok_or(ServiceError::IntegrityFailure)? {
                    s.work().visit(Visit::Element).map_err(core_error)?;
                    if same(r, ar.value(), s)? {
                        found = true;
                        break;
                    }
                }
                if !found {
                    continue;
                }
            }
            let ab = s
                .parse_representation(action.canonical_body(), RepresentationKind::Record)
                .map_err(core_error)?;
            let mut found = None;
            for decision in candidate.introduced() {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                if !acceptance::same_recipe_text(
                    &decision.reference().kind,
                    "workflow-decision",
                    s,
                )? {
                    continue;
                }
                let db = s
                    .parse_representation(decision.canonical_body(), RepresentationKind::Record)
                    .map_err(core_error)?;
                let refs = request_wrapper::field(db.value(), "actions_appended", s)?
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?;
                for r in refs {
                    s.semantic_work()
                        .visit(Visit::Element)
                        .map_err(core_error)?;
                    if same(r, ar.value(), s)? {
                        require(found.is_none(), "INTEGRITY_FAILURE")?;
                        found = Some(decision);
                    }
                }
            }
            let decision = found.ok_or(ServiceError::IntegrityFailure)?;
            let dr = wire::reference(decision.reference(), s)?;
            let dr = s
                .parse_representation(dr.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let db = s
                .parse_representation(decision.canonical_body(), RepresentationKind::Record)
                .map_err(core_error)?;
            let binding_ref = request_wrapper::field(db.value(), "original_terms_ref", s)?;
            let binding = prefix._inventory.body(binding_ref, s)?;
            let binding = s
                .parse_representation(binding.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            if !acceptance::same_recipe_text(
                request_wrapper::text(binding.value(), "customer", s)?,
                self.customer,
                s,
            )? {
                continue;
            }
            let accepted_at = timestamp(request_wrapper::field(db.value(), "accepted_at", s)?, s)?;
            let mut oldest = s
                .parse_representation(decision.canonical_body(), RepresentationKind::Record)
                .map_err(core_error)?;
            let mut previous_sequence = origin.wrappers().sequence();
            while let Some(prior) = request_wrapper::optional_field(oldest.value(), "prior_ref", s)?
            {
                s.semantic_work()
                    .visit(Visit::Element)
                    .map_err(core_error)?;
                let seq = prefix._inventory.introduction_sequence(prior, s)?;
                s.semantic_work()
                    .arithmetic(Op::Compare, &[&seq.into(), &previous_sequence.into()])
                    .map_err(core_error)?;
                require(seq < previous_sequence, "INTEGRITY_FAILURE")?;
                let body = prefix._inventory.body(prior, s)?;
                let prior = s
                    .parse_representation(body.bytes(), RepresentationKind::Record)
                    .map_err(core_error)?;
                require(
                    same(
                        request_wrapper::field(prior.value(), "decision_id", s)?,
                        request_wrapper::field(db.value(), "decision_id", s)?,
                        s,
                    )?,
                    "INTEGRITY_FAILURE",
                )?;
                oldest = prior;
                previous_sequence = seq;
            }
            let original_accepted_at =
                timestamp(request_wrapper::field(oldest.value(), "accepted_at", s)?, s)?;
            for value in [original_accepted_at, accepted_at] {
                s.semantic_work()
                    .arithmetic(
                        Op::Compare,
                        &[&value.into(), &self.start.into(), &self.end.into()],
                    )
                    .map_err(core_error)?;
            }
            if !(self.start <= original_accepted_at && original_accepted_at < self.end)
                && !(self.start <= accepted_at && accepted_at < self.end)
            {
                continue;
            }
            let er = request_wrapper::field(db.value(), "evaluation_ref", s)?;
            let evaluation = prefix._inventory.body(er, s)?;
            let evaluation = s
                .parse_representation(evaluation.bytes(), RepresentationKind::Record)
                .map_err(core_error)?;
            let line = workflow_statement::project_action(
                WorkflowActionData {
                    action: WorkflowRecordData {
                        reference: ar.value(),
                        body: ab.value(),
                    },
                    decision: WorkflowRecordData {
                        reference: dr.value(),
                        body: db.value(),
                    },
                    evaluation: WorkflowRecordData {
                        reference: er,
                        body: evaluation.value(),
                    },
                    binding: WorkflowRecordData {
                        reference: binding_ref,
                        body: binding.value(),
                    },
                    producer_receipt_ref: request_wrapper::field(
                        pointer.value(),
                        "receipt_ref",
                        s,
                    )?,
                    producer_key: request_wrapper::field(pointer.value(), "operation_key", s)?,
                },
                s,
            )
            .map_err(core_error)?;
            if let Some(manual) = &mut self.manual {
                manual.capture(line.value(), db.value(), prefix, s)?;
            }
            let raw = s
                .canonical_representation(line.value(), RepresentationKind::Record)
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::Add, &[&self.bytes.into(), &raw.bytes().len().into()])
                .map_err(core_error)?;
            self.bytes = self
                .bytes
                .checked_add(raw.bytes().len())
                .ok_or(ServiceError::Unavailable)?;
            s.semantic_work()
                .arithmetic(Op::Compare, &[&self.bytes.into(), &262144usize.into()])
                .map_err(core_error)?;
            require(self.bytes <= 262144, "BILLING_M5_BOUNDS")?;
            s.semantic_work()
                .arithmetic(
                    Op::Compare,
                    &[&self.rows.len().into(), &self.rows.capacity().into()],
                )
                .map_err(core_error)?;
            require(self.rows.len() < self.rows.capacity(), "BILLING_M5_BOUNDS")?;
            s.semantic_work()
                .write_bytes(std::mem::size_of::<Row>())
                .map_err(core_error)?;
            self.rows.push(Row {
                line,
                accepted_at,
                original_accepted_at,
                sequence: origin.wrappers().sequence(),
                is_adjustment: {
                    let kind = request_wrapper::text(origin.wrappers().operation(), "kind", s)?;
                    let mut adjustment = false;
                    for expected in [
                        "cost.correct",
                        "outcome.correct",
                        "quantity.correct",
                        "work.reverse",
                    ] {
                        s.semantic_work()
                            .visit(Visit::Element)
                            .map_err(core_error)?;
                        adjustment |= acceptance::same_recipe_text(kind, expected, s)?;
                    }
                    adjustment
                },
            });
        }
        Ok(())
    }
}
impl LocalWorkflowLedger {
    pub(crate) fn statement_ad_hoc_view_identity(
        &self,
        customer: &str,
        command_id: &str,
        s: &mut OriginalAuditSession,
    ) -> Result<AuditedValue> {
        let customer = wire::string(customer, s)?;
        let command_id = wire::string(command_id, s)?;
        let bytes = wire::object(
            &[
                ("command_id", command_id.bytes()),
                ("customer", customer.bytes()),
            ],
            262144,
            s,
        )?;
        s.parse_representation(bytes.bytes(), RepresentationKind::Record)
            .map_err(core_error)
    }
    /// Paid statement identity DATA using the existing canonical wire builder.
    pub(crate) fn statement_view_identity(
        &self,
        customer: &str,
        period: &Value,
        s: &mut OriginalAuditSession,
    ) -> Result<AuditedValue> {
        let customer = wire::string(customer, s)?;
        let period = s
            .canonical_representation(period, RepresentationKind::Record)
            .map_err(core_error)?;
        let bytes = wire::object(
            &[
                ("customer", customer.bytes()),
                ("period_id", period.bytes()),
            ],
            262144,
            s,
        )?;
        s.parse_representation(bytes.bytes(), RepresentationKind::Record)
            .map_err(core_error)
    }
    pub(crate) fn statement_snapshot(
        &self,
    ) -> Result<crate::service::workflow::resources::Snapshot> {
        crate::service::workflow::resources::Snapshot::capture(
            &self.admin.configuration,
            &self.admin.logical_store_id,
            &self.admin.scope,
        )
    }
    pub(crate) async fn statement_begin(
        &self,
        snapshot: &crate::service::workflow::resources::Snapshot,
        s: &mut OriginalAuditSession,
    ) -> Result<SqliteTx> {
        snapshot.begin(&self.store, s).await
    }
    fn statement_key(
        &self,
        s: &mut OriginalAuditSession,
    ) -> Result<(OperationKey, ExternalBufferLease)> {
        let mut key_bytes = std::mem::size_of::<OperationKey>();
        for text in [
            &self.admin.logical_store_id[..],
            &self.admin.scope[0],
            &self.admin.scope[1],
            "urn:internal:workflow-statement",
            "workflow-statement",
        ] {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .arithmetic(Op::Add, &[&key_bytes.into(), &text.len().into()])
                .map_err(core_error)?;
            key_bytes = key_bytes
                .checked_add(text.len())
                .ok_or(ServiceError::Unavailable)?;
            s.semantic_work()
                .scan_bytes(text.len())
                .map_err(core_error)?;
        }
        let owner = s.reserve_external_payload(key_bytes).map_err(core_error)?;
        s.semantic_work()
            .write_bytes(key_bytes)
            .map_err(core_error)?;
        let key = OperationKey {
            logical_store_id: self.admin.logical_store_id.clone(),
            scope: self.admin.scope.clone(),
            source: "urn:internal:workflow-statement".into(),
            operation_id: "workflow-statement".into(),
        };
        Ok((key, owner))
    }
    async fn statement_authorize(
        &self,
        tx: &mut SqliteTx,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let (key, _owner) = self.statement_key(s)?;
        proof_session::with_proofs(tx, &key, |tx| {
            Box::pin(async { self.admin.read(&self.store, tx, &key).await.map(|_| ()) })
        })
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn statement_rows(
        &self,
        tx: &mut SqliteTx,
        scope: &Value,
        customer: &str,
        start: i64,
        end: i64,
        cutoff: Option<i64>,
        requested: Option<&Value>,
        s: &mut OriginalAuditSession,
    ) -> Result<CompletedRows> {
        require(
            scope.as_object().is_some_and(|o| o.len() == 2),
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
        let supplied = request_wrapper::field(scope, "scope", s)?
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(supplied.len() == 2, "AUTHORITY_REQUIRED")?;
        for (given, expected) in supplied.iter().zip(&self.admin.scope) {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            require(
                acceptance::same_recipe_text(
                    given.as_str().ok_or(ServiceError::IntegrityFailure)?,
                    expected,
                    s,
                )?,
                "AUTHORITY_REQUIRED",
            )?;
        }
        let (key, _key) = self.statement_key(s)?;
        proof_session::with_proofs(tx, &key, |tx| {
            Box::pin(async {
                self.admin.read(&self.store, tx, &key).await?;
                let next = tx
                    .workflow_management_sequence(s)
                    .await
                    .map_err(prefix::read_error)?;
                s.semantic_work()
                    .arithmetic(Op::Subtract, &[&next.into(), &1i64.into()])
                    .map_err(core_error)?;
                let available = next.checked_sub(1).ok_or(ServiceError::IntegrityFailure)?;
                let through = cutoff.unwrap_or(available);
                s.semantic_work()
                    .arithmetic(
                        Op::Compare,
                        &[&through.into(), &0i64.into(), &available.into()],
                    )
                    .map_err(core_error)?;
                require(through >= 0 && through <= available, "INTEGRITY_FAILURE")?;
                let mut collector = Collector::new(customer, start, end, requested, s)?;
                prefix::fold_statement_prefix(self, tx, &key, through, s, &mut collector).await?;
                Ok(CompletedRows {
                    rows: collector.rows,
                    through,
                    _controls: collector._controls,
                })
            })
        })
        .await
    }
}

impl LocalWorkflowLedger {
    /// Exact retained statement bytes, recertified at its saved workflow cutoff.
    /// This path performs no append, current pricing, or fresh source rights.
    pub(crate) async fn read_statement(&self, raw: &[u8]) -> Result<Value> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Read {
            schema: String,
            kind: String,
            payload: Payload,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Payload {
            customer: String,
            statement_hash: String,
        }
        let v = ledgerlab_core::canonical::parse_bounded(raw, 262144)
            .map_err(|_| reject("INVALID_SHAPE"))?;
        let r: Read = serde_json::from_value(v).map_err(|_| reject("INVALID_SHAPE"))?;
        require(
            r.schema == "ledger-read/1" && r.kind == "billing.statement",
            "INVALID_SHAPE",
        )?;
        let snapshot = self.statement_snapshot()?;
        let arena = snapshot.core_arena().map_err(core_error)?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).map_err(core_error)?;
        let mut tx = self.statement_begin(&snapshot, &mut s).await?;
        let result=async {
            // Authenticate current read authority before any existence or profile distinction.
            self.statement_authorize(&mut tx,&mut s).await?;
            let legacy=tx.billing_snapshot().await.map_err(crate::service::store_error)?;
            crate::service::billing::validate_snapshot(&legacy)?;
            crate::service::billing::require_statement_read(&legacy,&r.payload.customer)?;
            let raw=tx.workflow_presentation_statement(&r.payload.customer,&r.payload.statement_hash).await.map_err(crate::service::store_error)?.ok_or_else(||reject("AUTHORITY_REQUIRED"))?;
            let v=s.parse_representation(&raw,RepresentationKind::Record).map_err(core_error)?;
            require(matches!(v.value()["schema"].as_str(),Some("ledger-billing-statement/5"|"ledger-billing-statement/6"|"ledger-billing-ad-hoc-statement-result/2"|"ledger-billing-ad-hoc-statement-result/3")),"UNSUPPORTED_SCHEMA_PROFILE")?;
            let through=v.value()["workflow_boundary"]["commit_sequence"].as_str().and_then(|v|v.parse::<i64>().ok()).ok_or(ServiceError::IntegrityFailure)?;
            let m5=v.value()["m5_high_water"].as_str().and_then(|v|v.parse::<i64>().ok()).ok_or(ServiceError::IntegrityFailure)?;
            let scope=serde_json::json!({"store":v.value()["workflow_boundary"]["store"],"scope":v.value()["workflow_boundary"]["scope"]});
            let (legacy_lines,_)=tx.m5_workflow_legacy_lines(v.value()).await.map_err(crate::service::store_error)?;
            let is_standard=matches!(v.value()["schema"].as_str(),Some("ledger-billing-statement/5"|"ledger-billing-statement/6"));
            let selected=if is_standard {
                let start=timestamp(&v.value()["start_utc"],&mut s)?;
                let end=timestamp(&v.value()["end_utc"],&mut s)?;
                crate::billing::workflow_statement::selected_lines(self,&mut tx,&scope,&r.payload.customer,start,end,Some(through),&v.value()["period_id"],&legacy_lines,Some((&r.payload.statement_hash,m5)),&mut s).await?
            } else {
                let id=v.value()["statement_id"].as_str().ok_or(ServiceError::IntegrityFailure)?;
                crate::billing::workflow_statement::selected_ad_hoc(self,&mut tx,&scope,&r.payload.customer,id,&v.value()["workflow_actions"],&legacy_lines,Some(through),Some((&r.payload.statement_hash,m5)),&mut s).await?
            };
            let manual=if matches!(v.value()["schema"].as_str(),Some("ledger-billing-statement/6"|"ledger-billing-ad-hoc-statement-result/3")) {
                let view=ledgerlab_core::domain::workflow_statement::manual_credit::presentation_view(v.value(),is_standard,&mut s).map_err(core_error)?;
                let (start,end,requested)=if is_standard{(timestamp(&v.value()["start_utc"],&mut s)?,timestamp(&v.value()["end_utc"],&mut s)?,None)}else{(i64::MIN,i64::MAX,Some(&v.value()["manual_credits"]))};
                Some(crate::billing::manual_credit::selected(self,&mut tx,&r.payload.customer,&scope,if is_standard{"standard"}else{"ad_hoc"},view.value(),&selected.lines,through,m5,start,end,requested,Some((&r.payload.statement_hash,m5)),&mut s).await?)
            }else{None};
            let (lines,totals)=if let Some(manual)=&manual{(manual.lines.value(),manual.totals.value())}else{(&selected.lines,selected.totals.value())};
            require(same(lines,&v.value()["lines"],&mut s)? && same(totals,&v.value()["totals"],&mut s)?,"INTEGRITY_FAILURE")?;
            snapshot.recheck()?;s.check_cancelled().map_err(core_error)?;
            Ok(serde_json::json!({"schema":"ledger-billing-statement-read/1","statement":v.value(),"canonical_bytes_base64":crate::service::workflow::publication::base64(&raw),"canonical_bytes_sha256":crate::service::workflow::records::raw_hash(&raw),"body_bytes":raw.len().to_string()}))
        }.await;
        tx.rollback().await.map_err(crate::service::store_error)?;
        result
    }
}
