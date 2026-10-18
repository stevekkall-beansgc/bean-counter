//! Owning real selected-publication compilation stage. The support is supplied
//! only by the code-owned selector AFTER identity resolution. These DATA owners
//! are not a prepared publication/append or a substitute for source custody.
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedBytes, ExternalBufferLease},
    policy::workflow::{
        DefinitionRef,
        selected_program::{
            SelectedCompilerSupport, SelectedProgram, SelectedSourceInput,
            compile_selected_publication,
        },
    },
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind as Kind,
        ValidationVisit as Visit,
    },
};
use std::mem;
const REGISTRY: &str = "sha256:762b708044ffad44717074a9b3456b91ebbe62d7a12d76cfa80d6a37ecbfe276";

pub(super) struct SelectedPublicationData {
    pub(super) program: SelectedProgram,
    pub(super) normalized_payload: AuditedBytes,
    retained: Vec<current::source::SourceRecord>,
    _retained_control: ExternalBufferLease,
    // This covers only compiler INPUT copies. The core owns its output graph.
    _compiler_input: ExternalBufferLease,
}
fn add(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Add, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_add(b).ok_or(ServiceError::Unavailable)
}
fn mul(a: usize, b: usize, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(Op::Multiply, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    a.checked_mul(b).ok_or(ServiceError::Unavailable)
}
fn bound(n: usize, lo: usize, hi: usize, s: &mut OriginalAuditSession) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &lo.into()])
        .map_err(core_error)?;
    require(n >= lo, "INVALID_SHAPE")?;
    s.semantic_work()
        .arithmetic(Op::Compare, &[&n.into(), &hi.into()])
        .map_err(core_error)?;
    require(n <= hi, "INVALID_SHAPE")
}
fn copy(raw: &[u8], s: &mut OriginalAuditSession) -> Result<Vec<u8>> {
    s.semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(raw.len())
        .map_err(core_error)?;
    let mut out = Vec::new();
    out.try_reserve_exact(raw.len())
        .map_err(|_| ServiceError::Unavailable)?;
    out.extend_from_slice(raw);
    Ok(out)
}
fn array<'a>(
    parts: impl Iterator<Item = &'a [u8]>,
    maximum: usize,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let (arena, work) = s.parts();
    let mut out = arena.byte_builder(maximum, work).map_err(core_error)?;
    out.append(b"[", s.semantic_work()).map_err(core_error)?;
    let mut first = true;
    for part in parts {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        if !first {
            out.append(b",", s.semantic_work()).map_err(core_error)?;
        }
        first = false;
        out.append(part, s.semantic_work()).map_err(core_error)?;
    }
    out.append(b"]", s.semantic_work()).map_err(core_error)?;
    out.finish(s.semantic_work()).map_err(core_error)
}
/// Actual SAME compiler body, usable by the public coordinator once the new
/// truthful support selector exists. It accepts no expected monetary outputs,
/// saved plan, current-builtin fallback or caller-declared completeness.
pub(super) async fn compile_request(
    tx: &mut SqliteTx,
    incoming: &ingress::IncomingRequest,
    support: &SelectedCompilerSupport,
    before: i64,
    s: &mut OriginalAuditSession,
) -> Result<SelectedPublicationData> {
    let result = async {
        require(
            proof_session::present(tx, incoming.key())?,
            "INTEGRITY_FAILURE",
        )?;
        request_wrapper::same_text(incoming.kind(), "policy.publish", s)?;
        let payload = incoming.payload(s)?;
        let mode = request_wrapper::text(payload, "mode", s)?;
        let inline = acceptance::same_recipe_text(mode, "inline", s)?;
        if !inline {
            request_wrapper::same_text(mode, "retained", s)?;
        }
        let field = if inline { "documents" } else { "document_refs" };
        request_wrapper::closed(
            payload,
            &[
                "schema",
                "mode",
                field,
                "root_policy_ref",
                "expected_plan_digest",
            ],
            s,
        )?;
        request_wrapper::same_text(
            request_wrapper::text(payload, "schema", s)?,
            "ledger-workflow-policy-publication/6",
            s,
        )?;
        let candidates = request_wrapper::field(payload, field, s)?
            .as_array()
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        bound(candidates.len(), 1, 128, s)?;
        let controls = add(
            mul(candidates.len(), mem::size_of::<AuditedBytes>(), s)?,
            mem::size_of::<Vec<AuditedBytes>>(),
            s,
        )?;
        let _body_control = s.reserve_external_payload(controls).map_err(core_error)?;
        let mut bodies = Vec::new();
        bodies
            .try_reserve_exact(candidates.len())
            .map_err(|_| ServiceError::Unavailable)?;
        let retained_size = add(
            mul(
                candidates.len(),
                mem::size_of::<current::source::SourceRecord>(),
                s,
            )?,
            mem::size_of::<Vec<current::source::SourceRecord>>(),
            s,
        )?;
        let retained_control = s
            .reserve_external_payload(retained_size)
            .map_err(core_error)?;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(candidates.len())
            .map_err(|_| ServiceError::Unavailable)?;
        let mut total = 0usize;
        for candidate in candidates {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let body = if inline {
                s.canonical_representation(candidate, Kind::SourceDefinition)
                    .map_err(core_error)?
            } else {
                let kind = request_wrapper::text(candidate, "kind", s)?;
                let mut schema = None;
                for (k, v) in [
                    ("workflow-product", "ledger-workflow-product/1"),
                    ("workflow-fact-schema", "ledger-workflow-fact-schema/1"),
                    ("workflow-slot-template", "ledger-workflow-slot-template/1"),
                    ("workflow-policy", "ledger-workflow-policy/4"),
                    (
                        "workflow-compiler-descriptor",
                        "ledger-workflow-compiler-descriptor/6",
                    ),
                ] {
                    if acceptance::same_recipe_text(kind, k, s)? {
                        schema = Some(v);
                        break;
                    }
                }
                let schema = schema.ok_or_else(|| reject("INVALID_REFERENCE"))?;
                let record = current::source::retained_record(
                    tx,
                    incoming.key(),
                    candidate,
                    kind,
                    schema,
                    before,
                    s,
                )
                .await?;
                let body = s
                    .copy_representation(record.canonical_body(), Kind::SourceDefinition)
                    .map_err(core_error)?;
                s.semantic_work()
                    .write_bytes(mem::size_of::<current::source::SourceRecord>())
                    .map_err(core_error)?;
                retained.push(record);
                body
            };
            total = add(total, body.bytes().len(), s)?;
            bound(total, 1, 1048576, s)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<AuditedBytes>())
                .map_err(core_error)?;
            bodies.push(body);
        }
        let root = request_wrapper::field(payload, "root_policy_ref", s)?;
        acceptance::validate_reference(root, "workflow-policy", s)?;
        let mut input_size = add(mul(bodies.len(), mem::size_of::<Vec<u8>>(), s)?, total, s)?;
        input_size = add(input_size, mem::size_of::<SelectedSourceInput>(), s)?;
        for name in ["kind", "id", "digest"] {
            input_size = add(input_size, request_wrapper::text(root, name, s)?.len(), s)?;
        }
        let compiler_input = s.reserve_external_payload(input_size).map_err(core_error)?;
        let root = DefinitionRef::new_with_work(
            request_wrapper::text(root, "kind", s)?,
            request_wrapper::text(root, "id", s)?,
            request_wrapper::text(root, "digest", s)?,
            s.semantic_work(),
        )
        .map_err(core_error)?;
        let mut documents = Vec::new();
        documents
            .try_reserve_exact(bodies.len())
            .map_err(|_| ServiceError::Unavailable)?;
        for body in &bodies {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            s.semantic_work()
                .write_bytes(mem::size_of::<Vec<u8>>())
                .map_err(core_error)?;
            documents.push(copy(body.bytes(), s)?);
        }
        let program = compile_selected_publication(
            SelectedSourceInput {
                documents,
                root_policy_ref: root,
            },
            support,
            s,
        )
        .map_err(core_error)?;
        // The actual compiled program inherits only its private support's
        // exact execution identity. Public descriptor data cannot select it.
        let registry=program.execution_registry().ok_or(ServiceError::Unavailable)?;
        if !acceptance::same_recipe_text(registry,wrappers::LOCAL_REGISTRY,s)? {
            request_wrapper::same_text(registry,REGISTRY,s)?;
        }
        request_wrapper::same_text(
            request_wrapper::text(payload, "expected_plan_digest", s)?,
            program.compiled_plan().reference().digest(),
            s,
        )?;
        let documents = if inline {
            array(
                program
                    .source_documents()
                    .iter()
                    .map(|d| d.canonical_bytes()),
                1048576,
                s,
            )?
        } else {
            wire::RefSet::new(
                retained.iter().map(|d| d.reference()),
                retained.len(),
                true,
                s,
            )?
            .encode(s)?
        };
        let mode = wire::string(mode, s)?;
        let root = request_wrapper::field(payload, "root_policy_ref", s)?;
        let root = s
            .canonical_representation(root, Kind::SourceDefinition)
            .map_err(core_error)?;
        let expected = wire::string(program.compiled_plan().reference().digest(), s)?;
        let normalized_payload = wire::object(
            &[
                (field, documents.bytes()),
                ("expected_plan_digest", expected.bytes()),
                ("mode", mode.bytes()),
                ("root_policy_ref", root.bytes()),
                ("schema", b"\"ledger-workflow-policy-publication/6\""),
            ],
            8388608,
            s,
        )?;
        Ok(SelectedPublicationData {
            program,
            normalized_payload,
            retained,
            _retained_control: retained_control,
            _compiler_input: compiler_input,
        })
    }
    .await;
    if result.is_err() {
        s.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}

