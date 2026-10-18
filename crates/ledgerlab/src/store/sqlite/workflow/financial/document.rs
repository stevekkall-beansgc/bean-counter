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
const SCHEMA: &str = "SELECT d.schema_name FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.digest=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?)";
const CHUNK: &str = "SELECT substr(d.canonical_bytes,?,?) AS chunk FROM workflow_documents d WHERE d.kind=? AND d.id=? AND d.digest=? AND EXISTS(SELECT 1 FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key WHERE m.kind=d.kind AND m.id=d.id AND m.digest=d.digest AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?)";

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
    /// Select one actual committed association for a targeted local operand.
    /// The caller still verifies that operation/manifest/receipt and the body;
    /// this derived lookup is not an original-recipe completion certificate.
    pub(crate) async fn workflow_selected_association_sequence(
        &mut self,reference:&ExactRef,store:&str,scope:&[String;2],before:i64,
        session:&mut OriginalAuditSession,
    )->Result<Option<i64>,FinancialReadError> {
        if self.failed || self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(StoreError::Integrity("poisoned selected association read").into());
        }
        self.failed=true;
        let result=timeout_at(self.deadline,async {
            let bindings=[reference.kind.as_bytes(),reference.id.as_bytes(),reference.digest.as_bytes(),store.as_bytes(),scope[0].as_bytes(),scope[1].as_bytes()];
            let payload=io::binding_payload(&bindings,&[],0,0,session)?;
            self.financial_reserve_io(session,payload)?;
            session.semantic_work().write_bytes(std::mem::size_of::<Option<i64>>())?;
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let sequence:Option<i64>=sqlx::query_scalar("SELECT min(o.commit_sequence) FROM workflow_manifest_members m JOIN workflow_operations o ON o.identity_key=m.identity_key JOIN workflow_documents d ON d.kind=m.kind AND d.id=m.id AND d.digest=m.digest WHERE m.kind=? AND m.id=? AND m.digest=? AND o.logical_store_id=? AND o.tenant=? AND o.environment=? AND o.commit_sequence<?")
                .bind(&reference.kind).bind(&reference.id).bind(&reference.digest).bind(store).bind(&scope[0]).bind(&scope[1]).bind(before).fetch_one(self.conn()).await?;
            self.financial_finish_io(session).await?;
            if let Some(sequence)=sequence {
                if !resource_compare(session,sequence,0)?.is_gt() || !resource_compare(session,sequence,before)?.is_lt() {return Err(invalid());}
            }
            Ok(sequence)
        }).await.map_err(|_|FinancialReadError::Store(StoreError::Deadline))?;
        if result.is_ok() {
            session.check_cancelled()?;
            if self.workflow_proof_failed.load(std::sync::atomic::Ordering::Acquire) || self.financial_io.is_some() {return Err(invalid());}
            self.failed=false;
        }
        result
    }
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

        // Fetch schema separately: direct octet probe already bounded its
        // stored UTF16 representation. Keep the reviewed8M conversion overlap
        // (M128); the I/O envelope covers fixed terminators/headers.
        let payload = io::binding_payload(&bindings, &[], 0, 8 * 128, session)?;
        session.semantic_work().scan_bytes(8 * 128)?;
        session.semantic_work().write_bytes(8 * 128)?;
        self.financial_reserve_io(session, payload)?;
        session.semantic_work().visit(ValidationVisit::RefLookup)?;
        let row = bind(SCHEMA, reference, store, scope, before)
            .fetch_optional(self.conn())
            .await?
            .ok_or_else(invalid)?;
        let schema = row.try_get::<&str, _>(0)?;
        if resource_compare(session, schema.len() as i64, 128)?.is_gt() {
            return Err(invalid());
        }
        let schema = session.copy_representation(schema.as_bytes(), RepresentationKind::Record)?;
        drop(row);
        self.financial_finish_io(session).await?;

        // The output is admitted before the first chunk, and owns every byte
        // after worker finalization. A chunk limit is not a record-size limit:
        // SQLite substr can still materialize its full4MiB source argument.
        let mut output = {
            let (arena, work) = session.parts();
            arena.byte_builder(body_bytes as usize, work)?
        };
        let mut offset = 0i64;
        while resource_compare(session, offset, body_bytes)?.is_lt() {
            let start = resource_add(session, offset, 1)?;
            let materialized = resource_add(session, body_bytes, 2 * 262144)?;
            let payload = io::binding_payload(&bindings, &[], 0, materialized as usize, session)?;
            // Conservative complete source/result pass reservation per actual
            // chunk statement, not observed examined bytes or a C relabel.
            session.semantic_work().scan_bytes(materialized as usize)?;
            session.semantic_work().write_bytes(materialized as usize)?;
            self.financial_reserve_io(session, payload)?;
            session.semantic_work().visit(ValidationVisit::RefLookup)?;
            let row = sqlx::query(CHUNK)
                .bind(start)
                .bind(262144i64)
                .bind(&reference.kind)
                .bind(&reference.id)
                .bind(&reference.digest)
                .bind(store)
                .bind(&scope[0])
                .bind(&scope[1])
                .bind(before)
                .fetch_optional(self.conn())
                .await?
                .ok_or_else(invalid)?;
            let chunk = row.try_get::<&[u8], _>("chunk")?;
            if !resource_compare(session, chunk.len() as i64, 0)?.is_gt()
                || resource_compare(session, chunk.len() as i64, 262144)?.is_gt()
            {
                return Err(invalid());
            }
            output.append(chunk, session.semantic_work())?;
            offset = resource_add(session, offset, chunk.len() as i64)?;
            drop(row);
            self.financial_finish_io(session).await?;
        }
        if !resource_compare(session, offset, body_bytes)?.is_eq() {
            return Err(invalid());
        }
        let body = output.finish(session.semantic_work())?;
        Ok(Some(RetainedDocument { schema, body }))
    }
}
