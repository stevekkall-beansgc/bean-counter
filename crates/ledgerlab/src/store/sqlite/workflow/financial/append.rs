//! Borrowed mechanical write operands from the private coordinator. Owning
//! artifacts and their resource session remain alive in that coordinator.
use super::effects::same_text;
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{EconomicCandidate, ExactRef};

pub(super) const MATCH_DOCUMENT: &str = "SELECT kind=? AND digest=? AND schema_name=? AND canonical_bytes=? FROM workflow_documents WHERE id=?";
pub(super) const INSERT_DOCUMENT: &str = "INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) VALUES(?,?,?,?,?) ON CONFLICT(id) DO NOTHING";
pub(super) const INSERT_OPERATION: &str = "INSERT INTO workflow_operations(identity_key,logical_store_id,tenant,environment,source,operation_id,operation_record_id,operation_digest,operation_bytes,normalized_request_hash,manifest_id,manifest_digest,manifest_bytes,receipt_id,receipt_digest,receipt_bytes,publication_id,plan_id,commit_sequence,accepted_at_us,operation_kind) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,NULL,NULL,?,?,?)";

pub(crate) struct FinancialDocument<'a> {
    pub reference: &'a ExactRef,
    pub schema: &'a str,
    pub canonical_bytes: &'a [u8],
}
/// One production binding path shared by append and its physical table
/// witness, so the test exercises the actual VALUES parameter list.
pub(super) fn insert_document<'a>(
    document: &'a FinancialDocument<'_>,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    sqlx::query(INSERT_DOCUMENT)
        .bind(&document.reference.kind)
        .bind(&document.reference.id)
        .bind(&document.reference.digest)
        .bind(document.schema)
        .bind(document.canonical_bytes)
}
pub(crate) struct FinancialJournal<'a> {
    pub identity: &'a [u8],
    pub kind: &'a str,
    pub operation_ref: &'a ExactRef,
    pub operation_bytes: &'a [u8],
    pub request_hash: &'a str,
    pub manifest_ref: &'a ExactRef,
    pub manifest_bytes: &'a [u8],
    pub receipt_ref: &'a ExactRef,
    pub receipt_bytes: &'a [u8],
    /// Borrowed exact candidate closure, excluding this new operation. The
    /// coordinator emits its canonical union with operation_ref in the manifest;
    /// storage adds that same Ref without a second parsed manifest tree.
    pub members: &'a [&'a ExactRef],
    pub sequence: i64,
    pub accepted_at_us: i64,
}
/// Neither a financial authority nor a public append surface. The service's
/// private prepared commit must own the authenticated checked snapshot, the
/// complete core candidate and all of these original/canonical byte artifacts.
pub(crate) struct FinancialAppend<'a> {
    pub candidate: &'a EconomicCandidate,
    pub documents: &'a [FinancialDocument<'a>],
    pub journal: FinancialJournal<'a>,
}
/// Borrowed key columns for the SAME administrative/financial journal rows.
/// This DATA view owns no authority, selection or commit capability.
#[derive(Clone, Copy)]
pub(super) struct JournalKey<'a> {
    pub logical_store_id: &'a str,
    pub scope: &'a [String; 2],
    pub source: &'a str,
    pub operation_id: &'a str,
}
pub(super) struct JournalDelta {
    before: super::super::capacity::WorkflowMeter,
    additional: i64,
}
fn extra(
    total: i64,
    amount: usize,
    session: &mut OriginalAuditSession,
) -> Result<i64, FinancialReadError> {
    session.semantic_work().arithmetic(
        ArithmeticOperation::Compare,
        &[&amount.into(), &(i64::MAX as u64).into()],
    )?;
    let amount = i64::try_from(amount).map_err(|_| StoreError::BillingHistoryLimit)?;
    session
        .semantic_work()
        .arithmetic(ArithmeticOperation::Add, &[&total.into(), &amount.into()])?;
    total
        .checked_add(amount)
        .ok_or_else(|| StoreError::BillingHistoryLimit.into())
}
fn at_most(
    length: usize,
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    session.semantic_work().arithmetic(
        ArithmeticOperation::Compare,
        &[&length.into(), &maximum.into()],
    )?;
    Ok(length <= maximum)
}
fn payload_bound(
    raw: &[u8],
    maximum: usize,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    Ok(!raw.is_empty() && at_most(raw.len(), maximum, session)?)
}
fn journal_framing(
    documents: &[FinancialDocument<'_>],
    o: &FinancialJournal<'_>,
    session: &mut OriginalAuditSession,
) -> Result<bool, FinancialReadError> {
    session.semantic_work().visit(ValidationVisit::Constraint)?;
    let mut kind = false;
    for allowed in [
        "agreement.accept",
        "agreement.activate",
        "work.accept",
        "work.reverse",
        "quantity.correct",
        "outcome.submit",
        "outcome.correct",
        "outcome.close",
        "link.assert",
        "evidence.retain",
        "authority.change",
        "invocation.authorize",
        "invocation.close",
        "cost.finalize",
        "cost.correct",
    ] {
        if same_text(o.kind, allowed, session)? {
            kind = true;
            break;
        }
    }
    Ok(kind
        && same_text(&o.operation_ref.kind, "workflow-operation", session)?
        && same_text(&o.manifest_ref.kind, "workflow-manifest", session)?
        && same_text(&o.receipt_ref.kind, "workflow-receipt", session)?
        && payload_bound(o.identity, 262144, session)?
        && payload_bound(o.operation_bytes, 8388608, session)?
        && payload_bound(o.manifest_bytes, 4194304, session)?
        && payload_bound(o.receipt_bytes, 1048576, session)?
        && at_most(documents.len(), 4095, session)?
        && at_most(o.members.len(), 4095, session)?)
}
impl SqliteTx {
    /// Same writer/poison/unknown-commit boundary as the administrative journal.
    /// No canonical payload is copied into an ordinary host Vec by this method.
    pub(crate) async fn workflow_financial_append(
        &mut self,
        prepared: &FinancialAppend<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        if self.failed {
            return Err(StoreError::Integrity("poisoned financial transaction").into());
        }
        self.failed = true;
        let result = crate::store::sqlite::workflow::until(
            self.workflow_deadline(),
            self.financial_append_inner(prepared, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            // An Ok SQL result cannot erase a failed/cancelled child, a caught
            // proof error or a worker payload still awaiting finalization.
            // The executing guard uses this SAME original session lifecycle.
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("financial completion retains poisoned work").into(),
                );
            }
            self.failed = false;
        }
        result
    }
    async fn financial_append_inner(
        &mut self,
        p: &FinancialAppend<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let key = p.candidate.key();
        let o = &p.journal;
        // The financial entry still excludes administrative acceptance; that
        // consumer supplies its own private preparation and mechanical effect.
        if !matches!(
            o.kind,
            "work.accept"
                | "work.reverse"
                | "quantity.correct"
                | "outcome.submit"
                | "outcome.correct"
                | "outcome.close"
                | "link.assert"
                | "evidence.retain"
                | "authority.change"
                | "invocation.authorize"
                | "invocation.close"
                | "cost.finalize"
                | "cost.correct"
        ) {
            return Err(StoreError::Integrity("financial prepared kind").into());
        }
        let delta = self
            .append_journal_rows(
                JournalKey {
                    logical_store_id: &key.logical_store_id,
                    scope: &key.scope,
                    source: &key.source,
                    operation_id: &key.operation_id,
                },
                p.documents,
                o,
                session,
            )
            .await?;
        for r in p.candidate.consumed_refs() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let bound = [
                o.identity,
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
            ];
            let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members WHERE identity_key=? AND kind=? AND id=? AND digest=?)")
                .bind(o.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).fetch_one(self.conn()).await?;
            self.financial_finish_io(session).await?;
            if !present {
                return Err(StoreError::Integrity(
                    "financial consumed reference absent from manifest",
                )
                .into());
            }
        }
        for d in p.candidate.introduced() {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let r = d.reference();
            let bound = [
                o.identity,
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
                d.schema().as_bytes(),
                d.canonical_body(),
            ];
            let payload = io::binding_payload(&bound, &[], 0, 4194304, session)?;
            self.financial_reserve_io(session, payload)?;
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents d JOIN workflow_manifest_members m ON m.kind=d.kind AND m.id=d.id AND m.digest=d.digest WHERE m.identity_key=? AND d.kind=? AND d.id=? AND d.digest=? AND d.schema_name=? AND d.canonical_bytes=?)")
                .bind(o.identity).bind(&r.kind).bind(&r.id).bind(&r.digest).bind(d.schema()).bind(d.canonical_body()).fetch_one(self.conn()).await?;
            self.financial_finish_io(session).await?;
            if !present {
                return Err(
                    StoreError::Integrity("financial introduced result absent or changed").into(),
                );
            }
        }
        self.apply_financial_effects(p.candidate, o.identity, o.sequence, session)
            .await?;
        self.finish_journal_rows(o, delta, session).await
    }
    pub(super) async fn append_journal_rows(
        &mut self,
        key: JournalKey<'_>,
        documents: &[FinancialDocument<'_>],
        o: &FinancialJournal<'_>,
        session: &mut OriginalAuditSession,
    ) -> Result<JournalDelta, FinancialReadError> {
        let payload = io::binding_payload(&[], &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().write_bytes(std::mem::size_of::<i64>())?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        self.financial_finish_io(session).await?;
        if !resource_compare(session, version, 13)?.is_eq()
            && !resource_compare(session, version, 14)?.is_eq()
            && !resource_compare(session, version, 15)?.is_eq()
            && !resource_compare(session, version, 16)?.is_eq()
        {
            return Err(StoreError::BillingUpgradeRequired.into());
        }
        if !journal_framing(documents, o, session)? {
            return Err(StoreError::Integrity("financial prepared framing").into());
        }
        let payload = io::binding_payload(&[o.identity], &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE identity_key=?)",
        )
        .bind(o.identity)
        .fetch_one(self.conn())
        .await?;
        self.financial_finish_io(session).await?;
        if occupied {
            return Err(StoreError::ExpectedCurrent.into());
        }
        let mut additional = 0;
        for bytes in [
            o.identity,
            o.operation_bytes,
            o.manifest_bytes,
            o.receipt_bytes,
        ] {
            additional = extra(additional, bytes.len(), session)?;
        }
        for d in documents {
            session.semantic_work().visit(ValidationVisit::Element)?;
            if same_text(&d.reference.kind, "workflow-operation", session)?
                || !payload_bound(d.canonical_bytes, 4194304, session)?
            {
                return Err(StoreError::Integrity("financial prepared document bounds").into());
            }
            // SQLite performs the exact comparison; no unleased multi-MiB
            // retained BLOB is fetched into Rust merely to compare it.
            let bindings = [
                d.reference.kind.as_bytes(),
                d.reference.digest.as_bytes(),
                d.schema.as_bytes(),
                d.canonical_bytes,
                d.reference.id.as_bytes(),
            ];
            // MATCH materializes at most the existing4MiB canonical document,
            // returns only a boolean and has no request-sized packed result.
            let payload = io::binding_payload(&bindings, &[], 0, 4194304, session)?;
            self.financial_reserve_io(session, payload)?;
            let retained: Option<bool> = sqlx::query_scalar(MATCH_DOCUMENT)
                .bind(&d.reference.kind)
                .bind(&d.reference.digest)
                .bind(d.schema)
                .bind(d.canonical_bytes)
                .bind(&d.reference.id)
                .fetch_optional(self.conn())
                .await?;
            self.financial_finish_io(session).await?;
            match retained {
                Some(false) => {
                    return Err(
                        StoreError::Integrity("immutable financial document conflict").into(),
                    );
                }
                Some(true) => {}
                None => {
                    additional = extra(additional, d.canonical_bytes.len(), session)?;
                }
            }
        }
        let before = self
            .accounted_capacity_check(1, additional, session)
            .await?;
        session.semantic_work().arithmetic(
            ArithmeticOperation::Add,
            &[&before.operations.into(), &1i64.into()],
        )?;
        let next = before
            .operations
            .checked_add(1)
            .ok_or(StoreError::BillingHistoryLimit)?;
        if !resource_compare(session, next, o.sequence)?.is_eq() {
            return Err(StoreError::Integrity("financial journal sequence gap").into());
        }
        for d in documents {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let r = d.reference;
            let bound = [
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
                d.schema.as_bytes(),
                d.canonical_bytes,
            ];
            // Exact three MakeRecord sites in the pinned direct VALUES VM:
            // full row, (kind,id,digest,rowid), (id,rowid). SCopy is shallow.
            let packed = [
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
                d.schema.as_bytes(),
                d.canonical_bytes,
                r.kind.as_bytes(),
                r.id.as_bytes(),
                r.digest.as_bytes(),
                r.id.as_bytes(),
            ];
            let payload = io::binding_payload(&bound, &packed, 2, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            insert_document(d).execute(self.conn()).await?;
            self.financial_finish_io(session).await?;
        }
        let bound = [
            o.identity,
            key.logical_store_id.as_bytes(),
            key.scope[0].as_bytes(),
            key.scope[1].as_bytes(),
            key.source.as_bytes(),
            key.operation_id.as_bytes(),
            o.operation_ref.id.as_bytes(),
            o.operation_ref.digest.as_bytes(),
            o.operation_bytes,
            o.request_hash.as_bytes(),
            o.manifest_ref.id.as_bytes(),
            o.manifest_ref.digest.as_bytes(),
            o.manifest_bytes,
            o.receipt_ref.id.as_bytes(),
            o.receipt_ref.digest.as_bytes(),
            o.receipt_bytes,
            o.kind.as_bytes(),
        ];
        // The full row plus the six unique-index records from the exact pinned
        // INSERT VM. Main row has two integers; index records add seven.
        let packed = [
            o.identity,
            key.logical_store_id.as_bytes(),
            key.scope[0].as_bytes(),
            key.scope[1].as_bytes(),
            key.source.as_bytes(),
            key.operation_id.as_bytes(),
            o.operation_ref.id.as_bytes(),
            o.operation_ref.digest.as_bytes(),
            o.operation_bytes,
            o.request_hash.as_bytes(),
            o.manifest_ref.id.as_bytes(),
            o.manifest_ref.digest.as_bytes(),
            o.manifest_bytes,
            o.receipt_ref.id.as_bytes(),
            o.receipt_ref.digest.as_bytes(),
            o.receipt_bytes,
            o.kind.as_bytes(),
            key.logical_store_id.as_bytes(),
            key.scope[0].as_bytes(),
            key.scope[1].as_bytes(),
            key.source.as_bytes(),
            key.operation_id.as_bytes(),
            o.receipt_ref.id.as_bytes(),
            o.manifest_ref.id.as_bytes(),
            o.operation_ref.id.as_bytes(),
            o.identity,
        ];
        let payload = io::binding_payload(&bound, &packed, 9, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        sqlx::query(INSERT_OPERATION)
            .bind(o.identity)
            .bind(&key.logical_store_id)
            .bind(&key.scope[0])
            .bind(&key.scope[1])
            .bind(&key.source)
            .bind(&key.operation_id)
            .bind(&o.operation_ref.id)
            .bind(&o.operation_ref.digest)
            .bind(o.operation_bytes)
            .bind(o.request_hash)
            .bind(&o.manifest_ref.id)
            .bind(&o.manifest_ref.digest)
            .bind(o.manifest_bytes)
            .bind(&o.receipt_ref.id)
            .bind(&o.receipt_ref.digest)
            .bind(o.receipt_bytes)
            .bind(o.sequence)
            .bind(o.accepted_at_us)
            .bind(o.kind)
            .execute(self.conn())
            .await?;
        self.financial_finish_io(session).await?;
        let mut own_operation_members = 0;
        for reference in o
            .members
            .iter()
            .copied()
            .chain(std::iter::once(o.operation_ref))
        {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let (kind, id, digest) = (
                reference.kind.as_str(),
                reference.id.as_str(),
                reference.digest.as_str(),
            );
            if same_text(kind, "workflow-operation", session)? {
                if same_text(id, &o.operation_ref.id, session)?
                    && same_text(digest, &o.operation_ref.digest, session)?
                {
                    own_operation_members = resource_add(session, own_operation_members, 1)?;
                } else {
                    let bound = [
                        key.logical_store_id.as_bytes(),
                        key.scope[0].as_bytes(),
                        key.scope[1].as_bytes(),
                        id.as_bytes(),
                        digest.as_bytes(),
                    ];
                    let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
                    self.financial_reserve_io(session, payload)?;
                    let original:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_operations WHERE logical_store_id=? AND tenant=? AND environment=? AND operation_record_id=? AND operation_digest=? AND commit_sequence<?)")
                        .bind(&key.logical_store_id).bind(&key.scope[0]).bind(&key.scope[1])
                        .bind(id).bind(digest).bind(o.sequence).fetch_one(self.conn()).await?;
                    self.financial_finish_io(session).await?;
                    if !original {
                        return Err(StoreError::Integrity(
                            "financial operation member not an earlier scoped original",
                        )
                        .into());
                    }
                }
            } else {
                let bound = [kind.as_bytes(), id.as_bytes(), digest.as_bytes()];
                let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
                self.financial_reserve_io(session, payload)?;
                let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_documents WHERE kind=? AND id=? AND digest=?)")
                    .bind(kind).bind(id).bind(digest).fetch_one(self.conn()).await?;
                self.financial_finish_io(session).await?;
                if !present {
                    return Err(StoreError::Integrity("financial manifest missing document").into());
                }
            }
            // The primary key rejects repeated full Refs, inside this same
            // poisoned transaction. No unleased host BTree/Ref clone is made.
            let bound = [
                o.identity,
                kind.as_bytes(),
                id.as_bytes(),
                digest.as_bytes(),
            ];
            // The member table and its composite primary-key index each hold
            // this exact four-field identity; no body/financial pricing input.
            let packed = [
                o.identity,
                kind.as_bytes(),
                id.as_bytes(),
                digest.as_bytes(),
                o.identity,
                kind.as_bytes(),
                id.as_bytes(),
                digest.as_bytes(),
            ];
            let payload = io::binding_payload(&bound, &packed, 1, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            sqlx::query("INSERT INTO workflow_manifest_members VALUES(?,?,?,?)")
                .bind(o.identity)
                .bind(kind)
                .bind(id)
                .bind(digest)
                .execute(self.conn())
                .await?;
            self.financial_finish_io(session).await?;
        }
        if !resource_compare(session, own_operation_members, 1)?.is_eq() {
            return Err(StoreError::Integrity("financial operation manifest membership").into());
        }
        for d in documents {
            session.semantic_work().visit(ValidationVisit::Element)?;
            let bound = [
                o.identity,
                d.reference.kind.as_bytes(),
                d.reference.id.as_bytes(),
                d.reference.digest.as_bytes(),
            ];
            let payload = io::binding_payload(&bound, &[], 0, 0, session)?;
            self.financial_reserve_io(session, payload)?;
            let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_manifest_members WHERE identity_key=? AND kind=? AND id=? AND digest=?)")
                .bind(o.identity).bind(&d.reference.kind).bind(&d.reference.id).bind(&d.reference.digest).fetch_one(self.conn()).await?;
            self.financial_finish_io(session).await?;
            if !present {
                return Err(
                    StoreError::Integrity("financial prepared document outside manifest").into(),
                );
            }
        }
        Ok(JournalDelta { before, additional })
    }
    // This fixed scalar query sequence uses the SAME Profile-B equations and
    // existing retained limits as every other workflow writer. Its worker
    // ownership spans all query results; no canonical body is materialized.
    async fn accounted_capacity_check(
        &mut self,
        operations: i64,
        bytes: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<super::super::capacity::WorkflowMeter, FinancialReadError> {
        let payload = io::binding_payload(&[], &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        let limits=self.workflow_operator;
        let meter =
            super::super::capacity::check_with_session(self.conn(), operations, bytes, session,limits)
                .await?;
        self.financial_finish_io(session).await?;
        Ok(meter)
    }
    pub(super) async fn finish_journal_rows(
        &mut self,
        o: &FinancialJournal<'_>,
        delta: JournalDelta,
        session: &mut OriginalAuditSession,
    ) -> Result<(), FinancialReadError> {
        let JournalDelta { before, additional } = delta;
        let payload = io::binding_payload(&[o.identity], &[o.identity, o.identity], 3, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        sqlx::query("INSERT INTO workflow_feed VALUES(?,?)")
            .bind(o.sequence)
            .bind(o.identity)
            .execute(self.conn())
            .await?;
        self.financial_finish_io(session).await?;
        let payload = io::binding_payload(&[], &[], 4, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().arithmetic(
            ArithmeticOperation::Add,
            &[&before.payload_bytes.into(), &additional.into()],
        )?;
        let next_bytes = before
            .payload_bytes
            .checked_add(additional)
            .ok_or(StoreError::BillingHistoryLimit)?;
        let changed=sqlx::query("UPDATE workflow_state SET operation_count=?,payload_bytes=? WHERE singleton=1 AND operation_count=? AND payload_bytes=?")
            .bind(o.sequence).bind(next_bytes)
            .bind(before.operations).bind(before.payload_bytes).execute(self.conn()).await?.rows_affected();
        self.financial_finish_io(session).await?;
        session.semantic_work().arithmetic(
            ArithmeticOperation::Compare,
            &[&changed.into(), &1u64.into()],
        )?;
        if changed != 1 {
            return Err(StoreError::ExpectedCurrent.into());
        }
        // Exact counted final state catches duplicate preflight documents and
        // any mismatch before this append may clear its poison bit.
        self.accounted_capacity_check(0, 0, session).await?;
        Ok(())
    }
}
