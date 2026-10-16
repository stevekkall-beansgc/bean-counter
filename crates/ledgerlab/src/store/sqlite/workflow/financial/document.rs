//! Scoped owning DATA read for the actual current/prefix caller. Manifest
//! reachability is visibility only; it does not authenticate an introduction.
use super::*;
use ledgerlab_core::{
    policy::workflow::transition_constructor::ExactRef, validation_work::RepresentationKind,
};

pub(crate) struct RetainedDocument {
    pub schema: AuditedBytes,
    pub body: AuditedBytes,
}

// Keep the predicates identical between the nonmaterializing size probe and
// fetch. The same held writer prevents a different value between these calls.
const LENGTHS: &str = "SELECT octet_length(d.schema_name),length(d.canonical_bytes) FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.digest=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?)";
const VALUE: &str = "SELECT d.schema_name,d.canonical_bytes FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.digest=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?)";

fn bind<'a>(
    sql: &'static str,
    reference: &'a ExactRef,
    store: &'a str,
    scope: &'a [String; 2],
    before: i64,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    sqlx::query(sql)
        .bind(&reference.kind)
        .bind(&reference.id)
        .bind(&reference.digest)
        .bind(store)
        .bind(&scope[0])
        .bind(&scope[1])
        .bind(before)
}

impl SqliteTx {
    /// An exact reference and earlier scoped membership select retained bytes,
    /// never completed provenance. Bodies retain their SAME arena ownership
    /// after worker finalization. No ordinary Vec/body clone is returned.
    pub(crate) async fn workflow_financial_document(
        &mut self,
        reference: &ExactRef,
        store: &str,
        scope: &[String; 2],
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<RetainedDocument>, FinancialReadError> {
        if self.failed
            || self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::Integrity("poisoned financial document observation").into());
        }
        self.failed = true;
        let result = timeout_at(
            self.deadline,
            self.financial_document_inner(reference, store, scope, before, session),
        )
        .await
        .map_err(|_| FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            session.check_cancelled()?;
            if self
                .workflow_proof_failed
                .load(std::sync::atomic::Ordering::Acquire)
                || self.financial_io.is_some()
            {
                return Err(
                    StoreError::Integrity("unfinished financial document observation").into(),
                );
            }
            self.failed = false;
        }
        result
    }

    async fn financial_document_inner(
        &mut self,
        reference: &ExactRef,
        store: &str,
        scope: &[String; 2],
        before: i64,
        session: &mut OriginalAuditSession,
    ) -> Result<Option<RetainedDocument>, FinancialReadError> {
        for (text, maximum) in [
            (reference.kind.as_str(), 48usize),
            (&reference.id, 104),
            (&reference.digest, 71),
            (store, 128),
            (&scope[0], 128),
            (&scope[1], 128),
        ] {
            session.semantic_work().visit(ValidationVisit::Constraint)?;
            if !resource_compare(session, text.len() as i64, 0)?.is_gt()
                || resource_compare(session, text.len() as i64, maximum as i64)?.is_gt()
            {
                return Err(invalid());
            }
        }
        if !resource_compare(session, before, 0)?.is_gt()
            || resource_compare(session, before, 100001)?.is_gt()
        {
            return Err(invalid());
        }
        let bindings = [
            reference.kind.as_bytes(),
            reference.id.as_bytes(),
            reference.digest.as_bytes(),
            store.as_bytes(),
            scope[0].as_bytes(),
            scope[1].as_bytes(),
        ];
        let payload = io::binding_payload(&bindings, &[], 0, 0, session)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let lengths = bind(LENGTHS, reference, store, scope, before)
            .fetch_optional(self.conn())
            .await?;
        let Some(lengths) = lengths else {
            self.financial_finish_io(session).await?;
            return Ok(None);
        };
        let schema_bytes: i64 = lengths.try_get(0)?;
        let body_bytes: i64 = lengths.try_get(1)?;
        // Schema names are bounded ASCII in the closed recipes, but prefetch
        // permits the stored UTF16 representation. Exact UTF8 bounds/grammar
        // are checked before the owning copy, without changing historical text.
        if !resource_compare(session, schema_bytes, 0)?.is_gt()
            || resource_compare(session, schema_bytes, 256)?.is_gt()
            || !resource_compare(session, body_bytes, 0)?.is_gt()
            || resource_compare(session, body_bytes, 4194304)?.is_gt()
        {
            return Err(invalid());
        }
        drop(lengths);
        self.financial_finish_io(session).await?;

        // Direct BLOB fetch: statement-held body + SQLx row duplicate, each
        // at most the probed length. The separate owning artifact is admitted
        // by copy_representation below. TEXT conversion uses the reviewed8M
        // overlap bound (M128); fixed terminators/headers use the I/O envelope.
        session.semantic_work().arithmetic(
            ArithmeticOperation::Multiply,
            &[&body_bytes.into(), &2i64.into()],
        )?;
        let body_overlap = body_bytes.checked_mul(2).ok_or_else(invalid)?;
        let materialized = resource_add(session, body_overlap, 8 * 128)?;
        let payload = io::binding_payload(&bindings, &[], 0, materialized as usize, session)?;
        // Conservative result-column byte passes, not observed examined bytes.
        // SQL/driver work stays outside the private codec callback scope.
        session.semantic_work().scan_bytes(materialized as usize)?;
        session.semantic_work().write_bytes(materialized as usize)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let row = bind(VALUE, reference, store, scope, before)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        let schema = row.try_get::<&str, _>(0)?;
        let body = row.try_get::<&[u8], _>(1)?;
        if resource_compare(session, schema.len() as i64, 128)?.is_gt()
            || !resource_compare(session, body.len() as i64, body_bytes)?.is_eq()
        {
            return Err(invalid());
        }
        let schema = session.copy_representation(schema.as_bytes(), RepresentationKind::Record)?;
        let body = session.copy_representation(body, RepresentationKind::Record)?;
        drop(row);
        self.financial_finish_io(session).await?;
        Ok(Some(RetainedDocument { schema, body }))
    }
}