impl SelectedPublicationData {
    pub(super) fn match_retained(
        &self,
        prefix: &mut prefix::FoldedInventory,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        for record in &self.retained {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            prefix.require_record(record.reference(), s)?;
        }
        Ok(())
    }
}

/// The actual held-owner/identity-first fresh stage. Captured root, literal
/// request and SAME compiler output remain DATA until source/build provenance,
/// exact wrappers, locked rereads and the private publication preparation close.
pub(super) struct PendingSelectedPublication {
    pub(super) incoming: ingress::IncomingRequest,
    pub(super) compiled: SelectedPublicationData,
    pub(super) local_root: wire::SourceRecord,
    pub(super) at: Timestamp,
    pub(super) before: i64,
    management: ManagementObservation,
    _time: ExternalBufferLease,
}
impl PendingSelectedPublication {
    pub(super) async fn recheck(
        &self,
        ledger: &LocalWorkflowLedger,
        tx: &mut SqliteTx,
        session: &mut OriginalAuditSession,
    ) -> Result<()> {
        self.management
            .recheck(ledger, tx, self.incoming.key(), session)
            .await
    }
}
/// The caller's selector is a binary-owned function, invoked only after the
/// global identity is absent. Production pins app_cost_causal_with_session once
/// it exists; an untrusted descriptor or current builtin cannot supply support.
pub(super) async fn prepare_new<F>(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    raw: &[u8],
    fresh_support: F,
    session: &mut OriginalAuditSession,
) -> Result<PendingSelectedPublication>
where
    F: FnOnce(&mut OriginalAuditSession) -> ledgerlab_core::Result<SelectedCompilerSupport>,
{
    let result = async {
        let incoming = ingress::IncomingRequest::parse_publication(raw, session)?;
        require(
            proof_session::present(tx, incoming.key())?,
            "INTEGRITY_FAILURE",
        )?;
        ledger.admin.read(&ledger.store, tx, incoming.key()).await?;
        // Reuse the actual owning frame reader and SQL-worker lease path even
        // for an absent-key probe. No ordinary raw key bind escapes admission.
        if let Some(original) = tx
            .workflow_original_frame(
                incoming.identity(),
                &incoming.key().logical_store_id,
                &incoming.key().scope,
                session,
            )
            .await
            .map_err(prefix::read_error)?
        {
            // Existing identities belong to the enclosing original dispatcher;
            // this fresh stage does not invoke a selector or emit another bill.
            drop(original);
            return Err(ServiceError::Unavailable);
        }
        let key_body = request_wrapper::field(incoming.value(), "key", session)?;
        let management =
            ManagementObservation::capture(ledger, tx, incoming.key(), key_body, session).await?;
        let setup = ledger
            .admin
            .fresh(&ledger.store, tx, incoming.key())
            .await?;
        let before = tx
            .workflow_sequence()
            .await
            .map_err(crate::service::store_error)?;
        let time = session
            .reserve_external_payload(4 * 64 + 27)
            .map_err(core_error)?;
        let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
        let support = fresh_support(session).map_err(core_error)?;
        let compiled = compile_request(tx, &incoming, &support, before, session).await?;
        let scope = request_wrapper::field(
            request_wrapper::field(incoming.value(), "key", session)?,
            "scope",
            session,
        )?;
        let local_root = current::party::produce::root(&setup, scope, &at, session)?;
        management
            .recheck(ledger, tx, incoming.key(), session)
            .await?;
        Ok(PendingSelectedPublication {
            incoming,
            compiled,
            local_root,
            at,
            before,
            management,
            _time: time,
        })
    }
    .await;
    if result.is_err() {
        session.invalidate();
        tx.workflow_proof_failure_handle()
            .store(true, Ordering::Release);
    }
    result
}
